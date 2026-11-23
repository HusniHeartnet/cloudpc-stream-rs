//! Gamepad dan keyboard fisik Android -> paket input GameStream.
//!
//! Android mengirim tombol sebagai `KeyEvent` (kode `KEYCODE_*`) dan stik/pemicu sebagai `MotionEvent` (sumbu -1..1).
//! Di sini keduanya dijadikan keadaan stik Xbox (`proto::input::Gamepad`) dan kode tombol virtual Windows.
//! Murni data, tanpa Android, supaya teruji di mesin pengembang.

use cloudpc_stream_client::input::{channel, key_packet, InputPacket, Keys};
use cloudpc_stream_proto::input::{self as wire, Gamepad};

/// Bit tombol sama dengan `XINPUT_GAMEPAD_*`.
pub mod bit {
    pub const UP: u32 = 0x0001;
    pub const DOWN: u32 = 0x0002;
    pub const LEFT: u32 = 0x0004;
    pub const RIGHT: u32 = 0x0008;
    pub const START: u32 = 0x0010;
    pub const BACK: u32 = 0x0020;
    pub const LEFT_STICK: u32 = 0x0040;
    pub const RIGHT_STICK: u32 = 0x0080;
    pub const LB: u32 = 0x0100;
    pub const RB: u32 = 0x0200;
    pub const GUIDE: u32 = 0x0400;
    pub const A: u32 = 0x1000;
    pub const B: u32 = 0x2000;
    pub const X: u32 = 0x4000;
    pub const Y: u32 = 0x8000;
}

/// `KeyEvent.KEYCODE_*` yang dipakai di sini.
pub mod keycode {
    pub const DPAD_UP: i32 = 19;
    pub const DPAD_DOWN: i32 = 20;
    pub const DPAD_LEFT: i32 = 21;
    pub const DPAD_RIGHT: i32 = 22;
    pub const BUTTON_A: i32 = 96;
    pub const BUTTON_B: i32 = 97;
    pub const BUTTON_X: i32 = 99;
    pub const BUTTON_Y: i32 = 100;
    pub const BUTTON_L1: i32 = 102;
    pub const BUTTON_R1: i32 = 103;
    pub const BUTTON_L2: i32 = 104;
    pub const BUTTON_R2: i32 = 105;
    pub const BUTTON_THUMBL: i32 = 106;
    pub const BUTTON_THUMBR: i32 = 107;
    pub const BUTTON_START: i32 = 108;
    pub const BUTTON_SELECT: i32 = 109;
    pub const BUTTON_MODE: i32 = 110;
}

pub const MAX_PADS: usize = 4;
/// Id perangkat untuk stik di layar (`overlay.rs`); id perangkat Android asli tidak pernah negatif sebesar ini.
pub const VIRTUAL_DEVICE: i32 = -1000;

/// Keadaan lengkap satu stik (konvensi XInput: Y positif = ke atas), untuk sumber yang sudah menghitungnya sendiri.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PadState {
    pub buttons: u32,
    pub triggers: (u8, u8),
    pub left: (i16, i16),
    pub right: (i16, i16),
}

fn button_bit(code: i32) -> Option<u32> {
    use keycode::*;
    Some(match code {
        DPAD_UP => bit::UP,
        DPAD_DOWN => bit::DOWN,
        DPAD_LEFT => bit::LEFT,
        DPAD_RIGHT => bit::RIGHT,
        BUTTON_A => bit::A,
        BUTTON_B => bit::B,
        BUTTON_X => bit::X,
        BUTTON_Y => bit::Y,
        BUTTON_L1 => bit::LB,
        BUTTON_R1 => bit::RB,
        BUTTON_THUMBL => bit::LEFT_STICK,
        BUTTON_THUMBR => bit::RIGHT_STICK,
        BUTTON_START => bit::START,
        BUTTON_SELECT => bit::BACK,
        BUTTON_MODE => bit::GUIDE,
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, Default)]
struct Slot {
    device: i32,
    /// Tombol dari `KeyEvent`.
    keys: u32,
    /// D-pad dari sumbu HAT (banyak stik melaporkan d-pad sebagai sumbu, bukan tombol).
    hat: u32,
    /// Pemicu yang dilaporkan sebagai tombol L2/R2 (stik tanpa sumbu pemicu).
    trigger_keys: (bool, bool),
    triggers: (u8, u8),
    left: (i16, i16),
    right: (i16, i16),
}

/// Sumbu stik dari satu `MotionEvent` (semua -1..1; Y positif = ke bawah seperti Android).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Axes {
    pub lx: f32,
    pub ly: f32,
    pub rx: f32,
    pub ry: f32,
    pub lt: f32,
    pub rt: f32,
    pub hat_x: f32,
    pub hat_y: f32,
}

fn stick(v: f32) -> i16 {
    (v.clamp(-1.0, 1.0) * 32767.0).round() as i16
}

fn trigger(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Sampai empat gamepad, dikenali dari id perangkat Android.
#[derive(Debug, Default)]
pub struct Pads {
    slots: [Option<Slot>; MAX_PADS],
}

impl Pads {
    fn index_of(&mut self, device: i32) -> Option<usize> {
        if let Some(i) = self.slots.iter().position(|s| s.is_some_and(|s| s.device == device)) {
            return Some(i);
        }
        let free = self.slots.iter().position(Option::is_none)?;
        self.slots[free] = Some(Slot { device, ..Slot::default() });
        Some(free)
    }

    fn mask(&self) -> u16 {
        self.slots.iter().enumerate().filter(|(_, s)| s.is_some()).fold(0, |m, (i, _)| m | 1 << i)
    }

    fn packet(&self, index: usize) -> InputPacket {
        let slot = self.slots[index].unwrap_or_default();
        let pad = Gamepad {
            index: index as u8,
            active_mask: self.mask(),
            buttons: slot.keys | slot.hat,
            left_trigger: if slot.trigger_keys.0 { 255 } else { slot.triggers.0 },
            right_trigger: if slot.trigger_keys.1 { 255 } else { slot.triggers.1 },
            left_stick: slot.left,
            right_stick: slot.right,
        };
        InputPacket { channel: channel::GAMEPAD_BASE + index as u8, data: wire::gamepad(&pad) }
    }

    /// Tombol gamepad. `None` = bukan tombol gamepad (biarkan Android/keyboard menanganinya) atau slot penuh.
    pub fn button(&mut self, device: i32, code: i32, down: bool) -> Option<InputPacket> {
        let is_trigger = matches!(code, keycode::BUTTON_L2 | keycode::BUTTON_R2);
        let bit = button_bit(code);
        if bit.is_none() && !is_trigger {
            return None;
        }
        let index = self.index_of(device)?;
        let slot = self.slots[index].as_mut()?;
        match (bit, code) {
            (Some(bit), _) if down => slot.keys |= bit,
            (Some(bit), _) => slot.keys &= !bit,
            (None, keycode::BUTTON_L2) => slot.trigger_keys.0 = down,
            (None, _) => slot.trigger_keys.1 = down,
        }
        Some(self.packet(index))
    }

    pub fn axes(&mut self, device: i32, a: Axes) -> Option<InputPacket> {
        let index = self.index_of(device)?;
        let slot = self.slots[index].as_mut()?;
        // XInput: Y positif = ke atas, kebalikan Android.
        slot.left = (stick(a.lx), stick(-a.ly));
        slot.right = (stick(a.rx), stick(-a.ry));
        slot.triggers = (trigger(a.lt), trigger(a.rt));
        slot.hat = 0;
        if a.hat_x < -0.5 {
            slot.hat |= bit::LEFT;
        } else if a.hat_x > 0.5 {
            slot.hat |= bit::RIGHT;
        }
        if a.hat_y < -0.5 {
            slot.hat |= bit::UP;
        } else if a.hat_y > 0.5 {
            slot.hat |= bit::DOWN;
        }
        Some(self.packet(index))
    }

    pub fn has(&self, device: i32) -> bool {
        self.slots.iter().any(|s| s.is_some_and(|s| s.device == device))
    }

    /// Ganti seluruh keadaan satu stik sekaligus (stik di layar).
    pub fn set_state(&mut self, device: i32, state: PadState) -> Option<InputPacket> {
        let index = self.index_of(device)?;
        let slot = self.slots[index].as_mut()?;
        slot.keys = state.buttons;
        slot.hat = 0;
        slot.trigger_keys = (false, false);
        slot.triggers = state.triggers;
        slot.left = state.left;
        slot.right = state.right;
        Some(self.packet(index))
    }

    /// Stik dicabut: host diberi tahu lewat `active_mask` tanpa bit stik ini.
    pub fn disconnect(&mut self, device: i32) -> Option<InputPacket> {
        let index = self.slots.iter().position(|s| s.is_some_and(|s| s.device == device))?;
        self.slots[index] = None;
        Some(self.packet(index))
    }
}

/// `KeyEvent.KEYCODE_*` keyboard -> kode tombol virtual Windows.
pub fn vk_from_android(code: i32) -> Option<u16> {
    Some(match code {
        7..=16 => 0x30 + (code - 7) as u16,       // 0-9
        29..=54 => 0x41 + (code - 29) as u16,     // A-Z
        131..=142 => 0x70 + (code - 131) as u16,  // F1-F12
        144..=153 => 0x60 + (code - 144) as u16,  // numpad 0-9
        19 => 0x26,                               // panah atas
        20 => 0x28,
        21 => 0x25,
        22 => 0x27,
        55 => 0xBC,  // ,
        56 => 0xBE,  // .
        57 => 0xA4,  // Alt kiri
        58 => 0xA5,  // Alt kanan
        59 => 0xA0,  // Shift kiri
        60 => 0xA1,  // Shift kanan
        61 => 0x09,  // Tab
        62 => 0x20,  // spasi
        66 => 0x0D,  // Enter
        67 => 0x08,  // Backspace (KEYCODE_DEL)
        68 => 0xC0,  // `
        69 => 0xBD,  // -
        70 => 0xBB,  // =
        71 => 0xDB,  // [
        72 => 0xDD,  // ]
        73 => 0xDC,  // \
        74 => 0xBA,  // ;
        75 => 0xDE,  // '
        76 => 0xBF,  // /
        92 => 0x21,  // Page Up
        93 => 0x22,  // Page Down
        111 => 0x1B, // Esc
        112 => 0x2E, // Delete (KEYCODE_FORWARD_DEL)
        113 => 0xA2, // Ctrl kiri
        114 => 0xA3, // Ctrl kanan
        115 => 0x14, // Caps Lock
        117 => 0x5B, // Windows kiri (Meta)
        118 => 0x5C, // Windows kanan
        120 => 0x2C, // Print Screen (SYSRQ)
        122 => 0x24, // Home
        123 => 0x23, // End
        124 => 0x2D, // Insert
        160 => 0x0D, // Enter numpad
        _ => return None,
    })
}

/// Keyboard fisik atau keyboard layar: melacak tombol yang ditahan supaya pengubah (Shift/Ctrl/Alt) ikut terkirim.
#[derive(Debug, Default)]
pub struct Keyboard {
    keys: Keys,
}

impl Keyboard {
    /// `None` = tombol tidak dikenal (biarkan Android menanganinya). Vektor kosong = ulangan otomatis; host mengulang sendiri.
    pub fn key(&mut self, code: i32, down: bool) -> Option<Vec<InputPacket>> {
        let vk = vk_from_android(code)?;
        if !self.keys.update(vk, down) {
            return Some(Vec::new());
        }
        Some(vec![key_packet(vk, down, self.keys.modifiers())])
    }

    /// Tombol dengan kode Windows langsung (stik layar profil keyboard). Kosong = keadaannya tidak berubah.
    pub fn vk(&mut self, vk: u16, down: bool) -> Vec<InputPacket> {
        if !self.keys.update(vk, down) {
            return Vec::new();
        }
        vec![key_packet(vk, down, self.keys.modifiers())]
    }

    /// Jendela stream kehilangan fokus/ditutup: tidak boleh ada tombol yang tertinggal ditekan di PC.
    pub fn release_all(&mut self) -> Vec<InputPacket> {
        self.keys.release_all()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cloudpc_stream_client::input::modifier;

    fn pad_of(packet: &InputPacket) -> Gamepad {
        match wire::decode(&packet.data) {
            Ok(wire::InputEvent::Gamepad(pad)) => pad,
            other => panic!("bukan paket gamepad: {other:?}"),
        }
    }

    #[test]
    fn buttons_sticks_and_hat_become_one_xbox_state() {
        let mut pads = Pads::default();
        let p = pads.button(7, keycode::BUTTON_A, true).unwrap();
        assert_eq!(p.channel, channel::GAMEPAD_BASE);
        assert_eq!(pad_of(&p).buttons, bit::A);
        let p = pads.axes(7, Axes { lx: 1.0, ly: 1.0, rx: -0.5, rt: 1.0, hat_x: -1.0, hat_y: 1.0, ..Axes::default() }).unwrap();
        let state = pad_of(&p);
        assert_eq!(state.buttons, bit::A | bit::LEFT | bit::DOWN);
        assert_eq!(state.left_stick, (32767, -32767)); // Y Android ke bawah = Y XInput negatif
        assert_eq!(state.right_stick.0, -16384);
        assert_eq!((state.left_trigger, state.right_trigger), (0, 255));
        assert_eq!(state.active_mask, 0b1);
        let p = pads.button(7, keycode::BUTTON_A, false).unwrap();
        assert_eq!(pad_of(&p).buttons, bit::LEFT | bit::DOWN); // stik dan hat tetap
    }

    #[test]
    fn trigger_buttons_count_as_full_pull_and_keyboard_keys_are_left_alone() {
        let mut pads = Pads::default();
        let p = pads.button(1, keycode::BUTTON_L2, true).unwrap();
        assert_eq!(pad_of(&p).left_trigger, 255);
        assert!(pads.button(1, 29 /* A keyboard */, true).is_none());
    }

    #[test]
    fn second_pad_gets_its_own_index_and_disconnect_clears_the_mask() {
        let mut pads = Pads::default();
        pads.button(10, keycode::BUTTON_X, true);
        let p = pads.button(11, keycode::BUTTON_Y, true).unwrap();
        assert_eq!(p.channel, channel::GAMEPAD_BASE + 1);
        let state = pad_of(&p);
        assert_eq!((state.index, state.active_mask, state.buttons), (1, 0b11, bit::Y));
        let gone = pads.disconnect(10).unwrap();
        assert_eq!((pad_of(&gone).index, pad_of(&gone).active_mask, pad_of(&gone).buttons), (0, 0b10, 0));
        assert!(pads.disconnect(10).is_none());
        // Slot 0 dipakai lagi oleh stik berikutnya.
        assert_eq!(pads.button(12, keycode::BUTTON_B, true).unwrap().channel, channel::GAMEPAD_BASE);
    }

    #[test]
    fn on_screen_pad_shares_the_slots_with_physical_pads() {
        let mut pads = Pads::default();
        pads.button(3, keycode::BUTTON_A, true); // stik fisik lebih dulu = pemain 1
        let state = PadState { buttons: bit::B | bit::UP, triggers: (255, 0), left: (100, -200), right: (0, 32767) };
        let p = pads.set_state(VIRTUAL_DEVICE, state).unwrap();
        assert_eq!(p.channel, channel::GAMEPAD_BASE + 1);
        let pad = pad_of(&p);
        assert_eq!((pad.index, pad.active_mask, pad.buttons), (1, 0b11, bit::B | bit::UP));
        assert_eq!((pad.left_trigger, pad.left_stick, pad.right_stick), (255, (100, -200), (0, 32767)));
        let gone = pads.disconnect(VIRTUAL_DEVICE).unwrap();
        assert_eq!(pad_of(&gone).active_mask, 0b01);
    }

    #[test]
    fn fifth_pad_is_ignored() {
        let mut pads = Pads::default();
        for device in 0..MAX_PADS as i32 {
            assert!(pads.button(device, keycode::BUTTON_A, true).is_some());
        }
        assert!(pads.button(99, keycode::BUTTON_A, true).is_none());
    }

    #[test]
    fn keyboard_maps_letters_and_carries_modifiers() {
        assert_eq!(vk_from_android(29), Some(0x41));
        assert_eq!(vk_from_android(54), Some(0x5A));
        assert_eq!(vk_from_android(16), Some(0x39));
        assert_eq!(vk_from_android(142), Some(0x7B));
        assert_eq!(vk_from_android(4 /* BACK */), None);

        let mut kb = Keyboard::default();
        assert_eq!(kb.key(59, true).unwrap(), vec![key_packet(0xA0, true, modifier::SHIFT)]);
        assert_eq!(kb.key(29, true).unwrap(), vec![key_packet(0x41, true, modifier::SHIFT)]);
        assert_eq!(kb.key(29, true).unwrap(), vec![]); // ulangan otomatis Android
        assert_eq!(kb.key(29, false).unwrap(), vec![key_packet(0x41, false, modifier::SHIFT)]);
        assert_eq!(kb.release_all(), vec![key_packet(0xA0, false, 0)]);
        assert!(kb.key(4, true).is_none());
    }
}
