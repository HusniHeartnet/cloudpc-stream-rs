//! Fase C3. Decode hardware H.264/HEVC/AV1 lewat FFmpeg + D3D11VA (Windows), seperti Moonlight.
//!
//! Rujukan: `moonlight-qt/app/streaming/video/ffmpeg.cpp` dan `ffmpeg-renderers/d3d11va.cpp`. Decoder memakai device
//! D3D11 milik renderer, jadi frame hasil decode (tekstur NV12 di GPU) langsung digambar tanpa salinan ke CPU.
//! Satu frame masuk, satu frame keluar (`AV_CODEC_FLAG_LOW_DELAY`, satu thread): tidak ada antrean di decoder.

use std::ffi::{c_void, CStr};
use std::ptr;

use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11Texture2D};

use super::ff::{error_text, to_nv12, AVERROR_EAGAIN, FF_THREAD_SLICE};
use super::ffi::*;
use crate::rtsp::Codec;

/// `AVD3D11VADeviceContext` (libavutil/hwcontext_d3d11va.h, FFmpeg 8.0). Ditulis tangan karena header aslinya
/// menarik d3d11.h; urutan field harus sama persis.
#[repr(C)]
struct D3d11vaDeviceContext {
    device: *mut c_void,
    device_context: *mut c_void,
    video_device: *mut c_void,
    video_context: *mut c_void,
    lock: Option<unsafe extern "C" fn(*mut c_void)>,
    unlock: Option<unsafe extern "C" fn(*mut c_void)>,
    lock_ctx: *mut c_void,
}

/// Isi gambar hasil decode.
pub enum Surface {
    /// Decode hardware: satu irisan tekstur array milik FFmpeg. Berlaku sampai `decode` berikutnya.
    Gpu { texture: ID3D11Texture2D, index: u32 },
    /// Decode perangkat lunak: NV12 di memori (bidang Y lalu UV berselang, lebar baris = `width`).
    Cpu(Vec<u8>),
}

pub struct Picture {
    pub surface: Surface,
    pub width: u32,
    pub height: u32,
    /// Matriks BT.601 (selain itu BT.709).
    pub bt601: bool,
    /// Rentang penuh 0-255 (selain itu terbatas 16-235).
    pub full_range: bool,
}

pub struct Decoder {
    context: *mut AVCodecContext,
    packet: *mut AVPacket,
    frame: *mut AVFrame,
    pub name: String,
    /// Decode di GPU (D3D11VA); `false` = decode perangkat lunak di CPU.
    pub hardware: bool,
}

// Semua pointer FFmpeg hanya dipakai dari thread pemilik `Decoder`.
unsafe impl Send for Decoder {}

unsafe extern "C" fn pick_d3d11(_context: *mut AVCodecContext, mut formats: *const AVPixelFormat) -> AVPixelFormat {
    while *formats != AVPixelFormat::AV_PIX_FMT_NONE {
        if *formats == AVPixelFormat::AV_PIX_FMT_D3D11 {
            return AVPixelFormat::AV_PIX_FMT_D3D11;
        }
        formats = formats.add(1);
    }
    // Tanpa D3D11 berarti GPU tidak mendukung codec ini: tolak, jangan diam-diam pindah ke decode CPU.
    AVPixelFormat::AV_PIX_FMT_NONE
}

impl Decoder {
    /// Siapkan decoder untuk `codec`. `hardware`: D3D11VA di atas `device` (dibuat dengan
    /// `D3D11_CREATE_DEVICE_VIDEO_SUPPORT`); kalau tidak, decode perangkat lunak seperti pilihan "Software" Moonlight
    /// (untuk GPU yang tidak punya decoder codec itu).
    pub fn new(codec: Codec, device: &ID3D11Device, hardware: bool) -> Result<Decoder, String> {
        unsafe {
            av_log_set_level(AV_LOG_ERROR as i32);
            // AV1: decoder bawaan FFmpeg ("av1") hanya jalan dengan akselerasi GPU; tanpa GPU dipakai libdav1d.
            let decoder = match codec {
                Codec::H264 => avcodec_find_decoder(AVCodecID::AV_CODEC_ID_H264),
                Codec::Hevc => avcodec_find_decoder(AVCodecID::AV_CODEC_ID_HEVC),
                Codec::Av1 if hardware => avcodec_find_decoder_by_name(c"av1".as_ptr()),
                // Tanpa GPU: dav1d (decoder AV1 perangkat lunak tercepat, juga dipakai Moonlight).
                Codec::Av1 => avcodec_find_decoder_by_name(c"libdav1d".as_ptr()),
            };
            if decoder.is_null() {
                return Err(format!("FFmpeg tidak punya decoder {codec:?}"));
            }
            let name = CStr::from_ptr((*decoder).name).to_string_lossy().into_owned();
            if !hardware {
                let context = avcodec_alloc_context3(decoder);
                (*context).flags |= AV_CODEC_FLAG_LOW_DELAY as i32;
                // Bagi per irisan, bukan per frame: tanpa antrean frame tambahan (latensi tetap satu frame).
                (*context).thread_type = FF_THREAD_SLICE;
                (*context).thread_count = 0;
                let status = avcodec_open2(context, decoder, ptr::null_mut());
                if status < 0 {
                    let mut context = context;
                    avcodec_free_context(&mut context);
                    return Err(format!("decoder {name} gagal dibuka: {}", error_text(status)));
                }
                return Ok(Decoder { context, packet: av_packet_alloc(), frame: av_frame_alloc(), name, hardware: false });
            }

            let hw = av_hwdevice_ctx_alloc(AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA);
            if hw.is_null() {
                return Err("FFmpeg tanpa dukungan D3D11VA".into());
            }
            let device_context = (*hw).data as *mut AVHWDeviceContext;
            let d3d = (*device_context).hwctx as *mut D3d11vaDeviceContext;
            // FFmpeg memegang satu referensi device dan melepasnya sendiri.
            (*d3d).device = device.clone().into_raw();
            let mut hw = hw;
            let status = av_hwdevice_ctx_init(hw);
            if status < 0 {
                av_buffer_unref(&mut hw);
                return Err(format!("D3D11VA gagal disiapkan: {}", error_text(status)));
            }

            let context = avcodec_alloc_context3(decoder);
            (*context).hw_device_ctx = av_buffer_ref(hw);
            av_buffer_unref(&mut hw);
            (*context).get_format = Some(pick_d3d11);
            (*context).flags |= AV_CODEC_FLAG_LOW_DELAY as i32;
            (*context).thread_count = 1;
            let status = avcodec_open2(context, decoder, ptr::null_mut());
            if status < 0 {
                let mut context = context;
                avcodec_free_context(&mut context);
                return Err(format!("decoder {name} gagal dibuka: {}", error_text(status)));
            }
            Ok(Decoder { context, packet: av_packet_alloc(), frame: av_frame_alloc(), name, hardware: true })
        }
    }

    /// Decode satu frame terenkode. `Ok(None)` bila decoder belum mengeluarkan gambar (mis. setelah frame rusak);
    /// `Err` = frame ini tidak bisa didecode dan aliran perlu IDR.
    pub fn decode(&mut self, data: &[u8]) -> Result<Option<Picture>, String> {
        unsafe {
            av_frame_unref(self.frame);
            av_packet_unref(self.packet);
            // av_new_packet menambah padding nol AV_INPUT_BUFFER_PADDING_SIZE yang wajib ada di belakang data.
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
            let frame = &*self.frame;
            let surface = if frame.format == AVPixelFormat::AV_PIX_FMT_D3D11.0 {
                let texture = ID3D11Texture2D::from_raw_borrowed(&(frame.data[0] as *mut c_void)).ok_or("tekstur kosong")?.clone();
                Surface::Gpu { texture, index: frame.data[1] as usize as u32 }
            } else {
                Surface::Cpu(to_nv12(frame)?)
            };
            Ok(Some(Picture {
                surface,
                width: frame.width as u32,
                height: frame.height as u32,
                bt601: matches!(frame.colorspace, AVColorSpace::AVCOL_SPC_BT470BG | AVColorSpace::AVCOL_SPC_SMPTE170M),
                full_range: frame.color_range == AVColorRange::AVCOL_RANGE_JPEG,
            }))
        }
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        unsafe {
            av_frame_free(&mut self.frame);
            av_packet_free(&mut self.packet);
            avcodec_free_context(&mut self.context);
        }
    }
}
