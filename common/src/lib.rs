//! Kode yang dipakai bersama host dan client stream: kripto pairing/stream, identitas (kunci + sertifikat), dan
//! pembungkus pesan RTSP/kontrol terenkripsi. Tanpa soket dan tanpa thread.

pub mod crypto;
pub mod identity;
pub mod secure;
#[cfg(windows)]
pub mod streaming_mode;
