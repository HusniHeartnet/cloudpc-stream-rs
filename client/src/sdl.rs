//! SDL3 dimuat saat jalan (tanpa tautan saat build): jendela, renderer, kejadian, suara, mouse, dan clipboard untuk
//! tampilan SDL (`frontend_sdl.rs`, wajib di macOS seperti Moonlight) dan stik (`gamepad.rs`).
//! Konstanta dan tata letak struct diambil dari header SDL 3.4.20 (`SDL_events.h`, `SDL_pixels.h`, dll.).

use std::ffi::{c_char, c_void, CStr};
use std::path::PathBuf;

pub const INIT_AUDIO: u32 = 0x10;
pub const INIT_VIDEO: u32 = 0x20;
pub const INIT_GAMEPAD: u32 = 0x2000;

pub const WINDOW_FULLSCREEN: u64 = 0x1;
pub const WINDOW_RESIZABLE: u64 = 0x20;
pub const WINDOW_HIGH_PIXEL_DENSITY: u64 = 0x2000;

pub const PIXELFORMAT_NV12: u32 = 0x3231_564e;
pub const TEXTUREACCESS_STREAMING: i64 = 1;
pub const COLORSPACE_BT601_LIMITED: i64 = 0x2110_18c6;
pub const COLORSPACE_BT601_FULL: i64 = 0x2210_18c6;
pub const COLORSPACE_BT709_LIMITED: i64 = 0x2110_0421;
pub const COLORSPACE_BT709_FULL: i64 = 0x2210_0421;

pub const AUDIO_F32LE: u32 = 0x8120;
pub const AUDIO_DEVICE_DEFAULT_PLAYBACK: u32 = 0xFFFF_FFFF;

pub mod event {
    pub const QUIT: u32 = 0x100;
    pub const WINDOW_PIXEL_SIZE_CHANGED: u32 = 0x207;
    pub const WINDOW_FOCUS_GAINED: u32 = 0x20E;
    pub const WINDOW_FOCUS_LOST: u32 = 0x20F;
    pub const WINDOW_CLOSE_REQUESTED: u32 = 0x210;
    pub const KEY_DOWN: u32 = 0x300;
    pub const KEY_UP: u32 = 0x301;
    pub const MOUSE_MOTION: u32 = 0x400;
    pub const MOUSE_BUTTON_DOWN: u32 = 0x401;
    pub const MOUSE_BUTTON_UP: u32 = 0x402;
    pub const MOUSE_WHEEL: u32 = 0x403;
}

/// `SDL_Event` (union 128 byte). Dibaca lewat offset sesuai struct di SDL_events.h.
#[repr(C, align(8))]
pub struct Event(pub [u8; 128]);

impl Event {
    pub fn kind(&self) -> u32 {
        self.u32(0)
    }
    pub fn u32(&self, at: usize) -> u32 {
        u32::from_ne_bytes(self.0[at..at + 4].try_into().unwrap())
    }
    pub fn i32(&self, at: usize) -> i32 {
        i32::from_ne_bytes(self.0[at..at + 4].try_into().unwrap())
    }
    pub fn f32(&self, at: usize) -> f32 {
        f32::from_ne_bytes(self.0[at..at + 4].try_into().unwrap())
    }
    pub fn u8(&self, at: usize) -> u8 {
        self.0[at]
    }
}

/// Offset field kejadian (SDL 3.4): keyboard {scancode 24, key 28, mod 32, raw 34, down 36, repeat 37};
/// mouse motion {x 28, y 32, xrel 36, yrel 40}; tombol {button 24, down 25}; roda {x 24, y 28, direction 32}.
pub mod at {
    pub const KEY_SCANCODE: usize = 24;
    pub const KEY_REPEAT: usize = 37;
    pub const MOTION_X: usize = 28;
    pub const MOTION_Y: usize = 32;
    pub const MOTION_XREL: usize = 36;
    pub const MOTION_YREL: usize = 40;
    pub const BUTTON: usize = 24;
    pub const WHEEL_X: usize = 24;
    pub const WHEEL_Y: usize = 28;
    pub const WHEEL_DIRECTION: usize = 32;
}

#[repr(C)]
pub struct AudioSpec {
    pub format: u32,
    pub channels: i32,
    pub freq: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub type Ptr = *mut c_void;

macro_rules! api {
    ($($field:ident : $symbol:literal => fn($($arg:ty),*) $(-> $ret:ty)?;)*) => {
        pub struct Sdl {
            _library: libloading::Library,
            $(pub $field: unsafe extern "C" fn($($arg),*) $(-> $ret)?,)*
        }

        impl Sdl {
            fn bind(library: libloading::Library) -> Result<Sdl, String> {
                unsafe {
                    $(let $field = *library
                        .get::<unsafe extern "C" fn($($arg),*) $(-> $ret)?>(concat!($symbol, "\0").as_bytes())
                        .map_err(|e| format!("SDL3 tanpa {}: {e}", $symbol))?;)*
                    Ok(Sdl { _library: library, $($field,)* })
                }
            }
        }
    };
}

api! {
    init: "SDL_Init" => fn(u32) -> bool;
    quit_subsystem: "SDL_QuitSubSystem" => fn(u32);
    set_hint: "SDL_SetHint" => fn(*const c_char, *const c_char) -> bool;
    get_error: "SDL_GetError" => fn() -> *const c_char;
    free: "SDL_free" => fn(Ptr);
    create_window: "SDL_CreateWindow" => fn(*const c_char, i32, i32, u64) -> Ptr;
    destroy_window: "SDL_DestroyWindow" => fn(Ptr);
    set_window_fullscreen: "SDL_SetWindowFullscreen" => fn(Ptr, bool) -> bool;
    get_window_size_in_pixels: "SDL_GetWindowSizeInPixels" => fn(Ptr, *mut i32, *mut i32) -> bool;
    get_window_size: "SDL_GetWindowSize" => fn(Ptr, *mut i32, *mut i32) -> bool;
    raise_window: "SDL_RaiseWindow" => fn(Ptr) -> bool;
    set_window_aspect_ratio: "SDL_SetWindowAspectRatio" => fn(Ptr, f32, f32) -> bool;
    minimize_window: "SDL_MinimizeWindow" => fn(Ptr) -> bool;
    set_relative_mouse_mode: "SDL_SetWindowRelativeMouseMode" => fn(Ptr, bool) -> bool;
    set_keyboard_grab: "SDL_SetWindowKeyboardGrab" => fn(Ptr, bool) -> bool;
    show_cursor: "SDL_ShowCursor" => fn() -> bool;
    hide_cursor: "SDL_HideCursor" => fn() -> bool;
    create_renderer: "SDL_CreateRenderer" => fn(Ptr, *const c_char) -> Ptr;
    destroy_renderer: "SDL_DestroyRenderer" => fn(Ptr);
    get_renderer_name: "SDL_GetRendererName" => fn(Ptr) -> *const c_char;
    set_render_vsync: "SDL_SetRenderVSync" => fn(Ptr, i32) -> bool;
    create_properties: "SDL_CreateProperties" => fn() -> u32;
    destroy_properties: "SDL_DestroyProperties" => fn(u32);
    set_number_property: "SDL_SetNumberProperty" => fn(u32, *const c_char, i64) -> bool;
    create_texture_with_properties: "SDL_CreateTextureWithProperties" => fn(Ptr, u32) -> Ptr;
    destroy_texture: "SDL_DestroyTexture" => fn(Ptr);
    update_nv_texture: "SDL_UpdateNVTexture" => fn(Ptr, *const c_void, *const u8, i32, *const u8, i32) -> bool;
    set_render_draw_color: "SDL_SetRenderDrawColor" => fn(Ptr, u8, u8, u8, u8) -> bool;
    set_render_draw_blend_mode: "SDL_SetRenderDrawBlendMode" => fn(Ptr, u32) -> bool;
    set_render_scale: "SDL_SetRenderScale" => fn(Ptr, f32, f32) -> bool;
    render_clear: "SDL_RenderClear" => fn(Ptr) -> bool;
    render_texture: "SDL_RenderTexture" => fn(Ptr, Ptr, *const FRect, *const FRect) -> bool;
    render_fill_rect: "SDL_RenderFillRect" => fn(Ptr, *const FRect) -> bool;
    render_debug_text: "SDL_RenderDebugText" => fn(Ptr, f32, f32, *const c_char) -> bool;
    render_present: "SDL_RenderPresent" => fn(Ptr) -> bool;
    poll_event: "SDL_PollEvent" => fn(*mut Event) -> bool;
    wait_event_timeout: "SDL_WaitEventTimeout" => fn(*mut Event, i32) -> bool;
    open_audio_device_stream: "SDL_OpenAudioDeviceStream" => fn(u32, *const AudioSpec, Ptr, Ptr) -> Ptr;
    resume_audio_stream_device: "SDL_ResumeAudioStreamDevice" => fn(Ptr) -> bool;
    put_audio_stream_data: "SDL_PutAudioStreamData" => fn(Ptr, *const c_void, i32) -> bool;
    get_audio_stream_queued: "SDL_GetAudioStreamQueued" => fn(Ptr) -> i32;
    clear_audio_stream: "SDL_ClearAudioStream" => fn(Ptr) -> bool;
    destroy_audio_stream: "SDL_DestroyAudioStream" => fn(Ptr);
    get_clipboard_text: "SDL_GetClipboardText" => fn() -> *mut c_char;
}

/// Nama berkas SDL3 per sistem.
fn names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["SDL3.dll"]
    } else if cfg!(target_os = "macos") {
        &["libSDL3.0.dylib", "libSDL3.dylib"]
    } else {
        &["libSDL3.so.0", "libSDL3.so"]
    }
}

/// Tempat SDL3 dicari: di samping program (bundel aplikasi), lalu Frameworks/ macOS, lalu pencarian sistem dan Homebrew.
fn candidates() -> Vec<PathBuf> {
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(PathBuf::from)).unwrap_or_default();
    let mut out = Vec::new();
    for name in names() {
        out.push(exe_dir.join(name));
        out.push(exe_dir.join("../Frameworks").join(name));
        out.push(PathBuf::from(name));
        for prefix in ["/opt/homebrew/lib", "/usr/local/lib"] {
            if cfg!(target_os = "macos") {
                out.push(PathBuf::from(prefix).join(name));
            }
        }
    }
    out
}

/// Pustaka SDL3 mentah (untuk modul yang mengikat fungsinya sendiri, seperti `gamepad.rs`).
pub fn open_library() -> Option<libloading::Library> {
    candidates().into_iter().filter(|p| p.components().count() == 1 || p.exists()).find_map(|p| unsafe { libloading::Library::new(p) }.ok())
}

/// Muat SDL3. `Err` berisi alasan (DLL tidak ada atau versi terlalu lama).
pub fn load() -> Result<Sdl, String> {
    let mut last = "SDL3 tidak ditemukan".to_string();
    for path in candidates() {
        // Nama tanpa folder dicari oleh sistem; nama dengan folder hanya bila berkasnya ada.
        if path.components().count() > 1 && !path.exists() {
            continue;
        }
        match unsafe { libloading::Library::new(&path) } {
            Ok(library) => return Sdl::bind(library),
            Err(e) => last = format!("{}: {e}", path.display()),
        }
    }
    Err(last)
}

impl Sdl {
    pub fn error(&self) -> String {
        unsafe {
            let text = (self.get_error)();
            if text.is_null() {
                String::new()
            } else {
                CStr::from_ptr(text).to_string_lossy().into_owned()
            }
        }
    }

    pub fn clipboard(&self) -> Option<String> {
        unsafe {
            let text = (self.get_clipboard_text)();
            if text.is_null() {
                return None;
            }
            let value = CStr::from_ptr(text).to_string_lossy().into_owned();
            (self.free)(text as Ptr);
            (!value.is_empty()).then_some(value)
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn event_union_is_128_bytes_and_8_aligned() {
        assert_eq!(std::mem::size_of::<super::Event>(), 128);
        assert_eq!(std::mem::align_of::<super::Event>(), 8);
    }
}
