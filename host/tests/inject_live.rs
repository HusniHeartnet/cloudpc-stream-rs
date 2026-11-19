//! Tes yang BENAR-BENAR menyuntikkan input ke desktop tempat tes berjalan. Tidak ikut `cargo test` biasa:
//! jalankan sengaja dengan `cargo test -p cloudpc-stream-host --test inject_live -- --ignored --test-threads 1`.
//! Efeknya dibuat sekecil mungkin: kursor bergeser 3 piksel lalu kembali, tombol F24 (tidak dipakai aplikasi)
//! ditekan sesaat, dan satu gamepad Xbox 360 virtual muncul sebentar (Windows membunyikan suara perangkat).
#![cfg(windows)]

use std::thread::sleep;
use std::time::Duration;

use cloudpc_stream_host::input::{Action, Injector, PadState};
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::Input::XboxController::{XInputGetState, XINPUT_STATE};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

fn cursor() -> (i32, i32) {
    let mut point = POINT::default();
    unsafe { GetCursorPos(&mut point).unwrap() };
    (point.x, point.y)
}

#[test]
#[ignore = "menggerakkan kursor 3 piksel dan menekan F24 di desktop ini"]
fn send_input_moves_the_cursor_and_presses_a_key() {
    let mut injector = Injector::default();
    let before = cursor();
    injector.apply(&Action::MouseMove { dx: 3, dy: 3 }).unwrap();
    sleep(Duration::from_millis(50));
    let moved = cursor();
    injector.apply(&Action::MouseMove { dx: -3, dy: -3 }).unwrap();
    sleep(Duration::from_millis(50));
    let after = cursor();
    assert_ne!(moved, before, "kursor harus bergerak");
    // Akselerasi penunjuk Windows bisa mengubah besar gerak, jadi hanya arah dan kembalinya yang diperiksa.
    assert!(moved.0 >= before.0 && moved.1 >= before.1, "arah gerak: {before:?} -> {moved:?}");
    assert!((after.0 - before.0).abs() <= 3 && (after.1 - before.1).abs() <= 3, "kursor harus kembali: {before:?} -> {after:?}");

    // F24 (0x87) tidak punya scancode di tabel, jadi lewat jalur kode virtual.
    let f24 = |up| Action::Key { vk: 0x87, scan: 0, extended: false, up };
    injector.apply(&f24(false)).unwrap();
    sleep(Duration::from_millis(30));
    let pressed = unsafe { GetAsyncKeyState(0x87) } as u16 & 0x8000 != 0;
    injector.apply(&f24(true)).unwrap();
    sleep(Duration::from_millis(30));
    let released = unsafe { GetAsyncKeyState(0x87) } as u16 & 0x8000 == 0;
    assert!(pressed && released, "F24 ditekan: {pressed}, dilepas: {released}");
}

#[test]
#[ignore = "memasang gamepad Xbox 360 virtual sebentar (butuh ViGEmBus)"]
fn virtual_gamepad_is_visible_to_xinput() {
    let mut injector = Injector::default();
    let state = PadState { buttons: 0x1000 | 0x0001, left_trigger: 200, right_trigger: 17, left_stick: (12345, -23456), right_stick: (-1, 32767) };
    injector.apply(&Action::Pad { index: 0, state: Some(state) }).unwrap();
    let user = injector.xinput_index(0).expect("gamepad virtual punya indeks XInput");
    sleep(Duration::from_millis(100));

    let mut read = XINPUT_STATE::default();
    assert_eq!(unsafe { XInputGetState(user, &mut read) }, 0, "XInput harus melihat gamepad {user}");
    let pad = read.Gamepad;
    assert_eq!((pad.wButtons.0, pad.bLeftTrigger, pad.bRightTrigger), (0x1001, 200, 17), "A + D-pad atas");
    assert_eq!((pad.sThumbLX, pad.sThumbLY, pad.sThumbRX, pad.sThumbRY), (12345, -23456, -1, 32767));

    // Dicabut: XInput tidak lagi menemukannya.
    injector.apply(&Action::Pad { index: 0, state: None }).unwrap();
    sleep(Duration::from_millis(300));
    assert_ne!(unsafe { XInputGetState(user, &mut read) }, 0, "gamepad harus hilang setelah dicabut");
}
