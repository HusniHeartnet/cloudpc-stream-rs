//! Kanal kontrol (ENet di UDP, port dasar + 10): pesan `[tipe u16 LE][panjang u16 LE][payload]`, dan bila
//! enkripsi aktif dibungkus amplop `[0x0001][panjang][seq][tag GCM 16][ciphertext]`.
//!
//! Tabel tipe = `packetTypes` di `Sunshine/src/stream.cpp` dan `packetTypesGen7Enc` di `ControlStream.c`.

use crate::{need, u16_le, u32_le, WireError};

/// Tipe pesan kontrol (generasi 7 terenkripsi, yang dipakai Sunshine).
pub mod packet_type {
    pub const START_A: u16 = 0x0305;
    pub const START_B: u16 = 0x0307;
    pub const INVALIDATE_REF_FRAMES: u16 = 0x0301;
    pub const LOSS_STATS: u16 = 0x0201;
    pub const FRAME_STATS: u16 = 0x0204;
    pub const INPUT_DATA: u16 = 0x0206;
    pub const RUMBLE_DATA: u16 = 0x010b;
    pub const TERMINATION: u16 = 0x0109;
    pub const PERIODIC_PING: u16 = 0x0200;
    pub const REQUEST_IDR_FRAME: u16 = 0x0302;
    pub const ENCRYPTED: u16 = 0x0001;
    pub const HDR_MODE: u16 = 0x010e;
    // Ekstensi protokol Sunshine (host -> client).
    pub const RUMBLE_TRIGGERS: u16 = 0x5500;
    pub const SET_MOTION_EVENT: u16 = 0x5501;
    pub const SET_RGB_LED: u16 = 0x5502;
    pub const SET_ADAPTIVE_TRIGGERS: u16 = 0x5503;
    pub const SET_PLAYER_LEDS: u16 = 0x5504;
    /// Client -> host: status FEC per frame (`SS_FRAME_FEC_PTYPE`), nilainya sama dengan `SET_RGB_LED` tapi arahnya berlawanan.
    pub const FRAME_FEC_STATUS: u16 = 0x5502;
}

/// Bendera enkripsi yang dinegosiasikan lewat SDP (`x-ss-general.encryption*`).
pub mod encryption {
    pub const CONTROL_V2: u32 = 0x01;
    pub const VIDEO: u32 = 0x02;
    pub const AUDIO: u32 = 0x04;
}

/// Bendera fitur client (`x-ss-general.featureFlags` dari Moonlight).
pub mod client_feature {
    /// Client mengirim status FEC per frame.
    pub const FEC_STATUS: u32 = 0x01;
    /// Client mendukung `X-SS-Ping-Payload` dan `X-SS-Connect-Data`.
    pub const SESSION_ID_V1: u32 = 0x02;
}

/// Payload `RUMBLE_DATA` (host -> client): `[4 byte cadangan][id gamepad][motor frekuensi rendah][motor frekuensi
/// tinggi]`, 16 bit little-endian, motor 0..=65535 (`control_rumble_t` Sunshine, `queueAsyncCallback` Moonlight).
pub fn rumble_payload(pad: u16, low: u16, high: u16) -> [u8; 10] {
    let mut out = [0u8; 10];
    out[4..6].copy_from_slice(&pad.to_le_bytes());
    out[6..8].copy_from_slice(&low.to_le_bytes());
    out[8..10].copy_from_slice(&high.to_le_bytes());
    out
}

/// Kebalikan `rumble_payload`: `(id gamepad, motor rendah, motor tinggi)`.
pub fn parse_rumble(payload: &[u8]) -> Option<(u16, u16, u16)> {
    let field = |at: usize| payload.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    Some((field(4)?, field(6)?, field(8)?))
}

pub const HEADER_SIZE: usize = 4;
pub const GCM_TAG_SIZE: usize = 16;
/// `[0x0001 u16][panjang u16][seq u32]`, semuanya little-endian.
pub const ENCRYPTED_HEADER_SIZE: usize = 8;

/// Susun pesan polos `[tipe][panjang][payload]`.
pub fn encode_message(packet_type: u16, payload: &[u8]) -> Result<Vec<u8>, WireError> {
    let len = u16::try_from(payload.len()).map_err(|_| WireError::Invalid("payload kontrol lebih dari 65535 byte"))?;
    let mut out = Vec::with_capacity(HEADER_SIZE + payload.len());
    out.extend_from_slice(&packet_type.to_le_bytes());
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(payload);
    Ok(out)
}

/// Baca pesan polos, mengembalikan `(tipe, payload)`. Panjang di header harus muat di buffer.
pub fn decode_message(buf: &[u8]) -> Result<(u16, &[u8]), WireError> {
    need(buf, HEADER_SIZE)?;
    let len = usize::from(u16_le(&buf[2..4]));
    need(buf, HEADER_SIZE + len)?;
    Ok((u16_le(&buf[0..2]), &buf[HEADER_SIZE..HEADER_SIZE + len]))
}

/// Siapa pengirim pesan kontrol terenkripsi; masuk ke IV supaya dua arah tidak pernah memakai IV yang sama.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Host,
    Client,
}

/// IV AES-GCM kanal kontrol (mode `CONTROL_V2`): seq little-endian, lalu `'H'`/`'C'` dan `'C'` di dua byte terakhir.
pub fn control_iv(seq: u32, origin: Origin) -> [u8; 12] {
    let mut iv = [0u8; 12];
    iv[..4].copy_from_slice(&seq.to_le_bytes());
    iv[10] = match origin {
        Origin::Host => b'H',
        Origin::Client => b'C',
    };
    iv[11] = b'C';
    iv
}

/// Header amplop terenkripsi untuk plaintext sepanjang `plaintext_len` (pesan polos termasuk headernya).
/// Field panjang = seq (4) + tag (16) + ciphertext.
pub fn encrypted_header(seq: u32, plaintext_len: usize) -> Result<[u8; ENCRYPTED_HEADER_SIZE], WireError> {
    let len = u16::try_from(4 + GCM_TAG_SIZE + plaintext_len).map_err(|_| WireError::Invalid("pesan kontrol terenkripsi terlalu panjang"))?;
    let mut out = [0u8; ENCRYPTED_HEADER_SIZE];
    out[0..2].copy_from_slice(&packet_type::ENCRYPTED.to_le_bytes());
    out[2..4].copy_from_slice(&len.to_le_bytes());
    out[4..8].copy_from_slice(&seq.to_le_bytes());
    Ok(out)
}

/// Amplop terenkripsi yang sudah dipilah (belum didekripsi).
#[derive(Debug, PartialEq, Eq)]
pub struct EncryptedMessage<'a> {
    pub seq: u32,
    pub tag: &'a [u8],
    pub ciphertext: &'a [u8],
}

pub fn split_encrypted(buf: &[u8]) -> Result<EncryptedMessage<'_>, WireError> {
    need(buf, ENCRYPTED_HEADER_SIZE + GCM_TAG_SIZE)?;
    if u16_le(&buf[0..2]) != packet_type::ENCRYPTED {
        return Err(WireError::Invalid("bukan pesan kontrol terenkripsi"));
    }
    let len = usize::from(u16_le(&buf[2..4]));
    if len < 4 + GCM_TAG_SIZE {
        return Err(WireError::Invalid("panjang amplop lebih kecil dari seq + tag"));
    }
    // Field panjang dihitung mulai dari seq.
    need(buf, 4 + len)?;
    let body = &buf[ENCRYPTED_HEADER_SIZE..4 + len];
    Ok(EncryptedMessage { seq: u32_le(&buf[4..8]), tag: &body[..GCM_TAG_SIZE], ciphertext: &body[GCM_TAG_SIZE..] })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_message_round_trip() {
        let wire = encode_message(packet_type::REQUEST_IDR_FRAME, &[0, 0]).unwrap();
        assert_eq!(wire, [0x02, 0x03, 0x02, 0x00, 0, 0]);
        assert_eq!(decode_message(&wire).unwrap(), (packet_type::REQUEST_IDR_FRAME, &[0u8, 0][..]));
    }

    #[test]
    fn declared_length_beyond_buffer_is_rejected() {
        assert_eq!(decode_message(&[0x06, 0x02, 0x10, 0x00, 1, 2]), Err(WireError::TooShort { need: 20, got: 6 }));
    }

    #[test]
    fn host_and_client_ivs_differ_for_same_seq() {
        let host = control_iv(0x0102_0304, Origin::Host);
        assert_eq!(host, [4, 3, 2, 1, 0, 0, 0, 0, 0, 0, b'H', b'C']);
        assert_ne!(host, control_iv(0x0102_0304, Origin::Client));
    }

    #[test]
    fn encrypted_envelope_layout() {
        let plaintext_len = 6;
        let mut wire = encrypted_header(9, plaintext_len).unwrap().to_vec();
        wire.extend_from_slice(&[0xAA; GCM_TAG_SIZE]);
        wire.extend_from_slice(&[0xBB; 6]);
        assert_eq!(&wire[..8], &[0x01, 0x00, 26, 0x00, 9, 0, 0, 0]);
        let msg = split_encrypted(&wire).unwrap();
        assert_eq!((msg.seq, msg.tag, msg.ciphertext), (9, &[0xAA; 16][..], &[0xBB; 6][..]));
    }

    #[test]
    fn truncated_or_wrong_envelope_is_rejected() {
        let mut wire = encrypted_header(1, 4).unwrap().to_vec();
        wire.extend_from_slice(&[0; GCM_TAG_SIZE + 3]);
        assert!(split_encrypted(&wire).is_err(), "ciphertext kurang 1 byte");
        let plain = encode_message(packet_type::LOSS_STATS, &[0; 32]).unwrap();
        assert!(split_encrypted(&plain).is_err());
    }

    #[test]
    fn rumble_payload_matches_sunshine_layout() {
        let payload = rumble_payload(1, 0x8000, 0xFFFF);
        assert_eq!(payload, [0, 0, 0, 0, 1, 0, 0x00, 0x80, 0xFF, 0xFF]);
        assert_eq!(parse_rumble(&payload), Some((1, 0x8000, 0xFFFF)));
        assert_eq!(parse_rumble(&payload[..9]), None);
    }
}
