//! Client streaming CloudPC: pengganti Moonlight yang kompatibel dengan host Sunshine dan host CloudPC Rust.
//! Status per fase ada di `stream-rs/README.md`. Sejak C1: pairing, launch, RTSP, kanal kontrol; sejak C2: frame video utuh;
//! sejak C3: decode hardware + tampil di jendela; sejak C4: suara, input, log bergaya Moonlight (Windows).

pub mod control;
pub mod http;
pub mod nvhttp;
pub mod rtsp;
pub mod session;
pub mod store;
pub mod video;

fn u16_le(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}

pub mod audio;
pub mod frontend_sdl;
pub mod gamepad;
pub mod sdl;
pub mod input;
