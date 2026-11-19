//! Jalur video client: terima UDP -> susun ulang + pulihkan FEC -> decode -> tampilkan.

#[cfg(windows)]
pub mod decode;
pub mod depacketize;
#[cfg(windows)]
pub mod display;
pub mod ff;
mod ffi;
#[cfg(windows)]
mod overlay;
#[cfg(windows)]
pub mod render;

/// Ringkasan tampilan (jendela Windows atau SDL) untuk log dan blok statistik.
/// Catatan dari thread jendela (mis. jendela macet) yang ikut dicetak di baris statistik berkala `session`.
pub static NOTES: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Ambil catatan yang menunggu dicetak.
pub fn take_notes() -> Vec<String> {
    NOTES.lock().map(|mut n| std::mem::take(&mut *n)).unwrap_or_default()
}

pub(crate) fn note(text: String) {
    if let Ok(mut n) = NOTES.lock() {
        n.push(text);
    }
}

#[derive(Debug, Clone, Default)]
pub struct DisplayReport {
    pub decoder: String,
    pub ffmpeg: String,
    pub decoded: u64,
    pub presented: u64,
    /// Frame yang didecode tetapi tidak ditampilkan karena frame lebih baru sudah menunggu.
    pub skipped: u64,
    pub decode_errors: u64,
    /// Rata-rata lama decode per frame, milidetik.
    pub decode_ms: f64,
    /// Rata-rata dari frame utuh tiba di client sampai tampil di layar, milidetik.
    pub receive_to_present_ms: f64,
    /// Rata-rata lama menggambar + Present, milidetik.
    pub render_ms: f64,
    pub error: Option<String>,
    /// "SDL3" atau "XInput", dan stik yang tersambung selama sesi.
    pub gamepad_backend: String,
    pub gamepads: Vec<String>,
}
