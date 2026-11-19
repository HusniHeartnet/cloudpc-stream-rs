//! Binding FFmpeg 8.0 (libavcodec 62, libavutil 60), Windows x64 dan macOS 64-bit (tanpa tipe C `long`, jadi tata letak
//! sama di kedua sistem). DIBUAT OTOMATIS dengan bindgen 0.72 dari header
//! paket BtbN.FFmpeg.GPL.Shared.8.0; daftar fungsi yang diambil ada di stream-rs/README.md (bagian C3). Jangan diedit tangan.
#![allow(non_camel_case_types, non_snake_case, non_upper_case_globals, dead_code, clippy::all)]


pub const AV_ERROR_MAX_STRING_SIZE: u32 = 64;
pub const AV_LOG_QUIET: i32 = -8;
pub const AV_LOG_PANIC: u32 = 0;
pub const AV_LOG_FATAL: u32 = 8;
pub const AV_LOG_ERROR: u32 = 16;
pub const AV_LOG_WARNING: u32 = 24;
pub const AV_LOG_INFO: u32 = 32;
pub const AV_LOG_VERBOSE: u32 = 40;
pub const AV_LOG_DEBUG: u32 = 48;
pub const AV_LOG_TRACE: u32 = 56;
pub const AV_LOG_MAX_OFFSET: u32 = 64;
pub const AV_LOG_SKIP_REPEATED: u32 = 1;
pub const AV_LOG_PRINT_LEVEL: u32 = 2;
pub const AV_LOG_PRINT_TIME: u32 = 4;
pub const AV_LOG_PRINT_DATETIME: u32 = 8;
pub const AV_INPUT_BUFFER_PADDING_SIZE: u32 = 64;
pub const AV_CODEC_FLAG_LOW_DELAY: u32 = 524288;
pub const AV_CODEC_FLAG2_FAST: u32 = 1;
impl AVSampleFormat {
    pub const AV_SAMPLE_FMT_NONE: AVSampleFormat = AVSampleFormat(-1);
    pub const AV_SAMPLE_FMT_U8: AVSampleFormat = AVSampleFormat(0);
    pub const AV_SAMPLE_FMT_S16: AVSampleFormat = AVSampleFormat(1);
    pub const AV_SAMPLE_FMT_S32: AVSampleFormat = AVSampleFormat(2);
    pub const AV_SAMPLE_FMT_FLT: AVSampleFormat = AVSampleFormat(3);
    pub const AV_SAMPLE_FMT_DBL: AVSampleFormat = AVSampleFormat(4);
    pub const AV_SAMPLE_FMT_U8P: AVSampleFormat = AVSampleFormat(5);
    pub const AV_SAMPLE_FMT_S16P: AVSampleFormat = AVSampleFormat(6);
    pub const AV_SAMPLE_FMT_S32P: AVSampleFormat = AVSampleFormat(7);
    pub const AV_SAMPLE_FMT_FLTP: AVSampleFormat = AVSampleFormat(8);
    pub const AV_SAMPLE_FMT_DBLP: AVSampleFormat = AVSampleFormat(9);
    pub const AV_SAMPLE_FMT_S64: AVSampleFormat = AVSampleFormat(10);
    pub const AV_SAMPLE_FMT_S64P: AVSampleFormat = AVSampleFormat(11);
    pub const AV_SAMPLE_FMT_NB: AVSampleFormat = AVSampleFormat(12);
}
#[repr(transparent)]
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct AVSampleFormat(pub ::std::os::raw::c_int);
unsafe extern "C" {
    pub fn avutil_version() -> ::std::os::raw::c_uint;
}
impl AVMediaType {
    pub const AVMEDIA_TYPE_UNKNOWN: AVMediaType = AVMediaType(-1);
    pub const AVMEDIA_TYPE_VIDEO: AVMediaType = AVMediaType(0);
    pub const AVMEDIA_TYPE_AUDIO: AVMediaType = AVMediaType(1);
    pub const AVMEDIA_TYPE_DATA: AVMediaType = AVMediaType(2);
    pub const AVMEDIA_TYPE_SUBTITLE: AVMediaType = AVMediaType(3);
    pub const AVMEDIA_TYPE_ATTACHMENT: AVMediaType = AVMediaType(4);
    pub const AVMEDIA_TYPE_NB: AVMediaType = AVMediaType(5);
}
#[repr(transparent)]
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct AVMediaType(pub ::std::os::raw::c_int);
impl AVPictureType {
    pub const AV_PICTURE_TYPE_NONE: AVPictureType = AVPictureType(0);
    pub const AV_PICTURE_TYPE_I: AVPictureType = AVPictureType(1);
    pub const AV_PICTURE_TYPE_P: AVPictureType = AVPictureType(2);
    pub const AV_PICTURE_TYPE_B: AVPictureType = AVPictureType(3);
    pub const AV_PICTURE_TYPE_S: AVPictureType = AVPictureType(4);
    pub const AV_PICTURE_TYPE_SI: AVPictureType = AVPictureType(5);
    pub const AV_PICTURE_TYPE_SP: AVPictureType = AVPictureType(6);
    pub const AV_PICTURE_TYPE_BI: AVPictureType = AVPictureType(7);
}
#[repr(transparent)]
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct AVPictureType(pub ::std::os::raw::c_int);
unsafe extern "C" {
    pub fn av_strerror(
        errnum: ::std::os::raw::c_int,
        errbuf: *mut ::std::os::raw::c_char,
        errbuf_size: usize,
    ) -> ::std::os::raw::c_int;
}
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct AVRational {
    pub num: ::std::os::raw::c_int,
    pub den: ::std::os::raw::c_int,
}
impl AVClassCategory {
    pub const AV_CLASS_CATEGORY_NA: AVClassCategory = AVClassCategory(0);
    pub const AV_CLASS_CATEGORY_INPUT: AVClassCategory = AVClassCategory(1);
    pub const AV_CLASS_CATEGORY_OUTPUT: AVClassCategory = AVClassCategory(2);
    pub const AV_CLASS_CATEGORY_MUXER: AVClassCategory = AVClassCategory(3);
    pub const AV_CLASS_CATEGORY_DEMUXER: AVClassCategory = AVClassCategory(4);
    pub const AV_CLASS_CATEGORY_ENCODER: AVClassCategory = AVClassCategory(5);
    pub const AV_CLASS_CATEGORY_DECODER: AVClassCategory = AVClassCategory(6);
    pub const AV_CLASS_CATEGORY_FILTER: AVClassCategory = AVClassCategory(7);
    pub const AV_CLASS_CATEGORY_BITSTREAM_FILTER: AVClassCategory = AVClassCategory(8);
    pub const AV_CLASS_CATEGORY_SWSCALER: AVClassCategory = AVClassCategory(9);
    pub const AV_CLASS_CATEGORY_SWRESAMPLER: AVClassCategory = AVClassCategory(10);
    pub const AV_CLASS_CATEGORY_HWDEVICE: AVClassCategory = AVClassCategory(11);
    pub const AV_CLASS_CATEGORY_DEVICE_VIDEO_OUTPUT: AVClassCategory = AVClassCategory(40);
    pub const AV_CLASS_CATEGORY_DEVICE_VIDEO_INPUT: AVClassCategory = AVClassCategory(41);
    pub const AV_CLASS_CATEGORY_DEVICE_AUDIO_OUTPUT: AVClassCategory = AVClassCategory(42);
    pub const AV_CLASS_CATEGORY_DEVICE_AUDIO_INPUT: AVClassCategory = AVClassCategory(43);
    pub const AV_CLASS_CATEGORY_DEVICE_OUTPUT: AVClassCategory = AVClassCategory(44);
    pub const AV_CLASS_CATEGORY_DEVICE_INPUT: AVClassCategory = AVClassCategory(45);
    pub const AV_CLASS_CATEGORY_NB: AVClassCategory = AVClassCategory(46);
}
#[repr(transparent)]
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct AVClassCategory(pub ::std::os::raw::c_int);
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct AVOptionRanges {
    _unused: [u8; 0],
}
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct AVClass {
    pub class_name: *const ::std::os::raw::c_char,
    pub item_name: ::std::option::Option<
        unsafe extern "C" fn(ctx: *mut ::std::os::raw::c_void) -> *const ::std::os::raw::c_char,
    >,
    pub option: *mut AVOption,
    pub version: ::std::os::raw::c_int,
    pub log_level_offset_offset: ::std::os::raw::c_int,
    pub parent_log_context_offset: ::std::os::raw::c_int,
    pub category: AVClassCategory,
    pub get_category: ::std::option::Option<
        unsafe extern "C" fn(ctx: *mut ::std::os::raw::c_void) -> AVClassCategory,
    >,
    pub query_ranges: ::std::option::Option<
        unsafe extern "C" fn(
            arg1: *mut *mut AVOptionRanges,
            obj: *mut ::std::os::raw::c_void,
            key: *const ::std::os::raw::c_char,
            flags: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int,
    >,
    pub child_next: ::std::option::Option<
        unsafe extern "C" fn(
            obj: *mut ::std::os::raw::c_void,
            prev: *mut ::std::os::raw::c_void,
        ) -> *mut ::std::os::raw::c_void,
    >,
    pub child_class_iterate: ::std::option::Option<
        unsafe extern "C" fn(iter: *mut *mut ::std::os::raw::c_void) -> *const AVClass,
    >,
    pub state_flags_offset: ::std::os::raw::c_int,
}
impl Default for AVClass {
    fn default() -> Self {
        let mut s = ::std::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            ::std::ptr::write_bytes(s.as_mut_ptr(), 0, 1);
            s.assume_init()
        }
    }
}
unsafe extern "C" {
    pub fn av_log_set_level(level: ::std::os::raw::c_int);
}
impl AVPixelFormat {
    pub const AV_PIX_FMT_NONE: AVPixelFormat = AVPixelFormat(-1);
    pub const AV_PIX_FMT_YUV420P: AVPixelFormat = AVPixelFormat(0);
    pub const AV_PIX_FMT_YUYV422: AVPixelFormat = AVPixelFormat(1);
    pub const AV_PIX_FMT_RGB24: AVPixelFormat = AVPixelFormat(2);
    pub const AV_PIX_FMT_BGR24: AVPixelFormat = AVPixelFormat(3);
    pub const AV_PIX_FMT_YUV422P: AVPixelFormat = AVPixelFormat(4);
    pub const AV_PIX_FMT_YUV444P: AVPixelFormat = AVPixelFormat(5);
    pub const AV_PIX_FMT_YUV410P: AVPixelFormat = AVPixelFormat(6);
    pub const AV_PIX_FMT_YUV411P: AVPixelFormat = AVPixelFormat(7);
    pub const AV_PIX_FMT_GRAY8: AVPixelFormat = AVPixelFormat(8);
    pub const AV_PIX_FMT_MONOWHITE: AVPixelFormat = AVPixelFormat(9);
    pub const AV_PIX_FMT_MONOBLACK: AVPixelFormat = AVPixelFormat(10);
    pub const AV_PIX_FMT_PAL8: AVPixelFormat = AVPixelFormat(11);
    pub const AV_PIX_FMT_YUVJ420P: AVPixelFormat = AVPixelFormat(12);
    pub const AV_PIX_FMT_YUVJ422P: AVPixelFormat = AVPixelFormat(13);
    pub const AV_PIX_FMT_YUVJ444P: AVPixelFormat = AVPixelFormat(14);
    pub const AV_PIX_FMT_UYVY422: AVPixelFormat = AVPixelFormat(15);
    pub const AV_PIX_FMT_UYYVYY411: AVPixelFormat = AVPixelFormat(16);
    pub const AV_PIX_FMT_BGR8: AVPixelFormat = AVPixelFormat(17);
    pub const AV_PIX_FMT_BGR4: AVPixelFormat = AVPixelFormat(18);
    pub const AV_PIX_FMT_BGR4_BYTE: AVPixelFormat = AVPixelFormat(19);
    pub const AV_PIX_FMT_RGB8: AVPixelFormat = AVPixelFormat(20);
    pub const AV_PIX_FMT_RGB4: AVPixelFormat = AVPixelFormat(21);
    pub const AV_PIX_FMT_RGB4_BYTE: AVPixelFormat = AVPixelFormat(22);
    pub const AV_PIX_FMT_NV12: AVPixelFormat = AVPixelFormat(23);
    pub const AV_PIX_FMT_NV21: AVPixelFormat = AVPixelFormat(24);
    pub const AV_PIX_FMT_ARGB: AVPixelFormat = AVPixelFormat(25);
    pub const AV_PIX_FMT_RGBA: AVPixelFormat = AVPixelFormat(26);
    pub const AV_PIX_FMT_ABGR: AVPixelFormat = AVPixelFormat(27);
    pub const AV_PIX_FMT_BGRA: AVPixelFormat = AVPixelFormat(28);
    pub const AV_PIX_FMT_GRAY16BE: AVPixelFormat = AVPixelFormat(29);
    pub const AV_PIX_FMT_GRAY16LE: AVPixelFormat = AVPixelFormat(30);
    pub const AV_PIX_FMT_YUV440P: AVPixelFormat = AVPixelFormat(31);
    pub const AV_PIX_FMT_YUVJ440P: AVPixelFormat = AVPixelFormat(32);
    pub const AV_PIX_FMT_YUVA420P: AVPixelFormat = AVPixelFormat(33);
    pub const AV_PIX_FMT_RGB48BE: AVPixelFormat = AVPixelFormat(34);
    pub const AV_PIX_FMT_RGB48LE: AVPixelFormat = AVPixelFormat(35);
    pub const AV_PIX_FMT_RGB565BE: AVPixelFormat = AVPixelFormat(36);
    pub const AV_PIX_FMT_RGB565LE: AVPixelFormat = AVPixelFormat(37);
    pub const AV_PIX_FMT_RGB555BE: AVPixelFormat = AVPixelFormat(38);
    pub const AV_PIX_FMT_RGB555LE: AVPixelFormat = AVPixelFormat(39);
    pub const AV_PIX_FMT_BGR565BE: AVPixelFormat = AVPixelFormat(40);
    pub const AV_PIX_FMT_BGR565LE: AVPixelFormat = AVPixelFormat(41);
    pub const AV_PIX_FMT_BGR555BE: AVPixelFormat = AVPixelFormat(42);
    pub const AV_PIX_FMT_BGR555LE: AVPixelFormat = AVPixelFormat(43);
    pub const AV_PIX_FMT_VAAPI: AVPixelFormat = AVPixelFormat(44);
    pub const AV_PIX_FMT_YUV420P16LE: AVPixelFormat = AVPixelFormat(45);
    pub const AV_PIX_FMT_YUV420P16BE: AVPixelFormat = AVPixelFormat(46);
    pub const AV_PIX_FMT_YUV422P16LE: AVPixelFormat = AVPixelFormat(47);
    pub const AV_PIX_FMT_YUV422P16BE: AVPixelFormat = AVPixelFormat(48);
    pub const AV_PIX_FMT_YUV444P16LE: AVPixelFormat = AVPixelFormat(49);
    pub const AV_PIX_FMT_YUV444P16BE: AVPixelFormat = AVPixelFormat(50);
    pub const AV_PIX_FMT_DXVA2_VLD: AVPixelFormat = AVPixelFormat(51);
    pub const AV_PIX_FMT_RGB444LE: AVPixelFormat = AVPixelFormat(52);
    pub const AV_PIX_FMT_RGB444BE: AVPixelFormat = AVPixelFormat(53);
    pub const AV_PIX_FMT_BGR444LE: AVPixelFormat = AVPixelFormat(54);
    pub const AV_PIX_FMT_BGR444BE: AVPixelFormat = AVPixelFormat(55);
    pub const AV_PIX_FMT_YA8: AVPixelFormat = AVPixelFormat(56);
    pub const AV_PIX_FMT_Y400A: AVPixelFormat = AVPixelFormat(56);
    pub const AV_PIX_FMT_GRAY8A: AVPixelFormat = AVPixelFormat(56);
    pub const AV_PIX_FMT_BGR48BE: AVPixelFormat = AVPixelFormat(57);
    pub const AV_PIX_FMT_BGR48LE: AVPixelFormat = AVPixelFormat(58);
    pub const AV_PIX_FMT_YUV420P9BE: AVPixelFormat = AVPixelFormat(59);
    pub const AV_PIX_FMT_YUV420P9LE: AVPixelFormat = AVPixelFormat(60);
    pub const AV_PIX_FMT_YUV420P10BE: AVPixelFormat = AVPixelFormat(61);
    pub const AV_PIX_FMT_YUV420P10LE: AVPixelFormat = AVPixelFormat(62);
    pub const AV_PIX_FMT_YUV422P10BE: AVPixelFormat = AVPixelFormat(63);
    pub const AV_PIX_FMT_YUV422P10LE: AVPixelFormat = AVPixelFormat(64);
    pub const AV_PIX_FMT_YUV444P9BE: AVPixelFormat = AVPixelFormat(65);
    pub const AV_PIX_FMT_YUV444P9LE: AVPixelFormat = AVPixelFormat(66);
    pub const AV_PIX_FMT_YUV444P10BE: AVPixelFormat = AVPixelFormat(67);
    pub const AV_PIX_FMT_YUV444P10LE: AVPixelFormat = AVPixelFormat(68);
    pub const AV_PIX_FMT_YUV422P9BE: AVPixelFormat = AVPixelFormat(69);
    pub const AV_PIX_FMT_YUV422P9LE: AVPixelFormat = AVPixelFormat(70);
    pub const AV_PIX_FMT_GBRP: AVPixelFormat = AVPixelFormat(71);
    pub const AV_PIX_FMT_GBR24P: AVPixelFormat = AVPixelFormat(71);
    pub const AV_PIX_FMT_GBRP9BE: AVPixelFormat = AVPixelFormat(72);
    pub const AV_PIX_FMT_GBRP9LE: AVPixelFormat = AVPixelFormat(73);
    pub const AV_PIX_FMT_GBRP10BE: AVPixelFormat = AVPixelFormat(74);
    pub const AV_PIX_FMT_GBRP10LE: AVPixelFormat = AVPixelFormat(75);
    pub const AV_PIX_FMT_GBRP16BE: AVPixelFormat = AVPixelFormat(76);
    pub const AV_PIX_FMT_GBRP16LE: AVPixelFormat = AVPixelFormat(77);
    pub const AV_PIX_FMT_YUVA422P: AVPixelFormat = AVPixelFormat(78);
    pub const AV_PIX_FMT_YUVA444P: AVPixelFormat = AVPixelFormat(79);
    pub const AV_PIX_FMT_YUVA420P9BE: AVPixelFormat = AVPixelFormat(80);
    pub const AV_PIX_FMT_YUVA420P9LE: AVPixelFormat = AVPixelFormat(81);
    pub const AV_PIX_FMT_YUVA422P9BE: AVPixelFormat = AVPixelFormat(82);
    pub const AV_PIX_FMT_YUVA422P9LE: AVPixelFormat = AVPixelFormat(83);
    pub const AV_PIX_FMT_YUVA444P9BE: AVPixelFormat = AVPixelFormat(84);
    pub const AV_PIX_FMT_YUVA444P9LE: AVPixelFormat = AVPixelFormat(85);
    pub const AV_PIX_FMT_YUVA420P10BE: AVPixelFormat = AVPixelFormat(86);
    pub const AV_PIX_FMT_YUVA420P10LE: AVPixelFormat = AVPixelFormat(87);
    pub const AV_PIX_FMT_YUVA422P10BE: AVPixelFormat = AVPixelFormat(88);
    pub const AV_PIX_FMT_YUVA422P10LE: AVPixelFormat = AVPixelFormat(89);
    pub const AV_PIX_FMT_YUVA444P10BE: AVPixelFormat = AVPixelFormat(90);
    pub const AV_PIX_FMT_YUVA444P10LE: AVPixelFormat = AVPixelFormat(91);
    pub const AV_PIX_FMT_YUVA420P16BE: AVPixelFormat = AVPixelFormat(92);
    pub const AV_PIX_FMT_YUVA420P16LE: AVPixelFormat = AVPixelFormat(93);
    pub const AV_PIX_FMT_YUVA422P16BE: AVPixelFormat = AVPixelFormat(94);
    pub const AV_PIX_FMT_YUVA422P16LE: AVPixelFormat = AVPixelFormat(95);
    pub const AV_PIX_FMT_YUVA444P16BE: AVPixelFormat = AVPixelFormat(96);
    pub const AV_PIX_FMT_YUVA444P16LE: AVPixelFormat = AVPixelFormat(97);
    pub const AV_PIX_FMT_VDPAU: AVPixelFormat = AVPixelFormat(98);
    pub const AV_PIX_FMT_XYZ12LE: AVPixelFormat = AVPixelFormat(99);
    pub const AV_PIX_FMT_XYZ12BE: AVPixelFormat = AVPixelFormat(100);
    pub const AV_PIX_FMT_NV16: AVPixelFormat = AVPixelFormat(101);
    pub const AV_PIX_FMT_NV20LE: AVPixelFormat = AVPixelFormat(102);
    pub const AV_PIX_FMT_NV20BE: AVPixelFormat = AVPixelFormat(103);
    pub const AV_PIX_FMT_RGBA64BE: AVPixelFormat = AVPixelFormat(104);
    pub const AV_PIX_FMT_RGBA64LE: AVPixelFormat = AVPixelFormat(105);
    pub const AV_PIX_FMT_BGRA64BE: AVPixelFormat = AVPixelFormat(106);
    pub const AV_PIX_FMT_BGRA64LE: AVPixelFormat = AVPixelFormat(107);
    pub const AV_PIX_FMT_YVYU422: AVPixelFormat = AVPixelFormat(108);
    pub const AV_PIX_FMT_YA16BE: AVPixelFormat = AVPixelFormat(109);
    pub const AV_PIX_FMT_YA16LE: AVPixelFormat = AVPixelFormat(110);
    pub const AV_PIX_FMT_GBRAP: AVPixelFormat = AVPixelFormat(111);
    pub const AV_PIX_FMT_GBRAP16BE: AVPixelFormat = AVPixelFormat(112);
    pub const AV_PIX_FMT_GBRAP16LE: AVPixelFormat = AVPixelFormat(113);
    pub const AV_PIX_FMT_QSV: AVPixelFormat = AVPixelFormat(114);
    pub const AV_PIX_FMT_MMAL: AVPixelFormat = AVPixelFormat(115);
    pub const AV_PIX_FMT_D3D11VA_VLD: AVPixelFormat = AVPixelFormat(116);
    pub const AV_PIX_FMT_CUDA: AVPixelFormat = AVPixelFormat(117);
    pub const AV_PIX_FMT_0RGB: AVPixelFormat = AVPixelFormat(118);
    pub const AV_PIX_FMT_RGB0: AVPixelFormat = AVPixelFormat(119);
    pub const AV_PIX_FMT_0BGR: AVPixelFormat = AVPixelFormat(120);
    pub const AV_PIX_FMT_BGR0: AVPixelFormat = AVPixelFormat(121);
    pub const AV_PIX_FMT_YUV420P12BE: AVPixelFormat = AVPixelFormat(122);
    pub const AV_PIX_FMT_YUV420P12LE: AVPixelFormat = AVPixelFormat(123);
    pub const AV_PIX_FMT_YUV420P14BE: AVPixelFormat = AVPixelFormat(124);
    pub const AV_PIX_FMT_YUV420P14LE: AVPixelFormat = AVPixelFormat(125);
    pub const AV_PIX_FMT_YUV422P12BE: AVPixelFormat = AVPixelFormat(126);
    pub const AV_PIX_FMT_YUV422P12LE: AVPixelFormat = AVPixelFormat(127);
    pub const AV_PIX_FMT_YUV422P14BE: AVPixelFormat = AVPixelFormat(128);
    pub const AV_PIX_FMT_YUV422P14LE: AVPixelFormat = AVPixelFormat(129);
    pub const AV_PIX_FMT_YUV444P12BE: AVPixelFormat = AVPixelFormat(130);
    pub const AV_PIX_FMT_YUV444P12LE: AVPixelFormat = AVPixelFormat(131);
    pub const AV_PIX_FMT_YUV444P14BE: AVPixelFormat = AVPixelFormat(132);
    pub const AV_PIX_FMT_YUV444P14LE: AVPixelFormat = AVPixelFormat(133);
    pub const AV_PIX_FMT_GBRP12BE: AVPixelFormat = AVPixelFormat(134);
    pub const AV_PIX_FMT_GBRP12LE: AVPixelFormat = AVPixelFormat(135);
    pub const AV_PIX_FMT_GBRP14BE: AVPixelFormat = AVPixelFormat(136);
    pub const AV_PIX_FMT_GBRP14LE: AVPixelFormat = AVPixelFormat(137);
    pub const AV_PIX_FMT_YUVJ411P: AVPixelFormat = AVPixelFormat(138);
    pub const AV_PIX_FMT_BAYER_BGGR8: AVPixelFormat = AVPixelFormat(139);
    pub const AV_PIX_FMT_BAYER_RGGB8: AVPixelFormat = AVPixelFormat(140);
    pub const AV_PIX_FMT_BAYER_GBRG8: AVPixelFormat = AVPixelFormat(141);
    pub const AV_PIX_FMT_BAYER_GRBG8: AVPixelFormat = AVPixelFormat(142);
    pub const AV_PIX_FMT_BAYER_BGGR16LE: AVPixelFormat = AVPixelFormat(143);
    pub const AV_PIX_FMT_BAYER_BGGR16BE: AVPixelFormat = AVPixelFormat(144);
    pub const AV_PIX_FMT_BAYER_RGGB16LE: AVPixelFormat = AVPixelFormat(145);
    pub const AV_PIX_FMT_BAYER_RGGB16BE: AVPixelFormat = AVPixelFormat(146);
    pub const AV_PIX_FMT_BAYER_GBRG16LE: AVPixelFormat = AVPixelFormat(147);
    pub const AV_PIX_FMT_BAYER_GBRG16BE: AVPixelFormat = AVPixelFormat(148);
    pub const AV_PIX_FMT_BAYER_GRBG16LE: AVPixelFormat = AVPixelFormat(149);
    pub const AV_PIX_FMT_BAYER_GRBG16BE: AVPixelFormat = AVPixelFormat(150);
    pub const AV_PIX_FMT_YUV440P10LE: AVPixelFormat = AVPixelFormat(151);
    pub const AV_PIX_FMT_YUV440P10BE: AVPixelFormat = AVPixelFormat(152);
    pub const AV_PIX_FMT_YUV440P12LE: AVPixelFormat = AVPixelFormat(153);
    pub const AV_PIX_FMT_YUV440P12BE: AVPixelFormat = AVPixelFormat(154);
    pub const AV_PIX_FMT_AYUV64LE: AVPixelFormat = AVPixelFormat(155);
    pub const AV_PIX_FMT_AYUV64BE: AVPixelFormat = AVPixelFormat(156);
    pub const AV_PIX_FMT_VIDEOTOOLBOX: AVPixelFormat = AVPixelFormat(157);
    pub const AV_PIX_FMT_P010LE: AVPixelFormat = AVPixelFormat(158);
    pub const AV_PIX_FMT_P010BE: AVPixelFormat = AVPixelFormat(159);
    pub const AV_PIX_FMT_GBRAP12BE: AVPixelFormat = AVPixelFormat(160);
    pub const AV_PIX_FMT_GBRAP12LE: AVPixelFormat = AVPixelFormat(161);
    pub const AV_PIX_FMT_GBRAP10BE: AVPixelFormat = AVPixelFormat(162);
    pub const AV_PIX_FMT_GBRAP10LE: AVPixelFormat = AVPixelFormat(163);
    pub const AV_PIX_FMT_MEDIACODEC: AVPixelFormat = AVPixelFormat(164);
    pub const AV_PIX_FMT_GRAY12BE: AVPixelFormat = AVPixelFormat(165);
    pub const AV_PIX_FMT_GRAY12LE: AVPixelFormat = AVPixelFormat(166);
    pub const AV_PIX_FMT_GRAY10BE: AVPixelFormat = AVPixelFormat(167);
    pub const AV_PIX_FMT_GRAY10LE: AVPixelFormat = AVPixelFormat(168);
    pub const AV_PIX_FMT_P016LE: AVPixelFormat = AVPixelFormat(169);
    pub const AV_PIX_FMT_P016BE: AVPixelFormat = AVPixelFormat(170);
    pub const AV_PIX_FMT_D3D11: AVPixelFormat = AVPixelFormat(171);
    pub const AV_PIX_FMT_GRAY9BE: AVPixelFormat = AVPixelFormat(172);
    pub const AV_PIX_FMT_GRAY9LE: AVPixelFormat = AVPixelFormat(173);
    pub const AV_PIX_FMT_GBRPF32BE: AVPixelFormat = AVPixelFormat(174);
    pub const AV_PIX_FMT_GBRPF32LE: AVPixelFormat = AVPixelFormat(175);
    pub const AV_PIX_FMT_GBRAPF32BE: AVPixelFormat = AVPixelFormat(176);
    pub const AV_PIX_FMT_GBRAPF32LE: AVPixelFormat = AVPixelFormat(177);
    pub const AV_PIX_FMT_DRM_PRIME: AVPixelFormat = AVPixelFormat(178);
    pub const AV_PIX_FMT_OPENCL: AVPixelFormat = AVPixelFormat(179);
    pub const AV_PIX_FMT_GRAY14BE: AVPixelFormat = AVPixelFormat(180);
    pub const AV_PIX_FMT_GRAY14LE: AVPixelFormat = AVPixelFormat(181);
    pub const AV_PIX_FMT_GRAYF32BE: AVPixelFormat = AVPixelFormat(182);
    pub const AV_PIX_FMT_GRAYF32LE: AVPixelFormat = AVPixelFormat(183);
    pub const AV_PIX_FMT_YUVA422P12BE: AVPixelFormat = AVPixelFormat(184);
    pub const AV_PIX_FMT_YUVA422P12LE: AVPixelFormat = AVPixelFormat(185);
    pub const AV_PIX_FMT_YUVA444P12BE: AVPixelFormat = AVPixelFormat(186);
    pub const AV_PIX_FMT_YUVA444P12LE: AVPixelFormat = AVPixelFormat(187);
    pub const AV_PIX_FMT_NV24: AVPixelFormat = AVPixelFormat(188);
    pub const AV_PIX_FMT_NV42: AVPixelFormat = AVPixelFormat(189);
    pub const AV_PIX_FMT_VULKAN: AVPixelFormat = AVPixelFormat(190);
    pub const AV_PIX_FMT_Y210BE: AVPixelFormat = AVPixelFormat(191);
    pub const AV_PIX_FMT_Y210LE: AVPixelFormat = AVPixelFormat(192);
    pub const AV_PIX_FMT_X2RGB10LE: AVPixelFormat = AVPixelFormat(193);
    pub const AV_PIX_FMT_X2RGB10BE: AVPixelFormat = AVPixelFormat(194);
    pub const AV_PIX_FMT_X2BGR10LE: AVPixelFormat = AVPixelFormat(195);
    pub const AV_PIX_FMT_X2BGR10BE: AVPixelFormat = AVPixelFormat(196);
    pub const AV_PIX_FMT_P210BE: AVPixelFormat = AVPixelFormat(197);
    pub const AV_PIX_FMT_P210LE: AVPixelFormat = AVPixelFormat(198);
    pub const AV_PIX_FMT_P410BE: AVPixelFormat = AVPixelFormat(199);
    pub const AV_PIX_FMT_P410LE: AVPixelFormat = AVPixelFormat(200);
    pub const AV_PIX_FMT_P216BE: AVPixelFormat = AVPixelFormat(201);
    pub const AV_PIX_FMT_P216LE: AVPixelFormat = AVPixelFormat(202);
    pub const AV_PIX_FMT_P416BE: AVPixelFormat = AVPixelFormat(203);
    pub const AV_PIX_FMT_P416LE: AVPixelFormat = AVPixelFormat(204);
    pub const AV_PIX_FMT_VUYA: AVPixelFormat = AVPixelFormat(205);
    pub const AV_PIX_FMT_RGBAF16BE: AVPixelFormat = AVPixelFormat(206);
    pub const AV_PIX_FMT_RGBAF16LE: AVPixelFormat = AVPixelFormat(207);
    pub const AV_PIX_FMT_VUYX: AVPixelFormat = AVPixelFormat(208);
    pub const AV_PIX_FMT_P012LE: AVPixelFormat = AVPixelFormat(209);
    pub const AV_PIX_FMT_P012BE: AVPixelFormat = AVPixelFormat(210);
    pub const AV_PIX_FMT_Y212BE: AVPixelFormat = AVPixelFormat(211);
    pub const AV_PIX_FMT_Y212LE: AVPixelFormat = AVPixelFormat(212);
    pub const AV_PIX_FMT_XV30BE: AVPixelFormat = AVPixelFormat(213);
    pub const AV_PIX_FMT_XV30LE: AVPixelFormat = AVPixelFormat(214);
    pub const AV_PIX_FMT_XV36BE: AVPixelFormat = AVPixelFormat(215);
    pub const AV_PIX_FMT_XV36LE: AVPixelFormat = AVPixelFormat(216);
    pub const AV_PIX_FMT_RGBF32BE: AVPixelFormat = AVPixelFormat(217);
    pub const AV_PIX_FMT_RGBF32LE: AVPixelFormat = AVPixelFormat(218);
    pub const AV_PIX_FMT_RGBAF32BE: AVPixelFormat = AVPixelFormat(219);
    pub const AV_PIX_FMT_RGBAF32LE: AVPixelFormat = AVPixelFormat(220);
    pub const AV_PIX_FMT_P212BE: AVPixelFormat = AVPixelFormat(221);
    pub const AV_PIX_FMT_P212LE: AVPixelFormat = AVPixelFormat(222);
    pub const AV_PIX_FMT_P412BE: AVPixelFormat = AVPixelFormat(223);
    pub const AV_PIX_FMT_P412LE: AVPixelFormat = AVPixelFormat(224);
    pub const AV_PIX_FMT_GBRAP14BE: AVPixelFormat = AVPixelFormat(225);
    pub const AV_PIX_FMT_GBRAP14LE: AVPixelFormat = AVPixelFormat(226);
    pub const AV_PIX_FMT_D3D12: AVPixelFormat = AVPixelFormat(227);
    pub const AV_PIX_FMT_AYUV: AVPixelFormat = AVPixelFormat(228);
    pub const AV_PIX_FMT_UYVA: AVPixelFormat = AVPixelFormat(229);
    pub const AV_PIX_FMT_VYU444: AVPixelFormat = AVPixelFormat(230);
    pub const AV_PIX_FMT_V30XBE: AVPixelFormat = AVPixelFormat(231);
    pub const AV_PIX_FMT_V30XLE: AVPixelFormat = AVPixelFormat(232);
    pub const AV_PIX_FMT_RGBF16BE: AVPixelFormat = AVPixelFormat(233);
    pub const AV_PIX_FMT_RGBF16LE: AVPixelFormat = AVPixelFormat(234);
    pub const AV_PIX_FMT_RGBA128BE: AVPixelFormat = AVPixelFormat(235);
    pub const AV_PIX_FMT_RGBA128LE: AVPixelFormat = AVPixelFormat(236);
    pub const AV_PIX_FMT_RGB96BE: AVPixelFormat = AVPixelFormat(237);
    pub const AV_PIX_FMT_RGB96LE: AVPixelFormat = AVPixelFormat(238);
    pub const AV_PIX_FMT_Y216BE: AVPixelFormat = AVPixelFormat(239);
    pub const AV_PIX_FMT_Y216LE: AVPixelFormat = AVPixelFormat(240);
    pub const AV_PIX_FMT_XV48BE: AVPixelFormat = AVPixelFormat(241);
    pub const AV_PIX_FMT_XV48LE: AVPixelFormat = AVPixelFormat(242);
    pub const AV_PIX_FMT_GBRPF16BE: AVPixelFormat = AVPixelFormat(243);
    pub const AV_PIX_FMT_GBRPF16LE: AVPixelFormat = AVPixelFormat(244);
    pub const AV_PIX_FMT_GBRAPF16BE: AVPixelFormat = AVPixelFormat(245);
    pub const AV_PIX_FMT_GBRAPF16LE: AVPixelFormat = AVPixelFormat(246);
    pub const AV_PIX_FMT_GRAYF16BE: AVPixelFormat = AVPixelFormat(247);
    pub const AV_PIX_FMT_GRAYF16LE: AVPixelFormat = AVPixelFormat(248);
    pub const AV_PIX_FMT_AMF_SURFACE: AVPixelFormat = AVPixelFormat(249);
    pub const AV_PIX_FMT_GRAY32BE: AVPixelFormat = AVPixelFormat(250);
    pub const AV_PIX_FMT_GRAY32LE: AVPixelFormat = AVPixelFormat(251);
    pub const AV_PIX_FMT_YAF32BE: AVPixelFormat = AVPixelFormat(252);
    pub const AV_PIX_FMT_YAF32LE: AVPixelFormat = AVPixelFormat(253);
    pub const AV_PIX_FMT_YAF16BE: AVPixelFormat = AVPixelFormat(254);
    pub const AV_PIX_FMT_YAF16LE: AVPixelFormat = AVPixelFormat(255);
    pub const AV_PIX_FMT_GBRAP32BE: AVPixelFormat = AVPixelFormat(256);
    pub const AV_PIX_FMT_GBRAP32LE: AVPixelFormat = AVPixelFormat(257);
    pub const AV_PIX_FMT_YUV444P10MSBBE: AVPixelFormat = AVPixelFormat(258);
    pub const AV_PIX_FMT_YUV444P10MSBLE: AVPixelFormat = AVPixelFormat(259);
    pub const AV_PIX_FMT_YUV444P12MSBBE: AVPixelFormat = AVPixelFormat(260);
    pub const AV_PIX_FMT_YUV444P12MSBLE: AVPixelFormat = AVPixelFormat(261);
    pub const AV_PIX_FMT_GBRP10MSBBE: AVPixelFormat = AVPixelFormat(262);
    pub const AV_PIX_FMT_GBRP10MSBLE: AVPixelFormat = AVPixelFormat(263);
    pub const AV_PIX_FMT_GBRP12MSBBE: AVPixelFormat = AVPixelFormat(264);
    pub const AV_PIX_FMT_GBRP12MSBLE: AVPixelFormat = AVPixelFormat(265);
    pub const AV_PIX_FMT_OHCODEC: AVPixelFormat = AVPixelFormat(266);
    pub const AV_PIX_FMT_NB: AVPixelFormat = AVPixelFormat(267);
}
#[repr(transparent)]
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct AVPixelFormat(pub ::std::os::raw::c_int);
impl AVColorPrimaries {
    pub const AVCOL_PRI_RESERVED0: AVColorPrimaries = AVColorPrimaries(0);
    pub const AVCOL_PRI_BT709: AVColorPrimaries = AVColorPrimaries(1);
    pub const AVCOL_PRI_UNSPECIFIED: AVColorPrimaries = AVColorPrimaries(2);
    pub const AVCOL_PRI_RESERVED: AVColorPrimaries = AVColorPrimaries(3);
    pub const AVCOL_PRI_BT470M: AVColorPrimaries = AVColorPrimaries(4);
    pub const AVCOL_PRI_BT470BG: AVColorPrimaries = AVColorPrimaries(5);
    pub const AVCOL_PRI_SMPTE170M: AVColorPrimaries = AVColorPrimaries(6);
    pub const AVCOL_PRI_SMPTE240M: AVColorPrimaries = AVColorPrimaries(7);
    pub const AVCOL_PRI_FILM: AVColorPrimaries = AVColorPrimaries(8);
    pub const AVCOL_PRI_BT2020: AVColorPrimaries = AVColorPrimaries(9);
    pub const AVCOL_PRI_SMPTE428: AVColorPrimaries = AVColorPrimaries(10);
    pub const AVCOL_PRI_SMPTEST428_1: AVColorPrimaries = AVColorPrimaries(10);
    pub const AVCOL_PRI_SMPTE431: AVColorPrimaries = AVColorPrimaries(11);
    pub const AVCOL_PRI_SMPTE432: AVColorPrimaries = AVColorPrimaries(12);
    pub const AVCOL_PRI_EBU3213: AVColorPrimaries = AVColorPrimaries(22);
    pub const AVCOL_PRI_JEDEC_P22: AVColorPrimaries = AVColorPrimaries(22);
    pub const AVCOL_PRI_NB: AVColorPrimaries = AVColorPrimaries(23);
}
#[repr(transparent)]
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct AVColorPrimaries(pub ::std::os::raw::c_int);
impl AVColorTransferCharacteristic {
    pub const AVCOL_TRC_RESERVED0: AVColorTransferCharacteristic = AVColorTransferCharacteristic(0);
    pub const AVCOL_TRC_BT709: AVColorTransferCharacteristic = AVColorTransferCharacteristic(1);
    pub const AVCOL_TRC_UNSPECIFIED: AVColorTransferCharacteristic =
        AVColorTransferCharacteristic(2);
    pub const AVCOL_TRC_RESERVED: AVColorTransferCharacteristic = AVColorTransferCharacteristic(3);
    pub const AVCOL_TRC_GAMMA22: AVColorTransferCharacteristic = AVColorTransferCharacteristic(4);
    pub const AVCOL_TRC_GAMMA28: AVColorTransferCharacteristic = AVColorTransferCharacteristic(5);
    pub const AVCOL_TRC_SMPTE170M: AVColorTransferCharacteristic = AVColorTransferCharacteristic(6);
    pub const AVCOL_TRC_SMPTE240M: AVColorTransferCharacteristic = AVColorTransferCharacteristic(7);
    pub const AVCOL_TRC_LINEAR: AVColorTransferCharacteristic = AVColorTransferCharacteristic(8);
    pub const AVCOL_TRC_LOG: AVColorTransferCharacteristic = AVColorTransferCharacteristic(9);
    pub const AVCOL_TRC_LOG_SQRT: AVColorTransferCharacteristic = AVColorTransferCharacteristic(10);
    pub const AVCOL_TRC_IEC61966_2_4: AVColorTransferCharacteristic =
        AVColorTransferCharacteristic(11);
    pub const AVCOL_TRC_BT1361_ECG: AVColorTransferCharacteristic =
        AVColorTransferCharacteristic(12);
    pub const AVCOL_TRC_IEC61966_2_1: AVColorTransferCharacteristic =
        AVColorTransferCharacteristic(13);
    pub const AVCOL_TRC_BT2020_10: AVColorTransferCharacteristic =
        AVColorTransferCharacteristic(14);
    pub const AVCOL_TRC_BT2020_12: AVColorTransferCharacteristic =
        AVColorTransferCharacteristic(15);
    pub const AVCOL_TRC_SMPTE2084: AVColorTransferCharacteristic =
        AVColorTransferCharacteristic(16);
    pub const AVCOL_TRC_SMPTEST2084: AVColorTransferCharacteristic =
        AVColorTransferCharacteristic(16);
    pub const AVCOL_TRC_SMPTE428: AVColorTransferCharacteristic = AVColorTransferCharacteristic(17);
    pub const AVCOL_TRC_SMPTEST428_1: AVColorTransferCharacteristic =
        AVColorTransferCharacteristic(17);
    pub const AVCOL_TRC_ARIB_STD_B67: AVColorTransferCharacteristic =
        AVColorTransferCharacteristic(18);
    pub const AVCOL_TRC_NB: AVColorTransferCharacteristic = AVColorTransferCharacteristic(19);
}
#[repr(transparent)]
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct AVColorTransferCharacteristic(pub ::std::os::raw::c_int);
impl AVColorSpace {
    pub const AVCOL_SPC_RGB: AVColorSpace = AVColorSpace(0);
    pub const AVCOL_SPC_BT709: AVColorSpace = AVColorSpace(1);
    pub const AVCOL_SPC_UNSPECIFIED: AVColorSpace = AVColorSpace(2);
    pub const AVCOL_SPC_RESERVED: AVColorSpace = AVColorSpace(3);
    pub const AVCOL_SPC_FCC: AVColorSpace = AVColorSpace(4);
    pub const AVCOL_SPC_BT470BG: AVColorSpace = AVColorSpace(5);
    pub const AVCOL_SPC_SMPTE170M: AVColorSpace = AVColorSpace(6);
    pub const AVCOL_SPC_SMPTE240M: AVColorSpace = AVColorSpace(7);
    pub const AVCOL_SPC_YCGCO: AVColorSpace = AVColorSpace(8);
    pub const AVCOL_SPC_YCOCG: AVColorSpace = AVColorSpace(8);
    pub const AVCOL_SPC_BT2020_NCL: AVColorSpace = AVColorSpace(9);
    pub const AVCOL_SPC_BT2020_CL: AVColorSpace = AVColorSpace(10);
    pub const AVCOL_SPC_SMPTE2085: AVColorSpace = AVColorSpace(11);
    pub const AVCOL_SPC_CHROMA_DERIVED_NCL: AVColorSpace = AVColorSpace(12);
    pub const AVCOL_SPC_CHROMA_DERIVED_CL: AVColorSpace = AVColorSpace(13);
    pub const AVCOL_SPC_ICTCP: AVColorSpace = AVColorSpace(14);
    pub const AVCOL_SPC_IPT_C2: AVColorSpace = AVColorSpace(15);
    pub const AVCOL_SPC_YCGCO_RE: AVColorSpace = AVColorSpace(16);
    pub const AVCOL_SPC_YCGCO_RO: AVColorSpace = AVColorSpace(17);
    pub const AVCOL_SPC_NB: AVColorSpace = AVColorSpace(18);
}
#[repr(transparent)]
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct AVColorSpace(pub ::std::os::raw::c_int);
impl AVColorRange {
    pub const AVCOL_RANGE_UNSPECIFIED: AVColorRange = AVColorRange(0);
    pub const AVCOL_RANGE_MPEG: AVColorRange = AVColorRange(1);
    pub const AVCOL_RANGE_JPEG: AVColorRange = AVColorRange(2);
    pub const AVCOL_RANGE_NB: AVColorRange = AVColorRange(3);
}
#[repr(transparent)]
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct AVColorRange(pub ::std::os::raw::c_int);
impl AVChromaLocation {
    pub const AVCHROMA_LOC_UNSPECIFIED: AVChromaLocation = AVChromaLocation(0);
    pub const AVCHROMA_LOC_LEFT: AVChromaLocation = AVChromaLocation(1);
    pub const AVCHROMA_LOC_CENTER: AVChromaLocation = AVChromaLocation(2);
    pub const AVCHROMA_LOC_TOPLEFT: AVChromaLocation = AVChromaLocation(3);
    pub const AVCHROMA_LOC_TOP: AVChromaLocation = AVChromaLocation(4);
    pub const AVCHROMA_LOC_BOTTOMLEFT: AVChromaLocation = AVChromaLocation(5);
    pub const AVCHROMA_LOC_BOTTOM: AVChromaLocation = AVChromaLocation(6);
    pub const AVCHROMA_LOC_NB: AVChromaLocation = AVChromaLocation(7);
}
#[repr(transparent)]
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct AVChromaLocation(pub ::std::os::raw::c_int);
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct AVBuffer {
    _unused: [u8; 0],
}
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct AVBufferRef {
    pub buffer: *mut AVBuffer,
    pub data: *mut u8,
    pub size: usize,
}
impl Default for AVBufferRef {
    fn default() -> Self {
        let mut s = ::std::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            ::std::ptr::write_bytes(s.as_mut_ptr(), 0, 1);
            s.assume_init()
        }
    }
}
unsafe extern "C" {
    pub fn av_buffer_ref(buf: *const AVBufferRef) -> *mut AVBufferRef;
}
unsafe extern "C" {
    pub fn av_buffer_unref(buf: *mut *mut AVBufferRef);
}
impl AVChannel {
    pub const AV_CHAN_NONE: AVChannel = AVChannel(-1);
    pub const AV_CHAN_FRONT_LEFT: AVChannel = AVChannel(0);
    pub const AV_CHAN_FRONT_RIGHT: AVChannel = AVChannel(1);
    pub const AV_CHAN_FRONT_CENTER: AVChannel = AVChannel(2);
    pub const AV_CHAN_LOW_FREQUENCY: AVChannel = AVChannel(3);
    pub const AV_CHAN_BACK_LEFT: AVChannel = AVChannel(4);
    pub const AV_CHAN_BACK_RIGHT: AVChannel = AVChannel(5);
    pub const AV_CHAN_FRONT_LEFT_OF_CENTER: AVChannel = AVChannel(6);
    pub const AV_CHAN_FRONT_RIGHT_OF_CENTER: AVChannel = AVChannel(7);
    pub const AV_CHAN_BACK_CENTER: AVChannel = AVChannel(8);
    pub const AV_CHAN_SIDE_LEFT: AVChannel = AVChannel(9);
    pub const AV_CHAN_SIDE_RIGHT: AVChannel = AVChannel(10);
    pub const AV_CHAN_TOP_CENTER: AVChannel = AVChannel(11);
    pub const AV_CHAN_TOP_FRONT_LEFT: AVChannel = AVChannel(12);
    pub const AV_CHAN_TOP_FRONT_CENTER: AVChannel = AVChannel(13);
    pub const AV_CHAN_TOP_FRONT_RIGHT: AVChannel = AVChannel(14);
    pub const AV_CHAN_TOP_BACK_LEFT: AVChannel = AVChannel(15);
    pub const AV_CHAN_TOP_BACK_CENTER: AVChannel = AVChannel(16);
    pub const AV_CHAN_TOP_BACK_RIGHT: AVChannel = AVChannel(17);
    pub const AV_CHAN_STEREO_LEFT: AVChannel = AVChannel(29);
    pub const AV_CHAN_STEREO_RIGHT: AVChannel = AVChannel(30);
    pub const AV_CHAN_WIDE_LEFT: AVChannel = AVChannel(31);
    pub const AV_CHAN_WIDE_RIGHT: AVChannel = AVChannel(32);
    pub const AV_CHAN_SURROUND_DIRECT_LEFT: AVChannel = AVChannel(33);
    pub const AV_CHAN_SURROUND_DIRECT_RIGHT: AVChannel = AVChannel(34);
    pub const AV_CHAN_LOW_FREQUENCY_2: AVChannel = AVChannel(35);
    pub const AV_CHAN_TOP_SIDE_LEFT: AVChannel = AVChannel(36);
    pub const AV_CHAN_TOP_SIDE_RIGHT: AVChannel = AVChannel(37);
    pub const AV_CHAN_BOTTOM_FRONT_CENTER: AVChannel = AVChannel(38);
    pub const AV_CHAN_BOTTOM_FRONT_LEFT: AVChannel = AVChannel(39);
    pub const AV_CHAN_BOTTOM_FRONT_RIGHT: AVChannel = AVChannel(40);
    pub const AV_CHAN_SIDE_SURROUND_LEFT: AVChannel = AVChannel(41);
    pub const AV_CHAN_SIDE_SURROUND_RIGHT: AVChannel = AVChannel(42);
    pub const AV_CHAN_TOP_SURROUND_LEFT: AVChannel = AVChannel(43);
    pub const AV_CHAN_TOP_SURROUND_RIGHT: AVChannel = AVChannel(44);
    pub const AV_CHAN_BINAURAL_LEFT: AVChannel = AVChannel(61);
    pub const AV_CHAN_BINAURAL_RIGHT: AVChannel = AVChannel(62);
    pub const AV_CHAN_UNUSED: AVChannel = AVChannel(512);
    pub const AV_CHAN_UNKNOWN: AVChannel = AVChannel(768);
    pub const AV_CHAN_AMBISONIC_BASE: AVChannel = AVChannel(1024);
    pub const AV_CHAN_AMBISONIC_END: AVChannel = AVChannel(2047);
}
#[repr(transparent)]
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct AVChannel(pub ::std::os::raw::c_int);
impl AVChannelOrder {
    pub const AV_CHANNEL_ORDER_UNSPEC: AVChannelOrder = AVChannelOrder(0);
    pub const AV_CHANNEL_ORDER_NATIVE: AVChannelOrder = AVChannelOrder(1);
    pub const AV_CHANNEL_ORDER_CUSTOM: AVChannelOrder = AVChannelOrder(2);
    pub const AV_CHANNEL_ORDER_AMBISONIC: AVChannelOrder = AVChannelOrder(3);
    pub const FF_CHANNEL_ORDER_NB: AVChannelOrder = AVChannelOrder(4);
}
#[repr(transparent)]
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct AVChannelOrder(pub ::std::os::raw::c_int);
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct AVChannelCustom {
    pub id: AVChannel,
    pub name: [::std::os::raw::c_char; 16usize],
    pub opaque: *mut ::std::os::raw::c_void,
}
impl Default for AVChannelCustom {
    fn default() -> Self {
        let mut s = ::std::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            ::std::ptr::write_bytes(s.as_mut_ptr(), 0, 1);
            s.assume_init()
        }
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct AVChannelLayout {
    pub order: AVChannelOrder,
    pub nb_channels: ::std::os::raw::c_int,
    pub u: AVChannelLayout__bindgen_ty_1,
    pub opaque: *mut ::std::os::raw::c_void,
}
#[repr(C)]
#[derive(Copy, Clone)]
pub union AVChannelLayout__bindgen_ty_1 {
    pub mask: u64,
    pub map: *mut AVChannelCustom,
}
impl Default for AVChannelLayout__bindgen_ty_1 {
    fn default() -> Self {
        let mut s = ::std::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            ::std::ptr::write_bytes(s.as_mut_ptr(), 0, 1);
            s.assume_init()
        }
    }
}
impl Default for AVChannelLayout {
    fn default() -> Self {
        let mut s = ::std::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            ::std::ptr::write_bytes(s.as_mut_ptr(), 0, 1);
            s.assume_init()
        }
    }
}
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct AVDictionary {
    _unused: [u8; 0],
}
impl AVFrameSideDataType {
    pub const AV_FRAME_DATA_PANSCAN: AVFrameSideDataType = AVFrameSideDataType(0);
    pub const AV_FRAME_DATA_A53_CC: AVFrameSideDataType = AVFrameSideDataType(1);
    pub const AV_FRAME_DATA_STEREO3D: AVFrameSideDataType = AVFrameSideDataType(2);
    pub const AV_FRAME_DATA_MATRIXENCODING: AVFrameSideDataType = AVFrameSideDataType(3);
    pub const AV_FRAME_DATA_DOWNMIX_INFO: AVFrameSideDataType = AVFrameSideDataType(4);
    pub const AV_FRAME_DATA_REPLAYGAIN: AVFrameSideDataType = AVFrameSideDataType(5);
    pub const AV_FRAME_DATA_DISPLAYMATRIX: AVFrameSideDataType = AVFrameSideDataType(6);
    pub const AV_FRAME_DATA_AFD: AVFrameSideDataType = AVFrameSideDataType(7);
    pub const AV_FRAME_DATA_MOTION_VECTORS: AVFrameSideDataType = AVFrameSideDataType(8);
    pub const AV_FRAME_DATA_SKIP_SAMPLES: AVFrameSideDataType = AVFrameSideDataType(9);
    pub const AV_FRAME_DATA_AUDIO_SERVICE_TYPE: AVFrameSideDataType = AVFrameSideDataType(10);
    pub const AV_FRAME_DATA_MASTERING_DISPLAY_METADATA: AVFrameSideDataType =
        AVFrameSideDataType(11);
    pub const AV_FRAME_DATA_GOP_TIMECODE: AVFrameSideDataType = AVFrameSideDataType(12);
    pub const AV_FRAME_DATA_SPHERICAL: AVFrameSideDataType = AVFrameSideDataType(13);
    pub const AV_FRAME_DATA_CONTENT_LIGHT_LEVEL: AVFrameSideDataType = AVFrameSideDataType(14);
    pub const AV_FRAME_DATA_ICC_PROFILE: AVFrameSideDataType = AVFrameSideDataType(15);
    pub const AV_FRAME_DATA_S12M_TIMECODE: AVFrameSideDataType = AVFrameSideDataType(16);
    pub const AV_FRAME_DATA_DYNAMIC_HDR_PLUS: AVFrameSideDataType = AVFrameSideDataType(17);
    pub const AV_FRAME_DATA_REGIONS_OF_INTEREST: AVFrameSideDataType = AVFrameSideDataType(18);
    pub const AV_FRAME_DATA_VIDEO_ENC_PARAMS: AVFrameSideDataType = AVFrameSideDataType(19);
    pub const AV_FRAME_DATA_SEI_UNREGISTERED: AVFrameSideDataType = AVFrameSideDataType(20);
    pub const AV_FRAME_DATA_FILM_GRAIN_PARAMS: AVFrameSideDataType = AVFrameSideDataType(21);
    pub const AV_FRAME_DATA_DETECTION_BBOXES: AVFrameSideDataType = AVFrameSideDataType(22);
    pub const AV_FRAME_DATA_DOVI_RPU_BUFFER: AVFrameSideDataType = AVFrameSideDataType(23);
    pub const AV_FRAME_DATA_DOVI_METADATA: AVFrameSideDataType = AVFrameSideDataType(24);
    pub const AV_FRAME_DATA_DYNAMIC_HDR_VIVID: AVFrameSideDataType = AVFrameSideDataType(25);
    pub const AV_FRAME_DATA_AMBIENT_VIEWING_ENVIRONMENT: AVFrameSideDataType =
        AVFrameSideDataType(26);
    pub const AV_FRAME_DATA_VIDEO_HINT: AVFrameSideDataType = AVFrameSideDataType(27);
    pub const AV_FRAME_DATA_LCEVC: AVFrameSideDataType = AVFrameSideDataType(28);
    pub const AV_FRAME_DATA_VIEW_ID: AVFrameSideDataType = AVFrameSideDataType(29);
    pub const AV_FRAME_DATA_3D_REFERENCE_DISPLAYS: AVFrameSideDataType = AVFrameSideDataType(30);
}
#[repr(transparent)]
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct AVFrameSideDataType(pub ::std::os::raw::c_int);
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct AVFrameSideData {
    pub type_: AVFrameSideDataType,
    pub data: *mut u8,
    pub size: usize,
    pub metadata: *mut AVDictionary,
    pub buf: *mut AVBufferRef,
}
impl Default for AVFrameSideData {
    fn default() -> Self {
        let mut s = ::std::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            ::std::ptr::write_bytes(s.as_mut_ptr(), 0, 1);
            s.assume_init()
        }
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct AVFrame {
    pub data: [*mut u8; 8usize],
    pub linesize: [::std::os::raw::c_int; 8usize],
    pub extended_data: *mut *mut u8,
    pub width: ::std::os::raw::c_int,
    pub height: ::std::os::raw::c_int,
    pub nb_samples: ::std::os::raw::c_int,
    pub format: ::std::os::raw::c_int,
    pub pict_type: AVPictureType,
    pub sample_aspect_ratio: AVRational,
    pub pts: i64,
    pub pkt_dts: i64,
    pub time_base: AVRational,
    pub quality: ::std::os::raw::c_int,
    pub opaque: *mut ::std::os::raw::c_void,
    pub repeat_pict: ::std::os::raw::c_int,
    pub sample_rate: ::std::os::raw::c_int,
    pub buf: [*mut AVBufferRef; 8usize],
    pub extended_buf: *mut *mut AVBufferRef,
    pub nb_extended_buf: ::std::os::raw::c_int,
    pub side_data: *mut *mut AVFrameSideData,
    pub nb_side_data: ::std::os::raw::c_int,
    pub flags: ::std::os::raw::c_int,
    pub color_range: AVColorRange,
    pub color_primaries: AVColorPrimaries,
    pub color_trc: AVColorTransferCharacteristic,
    pub colorspace: AVColorSpace,
    pub chroma_location: AVChromaLocation,
    pub best_effort_timestamp: i64,
    pub metadata: *mut AVDictionary,
    pub decode_error_flags: ::std::os::raw::c_int,
    pub hw_frames_ctx: *mut AVBufferRef,
    pub opaque_ref: *mut AVBufferRef,
    pub crop_top: usize,
    pub crop_bottom: usize,
    pub crop_left: usize,
    pub crop_right: usize,
    pub private_ref: *mut ::std::os::raw::c_void,
    pub ch_layout: AVChannelLayout,
    pub duration: i64,
}
impl Default for AVFrame {
    fn default() -> Self {
        let mut s = ::std::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            ::std::ptr::write_bytes(s.as_mut_ptr(), 0, 1);
            s.assume_init()
        }
    }
}
unsafe extern "C" {
    pub fn av_frame_alloc() -> *mut AVFrame;
}
unsafe extern "C" {
    pub fn av_frame_free(frame: *mut *mut AVFrame);
}
unsafe extern "C" {
    pub fn av_frame_unref(frame: *mut AVFrame);
}
impl AVHWDeviceType {
    pub const AV_HWDEVICE_TYPE_NONE: AVHWDeviceType = AVHWDeviceType(0);
    pub const AV_HWDEVICE_TYPE_VDPAU: AVHWDeviceType = AVHWDeviceType(1);
    pub const AV_HWDEVICE_TYPE_CUDA: AVHWDeviceType = AVHWDeviceType(2);
    pub const AV_HWDEVICE_TYPE_VAAPI: AVHWDeviceType = AVHWDeviceType(3);
    pub const AV_HWDEVICE_TYPE_DXVA2: AVHWDeviceType = AVHWDeviceType(4);
    pub const AV_HWDEVICE_TYPE_QSV: AVHWDeviceType = AVHWDeviceType(5);
    pub const AV_HWDEVICE_TYPE_VIDEOTOOLBOX: AVHWDeviceType = AVHWDeviceType(6);
    pub const AV_HWDEVICE_TYPE_D3D11VA: AVHWDeviceType = AVHWDeviceType(7);
    pub const AV_HWDEVICE_TYPE_DRM: AVHWDeviceType = AVHWDeviceType(8);
    pub const AV_HWDEVICE_TYPE_OPENCL: AVHWDeviceType = AVHWDeviceType(9);
    pub const AV_HWDEVICE_TYPE_MEDIACODEC: AVHWDeviceType = AVHWDeviceType(10);
    pub const AV_HWDEVICE_TYPE_VULKAN: AVHWDeviceType = AVHWDeviceType(11);
    pub const AV_HWDEVICE_TYPE_D3D12VA: AVHWDeviceType = AVHWDeviceType(12);
    pub const AV_HWDEVICE_TYPE_AMF: AVHWDeviceType = AVHWDeviceType(13);
    pub const AV_HWDEVICE_TYPE_OHCODEC: AVHWDeviceType = AVHWDeviceType(14);
}
#[repr(transparent)]
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct AVHWDeviceType(pub ::std::os::raw::c_int);
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct AVHWDeviceContext {
    pub av_class: *const AVClass,
    pub type_: AVHWDeviceType,
    pub hwctx: *mut ::std::os::raw::c_void,
    pub free: ::std::option::Option<unsafe extern "C" fn(ctx: *mut AVHWDeviceContext)>,
    pub user_opaque: *mut ::std::os::raw::c_void,
}
impl Default for AVHWDeviceContext {
    fn default() -> Self {
        let mut s = ::std::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            ::std::ptr::write_bytes(s.as_mut_ptr(), 0, 1);
            s.assume_init()
        }
    }
}
unsafe extern "C" {
    pub fn av_hwdevice_ctx_alloc(type_: AVHWDeviceType) -> *mut AVBufferRef;
}
unsafe extern "C" {
    pub fn av_hwdevice_ctx_init(ref_: *mut AVBufferRef) -> ::std::os::raw::c_int;
}
unsafe extern "C" {
    pub fn av_hwdevice_ctx_create(
        device_ctx: *mut *mut AVBufferRef,
        type_: AVHWDeviceType,
        device: *const ::std::os::raw::c_char,
        opts: *mut AVDictionary,
        flags: ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int;
}
unsafe extern "C" {
    pub fn av_hwframe_transfer_data(
        dst: *mut AVFrame,
        src: *const AVFrame,
        flags: ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int;
}
impl AVCodecID {
    pub const AV_CODEC_ID_NONE: AVCodecID = AVCodecID(0);
    pub const AV_CODEC_ID_MPEG1VIDEO: AVCodecID = AVCodecID(1);
    pub const AV_CODEC_ID_MPEG2VIDEO: AVCodecID = AVCodecID(2);
    pub const AV_CODEC_ID_H261: AVCodecID = AVCodecID(3);
    pub const AV_CODEC_ID_H263: AVCodecID = AVCodecID(4);
    pub const AV_CODEC_ID_RV10: AVCodecID = AVCodecID(5);
    pub const AV_CODEC_ID_RV20: AVCodecID = AVCodecID(6);
    pub const AV_CODEC_ID_MJPEG: AVCodecID = AVCodecID(7);
    pub const AV_CODEC_ID_MJPEGB: AVCodecID = AVCodecID(8);
    pub const AV_CODEC_ID_LJPEG: AVCodecID = AVCodecID(9);
    pub const AV_CODEC_ID_SP5X: AVCodecID = AVCodecID(10);
    pub const AV_CODEC_ID_JPEGLS: AVCodecID = AVCodecID(11);
    pub const AV_CODEC_ID_MPEG4: AVCodecID = AVCodecID(12);
    pub const AV_CODEC_ID_RAWVIDEO: AVCodecID = AVCodecID(13);
    pub const AV_CODEC_ID_MSMPEG4V1: AVCodecID = AVCodecID(14);
    pub const AV_CODEC_ID_MSMPEG4V2: AVCodecID = AVCodecID(15);
    pub const AV_CODEC_ID_MSMPEG4V3: AVCodecID = AVCodecID(16);
    pub const AV_CODEC_ID_WMV1: AVCodecID = AVCodecID(17);
    pub const AV_CODEC_ID_WMV2: AVCodecID = AVCodecID(18);
    pub const AV_CODEC_ID_H263P: AVCodecID = AVCodecID(19);
    pub const AV_CODEC_ID_H263I: AVCodecID = AVCodecID(20);
    pub const AV_CODEC_ID_FLV1: AVCodecID = AVCodecID(21);
    pub const AV_CODEC_ID_SVQ1: AVCodecID = AVCodecID(22);
    pub const AV_CODEC_ID_SVQ3: AVCodecID = AVCodecID(23);
    pub const AV_CODEC_ID_DVVIDEO: AVCodecID = AVCodecID(24);
    pub const AV_CODEC_ID_HUFFYUV: AVCodecID = AVCodecID(25);
    pub const AV_CODEC_ID_CYUV: AVCodecID = AVCodecID(26);
    pub const AV_CODEC_ID_H264: AVCodecID = AVCodecID(27);
    pub const AV_CODEC_ID_INDEO3: AVCodecID = AVCodecID(28);
    pub const AV_CODEC_ID_VP3: AVCodecID = AVCodecID(29);
    pub const AV_CODEC_ID_THEORA: AVCodecID = AVCodecID(30);
    pub const AV_CODEC_ID_ASV1: AVCodecID = AVCodecID(31);
    pub const AV_CODEC_ID_ASV2: AVCodecID = AVCodecID(32);
    pub const AV_CODEC_ID_FFV1: AVCodecID = AVCodecID(33);
    pub const AV_CODEC_ID_4XM: AVCodecID = AVCodecID(34);
    pub const AV_CODEC_ID_VCR1: AVCodecID = AVCodecID(35);
    pub const AV_CODEC_ID_CLJR: AVCodecID = AVCodecID(36);
    pub const AV_CODEC_ID_MDEC: AVCodecID = AVCodecID(37);
    pub const AV_CODEC_ID_ROQ: AVCodecID = AVCodecID(38);
    pub const AV_CODEC_ID_INTERPLAY_VIDEO: AVCodecID = AVCodecID(39);
    pub const AV_CODEC_ID_XAN_WC3: AVCodecID = AVCodecID(40);
    pub const AV_CODEC_ID_XAN_WC4: AVCodecID = AVCodecID(41);
    pub const AV_CODEC_ID_RPZA: AVCodecID = AVCodecID(42);
    pub const AV_CODEC_ID_CINEPAK: AVCodecID = AVCodecID(43);
    pub const AV_CODEC_ID_WS_VQA: AVCodecID = AVCodecID(44);
    pub const AV_CODEC_ID_MSRLE: AVCodecID = AVCodecID(45);
    pub const AV_CODEC_ID_MSVIDEO1: AVCodecID = AVCodecID(46);
    pub const AV_CODEC_ID_IDCIN: AVCodecID = AVCodecID(47);
    pub const AV_CODEC_ID_8BPS: AVCodecID = AVCodecID(48);
    pub const AV_CODEC_ID_SMC: AVCodecID = AVCodecID(49);
    pub const AV_CODEC_ID_FLIC: AVCodecID = AVCodecID(50);
    pub const AV_CODEC_ID_TRUEMOTION1: AVCodecID = AVCodecID(51);
    pub const AV_CODEC_ID_VMDVIDEO: AVCodecID = AVCodecID(52);
    pub const AV_CODEC_ID_MSZH: AVCodecID = AVCodecID(53);
    pub const AV_CODEC_ID_ZLIB: AVCodecID = AVCodecID(54);
    pub const AV_CODEC_ID_QTRLE: AVCodecID = AVCodecID(55);
    pub const AV_CODEC_ID_TSCC: AVCodecID = AVCodecID(56);
    pub const AV_CODEC_ID_ULTI: AVCodecID = AVCodecID(57);
    pub const AV_CODEC_ID_QDRAW: AVCodecID = AVCodecID(58);
    pub const AV_CODEC_ID_VIXL: AVCodecID = AVCodecID(59);
    pub const AV_CODEC_ID_QPEG: AVCodecID = AVCodecID(60);
    pub const AV_CODEC_ID_PNG: AVCodecID = AVCodecID(61);
    pub const AV_CODEC_ID_PPM: AVCodecID = AVCodecID(62);
    pub const AV_CODEC_ID_PBM: AVCodecID = AVCodecID(63);
    pub const AV_CODEC_ID_PGM: AVCodecID = AVCodecID(64);
    pub const AV_CODEC_ID_PGMYUV: AVCodecID = AVCodecID(65);
    pub const AV_CODEC_ID_PAM: AVCodecID = AVCodecID(66);
    pub const AV_CODEC_ID_FFVHUFF: AVCodecID = AVCodecID(67);
    pub const AV_CODEC_ID_RV30: AVCodecID = AVCodecID(68);
    pub const AV_CODEC_ID_RV40: AVCodecID = AVCodecID(69);
    pub const AV_CODEC_ID_VC1: AVCodecID = AVCodecID(70);
    pub const AV_CODEC_ID_WMV3: AVCodecID = AVCodecID(71);
    pub const AV_CODEC_ID_LOCO: AVCodecID = AVCodecID(72);
    pub const AV_CODEC_ID_WNV1: AVCodecID = AVCodecID(73);
    pub const AV_CODEC_ID_AASC: AVCodecID = AVCodecID(74);
    pub const AV_CODEC_ID_INDEO2: AVCodecID = AVCodecID(75);
    pub const AV_CODEC_ID_FRAPS: AVCodecID = AVCodecID(76);
    pub const AV_CODEC_ID_TRUEMOTION2: AVCodecID = AVCodecID(77);
    pub const AV_CODEC_ID_BMP: AVCodecID = AVCodecID(78);
    pub const AV_CODEC_ID_CSCD: AVCodecID = AVCodecID(79);
    pub const AV_CODEC_ID_MMVIDEO: AVCodecID = AVCodecID(80);
    pub const AV_CODEC_ID_ZMBV: AVCodecID = AVCodecID(81);
    pub const AV_CODEC_ID_AVS: AVCodecID = AVCodecID(82);
    pub const AV_CODEC_ID_SMACKVIDEO: AVCodecID = AVCodecID(83);
    pub const AV_CODEC_ID_NUV: AVCodecID = AVCodecID(84);
    pub const AV_CODEC_ID_KMVC: AVCodecID = AVCodecID(85);
    pub const AV_CODEC_ID_FLASHSV: AVCodecID = AVCodecID(86);
    pub const AV_CODEC_ID_CAVS: AVCodecID = AVCodecID(87);
    pub const AV_CODEC_ID_JPEG2000: AVCodecID = AVCodecID(88);
    pub const AV_CODEC_ID_VMNC: AVCodecID = AVCodecID(89);
    pub const AV_CODEC_ID_VP5: AVCodecID = AVCodecID(90);
    pub const AV_CODEC_ID_VP6: AVCodecID = AVCodecID(91);
    pub const AV_CODEC_ID_VP6F: AVCodecID = AVCodecID(92);
    pub const AV_CODEC_ID_TARGA: AVCodecID = AVCodecID(93);
    pub const AV_CODEC_ID_DSICINVIDEO: AVCodecID = AVCodecID(94);
    pub const AV_CODEC_ID_TIERTEXSEQVIDEO: AVCodecID = AVCodecID(95);
    pub const AV_CODEC_ID_TIFF: AVCodecID = AVCodecID(96);
    pub const AV_CODEC_ID_GIF: AVCodecID = AVCodecID(97);
    pub const AV_CODEC_ID_DXA: AVCodecID = AVCodecID(98);
    pub const AV_CODEC_ID_DNXHD: AVCodecID = AVCodecID(99);
    pub const AV_CODEC_ID_THP: AVCodecID = AVCodecID(100);
    pub const AV_CODEC_ID_SGI: AVCodecID = AVCodecID(101);
    pub const AV_CODEC_ID_C93: AVCodecID = AVCodecID(102);
    pub const AV_CODEC_ID_BETHSOFTVID: AVCodecID = AVCodecID(103);
    pub const AV_CODEC_ID_PTX: AVCodecID = AVCodecID(104);
    pub const AV_CODEC_ID_TXD: AVCodecID = AVCodecID(105);
    pub const AV_CODEC_ID_VP6A: AVCodecID = AVCodecID(106);
    pub const AV_CODEC_ID_AMV: AVCodecID = AVCodecID(107);
    pub const AV_CODEC_ID_VB: AVCodecID = AVCodecID(108);
    pub const AV_CODEC_ID_PCX: AVCodecID = AVCodecID(109);
    pub const AV_CODEC_ID_SUNRAST: AVCodecID = AVCodecID(110);
    pub const AV_CODEC_ID_INDEO4: AVCodecID = AVCodecID(111);
    pub const AV_CODEC_ID_INDEO5: AVCodecID = AVCodecID(112);
    pub const AV_CODEC_ID_MIMIC: AVCodecID = AVCodecID(113);
    pub const AV_CODEC_ID_RL2: AVCodecID = AVCodecID(114);
    pub const AV_CODEC_ID_ESCAPE124: AVCodecID = AVCodecID(115);
    pub const AV_CODEC_ID_DIRAC: AVCodecID = AVCodecID(116);
    pub const AV_CODEC_ID_BFI: AVCodecID = AVCodecID(117);
    pub const AV_CODEC_ID_CMV: AVCodecID = AVCodecID(118);
    pub const AV_CODEC_ID_MOTIONPIXELS: AVCodecID = AVCodecID(119);
    pub const AV_CODEC_ID_TGV: AVCodecID = AVCodecID(120);
    pub const AV_CODEC_ID_TGQ: AVCodecID = AVCodecID(121);
    pub const AV_CODEC_ID_TQI: AVCodecID = AVCodecID(122);
    pub const AV_CODEC_ID_AURA: AVCodecID = AVCodecID(123);
    pub const AV_CODEC_ID_AURA2: AVCodecID = AVCodecID(124);
    pub const AV_CODEC_ID_V210X: AVCodecID = AVCodecID(125);
    pub const AV_CODEC_ID_TMV: AVCodecID = AVCodecID(126);
    pub const AV_CODEC_ID_V210: AVCodecID = AVCodecID(127);
    pub const AV_CODEC_ID_DPX: AVCodecID = AVCodecID(128);
    pub const AV_CODEC_ID_MAD: AVCodecID = AVCodecID(129);
    pub const AV_CODEC_ID_FRWU: AVCodecID = AVCodecID(130);
    pub const AV_CODEC_ID_FLASHSV2: AVCodecID = AVCodecID(131);
    pub const AV_CODEC_ID_CDGRAPHICS: AVCodecID = AVCodecID(132);
    pub const AV_CODEC_ID_R210: AVCodecID = AVCodecID(133);
    pub const AV_CODEC_ID_ANM: AVCodecID = AVCodecID(134);
    pub const AV_CODEC_ID_BINKVIDEO: AVCodecID = AVCodecID(135);
    pub const AV_CODEC_ID_IFF_ILBM: AVCodecID = AVCodecID(136);
    pub const AV_CODEC_ID_KGV1: AVCodecID = AVCodecID(137);
    pub const AV_CODEC_ID_YOP: AVCodecID = AVCodecID(138);
    pub const AV_CODEC_ID_VP8: AVCodecID = AVCodecID(139);
    pub const AV_CODEC_ID_PICTOR: AVCodecID = AVCodecID(140);
    pub const AV_CODEC_ID_ANSI: AVCodecID = AVCodecID(141);
    pub const AV_CODEC_ID_A64_MULTI: AVCodecID = AVCodecID(142);
    pub const AV_CODEC_ID_A64_MULTI5: AVCodecID = AVCodecID(143);
    pub const AV_CODEC_ID_R10K: AVCodecID = AVCodecID(144);
    pub const AV_CODEC_ID_MXPEG: AVCodecID = AVCodecID(145);
    pub const AV_CODEC_ID_LAGARITH: AVCodecID = AVCodecID(146);
    pub const AV_CODEC_ID_PRORES: AVCodecID = AVCodecID(147);
    pub const AV_CODEC_ID_JV: AVCodecID = AVCodecID(148);
    pub const AV_CODEC_ID_DFA: AVCodecID = AVCodecID(149);
    pub const AV_CODEC_ID_WMV3IMAGE: AVCodecID = AVCodecID(150);
    pub const AV_CODEC_ID_VC1IMAGE: AVCodecID = AVCodecID(151);
    pub const AV_CODEC_ID_UTVIDEO: AVCodecID = AVCodecID(152);
    pub const AV_CODEC_ID_BMV_VIDEO: AVCodecID = AVCodecID(153);
    pub const AV_CODEC_ID_VBLE: AVCodecID = AVCodecID(154);
    pub const AV_CODEC_ID_DXTORY: AVCodecID = AVCodecID(155);
    pub const AV_CODEC_ID_V410: AVCodecID = AVCodecID(156);
    pub const AV_CODEC_ID_XWD: AVCodecID = AVCodecID(157);
    pub const AV_CODEC_ID_CDXL: AVCodecID = AVCodecID(158);
    pub const AV_CODEC_ID_XBM: AVCodecID = AVCodecID(159);
    pub const AV_CODEC_ID_ZEROCODEC: AVCodecID = AVCodecID(160);
    pub const AV_CODEC_ID_MSS1: AVCodecID = AVCodecID(161);
    pub const AV_CODEC_ID_MSA1: AVCodecID = AVCodecID(162);
    pub const AV_CODEC_ID_TSCC2: AVCodecID = AVCodecID(163);
    pub const AV_CODEC_ID_MTS2: AVCodecID = AVCodecID(164);
    pub const AV_CODEC_ID_CLLC: AVCodecID = AVCodecID(165);
    pub const AV_CODEC_ID_MSS2: AVCodecID = AVCodecID(166);
    pub const AV_CODEC_ID_VP9: AVCodecID = AVCodecID(167);
    pub const AV_CODEC_ID_AIC: AVCodecID = AVCodecID(168);
    pub const AV_CODEC_ID_ESCAPE130: AVCodecID = AVCodecID(169);
    pub const AV_CODEC_ID_G2M: AVCodecID = AVCodecID(170);
    pub const AV_CODEC_ID_WEBP: AVCodecID = AVCodecID(171);
    pub const AV_CODEC_ID_HNM4_VIDEO: AVCodecID = AVCodecID(172);
    pub const AV_CODEC_ID_HEVC: AVCodecID = AVCodecID(173);
    pub const AV_CODEC_ID_FIC: AVCodecID = AVCodecID(174);
    pub const AV_CODEC_ID_ALIAS_PIX: AVCodecID = AVCodecID(175);
    pub const AV_CODEC_ID_BRENDER_PIX: AVCodecID = AVCodecID(176);
    pub const AV_CODEC_ID_PAF_VIDEO: AVCodecID = AVCodecID(177);
    pub const AV_CODEC_ID_EXR: AVCodecID = AVCodecID(178);
    pub const AV_CODEC_ID_VP7: AVCodecID = AVCodecID(179);
    pub const AV_CODEC_ID_SANM: AVCodecID = AVCodecID(180);
    pub const AV_CODEC_ID_SGIRLE: AVCodecID = AVCodecID(181);
    pub const AV_CODEC_ID_MVC1: AVCodecID = AVCodecID(182);
    pub const AV_CODEC_ID_MVC2: AVCodecID = AVCodecID(183);
    pub const AV_CODEC_ID_HQX: AVCodecID = AVCodecID(184);
    pub const AV_CODEC_ID_TDSC: AVCodecID = AVCodecID(185);
    pub const AV_CODEC_ID_HQ_HQA: AVCodecID = AVCodecID(186);
    pub const AV_CODEC_ID_HAP: AVCodecID = AVCodecID(187);
    pub const AV_CODEC_ID_DDS: AVCodecID = AVCodecID(188);
    pub const AV_CODEC_ID_DXV: AVCodecID = AVCodecID(189);
    pub const AV_CODEC_ID_SCREENPRESSO: AVCodecID = AVCodecID(190);
    pub const AV_CODEC_ID_RSCC: AVCodecID = AVCodecID(191);
    pub const AV_CODEC_ID_AVS2: AVCodecID = AVCodecID(192);
    pub const AV_CODEC_ID_PGX: AVCodecID = AVCodecID(193);
    pub const AV_CODEC_ID_AVS3: AVCodecID = AVCodecID(194);
    pub const AV_CODEC_ID_MSP2: AVCodecID = AVCodecID(195);
    pub const AV_CODEC_ID_VVC: AVCodecID = AVCodecID(196);
    pub const AV_CODEC_ID_Y41P: AVCodecID = AVCodecID(197);
    pub const AV_CODEC_ID_AVRP: AVCodecID = AVCodecID(198);
    pub const AV_CODEC_ID_012V: AVCodecID = AVCodecID(199);
    pub const AV_CODEC_ID_AVUI: AVCodecID = AVCodecID(200);
    pub const AV_CODEC_ID_TARGA_Y216: AVCodecID = AVCodecID(201);
    pub const AV_CODEC_ID_V308: AVCodecID = AVCodecID(202);
    pub const AV_CODEC_ID_V408: AVCodecID = AVCodecID(203);
    pub const AV_CODEC_ID_YUV4: AVCodecID = AVCodecID(204);
    pub const AV_CODEC_ID_AVRN: AVCodecID = AVCodecID(205);
    pub const AV_CODEC_ID_CPIA: AVCodecID = AVCodecID(206);
    pub const AV_CODEC_ID_XFACE: AVCodecID = AVCodecID(207);
    pub const AV_CODEC_ID_SNOW: AVCodecID = AVCodecID(208);
    pub const AV_CODEC_ID_SMVJPEG: AVCodecID = AVCodecID(209);
    pub const AV_CODEC_ID_APNG: AVCodecID = AVCodecID(210);
    pub const AV_CODEC_ID_DAALA: AVCodecID = AVCodecID(211);
    pub const AV_CODEC_ID_CFHD: AVCodecID = AVCodecID(212);
    pub const AV_CODEC_ID_TRUEMOTION2RT: AVCodecID = AVCodecID(213);
    pub const AV_CODEC_ID_M101: AVCodecID = AVCodecID(214);
    pub const AV_CODEC_ID_MAGICYUV: AVCodecID = AVCodecID(215);
    pub const AV_CODEC_ID_SHEERVIDEO: AVCodecID = AVCodecID(216);
    pub const AV_CODEC_ID_YLC: AVCodecID = AVCodecID(217);
    pub const AV_CODEC_ID_PSD: AVCodecID = AVCodecID(218);
    pub const AV_CODEC_ID_PIXLET: AVCodecID = AVCodecID(219);
    pub const AV_CODEC_ID_SPEEDHQ: AVCodecID = AVCodecID(220);
    pub const AV_CODEC_ID_FMVC: AVCodecID = AVCodecID(221);
    pub const AV_CODEC_ID_SCPR: AVCodecID = AVCodecID(222);
    pub const AV_CODEC_ID_CLEARVIDEO: AVCodecID = AVCodecID(223);
    pub const AV_CODEC_ID_XPM: AVCodecID = AVCodecID(224);
    pub const AV_CODEC_ID_AV1: AVCodecID = AVCodecID(225);
    pub const AV_CODEC_ID_BITPACKED: AVCodecID = AVCodecID(226);
    pub const AV_CODEC_ID_MSCC: AVCodecID = AVCodecID(227);
    pub const AV_CODEC_ID_SRGC: AVCodecID = AVCodecID(228);
    pub const AV_CODEC_ID_SVG: AVCodecID = AVCodecID(229);
    pub const AV_CODEC_ID_GDV: AVCodecID = AVCodecID(230);
    pub const AV_CODEC_ID_FITS: AVCodecID = AVCodecID(231);
    pub const AV_CODEC_ID_IMM4: AVCodecID = AVCodecID(232);
    pub const AV_CODEC_ID_PROSUMER: AVCodecID = AVCodecID(233);
    pub const AV_CODEC_ID_MWSC: AVCodecID = AVCodecID(234);
    pub const AV_CODEC_ID_WCMV: AVCodecID = AVCodecID(235);
    pub const AV_CODEC_ID_RASC: AVCodecID = AVCodecID(236);
    pub const AV_CODEC_ID_HYMT: AVCodecID = AVCodecID(237);
    pub const AV_CODEC_ID_ARBC: AVCodecID = AVCodecID(238);
    pub const AV_CODEC_ID_AGM: AVCodecID = AVCodecID(239);
    pub const AV_CODEC_ID_LSCR: AVCodecID = AVCodecID(240);
    pub const AV_CODEC_ID_VP4: AVCodecID = AVCodecID(241);
    pub const AV_CODEC_ID_IMM5: AVCodecID = AVCodecID(242);
    pub const AV_CODEC_ID_MVDV: AVCodecID = AVCodecID(243);
    pub const AV_CODEC_ID_MVHA: AVCodecID = AVCodecID(244);
    pub const AV_CODEC_ID_CDTOONS: AVCodecID = AVCodecID(245);
    pub const AV_CODEC_ID_MV30: AVCodecID = AVCodecID(246);
    pub const AV_CODEC_ID_NOTCHLC: AVCodecID = AVCodecID(247);
    pub const AV_CODEC_ID_PFM: AVCodecID = AVCodecID(248);
    pub const AV_CODEC_ID_MOBICLIP: AVCodecID = AVCodecID(249);
    pub const AV_CODEC_ID_PHOTOCD: AVCodecID = AVCodecID(250);
    pub const AV_CODEC_ID_IPU: AVCodecID = AVCodecID(251);
    pub const AV_CODEC_ID_ARGO: AVCodecID = AVCodecID(252);
    pub const AV_CODEC_ID_CRI: AVCodecID = AVCodecID(253);
    pub const AV_CODEC_ID_SIMBIOSIS_IMX: AVCodecID = AVCodecID(254);
    pub const AV_CODEC_ID_SGA_VIDEO: AVCodecID = AVCodecID(255);
    pub const AV_CODEC_ID_GEM: AVCodecID = AVCodecID(256);
    pub const AV_CODEC_ID_VBN: AVCodecID = AVCodecID(257);
    pub const AV_CODEC_ID_JPEGXL: AVCodecID = AVCodecID(258);
    pub const AV_CODEC_ID_QOI: AVCodecID = AVCodecID(259);
    pub const AV_CODEC_ID_PHM: AVCodecID = AVCodecID(260);
    pub const AV_CODEC_ID_RADIANCE_HDR: AVCodecID = AVCodecID(261);
    pub const AV_CODEC_ID_WBMP: AVCodecID = AVCodecID(262);
    pub const AV_CODEC_ID_MEDIA100: AVCodecID = AVCodecID(263);
    pub const AV_CODEC_ID_VQC: AVCodecID = AVCodecID(264);
    pub const AV_CODEC_ID_PDV: AVCodecID = AVCodecID(265);
    pub const AV_CODEC_ID_EVC: AVCodecID = AVCodecID(266);
    pub const AV_CODEC_ID_RTV1: AVCodecID = AVCodecID(267);
    pub const AV_CODEC_ID_VMIX: AVCodecID = AVCodecID(268);
    pub const AV_CODEC_ID_LEAD: AVCodecID = AVCodecID(269);
    pub const AV_CODEC_ID_DNXUC: AVCodecID = AVCodecID(270);
    pub const AV_CODEC_ID_RV60: AVCodecID = AVCodecID(271);
    pub const AV_CODEC_ID_JPEGXL_ANIM: AVCodecID = AVCodecID(272);
    pub const AV_CODEC_ID_APV: AVCodecID = AVCodecID(273);
    pub const AV_CODEC_ID_PRORES_RAW: AVCodecID = AVCodecID(274);
    pub const AV_CODEC_ID_FIRST_AUDIO: AVCodecID = AVCodecID(65536);
    pub const AV_CODEC_ID_PCM_S16LE: AVCodecID = AVCodecID(65536);
    pub const AV_CODEC_ID_PCM_S16BE: AVCodecID = AVCodecID(65537);
    pub const AV_CODEC_ID_PCM_U16LE: AVCodecID = AVCodecID(65538);
    pub const AV_CODEC_ID_PCM_U16BE: AVCodecID = AVCodecID(65539);
    pub const AV_CODEC_ID_PCM_S8: AVCodecID = AVCodecID(65540);
    pub const AV_CODEC_ID_PCM_U8: AVCodecID = AVCodecID(65541);
    pub const AV_CODEC_ID_PCM_MULAW: AVCodecID = AVCodecID(65542);
    pub const AV_CODEC_ID_PCM_ALAW: AVCodecID = AVCodecID(65543);
    pub const AV_CODEC_ID_PCM_S32LE: AVCodecID = AVCodecID(65544);
    pub const AV_CODEC_ID_PCM_S32BE: AVCodecID = AVCodecID(65545);
    pub const AV_CODEC_ID_PCM_U32LE: AVCodecID = AVCodecID(65546);
    pub const AV_CODEC_ID_PCM_U32BE: AVCodecID = AVCodecID(65547);
    pub const AV_CODEC_ID_PCM_S24LE: AVCodecID = AVCodecID(65548);
    pub const AV_CODEC_ID_PCM_S24BE: AVCodecID = AVCodecID(65549);
    pub const AV_CODEC_ID_PCM_U24LE: AVCodecID = AVCodecID(65550);
    pub const AV_CODEC_ID_PCM_U24BE: AVCodecID = AVCodecID(65551);
    pub const AV_CODEC_ID_PCM_S24DAUD: AVCodecID = AVCodecID(65552);
    pub const AV_CODEC_ID_PCM_ZORK: AVCodecID = AVCodecID(65553);
    pub const AV_CODEC_ID_PCM_S16LE_PLANAR: AVCodecID = AVCodecID(65554);
    pub const AV_CODEC_ID_PCM_DVD: AVCodecID = AVCodecID(65555);
    pub const AV_CODEC_ID_PCM_F32BE: AVCodecID = AVCodecID(65556);
    pub const AV_CODEC_ID_PCM_F32LE: AVCodecID = AVCodecID(65557);
    pub const AV_CODEC_ID_PCM_F64BE: AVCodecID = AVCodecID(65558);
    pub const AV_CODEC_ID_PCM_F64LE: AVCodecID = AVCodecID(65559);
    pub const AV_CODEC_ID_PCM_BLURAY: AVCodecID = AVCodecID(65560);
    pub const AV_CODEC_ID_PCM_LXF: AVCodecID = AVCodecID(65561);
    pub const AV_CODEC_ID_S302M: AVCodecID = AVCodecID(65562);
    pub const AV_CODEC_ID_PCM_S8_PLANAR: AVCodecID = AVCodecID(65563);
    pub const AV_CODEC_ID_PCM_S24LE_PLANAR: AVCodecID = AVCodecID(65564);
    pub const AV_CODEC_ID_PCM_S32LE_PLANAR: AVCodecID = AVCodecID(65565);
    pub const AV_CODEC_ID_PCM_S16BE_PLANAR: AVCodecID = AVCodecID(65566);
    pub const AV_CODEC_ID_PCM_S64LE: AVCodecID = AVCodecID(65567);
    pub const AV_CODEC_ID_PCM_S64BE: AVCodecID = AVCodecID(65568);
    pub const AV_CODEC_ID_PCM_F16LE: AVCodecID = AVCodecID(65569);
    pub const AV_CODEC_ID_PCM_F24LE: AVCodecID = AVCodecID(65570);
    pub const AV_CODEC_ID_PCM_VIDC: AVCodecID = AVCodecID(65571);
    pub const AV_CODEC_ID_PCM_SGA: AVCodecID = AVCodecID(65572);
    pub const AV_CODEC_ID_ADPCM_IMA_QT: AVCodecID = AVCodecID(69632);
    pub const AV_CODEC_ID_ADPCM_IMA_WAV: AVCodecID = AVCodecID(69633);
    pub const AV_CODEC_ID_ADPCM_IMA_DK3: AVCodecID = AVCodecID(69634);
    pub const AV_CODEC_ID_ADPCM_IMA_DK4: AVCodecID = AVCodecID(69635);
    pub const AV_CODEC_ID_ADPCM_IMA_WS: AVCodecID = AVCodecID(69636);
    pub const AV_CODEC_ID_ADPCM_IMA_SMJPEG: AVCodecID = AVCodecID(69637);
    pub const AV_CODEC_ID_ADPCM_MS: AVCodecID = AVCodecID(69638);
    pub const AV_CODEC_ID_ADPCM_4XM: AVCodecID = AVCodecID(69639);
    pub const AV_CODEC_ID_ADPCM_XA: AVCodecID = AVCodecID(69640);
    pub const AV_CODEC_ID_ADPCM_ADX: AVCodecID = AVCodecID(69641);
    pub const AV_CODEC_ID_ADPCM_EA: AVCodecID = AVCodecID(69642);
    pub const AV_CODEC_ID_ADPCM_G726: AVCodecID = AVCodecID(69643);
    pub const AV_CODEC_ID_ADPCM_CT: AVCodecID = AVCodecID(69644);
    pub const AV_CODEC_ID_ADPCM_SWF: AVCodecID = AVCodecID(69645);
    pub const AV_CODEC_ID_ADPCM_YAMAHA: AVCodecID = AVCodecID(69646);
    pub const AV_CODEC_ID_ADPCM_SBPRO_4: AVCodecID = AVCodecID(69647);
    pub const AV_CODEC_ID_ADPCM_SBPRO_3: AVCodecID = AVCodecID(69648);
    pub const AV_CODEC_ID_ADPCM_SBPRO_2: AVCodecID = AVCodecID(69649);
    pub const AV_CODEC_ID_ADPCM_THP: AVCodecID = AVCodecID(69650);
    pub const AV_CODEC_ID_ADPCM_IMA_AMV: AVCodecID = AVCodecID(69651);
    pub const AV_CODEC_ID_ADPCM_EA_R1: AVCodecID = AVCodecID(69652);
    pub const AV_CODEC_ID_ADPCM_EA_R3: AVCodecID = AVCodecID(69653);
    pub const AV_CODEC_ID_ADPCM_EA_R2: AVCodecID = AVCodecID(69654);
    pub const AV_CODEC_ID_ADPCM_IMA_EA_SEAD: AVCodecID = AVCodecID(69655);
    pub const AV_CODEC_ID_ADPCM_IMA_EA_EACS: AVCodecID = AVCodecID(69656);
    pub const AV_CODEC_ID_ADPCM_EA_XAS: AVCodecID = AVCodecID(69657);
    pub const AV_CODEC_ID_ADPCM_EA_MAXIS_XA: AVCodecID = AVCodecID(69658);
    pub const AV_CODEC_ID_ADPCM_IMA_ISS: AVCodecID = AVCodecID(69659);
    pub const AV_CODEC_ID_ADPCM_G722: AVCodecID = AVCodecID(69660);
    pub const AV_CODEC_ID_ADPCM_IMA_APC: AVCodecID = AVCodecID(69661);
    pub const AV_CODEC_ID_ADPCM_VIMA: AVCodecID = AVCodecID(69662);
    pub const AV_CODEC_ID_ADPCM_AFC: AVCodecID = AVCodecID(69663);
    pub const AV_CODEC_ID_ADPCM_IMA_OKI: AVCodecID = AVCodecID(69664);
    pub const AV_CODEC_ID_ADPCM_DTK: AVCodecID = AVCodecID(69665);
    pub const AV_CODEC_ID_ADPCM_IMA_RAD: AVCodecID = AVCodecID(69666);
    pub const AV_CODEC_ID_ADPCM_G726LE: AVCodecID = AVCodecID(69667);
    pub const AV_CODEC_ID_ADPCM_THP_LE: AVCodecID = AVCodecID(69668);
    pub const AV_CODEC_ID_ADPCM_PSX: AVCodecID = AVCodecID(69669);
    pub const AV_CODEC_ID_ADPCM_AICA: AVCodecID = AVCodecID(69670);
    pub const AV_CODEC_ID_ADPCM_IMA_DAT4: AVCodecID = AVCodecID(69671);
    pub const AV_CODEC_ID_ADPCM_MTAF: AVCodecID = AVCodecID(69672);
    pub const AV_CODEC_ID_ADPCM_AGM: AVCodecID = AVCodecID(69673);
    pub const AV_CODEC_ID_ADPCM_ARGO: AVCodecID = AVCodecID(69674);
    pub const AV_CODEC_ID_ADPCM_IMA_SSI: AVCodecID = AVCodecID(69675);
    pub const AV_CODEC_ID_ADPCM_ZORK: AVCodecID = AVCodecID(69676);
    pub const AV_CODEC_ID_ADPCM_IMA_APM: AVCodecID = AVCodecID(69677);
    pub const AV_CODEC_ID_ADPCM_IMA_ALP: AVCodecID = AVCodecID(69678);
    pub const AV_CODEC_ID_ADPCM_IMA_MTF: AVCodecID = AVCodecID(69679);
    pub const AV_CODEC_ID_ADPCM_IMA_CUNNING: AVCodecID = AVCodecID(69680);
    pub const AV_CODEC_ID_ADPCM_IMA_MOFLEX: AVCodecID = AVCodecID(69681);
    pub const AV_CODEC_ID_ADPCM_IMA_ACORN: AVCodecID = AVCodecID(69682);
    pub const AV_CODEC_ID_ADPCM_XMD: AVCodecID = AVCodecID(69683);
    pub const AV_CODEC_ID_ADPCM_IMA_XBOX: AVCodecID = AVCodecID(69684);
    pub const AV_CODEC_ID_ADPCM_SANYO: AVCodecID = AVCodecID(69685);
    pub const AV_CODEC_ID_AMR_NB: AVCodecID = AVCodecID(73728);
    pub const AV_CODEC_ID_AMR_WB: AVCodecID = AVCodecID(73729);
    pub const AV_CODEC_ID_RA_144: AVCodecID = AVCodecID(77824);
    pub const AV_CODEC_ID_RA_288: AVCodecID = AVCodecID(77825);
    pub const AV_CODEC_ID_ROQ_DPCM: AVCodecID = AVCodecID(81920);
    pub const AV_CODEC_ID_INTERPLAY_DPCM: AVCodecID = AVCodecID(81921);
    pub const AV_CODEC_ID_XAN_DPCM: AVCodecID = AVCodecID(81922);
    pub const AV_CODEC_ID_SOL_DPCM: AVCodecID = AVCodecID(81923);
    pub const AV_CODEC_ID_SDX2_DPCM: AVCodecID = AVCodecID(81924);
    pub const AV_CODEC_ID_GREMLIN_DPCM: AVCodecID = AVCodecID(81925);
    pub const AV_CODEC_ID_DERF_DPCM: AVCodecID = AVCodecID(81926);
    pub const AV_CODEC_ID_WADY_DPCM: AVCodecID = AVCodecID(81927);
    pub const AV_CODEC_ID_CBD2_DPCM: AVCodecID = AVCodecID(81928);
    pub const AV_CODEC_ID_MP2: AVCodecID = AVCodecID(86016);
    pub const AV_CODEC_ID_MP3: AVCodecID = AVCodecID(86017);
    pub const AV_CODEC_ID_AAC: AVCodecID = AVCodecID(86018);
    pub const AV_CODEC_ID_AC3: AVCodecID = AVCodecID(86019);
    pub const AV_CODEC_ID_DTS: AVCodecID = AVCodecID(86020);
    pub const AV_CODEC_ID_VORBIS: AVCodecID = AVCodecID(86021);
    pub const AV_CODEC_ID_DVAUDIO: AVCodecID = AVCodecID(86022);
    pub const AV_CODEC_ID_WMAV1: AVCodecID = AVCodecID(86023);
    pub const AV_CODEC_ID_WMAV2: AVCodecID = AVCodecID(86024);
    pub const AV_CODEC_ID_MACE3: AVCodecID = AVCodecID(86025);
    pub const AV_CODEC_ID_MACE6: AVCodecID = AVCodecID(86026);
    pub const AV_CODEC_ID_VMDAUDIO: AVCodecID = AVCodecID(86027);
    pub const AV_CODEC_ID_FLAC: AVCodecID = AVCodecID(86028);
    pub const AV_CODEC_ID_MP3ADU: AVCodecID = AVCodecID(86029);
    pub const AV_CODEC_ID_MP3ON4: AVCodecID = AVCodecID(86030);
    pub const AV_CODEC_ID_SHORTEN: AVCodecID = AVCodecID(86031);
    pub const AV_CODEC_ID_ALAC: AVCodecID = AVCodecID(86032);
    pub const AV_CODEC_ID_WESTWOOD_SND1: AVCodecID = AVCodecID(86033);
    pub const AV_CODEC_ID_GSM: AVCodecID = AVCodecID(86034);
    pub const AV_CODEC_ID_QDM2: AVCodecID = AVCodecID(86035);
    pub const AV_CODEC_ID_COOK: AVCodecID = AVCodecID(86036);
    pub const AV_CODEC_ID_TRUESPEECH: AVCodecID = AVCodecID(86037);
    pub const AV_CODEC_ID_TTA: AVCodecID = AVCodecID(86038);
    pub const AV_CODEC_ID_SMACKAUDIO: AVCodecID = AVCodecID(86039);
    pub const AV_CODEC_ID_QCELP: AVCodecID = AVCodecID(86040);
    pub const AV_CODEC_ID_WAVPACK: AVCodecID = AVCodecID(86041);
    pub const AV_CODEC_ID_DSICINAUDIO: AVCodecID = AVCodecID(86042);
    pub const AV_CODEC_ID_IMC: AVCodecID = AVCodecID(86043);
    pub const AV_CODEC_ID_MUSEPACK7: AVCodecID = AVCodecID(86044);
    pub const AV_CODEC_ID_MLP: AVCodecID = AVCodecID(86045);
    pub const AV_CODEC_ID_GSM_MS: AVCodecID = AVCodecID(86046);
    pub const AV_CODEC_ID_ATRAC3: AVCodecID = AVCodecID(86047);
    pub const AV_CODEC_ID_APE: AVCodecID = AVCodecID(86048);
    pub const AV_CODEC_ID_NELLYMOSER: AVCodecID = AVCodecID(86049);
    pub const AV_CODEC_ID_MUSEPACK8: AVCodecID = AVCodecID(86050);
    pub const AV_CODEC_ID_SPEEX: AVCodecID = AVCodecID(86051);
    pub const AV_CODEC_ID_WMAVOICE: AVCodecID = AVCodecID(86052);
    pub const AV_CODEC_ID_WMAPRO: AVCodecID = AVCodecID(86053);
    pub const AV_CODEC_ID_WMALOSSLESS: AVCodecID = AVCodecID(86054);
    pub const AV_CODEC_ID_ATRAC3P: AVCodecID = AVCodecID(86055);
    pub const AV_CODEC_ID_EAC3: AVCodecID = AVCodecID(86056);
    pub const AV_CODEC_ID_SIPR: AVCodecID = AVCodecID(86057);
    pub const AV_CODEC_ID_MP1: AVCodecID = AVCodecID(86058);
    pub const AV_CODEC_ID_TWINVQ: AVCodecID = AVCodecID(86059);
    pub const AV_CODEC_ID_TRUEHD: AVCodecID = AVCodecID(86060);
    pub const AV_CODEC_ID_MP4ALS: AVCodecID = AVCodecID(86061);
    pub const AV_CODEC_ID_ATRAC1: AVCodecID = AVCodecID(86062);
    pub const AV_CODEC_ID_BINKAUDIO_RDFT: AVCodecID = AVCodecID(86063);
    pub const AV_CODEC_ID_BINKAUDIO_DCT: AVCodecID = AVCodecID(86064);
    pub const AV_CODEC_ID_AAC_LATM: AVCodecID = AVCodecID(86065);
    pub const AV_CODEC_ID_QDMC: AVCodecID = AVCodecID(86066);
    pub const AV_CODEC_ID_CELT: AVCodecID = AVCodecID(86067);
    pub const AV_CODEC_ID_G723_1: AVCodecID = AVCodecID(86068);
    pub const AV_CODEC_ID_G729: AVCodecID = AVCodecID(86069);
    pub const AV_CODEC_ID_8SVX_EXP: AVCodecID = AVCodecID(86070);
    pub const AV_CODEC_ID_8SVX_FIB: AVCodecID = AVCodecID(86071);
    pub const AV_CODEC_ID_BMV_AUDIO: AVCodecID = AVCodecID(86072);
    pub const AV_CODEC_ID_RALF: AVCodecID = AVCodecID(86073);
    pub const AV_CODEC_ID_IAC: AVCodecID = AVCodecID(86074);
    pub const AV_CODEC_ID_ILBC: AVCodecID = AVCodecID(86075);
    pub const AV_CODEC_ID_OPUS: AVCodecID = AVCodecID(86076);
    pub const AV_CODEC_ID_COMFORT_NOISE: AVCodecID = AVCodecID(86077);
    pub const AV_CODEC_ID_TAK: AVCodecID = AVCodecID(86078);
    pub const AV_CODEC_ID_METASOUND: AVCodecID = AVCodecID(86079);
    pub const AV_CODEC_ID_PAF_AUDIO: AVCodecID = AVCodecID(86080);
    pub const AV_CODEC_ID_ON2AVC: AVCodecID = AVCodecID(86081);
    pub const AV_CODEC_ID_DSS_SP: AVCodecID = AVCodecID(86082);
    pub const AV_CODEC_ID_CODEC2: AVCodecID = AVCodecID(86083);
    pub const AV_CODEC_ID_FFWAVESYNTH: AVCodecID = AVCodecID(86084);
    pub const AV_CODEC_ID_SONIC: AVCodecID = AVCodecID(86085);
    pub const AV_CODEC_ID_SONIC_LS: AVCodecID = AVCodecID(86086);
    pub const AV_CODEC_ID_EVRC: AVCodecID = AVCodecID(86087);
    pub const AV_CODEC_ID_SMV: AVCodecID = AVCodecID(86088);
    pub const AV_CODEC_ID_DSD_LSBF: AVCodecID = AVCodecID(86089);
    pub const AV_CODEC_ID_DSD_MSBF: AVCodecID = AVCodecID(86090);
    pub const AV_CODEC_ID_DSD_LSBF_PLANAR: AVCodecID = AVCodecID(86091);
    pub const AV_CODEC_ID_DSD_MSBF_PLANAR: AVCodecID = AVCodecID(86092);
    pub const AV_CODEC_ID_4GV: AVCodecID = AVCodecID(86093);
    pub const AV_CODEC_ID_INTERPLAY_ACM: AVCodecID = AVCodecID(86094);
    pub const AV_CODEC_ID_XMA1: AVCodecID = AVCodecID(86095);
    pub const AV_CODEC_ID_XMA2: AVCodecID = AVCodecID(86096);
    pub const AV_CODEC_ID_DST: AVCodecID = AVCodecID(86097);
    pub const AV_CODEC_ID_ATRAC3AL: AVCodecID = AVCodecID(86098);
    pub const AV_CODEC_ID_ATRAC3PAL: AVCodecID = AVCodecID(86099);
    pub const AV_CODEC_ID_DOLBY_E: AVCodecID = AVCodecID(86100);
    pub const AV_CODEC_ID_APTX: AVCodecID = AVCodecID(86101);
    pub const AV_CODEC_ID_APTX_HD: AVCodecID = AVCodecID(86102);
    pub const AV_CODEC_ID_SBC: AVCodecID = AVCodecID(86103);
    pub const AV_CODEC_ID_ATRAC9: AVCodecID = AVCodecID(86104);
    pub const AV_CODEC_ID_HCOM: AVCodecID = AVCodecID(86105);
    pub const AV_CODEC_ID_ACELP_KELVIN: AVCodecID = AVCodecID(86106);
    pub const AV_CODEC_ID_MPEGH_3D_AUDIO: AVCodecID = AVCodecID(86107);
    pub const AV_CODEC_ID_SIREN: AVCodecID = AVCodecID(86108);
    pub const AV_CODEC_ID_HCA: AVCodecID = AVCodecID(86109);
    pub const AV_CODEC_ID_FASTAUDIO: AVCodecID = AVCodecID(86110);
    pub const AV_CODEC_ID_MSNSIREN: AVCodecID = AVCodecID(86111);
    pub const AV_CODEC_ID_DFPWM: AVCodecID = AVCodecID(86112);
    pub const AV_CODEC_ID_BONK: AVCodecID = AVCodecID(86113);
    pub const AV_CODEC_ID_MISC4: AVCodecID = AVCodecID(86114);
    pub const AV_CODEC_ID_APAC: AVCodecID = AVCodecID(86115);
    pub const AV_CODEC_ID_FTR: AVCodecID = AVCodecID(86116);
    pub const AV_CODEC_ID_WAVARC: AVCodecID = AVCodecID(86117);
    pub const AV_CODEC_ID_RKA: AVCodecID = AVCodecID(86118);
    pub const AV_CODEC_ID_AC4: AVCodecID = AVCodecID(86119);
    pub const AV_CODEC_ID_OSQ: AVCodecID = AVCodecID(86120);
    pub const AV_CODEC_ID_QOA: AVCodecID = AVCodecID(86121);
    pub const AV_CODEC_ID_LC3: AVCodecID = AVCodecID(86122);
    pub const AV_CODEC_ID_G728: AVCodecID = AVCodecID(86123);
    pub const AV_CODEC_ID_FIRST_SUBTITLE: AVCodecID = AVCodecID(94208);
    pub const AV_CODEC_ID_DVD_SUBTITLE: AVCodecID = AVCodecID(94208);
    pub const AV_CODEC_ID_DVB_SUBTITLE: AVCodecID = AVCodecID(94209);
    pub const AV_CODEC_ID_TEXT: AVCodecID = AVCodecID(94210);
    pub const AV_CODEC_ID_XSUB: AVCodecID = AVCodecID(94211);
    pub const AV_CODEC_ID_SSA: AVCodecID = AVCodecID(94212);
    pub const AV_CODEC_ID_MOV_TEXT: AVCodecID = AVCodecID(94213);
    pub const AV_CODEC_ID_HDMV_PGS_SUBTITLE: AVCodecID = AVCodecID(94214);
    pub const AV_CODEC_ID_DVB_TELETEXT: AVCodecID = AVCodecID(94215);
    pub const AV_CODEC_ID_SRT: AVCodecID = AVCodecID(94216);
    pub const AV_CODEC_ID_MICRODVD: AVCodecID = AVCodecID(94217);
    pub const AV_CODEC_ID_EIA_608: AVCodecID = AVCodecID(94218);
    pub const AV_CODEC_ID_JACOSUB: AVCodecID = AVCodecID(94219);
    pub const AV_CODEC_ID_SAMI: AVCodecID = AVCodecID(94220);
    pub const AV_CODEC_ID_REALTEXT: AVCodecID = AVCodecID(94221);
    pub const AV_CODEC_ID_STL: AVCodecID = AVCodecID(94222);
    pub const AV_CODEC_ID_SUBVIEWER1: AVCodecID = AVCodecID(94223);
    pub const AV_CODEC_ID_SUBVIEWER: AVCodecID = AVCodecID(94224);
    pub const AV_CODEC_ID_SUBRIP: AVCodecID = AVCodecID(94225);
    pub const AV_CODEC_ID_WEBVTT: AVCodecID = AVCodecID(94226);
    pub const AV_CODEC_ID_MPL2: AVCodecID = AVCodecID(94227);
    pub const AV_CODEC_ID_VPLAYER: AVCodecID = AVCodecID(94228);
    pub const AV_CODEC_ID_PJS: AVCodecID = AVCodecID(94229);
    pub const AV_CODEC_ID_ASS: AVCodecID = AVCodecID(94230);
    pub const AV_CODEC_ID_HDMV_TEXT_SUBTITLE: AVCodecID = AVCodecID(94231);
    pub const AV_CODEC_ID_TTML: AVCodecID = AVCodecID(94232);
    pub const AV_CODEC_ID_ARIB_CAPTION: AVCodecID = AVCodecID(94233);
    pub const AV_CODEC_ID_IVTV_VBI: AVCodecID = AVCodecID(94234);
    pub const AV_CODEC_ID_FIRST_UNKNOWN: AVCodecID = AVCodecID(98304);
    pub const AV_CODEC_ID_TTF: AVCodecID = AVCodecID(98304);
    pub const AV_CODEC_ID_SCTE_35: AVCodecID = AVCodecID(98305);
    pub const AV_CODEC_ID_EPG: AVCodecID = AVCodecID(98306);
    pub const AV_CODEC_ID_BINTEXT: AVCodecID = AVCodecID(98307);
    pub const AV_CODEC_ID_XBIN: AVCodecID = AVCodecID(98308);
    pub const AV_CODEC_ID_IDF: AVCodecID = AVCodecID(98309);
    pub const AV_CODEC_ID_OTF: AVCodecID = AVCodecID(98310);
    pub const AV_CODEC_ID_SMPTE_KLV: AVCodecID = AVCodecID(98311);
    pub const AV_CODEC_ID_DVD_NAV: AVCodecID = AVCodecID(98312);
    pub const AV_CODEC_ID_TIMED_ID3: AVCodecID = AVCodecID(98313);
    pub const AV_CODEC_ID_BIN_DATA: AVCodecID = AVCodecID(98314);
    pub const AV_CODEC_ID_SMPTE_2038: AVCodecID = AVCodecID(98315);
    pub const AV_CODEC_ID_LCEVC: AVCodecID = AVCodecID(98316);
    pub const AV_CODEC_ID_SMPTE_436M_ANC: AVCodecID = AVCodecID(98317);
    pub const AV_CODEC_ID_PROBE: AVCodecID = AVCodecID(102400);
    pub const AV_CODEC_ID_MPEG2TS: AVCodecID = AVCodecID(131072);
    pub const AV_CODEC_ID_MPEG4SYSTEMS: AVCodecID = AVCodecID(131073);
    pub const AV_CODEC_ID_FFMETADATA: AVCodecID = AVCodecID(135168);
    pub const AV_CODEC_ID_WRAPPED_AVFRAME: AVCodecID = AVCodecID(135169);
    pub const AV_CODEC_ID_VNULL: AVCodecID = AVCodecID(135170);
    pub const AV_CODEC_ID_ANULL: AVCodecID = AVCodecID(135171);
}
#[repr(transparent)]
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct AVCodecID(pub ::std::os::raw::c_int);
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct AVProfile {
    pub profile: ::std::os::raw::c_int,
    pub name: *const ::std::os::raw::c_char,
}
impl Default for AVProfile {
    fn default() -> Self {
        let mut s = ::std::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            ::std::ptr::write_bytes(s.as_mut_ptr(), 0, 1);
            s.assume_init()
        }
    }
}
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct AVCodec {
    pub name: *const ::std::os::raw::c_char,
    pub long_name: *const ::std::os::raw::c_char,
    pub type_: AVMediaType,
    pub id: AVCodecID,
    pub capabilities: ::std::os::raw::c_int,
    pub max_lowres: u8,
    pub supported_framerates: *const AVRational,
    pub pix_fmts: *const AVPixelFormat,
    pub supported_samplerates: *const ::std::os::raw::c_int,
    pub sample_fmts: *const AVSampleFormat,
    pub priv_class: *const AVClass,
    pub profiles: *const AVProfile,
    pub wrapper_name: *const ::std::os::raw::c_char,
    pub ch_layouts: *const AVChannelLayout,
}
impl Default for AVCodec {
    fn default() -> Self {
        let mut s = ::std::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            ::std::ptr::write_bytes(s.as_mut_ptr(), 0, 1);
            s.assume_init()
        }
    }
}
unsafe extern "C" {
    pub fn avcodec_find_decoder(id: AVCodecID) -> *const AVCodec;
}
unsafe extern "C" {
    pub fn avcodec_find_decoder_by_name(name: *const ::std::os::raw::c_char) -> *const AVCodec;
}
pub const AV_CODEC_HW_CONFIG_METHOD_HW_DEVICE_CTX: _bindgen_ty_3 = _bindgen_ty_3(1);
pub const AV_CODEC_HW_CONFIG_METHOD_HW_FRAMES_CTX: _bindgen_ty_3 = _bindgen_ty_3(2);
pub const AV_CODEC_HW_CONFIG_METHOD_INTERNAL: _bindgen_ty_3 = _bindgen_ty_3(4);
pub const AV_CODEC_HW_CONFIG_METHOD_AD_HOC: _bindgen_ty_3 = _bindgen_ty_3(8);
#[repr(transparent)]
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct _bindgen_ty_3(pub ::std::os::raw::c_int);
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct AVCodecHWConfig {
    pub pix_fmt: AVPixelFormat,
    pub methods: ::std::os::raw::c_int,
    pub device_type: AVHWDeviceType,
}
impl Default for AVCodecHWConfig {
    fn default() -> Self {
        let mut s = ::std::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            ::std::ptr::write_bytes(s.as_mut_ptr(), 0, 1);
            s.assume_init()
        }
    }
}
unsafe extern "C" {
    pub fn avcodec_get_hw_config(
        codec: *const AVCodec,
        index: ::std::os::raw::c_int,
    ) -> *const AVCodecHWConfig;
}
impl AVFieldOrder {
    pub const AV_FIELD_UNKNOWN: AVFieldOrder = AVFieldOrder(0);
    pub const AV_FIELD_PROGRESSIVE: AVFieldOrder = AVFieldOrder(1);
    pub const AV_FIELD_TT: AVFieldOrder = AVFieldOrder(2);
    pub const AV_FIELD_BB: AVFieldOrder = AVFieldOrder(3);
    pub const AV_FIELD_TB: AVFieldOrder = AVFieldOrder(4);
    pub const AV_FIELD_BT: AVFieldOrder = AVFieldOrder(5);
}
#[repr(transparent)]
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct AVFieldOrder(pub ::std::os::raw::c_int);
impl AVDiscard {
    pub const AVDISCARD_NONE: AVDiscard = AVDiscard(-16);
    pub const AVDISCARD_DEFAULT: AVDiscard = AVDiscard(0);
    pub const AVDISCARD_NONREF: AVDiscard = AVDiscard(8);
    pub const AVDISCARD_BIDIR: AVDiscard = AVDiscard(16);
    pub const AVDISCARD_NONINTRA: AVDiscard = AVDiscard(24);
    pub const AVDISCARD_NONKEY: AVDiscard = AVDiscard(32);
    pub const AVDISCARD_ALL: AVDiscard = AVDiscard(48);
}
#[repr(transparent)]
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct AVDiscard(pub ::std::os::raw::c_int);
impl AVAudioServiceType {
    pub const AV_AUDIO_SERVICE_TYPE_MAIN: AVAudioServiceType = AVAudioServiceType(0);
    pub const AV_AUDIO_SERVICE_TYPE_EFFECTS: AVAudioServiceType = AVAudioServiceType(1);
    pub const AV_AUDIO_SERVICE_TYPE_VISUALLY_IMPAIRED: AVAudioServiceType = AVAudioServiceType(2);
    pub const AV_AUDIO_SERVICE_TYPE_HEARING_IMPAIRED: AVAudioServiceType = AVAudioServiceType(3);
    pub const AV_AUDIO_SERVICE_TYPE_DIALOGUE: AVAudioServiceType = AVAudioServiceType(4);
    pub const AV_AUDIO_SERVICE_TYPE_COMMENTARY: AVAudioServiceType = AVAudioServiceType(5);
    pub const AV_AUDIO_SERVICE_TYPE_EMERGENCY: AVAudioServiceType = AVAudioServiceType(6);
    pub const AV_AUDIO_SERVICE_TYPE_VOICE_OVER: AVAudioServiceType = AVAudioServiceType(7);
    pub const AV_AUDIO_SERVICE_TYPE_KARAOKE: AVAudioServiceType = AVAudioServiceType(8);
    pub const AV_AUDIO_SERVICE_TYPE_NB: AVAudioServiceType = AVAudioServiceType(9);
}
#[repr(transparent)]
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct AVAudioServiceType(pub ::std::os::raw::c_int);
impl AVPacketSideDataType {
    pub const AV_PKT_DATA_PALETTE: AVPacketSideDataType = AVPacketSideDataType(0);
    pub const AV_PKT_DATA_NEW_EXTRADATA: AVPacketSideDataType = AVPacketSideDataType(1);
    pub const AV_PKT_DATA_PARAM_CHANGE: AVPacketSideDataType = AVPacketSideDataType(2);
    pub const AV_PKT_DATA_H263_MB_INFO: AVPacketSideDataType = AVPacketSideDataType(3);
    pub const AV_PKT_DATA_REPLAYGAIN: AVPacketSideDataType = AVPacketSideDataType(4);
    pub const AV_PKT_DATA_DISPLAYMATRIX: AVPacketSideDataType = AVPacketSideDataType(5);
    pub const AV_PKT_DATA_STEREO3D: AVPacketSideDataType = AVPacketSideDataType(6);
    pub const AV_PKT_DATA_AUDIO_SERVICE_TYPE: AVPacketSideDataType = AVPacketSideDataType(7);
    pub const AV_PKT_DATA_QUALITY_STATS: AVPacketSideDataType = AVPacketSideDataType(8);
    pub const AV_PKT_DATA_FALLBACK_TRACK: AVPacketSideDataType = AVPacketSideDataType(9);
    pub const AV_PKT_DATA_CPB_PROPERTIES: AVPacketSideDataType = AVPacketSideDataType(10);
    pub const AV_PKT_DATA_SKIP_SAMPLES: AVPacketSideDataType = AVPacketSideDataType(11);
    pub const AV_PKT_DATA_JP_DUALMONO: AVPacketSideDataType = AVPacketSideDataType(12);
    pub const AV_PKT_DATA_STRINGS_METADATA: AVPacketSideDataType = AVPacketSideDataType(13);
    pub const AV_PKT_DATA_SUBTITLE_POSITION: AVPacketSideDataType = AVPacketSideDataType(14);
    pub const AV_PKT_DATA_MATROSKA_BLOCKADDITIONAL: AVPacketSideDataType = AVPacketSideDataType(15);
    pub const AV_PKT_DATA_WEBVTT_IDENTIFIER: AVPacketSideDataType = AVPacketSideDataType(16);
    pub const AV_PKT_DATA_WEBVTT_SETTINGS: AVPacketSideDataType = AVPacketSideDataType(17);
    pub const AV_PKT_DATA_METADATA_UPDATE: AVPacketSideDataType = AVPacketSideDataType(18);
    pub const AV_PKT_DATA_MPEGTS_STREAM_ID: AVPacketSideDataType = AVPacketSideDataType(19);
    pub const AV_PKT_DATA_MASTERING_DISPLAY_METADATA: AVPacketSideDataType =
        AVPacketSideDataType(20);
    pub const AV_PKT_DATA_SPHERICAL: AVPacketSideDataType = AVPacketSideDataType(21);
    pub const AV_PKT_DATA_CONTENT_LIGHT_LEVEL: AVPacketSideDataType = AVPacketSideDataType(22);
    pub const AV_PKT_DATA_A53_CC: AVPacketSideDataType = AVPacketSideDataType(23);
    pub const AV_PKT_DATA_ENCRYPTION_INIT_INFO: AVPacketSideDataType = AVPacketSideDataType(24);
    pub const AV_PKT_DATA_ENCRYPTION_INFO: AVPacketSideDataType = AVPacketSideDataType(25);
    pub const AV_PKT_DATA_AFD: AVPacketSideDataType = AVPacketSideDataType(26);
    pub const AV_PKT_DATA_PRFT: AVPacketSideDataType = AVPacketSideDataType(27);
    pub const AV_PKT_DATA_ICC_PROFILE: AVPacketSideDataType = AVPacketSideDataType(28);
    pub const AV_PKT_DATA_DOVI_CONF: AVPacketSideDataType = AVPacketSideDataType(29);
    pub const AV_PKT_DATA_S12M_TIMECODE: AVPacketSideDataType = AVPacketSideDataType(30);
    pub const AV_PKT_DATA_DYNAMIC_HDR10_PLUS: AVPacketSideDataType = AVPacketSideDataType(31);
    pub const AV_PKT_DATA_IAMF_MIX_GAIN_PARAM: AVPacketSideDataType = AVPacketSideDataType(32);
    pub const AV_PKT_DATA_IAMF_DEMIXING_INFO_PARAM: AVPacketSideDataType = AVPacketSideDataType(33);
    pub const AV_PKT_DATA_IAMF_RECON_GAIN_INFO_PARAM: AVPacketSideDataType =
        AVPacketSideDataType(34);
    pub const AV_PKT_DATA_AMBIENT_VIEWING_ENVIRONMENT: AVPacketSideDataType =
        AVPacketSideDataType(35);
    pub const AV_PKT_DATA_FRAME_CROPPING: AVPacketSideDataType = AVPacketSideDataType(36);
    pub const AV_PKT_DATA_LCEVC: AVPacketSideDataType = AVPacketSideDataType(37);
    pub const AV_PKT_DATA_3D_REFERENCE_DISPLAYS: AVPacketSideDataType = AVPacketSideDataType(38);
    pub const AV_PKT_DATA_RTCP_SR: AVPacketSideDataType = AVPacketSideDataType(39);
    pub const AV_PKT_DATA_NB: AVPacketSideDataType = AVPacketSideDataType(40);
}
#[repr(transparent)]
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct AVPacketSideDataType(pub ::std::os::raw::c_int);
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct AVPacketSideData {
    pub data: *mut u8,
    pub size: usize,
    pub type_: AVPacketSideDataType,
}
impl Default for AVPacketSideData {
    fn default() -> Self {
        let mut s = ::std::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            ::std::ptr::write_bytes(s.as_mut_ptr(), 0, 1);
            s.assume_init()
        }
    }
}
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct AVPacket {
    pub buf: *mut AVBufferRef,
    pub pts: i64,
    pub dts: i64,
    pub data: *mut u8,
    pub size: ::std::os::raw::c_int,
    pub stream_index: ::std::os::raw::c_int,
    pub flags: ::std::os::raw::c_int,
    pub side_data: *mut AVPacketSideData,
    pub side_data_elems: ::std::os::raw::c_int,
    pub duration: i64,
    pub pos: i64,
    pub opaque: *mut ::std::os::raw::c_void,
    pub opaque_ref: *mut AVBufferRef,
    pub time_base: AVRational,
}
impl Default for AVPacket {
    fn default() -> Self {
        let mut s = ::std::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            ::std::ptr::write_bytes(s.as_mut_ptr(), 0, 1);
            s.assume_init()
        }
    }
}
unsafe extern "C" {
    pub fn av_packet_alloc() -> *mut AVPacket;
}
unsafe extern "C" {
    pub fn av_packet_free(pkt: *mut *mut AVPacket);
}
unsafe extern "C" {
    pub fn av_new_packet(pkt: *mut AVPacket, size: ::std::os::raw::c_int) -> ::std::os::raw::c_int;
}
unsafe extern "C" {
    pub fn av_packet_unref(pkt: *mut AVPacket);
}
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct AVCodecDescriptor {
    pub id: AVCodecID,
    pub type_: AVMediaType,
    pub name: *const ::std::os::raw::c_char,
    pub long_name: *const ::std::os::raw::c_char,
    pub props: ::std::os::raw::c_int,
    pub mime_types: *const *const ::std::os::raw::c_char,
    pub profiles: *const AVProfile,
}
impl Default for AVCodecDescriptor {
    fn default() -> Self {
        let mut s = ::std::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            ::std::ptr::write_bytes(s.as_mut_ptr(), 0, 1);
            s.assume_init()
        }
    }
}
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct RcOverride {
    pub start_frame: ::std::os::raw::c_int,
    pub end_frame: ::std::os::raw::c_int,
    pub qscale: ::std::os::raw::c_int,
    pub quality_factor: f32,
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct AVCodecContext {
    pub av_class: *const AVClass,
    pub log_level_offset: ::std::os::raw::c_int,
    pub codec_type: AVMediaType,
    pub codec: *const AVCodec,
    pub codec_id: AVCodecID,
    pub codec_tag: ::std::os::raw::c_uint,
    pub priv_data: *mut ::std::os::raw::c_void,
    pub internal: *mut AVCodecInternal,
    pub opaque: *mut ::std::os::raw::c_void,
    pub bit_rate: i64,
    pub flags: ::std::os::raw::c_int,
    pub flags2: ::std::os::raw::c_int,
    pub extradata: *mut u8,
    pub extradata_size: ::std::os::raw::c_int,
    pub time_base: AVRational,
    pub pkt_timebase: AVRational,
    pub framerate: AVRational,
    pub delay: ::std::os::raw::c_int,
    pub width: ::std::os::raw::c_int,
    pub height: ::std::os::raw::c_int,
    pub coded_width: ::std::os::raw::c_int,
    pub coded_height: ::std::os::raw::c_int,
    pub sample_aspect_ratio: AVRational,
    pub pix_fmt: AVPixelFormat,
    pub sw_pix_fmt: AVPixelFormat,
    pub color_primaries: AVColorPrimaries,
    pub color_trc: AVColorTransferCharacteristic,
    pub colorspace: AVColorSpace,
    pub color_range: AVColorRange,
    pub chroma_sample_location: AVChromaLocation,
    pub field_order: AVFieldOrder,
    pub refs: ::std::os::raw::c_int,
    pub has_b_frames: ::std::os::raw::c_int,
    pub slice_flags: ::std::os::raw::c_int,
    pub draw_horiz_band: ::std::option::Option<
        unsafe extern "C" fn(
            s: *mut AVCodecContext,
            src: *const AVFrame,
            offset: *mut ::std::os::raw::c_int,
            y: ::std::os::raw::c_int,
            type_: ::std::os::raw::c_int,
            height: ::std::os::raw::c_int,
        ),
    >,
    pub get_format: ::std::option::Option<
        unsafe extern "C" fn(s: *mut AVCodecContext, fmt: *const AVPixelFormat) -> AVPixelFormat,
    >,
    pub max_b_frames: ::std::os::raw::c_int,
    pub b_quant_factor: f32,
    pub b_quant_offset: f32,
    pub i_quant_factor: f32,
    pub i_quant_offset: f32,
    pub lumi_masking: f32,
    pub temporal_cplx_masking: f32,
    pub spatial_cplx_masking: f32,
    pub p_masking: f32,
    pub dark_masking: f32,
    pub nsse_weight: ::std::os::raw::c_int,
    pub me_cmp: ::std::os::raw::c_int,
    pub me_sub_cmp: ::std::os::raw::c_int,
    pub mb_cmp: ::std::os::raw::c_int,
    pub ildct_cmp: ::std::os::raw::c_int,
    pub dia_size: ::std::os::raw::c_int,
    pub last_predictor_count: ::std::os::raw::c_int,
    pub me_pre_cmp: ::std::os::raw::c_int,
    pub pre_dia_size: ::std::os::raw::c_int,
    pub me_subpel_quality: ::std::os::raw::c_int,
    pub me_range: ::std::os::raw::c_int,
    pub mb_decision: ::std::os::raw::c_int,
    pub intra_matrix: *mut u16,
    pub inter_matrix: *mut u16,
    pub chroma_intra_matrix: *mut u16,
    pub intra_dc_precision: ::std::os::raw::c_int,
    pub mb_lmin: ::std::os::raw::c_int,
    pub mb_lmax: ::std::os::raw::c_int,
    pub bidir_refine: ::std::os::raw::c_int,
    pub keyint_min: ::std::os::raw::c_int,
    pub gop_size: ::std::os::raw::c_int,
    pub mv0_threshold: ::std::os::raw::c_int,
    pub slices: ::std::os::raw::c_int,
    pub sample_rate: ::std::os::raw::c_int,
    pub sample_fmt: AVSampleFormat,
    pub ch_layout: AVChannelLayout,
    pub frame_size: ::std::os::raw::c_int,
    pub block_align: ::std::os::raw::c_int,
    pub cutoff: ::std::os::raw::c_int,
    pub audio_service_type: AVAudioServiceType,
    pub request_sample_fmt: AVSampleFormat,
    pub initial_padding: ::std::os::raw::c_int,
    pub trailing_padding: ::std::os::raw::c_int,
    pub seek_preroll: ::std::os::raw::c_int,
    pub get_buffer2: ::std::option::Option<
        unsafe extern "C" fn(
            s: *mut AVCodecContext,
            frame: *mut AVFrame,
            flags: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int,
    >,
    pub bit_rate_tolerance: ::std::os::raw::c_int,
    pub global_quality: ::std::os::raw::c_int,
    pub compression_level: ::std::os::raw::c_int,
    pub qcompress: f32,
    pub qblur: f32,
    pub qmin: ::std::os::raw::c_int,
    pub qmax: ::std::os::raw::c_int,
    pub max_qdiff: ::std::os::raw::c_int,
    pub rc_buffer_size: ::std::os::raw::c_int,
    pub rc_override_count: ::std::os::raw::c_int,
    pub rc_override: *mut RcOverride,
    pub rc_max_rate: i64,
    pub rc_min_rate: i64,
    pub rc_max_available_vbv_use: f32,
    pub rc_min_vbv_overflow_use: f32,
    pub rc_initial_buffer_occupancy: ::std::os::raw::c_int,
    pub trellis: ::std::os::raw::c_int,
    pub stats_out: *mut ::std::os::raw::c_char,
    pub stats_in: *mut ::std::os::raw::c_char,
    pub workaround_bugs: ::std::os::raw::c_int,
    pub strict_std_compliance: ::std::os::raw::c_int,
    pub error_concealment: ::std::os::raw::c_int,
    pub debug: ::std::os::raw::c_int,
    pub err_recognition: ::std::os::raw::c_int,
    pub hwaccel: *const AVHWAccel,
    pub hwaccel_context: *mut ::std::os::raw::c_void,
    pub hw_frames_ctx: *mut AVBufferRef,
    pub hw_device_ctx: *mut AVBufferRef,
    pub hwaccel_flags: ::std::os::raw::c_int,
    pub extra_hw_frames: ::std::os::raw::c_int,
    pub error: [u64; 8usize],
    pub dct_algo: ::std::os::raw::c_int,
    pub idct_algo: ::std::os::raw::c_int,
    pub bits_per_coded_sample: ::std::os::raw::c_int,
    pub bits_per_raw_sample: ::std::os::raw::c_int,
    pub thread_count: ::std::os::raw::c_int,
    pub thread_type: ::std::os::raw::c_int,
    pub active_thread_type: ::std::os::raw::c_int,
    pub execute: ::std::option::Option<
        unsafe extern "C" fn(
            c: *mut AVCodecContext,
            func: ::std::option::Option<
                unsafe extern "C" fn(
                    c2: *mut AVCodecContext,
                    arg: *mut ::std::os::raw::c_void,
                ) -> ::std::os::raw::c_int,
            >,
            arg2: *mut ::std::os::raw::c_void,
            ret: *mut ::std::os::raw::c_int,
            count: ::std::os::raw::c_int,
            size: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int,
    >,
    pub execute2: ::std::option::Option<
        unsafe extern "C" fn(
            c: *mut AVCodecContext,
            func: ::std::option::Option<
                unsafe extern "C" fn(
                    c2: *mut AVCodecContext,
                    arg: *mut ::std::os::raw::c_void,
                    jobnr: ::std::os::raw::c_int,
                    threadnr: ::std::os::raw::c_int,
                ) -> ::std::os::raw::c_int,
            >,
            arg2: *mut ::std::os::raw::c_void,
            ret: *mut ::std::os::raw::c_int,
            count: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int,
    >,
    pub profile: ::std::os::raw::c_int,
    pub level: ::std::os::raw::c_int,
    pub properties: ::std::os::raw::c_uint,
    pub skip_loop_filter: AVDiscard,
    pub skip_idct: AVDiscard,
    pub skip_frame: AVDiscard,
    pub skip_alpha: ::std::os::raw::c_int,
    pub skip_top: ::std::os::raw::c_int,
    pub skip_bottom: ::std::os::raw::c_int,
    pub lowres: ::std::os::raw::c_int,
    pub codec_descriptor: *const AVCodecDescriptor,
    pub sub_charenc: *mut ::std::os::raw::c_char,
    pub sub_charenc_mode: ::std::os::raw::c_int,
    pub subtitle_header_size: ::std::os::raw::c_int,
    pub subtitle_header: *mut u8,
    pub dump_separator: *mut u8,
    pub codec_whitelist: *mut ::std::os::raw::c_char,
    pub coded_side_data: *mut AVPacketSideData,
    pub nb_coded_side_data: ::std::os::raw::c_int,
    pub export_side_data: ::std::os::raw::c_int,
    pub max_pixels: i64,
    pub apply_cropping: ::std::os::raw::c_int,
    pub discard_damaged_percentage: ::std::os::raw::c_int,
    pub max_samples: i64,
    pub get_encode_buffer: ::std::option::Option<
        unsafe extern "C" fn(
            s: *mut AVCodecContext,
            pkt: *mut AVPacket,
            flags: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int,
    >,
    pub frame_num: i64,
    pub side_data_prefer_packet: *mut ::std::os::raw::c_int,
    pub nb_side_data_prefer_packet: ::std::os::raw::c_uint,
    pub decoded_side_data: *mut *mut AVFrameSideData,
    pub nb_decoded_side_data: ::std::os::raw::c_int,
}
impl Default for AVCodecContext {
    fn default() -> Self {
        let mut s = ::std::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            ::std::ptr::write_bytes(s.as_mut_ptr(), 0, 1);
            s.assume_init()
        }
    }
}
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct AVHWAccel {
    pub name: *const ::std::os::raw::c_char,
    pub type_: AVMediaType,
    pub id: AVCodecID,
    pub pix_fmt: AVPixelFormat,
    pub capabilities: ::std::os::raw::c_int,
}
impl Default for AVHWAccel {
    fn default() -> Self {
        let mut s = ::std::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            ::std::ptr::write_bytes(s.as_mut_ptr(), 0, 1);
            s.assume_init()
        }
    }
}
unsafe extern "C" {
    pub fn avcodec_version() -> ::std::os::raw::c_uint;
}
unsafe extern "C" {
    pub fn avcodec_alloc_context3(codec: *const AVCodec) -> *mut AVCodecContext;
}
unsafe extern "C" {
    pub fn avcodec_free_context(avctx: *mut *mut AVCodecContext);
}
unsafe extern "C" {
    pub fn avcodec_open2(
        avctx: *mut AVCodecContext,
        codec: *const AVCodec,
        options: *mut *mut AVDictionary,
    ) -> ::std::os::raw::c_int;
}
unsafe extern "C" {
    pub fn avcodec_send_packet(
        avctx: *mut AVCodecContext,
        avpkt: *const AVPacket,
    ) -> ::std::os::raw::c_int;
}
unsafe extern "C" {
    pub fn avcodec_receive_frame(
        avctx: *mut AVCodecContext,
        frame: *mut AVFrame,
    ) -> ::std::os::raw::c_int;
}
impl AVPictureStructure {
    pub const AV_PICTURE_STRUCTURE_UNKNOWN: AVPictureStructure = AVPictureStructure(0);
    pub const AV_PICTURE_STRUCTURE_TOP_FIELD: AVPictureStructure = AVPictureStructure(1);
    pub const AV_PICTURE_STRUCTURE_BOTTOM_FIELD: AVPictureStructure = AVPictureStructure(2);
    pub const AV_PICTURE_STRUCTURE_FRAME: AVPictureStructure = AVPictureStructure(3);
}
#[repr(transparent)]
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct AVPictureStructure(pub ::std::os::raw::c_int);
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct AVCodecParserContext {
    pub priv_data: *mut ::std::os::raw::c_void,
    pub parser: *const AVCodecParser,
    pub frame_offset: i64,
    pub cur_offset: i64,
    pub next_frame_offset: i64,
    pub pict_type: ::std::os::raw::c_int,
    pub repeat_pict: ::std::os::raw::c_int,
    pub pts: i64,
    pub dts: i64,
    pub last_pts: i64,
    pub last_dts: i64,
    pub fetch_timestamp: ::std::os::raw::c_int,
    pub cur_frame_start_index: ::std::os::raw::c_int,
    pub cur_frame_offset: [i64; 4usize],
    pub cur_frame_pts: [i64; 4usize],
    pub cur_frame_dts: [i64; 4usize],
    pub flags: ::std::os::raw::c_int,
    pub offset: i64,
    pub cur_frame_end: [i64; 4usize],
    pub key_frame: ::std::os::raw::c_int,
    pub dts_sync_point: ::std::os::raw::c_int,
    pub dts_ref_dts_delta: ::std::os::raw::c_int,
    pub pts_dts_delta: ::std::os::raw::c_int,
    pub cur_frame_pos: [i64; 4usize],
    pub pos: i64,
    pub last_pos: i64,
    pub duration: ::std::os::raw::c_int,
    pub field_order: AVFieldOrder,
    pub picture_structure: AVPictureStructure,
    pub output_picture_number: ::std::os::raw::c_int,
    pub width: ::std::os::raw::c_int,
    pub height: ::std::os::raw::c_int,
    pub coded_width: ::std::os::raw::c_int,
    pub coded_height: ::std::os::raw::c_int,
    pub format: ::std::os::raw::c_int,
}
impl Default for AVCodecParserContext {
    fn default() -> Self {
        let mut s = ::std::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            ::std::ptr::write_bytes(s.as_mut_ptr(), 0, 1);
            s.assume_init()
        }
    }
}
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct AVCodecParser {
    pub codec_ids: [::std::os::raw::c_int; 7usize],
    pub priv_data_size: ::std::os::raw::c_int,
    pub parser_init: ::std::option::Option<
        unsafe extern "C" fn(s: *mut AVCodecParserContext) -> ::std::os::raw::c_int,
    >,
    pub parser_parse: ::std::option::Option<
        unsafe extern "C" fn(
            s: *mut AVCodecParserContext,
            avctx: *mut AVCodecContext,
            poutbuf: *mut *const u8,
            poutbuf_size: *mut ::std::os::raw::c_int,
            buf: *const u8,
            buf_size: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int,
    >,
    pub parser_close: ::std::option::Option<unsafe extern "C" fn(s: *mut AVCodecParserContext)>,
    pub split: ::std::option::Option<
        unsafe extern "C" fn(
            avctx: *mut AVCodecContext,
            buf: *const u8,
            buf_size: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int,
    >,
}
unsafe extern "C" {
    pub fn av_parser_init(codec_id: ::std::os::raw::c_int) -> *mut AVCodecParserContext;
}
unsafe extern "C" {
    pub fn av_parser_parse2(
        s: *mut AVCodecParserContext,
        avctx: *mut AVCodecContext,
        poutbuf: *mut *mut u8,
        poutbuf_size: *mut ::std::os::raw::c_int,
        buf: *const u8,
        buf_size: ::std::os::raw::c_int,
        pts: i64,
        dts: i64,
        pos: i64,
    ) -> ::std::os::raw::c_int;
}
unsafe extern "C" {
    pub fn av_parser_close(s: *mut AVCodecParserContext);
}
unsafe extern "C" {
    pub fn avcodec_flush_buffers(avctx: *mut AVCodecContext);
}
unsafe extern "C" {
    pub fn av_get_pix_fmt_name(pix_fmt: AVPixelFormat) -> *const ::std::os::raw::c_char;
}
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct AVOption {
    pub _address: u8,
}
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct AVCodecInternal {
    pub _address: u8,
}
