//! Keadaan satu stream yang dibagi antara thread sesi (jaringan + decode) dan panggilan dari Android (sentuhan, tombol,
//! permintaan berhenti). Murni data, tanpa Android.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use cloudpc_stream_client::input::InputPacket;

use crate::gesture::Trackpad;
use crate::gyro::Gyro;
use crate::overlay::Overlay;
use crate::pad::{Keyboard, Pads};

pub struct Shared {
    /// Android meminta stream ditutup (layar ditinggalkan, tombol kembali).
    pub stop: AtomicBool,
    /// Thread sesi sudah selesai (ditutup, diakhiri host, atau gagal).
    pub ended: AtomicBool,
    /// Paket input yang menunggu dikirim lewat kanal kontrol.
    pub input: Mutex<Vec<InputPacket>>,
    /// Ringkasan jaringan terbaru (untuk lapisan statistik) dan peringatan koneksi lambat.
    pub status: Mutex<String>,
    /// Baris ringkas satu detik terakhir: "60 FPS · 24 ms · 6 Mbps" (kosong sebelum gambar mengalir).
    pub brief: Mutex<String>,
    /// Kalimat kemajuan terakhir sebelum gambar pertama ("Memulai Desktop di ...").
    pub progress: Mutex<String>,
    /// Pesan galat bila stream gagal; kosong = berakhir normal.
    pub error: Mutex<String>,
    pub trackpad: Mutex<Trackpad>,
    pub pads: Mutex<Pads>,
    pub keyboard: Mutex<Keyboard>,
    pub gyro: Mutex<Gyro>,
    /// Stik di layar dan file tempat tata letak pemain disimpan.
    pub overlay: Mutex<Overlay>,
    pub overlay_path: PathBuf,
}

impl Shared {
    pub fn new(touch_slop: f32, pointer_speed: f32, overlay_path: PathBuf) -> Shared {
        Shared {
            stop: AtomicBool::new(false),
            ended: AtomicBool::new(false),
            input: Mutex::new(Vec::new()),
            status: Mutex::new(String::new()),
            brief: Mutex::new(String::new()),
            progress: Mutex::new(String::new()),
            error: Mutex::new(String::new()),
            trackpad: Mutex::new(Trackpad::new(touch_slop, pointer_speed)),
            pads: Mutex::new(Pads::default()),
            keyboard: Mutex::new(Keyboard::default()),
            gyro: Mutex::new(Gyro::default()),
            overlay: Mutex::new(Overlay::load(&overlay_path)),
            overlay_path,
        }
    }

    pub fn stopping(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    pub fn push(&self, packets: impl IntoIterator<Item = InputPacket>) {
        if let Ok(mut queue) = self.input.lock() {
            queue.extend(packets);
        }
    }

    pub fn take_input(&self) -> Vec<InputPacket> {
        self.input.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default()
    }

    /// Simpan tata letak stik layar pemain (dipanggil saat keluar dari mode atur, saat disembunyikan/ditampilkan, dan
    /// saat stream ditutup).
    pub fn save_overlay(&self) {
        if let Ok(overlay) = self.overlay.lock() {
            overlay.save(&self.overlay_path);
        }
    }

    pub fn set(slot: &Mutex<String>, text: impl Into<String>) {
        if let Ok(mut s) = slot.lock() {
            *s = text.into();
        }
    }

    pub fn get(slot: &Mutex<String>) -> String {
        slot.lock().map(|s| s.clone()).unwrap_or_default()
    }
}
