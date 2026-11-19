//! Fase C4. Tangkap keyboard/mouse/gamepad dari jendela stream lalu kirim lewat kanal kontrol.
//!
//! Rujukan: `moonlight-common-c/src/InputStream.c`, `moonlight-qt/app/streaming/input/*`; paket di
//! `cloudpc_stream_proto::input`. Perilaku mengikuti Moonlight:
//! - Keyboard lewat hook keyboard tingkat rendah selama jendela stream di depan, supaya Alt+Tab, tombol Windows, dll.
//!   bisa diteruskan ke host saat layar penuh (`--capture-system-keys fullscreen`). Ulangan tombol tidak dikirim
//!   (host mengulang sendiri).
//! - Mouse relatif (mode game, bawaan aplikasi: `--no-absolute-mouse`): Raw Input, kursor lokal disembunyikan dan
//!   dikurung di jendela. Mouse absolut: posisi di dalam gambar.
//! - Ctrl+Alt+Shift + Q keluar, X layar penuh, Z lepas/tangkap mouse dan keyboard, M ganti mode mouse, D kecilkan,
//!   V tempel teks clipboard, K tangkap tombol sistem. Tombol yang sedang ditekan dilepas di host setelah pintasan
//!   dan saat jendela kehilangan fokus.
//! - Gamepad lewat XInput (sampai 4).

use cloudpc_stream_proto::input as wire;

/// Kanal ENet per jenis input (`CTRL_CHANNEL_*` di moonlight-common-c).
pub mod channel {
    pub const KEYBOARD: u8 = 0x02;
    pub const MOUSE: u8 = 0x03;
    pub const UTF8: u8 = 0x06;
    pub const GAMEPAD_BASE: u8 = 0x10;
}

/// Satu paket input siap kirim (`INPUT_DATA` di kanal kontrol).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputPacket {
    pub channel: u8,
    pub data: Vec<u8>,
}

pub mod modifier {
    pub const SHIFT: u8 = 0x01;
    pub const CTRL: u8 = 0x02;
    pub const ALT: u8 = 0x04;
    pub const META: u8 = 0x08;
}

/// Pintasan Ctrl+Alt+Shift+huruf.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    Quit,
    ToggleFullscreen,
    ToggleCapture,
    ToggleMouseMode,
    Minimize,
    Paste,
    ToggleSystemKeys,
    ToggleStats,
}

const VK_SHIFT: u16 = 0x10;
const VK_CONTROL: u16 = 0x11;
const VK_MENU: u16 = 0x12;
const VK_LWIN: u16 = 0x5B;
const VK_RWIN: u16 = 0x5C;
const VK_LSHIFT: u16 = 0xA0;
const VK_RSHIFT: u16 = 0xA1;
const VK_LCONTROL: u16 = 0xA2;
const VK_RCONTROL: u16 = 0xA3;
const VK_LMENU: u16 = 0xA4;
const VK_RMENU: u16 = 0xA5;
const VK_TAB: u16 = 0x09;
const VK_ESCAPE: u16 = 0x1B;

/// Tombol yang sedang ditekan (kode VK), urut tekan.
#[derive(Debug, Default, Clone)]
pub struct Keys {
    held: Vec<u16>,
}

impl Keys {
    fn any(&self, codes: &[u16]) -> bool {
        self.held.iter().any(|k| codes.contains(k))
    }

    pub fn modifiers(&self) -> u8 {
        let mut m = 0;
        if self.any(&[VK_SHIFT, VK_LSHIFT, VK_RSHIFT]) {
            m |= modifier::SHIFT;
        }
        if self.any(&[VK_CONTROL, VK_LCONTROL, VK_RCONTROL]) {
            m |= modifier::CTRL;
        }
        if self.any(&[VK_MENU, VK_LMENU, VK_RMENU]) {
            m |= modifier::ALT;
        }
        if self.any(&[VK_LWIN, VK_RWIN]) {
            m |= modifier::META;
        }
        m
    }

    /// Catat tekan/lepas. `false` = ulangan otomatis (tombol sudah ditekan) atau lepas tombol yang tidak tercatat.
    pub fn update(&mut self, vk: u16, down: bool) -> bool {
        let position = self.held.iter().position(|&k| k == vk);
        match (down, position) {
            (true, Some(_)) | (false, None) => false,
            (true, None) => {
                self.held.push(vk);
                true
            }
            (false, Some(i)) => {
                self.held.remove(i);
                true
            }
        }
    }

    /// Lepas semua (paket tombol naik untuk tiap tombol yang masih ditekan di host).
    pub fn release_all(&mut self) -> Vec<InputPacket> {
        let mut out = Vec::new();
        while let Some(vk) = self.held.pop() {
            out.push(key_packet(vk, false, self.modifiers()));
        }
        out
    }

    /// Pintasan bila `vk` ditekan bersama Ctrl+Alt+Shift.
    pub fn shortcut(&self, vk: u16) -> Option<Shortcut> {
        if self.modifiers() & (modifier::SHIFT | modifier::CTRL | modifier::ALT) != modifier::SHIFT | modifier::CTRL | modifier::ALT {
            return None;
        }
        Some(match vk {
            0x51 => Shortcut::Quit,             // Q
            0x58 => Shortcut::ToggleFullscreen, // X
            0x5A => Shortcut::ToggleCapture,    // Z
            0x4D => Shortcut::ToggleMouseMode,  // M
            0x44 => Shortcut::Minimize,         // D
            0x56 => Shortcut::Paste,            // V
            0x4B => Shortcut::ToggleSystemKeys, // K
            0x53 => Shortcut::ToggleStats,      // S
            _ => return None,
        })
    }

    /// Kombinasi yang dibiarkan ke Windows lokal bila tombol sistem tidak ditangkap: tombol Windows, Alt+Tab,
    /// Alt+Esc, Ctrl+Esc.
    pub fn is_system_combo(&self, vk: u16) -> bool {
        let m = self.modifiers();
        matches!(vk, VK_LWIN | VK_RWIN) || (vk == VK_TAB && m & modifier::ALT != 0) || (vk == VK_ESCAPE && m & (modifier::ALT | modifier::CTRL) != 0)
    }
}

/// Shift, Ctrl, Alt (kiri/kanan/umum).
pub fn is_modifier(vk: u16) -> bool {
    matches!(vk, VK_SHIFT | VK_CONTROL | VK_MENU | VK_LSHIFT..=VK_RMENU)
}

/// Scancode SDL (kode HID USB, posisi fisik tombol) -> kode tombol virtual Windows yang dikirim ke host, seperti tabel
/// di `moonlight-qt/app/streaming/input/keyboard.cpp`. `None` = tombol yang tidak diteruskan.
pub fn vk_from_sdl_scancode(scancode: u32) -> Option<u16> {
    Some(match scancode {
        4..=29 => 0x41 + (scancode - 4) as u16,  // A..Z
        30..=38 => 0x31 + (scancode - 30) as u16, // 1..9
        39 => 0x30,                                // 0
        40 => 0x0D,                                // Enter
        41 => 0x1B,                                // Esc
        42 => 0x08,                                // Backspace
        43 => 0x09,                                // Tab
        44 => 0x20,                                // Spasi
        45 => 0xBD,                                // -
        46 => 0xBB,                                // =
        47 => 0xDB,                                // [
        48 => 0xDD,                                // ]
        49 | 50 => 0xDC,                           // \ (dan # non-US)
        51 => 0xBA,                                // ;
        52 => 0xDE,                                // '
        53 => 0xC0,                                // `
        54 => 0xBC,                                // ,
        55 => 0xBE,                                // .
        56 => 0xBF,                                // /
        57 => 0x14,                                // Caps Lock
        58..=69 => 0x70 + (scancode - 58) as u16,  // F1..F12
        70 => 0x2C,                                // Print Screen
        71 => 0x91,                                // Scroll Lock
        72 => 0x13,                                // Pause
        73 => 0x2D,                                // Insert
        74 => 0x24,                                // Home
        75 => 0x21,                                // Page Up
        76 => 0x2E,                                // Delete
        77 => 0x23,                                // End
        78 => 0x22,                                // Page Down
        79 => 0x27,                                // Kanan
        80 => 0x25,                                // Kiri
        81 => 0x28,                                // Bawah
        82 => 0x26,                                // Atas
        83 => 0x90,                                // Num Lock
        84 => 0x6F,                                // Numpad /
        85 => 0x6A,                                // Numpad *
        86 => 0x6D,                                // Numpad -
        87 => 0x6B,                                // Numpad +
        88 => 0x0D,                                // Numpad Enter
        89..=97 => 0x61 + (scancode - 89) as u16,  // Numpad 1..9
        98 => 0x60,                                // Numpad 0
        99 => 0x6E,                                // Numpad .
        100 => 0xE2,                               // \ di samping Shift kiri (ISO)
        101 => 0x5D,                               // Menu (Application)
        103 => 0x92,                               // Numpad = (OEM_NEC_EQUAL)
        104..=115 => 0x7C + (scancode - 104) as u16, // F13..F24
        127 => 0xAD,                               // Mute
        128 => 0xAF,                               // Volume naik
        129 => 0xAE,                               // Volume turun
        224 => VK_LCONTROL,
        225 => VK_LSHIFT,
        226 => VK_LMENU,
        227 => VK_LWIN,
        228 => VK_RCONTROL,
        229 => VK_RSHIFT,
        230 => VK_RMENU,
        231 => VK_RWIN,
        _ => return None,
    })
}

/// Paket tombol keyboard. Kode VK dikirim dengan bit 0x8000 seperti Moonlight.
pub fn key_packet(vk: u16, down: bool, modifiers: u8) -> InputPacket {
    InputPacket { channel: channel::KEYBOARD, data: wire::keyboard(down, 0x8000 | vk, modifiers, 0) }
}

/// Teks dipecah per paling banyak 32 byte tanpa memotong karakter.
pub fn text_packets(text: &str) -> Vec<InputPacket> {
    let mut out = Vec::new();
    let mut chunk = String::new();
    for c in text.chars() {
        if chunk.len() + c.len_utf8() > wire::MAX_TEXT_BYTES {
            out.push(InputPacket { channel: channel::UTF8, data: wire::text(&chunk) });
            chunk.clear();
        }
        chunk.push(c);
    }
    if !chunk.is_empty() {
        out.push(InputPacket { channel: channel::UTF8, data: wire::text(&chunk) });
    }
    out
}

/// Gerak mouse relatif; delta besar dipecah supaya muat i16.
pub fn relative_packets(mut dx: i32, mut dy: i32) -> Vec<InputPacket> {
    let mut out = Vec::new();
    while dx != 0 || dy != 0 {
        let (sx, sy) = (dx.clamp(-32767, 32767), dy.clamp(-32767, 32767));
        out.push(InputPacket { channel: channel::MOUSE, data: wire::mouse_move_rel(sx as i16, sy as i16) });
        dx -= sx;
        dy -= sy;
    }
    out
}

#[cfg(windows)]
pub mod win {
    //! Bagian Windows: dipanggil dari prosedur jendela dan loop thread video (`video::display`).

    use std::cell::RefCell;
    use std::sync::mpsc::Sender;

    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
    use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
    use windows::Win32::UI::Input::XboxController::{XInputGetState, XInputSetState, XINPUT_STATE, XINPUT_VIBRATION};
    use windows::Win32::UI::Input::{GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RIDEV_REMOVE, RID_INPUT, RIM_TYPEMOUSE};
    use windows::Win32::UI::WindowsAndMessaging::*;

    use super::*;

    /// Cara menangkap tombol sistem (`--capture-system-keys`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum SystemKeys {
        Never,
        Fullscreen,
        Always,
    }

    #[derive(Debug, Clone)]
    pub struct InputOptions {
        /// `false` = hanya menonton (`--no-input`): tidak ada yang ditangkap atau dikirim.
        pub enabled: bool,
        pub absolute_mouse: bool,
        pub system_keys: SystemKeys,
    }

    struct State {
        tx: Sender<InputPacket>,
        window: HWND,
        keys: Keys,
        /// Input diteruskan ke host (mouse ditangkap, keyboard lewat hook).
        captured: bool,
        absolute: bool,
        system_keys: SystemKeys,
        system_keys_forced: Option<bool>,
        fullscreen: bool,
        dx: i32,
        dy: i32,
        /// Kotak gambar di dalam area klien dan ukuran stream (untuk mouse absolut).
        picture: RECT,
        actions: Vec<Shortcut>,
        hook: Option<HHOOK>,
        /// Stik lewat SDL3 (PlayStation, Switch, Xbox); `None` = XInput saja.
        sdl: Option<crate::gamepad::Pads>,
        notices: Vec<String>,
        /// Jendela di depan pada putaran sebelumnya: tangkap saat mendapat fokus, lepas saat kehilangan fokus.
        was_foreground: bool,
        /// Input pernah ditangkap. Hanya pembukaan pertama jendela yang menangkap otomatis; sesudah itu (Alt+Tab, aplikasi
        /// lain ditutup lalu Windows mengaktifkan jendela stream) harus klik dulu, seperti Moonlight. Temuan sesi #86.
        ever_captured: bool,
        pads: [Pad; 4],
        pad_poll: u32,
    }

    #[derive(Default, Clone, Copy)]
    struct Pad {
        connected: bool,
        packet: u32,
    }

    thread_local! {
        static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    }

    /// Jalankan `f` dengan keadaan input. Panggilan bersarang (pesan jendela yang dikirim Windows selagi keadaan sedang
    /// dipakai, mis. dari PeekMessage di dalam SDL) dilewati, bukan panik: panik di prosedur jendela = program berhenti
    /// (sesi #87, 19 Nov 2026: "RefCell already borrowed" saat SDL_UpdateGamepads memompa pesan).
    fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
        STATE.with(|s| s.try_borrow_mut().ok()?.as_mut().map(f))
    }

    fn send(state: &State, packets: impl IntoIterator<Item = InputPacket>) {
        for packet in packets {
            let _ = state.tx.send(packet);
        }
    }

    /// Mulai menangkap input untuk `window` (di thread jendela).
    pub fn install(window: HWND, tx: Sender<InputPacket>, options: &InputOptions) {
        if !options.enabled {
            return;
        }
        unsafe {
            let device = RAWINPUTDEVICE { usUsagePage: 0x01, usUsage: 0x02, dwFlags: Default::default(), hwndTarget: window };
            let _ = RegisterRawInputDevices(&[device], std::mem::size_of::<RAWINPUTDEVICE>() as u32);
        }
        let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), None, 0).ok() };
        STATE.with(|s| {
            *s.borrow_mut() = Some(State {
                tx,
                window,
                keys: Keys::default(),
                captured: false,
                absolute: options.absolute_mouse,
                system_keys: options.system_keys,
                system_keys_forced: None,
                fullscreen: false,
                dx: 0,
                dy: 0,
                picture: RECT::default(),
                actions: Vec::new(),
                hook,
                sdl: crate::gamepad::Pads::open(),
                notices: Vec::new(),
                was_foreground: false,
                ever_captured: false,
                pads: [Pad::default(); 4],
                pad_poll: 0,
            })
        });
    }

    /// Lepas semua tombol di host, buang hook, kembalikan kursor.
    pub fn uninstall() {
        STATE.with(|s| {
            if let Some(mut state) = s.borrow_mut().take() {
                let packets = state.keys.release_all();
                send(&state, packets);
                // Getar yang sedang jalan jangan tertinggal setelah sesi selesai (stik SDL berhenti saat `Pads` di-drop).
                if state.sdl.is_none() {
                    for pad in 0..4u32 {
                        let stop = XINPUT_VIBRATION::default();
                        unsafe {
                            let _ = XInputSetState(pad, &stop);
                        }
                    }
                }
                unsafe {
                    if let Some(hook) = state.hook.take() {
                        let _ = UnhookWindowsHookEx(hook);
                    }
                    let device = RAWINPUTDEVICE { usUsagePage: 0x01, usUsage: 0x02, dwFlags: RIDEV_REMOVE, hwndTarget: HWND::default() };
                    let _ = RegisterRawInputDevices(&[device], std::mem::size_of::<RAWINPUTDEVICE>() as u32);
                    let _ = ClipCursor(None);
                }
            }
        });
    }

    /// Getar stik `pad` (motor kiri = frekuensi rendah, kanan = tinggi). Dipanggil di thread jendela (SDL ada di sana).
    pub fn rumble(pad: u16, low: u16, high: u16) {
        if with(|s| s.sdl.as_ref().map(|sdl| sdl.rumble(pad, low, high))).flatten().is_some() {
            return;
        }
        if pad < 4 {
            let vibration = XINPUT_VIBRATION { wLeftMotorSpeed: low, wRightMotorSpeed: high };
            unsafe {
                let _ = XInputSetState(u32::from(pad), &vibration);
            }
        }
    }

    pub fn set_fullscreen(on: bool) {
        with(|s| {
            s.fullscreen = on;
            confine(s);
        });
    }

    pub fn set_picture(rect: RECT) {
        with(|s| s.picture = rect);
    }

    /// Pintasan yang harus dijalankan jendela (layar penuh, keluar, kecilkan).
    pub fn take_actions() -> Vec<Shortcut> {
        with(|s| std::mem::take(&mut s.actions)).unwrap_or_default()
    }

    fn system_keys_captured(s: &State) -> bool {
        s.system_keys_forced.unwrap_or(match s.system_keys {
            SystemKeys::Never => false,
            SystemKeys::Fullscreen => s.fullscreen,
            SystemKeys::Always => true,
        })
    }

    /// Kurung kursor di area klien (mode relatif) atau lepas.
    fn confine(s: &State) {
        unsafe {
            if s.captured && !s.absolute {
                let mut rect = RECT::default();
                let _ = GetClientRect(s.window, &mut rect);
                let mut top_left = POINT { x: rect.left, y: rect.top };
                let mut bottom_right = POINT { x: rect.right, y: rect.bottom };
                let _ = ClientToScreen(s.window, &mut top_left);
                let _ = ClientToScreen(s.window, &mut bottom_right);
                let _ = ClipCursor(Some(&RECT { left: top_left.x, top: top_left.y, right: bottom_right.x, bottom: bottom_right.y }));
            } else {
                let _ = ClipCursor(None);
            }
        }
    }

    fn set_captured(s: &mut State, on: bool) {
        if s.captured == on {
            return;
        }
        if on {
            s.ever_captured = true;
        }
        s.captured = on;
        if !on {
            let packets = s.keys.release_all();
            send(s, packets);
            s.dx = 0;
            s.dy = 0;
        }
        confine(s);
    }

    /// Kirim gerak mouse relatif yang terkumpul dan keadaan gamepad yang berubah. Dipanggil tiap putaran thread video.
    pub fn flush() {
        with(|s| {
            // Ikuti fokus sebenarnya (WM_ACTIVATE bisa terjadi sebelum penangkap dipasang).
            let foreground = unsafe { GetForegroundWindow() } == s.window;
            if foreground != s.was_foreground {
                s.was_foreground = foreground;
                if !foreground {
                    set_captured(s, false);
                } else if !s.ever_captured {
                    set_captured(s, true);
                }
            }
            if s.dx != 0 || s.dy != 0 {
                let packets = relative_packets(s.dx, s.dy);
                send(s, packets);
                s.dx = 0;
                s.dy = 0;
            }
            if s.sdl.is_none() {
                poll_gamepads(s);
            }
        });
        // SDL3 memompa pesan jendela (PeekMessage) saat membaca stik, dan pesan itu masuk lagi ke prosedur jendela kita:
        // jadi stik SDL dibaca DI LUAR pinjaman keadaan, lalu hasilnya diterapkan.
        let Some(Some(mut sdl)) = with(|s| s.sdl.take()) else { return };
        let packets = sdl.poll();
        let mut arrived = std::mem::take(&mut sdl.arrived);
        with(move |s| {
            s.notices.append(&mut arrived);
            // Saat input tidak ditangkap, stik tetap dibaca (supaya keadaan terkini) tetapi tidak dikirim.
            if s.captured {
                send(s, packets);
            }
            s.sdl = Some(sdl);
        });
    }

    /// Stik yang baru tersambung sejak panggilan terakhir (untuk log).
    pub fn take_notices() -> Vec<String> {
        with(|s| std::mem::take(&mut s.notices)).unwrap_or_default()
    }

    /// Mesin stik yang dipakai (untuk log).
    pub fn gamepad_backend() -> &'static str {
        with(|s| if s.sdl.is_some() { "SDL3" } else { "XInput" }).unwrap_or("-")
    }

    /// Stik lewat XInput (SDL3 dibaca di `flush`).
    fn poll_gamepads(s: &mut State) {
        // XInputGetState pada slot kosong lambat: slot kosong hanya diperiksa tiap ±1 detik.
        s.pad_poll = s.pad_poll.wrapping_add(1);
        let scan_empty = s.pad_poll % 250 == 1;
        let mut changed = Vec::new();
        for index in 0..4u32 {
            let pad = s.pads[index as usize];
            if !pad.connected && !scan_empty {
                continue;
            }
            let mut state = XINPUT_STATE::default();
            let connected = unsafe { XInputGetState(index, &mut state) } == 0;
            let entry = &mut s.pads[index as usize];
            match (connected, entry.connected) {
                (true, _) if !entry.connected || state.dwPacketNumber != entry.packet => {
                    *entry = Pad { connected: true, packet: state.dwPacketNumber };
                    changed.push((index as u8, Some(state)));
                }
                (false, true) => {
                    *entry = Pad::default();
                    changed.push((index as u8, None));
                }
                _ => {}
            }
        }
        if changed.is_empty() || !s.captured {
            return;
        }
        let mask = s.pads.iter().enumerate().filter(|(_, p)| p.connected).fold(0u16, |m, (i, _)| m | 1 << i);
        for (index, state) in changed {
            let g = state.map(|st| st.Gamepad).unwrap_or_default();
            let pad = wire::Gamepad {
                index,
                active_mask: mask,
                buttons: u32::from(g.wButtons.0),
                left_trigger: g.bLeftTrigger,
                right_trigger: g.bRightTrigger,
                left_stick: (g.sThumbLX, g.sThumbLY),
                right_stick: (g.sThumbRX, g.sThumbRY),
            };
            send(s, [InputPacket { channel: channel::GAMEPAD_BASE + index, data: wire::gamepad(&pad) }]);
        }
    }

    fn clipboard_text() -> Option<String> {
        const CF_UNICODETEXT: u32 = 13;
        unsafe {
            OpenClipboard(None).ok()?;
            let text = GetClipboardData(CF_UNICODETEXT).ok().and_then(|handle| {
                let global = windows::Win32::Foundation::HGLOBAL(handle.0);
                let pointer = GlobalLock(global) as *const u16;
                if pointer.is_null() {
                    return None;
                }
                let mut len = 0;
                while *pointer.add(len) != 0 {
                    len += 1;
                }
                let text = String::from_utf16_lossy(std::slice::from_raw_parts(pointer, len));
                let _ = GlobalUnlock(global);
                Some(text)
            });
            let _ = CloseClipboard();
            text
        }
    }

    fn run_shortcut(s: &mut State, shortcut: Shortcut) {
        // Ctrl/Alt/Shift sudah ditekan di host: lepaskan dulu.
        let packets = s.keys.release_all();
        send(s, packets);
        match shortcut {
            Shortcut::ToggleCapture => {
                let on = !s.captured;
                set_captured(s, on);
            }
            Shortcut::ToggleMouseMode => {
                s.absolute = !s.absolute;
                confine(s);
            }
            Shortcut::ToggleSystemKeys => s.system_keys_forced = Some(!system_keys_captured(s)),
            Shortcut::Paste => {
                if let Some(text) = clipboard_text() {
                    send(s, text_packets(&text));
                }
            }
            Shortcut::Minimize => set_captured(s, false),
            Shortcut::Quit | Shortcut::ToggleFullscreen | Shortcut::ToggleStats => {}
        }
        s.actions.push(shortcut);
    }

    unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32 {
            let info = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            let vk = info.vkCode as u16;
            let down = matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
            let swallow = with(|s| {
                if !s.captured || GetForegroundWindow() != s.window {
                    return false;
                }
                if down {
                    if let Some(shortcut) = s.keys.shortcut(vk) {
                        run_shortcut(s, shortcut);
                        return true;
                    }
                    if !system_keys_captured(s) && s.keys.is_system_combo(vk) {
                        return false; // biarkan Windows lokal (Alt+Tab dll.)
                    }
                } else if !s.keys.held.contains(&vk) {
                    return false; // tombol ini ditekan sebelum stream menangkap keyboard
                }
                if s.keys.update(vk, down) {
                    let modifiers = s.keys.modifiers();
                    send(s, [key_packet(vk, down, modifiers)]);
                }
                // Tombol sistem tidak ditangkap (mode jendela): Ctrl/Alt/Shift juga diteruskan ke Windows lokal, seperti
                // Moonlight. Kalau ditelan, Windows tidak tahu Alt sedang ditekan dan Alt+Tab tidak berfungsi.
                system_keys_captured(s) || !is_modifier(vk)
            });
            if swallow == Some(true) {
                return LRESULT(1);
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    /// Pesan jendela yang berkaitan dengan input. `Some` = sudah ditangani.
    pub fn window_message(message: u32, _wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        with(|s| match message {
            WM_INPUT if s.captured => {
                let mut raw = RAWINPUT::default();
                let mut size = std::mem::size_of::<RAWINPUT>() as u32;
                let read = unsafe { GetRawInputData(HRAWINPUT(lparam.0 as *mut _), RID_INPUT, Some(&mut raw as *mut _ as *mut _), &mut size, std::mem::size_of::<RAWINPUTHEADER>() as u32) };
                if read == u32::MAX || raw.header.dwType != RIM_TYPEMOUSE.0 {
                    return None;
                }
                let mouse = unsafe { raw.data.mouse };
                // Mouse lokal bergerak relatif (mouse biasa); tablet/remote desktop memberi posisi absolut, diabaikan.
                if !s.absolute && mouse.usFlags.0 & 0x01 == 0 {
                    s.dx += mouse.lLastX;
                    s.dy += mouse.lLastY;
                }
                let (flags, data) = unsafe { (mouse.Anonymous.Anonymous.usButtonFlags, mouse.Anonymous.Anonymous.usButtonData as i16) };
                let buttons = [(0x0001, 0x0002, 1u8), (0x0004, 0x0008, 3), (0x0010, 0x0020, 2), (0x0040, 0x0080, 4), (0x0100, 0x0200, 5)];
                let mut packets = Vec::new();
                if !s.absolute && (s.dx != 0 || s.dy != 0) && flags != 0 {
                    // Tombol sesudah gerak: kirim gerak dulu supaya klik jatuh di tempat yang benar.
                    packets.extend(relative_packets(s.dx, s.dy));
                    s.dx = 0;
                    s.dy = 0;
                }
                for (down_flag, up_flag, button) in buttons {
                    if flags & down_flag != 0 {
                        packets.push(InputPacket { channel: channel::MOUSE, data: wire::mouse_button(true, button) });
                    }
                    if flags & up_flag != 0 {
                        packets.push(InputPacket { channel: channel::MOUSE, data: wire::mouse_button(false, button) });
                    }
                }
                if flags & 0x0400 != 0 {
                    packets.push(InputPacket { channel: channel::MOUSE, data: wire::scroll(data) });
                }
                if flags & 0x0800 != 0 {
                    packets.push(InputPacket { channel: channel::MOUSE, data: wire::horizontal_scroll(data) });
                }
                send(s, packets);
                None
            }
            WM_MOUSEMOVE if s.captured && s.absolute => {
                let (x, y) = ((lparam.0 & 0xFFFF) as i16 as i32, ((lparam.0 >> 16) & 0xFFFF) as i16 as i32);
                let p = s.picture;
                let (w, h) = (p.right - p.left, p.bottom - p.top);
                if w > 0 && h > 0 {
                    let (cx, cy) = ((x - p.left).clamp(0, w - 1), (y - p.top).clamp(0, h - 1));
                    send(s, [InputPacket { channel: channel::MOUSE, data: wire::mouse_move_abs(cx as i16, cy as i16, (w as i16, h as i16)) }]);
                }
                Some(LRESULT(0))
            }
            // Klik pertama ke jendela yang belum menangkap input = tangkap (tidak diteruskan).
            WM_LBUTTONDOWN | WM_RBUTTONDOWN if !s.captured => {
                set_captured(s, true);
                Some(LRESULT(0))
            }
            WM_SETCURSOR if s.captured && (lparam.0 & 0xFFFF) as u32 == HTCLIENT => {
                unsafe { SetCursor(None) };
                Some(LRESULT(1))
            }
            WM_SIZE | WM_MOVE => {
                confine(s);
                None
            }
            _ => None,
        })
        .flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifiers_and_autorepeat() {
        let mut keys = Keys::default();
        assert!(keys.update(VK_LCONTROL, true));
        assert!(!keys.update(VK_LCONTROL, true), "ulangan otomatis tidak dikirim");
        assert!(keys.update(VK_RSHIFT, true));
        assert_eq!(keys.modifiers(), modifier::CTRL | modifier::SHIFT);
        assert!(keys.update(VK_LCONTROL, false));
        assert!(!keys.update(0x41, false), "lepas tombol yang tidak ditekan diabaikan");
        assert_eq!(keys.modifiers(), modifier::SHIFT);
    }

    #[test]
    fn shortcuts_need_ctrl_alt_shift() {
        let mut keys = Keys::default();
        keys.update(VK_LCONTROL, true);
        keys.update(VK_LMENU, true);
        assert_eq!(keys.shortcut(0x51), None, "tanpa Shift");
        keys.update(VK_LSHIFT, true);
        assert_eq!(keys.shortcut(0x51), Some(Shortcut::Quit));
        assert_eq!(keys.shortcut(0x5A), Some(Shortcut::ToggleCapture));
        assert_eq!(keys.shortcut(0x41), None);
        let released = keys.release_all();
        assert_eq!(released.len(), 3);
        assert!(released.iter().all(|p| p.channel == channel::KEYBOARD && p.data[4] == 4), "semua paket tombol naik");
    }

    #[test]
    fn system_combos_are_recognised() {
        let mut keys = Keys::default();
        assert!(keys.is_system_combo(VK_LWIN));
        assert!(!keys.is_system_combo(VK_TAB));
        keys.update(VK_LMENU, true);
        assert!(keys.is_system_combo(VK_TAB), "Alt+Tab");
        assert!(keys.is_system_combo(VK_ESCAPE), "Alt+Esc");
    }

    #[test]
    fn modifiers_are_recognised_for_pass_through() {
        for vk in [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LSHIFT, VK_RSHIFT, VK_LCONTROL, VK_RCONTROL, VK_LMENU, VK_RMENU] {
            assert!(is_modifier(vk), "{vk:#x}");
        }
        for vk in [VK_TAB, VK_ESCAPE, VK_LWIN, 0x41, 0x73] {
            assert!(!is_modifier(vk), "{vk:#x}");
        }
    }

    #[test]
    fn sdl_scancodes_map_to_windows_keys() {
        let map = |s| vk_from_sdl_scancode(s);
        assert_eq!((map(4), map(29), map(30), map(39)), (Some(0x41), Some(0x5A), Some(0x31), Some(0x30)), "A, Z, 1, 0");
        assert_eq!((map(40), map(41), map(44), map(58), map(69)), (Some(0x0D), Some(0x1B), Some(0x20), Some(0x70), Some(0x7B)));
        assert_eq!((map(79), map(80), map(81), map(82)), (Some(0x27), Some(0x25), Some(0x28), Some(0x26)), "panah");
        assert_eq!((map(224), map(226), map(227), map(230)), (Some(VK_LCONTROL), Some(VK_LMENU), Some(VK_LWIN), Some(VK_RMENU)));
        assert_eq!(map(0), None);
    }

    #[test]
    fn key_packets_carry_the_moonlight_high_bit() {
        let packet = key_packet(0x41, true, modifier::SHIFT);
        assert_eq!(wire::decode(&packet.data), Ok(wire::InputEvent::Key { down: true, key: 0x41, modifiers: 1, flags: 0 }));
        assert_eq!(&packet.data[9..11], &[0x41, 0x80]);
    }

    #[test]
    fn long_text_and_large_moves_are_split() {
        let packets = text_packets(&"é".repeat(20));
        assert_eq!(packets.len(), 2, "40 byte -> 32 + 8, karakter tidak terpotong");
        assert_eq!(relative_packets(40_000, -5).len(), 2);
        assert!(relative_packets(0, 0).is_empty());
    }
}
