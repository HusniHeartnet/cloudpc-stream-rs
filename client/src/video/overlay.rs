//! Teks di atas gambar stream: statistik (Ctrl+Alt+Shift+S, seperti "performance overlay" Moonlight) dan peringatan
//! koneksi lambat. Digambar dengan Direct2D + DirectWrite langsung ke buffer swapchain sebelum Present.

use windows::core::{w, Interface};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1DeviceContext, ID2D1Factory1, ID2D1SolidColorBrush, D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1,
    D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
};
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11Texture2D};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_MEASURING_MODE_NATURAL,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Dxgi::{IDXGIDevice, IDXGISurface};

pub struct Overlay {
    context: ID2D1DeviceContext,
    text: IDWriteTextFormat,
    warning_text: IDWriteTextFormat,
    white: ID2D1SolidColorBrush,
    yellow: ID2D1SolidColorBrush,
    shade: ID2D1SolidColorBrush,
}

fn color(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r, g, b, a }
}

impl Overlay {
    pub fn new(device: &ID3D11Device) -> Result<Overlay, String> {
        let fail = |what: &str, e: windows::core::Error| format!("overlay {what}: {e}");
        unsafe {
            let factory: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).map_err(|e| fail("D2D", e))?;
            let dxgi: IDXGIDevice = device.cast().map_err(|e| fail("DXGI", e))?;
            let d2d = factory.CreateDevice(&dxgi).map_err(|e| fail("D2D device", e))?;
            let context = d2d.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE).map_err(|e| fail("D2D context", e))?;
            let write: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).map_err(|e| fail("DirectWrite", e))?;
            let format = |weight, size: f32| write.CreateTextFormat(w!("Segoe UI"), None, weight, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, w!("id-ID"));
            let text = format(DWRITE_FONT_WEIGHT_NORMAL, 15.0).map_err(|e| fail("font", e))?;
            let warning_text = format(DWRITE_FONT_WEIGHT_SEMI_BOLD, 18.0).map_err(|e| fail("font", e))?;
            let brush = |c: D2D1_COLOR_F| context.CreateSolidColorBrush(&c, None);
            Ok(Overlay {
                white: brush(color(1.0, 1.0, 1.0, 1.0)).map_err(|e| fail("kuas", e))?,
                yellow: brush(color(1.0, 0.85, 0.2, 1.0)).map_err(|e| fail("kuas", e))?,
                shade: brush(color(0.0, 0.0, 0.0, 0.6)).map_err(|e| fail("kuas", e))?,
                context,
                text,
                warning_text,
            })
        }
    }

    /// Gambar `stats` (kiri atas) dan `warning` (tengah atas) ke `target` (buffer belakang swapchain).
    pub fn draw(&self, target: &ID3D11Texture2D, stats: Option<&str>, warning: Option<&str>) -> Result<(), String> {
        let fail = |what: &str, e: windows::core::Error| format!("overlay {what}: {e}");
        unsafe {
            let surface: IDXGISurface = target.cast().map_err(|e| fail("permukaan", e))?;
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0,
                dpiY: 96.0,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                ..Default::default()
            };
            let bitmap = self.context.CreateBitmapFromDxgiSurface(&surface, Some(&props)).map_err(|e| fail("bitmap", e))?;
            self.context.SetTarget(&bitmap);
            self.context.BeginDraw();
            let size = self.context.GetSize();
            if let Some(text) = stats {
                let lines = text.lines().count().max(1) as f32;
                let width = text.lines().map(|l| l.chars().count()).max().unwrap_or(0) as f32 * 7.6 + 24.0;
                self.context.FillRectangle(&D2D_RECT_F { left: 8.0, top: 8.0, right: 8.0 + width, bottom: 16.0 + lines * 20.0 }, &self.shade);
                let wide: Vec<u16> = text.encode_utf16().collect();
                self.context.DrawText(&wide, &self.text, &D2D_RECT_F { left: 18.0, top: 12.0, right: size.width, bottom: size.height }, &self.white, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            }
            if let Some(text) = warning {
                let width = text.chars().count() as f32 * 9.5 + 32.0;
                let left = (size.width - width) / 2.0;
                self.context.FillRectangle(&D2D_RECT_F { left, top: 10.0, right: left + width, bottom: 44.0 }, &self.shade);
                let wide: Vec<u16> = text.encode_utf16().collect();
                self.context.DrawText(&wide, &self.warning_text, &D2D_RECT_F { left: left + 16.0, top: 14.0, right: left + width, bottom: 44.0 }, &self.yellow, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            }
            let result = self.context.EndDraw(None, None);
            self.context.SetTarget(None);
            result.map_err(|e| fail("EndDraw", e))
        }
    }
}
