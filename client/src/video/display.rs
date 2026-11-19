//! Fase C3. Thread video: jendela + decoder + renderer di satu thread (pesan jendela, device D3D11, dan konteks
//! FFmpeg tidak berpindah thread). Thread jaringan (`session::run_with`) mengirim frame lewat kanal.
//!
//! Bila beberapa frame menunggu sekaligus, semuanya didecode (P-frame saling merujuk) tetapi hanya yang terakhir
//! ditampilkan: gambar yang terlambat tidak pernah mengantre di layar.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::decode::{Decoder, Picture};
use super::ff::ffmpeg_version;
pub use super::DisplayReport;
use crate::input::win::{self as input, InputOptions};
use crate::input::{InputPacket, Shortcut};
use super::depacketize::VideoFrame;
use super::render::Renderer;
use crate::rtsp::Codec;
use crate::session::VideoSink;

enum Message {
    Start { codec: Codec },
    Status { network: String, warning: Option<String> },
    Rumble { pad: u16, low: u16, high: u16 },
    Frame { frame: VideoFrame, received: Instant },
}


/// Penjaga thread jendela: Windows menandai jendela "Not Responding" bila thread-nya tidak memompa pesan ±5 detik
/// (dilihat pemilik di sesi #90 selagi gambar tetap jalan). Thread jendela mencatat tahap dan detak; penjaga mencatat
/// jeda > 1 dtk beserta tahapnya, dan saat Windows menganggap jendela macet, ke log stream.
struct Watch {
    started: Instant,
    /// Milidetik sejak `started` saat pesan jendela terakhir dipompa.
    beat_ms: AtomicU64,
    stage: AtomicU8,
}

const STAGES: [&str; 8] = ["-", "pompa pesan", "input/stik", "pintasan", "menunggu frame", "decode", "tampilkan", "statistik"];

impl Watch {
    fn now_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }
    fn stage(&self, stage: u8) {
        self.stage.store(stage, Ordering::Relaxed);
    }
    fn beat(&self) {
        self.beat_ms.store(self.now_ms(), Ordering::Relaxed);
    }
}

/// Thread penjaga; berhenti saat `closed`.
fn watchdog(watch: Arc<Watch>, window: isize, closed: Arc<AtomicBool>) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::IsHungAppWindow;
    let (mut stall_start, mut worst_stage, mut was_hung) = (None::<u64>, 0u8, false);
    while !closed.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(200));
        let gap = watch.now_ms().saturating_sub(watch.beat_ms.load(Ordering::Relaxed));
        if gap > 1000 {
            stall_start.get_or_insert(watch.beat_ms.load(Ordering::Relaxed));
            worst_stage = watch.stage.load(Ordering::Relaxed);
        } else if let Some(start) = stall_start.take() {
            let length = watch.now_ms().saturating_sub(start);
            super::note(format!("Jendela tidak memompa pesan selama {length} ms (tahap: {})", STAGES[usize::from(worst_stage).min(STAGES.len() - 1)]));
        }
        // SAFETY: hanya membaca keadaan jendela; pegangan jendela masih hidup selama `closed` belum diset.
        let hung = unsafe { IsHungAppWindow(HWND(window as *mut _)) }.as_bool();
        if hung != was_hung {
            was_hung = hung;
            let stage = STAGES[usize::from(watch.stage.load(Ordering::Relaxed)).min(STAGES.len() - 1)];
            super::note(if hung {
                format!("Windows menandai jendela Not Responding (pompa terakhir {gap} ms lalu, tahap: {stage})")
            } else {
                "Jendela merespons lagi".to_string()
            });
        }
    }
}

/// Event Win32 yang dibangunkan tiap kali pesan untuk thread jendela dikirim. Thread jendela menunggu pesan jendela
/// ATAU event ini dengan `MsgWaitForMultipleObjectsEx`. Dulu ia menunggu frame dengan `recv_timeout` dan hanya
/// mengintip pesan jendela; Windows menganggap thread yang tidak pernah menunggu input dan tidak menerima pesan ±5 dtk
/// sebagai "Not Responding" (sesi #90: jendela di belakang menjadi Not Responding walau gambar tetap jalan dan pesan
/// WM_NULL dijawab dalam 22 ms).
struct Wake(windows::Win32::Foundation::HANDLE);
// SAFETY: pegangan event kernel boleh dipakai dari thread mana pun.
unsafe impl Send for Wake {}
unsafe impl Sync for Wake {}

impl Wake {
    fn new() -> Result<Wake, String> {
        // SAFETY: event auto-reset tanpa nama.
        unsafe { windows::Win32::System::Threading::CreateEventW(None, false, false, None) }.map(Wake).map_err(|e| format!("CreateEvent: {e}"))
    }
    fn signal(&self) {
        // SAFETY: pegangan masih hidup selama `Wake` ada.
        let _ = unsafe { windows::Win32::System::Threading::SetEvent(self.0) };
    }
    /// Tunggu pesan jendela, event ini, atau `timeout_ms`.
    fn wait(&self, timeout_ms: u32) {
        use windows::Win32::UI::WindowsAndMessaging::{MsgWaitForMultipleObjectsEx, MWMO_INPUTAVAILABLE, QS_ALLINPUT};
        // SAFETY: satu pegangan sah.
        unsafe { MsgWaitForMultipleObjectsEx(Some(&[self.0]), timeout_ms, QS_ALLINPUT, MWMO_INPUTAVAILABLE) };
    }
}

impl Drop for Wake {
    fn drop(&mut self) {
        // SAFETY: ditutup tepat sekali.
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(self.0) };
    }
}

pub struct Display {
    tx: Sender<Message>,
    wake: Arc<Wake>,
    hardware_codecs: Vec<Codec>,
    input: Receiver<InputPacket>,
    closed: Arc<AtomicBool>,
    need_idr: Arc<AtomicBool>,
    report: Arc<Mutex<DisplayReport>>,
    thread: Option<JoinHandle<()>>,
}

#[derive(Debug, Clone)]
pub struct DisplayOptions {
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub fullscreen: bool,
    pub input: InputOptions,
    pub decoder: DecoderChoice,
    /// Statistik tampil sejak awal (`--performance-overlay`); bisa diubah dengan Ctrl+Alt+Shift+S.
    pub stats: bool,
    pub vsync: bool,
}

/// `--video-decoder auto|hardware|software` (sama dengan Moonlight).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecoderChoice {
    Auto,
    Hardware,
    Software,
}

impl Display {
    /// Buka jendela di thread baru. Gagal di sini = tidak ada jendela/GPU yang bisa dipakai.
    pub fn open(options: DisplayOptions) -> Result<Display, String> {
        let (tx, rx) = mpsc::channel();
        let wake = Arc::new(Wake::new()?);
        let (input_tx, input_rx) = mpsc::channel();
        let closed = Arc::new(AtomicBool::new(false));
        let need_idr = Arc::new(AtomicBool::new(false));
        let report = Arc::new(Mutex::new(DisplayReport { ffmpeg: ffmpeg_version(), ..Default::default() }));
        let (ready_tx, ready_rx) = mpsc::channel::<Result<Vec<Codec>, String>>();
        let choice = options.decoder;
        let stats = options.stats;
        let vsync = options.vsync;
        let thread = {
            let (closed, need_idr, report, wake) = (closed.clone(), need_idr.clone(), report.clone(), wake.clone());
            std::thread::Builder::new()
                .name("video".into())
                .spawn(move || match Renderer::new(&options.title, options.width, options.height, options.fullscreen) {
                    Ok(mut renderer) => {
                        renderer.vsync = vsync;
                        input::install(renderer.window, input_tx, &options.input);
                        input::set_fullscreen(renderer.is_fullscreen());
                        if let Ok(mut r) = report.lock() {
                            r.gamepad_backend = input::gamepad_backend().into();
                        }
                        let codecs = if choice == DecoderChoice::Software { Vec::new() } else { renderer.hardware_codecs() };
                        let _ = ready_tx.send(Ok(codecs.clone()));
                        run(renderer, rx, &wake, &closed, &need_idr, &report, &codecs, choice, stats);
                        input::uninstall();
                        closed.store(true, Ordering::SeqCst);
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(error));
                    }
                })
                .map_err(|e| format!("thread video: {e}"))?
        };
        let hardware_codecs = ready_rx.recv().map_err(|_| "thread video berhenti".to_string())??;
        Ok(Display { tx, wake, hardware_codecs, input: input_rx, closed, need_idr, report, thread: Some(thread) })
    }

    fn post(&self, message: Message) {
        let _ = self.tx.send(message);
        self.wake.signal();
    }

    /// Pengguna menutup jendela (atau thread video berhenti karena galat).
    pub fn closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    /// Tutup jendela dan kembalikan ringkasan.
    pub fn finish(mut self) -> DisplayReport {
        self.shutdown();
        self.report.lock().map(|r| r.clone()).unwrap_or_default()
    }

    fn shutdown(&mut self) {
        // Kanal ditutup = thread keluar dari loop dan menghancurkan jendela.
        let (dummy, _) = mpsc::channel();
        drop(std::mem::replace(&mut self.tx, dummy));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Display {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl VideoSink for Display {
    fn start(&mut self, codec: Codec) {
        self.post(Message::Start { codec });
    }

    fn frame(&mut self, frame: VideoFrame) {
        self.post(Message::Frame { frame, received: Instant::now() });
    }

    fn take_idr_request(&mut self) -> bool {
        self.need_idr.swap(false, Ordering::SeqCst)
    }

    fn closed(&self) -> bool {
        Display::closed(self)
    }

    fn take_input(&mut self) -> Vec<InputPacket> {
        self.input.try_iter().collect()
    }

    fn hardware_codecs(&self) -> Option<Vec<Codec>> {
        Some(self.hardware_codecs.clone())
    }

    fn rumble(&mut self, pad: u16, low: u16, high: u16) {
        // SDL hanya boleh dipakai dari thread jendela.
        self.post(Message::Rumble { pad, low, high });
    }

    fn status(&mut self, network: String, warning: Option<String>) {
        self.post(Message::Status { network, warning });
    }
}

#[allow(clippy::too_many_arguments)]
fn run(mut renderer: Renderer, rx: Receiver<Message>, wake: &Wake, closed: &AtomicBool, need_idr: &AtomicBool, report: &Mutex<DisplayReport>, hardware: &[Codec], choice: DecoderChoice, stats: bool) {
    let mut decoder: Option<Decoder> = None;
    let (mut show_stats, mut network, mut warning) = (stats, String::new(), None::<String>);
    // Angka thread ini untuk overlay, dihitung ulang tiap detik.
    let (mut second_start, mut second_presented, mut second_decode, mut second_latency, mut second_decoded) = (Instant::now(), 0u64, Duration::ZERO, Duration::ZERO, 0u64);
    let mut local_line = String::new();
    let (mut decode_total, mut latency_total, mut render_total) = (Duration::ZERO, Duration::ZERO, Duration::ZERO);
    let update = |f: &mut dyn FnMut(&mut DisplayReport)| {
        if let Ok(mut r) = report.lock() {
            f(&mut r);
        }
    };
    let watch = Arc::new(Watch { started: Instant::now(), beat_ms: AtomicU64::new(0), stage: AtomicU8::new(0) });
    let watch_closed = Arc::new(AtomicBool::new(false));
    {
        let (watch, window, watch_closed) = (watch.clone(), renderer.window.0 as isize, watch_closed.clone());
        let _ = std::thread::Builder::new().name("penjaga-jendela".into()).spawn(move || watchdog(watch, window, watch_closed));
    }
    struct StopWatch(Arc<AtomicBool>);
    impl Drop for StopWatch {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
        }
    }
    let _stop_watch = StopWatch(watch_closed);
    let mut present_failures = 0u32;
    loop {
        watch.stage(1);
        if !renderer.pump() {
            closed.store(true, Ordering::SeqCst);
        }
        watch.beat();
        watch.stage(2);
        input::flush();
        watch.stage(3);
        let notices = input::take_notices();
        if !notices.is_empty() {
            update(&mut |r| r.gamepads.extend(notices.iter().cloned()));
        }
        for action in input::take_actions() {
            match action {
                Shortcut::Quit => renderer.request_close(),
                Shortcut::ToggleFullscreen => {
                    let on = !renderer.is_fullscreen();
                    renderer.set_fullscreen(on);
                }
                Shortcut::Minimize => renderer.minimize(),
                Shortcut::ToggleStats => show_stats = !show_stats,
                _ => {}
            }
        }
        watch.stage(4);
        let first = match rx.try_recv() {
            Ok(message) => message,
            Err(TryRecvError::Disconnected) => return,
            Err(TryRecvError::Empty) => {
                // Bangun oleh pesan jendela (input) ATAU frame baru, paling lama 5 ms (pembacaan stik XInput).
                wake.wait(5);
                continue;
            }
        };
        let mut latest: Option<(Picture, Instant)> = None;
        for message in std::iter::once(first).chain(rx.try_iter()) {
            match message {
                Message::Rumble { pad, low, high } => input::rumble(pad, low, high),
                Message::Status { network: text, warning: alert } => {
                    network = text;
                    warning = alert;
                }
                Message::Start { codec } => match Decoder::new(codec, &renderer.device, choice != DecoderChoice::Software && (choice == DecoderChoice::Hardware || hardware.contains(&codec))) {
                    Ok(d) => {
                        let name = if d.hardware { d.name.clone() } else { format!("{} (perangkat lunak)", d.name) };
                        update(&mut |r| r.decoder = name.clone());
                        decoder = Some(d);
                    }
                    Err(error) => {
                        update(&mut |r| r.error = Some(error.clone()));
                        closed.store(true, Ordering::SeqCst);
                        return;
                    }
                },
                Message::Frame { frame, received } => {
                    let Some(decoder) = decoder.as_mut() else { continue };
                    watch.stage(5);
                    let started = Instant::now();
                    match decoder.decode(&frame.data) {
                        Ok(Some(picture)) => {
                            decode_total += started.elapsed();
                            second_decode += started.elapsed();
                            second_decoded += 1;
                            if latest.is_some() {
                                update(&mut |r| r.skipped += 1);
                            }
                            update(&mut |r| r.decoded += 1);
                            latest = Some((picture, received));
                        }
                        Ok(None) => {}
                        Err(_) => {
                            update(&mut |r| r.decode_errors += 1);
                            need_idr.store(true, Ordering::SeqCst);
                        }
                    }
                }
            }
        }
        if let Some((picture, received)) = latest {
            watch.stage(6);
            let render_started = Instant::now();
            let stats = show_stats.then(|| format!("{network}\n{local_line}\nCtrl+Alt+Shift+S: tutup statistik"));
            if let Err(error) = renderer.present(&picture, stats.as_deref(), warning.as_deref()) {
                // Galat sesaat (mis. ukuran berubah saat Alt+Tab dari layar penuh) dulu langsung menutup stream. Sekarang
                // dicatat dan dicoba lagi di frame berikutnya; baru menyerah bila gagal terus ±1 detik.
                present_failures += 1;
                if present_failures == 1 || present_failures.is_multiple_of(50) {
                    super::note(format!("Gagal menampilkan frame ({present_failures}x berturut-turut): {error}"));
                }
                if present_failures >= 150 {
                    update(&mut |r| r.error = Some(error.clone()));
                    closed.store(true, Ordering::SeqCst);
                    return;
                }
                continue;
            }
            if present_failures > 0 {
                super::note(format!("Tampil pulih setelah {present_failures} frame gagal"));
                present_failures = 0;
            }
            render_total += render_started.elapsed();
            watch.stage(7);
            latency_total += received.elapsed();
            second_latency += received.elapsed();
            second_presented += 1;
            if second_start.elapsed() >= Duration::from_secs(1) {
                let n = second_presented.max(1) as f64;
                local_line = format!(
                    "Tampil: {:.0} FPS, decode {:.2} ms, tiba -> tampil {:.2} ms",
                    second_presented as f64 / second_start.elapsed().as_secs_f64(),
                    second_decode.as_secs_f64() * 1000.0 / second_decoded.max(1) as f64,
                    second_latency.as_secs_f64() * 1000.0 / n
                );
                (second_start, second_presented, second_decode, second_latency, second_decoded) = (Instant::now(), 0, Duration::ZERO, Duration::ZERO, 0);
            }
            let presented = renderer.presented;
            update(&mut |r| {
                r.presented = presented;
                r.decode_ms = decode_total.as_secs_f64() * 1000.0 / r.decoded.max(1) as f64;
                r.receive_to_present_ms = latency_total.as_secs_f64() * 1000.0 / presented.max(1) as f64;
                r.render_ms = render_total.as_secs_f64() * 1000.0 / presented.max(1) as f64;
            });
        }
    }
}
