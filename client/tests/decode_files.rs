//! Decoder melawan klip uji sintetis (`tests/data`, pola testsrc2 FFmpeg): CPU di semua sistem, dan di macOS juga
//! VideoToolbox. Tanpa host dan tanpa jendela, jadi jalan di CI.

use std::path::PathBuf;

use cloudpc_stream_client::rtsp::Codec;
use cloudpc_stream_client::video::ff::decode_file;

fn clip(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("data").join(name)
}

#[test]
fn every_codec_decodes_every_frame_on_the_cpu() {
    for (file, codec) in [("testsrc.h264", Codec::H264), ("testsrc.hevc", Codec::Hevc), ("testsrc.obu", Codec::Av1)] {
        let result = decode_file(&clip(file), codec, false).unwrap();
        assert!(!result.hardware);
        assert_eq!((result.frames, result.errors, result.size), (30, 0, (640, 360)), "{file}: {result:?}");
    }
}

/// macOS: H.264 dan HEVC lewat VideoToolbox (semua Mac yang didukung punya keduanya). AV1 hanya di chip M3 ke atas,
/// jadi cukup harus berhasil didecode (lewat hardware atau dav1d).
#[cfg(target_os = "macos")]
#[test]
fn videotoolbox_decodes_h264_and_hevc() {
    for (file, codec) in [("testsrc.h264", Codec::H264), ("testsrc.hevc", Codec::Hevc)] {
        let result = decode_file(&clip(file), codec, true).unwrap();
        assert!(result.hardware, "{file}: VideoToolbox tidak dipakai: {result:?}");
        assert_eq!((result.frames, result.errors), (30, 0), "{file}: {result:?}");
    }
    // AV1 hardware hanya di chip M3 ke atas; di Mac lain (atau bila hardware gagal di frame pertama) otomatis dav1d.
    let av1 = decode_file(&clip("testsrc.obu"), Codec::Av1, true).unwrap();
    eprintln!("AV1: {av1:?}, VideoToolbox mengaku mendukung: {:?}", cloudpc_stream_client::video::ff::hardware_codecs());
    assert_eq!((av1.frames, av1.errors), (30, 0), "{av1:?}");
}
