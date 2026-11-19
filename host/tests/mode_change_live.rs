//! Uji langsung (HANYA dengan `--ignored`): resolusi layar NVIDIA diganti sebentar, seperti game yang berganti mode tampilan
//! (temuan sesi #93), lalu dipastikan `Capture::reopen` memulihkan tangkapan dengan cepat tanpa membuat device/encoder
//! baru, dan ukuran sumber mengikuti layar baru. Layar dikembalikan ke mode semula di akhir (juga bila tes gagal).
//! Jangan jalankan di PC yang sedang dipakai orang: layar berkedip dan berganti resolusi sekitar 2 detik.
#![cfg(windows)]

use std::time::{Duration, Instant};

use cloudpc_stream_host::video::capture::{Capture, CaptureError};
use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::{ChangeDisplaySettingsExW, EnumDisplaySettingsW, CDS_TYPE, DEVMODEW, DISP_CHANGE_SUCCESSFUL, ENUM_CURRENT_SETTINGS, ENUM_DISPLAY_SETTINGS_MODE};

fn current() -> DEVMODEW {
    let mut mode = DEVMODEW { dmSize: std::mem::size_of::<DEVMODEW>() as u16, ..Default::default() };
    unsafe { EnumDisplaySettingsW(PCWSTR::null(), ENUM_CURRENT_SETTINGS, &mut mode).ok().expect("mode layar sekarang") };
    mode
}

/// Mode lain dengan refresh rate sama dan ukuran berbeda (lebih kecil lebih dulu).
fn other_mode(now: &DEVMODEW) -> DEVMODEW {
    let mut best: Option<DEVMODEW> = None;
    for i in 0.. {
        let mut mode = DEVMODEW { dmSize: std::mem::size_of::<DEVMODEW>() as u16, ..Default::default() };
        if !unsafe { EnumDisplaySettingsW(PCWSTR::null(), ENUM_DISPLAY_SETTINGS_MODE(i), &mut mode) }.as_bool() {
            break;
        }
        let differs = (mode.dmPelsWidth, mode.dmPelsHeight) != (now.dmPelsWidth, now.dmPelsHeight);
        if differs && mode.dmDisplayFrequency == now.dmDisplayFrequency && mode.dmBitsPerPel == now.dmBitsPerPel && mode.dmPelsWidth >= 1024 {
            if best.is_none_or(|b| mode.dmPelsWidth * mode.dmPelsHeight > b.dmPelsWidth * b.dmPelsHeight && mode.dmPelsWidth < now.dmPelsWidth) {
                best = Some(mode);
            }
        }
    }
    best.expect("tidak ada mode tampilan lain untuk diuji")
}

fn apply(mode: &DEVMODEW) {
    let result = unsafe { ChangeDisplaySettingsExW(PCWSTR::null(), Some(mode), None, CDS_TYPE(0), None) };
    assert_eq!(result, DISP_CHANGE_SUCCESSFUL, "ganti mode layar");
}

struct Restore(DEVMODEW);
impl Drop for Restore {
    fn drop(&mut self) {
        unsafe { ChangeDisplaySettingsExW(PCWSTR::null(), Some(&self.0), None, CDS_TYPE(0), None) };
    }
}

fn update_until_lost(capture: &mut Capture) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        match capture.update(50) {
            Ok(()) => {}
            Err(CaptureError::Lost) => return true,
            Err(CaptureError::Other(why)) => panic!("{why}"),
        }
    }
    false
}

fn reopen_and_wait_image(capture: &mut Capture) -> Duration {
    let started = Instant::now();
    loop {
        if capture.reopen().is_ok() {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(3), "duplikasi tidak bisa dibuka ulang dalam 3 detik");
        std::thread::sleep(Duration::from_millis(20));
    }
    while !capture.has_image {
        match capture.update(50) {
            Ok(()) => {}
            Err(CaptureError::Lost) => {
                let _ = capture.reopen();
            }
            Err(CaptureError::Other(why)) => panic!("{why}"),
        }
        assert!(started.elapsed() < Duration::from_secs(5), "tidak ada gambar baru dalam 5 detik");
    }
    started.elapsed()
}

#[test]
#[ignore = "mengganti resolusi layar sungguhan; jalankan manual: cargo test -p cloudpc-stream-host --test mode_change_live -- --ignored --nocapture"]
fn capture_survives_display_mode_change_without_rebuilding() {
    let original = current();
    let other = other_mode(&original);
    let _restore = Restore(original);
    let mut capture = Capture::new((1280, 720)).expect("tangkapan layar");
    let device_before = capture.device.clone();
    let first = capture.source;

    apply(&other);
    assert!(update_until_lost(&mut capture), "pergantian mode harus membuat duplikasi hilang");
    let took = reopen_and_wait_image(&mut capture);
    println!("mode {}x{} -> {}x{}: gambar kembali dalam {} ms", first.0, first.1, capture.source.0, capture.source.1, took.as_millis());
    assert_eq!(capture.source, (other.dmPelsWidth, other.dmPelsHeight), "sumber mengikuti layar baru");
    assert_eq!(capture.device, device_before, "device (dan encoder) tidak dibuat ulang");
    capture.render();

    apply(&original);
    assert!(update_until_lost(&mut capture));
    let took = reopen_and_wait_image(&mut capture);
    println!("kembali ke {}x{}: gambar kembali dalam {} ms", capture.source.0, capture.source.1, took.as_millis());
    assert_eq!(capture.source, first);
    assert!(took < Duration::from_secs(2), "pemulihan harus di bawah 2 detik");
}
