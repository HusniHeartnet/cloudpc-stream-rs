//! Fase C3. Jendela + swapchain D3D11 yang menampilkan frame hasil decode dengan latensi serendah mungkin.
//!
//! Rujukan: `moonlight-qt/app/streaming/video/ffmpeg-renderers/d3d11va.cpp`. Bedanya: konversi NV12 -> BGRA, skala,
//! dan letterbox dikerjakan Video Processor D3D11 (hardware video GPU), bukan shader. Swapchain flip-model dua buffer;
//! frame langsung ditampilkan tanpa menunggu vsync (tearing diizinkan bila didukung), seperti Moonlight dengan V-Sync mati.

use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::core::{w, Interface, BOOL};
use windows::Win32::Foundation::{HMODULE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_UNKNOWN, DXGI_RATIONAL, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, IDXGIFactory2, IDXGIFactory5, IDXGISwapChain1, DXGI_CREATE_FACTORY_FLAGS, DXGI_FEATURE_PRESENT_ALLOW_TEARING, DXGI_MWA_NO_ALT_ENTER, DXGI_PRESENT,
    DXGI_PRESENT_ALLOW_TEARING, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING, DXGI_SWAP_EFFECT_FLIP_DISCARD,
    DXGI_USAGE_RENDER_TARGET_OUTPUT,
};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::decode::{Picture, Surface};
use crate::rtsp::Codec;

const WINDOW_CLASS: windows::core::PCWSTR = w!("SDL_app");

static CLOSE_REQUESTED: AtomicBool = AtomicBool::new(false);
static RESIZED: AtomicBool = AtomicBool::new(false);
/// Rasio gambar stream (lebar << 32 | tinggi); 16:9 sampai frame pertama diketahui.
static ASPECT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new((16 << 32) | 9);

/// Rasio stream dalam bilangan terkecil (1920x1080 -> 16:9), supaya ukuran klien bisa dibuat kelipatan persisnya.
fn aspect() -> (i64, i64) {
    let v = ASPECT.load(Ordering::Relaxed);
    reduce(((v >> 32).max(1) as i64, (v & 0xFFFF_FFFF).max(1) as i64))
}

fn reduce((w, h): (i64, i64)) -> (i64, i64) {
    let (mut a, mut b) = (w, h);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    (w / a.max(1), h / a.max(1))
}

/// Tebal bingkai jendela (judul + tepi): selisih ukuran jendela dan area klien untuk gaya jendela ini.
unsafe fn frame_size(window: HWND) -> (i64, i64) {
    let style = WINDOW_STYLE(GetWindowLongPtrW(window, GWL_STYLE) as u32);
    let ex = WINDOW_EX_STYLE(GetWindowLongPtrW(window, GWL_EXSTYLE) as u32);
    let mut rect = RECT::default();
    let _ = AdjustWindowRectEx(&mut rect, style, false, ex);
    ((rect.right - rect.left) as i64, (rect.bottom - rect.top) as i64)
}

/// Saat tepi jendela ditarik: area klien tetap berasio stream (tanpa bilah hitam). `edge` = WMSZ_* dari WM_SIZING.
pub fn sizing_rect(rect: RECT, edge: u32, frame: (i64, i64), aspect: (i64, i64)) -> RECT {
    let (aw, ah) = reduce(aspect);
    let mut r = rect;
    let client_w = (r.right - r.left) as i64 - frame.0;
    let client_h = (r.bottom - r.top) as i64 - frame.1;
    // Klien = k kali rasio (16:9 -> 16k x 9k): persis, tanpa sisa piksel yang jadi bilah tipis.
    let k = match edge {
        3 | 6 => client_h / ah,
        _ => client_w / aw,
    }
    .max(8);
    let (width, height) = ((k * aw + frame.0) as i32, (k * ah + frame.1) as i32);
    // Tepi kiri/atas yang ditarik menggeser posisi; lainnya menggeser kanan/bawah.
    if matches!(edge, 1 | 4 | 7) {
        r.left = r.right - width;
    } else {
        r.right = r.left + width;
    }
    if matches!(edge, 3..=5) {
        r.top = r.bottom - height;
    } else {
        r.bottom = r.top + height;
    }
    r
}

/// Ukuran jendela terbesar yang muat di `cx`x`cy` dengan area klien persis k kali rasio stream (tanpa bilah hitam).
/// Dipakai untuk SEMUA perubahan ukuran selain tarik tepi dan maksimalkan: snap Windows, keluar dari layar penuh,
/// ukuran awal. Temuan 19 Nov 2026: keluar dari layar penuh memberi jendela 1280x720 TERMASUK bingkai -> bilah kiri-kanan.
pub fn fitted_size(cx: i32, cy: i32, frame: (i64, i64), aspect: (i64, i64)) -> (i32, i32) {
    let (aw, ah) = reduce(aspect);
    let k = ((cx as i64 - frame.0) / aw).min((cy as i64 - frame.1) / ah).max(8);
    ((k * aw + frame.0) as i32, (k * ah + frame.1) as i32)
}

/// Ukuran dan posisi jendela saat dimaksimalkan: area klien terbesar berasio stream yang muat di area kerja, di tengah.
/// Kembalian (posisi relatif monitor, ukuran jendela).
pub fn maximized_rect(work: RECT, monitor: RECT, frame: (i64, i64), aspect: (i64, i64)) -> ((i32, i32), (i32, i32)) {
    let (aw, ah) = reduce(aspect);
    let avail_w = (work.right - work.left) as i64 - frame.0;
    let avail_h = (work.bottom - work.top) as i64 - frame.1;
    let k = (avail_w / aw).min(avail_h / ah).max(1);
    let (client_w, client_h) = (k * aw, k * ah);
    let (w, h) = ((client_w + frame.0) as i32, (client_h + frame.1) as i32);
    let x = work.left - monitor.left + ((work.right - work.left) - w) / 2;
    let y = work.top - monitor.top + ((work.bottom - work.top) - h) / 2;
    ((x, y), (w, h))
}

unsafe extern "system" fn window_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if let Some(result) = crate::input::win::window_message(message, wparam, lparam) {
        return result;
    }
    match message {
        // Tombol X / Alt+F4: jendela tidak dihancurkan di sini; pemilik thread menutup sesi lalu jendelanya.
        WM_CLOSE => {
            CLOSE_REQUESTED.store(true, Ordering::SeqCst);
            LRESULT(0)
        }
        WM_SIZE => {
            RESIZED.store(true, Ordering::SeqCst);
            LRESULT(0)
        }
        // Layar penuh: saat jendela aktif lagi, tegaskan ukuran = seluruh monitor di atas jendela lain, supaya Windows
        // menyembunyikan taskbar. Bila jendela belum jadi jendela depan saat dibuka, taskbar laptop tetap terlihat
        // (temuan pemilik 19 Nov 2026).
        WM_ACTIVATE if (wparam.0 & 0xFFFF) != 0 && !WINDOW_STYLE(GetWindowLongPtrW(window, GWL_STYLE) as u32).contains(WS_CAPTION) => {
            let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            if GetMonitorInfoW(MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST), &mut info).as_bool() {
                let r = info.rcMonitor;
                let _ = SetWindowPos(window, Some(HWND_TOP), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOACTIVATE | SWP_FRAMECHANGED);
            }
            DefWindowProcW(window, message, wparam, lparam)
        }
        // Tarik tepi: pertahankan rasio stream supaya tidak ada bilah hitam (Moonlight membiarkan bilah).
        WM_SIZING => {
            let rect = &mut *(lparam.0 as *mut RECT);
            *rect = sizing_rect(*rect, wparam.0 as u32, frame_size(window), aspect());
            LRESULT(1)
        }
        // Maksimalkan: ukuran terbesar berasio stream di tengah layar, bukan seluruh layar dengan bilah hitam.
        WM_GETMINMAXINFO => {
            let info = &mut *(lparam.0 as *mut MINMAXINFO);
            let mut monitor = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            if GetMonitorInfoW(MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST), &mut monitor).as_bool() {
                let ((x, y), (w, h)) = maximized_rect(monitor.rcWork, monitor.rcMonitor, frame_size(window), aspect());
                info.ptMaxPosition = POINT { x, y };
                info.ptMaxSize = POINT { x: w, y: h };
            }
            LRESULT(0)
        }
        // Perubahan ukuran lain (snap, SetWindowPos, keluar dari layar penuh): sesuaikan ke rasio stream. Layar penuh
        // (tanpa judul), dimaksimalkan, dan diminimalkan dibiarkan.
        WM_WINDOWPOSCHANGING => {
            let pos = &mut *(lparam.0 as *mut WINDOWPOS);
            let style = WINDOW_STYLE(GetWindowLongPtrW(window, GWL_STYLE) as u32);
            let resizable = !pos.flags.contains(SWP_NOSIZE) && style.contains(WS_CAPTION) && !IsIconic(window).as_bool() && !IsZoomed(window).as_bool();
            if resizable && pos.cx > 0 && pos.cy > 0 {
                (pos.cx, pos.cy) = fitted_size(pos.cx, pos.cy, frame_size(window), aspect());
            }
            DefWindowProcW(window, message, wparam, lparam)
        }
        // Latar digambar swapchain; tanpa ini Windows menghapusnya putih dan layar berkedip saat diubah ukurannya.
        WM_ERASEBKGND => LRESULT(1),
        // Alt yang diteruskan ke Windows (mode jendela) jangan membuka menu jendela: tombol berikutnya harus ke host.
        WM_SYSCOMMAND if (wparam.0 & 0xFFF0) as u32 == SC_KEYMENU => LRESULT(0),
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

/// Bawa jendela ke depan dan beri fokus. Proses yang dijalankan aplikasi lain tidak boleh langsung merebut fokus
/// (SetForegroundWindow ditolak dan jendela muncul DI BELAKANG aplikasi CloudPC, ditemukan sesi #85), jadi antrean
/// input thread ini disambungkan sebentar ke thread jendela yang sedang di depan, seperti yang dilakukan Moonlight/SDL.
fn bring_to_front(window: HWND) {
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
    unsafe {
        let foreground = GetForegroundWindow();
        let theirs = GetWindowThreadProcessId(foreground, None);
        let ours = GetCurrentThreadId();
        let attached = theirs != 0 && theirs != ours && AttachThreadInput(ours, theirs, true).as_bool();
        let _ = BringWindowToTop(window);
        let _ = SetForegroundWindow(window);
        let _ = SetFocus(Some(window));
        if attached {
            let _ = AttachThreadInput(ours, theirs, false);
        }
    }
}

fn describe(what: &str, error: windows::core::Error) -> String {
    format!("{what}: {error}")
}

/// Ukuran area klien jendela.
fn client_size(window: HWND) -> (u32, u32) {
    let mut rect = RECT::default();
    unsafe {
        let _ = GetClientRect(window, &mut rect);
    }
    ((rect.right - rect.left).max(1) as u32, (rect.bottom - rect.top).max(1) as u32)
}

/// Kotak tujuan yang mempertahankan rasio gambar (letterbox/pillarbox), di tengah area keluaran.
pub fn fit(source: (u32, u32), target: (u32, u32)) -> RECT {
    let (sw, sh) = (u64::from(source.0.max(1)), u64::from(source.1.max(1)));
    let (tw, th) = (u64::from(target.0), u64::from(target.1));
    let (w, h) = if tw * sh <= th * sw { (tw, tw * sh / sw) } else { (th * sw / sh, th) };
    let (x, y) = ((tw - w) / 2, (th - h) / 2);
    RECT { left: x as i32, top: y as i32, right: (x + w) as i32, bottom: (y + h) as i32 }
}

/// Tekstur NV12 untuk gambar hasil decode perangkat lunak: staging (ditulis CPU) -> tekstur GPU (masukan Video Processor).
struct Upload {
    size: (u32, u32),
    staging: ID3D11Texture2D,
    texture: ID3D11Texture2D,
}

struct Processor {
    input: (u32, u32),
    output: (u32, u32),
    enumerator: ID3D11VideoProcessorEnumerator,
    processor: ID3D11VideoProcessor,
    /// Pandangan input per (tekstur, irisan); FFmpeg memakai ulang kumpulan tekstur yang sama.
    views: Vec<(usize, u32, ID3D11VideoProcessorInputView)>,
}

pub struct Renderer {
    pub window: HWND,
    pub device: ID3D11Device,
    context: ID3D11DeviceContext,
    video_device: ID3D11VideoDevice,
    video_context: ID3D11VideoContext,
    swapchain: IDXGISwapChain1,
    tearing: bool,
    processor: Option<Processor>,
    fullscreen: bool,
    /// Posisi dan ukuran jendela sebelum layar penuh, dipulihkan saat keluar.
    windowed: Option<RECT>,
    upload: Option<Upload>,
    overlay: Option<super::overlay::Overlay>,
    /// Tunggu refresh layar saat Present (`--vsync`): tanpa robekan gambar, latensi bertambah sampai satu refresh.
    pub vsync: bool,
    pub presented: u64,
}

impl Renderer {
    /// Buat jendela (`title`, area klien `width`x`height`, atau layar penuh tanpa bingkai) dan device D3D11 dengan
    /// dukungan video. Harus dipanggil di thread yang nanti memanggil `pump`.
    pub fn new(title: &str, width: u32, height: u32, fullscreen: bool) -> Result<Renderer, String> {
        unsafe {
            CLOSE_REQUESTED.store(false, Ordering::SeqCst);
            let instance = GetModuleHandleW(None).map_err(|e| describe("GetModuleHandle", e))?;
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(window_proc),
                hInstance: instance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                // Ikon CloudPC dari resource exe (ID 1, `assets/cloudpc-stream.rc`); tanpa itu ikon bawaan Windows.
                hIcon: LoadImageW(Some(instance.into()), windows::core::PCWSTR(1 as _), IMAGE_ICON, 0, 0, LR_DEFAULTSIZE | LR_SHARED).map(|h| HICON(h.0)).unwrap_or_default(),
                hIconSm: LoadImageW(Some(instance.into()), windows::core::PCWSTR(1 as _), IMAGE_ICON, GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON), LR_SHARED).map(|h| HICON(h.0)).unwrap_or_default(),
                // Nama kelas sama dengan jendela stream Moonlight (SDL): aplikasi CloudPC menutup stream dengan WM_CLOSE ke
                // kelas "SDL_app" supaya statistik sempat ditulis, jadi client ini bisa langsung menggantikan Moonlight.
                lpszClassName: WINDOW_CLASS,
                ..Default::default()
            };
            RegisterClassExW(&class); // gagal = kelas sudah terdaftar (sesi kedua di proses yang sama); tidak masalah.
            let mut rect = RECT { left: 0, top: 0, right: width as i32, bottom: height as i32 };
            let _ = AdjustWindowRectEx(&mut rect, WS_OVERLAPPEDWINDOW, false, WINDOW_EX_STYLE(0));
            let title: Vec<u16> = title.encode_utf16().chain([0]).collect();
            let window = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                WINDOW_CLASS,
                windows::core::PCWSTR(title.as_ptr()),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                rect.right - rect.left,
                rect.bottom - rect.top,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .map_err(|e| describe("CreateWindow", e))?;

            let mut device = None;
            let mut context = None;
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .map_err(|e| describe("D3D11CreateDevice", e))?;
            let device: ID3D11Device = device.ok_or("tanpa device D3D11")?;
            let context: ID3D11DeviceContext = context.ok_or("tanpa context D3D11")?;
            // Decoder FFmpeg memakai device yang sama; lindungi dari pemanggilan bersamaan.
            if let Ok(multithread) = context.cast::<ID3D11Multithread>() {
                let _ = multithread.SetMultithreadProtected(true);
            }
            let video_device: ID3D11VideoDevice = device.cast().map_err(|e| describe("ID3D11VideoDevice", e))?;
            let video_context: ID3D11VideoContext = context.cast().map_err(|e| describe("ID3D11VideoContext", e))?;

            let factory: IDXGIFactory2 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)).map_err(|e| describe("CreateDXGIFactory2", e))?;
            let mut allow = BOOL(0);
            let tearing = factory
                .cast::<IDXGIFactory5>()
                .is_ok_and(|f| f.CheckFeatureSupport(DXGI_FEATURE_PRESENT_ALLOW_TEARING, &mut allow as *mut BOOL as *mut _, std::mem::size_of::<BOOL>() as u32).is_ok() && allow.as_bool());
            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
                Flags: if tearing { DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING.0 as u32 } else { 0 },
                ..Default::default()
            };
            let swapchain = factory.CreateSwapChainForHwnd(&device, window, &desc, None, None).map_err(|e| describe("CreateSwapChainForHwnd", e))?;
            let _ = factory.MakeWindowAssociation(window, DXGI_MWA_NO_ALT_ENTER);

            let mut renderer = Renderer { window, device, context, video_device, video_context, swapchain, tearing, processor: None, fullscreen: false, windowed: None, upload: None, overlay: None, vsync: false, presented: 0 };
            if fullscreen {
                renderer.set_fullscreen(true);
            } else {
                // Stream sebesar layar (1920x1080 di layar 1920x1080) tidak muat sebagai jendela: perkecil ke 90% area kerja.
                let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
                let mut now = RECT::default();
                if GetMonitorInfoW(MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST), &mut info).as_bool() && GetWindowRect(window, &mut now).is_ok() {
                    let work = info.rcWork;
                    let (ww, wh) = (work.right - work.left, work.bottom - work.top);
                    if now.right - now.left > ww || now.bottom - now.top > wh {
                        let (w, h) = fitted_size(ww * 9 / 10, wh * 9 / 10, frame_size(window), aspect());
                        let _ = SetWindowPos(window, None, work.left + (ww - w) / 2, work.top + (wh - h) / 2, w, h, SWP_NOZORDER);
                    }
                }
            }
            let _ = ShowWindow(window, SW_SHOW);
            bring_to_front(window);
            Ok(renderer)
        }
    }

    /// Layar penuh tanpa bingkai di monitor tempat jendela berada (bukan mode eksklusif: Alt+Tab tetap cepat).
    pub fn set_fullscreen(&mut self, on: bool) {
        unsafe {
            if on {
                let mut current = RECT::default();
                if !self.fullscreen && GetWindowRect(self.window, &mut current).is_ok() && !IsZoomed(self.window).as_bool() {
                    self.windowed = Some(current);
                }
                let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
                let _ = GetMonitorInfoW(MonitorFromWindow(self.window, MONITOR_DEFAULTTONEAREST), &mut info);
                let r = info.rcMonitor;
                SetWindowLongPtrW(self.window, GWL_STYLE, (WS_POPUP | WS_VISIBLE).0 as isize);
                let _ = SetWindowPos(self.window, Some(HWND_TOP), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_FRAMECHANGED | SWP_SHOWWINDOW);
                bring_to_front(self.window);
            } else {
                SetWindowLongPtrW(self.window, GWL_STYLE, (WS_OVERLAPPEDWINDOW | WS_VISIBLE).0 as isize);
                // Ukuran sebelum layar penuh; tanpa itu klien 1280x720. WM_WINDOWPOSCHANGING menyesuaikannya ke rasio stream.
                let frame = frame_size(self.window);
                let r = self.windowed.unwrap_or(RECT { left: 100, top: 100, right: 100 + 1280 + frame.0 as i32, bottom: 100 + 720 + frame.1 as i32 });
                let _ = SetWindowPos(self.window, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_FRAMECHANGED | SWP_SHOWWINDOW | SWP_NOZORDER);
            }
        }
        self.fullscreen = on;
        crate::input::win::set_fullscreen(on);
        RESIZED.store(true, Ordering::SeqCst);
    }

    /// Codec yang bisa didecode GPU ini (profil decoder D3D11 dengan keluaran NV12), seperti pemeriksaan Moonlight
    /// sebelum memilih codec. Urutan: H.264, HEVC, AV1.
    pub fn hardware_codecs(&self) -> Vec<Codec> {
        let profiles = [(Codec::H264, D3D11_DECODER_PROFILE_H264_VLD_NOFGT), (Codec::Hevc, D3D11_DECODER_PROFILE_HEVC_VLD_MAIN), (Codec::Av1, D3D11_DECODER_PROFILE_AV1_VLD_PROFILE0)];
        let available: Vec<windows::core::GUID> = unsafe { (0..self.video_device.GetVideoDecoderProfileCount()).filter_map(|i| self.video_device.GetVideoDecoderProfile(i).ok()).collect() };
        profiles
            .into_iter()
            .filter(|(_, guid)| available.contains(guid) && unsafe { self.video_device.CheckVideoDecoderFormat(guid, windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_NV12) }.is_ok_and(|b| b.as_bool()))
            .map(|(codec, _)| codec)
            .collect()
    }

    /// Salin gambar NV12 dari memori ke tekstur GPU.
    fn upload(&mut self, width: u32, height: u32, nv12: &[u8]) -> Result<ID3D11Texture2D, String> {
        if self.upload.as_ref().is_none_or(|u| u.size != (width, height)) {
            let mut desc = D3D11_TEXTURE2D_DESC {
                Width: width,
                Height: height,
                MipLevels: 1,
                ArraySize: 1,
                Format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_NV12,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_STAGING,
                CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                ..Default::default()
            };
            let (mut staging, mut texture) = (None, None);
            unsafe {
                self.device.CreateTexture2D(&desc, None, Some(&mut staging)).map_err(|e| describe("tekstur staging NV12", e))?;
                desc.Usage = D3D11_USAGE_DEFAULT;
                desc.CPUAccessFlags = 0;
                desc.BindFlags = D3D11_BIND_RENDER_TARGET.0 as u32;
                self.device.CreateTexture2D(&desc, None, Some(&mut texture)).map_err(|e| describe("tekstur NV12", e))?;
            }
            self.upload = Some(Upload { size: (width, height), staging: staging.ok_or("tanpa staging")?, texture: texture.ok_or("tanpa tekstur")? });
        }
        let upload = self.upload.as_ref().expect("baru dibuat");
        let (w, h) = (width as usize, height as usize);
        unsafe {
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context.Map(&upload.staging, 0, D3D11_MAP_WRITE, 0, Some(&mut mapped)).map_err(|e| describe("Map NV12", e))?;
            let pitch = mapped.RowPitch as usize;
            let base = mapped.pData as *mut u8;
            // NV12 terpetakan: h baris Y, lalu h/2 baris UV, dengan lebar baris yang sama.
            for row in 0..h + h.div_ceil(2) {
                let source = &nv12[row * w..(row * w + w).min(nv12.len())];
                std::ptr::copy_nonoverlapping(source.as_ptr(), base.add(row * pitch), source.len());
            }
            self.context.Unmap(&upload.staging, 0);
            self.context.CopyResource(&upload.texture, &upload.staging);
        }
        Ok(upload.texture.clone())
    }

    pub fn minimize(&self) {
        unsafe {
            let _ = ShowWindow(self.window, SW_MINIMIZE);
        }
    }

    /// Minta jendela ditutup (seperti tombol X): sesi diakhiri oleh pemilik thread.
    pub fn request_close(&self) {
        CLOSE_REQUESTED.store(true, Ordering::SeqCst);
    }

    pub fn is_fullscreen(&self) -> bool {
        self.fullscreen
    }

    /// Kerjakan pesan jendela yang menunggu. `false` bila pengguna menutup jendela.
    pub fn pump(&mut self) -> bool {
        unsafe {
            let mut message = MSG::default();
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        !CLOSE_REQUESTED.load(Ordering::SeqCst)
    }

    fn output_size(&mut self) -> Result<(u32, u32), String> {
        let size = client_size(self.window);
        if RESIZED.swap(false, Ordering::SeqCst) {
            // Semua pandangan ke buffer lama harus dilepas dulu (pandangan output dibuat per frame, jadi aman).
            let flags = if self.tearing { DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING } else { DXGI_SWAP_CHAIN_FLAG(0) };
            unsafe {
                self.context.ClearState();
                if let Err(e) = self.swapchain.ResizeBuffers(0, size.0, size.1, DXGI_FORMAT_UNKNOWN, flags) {
                    // Coba lagi di frame berikutnya; sementara gambar ke buffer lama (Windows menskalakannya).
                    RESIZED.store(true, Ordering::SeqCst);
                    super::note(format!("ResizeBuffers {}x{} gagal: {e}", size.0, size.1));
                    let desc = self.swapchain.GetDesc1().map_err(|e| describe("GetDesc1", e))?;
                    return Ok((desc.Width.max(1), desc.Height.max(1)));
                }
            }
        }
        Ok(size)
    }

    fn processor(&mut self, input: (u32, u32), output: (u32, u32)) -> Result<&mut Processor, String> {
        let stale = self.processor.as_ref().is_none_or(|p| p.input != input || p.output != output);
        if stale {
            let desc = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
                InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
                InputFrameRate: DXGI_RATIONAL { Numerator: 60, Denominator: 1 },
                InputWidth: input.0,
                InputHeight: input.1,
                OutputFrameRate: DXGI_RATIONAL { Numerator: 60, Denominator: 1 },
                OutputWidth: output.0,
                OutputHeight: output.1,
                Usage: D3D11_VIDEO_USAGE_OPTIMAL_SPEED,
            };
            unsafe {
                let enumerator = self.video_device.CreateVideoProcessorEnumerator(&desc).map_err(|e| describe("CreateVideoProcessorEnumerator", e))?;
                let processor = self.video_device.CreateVideoProcessor(&enumerator, 0).map_err(|e| describe("CreateVideoProcessor", e))?;
                let vc = &self.video_context;
                // Tanpa penghalusan/penajaman otomatis driver: gambar apa adanya, latensi tetap.
                vc.VideoProcessorSetStreamAutoProcessingMode(&processor, 0, false);
                vc.VideoProcessorSetStreamFrameFormat(&processor, 0, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE);
                let full = RECT { left: 0, top: 0, right: output.0 as i32, bottom: output.1 as i32 };
                vc.VideoProcessorSetOutputTargetRect(&processor, true, Some(&full));
                vc.VideoProcessorSetStreamSourceRect(&processor, 0, true, Some(&RECT { left: 0, top: 0, right: input.0 as i32, bottom: input.1 as i32 }));
                vc.VideoProcessorSetStreamDestRect(&processor, 0, true, Some(&fit(input, output)));
                let black = D3D11_VIDEO_COLOR { Anonymous: D3D11_VIDEO_COLOR_0 { RGBA: D3D11_VIDEO_COLOR_RGBA { R: 0.0, G: 0.0, B: 0.0, A: 1.0 } } };
                vc.VideoProcessorSetOutputBackgroundColor(&processor, false, &black);
                // Keluaran: RGB rentang penuh 0-255 (bit RGB_Range = 0).
                vc.VideoProcessorSetOutputColorSpace(&processor, &D3D11_VIDEO_PROCESSOR_COLOR_SPACE { _bitfield: 0 });
                self.processor = Some(Processor { input, output, enumerator, processor, views: Vec::new() });
            }
        }
        Ok(self.processor.as_mut().expect("baru dibuat"))
    }

    /// Gambar satu frame hasil decode ke jendela (dengan teks statistik/peringatan bila ada) dan tampilkan sekarang.
    pub fn present(&mut self, picture: &Picture, stats: Option<&str>, warning: Option<&str>) -> Result<(), String> {
        if (stats.is_some() || warning.is_some()) && self.overlay.is_none() {
            self.overlay = Some(super::overlay::Overlay::new(&self.device)?);
        }
        let (texture, index) = match &picture.surface {
            Surface::Gpu { texture, index } => (texture.clone(), *index),
            Surface::Cpu(nv12) => (self.upload(picture.width, picture.height, nv12)?, 0),
        };
        let output = self.output_size()?;
        let video_device = self.video_device.clone();
        let video_context = self.video_context.clone();
        let swapchain = self.swapchain.clone();
        let processor = self.processor((picture.width, picture.height), output)?;
        unsafe {
            // Ruang warna masukan dari VUI aliran: bit YCbCr_Matrix (1 = BT.709) dan Nominal_Range (1 = 16-235, 2 = 0-255).
            let matrix = if picture.bt601 { 0 } else { 1 << 2 };
            let range = if picture.full_range { 2 << 4 } else { 1 << 4 };
            video_context.VideoProcessorSetStreamColorSpace(&processor.processor, 0, &D3D11_VIDEO_PROCESSOR_COLOR_SPACE { _bitfield: matrix | range });

            let key = (texture.as_raw() as usize, index);
            let input_view = match processor.views.iter().find(|(t, i, _)| (*t, *i) == key) {
                Some((_, _, view)) => view.clone(),
                None => {
                    let desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
                        FourCC: 0,
                        ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
                        Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_VPIV { MipSlice: 0, ArraySlice: index } },
                    };
                    let mut view = None;
                    video_device
                        .CreateVideoProcessorInputView(&texture, &processor.enumerator, &desc, Some(&mut view))
                        .map_err(|e| describe("CreateVideoProcessorInputView", e))?;
                    let view = view.ok_or("tanpa input view")?;
                    if processor.views.len() > 64 {
                        processor.views.clear(); // kumpulan tekstur decoder berganti (mis. resolusi berubah)
                    }
                    processor.views.push((key.0, key.1, view.clone()));
                    view
                }
            };

            let back_buffer: ID3D11Texture2D = swapchain.GetBuffer(0).map_err(|e| describe("GetBuffer", e))?;
            let desc = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 } },
            };
            let mut output_view = None;
            video_device
                .CreateVideoProcessorOutputView(&back_buffer, &processor.enumerator, &desc, Some(&mut output_view))
                .map_err(|e| describe("CreateVideoProcessorOutputView", e))?;
            let output_view = output_view.ok_or("tanpa output view")?;

            let mut stream = D3D11_VIDEO_PROCESSOR_STREAM { Enable: true.into(), pInputSurface: ManuallyDrop::new(Some(input_view)), ..Default::default() };
            let result = video_context.VideoProcessorBlt(&processor.processor, &output_view, 0, std::slice::from_ref(&stream));
            ManuallyDrop::drop(&mut stream.pInputSurface);
            result.map_err(|e| describe("VideoProcessorBlt", e))?;
            if let Some(overlay) = self.overlay.as_ref().filter(|_| stats.is_some() || warning.is_some()) {
                overlay.draw(&back_buffer, stats, warning)?;
            }

            let (interval, flags) = match (self.vsync, self.tearing) {
                (true, _) => (1, DXGI_PRESENT(0)),
                (false, true) => (0, DXGI_PRESENT_ALLOW_TEARING),
                (false, false) => (0, DXGI_PRESENT(0)),
            };
            swapchain.Present(interval, flags).ok().map_err(|e| describe("Present", e))?;
        }
        if let Some(p) = &self.processor {
            crate::input::win::set_picture(fit(p.input, p.output));
            ASPECT.store((u64::from(p.input.0) << 32) | u64::from(p.input.1), Ordering::Relaxed);
        }
        self.presented += 1;
        Ok(())
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.window);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fit;
    use windows::Win32::Foundation::RECT;

    #[test]
    fn dragging_edges_keeps_the_stream_shape() {
        use super::sizing_rect;
        let frame = (16, 39); // tepi kiri+kanan, judul+tepi bawah
        let rect = |l, t, r, b| RECT { left: l, top: t, right: r, bottom: b };
        let size = |r: RECT| ((r.right - r.left) as i64 - frame.0, (r.bottom - r.top) as i64 - frame.1);
        // Tarik kanan sampai klien 1600 lebar -> tinggi 900.
        assert_eq!(size(sizing_rect(rect(100, 100, 100 + 1616, 100 + 500), 2, frame, (16, 9))), (1600, 900));
        // Tarik bawah sampai klien 450 tinggi -> lebar 800.
        assert_eq!(size(sizing_rect(rect(0, 0, 2000, 489), 6, frame, (16, 9))), (800, 450));
        // Sudut kiri atas: tepi bawah tetap, atas bergeser.
        let r = sizing_rect(rect(0, 0, 1296, 300), 4, frame, (16, 9));
        assert_eq!((size(r), r.bottom, r.right), ((1280, 720), 300, 1296), "tepi kanan dan bawah tetap");
        // Ukuran ganjil dibulatkan ke kelipatan 16x9 (tanpa bilah 1 piksel), rasio 1920x1080 disederhanakan.
        assert_eq!(size(sizing_rect(rect(0, 0, 1016 + 7, 100), 2, frame, (1920, 1080))), (992, 558));
    }

    #[test]
    fn any_other_resize_is_fitted_to_the_stream_shape() {
        use super::fitted_size;
        let frame = (16, 39);
        let client = |(w, h): (i32, i32)| (w as i64 - frame.0, h as i64 - frame.1);
        // Keluar dari layar penuh dulu memberi jendela 1280x720 termasuk bingkai: klien harus tetap 16:9 tanpa bilah.
        let (w, h) = client(fitted_size(1280, 720, frame, (1920, 1080)));
        assert_eq!(w * 9, h * 16, "{w}x{h}");
        assert!(w <= 1280 - 16 && h <= 720 - 39, "tidak lebih besar dari yang diminta");
        // Snap setengah layar (960x1032): muat dan persis 16:9.
        let (w, h) = client(fitted_size(960, 1032, frame, (16, 9)));
        assert_eq!((w * 9, w <= 944), (h * 16, true));
        // Ukuran yang sudah pas tidak berubah (tarik tepi lalu WM_WINDOWPOSCHANGING tidak saling melawan).
        assert_eq!(fitted_size(1600 + 16, 900 + 39, frame, (16, 9)), (1616, 939));
    }

    #[test]
    fn maximizing_fills_the_screen_without_bars() {
        use super::maximized_rect;
        let screen = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
        let work = RECT { bottom: 1032, ..screen }; // taskbar 48 px
        let ((x, y), (w, h)) = maximized_rect(work, screen, (16, 39), (16, 9));
        assert_eq!((w - 16) * 9, (h - 39) * 16, "klien persis 16:9");
        assert!(h <= 1032 && w <= 1920 && x >= 0 && y >= 0, "{x},{y} {w}x{h}");
        assert_eq!(x, (1920 - w) / 2, "di tengah");
    }

    #[test]
    fn picture_keeps_its_aspect_ratio() {
        let r = fit((1920, 1080), (1920, 1080));
        assert_eq!((r.left, r.top, r.right, r.bottom), (0, 0, 1920, 1080));
        let r = fit((1920, 1080), (1000, 1000));
        assert_eq!((r.left, r.top, r.right, r.bottom), (0, 219, 1000, 781), "letterbox atas-bawah: tinggi 562, sisa 438 dibagi dua");
        let r = fit((1280, 720), (1920, 1200));
        assert_eq!((r.left, r.top, r.right, r.bottom), (0, 60, 1920, 1140));
        let r = fit((1920, 1080), (2560, 1080));
        assert_eq!((r.left, r.top, r.right, r.bottom), (320, 0, 2240, 1080), "pillarbox kiri-kanan");
    }
}
