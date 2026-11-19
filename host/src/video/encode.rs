//! Fase H2. Encoder NVENC (H.264/HEVC/AV1) lewat `nvEncodeAPI64.dll`, masukan tekstur D3D11 NV12.
//!
//! Rujukan: `Sunshine/src/nvenc/nvenc_base.cpp`. Setelan mengikuti produksi: preset P1, tuning ultra low latency,
//! CBR, tanpa two-pass, tanpa B-frame, GOP tak terbatas (IDR hanya saat diminta), VBV satu frame.
//! Konversi warna BGRA -> NV12 dilakukan shader di `capture.rs`; VUI menyatakan BT.709 rentang terbatas (harus cocok).
//! Invalidasi frame rujukan (RFI): beberapa frame terakhir disimpan sebagai rujukan, sehingga frame yang hilang
//! di client cukup "dilupakan" encoder dan frame berikutnya merujuk ke frame lebih lama, tanpa IDR yang besar.
//! Belum: 10-bit/HDR, 4:4:4, encode asinkron.
//!
//! Tipe dan konstanta dari crate `moq-nvenc` (binding hasil bindgen dari header NVIDIA Video Codec SDK 12.1, MIT).

use std::ffi::c_void;

use moq_nvenc::sys::nvEncodeAPI as nv;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11Texture2D};

use crate::session::StreamConfig;

pub struct EncodedFrame {
    pub data: Vec<u8>,
    pub idr: bool,
    /// Frame pertama setelah invalidasi frame rujukan (client menandainya sebagai titik pulih).
    pub after_invalidation: bool,
}

/// Yang didukung GPU + driver ini.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    pub h264: bool,
    pub hevc: bool,
    pub av1: bool,
    /// Invalidasi frame rujukan didukung (diperiksa untuk HEVC, atau H.264 bila HEVC tidak ada).
    pub ref_invalidation: bool,
}

fn check(status: nv::NVENCSTATUS, what: &str) -> Result<(), String> {
    match status as i32 {
        0 => Ok(()),
        code => Err(format!("NVENC {what} gagal (kode {code})")),
    }
}

macro_rules! call {
    ($api:expr, $name:ident) => {
        $api.$name.ok_or(concat!("driver NVIDIA tidak menyediakan ", stringify!($name)))?
    };
}

fn same_guid(a: &nv::GUID, b: &nv::GUID) -> bool {
    (a.Data1, a.Data2, a.Data3, a.Data4) == (b.Data1, b.Data2, b.Data3, b.Data4)
}

/// Sesi NVENC yang terbuka; ditutup (beserta DLL-nya) saat di-drop.
struct Session {
    api: nv::NV_ENCODE_API_FUNCTION_LIST,
    raw: *mut c_void,
    _library: libloading::Library,
}

impl Session {
    fn open(device: &ID3D11Device) -> Result<Session, String> {
        // Jalur lengkap, bukan nama saja: host berjalan sebagai SYSTEM dan tidak boleh memuat DLL dari folder kerja.
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
        let path = format!("{system_root}\\System32\\nvEncodeAPI64.dll");
        // SAFETY: DLL driver NVIDIA resmi di System32; fungsi yang diambil sesuai deklarasi SDK.
        let library = unsafe { libloading::Library::new(&path) }.map_err(|e| format!("NVENC tidak tersedia ({path}): {e}"))?;
        let mut api = nv::NV_ENCODE_API_FUNCTION_LIST { version: nv::NV_ENCODE_API_FUNCTION_LIST_VER, ..Default::default() };
        unsafe {
            let create = library
                .get::<unsafe extern "C" fn(*mut nv::NV_ENCODE_API_FUNCTION_LIST) -> nv::NVENCSTATUS>(b"NvEncodeAPICreateInstance\0")
                .map_err(|e| format!("NvEncodeAPICreateInstance tidak ada: {e}"))?;
            check(create(&mut api), "CreateInstance")?;
        }
        let mut open = nv::NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS {
            version: nv::NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS_VER,
            deviceType: nv::NV_ENC_DEVICE_TYPE::NV_ENC_DEVICE_TYPE_DIRECTX,
            device: device.as_raw(),
            apiVersion: nv::NVENCAPI_VERSION,
            ..Default::default()
        };
        let mut raw: *mut c_void = std::ptr::null_mut();
        // SAFETY: struct diisi sesuai SDK; pointer hidup selama panggilan.
        let opened = unsafe { call!(api, nvEncOpenEncodeSessionEx)(&mut open, &mut raw) };
        // Sesi yang gagal dibuka tetap harus dihancurkan (aturan SDK); `Drop` di bawah yang melakukannya.
        let session = Session { api, raw, _library: library };
        check(opened, "OpenEncodeSessionEx")?;
        Ok(session)
    }

    fn codecs(&self) -> Result<Vec<nv::GUID>, String> {
        let mut count = 0u32;
        // SAFETY: lihat `open`.
        check(unsafe { call!(self.api, nvEncGetEncodeGUIDCount)(self.raw, &mut count) }, "GetEncodeGUIDCount")?;
        let mut guids = vec![nv::GUID { Data1: 0, Data2: 0, Data3: 0, Data4: [0; 8] }; count as usize];
        let mut written = 0u32;
        check(unsafe { call!(self.api, nvEncGetEncodeGUIDs)(self.raw, guids.as_mut_ptr(), count, &mut written) }, "GetEncodeGUIDs")?;
        guids.truncate(written as usize);
        Ok(guids)
    }

    fn cap(&self, codec: nv::GUID, cap: nv::NV_ENC_CAPS) -> i32 {
        let mut query = nv::NV_ENC_CAPS_PARAM { version: nv::NV_ENC_CAPS_PARAM_VER, capsToQuery: cap, ..Default::default() };
        let mut value = 0i32;
        let Some(get) = self.api.nvEncGetEncodeCaps else { return 0 };
        // SAFETY: lihat `open`.
        match unsafe { get(self.raw, codec, &mut query, &mut value) } as i32 {
            0 => value,
            _ => 0,
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let (false, Some(destroy)) = (self.raw.is_null(), self.api.nvEncDestroyEncoder) {
            // SAFETY: sesi dihancurkan tepat sekali, setelah semua sumber dayanya dilepas (lihat `Encoder::drop`).
            unsafe { destroy(self.raw) };
        }
    }
}

/// Periksa codec dan fitur yang didukung GPU NVIDIA di mesin ini (membuka sesi NVENC sebentar).
pub fn probe() -> Result<Capabilities, String> {
    let (device, _context, _output) = super::capture::nvidia_device()?;
    let session = Session::open(&device)?;
    let codecs = session.codecs()?;
    let has = |guid: &nv::GUID| codecs.iter().any(|c| same_guid(c, guid));
    let (h264, hevc, av1) = (has(&nv::NV_ENC_CODEC_H264_GUID), has(&nv::NV_ENC_CODEC_HEVC_GUID), has(&nv::NV_ENC_CODEC_AV1_GUID));
    let reference = if hevc { nv::NV_ENC_CODEC_HEVC_GUID } else { nv::NV_ENC_CODEC_H264_GUID };
    let ref_invalidation = session.cap(reference, nv::NV_ENC_CAPS::NV_ENC_CAPS_SUPPORT_REF_PIC_INVALIDATION) != 0;
    Ok(Capabilities { h264, hevc, av1, ref_invalidation })
}

pub struct Encoder {
    bitstream: nv::NV_ENC_OUTPUT_PTR,
    registered: nv::NV_ENC_REGISTERED_PTR,
    width: u32,
    height: u32,
    /// Jumlah frame rujukan yang disimpan encoder; invalidasi hanya mungkin dalam jangkauan ini.
    ref_frames: u32,
    ref_invalidation: bool,
    last_encoded: u64,
    last_invalidated: (u64, u64),
    invalidation_pending: bool,
    _input: ID3D11Texture2D,
    // Paling akhir supaya di-drop setelah sumber daya di atas dilepas oleh `Drop for Encoder`.
    session: Session,
}

impl Encoder {
    /// `input`: tekstur NV12 milik `device` yang diisi ulang sebelum tiap `encode`.
    pub fn new(device: &ID3D11Device, input: &ID3D11Texture2D, width: u32, height: u32, config: &StreamConfig) -> Result<Encoder, String> {
        let session = Session::open(device)?;
        let mut encoder = Encoder {
            bitstream: std::ptr::null_mut(),
            registered: std::ptr::null_mut(),
            width,
            height,
            ref_frames: 1,
            ref_invalidation: false,
            last_encoded: 0,
            last_invalidated: (0, 0),
            invalidation_pending: false,
            _input: input.clone(),
            session,
        };
        encoder.initialize(input, config)?;
        Ok(encoder)
    }

    pub fn supports_invalidation(&self) -> bool {
        self.ref_invalidation
    }

    fn initialize(&mut self, input: &ID3D11Texture2D, config: &StreamConfig) -> Result<(), String> {
        let api = self.session.api;
        let raw = self.session.raw;
        let codec = match config.video_format {
            0 => nv::NV_ENC_CODEC_H264_GUID,
            1 => nv::NV_ENC_CODEC_HEVC_GUID,
            _ => nv::NV_ENC_CODEC_AV1_GUID,
        };
        if !self.session.codecs()?.iter().any(|c| same_guid(c, &codec)) {
            return Err(format!("GPU ini tidak punya encoder untuk codec {}", config.video_format));
        }
        let tuning = nv::NV_ENC_TUNING_INFO::NV_ENC_TUNING_INFO_ULTRA_LOW_LATENCY;

        // Frame rujukan: client boleh membatasi (decoder tertentu hanya sanggup 1); kalau tidak, 5 (AV1: 8) seperti Sunshine.
        let default_refs = if config.video_format == 2 { 8 } else { 5 };
        self.ref_frames = if config.ref_frames > 0 { config.ref_frames } else { default_refs };
        if self.session.cap(codec, nv::NV_ENC_CAPS::NV_ENC_CAPS_SUPPORT_MULTIPLE_REF_FRAMES) == 0 {
            self.ref_frames = 1;
        }
        self.ref_invalidation = self.ref_frames > 1 && self.session.cap(codec, nv::NV_ENC_CAPS::NV_ENC_CAPS_SUPPORT_REF_PIC_INVALIDATION) != 0;

        let mut preset = nv::NV_ENC_PRESET_CONFIG { version: nv::NV_ENC_PRESET_CONFIG_VER, ..Default::default() };
        preset.presetCfg.version = nv::NV_ENC_CONFIG_VER;
        // SAFETY: lihat `Session::open`.
        check(unsafe { call!(api, nvEncGetEncodePresetConfigEx)(raw, codec, nv::NV_ENC_PRESET_P1_GUID, tuning, &mut preset) }, "GetEncodePresetConfigEx")?;

        let mut cfg = preset.presetCfg;
        cfg.version = nv::NV_ENC_CONFIG_VER;
        cfg.profileGUID = nv::NV_ENC_CODEC_PROFILE_AUTOSELECT_GUID;
        cfg.gopLength = nv::NVENC_INFINITE_GOPLENGTH;
        cfg.frameIntervalP = 1;
        cfg.rcParams.rateControlMode = nv::NV_ENC_PARAMS_RC_MODE::NV_ENC_PARAMS_RC_CBR;
        cfg.rcParams.set_zeroReorderDelay(1);
        cfg.rcParams.set_enableLookahead(0);
        cfg.rcParams.lowDelayKeyFrameScale = 1;
        cfg.rcParams.multiPass = nv::NV_ENC_MULTI_PASS::NV_ENC_MULTI_PASS_DISABLED;
        cfg.rcParams.averageBitRate = config.bitrate_kbps.saturating_mul(1000);
        // Penyangga VBV satu frame: tiap frame kira-kira sama besar, tidak ada lonjakan yang menambah latensi.
        cfg.rcParams.vbvBufferSize = cfg.rcParams.averageBitRate / config.fps.max(1);

        let primaries = nv::NV_ENC_VUI_COLOR_PRIMARIES::NV_ENC_VUI_COLOR_PRIMARIES_BT709;
        let transfer = nv::NV_ENC_VUI_TRANSFER_CHARACTERISTIC::NV_ENC_VUI_TRANSFER_CHARACTERISTIC_BT709;
        let matrix = nv::NV_ENC_VUI_MATRIX_COEFFS::NV_ENC_VUI_MATRIX_COEFFS_BT709;
        // Hanya satu frame yang benar-benar dirujuk; sisanya cadangan untuk invalidasi.
        let one_reference = nv::NV_ENC_NUM_REF_FRAMES::NV_ENC_NUM_REF_FRAMES_1;
        // SAFETY: `encodeCodecConfig` adalah union; anggota yang dipakai sesuai codec yang dipilih.
        unsafe {
            match config.video_format {
                0 => {
                    cfg.profileGUID = nv::NV_ENC_H264_PROFILE_HIGH_GUID;
                    let h264 = &mut cfg.encodeCodecConfig.h264Config;
                    h264.set_repeatSPSPPS(1);
                    h264.idrPeriod = nv::NVENC_INFINITE_GOPLENGTH;
                    h264.sliceMode = 3;
                    h264.sliceModeData = config.slices_per_frame;
                    h264.maxNumRefFrames = self.ref_frames;
                    h264.numRefL0 = one_reference;
                    let vui = &mut h264.h264VUIParameters;
                    vui.videoSignalTypePresentFlag = 1;
                    vui.videoFormat = nv::NV_ENC_VUI_VIDEO_FORMAT::NV_ENC_VUI_VIDEO_FORMAT_UNSPECIFIED;
                    vui.videoFullRangeFlag = 0;
                    vui.colourDescriptionPresentFlag = 1;
                    vui.colourPrimaries = primaries;
                    vui.transferCharacteristics = transfer;
                    vui.colourMatrix = matrix;
                }
                1 => {
                    let hevc = &mut cfg.encodeCodecConfig.hevcConfig;
                    hevc.set_repeatSPSPPS(1);
                    hevc.idrPeriod = nv::NVENC_INFINITE_GOPLENGTH;
                    hevc.sliceMode = 3;
                    hevc.sliceModeData = config.slices_per_frame;
                    hevc.maxNumRefFramesInDPB = self.ref_frames;
                    hevc.numRefL0 = one_reference;
                    let vui = &mut hevc.hevcVUIParameters;
                    vui.videoSignalTypePresentFlag = 1;
                    vui.videoFormat = nv::NV_ENC_VUI_VIDEO_FORMAT::NV_ENC_VUI_VIDEO_FORMAT_UNSPECIFIED;
                    vui.videoFullRangeFlag = 0;
                    vui.colourDescriptionPresentFlag = 1;
                    vui.colourPrimaries = primaries;
                    vui.transferCharacteristics = transfer;
                    vui.colourMatrix = matrix;
                }
                _ => {
                    let av1 = &mut cfg.encodeCodecConfig.av1Config;
                    av1.set_repeatSeqHdr(1);
                    av1.idrPeriod = nv::NVENC_INFINITE_GOPLENGTH;
                    av1.maxNumRefFramesInDPB = self.ref_frames;
                    av1.numFwdRefs = one_reference;
                    av1.colorPrimaries = primaries;
                    av1.transferCharacteristics = transfer;
                    av1.matrixCoefficients = matrix;
                    av1.colorRange = 0;
                }
            }
        }

        let mut init = nv::NV_ENC_INITIALIZE_PARAMS {
            version: nv::NV_ENC_INITIALIZE_PARAMS_VER,
            encodeGUID: codec,
            presetGUID: nv::NV_ENC_PRESET_P1_GUID,
            tuningInfo: tuning,
            encodeWidth: self.width,
            encodeHeight: self.height,
            darWidth: self.width,
            darHeight: self.height,
            frameRateNum: config.fps,
            frameRateDen: 1,
            enablePTD: 1,
            enableEncodeAsync: 0,
            encodeConfig: &mut cfg,
            ..Default::default()
        };
        // SAFETY: `cfg` hidup sampai panggilan selesai; NVENC menyalin isinya.
        check(unsafe { call!(api, nvEncInitializeEncoder)(raw, &mut init) }, "InitializeEncoder")?;

        let mut buffer = nv::NV_ENC_CREATE_BITSTREAM_BUFFER { version: nv::NV_ENC_CREATE_BITSTREAM_BUFFER_VER, ..Default::default() };
        check(unsafe { call!(api, nvEncCreateBitstreamBuffer)(raw, &mut buffer) }, "CreateBitstreamBuffer")?;
        self.bitstream = buffer.bitstreamBuffer;

        let mut register = nv::NV_ENC_REGISTER_RESOURCE {
            version: nv::NV_ENC_REGISTER_RESOURCE_VER,
            resourceType: nv::NV_ENC_INPUT_RESOURCE_TYPE::NV_ENC_INPUT_RESOURCE_TYPE_DIRECTX,
            width: self.width,
            height: self.height,
            resourceToRegister: input.as_raw(),
            bufferFormat: nv::NV_ENC_BUFFER_FORMAT::NV_ENC_BUFFER_FORMAT_NV12,
            bufferUsage: nv::NV_ENC_BUFFER_USAGE::NV_ENC_INPUT_IMAGE,
            ..Default::default()
        };
        check(unsafe { call!(api, nvEncRegisterResource)(raw, &mut register) }, "RegisterResource")?;
        self.registered = register.registeredResource;
        Ok(())
    }

    /// Client kehilangan frame `first..=last`: minta encoder tidak merujuk ke frame itu maupun sesudahnya.
    /// `false` = tidak bisa (di luar jangkauan frame rujukan atau tidak didukung): pemanggil harus mengirim IDR.
    pub fn invalidate(&mut self, first: u64, last: u64) -> bool {
        if !self.ref_invalidation {
            return false;
        }
        if first >= self.last_invalidated.0 && last <= self.last_invalidated.1 {
            return true;
        }
        self.invalidation_pending = true;
        if last < first || first == 0 || first > self.last_encoded {
            return false;
        }
        // Semua frame sejak yang pertama hilang ikut tidak sah, karena saling merujuk berantai.
        let last = self.last_encoded;
        self.last_invalidated = (first, last);
        if last - first + 1 >= u64::from(self.ref_frames) {
            return false;
        }
        let Some(invalidate) = self.session.api.nvEncInvalidateRefFrames else { return false };
        // SAFETY: sesi hidup; cap waktu = nomor frame yang dipakai saat encode.
        (first..=last).all(|frame| unsafe { invalidate(self.session.raw, frame) } as i32 == 0)
    }

    /// Encode isi tekstur masukan saat ini sebagai frame nomor `frame_index` (mulai 1, naik satu-satu).
    /// Sinkron: kembali setelah bitstream frame ini siap.
    pub fn encode(&mut self, force_idr: bool, frame_index: u64) -> Result<EncodedFrame, String> {
        let api = self.session.api;
        let raw = self.session.raw;
        let mut map = nv::NV_ENC_MAP_INPUT_RESOURCE { version: nv::NV_ENC_MAP_INPUT_RESOURCE_VER, registeredResource: self.registered, ..Default::default() };
        // SAFETY: sesi, sumber daya terdaftar, dan penyangga bitstream dibuat di `initialize` dan masih hidup.
        check(unsafe { call!(api, nvEncMapInputResource)(raw, &mut map) }, "MapInputResource")?;

        let flags = if force_idr { nv::NV_ENC_PIC_FLAGS::NV_ENC_PIC_FLAG_FORCEIDR as u32 | nv::NV_ENC_PIC_FLAGS::NV_ENC_PIC_FLAG_OUTPUT_SPSPPS as u32 } else { 0 };
        let mut picture = nv::NV_ENC_PIC_PARAMS {
            version: nv::NV_ENC_PIC_PARAMS_VER,
            inputWidth: self.width,
            inputHeight: self.height,
            inputPitch: self.width,
            encodePicFlags: flags,
            inputTimeStamp: frame_index,
            inputBuffer: map.mappedResource,
            bufferFmt: map.mappedBufferFmt,
            outputBitstream: self.bitstream,
            pictureStruct: nv::NV_ENC_PIC_STRUCT::NV_ENC_PIC_STRUCT_FRAME,
            ..Default::default()
        };
        let encoded = check(unsafe { call!(api, nvEncEncodePicture)(raw, &mut picture) }, "EncodePicture");

        let result = encoded.and_then(|_| {
            let mut lock = nv::NV_ENC_LOCK_BITSTREAM { version: nv::NV_ENC_LOCK_BITSTREAM_VER, outputBitstream: self.bitstream, ..Default::default() };
            check(unsafe { call!(api, nvEncLockBitstream)(raw, &mut lock) }, "LockBitstream")?;
            // SAFETY: NVENC menjamin penunjuk dan ukuran ini sah sampai UnlockBitstream.
            let data = unsafe { std::slice::from_raw_parts(lock.bitstreamBufferPtr as *const u8, lock.bitstreamSizeInBytes as usize) }.to_vec();
            let idr = lock.pictureType as i32 == nv::NV_ENC_PIC_TYPE::NV_ENC_PIC_TYPE_IDR as i32;
            check(unsafe { call!(api, nvEncUnlockBitstream)(raw, self.bitstream) }, "UnlockBitstream")?;
            Ok((data, idr))
        });
        // Selalu lepas pemetaan, juga saat encode gagal.
        let unmapped = check(unsafe { call!(api, nvEncUnmapInputResource)(raw, map.mappedResource) }, "UnmapInputResource");
        let (data, idr) = result?;
        unmapped?;
        self.last_encoded = frame_index;
        let after_invalidation = std::mem::take(&mut self.invalidation_pending) && !idr;
        Ok(EncodedFrame { data, idr, after_invalidation })
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        // SAFETY: tiap sumber daya dilepas tepat sekali; sesi sendiri dihancurkan sesudahnya oleh `Drop for Session`.
        unsafe {
            if !self.registered.is_null() {
                if let Some(unregister) = self.session.api.nvEncUnregisterResource {
                    unregister(self.session.raw, self.registered);
                }
            }
            if !self.bitstream.is_null() {
                if let Some(destroy) = self.session.api.nvEncDestroyBitstreamBuffer {
                    destroy(self.session.raw, self.bitstream);
                }
            }
        }
    }
}
