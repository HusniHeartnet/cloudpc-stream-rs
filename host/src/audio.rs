//! Fase H3. Audio: suara yang sedang diputar Windows (WASAPI loopback) -> Opus -> RTP + FEC 4+2 -> UDP (dasar + 11).
//!
//! Rujukan: `Sunshine/src/audio.cpp`, `platform/windows/audio.cpp`, `audioBroadcastThread` di `stream.cpp`.
//! Tiap paket = satu frame Opus stereo 48 kHz (5 atau 10 ms, pilihan client), dienkripsi AES-128-CBC dengan kunci
//! sesi. Tiap 4 paket diikuti 2 paket paritas, sehingga 2 dari 6 boleh hilang.
//! Aliran dibuat menerus: saat tidak ada suara (loopback tidak memberi data), host mengirim frame hening.
//! Belum: surround 5.1/7.1, perangkat suara virtual (suara tetap terdengar juga di speaker host), kualitas tinggi.

use std::net::{SocketAddr, UdpSocket};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use cloudpc_stream_proto::audio::{audio_iv, fec_parity, AudioFecHeader, DATA_SHARDS, FEC_HEADER_SIZE, PACKET_TYPE_AUDIO, PACKET_TYPE_FEC};
use cloudpc_stream_proto::rtp::{RtpHeader, RTP_HEADER_SIZE, RTP_VERSION_2};

use crate::crypto::cbc_encrypt;
use crate::nvhttp::Host;

pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: usize = 2;
/// Bitrate Opus stereo kualitas normal di Sunshine.
const BITRATE: i32 = 96_000;
/// Client harus mengirim ping audio dalam waktu ini; kalau tidak, sesi jalan tanpa suara.
const PING_TIMEOUT: Duration = Duration::from_secs(10);
/// Suara tertangkap yang menumpuk lebih dari ini dibuang supaya tidak menjadi keterlambatan.
const MAX_BACKLOG_MS: usize = 100;

/// Frame Opus -> datagram RTP terenkripsi, dengan dua paket paritas tiap empat paket.
pub struct AudioPacketizer {
    key: [u8; 16],
    key_id: u32,
    duration_ms: u32,
    sequence: u16,
    timestamp: u32,
    /// Payload terenkripsi blok FEC yang sedang berjalan, beserta nomor urut dan cap waktu paket pertamanya.
    block: Vec<Vec<u8>>,
    block_base: (u16, u32),
}

impl AudioPacketizer {
    pub fn new(key: [u8; 16], key_id: u32, duration_ms: u32) -> AudioPacketizer {
        AudioPacketizer { key, key_id, duration_ms, sequence: 0, timestamp: 0, block: Vec::with_capacity(DATA_SHARDS), block_base: (0, 0) }
    }

    /// Satu frame Opus -> satu datagram data, ditambah dua datagram paritas setelah paket keempat tiap blok.
    pub fn push(&mut self, opus: &[u8]) -> Vec<Vec<u8>> {
        let (sequence, timestamp) = (self.sequence, self.timestamp);
        let payload = cbc_encrypt(&self.key, &audio_iv(self.key_id, sequence), opus);

        let mut data = vec![0u8; RTP_HEADER_SIZE];
        RtpHeader { header: RTP_VERSION_2, packet_type: PACKET_TYPE_AUDIO, sequence_number: sequence, timestamp, ssrc: 0 }.write(&mut data).expect("header RTP 12 byte");
        data.extend_from_slice(&payload);
        let mut out = vec![data];

        if usize::from(sequence) % DATA_SHARDS == 0 {
            self.block.clear();
            self.block_base = (sequence, timestamp);
        }
        self.block.push(payload);
        if self.block.len() == DATA_SHARDS {
            // Paritas butuh empat payload sama panjang; Opus CBR menjaminnya. Kalau tidak, blok ini tanpa paritas.
            if let Some(parity) = fec_parity([&self.block[0], &self.block[1], &self.block[2], &self.block[3]]) {
                for (index, shard) in parity.iter().enumerate() {
                    let mut packet = vec![0u8; RTP_HEADER_SIZE + FEC_HEADER_SIZE];
                    // Nomor urut paket paritas melanjutkan paket data terakhir (dan memang bertumpuk dengan blok berikutnya).
                    RtpHeader { header: RTP_VERSION_2, packet_type: PACKET_TYPE_FEC, sequence_number: sequence.wrapping_add(index as u16 + 1), timestamp: 0, ssrc: 0 }
                        .write(&mut packet)
                        .expect("header RTP 12 byte");
                    AudioFecHeader { fec_shard_index: index as u8, payload_type: PACKET_TYPE_AUDIO, base_sequence_number: self.block_base.0, base_timestamp: self.block_base.1, ssrc: 0 }
                        .write(&mut packet[RTP_HEADER_SIZE..])
                        .expect("header FEC 12 byte");
                    packet.extend_from_slice(shard);
                    out.push(packet);
                }
            }
            self.block.clear();
        }
        self.sequence = sequence.wrapping_add(1);
        self.timestamp = timestamp.wrapping_add(self.duration_ms);
        out
    }
}

/// Encoder Opus stereo 48 kHz, latensi rendah, bitrate tetap (ukuran paket konstan, syarat FEC).
pub struct OpusEncoder(*mut unsafe_libopus::OpusEncoder);

impl OpusEncoder {
    pub fn new() -> Result<OpusEncoder, String> {
        let mut error = 0;
        // SAFETY: argumen sah menurut API libopus; penunjuk hasil diperiksa.
        let raw = unsafe { unsafe_libopus::opus_encoder_create(SAMPLE_RATE as i32, CHANNELS as i32, unsafe_libopus::OPUS_APPLICATION_RESTRICTED_LOWDELAY, &mut error) };
        if raw.is_null() || error != 0 {
            return Err(format!("encoder Opus gagal dibuat (kode {error})"));
        }
        let encoder = OpusEncoder(raw);
        // SAFETY: `raw` encoder yang sah; kedua permintaan menerima satu argumen i32.
        unsafe {
            unsafe_libopus::opus_encoder_ctl!(raw, unsafe_libopus::OPUS_SET_BITRATE_REQUEST, BITRATE);
            unsafe_libopus::opus_encoder_ctl!(raw, unsafe_libopus::OPUS_SET_VBR_REQUEST, 0);
        }
        Ok(encoder)
    }

    /// `pcm`: sampel float berselang (kiri, kanan) untuk tepat satu frame 5/10/20 ms. Mengembalikan jumlah byte.
    pub fn encode(&mut self, pcm: &[f32], out: &mut [u8]) -> Result<usize, String> {
        let frames = pcm.len() / CHANNELS;
        // SAFETY: `pcm` berisi `frames * CHANNELS` sampel dan `out` sepanjang yang dinyatakan.
        let written = unsafe { unsafe_libopus::opus_encode_float(self.0, pcm.as_ptr(), frames as i32, out.as_mut_ptr(), out.len() as i32) };
        usize::try_from(written).map_err(|_| format!("Opus gagal meng-encode (kode {written})"))
    }
}

impl Drop for OpusEncoder {
    fn drop(&mut self) {
        // SAFETY: dibuat oleh opus_encoder_create dan dihancurkan tepat sekali.
        unsafe { unsafe_libopus::opus_encoder_destroy(self.0) };
    }
}

#[cfg(windows)]
mod loopback {
    use std::ffi::c_void;

    use windows::Win32::Media::Audio::{
        eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator, AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
        AUDCLNT_STREAMFLAGS_LOOPBACK, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, WAVEFORMATEX,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};

    use super::{CHANNELS, SAMPLE_RATE};

    const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;

    /// Tangkapan suara keluaran perangkat putar bawaan Windows.
    pub struct Loopback {
        client: IAudioClient,
        capture: IAudioCaptureClient,
    }

    fn describe(what: &str, error: windows::core::Error) -> String {
        format!("{what}: {error}")
    }

    impl Loopback {
        pub fn open() -> Result<Loopback, String> {
            // SAFETY: panggilan COM/WASAPI biasa di thread audio; semua hasil diperiksa.
            unsafe {
                // Sudah diinisialisasi di thread ini = bukan galat.
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| describe("MMDeviceEnumerator", e))?;
                let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).map_err(|e| describe("tidak ada perangkat putar suara bawaan", e))?;
                let client: IAudioClient = device.Activate(CLSCTX_ALL, None).map_err(|e| describe("IAudioClient", e))?;
                // Float 32-bit stereo 48 kHz; Windows yang mengonversi dari format perangkat.
                let block = (CHANNELS * 4) as u16;
                let format = WAVEFORMATEX { wFormatTag: WAVE_FORMAT_IEEE_FLOAT, nChannels: CHANNELS as u16, nSamplesPerSec: SAMPLE_RATE, nAvgBytesPerSec: SAMPLE_RATE * u32::from(block), nBlockAlign: block, wBitsPerSample: 32, cbSize: 0 };
                let flags = AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
                // Penyangga 100 ms (satuan 100 ns).
                client.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, 1_000_000, 0, &format, None).map_err(|e| describe("IAudioClient::Initialize", e))?;
                let capture: IAudioCaptureClient = client.GetService().map_err(|e| describe("IAudioCaptureClient", e))?;
                client.Start().map_err(|e| describe("IAudioClient::Start", e))?;
                Ok(Loopback { client, capture })
            }
        }

        /// Tambahkan semua sampel yang sudah tersedia ke `out` (berselang kiri/kanan). Tidak menunggu.
        pub fn read(&mut self, out: &mut Vec<f32>) -> Result<(), String> {
            // SAFETY: penyangga dari GetBuffer sah sampai ReleaseBuffer dan berisi `frames * CHANNELS` float.
            unsafe {
                loop {
                    if self.capture.GetNextPacketSize().map_err(|e| describe("GetNextPacketSize", e))? == 0 {
                        return Ok(());
                    }
                    let mut data: *mut u8 = std::ptr::null_mut();
                    let (mut frames, mut flags) = (0u32, 0u32);
                    self.capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None).map_err(|e| describe("GetBuffer", e))?;
                    let samples = frames as usize * CHANNELS;
                    if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null() {
                        out.resize(out.len() + samples, 0.0);
                    } else {
                        out.extend_from_slice(std::slice::from_raw_parts(data as *const c_void as *const f32, samples));
                    }
                    self.capture.ReleaseBuffer(frames).map_err(|e| describe("ReleaseBuffer", e))?;
                }
            }
        }
    }

    impl Drop for Loopback {
        fn drop(&mut self) {
            // SAFETY: klien masih hidup; Stop pada klien yang sudah berhenti tidak berbahaya.
            unsafe {
                let _ = self.client.Stop();
            }
        }
    }
}

/// Sumber suara: loopback Windows bila tersedia, dibuka ulang bila perangkat berganti atau hilang.
struct Source {
    #[cfg(windows)]
    loopback: Option<loopback::Loopback>,
    retry_at: Instant,
}

impl Source {
    fn new() -> Source {
        Source {
            #[cfg(windows)]
            loopback: None,
            retry_at: Instant::now(),
        }
    }

    /// Tambahkan sampel yang tersedia ke `out`. Mengembalikan keterangan galat sekali tiap percobaan gagal.
    fn read(&mut self, out: &mut Vec<f32>) -> Option<String> {
        let _ = &out;
        #[cfg(windows)]
        {
            if self.loopback.is_none() && Instant::now() >= self.retry_at {
                self.retry_at = Instant::now() + Duration::from_secs(2);
                match loopback::Loopback::open() {
                    Ok(loopback) => self.loopback = Some(loopback),
                    Err(why) => return Some(why),
                }
            }
            if let Some(loopback) = &mut self.loopback {
                if let Err(why) = loopback.read(out) {
                    // Perangkat dicabut atau perangkat bawaan diganti: buka ulang nanti.
                    self.loopback = None;
                    return Some(why);
                }
            }
        }
        None
    }
}

/// Diagnostik: tangkap suara yang sedang diputar selama `duration` dan kembalikan (jumlah sampel, puncak 0..1).
/// Tidak memutar apa pun; kalau tidak ada yang sedang berbunyi, jumlah sampel bisa nol.
pub fn check_capture(duration: Duration) -> Result<(usize, f32), String> {
    let mut source = Source::new();
    let mut samples = Vec::new();
    let end = Instant::now() + duration;
    while Instant::now() < end {
        if let Some(why) = source.read(&mut samples) {
            return Err(why);
        }
        thread::sleep(Duration::from_millis(5));
    }
    if !cfg!(windows) {
        return Err("tangkapan suara hanya ada untuk Windows".into());
    }
    Ok((samples.len(), samples.iter().fold(0.0f32, |peak, s| peak.max(s.abs()))))
}

struct Target {
    launch_id: u32,
    key: [u8; 16],
    key_id: u32,
    duration_ms: u32,
}

pub(crate) fn serve(host: Arc<Host>, socket: UdpSocket) {
    match socket.try_clone() {
        Ok(receiver) => {
            let host = host.clone();
            thread::spawn(move || crate::video::listen_for_pings(host, receiver, |session, from| session.audio_client = Some(from)));
        }
        Err(e) => host.log(|| format!("audio: soket tidak bisa digandakan ({e}); audio tidak akan jalan")),
    }
    let mut served: Option<u32> = None;
    loop {
        let target = host.stream.lock().unwrap().as_ref().filter(|s| s.peer.is_some() && Some(s.launch_id) != served).map(|s| Target { launch_id: s.launch_id, key: s.key, key_id: s.key_id, duration_ms: s.config.audio_packet_ms });
        let Some(target) = target else {
            thread::sleep(Duration::from_millis(20));
            continue;
        };
        served = Some(target.launch_id);
        // Suara tidak wajib: kegagalan di sini dicatat, sesi (video) tetap jalan.
        if let Err(why) = stream(&host, &socket, &target) {
            host.log(|| format!("audio: {why}; sesi lanjut tanpa suara"));
        }
    }
}

/// Alamat audio client sesi ini. `Err(())` = sesi sudah berakhir.
fn client_of(host: &Host, launch_id: u32) -> Result<Option<SocketAddr>, ()> {
    host.stream.lock().unwrap().as_ref().filter(|s| s.launch_id == launch_id).map(|s| s.audio_client).ok_or(())
}

fn stream(host: &Host, socket: &UdpSocket, target: &Target) -> Result<(), String> {
    let deadline = Instant::now() + PING_TIMEOUT;
    let mut client = loop {
        match client_of(host, target.launch_id) {
            Err(()) => return Ok(()),
            Ok(Some(client)) => break client,
            Ok(None) if Instant::now() >= deadline => return Err("client tidak mengirim ping audio".into()),
            Ok(None) => thread::sleep(Duration::from_millis(5)),
        }
    };

    let mut encoder = OpusEncoder::new()?;
    let mut packetizer = AudioPacketizer::new(target.key, target.key_id, target.duration_ms);
    let mut source = Source::new();
    let frame_samples = (SAMPLE_RATE / 1000 * target.duration_ms) as usize * CHANNELS;
    let backlog_samples = SAMPLE_RATE as usize / 1000 * MAX_BACKLOG_MS * CHANNELS;
    let interval = Duration::from_millis(u64::from(target.duration_ms));
    let mut pending: Vec<f32> = Vec::with_capacity(backlog_samples);
    let mut packet = [0u8; 1400];
    let (mut frames, mut silent_frames) = (0u64, 0u64);
    let mut next_silence = Instant::now() + interval;
    host.log(|| format!("audio: mulai ke {client}, Opus stereo {} ms {} kbps", target.duration_ms, BITRATE / 1000));

    let drop_every = u64::from(host.cfg.test_drop_audio_every);
    let mut sent = 0u64;
    let mut send = |encoder: &mut OpusEncoder, pcm: &[f32], client: SocketAddr| -> Result<(), String> {
        let length = encoder.encode(pcm, &mut packet)?;
        for datagram in packetizer.push(&packet[..length]) {
            sent += 1;
            if drop_every != 0 && sent.is_multiple_of(drop_every) {
                continue;
            }
            let _ = socket.send_to(&datagram, client);
        }
        Ok(())
    };

    loop {
        match client_of(host, target.launch_id) {
            Err(()) => {
                host.log(|| format!("audio: selesai, {frames} frame ({silent_frames} hening)"));
                return Ok(());
            }
            Ok(Some(address)) => client = address,
            Ok(None) => {}
        }
        thread::sleep(Duration::from_millis(2));
        if !host.cfg.test_silent_audio {
            if let Some(why) = source.read(&mut pending) {
                host.log(|| format!("audio: tangkapan suara tidak tersedia ({why}); mengirim hening"));
            }
        }
        if pending.len() > backlog_samples {
            let excess = (pending.len() - backlog_samples) / frame_samples * frame_samples;
            pending.drain(..excess);
        }

        let now = Instant::now();
        while pending.len() >= frame_samples {
            send(&mut encoder, &pending[..frame_samples], client)?;
            pending.drain(..frame_samples);
            frames += 1;
            // Selama suara mengalir, frame hening ditunda.
            next_silence = now + interval * 2;
        }
        if now.saturating_duration_since(next_silence) > interval * 10 {
            // Thread sempat tertahan: jangan mengejar dengan semburan frame hening.
            next_silence = now;
        }
        while now >= next_silence {
            // Tidak ada suara: kirim hening (sisa sampel yang belum genap satu frame ikut dihabiskan).
            let mut pcm = vec![0f32; frame_samples];
            let carried = pending.len().min(frame_samples);
            pcm[..carried].copy_from_slice(&pending[..carried]);
            pending.clear();
            send(&mut encoder, &pcm, client)?;
            frames += 1;
            silent_frames += 1;
            next_silence += interval;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::cbc_decrypt;
    use cloudpc_stream_proto::audio::FEC_PARITY_MATRIX;
    use cloudpc_stream_proto::fec::{gf_inv, gf_mul};

    const KEY: [u8; 16] = [0x21; 16];

    #[test]
    fn data_packets_carry_encrypted_opus_with_running_sequence_and_timestamp() {
        let mut packetizer = AudioPacketizer::new(KEY, 1000, 5);
        for n in 0..3u16 {
            let opus = vec![n as u8 + 1; 60];
            let out = packetizer.push(&opus);
            assert_eq!(out.len(), 1, "belum genap satu blok FEC");
            let header = RtpHeader::read(&out[0]).unwrap();
            assert_eq!((header.header, header.packet_type, header.sequence_number, header.timestamp), (0x80, 97, n, u32::from(n) * 5));
            assert_eq!(out[0].len(), RTP_HEADER_SIZE + 64, "60 byte Opus + padding PKCS#7 = 64");
            assert_eq!(cbc_decrypt(&KEY, &audio_iv(1000, n), &out[0][RTP_HEADER_SIZE..]), Some(opus));
        }
    }

    #[test]
    fn every_fourth_packet_is_followed_by_two_parity_packets_that_recover_a_loss() {
        let mut packetizer = AudioPacketizer::new(KEY, 7, 10);
        // Lewati blok pertama supaya nomor dasar blok bukan nol.
        for _ in 0..4 {
            packetizer.push(&[0; 60]);
        }
        let mut payloads = Vec::new();
        let mut last = Vec::new();
        for n in 0..4u8 {
            last = packetizer.push(&[n * 3 + 1; 60]);
            payloads.push(last[0][RTP_HEADER_SIZE..].to_vec());
        }
        assert_eq!(last.len(), 3, "paket data keempat + dua paritas");
        for (index, packet) in last[1..].iter().enumerate() {
            let rtp = RtpHeader::read(packet).unwrap();
            assert_eq!((rtp.packet_type, rtp.sequence_number, rtp.timestamp), (127, 7 + index as u16 + 1, 0));
            let fec = AudioFecHeader::read(&packet[RTP_HEADER_SIZE..]).unwrap();
            assert_eq!((fec.fec_shard_index, fec.payload_type, fec.base_sequence_number, fec.base_timestamp), (index as u8, 97, 4, 40));
            // Pulihkan paket data ke-2 blok ini dari paritas ini saja.
            let parity = &packet[RTP_HEADER_SIZE + FEC_HEADER_SIZE..];
            let lost = 2;
            let recovered: Vec<u8> = (0..parity.len())
                .map(|byte| {
                    let rest = (0..4).filter(|i| *i != lost).fold(parity[byte], |acc, i| acc ^ gf_mul(FEC_PARITY_MATRIX[index][i], payloads[i][byte]));
                    gf_mul(gf_inv(FEC_PARITY_MATRIX[index][lost]), rest)
                })
                .collect();
            assert_eq!(recovered, payloads[lost]);
            assert_eq!(cbc_decrypt(&KEY, &audio_iv(7, 6), &recovered), Some(vec![7u8; 60]));
        }
    }

    #[test]
    fn unequal_payload_sizes_skip_parity_instead_of_sending_garbage() {
        let mut packetizer = AudioPacketizer::new(KEY, 0, 5);
        for size in [60, 60, 60] {
            packetizer.push(&vec![1; size]);
        }
        assert_eq!(packetizer.push(&[1; 90]).len(), 1, "blok dengan ukuran berbeda tidak diberi paritas");
        // Blok berikutnya normal lagi.
        for _ in 0..3 {
            assert_eq!(packetizer.push(&[2; 60]).len(), 1);
        }
        assert_eq!(packetizer.push(&[2; 60]).len(), 3);
    }

    #[test]
    fn opus_encoder_is_constant_bitrate_and_decodes_back_to_sound() {
        let mut encoder = OpusEncoder::new().unwrap();
        let mut packet = [0u8; 1400];
        let mut sizes = Vec::new();
        let mut energy = 0.0f32;
        let mut error = 0;
        // SAFETY: penggunaan decoder libopus sesuai API; hanya di tes.
        let decoder = unsafe { unsafe_libopus::opus_decoder_create(SAMPLE_RATE as i32, CHANNELS as i32, &mut error) };
        assert!(!decoder.is_null() && error == 0);
        for frame in 0..40 {
            // Nada 440 Hz, frame 5 ms (240 sampel per kanal).
            let pcm: Vec<f32> = (0..240).flat_map(|i| [(((frame * 240 + i) as f32) * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.5; 2]).collect();
            let length = encoder.encode(&pcm, &mut packet).unwrap();
            sizes.push(length);
            let mut decoded = [0f32; 480];
            let samples = unsafe { unsafe_libopus::opus_decode_float(decoder, packet.as_ptr(), length as i32, decoded.as_mut_ptr(), 240, 0) };
            assert_eq!(samples, 240);
            if frame > 4 {
                energy += decoded.iter().map(|s| s * s).sum::<f32>();
            }
        }
        unsafe { unsafe_libopus::opus_decoder_destroy(decoder) };
        // 96 kbit/s x 5 ms = 60 byte per frame, selalu.
        assert!(sizes.iter().all(|s| *s == 60), "ukuran paket: {sizes:?}");
        assert!(energy > 100.0, "hasil decode harus berisi nada, energi = {energy}");
    }
}
