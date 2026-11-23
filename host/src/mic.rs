//! Mikrofon pemain di host (ekstensi CloudPC, lihat `proto::mic`): paket Opus dari kanal kontrol -> PCM -> keluaran.
//!
//! Windows tidak punya cara membuat mikrofon virtual tanpa driver kernel bertanda tangan, jadi keluarannya PERANGKAT
//! USB kecil ("CloudPC Mic", Raspberry Pi Pico dengan firmware `mic-pico`) yang oleh Windows dan game terlihat sebagai
//! mikrofon USB biasa. Host mengirim PCM ke perangkat itu lewat port serial USB-nya; perangkat itulah yang memegang
//! jam (ia menyajikan sampel ke Windows 1 ms sekali) dan menyangga jitter jaringan.
//!
//! Di host tidak ada jam putar: tiap paket didecode begitu tiba dan langsung diteruskan. Paket yang hilang diisi dari
//! salinan FEC di paket berikutnya atau tebakan Opus, supaya garis waktu suara tidak memendek.

use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cloudpc_stream_common::voice::VoiceDecoder;
use cloudpc_stream_proto::mic::{FRAME_MS, FRAME_SAMPLES, SAMPLE_RATE};

use crate::nvhttp::Host;

/// Ke mana suara mikrofon pemain dikeluarkan (`--mic`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MicTarget {
    /// Perangkat CloudPC Mic. `None` = cari sendiri dari VID/PID USB; `Some("COM7")` = port ini.
    Pico(Option<String>),
    /// Berkas WAV (uji).
    Wav(PathBuf),
}

impl MicTarget {
    /// `pico`, `pico:COM7`, atau `wav:<berkas>`.
    pub fn parse(text: &str) -> Result<MicTarget, String> {
        match text.split_once(':') {
            None if text == "pico" => Ok(MicTarget::Pico(None)),
            Some(("pico", port)) if !port.is_empty() => Ok(MicTarget::Pico(Some(port.to_string()))),
            Some(("wav", path)) if !path.is_empty() => Ok(MicTarget::Wav(PathBuf::from(path))),
            _ => Err(format!("--mic tidak dikenal: {text} (pakai pico, pico:COM7, atau wav:<berkas>)")),
        }
    }
}

/// Satu paket mikrofon dari kanal kontrol; `None` = sesi berakhir (buang sisa suara).
pub(crate) type MicMessage = Option<(u16, Vec<u8>)>;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReceiverStats {
    pub packets: u64,
    /// Frame hilang yang diisi dari salinan FEC di paket berikutnya.
    pub recovered: u64,
    /// Frame hilang yang diisi tebakan Opus.
    pub concealed: u64,
    /// Paket yang datang terlambat (sudah lewat gilirannya) dan dibuang.
    pub late: u64,
}

/// Lebih dari ini frame hilang berturut-turut tidak ditambal: dianggap jeda (mis. jaringan sempat putus).
const MAX_PATCHED_FRAMES: u16 = 5;
/// Tidak ada paket selama ini = pembicaraan baru (mis. mikrofon dibisukan lalu dinyalakan): decoder mulai bersih.
const TALK_GAP: Duration = Duration::from_millis(300);

/// Paket bernomor urut -> PCM berurutan. Tanpa penyangga: jitter ditangani perangkat keluaran.
pub struct MicReceiver {
    decoder: VoiceDecoder,
    next: Option<u16>,
    last_packet: Option<Instant>,
    stats: ReceiverStats,
}

impl MicReceiver {
    pub fn new() -> Result<MicReceiver, String> {
        Ok(MicReceiver { decoder: VoiceDecoder::new()?, next: None, last_packet: None, stats: ReceiverStats::default() })
    }

    pub fn stats(&self) -> ReceiverStats {
        self.stats
    }

    /// Sesi berakhir atau berganti: paket berikutnya adalah awal pembicaraan baru.
    pub fn restart(&mut self) {
        self.decoder.reset();
        self.next = None;
        self.last_packet = None;
    }

    /// PCM mono 48 kHz yang dihasilkan paket ini (kosong bila paket terlambat).
    pub fn push(&mut self, sequence: u16, opus: &[u8], now: Instant) -> Vec<i16> {
        self.stats.packets += 1;
        if self.last_packet.is_some_and(|last| now.duration_since(last) > TALK_GAP) {
            self.decoder.reset();
            self.next = None;
        }
        self.last_packet = Some(now);
        let mut pcm = Vec::with_capacity(FRAME_SAMPLES * 2);
        if let Some(next) = self.next {
            let ahead = sequence.wrapping_sub(next);
            if ahead >= 0x8000 {
                self.stats.late += 1;
                return pcm;
            }
            if (1..=MAX_PATCHED_FRAMES).contains(&ahead) {
                for _ in 1..ahead {
                    pcm.extend_from_slice(&self.decoder.conceal());
                    self.stats.concealed += 1;
                }
                pcm.extend_from_slice(&self.decoder.recover_from(opus));
                self.stats.recovered += 1;
            } else if ahead > MAX_PATCHED_FRAMES {
                self.decoder.reset();
            }
        }
        pcm.extend_from_slice(&self.decoder.decode(opus));
        self.next = Some(sequence.wrapping_add(1));
        pcm
    }
}

/// Format kawat serial host -> CloudPC Mic. HARUS sama dengan `mic-pico/src/link.rs` (firmware ada di repo lain,
/// jadi konstanta ini disalin; tes `serial_frame_layout` di kedua sisi memakai vektor yang sama).
pub mod link {
    /// VID pid.codes dengan PID uji: cukup untuk perangkat yang hanya dipakai di PC host sendiri.
    pub const USB_VID: u16 = 0x1209;
    pub const USB_PID: u16 = 0x0001;
    /// Awal tiap bingkai host -> perangkat: `[MAGIC][jenis][panjang u16 LE][isi]`.
    pub const MAGIC: u8 = 0xA5;
    /// Isi: PCM mono 48 kHz, 16 bit little-endian.
    pub const KIND_PCM: u8 = 0x01;
    /// Buang suara yang masih disangga (sesi berakhir). Tanpa isi.
    pub const KIND_FLUSH: u8 = 0x02;
    /// Atur penyangga perangkat: `[mulai putar u16][batas bawah u16][batas atas u16]`, satuan sampel.
    pub const KIND_CONFIG: u8 = 0x03;
    /// Masuk mode pengisian firmware (drive RPI-RP2) tanpa menekan tombol di papan. Isi harus `BOOT`.
    pub const KIND_BOOTLOADER: u8 = 0x7F;
    /// Awal laporan perangkat -> host (8 byte, tiap detik):
    /// `[STATUS_MAGIC][versi][isi penyangga u16 LE, sampel][kehabisan u16 LE][dibuang u16 LE, satuan 10 ms]`.
    pub const STATUS_MAGIC: u8 = 0x5A;
    pub const STATUS_SIZE: usize = 8;

    pub fn pcm_frame(pcm: &[i16]) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + pcm.len() * 2);
        out.extend_from_slice(&[MAGIC, KIND_PCM]);
        out.extend_from_slice(&((pcm.len() * 2) as u16).to_le_bytes());
        for sample in pcm {
            out.extend_from_slice(&sample.to_le_bytes());
        }
        out
    }

    pub fn flush_frame() -> [u8; 4] {
        [MAGIC, KIND_FLUSH, 0, 0]
    }

    pub fn config_frame(start: u16, low: u16, high: u16) -> [u8; 10] {
        let (s, l, h) = (start.to_le_bytes(), low.to_le_bytes(), high.to_le_bytes());
        [MAGIC, KIND_CONFIG, 6, 0, s[0], s[1], l[0], l[1], h[0], h[1]]
    }

    pub fn bootloader_frame() -> [u8; 8] {
        [MAGIC, KIND_BOOTLOADER, 4, 0, b'B', b'O', b'O', b'T']
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Status {
        pub version: u8,
        /// Sampel yang sedang disangga perangkat.
        pub buffered: u16,
        /// Berapa kali suara habis saat sedang diputar (jitter melebihi penyangga).
        pub underruns: u16,
        /// Suara yang dibuang karena penyangga penuh, satuan 10 ms.
        pub dropped: u16,
    }

    /// Ambil laporan terakhir yang utuh dari byte yang dikirim perangkat; sisanya (laporan terpotong) tetap di `buffer`.
    pub fn take_status(buffer: &mut Vec<u8>) -> Option<Status> {
        let mut latest = None;
        loop {
            match buffer.iter().position(|&byte| byte == STATUS_MAGIC) {
                None => {
                    buffer.clear();
                    return latest;
                }
                Some(start) => {
                    buffer.drain(..start);
                }
            }
            if buffer.len() < STATUS_SIZE {
                return latest;
            }
            let field = |at: usize| u16::from_le_bytes([buffer[at], buffer[at + 1]]);
            latest = Some(Status { version: buffer[1], buffered: field(2), underruns: field(4), dropped: field(6) });
            buffer.drain(..STATUS_SIZE);
        }
    }
}

/// Tujuan PCM mono 48 kHz.
pub trait MicOutput {
    fn write(&mut self, pcm: &[i16]) -> Result<(), String>;
    /// Sesi berakhir: buang suara yang belum diputar.
    fn flush(&mut self) -> Result<(), String>;
    /// Kabar dari perangkat sejak pemanggilan terakhir (untuk log), bila ada.
    fn report(&mut self) -> Option<String> {
        None
    }
}

/// Berkas WAV mono 16 bit 48 kHz; ukuran di header diperbarui tiap tulis supaya berkas selalu bisa dibaca.
pub struct WavOutput {
    file: std::fs::File,
    data_bytes: u32,
}

impl WavOutput {
    pub fn create(path: &std::path::Path) -> Result<WavOutput, String> {
        let file = std::fs::File::create(path).map_err(|e| format!("berkas {}: {e}", path.display()))?;
        let mut output = WavOutput { file, data_bytes: 0 };
        output.header().map_err(|e| format!("menulis {}: {e}", path.display()))?;
        Ok(output)
    }

    fn header(&mut self) -> std::io::Result<()> {
        let mut header = Vec::with_capacity(44);
        header.extend_from_slice(b"RIFF");
        header.extend_from_slice(&(36 + self.data_bytes).to_le_bytes());
        header.extend_from_slice(b"WAVEfmt ");
        header.extend_from_slice(&16u32.to_le_bytes());
        header.extend_from_slice(&1u16.to_le_bytes()); // PCM
        header.extend_from_slice(&1u16.to_le_bytes()); // mono
        header.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
        header.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
        header.extend_from_slice(&2u16.to_le_bytes());
        header.extend_from_slice(&16u16.to_le_bytes());
        header.extend_from_slice(b"data");
        header.extend_from_slice(&self.data_bytes.to_le_bytes());
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(&header)?;
        self.file.seek(SeekFrom::End(0))?;
        Ok(())
    }
}

impl MicOutput for WavOutput {
    fn write(&mut self, pcm: &[i16]) -> Result<(), String> {
        let bytes: Vec<u8> = pcm.iter().flat_map(|sample| sample.to_le_bytes()).collect();
        self.data_bytes += bytes.len() as u32;
        self.file.write_all(&bytes).and_then(|_| self.header()).and_then(|_| self.file.flush()).map_err(|e| format!("menulis WAV: {e}"))
    }

    fn flush(&mut self) -> Result<(), String> {
        Ok(())
    }
}

/// Perangkat CloudPC Mic lewat port serial USB-nya.
pub struct PicoOutput {
    port: Box<dyn serialport::SerialPort>,
    name: String,
    inbox: Vec<u8>,
    last_status: Option<link::Status>,
    last_report: Instant,
}

/// Port serial perangkat CloudPC Mic yang tercolok (dari VID/PID USB), bila ada.
pub fn find_pico() -> Option<String> {
    let ports = serialport::available_ports().ok()?;
    ports.into_iter().find_map(|port| match port.port_type {
        serialport::SerialPortType::UsbPort(usb) if usb.vid == link::USB_VID && usb.pid == link::USB_PID => Some(port.port_name),
        _ => None,
    })
}

impl PicoOutput {
    pub fn open(port: Option<&str>) -> Result<PicoOutput, String> {
        let name = match port {
            Some(name) => name.to_string(),
            None => find_pico().ok_or("perangkat CloudPC Mic tidak tercolok (tidak ada port serial USB 1209:0001)")?,
        };
        // Kecepatan baud tidak berarti untuk serial USB; batas waktu tulis pendek supaya perangkat yang macet tidak
        // menahan thread mikrofon.
        let mut port = serialport::new(&name, 115_200).timeout(Duration::from_millis(50)).open().map_err(|e| format!("port {name}: {e}"))?;
        port.write_data_terminal_ready(true).map_err(|e| format!("port {name}: {e}"))?;
        let mut output = PicoOutput { port, name, inbox: Vec::new(), last_status: None, last_report: Instant::now() };
        output.flush()?;
        Ok(output)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Laporan terakhir dari perangkat (dikirimnya tiap detik), bila sudah ada.
    pub fn status(&mut self) -> Option<link::Status> {
        self.read_status();
        self.last_status
    }

    /// Atur penyangga perangkat (milidetik): mulai putar, batas bawah, batas atas.
    pub fn configure(&mut self, start_ms: u16, low_ms: u16, high_ms: u16) -> Result<(), String> {
        let samples = |ms: u16| ms.saturating_mul((SAMPLE_RATE / 1000) as u16);
        self.send(&link::config_frame(samples(start_ms), samples(low_ms), samples(high_ms)))
    }

    /// Suruh perangkat masuk mode pengisian firmware (muncul sebagai drive RPI-RP2).
    pub fn enter_bootloader(mut self) -> Result<(), String> {
        self.send(&link::bootloader_frame())
    }

    fn send(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.port.write_all(bytes).map_err(|e| format!("port {}: {e}", self.name))
    }

    fn read_status(&mut self) {
        let waiting = self.port.bytes_to_read().unwrap_or(0) as usize;
        if waiting == 0 {
            return;
        }
        let mut chunk = vec![0u8; waiting.min(256)];
        if let Ok(read) = self.port.read(&mut chunk) {
            self.inbox.extend_from_slice(&chunk[..read]);
            if let Some(status) = link::take_status(&mut self.inbox) {
                self.last_status = Some(status);
            }
        }
    }
}

impl MicOutput for PicoOutput {
    fn write(&mut self, pcm: &[i16]) -> Result<(), String> {
        self.read_status();
        self.send(&link::pcm_frame(pcm))
    }

    fn flush(&mut self) -> Result<(), String> {
        self.send(&link::flush_frame())
    }

    fn report(&mut self) -> Option<String> {
        self.read_status();
        if self.last_report.elapsed() < Duration::from_secs(30) {
            return None;
        }
        self.last_report = Instant::now();
        let status = self.last_status.take()?;
        Some(format!(
            "CloudPC Mic {} (firmware v{}): disangga {} ms, suara habis {} kali, dibuang {} ms",
            self.name,
            status.version,
            u32::from(status.buffered) * 1000 / SAMPLE_RATE,
            status.underruns,
            u32::from(status.dropped) * 10
        ))
    }
}

/// Uji perangkat tanpa sesi stream: kirim nada 440 Hz selama `seconds` dengan tempo waktu nyata (rekam "CloudPC Mic"
/// di Windows untuk mendengarnya) dan kembalikan laporan perangkat tiap detik. `seconds` 0 = hanya baca laporan.
pub fn check_pico(port: Option<&str>, seconds: u64, mut progress: impl FnMut(String)) -> Result<(), String> {
    let mut pico = PicoOutput::open(port)?;
    progress(format!("CloudPC Mic ditemukan di {}", pico.name()));
    let frames = seconds.max(2) * 1000 / u64::from(FRAME_MS);
    let start = Instant::now();
    let mut reported = 0u64;
    for frame in 0..frames {
        if let Some(wait) = (start + Duration::from_millis(u64::from(FRAME_MS) * frame)).checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
        if seconds > 0 {
            let tone: Vec<i16> = cloudpc_stream_common::voice::test_tone(440.0, frame).iter().map(|s| (s * 32767.0) as i16).collect();
            pico.write(&tone)?;
        }
        let second = start.elapsed().as_secs();
        if second > reported {
            reported = second;
            match pico.status() {
                Some(status) => progress(format!("detik {second}: firmware v{}, disangga {} ms, suara habis {} kali, dibuang {} ms", status.version, u32::from(status.buffered) * 1000 / SAMPLE_RATE, status.underruns, u32::from(status.dropped) * 10)),
                None => progress(format!("detik {second}: belum ada laporan dari perangkat")),
            }
        }
    }
    pico.flush()?;
    match pico.status() {
        Some(_) => Ok(()),
        None => Err("port terbuka tetapi perangkat tidak mengirim laporan: bukan CloudPC Mic, atau firmware macet".into()),
    }
}

fn open_output(target: &MicTarget) -> Result<(Box<dyn MicOutput>, String), String> {
    match target {
        MicTarget::Wav(path) => Ok((Box::new(WavOutput::create(path)?), format!("berkas {}", path.display()))),
        MicTarget::Pico(port) => {
            let output = PicoOutput::open(port.as_deref())?;
            let name = format!("CloudPC Mic di {}", output.name());
            Ok((Box::new(output), name))
        }
    }
}

/// Thread mikrofon: hidup selama host hidup. Keluaran dibuka sejak awal (supaya log langsung menyebut ada tidaknya
/// perangkat) dan dicoba lagi tiap beberapa detik bila hilang (kabel dicabut, perangkat di-reset).
pub(crate) fn serve(host: Arc<Host>, target: MicTarget, packets: Receiver<MicMessage>) {
    let mut receiver = match MicReceiver::new() {
        Ok(receiver) => receiver,
        Err(why) => return host.log(|| format!("mikrofon: {why}")),
    };
    let mut output: Option<Box<dyn MicOutput>> = None;
    let mut retry_at = Instant::now();
    let mut last_failure = String::new();
    let mut talking = false;
    let mut frames = 0u64;
    loop {
        if output.is_none() && Instant::now() >= retry_at {
            retry_at = Instant::now() + Duration::from_secs(3);
            match open_output(&target) {
                Ok((opened, name)) => {
                    host.log(|| format!("mikrofon: keluaran {name} siap"));
                    output = Some(opened);
                    last_failure.clear();
                }
                // Galat yang sama tidak dicatat berulang-ulang tiap 3 detik.
                Err(why) if why != last_failure => {
                    host.log(|| format!("mikrofon: {why}"));
                    last_failure = why;
                }
                Err(_) => {}
            }
        }
        let message = match packets.recv_timeout(Duration::from_millis(500)) {
            Ok(message) => message,
            Err(RecvTimeoutError::Timeout) => {
                if let Some(text) = output.as_mut().and_then(|o| o.report()) {
                    host.log(|| format!("mikrofon: {text}"));
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => return,
        };
        let result = match message {
            None => {
                if talking {
                    let stats = receiver.stats();
                    host.log(|| format!("mikrofon: sesi selesai; {} paket ({} dtk suara), {} dipulihkan FEC, {} ditebak, {} terlambat", stats.packets, frames * u64::from(FRAME_MS) / 1000, stats.recovered, stats.concealed, stats.late));
                }
                talking = false;
                frames = 0;
                // Penerima baru: hitungan dan keadaan decoder mulai dari nol untuk sesi berikutnya.
                match MicReceiver::new() {
                    Ok(fresh) => receiver = fresh,
                    Err(_) => receiver.restart(),
                }
                output.as_mut().map_or(Ok(()), |o| o.flush())
            }
            Some((sequence, opus)) => {
                if !talking {
                    talking = true;
                    host.log(|| "mikrofon: suara pemain mulai diterima".into());
                }
                let pcm = receiver.push(sequence, &opus, Instant::now());
                frames += (pcm.len() / FRAME_SAMPLES) as u64;
                match output.as_mut() {
                    Some(output) if !pcm.is_empty() => output.write(&pcm),
                    _ => Ok(()),
                }
            }
        };
        if let Err(why) = result {
            host.log(|| format!("mikrofon: keluaran bermasalah, dibuka ulang: {why}"));
            output = None;
            retry_at = Instant::now() + Duration::from_secs(1);
        }
        if let Some(text) = output.as_mut().and_then(|o| o.report()) {
            host.log(|| format!("mikrofon: {text}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cloudpc_stream_common::voice::{test_tone, tone_purity, VoiceEncoder};

    fn tone_packets(count: u64) -> Vec<Vec<u8>> {
        let mut encoder = VoiceEncoder::new().unwrap();
        (0..count).map(|frame| encoder.encode(&test_tone(440.0, frame)).unwrap()).collect()
    }

    /// Jalankan paket lewat penerima dengan jarak 20 ms antar paket; `keep` memilih paket yang "tiba".
    fn receive(packets: &[Vec<u8>], keep: impl Fn(usize) -> bool) -> (Vec<i16>, ReceiverStats) {
        let mut receiver = MicReceiver::new().unwrap();
        let start = Instant::now();
        let mut pcm = Vec::new();
        for (index, packet) in packets.iter().enumerate() {
            if keep(index) {
                pcm.extend(receiver.push(index as u16, packet, start + Duration::from_millis(20 * index as u64)));
            }
        }
        (pcm, receiver.stats())
    }

    #[test]
    fn target_parsing() {
        assert_eq!(MicTarget::parse("pico"), Ok(MicTarget::Pico(None)));
        assert_eq!(MicTarget::parse("pico:COM7"), Ok(MicTarget::Pico(Some("COM7".into()))));
        assert_eq!(MicTarget::parse("wav:C:\\x\\mic.wav"), Ok(MicTarget::Wav(PathBuf::from("C:\\x\\mic.wav"))));
        assert!(MicTarget::parse("pico:").is_err());
        assert!(MicTarget::parse("speaker").is_err());
    }

    #[test]
    fn clean_stream_decodes_every_frame() {
        let packets = tone_packets(50);
        let (pcm, stats) = receive(&packets, |_| true);
        assert_eq!(pcm.len(), 50 * FRAME_SAMPLES);
        assert_eq!(stats, ReceiverStats { packets: 50, ..Default::default() });
        assert!(tone_purity(&pcm[FRAME_SAMPLES * 5..], 440.0) > 0.9);
    }

    #[test]
    fn lost_packets_keep_the_timeline_and_the_tone() {
        let packets = tone_packets(100);
        // Tiap paket ke-10 hilang, dan sekali dua paket berturut-turut.
        let (pcm, stats) = receive(&packets, |i| i % 10 != 5 && i != 46);
        assert_eq!(pcm.len(), 100 * FRAME_SAMPLES, "frame yang hilang tetap diisi");
        assert_eq!((stats.recovered, stats.concealed, stats.late), (10, 1, 0));
        assert!(tone_purity(&pcm[FRAME_SAMPLES * 5..], 440.0) > 0.8, "kemurnian {}", tone_purity(&pcm[FRAME_SAMPLES * 5..], 440.0));
    }

    #[test]
    fn late_and_duplicate_packets_are_dropped() {
        let packets = tone_packets(10);
        let mut receiver = MicReceiver::new().unwrap();
        let now = Instant::now();
        assert_eq!(receiver.push(0, &packets[0], now).len(), FRAME_SAMPLES);
        assert_eq!(receiver.push(2, &packets[2], now).len(), FRAME_SAMPLES * 2, "frame 1 ditambal");
        assert!(receiver.push(1, &packets[1], now).is_empty(), "frame 1 datang terlambat");
        assert!(receiver.push(2, &packets[2], now).is_empty(), "duplikat");
        assert_eq!(receiver.stats().late, 2);
    }

    #[test]
    fn long_gaps_are_not_patched_and_sequence_wraps() {
        let packets = tone_packets(4);
        let mut receiver = MicReceiver::new().unwrap();
        let now = Instant::now();
        assert_eq!(receiver.push(0xFFFE, &packets[0], now).len(), FRAME_SAMPLES);
        assert_eq!(receiver.push(0xFFFF, &packets[1], now).len(), FRAME_SAMPLES);
        assert_eq!(receiver.push(0x0000, &packets[2], now).len(), FRAME_SAMPLES, "nomor urut berputar");
        // Lompatan nomor besar (jaringan sempat putus): tidak diisi ratusan frame tebakan.
        assert_eq!(receiver.push(0x0100, &packets[3], now).len(), FRAME_SAMPLES);
        // Jeda waktu panjang (dibisukan): nomor yang tampak "terlambat" pun diterima sebagai pembicaraan baru.
        assert_eq!(receiver.push(0x0005, &packets[3], now + Duration::from_secs(2)).len(), FRAME_SAMPLES);
        assert_eq!(receiver.stats().concealed, 0);
    }

    #[test]
    fn serial_frame_layout() {
        // Vektor yang sama dengan tes firmware (`mic-pico/src/link.rs`).
        assert_eq!(link::pcm_frame(&[1, -2, 0x1234]), [0xA5, 0x01, 6, 0, 1, 0, 0xFE, 0xFF, 0x34, 0x12]);
        assert_eq!(link::flush_frame(), [0xA5, 0x02, 0, 0]);
        assert_eq!(link::config_frame(2880, 1680, 4080), [0xA5, 0x03, 6, 0, 0x40, 0x0B, 0x90, 0x06, 0xF0, 0x0F]);
        assert_eq!(link::bootloader_frame(), [0xA5, 0x7F, 4, 0, b'B', b'O', b'O', b'T']);
    }

    #[test]
    fn device_status_is_parsed_from_a_noisy_byte_stream() {
        let mut bytes = vec![0x00, 0x11, 0x5A, 1, 0x40, 0x0B, 2, 0, 3, 0, 0x5A, 1, 0x80];
        let status = link::take_status(&mut bytes).unwrap();
        assert_eq!(status, link::Status { version: 1, buffered: 2880, underruns: 2, dropped: 3 });
        assert_eq!(bytes, [0x5A, 1, 0x80], "laporan terpotong menunggu sisanya");
        bytes.extend_from_slice(&[0x07, 0, 0, 0, 0]);
        assert_eq!(link::take_status(&mut bytes).unwrap().buffered, 0x0780);
        assert!(bytes.is_empty());
        assert_eq!(link::take_status(&mut vec![1, 2, 3]), None);
    }

    #[test]
    fn wav_output_is_a_valid_file_after_every_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mic.wav");
        let mut wav = WavOutput::create(&path).unwrap();
        wav.write(&[1, 2, 3]).unwrap();
        wav.write(&[-1]).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes.len(), 44 + 8);
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 36 + 8);
        assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), 48_000);
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 8);
        assert_eq!(&bytes[44..], &[1, 0, 2, 0, 3, 0, 0xFF, 0xFF]);
    }
}
