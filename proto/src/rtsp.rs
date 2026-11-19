//! RTSP GameStream (TCP, port dasar + 21): satu pesan per koneksi, dipakai untuk menegosiasikan sesi stream.
//!
//! Pesan terenkripsi (URL `rtspenc://`, client dengan `corever >= 1`) dibungkus
//! `[0x80000000 | panjang u32 BE][seq u32 BE][tag GCM 16][ciphertext]`, AES-128-GCM dengan kunci `rikey`.
//! Rujukan: `Sunshine/src/rtsp.cpp`, `rtsp.h`; `moonlight-common-c/src/RtspConnection.c`, `RtspParser.c`.

use std::collections::HashMap;

use crate::control::Origin;
use crate::{need, u32_be, WireError};

/// Bit teratas field panjang menandai pesan terenkripsi.
pub const ENCRYPTED_FLAG: u32 = 0x8000_0000;
pub const ENCRYPTED_HEADER_SIZE: usize = 24;

/// IV AES-GCM pesan RTSP: seq little-endian, lalu `'H'`/`'C'` dan `'R'` di dua byte terakhir.
pub fn rtsp_iv(seq: u32, origin: Origin) -> [u8; 12] {
    let mut iv = [0u8; 12];
    iv[..4].copy_from_slice(&seq.to_le_bytes());
    iv[10] = match origin {
        Origin::Host => b'H',
        Origin::Client => b'C',
    };
    iv[11] = b'R';
    iv
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncryptedHeader {
    /// Panjang ciphertext (tanpa header ini).
    pub length: u32,
    pub seq: u32,
    pub tag: [u8; 16],
}

impl EncryptedHeader {
    pub fn write(&self) -> [u8; ENCRYPTED_HEADER_SIZE] {
        let mut out = [0u8; ENCRYPTED_HEADER_SIZE];
        out[0..4].copy_from_slice(&(ENCRYPTED_FLAG | self.length).to_be_bytes());
        out[4..8].copy_from_slice(&self.seq.to_be_bytes());
        out[8..24].copy_from_slice(&self.tag);
        out
    }

    pub fn read(buf: &[u8]) -> Result<EncryptedHeader, WireError> {
        need(buf, ENCRYPTED_HEADER_SIZE)?;
        let type_and_length = u32_be(&buf[0..4]);
        if type_and_length & ENCRYPTED_FLAG == 0 {
            return Err(WireError::Invalid("pesan RTSP tidak terenkripsi"));
        }
        let mut tag = [0u8; 16];
        tag.copy_from_slice(&buf[8..24]);
        Ok(EncryptedHeader { length: type_and_length & !ENCRYPTED_FLAG, seq: u32_be(&buf[4..8]), tag })
    }
}

fn split_head(raw: &[u8]) -> Result<(Vec<String>, &[u8]), WireError> {
    let at = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or(WireError::Invalid("pesan RTSP tanpa baris kosong penutup header"))?;
    let head = std::str::from_utf8(&raw[..at]).map_err(|_| WireError::Invalid("header RTSP bukan UTF-8"))?;
    Ok((head.split("\r\n").map(str::to_string).collect(), &raw[at + 4..]))
}

fn parse_headers(lines: &[String]) -> Vec<(String, String)> {
    lines.iter().filter_map(|line| line.split_once(':')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect()
}

fn find_header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}

fn serialize(first_line: String, headers: &[(String, String)], payload: &[u8]) -> Vec<u8> {
    let mut out = first_line;
    for (name, value) in headers {
        out.push_str(&format!("\r\n{name}: {value}"));
    }
    out.push_str("\r\n\r\n");
    let mut out = out.into_bytes();
    out.extend_from_slice(payload);
    out
}

/// Permintaan dari client, mis. `SETUP streamid=video/0/0 RTSP/1.0`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub command: String,
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub payload: Vec<u8>,
}

impl Request {
    pub fn parse(raw: &[u8]) -> Result<Request, WireError> {
        let (lines, payload) = split_head(raw)?;
        let mut first = lines[0].split(' ');
        match (first.next(), first.next(), first.next()) {
            (Some(command), Some(target), Some(protocol)) if protocol.starts_with("RTSP/") && !command.is_empty() => {
                Ok(Request { command: command.to_string(), target: target.to_string(), headers: parse_headers(&lines[1..]), payload: payload.to_vec() })
            }
            _ => Err(WireError::Invalid("baris pertama permintaan RTSP tidak valid")),
        }
    }

    pub fn serialize(&self) -> Vec<u8> {
        serialize(format!("{} {} RTSP/1.0", self.command, self.target), &self.headers, &self.payload)
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        find_header(&self.headers, name)
    }

    /// Nomor urut `CSeq`; 0 bila tidak ada.
    pub fn cseq(&self) -> u32 {
        self.header("CSeq").and_then(|v| v.parse().ok()).unwrap_or(0)
    }
}

/// Balasan host, mis. `RTSP/1.0 200 OK`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub reason: String,
    pub headers: Vec<(String, String)>,
    pub payload: Vec<u8>,
}

impl Response {
    pub fn new(status: u16, reason: &str, cseq: u32) -> Response {
        Response { status, reason: reason.to_string(), headers: vec![("CSeq".into(), cseq.to_string())], payload: Vec::new() }
    }

    pub fn with_header(mut self, name: &str, value: impl ToString) -> Response {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    pub fn with_payload(mut self, payload: impl Into<Vec<u8>>) -> Response {
        self.payload = payload.into();
        self
    }

    pub fn serialize(&self) -> Vec<u8> {
        serialize(format!("RTSP/1.0 {} {}", self.status, self.reason), &self.headers, &self.payload)
    }

    pub fn parse(raw: &[u8]) -> Result<Response, WireError> {
        let (lines, payload) = split_head(raw)?;
        let mut first = lines[0].splitn(3, ' ');
        match (first.next(), first.next().and_then(|s| s.parse().ok()), first.next()) {
            (Some(protocol), Some(status), reason) if protocol.starts_with("RTSP/") => {
                Ok(Response { status, reason: reason.unwrap_or("").to_string(), headers: parse_headers(&lines[1..]), payload: payload.to_vec() })
            }
            _ => Err(WireError::Invalid("baris pertama balasan RTSP tidak valid")),
        }
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        find_header(&self.headers, name)
    }
}

/// Atribut SDP `a=nama:nilai` dari payload ANNOUNCE/DESCRIBE. Nama yang muncul dua kali: yang pertama dipakai.
pub fn sdp_attributes(payload: &[u8]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in String::from_utf8_lossy(payload).split(['\r', '\n']) {
        if let Some((name, value)) = line.strip_prefix("a=").and_then(|rest| rest.split_once(':')) {
            out.entry(name.to_string()).or_insert_with(|| value.trim_end_matches(' ').to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypted_header_layout_is_big_endian_with_flag() {
        let header = EncryptedHeader { length: 0x0102, seq: 7, tag: [0xAB; 16] };
        let wire = header.write();
        assert_eq!(&wire[..8], &[0x80, 0, 0x01, 0x02, 0, 0, 0, 7]);
        assert_eq!(EncryptedHeader::read(&wire).unwrap(), header);
        let mut plain = wire;
        plain[0] = 0;
        assert!(EncryptedHeader::read(&plain).is_err(), "tanpa bit enkripsi harus ditolak");
        assert!(EncryptedHeader::read(&wire[..23]).is_err());
    }

    #[test]
    fn iv_separates_host_and_client() {
        assert_eq!(rtsp_iv(1, Origin::Client), [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, b'C', b'R']);
        assert_eq!(rtsp_iv(1, Origin::Host)[10..], [b'H', b'R']);
    }

    #[test]
    fn parses_moonlight_setup_request() {
        let raw = b"SETUP streamid=video/0/0 RTSP/1.0\r\nCSeq: 4\r\nX-GS-ClientVersion: 14\r\nHost: 127.0.0.1\r\nSession: DEADBEEFCAFE\r\nTransport: unicast;X-GS-ClientPort=50000-50001\r\n\r\n";
        let req = Request::parse(raw).unwrap();
        assert_eq!((req.command.as_str(), req.target.as_str(), req.cseq()), ("SETUP", "streamid=video/0/0", 4));
        assert_eq!(req.header("transport"), Some("unicast;X-GS-ClientPort=50000-50001"));
        assert!(req.payload.is_empty());
        assert_eq!(Request::parse(&req.serialize()).unwrap(), req);
    }

    #[test]
    fn announce_payload_and_sdp_attributes() {
        let sdp = "v=0\r\ns=NVIDIA Streaming Client\r\na=x-nv-video[0].clientViewportWd:1920 \r\na=x-nv-video[0].maxFPS:60\na=x-nv-video[0].maxFPS:30\r\n";
        let raw = [b"ANNOUNCE streamid=control/13/0 RTSP/1.0\r\nCSeq: 6\r\nContent-length: 99\r\n\r\n".as_slice(), sdp.as_bytes()].concat();
        let req = Request::parse(&raw).unwrap();
        let attrs = sdp_attributes(&req.payload);
        assert_eq!(attrs.get("x-nv-video[0].clientViewportWd").map(String::as_str), Some("1920"), "spasi di akhir dibuang");
        assert_eq!(attrs.get("x-nv-video[0].maxFPS").map(String::as_str), Some("60"), "nilai pertama yang dipakai");
        assert_eq!(attrs.len(), 2);
    }

    #[test]
    fn response_matches_sunshine_serialization() {
        let resp = Response::new(200, "OK", 3).with_header("Session", "DEADBEEFCAFE;timeout = 90").with_payload("a=x:1\n");
        let wire = resp.serialize();
        assert_eq!(wire, b"RTSP/1.0 200 OK\r\nCSeq: 3\r\nSession: DEADBEEFCAFE;timeout = 90\r\n\r\na=x:1\n");
        assert_eq!(Response::parse(&wire).unwrap(), resp);
        assert_eq!(Response::parse(b"RTSP/1.0 404 NOT FOUND\r\nCSeq: 1\r\n\r\n").unwrap().reason, "NOT FOUND");
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(Request::parse(b"SETUP x RTSP/1.0\r\nCSeq: 1\r\n").is_err(), "tanpa baris kosong");
        assert!(Request::parse(b"HALO\r\n\r\n").is_err());
        assert!(Response::parse(b"RTSP/1.0 abc OK\r\n\r\n").is_err());
    }
}
