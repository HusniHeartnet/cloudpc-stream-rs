//! Fase H2. Tangkap layar dengan DXGI Desktop Duplication dan susun gambar akhir di GPU, tanpa lewat RAM:
//! frame desktop disalin ke tekstur `desktop`, lalu digambar (diskala, dengan kursor di atasnya) ke tekstur
//! `frame` beresolusi permintaan client. Terakhir `frame` diubah ke YUV 4:2:0 (NV12, BT.709 rentang terbatas) oleh
//! pixel shader ke tekstur `nv12`, masukan NVENC. Seperti Sunshine: kalau NVENC diberi ARGB, ia mengonversi sendiri dan
//! itu terukur memakai ±3,5x beban GPU (benchmark 19 Nov 2026).
//!
//! Rujukan: `Sunshine/src/platform/windows/display_base.cpp`, `display_vram.cpp`.
//! Belum: HDR, pilihan monitor (dipakai layar pertama di GPU NVIDIA), rotasi layar.

use std::ffi::c_void;

use windows::core::{s, Interface, PCSTR};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{ID3DBlob, D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST, D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11BlendState, ID3D11Device, ID3D11DeviceContext, ID3D11PixelShader, ID3D11RenderTargetView, ID3D11SamplerState, ID3D11ShaderResourceView, ID3D11Texture2D,
    ID3D11VertexShader, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_BLEND, D3D11_BLEND_DESC, D3D11_BLEND_INV_DEST_COLOR, D3D11_BLEND_INV_SRC_ALPHA, D3D11_BLEND_INV_SRC_COLOR,
    D3D11_BLEND_ONE, D3D11_BLEND_OP_ADD, D3D11_BLEND_SRC_ALPHA, D3D11_BLEND_ZERO, D3D11_COLOR_WRITE_ENABLE_ALL, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_FILTER_MIN_MAG_MIP_LINEAR,
    D3D11_RENDER_TARGET_VIEW_DESC, D3D11_RENDER_TARGET_VIEW_DESC_0, D3D11_RTV_DIMENSION_TEXTURE2D, D3D11_SAMPLER_DESC, D3D11_SDK_VERSION, D3D11_TEX2D_RTV, D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE2D_DESC, D3D11_TEXTURE_ADDRESS_CLAMP, D3D11_USAGE_DEFAULT, D3D11_USAGE_IMMUTABLE, D3D11_VIEWPORT,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12, DXGI_FORMAT_R8G8_UNORM, DXGI_FORMAT_R8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1, IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource, DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO,
    DXGI_OUTDUPL_POINTER_SHAPE_INFO,
};

use super::cursor::{self, CursorImage};
use super::fit;

const VENDOR_NVIDIA: u32 = 0x10DE;

/// Satu segitiga yang menutupi seluruh viewport (tanpa vertex buffer), dan pengambil sampel tekstur.
const SHADERS: &str = "
struct VsOut { float4 pos : SV_Position; float2 uv : TEXCOORD0; };
VsOut vs_main(uint id : SV_VertexID) {
    VsOut o;
    o.uv = float2((id << 1) & 2, id & 2);
    o.pos = float4(o.uv * float2(2, -2) + float2(-1, 1), 0, 1);
    return o;
}
Texture2D image : register(t0);
SamplerState smp : register(s0);
float4 ps_main(VsOut i) : SV_Target { return image.Sample(smp, i.uv); }
// BT.709 rentang terbatas (sesuai VUI di encode.rs). Nilai warna desktop sudah ber-gamma, dipakai apa adanya.
float ps_y(VsOut i) : SV_Target {
    float3 c = image.Sample(smp, i.uv).rgb;
    return 16.0 / 255.0 + dot(c, float3(0.2126, 0.7152, 0.0722)) * (219.0 / 255.0);
}
// Dijalankan pada setengah resolusi: sampel linear di tengah blok 2x2 = rata-rata keempat piksel.
float2 ps_uv(VsOut i) : SV_Target {
    float3 c = image.Sample(smp, i.uv).rgb;
    float u = dot(c, float3(-0.1146, -0.3854, 0.5));
    float v = dot(c, float3(0.5, -0.4542, -0.0458));
    return 128.0 / 255.0 + float2(u, v) * (224.0 / 255.0);
}
";

pub enum CaptureError {
    /// Duplikasi hilang (ganti resolusi, layar kunci, UAC): buat ulang `Capture`.
    Lost,
    Other(String),
}

fn describe(what: &str, error: windows::core::Error) -> String {
    format!("{what}: {error}")
}

/// GPU NVIDIA pertama dan layar pertamanya yang tersambung ke desktop (bila ada).
fn nvidia_adapter() -> Result<(IDXGIAdapter1, Option<IDXGIOutput1>), String> {
    // SAFETY: panggilan COM/DXGI biasa; semua hasil diperiksa.
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().map_err(|e| describe("CreateDXGIFactory1", e))?;
        let mut first: Option<(IDXGIAdapter1, Option<IDXGIOutput1>)> = None;
        let mut adapter_index = 0;
        while let Ok(adapter) = factory.EnumAdapters1(adapter_index) {
            adapter_index += 1;
            if adapter.GetDesc1().map(|d| d.VendorId != VENDOR_NVIDIA).unwrap_or(true) {
                continue;
            }
            let mut output_index = 0;
            while let Ok(output) = adapter.EnumOutputs(output_index) {
                output_index += 1;
                let attached = output.GetDesc().map(|d| d.AttachedToDesktop.as_bool()).unwrap_or(false);
                if let (true, Ok(output)) = (attached, output.cast::<IDXGIOutput1>()) {
                    return Ok((adapter, Some(output)));
                }
            }
            first.get_or_insert((adapter, None));
        }
        first.ok_or_else(|| "tidak ada GPU NVIDIA".to_string())
    }
}

/// Device D3D11 di GPU NVIDIA, untuk capture maupun sekadar memeriksa kemampuan encoder.
pub fn nvidia_device() -> Result<(ID3D11Device, ID3D11DeviceContext, Option<IDXGIOutput1>), String> {
    let (adapter, output) = nvidia_adapter()?;
    let (mut device, mut context) = (None, None);
    // SAFETY: lihat `nvidia_adapter`.
    unsafe {
        D3D11CreateDevice(&adapter, D3D_DRIVER_TYPE_UNKNOWN, HMODULE::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT, Some(&[D3D_FEATURE_LEVEL_11_0]), D3D11_SDK_VERSION, Some(&mut device), None, Some(&mut context))
            .map_err(|e| describe("D3D11CreateDevice", e))?;
    }
    Ok((device.ok_or("D3D11CreateDevice tanpa device")?, context.ok_or("D3D11CreateDevice tanpa context")?, output))
}

struct Layer {
    view: ID3D11ShaderResourceView,
    _texture: ID3D11Texture2D,
}

struct Cursor {
    width: u32,
    height: u32,
    alpha: Layer,
    invert: Option<Layer>,
}

pub struct Capture {
    pub device: ID3D11Device,
    context: ID3D11DeviceContext,
    /// Layar yang diduplikasi; disimpan supaya duplikasi bisa dibuka ulang tanpa membuat device dan encoder baru.
    output: IDXGIOutput1,
    /// `None` hanya sesaat di `reopen` (duplikasi lama wajib dilepas sebelum yang baru dibuat).
    duplication: Option<IDXGIOutputDuplication>,
    desktop: ID3D11Texture2D,
    desktop_view: ID3D11ShaderResourceView,
    /// Gambar tersusun (desktop + kursor) beresolusi client, BGRA.
    _frame: ID3D11Texture2D,
    frame_view: ID3D11RenderTargetView,
    frame_source: ID3D11ShaderResourceView,
    /// `frame` dalam NV12; inilah yang didaftarkan ke encoder.
    pub nv12: ID3D11Texture2D,
    luma_view: ID3D11RenderTargetView,
    chroma_view: ID3D11RenderTargetView,
    vertex_shader: ID3D11VertexShader,
    pixel_shader: ID3D11PixelShader,
    luma_shader: ID3D11PixelShader,
    chroma_shader: ID3D11PixelShader,
    sampler: ID3D11SamplerState,
    blend_alpha: ID3D11BlendState,
    blend_invert: ID3D11BlendState,
    cursor: Option<Cursor>,
    cursor_visible: bool,
    cursor_position: (i32, i32),
    /// Ukuran layar host.
    pub source: (u32, u32),
    target: (u32, u32),
    /// Sudah pernah menerima gambar desktop.
    pub has_image: bool,
}

unsafe fn compile(entry: PCSTR, profile: PCSTR) -> Result<Vec<u8>, String> {
    let mut code: Option<ID3DBlob> = None;
    D3DCompile(SHADERS.as_ptr() as *const c_void, SHADERS.len(), PCSTR::null(), None, None, entry, profile, 0, 0, &mut code, None).map_err(|e| describe("D3DCompile", e))?;
    let code = code.ok_or("D3DCompile tanpa hasil")?;
    Ok(std::slice::from_raw_parts(code.GetBufferPointer() as *const u8, code.GetBufferSize()).to_vec())
}

unsafe fn blend_state(device: &ID3D11Device, source: D3D11_BLEND, destination: D3D11_BLEND) -> Result<ID3D11BlendState, String> {
    let mut desc = D3D11_BLEND_DESC::default();
    let target = &mut desc.RenderTarget[0];
    target.BlendEnable = true.into();
    target.SrcBlend = source;
    target.DestBlend = destination;
    target.BlendOp = D3D11_BLEND_OP_ADD;
    // Kanal alpha target tidak dipakai encoder; biarkan apa adanya.
    target.SrcBlendAlpha = D3D11_BLEND_ZERO;
    target.DestBlendAlpha = D3D11_BLEND_ONE;
    target.BlendOpAlpha = D3D11_BLEND_OP_ADD;
    target.RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8;
    let mut state = None;
    device.CreateBlendState(&desc, Some(&mut state)).map_err(|e| describe("CreateBlendState", e))?;
    state.ok_or_else(|| "CreateBlendState tanpa hasil".to_string())
}

unsafe fn texture(device: &ID3D11Device, width: u32, height: u32, bind: u32, pixels: Option<&[u8]>) -> Result<ID3D11Texture2D, String> {
    texture_as(device, DXGI_FORMAT_B8G8R8A8_UNORM, width, height, bind, pixels)
}

unsafe fn texture_as(device: &ID3D11Device, format: DXGI_FORMAT, width: u32, height: u32, bind: u32, pixels: Option<&[u8]>) -> Result<ID3D11Texture2D, String> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: if pixels.is_some() { D3D11_USAGE_IMMUTABLE } else { D3D11_USAGE_DEFAULT },
        BindFlags: bind,
        ..Default::default()
    };
    let initial = pixels.map(|p| D3D11_SUBRESOURCE_DATA { pSysMem: p.as_ptr() as *const c_void, SysMemPitch: width * 4, SysMemSlicePitch: 0 });
    let mut out = None;
    device.CreateTexture2D(&desc, initial.as_ref().map(|d| d as *const _), Some(&mut out)).map_err(|e| describe("CreateTexture2D", e))?;
    out.ok_or_else(|| "CreateTexture2D tanpa tekstur".to_string())
}

/// Render target ke satu bidang tekstur NV12: format R8 = bidang Y, R8G8 = bidang UV (aturan D3D11 untuk NV12).
unsafe fn plane_view(device: &ID3D11Device, texture: &ID3D11Texture2D, format: DXGI_FORMAT) -> Result<ID3D11RenderTargetView, String> {
    let desc = D3D11_RENDER_TARGET_VIEW_DESC {
        Format: format,
        ViewDimension: D3D11_RTV_DIMENSION_TEXTURE2D,
        Anonymous: D3D11_RENDER_TARGET_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_RTV { MipSlice: 0 } },
    };
    let mut view = None;
    device.CreateRenderTargetView(texture, Some(&desc), Some(&mut view)).map_err(|e| describe("CreateRenderTargetView (NV12)", e))?;
    view.ok_or_else(|| "CreateRenderTargetView tanpa hasil".to_string())
}

unsafe fn shader_view(device: &ID3D11Device, texture: &ID3D11Texture2D) -> Result<ID3D11ShaderResourceView, String> {
    let mut view = None;
    device.CreateShaderResourceView(texture, None, Some(&mut view)).map_err(|e| describe("CreateShaderResourceView", e))?;
    view.ok_or_else(|| "CreateShaderResourceView tanpa hasil".to_string())
}

unsafe fn duplicate(output: &IDXGIOutput1, device: &ID3D11Device) -> Result<(IDXGIOutputDuplication, (u32, u32)), String> {
    let duplication = output.DuplicateOutput(device).map_err(|e| describe("DuplicateOutput (layar terkunci atau sudah diduplikasi terlalu banyak?)", e))?;
    let mode = duplication.GetDesc().ModeDesc;
    if mode.Format != DXGI_FORMAT_B8G8R8A8_UNORM {
        return Err(format!("format layar {:?} belum didukung (HDR menyala?)", mode.Format));
    }
    Ok((duplication, (mode.Width, mode.Height)))
}

unsafe fn desktop_texture(device: &ID3D11Device, source: (u32, u32)) -> Result<(ID3D11Texture2D, ID3D11ShaderResourceView), String> {
    let desktop = texture(device, source.0, source.1, D3D11_BIND_SHADER_RESOURCE.0 as u32, None)?;
    let view = shader_view(device, &desktop)?;
    Ok((desktop, view))
}

impl Capture {
    /// Buka ulang duplikasi layar setelah `CaptureError::Lost` (game ganti mode tampilan, desktop berganti), dengan device,
    /// tekstur akhir, dan encoder yang SAMA: ukuran video ke client tidak berubah, jadi hanya tekstur sumber yang mengikuti
    /// ukuran layar baru. Jauh lebih cepat daripada membuat semuanya ulang (temuan sesi #93: gambar berhenti 1-5 dtk tiap
    /// RE2 berganti 1920x1080 <-> 1920x1440). Gagal = pemanggil jatuh ke pembuatan ulang penuh.
    pub fn reopen(&mut self) -> Result<(), String> {
        crate::supervise::sync_thread_desktop();
        // SAFETY: lihat `new`.
        unsafe {
            // Aturan DXGI: setelah ACCESS_LOST, lepas duplikasi lama dulu, baru buat yang baru.
            self.duplication = None;
            let (duplication, source) = duplicate(&self.output, &self.device)?;
            if source != self.source {
                let (desktop, view) = desktop_texture(&self.device, source)?;
                self.desktop = desktop;
                self.desktop_view = view;
                self.source = source;
            }
            self.duplication = Some(duplication);
        }
        self.has_image = false;
        Ok(())
    }

    /// Buka layar pertama di GPU NVIDIA; gambar akhir berukuran `target` (resolusi yang diminta client).
    pub fn new(target: (u32, u32)) -> Result<Capture, String> {
        // Duplikasi terikat ke desktop thread ini; layar kunci dan UAC adalah desktop lain.
        crate::supervise::sync_thread_desktop();
        let (device, context, output) = nvidia_device()?;
        let output = output.ok_or("tidak ada layar yang tersambung ke GPU NVIDIA")?;
        // SAFETY: panggilan COM/D3D11 biasa; semua hasil diperiksa lewat `Result`.
        unsafe {
            let (duplication, source) = duplicate(&output, &device)?;
            let (desktop, desktop_view) = desktop_texture(&device, source)?;
            let frame = texture(&device, target.0, target.1, (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32, None)?;
            let mut frame_view = None;
            device.CreateRenderTargetView(&frame, None, Some(&mut frame_view)).map_err(|e| describe("CreateRenderTargetView", e))?;
            let frame_source = shader_view(&device, &frame)?;
            if !target.0.is_multiple_of(2) || !target.1.is_multiple_of(2) {
                return Err(format!("resolusi {}x{} harus genap (YUV 4:2:0)", target.0, target.1));
            }
            let nv12 = texture_as(&device, DXGI_FORMAT_NV12, target.0, target.1, D3D11_BIND_RENDER_TARGET.0 as u32, None)?;
            let luma_view = plane_view(&device, &nv12, DXGI_FORMAT_R8_UNORM)?;
            let chroma_view = plane_view(&device, &nv12, DXGI_FORMAT_R8G8_UNORM)?;
            let (mut luma_shader, mut chroma_shader) = (None, None);
            device.CreatePixelShader(&compile(s!("ps_y"), s!("ps_4_0"))?, None, Some(&mut luma_shader)).map_err(|e| describe("CreatePixelShader", e))?;
            device.CreatePixelShader(&compile(s!("ps_uv"), s!("ps_4_0"))?, None, Some(&mut chroma_shader)).map_err(|e| describe("CreatePixelShader", e))?;

            let (mut vertex_shader, mut pixel_shader, mut sampler) = (None, None, None);
            device.CreateVertexShader(&compile(s!("vs_main"), s!("vs_4_0"))?, None, Some(&mut vertex_shader)).map_err(|e| describe("CreateVertexShader", e))?;
            device.CreatePixelShader(&compile(s!("ps_main"), s!("ps_4_0"))?, None, Some(&mut pixel_shader)).map_err(|e| describe("CreatePixelShader", e))?;
            let sampler_desc = D3D11_SAMPLER_DESC {
                Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
                AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                MaxLOD: f32::MAX,
                ..Default::default()
            };
            device.CreateSamplerState(&sampler_desc, Some(&mut sampler)).map_err(|e| describe("CreateSamplerState", e))?;

            Ok(Capture {
                blend_alpha: blend_state(&device, D3D11_BLEND_SRC_ALPHA, D3D11_BLEND_INV_SRC_ALPHA)?,
                // hasil = sumber * (1 - layar) + layar * (1 - sumber): putih membalik layar, hitam membiarkannya.
                blend_invert: blend_state(&device, D3D11_BLEND_INV_DEST_COLOR, D3D11_BLEND_INV_SRC_COLOR)?,
                device,
                context,
                output,
                duplication: Some(duplication),
                desktop,
                desktop_view,
                _frame: frame,
                frame_view: frame_view.ok_or("CreateRenderTargetView tanpa hasil")?,
                frame_source,
                nv12,
                luma_view,
                chroma_view,
                luma_shader: luma_shader.ok_or("CreatePixelShader tanpa hasil")?,
                chroma_shader: chroma_shader.ok_or("CreatePixelShader tanpa hasil")?,
                vertex_shader: vertex_shader.ok_or("CreateVertexShader tanpa hasil")?,
                pixel_shader: pixel_shader.ok_or("CreatePixelShader tanpa hasil")?,
                sampler: sampler.ok_or("CreateSamplerState tanpa hasil")?,
                cursor: None,
                cursor_visible: false,
                cursor_position: (0, 0),
                source,
                target,
                has_image: false,
            })
        }
    }

    /// Ambil perubahan desktop dan kursor terbaru; menunggu paling lama `timeout_ms`.
    pub fn update(&mut self, timeout_ms: u32) -> Result<(), CaptureError> {
        // SAFETY: lihat `new`. Frame yang diperoleh selalu dilepas sebelum fungsi kembali.
        unsafe {
            let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut resource: Option<IDXGIResource> = None;
            let Some(duplication) = self.duplication.clone() else { return Err(CaptureError::Lost) };
            match duplication.AcquireNextFrame(timeout_ms, &mut info, &mut resource) {
                Ok(()) => {}
                Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(()),
                Err(e) if e.code() == DXGI_ERROR_ACCESS_LOST => return Err(CaptureError::Lost),
                Err(e) => return Err(CaptureError::Other(describe("AcquireNextFrame", e))),
            }
            // LastPresentTime 0 = gambar desktop tidak berubah (hanya kursor).
            if info.LastPresentTime != 0 {
                if let Some(texture) = resource.and_then(|r| r.cast::<ID3D11Texture2D>().ok()) {
                    self.context.CopyResource(&self.desktop, &texture);
                    self.has_image = true;
                }
            }
            if info.LastMouseUpdateTime != 0 {
                self.cursor_visible = info.PointerPosition.Visible.as_bool();
                self.cursor_position = (info.PointerPosition.Position.x, info.PointerPosition.Position.y);
            }
            if info.PointerShapeBufferSize > 0 {
                self.read_cursor_shape(info.PointerShapeBufferSize);
            }
            match duplication.ReleaseFrame() {
                Ok(()) => Ok(()),
                Err(e) if e.code() == DXGI_ERROR_ACCESS_LOST => Err(CaptureError::Lost),
                Err(e) => Err(CaptureError::Other(describe("ReleaseFrame", e))),
            }
        }
    }

    /// Bentuk kursor baru. Kegagalan di sini tidak fatal: kursor lama (atau tanpa kursor) tetap dipakai.
    unsafe fn read_cursor_shape(&mut self, size: u32) {
        let mut data = vec![0u8; size as usize];
        let mut required = 0u32;
        let mut shape = DXGI_OUTDUPL_POINTER_SHAPE_INFO::default();
        let Some(duplication) = self.duplication.as_ref() else { return };
        if duplication.GetFramePointerShape(size, data.as_mut_ptr() as *mut c_void, &mut required, &mut shape).is_err() {
            return;
        }
        let Some(image) = cursor::convert(shape.Type, shape.Width, shape.Height, shape.Pitch, &data) else { return };
        self.cursor = self.upload_cursor(&image).ok();
    }

    unsafe fn upload_cursor(&self, image: &CursorImage) -> Result<Cursor, String> {
        let layer = |pixels: &[u8]| -> Result<Layer, String> {
            let texture = texture(&self.device, image.width, image.height, D3D11_BIND_SHADER_RESOURCE.0 as u32, Some(pixels))?;
            Ok(Layer { view: shader_view(&self.device, &texture)?, _texture: texture })
        };
        Ok(Cursor { width: image.width, height: image.height, alpha: layer(&image.alpha)?, invert: image.invert.as_deref().map(layer).transpose()? })
    }

    /// Gambar desktop (diskala, rasio terjaga) lalu kursor ke `frame`, lalu ubah `frame` ke `nv12`.
    pub fn render(&self) {
        let (left, top, width, height) = fit(self.source, self.target);
        let (scale_x, scale_y) = (width / self.source.0 as f32, height / self.source.1 as f32);
        let viewport = |x: f32, y: f32, w: f32, h: f32| D3D11_VIEWPORT { TopLeftX: x, TopLeftY: y, Width: w, Height: h, MinDepth: 0.0, MaxDepth: 1.0 };
        // SAFETY: semua objek milik device yang sama dan masih hidup; hanya dipakai dari thread video.
        unsafe {
            let context = &self.context;
            context.OMSetRenderTargets(Some(&[Some(self.frame_view.clone())]), None);
            context.ClearRenderTargetView(&self.frame_view, &[0.0, 0.0, 0.0, 1.0]);
            context.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            context.VSSetShader(&self.vertex_shader, None);
            context.PSSetShader(&self.pixel_shader, None);
            context.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));

            context.RSSetViewports(Some(&[viewport(left, top, width, height)]));
            context.OMSetBlendState(None, None, u32::MAX);
            context.PSSetShaderResources(0, Some(&[Some(self.desktop_view.clone())]));
            context.Draw(3, 0);

            if let (true, Some(cursor)) = (self.cursor_visible, &self.cursor) {
                // Kursor digambar dengan segitiga yang sama; viewport-nya dipersempit ke kotak kursor.
                let x = left + self.cursor_position.0 as f32 * scale_x;
                let y = top + self.cursor_position.1 as f32 * scale_y;
                context.RSSetViewports(Some(&[viewport(x, y, cursor.width as f32 * scale_x, cursor.height as f32 * scale_y)]));
                context.OMSetBlendState(&self.blend_alpha, None, u32::MAX);
                context.PSSetShaderResources(0, Some(&[Some(cursor.alpha.view.clone())]));
                context.Draw(3, 0);
                if let Some(invert) = &cursor.invert {
                    context.OMSetBlendState(&self.blend_invert, None, u32::MAX);
                    context.PSSetShaderResources(0, Some(&[Some(invert.view.clone())]));
                    context.Draw(3, 0);
                }
            }
            // BGRA -> NV12: Y pada resolusi penuh, UV pada setengahnya.
            let (w, h) = (self.target.0 as f32, self.target.1 as f32);
            context.OMSetBlendState(None, None, u32::MAX);
            context.OMSetRenderTargets(Some(&[Some(self.luma_view.clone())]), None);
            context.PSSetShaderResources(0, Some(&[Some(self.frame_source.clone())]));
            context.PSSetShader(&self.luma_shader, None);
            context.RSSetViewports(Some(&[viewport(0.0, 0.0, w, h)]));
            context.Draw(3, 0);
            context.OMSetRenderTargets(Some(&[Some(self.chroma_view.clone())]), None);
            context.PSSetShader(&self.chroma_shader, None);
            context.RSSetViewports(Some(&[viewport(0.0, 0.0, w / 2.0, h / 2.0)]));
            context.Draw(3, 0);
            // Lepas target dan sumber supaya encoder membaca tekstur yang sudah selesai digambar, dan `frame` boleh jadi
            // render target lagi di frame berikutnya.
            context.OMSetRenderTargets(None, None);
            context.PSSetShaderResources(0, Some(&[None]));
            context.Flush();
        }
    }
}
