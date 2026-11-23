//! Fase H1. Server RTSP (TCP, dasar + 21): OPTIONS, DESCRIBE, SETUP, ANNOUNCE, PLAY, satu pesan per koneksi.
//!
//! Rujukan: `Sunshine/src/rtsp.cpp`. Beda dari Sunshine: HANYA RTSP terenkripsi (`rtspenc://`). `/launch` sudah
//! menolak client tanpa dukungan itu, jadi koneksi yang tidak bisa didekripsi dengan kunci sesi langsung ditutup.

use std::io::Write;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use cloudpc_stream_proto::control::{encryption, Origin};
use cloudpc_stream_proto::rtsp::{sdp_attributes, Request, Response};

use crate::nvhttp::Host;
use crate::session::{ConfigError, StreamConfig, StreamSession};
use crate::video::VideoCapabilities;

const IO_TIMEOUT: Duration = Duration::from_secs(10);

pub use cloudpc_stream_common::secure::{rtsp_read as read_sealed, rtsp_seal as seal};

pub(crate) fn serve(host: Arc<Host>, listener: TcpListener) {
    for stream in listener.incoming().flatten() {
        let host = host.clone();
        thread::spawn(move || handle(&host, stream));
    }
}

fn handle(host: &Host, mut stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    // Hanya dilayani selama ada sesi peluncuran yang masih berlaku; kuncinya satu-satunya cara membaca pesan.
    let pending = host.launch.lock().unwrap().as_ref().filter(|l| !l.expired()).map(|l| (l.id, l.key));
    let Some((launch_id, key)) = pending else { return };
    let Ok(plaintext) = read_sealed(&mut stream, &key, Origin::Client) else {
        host.log(|| "RTSP: pesan ditolak (bukan dari pemilik sesi peluncuran)".into());
        return;
    };
    let Ok(request) = Request::parse(&plaintext) else { return };
    let response = respond(host, launch_id, &request);
    host.log(|| format!("RTSP {} {} -> {}", request.command, request.target, response.status));

    let seq = {
        let mut launch = host.launch.lock().unwrap();
        let Some(launch) = launch.as_mut().filter(|l| l.id == launch_id) else { return };
        launch.rtsp_seq += 1;
        launch.rtsp_seq
    };
    let _ = stream.write_all(&seal(&key, seq, Origin::Host, &response.serialize()));
    let _ = stream.shutdown(Shutdown::Both);
}

fn respond(host: &Host, launch_id: u32, request: &Request) -> Response {
    let cseq = request.cseq();
    match request.command.as_str() {
        "OPTIONS" | "PLAY" => Response::new(200, "OK", cseq),
        "DESCRIBE" => Response::new(200, "OK", cseq).with_payload(describe(crate::video::capabilities(), host.mic_enabled())),
        "SETUP" => setup(host, launch_id, request),
        "ANNOUNCE" => announce(host, launch_id, request),
        _ => Response::new(404, "NOT FOUND", cseq),
    }
}

/// Kemampuan host. Enkripsi kontrol, video, dan audio didukung DAN diminta (wajib, lihat `StreamConfig::from_sdp`).
fn describe(video: VideoCapabilities, mic: bool) -> String {
    let all = encryption::CONTROL_V2 | encryption::VIDEO | encryption::AUDIO;
    let mut lines = vec![
        "a=x-ss-general.featureFlags:0".to_string(),
        format!("a=x-ss-general.encryptionSupported:{all}"),
        format!("a=x-ss-general.encryptionRequested:{all}"),
    ];
    // Client boleh mengirim "invalidasi frame rujukan" alih-alih meminta IDR saat paket hilang.
    if video.ref_invalidation {
        lines.push("a=x-nv-video[0].refPicInvalidation:1".to_string());
    }
    // Penanda HEVC dan AV1 yang dicari Moonlight; sejalan dengan ServerCodecModeSupport di /serverinfo.
    if video.hevc {
        lines.push("sprop-parameter-sets=AAAAAU".to_string());
    }
    if video.av1 {
        lines.push("a=rtpmap:98 AV1/90000".to_string());
    }
    // Opus stereo: 2 kanal, 1 stream, 1 coupled, pemetaan 0,1 (kualitas normal dan tinggi). Surround menyusul di H3.
    lines.push("a=fmtp:97 surround-params=21101".to_string());
    // Ekstensi CloudPC: host punya keluaran mikrofon, client boleh mengirim suara pemain (`proto::mic`).
    if mic {
        lines.push(format!("a={}:{}", cloudpc_stream_proto::mic::SDP_ATTRIBUTE, cloudpc_stream_proto::mic::VERSION));
    }
    lines.into_iter().map(|line| line + "\n").collect()
}

/// SETUP memberi tahu port tiap aliran dan data yang harus dibawa client saat menyambung ke sana.
fn setup(host: &Host, launch_id: u32, request: &Request) -> Response {
    let cseq = request.cseq();
    let launch = host.launch.lock().unwrap();
    let Some(launch) = launch.as_ref().filter(|l| l.id == launch_id) else { return Response::new(404, "NOT FOUND", cseq) };
    // Target berbentuk `streamid=video/0/0`.
    let kind = request.target.split_once('=').and_then(|(_, rest)| rest.split('/').next());
    let (port, extra, value) = match kind {
        Some("audio") => (host.ports.audio, "X-SS-Ping-Payload", launch.ping_payload.clone()),
        Some("video") => (host.ports.video, "X-SS-Ping-Payload", launch.ping_payload.clone()),
        Some("control") => (host.ports.control, "X-SS-Connect-Data", launch.connect_data.to_string()),
        _ => return Response::new(404, "NOT FOUND", cseq),
    };
    Response::new(200, "OK", cseq).with_header("Session", "DEADBEEFCAFE;timeout = 90").with_header("Transport", format!("server_port={port}")).with_header(extra, value)
}

/// ANNOUNCE membawa pilihan client; kalau diterima, sesi stream dibuat dan menunggu kanal kontrol.
fn announce(host: &Host, launch_id: u32, request: &Request) -> Response {
    let cseq = request.cseq();
    let config = match StreamConfig::from_sdp(&sdp_attributes(&request.payload)) {
        Ok(config) => config,
        Err(ConfigError::EncryptionRequired) => {
            host.log(|| "RTSP ANNOUNCE ditolak: client tidak menyalakan enkripsi penuh".into());
            return Response::new(403, "Forbidden", cseq);
        }
        Err(ConfigError::Bad(why)) => {
            host.log(|| format!("RTSP ANNOUNCE ditolak: {why}"));
            return Response::new(400, "BAD REQUEST", cseq);
        }
    };
    if !crate::video::capabilities().supports(config.video_format) {
        host.log(|| format!("RTSP ANNOUNCE ditolak: GPU tidak punya encoder untuk codec {}", config.video_format));
        return Response::new(400, "BAD REQUEST", cseq);
    }
    let session = {
        let launch = host.launch.lock().unwrap();
        let Some(launch) = launch.as_ref().filter(|l| l.id == launch_id) else { return Response::new(400, "BAD REQUEST", cseq) };
        StreamSession { launch_id, key: launch.key, key_id: launch.key_id, connect_data: launch.connect_data, ping_payload: launch.ping_payload.clone(), video_client: None, audio_client: None, client_name: launch.client_name.clone(), config, peer: None, out_seq: 0, messages: Default::default() }
    };
    host.log(|| {
        let c = &session.config;
        format!("sesi stream untuk \"{}\": {}x{}@{} {} kbps, codec {}, paket {} byte", session.client_name, c.width, c.height, c.fps, c.bitrate_kbps, c.video_format, c.packet_size)
    });
    // Sesi lama (bila ada) tergantikan; thread kontrol memutus peer-nya karena tidak lagi cocok dengan sesi aktif.
    *host.stream.lock().unwrap() = Some(session);
    Response::new(200, "OK", cseq)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cloudpc_stream_proto::rtsp::EncryptedHeader;

    #[test]
    fn seal_and_read_round_trip_and_reject_wrong_direction_or_key() {
        let key = [9u8; 16];
        let wire = seal(&key, 5, Origin::Client, b"OPTIONS rtspenc://x RTSP/1.0\r\nCSeq: 1\r\n\r\n");
        assert_eq!(&wire[..4], &[0x80, 0, 0, 41], "bit enkripsi + panjang ciphertext");
        assert_eq!(read_sealed(&mut &wire[..], &key, Origin::Client).unwrap(), b"OPTIONS rtspenc://x RTSP/1.0\r\nCSeq: 1\r\n\r\n");
        assert!(read_sealed(&mut &wire[..], &key, Origin::Host).is_err(), "pesan client tidak boleh terbaca sebagai pesan host");
        assert!(read_sealed(&mut &wire[..], &[8u8; 16], Origin::Client).is_err());
        assert!(read_sealed(&mut &wire[..wire.len() - 1], &key, Origin::Client).is_err());
    }

    #[test]
    fn oversized_length_is_refused_before_allocating() {
        let header = EncryptedHeader { length: 0x7FFF_FFFF, seq: 1, tag: [0; 16] }.write();
        assert!(read_sealed(&mut &header[..], &[0; 16], Origin::Client).is_err());
    }

    #[test]
    fn describe_requests_full_encryption_and_advertises_codecs() {
        let all = VideoCapabilities { h264: true, hevc: true, av1: true, ref_invalidation: true, probed: true };
        let sdp = describe(all, false);
        assert!(!sdp.contains("x-cloudpc-mic"), "tanpa keluaran mikrofon host tidak menawarkannya");
        assert_eq!(sdp_attributes(describe(all, true).as_bytes()).get("x-cloudpc-mic").map(String::as_str), Some("1"));
        let attrs = sdp_attributes(sdp.as_bytes());
        assert_eq!(attrs.get("x-ss-general.encryptionSupported").map(String::as_str), Some("7"));
        assert_eq!(attrs.get("x-ss-general.encryptionRequested").map(String::as_str), Some("7"));
        assert_eq!(attrs.get("x-nv-video[0].refPicInvalidation").map(String::as_str), Some("1"));
        assert!(sdp.contains("sprop-parameter-sets=AAAAAU\n") && sdp.contains("AV1/90000\n"));

        // GPU tanpa HEVC/AV1/RFI tidak boleh menawarkannya.
        let basic = describe(VideoCapabilities { hevc: false, av1: false, ref_invalidation: false, ..all }, false);
        assert!(!basic.contains("sprop-parameter-sets") && !basic.contains("AV1") && !basic.contains("refPicInvalidation"));
        assert!(basic.contains("encryptionRequested:7"));
    }
}
