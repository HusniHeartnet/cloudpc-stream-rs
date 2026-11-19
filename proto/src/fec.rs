//! Reed-Solomon untuk FEC video, byte-per-byte sama dengan `nanors` (`rs.c`) yang dipakai Moonlight dan Sunshine.
//!
//! Medan GF(2^8) dengan polinomial 285 (0x11d). Matriks paritas Cauchy: baris `j`, kolom `i` berisi
//! `inv((jumlah_paritas + i) ^ j)`. Shard paritas ke-`j` = jumlah (XOR) tiap shard data dikali koefisiennya.
//! Host membuat paritas (`encode_parity`); client memulihkan shard data yang hilang (`reconstruct`).

/// Batas nanors: data + paritas paling banyak 255 shard.
pub const MAX_SHARDS: usize = 255;

struct Field {
    exp: [u8; 512],
    log: [u8; 256],
}

const fn build_field() -> Field {
    let mut exp = [0u8; 512];
    let mut log = [0u8; 256];
    let mut x: u16 = 1;
    let mut i = 0;
    while i < 255 {
        exp[i] = x as u8;
        log[x as usize] = i as u8;
        x <<= 1;
        if x & 0x100 != 0 {
            x ^= 0x11d;
        }
        i += 1;
    }
    // Digandakan supaya exp[log a + log b] tidak perlu modulo.
    while i < 512 {
        exp[i] = exp[i - 255];
        i += 1;
    }
    Field { exp, log }
}

static FIELD: Field = build_field();

pub fn gf_mul(a: u8, b: u8) -> u8 {
    if a == 0 || b == 0 {
        return 0;
    }
    FIELD.exp[usize::from(FIELD.log[usize::from(a)]) + usize::from(FIELD.log[usize::from(b)])]
}

/// Invers perkalian; `gf_inv(0)` = 0 seperti tabel nanors.
pub fn gf_inv(a: u8) -> u8 {
    if a == 0 {
        return 0;
    }
    FIELD.exp[255 - usize::from(FIELD.log[usize::from(a)])]
}

/// Koefisien matriks paritas nanors untuk `(baris paritas, kolom data)`.
pub fn parity_coefficient(parity_shards: usize, parity_row: usize, data_column: usize) -> u8 {
    gf_inv(((parity_shards + data_column) ^ parity_row) as u8)
}

/// Isi `parity` (tiap shard sepanjang shard data) dari `data`. Semua shard harus sama panjang.
/// Mengembalikan `false` tanpa menulis apa pun bila jumlah atau ukuran shard tidak sah.
pub fn encode_parity(data: &[&[u8]], parity: &mut [Vec<u8>]) -> bool {
    let parity_shards = parity.len();
    encode_parity_with(data, parity, |row, column| parity_coefficient(parity_shards, row, column))
}

/// Seperti `encode_parity`, dengan koefisien `(baris paritas, kolom data)` dari pemanggil.
pub fn encode_parity_with(data: &[&[u8]], parity: &mut [Vec<u8>], coefficient: impl Fn(usize, usize) -> u8) -> bool {
    let (ds, ps) = (data.len(), parity.len());
    if ds == 0 || ps == 0 || ds + ps > MAX_SHARDS {
        return false;
    }
    let size = data[0].len();
    if data.iter().any(|d| d.len() != size) || parity.iter().any(|p| p.len() != size) {
        return false;
    }
    for (row, out) in parity.iter_mut().enumerate() {
        out.fill(0);
        for (column, shard) in data.iter().enumerate() {
            let factor = coefficient(row, column);
            // Tabel perkalian satu koefisien: 256 entri, jauh lebih cepat daripada log/exp per byte.
            let mut table = [0u8; 256];
            for (value, entry) in table.iter_mut().enumerate() {
                *entry = gf_mul(factor, value as u8);
            }
            for (o, s) in out.iter_mut().zip(shard.iter()) {
                *o ^= table[usize::from(*s)];
            }
        }
    }
    true
}

/// Pulihkan shard data yang hilang. `shards` berisi `data_shards` shard data lalu shard paritas, berurutan seperti
/// dikirim host (`None` = tidak tiba); semua shard yang ada harus sama panjang. Mengembalikan `false` tanpa mengubah
/// apa pun bila shard yang tiba kurang dari `data_shards` atau bentuknya tidak sah. Shard paritas yang hilang tidak
/// dibuat ulang (client tidak membutuhkannya).
///
/// Cara kerja: tiap shard yang tiba adalah satu baris matriks pembangkit (identitas untuk data, Cauchy untuk paritas)
/// dikali vektor shard data. Ambil `data_shards` baris yang tiba, balik matriks itu (Gauss-Jordan di GF(2^8)), lalu
/// shard data yang hilang = baris inversnya dikali shard yang tiba. Matriks Cauchy menjamin pilihan baris mana pun
/// bisa dibalik, jadi hasilnya sama persis dengan dekoder nanors.
pub fn reconstruct(shards: &mut [Option<Vec<u8>>], data_shards: usize) -> bool {
    let parity_shards = shards.len().saturating_sub(data_shards);
    reconstruct_with(shards, data_shards, |row, column| parity_coefficient(parity_shards, row, column))
}

/// Seperti `reconstruct`, dengan koefisien paritas `(baris paritas, kolom data)` dari pemanggil (FEC audio memakai
/// matriks tetap NVIDIA, bukan Cauchy). Koefisien harus membentuk kode MDS supaya pilihan baris mana pun bisa dibalik.
pub fn reconstruct_with(shards: &mut [Option<Vec<u8>>], data_shards: usize, coefficient: impl Fn(usize, usize) -> u8) -> bool {
    let total = shards.len();
    if data_shards == 0 || data_shards > total || total > MAX_SHARDS {
        return false;
    }
    let missing: Vec<usize> = (0..data_shards).filter(|&i| shards[i].is_none()).collect();
    if missing.is_empty() {
        return true;
    }
    let Some(size) = shards.iter().flatten().map(Vec::len).next() else { return false };
    if shards.iter().flatten().any(|s| s.len() != size) {
        return false;
    }
    // Baris yang dipakai: semua shard data yang tiba, lalu paritas secukupnya.
    let rows: Vec<usize> = (0..total).filter(|&i| shards[i].is_some()).take(data_shards).collect();
    if rows.len() < data_shards {
        return false;
    }
    let generator_row = |row: usize| -> Vec<u8> {
        (0..data_shards)
            .map(|column| if row < data_shards { u8::from(row == column) } else { coefficient(row - data_shards, column) })
            .collect()
    };
    let mut matrix: Vec<Vec<u8>> = rows.iter().map(|&row| generator_row(row)).collect();
    let Some(inverse) = invert(&mut matrix) else { return false };

    let mut recovered = Vec::with_capacity(missing.len());
    for &lost in &missing {
        let mut out = vec![0u8; size];
        for (k, &row) in rows.iter().enumerate() {
            let factor = inverse[lost][k];
            if factor == 0 {
                continue;
            }
            let mut table = [0u8; 256];
            for (value, entry) in table.iter_mut().enumerate() {
                *entry = gf_mul(factor, value as u8);
            }
            for (o, s) in out.iter_mut().zip(shards[row].as_ref().expect("baris dipilih dari shard yang tiba")) {
                *o ^= table[usize::from(*s)];
            }
        }
        recovered.push(out);
    }
    for (lost, shard) in missing.into_iter().zip(recovered) {
        shards[lost] = Some(shard);
    }
    true
}

/// Invers matriks persegi di GF(2^8) dengan eliminasi Gauss-Jordan; `None` bila singular.
#[allow(clippy::needless_range_loop)] // indeks baris/kolom matriks lebih jelas daripada iterator
fn invert(matrix: &mut [Vec<u8>]) -> Option<Vec<Vec<u8>>> {
    let n = matrix.len();
    let mut inverse: Vec<Vec<u8>> = (0..n).map(|r| (0..n).map(|c| u8::from(r == c)).collect()).collect();
    for column in 0..n {
        let pivot = (column..n).find(|&r| matrix[r][column] != 0)?;
        matrix.swap(column, pivot);
        inverse.swap(column, pivot);
        let scale = gf_inv(matrix[column][column]);
        for c in 0..n {
            matrix[column][c] = gf_mul(matrix[column][c], scale);
            inverse[column][c] = gf_mul(inverse[column][c], scale);
        }
        for r in 0..n {
            let factor = matrix[r][column];
            if r == column || factor == 0 {
                continue;
            }
            for c in 0..n {
                matrix[r][c] ^= gf_mul(factor, matrix[column][c]);
                inverse[r][c] ^= gf_mul(factor, inverse[column][c]);
            }
        }
    }
    Some(inverse)
}

#[cfg(test)]
#[allow(clippy::needless_range_loop)] // indeks baris/kolom matriks lebih jelas daripada iterator
mod tests {
    use super::*;

    #[test]
    fn field_matches_nanors_tables() {
        // Baris pertama GF2_8_EXP dan GF2_8_INV di nanors/deps/obl/gf2_8_tables.h (polinomial 285).
        let exp: Vec<u8> = (0..16).map(|i| FIELD.exp[i]).collect();
        assert_eq!(exp, [1, 2, 4, 8, 16, 32, 64, 128, 29, 58, 116, 232, 205, 135, 19, 38]);
        let inv: Vec<u8> = (0..16).map(|i| gf_inv(i as u8)).collect();
        assert_eq!(inv, [0, 1, 142, 244, 71, 167, 122, 186, 173, 157, 221, 152, 61, 170, 93, 150]);
        assert_eq!((gf_inv(16), gf_inv(17), gf_inv(31)), (216, 114, 42));
    }

    #[test]
    fn multiplication_and_inverse_are_consistent() {
        for a in 1..=255u8 {
            assert_eq!(gf_mul(a, gf_inv(a)), 1, "a = {a}");
            assert_eq!(gf_mul(a, 1), a);
            assert_eq!(gf_mul(a, 0), 0);
        }
        // Distributif terhadap XOR.
        assert_eq!(gf_mul(0x57, 0x13 ^ 0x83), gf_mul(0x57, 0x13) ^ gf_mul(0x57, 0x83));
    }

    #[test]
    fn parity_is_the_cauchy_combination_of_data() {
        let data: [&[u8]; 3] = [&[1, 0, 7], &[0, 1, 9], &[0, 0, 200]];
        let mut parity = vec![vec![0xEEu8; 3]; 2];
        assert!(encode_parity(&data, &mut parity));
        // Kolom 0 dan 1 hanya berisi satu shard bernilai 1, jadi paritasnya = koefisien itu sendiri.
        for row in 0..2 {
            assert_eq!(parity[row][0], gf_inv((2 ^ row) as u8));
            assert_eq!(parity[row][1], gf_inv((3 ^ row) as u8));
            let expected = gf_mul(parity_coefficient(2, row, 0), 7) ^ gf_mul(parity_coefficient(2, row, 1), 9) ^ gf_mul(parity_coefficient(2, row, 2), 200);
            assert_eq!(parity[row][2], expected);
        }
    }

    #[test]
    fn any_lost_data_shard_is_recoverable_from_one_parity_shard() {
        // Dengan satu shard hilang: data_x = inv(c_x) * (paritas ^ jumlah c_i * data_i untuk i != x).
        let data: Vec<Vec<u8>> = (0..5u8).map(|i| (0..32u8).map(|b| b.wrapping_mul(31).wrapping_add(i * 17)).collect()).collect();
        let refs: Vec<&[u8]> = data.iter().map(Vec::as_slice).collect();
        let mut parity = vec![vec![0u8; 32]; 2];
        assert!(encode_parity(&refs, &mut parity));
        for lost in 0..5 {
            for row in 0..2 {
                let recovered: Vec<u8> = (0..32)
                    .map(|byte| {
                        let mut acc = parity[row][byte];
                        for (i, shard) in data.iter().enumerate().filter(|(i, _)| *i != lost) {
                            acc ^= gf_mul(parity_coefficient(2, row, i), shard[byte]);
                        }
                        gf_mul(gf_inv(parity_coefficient(2, row, lost)), acc)
                    })
                    .collect();
                assert_eq!(recovered, data[lost], "shard {lost} dari paritas {row}");
            }
        }
    }

    fn encoded(data_shards: usize, parity_shards: usize, size: usize) -> Vec<Vec<u8>> {
        let data: Vec<Vec<u8>> = (0..data_shards).map(|d| (0..size).map(|b| (d * 31 + b * 7 + (b * d) / 5) as u8).collect()).collect();
        let mut parity = vec![vec![0u8; size]; parity_shards];
        let refs: Vec<&[u8]> = data.iter().map(Vec::as_slice).collect();
        assert!(encode_parity(&refs, &mut parity));
        data.into_iter().chain(parity).collect()
    }

    #[test]
    fn reconstruct_recovers_any_loss_up_to_the_parity_count() {
        // Bentuk yang dipakai Sunshine: blok kecil, blok besar, dan paritas minimum di frame kecil.
        for (ds, ps) in [(1, 2), (3, 2), (10, 2), (40, 8), (100, 20), (212, 43)] {
            let original = encoded(ds, ps, 64);
            let n = ps.min(ds);
            // Hilangkan data di depan, di belakang, dan berselang; pakai seluruh paritas.
            let patterns: Vec<Vec<usize>> = vec![(0..n).collect(), (ds - n..ds).collect(), (0..ds).step_by(2).take(ps).collect()];
            for lost in patterns {
                let mut shards: Vec<Option<Vec<u8>>> = original.iter().cloned().map(Some).collect();
                for &i in &lost {
                    shards[i] = None;
                }
                assert!(reconstruct(&mut shards, ds), "{ds}+{ps} kehilangan {lost:?}");
                for i in 0..ds {
                    assert_eq!(shards[i].as_ref(), Some(&original[i]), "{ds}+{ps} shard {i}");
                }
            }
        }
    }

    #[test]
    fn reconstruct_mixes_lost_data_and_lost_parity() {
        let original = encoded(6, 3, 32);
        let mut shards: Vec<Option<Vec<u8>>> = original.iter().cloned().map(Some).collect();
        shards[1] = None;
        shards[4] = None;
        shards[6] = None; // paritas pertama juga hilang
        assert!(reconstruct(&mut shards, 6));
        assert_eq!(shards[1].as_ref(), Some(&original[1]));
        assert_eq!(shards[4].as_ref(), Some(&original[4]));
        assert!(shards[6].is_none(), "paritas tidak dibuat ulang");
    }

    #[test]
    fn reconstruct_refuses_when_too_much_is_missing() {
        let original = encoded(5, 2, 16);
        let mut shards: Vec<Option<Vec<u8>>> = original.iter().cloned().map(Some).collect();
        for i in [0, 2, 3] {
            shards[i] = None;
        }
        let before = shards.clone();
        assert!(!reconstruct(&mut shards, 5));
        assert_eq!(shards, before, "tidak ada yang diubah");
        let mut uneven = vec![Some(vec![1u8; 4]), None, Some(vec![2u8; 5])];
        assert!(!reconstruct(&mut uneven, 2), "panjang shard berbeda");
        assert!(reconstruct(&mut [Some(vec![1u8])], 1), "tidak ada yang hilang");
    }

    #[test]
    fn invalid_shapes_are_refused() {
        let a = [1u8, 2];
        let b = [1u8];
        assert!(!encode_parity(&[&a, &b], &mut [vec![0; 2]]), "panjang shard berbeda");
        assert!(!encode_parity(&[&a], &mut [vec![0; 3]]), "panjang paritas berbeda");
        assert!(!encode_parity(&[], &mut [vec![0; 2]]));
        let many: Vec<&[u8]> = vec![&a; 250];
        assert!(!encode_parity(&many, &mut vec![vec![0; 2]; 6]), "lebih dari 255 shard");
    }
}
