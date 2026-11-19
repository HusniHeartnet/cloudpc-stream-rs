//! Header RTP 12 byte yang mengawali paket video dan audio (`RTP_PACKET` di `Video.h`). Semua field big-endian.

use crate::{need, u16_be, u32_be, WireError};

pub const RTP_HEADER_SIZE: usize = 12;
/// Nilai `header` dasar (RTP versi 2).
pub const RTP_VERSION_2: u8 = 0x80;
/// Bit ekstensi; paket video selalu menyalakannya.
pub const RTP_FLAG_EXTENSION: u8 = 0x10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RtpHeader {
    pub header: u8,
    pub packet_type: u8,
    pub sequence_number: u16,
    pub timestamp: u32,
    pub ssrc: u32,
}

impl RtpHeader {
    pub fn write(&self, out: &mut [u8]) -> Result<(), WireError> {
        need(out, RTP_HEADER_SIZE)?;
        out[0] = self.header;
        out[1] = self.packet_type;
        out[2..4].copy_from_slice(&self.sequence_number.to_be_bytes());
        out[4..8].copy_from_slice(&self.timestamp.to_be_bytes());
        out[8..12].copy_from_slice(&self.ssrc.to_be_bytes());
        Ok(())
    }

    pub fn read(buf: &[u8]) -> Result<RtpHeader, WireError> {
        need(buf, RTP_HEADER_SIZE)?;
        Ok(RtpHeader {
            header: buf[0],
            packet_type: buf[1],
            sequence_number: u16_be(&buf[2..4]),
            timestamp: u32_be(&buf[4..8]),
            ssrc: u32_be(&buf[8..12]),
        })
    }
}

/// Ping UDP yang dikirim client ke port video/audio supaya host tahu alamat tujuan (`SS_PING`):
/// 16 byte payload dari RTSP (`X-SS-Ping-Payload`) lalu nomor urut big-endian.
pub const PING_SIZE: usize = 20;

pub fn write_ping(payload: &[u8; 16], sequence_number: u32) -> [u8; PING_SIZE] {
    let mut out = [0u8; PING_SIZE];
    out[..16].copy_from_slice(payload);
    out[16..].copy_from_slice(&sequence_number.to_be_bytes());
    out
}

pub fn read_ping(buf: &[u8]) -> Result<([u8; 16], u32), WireError> {
    need(buf, PING_SIZE)?;
    let mut payload = [0u8; 16];
    payload.copy_from_slice(&buf[..16]);
    Ok((payload, u32_be(&buf[16..20])))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtp_header_layout_is_big_endian() {
        let h = RtpHeader { header: RTP_VERSION_2 | RTP_FLAG_EXTENSION, packet_type: 97, sequence_number: 0x0102, timestamp: 0x0304_0506, ssrc: 0 };
        let mut buf = [0u8; RTP_HEADER_SIZE];
        h.write(&mut buf).unwrap();
        assert_eq!(buf, [0x90, 97, 1, 2, 3, 4, 5, 6, 0, 0, 0, 0]);
        assert_eq!(RtpHeader::read(&buf).unwrap(), h);
    }

    #[test]
    fn short_buffer_is_an_error() {
        assert_eq!(RtpHeader::read(&[0u8; 11]), Err(WireError::TooShort { need: 12, got: 11 }));
    }

    #[test]
    fn ping_round_trip() {
        let payload = *b"0123456789abcdef";
        let wire = write_ping(&payload, 7);
        assert_eq!(&wire[16..], &[0, 0, 0, 7]);
        assert_eq!(read_ping(&wire).unwrap(), (payload, 7));
    }
}
