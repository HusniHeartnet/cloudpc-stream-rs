//! Fase C1. Satu sesi stream dari awal sampai akhir: launch/resume, RTSP, kanal kontrol, lalu menerima aliran video
//! dan audio. Paket hanya dihitung (pemulihan FEC dan decode menyusul di C2/C3).

use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use cloudpc_stream_proto::control::encryption;
use cloudpc_stream_proto::rtp::write_ping;
use cloudpc_stream_proto::video::server_codec_mode;

use crate::control::{ControlEvent, ControlLink};
use crate::http;
use crate::nvhttp::{self, HostAddr};
use crate::rtsp::{self, Codec, StreamParams};
use crate::store::Store;
use crate::audio::{AudioDepacketizer, OpusDecoder};
use crate::input::InputPacket;
use crate::video::depacketize::{self, Depacketizer, Output, Recovery, Repair};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(Default)]
pub enum CodecChoice {
    #[default]
    Auto,
    Only(Codec),
}

#[derive(Debug, Clone, Default)]
pub struct StreamOptions {
    pub app: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
    pub codec: CodecChoice,
    /// Tulis bitstream frame yang lolos ke decoder ke file ini (Annex B untuk H.264/HEVC, OBU untuk AV1), untuk
    /// diperiksa dengan ffprobe/ffmpeg.
    pub dump_video: Option<std::path::PathBuf>,
    pub launch: nvhttp::LaunchExtras,
}

/// Tujuan frame video yang sudah utuh dan boleh didecode (jendela + decoder di thread lain, lihat `video::display`).
pub trait VideoSink {
    /// Dipanggil sekali setelah RTSP, sebelum frame pertama.
    fn start(&mut self, codec: Codec);
    fn frame(&mut self, frame: depacketize::VideoFrame);
    /// Decoder gagal sejak pemanggilan terakhir: minta IDR ke host.
    fn take_idr_request(&mut self) -> bool;
    /// Pengguna menutup jendela: sesi diakhiri.
    fn closed(&self) -> bool;
    /// Paket input yang menunggu dikirim ke host.
    fn take_input(&mut self) -> Vec<InputPacket> {
        Vec::new()
    }
    /// Codec yang bisa didecode GPU client (`None` = tidak diketahui). Pilihan "auto" hanya memakai HEVC/AV1 yang
    /// didukung GPU; sisanya H.264 (bila perlu didecode CPU).
    fn hardware_codecs(&self) -> Option<Vec<Codec>> {
        None
    }
    /// Getar gamepad dari host.
    fn rumble(&mut self, _pad: u16, _low: u16, _high: u16) {}
    /// Tiap detik: ringkasan jaringan (untuk overlay statistik) dan peringatan koneksi lambat bila ada.
    fn status(&mut self, _network: String, _warning: Option<String>) {}
    /// Tiap detik, bersama `status`: angka ringkas satu detik terakhir (FPS diterima, Mbit/dtk, waktu pulang-pergi ms).
    fn numbers(&mut self, _fps: f64, _mbps: f64, _rtt_ms: u32) {}
}

/// Tujuan suara yang sudah didecode (PCM float stereo 48 kHz, berselang kiri/kanan).
pub trait AudioSink {
    fn samples(&mut self, pcm: &[f32]);
}

/// Sumber suara mikrofon pemain: paket Opus 20 ms (`proto::mic`) yang siap dikirim.
pub trait MicSource {
    fn take_packets(&mut self) -> Vec<Vec<u8>>;
    /// Galat perangkat rekam sejak pemanggilan terakhir, untuk log.
    fn take_trouble(&mut self) -> Option<String> {
        None
    }
}

/// Kejadian penting selama sesi, untuk log bergaya Moonlight (`main.rs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// RTSP selesai: resolusi, FPS, `VIDEO_FORMAT_*`, bitrate.
    Negotiated { width: u32, height: u32, fps: u32, video_format: u32, bitrate_kbps: u32 },
    FirstVideoPacket(Duration),
    /// Invalidasi frame rujukan dikirim untuk frame ini.
    RfiRequest(u32),
    /// Getar gamepad pertama dari host di sesi ini (dicatat sekali).
    FirstRumble { pad: u16, low: u16, high: u16 },
}

#[derive(Debug, Clone, Default)]
pub struct StreamReport {
    pub resumed: bool,
    pub codec: Option<Codec>,
    pub encryption: u32,
    pub packet_size: u32,
    pub connected_after: Duration,
    pub duration: Duration,
    pub video_packets: u64,
    pub video_bytes: u64,
    pub audio_packets: u64,
    /// Paket video yang tidak pernah tiba (dari penghitung IV enkripsi video); `None` bila video tidak terenkripsi.
    pub video_lost: Option<u64>,
    /// Hasil depaketisasi: frame utuh, frame hilang, shard yang dipulihkan FEC.
    pub video: depacketize::Stats,
    /// Frame yang diberikan ke decoder (frame utuh dikurangi yang ditahan menunggu frame pemulih).
    pub frames_decodable: u64,
    pub frames_held: u64,
    pub invalidations_sent: u64,
    pub idr_requests_sent: u64,
    /// Rata-rata lama tangkap + encode di host per frame (laporan host), milidetik.
    pub host_latency_ms: Option<f64>,
    /// Minimum dan maksimum latensi host, milidetik.
    pub host_latency_range_ms: Option<(f64, f64)>,
    /// Waktu pulang-pergi ENet ke host (rata-rata sampel tiap detik) dan variasinya, milidetik.
    pub network_rtt_ms: Option<(f64, f64)>,
    pub audio: crate::audio::AudioStats,
    pub input_packets: u64,
    /// Paket suara mikrofon yang dikirim ke host.
    pub mic_packets: u64,
    /// Mikrofon diminta tetapi host tidak menawarkannya (Sunshine, atau host tanpa perangkat mikrofon).
    pub mic_refused: bool,
    pub rumble_messages: u64,
    pub host_messages: u64,
    pub termination: Option<u32>,
    pub disconnected: bool,
}

/// Angka kumulatif sesi pada satu titik waktu (diambil tiap detik).
#[derive(Debug, Default, Clone, Copy)]
pub struct StatusSample {
    pub frames: u64,
    pub frames_lost: u64,
    pub frames_recovered: u64,
    pub packets: u64,
    pub packets_lost: u64,
    pub bytes: u64,
    pub latency_sum: u64,
    pub latency_frames: u64,
    /// Waktu pulang-pergi ENet dan variasinya, ms.
    pub rtt: (u32, u32),
}

/// Statistik per detik dan peringatan koneksi lambat (seperti overlay dan "Slow connection to PC" Moonlight).
#[derive(Default)]
pub struct StatusWindow {
    /// Sampel tiap detik, paling banyak 4 (jendela 3 detik).
    history: std::collections::VecDeque<StatusSample>,
}

impl StatusWindow {
    pub fn update(&mut self, sample: StatusSample, params: &StreamParams) -> (String, Option<String>) {
        self.history.push_back(sample);
        while self.history.len() > 4 {
            self.history.pop_front();
        }
        let first = self.history.front().copied().unwrap_or_default();
        // Sampel pertama dibandingkan dengan nol (awal sesi).
        let previous = self.history.iter().rev().nth(1).copied().unwrap_or_default();
        let seconds = (self.history.len() as f64 - 1.0).max(1.0);
        // Satu detik terakhir untuk angka yang ditampilkan.
        let fps = sample.frames.saturating_sub(previous.frames) as f64;
        let mbps = sample.bytes.saturating_sub(previous.bytes) as f64 * 8.0 / 1e6;
        let host_ms = match sample.latency_frames.saturating_sub(previous.latency_frames) {
            0 => None,
            n => Some(sample.latency_sum.saturating_sub(previous.latency_sum) as f64 / n as f64 / 10.0),
        };
        // Tiga detik terakhir untuk peringatan (satu detik buruk saja tidak cukup).
        let frames = sample.frames.saturating_sub(first.frames) + sample.frames_lost.saturating_sub(first.frames_lost);
        let frame_loss = if frames > 0 { sample.frames_lost.saturating_sub(first.frames_lost) as f64 * 100.0 / frames as f64 } else { 0.0 };
        let packets = sample.packets.saturating_sub(first.packets) + sample.packets_lost.saturating_sub(first.packets_lost);
        let packet_loss = if packets > 0 { sample.packets_lost.saturating_sub(first.packets_lost) as f64 * 100.0 / packets as f64 } else { 0.0 };
        let recovered = sample.frames_recovered.saturating_sub(first.frames_recovered) as f64 / seconds;
        let mut text = format!(
            "Video: {}x{} {:?}, {} FPS diminta\nDiterima: {fps:.0} FPS, {mbps:.1} Mbit/dtk\nPaket hilang: {packet_loss:.1}% (dipulihkan FEC: {recovered:.0} frame/dtk)\nFrame hilang: {frame_loss:.1}%\nLatensi jaringan: {} ms (variasi {} ms)",
            params.width, params.height, params.codec, params.fps, sample.rtt.0, sample.rtt.1
        );
        if let Some(ms) = host_ms {
            text.push_str(&format!("\nLatensi host: {ms:.1} ms"));
        }
        let poor = self.history.len() >= 3 && (frame_loss >= 5.0 || packet_loss >= 10.0 || sample.rtt.0 > 150);
        (text, poor.then(|| "Koneksi ke PC lambat: coba turunkan bitrate".to_string()))
    }

    /// Angka satu detik terakhir untuk tampilan ringkas: (FPS diterima, Mbit/dtk, waktu pulang-pergi ms).
    pub fn latest(&self) -> (f64, f64, u32) {
        let Some(last) = self.history.back().copied() else { return (0.0, 0.0, 0) };
        let previous = self.history.iter().rev().nth(1).copied().unwrap_or_default();
        (last.frames.saturating_sub(previous.frames) as f64, last.bytes.saturating_sub(previous.bytes) as f64 * 8.0 / 1e6, last.rtt.0)
    }
}

/// Menghitung paket video yang hilang dari penghitung IV di awal setiap paket terenkripsi (`[iv 12]...`, 8 byte
/// pertama = nomor urut paket little-endian, naik satu per paket termasuk paritas FEC; host CloudPC dan Sunshine sama).
/// Paket yang datang tidak berurutan tetap terhitung benar; duplikat diabaikan karena sangat jarang.
#[derive(Debug, Default, Clone, Copy)]
pub struct VideoLoss {
    range: Option<(u64, u64)>,
    received: u64,
}

impl VideoLoss {
    pub fn record(&mut self, packet: &[u8]) {
        let Some(head) = packet.get(..8) else { return };
        let counter = u64::from_le_bytes(head.try_into().unwrap());
        self.range = Some(match self.range {
            None => (counter, counter),
            Some((low, high)) => (low.min(counter), high.max(counter)),
        });
        self.received += 1;
    }

    pub fn lost(&self) -> u64 {
        self.range.map_or(0, |(low, high)| (high - low + 1).saturating_sub(self.received))
    }
}

/// Alamat LAN/loopback memakai paket besar; lewat internet paket lebih kecil supaya muat di MTU terowongan.
pub fn packet_size_for(ip: IpAddr) -> u32 {
    let local = match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_loopback() || v4.is_link_local(),
        IpAddr::V6(v6) => v6.is_loopback() || (v6.segments()[0] & 0xffc0) == 0xfe80 || (v6.segments()[0] & 0xfe00) == 0xfc00,
    };
    if local {
        1392
    } else {
        1024
    }
}

/// H.264 pada decoder hardware: minta host memakai satu frame rujukan, sehingga frame yang hilang dijawab IDR dan
/// bukan invalidasi frame rujukan (P-frame yang merujuk gambar lebih lama). Sama dengan Moonlight, yang hanya memakai
/// invalidasi H.264 pada decoder yang menyatakan sanggup (`CAPABILITY_REFERENCE_FRAME_INVALIDATION_AVC`): setelah
/// invalidasi decoder menerima aliran dengan nomor frame yang melompat, dan decoder GPU tertentu lalu menampilkan
/// gambar lama berulang-ulang atau gelap sampai IDR berikutnya (sesi #117, 24 Nov 2026, grafis terpadu AMD Radeon 660M;
/// pada NVIDIA dan decoder perangkat lunak FFmpeg hasilnya benar, tetapi client tidak memilah merek GPU). AV1: gejala yang
/// sama dilaporkan 26 Nov 2026 di PC dan HP; AV1 SELALU satu frame rujukan, juga saat didecode perangkat lunak (dav1d di
/// Mac M1/M2, decoder perangkat lunak MediaCodec di HP tanpa AV1 hardware: keduanya belum diuji dengan invalidasi).
/// Hanya HEVC (dan H.264 di CPU) yang tetap memakai invalidasi. `hardware`: codec yang didecode hardware oleh penampil;
/// `None` = tanpa penampil (rekam saja).
pub fn needs_single_reference(codec: Codec, hardware: Option<&[Codec]>) -> bool {
    match codec {
        Codec::H264 => hardware.is_some_and(|codecs| codecs.contains(&Codec::H264)),
        Codec::Av1 => hardware.is_some(),
        Codec::Hevc => false,
    }
}

/// Codec akhir dari pilihan pengguna dan yang dinyatakan host (`ServerCodecModeSupport` + DESCRIBE).
pub fn pick_codec(choice: CodecChoice, server_codecs: u32, offer: &rtsp::HostOffer, client: Option<&[Codec]>) -> Result<Codec, String> {
    let hevc = server_codecs & server_codec_mode::HEVC != 0 || offer.hevc;
    let av1 = server_codecs & server_codec_mode::AV1_MAIN8 != 0 || offer.av1;
    let client_hevc = client.is_none_or(|c| c.contains(&Codec::Hevc));
    match choice {
        CodecChoice::Auto if hevc && client_hevc => Ok(Codec::Hevc),
        CodecChoice::Auto => Ok(Codec::H264),
        CodecChoice::Only(Codec::Hevc) if !hevc => Err("host tidak mendukung HEVC".into()),
        CodecChoice::Only(Codec::Av1) if !av1 => Err("host tidak mendukung AV1".into()),
        CodecChoice::Only(codec) => Ok(codec),
    }
}

const UDP_PING_INTERVAL: Duration = Duration::from_millis(500);

/// Penyangga terima UDP, sama dengan `RTP_RECV_BUFFER` Moonlight (512 KiB). Bawaan Windows 64 KiB: semburan frame
/// besar (IDR) dibuang kernel sebelum sempat dibaca.
const UDP_RECV_BUFFER: usize = 512 * 1024;

fn udp_to(target: SocketAddr) -> Result<UdpSocket, String> {
    let socket = UdpSocket::bind(if target.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" }).map_err(|e| format!("soket UDP: {e}"))?;
    let socket = socket2::Socket::from(socket);
    let _ = socket.set_recv_buffer_size(UDP_RECV_BUFFER);
    socket.set_nonblocking(true).map_err(|e| format!("soket UDP: {e}"))?;
    Ok(socket.into())
}

/// Jalankan sesi sampai `stop()` bernilai true, host mengakhirinya, atau kanal kontrol putus.
/// `progress` menerima kalimat singkat untuk ditampilkan.
pub fn run(addr: &HostAddr, store: &Store, options: &StreamOptions, stop: impl FnMut() -> bool, progress: impl FnMut(String)) -> Result<StreamReport, String> {
    run_with(addr, store, options, Outputs::default(), stop, progress)
}

/// Ke mana frame video, suara, dan kejadian sesi dikirim.
#[derive(Default)]
pub struct Outputs<'a> {
    pub video: Option<&'a mut dyn VideoSink>,
    pub audio: Option<&'a mut dyn AudioSink>,
    pub events: Option<&'a mut dyn FnMut(Event)>,
    pub mic: Option<&'a mut dyn MicSource>,
}

/// Seperti `run`, dengan frame dikirim ke `sink` (decode + tampil). Sesi juga berakhir bila jendela `sink` ditutup.
pub fn run_with(
    addr: &HostAddr,
    store: &Store,
    options: &StreamOptions,
    outputs: Outputs<'_>,
    mut stop: impl FnMut() -> bool,
    mut progress: impl FnMut(String),
) -> Result<StreamReport, String> {
    let started = Instant::now();
    let ip = http::resolve(&addr.host, addr.http_port)?.ip();
    let packet_size = packet_size_for(ip);

    let (info, launched) = nvhttp::launch_with(addr, store, &options.app, options.width, options.height, options.fps, options.launch)?;
    progress(format!("{} {} di {} ({})", if launched.resumed { "Melanjutkan" } else { "Memulai" }, options.app, info.hostname, launched.session_url));

    let Outputs { video: mut sink, audio: mut audio_sink, events: mut event_sink, mic: mut mic_source } = outputs;
    let mut emit = |event: Event| {
        if let Some(sink) = event_sink.as_deref_mut() {
            sink(event);
        }
    };
    let client_codecs = sink.as_deref().and_then(|s| s.hardware_codecs());
    let (params, negotiated) = rtsp::handshake(&addr.host, &launched.session_url, launched.key, |offer| {
        let codec = pick_codec(options.codec, info.codecs, offer, client_codecs.as_deref())?;
        let single_reference = needs_single_reference(codec, client_codecs.as_deref());
        Ok(StreamParams { width: options.width, height: options.height, fps: options.fps, bitrate_kbps: options.bitrate_kbps, codec, packet_size, single_reference })
    })?;
    progress(format!(
        "RTSP selesai: {:?} {}x{}@{} {} kbps, enkripsi {}, frame hilang dipulihkan dengan {}",
        params.codec,
        params.width,
        params.height,
        params.fps,
        params.bitrate_kbps,
        negotiated.encryption,
        if negotiated.offer.ref_invalidation && !params.single_reference { "invalidasi frame rujukan" } else { "IDR" }
    ));

    let video_target = SocketAddr::new(ip, negotiated.video_port);
    let audio_target = SocketAddr::new(ip, negotiated.audio_port);
    let (video, audio) = (udp_to(video_target)?, udp_to(audio_target)?);
    let mut link = ControlLink::connect(SocketAddr::new(ip, negotiated.control_port), negotiated.connect_data, launched.key, Duration::from_secs(10))?;
    let connected_after = started.elapsed();
    // Timer 0,5 ms, prioritas tinggi, Wi-Fi tanpa pemindaian latar selama stream (moonlight-common-c melakukan yang sama).
    #[cfg(windows)]
    let _mode = cloudpc_stream_common::streaming_mode::StreamingMode::enter(cloudpc_stream_common::streaming_mode::Options { host: false });
    // macOS: tanpa App Nap selama stream, supaya jendela yang tertutup jendela lain (chat TikTok LIVE) tetap lancar dan
    // tetap bisa diambil Window Capture.
    #[cfg(target_os = "macos")]
    let _activity = crate::macos_activity::Activity::begin("CloudPC streaming");
    progress(format!("Kanal kontrol tersambung setelah {} ms", connected_after.as_millis()));

    let mut report = StreamReport { resumed: launched.resumed, codec: Some(params.codec), encryption: negotiated.encryption, packet_size, connected_after, ..Default::default() };
    let mut buffer = vec![0u8; 4096];
    // Ping audio digeser setengah interval dari ping video. Sunshine sebelum c101250 (2 Nov 2026, termasuk 2026.914 di
    // PC uji) memakai satu endpoint untuk dua penerimaan UDP-nya: ping video dan audio yang tiba hampir bersamaan
    // membuat video dikirim ke port audio (lewat NAT VPS = video hilang). Moonlight mengirim keduanya dari thread terpisah.
    let start = Instant::now();
    let (mut next_ping, mut next_progress) = (start, start + Duration::from_secs(2));
    let (mut next_video_ping, mut next_audio_ping) = (start, start + UDP_PING_INTERVAL / 2);
    let (mut video_ping_seq, mut audio_ping_seq) = (0u32, 0u32);
    let mut last_video = 0u64;
    let mut loss = VideoLoss::default();
    let video_encrypted = negotiated.encryption & encryption::VIDEO != 0;
    let mut depacketizer = Depacketizer::new(packet_size, video_encrypted.then_some(launched.key));
    // Satu frame rujukan = host tidak bisa menginvalidasi; langsung minta IDR, jangan menunggu jawaban yang tidak datang.
    let mut recovery = Recovery::new(negotiated.offer.ref_invalidation && !params.single_reference);
    let mut dump = match &options.dump_video {
        Some(path) => Some(std::io::BufWriter::new(std::fs::File::create(path).map_err(|e| format!("file {}: {e}", path.display()))?)),
        None => None,
    };
    let (mut latency_sum, mut latency_frames) = (0u64, 0u64);
    let mut last_frames = 0u64;
    if let Some(sink) = sink.as_deref_mut() {
        sink.start(params.codec);
    }
    let video_format = match params.codec {
        Codec::H264 => cloudpc_stream_proto::video::format::H264,
        Codec::Hevc => cloudpc_stream_proto::video::format::H265,
        Codec::Av1 => cloudpc_stream_proto::video::format::AV1_MAIN8,
    };
    emit(Event::Negotiated { width: params.width, height: params.height, fps: params.fps, video_format, bitrate_kbps: params.bitrate_kbps });
    let audio_key = (negotiated.encryption & encryption::AUDIO != 0).then_some((launched.key, launched.key_id));
    let mut audio_depacketizer = AudioDepacketizer::new(audio_key);
    let mut opus = match audio_sink.is_some() {
        true => Some(OpusDecoder::new()?),
        false => None,
    };
    let (mut latency_min, mut latency_max) = (u16::MAX, 0u16);
    let (mut rtt_sum, mut rtt_var_sum, mut rtt_samples) = (0u64, 0u64, 0u64);
    let mut next_rtt = start + Duration::from_secs(1);
    let mut status = StatusWindow::default();
    let mut mic_sequence = 0u16;
    if mic_source.is_some() {
        report.mic_refused = !negotiated.offer.mic;
        progress(if negotiated.offer.mic { "Mikrofon: host menerima suara pemain".into() } else { "Mikrofon: host ini tidak mendukung mikrofon, suara tidak dikirim".to_string() });
    }

    while !stop() && !sink.as_deref().is_some_and(|s| s.closed()) {
        let now = Instant::now();
        if let Some(video_sink) = sink.as_deref_mut() {
            for packet in video_sink.take_input() {
                link.send_input(packet.channel, &packet.data);
                report.input_packets += 1;
            }
        }
        if let Some(mic) = mic_source.as_deref_mut() {
            // Antrean selalu dikosongkan; ke host yang tidak menawarkan mikrofon paketnya dibuang.
            for packet in mic.take_packets() {
                if negotiated.offer.mic {
                    link.send_mic(mic_sequence, &packet);
                    mic_sequence = mic_sequence.wrapping_add(1);
                    report.mic_packets += 1;
                }
            }
            if let Some(trouble) = mic.take_trouble() {
                progress(format!("Mikrofon: {trouble}"));
            }
        }
        if now >= next_rtt {
            let (rtt, variance) = link.round_trip();
            let last_rtt = (rtt, variance);
            if rtt > 0 {
                rtt_sum += u64::from(rtt);
                rtt_var_sum += u64::from(variance);
                rtt_samples += 1;
            }
            next_rtt = now + Duration::from_secs(1);
            if let Some(sink) = sink.as_deref_mut() {
                let lost_packets = if video_encrypted { loss.lost() } else { 0 };
                let sample = StatusSample {
                    frames: depacketizer.stats().frames,
                    frames_lost: depacketizer.stats().frames_lost,
                    frames_recovered: depacketizer.stats().frames_recovered,
                    packets: report.video_packets,
                    packets_lost: lost_packets,
                    bytes: report.video_bytes,
                    latency_sum,
                    latency_frames,
                    rtt: last_rtt,
                };
                let (text, warning) = status.update(sample, &params);
                sink.status(text, warning);
                let (fps, mbps, rtt_ms) = status.latest();
                sink.numbers(fps, mbps, rtt_ms);
            }
        }
        if sink.as_deref_mut().is_some_and(|s| s.take_idr_request()) {
            recovery.need_idr();
            link.request_idr();
            report.idr_requests_sent += 1;
        }
        if now >= next_ping {
            link.ping();
            next_ping = now + Duration::from_millis(100);
        }
        // Ping ke port video dan audio: dari situ host tahu ke mana aliran dikirim (juga menembus NAT).
        if now >= next_video_ping {
            video_ping_seq += 1;
            let _ = video.send_to(&write_ping(&negotiated.ping_payload, video_ping_seq), video_target);
            next_video_ping = now + UDP_PING_INTERVAL;
        }
        if now >= next_audio_ping {
            audio_ping_seq += 1;
            let _ = audio.send_to(&write_ping(&negotiated.ping_payload, audio_ping_seq), audio_target);
            next_audio_ping = now + UDP_PING_INTERVAL;
        }
        while let Ok((length, _)) = video.recv_from(&mut buffer) {
            if report.video_packets == 0 {
                emit(Event::FirstVideoPacket(started.elapsed()));
            }
            report.video_packets += 1;
            report.video_bytes += length as u64;
            if video_encrypted {
                loss.record(&buffer[..length]);
            }
            for output in depacketizer.push(&buffer[..length]) {
                let repair = match output {
                    Output::Lost { first, last } => Some(recovery.lost(first, last)),
                    Output::Frame(frame) => {
                        if frame.host_latency > 0 {
                            latency_sum += u64::from(frame.host_latency);
                            latency_frames += 1;
                            latency_min = latency_min.min(frame.host_latency);
                            latency_max = latency_max.max(frame.host_latency);
                        }
                        let (admit, repair) = recovery.admit(frame.frame_type);
                        if admit {
                            report.frames_decodable += 1;
                            if let Some(file) = dump.as_mut() {
                                std::io::Write::write_all(file, &frame.data).map_err(|e| format!("menulis video: {e}"))?;
                            }
                            if let Some(sink) = sink.as_deref_mut() {
                                sink.frame(frame);
                            }
                        }
                        repair
                    }
                };
                match repair {
                    Some(Repair::Invalidate { first, last }) => {
                        emit(Event::RfiRequest(last));
                        link.invalidate_reference_frames(first, last);
                        report.invalidations_sent += 1;
                    }
                    Some(Repair::Idr) => {
                        link.request_idr();
                        report.idr_requests_sent += 1;
                    }
                    None => {}
                }
            }
        }
        while let Ok((length, _)) = audio.recv_from(&mut buffer) {
            report.audio_packets += 1;
            for frame in audio_depacketizer.push(&buffer[..length]) {
                if let (Some(decoder), Some(sink)) = (opus.as_mut(), audio_sink.as_deref_mut()) {
                    sink.samples(&decoder.decode(&frame));
                }
            }
        }
        for event in link.pump(Duration::from_millis(2)) {
            match event {
                ControlEvent::Terminated(code) => report.termination = Some(code),
                ControlEvent::Disconnected => report.disconnected = true,
                ControlEvent::Rumble { pad, low, high } => {
                    if report.rumble_messages == 0 {
                        emit(Event::FirstRumble { pad, low, high });
                    }
                    report.rumble_messages += 1;
                    if let Some(sink) = sink.as_deref_mut() {
                        sink.rumble(pad, low, high);
                    }
                }
                ControlEvent::Message(_) => report.host_messages += 1,
            }
        }
        if report.termination.is_some() || report.disconnected {
            break;
        }
        if now >= next_progress {
            let rate = (report.video_packets - last_video) / 2;
            last_video = report.video_packets;
            let stats = depacketizer.stats();
            let fps = (stats.frames - last_frames) as f64 / 2.0;
            last_frames = stats.frames;
            progress(format!(
                "Video {} paket ({rate}/dtk, {:.1} MB), {} frame ({fps:.1} FPS, {} hilang, {} dipulihkan FEC), audio {} paket",
                report.video_packets,
                report.video_bytes as f64 / 1e6,
                stats.frames,
                stats.frames_lost,
                stats.frames_recovered,
                report.audio_packets
            ));
            next_progress = now + Duration::from_secs(2);
            for line in crate::video::take_notes() {
                progress(line);
            }
        }
    }
    report.duration = started.elapsed();
    report.video_lost = video_encrypted.then(|| loss.lost());
    report.video = depacketizer.stats();
    report.frames_held = recovery.held;
    report.host_latency_ms = (latency_frames > 0).then(|| latency_sum as f64 / latency_frames as f64 / 10.0);
    report.host_latency_range_ms = (latency_frames > 0).then(|| (f64::from(latency_min) / 10.0, f64::from(latency_max) / 10.0));
    report.network_rtt_ms = (rtt_samples > 0).then(|| (rtt_sum as f64 / rtt_samples as f64, rtt_var_sum as f64 / rtt_samples as f64));
    report.audio = audio_depacketizer.stats();
    if let Some(mut file) = dump {
        std::io::Write::flush(&mut file).map_err(|e| format!("menulis video: {e}"))?;
    }
    if !report.disconnected && report.termination.is_none() {
        link.close();
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(counter: u64) -> Vec<u8> {
        let mut bytes = counter.to_le_bytes().to_vec();
        bytes.extend_from_slice(&[0, 0, 0, b'V', 9, 9]);
        bytes
    }

    #[test]
    fn status_warns_only_after_sustained_loss() {
        let params = StreamParams { width: 1920, height: 1080, fps: 60, bitrate_kbps: 20_000, codec: Codec::Hevc, packet_size: 1392, single_reference: false };
        let mut window = StatusWindow::default();
        let mut sample = StatusSample { rtt: (20, 2), ..Default::default() };
        let mut warnings = Vec::new();
        for second in 0..9u64 {
            sample.frames += 60;
            sample.packets += 400;
            if (3..6).contains(&second) {
                sample.frames_lost += 10; // ±14% frame hilang selama tiga detik
            }
            let (text, warning) = window.update(sample, &params);
            assert!(text.contains("Diterima: 60 FPS") && text.contains("1920x1080 Hevc"), "{text}");
            warnings.push(warning.is_some());
        }
        // Kehilangan terakhir di detik 5; jendela 3 detik bersih lagi di detik 8.
        assert_eq!(warnings, [false, false, false, true, true, true, true, true, false], "peringatan menyala selama kehilangan, padam 3 detik kemudian");
    }

    #[test]
    fn h264_on_a_hardware_decoder_asks_for_one_reference_frame() {
        let all = [Codec::H264, Codec::Hevc, Codec::Av1];
        // H.264 yang didecode GPU: satu frame rujukan (frame hilang dijawab IDR), seperti Moonlight.
        assert!(needs_single_reference(Codec::H264, Some(&all)));
        // HEVC dan AV1 tetap memakai invalidasi frame rujukan.
        assert!(!needs_single_reference(Codec::Hevc, Some(&all)));
        assert!(needs_single_reference(Codec::Av1, Some(&all)));
        assert!(needs_single_reference(Codec::Av1, Some(&[Codec::H264, Codec::Hevc])));
        assert!(!needs_single_reference(Codec::Av1, None));
        // H.264 di CPU (GPU tanpa decoder H.264, atau `--video-decoder software`) dan tanpa penampil: tidak dibatasi.
        assert!(!needs_single_reference(Codec::H264, Some(&[Codec::Hevc])));
        assert!(!needs_single_reference(Codec::H264, Some(&[])));
        assert!(!needs_single_reference(Codec::H264, None));
    }

    #[test]
    fn video_loss_counts_holes_even_out_of_order() {
        let mut loss = VideoLoss::default();
        assert_eq!(loss.lost(), 0);
        for counter in [5, 6, 9, 7, 12] {
            loss.record(&packet(counter));
        }
        loss.record(&[1, 2, 3]);
        assert_eq!(loss.lost(), 3, "8, 10, 11 tidak pernah tiba");
    }

    #[test]
    fn packet_size_depends_on_where_the_host_is() {
        assert_eq!(packet_size_for("192.168.1.20".parse().unwrap()), 1392);
        assert_eq!(packet_size_for("10.8.0.2".parse().unwrap()), 1392);
        assert_eq!(packet_size_for("127.0.0.1".parse().unwrap()), 1392);
        assert_eq!(packet_size_for("203.0.113.10".parse().unwrap()), 1024);
        assert_eq!(packet_size_for("2402:8780::1".parse().unwrap()), 1024);
        assert_eq!(packet_size_for("fe80::1".parse().unwrap()), 1392);
    }

    #[test]
    fn codec_choice_respects_the_host() {
        let none = rtsp::HostOffer::default();
        let all = server_codec_mode::H264 | server_codec_mode::HEVC | server_codec_mode::AV1_MAIN8;
        assert_eq!(pick_codec(CodecChoice::Auto, all, &none, None), Ok(Codec::Hevc));
        assert_eq!(pick_codec(CodecChoice::Auto, server_codec_mode::H264, &none, None), Ok(Codec::H264));
        assert_eq!(pick_codec(CodecChoice::Only(Codec::Av1), all, &none, None), Ok(Codec::Av1));
        assert!(pick_codec(CodecChoice::Only(Codec::Av1), server_codec_mode::H264, &none, None).is_err());
        assert_eq!(pick_codec(CodecChoice::Only(Codec::H264), server_codec_mode::H264, &none, None), Ok(Codec::H264));
        // GPU client tanpa decoder HEVC: "auto" memilih H.264 walau host mendukung HEVC (seperti Moonlight).
        assert_eq!(pick_codec(CodecChoice::Auto, all, &none, Some(&[Codec::H264])), Ok(Codec::H264));
        assert_eq!(pick_codec(CodecChoice::Auto, all, &none, Some(&[])), Ok(Codec::H264), "tanpa decoder GPU sama sekali: H.264 di CPU");
        // Pilihan paksa tetap dihormati (didecode CPU bila GPU tidak bisa).
        assert_eq!(pick_codec(CodecChoice::Only(Codec::Hevc), all, &none, Some(&[Codec::H264])), Ok(Codec::Hevc));
    }
}
