//! Suara di Android: PCM float stereo 48 kHz dari decoder Opus -> AAudio (mode latensi rendah). AAudio menarik data
//! lewat callback; thread sesi mengisi antrean pendek di antaranya.

use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::{Arc, Mutex};

use cloudpc_stream_client::audio::{CHANNELS, SAMPLE_RATE};
use cloudpc_stream_client::session::AudioSink;
use ndk::audio::{AudioCallbackResult, AudioDirection, AudioFormat, AudioPerformanceMode, AudioSharingMode, AudioStream, AudioStreamBuilder};

/// Antrean paling panjang 120 ms; lebih dari itu sampel tertua dibuang supaya suara tidak makin tertinggal dari gambar.
const MAX_SAMPLES: usize = SAMPLE_RATE as usize * CHANNELS * 120 / 1000;
/// Mulai (atau mulai lagi setelah kehabisan) memutar setelah terkumpul 30 ms, supaya tidak putus-putus tiap paket telat.
const PRIME_SAMPLES: usize = SAMPLE_RATE as usize * CHANNELS * 30 / 1000;

struct Ring {
    samples: VecDeque<f32>,
    primed: bool,
}

pub struct AndroidAudio {
    /// Dipegang supaya aliran tetap terbuka; ditutup saat di-drop.
    _stream: AudioStream,
    ring: Arc<Mutex<Ring>>,
}

impl AndroidAudio {
    pub fn open() -> Result<AndroidAudio, String> {
        let ring = Arc::new(Mutex::new(Ring { samples: VecDeque::with_capacity(MAX_SAMPLES), primed: false }));
        let source = Arc::clone(&ring);
        let stream = AudioStreamBuilder::new()
            .map_err(|e| format!("AAudio: {e}"))?
            .direction(AudioDirection::Output)
            .performance_mode(AudioPerformanceMode::LowLatency)
            .sharing_mode(AudioSharingMode::Shared)
            .format(AudioFormat::PCM_Float)
            .channel_count(CHANNELS as i32)
            .sample_rate(SAMPLE_RATE as i32)
            .data_callback(Box::new(move |_stream: &AudioStream, data: *mut c_void, frames: i32| {
                // SAFETY: AAudio memberi penyangga `frames` x kanal sampel float (format dan jumlah kanal kita yang minta).
                let out = unsafe { std::slice::from_raw_parts_mut(data.cast::<f32>(), frames.max(0) as usize * CHANNELS) };
                // try_lock: callback suara tidak boleh menunggu. Kalau thread sesi sedang mengisi, satu callback diam.
                match source.try_lock() {
                    Ok(mut ring) => {
                        if !ring.primed && ring.samples.len() >= PRIME_SAMPLES {
                            ring.primed = true;
                        }
                        if ring.primed {
                            for sample in out.iter_mut() {
                                *sample = ring.samples.pop_front().unwrap_or(0.0);
                            }
                            if ring.samples.is_empty() {
                                ring.primed = false;
                            }
                        } else {
                            out.fill(0.0);
                        }
                    }
                    Err(_) => out.fill(0.0),
                }
                AudioCallbackResult::Continue
            }))
            .open_stream()
            .map_err(|e| format!("membuka keluaran suara: {e}"))?;
        stream.request_start().map_err(|e| format!("memulai keluaran suara: {e}"))?;
        Ok(AndroidAudio { _stream: stream, ring })
    }
}

impl AudioSink for AndroidAudio {
    fn samples(&mut self, pcm: &[f32]) {
        if let Ok(mut ring) = self.ring.lock() {
            ring.samples.extend(pcm.iter().copied());
            let excess = ring.samples.len().saturating_sub(MAX_SAMPLES);
            if excess > 0 {
                // Buang kelipatan kanal supaya kiri/kanan tidak tertukar.
                let cut = (excess.div_ceil(CHANNELS) * CHANNELS).min(ring.samples.len());
                ring.samples.drain(..cut);
            }
        }
    }
}
