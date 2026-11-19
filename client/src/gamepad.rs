//! Gamepad lewat SDL3 seperti Moonlight: stik Xbox, PlayStation 4/5, Switch Pro, dll. dengan tata letak tombol yang
//! sama (tombol bawah = A), getar, dan pesan kedatangan stik (`SS_CONTROLLER_ARRIVAL`) yang memberi tahu host jenis stik
//! (Sunshine meniru DS4 untuk stik PlayStation). SDL3 dimuat saat jalan (`sdl.rs`); di Windows tanpa SDL3 `input.rs`
//! memakai XInput (hanya stik Xbox).
//!
//! Rujukan: `moonlight-qt/app/streaming/input/gamepad.cpp` (pemetaan tombol, sumbu Y dibalik, pemicu 0..255).

use std::ffi::{c_char, c_void, CStr};

use cloudpc_stream_proto::input as wire;

use crate::input::{channel, InputPacket};

const SDL_INIT_GAMEPAD: u32 = 0x0000_2000;
/// Stik dibaca walau jendela stream tidak sedang di depan (tetap disaring oleh status tangkap input).
const HINT_BACKGROUND: &CStr = c"SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS";

/// Bendera tombol Moonlight (`Limelight.h`) untuk `SDL_GamepadButton` 0..=20.
const BUTTONS: [u32; 21] = [
    0x1000,   // SOUTH -> A
    0x2000,   // EAST -> B
    0x4000,   // WEST -> X
    0x8000,   // NORTH -> Y
    0x0020,   // BACK
    0x0400,   // GUIDE (SPECIAL)
    0x0010,   // START (PLAY)
    0x0040,   // LEFT_STICK
    0x0080,   // RIGHT_STICK
    0x0100,   // LEFT_SHOULDER
    0x0200,   // RIGHT_SHOULDER
    0x0001,   // DPAD_UP
    0x0002,   // DPAD_DOWN
    0x0004,   // DPAD_LEFT
    0x0008,   // DPAD_RIGHT
    0x200000, // MISC1 (Share/Mic/Capture)
    0x010000, // RIGHT_PADDLE1 -> PADDLE1
    0x040000, // LEFT_PADDLE1 -> PADDLE3
    0x020000, // RIGHT_PADDLE2 -> PADDLE2
    0x080000, // LEFT_PADDLE2 -> PADDLE4
    0x100000, // TOUCHPAD
];

/// Jenis stik Moonlight (`LI_CTYPE_*`) dari `SDL_GamepadType`.
pub fn controller_type(sdl_type: i32) -> u8 {
    match sdl_type {
        2 | 3 => 0x01,          // Xbox 360 / One
        4..=6 => 0x02,          // PS3 / PS4 / PS5
        7..=10 => 0x03,         // Switch Pro / Joy-Con / GameCube
        11 => 0x04,             // Steam
        _ => 0x00,
    }
}

/// `SS_CONTROLLER_ARRIVAL_PACKET`: header input + nomor stik, jenis, kemampuan, tombol yang ada (little-endian).
pub fn arrival_packet(index: u8, kind: u8, capabilities: u16, buttons: u32) -> Vec<u8> {
    let mut body = Vec::with_capacity(8);
    body.push(index);
    body.push(kind);
    body.extend_from_slice(&capabilities.to_le_bytes());
    body.extend_from_slice(&buttons.to_le_bytes());
    let mut packet = (body.len() as u32 + 4).to_be_bytes().to_vec();
    packet.extend_from_slice(&0x5500_0004u32.to_le_bytes());
    packet.extend_from_slice(&body);
    packet
}

/// Sumbu SDL -> nilai Moonlight: Y dibalik (atas = positif), pemicu 0..=32767 -> 0..=255.
pub fn stick(x: i16, y: i16) -> (i16, i16) {
    (x, if y == i16::MIN { i16::MAX } else { -y })
}

pub fn trigger(value: i16) -> u8 {
    (i32::from(value.max(0)) * 255 / 32767) as u8
}

/// Kemampuan yang diumumkan: pemicu analog + getar. Touchpad dan sensor gerak belum dikirim, jadi tidak diumumkan.
pub const CAPABILITIES: u16 = 0x01 | 0x02;

type FnInit = unsafe extern "C" fn(u32) -> bool;
type FnSetHint = unsafe extern "C" fn(*const c_char, *const c_char) -> bool;
type FnGetGamepads = unsafe extern "C" fn(*mut i32) -> *mut u32;
type FnFree = unsafe extern "C" fn(*mut c_void);
type FnOpen = unsafe extern "C" fn(u32) -> *mut c_void;
type FnClose = unsafe extern "C" fn(*mut c_void);
type FnUpdate = unsafe extern "C" fn();
type FnConnected = unsafe extern "C" fn(*mut c_void) -> bool;
type FnButton = unsafe extern "C" fn(*mut c_void, i32) -> bool;
type FnAxis = unsafe extern "C" fn(*mut c_void, i32) -> i16;
type FnType = unsafe extern "C" fn(*mut c_void) -> i32;
type FnRumble = unsafe extern "C" fn(*mut c_void, u16, u16, u32) -> bool;
type FnName = unsafe extern "C" fn(*mut c_void) -> *const c_char;

struct Api {
    _library: libloading::Library,
    get_gamepads: FnGetGamepads,
    free: FnFree,
    open: FnOpen,
    close: FnClose,
    update: FnUpdate,
    connected: FnConnected,
    button: FnButton,
    axis: FnAxis,
    kind: FnType,
    rumble: FnRumble,
    name: FnName,
}

struct Slot {
    id: u32,
    pad: *mut c_void,
    state: wire::Gamepad,
}

pub struct Pads {
    api: Api,
    slots: [Option<Slot>; 4],
    /// Nama stik yang baru tersambung (untuk log).
    pub arrived: Vec<String>,
}

impl Pads {
    /// Muat `SDL3.dll` (di samping exe atau di PATH). `None` = tidak ada; pakai XInput.
    pub fn open() -> Option<Pads> {
        unsafe {
            let library = crate::sdl::open_library()?;
            macro_rules! sym {
                ($name:literal, $ty:ty) => {
                    *library.get::<$ty>($name).ok()?
                };
            }
            let init: FnInit = sym!(b"SDL_Init\0", FnInit);
            let set_hint: FnSetHint = sym!(b"SDL_SetHint\0", FnSetHint);
            let api = Api {
                get_gamepads: sym!(b"SDL_GetGamepads\0", FnGetGamepads),
                free: sym!(b"SDL_free\0", FnFree),
                open: sym!(b"SDL_OpenGamepad\0", FnOpen),
                close: sym!(b"SDL_CloseGamepad\0", FnClose),
                update: sym!(b"SDL_UpdateGamepads\0", FnUpdate),
                connected: sym!(b"SDL_GamepadConnected\0", FnConnected),
                button: sym!(b"SDL_GetGamepadButton\0", FnButton),
                axis: sym!(b"SDL_GetGamepadAxis\0", FnAxis),
                kind: sym!(b"SDL_GetGamepadType\0", FnType),
                rumble: sym!(b"SDL_RumbleGamepad\0", FnRumble),
                name: sym!(b"SDL_GetGamepadName\0", FnName),
                _library: library,
            };
            set_hint(HINT_BACKGROUND.as_ptr(), c"1".as_ptr());
            if !init(SDL_INIT_GAMEPAD) {
                return None;
            }
            Some(Pads { api, slots: [None, None, None, None], arrived: Vec::new() })
        }
    }

    fn mask(&self) -> u16 {
        self.slots.iter().enumerate().filter(|(_, s)| s.is_some()).fold(0, |m, (i, _)| m | 1 << i)
    }

    fn read(&self, slot: &Slot, index: u8) -> wire::Gamepad {
        let a = &self.api;
        unsafe {
            let buttons = BUTTONS.iter().enumerate().filter(|(b, _)| (a.button)(slot.pad, *b as i32)).fold(0u32, |m, (_, flag)| m | flag);
            wire::Gamepad {
                index,
                active_mask: self.mask(),
                buttons,
                left_trigger: trigger((a.axis)(slot.pad, 4)),
                right_trigger: trigger((a.axis)(slot.pad, 5)),
                left_stick: stick((a.axis)(slot.pad, 0), (a.axis)(slot.pad, 1)),
                right_stick: stick((a.axis)(slot.pad, 2), (a.axis)(slot.pad, 3)),
            }
        }
    }

    /// Baca stik; kembalikan paket yang perlu dikirim (kedatangan, keadaan berubah, stik dicabut).
    pub fn poll(&mut self) -> Vec<InputPacket> {
        let mut out = Vec::new();
        unsafe {
            (self.api.update)();
            // Stik dicabut.
            for index in 0..4 {
                if self.slots[index].as_ref().is_some_and(|s| !(self.api.connected)(s.pad)) {
                    let slot = self.slots[index].take().expect("ada");
                    (self.api.close)(slot.pad);
                    let gone = wire::Gamepad { index: index as u8, active_mask: self.mask(), ..Default::default() };
                    out.push(InputPacket { channel: channel::GAMEPAD_BASE + index as u8, data: wire::gamepad(&gone) });
                }
            }
            // Stik baru.
            let mut count = 0;
            let ids = (self.api.get_gamepads)(&mut count);
            if !ids.is_null() {
                for &id in std::slice::from_raw_parts(ids, count.max(0) as usize) {
                    if self.slots.iter().flatten().any(|s| s.id == id) {
                        continue;
                    }
                    let Some(index) = self.slots.iter().position(Option::is_none) else { break };
                    let pad = (self.api.open)(id);
                    if pad.is_null() {
                        continue;
                    }
                    let name = (self.api.name)(pad);
                    let name = if name.is_null() { "stik".into() } else { CStr::from_ptr(name).to_string_lossy().into_owned() };
                    let kind = controller_type((self.api.kind)(pad));
                    self.arrived.push(format!("{name} (jenis {kind}) di slot {index}"));
                    out.push(InputPacket { channel: channel::GAMEPAD_BASE + index as u8, data: arrival_packet(index as u8, kind, CAPABILITIES, BUTTONS.iter().fold(0, |m, f| m | f)) });
                    self.slots[index] = Some(Slot { id, pad, state: wire::Gamepad { index: index as u8, ..Default::default() } });
                    // Keadaan pertama selalu dikirim.
                    if let Some(slot) = self.slots[index].as_mut() {
                        slot.state.buttons = u32::MAX;
                    }
                }
                (self.api.free)(ids as *mut c_void);
            }
            // Keadaan yang berubah.
            for index in 0..4 {
                let Some(slot) = self.slots[index].as_ref() else { continue };
                let state = self.read(slot, index as u8);
                if state != slot.state {
                    out.push(InputPacket { channel: channel::GAMEPAD_BASE + index as u8, data: wire::gamepad(&state) });
                    self.slots[index].as_mut().expect("ada").state = state;
                }
            }
        }
        out
    }

    /// Getar stik di slot `index` (30 detik atau sampai pesan getar berikutnya, seperti Moonlight).
    pub fn rumble(&self, index: u16, low: u16, high: u16) -> bool {
        match self.slots.get(usize::from(index)).and_then(Option::as_ref) {
            Some(slot) => unsafe { (self.api.rumble)(slot.pad, low, high, if low == 0 && high == 0 { 0 } else { 30_000 }) },
            None => false,
        }
    }

    pub fn connected(&self) -> usize {
        self.slots.iter().flatten().count()
    }
}

impl Drop for Pads {
    fn drop(&mut self) {
        for slot in self.slots.iter_mut().filter_map(Option::take) {
            unsafe {
                (self.api.rumble)(slot.pad, 0, 0, 0);
                (self.api.close)(slot.pad);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrival_packet_matches_moonlight_layout() {
        let packet = arrival_packet(1, 0x02, CAPABILITIES, 0x0012_3456);
        // Ukuran = magic (4) + isi (8) big-endian; magic little-endian; isi little-endian.
        assert_eq!(packet, [0, 0, 0, 12, 0x04, 0x00, 0x00, 0x55, 1, 0x02, 0x03, 0x00, 0x56, 0x34, 0x12, 0x00]);
        assert_eq!(wire::decode(&packet), Ok(wire::InputEvent::Other(0x5500_0004)));
    }

    #[test]
    fn axes_and_types_follow_moonlight() {
        assert_eq!(stick(100, -32768), (100, 32767), "SDL atas = negatif; Moonlight atas = positif");
        assert_eq!(stick(0, 32767), (0, -32767));
        assert_eq!(stick(0, 0), (0, 0), "tengah tetap tengah");
        assert_eq!((trigger(0), trigger(32767), trigger(-5)), (0, 255, 0));
        assert_eq!((controller_type(3), controller_type(5), controller_type(6), controller_type(7), controller_type(1)), (0x01, 0x02, 0x02, 0x03, 0x00));
    }

    #[test]
    fn sdl3_loads_when_the_dll_is_next_to_the_program() {
        let beside = std::env::current_exe().unwrap().with_file_name("SDL3.dll");
        if !beside.exists() {
            eprintln!("SDL3.dll tidak ada di {}, tes dilewati", beside.display());
            return;
        }
        let pads = Pads::open().expect("SDL3 dimuat dan subsistem gamepad hidup");
        // Tanpa stik tercolok tidak ada slot terisi; dengan stik, satu atau lebih.
        let mut pads = pads;
        let _ = pads.poll();
        eprintln!("stik tersambung: {} {:?}", pads.connected(), pads.arrived);
    }
}
