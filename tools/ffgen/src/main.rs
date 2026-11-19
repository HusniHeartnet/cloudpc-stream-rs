// Membuat client/src/video/ffi.rs dari header FFmpeg 8.0 (sekali jalan; hasilnya di-commit). Crate tersendiri, bukan anggota
// workspace stream-rs: butuh libclang (LIBCLANG_PATH). Lihat README bagian C3.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (ffmpeg, out) = (&args[1], &args[2]);
    let functions = [
        "avcodec_version", "avutil_version", "avcodec_find_decoder", "avcodec_find_decoder_by_name", "avcodec_alloc_context3",
        "avcodec_free_context", "avcodec_open2", "avcodec_send_packet", "avcodec_receive_frame", "avcodec_flush_buffers",
        "avcodec_get_hw_config", "av_packet_alloc", "av_packet_free", "av_new_packet", "av_packet_unref", "av_frame_alloc",
        "av_frame_free", "av_frame_unref", "av_hwdevice_ctx_alloc", "av_hwdevice_ctx_init", "av_buffer_ref", "av_buffer_unref",
        "av_strerror", "av_log_set_level", "av_get_pix_fmt_name", "av_hwdevice_ctx_create", "av_hwframe_transfer_data", "av_parser_init", "av_parser_parse2", "av_parser_close",
    ];
    let mut builder = bindgen::Builder::default()
        .header(format!("{}/wrapper.h", env!("CARGO_MANIFEST_DIR")))
        .clang_arg(format!("-I{ffmpeg}/include"))
        .allowlist_type("AVHWDeviceContext")
        .allowlist_var("AV_CODEC_FLAG_LOW_DELAY|AV_CODEC_FLAG2_FAST|AV_INPUT_BUFFER_PADDING_SIZE|AV_LOG_.*|AV_CODEC_HW_CONFIG_METHOD_.*|AV_ERROR_MAX_STRING_SIZE")
        .default_enum_style(bindgen::EnumVariation::NewType { is_bitfield: false, is_global: false })
        .layout_tests(false)
        .generate_comments(false)
        .derive_default(true);
    for f in functions {
        builder = builder.allowlist_function(f);
    }
    builder.generate().expect("bindgen").write_to_file(out).expect("tulis");
}
