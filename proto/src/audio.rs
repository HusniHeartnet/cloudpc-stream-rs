//! Paket audio: Opus di RTP (tipe 97), tiap 4 paket data diikuti 2 paket FEC Reed-Solomon (tipe 127)
//! yang membawa `AUDIO_FEC_HEADER` (`RtpAudioQueue.h`, audio broadcast di `Sunshine/src/stream.cpp`).

use crate::fec::encode_parity_with;
use crate::{need, u16_be, u32_be, WireError};

pub const DATA_SHARDS: usize = 4;
pub const FEC_SHARDS: usize = 2;
pub const TOTAL_SHARDS: usize = DATA_SHARDS + FEC_SHARDS;

pub const PACKET_TYPE_AUDIO: u8 = 97;
pub const PACKET_TYPE_FEC: u8 = 127;

pub const FEC_HEADER_SIZE: usize = 12;

/// Matriks paritas FEC audio. BUKAN matriks Cauchy nanors: NVIDIA memakai matriks lain untuk audio, dan Sunshine
/// maupun Moonlight menimpa matriks nanors dengan nilai tetap ini (`audioBroadcastThread`, `RtpAudioQueue.c`).
pub const FEC_PARITY_MATRIX: [[u8; DATA_SHARDS]; FEC_SHARDS] = [[0x77, 0x40, 0x38, 0x0e], [0xc7, 0xa7, 0x0d, 0x6c]];

/// Dua shard paritas untuk satu blok berisi empat payload audio yang sama panjang. `None` bila panjangnya berbeda.
pub fn fec_parity(data: [&[u8]; DATA_SHARDS]) -> Option<[Vec<u8>; FEC_SHARDS]> {
    let mut parity = [vec![0u8; data[0].len()], vec![0u8; data[0].len()]];
    encode_parity_with(&data, &mut parity, |row, column| FEC_PARITY_MATRIX[row][column]).then_some(parity)
}

/// IV AES-CBC satu paket audio: `rikeyid + nomor urut` (32 bit, big-endian) di 4 byte pertama, sisanya nol.
pub fn audio_iv(key_id: u32, sequence_number: u16) -> [u8; 16] {
    let mut iv = [0u8; 16];
    iv[..4].copy_from_slice(&key_id.wrapping_add(u32::from(sequence_number)).to_be_bytes());
    iv
}

/// Header setelah RTP pada paket FEC audio. Field multi-byte big-endian.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFecHeader {
    /// 0 atau 1.
    pub fec_shard_index: u8,
    /// Tipe payload paket data yang dilindungi (97).
    pub payload_type: u8,
    /// Nomor urut paket data pertama di blok (kelipatan 4).
    pub base_sequence_number: u16,
    pub base_timestamp: u32,
    pub ssrc: u32,
}

impl AudioFecHeader {
    pub fn write(&self, out: &mut [u8]) -> Result<(), WireError> {
        need(out, FEC_HEADER_SIZE)?;
        out[0] = self.fec_shard_index;
        out[1] = self.payload_type;
        out[2..4].copy_from_slice(&self.base_sequence_number.to_be_bytes());
        out[4..8].copy_from_slice(&self.base_timestamp.to_be_bytes());
        out[8..12].copy_from_slice(&self.ssrc.to_be_bytes());
        Ok(())
    }

    pub fn read(buf: &[u8]) -> Result<AudioFecHeader, WireError> {
        need(buf, FEC_HEADER_SIZE)?;
        if usize::from(buf[0]) >= FEC_SHARDS {
            return Err(WireError::Invalid("indeks shard FEC audio di luar 0..2"));
        }
        Ok(AudioFecHeader {
            fec_shard_index: buf[0],
            payload_type: buf[1],
            base_sequence_number: u16_be(&buf[2..4]),
            base_timestamp: u32_be(&buf[4..8]),
            ssrc: u32_be(&buf[8..12]),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fec_header_round_trip() {
        let h = AudioFecHeader { fec_shard_index: 1, payload_type: PACKET_TYPE_AUDIO, base_sequence_number: 0x0104, base_timestamp: 480, ssrc: 0 };
        let mut buf = [0u8; FEC_HEADER_SIZE];
        h.write(&mut buf).unwrap();
        assert_eq!(buf, [1, 97, 0x01, 0x04, 0, 0, 0x01, 0xE0, 0, 0, 0, 0]);
        assert_eq!(AudioFecHeader::read(&buf).unwrap(), h);
    }

    #[test]
    fn iv_adds_sequence_to_key_id_with_wraparound() {
        assert_eq!(audio_iv(0x0102_0304, 2)[..4], [1, 2, 3, 6]);
        assert_eq!(audio_iv(u32::MAX, 2)[..4], [0, 0, 0, 1], "penjumlahan melingkar seperti di client");
        assert_eq!(audio_iv(7, 0)[4..], [0; 12]);
    }

    #[test]
    fn parity_uses_the_fixed_matrix_and_recovers_a_lost_packet() {
        use crate::fec::{gf_inv, gf_mul};
        // Satu shard berisi 1 di tiap kolom: paritasnya adalah koefisien matriks itu sendiri.
        let unit = fec_parity([&[1, 0, 0, 0], &[0, 1, 0, 0], &[0, 0, 1, 0], &[0, 0, 0, 1]]).unwrap();
        assert_eq!(unit, [vec![0x77, 0x40, 0x38, 0x0e], vec![0xc7, 0xa7, 0x0d, 0x6c]]);

        let data: [Vec<u8>; 4] = std::array::from_fn(|i| (0..40u8).map(|b| b.wrapping_mul(13).wrapping_add(i as u8 * 71)).collect());
        let parity = fec_parity([&data[0], &data[1], &data[2], &data[3]]).unwrap();
        for lost in 0..4 {
            for row in 0..2 {
                let recovered: Vec<u8> = (0..40)
                    .map(|byte| {
                        let rest = (0..4).filter(|i| *i != lost).fold(parity[row][byte], |acc, i| acc ^ gf_mul(FEC_PARITY_MATRIX[row][i], data[i][byte]));
                        gf_mul(gf_inv(FEC_PARITY_MATRIX[row][lost]), rest)
                    })
                    .collect();
                assert_eq!(recovered, data[lost], "paket {lost} dari paritas {row}");
            }
        }
        assert_eq!(fec_parity([&[1, 2], &[1], &[1, 2], &[1, 2]]), None, "panjang berbeda");
    }

    #[test]
    fn shard_index_out_of_range_is_rejected() {
        let mut buf = [0u8; FEC_HEADER_SIZE];
        buf[0] = 2;
        assert!(AudioFecHeader::read(&buf).is_err());
    }
}
