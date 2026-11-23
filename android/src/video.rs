//! Decode video di Android: frame utuh dari `session` -> `AMediaCodec` (decoder hardware HP) -> langsung ke `Surface`
//! milik layar stream. Tidak ada salinan gambar di sisi kita; codec yang menggambar.
//!
//! Dua thread: thread sesi mengisi penyangga masukan (`frame`), thread keluaran melepas tiap gambar ke layar begitu
//! selesai didecode (tanpa menunggu waktu tampil: latensi serendah mungkin, seperti Moonlight).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cloudpc_stream_client::input::InputPacket;
use cloudpc_stream_client::rtsp::Codec;
use cloudpc_stream_client::session::VideoSink;
use cloudpc_stream_client::video::depacketize::VideoFrame;
use cloudpc_stream_client::video::DisplayReport;
use ndk::media::media_codec::{DequeuedInputBufferResult, DequeuedOutputBufferInfoResult, MediaCodec, MediaCodecDirection};
use ndk::media::media_format::MediaFormat;
use ndk::native_window::NativeWindow;

use crate::state::Shared;

/// Paling lama menunggu penyangga masukan codec. Thread sesi juga yang menerima paket jaringan, jadi tidak boleh lama.
const INPUT_WAIT: Duration = Duration::from_millis(4);
/// Jarak minimum antar permintaan IDR dari sini (decoder tertinggal/galat), supaya host tidak dibanjiri.
const IDR_GAP: Duration = Duration::from_millis(500);

struct SharedCodec(MediaCodec);

// SAFETY: `AMediaCodec` membungkus `MediaCodec` framework yang metodenya aman dipanggil dari thread berbeda; pola satu
// thread mengisi masukan dan satu thread mengambil keluaran adalah pemakaian yang lazim (callback async tidak dipakai).
// Crate `ndk` tidak menandainya Send/Sync hanya karena menyimpan pointer mentah.
unsafe impl Send for SharedCodec {}
unsafe impl Sync for SharedCodec {}

#[derive(Default)]
struct Counters {
    decoded: AtomicU64,
    decode_us: AtomicU64,
    errors: AtomicU64,
    /// Frame yang dibuang karena decoder tidak sempat menerimanya.
    dropped: AtomicU64,
}

/// Kapan tiap frame diserahkan ke codec (kunci = stempel waktu yang kita berikan), untuk mengukur lama decode.
type Pending = Mutex<VecDeque<(i64, Instant)>>;

pub fn mime(codec: Codec) -> &'static str {
    match codec {
        Codec::H264 => "video/avc",
        Codec::Hevc => "video/hevc",
        Codec::Av1 => "video/av01",
    }
}

pub struct AndroidVideo {
    shared: Arc<Shared>,
    window: NativeWindow,
    size: (u32, u32),
    fps: u32,
    hardware: Vec<Codec>,
    codec: Option<Arc<SharedCodec>>,
    name: String,
    output: Option<JoinHandle<()>>,
    output_stop: Arc<AtomicBool>,
    counters: Arc<Counters>,
    pending: Arc<Pending>,
    idr: Arc<AtomicBool>,
    last_idr: Option<Instant>,
    failed: Option<String>,
    started: Instant,
}

impl AndroidVideo {
    /// `hardware`: codec yang punya decoder hardware di HP ini (dari `MediaCodecList` di sisi Kotlin).
    pub fn new(shared: Arc<Shared>, window: NativeWindow, size: (u32, u32), fps: u32, hardware: Vec<Codec>) -> AndroidVideo {
        AndroidVideo {
            shared,
            window,
            size,
            fps,
            hardware,
            codec: None,
            name: String::new(),
            output: None,
            output_stop: Arc::new(AtomicBool::new(false)),
            counters: Arc::new(Counters::default()),
            pending: Arc::new(Mutex::new(VecDeque::new())),
            idr: Arc::new(AtomicBool::new(false)),
            last_idr: None,
            failed: None,
            started: Instant::now(),
        }
    }

    pub fn error(&self) -> Option<&str> {
        self.failed.as_deref()
    }

    pub fn report(&self) -> DisplayReport {
        let decoded = self.counters.decoded.load(Ordering::Relaxed);
        let decode_ms = if decoded > 0 { self.counters.decode_us.load(Ordering::Relaxed) as f64 / decoded as f64 / 1000.0 } else { 0.0 };
        DisplayReport {
            decoder: self.name.clone(),
            ffmpeg: "-".into(),
            decoded,
            presented: decoded,
            skipped: self.counters.dropped.load(Ordering::Relaxed),
            decode_errors: self.counters.errors.load(Ordering::Relaxed),
            decode_ms,
            // Gambar dilepas ke layar begitu selesai didecode; waktu komposisi layar Android tidak terlihat dari sini.
            receive_to_present_ms: decode_ms,
            render_ms: 0.0,
            error: self.failed.clone(),
            gamepad_backend: "Android".into(),
            gamepads: Vec::new(),
        }
    }

    fn open(&mut self, codec: Codec) -> Result<(), String> {
        let decoder = MediaCodec::from_decoder_type(mime(codec)).ok_or_else(|| format!("HP ini tidak punya decoder {}", mime(codec)))?;
        let mut format = MediaFormat::new();
        format.set_str("mime", mime(codec));
        format.set_i32("width", self.size.0 as i32);
        format.set_i32("height", self.size.1 as i32);
        format.set_i32("frame-rate", self.fps as i32);
        // Latensi rendah: kunci resmi Android 11+ dan kunci vendor (Qualcomm, HiSilicon, MediaTek/RTC, Exynos). Kunci yang
        // tidak dikenal decoder diabaikan, jadi aman dipasang semua (daftar yang sama dipakai Moonlight Android).
        format.set_i32("low-latency", 1);
        format.set_i32("priority", 0);
        format.set_i32("vendor.qti-ext-dec-low-latency.enable", 1);
        format.set_i32("vendor.hisi-ext-low-latency-video-dec.video-scene-for-low-latency-req", 1);
        format.set_i32("vendor.hisi-ext-low-latency-video-dec.video-scene-for-low-latency-rdy", -1);
        format.set_i32("vendor.rtc-ext-dec-low-latency.enable", 1);
        format.set_i32("vendor.low-latency.enable", 1);
        decoder.configure(&format, Some(&self.window), MediaCodecDirection::Decoder).map_err(|e| format!("configure decoder: {e}"))?;
        decoder.start().map_err(|e| format!("start decoder: {e}"))?;
        // Nama decoder (`AMediaCodec_getName`) baru ada di Android 9; jenisnya cukup untuk log dan statistik.
        self.name = format!("MediaCodec {}", mime(codec));

        let codec = Arc::new(SharedCodec(decoder));
        self.codec = Some(Arc::clone(&codec));
        let (stop, counters, pending, idr) = (Arc::clone(&self.output_stop), Arc::clone(&self.counters), Arc::clone(&self.pending), Arc::clone(&self.idr));
        let thread = std::thread::Builder::new().name("cloudpc-video-out".into()).spawn(move || drain(&codec, &stop, &counters, &pending, &idr));
        self.output = Some(thread.map_err(|e| format!("thread keluaran video: {e}"))?);
        Ok(())
    }

    fn want_idr(&mut self) {
        let now = Instant::now();
        if self.last_idr.is_none_or(|t| now.duration_since(t) >= IDR_GAP) {
            self.last_idr = Some(now);
            self.idr.store(true, Ordering::Relaxed);
        }
    }
}

/// Thread keluaran: tiap gambar yang selesai didecode langsung dilepas ke `Surface`.
fn drain(codec: &SharedCodec, stop: &AtomicBool, counters: &Counters, pending: &Pending, idr: &AtomicBool) {
    while !stop.load(Ordering::Relaxed) {
        match codec.0.dequeue_output_buffer(Duration::from_millis(20)) {
            Ok(DequeuedOutputBufferInfoResult::Buffer(buffer)) => {
                let stamp = buffer.info().presentation_time_us();
                if codec.0.release_output_buffer(buffer, true).is_err() {
                    counters.errors.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                let mut queued = None;
                if let Ok(mut pending) = pending.lock() {
                    while let Some(&(key, at)) = pending.front() {
                        if key > stamp {
                            break;
                        }
                        pending.pop_front();
                        if key == stamp {
                            queued = Some(at);
                        }
                    }
                }
                if let Some(at) = queued {
                    counters.decode_us.fetch_add(at.elapsed().as_micros() as u64, Ordering::Relaxed);
                }
                counters.decoded.fetch_add(1, Ordering::Relaxed);
            }
            Ok(_) => {} // belum ada gambar, atau format/penyangga keluaran berubah
            Err(_) => {
                counters.errors.fetch_add(1, Ordering::Relaxed);
                idr.store(true, Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

impl VideoSink for AndroidVideo {
    fn start(&mut self, codec: Codec) {
        if let Err(e) = self.open(codec) {
            self.failed = Some(e);
        }
    }

    fn frame(&mut self, frame: VideoFrame) {
        let Some(codec) = self.codec.clone() else { return };
        match codec.0.dequeue_input_buffer(INPUT_WAIT) {
            Ok(DequeuedInputBufferResult::Buffer(mut buffer)) => {
                let space = buffer.buffer_mut();
                let fits = frame.data.len() <= space.len();
                if fits {
                    // SAFETY: tujuan adalah penyangga masukan codec sepanjang `space.len()` byte dan panjang sumber sudah
                    // dicek muat; kedua wilayah tidak tumpang-tindih.
                    unsafe { std::ptr::copy_nonoverlapping(frame.data.as_ptr(), space.as_mut_ptr().cast::<u8>(), frame.data.len()) };
                }
                let stamp = self.started.elapsed().as_micros() as u64;
                if fits {
                    if let Ok(mut pending) = self.pending.lock() {
                        pending.push_back((stamp as i64, Instant::now()));
                        // Decoder yang macet tidak boleh membuat daftar ini tumbuh tanpa batas.
                        while pending.len() > 240 {
                            pending.pop_front();
                        }
                    }
                }
                // Penyangga yang sudah diambil HARUS dikembalikan; frame yang tidak muat dikembalikan kosong.
                let queued = codec.0.queue_input_buffer(buffer, 0, if fits { frame.data.len() } else { 0 }, stamp, 0);
                if !fits || queued.is_err() {
                    self.counters.errors.fetch_add(1, Ordering::Relaxed);
                    self.want_idr();
                }
            }
            Ok(DequeuedInputBufferResult::TryAgainLater) => {
                // Decoder tertinggal: frame ini dibuang, jadi frame berikutnya tidak punya rujukan -> minta IDR.
                self.counters.dropped.fetch_add(1, Ordering::Relaxed);
                self.want_idr();
            }
            Err(_) => {
                self.counters.errors.fetch_add(1, Ordering::Relaxed);
                self.want_idr();
            }
        }
    }

    fn take_idr_request(&mut self) -> bool {
        self.idr.swap(false, Ordering::Relaxed)
    }

    fn closed(&self) -> bool {
        self.failed.is_some() || self.shared.stopping()
    }

    fn take_input(&mut self) -> Vec<InputPacket> {
        self.shared.take_input()
    }

    fn hardware_codecs(&self) -> Option<Vec<Codec>> {
        Some(self.hardware.clone())
    }

    fn status(&mut self, network: String, warning: Option<String>) {
        let decoded = self.counters.decoded.load(Ordering::Relaxed);
        let decode_ms = if decoded > 0 { self.counters.decode_us.load(Ordering::Relaxed) as f64 / decoded as f64 / 1000.0 } else { 0.0 };
        let mut text = format!("{network}\nDecode {decode_ms:.1} ms ({})", self.name);
        if let Some(warning) = warning {
            text.push('\n');
            text.push_str(&warning);
        }
        Shared::set(&self.shared.status, text);
    }

    fn numbers(&mut self, fps: f64, mbps: f64, rtt_ms: u32) {
        Shared::set(&self.shared.brief, brief_line(fps, mbps, rtt_ms));
    }
}

/// Baris ringkas untuk pil statistik di kiri atas layar stream: "60 FPS · 24 ms · 6 Mbps".
pub fn brief_line(fps: f64, mbps: f64, rtt_ms: u32) -> String {
    format!("{fps:.0} FPS \u{b7} {rtt_ms} ms \u{b7} {mbps:.0} Mbps")
}

impl Drop for AndroidVideo {
    fn drop(&mut self) {
        self.output_stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.output.take() {
            let _ = thread.join();
        }
        if let Some(codec) = self.codec.take() {
            let _ = codec.0.stop();
        }
    }
}
