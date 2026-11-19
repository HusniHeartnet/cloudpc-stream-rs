//! Bentuk kursor dari Desktop Duplication -> gambar BGRA yang bisa digambar GPU.
//!
//! Windows punya tiga jenis bentuk kursor. Dua di antaranya bisa berisi piksel "balik warna layar" (mis. kursor
//! teks I), yang tidak bisa dinyatakan dengan alpha biasa. Karena itu hasilnya dua lapis, seperti di
//! `Sunshine/src/platform/windows/display_vram.cpp`:
//! - `alpha`: digambar dengan alpha blending biasa;
//! - `invert`: digambar dengan blending pembalik (putih = balik warna layar, hitam = biarkan).

/// Nilai `DXGI_OUTDUPL_POINTER_SHAPE_TYPE`.
pub const SHAPE_MONOCHROME: u32 = 1;
pub const SHAPE_COLOR: u32 = 2;
pub const SHAPE_MASKED_COLOR: u32 = 4;

/// Batas ukuran kursor yang diterima (kursor Windows terbesar 256x256).
const MAX_SIDE: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorImage {
    pub width: u32,
    pub height: u32,
    /// BGRA, alpha lurus, `width * height * 4` byte.
    pub alpha: Vec<u8>,
    /// BGRA untuk lapis pembalik; `None` bila bentuk ini tidak punya piksel pembalik.
    pub invert: Option<Vec<u8>>,
}

/// `None` bila ukuran atau panjang data tidak masuk akal (data dari driver tetap diperlakukan tidak tepercaya).
pub fn convert(shape_type: u32, width: u32, height: u32, pitch: u32, data: &[u8]) -> Option<CursorImage> {
    let (width, pitch) = (width as usize, pitch as usize);
    // Monokrom menyimpan dua topeng bertumpuk, jadi tinggi gambarnya separuh.
    let rows = if shape_type == SHAPE_MONOCHROME { height as usize / 2 } else { height as usize };
    if width == 0 || rows == 0 || width > MAX_SIDE || rows > MAX_SIDE {
        return None;
    }
    let mut alpha = vec![0u8; width * rows * 4];
    let mut invert = vec![0u8; width * rows * 4];
    let mut any_invert = false;

    match shape_type {
        SHAPE_MONOCHROME => {
            if pitch < width.div_ceil(8) || data.len() < pitch * rows * 2 {
                return None;
            }
            for y in 0..rows {
                for x in 0..width {
                    let bit = |row: usize| data[row * pitch + x / 8] >> (7 - x % 8) & 1;
                    let (and, xor) = (bit(y), bit(y + rows));
                    let at = (y * width + x) * 4;
                    match (and, xor) {
                        (0, 0) => alpha[at..at + 4].copy_from_slice(&[0, 0, 0, 255]),
                        (0, _) => alpha[at..at + 4].copy_from_slice(&[255, 255, 255, 255]),
                        (_, 0) => {}
                        _ => {
                            invert[at..at + 4].copy_from_slice(&[255, 255, 255, 255]);
                            any_invert = true;
                        }
                    }
                }
            }
        }
        SHAPE_COLOR | SHAPE_MASKED_COLOR => {
            if pitch < width * 4 || data.len() < pitch * (rows - 1) + width * 4 {
                return None;
            }
            for y in 0..rows {
                for x in 0..width {
                    let pixel = &data[y * pitch + x * 4..y * pitch + x * 4 + 4];
                    let at = (y * width + x) * 4;
                    if shape_type == SHAPE_COLOR {
                        alpha[at..at + 4].copy_from_slice(pixel);
                    } else if pixel[3] == 0 {
                        // Topeng 0: ganti piksel layar dengan warna ini.
                        alpha[at..at + 4].copy_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
                    } else if pixel[..3] != [0, 0, 0] {
                        // Topeng 0xFF: XOR dengan layar. Didekati dengan pembalikan (tepat untuk putih).
                        invert[at..at + 4].copy_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
                        any_invert = true;
                    }
                }
            }
        }
        _ => return None,
    }
    Some(CursorImage { width: width as u32, height: rows as u32, alpha, invert: any_invert.then_some(invert) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_cursor_is_passed_through_respecting_pitch() {
        // 2x2 dengan pitch 12 (4 byte isian per baris).
        let data = [1, 2, 3, 255, 4, 5, 6, 128, 9, 9, 9, 9, 7, 8, 9, 0, 10, 11, 12, 255, 9, 9, 9, 9];
        let image = convert(SHAPE_COLOR, 2, 2, 12, &data).unwrap();
        assert_eq!((image.width, image.height), (2, 2));
        assert_eq!(image.alpha, [1, 2, 3, 255, 4, 5, 6, 128, 7, 8, 9, 0, 10, 11, 12, 255]);
        assert_eq!(image.invert, None);
    }

    #[test]
    fn monochrome_masks_map_to_black_white_transparent_and_invert() {
        // Lebar 4, tinggi 1 (data berisi 2 baris: AND lalu XOR). Piksel: hitam, putih, tembus, balik.
        let image = convert(SHAPE_MONOCHROME, 4, 2, 1, &[0b0011_0000, 0b0101_0000]).unwrap();
        assert_eq!((image.width, image.height), (4, 1));
        assert_eq!(image.alpha, [0, 0, 0, 255, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(image.invert.unwrap(), [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255]);
    }

    #[test]
    fn masked_color_splits_replace_and_xor_pixels() {
        // Piksel 1: ganti dengan merah; piksel 2: XOR putih (balik); piksel 3: XOR hitam (tidak mengubah layar).
        let data = [0, 0, 255, 0, 255, 255, 255, 255, 0, 0, 0, 255];
        let image = convert(SHAPE_MASKED_COLOR, 3, 1, 12, &data).unwrap();
        assert_eq!(image.alpha, [0, 0, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(image.invert.unwrap(), [0, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0]);
    }

    #[test]
    fn truncated_or_absurd_shapes_are_refused() {
        assert_eq!(convert(SHAPE_COLOR, 2, 2, 8, &[0; 15]), None, "data kurang 1 byte");
        assert_eq!(convert(SHAPE_COLOR, 2, 2, 4, &[0; 64]), None, "pitch lebih kecil dari satu baris");
        assert_eq!(convert(SHAPE_MONOCHROME, 16, 2, 2, &[0; 3]), None);
        assert_eq!(convert(SHAPE_COLOR, 0, 4, 16, &[0; 64]), None);
        assert_eq!(convert(SHAPE_COLOR, 4096, 4096, 16384, &[0; 64]), None);
        assert_eq!(convert(9, 2, 2, 8, &[0; 16]), None, "jenis tidak dikenal");
    }
}
