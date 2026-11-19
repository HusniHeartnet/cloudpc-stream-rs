//! Fase H2. Frame terenkode -> shard berukuran sama + paritas Reed-Solomon -> datagram UDP (AES-GCM bila ada kunci).
//!
//! Mengikuti video broadcast dan `fec::encode` di `Sunshine/src/stream.cpp` langkah demi langkah, termasuk
//! urutan pengisian header: sebagian field diisi SEBELUM paritas dihitung (ikut terlindungi FEC), sebagian
//! SESUDAHNYA di semua shard (client mengisi ulang sendiri pada shard yang dipulihkan).

use cloudpc_stream_proto::fec::{encode_parity, MAX_SHARDS};
use cloudpc_stream_proto::video::{
    pack_multi_fec_blocks, video_iv, FecInfo, FrameType, ShortFrameHeader, ENC_VIDEO_PREFIX_SIZE, FLAG_CONTAINS_PIC_DATA, FLAG_EOF, FLAG_SOF, MAX_FEC_BLOCKS, MULTI_FEC_FLAGS,
    SHORT_FRAME_HEADER_SIZE, VIDEO_PACKET_HEADER_SIZE,
};

use crate::crypto::gcm_encrypt;

/// Persentase paritas bawaan Sunshine.
pub const FEC_PERCENTAGE: usize = 20;
/// Field `fecInfo` hanya punya 10 bit untuk jumlah shard data per blok.
const MAX_DATA_SHARDS_PER_BLOCK: usize = 1023;

// Letak field di dalam header 32 byte: RTP (12) + cadangan (4) + NV_VIDEO_PACKET (16).
const RTP_HEADER: usize = 0;
const RTP_SEQUENCE: usize = 2;
const RTP_TIMESTAMP: usize = 4;
const NV_STREAM_PACKET_INDEX: usize = 16;
const NV_FRAME_INDEX: usize = 20;
const NV_FLAGS: usize = 24;
const NV_MULTI_FEC_FLAGS: usize = 26;
const NV_MULTI_FEC_BLOCKS: usize = 27;
const NV_FEC_INFO: usize = 28;

pub struct Frame<'a> {
    /// Nomor frame, mulai dari 1.
    pub index: u32,
    pub frame_type: FrameType,
    /// Waktu tangkap dalam satuan 1/90000 detik.
    pub timestamp: u32,
    /// Lama tangkap + encode di host, satuan 0,1 ms.
    pub processing_latency: u16,
    /// Bitstream encoder (Annex B untuk H.264/HEVC, OBU untuk AV1).
    pub data: &'a [u8],
}

pub struct Packetizer {
    /// Ukuran satu shard: `packetSize` dari client + 16.
    block_size: usize,
    min_parity_shards: usize,
    key: Option<[u8; 16]>,
    /// Nomor urut shard berikutnya (data dan paritas sama-sama dihitung).
    next_sequence: u32,
    /// Penghitung IV AES-GCM; tidak pernah diulang selama satu kunci dipakai.
    iv_counter: u64,
}

impl Packetizer {
    pub fn new(packet_size: u32, min_fec_packets: u32, key: Option<[u8; 16]>) -> Packetizer {
        Packetizer { block_size: packet_size as usize + 16, min_parity_shards: min_fec_packets as usize, key, next_sequence: 0, iv_counter: 0 }
    }

    /// Ubah satu frame jadi datagram siap kirim, berurutan. Kosong bila frame terlalu besar untuk protokol.
    pub fn packetize(&mut self, frame: &Frame) -> Vec<Vec<u8>> {
        let block_size = self.block_size;
        let payload_size = block_size - VIDEO_PACKET_HEADER_SIZE;

        // Header frame 8 byte + bitstream, dipotong per `payload_size`, tiap potongan diawali ruang header 32 byte.
        let total = SHORT_FRAME_HEADER_SIZE + frame.data.len();
        let last_payload_len = match total % payload_size {
            0 => payload_size,
            rest => rest,
        };
        let mut content = vec![0u8; total];
        ShortFrameHeader { frame_processing_latency: frame.processing_latency, frame_type: frame.frame_type, last_payload_len: last_payload_len as u16 }
            .write(&mut content)
            .expect("header frame 8 byte");
        content[SHORT_FRAME_HEADER_SIZE..].copy_from_slice(frame.data);
        let mut shards: Vec<Vec<u8>> = content
            .chunks(payload_size)
            .map(|chunk| {
                // Potongan terakhir diisi nol sampai penuh.
                let mut shard = vec![0u8; block_size];
                shard[VIDEO_PACKET_HEADER_SIZE..VIDEO_PACKET_HEADER_SIZE + chunk.len()].copy_from_slice(chunk);
                shard
            })
            .collect();

        // Bagi ke paling banyak 4 blok FEC; frame raksasa dikirim tanpa paritas.
        let mut percentage = FEC_PERCENTAGE;
        let max_data_per_block = MAX_SHARDS * 100 / (100 + percentage);
        let mut blocks = shards.len().div_ceil(max_data_per_block);
        if blocks > usize::from(MAX_FEC_BLOCKS) {
            percentage = 0;
            blocks = usize::from(MAX_FEC_BLOCKS);
        }
        let per_block = shards.len().div_ceil(blocks);
        if per_block > MAX_DATA_SHARDS_PER_BLOCK {
            return Vec::new();
        }

        let mut datagrams = Vec::with_capacity(shards.len() + shards.len() * percentage / 100 + 2 * blocks);
        let mut remaining = shards.as_mut_slice();
        for block_index in 0..blocks {
            let take = if block_index == blocks - 1 { remaining.len() } else { per_block.min(remaining.len()) };
            let (block, rest) = remaining.split_at_mut(take);
            remaining = rest;
            if block.is_empty() {
                continue;
            }
            let multi_fec = pack_multi_fec_blocks(block_index as u8, blocks as u8).expect("blok FEC 1..=4");
            self.emit_block(frame, block, multi_fec, percentage, &mut datagrams);
        }
        datagrams
    }

    fn emit_block(&mut self, frame: &Frame, data: &mut [Vec<u8>], multi_fec: u8, mut percentage: usize, out: &mut Vec<Vec<u8>>) {
        let data_shards = data.len();
        let first_sequence = self.next_sequence;

        // Tahap 1: field yang ikut dilindungi FEC.
        for (x, shard) in data.iter_mut().enumerate() {
            let mut flags = FLAG_CONTAINS_PIC_DATA;
            if x == 0 {
                flags |= FLAG_SOF;
            }
            if x == data_shards - 1 {
                flags |= FLAG_EOF;
            }
            shard[NV_STREAM_PACKET_INDEX..NV_STREAM_PACKET_INDEX + 4].copy_from_slice(&(first_sequence.wrapping_add(x as u32) << 8).to_le_bytes());
            shard[NV_FRAME_INDEX..NV_FRAME_INDEX + 4].copy_from_slice(&frame.index.to_le_bytes());
            shard[NV_FLAGS] = flags;
            shard[NV_MULTI_FEC_FLAGS] = MULTI_FEC_FLAGS;
            shard[NV_MULTI_FEC_BLOCKS] = multi_fec;
        }

        // Tahap 2: paritas atas seluruh shard (header + payload).
        let mut parity_shards = (data_shards * percentage).div_ceil(100);
        if percentage != 0 && parity_shards < self.min_parity_shards {
            parity_shards = self.min_parity_shards;
            percentage = 100 * parity_shards / data_shards;
        }
        let mut parity = vec![vec![0u8; self.block_size]; parity_shards];
        if parity_shards > 0 {
            let refs: Vec<&[u8]> = data.iter().map(Vec::as_slice).collect();
            if !encode_parity(&refs, &mut parity) {
                // Tidak muat di satu blok Reed-Solomon (min paritas terlalu besar): kirim tanpa paritas.
                parity.clear();
                percentage = 0;
            }
        }

        // Tahap 3: field yang diisi di semua shard setelah paritas dihitung, lalu enkripsi.
        for (x, shard) in data.iter_mut().chain(parity.iter_mut()).enumerate() {
            let sequence = first_sequence.wrapping_add(x as u32);
            shard[RTP_HEADER] = 0x90;
            shard[RTP_SEQUENCE..RTP_SEQUENCE + 2].copy_from_slice(&(sequence as u16).to_be_bytes());
            shard[RTP_TIMESTAMP..RTP_TIMESTAMP + 4].copy_from_slice(&frame.timestamp.to_be_bytes());
            shard[NV_FRAME_INDEX..NV_FRAME_INDEX + 4].copy_from_slice(&frame.index.to_le_bytes());
            shard[NV_MULTI_FEC_BLOCKS] = multi_fec;
            let fec_info = FecInfo { shard_index: x as u16, data_shards: data_shards as u16, percentage: percentage as u8 };
            shard[NV_FEC_INFO..NV_FEC_INFO + 4].copy_from_slice(&fec_info.pack().to_le_bytes());

            out.push(match &self.key {
                None => std::mem::take(shard),
                Some(key) => {
                    let iv = video_iv(self.iv_counter);
                    self.iv_counter += 1;
                    let (tag, ciphertext) = gcm_encrypt(key, &iv, shard);
                    let mut datagram = Vec::with_capacity(ENC_VIDEO_PREFIX_SIZE + ciphertext.len());
                    datagram.extend_from_slice(&iv);
                    datagram.extend_from_slice(&frame.index.to_le_bytes());
                    datagram.extend_from_slice(&tag);
                    datagram.extend_from_slice(&ciphertext);
                    datagram
                }
            });
        }
        self.next_sequence = first_sequence.wrapping_add((data_shards + parity.len()) as u32);
    }
}

#[cfg(test)]
#[allow(clippy::needless_range_loop)] // indeks byte/shard lebih jelas daripada iterator
mod tests {
    use super::*;
    use crate::crypto::gcm_decrypt;
    use cloudpc_stream_proto::fec::{gf_inv, gf_mul, parity_coefficient};
    use cloudpc_stream_proto::video::{unpack_multi_fec_blocks, VideoPacketHeader};

    const PACKET_SIZE: u32 = 1392;
    const PAYLOAD: usize = PACKET_SIZE as usize + 16 - VIDEO_PACKET_HEADER_SIZE;

    fn bitstream(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i * 7 + i / 251) as u8).collect()
    }

    fn frame(data: &[u8], index: u32) -> Frame<'_> {
        Frame { index, frame_type: FrameType::Idr, timestamp: 90_000, processing_latency: 23, data }
    }

    /// Susun ulang frame dari shard data saja, seperti client tanpa paket hilang.
    fn reassemble(datagrams: &[Vec<u8>]) -> (ShortFrameHeader, Vec<u8>) {
        let mut content = Vec::new();
        for datagram in datagrams {
            let header = VideoPacketHeader::read(datagram).unwrap();
            let info = FecInfo::unpack(header.nv.fec_info);
            if info.shard_index < info.data_shards {
                content.extend_from_slice(&datagram[VIDEO_PACKET_HEADER_SIZE..]);
            }
        }
        let header = ShortFrameHeader::read(&content).unwrap();
        // Buang isian nol di shard terakhir memakai lastPayloadLen.
        let shards = content.len() / PAYLOAD;
        content.truncate((shards - 1) * PAYLOAD + usize::from(header.last_payload_len));
        (header, content[SHORT_FRAME_HEADER_SIZE..].to_vec())
    }

    #[test]
    fn small_frame_layout_matches_protocol() {
        let data = bitstream(3000);
        let datagrams = Packetizer::new(PACKET_SIZE, 2, None).packetize(&frame(&data, 7));
        // 3008 byte isi -> 3 shard data; 20% -> 1 paritas, dinaikkan ke minimum 2 (persentase jadi 66).
        assert_eq!(datagrams.len(), 5);
        for (x, datagram) in datagrams.iter().enumerate() {
            assert_eq!(datagram.len(), PACKET_SIZE as usize + 16);
            let header = VideoPacketHeader::read(datagram).unwrap();
            assert_eq!((header.rtp.header, header.rtp.sequence_number, header.rtp.timestamp), (0x90, x as u16, 90_000));
            assert_eq!(header.nv.frame_index, 7);
            assert_eq!(FecInfo::unpack(header.nv.fec_info), FecInfo { shard_index: x as u16, data_shards: 3, percentage: 66 });
            assert_eq!(unpack_multi_fec_blocks(header.nv.multi_fec_blocks), (0, 1));
            if x < 3 {
                assert_eq!(header.nv.stream_packet_index, (x as u32) << 8);
                assert_eq!(header.nv.multi_fec_flags, MULTI_FEC_FLAGS);
                let expected = FLAG_CONTAINS_PIC_DATA | if x == 0 { FLAG_SOF } else { 0 } | if x == 2 { FLAG_EOF } else { 0 };
                assert_eq!(header.nv.flags, expected);
            }
        }
        let (header, payload) = reassemble(&datagrams);
        assert_eq!((header.frame_type, header.frame_processing_latency), (FrameType::Idr, 23));
        assert_eq!(usize::from(header.last_payload_len), 3008 - 2 * PAYLOAD);
        assert_eq!(payload, data);
    }

    #[test]
    fn exact_multiple_reports_full_last_payload() {
        let data = bitstream(2 * PAYLOAD - SHORT_FRAME_HEADER_SIZE);
        let datagrams = Packetizer::new(PACKET_SIZE, 0, None).packetize(&frame(&data, 1));
        let (header, payload) = reassemble(&datagrams);
        assert_eq!(usize::from(header.last_payload_len), PAYLOAD);
        assert_eq!(payload, data);
        assert_eq!(datagrams.len(), 3, "2 data + ceil(2 * 20%) paritas");
    }

    #[test]
    fn lost_shard_payload_is_recoverable_from_parity() {
        let data = bitstream(6000);
        let datagrams = Packetizer::new(PACKET_SIZE, 0, None).packetize(&frame(&data, 3));
        let info = FecInfo::unpack(VideoPacketHeader::read(&datagrams[0]).unwrap().nv.fec_info);
        let (ds, ps) = (usize::from(info.data_shards), datagrams.len() - usize::from(info.data_shards));
        assert_eq!((ds, ps), (5, 1));
        // Pulihkan shard 2 dari paritas 0; hanya daerah payload yang dibandingkan (header diisi ulang client).
        let lost = 2;
        for byte in VIDEO_PACKET_HEADER_SIZE..datagrams[0].len() {
            let mut acc = datagrams[ds][byte];
            for i in (0..ds).filter(|i| *i != lost) {
                acc ^= gf_mul(parity_coefficient(ps, 0, i), datagrams[i][byte]);
            }
            assert_eq!(gf_mul(gf_inv(parity_coefficient(ps, 0, lost)), acc), datagrams[lost][byte], "byte {byte}");
        }
        // Bendera SOF/EOF ikut terlindungi: byte flags shard yang hilang juga pulih.
        let mut acc = datagrams[ds][NV_FLAGS];
        for i in (0..ds).filter(|i| *i != lost) {
            acc ^= gf_mul(parity_coefficient(ps, 0, i), datagrams[i][NV_FLAGS]);
        }
        assert_eq!(gf_mul(gf_inv(parity_coefficient(ps, 0, lost)), acc), FLAG_CONTAINS_PIC_DATA);
    }

    #[test]
    fn large_frame_is_split_into_fec_blocks_with_continuous_sequence() {
        // 300 KB -> 218 shard data -> 2 blok (maks 212 shard data per blok pada 20%).
        let data = bitstream(300_000);
        let mut packetizer = Packetizer::new(PACKET_SIZE, 0, None);
        let datagrams = packetizer.packetize(&frame(&data, 9));
        let headers: Vec<_> = datagrams.iter().map(|d| VideoPacketHeader::read(d).unwrap()).collect();
        let blocks: Vec<(u8, u8)> = headers.iter().map(|h| unpack_multi_fec_blocks(h.nv.multi_fec_blocks)).collect();
        assert_eq!((blocks[0], *blocks.last().unwrap()), ((0, 2), (1, 2)));
        for (x, header) in headers.iter().enumerate() {
            assert_eq!(header.rtp.sequence_number, x as u16, "nomor urut menerus melewati batas blok");
        }
        let data_shards: usize = headers.iter().filter(|h| FecInfo::unpack(h.nv.fec_info).shard_index == 0).map(|h| usize::from(FecInfo::unpack(h.nv.fec_info).data_shards)).sum();
        assert_eq!(data_shards, (300_000 + SHORT_FRAME_HEADER_SIZE).div_ceil(PAYLOAD));
        assert_eq!(reassemble(&datagrams).1, data);

        // Frame berikutnya melanjutkan nomor urut.
        let next = packetizer.packetize(&frame(&bitstream(100), 10));
        assert_eq!(usize::from(VideoPacketHeader::read(&next[0]).unwrap().rtp.sequence_number), datagrams.len());
    }

    #[test]
    fn enormous_frames_drop_fec_or_are_refused() {
        // Lebih dari 4 blok ber-FEC: dikirim sebagai 4 blok tanpa paritas.
        let data = bitstream(1_500_000);
        let datagrams = Packetizer::new(PACKET_SIZE, 2, None).packetize(&frame(&data, 1));
        let first = VideoPacketHeader::read(&datagrams[0]).unwrap();
        assert_eq!(FecInfo::unpack(first.nv.fec_info).percentage, 0);
        assert_eq!(datagrams.len(), (1_500_000 + SHORT_FRAME_HEADER_SIZE).div_ceil(PAYLOAD), "tanpa shard paritas");
        assert_eq!(reassemble(&datagrams).1, data);
        // Di atas 4 x 1023 shard tidak bisa dinyatakan protokol.
        assert!(Packetizer::new(PACKET_SIZE, 0, None).packetize(&frame(&bitstream(6_000_000), 1)).is_empty());
    }

    #[test]
    fn encrypted_datagrams_decrypt_to_the_plain_ones_with_unique_ivs() {
        let key = [0x5C; 16];
        let data = bitstream(5000);
        let plain = Packetizer::new(PACKET_SIZE, 2, None).packetize(&frame(&data, 4));
        let mut packetizer = Packetizer::new(PACKET_SIZE, 2, Some(key));
        let sealed = packetizer.packetize(&frame(&data, 4));
        assert_eq!(sealed.len(), plain.len());
        for (x, (datagram, expected)) in sealed.iter().zip(&plain).enumerate() {
            assert_eq!(datagram.len(), expected.len() + ENC_VIDEO_PREFIX_SIZE);
            let (iv, rest) = datagram.split_at(12);
            assert_eq!(iv, video_iv(x as u64));
            assert_eq!(&rest[..4], 4u32.to_le_bytes());
            assert_eq!(gcm_decrypt(&key, iv.try_into().unwrap(), &rest[4..20], &rest[20..]).as_ref(), Some(expected));
        }
        // IV terus naik di frame berikutnya (tidak pernah dipakai ulang dengan kunci yang sama).
        let next = packetizer.packetize(&frame(&data, 5));
        assert_eq!(&next[0][..12], video_iv(sealed.len() as u64));
    }
}
