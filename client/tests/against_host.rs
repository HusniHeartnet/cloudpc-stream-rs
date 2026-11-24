//! Client sungguhan melawan host CloudPC Rust sungguhan (di proses yang sama, 127.0.0.1): pairing, daftar aplikasi,
//! satu sesi stream singkat, lalu quit. Host memakai mode uji: input hanya dicatat, audio hening.

use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use cloudpc_stream_client::nvhttp::{self, HostAddr};
use cloudpc_stream_client::session::{self, CodecChoice, StreamOptions};
use cloudpc_stream_client::store::Store;
use cloudpc_stream_host::nvhttp::{self as host_http, Host, HostConfig};
use cloudpc_stream_host::state::write_pin;
use cloudpc_stream_proto::control::packet_type;

static NEXT_BASE: AtomicU16 = AtomicU16::new(0);

struct Rig {
    host: Arc<Host>,
    addr: HostAddr,
    state: tempfile::TempDir,
    data: tempfile::TempDir,
}

/// Tes yang benar-benar men-stream bergiliran: dua host uji di satu proses berebut tangkapan layar (Desktop
/// Duplication) dan yang kedua gagal.
static STREAMING: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn streaming() -> std::sync::MutexGuard<'static, ()> {
    STREAMING.lock().unwrap_or_else(|e| e.into_inner())
}

fn rig() -> Rig {
    rig_with(false)
}

fn rig_with(test_rumble: bool) -> Rig {
    rig_full(test_rumble, None)
}

fn rig_full(test_rumble: bool, mic: Option<cloudpc_stream_host::mic::MicTarget>) -> Rig {
    rig_custom(test_rumble, mic, (0, 1))
}

/// `drop_video`: (tiap berapa paket, berapa paket berturut-turut) yang dibuang host; (0, 1) = tidak ada.
fn rig_custom(test_rumble: bool, mic: Option<cloudpc_stream_host::mic::MicTarget>, drop_video: (u32, u32)) -> Rig {
    let state = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let start = 31000 + (std::process::id() % 1500) as u16 * 10;
    for _ in 0..50 {
        let base = start + NEXT_BASE.fetch_add(30, Ordering::SeqCst);
        let cfg = HostConfig {
            bind: "127.0.0.1".parse().unwrap(),
            base_port: base,
            name: "PC Uji".into(),
            state_dir: state.path().to_path_buf(),
            uniqueid: None,
            pin_wait: Duration::from_secs(10),
            verbose: false,
            test_drop_video_every: drop_video.0,
            test_drop_video_burst: drop_video.1,
            test_silent_audio: true,
            test_drop_audio_every: 0,
            test_record_input: true,
            test_rumble,
            mic: mic.clone(),
        };
        if let Ok(host) = host_http::start(cfg) {
            return Rig { host, addr: HostAddr { host: "127.0.0.1".into(), http_port: base }, state, data };
        }
    }
    panic!("tidak menemukan port bebas untuk host uji");
}

/// Pairing dari client; PIN dititipkan ke host sedikit kemudian, seperti agent saat sesi dimulai.
fn pair(rig: &Rig, host_pin: &str, client_pin: &str) -> Result<String, String> {
    let state = rig.state.path().to_path_buf();
    let host_pin = host_pin.to_string();
    let feeder = thread::spawn(move || {
        thread::sleep(Duration::from_millis(300));
        write_pin(&state, &host_pin, "cloudpc-uji").unwrap();
    });
    let mut store = Store::open(rig.data.path()).unwrap();
    let result = nvhttp::pair(&rig.addr, &mut store, client_pin);
    feeder.join().unwrap();
    result
}

#[test]
fn wrong_pin_is_detected_by_the_client_and_nothing_is_remembered() {
    let rig = rig();
    let err = pair(&rig, "1111", "2222").unwrap_err();
    assert!(err.contains("PIN salah"), "{err}");
    assert!(rig.host.paired_names().is_empty());
    let store = Store::open(rig.data.path()).unwrap();
    let info = nvhttp::server_info(&rig.addr, &store).unwrap();
    assert!(store.host(&info.uniqueid).is_none(), "host yang gagal dipairing tidak boleh diingat");
    assert!(nvhttp::apps(&rig.addr, &store).unwrap_err().contains("belum dipairing"));
}

#[test]
fn pair_list_stream_and_quit() {
    let _turn = streaming();
    let rig = rig();
    assert_eq!(pair(&rig, "4821", "4821").unwrap(), "PC Uji");
    assert_eq!(rig.host.paired_names(), ["cloudpc-uji"]);

    let store = Store::open(rig.data.path()).unwrap();
    let apps = nvhttp::apps(&rig.addr, &store).unwrap();
    assert_eq!(apps.iter().map(|a| a.title.as_str()).collect::<Vec<_>>(), ["Desktop"]);
    assert!(nvhttp::launch(&rig.addr, &store, "Tidak Ada", 1280, 720, 60).unwrap_err().contains("tidak ada di host"));

    let dump = rig.data.path().join("video.h265");
    let options = StreamOptions { app: "Desktop".into(), width: 1280, height: 720, fps: 60, bitrate_kbps: 10_000, codec: CodecChoice::Auto, dump_video: Some(dump.clone()), ..Default::default() };
    let started = Instant::now();
    let mut lines = Vec::new();
    // Di tengah sesi: apa yang dilihat host dari client ini.
    let mut seen_by_host = None;
    let stop = || {
        if seen_by_host.is_none() && started.elapsed() > Duration::from_secs(3) {
            seen_by_host = rig.host.stream_snapshot();
        }
        started.elapsed() > Duration::from_secs(4)
    };
    let report = session::run(&rig.addr, &store, &options, stop, |line| lines.push(line)).unwrap();
    let seen = seen_by_host.expect("host punya sesi stream yang aktif");
    assert!(seen.control_connected && seen.client_name == "cloudpc-uji");
    assert_eq!((seen.config.width, seen.config.height, seen.config.fps, seen.config.video_format), (1280, 720, 60, 1), "pilihan client sampai ke host lewat SDP");
    for kind in [packet_type::REQUEST_IDR_FRAME, packet_type::START_B, packet_type::PERIODIC_PING] {
        assert!(seen.messages.contains_key(&kind), "host menerima pesan 0x{kind:04x}: {:x?}", seen.messages);
    }
    assert!(seen.messages[&packet_type::PERIODIC_PING] >= 20, "ping tiap 100 ms: {:x?}", seen.messages);

    // Host melihat sesi dari client ini: kanal kontrol tersambung, pembuka dan ping tiba terenkripsi.
    assert!(report.connected_after < Duration::from_secs(3), "{lines:?}");
    assert_eq!(report.encryption, 7, "host CloudPC mewajibkan enkripsi kontrol, video, dan audio");
    assert_eq!(report.packet_size, 1392);
    assert!(report.termination.is_none() && !report.disconnected);
    assert!(report.audio_packets > 400, "audio 5 ms = ±200 paket/dtk: {report:?}");
    if cloudpc_stream_host::video::capabilities().probed {
        assert!(report.video_packets > 100, "layar ditangkap dan dikirim: {report:?}");
        // Penghitung IV host naik satu per paket: di loopback praktis tidak ada yang hilang.
        let lost = report.video_lost.expect("video terenkripsi, jadi kehilangan terukur");
        assert!(lost * 50 < report.video_packets, "loopback kehilangan {lost} paket: {report:?}");
        // C2: frame utuh tersusun, decoder mendapat aliran yang dimulai dari IDR.
        assert!(report.video.frames > 30, "frame utuh dari host: {report:?}");
        assert!(report.frames_decodable > 30 && report.video.invalid == 0, "{report:?}");
        assert!(report.host_latency_ms.is_some(), "host CloudPC melaporkan latensi tangkap + encode");
        let bytes = std::fs::read(&dump).unwrap();
        assert!(bytes.starts_with(&[0, 0, 0, 1]), "Annex B dimulai kode awal");
        // Unit NAL pertama HEVC dari IDR: VPS (jenis 32).
        assert_eq!((bytes[4] >> 1) & 0x3F, 32, "aliran dimulai dari IDR (VPS/SPS/PPS)");
    }

    // Client sudah memutus kanal kontrol; aplikasi tetap "berjalan" sampai quit (perilaku Moonlight).
    let (_, xml) = cloudpc_stream_client::http::get("127.0.0.1", rig.addr.http_port, "/serverinfo", None, Duration::from_secs(5)).unwrap();
    assert_eq!(cloudpc_stream_client::http::tag(&xml, "currentgame").as_deref(), Some("1"), "aplikasi tetap berjalan setelah client pergi (seperti Sunshine)");
    // Sambung lagi = resume.
    let again = session::run(&rig.addr, &store, &options, || true, |_| {}).unwrap();
    assert!(again.resumed, "aplikasi masih berjalan, jadi client melanjutkannya");

    nvhttp::quit(&rig.addr, &store).unwrap();
    assert!(rig.host.stream_snapshot().is_none() || {
        thread::sleep(Duration::from_millis(500));
        rig.host.stream_snapshot().is_none()
    });
    let again = nvhttp::launch(&rig.addr, &store, "Desktop", 1280, 720, 60).unwrap().1;
    assert!(!again.resumed, "setelah quit, peluncuran berikutnya mulai baru");
}

/// C3: jendela + decode D3D11VA + tampil, untuk ketiga codec. Membuka jendela sungguhan dan butuh GPU dengan NVENC
/// (host) dan decode hardware (client), jadi tidak ikut `cargo test` biasa:
/// `cargo test -p cloudpc-stream-client --test against_host -- --ignored`.
#[cfg(windows)]
#[test]
#[ignore]
fn frames_are_decoded_and_shown_in_a_window() {
    let _turn = streaming();
    use cloudpc_stream_client::rtsp::Codec;
    use cloudpc_stream_client::video::display::{Display, DisplayOptions};
    assert!(cloudpc_stream_host::video::capabilities().probed, "butuh GPU NVIDIA untuk host uji");
    let rig = rig();
    pair(&rig, "4821", "4821").unwrap();
    let store = Store::open(rig.data.path()).unwrap();
    use cloudpc_stream_client::video::display::DecoderChoice;
    let cases = [(Codec::Hevc, DecoderChoice::Auto), (Codec::Av1, DecoderChoice::Auto), (Codec::H264, DecoderChoice::Auto), (Codec::H264, DecoderChoice::Software), (Codec::Hevc, DecoderChoice::Software), (Codec::Av1, DecoderChoice::Software)];
    for (codec, decoder) in cases {
        let options = StreamOptions { app: "Desktop".into(), width: 1280, height: 720, fps: 60, bitrate_kbps: 10_000, codec: CodecChoice::Only(codec), dump_video: None, ..Default::default() };
        let mut display = Display::open(DisplayOptions { title: format!("CloudPC uji {codec:?}"), width: 640, height: 360, fullscreen: false, input: cloudpc_stream_client::input::win::InputOptions { enabled: false, absolute_mouse: false, system_keys: cloudpc_stream_client::input::win::SystemKeys::Never }, decoder, stats: true, vsync: false }).unwrap();
        let started = Instant::now();
        let outputs = session::Outputs { video: Some(&mut display), ..Default::default() };
        let report = session::run_with(&rig.addr, &store, &options, outputs, || started.elapsed() > Duration::from_secs(3), |_| {}).unwrap();
        let shown = display.finish();
        assert_eq!(shown.error, None, "{codec:?} {decoder:?}");
        assert_eq!(shown.decoder.contains("perangkat lunak"), decoder == DecoderChoice::Software, "{shown:?}");
        assert_eq!(shown.decode_errors, 0, "{codec:?}: {shown:?}");
        assert!(shown.presented > 60, "{codec:?}: gambar tampil terus: {shown:?}, {report:?}");
        assert!(shown.receive_to_present_ms < 16.0, "{codec:?}: tanpa antrean di client: {shown:?}");
        nvhttp::quit(&rig.addr, &store).unwrap();
    }
}

/// Penampil tiruan tanpa jendela: mencatat nomor dan jenis tiap frame yang diberikan ke decoder, dan menyatakan codec
/// mana yang "didecode hardware".
struct TypeSink {
    hardware: Vec<cloudpc_stream_client::rtsp::Codec>,
    frames: Vec<(u32, cloudpc_stream_proto::video::FrameType)>,
}

impl session::VideoSink for TypeSink {
    fn start(&mut self, _: cloudpc_stream_client::rtsp::Codec) {}
    fn frame(&mut self, frame: cloudpc_stream_client::video::depacketize::VideoFrame) {
        self.frames.push((frame.index, frame.frame_type));
    }
    fn take_idr_request(&mut self) -> bool {
        false
    }
    fn closed(&self) -> bool {
        false
    }
    fn hardware_codecs(&self) -> Option<Vec<cloudpc_stream_client::rtsp::Codec>> {
        Some(self.hardware.clone())
    }
}

/// Sesi #117 (24 Nov 2026): H.264 di decoder GPU AMD menampilkan gambar lama berulang-ulang setelah frame hilang
/// dipulihkan dengan invalidasi frame rujukan. Sekarang H.264 pada decoder hardware meminta satu frame rujukan: tiap
/// celah di aliran yang sampai ke decoder harus dilanjutkan oleh IDR, tidak pernah oleh P-frame jenis 5. H.264 di CPU
/// (decoder perangkat lunak) tetap memakai invalidasi. Host membuang paket berkelompok; tanpa jendela.
#[test]
fn h264_on_a_hardware_decoder_recovers_lost_frames_with_idr() {
    use cloudpc_stream_client::rtsp::Codec;
    use cloudpc_stream_proto::video::FrameType;
    let _turn = streaming();
    let rig = rig_custom(false, None, (120, 4));
    pair(&rig, "4821", "4821").unwrap();
    let store = Store::open(rig.data.path()).unwrap();
    // Setelan sesi #117: 720p 30 FPS 1 Mbps H.264.
    let options = StreamOptions { app: "Desktop".into(), width: 1280, height: 720, fps: 30, bitrate_kbps: 1_000, codec: CodecChoice::Only(Codec::H264), dump_video: None, ..Default::default() };
    for hardware in [vec![Codec::H264, Codec::Hevc], Vec::new()] {
        let on_gpu = !hardware.is_empty();
        let mut sink = TypeSink { hardware, frames: Vec::new() };
        let started = Instant::now();
        let outputs = session::Outputs { video: Some(&mut sink), ..Default::default() };
        let report = session::run_with(&rig.addr, &store, &options, outputs, || started.elapsed() > Duration::from_secs(8), |_| {}).unwrap();
        nvhttp::quit(&rig.addr, &store).unwrap();
        assert!(report.video.frames_lost >= 1, "host uji harus sempat menghilangkan frame: {report:?}");
        assert_eq!(sink.frames.first().map(|f| f.1), Some(FrameType::Idr), "aliran dimulai dari IDR");
        // Frame pertama setelah tiap celah nomor frame.
        let resumes: Vec<FrameType> = sink.frames.windows(2).filter(|w| w[1].0 != w[0].0 + 1).map(|w| w[1].1).collect();
        assert!(!resumes.is_empty(), "ada celah yang dipulihkan: {report:?}");
        if on_gpu {
            assert_eq!((report.invalidations_sent, report.idr_requests_sent >= 1), (0, true), "decoder GPU: minta IDR, bukan invalidasi: {report:?}");
            assert!(resumes.iter().all(|t| *t == FrameType::Idr), "decoder GPU: tiap celah dilanjutkan IDR: {resumes:?}");
            assert!(sink.frames.iter().all(|f| f.1 != FrameType::AfterRefInvalidation), "decoder GPU tidak pernah menerima P-frame hasil invalidasi");
        } else {
            assert!(report.invalidations_sent >= 1, "decoder perangkat lunak tetap memakai invalidasi: {report:?}");
            assert!(resumes.contains(&FrameType::AfterRefInvalidation), "decoder perangkat lunak: celah dilanjutkan P-frame jenis 5: {resumes:?}");
        }
    }
}

/// Jendela tiruan: mengirim satu paket gamepad, mencatat getar yang datang dari host.
#[derive(Default)]
struct PadSink {
    sent: bool,
    rumble: Vec<(u16, u16, u16)>,
}

impl session::VideoSink for PadSink {
    fn start(&mut self, _: cloudpc_stream_client::rtsp::Codec) {}
    fn frame(&mut self, _: cloudpc_stream_client::video::depacketize::VideoFrame) {}
    fn take_idr_request(&mut self) -> bool {
        false
    }
    fn closed(&self) -> bool {
        false
    }
    fn take_input(&mut self) -> Vec<cloudpc_stream_client::input::InputPacket> {
        if std::mem::replace(&mut self.sent, true) {
            return Vec::new();
        }
        let pad = cloudpc_stream_proto::input::Gamepad { index: 0, active_mask: 1, buttons: 0x1000, ..Default::default() };
        vec![cloudpc_stream_client::input::InputPacket { channel: cloudpc_stream_client::input::channel::GAMEPAD_BASE, data: cloudpc_stream_proto::input::gamepad(&pad) }]
    }
    fn rumble(&mut self, pad: u16, low: u16, high: u16) {
        self.rumble.push((pad, low, high));
    }
}

#[test]
fn gamepad_rumble_from_the_host_reaches_the_client() {
    let _turn = streaming();
    let rig = rig_with(true);
    pair(&rig, "4821", "4821").unwrap();
    let store = Store::open(rig.data.path()).unwrap();
    let options = StreamOptions { app: "Desktop".into(), width: 640, height: 360, fps: 30, bitrate_kbps: 2_000, codec: CodecChoice::Auto, dump_video: None, ..Default::default() };
    let mut sink = PadSink::default();
    let started = Instant::now();
    let outputs = session::Outputs { video: Some(&mut sink), ..Default::default() };
    let report = session::run_with(&rig.addr, &store, &options, outputs, || started.elapsed() > Duration::from_secs(3), |_| {}).unwrap();
    assert_eq!(report.input_packets, 1);
    // Host uji membalas paket gamepad pertama dengan getar (0x80, 0xFF) -> digeser 8 bit seperti Sunshine.
    assert_eq!(sink.rumble, [(0, 0x8000, 0xFF00)], "{report:?}");
    assert_eq!(report.rumble_messages, 1);
    nvhttp::quit(&rig.addr, &store).unwrap();
}

/// Decoder tanpa jendela: GPU (D3D11VA di atas device D3D11 sendiri) dan CPU, untuk ketiga codec. Dipakai juga untuk
/// menguji DLL FFmpeg lain (mis. build ramping): salin exe tes ini ke folder berisi DLL itu lalu jalankan dengan
/// `--ignored decoders_work_without_a_window`.
#[cfg(windows)]
#[test]
#[ignore]
fn decoders_work_without_a_window() {
    use cloudpc_stream_client::rtsp::Codec;
    use cloudpc_stream_client::video::decode::Decoder;
    use cloudpc_stream_client::video::depacketize::VideoFrame;
    use cloudpc_stream_client::video::ff::CpuDecoder;
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
    use windows::Win32::Graphics::Direct3D11::{D3D11CreateDevice, D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION};

    struct Decoding {
        gpu: Option<Decoder>,
        cpu: Option<CpuDecoder>,
        device: windows::Win32::Graphics::Direct3D11::ID3D11Device,
        counts: (u32, u32, u32),
    }
    impl session::VideoSink for Decoding {
        fn start(&mut self, codec: Codec) {
            self.gpu = Some(Decoder::new(codec, &self.device, true).unwrap());
            self.cpu = Some(CpuDecoder::new(codec, false).unwrap());
        }
        fn frame(&mut self, frame: VideoFrame) {
            match self.gpu.as_mut().unwrap().decode(&frame.data) {
                Ok(Some(_)) => self.counts.0 += 1,
                Ok(None) => {}
                Err(_) => self.counts.2 += 1,
            }
            match self.cpu.as_mut().unwrap().decode(&frame.data) {
                Ok(Some(p)) => {
                    assert_eq!(p.nv12.len(), (p.width * p.height * 3 / 2) as usize);
                    self.counts.1 += 1
                }
                Ok(None) => {}
                Err(_) => self.counts.2 += 1,
            }
        }
        fn take_idr_request(&mut self) -> bool {
            false
        }
        fn closed(&self) -> bool {
            false
        }
    }

    let _turn = streaming();
    eprintln!("FFmpeg {}", cloudpc_stream_client::video::ff::ffmpeg_version());
    let rig = rig();
    pair(&rig, "4821", "4821").unwrap();
    let store = Store::open(rig.data.path()).unwrap();
    for codec in [Codec::H264, Codec::Hevc, Codec::Av1] {
        let mut device = None;
        unsafe { D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(), D3D11_CREATE_DEVICE_VIDEO_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut device), None, None) }.unwrap();
        let mut sink = Decoding { gpu: None, cpu: None, device: device.unwrap(), counts: (0, 0, 0) };
        let options = StreamOptions { app: "Desktop".into(), width: 1280, height: 720, fps: 30, bitrate_kbps: 8_000, codec: CodecChoice::Only(codec), ..Default::default() };
        let started = Instant::now();
        let outputs = session::Outputs { video: Some(&mut sink), ..Default::default() };
        session::run_with(&rig.addr, &store, &options, outputs, || started.elapsed() > Duration::from_secs(3), |_| {}).unwrap();
        let (gpu, cpu, errors) = sink.counts;
        eprintln!("{codec:?}: GPU {gpu} frame, CPU {cpu} frame, galat {errors}");
        assert!(gpu > 30 && cpu > 30 && errors == 0, "{codec:?}: {:?}", sink.counts);
        nvhttp::quit(&rig.addr, &store).unwrap();
    }
}

/// Sesi singkat dengan nada 440 Hz sebagai "mikrofon"; mengembalikan laporan sesi dan pesan yang dilihat host.
fn stream_with_tone(rig: &Rig, seconds: u64) -> (session::StreamReport, std::collections::BTreeMap<u16, u32>) {
    assert_eq!(pair(rig, "7310", "7310").unwrap(), "PC Uji");
    let store = Store::open(rig.data.path()).unwrap();
    let options = StreamOptions { app: "Desktop".into(), width: 1280, height: 720, fps: 60, bitrate_kbps: 5_000, codec: CodecChoice::Auto, ..Default::default() };
    let mut microphone = cloudpc_stream_client::mic::Microphone::test_tone().unwrap();
    let started = Instant::now();
    let mut seen = None;
    let stop = || {
        if started.elapsed() > Duration::from_secs(seconds) - Duration::from_millis(300) {
            seen = rig.host.stream_snapshot().map(|s| s.messages).or(seen.take());
        }
        started.elapsed() > Duration::from_secs(seconds)
    };
    let outputs = session::Outputs { mic: Some(&mut microphone), ..Default::default() };
    let report = session::run_with(&rig.addr, &store, &options, outputs, stop, |_| {}).unwrap();
    (report, seen.expect("host punya sesi stream yang aktif"))
}

#[test]
fn microphone_tone_reaches_the_host_output_intact() {
    use cloudpc_stream_common::voice::tone_purity;
    let _turn = streaming();
    let out = tempfile::tempdir().unwrap();
    let wav = out.path().join("mic.wav");
    let rig = rig_full(false, Some(cloudpc_stream_host::mic::MicTarget::Wav(wav.clone())));
    let (report, seen) = stream_with_tone(&rig, 4);
    assert!(!report.mic_refused, "host dengan keluaran mikrofon menawarkannya di DESCRIBE");
    assert!(report.mic_packets > 100, "20 ms per paket selama ±4 dtk: {}", report.mic_packets);
    let at_host = u64::from(seen.get(&cloudpc_stream_proto::mic::PACKET_TYPE).copied().unwrap_or(0));
    assert!(at_host + 30 >= report.mic_packets, "paket mikrofon tiba di host: {at_host} dari {}", report.mic_packets);

    // Thread mikrofon host menulis tiap paket begitu tiba; beri waktu sebentar untuk yang terakhir.
    thread::sleep(Duration::from_millis(300));
    let bytes = std::fs::read(&wav).unwrap();
    assert_eq!(&bytes[..4], b"RIFF");
    let pcm: Vec<i16> = bytes[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
    let frames = pcm.len() / 960;
    assert!(frames as u64 + 30 >= report.mic_packets && frames as u64 <= report.mic_packets + 5, "suara di host {frames} frame, dikirim {}", report.mic_packets);
    // Lewati awal (encoder baru mulai); sisanya harus nada 440 Hz yang bersih, bukan suara rusak.
    let purity = tone_purity(&pcm[960 * 10..], 440.0);
    assert!(purity > 0.9, "kemurnian nada di host {purity}");
}

#[test]
fn hosts_without_a_microphone_output_are_never_sent_microphone_packets() {
    let _turn = streaming();
    let rig = rig();
    let (report, seen) = stream_with_tone(&rig, 3);
    assert!(report.mic_refused);
    assert_eq!(report.mic_packets, 0);
    assert!(!seen.contains_key(&cloudpc_stream_proto::mic::PACKET_TYPE), "host tanpa mikrofon tidak boleh menerima pesan mikrofon: {seen:x?}");
}
