//! C2: depacketizer client melawan packetizer host CloudPC (format kawat sama dengan Sunshine): frame utuh kembali
//! persis, shard hilang dipulihkan dari paritas, frame yang tidak bisa dipulihkan dilaporkan.

use cloudpc_stream_client::video::depacketize::{Depacketizer, Output, VideoFrame};
use cloudpc_stream_host::video::packetize::{Frame, Packetizer};
use cloudpc_stream_proto::video::FrameType;

const PACKET_SIZE: u32 = 1392;
const KEY: [u8; 16] = *b"kunci-uji-c2-vid";

/// Generator acak deterministik (LCG), supaya pola paket hilang bisa diulang.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn bitstream(index: u32, len: usize) -> Vec<u8> {
    (0..len).map(|i| (i as u32 * 13 + index * 101 + (i as u32 >> 8)) as u8).collect()
}

/// Ukuran frame yang mewakili: 1 shard, beberapa shard, pas kelipatan payload, IDR besar (2 blok FEC).
const SIZES: [usize; 6] = [40, 5_000, 1_368 * 3 - 8, 60_000, 300_000, 900];

/// Frame asli `(nomor, jenis, bitstream)` dan datagram per frame.
type Packetized = (Vec<(u32, FrameType, Vec<u8>)>, Vec<Vec<Vec<u8>>>);

fn packetize_all(key: Option<[u8; 16]>, frames: u32) -> Packetized {
    let mut packetizer = Packetizer::new(PACKET_SIZE, 2, key);
    let mut originals = Vec::new();
    let mut datagrams = Vec::new();
    for index in 1..=frames {
        let data = bitstream(index, SIZES[(index as usize - 1) % SIZES.len()]);
        let frame_type = if index == 1 { FrameType::Idr } else { FrameType::P };
        datagrams.push(packetizer.packetize(&Frame { index, frame_type, timestamp: index * 1500, processing_latency: 17, data: &data }));
        originals.push((index, frame_type, data));
    }
    (originals, datagrams)
}

fn frames_of(outputs: &[Output]) -> Vec<&VideoFrame> {
    outputs.iter().filter_map(|o| if let Output::Frame(f) = o { Some(f) } else { None }).collect()
}

#[test]
fn every_frame_comes_back_exactly_without_loss() {
    for key in [Some(KEY), None] {
        let (originals, datagrams) = packetize_all(key, 18);
        let mut depacketizer = Depacketizer::new(PACKET_SIZE, key);
        let outputs: Vec<Output> = datagrams.iter().flatten().flat_map(|d| depacketizer.push(d)).collect();
        let frames = frames_of(&outputs);
        assert_eq!(frames.len(), originals.len(), "{outputs:?}");
        for (frame, (index, frame_type, data)) in frames.iter().zip(&originals) {
            assert_eq!((frame.index, frame.frame_type, frame.host_latency, frame.rtp_timestamp), (*index, *frame_type, 17, index * 1500));
            assert_eq!(&frame.data, data, "frame {index}: bitstream persis, tanpa padding");
            assert_eq!(frame.recovered_shards, 0);
        }
        let stats = depacketizer.stats();
        assert_eq!((stats.frames, stats.frames_lost, stats.invalid, stats.duplicates), (18, 0, 0, 0));
        assert!(stats.late > 0, "paritas yang datang setelah blok lengkap diabaikan");
    }
}

#[test]
fn lost_shards_are_recovered_from_parity_in_every_block() {
    let (originals, datagrams) = packetize_all(Some(KEY), 30);
    let mut depacketizer = Depacketizer::new(PACKET_SIZE, Some(KEY));
    let mut rng = Lcg(7);
    let mut outputs = Vec::new();
    for packets in &datagrams {
        // Buang paket sebanyak paritas minimum (2) di tiap frame, di posisi acak: selalu bisa dipulihkan karena
        // tiap blok punya paritas >= 2.
        let drop: Vec<usize> = (0..2).map(|_| rng.below(packets.len())).collect();
        for (i, packet) in packets.iter().enumerate() {
            if !drop.contains(&i) {
                outputs.extend(depacketizer.push(packet));
            }
        }
    }
    let frames = frames_of(&outputs);
    assert_eq!(frames.len(), 30, "{:?}", depacketizer.stats());
    for (frame, (_, _, data)) in frames.iter().zip(&originals) {
        assert_eq!(&frame.data, data, "frame {}", frame.index);
    }
    let stats = depacketizer.stats();
    assert!(stats.frames_recovered > 10 && stats.shards_recovered > 10, "{stats:?}");
    assert_eq!(stats.frames_lost, 0);
}

#[test]
fn reordered_and_duplicated_packets_within_a_block_are_fine() {
    let (originals, datagrams) = packetize_all(Some(KEY), 6);
    let mut depacketizer = Depacketizer::new(PACKET_SIZE, Some(KEY));
    let mut outputs = Vec::new();
    for packets in &datagrams {
        let mut order: Vec<&Vec<u8>> = packets.iter().collect();
        // Tukar pasangan bertetangga dan kirim paket pertama dua kali.
        for pair in order.chunks_mut(2) {
            pair.reverse();
        }
        outputs.extend(depacketizer.push(order[0]));
        for packet in order {
            outputs.extend(depacketizer.push(packet));
        }
    }
    let frames = frames_of(&outputs);
    assert_eq!(frames.len(), 6, "{:?}", depacketizer.stats());
    for (frame, (_, _, data)) in frames.iter().zip(&originals) {
        assert_eq!(&frame.data, data);
    }
    assert!(depacketizer.stats().duplicates >= 1);
}

#[test]
fn unrecoverable_and_missing_frames_are_reported_and_the_stream_continues() {
    let (originals, datagrams) = packetize_all(Some(KEY), 9);
    let mut depacketizer = Depacketizer::new(PACKET_SIZE, Some(KEY));
    let mut outputs = Vec::new();
    for (i, packets) in datagrams.iter().enumerate() {
        let index = i as u32 + 1;
        let skip = match index {
            // Frame 3 (1 shard data + 2 paritas): ketiganya hilang = frame tidak pernah terlihat.
            3 => packets.len(),
            // Frame 4: kehilangan lebih banyak dari paritasnya.
            4 => packets.len() / 2,
            _ => 0,
        };
        for packet in &packets[skip..] {
            outputs.extend(depacketizer.push(packet));
        }
    }
    let lost: Vec<&Output> = outputs.iter().filter(|o| matches!(o, Output::Lost { .. })).collect();
    // Frame 3 dilaporkan saat paket frame 4 tiba (tidak pernah terlihat); frame 4 saat paket frame 5 tiba (tidak lengkap).
    assert_eq!(lost, [&Output::Lost { first: 3, last: 3 }, &Output::Lost { first: 4, last: 4 }]);
    let indexes: Vec<u32> = frames_of(&outputs).iter().map(|f| f.index).collect();
    assert_eq!(indexes, [1, 2, 5, 6, 7, 8, 9]);
    for frame in frames_of(&outputs) {
        assert_eq!(frame.data, originals[frame.index as usize - 1].2);
    }
    assert_eq!(depacketizer.stats().frames_lost, 2);
}

#[test]
fn a_missing_first_block_of_a_large_frame_loses_only_that_frame() {
    let (_, datagrams) = packetize_all(Some(KEY), 6);
    // Frame 5 = 300 KB: lebih dari satu blok FEC. Buang seluruh blok pertamanya.
    // 300 KB + header 8 = 219 shard data (payload 1376) > 212 (batas satu blok dengan paritas 20%), jadi dua blok:
    // 110 data + 22 paritas, lalu 109 + 22. Datagram 0..132 = seluruh blok pertama.
    let frame5 = &datagrams[4];
    assert_eq!(frame5.len(), 263, "dua blok 110+22 dan 109+22");
    let half = 110 + 22;
    let mut depacketizer = Depacketizer::new(PACKET_SIZE, Some(KEY));
    let mut outputs = Vec::new();
    for (i, packets) in datagrams.iter().enumerate() {
        let start = if i == 4 { half } else { 0 };
        for packet in &packets[start..] {
            outputs.extend(depacketizer.push(packet));
        }
    }
    let indexes: Vec<u32> = frames_of(&outputs).iter().map(|f| f.index).collect();
    assert_eq!(indexes, [1, 2, 3, 4, 6]);
    assert!(outputs.contains(&Output::Lost { first: 5, last: 5 }), "{outputs:?}");
}

#[test]
fn wrong_key_is_counted_as_invalid() {
    let (_, datagrams) = packetize_all(Some(KEY), 2);
    let mut depacketizer = Depacketizer::new(PACKET_SIZE, Some(*b"kunci-yang-salah"));
    let outputs: Vec<Output> = datagrams.iter().flatten().flat_map(|d| depacketizer.push(d)).collect();
    assert!(outputs.is_empty());
    assert_eq!(depacketizer.stats().invalid, depacketizer.stats().packets);
}
