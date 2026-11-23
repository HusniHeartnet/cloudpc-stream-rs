//! Mikrofon pemain -> host (ekstensi CloudPC, lihat `proto::mic`): rekam -> frame 20 ms mono -> Opus -> antrean yang
//! diambil loop sesi dan dikirim lewat kanal kontrol.
//!
//! Privasi: mikrofon hanya direkam bila pemain memintanya (`--microphone`), bisa dibisukan kapan saja
//! (Ctrl+Alt+Shift+B), dan saat dibisukan perangkat rekam DITUTUP (bukan sekadar membuang sampel), jadi penanda
//! "mikrofon sedang dipakai" di Windows ikut padam.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cloudpc_stream_common::voice::{test_tone, VoiceEncoder};
use cloudpc_stream_proto::mic::{FRAME_MS, FRAME_SAMPLES};

/// Paling banyak segini paket menunggu dikirim (200 ms). Lebih dari itu berarti loop sesi tertahan: yang lama dibuang,
/// karena suara basi lebih mengganggu daripada suara terpotong.
const MAX_QUEUED: usize = 10;

/// Ada mikrofon aktif di proses ini (untuk pintasan bisu).
static PRESENT: AtomicBool = AtomicBool::new(false);
static MUTED: AtomicBool = AtomicBool::new(false);

/// Balik bisu/nyala. `None` = stream ini tidak memakai mikrofon; selain itu keadaan baru (`true` = dibisukan).
pub fn toggle_mute() -> Option<bool> {
    if !PRESENT.load(Ordering::SeqCst) {
        return None;
    }
    Some(!MUTED.fetch_xor(true, Ordering::SeqCst))
}

/// Kalimat untuk ditampilkan setelah pintasan bisu ditekan.
pub fn toggle_notice() -> String {
    match toggle_mute() {
        None => "Mikrofon tidak aktif di stream ini".into(),
        Some(true) => "Mikrofon dibisukan".into(),
        Some(false) => "Mikrofon menyala".into(),
    }
}

/// Sampel mono yang datang sepotong-sepotong -> paket Opus per 20 ms.
pub struct Framer {
    encoder: VoiceEncoder,
    pending: Vec<f32>,
}

impl Framer {
    pub fn new() -> Result<Framer, String> {
        Ok(Framer { encoder: VoiceEncoder::new()?, pending: Vec::with_capacity(FRAME_SAMPLES * 2) })
    }

    pub fn push(&mut self, samples: &[f32]) -> Vec<Vec<u8>> {
        self.pending.extend_from_slice(samples);
        let mut packets = Vec::new();
        while self.pending.len() >= FRAME_SAMPLES {
            if let Ok(packet) = self.encoder.encode(&self.pending[..FRAME_SAMPLES]) {
                packets.push(packet);
            }
            self.pending.drain(..FRAME_SAMPLES);
        }
        packets
    }

    /// Buang potongan frame yang belum lengkap (rekaman berhenti: dibisukan atau perangkat berganti).
    pub fn clear(&mut self) {
        self.pending.clear();
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MicStats {
    /// Paket Opus yang dihasilkan.
    pub packets: u64,
    /// Paket yang dibuang karena antrean kirim penuh.
    pub dropped: u64,
    /// Berapa kali perangkat rekam dibuka (1 = tidak pernah berganti/dibisukan).
    pub opens: u64,
    /// Tingkat suara tertinggi yang terekam, 0..=1000 (per mil skala penuh). Hanya angka; sampel tidak disimpan.
    pub peak_permille: u32,
}

#[derive(Default)]
struct Shared {
    queue: Mutex<VecDeque<Vec<u8>>>,
    packets: AtomicU64,
    dropped: AtomicU64,
    opens: AtomicU64,
    peak_permille: AtomicU64,
    stop: AtomicBool,
    /// Galat perangkat terakhir yang belum dilaporkan ke log.
    trouble: Mutex<Option<String>>,
}

impl Shared {
    fn note_level(&self, samples: &[f32]) {
        let peak = samples.iter().fold(0f32, |peak, sample| peak.max(sample.abs()));
        self.peak_permille.fetch_max((peak.min(1.0) * 1000.0) as u64, Ordering::Relaxed);
    }

    fn offer(&self, packets: Vec<Vec<u8>>) {
        let Ok(mut queue) = self.queue.lock() else { return };
        for packet in packets {
            self.packets.fetch_add(1, Ordering::Relaxed);
            if queue.len() >= MAX_QUEUED {
                queue.pop_front();
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
            queue.push_back(packet);
        }
    }
}

/// Mikrofon yang sedang direkam di thread sendiri.
pub struct Microphone {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Microphone {
    /// Rekam perangkat mikrofon bawaan Windows (peran "komunikasi"). `muted`: mulai dalam keadaan bisu.
    #[cfg(windows)]
    pub fn open(muted: bool) -> Result<Microphone, String> {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let microphone = Self::spawn(muted, move |shared| capture::run(shared, &ready_tx))?;
        // Perangkat dibuka sekali di awal supaya "tidak ada mikrofon" ketahuan sebelum stream jalan (kecuali mulai bisu).
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(microphone),
            Ok(Err(why)) => Err(why),
            Err(_) => Err("perangkat mikrofon tidak menjawab".into()),
        }
    }

    #[cfg(not(windows))]
    pub fn open(_muted: bool) -> Result<Microphone, String> {
        Err("mikrofon belum didukung di platform ini".into())
    }

    /// Uji: nada 440 Hz sebagai pengganti mikrofon, dengan tempo waktu nyata.
    pub fn test_tone() -> Result<Microphone, String> {
        Self::spawn(false, |shared| {
            let Ok(mut framer) = Framer::new() else { return };
            let start = Instant::now();
            let mut frame = 0u64;
            while !shared.stop.load(Ordering::SeqCst) {
                let due = start + Duration::from_millis(u64::from(FRAME_MS) * frame);
                if let Some(wait) = due.checked_duration_since(Instant::now()) {
                    std::thread::sleep(wait);
                }
                if !MUTED.load(Ordering::SeqCst) {
                    let tone = test_tone(440.0, frame);
                    shared.note_level(&tone);
                    shared.offer(framer.push(&tone));
                }
                frame += 1;
            }
        })
    }

    fn spawn(muted: bool, body: impl FnOnce(&Shared) + Send + 'static) -> Result<Microphone, String> {
        let shared = Arc::new(Shared::default());
        MUTED.store(muted, Ordering::SeqCst);
        PRESENT.store(true, Ordering::SeqCst);
        let for_thread = shared.clone();
        let thread = std::thread::Builder::new().name("mikrofon".into()).spawn(move || body(&for_thread)).map_err(|e| format!("thread mikrofon: {e}"))?;
        Ok(Microphone { shared, thread: Some(thread) })
    }

    pub fn stats(&self) -> MicStats {
        MicStats { packets: self.shared.packets.load(Ordering::Relaxed), dropped: self.shared.dropped.load(Ordering::Relaxed), opens: self.shared.opens.load(Ordering::Relaxed), peak_permille: self.shared.peak_permille.load(Ordering::Relaxed) as u32 }
    }

    /// Galat perangkat sejak pemanggilan terakhir (dicatat sekali).
    pub fn take_trouble(&self) -> Option<String> {
        self.shared.trouble.lock().ok()?.take()
    }
}

impl crate::session::MicSource for Microphone {
    fn take_packets(&mut self) -> Vec<Vec<u8>> {
        self.shared.queue.lock().map(|mut queue| queue.drain(..).collect()).unwrap_or_default()
    }

    fn take_trouble(&mut self) -> Option<String> {
        Microphone::take_trouble(self)
    }
}

impl Drop for Microphone {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        PRESENT.store(false, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Rekam WASAPI (mode bersama, berbasis event): float mono 48 kHz, Windows yang mengonversi dari format perangkat.
#[cfg(windows)]
mod capture {
    use std::sync::atomic::Ordering;
    use std::sync::mpsc::Sender;
    use std::time::{Duration, Instant};

    use windows::core::Interface;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::Media::Audio::{
        eCapture, eCommunications, AudioCategory_Communications, AudioClientProperties, IAudioCaptureClient, IAudioClient, IAudioClient2, IMMDeviceEnumerator, MMDeviceEnumerator, AUDCLNT_BUFFERFLAGS_SILENT,
        AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, AUDCLNT_STREAMOPTIONS_NONE, WAVEFORMATEX,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED};
    use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

    use cloudpc_stream_proto::mic::SAMPLE_RATE;

    use super::{Framer, Shared, MUTED};

    const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
    /// Penyangga perangkat (satuan 100 ns): 40 ms.
    const DEVICE_BUFFER: i64 = 400_000;

    struct Device {
        id: String,
        client: IAudioClient,
        capture: IAudioCaptureClient,
        event: HANDLE,
    }

    impl Drop for Device {
        fn drop(&mut self) {
            // SAFETY: klien dan event masih hidup; Stop pada klien yang sudah berhenti tidak berbahaya.
            unsafe {
                let _ = self.client.Stop();
                let _ = CloseHandle(self.event);
            }
        }
    }

    fn describe(what: &str, error: windows::core::Error) -> String {
        format!("{what}: {error}")
    }

    fn default_id(enumerator: &IMMDeviceEnumerator) -> Option<String> {
        // SAFETY: panggilan COM biasa; string dari GetId dibebaskan setelah disalin.
        unsafe {
            let device = enumerator.GetDefaultAudioEndpoint(eCapture, eCommunications).ok()?;
            let id = device.GetId().ok()?;
            let text = id.to_string().ok();
            CoTaskMemFree(Some(id.0 as *const _));
            text
        }
    }

    fn open_default(enumerator: &IMMDeviceEnumerator) -> Result<Device, String> {
        // SAFETY: panggilan COM/WASAPI biasa di thread mikrofon; semua hasil diperiksa.
        unsafe {
            let device = enumerator.GetDefaultAudioEndpoint(eCapture, eCommunications).map_err(|e| describe("tidak ada mikrofon di perangkat ini", e))?;
            let id = default_id(enumerator).unwrap_or_default();
            let client: IAudioClient = device.Activate(CLSCTX_ALL, None).map_err(|e| describe("IAudioClient", e))?;
            // Kategori "komunikasi": Windows menerapkan pemrosesan suara panggilan milik perangkat/sistem (peredam
            // gema dan bising bila tersedia). Gagal di sini bukan masalah: rekaman tetap jalan tanpa pemrosesan itu.
            if let Ok(client2) = client.cast::<IAudioClient2>() {
                let properties = AudioClientProperties { cbSize: std::mem::size_of::<AudioClientProperties>() as u32, bIsOffload: false.into(), eCategory: AudioCategory_Communications, Options: AUDCLNT_STREAMOPTIONS_NONE };
                let _ = client2.SetClientProperties(&properties);
            }
            let format = WAVEFORMATEX { wFormatTag: WAVE_FORMAT_IEEE_FLOAT, nChannels: 1, nSamplesPerSec: SAMPLE_RATE, nAvgBytesPerSec: SAMPLE_RATE * 4, nBlockAlign: 4, wBitsPerSample: 32, cbSize: 0 };
            let flags = AUDCLNT_STREAMFLAGS_EVENTCALLBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
            client.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, DEVICE_BUFFER, 0, &format, None).map_err(|e| describe("mikrofon tidak bisa dibuka (izin mikrofon Windows?)", e))?;
            let event = CreateEventW(None, false, false, None).map_err(|e| describe("CreateEvent", e))?;
            // Dibungkus dulu supaya event ditutup juga bila langkah berikutnya gagal.
            let device = Device { id, client: client.clone(), capture: client.GetService().map_err(|e| describe("IAudioCaptureClient", e))?, event };
            client.SetEventHandle(event).map_err(|e| describe("SetEventHandle", e))?;
            client.Start().map_err(|e| describe("IAudioClient::Start", e))?;
            Ok(device)
        }
    }

    /// Ambil semua sampel yang sudah tersedia. `false` = perangkat tidak bisa dipakai lagi.
    fn read(device: &Device, out: &mut Vec<f32>) -> bool {
        // SAFETY: penyangga dari GetBuffer sah sampai ReleaseBuffer dan berisi `frames` float mono.
        unsafe {
            if WaitForSingleObject(device.event, 100) != WAIT_OBJECT_0 {
                return true;
            }
            loop {
                match device.capture.GetNextPacketSize() {
                    Ok(0) => return true,
                    Ok(_) => {}
                    Err(_) => return false,
                }
                let mut data: *mut u8 = std::ptr::null_mut();
                let (mut frames, mut flags) = (0u32, 0u32);
                if device.capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None).is_err() {
                    return false;
                }
                if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null() {
                    out.resize(out.len() + frames as usize, 0.0);
                } else {
                    out.extend_from_slice(std::slice::from_raw_parts(data as *const f32, frames as usize));
                }
                if device.capture.ReleaseBuffer(frames).is_err() {
                    return false;
                }
            }
        }
    }

    pub(super) fn run(shared: &Shared, ready: &Sender<Result<(), String>>) {
        // SAFETY: inisialisasi COM untuk thread ini; sudah diinisialisasi = bukan galat.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let enumerator: IMMDeviceEnumerator = match unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) } {
            Ok(enumerator) => enumerator,
            Err(error) => {
                let _ = ready.send(Err(describe("MMDeviceEnumerator", error)));
                return;
            }
        };
        let Ok(mut framer) = Framer::new() else {
            let _ = ready.send(Err("encoder suara gagal dibuat".into()));
            return;
        };
        let mut device: Option<Device> = None;
        let mut announced = false;
        let (mut retry_at, mut last_check) = (Instant::now(), Instant::now());
        let mut samples = Vec::with_capacity(4800);
        while !shared.stop.load(Ordering::SeqCst) {
            if MUTED.load(Ordering::SeqCst) {
                // Dibisukan: perangkat ditutup, jadi Windows tidak lagi menandai mikrofon sedang dipakai.
                if device.take().is_some() {
                    framer.clear();
                }
                if !announced {
                    announced = true;
                    let _ = ready.send(Ok(()));
                }
                std::thread::sleep(Duration::from_millis(30));
                continue;
            }
            if device.is_none() {
                if Instant::now() < retry_at {
                    std::thread::sleep(Duration::from_millis(30));
                    continue;
                }
                retry_at = Instant::now() + Duration::from_secs(1);
                match open_default(&enumerator) {
                    Ok(opened) => {
                        shared.opens.fetch_add(1, Ordering::Relaxed);
                        device = Some(opened);
                        if !announced {
                            announced = true;
                            let _ = ready.send(Ok(()));
                        }
                    }
                    Err(why) => {
                        if !announced {
                            let _ = ready.send(Err(why));
                            return;
                        }
                        if let Ok(mut trouble) = shared.trouble.lock() {
                            *trouble = Some(why);
                        }
                        continue;
                    }
                }
            }
            let Some(current) = device.as_ref() else { continue };
            samples.clear();
            let healthy = read(current, &mut samples);
            shared.note_level(&samples);
            shared.offer(framer.push(&samples));
            // Mikrofon bawaan berganti (headset dicolok/dicabut): ikut pindah, seperti pemutar suara.
            let moved = last_check.elapsed() >= Duration::from_secs(1) && {
                last_check = Instant::now();
                default_id(&enumerator).is_some_and(|id| id != current.id)
            };
            if !healthy || moved {
                device = None;
                framer.clear();
                retry_at = Instant::now();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cloudpc_stream_common::voice::{tone_purity, VoiceDecoder};

    #[test]
    fn framer_cuts_exact_twenty_millisecond_packets_from_odd_chunks() {
        let mut framer = Framer::new().unwrap();
        let tone: Vec<f32> = (0..10u64).flat_map(|frame| test_tone(440.0, frame)).collect();
        let mut packets = Vec::new();
        // Potongan 441 sampel (seperti perangkat 44,1 kHz): tidak pernah pas satu frame.
        for chunk in tone.chunks(441) {
            packets.extend(framer.push(chunk));
        }
        assert_eq!(packets.len(), 10);
        let mut decoder = VoiceDecoder::new().unwrap();
        let pcm: Vec<i16> = packets.iter().flat_map(|packet| decoder.decode(packet)).collect();
        assert!(tone_purity(&pcm[FRAME_SAMPLES * 3..], 440.0) > 0.9);
    }

    #[test]
    fn clear_drops_the_partial_frame() {
        let mut framer = Framer::new().unwrap();
        assert!(framer.push(&[0.0; FRAME_SAMPLES - 1]).is_empty());
        framer.clear();
        assert!(framer.push(&[0.0; 1]).is_empty(), "sisa lama tidak boleh menyelesaikan frame");
    }

    #[test]
    fn queue_keeps_only_the_newest_packets() {
        let shared = Shared::default();
        shared.offer((0..MAX_QUEUED as u8 + 3).map(|n| vec![n]).collect());
        let queue = shared.queue.lock().unwrap();
        assert_eq!(queue.len(), MAX_QUEUED);
        assert_eq!(queue.front(), Some(&vec![3u8]), "yang paling lama dibuang");
        assert_eq!(shared.dropped.load(Ordering::Relaxed), 3);
    }
}
