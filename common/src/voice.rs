//! Codec suara mikrofon (ekstensi CloudPC, lihat `proto::mic`): Opus mono 48 kHz, frame 20 ms, mode suara, dengan
//! FEC di dalam aliran. Encoder dipakai client, decoder dipakai host; keduanya di sini supaya diuji berpasangan.

use cloudpc_stream_proto::mic::{FRAME_SAMPLES, MAX_OPUS, SAMPLE_RATE};

/// 32 kbit/dtk: suara jernih untuk voice chat, dan masih ada ruang untuk salinan FEC.
const BITRATE: i32 = 32_000;
/// Encoder diberi tahu sekitar segini paket hilang, supaya ia menyisipkan salinan ringkas frame sebelumnya (LBRR).
const EXPECTED_LOSS_PERCENT: i32 = 10;

pub struct VoiceEncoder(*mut unsafe_libopus::OpusEncoder);

// Hanya dipakai dari satu thread pada satu waktu.
unsafe impl Send for VoiceEncoder {}

impl VoiceEncoder {
    pub fn new() -> Result<VoiceEncoder, String> {
        let mut error = 0;
        // SAFETY: argumen sah menurut API libopus; penunjuk hasil diperiksa.
        let raw = unsafe { unsafe_libopus::opus_encoder_create(SAMPLE_RATE as i32, 1, unsafe_libopus::OPUS_APPLICATION_VOIP, &mut error) };
        if raw.is_null() || error != 0 {
            return Err(format!("encoder suara Opus gagal dibuat (kode {error})"));
        }
        // SAFETY: `raw` encoder yang sah; tiap permintaan menerima satu argumen i32.
        unsafe {
            unsafe_libopus::opus_encoder_ctl!(raw, unsafe_libopus::OPUS_SET_BITRATE_REQUEST, BITRATE);
            unsafe_libopus::opus_encoder_ctl!(raw, unsafe_libopus::OPUS_SET_SIGNAL_REQUEST, unsafe_libopus::OPUS_SIGNAL_VOICE);
            unsafe_libopus::opus_encoder_ctl!(raw, unsafe_libopus::OPUS_SET_INBAND_FEC_REQUEST, 1);
            unsafe_libopus::opus_encoder_ctl!(raw, unsafe_libopus::OPUS_SET_PACKET_LOSS_PERC_REQUEST, EXPECTED_LOSS_PERCENT);
        }
        Ok(VoiceEncoder(raw))
    }

    /// `pcm`: tepat satu frame (`FRAME_SAMPLES` sampel mono, -1..=1). Mengembalikan satu paket Opus.
    pub fn encode(&mut self, pcm: &[f32]) -> Result<Vec<u8>, String> {
        if pcm.len() != FRAME_SAMPLES {
            return Err(format!("frame suara harus {FRAME_SAMPLES} sampel, bukan {}", pcm.len()));
        }
        let mut out = vec![0u8; MAX_OPUS];
        // SAFETY: `pcm` berisi tepat satu frame dan `out` sepanjang yang dinyatakan.
        let written = unsafe { unsafe_libopus::opus_encode_float(self.0, pcm.as_ptr(), FRAME_SAMPLES as i32, out.as_mut_ptr(), out.len() as i32) };
        let written = usize::try_from(written).map_err(|_| format!("Opus gagal meng-encode suara (kode {written})"))?;
        out.truncate(written);
        Ok(out)
    }
}

impl Drop for VoiceEncoder {
    fn drop(&mut self) {
        // SAFETY: dibuat oleh opus_encoder_create dan dihancurkan tepat sekali.
        unsafe { unsafe_libopus::opus_encoder_destroy(self.0) };
    }
}

pub struct VoiceDecoder(*mut unsafe_libopus::OpusDecoder);

// Hanya dipakai dari satu thread pada satu waktu.
unsafe impl Send for VoiceDecoder {}

impl VoiceDecoder {
    pub fn new() -> Result<VoiceDecoder, String> {
        let mut error = 0;
        // SAFETY: argumen sah menurut API libopus; penunjuk hasil diperiksa.
        let raw = unsafe { unsafe_libopus::opus_decoder_create(SAMPLE_RATE as i32, 1, &mut error) };
        if raw.is_null() || error != 0 {
            return Err(format!("decoder suara Opus gagal dibuat (kode {error})"));
        }
        Ok(VoiceDecoder(raw))
    }

    fn run(&mut self, data: Option<&[u8]>, fec: bool) -> [i16; FRAME_SAMPLES] {
        let mut pcm = [0i16; FRAME_SAMPLES];
        let (pointer, length) = data.map_or((std::ptr::null(), 0), |d| (d.as_ptr(), d.len() as i32));
        // SAFETY: `pcm` memuat tepat `FRAME_SAMPLES` sampel mono; `data` (bila ada) hidup selama panggilan.
        let decoded = unsafe { unsafe_libopus::opus_decode(self.0, pointer, length, pcm.as_mut_ptr(), FRAME_SAMPLES as i32, i32::from(fec)) };
        // Paket rusak atau lamanya bukan 20 ms: hening untuk frame ini, aliran tetap jalan.
        if decoded != FRAME_SAMPLES as i32 {
            pcm = [0; FRAME_SAMPLES];
        }
        pcm
    }

    /// Decode satu paket yang tiba.
    pub fn decode(&mut self, opus: &[u8]) -> [i16; FRAME_SAMPLES] {
        self.run(Some(opus), false)
    }

    /// Frame SEBELUM `next` hilang: pulihkan dari salinan FEC di dalam `next` (kalau tidak ada, Opus menebak).
    /// Panggil ini dulu, baru `decode(next)`.
    pub fn recover_from(&mut self, next: &[u8]) -> [i16; FRAME_SAMPLES] {
        self.run(Some(next), true)
    }

    /// Frame hilang tanpa paket sesudahnya di tangan: Opus menebak isinya (makin lama makin pelan).
    pub fn conceal(&mut self) -> [i16; FRAME_SAMPLES] {
        self.run(None, false)
    }

    /// Lupakan suara sebelumnya (setelah jeda panjang, mis. mikrofon dibisukan lalu dinyalakan lagi). Decoder dibuat
    /// ulang: makro `opus_decoder_ctl!` tanpa argumen (OPUS_RESET_STATE) di crate ini tidak bisa dipakai dari luar.
    pub fn reset(&mut self) {
        if let Ok(fresh) = VoiceDecoder::new() {
            *self = fresh;
        }
    }
}

impl Drop for VoiceDecoder {
    fn drop(&mut self) {
        // SAFETY: dibuat oleh opus_decoder_create dan dihancurkan tepat sekali.
        unsafe { unsafe_libopus::opus_decoder_destroy(self.0) };
    }
}

/// Nada sinus untuk uji (amplitudo 0,5), `frame` ke berapa supaya fasenya bersambung antar frame.
pub fn test_tone(frequency: f32, frame: u64) -> Vec<f32> {
    let start = frame * FRAME_SAMPLES as u64;
    (0..FRAME_SAMPLES as u64).map(|i| (((start + i) as f64 * f64::from(frequency) * std::f64::consts::TAU / f64::from(SAMPLE_RATE)).sin() * 0.5) as f32).collect()
}

/// Seberapa kuat `frequency` di `pcm` dibanding seluruh energinya (0..=1), dengan algoritme Goertzel. Untuk uji:
/// nada yang lolos utuh lewat encoder + jaringan + decoder menghasilkan nilai mendekati 1.
pub fn tone_purity(pcm: &[i16], frequency: f32) -> f64 {
    if pcm.is_empty() {
        return 0.0;
    }
    let omega = std::f64::consts::TAU * f64::from(frequency) / f64::from(SAMPLE_RATE);
    let coefficient = 2.0 * omega.cos();
    let (mut previous, mut before) = (0.0f64, 0.0f64);
    let mut energy = 0.0f64;
    for &sample in pcm {
        let x = f64::from(sample) / 32768.0;
        energy += x * x;
        let current = x + coefficient * previous - before;
        before = previous;
        previous = current;
    }
    if energy <= f64::EPSILON {
        return 0.0;
    }
    let power = previous * previous + before * before - coefficient * previous * before;
    (2.0 * power / (pcm.len() as f64 * energy)).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_tone(frames: u64) -> Vec<Vec<u8>> {
        let mut encoder = VoiceEncoder::new().unwrap();
        (0..frames).map(|frame| encoder.encode(&test_tone(440.0, frame)).unwrap()).collect()
    }

    #[test]
    fn tone_survives_encode_and_decode() {
        let mut decoder = VoiceDecoder::new().unwrap();
        let pcm: Vec<i16> = encode_tone(50).iter().flat_map(|packet| decoder.decode(packet)).collect();
        // Lewati awal (encoder baru mulai) lalu periksa sisanya.
        let steady = &pcm[FRAME_SAMPLES * 5..];
        assert!(tone_purity(steady, 440.0) > 0.9, "kemurnian {}", tone_purity(steady, 440.0));
        assert!(tone_purity(steady, 1000.0) < 0.05);
    }

    #[test]
    fn packets_are_small_and_wrong_frame_length_is_refused() {
        let packets = encode_tone(50);
        let average = packets.iter().map(Vec::len).sum::<usize>() / packets.len();
        assert!((20..=160).contains(&average), "rata-rata {average} byte per 20 ms");
        assert!(VoiceEncoder::new().unwrap().encode(&[0.0; 100]).is_err());
    }

    #[test]
    fn one_lost_packet_is_recovered_from_the_next() {
        let packets = encode_tone(60);
        let mut decoder = VoiceDecoder::new().unwrap();
        let mut pcm = Vec::new();
        for (index, packet) in packets.iter().enumerate() {
            if index % 10 == 5 {
                continue; // hilang
            }
            if index % 10 == 6 {
                pcm.extend(decoder.recover_from(packet));
            }
            pcm.extend(decoder.decode(packet));
        }
        assert_eq!(pcm.len(), packets.len() * FRAME_SAMPLES, "tidak ada frame yang lenyap dari garis waktu");
        assert!(tone_purity(&pcm[FRAME_SAMPLES * 5..], 440.0) > 0.8, "kemurnian {}", tone_purity(&pcm[FRAME_SAMPLES * 5..], 440.0));
    }

    #[test]
    fn garbage_becomes_silence_not_noise() {
        let mut decoder = VoiceDecoder::new().unwrap();
        // Byte TOC 0xFF + isi acak: lamanya bukan 20 ms atau isinya tidak sah.
        let pcm = decoder.decode(&[0xFF, 0x12, 0x34]);
        assert!(pcm.iter().all(|&s| s == 0));
    }

    #[test]
    fn purity_of_silence_is_zero() {
        assert_eq!(tone_purity(&[0; 960], 440.0), 0.0);
        assert_eq!(tone_purity(&[], 440.0), 0.0);
    }
}
