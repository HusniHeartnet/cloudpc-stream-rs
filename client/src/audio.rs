//! Fase C4. Audio: RTP -> (dekripsi AES-CBC) -> pemulihan FEC 4+2 -> Opus -> WASAPI.
//!
//! Rujukan: `moonlight-common-c/src/AudioStream.c`, `RtpAudioQueue.c`, dan `moonlight-qt/app/streaming/audio`.
//! Format kawat sama dengan host CloudPC (`host/src/audio.rs`) dan Sunshine: paket data tipe 97 bernomor urut, blok
//! empat paket mulai dari nomor kelipatan 4, lalu dua paket paritas tipe 127 (matriks tetap NVIDIA) atas payload
//! TERENKRIPSI. Paket yang hilang dipulihkan dari paritas; kalau tidak bisa, Opus menebak isinya (PLC) supaya suara
//! tidak tersendat-sendat.

use std::collections::BTreeMap;

use cloudpc_stream_common::crypto::cbc_decrypt;
use cloudpc_stream_proto::audio::{audio_iv, AudioFecHeader, DATA_SHARDS, FEC_HEADER_SIZE, FEC_PARITY_MATRIX, FEC_SHARDS, PACKET_TYPE_AUDIO, PACKET_TYPE_FEC};
use cloudpc_stream_proto::fec;
use cloudpc_stream_proto::rtp::{RtpHeader, RTP_HEADER_SIZE};

pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: usize = 2;

/// Satu frame audio dalam urutan putar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioOut {
    /// Paket Opus (sudah didekripsi).
    Opus(Vec<u8>),
    /// Frame ini hilang dan tidak bisa dipulihkan: decoder menebak isinya.
    Lost,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AudioStats {
    pub packets: u64,
    pub invalid: u64,
    /// Frame yang dipulihkan dari paritas.
    pub recovered: u64,
    /// Frame yang hilang dan diganti tebakan Opus.
    pub lost: u64,
}

/// Payload terenkripsi satu paket data; `recovered` = dibuat ulang dari paritas.
struct Shard {
    payload: Vec<u8>,
    recovered: bool,
}

#[derive(Default)]
struct Block {
    data: [Option<Shard>; DATA_SHARDS],
    parity: [Option<Vec<u8>>; FEC_SHARDS],
}

/// Menyusun paket audio jadi frame berurutan (`RtpAudioQueue.c`, disederhanakan): frame berikutnya keluar begitu ada;
/// frame yang hilang ditunggu sampai paritas bloknya tiba, atau dinyatakan hilang begitu paket blok sesudahnya datang.
pub struct AudioDepacketizer {
    key: Option<([u8; 16], u32)>,
    next: Option<u16>,
    blocks: BTreeMap<u16, Block>,
    stats: AudioStats,
}

fn block_of(sequence: u16) -> u16 {
    sequence & !(DATA_SHARDS as u16 - 1)
}

/// `a` lebih awal dari `b` (putaran 16 bit).
fn before16(a: u16, b: u16) -> bool {
    (a.wrapping_sub(b) as i16) < 0
}

impl AudioDepacketizer {
    /// `key` = (`rikey`, `rikeyid`) bila audio dienkripsi (bit `encryption::AUDIO`).
    pub fn new(key: Option<([u8; 16], u32)>) -> AudioDepacketizer {
        AudioDepacketizer { key, next: None, blocks: BTreeMap::new(), stats: AudioStats::default() }
    }

    pub fn stats(&self) -> AudioStats {
        self.stats
    }

    pub fn push(&mut self, packet: &[u8]) -> Vec<AudioOut> {
        self.stats.packets += 1;
        let Ok(rtp) = RtpHeader::read(packet) else {
            self.stats.invalid += 1;
            return Vec::new();
        };
        let highest = match rtp.packet_type & 0x7F {
            PACKET_TYPE_AUDIO => {
                let sequence = rtp.sequence_number;
                if self.next.is_some_and(|next| before16(sequence, next)) {
                    return Vec::new(); // terlambat: sudah diputar atau sudah dinyatakan hilang
                }
                let next = *self.next.get_or_insert(sequence);
                let base = block_of(sequence);
                if before16(base, block_of(next)) {
                    return Vec::new();
                }
                self.blocks.entry(base).or_default().data[usize::from(sequence - base)] = Some(Shard { payload: packet[RTP_HEADER_SIZE..].to_vec(), recovered: false });
                sequence
            }
            PACKET_TYPE_FEC => {
                let Ok(header) = AudioFecHeader::read(&packet[RTP_HEADER_SIZE.min(packet.len())..]) else {
                    self.stats.invalid += 1;
                    return Vec::new();
                };
                let base = header.base_sequence_number;
                let index = usize::from(header.fec_shard_index);
                if index >= FEC_SHARDS || base != block_of(base) || self.next.is_none_or(|next| before16(base, block_of(next))) {
                    return Vec::new();
                }
                self.blocks.entry(base).or_default().parity[index] = Some(packet[RTP_HEADER_SIZE + FEC_HEADER_SIZE..].to_vec());
                // Paritas datang setelah data terakhir bloknya.
                base.wrapping_add(DATA_SHARDS as u16 - 1)
            }
            _ => {
                self.stats.invalid += 1;
                return Vec::new();
            }
        };
        self.drain(highest)
    }

    /// Keluarkan frame yang sudah bisa diputar. `highest` = nomor urut tertinggi yang kini diketahui pernah dikirim.
    fn drain(&mut self, highest: u16) -> Vec<AudioOut> {
        let mut out = Vec::new();
        while let Some(next) = self.next {
            let base = block_of(next);
            let slot = usize::from(next - base);
            let block = self.blocks.entry(base).or_default();
            if block.data[slot].is_none() {
                Self::recover(block);
            }
            // Paket yang sudah diputar tetap disimpan sampai bloknya selesai: pemulihan paket lain di blok yang sama butuh.
            let frame = match block.data[slot].as_ref().map(|shard| (shard.recovered, shard.payload.clone())) {
                Some((recovered, payload)) => {
                    if recovered {
                        self.stats.recovered += 1;
                    }
                    match self.open(next, &payload) {
                        Some(opus) => AudioOut::Opus(opus),
                        None => {
                            self.stats.invalid += 1;
                            AudioOut::Lost
                        }
                    }
                }
                // Masih bisa datang (atau dipulihkan) selama blok sesudahnya belum terlihat.
                None if !before16(base.wrapping_add(DATA_SHARDS as u16), highest) => break,
                None => {
                    self.stats.lost += 1;
                    AudioOut::Lost
                }
            };
            out.push(frame);
            self.next = Some(next.wrapping_add(1));
            if slot == DATA_SHARDS - 1 {
                self.blocks.remove(&base);
            }
        }
        // Buang blok lama yang tertinggal (paritas sisa).
        if let Some(next) = self.next {
            let keep = block_of(next);
            self.blocks.retain(|&b, _| !before16(b, keep));
        }
        out
    }

    fn recover(block: &mut Block) {
        let have = block.data.iter().filter(|s| s.is_some()).count() + block.parity.iter().filter(|s| s.is_some()).count();
        if have < DATA_SHARDS || block.parity.iter().all(Option::is_none) {
            return;
        }
        let mut shards: Vec<Option<Vec<u8>>> = block.data.iter().map(|d| d.as_ref().map(|s| s.payload.clone())).chain(block.parity.iter().cloned()).collect();
        if fec::reconstruct_with(&mut shards, DATA_SHARDS, |row, column| FEC_PARITY_MATRIX[row][column]) {
            for (slot, shard) in shards.into_iter().take(DATA_SHARDS).enumerate() {
                if block.data[slot].is_none() {
                    block.data[slot] = Some(Shard { payload: shard.expect("dipulihkan"), recovered: true });
                }
            }
        }
    }

    fn open(&self, sequence: u16, payload: &[u8]) -> Option<Vec<u8>> {
        match &self.key {
            Some((key, key_id)) => cbc_decrypt(key, &audio_iv(*key_id, sequence), payload),
            None => Some(payload.to_vec()),
        }
    }
}

/// Decoder Opus stereo 48 kHz. Aliran stereo Sunshine/host CloudPC = satu aliran Opus biasa.
pub struct OpusDecoder {
    raw: *mut unsafe_libopus::OpusDecoder,
    /// Ukuran frame terakhir (sampel per kanal), dipakai untuk menebak frame yang hilang.
    frame_size: usize,
}

// Hanya dipakai dari satu thread pada satu waktu.
unsafe impl Send for OpusDecoder {}

impl OpusDecoder {
    pub fn new() -> Result<OpusDecoder, String> {
        let mut error = 0;
        let raw = unsafe { unsafe_libopus::opus_decoder_create(SAMPLE_RATE as i32, CHANNELS as i32, &mut error) };
        if raw.is_null() || error != 0 {
            return Err(format!("decoder Opus gagal dibuat ({error})"));
        }
        Ok(OpusDecoder { raw, frame_size: SAMPLE_RATE as usize / 200 })
    }

    /// Sampel berselang kiri/kanan untuk satu frame (`None` = frame hilang, ditebak).
    pub fn decode(&mut self, frame: &AudioOut) -> Vec<f32> {
        let mut pcm = vec![0f32; 5760 * CHANNELS]; // frame Opus terpanjang: 120 ms
        let frames = unsafe {
            match frame {
                AudioOut::Opus(data) => unsafe_libopus::opus_decode_float(self.raw, data.as_ptr(), data.len() as i32, pcm.as_mut_ptr(), 5760, 0),
                AudioOut::Lost => unsafe_libopus::opus_decode_float(self.raw, std::ptr::null(), 0, pcm.as_mut_ptr(), self.frame_size as i32, 0),
            }
        };
        if frames <= 0 {
            pcm.truncate(self.frame_size * CHANNELS);
            pcm.fill(0.0);
            return pcm;
        }
        if matches!(frame, AudioOut::Opus(_)) {
            self.frame_size = frames as usize;
        }
        pcm.truncate(frames as usize * CHANNELS);
        pcm
    }
}

impl Drop for OpusDecoder {
    fn drop(&mut self) {
        unsafe { unsafe_libopus::opus_decoder_destroy(self.raw) };
    }
}

/// Pemutar WASAPI (mode bersama, berbasis event) di thread sendiri, dengan antrean kecil di depannya.
#[cfg(windows)]
pub mod player {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::thread::JoinHandle;

    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::Media::Audio::{
        eConsole, eRender, IAudioClient, IAudioRenderClient, IMMDeviceEnumerator, MMDeviceEnumerator, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
        AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, WAVEFORMATEX,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
    use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

    use super::{CHANNELS, SAMPLE_RATE};

    const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
    /// Sampel per kanal per milidetik.
    const PER_MS: usize = SAMPLE_RATE as usize / 1000;
    /// Isi antrean sebelum mulai (atau mulai lagi setelah kosong) diputar.
    const PREBUFFER_MS: usize = 10;
    /// Antrean lebih dari ini = latensi menumpuk (mis. jaringan sempat macet lalu semua paket tiba sekaligus):
    /// sisakan `PREBUFFER_MS` terbaru, sisanya dibuang.
    const MAX_QUEUE_MS: usize = 60;
    /// Penyangga perangkat (satuan 100 ns): 20 ms.
    const DEVICE_BUFFER: i64 = 200_000;

    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
    pub struct PlayerStats {
        /// Saat suara habis sebelum frame berikutnya tiba (terdengar sebagai jeda).
        pub underruns: u64,
        /// Milidetik suara yang dibuang karena antrean terlalu panjang.
        pub trimmed_ms: u64,
        /// Pindah ke perangkat putar lain (perangkat bawaan berganti atau perangkat lama hilang).
        pub device_changes: u64,
    }

    #[derive(Default)]
    struct Queue {
        samples: VecDeque<f32>,
        playing: bool,
        stats: PlayerStats,
    }

    pub struct Player {
        queue: Arc<Mutex<Queue>>,
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    fn describe(what: &str, error: windows::core::Error) -> String {
        format!("{what}: {error}")
    }

    impl Player {
        /// Buka perangkat putar bawaan Windows. Gagal = tidak ada perangkat suara (sesi tetap jalan tanpa suara).
        pub fn open() -> Result<Player, String> {
            let queue = Arc::new(Mutex::new(Queue::default()));
            let stop = Arc::new(AtomicBool::new(false));
            let (ready_tx, ready_rx) = mpsc::channel();
            let thread = {
                let (queue, stop) = (queue.clone(), stop.clone());
                std::thread::Builder::new()
                    .name("audio".into())
                    .spawn(move || {
                        if let Err(error) = render(&queue, &stop, &ready_tx) {
                            let _ = ready_tx.send(Err(error));
                        }
                    })
                    .map_err(|e| format!("thread audio: {e}"))?
            };
            ready_rx.recv().map_err(|_| "thread audio berhenti".to_string())??;
            Ok(Player { queue, stop, thread: Some(thread) })
        }

        /// Tambahkan sampel berselang kiri/kanan ke antrean putar.
        pub fn push(&self, pcm: &[f32]) {
            let Ok(mut queue) = self.queue.lock() else { return };
            queue.samples.extend(pcm);
            let max = MAX_QUEUE_MS * PER_MS * CHANNELS;
            if queue.samples.len() > max {
                let keep = PREBUFFER_MS * PER_MS * CHANNELS;
                let drop = queue.samples.len() - keep;
                queue.samples.drain(..drop);
                queue.stats.trimmed_ms += (drop / CHANNELS / PER_MS) as u64;
            }
        }

        pub fn stats(&self) -> PlayerStats {
            self.queue.lock().map(|q| q.stats).unwrap_or_default()
        }
    }

    impl crate::session::AudioSink for Player {
        fn samples(&mut self, pcm: &[f32]) {
            self.push(pcm);
        }
    }

    impl Drop for Player {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    /// Perangkat putar yang sedang dipakai.
    struct Device {
        id: String,
        client: IAudioClient,
        output: IAudioRenderClient,
        event: windows::Win32::Foundation::HANDLE,
        size: u32,
    }

    impl Drop for Device {
        fn drop(&mut self) {
            unsafe {
                let _ = self.client.Stop();
                let _ = CloseHandle(self.event);
            }
        }
    }

    fn default_id(enumerator: &IMMDeviceEnumerator) -> Option<String> {
        unsafe {
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
            let id = device.GetId().ok()?;
            let text = id.to_string().ok();
            windows::Win32::System::Com::CoTaskMemFree(Some(id.0 as *const _));
            text
        }
    }

    /// Buka perangkat putar bawaan Windows saat ini (float stereo 48 kHz; Windows mengonversi ke format perangkat).
    fn open_default(enumerator: &IMMDeviceEnumerator) -> Result<Device, String> {
        unsafe {
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).map_err(|e| describe("tidak ada perangkat putar suara", e))?;
            let id = device.GetId().ok().and_then(|id| {
                let text = id.to_string().ok();
                windows::Win32::System::Com::CoTaskMemFree(Some(id.0 as *const _));
                text
            });
            let client: IAudioClient = device.Activate(CLSCTX_ALL, None).map_err(|e| describe("IAudioClient", e))?;
            let block = (CHANNELS * 4) as u16;
            let format = WAVEFORMATEX { wFormatTag: WAVE_FORMAT_IEEE_FLOAT, nChannels: CHANNELS as u16, nSamplesPerSec: SAMPLE_RATE, nAvgBytesPerSec: SAMPLE_RATE * u32::from(block), nBlockAlign: block, wBitsPerSample: 32, cbSize: 0 };
            let flags = AUDCLNT_STREAMFLAGS_EVENTCALLBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
            client.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, DEVICE_BUFFER, 0, &format, None).map_err(|e| describe("IAudioClient::Initialize", e))?;
            let event = CreateEventW(None, false, false, None).map_err(|e| describe("CreateEvent", e))?;
            // Dibungkus dulu supaya event ditutup juga bila langkah berikutnya gagal.
            let mut device = Device { id: id.unwrap_or_default(), client: client.clone(), output: client.GetService().map_err(|e| describe("IAudioRenderClient", e))?, event, size: 0 };
            client.SetEventHandle(event).map_err(|e| describe("SetEventHandle", e))?;
            device.size = client.GetBufferSize().map_err(|e| describe("GetBufferSize", e))?;
            client.Start().map_err(|e| describe("IAudioClient::Start", e))?;
            Ok(device)
        }
    }

    /// Isi penyangga perangkat dari antrean. `false` = perangkat tidak bisa dipakai lagi (dicabut, dinonaktifkan).
    fn feed(device: &Device, queue: &Mutex<Queue>) -> bool {
        // SAFETY: penyangga dari GetBuffer hanya dipakai sampai ReleaseBuffer.
        unsafe {
            if WaitForSingleObject(device.event, 100) != WAIT_OBJECT_0 {
                return true;
            }
            let Ok(padding) = device.client.GetCurrentPadding() else { return false };
            let frames = device.size.saturating_sub(padding);
            if frames == 0 {
                return true;
            }
            let Ok(data) = device.output.GetBuffer(frames) else { return false };
            let out = std::slice::from_raw_parts_mut(data as *mut f32, frames as usize * CHANNELS);
            let mut filled = 0;
            if let Ok(mut queue) = queue.lock() {
                if !queue.playing && queue.samples.len() >= PREBUFFER_MS * PER_MS * CHANNELS {
                    queue.playing = true;
                }
                if queue.playing {
                    let take = out.len().min(queue.samples.len());
                    for (slot, sample) in out.iter_mut().zip(queue.samples.drain(..take)) {
                        *slot = sample;
                    }
                    filled = take;
                    if filled < out.len() {
                        queue.playing = false;
                        queue.stats.underruns += 1;
                    }
                }
            }
            out[filled..].fill(0.0);
            device.output.ReleaseBuffer(frames, 0).is_ok()
        }
    }

    /// Putar terus sampai `stop`. Seperti Moonlight, suara ikut pindah bila perangkat bawaan Windows berganti (headset
    /// dicolok) dan dibuka lagi bila perangkatnya hilang; antrean dikosongkan supaya tidak terlambat.
    fn render(queue: &Mutex<Queue>, stop: &AtomicBool, ready: &mpsc::Sender<Result<(), String>>) -> Result<(), String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let enumerator: IMMDeviceEnumerator = unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(|e| describe("MMDeviceEnumerator", e))?;
        let mut device = Some(open_default(&enumerator)?);
        let _ = ready.send(Ok(()));
        let mut last_check = std::time::Instant::now();
        while !stop.load(Ordering::SeqCst) {
            let healthy = match &device {
                Some(current) => feed(current, queue),
                None => {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    false
                }
            };
            let check = last_check.elapsed() >= std::time::Duration::from_secs(1);
            let moved = check && device.as_ref().is_some_and(|d| default_id(&enumerator).is_some_and(|id| id != d.id));
            if check {
                last_check = std::time::Instant::now();
            }
            if !healthy || moved {
                if !healthy && device.is_none() && !check {
                    continue; // coba buka lagi paling cepat tiap detik
                }
                device = None;
                if let Ok(next) = open_default(&enumerator) {
                    device = Some(next);
                    if let Ok(mut queue) = queue.lock() {
                        queue.samples.clear();
                        queue.playing = false;
                        queue.stats.device_changes += 1;
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cloudpc_stream_common::crypto::cbc_encrypt;
    use cloudpc_stream_proto::audio::fec_parity;
    use cloudpc_stream_proto::rtp::RTP_VERSION_2;

    const KEY: [u8; 16] = *b"kunci-audio-uji!";
    const KEY_ID: u32 = 4242;

    /// Paket seperti host: data terenkripsi, lalu dua paritas tiap empat paket (payload 60 byte -> 64 terenkripsi).
    fn stream(count: u16) -> Vec<(u16, Vec<u8>)> {
        let mut out = Vec::new();
        let mut block = Vec::new();
        for sequence in 0..count {
            let opus = vec![sequence as u8; 60];
            let payload = cbc_encrypt(&KEY, &audio_iv(KEY_ID, sequence), &opus);
            let mut packet = vec![0u8; RTP_HEADER_SIZE];
            RtpHeader { header: RTP_VERSION_2, packet_type: PACKET_TYPE_AUDIO, sequence_number: sequence, timestamp: u32::from(sequence) * 5, ssrc: 0 }.write(&mut packet).unwrap();
            packet.extend_from_slice(&payload);
            out.push((sequence, packet));
            block.push(payload);
            if block.len() == DATA_SHARDS {
                let parity = fec_parity([&block[0], &block[1], &block[2], &block[3]]).unwrap();
                for (index, shard) in parity.iter().enumerate() {
                    let mut packet = vec![0u8; RTP_HEADER_SIZE + FEC_HEADER_SIZE];
                    RtpHeader { header: RTP_VERSION_2, packet_type: PACKET_TYPE_FEC, sequence_number: sequence + 1 + index as u16, timestamp: 0, ssrc: 0 }.write(&mut packet).unwrap();
                    AudioFecHeader { fec_shard_index: index as u8, payload_type: PACKET_TYPE_AUDIO, base_sequence_number: sequence - 3, base_timestamp: 0, ssrc: 0 }
                        .write(&mut packet[RTP_HEADER_SIZE..])
                        .unwrap();
                    packet.extend_from_slice(shard);
                    out.push((u16::MAX, packet));
                }
                block.clear();
            }
        }
        out
    }

    fn opus_bytes(frames: &[AudioOut]) -> Vec<Option<u8>> {
        frames.iter().map(|f| if let AudioOut::Opus(d) = f { Some(d[0]) } else { None }).collect()
    }

    #[test]
    fn frames_come_out_in_order_and_decrypted() {
        let mut depacketizer = AudioDepacketizer::new(Some((KEY, KEY_ID)));
        let frames: Vec<AudioOut> = stream(12).iter().flat_map(|(_, p)| depacketizer.push(p)).collect();
        assert_eq!(opus_bytes(&frames), (0..12).map(Some).collect::<Vec<_>>());
        assert_eq!(depacketizer.stats().recovered, 0);
    }

    #[test]
    fn up_to_two_lost_packets_per_block_are_recovered() {
        let mut depacketizer = AudioDepacketizer::new(Some((KEY, KEY_ID)));
        // Blok 1 (4..8) kehilangan 5 dan 6; blok 2 kehilangan 8.
        let frames: Vec<AudioOut> = stream(12).iter().filter(|(s, _)| ![5, 6, 8].contains(s)).flat_map(|(_, p)| depacketizer.push(p)).collect();
        assert_eq!(opus_bytes(&frames), (0..12).map(Some).collect::<Vec<_>>());
        assert_eq!(depacketizer.stats().recovered, 3);
    }

    #[test]
    fn unrecoverable_frames_become_lost_without_stalling() {
        let mut depacketizer = AudioDepacketizer::new(Some((KEY, KEY_ID)));
        // Blok 1 kehilangan tiga paket data: paritas tidak cukup.
        let frames: Vec<AudioOut> = stream(12).iter().filter(|(s, _)| ![4, 5, 6].contains(s)).flat_map(|(_, p)| depacketizer.push(p)).collect();
        let expected: Vec<Option<u8>> = (0..12).map(|s| if (4..7).contains(&s) { None } else { Some(s) }).collect();
        assert_eq!(opus_bytes(&frames), expected);
        assert_eq!(depacketizer.stats().lost, 3);
    }

    #[test]
    fn opus_round_trip_and_concealment() {
        let mut encoder_error = 0;
        let encoder = unsafe { unsafe_libopus::opus_encoder_create(48_000, 2, unsafe_libopus::OPUS_APPLICATION_RESTRICTED_LOWDELAY, &mut encoder_error) };
        let tone: Vec<f32> = (0..240 * 2).map(|i| ((i / 2) as f32 * 0.05).sin() * 0.3).collect();
        let mut packet = vec![0u8; 1500];
        let len = unsafe { unsafe_libopus::opus_encode_float(encoder, tone.as_ptr(), 240, packet.as_mut_ptr(), packet.len() as i32) };
        unsafe { unsafe_libopus::opus_encoder_destroy(encoder) };
        packet.truncate(len as usize);
        let mut decoder = OpusDecoder::new().unwrap();
        assert_eq!(decoder.decode(&AudioOut::Opus(packet)).len(), 240 * 2, "5 ms stereo");
        assert_eq!(decoder.decode(&AudioOut::Lost).len(), 240 * 2, "tebakan sepanjang frame terakhir");
    }
}
