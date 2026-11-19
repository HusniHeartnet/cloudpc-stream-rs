//! Fase H4. Input dari client -> Windows.
//!
//! Dua tahap supaya bisa diuji tanpa menyentuh desktop: `Translator` mengubah kejadian input (dari
//! `cloudpc_stream_proto::input`) jadi `Action` tingkat Windows (murni data), lalu `Injector` menjalankannya dengan
//! `SendInput` (keyboard/mouse) dan ViGEmBus (gamepad Xbox 360 virtual).
//!
//! Sunshine sekarang memakai driver HID virtual berlisensi; yang di sini mengikuti jalur `SendInput` Sunshine lama.
//! Batas `SendInput` tetap berlaku: tanpa mouse fisik di host input tidak masuk Raw Input (Sunshine issue #4792),
//! dan jendela yang berjalan sebagai administrator tidak menerima input dari proses biasa.
//! Belum: sentuh, pena, getar gamepad ke client, sensor gerak.

use std::time::{Duration, Instant};

use cloudpc_stream_proto::input::{Gamepad, InputEvent, KEY_FLAG_NON_NORMALIZED};

/// Tombol yang ditahan mulai berulang setelah ini (Windows tidak mengulang tombol sintetis sendiri).
const KEY_REPEAT_DELAY: Duration = Duration::from_millis(500);
const KEY_REPEAT_INTERVAL: Duration = Duration::from_millis(33);
/// XInput mengenal paling banyak empat gamepad.
pub const MAX_PADS: usize = 4;

/// Keadaan gamepad Xbox 360 (`XINPUT_GAMEPAD`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PadState {
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub left_stick: (i16, i16),
    pub right_stick: (i16, i16),
}

/// Satu tindakan di Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// `scan` bukan 0: dikirim sebagai scancode (cara yang dibaca game); kalau 0, sebagai kode virtual `vk`.
    Key { vk: u8, scan: u16, extended: bool, up: bool },
    /// Satu unit UTF-16 dari teks.
    Unicode { unit: u16, up: bool },
    MouseMove { dx: i32, dy: i32 },
    /// Posisi di layar utama, 0..=65535 per sumbu.
    MouseTo { x: u16, y: u16 },
    /// `button`: 1 kiri, 2 tengah, 3 kanan, 4 X1, 5 X2.
    MouseButton { button: u8, up: bool },
    Wheel { amount: i32, horizontal: bool },
    /// `None` = gamepad dicabut.
    Pad { index: u8, state: Option<PadState> },
}

/// Ukuran layar host dan gambar stream, untuk memetakan posisi mouse absolut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometry {
    pub desktop: (u32, u32),
    pub stream: (u32, u32),
}

/// Scancode (set 1) tata letak US untuk kode tombol virtual; 0 = tidak ada. Client menormalkan kode ke tata letak
/// US, jadi tabel tetap ini benar apa pun tata letak keyboard host.
fn us_scancode(vk: u8) -> u16 {
    match vk {
        0x08 => 0x0E,                        // Backspace
        0x09 => 0x0F,                        // Tab
        0x0D => 0x1C,                        // Enter
        0x10 | 0xA0 => 0x2A,                 // Shift kiri
        0xA1 => 0x36,                        // Shift kanan
        0x11 | 0xA2 | 0xA3 => 0x1D,          // Ctrl (kanan = extended)
        0x12 | 0xA4 | 0xA5 => 0x38,          // Alt (kanan = extended)
        0x14 => 0x3A,                        // Caps Lock
        0x1B => 0x01,                        // Esc
        0x20 => 0x39,                        // Spasi
        0x21 => 0x49,                        // Page Up
        0x22 => 0x51,                        // Page Down
        0x23 => 0x4F,                        // End
        0x24 => 0x47,                        // Home
        0x25 => 0x4B,                        // Kiri
        0x26 => 0x48,                        // Atas
        0x27 => 0x4D,                        // Kanan
        0x28 => 0x50,                        // Bawah
        0x2D => 0x52,                        // Insert
        0x2E => 0x53,                        // Delete
        0x30 => 0x0B,                        // 0
        0x31..=0x39 => u16::from(vk) - 0x2F, // 1..9
        0x41..=0x5A => [0x1E, 0x30, 0x2E, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32, 0x31, 0x18, 0x19, 0x10, 0x13, 0x1F, 0x14, 0x16, 0x2F, 0x11, 0x2D, 0x15, 0x2C][usize::from(vk - 0x41)],
        0x5B => 0x5B,                        // Win kiri
        0x5C => 0x5C,                        // Win kanan
        0x5D => 0x5D,                        // Menu
        0x60 => 0x52,                        // Numpad 0
        0x61..=0x63 => u16::from(vk) - 0x12, // Numpad 1..3
        0x64..=0x66 => u16::from(vk) - 0x19, // Numpad 4..6
        0x67..=0x69 => u16::from(vk) - 0x20, // Numpad 7..9
        0x6A => 0x37,                        // Numpad *
        0x6B => 0x4E,                        // Numpad +
        0x6D => 0x4A,                        // Numpad -
        0x6E => 0x53,                        // Numpad .
        0x6F => 0x35,                        // Numpad /
        0x70..=0x79 => u16::from(vk) - 0x35, // F1..F10
        0x7A => 0x57,                        // F11
        0x7B => 0x58,                        // F12
        0x90 => 0x45,                        // Num Lock
        0x91 => 0x46,                        // Scroll Lock
        0xBA => 0x27,                        // ;
        0xBB => 0x0D,                        // =
        0xBC => 0x33,                        // ,
        0xBD => 0x0C,                        // -
        0xBE => 0x34,                        // .
        0xBF => 0x35,                        // /
        0xC0 => 0x29,                        // `
        0xDB => 0x1A,                        // [
        0xDC => 0x2B,                        // \
        0xDD => 0x1B,                        // ]
        0xDE => 0x28,                        // '
        _ => 0,
    }
}

/// Tombol yang scancode-nya berawalan E0.
fn is_extended(vk: u8) -> bool {
    matches!(vk, 0x21..=0x28 | 0x2D | 0x2E | 0x5B | 0x5C | 0x5D | 0x6F | 0xA3 | 0xA5)
}

fn is_modifier(vk: u8) -> bool {
    matches!(vk, 0x10..=0x12 | 0x14 | 0x5B | 0x5C | 0x90 | 0x91 | 0xA0..=0xA5)
}

/// Posisi absolut dari client -> koordinat layar utama ternormalisasi, memperhitungkan bilah hitam di stream.
fn absolute_position(x: i16, y: i16, width: i16, height: i16, geometry: Geometry) -> Option<(u16, u16)> {
    let (desktop, stream) = (geometry.desktop, geometry.stream);
    if width <= 0 || height <= 0 || desktop.0 == 0 || desktop.1 == 0 || stream.0 == 0 || stream.1 == 0 {
        return None;
    }
    // Posisi di gambar stream, lalu dibuang bilah hitamnya (kebalikan `video::fit`).
    let in_stream = (f32::from(x.max(0)) / f32::from(width) * stream.0 as f32, f32::from(y.max(0)) / f32::from(height) * stream.1 as f32);
    let (left, top, shown_width, shown_height) = crate::video::fit(desktop, stream);
    let unit = (((in_stream.0 - left) / shown_width).clamp(0.0, 1.0), ((in_stream.1 - top) / shown_height).clamp(0.0, 1.0));
    Some(((unit.0 * 65535.0).round() as u16, (unit.1 * 65535.0).round() as u16))
}

/// Mengubah kejadian input jadi tindakan Windows, sambil mengingat apa yang sedang ditekan supaya bisa dilepas
/// semua saat sesi berakhir (tombol yang "nyangkut" adalah kegagalan yang paling terasa).
pub struct Translator {
    keys_down: [bool; 256],
    buttons_down: [bool; 6],
    pads: [bool; MAX_PADS],
    repeat: Option<(Action, Instant)>,
}

impl Default for Translator {
    fn default() -> Self {
        Translator { keys_down: [false; 256], buttons_down: [false; 6], pads: [false; MAX_PADS], repeat: None }
    }
}

impl Translator {
    pub fn translate(&mut self, event: &InputEvent, geometry: Geometry, now: Instant) -> Vec<Action> {
        match event {
            InputEvent::Key { down, key, flags, .. } => {
                let scan = if flags & KEY_FLAG_NON_NORMALIZED == 0 { us_scancode(*key) } else { 0 };
                let action = Action::Key { vk: *key, scan, extended: is_extended(*key), up: !down };
                self.keys_down[usize::from(*key)] = *down;
                if *down && !is_modifier(*key) {
                    self.repeat = Some((action, now + KEY_REPEAT_DELAY));
                } else if matches!(self.repeat, Some((Action::Key { vk, .. }, _)) if vk == *key) {
                    self.repeat = None;
                }
                vec![action]
            }
            InputEvent::Text(text) => text.encode_utf16().flat_map(|unit| [Action::Unicode { unit, up: false }, Action::Unicode { unit, up: true }]).collect(),
            InputEvent::MouseMoveRelative { dx, dy } => vec![Action::MouseMove { dx: i32::from(*dx), dy: i32::from(*dy) }],
            InputEvent::MouseMoveAbsolute { x, y, width, height } => absolute_position(*x, *y, *width, *height, geometry).map(|(x, y)| Action::MouseTo { x, y }).into_iter().collect(),
            InputEvent::MouseButton { down, button } if (1..=5).contains(button) => {
                self.buttons_down[usize::from(*button)] = *down;
                vec![Action::MouseButton { button: *button, up: !down }]
            }
            InputEvent::MouseButton { .. } => Vec::new(),
            InputEvent::Scroll(amount) => vec![Action::Wheel { amount: i32::from(*amount), horizontal: false }],
            InputEvent::HorizontalScroll(amount) => vec![Action::Wheel { amount: i32::from(*amount), horizontal: true }],
            InputEvent::Gamepad(pad) => self.gamepad(pad),
            InputEvent::Other(_) => Vec::new(),
        }
    }

    fn gamepad(&mut self, pad: &Gamepad) -> Vec<Action> {
        let mut actions = Vec::new();
        // Gamepad yang tidak lagi ada di mask client dicabut.
        for index in 0..MAX_PADS {
            if self.pads[index] && pad.active_mask & (1 << index) == 0 {
                self.pads[index] = false;
                actions.push(Action::Pad { index: index as u8, state: None });
            }
        }
        let index = usize::from(pad.index);
        if index < MAX_PADS && pad.active_mask & (1 << index) != 0 {
            self.pads[index] = true;
            let state = PadState { buttons: pad.buttons as u16, left_trigger: pad.left_trigger, right_trigger: pad.right_trigger, left_stick: pad.left_stick, right_stick: pad.right_stick };
            actions.push(Action::Pad { index: pad.index, state: Some(state) });
        }
        actions
    }

    /// Ulangan tombol yang sedang ditahan, bila sudah waktunya.
    pub fn repeat(&mut self, now: Instant) -> Option<Action> {
        let (action, due) = self.repeat.as_mut()?;
        if now < *due {
            return None;
        }
        *due = now + KEY_REPEAT_INTERVAL;
        Some(*action)
    }

    /// Lepas semua tombol keyboard dan mouse yang masih ditekan dan cabut semua gamepad.
    pub fn release_all(&mut self) -> Vec<Action> {
        let mut actions = Vec::new();
        for vk in 0..=255u8 {
            if std::mem::take(&mut self.keys_down[usize::from(vk)]) {
                actions.push(Action::Key { vk, scan: us_scancode(vk), extended: is_extended(vk), up: true });
            }
        }
        for button in 1..=5u8 {
            if std::mem::take(&mut self.buttons_down[usize::from(button)]) {
                actions.push(Action::MouseButton { button, up: true });
            }
        }
        for index in 0..MAX_PADS {
            if std::mem::take(&mut self.pads[index]) {
                actions.push(Action::Pad { index: index as u8, state: None });
            }
        }
        self.repeat = None;
        actions
    }
}

#[cfg(windows)]
mod windows_injector {
    use std::sync::Arc;

    use vigem_client::{Client, TargetId, XButtons, XGamepad, Xbox360Wired};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, KEYEVENTF_UNICODE, MOUSEEVENTF_ABSOLUTE,
        MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL,
        MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT, MOUSE_EVENT_FLAGS, VIRTUAL_KEY,
    };

    use super::{Action, MAX_PADS};

    /// Menjalankan `Action` di desktop tempat proses ini berjalan.
    #[derive(Default)]
    pub struct Injector {
        vigem: Option<Arc<Client>>,
        pads: [Option<Xbox360Wired<Arc<Client>>>; MAX_PADS],
        /// Getar yang diminta game ke gamepad virtual: `(indeks, motor besar, motor kecil)` 0..=255.
        rumble: Option<std::sync::mpsc::Sender<(u8, u8, u8)>>,
    }

    fn send(input: INPUT) -> Result<(), String> {
        // SAFETY: satu struct INPUT yang terisi lengkap.
        let inject = || unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) } == 1;
        if inject() {
            return Ok(());
        }
        // Mungkin desktop berganti (layar kunci, UAC): ikut pindah (hanya berhasil sebagai SYSTEM) dan coba sekali lagi.
        crate::supervise::sync_thread_desktop();
        if inject() {
            Ok(())
        } else {
            Err("SendInput ditolak (desktop terkunci, UAC, atau jendela berhak admin)".into())
        }
    }

    fn keyboard(vk: u16, scan: u16, flags: KEYBD_EVENT_FLAGS) -> Result<(), String> {
        send(INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(vk), wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } } })
    }

    fn mouse(dx: i32, dy: i32, data: i32, flags: MOUSE_EVENT_FLAGS) -> Result<(), String> {
        send(INPUT { r#type: INPUT_MOUSE, Anonymous: INPUT_0 { mi: MOUSEINPUT { dx, dy, mouseData: data as u32, dwFlags: flags, time: 0, dwExtraInfo: 0 } } })
    }

    impl Injector {
        /// Ke mana getar gamepad virtual dikirim.
        pub fn set_rumble_sink(&mut self, tx: std::sync::mpsc::Sender<(u8, u8, u8)>) {
            self.rumble = Some(tx);
        }

        pub fn apply(&mut self, action: &Action) -> Result<(), String> {
            match *action {
                Action::Key { vk, scan, extended, up } => {
                    let mut flags = KEYBD_EVENT_FLAGS(0);
                    if scan != 0 {
                        flags |= KEYEVENTF_SCANCODE;
                    }
                    if extended {
                        flags |= KEYEVENTF_EXTENDEDKEY;
                    }
                    if up {
                        flags |= KEYEVENTF_KEYUP;
                    }
                    keyboard(if scan != 0 { 0 } else { u16::from(vk) }, scan, flags)
                }
                Action::Unicode { unit, up } => keyboard(0, unit, if up { KEYEVENTF_UNICODE | KEYEVENTF_KEYUP } else { KEYEVENTF_UNICODE }),
                Action::MouseMove { dx, dy } => mouse(dx, dy, 0, MOUSEEVENTF_MOVE),
                Action::MouseTo { x, y } => mouse(i32::from(x), i32::from(y), 0, MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE),
                Action::MouseButton { button, up } => {
                    let (flags, data) = match (button, up) {
                        (1, false) => (MOUSEEVENTF_LEFTDOWN, 0),
                        (1, true) => (MOUSEEVENTF_LEFTUP, 0),
                        (2, false) => (MOUSEEVENTF_MIDDLEDOWN, 0),
                        (2, true) => (MOUSEEVENTF_MIDDLEUP, 0),
                        (3, false) => (MOUSEEVENTF_RIGHTDOWN, 0),
                        (3, true) => (MOUSEEVENTF_RIGHTUP, 0),
                        // XBUTTON1 = 1, XBUTTON2 = 2.
                        (4 | 5, false) => (MOUSEEVENTF_XDOWN, i32::from(button) - 3),
                        (4 | 5, true) => (MOUSEEVENTF_XUP, i32::from(button) - 3),
                        _ => return Ok(()),
                    };
                    mouse(0, 0, data, flags)
                }
                Action::Wheel { amount, horizontal } => mouse(0, 0, amount, if horizontal { MOUSEEVENTF_HWHEEL } else { MOUSEEVENTF_WHEEL }),
                Action::Pad { index, state } => self.pad(usize::from(index), state),
            }
        }

        fn pad(&mut self, index: usize, state: Option<super::PadState>) -> Result<(), String> {
            if index >= MAX_PADS {
                return Ok(());
            }
            let Some(state) = state else {
                // Men-drop target mencabut gamepad virtualnya.
                self.pads[index] = None;
                return Ok(());
            };
            let fresh = self.pads[index].is_none();
            if fresh {
                let client = match &self.vigem {
                    Some(client) => client.clone(),
                    None => {
                        let client = Arc::new(Client::connect().map_err(|e| format!("ViGEmBus tidak tersedia ({e:?}); gamepad diabaikan"))?);
                        self.vigem = Some(client.clone());
                        client
                    }
                };
                let mut target = Xbox360Wired::new(client, TargetId::XBOX360_WIRED);
                target.plugin().map_err(|e| format!("gamepad virtual gagal dipasang: {e:?}"))?;
                target.wait_ready().map_err(|e| format!("gamepad virtual tidak siap: {e:?}"))?;
                // Getar dari game diteruskan ke client (thread berhenti sendiri saat gamepad virtual dicabut).
                if let (Some(tx), Ok(request)) = (self.rumble.clone(), target.request_notification()) {
                    let pad = index as u8;
                    let _ = request.spawn_thread(move |_, n| {
                        let _ = tx.send((pad, n.large_motor, n.small_motor));
                    });
                }
                self.pads[index] = Some(target);
            }
            let report = XGamepad {
                buttons: XButtons(state.buttons),
                left_trigger: state.left_trigger,
                right_trigger: state.right_trigger,
                thumb_lx: state.left_stick.0,
                thumb_ly: state.left_stick.1,
                thumb_rx: state.right_stick.0,
                thumb_ry: state.right_stick.1,
            };
            let target = self.pads[index].as_mut().expect("baru saja diisi");
            // ERROR_NO_MORE_ITEMS (259): belum ada permintaan baca dari driver XInput, laporan DIBUANG (terbukti di PC uji:
            // keadaan pertama hilang tepat setelah gamepad dipasang). Gamepad baru: ulangi sebentar sampai diterima, supaya
            // tombol yang sudah ditahan saat tersambung tidak hilang. Gamepad lama: abaikan, laporan berikutnya menyusul.
            for attempt in 0.. {
                match target.update(&report) {
                    Ok(()) => return Ok(()),
                    Err(vigem_client::Error::WinError(259)) if fresh && attempt < 40 => std::thread::sleep(std::time::Duration::from_millis(10)),
                    Err(vigem_client::Error::WinError(259)) => return Ok(()),
                    Err(e) => return Err(format!("gamepad virtual gagal diperbarui: {e:?}")),
                }
            }
            unreachable!("perulangan di atas selalu kembali")
        }

        /// Indeks XInput gamepad virtual ke-`index` (untuk diagnostik dan tes).
        pub fn xinput_index(&mut self, index: usize) -> Option<u32> {
            self.pads.get_mut(index)?.as_mut()?.get_user_index().ok()
        }
    }
}

#[cfg(windows)]
pub use windows_injector::Injector;

/// Di luar Windows tidak ada yang bisa disuntikkan; tindakan ditolak dengan pesan jelas.
#[cfg(not(windows))]
#[derive(Default)]
pub struct Injector;

#[cfg(not(windows))]
impl Injector {
    pub fn set_rumble_sink(&mut self, _tx: std::sync::mpsc::Sender<(u8, u8, u8)>) {}

    pub fn apply(&mut self, _action: &Action) -> Result<(), String> {
        Err("input hanya ada untuk Windows".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cloudpc_stream_proto::input as wire;

    const SAME: Geometry = Geometry { desktop: (1920, 1080), stream: (1920, 1080) };

    fn run(translator: &mut Translator, packet: &[u8]) -> Vec<Action> {
        translator.translate(&wire::decode(packet).unwrap(), SAME, Instant::now())
    }

    #[test]
    fn keys_become_us_scancodes_with_extended_flag_where_needed() {
        let mut t = Translator::default();
        assert_eq!(run(&mut t, &wire::keyboard(true, 0x8041, 0, 0)), [Action::Key { vk: 0x41, scan: 0x1E, extended: false, up: false }], "A");
        assert_eq!(run(&mut t, &wire::keyboard(false, 0x8041, 0, 0)), [Action::Key { vk: 0x41, scan: 0x1E, extended: false, up: true }]);
        assert_eq!(run(&mut t, &wire::keyboard(true, 0x8025, 0, 0)), [Action::Key { vk: 0x25, scan: 0x4B, extended: true, up: false }], "panah kiri");
        assert_eq!(run(&mut t, &wire::keyboard(true, 0x80A5, 0, 0)), [Action::Key { vk: 0xA5, scan: 0x38, extended: true, up: false }], "Alt kanan");
        // Kode yang tidak dinormalkan client, atau tanpa scancode, dikirim sebagai kode virtual.
        assert_eq!(run(&mut t, &wire::keyboard(true, 0x8041, 0, wire::KEY_FLAG_NON_NORMALIZED)), [Action::Key { vk: 0x41, scan: 0, extended: false, up: false }]);
        assert_eq!(run(&mut t, &wire::keyboard(true, 0x80B3, 0, 0)), [Action::Key { vk: 0xB3, scan: 0, extended: false, up: false }], "tombol media");
    }

    #[test]
    fn scancode_table_covers_letters_digits_and_function_keys() {
        assert_eq!((us_scancode(b'Q'), us_scancode(b'W'), us_scancode(b'Z'), us_scancode(b'M')), (0x10, 0x11, 0x2C, 0x32));
        assert_eq!((us_scancode(b'1'), us_scancode(b'9'), us_scancode(b'0')), (0x02, 0x0A, 0x0B));
        assert_eq!((us_scancode(0x70), us_scancode(0x79), us_scancode(0x7A), us_scancode(0x7B)), (0x3B, 0x44, 0x57, 0x58), "F1, F10, F11, F12");
        assert_eq!((us_scancode(0x61), us_scancode(0x64), us_scancode(0x67), us_scancode(0x69)), (0x4F, 0x4B, 0x47, 0x49), "numpad 1, 4, 7, 9");
        assert_eq!((us_scancode(0x20), us_scancode(0x1B), us_scancode(0x0D)), (0x39, 0x01, 0x1C));
        // Tiap huruf punya scancode berbeda.
        let mut letters: Vec<u16> = (b'A'..=b'Z').map(us_scancode).collect();
        letters.sort_unstable();
        letters.dedup();
        assert_eq!(letters.len(), 26);
    }

    #[test]
    fn mouse_events_translate_directly() {
        let mut t = Translator::default();
        assert_eq!(run(&mut t, &wire::mouse_move_rel(-5, 12)), [Action::MouseMove { dx: -5, dy: 12 }]);
        assert_eq!(run(&mut t, &wire::mouse_button(true, 3)), [Action::MouseButton { button: 3, up: false }]);
        assert_eq!(run(&mut t, &wire::mouse_button(true, 9)), [], "tombol tidak dikenal diabaikan");
        assert_eq!(run(&mut t, &wire::scroll(-120)), [Action::Wheel { amount: -120, horizontal: false }]);
        assert_eq!(run(&mut t, &wire::horizontal_scroll(120)), [Action::Wheel { amount: 120, horizontal: true }]);
        assert_eq!(run(&mut t, &wire::text("aé")), [
            Action::Unicode { unit: 0x61, up: false },
            Action::Unicode { unit: 0x61, up: true },
            Action::Unicode { unit: 0xE9, up: false },
            Action::Unicode { unit: 0xE9, up: true }
        ]);
    }

    #[test]
    fn absolute_mouse_maps_through_letterbox() {
        let at = |x, y, reference: (i16, i16), geometry| Translator::default().translate(&wire::decode(&wire::mouse_move_abs(x, y, reference)).unwrap(), geometry, Instant::now());
        // Tanpa bilah: pojok dan tengah.
        assert_eq!(at(0, 0, (1280, 720), SAME), [Action::MouseTo { x: 0, y: 0 }]);
        assert_eq!(at(1279, 719, (1280, 720), SAME), [Action::MouseTo { x: 65535, y: 65535 }]);
        // Layar 16:9 di stream 4:3 (bilah atas-bawah 150 px dari 1200): tengah tetap tengah, tepi atas gambar = y 0.
        let boxed = Geometry { desktop: (1920, 1080), stream: (1600, 1200) };
        assert_eq!(at(800, 600, (1601, 1201), boxed), [Action::MouseTo { x: 32768, y: 32768 }]);
        assert_eq!(at(0, 150, (1601, 1201), boxed), [Action::MouseTo { x: 0, y: 0 }]);
        // Di dalam bilah hitam: ditahan di tepi layar.
        assert_eq!(at(0, 10, (1601, 1201), boxed), [Action::MouseTo { x: 0, y: 0 }]);
        // Bidang acuan nol dari client yang rusak: diabaikan.
        assert_eq!(at(5, 5, (1, 1), SAME), []);
    }

    #[test]
    fn gamepads_follow_the_active_mask() {
        let mut t = Translator::default();
        let pad = |index, mask, buttons| wire::gamepad(&wire::Gamepad { index, active_mask: mask, buttons, left_trigger: 255, right_trigger: 0, left_stick: (100, -100), right_stick: (0, 0) });
        let expected = PadState { buttons: 0x1000, left_trigger: 255, right_trigger: 0, left_stick: (100, -100), right_stick: (0, 0) };
        assert_eq!(run(&mut t, &pad(0, 0b01, 0x1000)), [Action::Pad { index: 0, state: Some(expected) }]);
        assert_eq!(run(&mut t, &pad(1, 0b11, 0x1000)), [Action::Pad { index: 1, state: Some(expected) }]);
        // Gamepad 0 hilang dari mask: dicabut, gamepad 1 tetap diperbarui.
        assert_eq!(run(&mut t, &pad(1, 0b10, 0x1000)), [Action::Pad { index: 0, state: None }, Action::Pad { index: 1, state: Some(expected) }]);
        // Di luar empat gamepad XInput: diabaikan.
        assert_eq!(run(&mut t, &pad(5, 0b10_0010, 0)), []);
    }

    #[test]
    fn held_key_repeats_after_delay_until_released() {
        let mut t = Translator::default();
        let start = Instant::now();
        let down = t.translate(&wire::decode(&wire::keyboard(true, 0x8008, 0, 0)).unwrap(), SAME, start)[0];
        assert_eq!(t.repeat(start + Duration::from_millis(400)), None, "belum lewat jeda awal");
        assert_eq!(t.repeat(start + Duration::from_millis(510)), Some(down));
        assert_eq!(t.repeat(start + Duration::from_millis(520)), None, "belum lewat selang ulang");
        assert_eq!(t.repeat(start + Duration::from_millis(550)), Some(down));
        t.translate(&wire::decode(&wire::keyboard(false, 0x8008, 0, 0)).unwrap(), SAME, start);
        assert_eq!(t.repeat(start + Duration::from_secs(5)), None, "dilepas: tidak berulang lagi");
        // Tombol pengubah (Shift) tidak pernah berulang.
        t.translate(&wire::decode(&wire::keyboard(true, 0x8010, 0, 0)).unwrap(), SAME, start);
        assert_eq!(t.repeat(start + Duration::from_secs(5)), None);
    }

    #[test]
    fn release_all_lifts_everything_that_is_still_held() {
        let mut t = Translator::default();
        run(&mut t, &wire::keyboard(true, 0x8057, 0, 0));
        run(&mut t, &wire::keyboard(true, 0x8010, 0, 0));
        run(&mut t, &wire::keyboard(true, 0x8041, 0, 0));
        run(&mut t, &wire::keyboard(false, 0x8041, 0, 0));
        run(&mut t, &wire::mouse_button(true, 1));
        run(&mut t, &wire::gamepad(&wire::Gamepad { index: 0, active_mask: 1, ..Default::default() }));
        assert_eq!(t.release_all(), [
            Action::Key { vk: 0x10, scan: 0x2A, extended: false, up: true },
            Action::Key { vk: 0x57, scan: 0x11, extended: false, up: true },
            Action::MouseButton { button: 1, up: true },
            Action::Pad { index: 0, state: None }
        ]);
        assert_eq!(t.release_all(), [], "kedua kalinya tidak ada lagi yang perlu dilepas");
        assert_eq!(t.repeat(Instant::now() + Duration::from_secs(5)), None);
    }
}
