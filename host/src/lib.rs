//! Host streaming CloudPC: pengganti Sunshine yang kompatibel dengan client Moonlight.
//! Status per fase ada di `stream-rs/README.md`.

pub mod audio;
pub mod control;
pub use cloudpc_stream_common::crypto;
pub mod http;
pub mod input;
pub mod mic;
pub mod nvhttp;
#[cfg(windows)]
pub mod nvprefs;
pub mod rtsp;
pub mod session;
pub mod state;
#[cfg(windows)]
pub mod supervise;
pub mod tls;
pub mod video;
pub mod xml;
