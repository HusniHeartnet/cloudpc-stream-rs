//! Client streaming CloudPC untuk Android. Protokolnya crate `cloudpc-stream-client` apa adanya (pairing, RTSP, kanal
//! kontrol, FEC, Opus); yang khusus Android ada di sini:
//!
//! - `video` : decode hardware lewat NDK `AMediaCodec` langsung ke `Surface` (tanpa FFmpeg, tanpa salinan gambar);
//! - `audio` : keluaran suara AAudio (Android 8.0+);
//! - `jni_api`: fungsi yang dipanggil kelas Kotlin `id.cloudpc.stream.Native`;
//! - `gesture`, `overlay`, `pad`, `report`, `state`: logika murni (layar sentuh sebagai trackpad, stik di layar yang bisa
//!   diatur pemain, gamepad/keyboard Android, log statistik bergaya Moonlight), teruji di mesin pengembang dengan
//!   `cargo test -p cloudpc-stream-android`.
//!
//! Di luar Android crate ini hanya berisi bagian murni itu.

pub mod gesture;
pub mod gyro;
pub mod overlay;
pub mod pad;
pub mod report;
pub mod state;

#[cfg(target_os = "android")]
mod audio;
#[cfg(target_os = "android")]
mod jni_api;
#[cfg(target_os = "android")]
mod video;
