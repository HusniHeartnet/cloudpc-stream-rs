//! Fase C1. Kanal kontrol ENet dari sisi client.
//!
//! Rujukan: `moonlight-common-c/src/ControlStream.c`. Setelah tersambung (membawa `X-SS-Connect-Data`), client
//! mengirim permintaan IDR lalu START_B, kemudian ping tiap 100 ms. Semua pesan terenkripsi bila host menyalakan
//! enkripsi kontrol (selalu, untuk host CloudPC).

use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use cloudpc_stream_common::secure::{open, seal};
use cloudpc_stream_proto::control::{packet_type, Origin};
use rusty_enet as enet;

/// Moonlight membuka 0x30 kanal ENet (`CTRL_CHANNEL_COUNT`).
const CHANNELS: usize = 0x30;
const CHANNEL_GENERIC: u8 = 0;
const CHANNEL_URGENT: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlEvent {
    /// Host mengakhiri sesi dengan kode ini (`0x80030023` = ditutup host).
    Terminated(u32),
    Disconnected,
    /// Getar gamepad `pad`: motor frekuensi rendah dan tinggi, 0..=65535.
    Rumble { pad: u16, low: u16, high: u16 },
    /// Pesan sah lain dari host (HDR, LED, ...): tipenya saja.
    Message(u16),
}

pub struct ControlLink {
    enet: enet::Host<UdpSocket>,
    peer: enet::PeerID,
    key: [u8; 16],
    seq: u32,
}

impl ControlLink {
    /// Sambung ke kanal kontrol dan kirim pembuka (IDR + START_B).
    pub fn connect(address: SocketAddr, connect_data: u32, key: [u8; 16], timeout: Duration) -> Result<ControlLink, String> {
        let bind = if address.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" };
        let socket = UdpSocket::bind(bind).map_err(|e| format!("soket kontrol: {e}"))?;
        let mut enet = enet::Host::new(socket, enet::HostSettings { peer_limit: 1, ..Default::default() }).map_err(|_| "ENet gagal dimulai".to_string())?;
        let peer = enet.connect(address, CHANNELS, connect_data).map_err(|_| "ENet tidak bisa menyambung".to_string())?.id();
        let deadline = Instant::now() + timeout;
        loop {
            match enet.service() {
                Ok(Some(event)) => match event.no_ref() {
                    enet::EventNoRef::Connect { .. } => break,
                    enet::EventNoRef::Disconnect { .. } => return Err("host menolak kanal kontrol".into()),
                    enet::EventNoRef::Receive { .. } => {}
                },
                Ok(None) | Err(_) => std::thread::sleep(Duration::from_millis(2)),
            }
            if Instant::now() >= deadline {
                return Err(format!("kanal kontrol {address} tidak tersambung dalam {} detik", timeout.as_secs()));
            }
        }
        let mut link = ControlLink { enet, peer, key, seq: 0 };
        link.send(packet_type::REQUEST_IDR_FRAME, &[0, 0], CHANNEL_GENERIC, true);
        link.send(packet_type::START_B, &[0], CHANNEL_GENERIC, true);
        Ok(link)
    }

    fn send(&mut self, kind: u16, payload: &[u8], channel: u8, reliable: bool) {
        let message = seal(&self.key, self.seq, Origin::Client, kind, payload);
        self.seq = self.seq.wrapping_add(1);
        let packet = if reliable { enet::Packet::reliable(message.as_slice()) } else { enet::Packet::unreliable(message.as_slice()) };
        let _ = self.enet.peer_mut(self.peer).send(channel, &packet);
    }

    /// Ping berkala (`0x0200`, payload `[panjang 4][stempel 0]` little-endian), tiap 100 ms seperti Moonlight.
    pub fn ping(&mut self) {
        self.send(packet_type::PERIODIC_PING, &[4, 0, 0, 0, 0, 0, 0, 0], CHANNEL_GENERIC, false);
    }

    /// Minta frame IDR (setelah paket video hilang tak terpulihkan).
    pub fn request_idr(&mut self) {
        self.send(packet_type::REQUEST_IDR_FRAME, &[0, 0], CHANNEL_URGENT, true);
    }

    /// Minta host berhenti merujuk frame `first..=last` (`SS_RFI_REQUEST`: dua indeks 64 bit little-endian + 8 byte nol,
    /// sama dengan Moonlight; host CloudPC dan Sunshine membaca dua angka pertama).
    pub fn invalidate_reference_frames(&mut self, first: u32, last: u32) {
        let mut payload = [0u8; 24];
        payload[..8].copy_from_slice(&u64::from(first).to_le_bytes());
        payload[8..16].copy_from_slice(&u64::from(last).to_le_bytes());
        self.send(packet_type::INVALIDATE_REF_FRAMES, &payload, CHANNEL_URGENT, true);
    }

    /// Paket input (`INPUT_DATA`) di kanal ENet jenisnya, terkirim pasti dan berurutan seperti Moonlight.
    pub fn send_input(&mut self, channel: u8, packet: &[u8]) {
        self.send(packet_type::INPUT_DATA, packet, channel, true);
    }

    /// Waktu pulang-pergi ENet ke host dan variasinya (milidetik), seperti statistik Moonlight.
    pub fn round_trip(&mut self) -> (u32, u32) {
        let peer = self.enet.peer_mut(self.peer);
        (peer.round_trip_time().as_millis() as u32, peer.round_trip_time_variance().as_millis() as u32)
    }

    /// Kerjakan ENet sampai `budget` habis; kembalikan kejadian dari host.
    pub fn pump(&mut self, budget: Duration) -> Vec<ControlEvent> {
        let deadline = Instant::now() + budget;
        let mut events = Vec::new();
        loop {
            match self.enet.service() {
                Ok(Some(event)) => match event.no_ref() {
                    enet::EventNoRef::Disconnect { .. } => events.push(ControlEvent::Disconnected),
                    enet::EventNoRef::Receive { packet, .. } => match open(&self.key, Origin::Host, packet.data()) {
                        Ok((packet_type::TERMINATION, payload)) => {
                            // Kode terminasi big-endian; payload pendek (host lama) dianggap "ditutup host".
                            let code = payload.get(..4).map(|b| u32::from_be_bytes(b.try_into().unwrap())).unwrap_or(0x8003_0023);
                            events.push(ControlEvent::Terminated(code));
                        }
                        Ok((packet_type::RUMBLE_DATA, payload)) => match cloudpc_stream_proto::control::parse_rumble(&payload) {
                            Some((pad, low, high)) => events.push(ControlEvent::Rumble { pad, low, high }),
                            None => events.push(ControlEvent::Message(packet_type::RUMBLE_DATA)),
                        },
                        Ok((kind, _)) => events.push(ControlEvent::Message(kind)),
                        Err(_) => {}
                    },
                    enet::EventNoRef::Connect { .. } => {}
                },
                Ok(None) | Err(_) => {
                    if Instant::now() >= deadline {
                        return events;
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        }
    }

    /// Putus dengan sopan (host melihat client pergi, bukan waktu habis).
    pub fn close(mut self) {
        self.enet.peer_mut(self.peer).disconnect(0);
        let _ = self.pump(Duration::from_millis(200));
    }
}
