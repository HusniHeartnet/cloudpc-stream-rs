//! Bidik dengan memiringkan HP: kecepatan putar giroskop -> gerak mouse relatif. Jempol untuk putaran besar, gyro untuk
//! koreksi halus (seperti bidikan gyro di game tembak-menembak HP).
//!
//! Masukan sudah dalam sumbu LAYAR (sisi Kotlin yang memetakan sumbu sensor menurut rotasi layar):
//! - `yaw`  : kecepatan putar mengelilingi sumbu atas layar, rad/dtk; positif = HP diputar ke KIRI (berlawanan jarum jam
//!   dilihat dari atas);
//! - `pitch`: kecepatan putar mengelilingi sumbu kanan layar, rad/dtk; positif = tepi atas HP mendekat ke pemain
//!   (punggung HP menghadap ke atas = melihat ke atas).
//!
//! Murni data, tanpa Android.

/// Hitungan mouse per radian putaran HP. ±1200 sedikit di bawah 1:1 untuk sensitivitas mouse game yang umum, supaya
/// gyro terasa sebagai penghalus, bukan pengganti jempol.
pub const COUNTS_PER_RADIAN: f32 = 1200.0;
/// Putaran lebih pelan dari ini (rad/dtk) dianggap getaran tangan/derau sensor.
const DEAD_ZONE: f32 = 0.012;
/// Jeda antar sampel dibatasi: sampel pertama setelah sensor berhenti sebentar tidak boleh jadi lompatan besar.
const MAX_STEP_SECONDS: f32 = 0.05;

#[derive(Debug, Default)]
pub struct Gyro {
    carry: (f32, f32),
}

impl Gyro {
    /// Satu sampel sensor -> gerak mouse (dx, dy) dalam hitungan, atau `None` bila belum cukup untuk satu hitungan.
    /// `invert` membalik kedua arah (untuk HP yang melaporkan rotasi layar sebaliknya).
    pub fn update(&mut self, yaw: f32, pitch: f32, seconds: f32, invert: bool) -> Option<(i32, i32)> {
        if !(yaw.is_finite() && pitch.is_finite() && seconds.is_finite()) || seconds <= 0.0 {
            return None;
        }
        let step = seconds.min(MAX_STEP_SECONDS);
        let quiet = |v: f32| if v.abs() < DEAD_ZONE { 0.0 } else { v };
        let sign = if invert { 1.0 } else { -1.0 };
        // Putar ke kiri = kamera ke kiri (dx negatif); punggung HP ke atas = kamera ke atas (dy negatif).
        let fx = sign * quiet(yaw) * step * COUNTS_PER_RADIAN + self.carry.0;
        let fy = sign * quiet(pitch) * step * COUNTS_PER_RADIAN + self.carry.1;
        let (dx, dy) = (fx.trunc(), fy.trunc());
        self.carry = (fx - dx, fy - dy);
        (dx != 0.0 || dy != 0.0).then_some((dx as i32, dy as i32))
    }

    /// Buang sisa pecahan (gyro dimatikan/dinyalakan lagi, toko dibuka).
    pub fn reset(&mut self) {
        self.carry = (0.0, 0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turning_left_and_tilting_back_move_the_camera_left_and_up() {
        let mut g = Gyro::default();
        // 0,5 rad/dtk selama 10 ms = 0,005 rad = 6 hitungan.
        assert_eq!(g.update(0.5, 0.0, 0.01, false), Some((-6, 0)));
        assert_eq!(g.update(0.0, 0.5, 0.01, false), Some((0, -6)));
        assert_eq!(g.update(-0.5, -0.25, 0.01, false), Some((6, 3)));
        assert_eq!(g.update(0.5, 0.5, 0.01, true), Some((6, 6)));
    }

    #[test]
    fn hand_tremor_is_ignored_and_slow_turns_accumulate() {
        let mut g = Gyro::default();
        for _ in 0..200 {
            assert_eq!(g.update(0.01, -0.011, 0.005, false), None); // di bawah zona mati
        }
        // 0,05 rad/dtk x 5 ms = 0,3 hitungan per sampel: tiap ±4 sampel jadi satu hitungan, tidak ada yang hilang.
        let mut total = 0;
        for _ in 0..200 {
            if let Some((dx, _)) = g.update(-0.05, 0.0, 0.005, false) {
                total += dx;
            }
        }
        assert_eq!(total, 60);
    }

    #[test]
    fn a_gap_in_samples_is_not_a_jump_and_bad_numbers_are_dropped() {
        let mut g = Gyro::default();
        // Jeda 2 detik diperlakukan sebagai 50 ms.
        assert_eq!(g.update(1.0, 0.0, 2.0, false), Some((-60, 0)));
        assert_eq!(g.update(f32::NAN, 0.0, 0.01, false), None);
        assert_eq!(g.update(1.0, 0.0, 0.0, false), None);
        assert_eq!(g.update(1.0, 0.0, -1.0, false), None);
    }
}
