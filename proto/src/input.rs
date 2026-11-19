//! Paket input (dikirim client lewat kanal kontrol, tipe `INPUT_DATA`): `[ukuran u32 BE][magic u32 LE][isi]`.
//! Ukuran tidak menghitung field ukuran itu sendiri. Tata letak dari `moonlight-common-c/src/Input.h`,
//! urutan byte tiap field dari `InputStream.c`.
//!
//! Dikenali: keyboard, teks UTF-8, mouse relatif/absolut, tombol mouse, scroll tegak/datar, gamepad.
//! Belum: sentuh, pena, sensor gerak dan touchpad gamepad, baterai gamepad (dilaporkan sebagai `Other`).

use crate::{need, u16_be, u16_le, u32_be, u32_le, WireError};

/// Magic paket (generasi 5 ke atas, yang dipakai Sunshine).
pub mod magic {
    pub const KEY_DOWN: u32 = 0x03;
    pub const KEY_UP: u32 = 0x04;
    pub const MOUSE_MOVE_ABS: u32 = 0x05;
    pub const MOUSE_MOVE_REL: u32 = 0x07;
    pub const MOUSE_BUTTON_DOWN: u32 = 0x08;
    pub const MOUSE_BUTTON_UP: u32 = 0x09;
    pub const SCROLL: u32 = 0x0A;
    pub const MULTI_CONTROLLER: u32 = 0x0C;
    pub const ENABLE_HAPTICS: u32 = 0x0D;
    pub const UTF8_TEXT: u32 = 0x17;
    // Ekstensi Sunshine.
    pub const SS_HSCROLL: u32 = 0x5500_0001;
    pub const SS_TOUCH: u32 = 0x5500_0002;
    pub const SS_PEN: u32 = 0x5500_0003;
    pub const SS_CONTROLLER_ARRIVAL: u32 = 0x5500_0004;
}

/// Bendera di paket keyboard: kode tombol TIDAK dinormalkan ke tata letak US oleh client.
pub const KEY_FLAG_NON_NORMALIZED: u8 = 0x01;
/// Jumlah gamepad yang bisa dinyatakan protokol.
pub const MAX_GAMEPADS: usize = 16;
/// Panjang teks UTF-8 terbesar dalam satu paket.
pub const MAX_TEXT_BYTES: usize = 32;

pub const HEADER_SIZE: usize = 8;

/// Keadaan satu gamepad. Bit `buttons` sama dengan `XINPUT_GAMEPAD_*` (16 bit bawah); bit atas ekstensi Sunshine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Gamepad {
    pub index: u8,
    /// Bit ke-n menyala = gamepad n masih tersambung di client.
    pub active_mask: u16,
    pub buttons: u32,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub left_stick: (i16, i16),
    pub right_stick: (i16, i16),
}

/// Satu kejadian input yang sudah dipilah dari paket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputEvent {
    /// `key`: kode tombol virtual Windows (0..=255).
    Key { down: bool, key: u8, modifiers: u8, flags: u8 },
    Text(String),
    MouseMoveRelative { dx: i16, dy: i16 },
    /// Posisi di dalam bidang acuan client berukuran `(width, height)` (keduanya sudah dikurangi 1 oleh client).
    MouseMoveAbsolute { x: i16, y: i16, width: i16, height: i16 },
    /// `button`: 1 kiri, 2 tengah, 3 kanan, 4 X1, 5 X2.
    MouseButton { down: bool, button: u8 },
    /// 120 = satu takik roda.
    Scroll(i16),
    HorizontalScroll(i16),
    Gamepad(Gamepad),
    /// Paket sah yang belum ditangani (magic-nya).
    Other(u32),
}

fn packet(magic: u32, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_SIZE + body.len());
    out.extend_from_slice(&((4 + body.len()) as u32).to_be_bytes());
    out.extend_from_slice(&magic.to_le_bytes());
    out.extend_from_slice(body);
    out
}

/// Pilah satu paket input jadi `(magic, isi)`; ukuran di header harus persis sama dengan buffer.
pub fn parse(buf: &[u8]) -> Result<(u32, &[u8]), WireError> {
    need(buf, HEADER_SIZE)?;
    let size = u32_be(&buf[0..4]) as usize;
    if size < 4 || size != buf.len() - 4 {
        return Err(WireError::Invalid("ukuran di header input tidak cocok dengan paket"));
    }
    Ok((u32_le(&buf[4..8]), &buf[HEADER_SIZE..]))
}

/// Pilah paket input jadi kejadian. Paket dengan isi terlalu pendek untuk jenisnya ditolak.
pub fn decode(buf: &[u8]) -> Result<InputEvent, WireError> {
    let (magic, body) = parse(buf)?;
    let i16_be = |at: usize| u16_be(&body[at..at + 2]) as i16;
    let i16_le = |at: usize| u16_le(&body[at..at + 2]) as i16;
    Ok(match magic {
        magic::KEY_DOWN | magic::KEY_UP => {
            need(body, 6)?;
            // Byte tinggi kode tombol hanya penanda dari client (0x80); kode virtualnya di byte rendah.
            InputEvent::Key { down: magic == magic::KEY_DOWN, key: body[1], modifiers: body[3], flags: body[0] }
        }
        magic::UTF8_TEXT => {
            if body.is_empty() || body.len() > MAX_TEXT_BYTES {
                return Err(WireError::Invalid("panjang teks di luar 1..=32 byte"));
            }
            InputEvent::Text(std::str::from_utf8(body).map_err(|_| WireError::Invalid("teks bukan UTF-8"))?.to_string())
        }
        magic::MOUSE_MOVE_REL => {
            need(body, 4)?;
            InputEvent::MouseMoveRelative { dx: i16_be(0), dy: i16_be(2) }
        }
        magic::MOUSE_MOVE_ABS => {
            need(body, 10)?;
            InputEvent::MouseMoveAbsolute { x: i16_be(0), y: i16_be(2), width: i16_be(6), height: i16_be(8) }
        }
        magic::MOUSE_BUTTON_DOWN | magic::MOUSE_BUTTON_UP => {
            need(body, 1)?;
            InputEvent::MouseButton { down: magic == magic::MOUSE_BUTTON_DOWN, button: body[0] }
        }
        magic::SCROLL => {
            need(body, 6)?;
            InputEvent::Scroll(i16_be(0))
        }
        magic::SS_HSCROLL => {
            need(body, 2)?;
            InputEvent::HorizontalScroll(i16_be(0))
        }
        magic::MULTI_CONTROLLER => {
            need(body, 26)?;
            let index = u16_le(&body[2..4]);
            if usize::from(index) >= MAX_GAMEPADS {
                return Err(WireError::Invalid("nomor gamepad di luar 0..16"));
            }
            InputEvent::Gamepad(Gamepad {
                index: index as u8,
                active_mask: u16_le(&body[4..6]),
                // 16 bit bawah di `buttonFlags`, 16 bit atas (ekstensi Sunshine) di `buttonFlags2`.
                buttons: u32::from(u16_le(&body[8..10])) | u32::from(u16_le(&body[22..24])) << 16,
                left_trigger: body[10],
                right_trigger: body[11],
                left_stick: (i16_le(12), i16_le(14)),
                right_stick: (i16_le(16), i16_le(18)),
            })
        }
        other => InputEvent::Other(other),
    })
}

/// Tombol keyboard: `flags` (ekstensi Sunshine), kode tombol virtual Windows (LE), `modifiers`.
pub fn keyboard(down: bool, key_code: u16, modifiers: u8, flags: u8) -> Vec<u8> {
    let code = key_code.to_le_bytes();
    packet(if down { magic::KEY_DOWN } else { magic::KEY_UP }, &[flags, code[0], code[1], modifiers, 0, 0])
}

/// Teks UTF-8 (paling banyak 32 byte per paket).
pub fn text(text: &str) -> Vec<u8> {
    packet(magic::UTF8_TEXT, text.as_bytes())
}

/// Gerak mouse relatif (mode game). Delta big-endian.
pub fn mouse_move_rel(delta_x: i16, delta_y: i16) -> Vec<u8> {
    let (x, y) = (delta_x.to_be_bytes(), delta_y.to_be_bytes());
    packet(magic::MOUSE_MOVE_REL, &[x[0], x[1], y[0], y[1]])
}

/// Posisi mouse absolut di bidang acuan `reference` (lebar, tinggi dalam piksel; dikirim dikurangi 1).
pub fn mouse_move_abs(x: i16, y: i16, reference: (i16, i16)) -> Vec<u8> {
    let mut body = [0u8; 10];
    body[0..2].copy_from_slice(&x.to_be_bytes());
    body[2..4].copy_from_slice(&y.to_be_bytes());
    body[6..8].copy_from_slice(&(reference.0 - 1).to_be_bytes());
    body[8..10].copy_from_slice(&(reference.1 - 1).to_be_bytes());
    packet(magic::MOUSE_MOVE_ABS, &body)
}

/// Tombol mouse: 1 kiri, 2 tengah, 3 kanan, 4 X1, 5 X2.
pub fn mouse_button(down: bool, button: u8) -> Vec<u8> {
    packet(if down { magic::MOUSE_BUTTON_DOWN } else { magic::MOUSE_BUTTON_UP }, &[button])
}

/// Scroll vertikal (120 = satu takik). Jumlah dikirim dua kali, big-endian, diikuti dua byte nol.
pub fn scroll(amount: i16) -> Vec<u8> {
    let a = amount.to_be_bytes();
    packet(magic::SCROLL, &[a[0], a[1], a[0], a[1], 0, 0])
}

/// Scroll datar (ekstensi Sunshine).
pub fn horizontal_scroll(amount: i16) -> Vec<u8> {
    packet(magic::SS_HSCROLL, &amount.to_be_bytes())
}

/// Keadaan gamepad (`NV_MULTI_CONTROLLER_PACKET`, 34 byte). Field little-endian.
pub fn gamepad(pad: &Gamepad) -> Vec<u8> {
    let mut body = [0u8; 26];
    body[0..2].copy_from_slice(&0x001Au16.to_le_bytes());
    body[2..4].copy_from_slice(&u16::from(pad.index).to_le_bytes());
    body[4..6].copy_from_slice(&pad.active_mask.to_le_bytes());
    body[6..8].copy_from_slice(&0x0014u16.to_le_bytes());
    body[8..10].copy_from_slice(&(pad.buttons as u16).to_le_bytes());
    body[10] = pad.left_trigger;
    body[11] = pad.right_trigger;
    body[12..14].copy_from_slice(&pad.left_stick.0.to_le_bytes());
    body[14..16].copy_from_slice(&pad.left_stick.1.to_le_bytes());
    body[16..18].copy_from_slice(&pad.right_stick.0.to_le_bytes());
    body[18..20].copy_from_slice(&pad.right_stick.1.to_le_bytes());
    body[20..22].copy_from_slice(&0x009Cu16.to_le_bytes());
    body[22..24].copy_from_slice(&((pad.buttons >> 16) as u16).to_le_bytes());
    body[24..26].copy_from_slice(&0x0055u16.to_le_bytes());
    packet(magic::MULTI_CONTROLLER, &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_packet_matches_c_struct() {
        // NV_KEYBOARD_PACKET = 14 byte, ukuran di header = 10.
        let wire = keyboard(true, 0x8041, 0x02, 0);
        assert_eq!(wire, [0, 0, 0, 10, 3, 0, 0, 0, 0, 0x41, 0x80, 0x02, 0, 0]);
        assert_eq!(decode(&wire), Ok(InputEvent::Key { down: true, key: 0x41, modifiers: 2, flags: 0 }));
        assert_eq!(decode(&keyboard(false, 0x800D, 0, KEY_FLAG_NON_NORMALIZED)), Ok(InputEvent::Key { down: false, key: 0x0D, modifiers: 0, flags: 1 }));
    }

    #[test]
    fn relative_mouse_deltas_are_big_endian() {
        // NV_REL_MOUSE_MOVE_PACKET = 12 byte, ukuran = 8.
        let wire = mouse_move_rel(-2, 258);
        assert_eq!(wire, [0, 0, 0, 8, 7, 0, 0, 0, 0xFF, 0xFE, 0x01, 0x02]);
        assert_eq!(decode(&wire), Ok(InputEvent::MouseMoveRelative { dx: -2, dy: 258 }));
    }

    #[test]
    fn absolute_mouse_carries_reference_size_minus_one() {
        // NV_ABS_MOUSE_MOVE_PACKET = 18 byte, ukuran = 14.
        let wire = mouse_move_abs(960, 540, (1920, 1080));
        assert_eq!(wire, [0, 0, 0, 14, 5, 0, 0, 0, 0x03, 0xC0, 0x02, 0x1C, 0, 0, 0x07, 0x7F, 0x04, 0x37]);
        assert_eq!(decode(&wire), Ok(InputEvent::MouseMoveAbsolute { x: 960, y: 540, width: 1919, height: 1079 }));
    }

    #[test]
    fn mouse_button_scroll_and_text() {
        assert_eq!(mouse_button(true, 1), [0, 0, 0, 5, 8, 0, 0, 0, 1]);
        assert_eq!(decode(&mouse_button(false, 3)), Ok(InputEvent::MouseButton { down: false, button: 3 }));
        assert_eq!(scroll(-120), [0, 0, 0, 10, 0x0A, 0, 0, 0, 0xFF, 0x88, 0xFF, 0x88, 0, 0]);
        assert_eq!(decode(&scroll(-120)), Ok(InputEvent::Scroll(-120)));
        assert_eq!(horizontal_scroll(240), [0, 0, 0, 6, 1, 0, 0, 0x55, 0, 0xF0]);
        assert_eq!(decode(&horizontal_scroll(240)), Ok(InputEvent::HorizontalScroll(240)));
        assert_eq!(decode(&text("hé")), Ok(InputEvent::Text("hé".into())));
        assert!(decode(&packet(magic::UTF8_TEXT, &[0xFF, 0xFE])).is_err(), "bukan UTF-8");
        assert!(decode(&packet(magic::UTF8_TEXT, &[b'a'; 33])).is_err(), "terlalu panjang");
    }

    #[test]
    fn gamepad_packet_matches_c_struct_and_round_trips() {
        let pad = Gamepad { index: 1, active_mask: 0b11, buttons: 0x0020_1001, left_trigger: 200, right_trigger: 7, left_stick: (-32768, 32767), right_stick: (1000, -1000) };
        let wire = gamepad(&pad);
        // NV_MULTI_CONTROLLER_PACKET = 34 byte, ukuran = 30, magic 0x0C.
        assert_eq!(wire.len(), 34);
        assert_eq!(&wire[..8], &[0, 0, 0, 30, 0x0C, 0, 0, 0]);
        assert_eq!(&wire[8..18], &[0x1A, 0, 1, 0, 3, 0, 0x14, 0, 0x01, 0x10], "headerB, nomor, mask, midB, buttonFlags");
        assert_eq!(&wire[28..34], &[0x9C, 0, 0x20, 0, 0x55, 0], "tailA, buttonFlags2, tailB");
        assert_eq!(decode(&wire), Ok(InputEvent::Gamepad(pad)));

        let mut bad = wire.clone();
        bad[10] = 16;
        assert!(decode(&bad).is_err(), "nomor gamepad 16 tidak sah");
    }

    #[test]
    fn unknown_and_malformed_packets() {
        assert_eq!(decode(&packet(magic::ENABLE_HAPTICS, &[1, 0])), Ok(InputEvent::Other(0x0D)));
        assert_eq!(decode(&packet(magic::SS_TOUCH, &[0; 32])), Ok(InputEvent::Other(0x5500_0002)));
        let mut wire = mouse_button(true, 1);
        wire.push(0);
        assert!(decode(&wire).is_err(), "ukuran tidak cocok");
        assert!(decode(&[0, 0, 0, 4, 8, 0, 0]).is_err());
        assert!(decode(&packet(magic::MOUSE_MOVE_REL, &[0, 1])).is_err(), "isi terlalu pendek");
        assert!(decode(&packet(magic::KEY_DOWN, &[0; 5])).is_err());
        assert!(decode(&packet(magic::MULTI_CONTROLLER, &[0; 25])).is_err());
    }
}
