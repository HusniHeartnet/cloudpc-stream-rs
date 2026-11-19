//! C4: suara utuh dari encoder + packetizer host CloudPC (format kawat Sunshine) sampai PCM di client: nada 440 Hz
//! keluar lagi sebagai 440 Hz, juga saat paket hilang (dipulihkan FEC atau ditebak Opus).

use cloudpc_stream_client::audio::{AudioDepacketizer, OpusDecoder, CHANNELS, SAMPLE_RATE};
use cloudpc_stream_host::audio::{AudioPacketizer, OpusEncoder};

const KEY: [u8; 16] = *b"kunci-suara-uji!";
const KEY_ID: u32 = 77;
/// 5 ms per frame, seperti `x-nv-aqos.packetDuration` yang dikirim client.
const FRAME: usize = SAMPLE_RATE as usize / 200;

fn tone(frames: usize) -> Vec<f32> {
    (0..frames * FRAME)
        .flat_map(|i| {
            let v = (i as f32 * 440.0 * std::f32::consts::TAU / SAMPLE_RATE as f32).sin() * 0.4;
            [v, v]
        })
        .collect()
}

/// Kirim `frames` frame nada lewat host -> buang paket sesuai `drop` -> client; kembalikan PCM dan statistik.
fn run(frames: usize, drop: impl Fn(usize) -> bool) -> (Vec<f32>, cloudpc_stream_client::audio::AudioStats) {
    let pcm = tone(frames);
    let mut encoder = OpusEncoder::new().unwrap();
    let mut packetizer = AudioPacketizer::new(KEY, KEY_ID, 5);
    let mut depacketizer = AudioDepacketizer::new(Some((KEY, KEY_ID)));
    let mut decoder = OpusDecoder::new().unwrap();
    let mut out = Vec::new();
    let mut index = 0;
    for chunk in pcm.chunks(FRAME * CHANNELS) {
        let mut opus = vec![0u8; 1500];
        let len = encoder.encode(chunk, &mut opus).unwrap();
        for datagram in packetizer.push(&opus[..len]) {
            index += 1;
            if !drop(index) {
                for frame in depacketizer.push(&datagram) {
                    out.extend(decoder.decode(&frame));
                }
            }
        }
    }
    (out, depacketizer.stats())
}

/// Frekuensi dominan kanal kiri dari jumlah lintasan nol (cukup untuk nada murni).
fn frequency(pcm: &[f32]) -> f32 {
    let left: Vec<f32> = pcm.iter().step_by(CHANNELS).copied().skip(FRAME * 20).collect();
    let crossings = left.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
    crossings as f32 * SAMPLE_RATE as f32 / left.len() as f32
}

fn level(pcm: &[f32]) -> f32 {
    (pcm.iter().map(|v| v * v).sum::<f32>() / pcm.len() as f32).sqrt()
}

#[test]
fn a_tone_comes_out_as_the_same_tone() {
    let (pcm, stats) = run(400, |_| false);
    assert_eq!(pcm.len(), 400 * FRAME * CHANNELS, "2 detik suara, tidak ada frame yang hilang");
    let f = frequency(&pcm);
    assert!((430.0..450.0).contains(&f), "frekuensi {f} Hz");
    assert!(level(&pcm) > 0.2, "level {}", level(&pcm));
    assert_eq!((stats.recovered, stats.lost, stats.invalid), (0, 0, 0));
}

#[test]
fn lost_packets_are_recovered_or_concealed_without_gaps() {
    // Satu dari tiap 9 datagram dibuang (data maupun paritas): sebagian besar dipulihkan FEC.
    let (pcm, stats) = run(400, |i| i % 9 == 0);
    assert_eq!(pcm.len(), 400 * FRAME * CHANNELS, "panjang suara tetap: frame hilang diganti tebakan, bukan dilewati");
    assert!(stats.recovered > 20, "{stats:?}");
    let f = frequency(&pcm);
    assert!((425.0..455.0).contains(&f), "frekuensi {f} Hz");
    // Semburan: dua paket data berturut-turut di tiap blok ke-10 -> masih bisa dipulihkan (2 paritas).
    let (_, burst) = run(400, |i| i % 60 == 1 || i % 60 == 2);
    assert_eq!(burst.lost, 0, "{burst:?}");
}
