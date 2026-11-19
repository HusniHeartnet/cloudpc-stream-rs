//! Format kawat protokol GameStream seperti yang dipakai Sunshine dan Moonlight.
//!
//! Crate ini hanya berisi tata letak byte dan konstanta, tanpa soket, tanpa kripto berat, tanpa thread,
//! supaya host dan client memakai definisi yang SAMA dan semuanya bisa diuji tanpa jaringan.
//!
//! Sumber rujukan (GPLv3): `Sunshine/src/stream.cpp`, `rtsp.cpp`, `nvhttp.cpp` dan
//! `moonlight-common-c/src/Video.h`, `ControlStream.c`, `Input.h`, `RtpAudioQueue.h`.

pub mod audio;
pub mod control;
pub mod fec;
pub mod input;
pub mod pairing;
pub mod ports;
pub mod rtp;
pub mod rtsp;
pub mod video;

/// Galat saat membaca atau menulis paket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireError {
    /// Buffer lebih pendek dari yang dibutuhkan.
    TooShort { need: usize, got: usize },
    /// Isi paket tidak sesuai format.
    Invalid(&'static str),
}

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WireError::TooShort { need, got } => write!(f, "paket terlalu pendek: butuh {need} byte, ada {got}"),
            WireError::Invalid(why) => write!(f, "paket tidak valid: {why}"),
        }
    }
}

impl std::error::Error for WireError {}

pub(crate) fn need(buf: &[u8], n: usize) -> Result<(), WireError> {
    if buf.len() < n {
        return Err(WireError::TooShort { need: n, got: buf.len() });
    }
    Ok(())
}

pub(crate) fn u16_le(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}

pub(crate) fn u16_be(b: &[u8]) -> u16 {
    u16::from_be_bytes([b[0], b[1]])
}

pub(crate) fn u32_le(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

pub(crate) fn u32_be(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}
