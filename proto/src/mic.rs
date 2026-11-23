//! Mikrofon pemain -> host. EKSTENSI CloudPC: protokol GameStream (Sunshine/Moonlight) tidak punya saluran mikrofon.
//!
//! Suara mikrofon dikirim client sebagai pesan kontrol `MIC_DATA` (terenkripsi AES-GCM seperti pesan kontrol lain),
//! lewat kanal ENet sendiri dan TIDAK dijamin sampai (suara yang terlambat lebih buruk daripada suara yang hilang).
//! Jadi tidak ada port baru: yang sudah menembus NAT/VPS untuk kanal kontrol otomatis membawa mikrofon juga.
//!
//! Host yang punya keluaran mikrofon menyatakannya di DESCRIBE (`a=x-cloudpc-mic:1`); client hanya mengirim ke host
//! yang menyatakannya, jadi Sunshine asli tidak pernah menerima pesan yang tidak dikenalnya.
//!
//! Isi pesan: `[nomor urut u16 LE][satu paket Opus]`. Opus mono 48 kHz, frame 20 ms, dengan FEC di dalam aliran
//! (paket berikutnya membawa salinan ringkas paket sebelumnya), jadi satu paket hilang masih bisa dipulihkan.

use crate::{need, u16_le, WireError};

/// Tipe pesan kontrol (client -> host). Di luar rentang ekstensi Sunshine (0x5500..=0x5504).
pub const PACKET_TYPE: u16 = 0x5580;
/// Atribut SDP di DESCRIBE host; nilainya versi format (sekarang 1).
pub const SDP_ATTRIBUTE: &str = "x-cloudpc-mic";
pub const VERSION: u32 = 1;
/// Kanal ENet khusus mikrofon (Moonlight membuka 0x30 kanal; yang ini tidak dipakai jenis input mana pun).
pub const ENET_CHANNEL: u8 = 0x2F;

pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: usize = 1;
pub const FRAME_MS: u32 = 20;
/// Sampel per frame (20 ms di 48 kHz).
pub const FRAME_SAMPLES: usize = (SAMPLE_RATE / 1000 * FRAME_MS) as usize;
/// Paket Opus terbesar yang diterima host (suara 20 ms di 32 kbit/dtk ±80 byte; batas ini jauh di atasnya).
pub const MAX_OPUS: usize = 400;

const HEADER: usize = 2;

/// Susun isi pesan `MIC_DATA`.
pub fn encode(sequence: u16, opus: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER + opus.len());
    out.extend_from_slice(&sequence.to_le_bytes());
    out.extend_from_slice(opus);
    out
}

/// Baca isi pesan `MIC_DATA`: `(nomor urut, paket Opus)`.
pub fn decode(payload: &[u8]) -> Result<(u16, &[u8]), WireError> {
    need(payload, HEADER + 1)?;
    let opus = &payload[HEADER..];
    if opus.len() > MAX_OPUS {
        return Err(WireError::Invalid("paket mikrofon terlalu besar"));
    }
    Ok((u16_le(&payload[..HEADER]), opus))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_layout() {
        let wire = encode(0x1234, &[9, 8, 7]);
        assert_eq!(wire, [0x34, 0x12, 9, 8, 7]);
        assert_eq!(decode(&wire).unwrap(), (0x1234, &[9u8, 8, 7][..]));
    }

    #[test]
    fn empty_or_oversized_packets_are_rejected() {
        assert!(decode(&[1, 0]).is_err(), "tanpa isi Opus");
        assert!(decode(&encode(1, &[0; MAX_OPUS])).is_ok());
        assert!(decode(&encode(1, &[0; MAX_OPUS + 1])).is_err());
    }

    #[test]
    fn frame_is_twenty_milliseconds() {
        assert_eq!(FRAME_SAMPLES, 960);
    }
}
