//! Fase C1. Jabat tangan RTSP (polos atau `rtspenc://`) dan SDP pilihan client.
//!
//! Rujukan: `moonlight-common-c/src/RtspConnection.c`, `SdpGenerator.c`. Urutan: OPTIONS, DESCRIBE, SETUP audio,
//! SETUP video, SETUP control, ANNOUNCE (SDP), PLAY; satu koneksi TCP per pesan. Koneksi selalu ke alamat yang diberikan
//! pemanggil; alamat di URL sesi dari host hanya dipakai port-nya (host di balik VPS menyebut alamat privatnya).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use cloudpc_stream_common::secure::{rtsp_read, rtsp_seal};
use cloudpc_stream_proto::control::{client_feature, encryption, Origin};
use cloudpc_stream_proto::rtsp::{sdp_attributes, Request, Response};

use crate::http;

const IO_TIMEOUT: Duration = Duration::from_secs(10);
/// Versi klien RTSP yang dikirim Moonlight 6 (`X-GS-ClientVersion`).
const CLIENT_VERSION: u32 = 14;

/// Format video yang diminta (`x-nv-vqos[0].bitStreamFormat`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    H264 = 0,
    Hevc = 1,
    Av1 = 2,
}

/// Pilihan stream dari client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamParams {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
    pub codec: Codec,
    /// Ukuran payload paket video: 1392 di LAN, 1024 lewat internet (aman untuk MTU terowongan).
    pub packet_size: u32,
}

/// Yang dinyatakan host di DESCRIBE.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HostOffer {
    pub encryption_supported: u32,
    pub encryption_requested: u32,
    pub ref_invalidation: bool,
    pub hevc: bool,
    pub av1: bool,
}

/// Hasil jabat tangan: port aliran dan data yang dibawa client ke sana.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Negotiated {
    pub offer: HostOffer,
    pub encryption: u32,
    pub audio_port: u16,
    pub video_port: u16,
    pub control_port: u16,
    pub ping_payload: [u8; 16],
    pub connect_data: u32,
}

/// `rtspenc://alamat:port` atau `rtsp://alamat:port` -> (terenkripsi, port).
pub fn parse_session_url(url: &str) -> Result<(bool, u16), String> {
    let (encrypted, rest) = if let Some(rest) = url.strip_prefix("rtspenc://") {
        (true, rest)
    } else if let Some(rest) = url.strip_prefix("rtsp://") {
        (false, rest)
    } else {
        return Err(format!("alamat RTSP tidak dikenal: {url}"));
    };
    let authority = rest.split('/').next().unwrap_or("");
    let (_, port) = http::split_host_port(authority, 48010)?;
    Ok((encrypted, port))
}

/// SDP yang dikirim di ANNOUNCE. Atribut dan nilainya mengikuti `SdpGenerator.c` untuk host generasi 7 (Sunshine).
pub fn build_sdp(params: &StreamParams, encryption: u32, host: &str) -> String {
    let fps = params.fps.to_string();
    let bitrate = params.bitrate_kbps.to_string();
    let codec = (params.codec as u32).to_string();
    let attributes: Vec<(&str, String)> = vec![
        ("x-ml-general.featureFlags", (client_feature::FEC_STATUS | client_feature::SESSION_ID_V1).to_string()),
        ("x-ss-general.encryptionEnabled", encryption.to_string()),
        ("x-ss-video[0].chromaSamplingType", "0".into()),
        ("x-ss-video[0].intraRefresh", "0".into()),
        ("x-nv-video[0].clientViewportWd", params.width.to_string()),
        ("x-nv-video[0].clientViewportHt", params.height.to_string()),
        ("x-nv-video[0].maxFPS", fps.clone()),
        ("x-nv-video[0].packetSize", params.packet_size.to_string()),
        ("x-nv-video[0].rateControlMode", "4".into()),
        ("x-nv-video[0].timeoutLengthMs", "7000".into()),
        ("x-nv-video[0].framesWithInvalidRefThreshold", "0".into()),
        ("x-nv-video[0].initialBitrateKbps", bitrate.clone()),
        ("x-nv-video[0].initialPeakBitrateKbps", bitrate.clone()),
        ("x-nv-vqos[0].bw.minimumBitrateKbps", bitrate.clone()),
        ("x-nv-vqos[0].bw.maximumBitrateKbps", bitrate.clone()),
        ("x-ml-video.configuredBitrateKbps", bitrate),
        ("x-nv-vqos[0].fec.enable", "1".into()),
        ("x-nv-vqos[0].videoQualityScoreUpdateTime", "5000".into()),
        ("x-nv-vqos[0].qosTrafficType", "5".into()),
        ("x-nv-aqos.qosTrafficType", "4".into()),
        ("x-nv-general.featureFlags", "135".into()),
        ("x-nv-general.useReliableUdp", "13".into()),
        ("x-nv-vqos[0].fec.minRequiredFecPackets", "2".into()),
        ("x-nv-vqos[0].bllFec.enable", "0".into()),
        ("x-nv-ri.useControlChannel", "1".into()),
        ("x-nv-vqos[0].fec.repairPercent", "20".into()),
        ("x-nv-vqos[0].drc.enable", "0".into()),
        ("x-nv-general.enableRecoveryMode", "0".into()),
        ("x-nv-video[0].videoEncoderSlicesPerFrame", "1".into()),
        ("x-nv-vqos[0].bitStreamFormat", codec),
        ("x-nv-clientSupportHevc", if params.codec == Codec::Hevc { "1" } else { "0" }.into()),
        ("x-nv-video[0].dynamicRangeMode", "0".into()),
        ("x-nv-video[0].maxNumReferenceFrames", "0".into()),
        ("x-nv-video[0].clientRefreshRateX100", (params.fps * 100).to_string()),
        ("x-nv-audio.surround.numChannels", "2".into()),
        ("x-nv-audio.surround.channelMask", "3".into()),
        ("x-nv-audio.surround.enable", "0".into()),
        ("x-nv-audio.surround.AudioQuality", "0".into()),
        ("x-nv-aqos.packetDuration", "5".into()),
        // Rec. 709 (1) << 1 | rentang terbatas (0).
        ("x-nv-video[0].encoderCscMode", "2".into()),
    ];
    let family = if host.contains(':') { "IPv6" } else { "IPv4" };
    let mut sdp = format!("v=0\r\no=android 0 {CLIENT_VERSION} IN {family} {host}\r\ns=NVIDIA Streaming Client\r\n");
    for (name, value) in attributes {
        sdp.push_str(&format!("a={name}:{value} \r\n"));
    }
    sdp.push_str("t=0 0\r\nm=video 47998  \r\n");
    sdp
}

fn parse_offer(sdp: &[u8]) -> HostOffer {
    let attrs = sdp_attributes(sdp);
    let number = |name: &str| attrs.get(name).and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(0);
    let text = String::from_utf8_lossy(sdp);
    HostOffer {
        encryption_supported: number("x-ss-general.encryptionSupported"),
        encryption_requested: number("x-ss-general.encryptionRequested"),
        ref_invalidation: attrs.contains_key("x-nv-video[0].refPicInvalidation"),
        hevc: text.contains("sprop-parameter-sets=AAAAAU"),
        av1: text.contains("AV1/90000"),
    }
}

/// Satu percakapan RTSP: koneksi baru per pesan, dienkripsi bila URL sesinya `rtspenc://`.
struct Rtsp<'a> {
    host: &'a str,
    port: u16,
    key: Option<[u8; 16]>,
    seq: u32,
    target: &'a str,
    session: Option<String>,
}

impl Rtsp<'_> {
    fn call(&mut self, command: &str, target: &str, extra: &[(&str, String)], payload: Vec<u8>) -> Result<Response, String> {
        self.seq += 1;
        let mut headers = vec![("CSeq".to_string(), self.seq.to_string()), ("X-GS-ClientVersion".to_string(), CLIENT_VERSION.to_string()), ("Host".to_string(), self.host.to_string())];
        if let Some(session) = &self.session {
            headers.push(("Session".into(), session.clone()));
        }
        headers.extend(extra.iter().map(|(k, v)| (k.to_string(), v.clone())));
        let request = Request { command: command.into(), target: target.into(), headers, payload }.serialize();

        let addr = http::resolve(self.host, self.port)?;
        let mut stream = TcpStream::connect_timeout(&addr, IO_TIMEOUT).map_err(|e| format!("RTSP {command}: tidak bisa menyambung ke {}:{}: {e}", self.host, self.port))?;
        let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
        let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
        let raw = match self.key {
            Some(key) => {
                stream.write_all(&rtsp_seal(&key, self.seq, Origin::Client, &request)).map_err(|e| format!("RTSP {command}: {e}"))?;
                rtsp_read(&mut stream, &key, Origin::Host).map_err(|e| format!("RTSP {command}: balasan tidak terbaca ({e})"))?
            }
            None => {
                stream.write_all(&request).map_err(|e| format!("RTSP {command}: {e}"))?;
                let mut raw = Vec::new();
                let _ = stream.read_to_end(&mut raw);
                raw
            }
        };
        let response = Response::parse(&raw).map_err(|e| format!("RTSP {command}: {e}"))?;
        if response.status != 200 {
            return Err(format!("RTSP {command} ditolak host ({} {})", response.status, response.reason));
        }
        Ok(response)
    }

    fn setup(&mut self, stream: &str) -> Result<Response, String> {
        let transport = ("Transport", "unicast;X-GS-ClientPort=50000-50001".to_string());
        let since = ("If-Modified-Since", "Thu, 01 Jan 1970 00:00:00 GMT".to_string());
        let response = self.call("SETUP", stream, &[transport, since], Vec::new())?;
        if self.session.is_none() {
            // "DEADBEEFCAFE;timeout = 90" -> hanya ID sesinya.
            self.session = response.header("Session").map(|s| s.split(';').next().unwrap_or(s).trim().to_string());
        }
        Ok(response)
    }
}

fn server_port(response: &Response) -> Result<u16, String> {
    response
        .header("Transport")
        .and_then(|t| t.split(';').find_map(|part| part.trim().strip_prefix("server_port=")))
        .and_then(|p| p.split('-').next()?.parse().ok())
        .ok_or_else(|| "host tidak menyebut port aliran (Transport)".into())
}

/// Jabat tangan lengkap. `choose` dipanggil setelah DESCRIBE untuk menyesuaikan pilihan dengan kemampuan host.
pub fn handshake(host: &str, session_url: &str, key: [u8; 16], choose: impl FnOnce(&HostOffer) -> Result<StreamParams, String>) -> Result<(StreamParams, Negotiated), String> {
    let (encrypted, port) = parse_session_url(session_url)?;
    let mut rtsp = Rtsp { host, port, key: encrypted.then_some(key), seq: 0, target: session_url, session: None };
    let target = rtsp.target;

    rtsp.call("OPTIONS", target, &[], Vec::new())?;
    let describe = rtsp.call("DESCRIBE", target, &[("Accept", "application/sdp".into()), ("If-Modified-Since", "Thu, 01 Jan 1970 00:00:00 GMT".into())], Vec::new())?;
    let offer = parse_offer(&describe.payload);
    let params = choose(&offer)?;
    // Semua enkripsi yang didukung host dinyalakan: stream CloudPC lewat internet.
    let encryption = offer.encryption_supported & (encryption::CONTROL_V2 | encryption::VIDEO | encryption::AUDIO);
    if encryption & offer.encryption_requested != offer.encryption_requested {
        return Err("host meminta enkripsi yang tidak didukung client ini".into());
    }

    let audio = rtsp.setup("streamid=audio/0/0")?;
    let video = rtsp.setup("streamid=video/0/0")?;
    let control = rtsp.setup("streamid=control/13/0")?;
    let ping_text = video.header("X-SS-Ping-Payload").or(audio.header("X-SS-Ping-Payload")).ok_or("host tidak memberi X-SS-Ping-Payload (Sunshine terlalu lama?)")?;
    let ping_payload: [u8; 16] = ping_text.as_bytes().try_into().map_err(|_| "X-SS-Ping-Payload bukan 16 karakter")?;
    let connect_data = control.header("X-SS-Connect-Data").and_then(|v| v.parse::<u32>().ok()).ok_or("host tidak memberi X-SS-Connect-Data")?;
    let negotiated = Negotiated { offer, encryption, audio_port: server_port(&audio)?, video_port: server_port(&video)?, control_port: server_port(&control)?, ping_payload, connect_data };

    let sdp = build_sdp(&params, encryption, host).into_bytes();
    let length = sdp.len().to_string();
    rtsp.call("ANNOUNCE", "streamid=control/13/0", &[("Content-type", "application/sdp".into()), ("Content-length", length)], sdp)?;
    rtsp.call("PLAY", "/", &[], Vec::new())?;
    Ok((params, negotiated))
}

/// Atribut SDP sebagai peta (untuk tes dan diagnostik).
pub fn sdp_map(sdp: &str) -> HashMap<String, String> {
    sdp_attributes(sdp.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_urls() {
        assert_eq!(parse_session_url("rtspenc://10.8.0.2:48010").unwrap(), (true, 48010));
        assert_eq!(parse_session_url("rtsp://[fe80::1]:48010/x").unwrap(), (false, 48010));
        assert_eq!(parse_session_url("rtsp://192.168.1.20").unwrap(), (false, 48010));
        assert!(parse_session_url("http://x:1").is_err());
    }

    #[test]
    fn sdp_carries_everything_the_host_requires() {
        let params = StreamParams { width: 1920, height: 1080, fps: 120, bitrate_kbps: 30000, codec: Codec::Hevc, packet_size: 1024 };
        let sdp = build_sdp(&params, 7, "203.0.113.10");
        assert!(sdp.starts_with("v=0\r\no=android 0 14 IN IPv4 203.0.113.10\r\ns=NVIDIA Streaming Client\r\n"));
        assert!(sdp.ends_with("t=0 0\r\nm=video 47998  \r\n"));
        let attrs = sdp_map(&sdp);
        // Atribut yang wajib bagi Sunshine (`args.at(...)` di rtsp.cpp) dan host CloudPC (`StreamConfig::from_sdp`).
        for name in [
            "x-nv-audio.surround.numChannels",
            "x-nv-audio.surround.channelMask",
            "x-nv-aqos.packetDuration",
            "x-nv-audio.surround.AudioQuality",
            "x-nv-general.useReliableUdp",
            "x-nv-video[0].packetSize",
            "x-nv-vqos[0].fec.minRequiredFecPackets",
            "x-ml-general.featureFlags",
            "x-nv-aqos.qosTrafficType",
            "x-nv-vqos[0].qosTrafficType",
            "x-ss-general.encryptionEnabled",
            "x-nv-general.featureFlags",
            "x-nv-video[0].clientViewportHt",
            "x-nv-video[0].clientViewportWd",
            "x-nv-video[0].maxFPS",
            "x-nv-video[0].clientRefreshRateX100",
            "x-nv-vqos[0].bw.maximumBitrateKbps",
            "x-nv-video[0].videoEncoderSlicesPerFrame",
            "x-nv-video[0].maxNumReferenceFrames",
            "x-nv-video[0].encoderCscMode",
            "x-nv-vqos[0].bitStreamFormat",
            "x-nv-video[0].dynamicRangeMode",
            "x-ss-video[0].chromaSamplingType",
            "x-ss-video[0].intraRefresh",
            "x-ml-video.configuredBitrateKbps",
        ] {
            assert!(attrs.contains_key(name), "{name} tidak ada");
        }
        assert_eq!((attrs["x-nv-video[0].maxFPS"].as_str(), attrs["x-nv-vqos[0].bitStreamFormat"].as_str(), attrs["x-ss-general.encryptionEnabled"].as_str()), ("120", "1", "7"));
        assert!(build_sdp(&params, 7, "fe80::1").contains("IN IPv6 fe80::1"));
    }

    #[test]
    fn host_offer_from_describe() {
        let sunshine = b"a=x-ss-general.featureFlags:3\na=x-ss-general.encryptionSupported:7\na=x-ss-general.encryptionRequested:1\na=x-nv-video[0].refPicInvalidation:1\nsprop-parameter-sets=AAAAAU\na=rtpmap:98 AV1/90000\n";
        assert_eq!(parse_offer(sunshine), HostOffer { encryption_supported: 7, encryption_requested: 1, ref_invalidation: true, hevc: true, av1: true });
        assert_eq!(parse_offer(b"a=x-ss-general.encryptionSupported:5\n"), HostOffer { encryption_supported: 5, ..Default::default() });
    }
}
