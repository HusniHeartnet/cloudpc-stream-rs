//! Fase C2. Datagram video -> frame utuh: dekripsi, kelompokkan per blok FEC, pulihkan shard hilang dengan
//! Reed-Solomon, gabungkan blok, buang header frame dan padding.
//!
//! Rujukan: `moonlight-common-c/src/RtpVideoQueue.c` (antrean per blok FEC) dan `VideoDepacketizer.c` (header frame
//! pendek 8 byte). Aturannya sama dengan Moonlight: satu blok FEC dikerjakan pada satu waktu; blok yang belum lengkap
//! saat paket frame/blok berikutnya datang dianggap hilang, dan frame itu dilaporkan supaya host mengirim pengganti
//! (invalidasi frame rujukan atau IDR). Paket yang datang setelah bloknya selesai (paritas sisa) diabaikan.
//!
//! Murni logika (tanpa soket/GPU), diuji dengan keluaran packetizer host CloudPC dan dengan aliran Sunshine asli.

use cloudpc_stream_common::crypto::gcm_decrypt;
use cloudpc_stream_proto::fec;
use cloudpc_stream_proto::video::{unpack_multi_fec_blocks, FecInfo, FrameType, ShortFrameHeader, VideoPacketHeader, ENC_VIDEO_PREFIX_SIZE, SHORT_FRAME_HEADER_SIZE, VIDEO_PACKET_HEADER_SIZE};

use crate::u16_le;

/// Satu frame terenkode yang siap didecode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoFrame {
    /// Nomor frame dari host (mulai 1, naik satu per frame).
    pub index: u32,
    pub frame_type: FrameType,
    /// Lama tangkap + encode di host, satuan 0,1 ms (0 = host tidak melapor).
    pub host_latency: u16,
    /// Stempel waktu RTP (90 kHz).
    pub rtp_timestamp: u32,
    /// Bitstream encoder: Annex B (H.264/HEVC) atau OBU (AV1), tanpa header frame dan padding.
    pub data: Vec<u8>,
    /// Shard data frame ini yang dipulihkan dari paritas.
    pub recovered_shards: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    Frame(VideoFrame),
    /// Frame `first..=last` tidak bisa disusun. Decoder butuh pengganti dari host.
    Lost { first: u32, last: u32 },
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    /// Datagram yang diterima.
    pub packets: u64,
    /// Gagal didekripsi atau header tidak sah.
    pub invalid: u64,
    /// Datagram untuk frame/blok yang sudah selesai atau sudah ditinggalkan (biasanya paritas sisa).
    pub late: u64,
    pub duplicates: u64,
    pub frames: u64,
    /// Frame yang perlu dipulihkan dengan paritas.
    pub frames_recovered: u64,
    pub shards_recovered: u64,
    /// Frame yang tidak bisa disusun (termasuk yang tidak pernah terlihat sama sekali).
    pub frames_lost: u64,
}

/// Satu blok FEC yang sedang dikumpulkan.
struct Block {
    frame: u32,
    index: u8,
    count: u8,
    lowest_sequence: u16,
    data_shards: usize,
    rtp_timestamp: u32,
    shards: Vec<Option<Vec<u8>>>,
    received: usize,
}

pub struct Depacketizer {
    key: Option<[u8; 16]>,
    /// Ukuran shard di kawat: `packetSize` + 16 (header RTP 12 + cadangan 4 + NV_VIDEO_PACKET 16 + payload).
    shard_size: usize,
    /// Frame yang sedang dikumpulkan / ditunggu berikutnya.
    frame: u32,
    /// Blok FEC berikutnya yang diharapkan di `frame`.
    next_block: u8,
    block: Option<Block>,
    /// Shard data dari blok yang sudah selesai pada `frame`, berurutan.
    finished: Vec<Vec<u8>>,
    recovered_in_frame: u32,
    stats: Stats,
}

/// Permintaan ke host setelah frame hilang.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repair {
    /// Host berhenti merujuk frame `first..=last` dan mengirim P-frame jenis 5 (murah).
    Invalidate { first: u32, last: u32 },
    /// Host mengirim IDR (mahal, dipakai bila invalidasi tidak tersedia atau gagal).
    Idr,
}

/// Penjaga decoder, mengikuti `waitingForIdrFrame`/`waitingForRefInvalFrame` di `VideoDepacketizer.c`: setelah frame
/// hilang, frame berikutnya merujuk gambar yang tidak dimiliki decoder, jadi ditahan sampai host mengirim frame
/// pemulih (IDR, atau jenis 4/5 setelah invalidasi). Aliran selalu dimulai dari IDR.
#[derive(Debug)]
pub struct Recovery {
    invalidation: bool,
    waiting_idr: bool,
    waiting_invalidation: bool,
    dropped_in_a_row: u32,
    /// Frame yang ditahan (tidak diberikan ke decoder).
    pub held: u64,
}

/// Batas frame berturut-turut yang ditahan sebelum meminta IDR (`CONSECUTIVE_DROP_LIMIT` Moonlight).
const HOLD_LIMIT: u32 = 120;

impl Recovery {
    /// `invalidation`: host mengumumkan `refPicInvalidation` di DESCRIBE.
    pub fn new(invalidation: bool) -> Recovery {
        Recovery { invalidation, waiting_idr: true, waiting_invalidation: false, dropped_in_a_row: 0, held: 0 }
    }

    /// Frame `first..=last` hilang: apa yang diminta dari host.
    pub fn lost(&mut self, first: u32, last: u32) -> Repair {
        if self.invalidation && !self.waiting_idr {
            self.waiting_invalidation = true;
            Repair::Invalidate { first, last }
        } else {
            self.waiting_idr = true;
            self.waiting_invalidation = false;
            Repair::Idr
        }
    }

    /// Decoder gagal: tahan frame sampai IDR berikutnya (yang diminta pemanggil).
    pub fn need_idr(&mut self) {
        self.waiting_idr = true;
        self.waiting_invalidation = false;
    }

    /// Boleh frame ini masuk decoder? `Some(Repair::Idr)` bila sudah terlalu lama menunggu.
    pub fn admit(&mut self, frame_type: FrameType) -> (bool, Option<Repair>) {
        match frame_type {
            FrameType::Idr => {
                self.waiting_idr = false;
                self.waiting_invalidation = false;
            }
            FrameType::IntraRefresh | FrameType::AfterRefInvalidation if !self.waiting_idr => self.waiting_invalidation = false,
            _ => {}
        }
        if !self.waiting_idr && !self.waiting_invalidation {
            self.dropped_in_a_row = 0;
            return (true, None);
        }
        self.held += 1;
        self.dropped_in_a_row += 1;
        if self.dropped_in_a_row >= HOLD_LIMIT {
            self.dropped_in_a_row = 0;
            self.waiting_idr = true;
            self.waiting_invalidation = false;
            return (false, Some(Repair::Idr));
        }
        (false, None)
    }
}

/// `a` lebih awal dari `b` dengan memperhitungkan putaran 32 bit.
fn before32(a: u32, b: u32) -> bool {
    (a.wrapping_sub(b) as i32) < 0
}

impl Depacketizer {
    /// `packet_size` = nilai `x-nv-video[0].packetSize` yang dikirim client di SDP. `key` = kunci `rikey` bila video
    /// dienkripsi (bit `encryption::VIDEO`).
    pub fn new(packet_size: u32, key: Option<[u8; 16]>) -> Depacketizer {
        Depacketizer { key, shard_size: packet_size as usize + 16, frame: 1, next_block: 0, block: None, finished: Vec::new(), recovered_in_frame: 0, stats: Stats::default() }
    }

    pub fn stats(&self) -> Stats {
        self.stats
    }

    fn payload_size(&self) -> usize {
        self.shard_size - VIDEO_PACKET_HEADER_SIZE
    }

    /// Masukkan satu datagram dari port video; kembalikan frame yang selesai dan frame yang hilang.
    pub fn push(&mut self, datagram: &[u8]) -> Vec<Output> {
        self.stats.packets += 1;
        let mut out = Vec::new();
        let Some(mut shard) = self.open(datagram) else {
            self.stats.invalid += 1;
            return out;
        };
        let Ok(header) = VideoPacketHeader::read(&shard) else {
            self.stats.invalid += 1;
            return out;
        };
        shard.resize(self.shard_size, 0);
        let frame = header.nv.frame_index;
        let (block_index, block_count) = unpack_multi_fec_blocks(header.nv.multi_fec_blocks);
        let fec = FecInfo::unpack(header.nv.fec_info);
        let sequence = header.rtp.sequence_number;

        if before32(frame, self.frame) || (frame == self.frame && block_index < self.next_block) {
            self.stats.late += 1;
            return out;
        }

        let same_block = self.block.as_ref().is_some_and(|b| b.frame == frame && b.index == block_index);
        if !same_block {
            // Blok sebelumnya belum lengkap: frame-nya tidak bisa disusun lagi.
            if self.block.is_some() {
                self.block = None;
                let lost_upto = if frame == self.frame { frame } else { frame.wrapping_sub(1) };
                self.lose(self.frame, lost_upto, &mut out);
                if frame == lost_upto {
                    // Paket ini milik frame yang baru saja dinyatakan hilang.
                    self.stats.late += 1;
                    return out;
                }
            }
            let expected_block = if frame == self.frame { self.next_block } else { 0 };
            if block_index != expected_block {
                // Blok awal frame ini terlewat seluruhnya.
                self.lose(self.frame, frame, &mut out);
                self.stats.late += 1;
                return out;
            }
            if frame != self.frame {
                // Frame di antaranya tidak pernah terlihat.
                self.lose(self.frame, frame.wrapping_sub(1), &mut out);
            }
            let data_shards = usize::from(fec.data_shards);
            let parity_shards = (data_shards * usize::from(fec.percentage)).div_ceil(100);
            let total = data_shards + parity_shards;
            if data_shards == 0 || (parity_shards > 0 && total > fec::MAX_SHARDS) || usize::from(fec.shard_index) >= total {
                self.stats.invalid += 1;
                return out;
            }
            self.block = Some(Block {
                frame,
                index: block_index,
                count: block_count,
                lowest_sequence: sequence.wrapping_sub(fec.shard_index),
                data_shards,
                rtp_timestamp: header.rtp.timestamp,
                shards: vec![None; data_shards + parity_shards],
                received: 0,
            });
        }

        let block = self.block.as_mut().expect("blok baru saja dibuat");
        let slot = usize::from(sequence.wrapping_sub(block.lowest_sequence));
        if slot >= block.shards.len() {
            self.stats.invalid += 1;
            return out;
        }
        if block.shards[slot].is_some() {
            self.stats.duplicates += 1;
            return out;
        }
        block.shards[slot] = Some(shard);
        block.received += 1;
        if block.received < block.data_shards {
            return out;
        }

        // Cukup shard untuk blok ini: pulihkan yang hilang lalu simpan shard datanya.
        let block = self.block.take().expect("blok ada");
        let mut shards = block.shards;
        let missing = shards[..block.data_shards].iter().filter(|s| s.is_none()).count() as u32;
        if missing > 0 {
            if !fec::reconstruct(&mut shards, block.data_shards) {
                self.stats.invalid += 1;
                self.lose(self.frame, block.frame, &mut out);
                return out;
            }
            self.recovered_in_frame += missing;
            self.stats.shards_recovered += u64::from(missing);
        }
        self.finished.extend(shards.into_iter().take(block.data_shards).map(|s| s.expect("dipulihkan")));
        if block.index + 1 < block.count {
            self.next_block = block.index + 1;
            return out;
        }

        // Blok terakhir: frame lengkap.
        let shards = std::mem::take(&mut self.finished);
        let recovered = std::mem::take(&mut self.recovered_in_frame);
        self.frame = block.frame.wrapping_add(1);
        self.next_block = 0;
        match self.assemble(&shards) {
            Some((header, data)) => {
                self.stats.frames += 1;
                if recovered > 0 {
                    self.stats.frames_recovered += 1;
                }
                out.push(Output::Frame(VideoFrame {
                    index: block.frame,
                    frame_type: header.frame_type,
                    host_latency: header.frame_processing_latency,
                    rtp_timestamp: block.rtp_timestamp,
                    data,
                    recovered_shards: recovered,
                }));
            }
            None => {
                self.stats.invalid += 1;
                self.stats.frames_lost += 1;
                out.push(Output::Lost { first: block.frame, last: block.frame });
            }
        }
        out
    }

    /// Frame `first..=last` dinyatakan hilang; berikutnya yang ditunggu `last + 1`.
    fn lose(&mut self, first: u32, last: u32, out: &mut Vec<Output>) {
        self.finished.clear();
        self.recovered_in_frame = 0;
        self.next_block = 0;
        self.frame = last.wrapping_add(1);
        if before32(last, first) {
            return;
        }
        self.stats.frames_lost += u64::from(last.wrapping_sub(first)) + 1;
        out.push(Output::Lost { first, last });
    }

    /// Dekripsi (bila ada kunci). Hasil: shard polos `[header 32][payload]`.
    fn open(&self, datagram: &[u8]) -> Option<Vec<u8>> {
        let Some(key) = &self.key else { return Some(datagram.to_vec()) };
        if datagram.len() < ENC_VIDEO_PREFIX_SIZE + VIDEO_PACKET_HEADER_SIZE {
            return None;
        }
        let iv: [u8; 12] = datagram[..12].try_into().ok()?;
        gcm_decrypt(key, &iv, &datagram[16..32], &datagram[ENC_VIDEO_PREFIX_SIZE..])
    }

    /// Gabungkan payload shard data, baca header frame 8 byte, buang padding shard terakhir.
    fn assemble(&self, shards: &[Vec<u8>]) -> Option<(ShortFrameHeader, Vec<u8>)> {
        let payload = self.payload_size();
        let mut content = Vec::with_capacity(shards.len() * payload);
        for shard in shards {
            content.extend_from_slice(&shard[VIDEO_PACKET_HEADER_SIZE..]);
        }
        // Header frame pendek: `[01][latensi host 2][jenis][panjang payload terakhir 2][2]`. Jenis yang tidak dikenal
        // dianggap P-frame, seperti Moonlight (yang hanya mencatatnya).
        if content.len() < SHORT_FRAME_HEADER_SIZE || content[0] != 0x01 {
            return None;
        }
        let header = ShortFrameHeader {
            frame_processing_latency: u16_le(&content[1..3]),
            frame_type: FrameType::from_wire(content[3]).unwrap_or(FrameType::P),
            last_payload_len: u16_le(&content[4..6]),
        };
        // `lastPayloadLen` (ekstensi Sunshine): panjang isi shard terakhir. Tanpa itu padding nol dibiarkan
        // (Annex B mengizinkan nol di akhir; AV1 tidak, karena itu host modern selalu mengisinya).
        let last = usize::from(header.last_payload_len);
        let total = if (1..=payload).contains(&last) { (shards.len() - 1) * payload + last } else { content.len() };
        if total < SHORT_FRAME_HEADER_SIZE {
            return None;
        }
        content.truncate(total);
        content.drain(..SHORT_FRAME_HEADER_SIZE);
        Some((header, content))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_starts_at_idr_and_loss_waits_for_a_recovery_frame() {
        let mut recovery = Recovery::new(true);
        assert_eq!(recovery.admit(FrameType::P), (false, None), "sebelum IDR pertama");
        assert_eq!(recovery.admit(FrameType::Idr), (true, None));
        assert_eq!(recovery.admit(FrameType::P), (true, None));
        assert_eq!(recovery.lost(7, 8), Repair::Invalidate { first: 7, last: 8 });
        assert_eq!(recovery.admit(FrameType::P), (false, None), "masih merujuk frame yang hilang");
        assert_eq!(recovery.admit(FrameType::AfterRefInvalidation), (true, None));
        assert_eq!(recovery.admit(FrameType::P), (true, None));
        assert_eq!(recovery.held, 2);
    }

    #[test]
    fn without_invalidation_or_before_the_first_idr_loss_asks_for_idr() {
        let mut recovery = Recovery::new(false);
        assert_eq!(recovery.admit(FrameType::Idr), (true, None));
        assert_eq!(recovery.lost(3, 3), Repair::Idr);
        assert_eq!(recovery.admit(FrameType::AfterRefInvalidation), (false, None), "jenis 5 tidak cukup tanpa IDR");
        assert_eq!(recovery.admit(FrameType::Idr), (true, None));
        let mut fresh = Recovery::new(true);
        assert_eq!(fresh.lost(1, 1), Repair::Idr, "decoder belum pernah punya IDR");
    }

    #[test]
    fn holding_too_long_asks_for_idr() {
        let mut recovery = Recovery::new(true);
        recovery.admit(FrameType::Idr);
        recovery.lost(10, 10);
        for _ in 1..HOLD_LIMIT {
            assert_eq!(recovery.admit(FrameType::P), (false, None));
        }
        assert_eq!(recovery.admit(FrameType::P), (false, Some(Repair::Idr)));
        assert_eq!(recovery.admit(FrameType::AfterRefInvalidation), (false, None), "sekarang menunggu IDR");
        assert_eq!(recovery.admit(FrameType::Idr), (true, None));
    }
}
