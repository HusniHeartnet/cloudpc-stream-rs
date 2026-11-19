//! Bagian FFmpeg yang sama di semua sistem: galat, konversi ke NV12, dan decoder yang mengeluarkan gambar di memori
//! (`CpuDecoder`) untuk tampilan SDL (macOS, dan Windows bila `--frontend sdl`). Di macOS decode dikerjakan
//! VideoToolbox lalu disalin ke memori (memori Apple Silicon dibagi CPU/GPU, jadi salinannya murah); tanpa VideoToolbox
//! atau dengan `--video-decoder software` decode dikerjakan CPU, seperti Moonlight.

use std::ffi::CStr;
use std::ptr;

use super::ffi::*;
use crate::rtsp::Codec;

/// `AVERROR(EAGAIN)`: EAGAIN = 35 di macOS, 11 di Windows dan Linux.
#[cfg(target_os = "macos")]
pub const AVERROR_EAGAIN: i32 = -35;
#[cfg(not(target_os = "macos"))]
pub const AVERROR_EAGAIN: i32 = -11;

/// `FF_THREAD_SLICE` (libavcodec/avcodec.h).
pub const FF_THREAD_SLICE: i32 = 2;

pub fn error_text(code: i32) -> String {
    let mut buffer = [0 as std::os::raw::c_char; AV_ERROR_MAX_STRING_SIZE as usize];
    unsafe {
        av_strerror(code, buffer.as_mut_ptr(), buffer.len());
        CStr::from_ptr(buffer.as_ptr()).to_string_lossy().into_owned()
    }
}

/// Versi libavcodec yang dimuat (untuk log), mis. "62.11.100".
pub fn ffmpeg_version() -> String {
    let v = unsafe { avcodec_version() };
    format!("{}.{}.{}", v >> 16, (v >> 8) & 0xff, v & 0xff)
}

/// Frame perangkat lunak (YUV 4:2:0 tiga bidang atau NV12) -> satu buffer NV12 rapat (lebar baris = lebar gambar).
///
/// # Safety
/// `frame` harus frame FFmpeg yang sah dengan data di memori.
pub unsafe fn to_nv12(frame: &AVFrame) -> Result<Vec<u8>, String> {
    let (w, h) = (frame.width as usize, frame.height as usize);
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    let mut out = vec![0u8; w * h + cw * 2 * ch];
    for y in 0..h {
        let row = std::slice::from_raw_parts(frame.data[0].add(y * frame.linesize[0] as usize), w);
        out[y * w..y * w + w].copy_from_slice(row);
    }
    let uv = &mut out[w * h..];
    let format = AVPixelFormat(frame.format);
    if format == AVPixelFormat::AV_PIX_FMT_NV12 {
        for y in 0..ch {
            let row = std::slice::from_raw_parts(frame.data[1].add(y * frame.linesize[1] as usize), cw * 2);
            uv[y * cw * 2..(y + 1) * cw * 2].copy_from_slice(row);
        }
    } else if format == AVPixelFormat::AV_PIX_FMT_YUV420P || format == AVPixelFormat::AV_PIX_FMT_YUVJ420P {
        for y in 0..ch {
            let u = std::slice::from_raw_parts(frame.data[1].add(y * frame.linesize[1] as usize), cw);
            let v = std::slice::from_raw_parts(frame.data[2].add(y * frame.linesize[2] as usize), cw);
            let dst = &mut uv[y * cw * 2..(y + 1) * cw * 2];
            for x in 0..cw {
                dst[2 * x] = u[x];
                dst[2 * x + 1] = v[x];
            }
        }
    } else {
        let name = av_get_pix_fmt_name(format);
        let name = if name.is_null() { "?".into() } else { CStr::from_ptr(name).to_string_lossy().into_owned() };
        return Err(format!("format piksel {name} belum didukung (hanya 8-bit 4:2:0)"));
    }
    Ok(out)
}

/// Gambar NV12 di memori.
pub struct CpuPicture {
    pub width: u32,
    pub height: u32,
    pub nv12: Vec<u8>,
    pub bt601: bool,
    pub full_range: bool,
}

pub struct CpuDecoder {
    context: *mut AVCodecContext,
    packet: *mut AVPacket,
    frame: *mut AVFrame,
    transfer: *mut AVFrame,
    pub name: String,
    pub hardware: bool,
    codec: Codec,
    /// Gambar yang sudah keluar; selama masih 0, galat hardware = pindah ke CPU (seperti Moonlight mencoba decoder
    /// berikutnya). Contoh: runner CI macOS mengaku punya AV1 hardware tetapi inisialisasinya gagal di frame pertama.
    decoded: u64,
}

// Dipakai dari satu thread pada satu waktu.
unsafe impl Send for CpuDecoder {}

/// Perangkat decode hardware untuk sistem ini (`None` = hanya CPU).
fn hardware_device() -> Option<AVHWDeviceType> {
    if cfg!(target_os = "macos") {
        Some(AVHWDeviceType::AV_HWDEVICE_TYPE_VIDEOTOOLBOX)
    } else {
        None
    }
}

#[cfg(target_os = "macos")]
#[link(name = "VideoToolbox", kind = "framework")]
unsafe extern "C" {
    fn VTIsHardwareDecodeSupported(codec_type: u32) -> u8;
}

/// Codec yang bisa didecode hardware mesin ini, ditanyakan ke sistem seperti Moonlight (`VTIsHardwareDecodeSupported`
/// di macOS: AV1 hanya chip M3 ke atas). Selain macOS: kosong (tampilan SDL di sana mendecode di CPU).
pub fn hardware_codecs() -> Vec<Codec> {
    #[cfg(target_os = "macos")]
    {
        // kCMVideoCodecType_H264 'avc1', _HEVC 'hvc1', _AV1 'av01'.
        [(Codec::H264, 0x6176_6331u32), (Codec::Hevc, 0x6876_6331), (Codec::Av1, 0x6176_3031)]
            .into_iter()
            .filter(|(_, fourcc)| unsafe { VTIsHardwareDecodeSupported(*fourcc) } != 0)
            .map(|(codec, _)| codec)
            .collect()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}

impl CpuDecoder {
    /// `hardware`: coba decode hardware (VideoToolbox di macOS); bila gagal dibuka, otomatis ke CPU.
    pub fn new(codec: Codec, hardware: bool) -> Result<CpuDecoder, String> {
        if hardware && hardware_codecs().contains(&codec) {
            if let Some(device) = hardware_device() {
                if let Ok(decoder) = Self::open(codec, Some(device)) {
                    return Ok(decoder);
                }
            }
        }
        Self::open(codec, None)
    }

    fn open(codec: Codec, device: Option<AVHWDeviceType>) -> Result<CpuDecoder, String> {
        unsafe {
            av_log_set_level(AV_LOG_ERROR as i32);
            let decoder = match codec {
                Codec::H264 => avcodec_find_decoder(AVCodecID::AV_CODEC_ID_H264),
                Codec::Hevc => avcodec_find_decoder(AVCodecID::AV_CODEC_ID_HEVC),
                // Decoder "av1" bawaan FFmpeg hanya jalan dengan hardware; di CPU dipakai dav1d.
                Codec::Av1 if device.is_some() => avcodec_find_decoder_by_name(c"av1".as_ptr()),
                Codec::Av1 => avcodec_find_decoder_by_name(c"libdav1d".as_ptr()),
            };
            if decoder.is_null() {
                return Err(format!("FFmpeg tidak punya decoder {codec:?}"));
            }
            let name = CStr::from_ptr((*decoder).name).to_string_lossy().into_owned();
            let context = avcodec_alloc_context3(decoder);
            (*context).flags |= AV_CODEC_FLAG_LOW_DELAY as i32;
            if let Some(device) = device {
                let mut hw: *mut AVBufferRef = ptr::null_mut();
                let status = av_hwdevice_ctx_create(&mut hw, device, ptr::null(), ptr::null_mut(), 0);
                if status < 0 {
                    let mut context = context;
                    avcodec_free_context(&mut context);
                    return Err(format!("decoder hardware tidak tersedia: {}", error_text(status)));
                }
                (*context).hw_device_ctx = hw;
                (*context).thread_count = 1;
            } else {
                (*context).thread_type = FF_THREAD_SLICE;
                (*context).thread_count = 0;
            }
            let status = avcodec_open2(context, decoder, ptr::null_mut());
            if status < 0 {
                let mut context = context;
                avcodec_free_context(&mut context);
                return Err(format!("decoder {name} gagal dibuka: {}", error_text(status)));
            }
            Ok(CpuDecoder { context, packet: av_packet_alloc(), frame: av_frame_alloc(), transfer: av_frame_alloc(), name, hardware: device.is_some(), codec, decoded: 0 })
        }
    }

    /// Akhir aliran: keluarkan semua gambar yang masih ditahan decoder (hanya untuk berkas; aliran langsung tidak pernah
    /// berakhir begini).
    pub fn drain(&mut self) -> u32 {
        let mut frames = 0;
        unsafe {
            if avcodec_send_packet(self.context, ptr::null()) < 0 {
                return 0;
            }
            while avcodec_receive_frame(self.context, self.frame) >= 0 {
                frames += 1;
                av_frame_unref(self.frame);
            }
        }
        frames
    }

    /// Decode satu frame; `Ok(None)` bila belum ada gambar, `Err` bila aliran perlu IDR.
    pub fn decode(&mut self, data: &[u8]) -> Result<Option<CpuPicture>, String> {
        match self.decode_once(data) {
            Ok(picture) => {
                if picture.is_some() {
                    self.decoded += 1;
                }
                Ok(picture)
            }
            Err(error) if self.hardware && self.decoded == 0 => {
                // Hardware gagal sebelum gambar pertama: ganti ke CPU dan coba frame ini lagi (frame pertama = IDR).
                *self = Self::open(self.codec, None).map_err(|e| format!("{error}; decode CPU juga gagal: {e}"))?;
                self.decode(data)
            }
            Err(error) => Err(error),
        }
    }

    fn decode_once(&mut self, data: &[u8]) -> Result<Option<CpuPicture>, String> {
        unsafe {
            av_frame_unref(self.frame);
            av_frame_unref(self.transfer);
            av_packet_unref(self.packet);
            let status = av_new_packet(self.packet, data.len() as i32);
            if status < 0 {
                return Err(format!("paket FFmpeg: {}", error_text(status)));
            }
            ptr::copy_nonoverlapping(data.as_ptr(), (*self.packet).data, data.len());
            let status = avcodec_send_packet(self.context, self.packet);
            if status < 0 && status != AVERROR_EAGAIN {
                return Err(format!("decode: {}", error_text(status)));
            }
            let status = avcodec_receive_frame(self.context, self.frame);
            if status == AVERROR_EAGAIN {
                return Ok(None);
            }
            if status < 0 {
                return Err(format!("decode: {}", error_text(status)));
            }
            // Frame hardware (VideoToolbox): salin ke memori sebagai NV12.
            let frame = if (*self.frame).hw_frames_ctx.is_null() {
                &*self.frame
            } else {
                let status = av_hwframe_transfer_data(self.transfer, self.frame, 0);
                if status < 0 {
                    return Err(format!("salin frame hardware: {}", error_text(status)));
                }
                (*self.transfer).color_range = (*self.frame).color_range;
                (*self.transfer).colorspace = (*self.frame).colorspace;
                &*self.transfer
            };
            Ok(Some(CpuPicture {
                width: frame.width as u32,
                height: frame.height as u32,
                nv12: to_nv12(frame)?,
                bt601: matches!(frame.colorspace, AVColorSpace::AVCOL_SPC_BT470BG | AVColorSpace::AVCOL_SPC_SMPTE170M),
                full_range: frame.color_range == AVColorRange::AVCOL_RANGE_JPEG,
            }))
        }
    }
}

impl Drop for CpuDecoder {
    fn drop(&mut self) {
        unsafe {
            av_frame_free(&mut self.frame);
            av_frame_free(&mut self.transfer);
            av_packet_free(&mut self.packet);
            avcodec_free_context(&mut self.context);
        }
    }
}

/// Hasil `decode_file`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDecode {
    pub decoder: String,
    pub hardware: bool,
    pub frames: u32,
    pub errors: u32,
    pub size: (u32, u32),
}

/// Decode berkas aliran mentah (Annex B H.264/HEVC atau OBU AV1) dengan pengurai FFmpeg, untuk menguji decoder di
/// mesin tanpa host (CI macOS, PC pemain): `cloudpc-stream-client decode-test <berkas> <codec>`.
pub fn decode_file(path: &std::path::Path, codec: Codec, hardware: bool) -> Result<FileDecode, String> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut decoder = CpuDecoder::new(codec, hardware)?;
    let id = match codec {
        Codec::H264 => AVCodecID::AV_CODEC_ID_H264,
        Codec::Hevc => AVCodecID::AV_CODEC_ID_HEVC,
        Codec::Av1 => AVCodecID::AV_CODEC_ID_AV1,
    };
    let mut result = FileDecode { decoder: decoder.name.clone(), hardware: decoder.hardware, frames: 0, errors: 0, size: (0, 0) };
    unsafe {
        // Di macOS bindgen membuat AVCodecID bertipe u32, jadi konversi ini perlu walau di Windows sudah i32.
        #[allow(clippy::unnecessary_cast)]
        let parser = av_parser_init(id.0 as i32);
        if parser.is_null() {
            return Err("pengurai FFmpeg tidak ada untuk codec ini".into());
        }
        let mut rest: &[u8] = &data;
        let mut flushed = false;
        while !flushed {
            let (input, size) = if rest.is_empty() { (ptr::null(), 0) } else { (rest.as_ptr(), rest.len().min(1 << 20) as i32) };
            flushed = rest.is_empty();
            let (mut out, mut out_size) = (ptr::null_mut(), 0);
            let used = av_parser_parse2(parser, decoder.context, &mut out, &mut out_size, input, size, i64::MIN, i64::MIN, 0);
            if used < 0 {
                break;
            }
            rest = &rest[used as usize..];
            if out_size > 0 {
                match decoder.decode(std::slice::from_raw_parts(out, out_size as usize)) {
                    Ok(Some(picture)) => {
                        result.frames += 1;
                        result.size = (picture.width, picture.height);
                    }
                    Ok(None) => {}
                    Err(_) => result.errors += 1,
                }
            }
        }
        av_parser_close(parser);
    }
    result.frames += decoder.drain();
    // Decoder bisa sudah berganti ke CPU di tengah jalan.
    result.decoder = decoder.name.clone();
    result.hardware = decoder.hardware;
    Ok(result)
}
