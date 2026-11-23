//! Layar sentuh HP sebagai trackpad (mode mouse relatif, seperti mode "trackpad" Moonlight Android):
//!
//! - satu jari digeser = kursor bergerak;
//! - ketuk satu jari = klik kiri; ketuk lalu tahan-geser = seret (tombol kiri ditahan);
//! - ketuk dua jari = klik kanan; geser dua jari = scroll;
//! - ketuk tiga jari = buka/tutup keyboard layar (ditangani sisi Android, lihat `Action`).
//!
//! Murni data masuk -> paket keluar, tanpa Android, supaya teruji di mesin pengembang.

use cloudpc_stream_client::input::{channel, relative_packets, InputPacket};
use cloudpc_stream_proto::input as wire;

/// Sentuhan yang lebih singkat dari ini dan tidak bergeser = ketukan.
pub const TAP_MS: u64 = 250;
/// Sentuhan baru dalam waktu ini setelah ketukan = ketuk-lalu-seret (atau klik ganda bila tidak digeser).
pub const DOUBLE_TAP_MS: u64 = 300;
/// Satuan roda mouse per piksel geseran dua jari (120 = satu takik; 40 px per takik).
const SCROLL_PER_PX: f32 = 3.0;
/// Scroll dikirim bila sudah terkumpul sebanyak ini (scroll halus, tetapi tidak satu paket per piksel).
const SCROLL_STEP: f32 = 24.0;

pub const BUTTON_LEFT: u8 = 1;
pub const BUTTON_RIGHT: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchKind {
    Down,
    Move,
    Up,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Touch {
    pub kind: TouchKind,
    /// Id jari dari Android (`MotionEvent.getPointerId`).
    pub id: i32,
    pub x: f32,
    pub y: f32,
    pub time_ms: u64,
}

/// Hal yang harus dilakukan aplikasi Android, bukan dikirim ke host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    ToggleKeyboard,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub packets: Vec<InputPacket>,
    pub action: Option<Action>,
}

struct Finger {
    id: i32,
    x: f32,
    y: f32,
    start: (f32, f32),
}

pub struct Trackpad {
    /// Geseran di bawah ini (piksel) masih dianggap diam.
    slop: f32,
    /// Pengali gerak jari -> gerak kursor.
    speed: f32,
    fingers: Vec<Finger>,
    max_fingers: usize,
    moved: bool,
    started_ms: u64,
    last_tap_ms: Option<u64>,
    drag_armed: bool,
    dragging: bool,
    carry: (f32, f32),
    scroll_carry: f32,
}

fn button(down: bool, which: u8) -> InputPacket {
    InputPacket { channel: channel::MOUSE, data: wire::mouse_button(down, which) }
}

impl Trackpad {
    pub fn new(slop: f32, speed: f32) -> Trackpad {
        Trackpad {
            slop: slop.max(1.0),
            speed: if speed > 0.0 { speed } else { 1.0 },
            fingers: Vec::new(),
            max_fingers: 0,
            moved: false,
            started_ms: 0,
            last_tap_ms: None,
            drag_armed: false,
            dragging: false,
            carry: (0.0, 0.0),
            scroll_carry: 0.0,
        }
    }

    pub fn touch(&mut self, t: Touch) -> Outcome {
        let mut out = Outcome::default();
        match t.kind {
            TouchKind::Down => {
                if self.fingers.is_empty() {
                    self.max_fingers = 0;
                    self.moved = false;
                    self.started_ms = t.time_ms;
                    self.carry = (0.0, 0.0);
                    self.scroll_carry = 0.0;
                    self.drag_armed = self.last_tap_ms.is_some_and(|tap| t.time_ms.saturating_sub(tap) <= DOUBLE_TAP_MS);
                }
                self.fingers.retain(|f| f.id != t.id);
                self.fingers.push(Finger { id: t.id, x: t.x, y: t.y, start: (t.x, t.y) });
                self.max_fingers = self.max_fingers.max(self.fingers.len());
            }
            TouchKind::Move => self.moved_finger(t, &mut out),
            TouchKind::Up => {
                self.fingers.retain(|f| f.id != t.id);
                if self.fingers.is_empty() {
                    self.finish(t.time_ms, &mut out);
                }
            }
            TouchKind::Cancel => {
                if self.dragging {
                    out.packets.push(button(false, BUTTON_LEFT));
                }
                self.fingers.clear();
                self.dragging = false;
                self.drag_armed = false;
                self.last_tap_ms = None;
            }
        }
        out
    }

    fn moved_finger(&mut self, t: Touch, out: &mut Outcome) {
        let count = self.fingers.len();
        let Some(finger) = self.fingers.iter_mut().find(|f| f.id == t.id) else { return };
        let (dx, dy) = (t.x - finger.x, t.y - finger.y);
        finger.x = t.x;
        finger.y = t.y;
        let far = (t.x - finger.start.0).hypot(t.y - finger.start.1) > self.slop;
        if !self.moved {
            if !far {
                return;
            }
            // Baru saja melewati batas diam: lompatan awal dibuang supaya kursor tidak meloncat.
            self.moved = true;
            if self.drag_armed && self.max_fingers == 1 {
                out.packets.push(button(true, BUTTON_LEFT));
                self.dragging = true;
            }
            return;
        }
        if self.max_fingers == 1 && count == 1 {
            let (fx, fy) = (dx * self.speed + self.carry.0, dy * self.speed + self.carry.1);
            let (ix, iy) = (fx.trunc(), fy.trunc());
            self.carry = (fx - ix, fy - iy);
            out.packets.extend(relative_packets(ix as i32, iy as i32));
        } else if count == 2 {
            // Tiap jari menyumbang setengah: dua jari yang bergeser bersama = satu geseran.
            self.scroll_carry += dy * 0.5 * SCROLL_PER_PX;
            if self.scroll_carry.abs() >= SCROLL_STEP {
                let amount = self.scroll_carry.trunc().clamp(-32767.0, 32767.0);
                self.scroll_carry -= amount;
                out.packets.push(InputPacket { channel: channel::MOUSE, data: wire::scroll(amount as i16) });
            }
        }
    }

    fn finish(&mut self, time_ms: u64, out: &mut Outcome) {
        let tap = !self.moved && time_ms.saturating_sub(self.started_ms) <= TAP_MS;
        let mut tapped_once = false;
        if self.dragging {
            out.packets.push(button(false, BUTTON_LEFT));
        } else if tap {
            match self.max_fingers {
                1 => {
                    out.packets.push(button(true, BUTTON_LEFT));
                    out.packets.push(button(false, BUTTON_LEFT));
                    tapped_once = true;
                }
                2 => {
                    out.packets.push(button(true, BUTTON_RIGHT));
                    out.packets.push(button(false, BUTTON_RIGHT));
                }
                3 => out.action = Some(Action::ToggleKeyboard),
                _ => {}
            }
        }
        self.last_tap_ms = tapped_once.then_some(time_ms);
        self.dragging = false;
        self.drag_armed = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(kind: TouchKind, id: i32, x: f32, y: f32, time_ms: u64) -> Touch {
        Touch { kind, id, x, y, time_ms }
    }

    fn left(down: bool) -> InputPacket {
        button(down, BUTTON_LEFT)
    }

    #[test]
    fn tap_is_a_left_click_and_does_not_move_the_cursor() {
        let mut pad = Trackpad::new(10.0, 1.0);
        assert_eq!(pad.touch(t(TouchKind::Down, 0, 100.0, 100.0, 0)), Outcome::default());
        assert_eq!(pad.touch(t(TouchKind::Move, 0, 103.0, 101.0, 30)), Outcome::default()); // jari bergetar sedikit
        let up = pad.touch(t(TouchKind::Up, 0, 103.0, 101.0, 90));
        assert_eq!(up.packets, vec![left(true), left(false)]);
    }

    #[test]
    fn long_press_without_moving_is_not_a_click() {
        let mut pad = Trackpad::new(10.0, 1.0);
        pad.touch(t(TouchKind::Down, 0, 100.0, 100.0, 0));
        assert_eq!(pad.touch(t(TouchKind::Up, 0, 100.0, 100.0, TAP_MS + 1)), Outcome::default());
    }

    #[test]
    fn one_finger_drag_moves_the_cursor_without_the_initial_jump() {
        let mut pad = Trackpad::new(10.0, 1.5);
        pad.touch(t(TouchKind::Down, 0, 100.0, 100.0, 0));
        assert!(pad.touch(t(TouchKind::Move, 0, 120.0, 100.0, 20)).packets.is_empty()); // melewati batas diam: dibuang
        let moved = pad.touch(t(TouchKind::Move, 0, 130.0, 96.0, 40));
        assert_eq!(moved.packets, relative_packets(15, -6));
        assert!(pad.touch(t(TouchKind::Up, 0, 130.0, 96.0, 60)).packets.is_empty()); // bukan klik
    }

    #[test]
    fn fractions_are_carried_so_slow_movement_is_not_lost() {
        let mut pad = Trackpad::new(2.0, 0.5);
        pad.touch(t(TouchKind::Down, 0, 0.0, 0.0, 0));
        pad.touch(t(TouchKind::Move, 0, 5.0, 0.0, 10));
        let mut total = 0;
        for step in 1..=10 {
            let out = pad.touch(t(TouchKind::Move, 0, 5.0 + step as f32, 0.0, 10 + step));
            total += out.packets.len();
        }
        assert_eq!(total, 5); // 10 px x 0,5 = 5 langkah 1 px
    }

    #[test]
    fn tap_then_drag_holds_the_left_button() {
        let mut pad = Trackpad::new(10.0, 1.0);
        pad.touch(t(TouchKind::Down, 0, 50.0, 50.0, 0));
        pad.touch(t(TouchKind::Up, 0, 50.0, 50.0, 80));
        pad.touch(t(TouchKind::Down, 0, 50.0, 50.0, 200));
        assert_eq!(pad.touch(t(TouchKind::Move, 0, 70.0, 50.0, 260)).packets, vec![left(true)]);
        assert_eq!(pad.touch(t(TouchKind::Move, 0, 80.0, 50.0, 280)).packets, relative_packets(10, 0));
        assert_eq!(pad.touch(t(TouchKind::Up, 0, 80.0, 50.0, 900)).packets, vec![left(false)]);
    }

    #[test]
    fn two_quick_taps_are_two_clicks() {
        let mut pad = Trackpad::new(10.0, 1.0);
        pad.touch(t(TouchKind::Down, 0, 50.0, 50.0, 0));
        pad.touch(t(TouchKind::Up, 0, 50.0, 50.0, 80));
        pad.touch(t(TouchKind::Down, 0, 50.0, 50.0, 200));
        assert_eq!(pad.touch(t(TouchKind::Up, 0, 50.0, 50.0, 260)).packets, vec![left(true), left(false)]);
    }

    #[test]
    fn two_finger_tap_is_a_right_click_and_three_toggles_the_keyboard() {
        let mut pad = Trackpad::new(10.0, 1.0);
        pad.touch(t(TouchKind::Down, 0, 50.0, 50.0, 0));
        pad.touch(t(TouchKind::Down, 1, 150.0, 50.0, 10));
        assert!(pad.touch(t(TouchKind::Up, 1, 150.0, 50.0, 90)).packets.is_empty());
        let up = pad.touch(t(TouchKind::Up, 0, 50.0, 50.0, 100));
        assert_eq!(up.packets, vec![button(true, BUTTON_RIGHT), button(false, BUTTON_RIGHT)]);

        for id in 0..3 {
            pad.touch(t(TouchKind::Down, id, 50.0 * id as f32, 50.0, 1000 + id as u64));
        }
        pad.touch(t(TouchKind::Up, 2, 100.0, 50.0, 1080));
        pad.touch(t(TouchKind::Up, 1, 50.0, 50.0, 1085));
        let up = pad.touch(t(TouchKind::Up, 0, 0.0, 50.0, 1090));
        assert_eq!(up, Outcome { packets: vec![], action: Some(Action::ToggleKeyboard) });
    }

    #[test]
    fn two_finger_drag_scrolls_and_never_clicks_or_moves() {
        let mut pad = Trackpad::new(10.0, 1.0);
        pad.touch(t(TouchKind::Down, 0, 100.0, 100.0, 0));
        pad.touch(t(TouchKind::Down, 1, 200.0, 100.0, 5));
        let mut packets = Vec::new();
        for step in 1..=8 {
            let y = 100.0 + 10.0 * step as f32;
            packets.extend(pad.touch(t(TouchKind::Move, 0, 100.0, y, 10 * step)).packets);
            packets.extend(pad.touch(t(TouchKind::Move, 1, 200.0, y, 10 * step)).packets);
        }
        packets.extend(pad.touch(t(TouchKind::Up, 1, 200.0, 180.0, 200)).packets);
        packets.extend(pad.touch(t(TouchKind::Up, 0, 100.0, 180.0, 210)).packets);
        assert!(!packets.is_empty());
        // Jari turun 80 px (10 px pertama terbuang di batas diam untuk jari pertama saja): semua paket adalah scroll ke atas.
        let total: i32 = packets
            .iter()
            .map(|p| match wire::decode(&p.data) {
                Ok(wire::InputEvent::Scroll(amount)) => i32::from(amount),
                other => panic!("bukan scroll: {other:?}"),
            })
            .sum();
        assert!((180..=240).contains(&total), "total scroll {total}");
    }

    #[test]
    fn cancel_releases_a_held_button() {
        let mut pad = Trackpad::new(10.0, 1.0);
        pad.touch(t(TouchKind::Down, 0, 50.0, 50.0, 0));
        pad.touch(t(TouchKind::Up, 0, 50.0, 50.0, 80));
        pad.touch(t(TouchKind::Down, 0, 50.0, 50.0, 200));
        pad.touch(t(TouchKind::Move, 0, 90.0, 50.0, 260));
        assert_eq!(pad.touch(t(TouchKind::Cancel, 0, 90.0, 50.0, 270)).packets, vec![left(false)]);
    }
}
