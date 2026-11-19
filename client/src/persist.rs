//! `stream --persist`: beberapa stream berturut-turut di JENDELA YANG SAMA.
//!
//! Temuan 20 Nov 2026 (tester Mac yang live di TikTok): tiap kali stream dibuka ulang (ganti kualitas, koneksi putus lalu
//! "Sambung lagi"), aplikasi menjalankan proses client baru = jendela baru, sehingga Window Capture TikTok LIVE/OBS
//! kehilangan jendelanya dan harus dipilih ulang di tengah live. Dengan `--persist`:
//! - koneksi ke PC putus setelah stream sempat berjalan: client menyambung ulang sendiri (resume) di jendela yang sama,
//!   selama `RECONNECT_FOR`;
//! - aplikasi mengganti kualitas dengan menulis satu baris ke stdin client, misalnya
//!   `quality --bitrate 15000 --fps 60 --resolution 1920x1080 --video-codec auto`; stream lama ditutup baik-baik dan
//!   stream baru dibuka di jendela yang sama.
//!
//! Tanpa `--persist` perilakunya sama persis dengan sebelumnya (satu stream, lalu keluar), seperti Moonlight.

use std::io::BufRead;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::session::{StreamOptions, StreamReport};

/// Lama client mencoba menyambung ulang sendiri setelah koneksi putus (lebih pendek dari masa tenggang server 180 dtk).
pub const RECONNECT_FOR: Duration = Duration::from_secs(90);
/// Stream yang sudah berjalan selama ini dianggap pulih: hitungan waktu sambung ulang dimulai dari awal lagi.
const HEALTHY_AFTER: Duration = Duration::from_secs(15);

/// Perintah dari aplikasi lewat stdin.
#[derive(Default, Clone)]
pub struct Control {
    quality: Arc<Mutex<Option<Vec<String>>>>,
}

impl Control {
    /// Baca stdin di thread latar. Stdin tertutup/tidak ada = tidak ada perintah (tetap aman).
    pub fn from_stdin() -> Control {
        let control = Control::default();
        let reader = control.clone();
        let _ = std::thread::Builder::new().name("perintah-stdin".into()).spawn(move || {
            for line in std::io::stdin().lock().lines() {
                let Ok(line) = line else { break };
                reader.push(&line);
            }
        });
        control
    }

    /// Satu baris perintah. Baris tak dikenal diabaikan.
    pub fn push(&self, line: &str) {
        let mut words = line.split_whitespace().map(str::to_string);
        if words.next().as_deref() == Some("quality") {
            *self.quality.lock().unwrap() = Some(words.collect());
        }
    }

    pub fn pending(&self) -> bool {
        self.quality.lock().unwrap().is_some()
    }

    fn take(&self) -> Option<Vec<String>> {
        self.quality.lock().unwrap().take()
    }
}

/// Yang dibutuhkan `run_series` dari jendela/suara/sesi (dipisah supaya bisa diuji tanpa host dan tanpa jendela).
pub trait Env {
    /// Satu stream sampai selesai. `stop` = berhenti sekarang dengan baik-baik.
    fn run(&mut self, options: &StreamOptions, stop: &mut dyn FnMut() -> bool) -> Result<StreamReport, String>;
    /// Pengguna menutup jendela.
    fn closed(&self) -> bool;
    /// Aplikasi meminta berhenti (WM_CLOSE/SIGTERM) atau `--seconds` habis.
    fn stopped(&self) -> bool;
    /// Pesan ke log dan, bila ada, ke layar (peringatan di jendela).
    fn notify(&mut self, text: &str);
    /// Opsi stream baru dari argumen perintah `quality`.
    fn reapply(&mut self, options: &StreamOptions, words: &[String]) -> StreamOptions;
    fn sleep(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// Jalankan stream; dengan `control` (mode `--persist`) sambung ulang dan ganti kualitas di jendela yang sama.
/// Mengembalikan laporan stream terakhir yang berhasil (untuk ringkasan dan blok statistik).
pub fn run_series(env: &mut dyn Env, mut options: StreamOptions, control: Option<&Control>) -> Result<StreamReport, String> {
    let Some(control) = control else {
        return env.run(&options, &mut || false);
    };
    let mut last_ok: Option<StreamReport> = None;
    let mut reconnect_until: Option<Instant> = None;
    loop {
        let started = Instant::now();
        let result = env.run(&options, &mut || control.pending());
        let ran_well = result.as_ref().is_ok_and(|r| r.video_packets > 0);
        if ran_well && started.elapsed() >= HEALTHY_AFTER {
            reconnect_until = None;
        }
        let ended_by_host = result.as_ref().is_ok_and(|r| r.termination.is_some());
        if let Ok(report) = &result {
            if report.video_packets > 0 || last_ok.is_none() {
                last_ok = Some(report.clone());
            }
        }
        if env.closed() || env.stopped() {
            return finish(result, last_ok);
        }
        if let Some(words) = control.take() {
            options = env.reapply(&options, &words);
            env.notify(&format!("Menerapkan kualitas baru di jendela yang sama: {}x{}@{} {} kbps", options.width, options.height, options.fps, options.bitrate_kbps));
            reconnect_until = None;
            continue;
        }
        // Host sengaja mengakhiri (sesi selesai, aplikasi ditutup), atau stream belum pernah jalan (biar aplikasi yang
        // memutuskan, mis. pindah ke relay): selesai seperti biasa.
        if ended_by_host || last_ok.as_ref().is_none_or(|r| r.video_packets == 0) {
            return finish(result, last_ok);
        }
        let until = *reconnect_until.get_or_insert_with(|| Instant::now() + RECONNECT_FOR);
        if Instant::now() >= until {
            env.notify("Sambung ulang otomatis menyerah; jendela ditutup");
            return finish(result, last_ok);
        }
        let why = match &result {
            Ok(_) => "kanal kontrol diputus".to_string(),
            Err(e) => e.clone(),
        };
        env.notify(&format!("Koneksi ke PC terputus ({why}). Menyambung ulang di jendela yang sama..."));
        // Tunggu sebentar, tetap peka terhadap jendela ditutup / permintaan berhenti / ganti kualitas.
        let wait_until = Instant::now() + Duration::from_secs(1);
        while Instant::now() < wait_until && !env.closed() && !env.stopped() && !control.pending() {
            env.sleep(Duration::from_millis(100));
        }
    }
}

fn finish(result: Result<StreamReport, String>, last_ok: Option<StreamReport>) -> Result<StreamReport, String> {
    match result {
        Ok(report) if report.video_packets > 0 => Ok(report),
        Ok(report) => Ok(last_ok.filter(|r| r.video_packets > 0).unwrap_or(report)),
        Err(e) => last_ok.filter(|r| r.video_packets > 0).ok_or(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// Lingkungan tiruan: hasil `run` diambil dari antrean; mencatat opsi tiap stream dan pesan.
    struct Fake {
        outcomes: VecDeque<Result<StreamReport, String>>,
        runs: Vec<StreamOptions>,
        notes: Vec<String>,
        closed_after: Option<usize>,
        control: Option<Control>,
        command_during: Option<(usize, &'static str)>,
    }

    impl Fake {
        fn new(outcomes: Vec<Result<StreamReport, String>>) -> Fake {
            Fake { outcomes: outcomes.into(), runs: Vec::new(), notes: Vec::new(), closed_after: None, control: None, command_during: None }
        }
    }

    impl Env for Fake {
        fn run(&mut self, options: &StreamOptions, stop: &mut dyn FnMut() -> bool) -> Result<StreamReport, String> {
            self.runs.push(options.clone());
            if let (Some((n, line)), Some(control)) = (self.command_during, &self.control) {
                if n == self.runs.len() {
                    control.push(line);
                    assert!(stop(), "perintah kualitas harus menghentikan stream yang berjalan");
                }
            }
            self.outcomes.pop_front().unwrap_or_else(|| Err("tidak ada hasil lagi".into()))
        }
        fn closed(&self) -> bool {
            self.closed_after.is_some_and(|n| self.runs.len() >= n)
        }
        fn stopped(&self) -> bool {
            false
        }
        fn notify(&mut self, text: &str) {
            self.notes.push(text.to_string());
        }
        fn reapply(&mut self, options: &StreamOptions, words: &[String]) -> StreamOptions {
            let mut next = options.clone();
            if let Some(i) = words.iter().position(|w| w == "--bitrate") {
                next.bitrate_kbps = words[i + 1].parse().unwrap();
            }
            next
        }
        fn sleep(&mut self, _: Duration) {}
    }

    fn streamed(packets: u64) -> Result<StreamReport, String> {
        Ok(StreamReport { video_packets: packets, disconnected: true, ..Default::default() })
    }

    fn opts() -> StreamOptions {
        StreamOptions { app: "Desktop".into(), width: 1920, height: 1080, fps: 60, bitrate_kbps: 20_000, ..Default::default() }
    }

    #[test]
    fn without_persist_runs_exactly_once() {
        let mut env = Fake::new(vec![streamed(100), streamed(100)]);
        assert!(run_series(&mut env, opts(), None).is_ok());
        assert_eq!(env.runs.len(), 1);
    }

    #[test]
    fn dropped_connection_reconnects_in_the_same_window() {
        let control = Control::default();
        let mut env = Fake::new(vec![streamed(500), Err("kanal kontrol tidak tersambung".into()), streamed(300)]);
        env.closed_after = Some(3); // pemain menutup jendela setelah stream ketiga
        let report = run_series(&mut env, opts(), Some(&control)).unwrap();
        assert_eq!(env.runs.len(), 3, "dua kali sambung ulang, satu jendela");
        assert_eq!(report.video_packets, 300);
        assert!(env.notes.iter().filter(|n| n.contains("Menyambung ulang")).count() == 2);
    }

    #[test]
    fn quality_command_restarts_stream_with_new_options() {
        let control = Control::default();
        let mut env = Fake::new(vec![Ok(StreamReport { video_packets: 100, ..Default::default() }), streamed(50)]);
        env.control = Some(control.clone());
        env.command_during = Some((1, "quality --bitrate 8000 --fps 60"));
        env.closed_after = Some(2);
        run_series(&mut env, opts(), Some(&control)).unwrap();
        assert_eq!(env.runs.iter().map(|o| o.bitrate_kbps).collect::<Vec<_>>(), vec![20_000, 8_000]);
        assert!(env.notes[0].contains("8000 kbps"));
    }

    #[test]
    fn host_termination_and_never_started_streams_are_not_retried() {
        let control = Control::default();
        let mut env = Fake::new(vec![streamed(100), Ok(StreamReport { video_packets: 10, termination: Some(0x8003_0023), ..Default::default() })]);
        run_series(&mut env, opts(), Some(&control)).unwrap();
        assert_eq!(env.runs.len(), 2, "terminasi dari host = selesai");

        // Gagal sejak awal (mis. jalur langsung diblokir): kembali ke aplikasi, tanpa mencoba ulang.
        let mut env = Fake::new(vec![Err("RTSP gagal".into())]);
        assert_eq!(run_series(&mut env, opts(), Some(&control)).unwrap_err(), "RTSP gagal");
        assert_eq!(env.runs.len(), 1);
    }

    #[test]
    fn unknown_stdin_lines_are_ignored() {
        let control = Control::default();
        control.push("halo");
        control.push("");
        assert!(!control.pending());
        control.push("quality --fps 120");
        assert_eq!(control.take(), Some(vec!["--fps".to_string(), "120".to_string()]));
    }
}
