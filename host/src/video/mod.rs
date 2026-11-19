//! Jalur video host: tangkap layar -> susun (skala + kursor) -> encode -> shard + FEC -> kirim UDP (port dasar + 9).
//!
//! Dua thread: penerima ping (mencatat alamat UDP client dari ping berisi `X-SS-Ping-Payload`) dan thread video
//! yang melayani sesi stream aktif mulai dari kanal kontrol tersambung sampai sesi berakhir.
//! Belum: pacing yang mengikuti kecepatan jaringan client (sekarang hanya batas 800 Mbit/s seperti Sunshine).

#[cfg(windows)]
pub mod capture;
pub mod cursor;
#[cfg(windows)]
pub mod encode;
pub mod packetize;

use std::net::{SocketAddr, UdpSocket};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use cloudpc_stream_proto::rtp::read_ping;
use cloudpc_stream_proto::video::server_codec_mode;

use crate::control::Command;
use crate::nvhttp::Host;
use crate::session::{StreamConfig, StreamSession};

/// Client harus mengirim ping video dalam waktu ini setelah kanal kontrol tersambung.
const PING_TIMEOUT: Duration = Duration::from_secs(10);
/// Batas laju kirim di dalam satu frame (80% dari 1 Gbit/s, seperti Sunshine).
const PACING_BITS_PER_SECOND: usize = 800_000_000;
/// Selama ini tangkapan layar boleh hilang (layar kunci, UAC, ganti resolusi) sebelum sesi dianggap gagal.
const CAPTURE_PATIENCE: Duration = Duration::from_secs(60);

/// Permintaan dari kanal kontrol ke thread video.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoRequest {
    /// Kirim frame IDR.
    Idr,
    /// Client kehilangan frame `first..=last`.
    Invalidate { first: u64, last: u64 },
}

/// Codec dan fitur encoder yang tersedia di mesin ini.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoCapabilities {
    pub h264: bool,
    pub hevc: bool,
    pub av1: bool,
    pub ref_invalidation: bool,
    /// `false` = GPU tidak bisa diperiksa; codec dianggap ada dan kegagalan baru terlihat saat sesi dimulai.
    pub probed: bool,
}

impl VideoCapabilities {
    const UNKNOWN: VideoCapabilities = VideoCapabilities { h264: true, hevc: true, av1: true, ref_invalidation: false, probed: false };

    /// `format`: 0 = H.264, 1 = HEVC, 2 = AV1 (nilai `bitStreamFormat` dari client).
    pub fn supports(&self, format: u32) -> bool {
        match format {
            0 => self.h264,
            1 => self.hevc,
            2 => self.av1,
            _ => false,
        }
    }

    /// Nilai `ServerCodecModeSupport` untuk `/serverinfo`.
    pub fn server_codec_mode(&self) -> u32 {
        [(self.h264, server_codec_mode::H264), (self.hevc, server_codec_mode::HEVC), (self.av1, server_codec_mode::AV1_MAIN8)].iter().filter(|(on, _)| *on).map(|(_, bit)| bit).sum()
    }
}

/// Kemampuan encoder mesin ini; GPU diperiksa sekali per proses.
pub fn capabilities() -> VideoCapabilities {
    static CACHE: OnceLock<VideoCapabilities> = OnceLock::new();
    *CACHE.get_or_init(|| {
        #[cfg(windows)]
        if let Ok(c) = encode::probe() {
            return VideoCapabilities { h264: c.h264, hevc: c.hevc, av1: c.av1, ref_invalidation: c.ref_invalidation, probed: true };
        }
        VideoCapabilities::UNKNOWN
    })
}

/// Persegi tujuan `(kiri, atas, lebar, tinggi)` di dalam `target` supaya rasio `source` terjaga (sisanya hitam).
pub fn fit(source: (u32, u32), target: (u32, u32)) -> (f32, f32, f32, f32) {
    let (sw, sh, tw, th) = (source.0 as f32, source.1 as f32, target.0 as f32, target.1 as f32);
    let scale = (tw / sw).min(th / sh);
    let (w, h) = (sw * scale, sh * scale);
    ((tw - w) / 2.0, (th - h) / 2.0, w, h)
}

struct Target {
    launch_id: u32,
    key: [u8; 16],
    config: StreamConfig,
}

/// Datagram ini ping yang sah untuk sesi dengan `ping_payload` tersebut?
fn is_ping(datagram: &[u8], ping_payload: &str) -> bool {
    datagram.len() == 20 && read_ping(datagram).is_ok_and(|(payload, _)| payload == ping_payload.as_bytes())
}

/// Catat alamat pengirim ping sebagai tujuan video (atau audio) sesi aktif lewat `store`. Client mengulang ping
/// tiap ±500 ms, jadi alamat yang berubah (NAT) ikut terbarui.
pub(crate) fn listen_for_pings(host: Arc<Host>, socket: UdpSocket, store: fn(&mut StreamSession, SocketAddr)) {
    let mut buffer = [0u8; 64];
    loop {
        match socket.recv_from(&mut buffer) {
            Ok((length, from)) => {
                if let Some(session) = host.stream.lock().unwrap().as_mut() {
                    if is_ping(&buffer[..length], &session.ping_payload) {
                        store(session, from);
                    }
                }
            }
            // Di Windows, ICMP "port unreachable" dari client yang sudah pergi muncul sebagai galat baca.
            Err(_) => thread::sleep(Duration::from_millis(1)),
        }
    }
}

#[cfg(windows)]
fn grow_send_buffer(socket: &UdpSocket) {
    use std::os::windows::io::AsRawSocket;
    use windows::Win32::Networking::WinSock::{setsockopt, SOCKET, SOL_SOCKET, SO_SNDBUF};
    // Bawaan Windows 64 KiB, lebih kecil dari satu frame IDR; 1 MiB menampung beberapa frame.
    let size: i32 = 1 << 20;
    // SAFETY: soket hidup selama panggilan; gagal tidak fatal (hanya penyangga tetap kecil).
    unsafe {
        setsockopt(SOCKET(socket.as_raw_socket() as usize), SOL_SOCKET, SO_SNDBUF, Some(&size.to_ne_bytes()));
    }
}

#[cfg(not(windows))]
fn grow_send_buffer(_socket: &UdpSocket) {}

pub(crate) fn serve(host: Arc<Host>, socket: UdpSocket) {
    grow_send_buffer(&socket);
    match socket.try_clone() {
        Ok(receiver) => {
            let host = host.clone();
            thread::spawn(move || listen_for_pings(host, receiver, |session, from| session.video_client = Some(from)));
        }
        Err(e) => host.log(|| format!("video: soket tidak bisa digandakan ({e}); video tidak akan jalan")),
    }
    // Sesi yang sudah pernah dilayani thread ini (berhasil atau gagal) tidak dimulai ulang.
    let mut served: Option<u32> = None;
    loop {
        let target = host.stream.lock().unwrap().as_ref().filter(|s| s.peer.is_some() && Some(s.launch_id) != served).map(|s| Target { launch_id: s.launch_id, key: s.key, config: s.config.clone() });
        let Some(target) = target else {
            thread::sleep(Duration::from_millis(20));
            continue;
        };
        served = Some(target.launch_id);
        if let Err(why) = stream(&host, &socket, &target) {
            host.log(|| format!("video: {why}; sesi dihentikan"));
            host.send_control(Command::Terminate(Some(target.launch_id)));
        }
    }
}

/// Alamat video client sesi ini. `Err(())` = sesi sudah berakhir.
fn client_of(host: &Host, launch_id: u32) -> Result<Option<SocketAddr>, ()> {
    host.stream.lock().unwrap().as_ref().filter(|s| s.launch_id == launch_id).map(|s| s.video_client).ok_or(())
}

/// Kirim datagram satu frame, ditahan supaya tidak melebihi batas laju.
/// `skip` (hanya uji): `(n, b)` membuang `b` datagram berturut-turut tiap `n` datagram; `n` = 0 mematikannya.
/// `next_start`: kapan frame berikutnya boleh mulai dikirim. Seperti `ratecontrol_next_frame_start` Sunshine, sisa
/// jadwal frame sebelumnya diperhitungkan supaya dua frame yang berdekatan tidak terkirim sebagai satu semburan.
fn send_paced(socket: &UdpSocket, client: SocketAddr, datagrams: &[Vec<u8>], skip: (u64, u64), sent: &mut u64, next_start: &mut Instant) {
    let size = datagrams.first().map_or(1, Vec::len).max(1);
    let per_millisecond = (PACING_BITS_PER_SECOND / 8 / 1000 / size).max(1);
    let start = (*next_start).max(Instant::now());
    *next_start = start + Duration::from_secs_f64(datagrams.len() as f64 / per_millisecond as f64 / 1000.0);
    for (n, datagram) in datagrams.iter().enumerate() {
        if n.is_multiple_of(per_millisecond) {
            let due = start + Duration::from_millis((n / per_millisecond) as u64);
            if let Some(wait) = due.checked_duration_since(Instant::now()) {
                thread::sleep(wait);
            }
        }
        *sent += 1;
        if skip.0 != 0 && *sent % skip.0 < skip.1 {
            continue;
        }
        let _ = socket.send_to(datagram, client);
    }
}

#[cfg(not(windows))]
fn stream(_host: &Host, _socket: &UdpSocket, _target: &Target) -> Result<(), String> {
    Err("jalur video hanya ada untuk Windows".into())
}

#[cfg(windows)]
fn stream(host: &Host, socket: &UdpSocket, target: &Target) -> Result<(), String> {
    use cloudpc_stream_proto::video::FrameType;

    use capture::{Capture, CaptureError};
    use encode::Encoder;
    use packetize::{Frame, Packetizer};

    // Tunggu ping pertama: dari situ alamat tujuan diketahui.
    let deadline = Instant::now() + PING_TIMEOUT;
    let mut client = loop {
        match client_of(host, target.launch_id) {
            Err(()) => return Ok(()),
            Ok(Some(client)) => break client,
            Ok(None) if Instant::now() >= deadline => return Err("client tidak mengirim ping video".into()),
            Ok(None) => thread::sleep(Duration::from_millis(5)),
        }
    };
    host.log(|| format!("video: ping diterima dari {client}"));
    // Selama stream: timer 0,5 ms, prioritas tinggi, MMCSS DWM, Wi-Fi mode streaming, Mouse Keys bila tanpa mouse
    // (seperti Sunshine). Dikembalikan saat fungsi ini selesai.
    let mode = cloudpc_stream_common::streaming_mode::StreamingMode::enter(cloudpc_stream_common::streaming_mode::Options { host: true });
    host.log(|| format!("video: mode streaming {:?}", mode.applied));

    let config = &target.config;
    let mut packetizer = Packetizer::new(config.packet_size, config.min_fec_packets, Some(target.key));
    let interval = Duration::from_secs_f64(1.0 / f64::from(config.fps));
    let epoch = Instant::now();
    let mut index: u32 = 0;
    // Sejak kapan tangkapan layar hilang tanpa satu frame pun berhasil sesudahnya.
    let mut lost_since: Option<Instant> = None;
    let mut sent: u64 = 0;
    let (mut idr_frames, mut invalidations) = (0u32, 0u32);
    let mut pace = Instant::now();
    host.video_requests.lock().unwrap().clear();

    // Lingkar luar: (buat ulang) tangkapan + encoder; lingkar dalam: satu putaran per frame.
    loop {
        let built = Capture::new((config.width, config.height)).and_then(|capture| {
            let encoder = Encoder::new(&capture.device, &capture.nv12, config.width, config.height, config)?;
            Ok((capture, encoder))
        });
        let (mut capture, mut encoder) = match built {
            Ok(pair) => pair,
            // Sedang memulihkan tangkapan yang hilang: desktop mungkin belum bisa diduplikasi lagi, coba terus.
            Err(_) if lost_since.is_some_and(|since| since.elapsed() < CAPTURE_PATIENCE) => {
                if client_of(host, target.launch_id).is_err() {
                    return Ok(());
                }
                thread::sleep(Duration::from_millis(250));
                continue;
            }
            Err(why) => return Err(why),
        };
        *host.desktop_size.lock().unwrap() = capture.source;
        host.log(|| {
            format!("video: NVENC siap, layar {}x{} -> {}x{}@{} {} kbps, RFI: {}", capture.source.0, capture.source.1, config.width, config.height, config.fps, config.bitrate_kbps, encoder.supports_invalidation())
        });
        // Encoder baru selalu mulai dari IDR.
        let mut force_idr = true;
        // Waktu tangkapan hilang yang sedang dipulihkan, untuk mencatat lamanya gambar berhenti.
        let mut recovering: Option<Instant> = lost_since;
        let mut next_tick = Instant::now();

        loop {
            match client_of(host, target.launch_id) {
                Err(()) => {
                    host.log(|| format!("video: selesai, {index} frame ({idr_frames} IDR, {invalidations} invalidasi frame rujukan), {sent} datagram"));
                    return Ok(());
                }
                Ok(Some(address)) => client = address,
                Ok(None) => {}
            }

            let now = Instant::now();
            if next_tick > now {
                thread::sleep(next_tick - now);
            } else if now - next_tick > interval * 4 {
                // Tertinggal jauh (mis. desktop terkunci sebentar): jangan mengejar dengan semburan frame.
                next_tick = now;
            }
            next_tick += interval;

            let captured_at = Instant::now();
            // Belum ada gambar pertama: tunggu sampai desktop menggambar sesuatu.
            let wait_ms = if capture.has_image { 0 } else { 100 };
            match capture.update(wait_ms) {
                Ok(()) => {}
                Err(CaptureError::Lost) => {
                    // Jalur cepat: buka ulang duplikasi saja (device + encoder tetap, frame rujukan tetap sah, tanpa IDR).
                    let since = *lost_since.get_or_insert_with(Instant::now);
                    recovering = Some(since);
                    if reopen_quickly(host, target.launch_id, &mut capture) {
                        *host.desktop_size.lock().unwrap() = capture.source;
                        continue;
                    }
                    break;
                }
                Err(CaptureError::Other(why)) => return Err(why),
            }
            if !capture.has_image {
                continue;
            }
            capture.render();

            for request in std::mem::take(&mut *host.video_requests.lock().unwrap()) {
                match request {
                    VideoRequest::Idr => force_idr = true,
                    // Di luar jangkauan frame rujukan (atau tidak didukung): pulihkan dengan IDR.
                    VideoRequest::Invalidate { first, last } => {
                        if encoder.invalidate(first, last) {
                            invalidations += 1;
                        } else {
                            force_idr = true;
                        }
                    }
                }
            }
            index += 1;
            let encoded = encoder.encode(force_idr, u64::from(index))?;
            force_idr = false;
            lost_since = None;
            if let Some(since) = recovering.take() {
                host.log(|| format!("video: gambar jalan lagi {} ms setelah tangkapan hilang (layar {}x{})", since.elapsed().as_millis(), capture.source.0, capture.source.1));
            }
            idr_frames += u32::from(encoded.idr);
            let frame_type = match (encoded.idr, encoded.after_invalidation) {
                (true, _) => FrameType::Idr,
                (false, true) => FrameType::AfterRefInvalidation,
                (false, false) => FrameType::P,
            };
            let frame = Frame {
                index,
                frame_type,
                timestamp: (captured_at.duration_since(epoch).as_micros() * 9 / 100) as u32,
                processing_latency: (captured_at.elapsed().as_micros() / 100).min(u128::from(u16::MAX)) as u16,
                data: &encoded.data,
            };
            let skip = (u64::from(host.cfg.test_drop_video_every), u64::from(host.cfg.test_drop_video_burst.max(1)));
            send_paced(socket, client, &packetizer.packetize(&frame), skip, &mut sent, &mut pace);
            if index == 1 {
                host.log(|| format!("video: frame pertama terkirim ({} byte, IDR: {})", encoded.data.len(), encoded.idr));
            }
        }

        // Duplikasi hilang: lepas semuanya, tunggu sebentar, coba lagi beberapa kali.
        drop(encoder);
        drop(capture);
        let since = *lost_since.get_or_insert_with(Instant::now);
        if since.elapsed() >= CAPTURE_PATIENCE {
            return Err("tangkapan layar hilang lebih dari 60 detik".into());
        }
        host.log(|| "video: tangkapan layar hilang, dibuat ulang".into());
        thread::sleep(Duration::from_millis(250));
    }
}

#[cfg(windows)]
/// Coba buka ulang duplikasi layar dengan jeda pendek selama +-2 detik (pergantian mode tampilan biasanya selesai dalam
/// ratusan milidetik, dan duplikasi bisa hilang lagi sekali-dua kali di tengahnya). `false` = pakai pembuatan ulang penuh.
fn reopen_quickly(host: &Host, launch_id: u32, capture: &mut capture::Capture) -> bool {
    let started = Instant::now();
    for wait_ms in REOPEN_BACKOFF_MS {
        thread::sleep(Duration::from_millis(wait_ms));
        if client_of(host, launch_id).is_err() {
            return false;
        }
        if capture.reopen().is_ok() {
            host.log(|| format!("video: tangkapan layar hilang, dibuka ulang dalam {} ms (layar {}x{})", started.elapsed().as_millis(), capture.source.0, capture.source.1));
            return true;
        }
    }
    false
}

#[cfg(windows)]
/// Jeda antar percobaan `reopen_quickly` (total +-2 detik).
const REOPEN_BACKOFF_MS: [u64; 9] = [0, 20, 50, 100, 150, 250, 350, 450, 600];

#[cfg(test)]
mod tests {
    use super::*;
    use cloudpc_stream_proto::rtp::write_ping;

    #[cfg(windows)]
    #[test]
    fn quick_reopen_tries_immediately_and_gives_up_within_about_two_seconds() {
        assert_eq!(REOPEN_BACKOFF_MS[0], 0, "percobaan pertama tanpa jeda");
        let total: u64 = REOPEN_BACKOFF_MS.iter().sum();
        assert!((1500..=2500).contains(&total), "total {total} ms");
        assert!(REOPEN_BACKOFF_MS.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn only_pings_with_the_session_payload_are_accepted() {
        let good = write_ping(b"0123456789ABCDEF", 3);
        assert!(is_ping(&good, "0123456789ABCDEF"));
        assert!(!is_ping(&good, "FEDCBA9876543210"), "payload sesi lain");
        assert!(!is_ping(&good[..19], "0123456789ABCDEF"));
        assert!(!is_ping(b"PING", "0123456789ABCDEF"), "ping gaya lama tanpa payload");
        let mut long = good.to_vec();
        long.push(0);
        assert!(!is_ping(&long, "0123456789ABCDEF"));
    }

    #[test]
    fn capabilities_map_to_protocol_values() {
        let caps = VideoCapabilities { h264: true, hevc: true, av1: false, ref_invalidation: true, probed: true };
        assert_eq!(caps.server_codec_mode(), 0x101);
        assert!(caps.supports(0) && caps.supports(1) && !caps.supports(2) && !caps.supports(3));
        assert_eq!(VideoCapabilities::UNKNOWN.server_codec_mode(), 0x1_0101);
    }

    #[test]
    fn pacing_sends_everything_and_test_drop_skips_every_nth() {
        let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
        receiver.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        let datagrams: Vec<Vec<u8>> = (0..10u8).map(|i| vec![i; 32]).collect();
        let mut sent = 0;
        send_paced(&sender, receiver.local_addr().unwrap(), &datagrams, (4, 1), &mut sent, &mut Instant::now());
        assert_eq!(sent, 10);
        let receive_all = || {
            let mut got = Vec::new();
            let mut buffer = [0u8; 64];
            while let Ok((length, _)) = receiver.recv_from(&mut buffer) {
                assert_eq!(length, 32);
                got.push(buffer[0]);
            }
            got
        };
        assert_eq!(receive_all(), [0, 1, 2, 4, 5, 6, 8, 9], "datagram ke-4 dan ke-8 dibuang");

        // Semburan: 3 datagram berturut-turut tiap 6; penghitung berlanjut antar-frame (sudah 10 terkirim).
        send_paced(&sender, receiver.local_addr().unwrap(), &datagrams, (6, 3), &mut sent, &mut Instant::now());
        assert_eq!(receive_all(), [0, 4, 5, 6], "ke-12..14 dan ke-18..20 dibuang");
        send_paced(&sender, receiver.local_addr().unwrap(), &datagrams, (0, 3), &mut sent, &mut Instant::now());
        assert_eq!(receive_all().len(), 10, "0 = tidak ada yang dibuang");
    }

    #[test]
    fn letterbox_keeps_aspect_ratio() {
        assert_eq!(fit((1920, 1080), (1280, 720)), (0.0, 0.0, 1280.0, 720.0));
        assert_eq!(fit((1920, 1080), (1920, 1080)), (0.0, 0.0, 1920.0, 1080.0));
        // Layar 16:9 ke target 4:3: lebar penuh, bilah hitam atas-bawah.
        assert_eq!(fit((1920, 1080), (1600, 1200)), (0.0, 150.0, 1600.0, 900.0));
        // Layar 16:10 ke target 16:9: tinggi penuh, bilah hitam kiri-kanan.
        assert_eq!(fit((1920, 1200), (1920, 1080)), (96.0, 0.0, 1728.0, 1080.0));
    }
}
