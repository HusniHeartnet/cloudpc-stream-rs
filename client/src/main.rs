//! Baris perintah client stream CloudPC. Bentuk perintahnya sama dengan yang dipakai aplikasi CloudPC untuk Moonlight
//! (`pair <host> --pin <pin>`, `stream <host> <app> ...`), supaya bisa menggantikannya tanpa mengubah aplikasi.
//!
//! Beda penting: host dihubungi HANYA di alamat yang diberikan (Moonlight bisa memakai alamat LAN yang diingatnya).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use cloudpc_stream_client::nvhttp::{self, HostAddr};
use cloudpc_stream_client::rtsp::Codec;
use cloudpc_stream_client::session::{self, CodecChoice, StreamOptions};
use cloudpc_stream_client::store::{self, Store};

const USAGE: &str = "Perintah:
  cloudpc-stream-client pair <host[:port]> --pin <4 angka>
  cloudpc-stream-client list <host[:port]>
  cloudpc-stream-client stream <host[:port]> <aplikasi> [--resolution 1920x1080] [--fps 60] [--bitrate 20000]
                        [--video-codec auto|H.264|HEVC|AV1] [--video-decoder auto|hardware|software]
                        [--display-mode windowed|borderless|fullscreen] [--frontend sdl]
                        [--seconds N] [--dump-video <file>] [--headless]
  cloudpc-stream-client quit <host[:port]>
  cloudpc-stream-client decode-test <berkas .h264|.hevc|.obu> <H.264|HEVC|AV1> [--video-decoder software]
  cloudpc-stream-client self-test [--seconds 5]      (jendela + suara tanpa host)
  cloudpc-stream-client --version
Opsi umum: --data <folder> (bawaan %APPDATA%\\CloudPC\\stream-client).
Opsi Moonlight lain yang tidak dikenal diterima dan diabaikan.
`stream` menampilkan gambar di jendela (decode hardware D3D11VA lewat FFmpeg); --headless = tanpa jendela.
Suara diputar di perangkat bawaan (--no-audio = tanpa suara). Input: keyboard, mouse (--absolute-mouse | bawaan relatif),
gamepad XInput (--no-input = hanya menonton); --vsync, --quit-after, --audio-on-host, --game-optimization seperti Moonlight; --capture-system-keys never|fullscreen|always. Ctrl+Alt+Shift + Q keluar, X layar penuh, Z lepas input,
M mode mouse, D kecilkan, V tempel teks, K tangkap tombol sistem, S statistik (--performance-overlay = tampil sejak awal). Log bergaya Moonlight (blok Global video stats).";

/// Opsi Moonlight yang berdiri sendiri (tanpa nilai).
const FLAGS: [&str; 16] = [
    "--performance-overlay",
    "--no-input",
    "--no-audio",
    "--headless",
    "--no-absolute-mouse",
    "--absolute-mouse",
    "--quit-after",
    "--no-quit-after",
    "--vsync",
    "--no-vsync",
    "--game-optimization",
    "--no-game-optimization",
    "--audio-on-host",
    "--no-audio-on-host",
    "--multi-controller",
    "--no-multi-controller",
];

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    run_log::write(&format!("GAGAL: {message}"));
    std::process::exit(1);
}

struct Args {
    positional: Vec<String>,
    options: Vec<(String, String)>,
}

impl Args {
    fn parse(raw: Vec<String>) -> Args {
        let (mut positional, mut options) = (Vec::new(), Vec::new());
        let mut items = raw.into_iter().peekable();
        while let Some(item) = items.next() {
            if !item.starts_with("--") {
                positional.push(item);
            } else if FLAGS.contains(&item.as_str()) || items.peek().is_none_or(|next| next.starts_with("--")) {
                options.push((item, String::new()));
            } else {
                let value = items.next().unwrap_or_default();
                options.push((item, value));
            }
        }
        Args { positional, options }
    }

    fn has(&self, name: &str) -> bool {
        self.options.iter().any(|(key, _)| key == name)
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.options.iter().rev().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    fn number(&self, name: &str, default: u32) -> u32 {
        match self.get(name) {
            None => default,
            Some(text) => text.parse().unwrap_or_else(|_| fail(&format!("{name} butuh angka, bukan \"{text}\""))),
        }
    }

    fn host(&self) -> HostAddr {
        let text = self.positional.get(1).unwrap_or_else(|| fail(USAGE));
        HostAddr::parse(text).unwrap_or_else(|e| fail(&e))
    }
}

fn parse_codec(text: Option<&str>) -> CodecChoice {
    match text.map(str::to_ascii_lowercase).as_deref() {
        None | Some("auto") => CodecChoice::Auto,
        Some("h.264" | "h264") => CodecChoice::Only(Codec::H264),
        Some("hevc" | "h.265" | "h265") => CodecChoice::Only(Codec::Hevc),
        Some("av1") => CodecChoice::Only(Codec::Av1),
        Some(other) => fail(&format!("--video-codec tidak dikenal: {other}")),
    }
}

fn main() {
    let args = Args::parse(std::env::args().skip(1).collect());
    let command = args.positional.first().map(String::as_str);
    if args.get("--version").is_some() || command == Some("--version") {
        println!("cloudpc-stream-client {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let data = args.get("--data").map(PathBuf::from).unwrap_or_else(store::default_dir);
    let open_store = || Store::open(&data).unwrap_or_else(|e| fail(&e));

    match command {
        Some("pair") => {
            let addr = args.host();
            let pin = args.get("--pin").unwrap_or_else(|| fail("pair butuh --pin <4 angka>"));
            let mut store = open_store();
            println!("Pairing dengan {}:{} memakai PIN {pin}...", addr.host, addr.http_port);
            match nvhttp::pair(&addr, &mut store, pin) {
                Ok(name) => println!("Pairing berhasil dengan {name}."),
                Err(why) => fail(&format!("Pairing gagal: {why}")),
            }
        }
        Some("list") => {
            let apps = nvhttp::apps(&args.host(), &open_store()).unwrap_or_else(|e| fail(&e));
            for app in apps {
                println!("{}", app.title);
            }
        }
        Some("quit") => {
            nvhttp::quit(&args.host(), &open_store()).unwrap_or_else(|e| fail(&e));
            println!("Aplikasi di host ditutup.");
        }
        Some("decode-test") => {
            // Uji decoder tanpa host: berkas aliran mentah + codec.
            let path = args.positional.get(1).unwrap_or_else(|| fail(USAGE));
            let codec = match parse_codec(args.positional.get(2).map(String::as_str)) {
                CodecChoice::Only(c) => c,
                CodecChoice::Auto => fail("decode-test butuh codec: H.264, HEVC, atau AV1"),
            };
            let hardware = args.get("--video-decoder") != Some("software");
            match cloudpc_stream_client::video::ff::decode_file(std::path::Path::new(path), codec, hardware) {
                Ok(r) if r.frames > 0 && r.errors == 0 => println!("Decode OK: {} ({}), {} frame {}x{}", r.decoder, if r.hardware { "hardware" } else { "perangkat lunak" }, r.frames, r.size.0, r.size.1),
                Ok(r) => fail(&format!("Decode gagal: {r:?}")),
                Err(e) => fail(&format!("Decode gagal: {e}")),
            }
        }
        Some("self-test") => {
            // Uji jendela + suara tanpa host (mis. di Mac baru): pola uji bergerak dan nada 440 Hz.
            let seconds = u64::from(args.number("--seconds", 5));
            match cloudpc_stream_client::frontend_sdl::self_test(seconds) {
                Ok((renderer, frames)) => println!("Uji mandiri OK: renderer {renderer}, {frames} frame tampil, nada 440 Hz diputar"),
                Err(e) => fail(&format!("Uji mandiri gagal: {e}")),
            }
        }
        Some("stream") => {
            let addr = args.host();
            let app = args.positional.get(2).cloned().unwrap_or_else(|| fail(USAGE));
            let (width, height) = match args.get("--resolution") {
                None => (1920, 1080),
                Some(text) => text.split_once('x').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?))).unwrap_or_else(|| fail(&format!("--resolution tidak valid: {text}"))),
            };
            let options = StreamOptions {
                app,
                width,
                height,
                fps: args.number("--fps", 60),
                bitrate_kbps: args.number("--bitrate", 20_000),
                codec: parse_codec(args.get("--video-codec")),
                dump_video: args.get("--dump-video").map(std::path::PathBuf::from),
                launch: nvhttp::LaunchExtras { game_optimization: args.has("--game-optimization"), audio_on_host: args.has("--audio-on-host") },
            };
            let limit = args.get("--seconds").map(|_| Duration::from_secs(u64::from(args.number("--seconds", 0))));
            let started = Instant::now();
            run_log::start(&data);
            let log = |text: &str| log_line(started, text);
            let quit_after = args.has("--quit-after");
            // macOS: aplikasi CloudPC menghentikan stream dengan SIGTERM; akhiri sesi dengan rapi (statistik tetap ditulis).
            terminate::install();
            let (result, shown, audio_out) = if use_sdl(&args) {
                // Tampilan SDL memegang thread ini (aturan macOS); sesi jalan di thread lain.
                let sdl_options = sdl_options(&args, width, height);
                let (addr, data, options) = (addr.clone(), data.clone(), options.clone());
                let run = cloudpc_stream_client::frontend_sdl::run(sdl_options, move |mut sink, mut audio| {
                    let store = Store::open(&data)?;
                    let mut events = event_logger(started, "SDL");
                    let outputs = session::Outputs { video: Some(&mut sink), audio: audio.as_mut().map(|a| a as &mut dyn session::AudioSink), events: Some(&mut events) };
                    let result = session::run_with(&addr, &store, &options, outputs, || terminate::requested() || limit.is_some_and(|l| started.elapsed() >= l), |line| log_line(started, &line));
                    if quit_after {
                        quit_host(&addr, &store, started);
                    }
                    result
                });
                match run {
                    Ok((result, shown)) => (result, Some(shown), None),
                    Err(e) => fail(&format!("Tampilan SDL tidak bisa dibuka: {e}")),
                }
            } else {
                let store = open_store();
                let stop = || terminate::requested() || limit.is_some_and(|l| started.elapsed() >= l);
                let mut display = open_display(&args, width, height);
                let mut player = open_player(&args, display.is_some(), &log);
                let mut events = event_logger(started, if display.is_some() { "D3D11VA" } else { "" });
                let outputs = session::Outputs { video: display.as_mut().map(|d| d as &mut dyn session::VideoSink), audio: player.as_mut().map(|p| p as &mut dyn session::AudioSink), events: Some(&mut events) };
                let result = session::run_with(&addr, &store, &options, outputs, stop, |line| log(&line));
                let shown = display.map(finish_display);
                let audio_out = player.map(finish_player);
                // `--quit-after`: tutup aplikasi di host begitu stream selesai (seperti Moonlight).
                if quit_after {
                    quit_host(&addr, &store, started);
                }
                (result, shown, audio_out)
            };
            let report = result.unwrap_or_else(|e| fail(&format!("Stream gagal: {e}")));
            let seconds = report.duration.as_secs_f64().max(0.001);
            log(&format!("Ringkasan: codec {:?}, enkripsi {}, paket video {} byte, tersambung setelah {} ms, berjalan {:.1} detik", report.codec.unwrap_or(Codec::H264), report.encryption, report.packet_size, report.connected_after.as_millis(), seconds));
            let lost_text = report.video_lost.map(|lost| {
                let percent = if lost + report.video_packets > 0 { lost as f64 * 100.0 / (lost + report.video_packets) as f64 } else { 0.0 };
                format!(", hilang di jaringan {lost} ({percent:.2}%)")
            });
            log(&format!("Ringkasan: video {} paket ({:.0}/dtk), {:.1} Mbit/dtk{}", report.video_packets, report.video_packets as f64 / seconds, report.video_bytes as f64 * 8.0 / seconds / 1e6, lost_text.unwrap_or_default()));
            let v = report.video;
            log(&format!(
                "Ringkasan: frame utuh {} ({:.1} FPS), dipulihkan FEC {} frame ({} shard), hilang {}, ditahan menunggu pemulih {}, permintaan ke host {} invalidasi + {} IDR",
                v.frames,
                v.frames as f64 / seconds,
                v.frames_recovered,
                v.shards_recovered,
                v.frames_lost,
                report.frames_held,
                report.invalidations_sent,
                report.idr_requests_sent
            ));
            if let Some(shown) = &shown {
                print_display(shown, seconds, &log);
            }
            let a = report.audio;
            log(&format!(
                "Ringkasan: audio {} paket, dipulihkan FEC {}, hilang {}{}; input {} paket",
                report.audio_packets,
                a.recovered,
                a.lost,
                audio_out.map(|text| format!(", {text}")).unwrap_or_default(),
                report.input_packets
            ));
            print_global_stats(&report, shown.as_ref(), &log);
            match (report.termination, report.disconnected) {
                (Some(code), _) => log(&format!("Connection terminated: {}", code as i32)),
                (None, true) => log("Kanal kontrol diputus host"),
                _ => log("Stream diakhiri client"),
            }
        }
        _ => fail(USAGE),
    }
}

#[cfg(windows)]
type DisplayHandle = cloudpc_stream_client::video::display::Display;
#[cfg(not(windows))]
type DisplayHandle = NoDisplay;
type DisplaySummary = cloudpc_stream_client::video::DisplayReport;

/// Jendela video belum ada di luar Windows (macOS menyusul): `stream` berjalan tanpa jendela.
#[cfg(not(windows))]
struct NoDisplay;
#[cfg(not(windows))]
impl session::VideoSink for NoDisplay {
    fn start(&mut self, _: Codec) {}
    fn frame(&mut self, _: cloudpc_stream_client::video::depacketize::VideoFrame) {}
    fn take_idr_request(&mut self) -> bool {
        false
    }
    fn closed(&self) -> bool {
        false
    }
}

#[cfg(windows)]
fn open_display(args: &Args, width: u32, height: u32) -> Option<DisplayHandle> {
    use cloudpc_stream_client::video::display::{Display, DisplayOptions};
    if args.has("--headless") {
        return None;
    }
    let fullscreen = matches!(args.get("--display-mode"), Some("fullscreen" | "borderless"));
    // Mode jendela: ukuran stream, paling besar 1280x720 (bisa diubah dengan menarik tepi jendela).
    let scale = (1280.0 / width as f64).min(720.0 / height as f64).min(1.0);
    use cloudpc_stream_client::input::win::{InputOptions, SystemKeys};
    let system_keys = match args.get("--capture-system-keys") {
        Some("never") => SystemKeys::Never,
        Some("always") => SystemKeys::Always,
        _ => SystemKeys::Fullscreen,
    };
    // Bawaan relatif (mode game), sama dengan Moonlight dan argumen aplikasi CloudPC (`--no-absolute-mouse`).
    let input = InputOptions { enabled: !args.has("--no-input"), absolute_mouse: args.has("--absolute-mouse"), system_keys };
    use cloudpc_stream_client::video::display::DecoderChoice;
    let decoder = match args.get("--video-decoder") {
        Some("software") => DecoderChoice::Software,
        Some("hardware") => DecoderChoice::Hardware,
        _ => DecoderChoice::Auto,
    };
    let options = DisplayOptions { title: "CloudPC".into(), width: (width as f64 * scale) as u32, height: (height as f64 * scale) as u32, fullscreen, input, decoder, stats: args.has("--performance-overlay"), vsync: args.has("--vsync") };
    Some(Display::open(options).unwrap_or_else(|e| fail(&format!("Jendela video tidak bisa dibuka: {e}"))))
}

#[cfg(not(windows))]
fn open_display(_: &Args, _: u32, _: u32) -> Option<DisplayHandle> {
    None
}

#[cfg(windows)]
fn finish_display(display: DisplayHandle) -> DisplaySummary {
    display.finish()
}

#[cfg(not(windows))]
fn finish_display(_: DisplayHandle) -> DisplaySummary {
    DisplaySummary::default()
}

fn print_display(shown: &DisplaySummary, seconds: f64, log: &dyn Fn(&str)) {
    log(&format!(
        "Ringkasan: tampil {} frame ({:.1} FPS), decoder {} (FFmpeg {}), decode {:.2} ms, frame tiba -> tampil {:.2} ms, dilewati {}, galat decode {}",
        shown.presented,
        shown.presented as f64 / seconds,
        if shown.decoder.is_empty() { "-" } else { &shown.decoder },
        shown.ffmpeg,
        shown.decode_ms,
        shown.receive_to_present_ms,
        shown.skipped,
        shown.decode_errors
    ));
    if let Some(error) = &shown.error {
        log(&format!("Galat video: {error}"));
    }
    log(&format!("Ringkasan: stik lewat {}; tersambung: {}", shown.gamepad_backend, if shown.gamepads.is_empty() { "tidak ada".into() } else { shown.gamepads.join(", ") }));
}


/// Satu baris log bergaya Moonlight (`HH:MM:SS - ...`, waktu sejak program mulai), supaya aplikasi CloudPC bisa
/// membaca log ini dengan `parse_stream_log` yang sama.
fn log_line(started: Instant, text: &str) {
    let t = started.elapsed().as_secs();
    let line = format!("{:02}:{:02}:{:02} - CloudPC Info: {text}", t / 3600, t / 60 % 60, t % 60);
    println!("{line}");
    run_log::write(&line);
}

/// Salinan log tiap stream di `<data>/logs/stream-TANGGAL-JAM.log` (10 terakhir). Aplikasi CloudPC menimpa
/// `moonlight-stream.log` tiap stream dibuka, jadi tanpa ini bukti stream yang mati hilang begitu pemain menyambung lagi
/// (19 Nov 2026: stream mati setelah Alt+Tab dari layar penuh, lognya tertimpa). Panik juga ditulis ke sini.
mod run_log {
    use std::io::Write;
    use std::sync::Mutex;

    static FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);

    pub fn start(data: &std::path::Path) {
        let dir = data.join("logs");
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let path = dir.join(format!("stream-{stamp}.log"));
        if let Ok(file) = std::fs::File::create(&path) {
            *FILE.lock().unwrap_or_else(|e| e.into_inner()) = Some(file);
        }
        // Simpan 10 terbaru.
        if let Ok(entries) = std::fs::read_dir(&dir) {
            let mut logs: Vec<_> = entries.flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("stream-"))).collect();
            logs.sort();
            let excess = logs.len().saturating_sub(10);
            for old in logs.into_iter().take(excess) {
                let _ = std::fs::remove_file(old);
            }
        }
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            write(&format!("PANIK: {info}"));
            previous(info);
        }));
    }

    pub fn write(line: &str) {
        if let Ok(mut guard) = FILE.lock() {
            if let Some(file) = guard.as_mut() {
                let _ = writeln!(file, "{line}");
            }
        }
    }
}

/// Blok "Global video stats" persis format Moonlight 6.2 (dibaca `moonlight.parse_stream_log` aplikasi).
fn print_global_stats(report: &session::StreamReport, shown: Option<&DisplaySummary>, log: &dyn Fn(&str)) {
    let seconds = report.duration.saturating_sub(report.connected_after).as_secs_f64().max(0.001);
    let v = report.video;
    let received = v.frames as f64 / seconds;
    let (decoded, rendered, decode_ms, queue_ms, render_ms, skipped) = display_numbers(shown, seconds);
    let (decoded, rendered) = (decoded.unwrap_or(received), rendered.unwrap_or(received));
    let dropped_network = if v.frames + v.frames_lost > 0 { v.frames_lost as f64 * 100.0 / (v.frames + v.frames_lost) as f64 } else { 0.0 };
    let dropped_jitter = if v.frames > 0 { skipped as f64 * 100.0 / v.frames as f64 } else { 0.0 };
    let (low, high) = report.host_latency_range_ms.unwrap_or((0.0, 0.0));
    let (rtt, variance) = report.network_rtt_ms.unwrap_or((0.0, 0.0));
    log("");
    println!("Global video stats");
    println!("-------------------");
    println!("Incoming frame rate from network: {received:.2} FPS");
    println!("Decoding frame rate: {decoded:.2} FPS");
    println!("Rendering frame rate: {rendered:.2} FPS");
    println!("Host processing latency min/max/average: {low:.1}/{high:.1}/{:.1} ms", report.host_latency_ms.unwrap_or(0.0));
    println!("Frames dropped by your network connection: {dropped_network:.2}%");
    println!("Frames dropped due to network jitter: {dropped_jitter:.2}%");
    println!("Average network latency: {rtt:.0} ms (variance: {variance:.0} ms)");
    println!("Average decoding time: {decode_ms:.2} ms");
    println!("Average frame queue delay: {queue_ms:.2} ms");
    println!("Average rendering time (including monitor V-sync latency): {render_ms:.2} ms");
}

fn display_numbers(shown: Option<&DisplaySummary>, seconds: f64) -> (Option<f64>, Option<f64>, f64, f64, f64, u64) {
    match shown {
        Some(s) => {
            let queue = (s.receive_to_present_ms - s.decode_ms - s.render_ms).max(0.0);
            (Some(s.decoded as f64 / seconds), Some(s.presented as f64 / seconds), s.decode_ms, queue, s.render_ms, s.skipped)
        }
        None => (None, None, 0.0, 0.0, 0.0, 0),
    }
}


#[cfg(windows)]
type PlayerHandle = cloudpc_stream_client::audio::player::Player;
#[cfg(not(windows))]
type PlayerHandle = NoPlayer;

#[cfg(not(windows))]
struct NoPlayer;
#[cfg(not(windows))]
impl session::AudioSink for NoPlayer {
    fn samples(&mut self, _: &[f32]) {}
}

/// Suara diputar bila ada jendela (bukan `--headless`) dan tidak `--no-audio`. Tanpa perangkat suara: tetap jalan.
#[cfg(windows)]
fn open_player(args: &Args, with_display: bool, log: &dyn Fn(&str)) -> Option<PlayerHandle> {
    if !with_display || args.has("--no-audio") {
        return None;
    }
    match cloudpc_stream_client::audio::player::Player::open() {
        Ok(player) => Some(player),
        Err(error) => {
            log(&format!("Suara tidak bisa diputar: {error}"));
            None
        }
    }
}

#[cfg(not(windows))]
fn open_player(_: &Args, _: bool, _: &dyn Fn(&str)) -> Option<PlayerHandle> {
    None
}

#[cfg(windows)]
fn finish_player(player: PlayerHandle) -> String {
    let stats = player.stats();
    format!("suara habis sebelum waktunya {} kali, dibuang {} ms (antrean terlalu panjang), pindah perangkat {} kali", stats.underruns, stats.trimmed_ms, stats.device_changes)
}

#[cfg(not(windows))]
fn finish_player(_: PlayerHandle) -> String {
    String::new()
}

/// Tampilan SDL: wajib di luar Windows; di Windows dengan `--frontend sdl`.
fn use_sdl(args: &Args) -> bool {
    !cfg!(windows) || args.get("--frontend") == Some("sdl")
}

fn sdl_options(args: &Args, width: u32, height: u32) -> cloudpc_stream_client::frontend_sdl::SdlOptions {
    use cloudpc_stream_client::frontend_sdl::{SdlOptions, SystemKeys};
    let scale = (1280.0 / width as f64).min(720.0 / height as f64).min(1.0);
    SdlOptions {
        title: "CloudPC".into(),
        width: (width as f64 * scale) as u32,
        height: (height as f64 * scale) as u32,
        fullscreen: matches!(args.get("--display-mode"), Some("fullscreen" | "borderless")),
        input: !args.has("--no-input"),
        absolute_mouse: args.has("--absolute-mouse"),
        system_keys: match args.get("--capture-system-keys") {
            Some("never") => SystemKeys::Never,
            Some("always") => SystemKeys::Always,
            _ => SystemKeys::Fullscreen,
        },
        hardware_decode: args.get("--video-decoder") != Some("software"),
        stats: args.has("--performance-overlay"),
        vsync: args.has("--vsync"),
        audio: !args.has("--no-audio"),
    }
}

/// Kejadian sesi -> baris log bergaya Moonlight.
fn event_logger(started: Instant, renderer: &'static str) -> impl FnMut(session::Event) {
    move |event| {
        let log = |text: &str| log_line(started, text);
        match event {
            session::Event::Negotiated { width, height, fps, video_format, bitrate_kbps } => {
                log(&format!("Video stream is {width}x{height}x{fps} (format {video_format:#x})"));
                log(&format!("Video bitrate: {bitrate_kbps} kbps"));
                if !renderer.is_empty() {
                    log(&format!("Renderer '{renderer}' chosen"));
                }
            }
            session::Event::FirstVideoPacket(after) => log(&format!("Received first video packet after {} ms", after.as_millis())),
            session::Event::RfiRequest(frame) => log(&format!("Sending RFI request for unrecoverable frame {frame}")),
            session::Event::FirstRumble { pad, low, high } => log(&format!("Getar gamepad {pad} dari host (motor {low}/{high})")),
        }
    }
}

/// `--quit-after`: tutup aplikasi di host begitu stream selesai (seperti Moonlight).
fn quit_host(addr: &HostAddr, store: &Store, started: Instant) {
    match nvhttp::quit(addr, store) {
        Ok(()) => log_line(started, "Aplikasi di host ditutup (--quit-after)"),
        Err(e) => log_line(started, &format!("Aplikasi di host gagal ditutup: {e}")),
    }
}

/// SIGTERM/SIGINT (macOS, Linux) -> sesi diakhiri dengan rapi seperti WM_CLOSE di Windows.
mod terminate {
    use std::sync::atomic::{AtomicBool, Ordering};

    static REQUESTED: AtomicBool = AtomicBool::new(false);

    pub fn requested() -> bool {
        REQUESTED.load(Ordering::SeqCst)
    }

    #[cfg(unix)]
    pub fn install() {
        extern "C" fn on_signal(_: libc::c_int) {
            REQUESTED.store(true, Ordering::SeqCst);
        }
        unsafe {
            libc::signal(libc::SIGTERM, on_signal as libc::sighandler_t);
            libc::signal(libc::SIGINT, on_signal as libc::sighandler_t);
        }
    }

    #[cfg(not(unix))]
    pub fn install() {
        let _ = &REQUESTED;
    }
}
