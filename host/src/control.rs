//! Fase H1. Kanal kontrol ENet (UDP, dasar + 10).
//!
//! Rujukan: bagian control di `Sunshine/src/stream.cpp`. Client menyambung dengan angka `X-SS-Connect-Data` dari
//! RTSP; semua pesan wajib terenkripsi AES-GCM dengan kunci sesi (pesan polos dibuang, pesan yang gagal
//! didekripsi mengakhiri sesi). Permintaan IDR dan invalidasi frame rujukan diteruskan ke jalur video, paket input
//! diterjemahkan dan disuntikkan ke Windows langsung dari thread ini; pesan lain hanya dihitung per tipe.
//!
//! ENet memakai crate `rusty_enet` (ENet 1.3.18 hasil transpilasi); kecocokannya dengan fork ENet Moonlight
//! dibuktikan dengan Moonlight asli, lihat README.

use std::collections::HashSet;
use std::net::UdpSocket;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use cloudpc_stream_proto::input::{self as wire_input, InputEvent};
use cloudpc_stream_proto::control::{packet_type, Origin};
use rusty_enet as enet;

use crate::input::{Action, Geometry, Injector, Translator};
use crate::nvhttp::Host;
use crate::video::VideoRequest;

/// Kode terminasi "server menutup sesi" (`NVST_DISCONN_SERVER_TERMINATED_CLOSED`), sama dengan yang dikirim Sunshine.
pub const TERMINATION_CLOSED: u32 = 0x8003_0023;

/// Perintah dari thread lain ke thread kontrol (ENet tidak boleh disentuh dari luar thread-nya).
pub(crate) enum Command {
    /// Beri tahu client sesi diakhiri, lalu buang sesi stream. `Some(id)`: hanya bila sesi aktif masih sesi itu.
    Terminate(Option<u32>),
}

pub use cloudpc_stream_common::secure::{open, seal, OpenError};

/// Pesan kontrol yang harus ditindaklanjuti jalur video.
fn video_request(kind: u16, payload: &[u8]) -> Option<VideoRequest> {
    match kind {
        packet_type::REQUEST_IDR_FRAME => Some(VideoRequest::Idr),
        // `SS_RFI_REQUEST`: frame pertama dan terakhir yang hilang, masing-masing 64 bit little-endian.
        packet_type::INVALIDATE_REF_FRAMES => Some(match (payload.get(0..8), payload.get(8..16)) {
            (Some(first), Some(last)) => VideoRequest::Invalidate { first: u64::from_le_bytes(first.try_into().unwrap()), last: u64::from_le_bytes(last.try_into().unwrap()) },
            _ => VideoRequest::Idr,
        }),
        _ => None,
    }
}

/// Tunggu sampai ada datagram masuk atau `timeout_ms` lewat, supaya input diproses begitu tiba tanpa memutar CPU.
#[cfg(windows)]
fn wait_readable(socket: Option<&UdpSocket>, timeout_ms: i32) {
    use std::os::windows::io::AsRawSocket;
    use windows::Win32::Networking::WinSock::{WSAPoll, POLLRDNORM, SOCKET, WSAPOLLFD};
    let Some(socket) = socket else { return thread::sleep(Duration::from_millis(1)) };
    let mut descriptor = WSAPOLLFD { fd: SOCKET(socket.as_raw_socket() as usize), events: POLLRDNORM, revents: Default::default() };
    // SAFETY: satu deskriptor yang hidup selama panggilan; hasilnya tidak dipakai (bangun karena data atau waktu habis).
    unsafe {
        WSAPoll(&mut descriptor, 1, timeout_ms);
    }
}

#[cfg(not(windows))]
fn wait_readable(_socket: Option<&UdpSocket>, _timeout_ms: i32) {
    thread::sleep(Duration::from_millis(1));
}

fn event_name(event: &InputEvent) -> &'static str {
    match event {
        InputEvent::Key { .. } => "keyboard",
        InputEvent::Text(_) => "teks",
        InputEvent::MouseMoveRelative { .. } => "mouse relatif",
        InputEvent::MouseMoveAbsolute { .. } => "mouse absolut",
        InputEvent::MouseButton { .. } => "tombol mouse",
        InputEvent::Scroll(_) => "scroll",
        InputEvent::HorizontalScroll(_) => "scroll datar",
        InputEvent::Gamepad(_) => "gamepad",
        InputEvent::Other(_) => "lain (belum ditangani)",
    }
}

/// Jalur input sesi aktif: penerjemah (ingat apa yang sedang ditekan) + penyuntik ke Windows.
struct InputPath {
    translator: Translator,
    injector: Injector,
    seen: HashSet<&'static str>,
    failures: u32,
    /// Getar dari gamepad virtual menunggu dikirim ke client.
    rumble: std::sync::mpsc::Receiver<(u8, u8, u8)>,
}

impl Default for InputPath {
    fn default() -> Self {
        let (tx, rumble) = std::sync::mpsc::channel();
        let mut injector = Injector::default();
        injector.set_rumble_sink(tx);
        InputPath { translator: Translator::default(), injector, seen: HashSet::new(), failures: 0, rumble }
    }
}

impl InputPath {
    fn handle(&mut self, host: &Host, event: &InputEvent, geometry: Geometry) {
        if self.seen.insert(event_name(event)) {
            host.log(|| format!("input: pertama kali {}", event_name(event)));
        }
        let actions = self.translator.translate(event, geometry, Instant::now());
        self.run(host, &actions);
    }

    fn run(&mut self, host: &Host, actions: &[Action]) {
        for action in actions {
            if host.cfg.test_record_input {
                host.recorded_input.lock().unwrap().push(*action);
                host.log(|| format!("input (hanya dicatat): {action:?}"));
            } else if let Err(why) = self.injector.apply(action) {
                self.failures += 1;
                // Kegagalan bisa beruntun (mis. selama layar UAC): catat yang pertama, lalu tiap 500.
                if self.failures % 500 == 1 {
                    host.log(|| format!("input: {why} (gagal ke-{})", self.failures));
                }
            }
        }
    }

    /// Sesi berakhir: lepas semua yang masih ditekan dan cabut gamepad.
    fn release(&mut self, host: &Host) {
        let actions = self.translator.release_all();
        if !actions.is_empty() {
            host.log(|| format!("input: melepas {} tombol/gamepad yang masih aktif", actions.len()));
        }
        self.run(host, &actions);
        self.seen.clear();
        self.failures = 0;
    }
}

pub(crate) fn serve(host: Arc<Host>, socket: UdpSocket, commands: Receiver<Command>) {
    let waiter = socket.try_clone().ok();
    let settings = enet::HostSettings { peer_limit: 8, ..Default::default() };
    let mut enet = match enet::Host::new(socket, settings) {
        Ok(enet) => enet,
        Err(_) => {
            host.log(|| "kanal kontrol: ENet gagal dimulai".into());
            return;
        }
    };
    // Peer yang sedang memegang sesi stream menurut thread ini.
    let mut active: Option<enet::PeerID> = None;
    let mut input = InputPath::default();
    loop {
        while let Ok(Command::Terminate(only)) = commands.try_recv() {
            let taken = {
                let mut stream = host.stream.lock().unwrap();
                if only.is_some_and(|id| stream.as_ref().is_some_and(|s| s.launch_id != id)) {
                    None
                } else {
                    stream.take()
                }
            };
            let Some(mut session) = taken else { continue };
            if let Some(peer) = session.peer {
                let message = seal(&session.key, session.out_seq, Origin::Host, packet_type::TERMINATION, &TERMINATION_CLOSED.to_be_bytes());
                session.out_seq += 1;
                let _ = enet.peer_mut(enet::PeerID(peer)).send(0, &enet::Packet::reliable(message.as_slice()));
                host.log(|| format!("kanal kontrol: terminasi dikirim ke \"{}\"", session.client_name));
            }
        }

        // Sesi diganti, dibatalkan, atau dihentikan: putus peer yang tidak lagi memegang sesi aktif.
        let wanted = host.stream.lock().unwrap().as_ref().and_then(|s| s.peer);
        if let Some(peer) = active.filter(|peer| wanted != Some(peer.0)) {
            enet.peer_mut(peer).disconnect_later(0);
            active = None;
            input.release(&host);
        }
        if let Some(action) = input.translator.repeat(Instant::now()) {
            input.run(&host, &[action]);
        }
        // Getar gamepad: motor 0..=255 ViGEm -> 0..=65535 (seperti Sunshine, digeser 8 bit).
        let mut rumble: Vec<(u8, u8, u8)> = input.rumble.try_iter().collect();
        if host.cfg.test_rumble && active.is_some() && input.seen.contains("gamepad") && !input.seen.contains("uji getar") {
            input.seen.insert("uji getar");
            rumble.push((0, 0x80, 0xFF));
        }
        for (pad, large, small) in rumble {
            let mut stream = host.stream.lock().unwrap();
            let Some(session) = stream.as_mut() else { continue };
            let Some(peer) = session.peer else { continue };
            let payload = cloudpc_stream_proto::control::rumble_payload(u16::from(pad), u16::from(large) << 8, u16::from(small) << 8);
            let message = seal(&session.key, session.out_seq, Origin::Host, packet_type::RUMBLE_DATA, &payload);
            session.out_seq += 1;
            let _ = enet.peer_mut(enet::PeerID(peer)).send(0, &enet::Packet::reliable(message.as_slice()));
            host.log(|| format!("kanal kontrol: getar gamepad {pad} ({large}, {small}) dikirim"));
        }

        let event = match enet.service() {
            Ok(Some(event)) => event.no_ref(),
            // Tidak ada kejadian (atau, di Windows, galat baca karena ICMP "port unreachable" dari client yang sudah
            // pergi): tunggu datagram berikutnya, paling lama beberapa milidetik supaya pengatur waktu ENet tetap jalan.
            Ok(None) | Err(_) => {
                wait_readable(waiter.as_ref(), 4);
                continue;
            }
        };
        match event {
            enet::EventNoRef::Connect { peer, data } => {
                if claim(&host, peer.0, data) {
                    active = Some(peer);
                } else {
                    host.log(|| "kanal kontrol: sambungan ditolak (tidak cocok dengan sesi mana pun)".into());
                    enet.peer_mut(peer).disconnect_now(0);
                }
            }
            enet::EventNoRef::Disconnect { peer, .. } => {
                {
                    let mut stream = host.stream.lock().unwrap();
                    if let Some(session) = stream.as_ref().filter(|s| s.peer == Some(peer.0)) {
                        host.log(|| format!("kanal kontrol: client terputus, sesi stream selesai; pesan per tipe: {:x?}", session.messages));
                        *stream = None;
                    }
                }
                if active == Some(peer) {
                    active = None;
                    input.release(&host);
                }
            }
            enet::EventNoRef::Receive { peer, packet, .. } => {
                if let Some((event, geometry)) = receive(&host, peer.0, packet.data()) {
                    input.handle(&host, &event, geometry);
                }
            }
        }
    }
}

/// Client yang menyambung harus membawa `connect_data` sesi yang belum punya peer.
fn claim(host: &Host, peer: usize, connect_data: u32) -> bool {
    let mut stream = host.stream.lock().unwrap();
    let Some(session) = stream.as_mut().filter(|s| s.peer.is_none() && s.connect_data == connect_data) else { return false };
    session.peer = Some(peer);
    host.log(|| format!("kanal kontrol: \"{}\" tersambung", session.client_name));
    // Sesi peluncuran sudah terpakai; RTSP untuk sesi ini tidak dilayani lagi.
    let mut launch = host.launch.lock().unwrap();
    if launch.as_ref().is_some_and(|l| l.id == session.launch_id) {
        *launch = None;
    }
    true
}

/// Proses satu paket kontrol dari peer sesi aktif. Mengembalikan kejadian input bila paket itu input yang sah,
/// supaya penyuntikannya dilakukan di luar kunci sesi.
fn receive(host: &Host, peer: usize, packet: &[u8]) -> Option<(InputEvent, Geometry)> {
    let mut stream = host.stream.lock().unwrap();
    let session = stream.as_mut().filter(|s| s.peer == Some(peer))?;
    match open(&session.key, Origin::Client, packet) {
        Ok((kind, payload)) => {
            if let Some(request) = video_request(kind, &payload) {
                host.video_requests.lock().unwrap().push(request);
            }
            let count = session.messages.entry(kind).or_insert(0);
            *count += 1;
            if *count == 1 {
                host.log(|| format!("kanal kontrol: pesan pertama tipe 0x{kind:04x} ({} byte)", payload.len()));
            }
            if kind != packet_type::INPUT_DATA {
                return None;
            }
            // Paket input yang rusak dibuang tanpa mengganggu sesi.
            let event = wire_input::decode(&payload).ok()?;
            let stream_size = (session.config.width, session.config.height);
            let desktop = match *host.desktop_size.lock().unwrap() {
                (0, _) | (_, 0) => stream_size,
                known => known,
            };
            Some((event, Geometry { desktop, stream: stream_size }))
        }
        Err(OpenError::NotEncrypted) | Err(OpenError::Malformed) => None,
        Err(OpenError::BadTag) => {
            host.log(|| "kanal kontrol: pesan gagal didekripsi, sesi dihentikan".into());
            *stream = None;
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cloudpc_stream_proto::control::encode_message;

    const KEY: [u8; 16] = [3; 16];

    #[test]
    fn sealed_message_opens_only_with_same_key_and_direction() {
        let wire = seal(&KEY, 0, Origin::Client, packet_type::REQUEST_IDR_FRAME, &[0, 0]);
        // seq (4) + tag (16) + header dalam (4) + payload (2)
        assert_eq!(&wire[..8], &[0x01, 0x00, 26, 0x00, 0, 0, 0, 0]);
        assert_eq!(open(&KEY, Origin::Client, &wire), Ok((packet_type::REQUEST_IDR_FRAME, vec![0, 0])));
        assert_eq!(open(&KEY, Origin::Host, &wire), Err(OpenError::BadTag), "arah berbeda memakai IV berbeda");
        assert_eq!(open(&[4; 16], Origin::Client, &wire), Err(OpenError::BadTag));
    }

    #[test]
    fn tampered_plain_and_nested_messages_are_rejected() {
        let mut wire = seal(&KEY, 7, Origin::Client, packet_type::LOSS_STATS, &[0; 32]);
        let last = wire.len() - 1;
        wire[last] ^= 0x80;
        assert_eq!(open(&KEY, Origin::Client, &wire), Err(OpenError::BadTag));

        let plain = encode_message(packet_type::INPUT_DATA, &[1, 2, 3]).unwrap();
        assert_eq!(open(&KEY, Origin::Client, &plain), Err(OpenError::NotEncrypted), "pesan polos tidak pernah diterima");

        let nested = seal(&KEY, 1, Origin::Client, packet_type::ENCRYPTED, &[0; 24]);
        assert_eq!(open(&KEY, Origin::Client, &nested), Err(OpenError::Malformed));
    }

    #[test]
    fn idr_and_invalidation_messages_become_video_requests() {
        assert_eq!(video_request(packet_type::REQUEST_IDR_FRAME, &[0, 0]), Some(VideoRequest::Idr));
        let mut payload = [0u8; 24];
        payload[0..4].copy_from_slice(&120u32.to_le_bytes());
        payload[8..12].copy_from_slice(&123u32.to_le_bytes());
        assert_eq!(video_request(packet_type::INVALIDATE_REF_FRAMES, &payload), Some(VideoRequest::Invalidate { first: 120, last: 123 }));
        assert_eq!(video_request(packet_type::INVALIDATE_REF_FRAMES, &[1, 2, 3]), Some(VideoRequest::Idr), "payload terpotong: aman dengan IDR");
        assert_eq!(video_request(packet_type::PERIODIC_PING, &[0; 8]), None);
        assert_eq!(video_request(packet_type::INPUT_DATA, &[0; 10]), None);
    }

    #[test]
    fn termination_code_is_big_endian_on_the_wire() {
        let wire = seal(&KEY, 0, Origin::Host, packet_type::TERMINATION, &TERMINATION_CLOSED.to_be_bytes());
        assert_eq!(open(&KEY, Origin::Host, &wire), Ok((0x0109, vec![0x80, 0x03, 0x00, 0x23])));
    }
}
