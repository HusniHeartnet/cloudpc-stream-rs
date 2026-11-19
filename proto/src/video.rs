//! Paket video: `[RTP 12][cadangan 4][NV_VIDEO_PACKET 16][payload]`, opsional diawali prefiks AES-GCM 32 byte.
//!
//! Satu frame dipotong jadi shard berukuran sama, dibagi sampai 4 blok FEC (Reed-Solomon), dan tiap shard
//! membawa `fecInfo` supaya client bisa memulihkan paket hilang (`stream.cpp` video broadcast di Sunshine,
//! `RtpVideoQueue.c` di moonlight-common-c).

use crate::rtp::{RtpHeader, RTP_HEADER_SIZE};
use crate::{need, u16_le, u32_le, WireError};

pub const FLAG_CONTAINS_PIC_DATA: u8 = 0x1;
pub const FLAG_EOF: u8 = 0x2;
pub const FLAG_SOF: u8 = 0x4;

/// Nilai `multiFecFlags` yang dikirim Sunshine dan diharapkan Moonlight.
pub const MULTI_FEC_FLAGS: u8 = 0x10;
/// Satu frame paling banyak dibagi ke sekian blok FEC.
pub const MAX_FEC_BLOCKS: u8 = 4;

pub const NV_VIDEO_PACKET_SIZE: usize = 16;
/// RTP + 4 byte cadangan + NV_VIDEO_PACKET.
pub const VIDEO_PACKET_HEADER_SIZE: usize = RTP_HEADER_SIZE + 4 + NV_VIDEO_PACKET_SIZE;

/// Header GameStream per shard video. Field multi-byte little-endian.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NvVideoPacket {
    /// Nomor urut shard digeser 8 bit ke kiri.
    pub stream_packet_index: u32,
    pub frame_index: u32,
    pub flags: u8,
    pub extra_flags: u8,
    pub multi_fec_flags: u8,
    pub multi_fec_blocks: u8,
    pub fec_info: u32,
}

impl NvVideoPacket {
    pub fn write(&self, out: &mut [u8]) -> Result<(), WireError> {
        need(out, NV_VIDEO_PACKET_SIZE)?;
        out[0..4].copy_from_slice(&self.stream_packet_index.to_le_bytes());
        out[4..8].copy_from_slice(&self.frame_index.to_le_bytes());
        out[8] = self.flags;
        out[9] = self.extra_flags;
        out[10] = self.multi_fec_flags;
        out[11] = self.multi_fec_blocks;
        out[12..16].copy_from_slice(&self.fec_info.to_le_bytes());
        Ok(())
    }

    pub fn read(buf: &[u8]) -> Result<NvVideoPacket, WireError> {
        need(buf, NV_VIDEO_PACKET_SIZE)?;
        Ok(NvVideoPacket {
            stream_packet_index: u32_le(&buf[0..4]),
            frame_index: u32_le(&buf[4..8]),
            flags: buf[8],
            extra_flags: buf[9],
            multi_fec_flags: buf[10],
            multi_fec_blocks: buf[11],
            fec_info: u32_le(&buf[12..16]),
        })
    }
}

/// Header lengkap satu shard video (sebelum payload).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VideoPacketHeader {
    pub rtp: RtpHeader,
    pub nv: NvVideoPacket,
}

impl VideoPacketHeader {
    pub fn write(&self, out: &mut [u8]) -> Result<(), WireError> {
        need(out, VIDEO_PACKET_HEADER_SIZE)?;
        self.rtp.write(&mut out[..RTP_HEADER_SIZE])?;
        out[RTP_HEADER_SIZE..RTP_HEADER_SIZE + 4].fill(0);
        self.nv.write(&mut out[RTP_HEADER_SIZE + 4..])
    }

    pub fn read(buf: &[u8]) -> Result<VideoPacketHeader, WireError> {
        need(buf, VIDEO_PACKET_HEADER_SIZE)?;
        Ok(VideoPacketHeader { rtp: RtpHeader::read(buf)?, nv: NvVideoPacket::read(&buf[RTP_HEADER_SIZE + 4..])? })
    }
}

/// Isi `fecInfo`: indeks shard (10 bit, mulai bit 12), jumlah shard data (10 bit, mulai bit 22),
/// persentase FEC (8 bit, mulai bit 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FecInfo {
    pub shard_index: u16,
    pub data_shards: u16,
    pub percentage: u8,
}

impl FecInfo {
    pub fn pack(&self) -> u32 {
        (u32::from(self.shard_index) & 0x3FF) << 12 | (u32::from(self.data_shards) & 0x3FF) << 22 | u32::from(self.percentage) << 4
    }

    pub fn unpack(v: u32) -> FecInfo {
        FecInfo { shard_index: ((v & 0x003F_F000) >> 12) as u16, data_shards: ((v & 0xFFC0_0000) >> 22) as u16, percentage: ((v & 0xFF0) >> 4) as u8 }
    }
}

/// `multiFecBlocks`: indeks blok di bit 4-5, indeks blok terakhir di bit 6-7.
pub fn pack_multi_fec_blocks(block_index: u8, block_count: u8) -> Result<u8, WireError> {
    if block_count == 0 || block_count > MAX_FEC_BLOCKS || block_index >= block_count {
        return Err(WireError::Invalid("blok FEC di luar rentang 1..=4"));
    }
    Ok(block_index << 4 | (block_count - 1) << 6)
}

/// Mengembalikan `(indeks blok, jumlah blok)`.
pub fn unpack_multi_fec_blocks(v: u8) -> (u8, u8) {
    ((v >> 4) & 0x3, ((v >> 6) & 0x3) + 1)
}

/// Jenis frame di header frame pendek.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameType {
    P = 1,
    Idr = 2,
    /// P-frame dengan blok intra-refresh.
    IntraRefresh = 4,
    /// P-frame setelah invalidasi frame rujukan.
    AfterRefInvalidation = 5,
}

impl FrameType {
    pub fn from_wire(v: u8) -> Option<FrameType> {
        match v {
            1 => Some(FrameType::P),
            2 => Some(FrameType::Idr),
            4 => Some(FrameType::IntraRefresh),
            5 => Some(FrameType::AfterRefInvalidation),
            _ => None,
        }
    }
}

pub const SHORT_FRAME_HEADER_SIZE: usize = 8;

/// Header 8 byte di awal data tiap frame (sebelum bitstream encoder).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShortFrameHeader {
    /// Ekstensi Sunshine: lama pemrosesan frame di host, satuan 0,1 ms (0 = tidak diketahui).
    pub frame_processing_latency: u16,
    pub frame_type: FrameType,
    /// Ekstensi Sunshine: panjang payload shard terakhir, untuk codec yang tidak tahan padding nol (AV1).
    pub last_payload_len: u16,
}

impl ShortFrameHeader {
    pub fn write(&self, out: &mut [u8]) -> Result<(), WireError> {
        need(out, SHORT_FRAME_HEADER_SIZE)?;
        out[0] = 0x01;
        out[1..3].copy_from_slice(&self.frame_processing_latency.to_le_bytes());
        out[3] = self.frame_type as u8;
        out[4..6].copy_from_slice(&self.last_payload_len.to_le_bytes());
        out[6..8].fill(0);
        Ok(())
    }

    pub fn read(buf: &[u8]) -> Result<ShortFrameHeader, WireError> {
        need(buf, SHORT_FRAME_HEADER_SIZE)?;
        if buf[0] != 0x01 {
            return Err(WireError::Invalid("bukan header frame pendek"));
        }
        Ok(ShortFrameHeader {
            frame_processing_latency: u16_le(&buf[1..3]),
            frame_type: FrameType::from_wire(buf[3]).ok_or(WireError::Invalid("jenis frame tidak dikenal"))?,
            last_payload_len: u16_le(&buf[4..6]),
        })
    }
}

/// Prefiks shard video terenkripsi: `[iv 12][nomor frame 4][tag GCM 16]`.
pub const ENC_VIDEO_PREFIX_SIZE: usize = 32;

/// IV AES-GCM video: penghitung 64 bit little-endian, byte terakhir `'V'` (NIST SP 800-38D 8.2.1).
pub fn video_iv(counter: u64) -> [u8; 12] {
    let mut iv = [0u8; 12];
    iv[..8].copy_from_slice(&counter.to_le_bytes());
    iv[11] = b'V';
    iv
}

/// Format video yang dinegosiasikan client (`VIDEO_FORMAT_*` di `Limelight.h`).
pub mod format {
    pub const H264: u32 = 0x0001;
    pub const H264_HIGH8_444: u32 = 0x0004;
    pub const H265: u32 = 0x0100;
    pub const H265_MAIN10: u32 = 0x0200;
    pub const H265_REXT8_444: u32 = 0x0400;
    pub const H265_REXT10_444: u32 = 0x0800;
    pub const AV1_MAIN8: u32 = 0x1000;
    pub const AV1_MAIN10: u32 = 0x2000;
    pub const AV1_HIGH8_444: u32 = 0x4000;
    pub const AV1_HIGH10_444: u32 = 0x8000;

    pub const MASK_H264: u32 = 0x000F;
    pub const MASK_H265: u32 = 0x0F00;
    pub const MASK_AV1: u32 = 0xF000;
    pub const MASK_10BIT: u32 = 0xAA00;
    pub const MASK_YUV444: u32 = 0xCC04;
}

/// `ServerCodecModeSupport` di `/serverinfo` (`SCM_*` di `Limelight.h`).
pub mod server_codec_mode {
    pub const H264: u32 = 0x0000_0001;
    pub const HEVC: u32 = 0x0000_0100;
    pub const HEVC_MAIN10: u32 = 0x0000_0200;
    pub const AV1_MAIN8: u32 = 0x0001_0000;
    pub const AV1_MAIN10: u32 = 0x0002_0000;
    pub const H264_HIGH8_444: u32 = 0x0004_0000;
    pub const HEVC_REXT8_444: u32 = 0x0008_0000;
    pub const HEVC_REXT10_444: u32 = 0x0010_0000;
    pub const AV1_HIGH8_444: u32 = 0x0020_0000;
    pub const AV1_HIGH10_444: u32 = 0x0040_0000;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rtp::{RTP_FLAG_EXTENSION, RTP_VERSION_2};

    #[test]
    fn header_is_32_bytes_and_round_trips() {
        let h = VideoPacketHeader {
            rtp: RtpHeader { header: RTP_VERSION_2 | RTP_FLAG_EXTENSION, packet_type: 0, sequence_number: 300, timestamp: 90_000, ssrc: 0 },
            nv: NvVideoPacket {
                stream_packet_index: 300 << 8,
                frame_index: 42,
                flags: FLAG_CONTAINS_PIC_DATA | FLAG_SOF,
                extra_flags: 0,
                multi_fec_flags: MULTI_FEC_FLAGS,
                multi_fec_blocks: pack_multi_fec_blocks(1, 3).unwrap(),
                fec_info: FecInfo { shard_index: 5, data_shards: 20, percentage: 20 }.pack(),
            },
        };
        let mut buf = [0xAAu8; VIDEO_PACKET_HEADER_SIZE];
        h.write(&mut buf).unwrap();
        assert_eq!(VIDEO_PACKET_HEADER_SIZE, 32);
        assert_eq!(&buf[12..16], &[0, 0, 0, 0], "byte cadangan harus nol");
        assert_eq!(&buf[20..24], &42u32.to_le_bytes(), "frameIndex little-endian di offset 20");
        assert_eq!(VideoPacketHeader::read(&buf).unwrap(), h);
    }

    #[test]
    fn fec_info_matches_sunshine_and_moonlight_bit_layout() {
        // Sunshine: x << 12 | data_shards << 22 | percentage << 4
        let packed = FecInfo { shard_index: 7, data_shards: 100, percentage: 20 }.pack();
        assert_eq!(packed, 7 << 12 | 100 << 22 | 20 << 4);
        // Moonlight: (v & 0x3FF000) >> 12, (v & 0xFFC00000) >> 22, (v & 0xFF0) >> 4
        assert_eq!(FecInfo::unpack(packed), FecInfo { shard_index: 7, data_shards: 100, percentage: 20 });
        let max = FecInfo { shard_index: 1023, data_shards: 1023, percentage: 255 };
        assert_eq!(FecInfo::unpack(max.pack()), max);
    }

    #[test]
    fn multi_fec_blocks_round_trip_and_limits() {
        for count in 1..=MAX_FEC_BLOCKS {
            for index in 0..count {
                assert_eq!(unpack_multi_fec_blocks(pack_multi_fec_blocks(index, count).unwrap()), (index, count));
            }
        }
        assert!(pack_multi_fec_blocks(0, 5).is_err());
        assert!(pack_multi_fec_blocks(2, 2).is_err());
        assert!(pack_multi_fec_blocks(0, 0).is_err());
    }

    #[test]
    fn short_frame_header_round_trip() {
        let h = ShortFrameHeader { frame_processing_latency: 18, frame_type: FrameType::Idr, last_payload_len: 777 };
        let mut buf = [0xFFu8; SHORT_FRAME_HEADER_SIZE];
        h.write(&mut buf).unwrap();
        assert_eq!(buf, [1, 18, 0, 2, 0x09, 0x03, 0, 0]);
        assert_eq!(ShortFrameHeader::read(&buf).unwrap(), h);
        buf[0] = 2;
        assert!(ShortFrameHeader::read(&buf).is_err());
    }

    #[test]
    fn video_iv_never_repeats_for_consecutive_counters() {
        assert_eq!(video_iv(1), [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, b'V']);
        assert_ne!(video_iv(1), video_iv(2));
    }
}
