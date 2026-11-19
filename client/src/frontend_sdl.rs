//! Tampilan SDL3 (seperti Moonlight): jendela, gambar, suara, keyboard/mouse/stik, overlay statistik. Wajib di macOS;
//! di Windows bisa dipilih dengan `--frontend sdl` (dipakai untuk menguji jalur ini tanpa Mac).
//!
//! SDL harus memegang thread utama (aturan macOS), jadi `run` menjalankan loop SDL di thread pemanggil dan sesi jaringan
//! (`session::run_with`) di thread lain. Gambar: decode lewat `ff::CpuDecoder` (VideoToolbox di macOS, CPU bila tidak
//! ada) -> tekstur NV12 SDL -> renderer SDL (Metal di macOS, Direct3D di Windows).

use std::ffi::CString;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::gamepad::Pads;
use crate::input::{key_packet, relative_packets, text_packets, vk_from_sdl_scancode, InputPacket, Keys, Shortcut};
use crate::rtsp::Codec;
use crate::sdl::{self, at, event, AudioSpec, Event, FRect, Ptr, Sdl};
use crate::session::{AudioSink, VideoSink};
use crate::video::depacketize::VideoFrame;
use crate::video::ff::{ffmpeg_version, CpuDecoder, CpuPicture};
use crate::video::DisplayReport;
use cloudpc_stream_proto::input as wire;

/// Kapan tombol sistem (Cmd+Tab, Alt+Tab, tombol Windows) diteruskan ke host lewat keyboard grab SDL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemKeys {
    Never,
    Fullscreen,
    Always,
}

#[derive(Debug, Clone)]
pub struct SdlOptions {
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub fullscreen: bool,
    pub input: bool,
    pub absolute_mouse: bool,
    pub system_keys: SystemKeys,
    /// `false` = `--video-decoder software`.
    pub hardware_decode: bool,
    pub stats: bool,
    pub vsync: bool,
    pub audio: bool,
}

enum Message {
    Start(Codec),
    Frame(VideoFrame, Instant),
    Status(String, Option<String>),
    Rumble(u16, u16, u16),
}

/// Sisi sesi: dipegang thread jaringan.
pub struct SdlSink {
    tx: Sender<Message>,
    input: Receiver<InputPacket>,
    closed: Arc<AtomicBool>,
    need_idr: Arc<AtomicBool>,
}

impl VideoSink for SdlSink {
    fn start(&mut self, codec: Codec) {
        let _ = self.tx.send(Message::Start(codec));
    }
    fn frame(&mut self, frame: VideoFrame) {
        let _ = self.tx.send(Message::Frame(frame, Instant::now()));
    }
    fn take_idr_request(&mut self) -> bool {
        self.need_idr.swap(false, Ordering::SeqCst)
    }
    fn closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
    fn take_input(&mut self) -> Vec<InputPacket> {
        self.input.try_iter().collect()
    }
    fn rumble(&mut self, pad: u16, low: u16, high: u16) {
        let _ = self.tx.send(Message::Rumble(pad, low, high));
    }
    fn status(&mut self, network: String, warning: Option<String>) {
        let _ = self.tx.send(Message::Status(network, warning));
    }
    fn hardware_codecs(&self) -> Option<Vec<Codec>> {
        // "auto" memilih HEVC hanya bila mesin ini bisa mendecodenya di hardware (selain itu H.264 di CPU).
        Some(crate::video::ff::hardware_codecs())
    }
}

/// Suara: aliran audio SDL aman dipakai dari thread lain.
pub struct SdlAudio {
    stream: Ptr,
    put: unsafe extern "C" fn(Ptr, *const std::ffi::c_void, i32) -> bool,
    queued: unsafe extern "C" fn(Ptr) -> i32,
    clear: unsafe extern "C" fn(Ptr) -> bool,
    trimmed_ms: Arc<Mutex<u64>>,
}

unsafe impl Send for SdlAudio {}

/// Batas antrean suara sebelum dipangkas (seperti pemutar WASAPI): 60 ms float stereo 48 kHz.
const AUDIO_MAX_BYTES: i32 = 48 * 60 * 2 * 4;

impl AudioSink for SdlAudio {
    fn samples(&mut self, pcm: &[f32]) {
        unsafe {
            if (self.queued)(self.stream) > AUDIO_MAX_BYTES {
                (self.clear)(self.stream);
                if let Ok(mut t) = self.trimmed_ms.lock() {
                    *t += 60;
                }
            }
            (self.put)(self.stream, pcm.as_ptr().cast(), (pcm.len() * 4) as i32);
        }
    }
}

struct Window<'a> {
    sdl: &'a Sdl,
    window: Ptr,
    renderer: Ptr,
    texture: Ptr,
    texture_key: (u32, u32, bool, bool),
    fullscreen: bool,
}

impl Drop for Window<'_> {
    fn drop(&mut self) {
        unsafe {
            if !self.texture.is_null() {
                (self.sdl.destroy_texture)(self.texture);
            }
            (self.sdl.destroy_renderer)(self.renderer);
            (self.sdl.destroy_window)(self.window);
        }
    }
}

/// Kotak gambar yang mempertahankan rasio, di tengah `target`.
pub fn fit(source: (u32, u32), target: (f32, f32)) -> FRect {
    let (sw, sh) = (source.0.max(1) as f32, source.1.max(1) as f32);
    let scale = (target.0 / sw).min(target.1 / sh);
    let (w, h) = (sw * scale, sh * scale);
    FRect { x: (target.0 - w) / 2.0, y: (target.1 - h) / 2.0, w, h }
}

impl Window<'_> {
    fn pixel_size(&self) -> (f32, f32) {
        let (mut w, mut h) = (0, 0);
        unsafe { (self.sdl.get_window_size_in_pixels)(self.window, &mut w, &mut h) };
        (w.max(1) as f32, h.max(1) as f32)
    }

    fn point_size(&self) -> (f32, f32) {
        let (mut w, mut h) = (0, 0);
        unsafe { (self.sdl.get_window_size)(self.window, &mut w, &mut h) };
        (w.max(1) as f32, h.max(1) as f32)
    }

    fn texture_for(&mut self, picture: &CpuPicture) -> Result<Ptr, String> {
        let key = (picture.width, picture.height, picture.bt601, picture.full_range);
        if self.texture.is_null() || self.texture_key != key {
            let s = self.sdl;
            unsafe {
                if !self.texture.is_null() {
                    (s.destroy_texture)(self.texture);
                }
                let colorspace = match (picture.bt601, picture.full_range) {
                    (true, false) => sdl::COLORSPACE_BT601_LIMITED,
                    (true, true) => sdl::COLORSPACE_BT601_FULL,
                    (false, false) => sdl::COLORSPACE_BT709_LIMITED,
                    (false, true) => sdl::COLORSPACE_BT709_FULL,
                };
                let props = (s.create_properties)();
                (s.set_number_property)(props, c"SDL.texture.create.format".as_ptr(), i64::from(sdl::PIXELFORMAT_NV12));
                (s.set_number_property)(props, c"SDL.texture.create.access".as_ptr(), sdl::TEXTUREACCESS_STREAMING);
                (s.set_number_property)(props, c"SDL.texture.create.width".as_ptr(), i64::from(picture.width));
                (s.set_number_property)(props, c"SDL.texture.create.height".as_ptr(), i64::from(picture.height));
                (s.set_number_property)(props, c"SDL.texture.create.colorspace".as_ptr(), colorspace);
                self.texture = (s.create_texture_with_properties)(self.renderer, props);
                (s.destroy_properties)(props);
                if self.texture.is_null() {
                    return Err(format!("tekstur NV12: {}", s.error()));
                }
                self.texture_key = key;
            }
        }
        Ok(self.texture)
    }

    fn present(&mut self, picture: &CpuPicture, stats: Option<&str>, warning: Option<&str>) -> Result<FRect, String> {
        let texture = self.texture_for(picture)?;
        let s = self.sdl;
        let (w, h) = (picture.width as usize, picture.height as usize);
        let uv = &picture.nv12[w * h..];
        let size = self.pixel_size();
        let dest = fit((picture.width, picture.height), size);
        unsafe {
            if !(s.update_nv_texture)(texture, std::ptr::null(), picture.nv12.as_ptr(), w as i32, uv.as_ptr(), (w.div_ceil(2) * 2) as i32) {
                return Err(format!("unggah gambar: {}", s.error()));
            }
            (s.set_render_draw_color)(self.renderer, 0, 0, 0, 255);
            (s.render_clear)(self.renderer);
            (s.render_texture)(self.renderer, texture, std::ptr::null(), &dest);
            self.overlay(stats, warning);
            (s.render_present)(self.renderer);
        }
        Ok(dest)
    }

    /// Teks di atas gambar dengan huruf bawaan SDL (8x8, diperbesar).
    fn overlay(&self, stats: Option<&str>, warning: Option<&str>) {
        let s = self.sdl;
        let scale = (self.pixel_size().1 / 540.0).clamp(1.5, 3.0);
        unsafe {
            (s.set_render_scale)(self.renderer, scale, scale);
            (s.set_render_draw_blend_mode)(self.renderer, 1); // SDL_BLENDMODE_BLEND
            if let Some(text) = stats {
                let lines: Vec<&str> = text.lines().collect();
                let width = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as f32 * 8.0 + 12.0;
                (s.set_render_draw_color)(self.renderer, 0, 0, 0, 160);
                (s.render_fill_rect)(self.renderer, &FRect { x: 4.0, y: 4.0, w: width, h: lines.len() as f32 * 11.0 + 8.0 });
                (s.set_render_draw_color)(self.renderer, 255, 255, 255, 255);
                for (i, line) in lines.iter().enumerate() {
                    if let Ok(c) = CString::new(ascii(line)) {
                        (s.render_debug_text)(self.renderer, 10.0, 8.0 + i as f32 * 11.0, c.as_ptr());
                    }
                }
            }
            if let Some(text) = warning {
                let width = text.chars().count() as f32 * 8.0 + 16.0;
                let x = (self.pixel_size().0 / scale - width) / 2.0;
                (s.set_render_draw_color)(self.renderer, 0, 0, 0, 180);
                (s.render_fill_rect)(self.renderer, &FRect { x, y: 6.0, w: width, h: 18.0 });
                (s.set_render_draw_color)(self.renderer, 255, 217, 51, 255);
                if let Ok(c) = CString::new(ascii(text)) {
                    (s.render_debug_text)(self.renderer, x + 8.0, 11.0, c.as_ptr());
                }
            }
            (s.set_render_scale)(self.renderer, 1.0, 1.0);
        }
    }

    fn set_fullscreen(&mut self, on: bool) {
        unsafe { (self.sdl.set_window_fullscreen)(self.window, on) };
        self.fullscreen = on;
    }
}

/// Huruf bawaan SDL hanya ASCII.
fn ascii(text: &str) -> String {
    text.chars().map(|c| if c.is_ascii() { c } else { '?' }).collect()
}

/// Keadaan input jendela SDL.
struct Input {
    enabled: bool,
    captured: bool,
    absolute: bool,
    system_keys: SystemKeys,
    system_forced: Option<bool>,
    keys: Keys,
    dx: f32,
    dy: f32,
    wheel: f32,
    ever_captured: bool,
    tx: Sender<InputPacket>,
}

impl Input {
    fn send(&self, packets: impl IntoIterator<Item = InputPacket>) {
        for p in packets {
            let _ = self.tx.send(p);
        }
    }

    fn system_captured(&self, fullscreen: bool) -> bool {
        self.system_forced.unwrap_or(match self.system_keys {
            SystemKeys::Never => false,
            SystemKeys::Fullscreen => fullscreen,
            SystemKeys::Always => true,
        })
    }

    fn apply(&mut self, window: &Window, on: bool) {
        let s = window.sdl;
        if !on && self.captured {
            let ups = self.keys.release_all();
            self.send(ups);
        }
        self.captured = on && self.enabled;
        self.ever_captured |= self.captured;
        unsafe {
            (s.set_relative_mouse_mode)(window.window, self.captured && !self.absolute);
            (s.set_keyboard_grab)(window.window, self.captured && self.system_captured(window.fullscreen));
            if self.captured {
                (s.hide_cursor)();
            } else {
                (s.show_cursor)();
            }
        }
    }
}

/// Jalankan tampilan SDL di thread ini dan `session` di thread lain sampai sesi selesai.
pub fn run<R: Send + 'static>(options: SdlOptions, session: impl FnOnce(SdlSink, Option<SdlAudio>) -> R + Send + 'static) -> Result<(R, DisplayReport), String> {
    let s = sdl::load()?;
    unsafe {
        (s.set_hint)(c"SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS".as_ptr(), c"1".as_ptr());
        if !(s.init)(sdl::INIT_VIDEO | sdl::INIT_AUDIO) {
            return Err(format!("SDL gagal dimulai: {}", s.error()));
        }
    }
    let title = CString::new(options.title.clone()).unwrap_or_default();
    let flags = sdl::WINDOW_RESIZABLE | sdl::WINDOW_HIGH_PIXEL_DENSITY | if options.fullscreen { sdl::WINDOW_FULLSCREEN } else { 0 };
    let raw = unsafe { (s.create_window)(title.as_ptr(), options.width as i32, options.height as i32, flags) };
    if raw.is_null() {
        return Err(format!("jendela SDL: {}", s.error()));
    }
    let renderer = unsafe { (s.create_renderer)(raw, std::ptr::null()) };
    if renderer.is_null() {
        unsafe { (s.destroy_window)(raw) };
        return Err(format!("renderer SDL: {}", s.error()));
    }
    let mut window = Window { sdl: &s, window: raw, renderer, texture: std::ptr::null_mut(), texture_key: (0, 0, false, false), fullscreen: options.fullscreen };
    unsafe {
        (s.set_render_vsync)(renderer, i32::from(options.vsync));
        // Ubah ukuran/maksimalkan tetap berasio stream: tanpa bilah hitam di mode jendela.
        let ratio = options.width as f32 / options.height.max(1) as f32;
        (s.set_window_aspect_ratio)(raw, ratio, ratio);
        (s.raise_window)(raw);
    }
    let renderer_name = unsafe {
        let name = (s.get_renderer_name)(renderer);
        if name.is_null() { "?".into() } else { std::ffi::CStr::from_ptr(name).to_string_lossy().into_owned() }
    };

    let trimmed = Arc::new(Mutex::new(0u64));
    let audio = if options.audio {
        let spec = AudioSpec { format: sdl::AUDIO_F32LE, channels: 2, freq: 48_000 };
        let stream = unsafe { (s.open_audio_device_stream)(sdl::AUDIO_DEVICE_DEFAULT_PLAYBACK, &spec, std::ptr::null_mut(), std::ptr::null_mut()) };
        if stream.is_null() {
            None
        } else {
            unsafe { (s.resume_audio_stream_device)(stream) };
            Some(SdlAudio { stream, put: s.put_audio_stream_data, queued: s.get_audio_stream_queued, clear: s.clear_audio_stream, trimmed_ms: trimmed.clone() })
        }
    } else {
        None
    };
    let audio_stream = audio.as_ref().map(|a| a.stream);

    let (tx, rx) = mpsc::channel();
    let (input_tx, input_rx) = mpsc::channel();
    let closed = Arc::new(AtomicBool::new(false));
    let need_idr = Arc::new(AtomicBool::new(false));
    let sink = SdlSink { tx, input: input_rx, closed: closed.clone(), need_idr: need_idr.clone() };
    let worker = std::thread::Builder::new().name("sesi".into()).spawn(move || session(sink, audio)).map_err(|e| format!("thread sesi: {e}"))?;

    let mut report = DisplayReport { ffmpeg: ffmpeg_version(), gamepad_backend: "SDL3".into(), ..Default::default() };
    let mut input = Input {
        enabled: options.input,
        captured: false,
        absolute: options.absolute_mouse,
        system_keys: options.system_keys,
        system_forced: None,
        keys: Keys::default(),
        dx: 0.0,
        dy: 0.0,
        wheel: 0.0,
        ever_captured: false,
        tx: input_tx,
    };
    let mut pads = if options.input { Pads::open() } else { None };
    if pads.is_none() {
        report.gamepad_backend = "-".into();
    }
    let mut decoder: Option<CpuDecoder> = None;
    let (mut show_stats, mut network, mut warning) = (options.stats, String::new(), None::<String>);
    let mut picture_rect = FRect::default();
    let (mut decode_total, mut latency_total, mut render_total) = (Duration::ZERO, Duration::ZERO, Duration::ZERO);
    let mut local_line = String::new();
    let (mut second_start, mut second_presented) = (Instant::now(), 0u64);
    let mut event = Event([0; 128]);

    while !worker.is_finished() {
        // Kejadian jendela dan input.
        while unsafe { (s.poll_event)(&mut event) } {
            let kind = event.kind();
            match kind {
                event::QUIT | event::WINDOW_CLOSE_REQUESTED => closed.store(true, Ordering::SeqCst),
                // Hanya fokus pertama yang menangkap otomatis; sesudahnya klik dulu (seperti Moonlight).
                event::WINDOW_FOCUS_GAINED if !input.ever_captured => input.apply(&window, true),
                event::WINDOW_FOCUS_LOST => input.apply(&window, false),
                event::KEY_DOWN | event::KEY_UP if input.captured => {
                    let down = kind == event::KEY_DOWN;
                    if event.u8(at::KEY_REPEAT) != 0 {
                        continue; // host mengulang sendiri
                    }
                    let Some(vk) = vk_from_sdl_scancode(event.u32(at::KEY_SCANCODE)) else { continue };
                    if down {
                        if let Some(shortcut) = input.keys.shortcut(vk) {
                            let ups = input.keys.release_all();
                            input.send(ups);
                            match shortcut {
                                Shortcut::Quit => closed.store(true, Ordering::SeqCst),
                                Shortcut::ToggleFullscreen => {
                                    let on = !window.fullscreen;
                                    window.set_fullscreen(on);
                                    input.apply(&window, true);
                                }
                                Shortcut::ToggleCapture => {
                                    let on = !input.captured;
                                    input.apply(&window, on);
                                }
                                Shortcut::ToggleMouseMode => {
                                    input.absolute = !input.absolute;
                                    input.apply(&window, true);
                                }
                                Shortcut::Minimize => {
                                    input.apply(&window, false);
                                    unsafe { (s.minimize_window)(window.window) };
                                }
                                Shortcut::Paste => {
                                    if let Some(text) = s.clipboard() {
                                        input.send(text_packets(&text));
                                    }
                                }
                                Shortcut::ToggleSystemKeys => {
                                    input.system_forced = Some(!input.system_captured(window.fullscreen));
                                    input.apply(&window, true);
                                }
                                Shortcut::ToggleStats => show_stats = !show_stats,
                            }
                            continue;
                        }
                    }
                    if input.keys.update(vk, down) {
                        let modifiers = input.keys.modifiers();
                        input.send([key_packet(vk, down, modifiers)]);
                    }
                }
                event::MOUSE_MOTION if input.captured => {
                    if input.absolute {
                        // Koordinat SDL dalam titik; kotak gambar dalam piksel.
                        let ratio = window.pixel_size().0 / window.point_size().0;
                        let (x, y) = (event.f32(at::MOTION_X) * ratio - picture_rect.x, event.f32(at::MOTION_Y) * ratio - picture_rect.y);
                        if picture_rect.w > 0.0 && picture_rect.h > 0.0 {
                            let (w, h) = (picture_rect.w as i16, picture_rect.h as i16);
                            let (x, y) = ((x as i16).clamp(0, w - 1), (y as i16).clamp(0, h - 1));
                            input.send([InputPacket { channel: crate::input::channel::MOUSE, data: wire::mouse_move_abs(x, y, (w, h)) }]);
                        }
                    } else {
                        input.dx += event.f32(at::MOTION_XREL);
                        input.dy += event.f32(at::MOTION_YREL);
                    }
                }
                event::MOUSE_BUTTON_DOWN | event::MOUSE_BUTTON_UP => {
                    if !input.captured {
                        if kind == event::MOUSE_BUTTON_DOWN && input.enabled {
                            input.apply(&window, true); // klik pertama = tangkap, tidak diteruskan
                        }
                        continue;
                    }
                    flush_motion(&mut input);
                    let button = event.u8(at::BUTTON);
                    if (1..=5).contains(&button) {
                        input.send([InputPacket { channel: crate::input::channel::MOUSE, data: wire::mouse_button(kind == event::MOUSE_BUTTON_DOWN, button) }]);
                    }
                }
                event::MOUSE_WHEEL if input.captured => {
                    let flip = if event.u32(at::WHEEL_DIRECTION) == 1 { -1.0 } else { 1.0 };
                    input.wheel += event.f32(at::WHEEL_Y) * flip * 120.0;
                    let whole = input.wheel.trunc();
                    if whole != 0.0 {
                        input.wheel -= whole;
                        input.send([InputPacket { channel: crate::input::channel::MOUSE, data: wire::scroll(whole as i16) }]);
                    }
                    let horizontal = event.f32(at::WHEEL_X) * flip * 120.0;
                    if horizontal.abs() >= 1.0 {
                        input.send([InputPacket { channel: crate::input::channel::MOUSE, data: wire::horizontal_scroll(horizontal as i16) }]);
                    }
                }
                _ => {}
            }
        }
        flush_motion(&mut input);
        if let Some(pads) = pads.as_mut() {
            let packets = pads.poll();
            report.gamepads.append(&mut pads.arrived);
            if input.captured {
                input.send(packets);
            }
        }

        // Frame dan pesan dari sesi.
        let first = match rx.recv_timeout(Duration::from_millis(1)) {
            Ok(m) => m,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        let mut latest: Option<(CpuPicture, Instant)> = None;
        for message in std::iter::once(first).chain(rx.try_iter()) {
            match message {
                Message::Start(codec) => match CpuDecoder::new(codec, options.hardware_decode) {
                    Ok(d) => {
                        report.decoder = if d.hardware { format!("{} (hardware)", d.name) } else { format!("{} (perangkat lunak)", d.name) };
                        decoder = Some(d);
                    }
                    Err(e) => {
                        report.error = Some(e);
                        closed.store(true, Ordering::SeqCst);
                    }
                },
                Message::Status(text, alert) => {
                    network = text;
                    warning = alert;
                }
                Message::Rumble(pad, low, high) => {
                    if let Some(pads) = pads.as_ref() {
                        pads.rumble(pad, low, high);
                    }
                }
                Message::Frame(frame, received) => {
                    let Some(d) = decoder.as_mut() else { continue };
                    let started = Instant::now();
                    match d.decode(&frame.data) {
                        Ok(Some(picture)) => {
                            decode_total += started.elapsed();
                            report.decoded += 1;
                            if latest.is_some() {
                                report.skipped += 1;
                            }
                            latest = Some((picture, received));
                        }
                        Ok(None) => {}
                        Err(_) => {
                            report.decode_errors += 1;
                            need_idr.store(true, Ordering::SeqCst);
                        }
                    }
                }
            }
        }
        if let Some((picture, received)) = latest {
            let stats = show_stats.then(|| format!("{network}\n{local_line}\nRenderer SDL: {renderer_name}\nCtrl+Alt+Shift+S: tutup statistik"));
            let started = Instant::now();
            match window.present(&picture, stats.as_deref(), warning.as_deref()) {
                Ok(rect) => picture_rect = rect,
                Err(e) => {
                    report.error = Some(e);
                    closed.store(true, Ordering::SeqCst);
                }
            }
            render_total += started.elapsed();
            latency_total += received.elapsed();
            report.presented += 1;
            second_presented += 1;
            let n = report.presented.max(1) as f64;
            report.decode_ms = decode_total.as_secs_f64() * 1000.0 / report.decoded.max(1) as f64;
            report.render_ms = render_total.as_secs_f64() * 1000.0 / n;
            report.receive_to_present_ms = latency_total.as_secs_f64() * 1000.0 / n;
            if second_start.elapsed() >= Duration::from_secs(1) {
                local_line = format!("Tampil: {:.0} FPS, decode {:.2} ms, render {:.2} ms", second_presented as f64 / second_start.elapsed().as_secs_f64(), report.decode_ms, report.render_ms);
                (second_start, second_presented) = (Instant::now(), 0);
            }
        }
    }
    // Sesi selesai: lepas input, hentikan getar, tutup suara.
    input.apply(&window, false);
    drop(pads);
    if let Some(stream) = audio_stream {
        unsafe { (s.destroy_audio_stream)(stream) };
    }
    let result = worker.join().map_err(|_| "thread sesi panik".to_string())?;
    drop(window);
    unsafe { (s.quit_subsystem)(sdl::INIT_VIDEO | sdl::INIT_AUDIO) };
    let _ = trimmed;
    Ok((result, report))
}

fn flush_motion(input: &mut Input) {
    let (dx, dy) = (input.dx.trunc(), input.dy.trunc());
    if dx != 0.0 || dy != 0.0 {
        input.dx -= dx;
        input.dy -= dy;
        let packets = relative_packets(dx as i32, dy as i32);
        input.send(packets);
    }
}

/// Uji mandiri tanpa host (`cloudpc-stream-client self-test`): buka jendela SDL, tampilkan pola uji NV12 bergerak dengan
/// overlay, putar nada 440 Hz, selama `seconds`. Mengembalikan nama renderer dan jumlah frame yang tampil.
pub fn self_test(seconds: u64) -> Result<(String, u64), String> {
    let s = sdl::load()?;
    unsafe {
        if !(s.init)(sdl::INIT_VIDEO | sdl::INIT_AUDIO) {
            return Err(format!("SDL gagal dimulai: {}", s.error()));
        }
    }
    let raw = unsafe { (s.create_window)(c"CloudPC".as_ptr(), 960, 540, sdl::WINDOW_RESIZABLE | sdl::WINDOW_HIGH_PIXEL_DENSITY) };
    if raw.is_null() {
        return Err(format!("jendela SDL: {}", s.error()));
    }
    let renderer = unsafe { (s.create_renderer)(raw, std::ptr::null()) };
    if renderer.is_null() {
        return Err(format!("renderer SDL: {}", s.error()));
    }
    let name = unsafe { std::ffi::CStr::from_ptr((s.get_renderer_name)(renderer)).to_string_lossy().into_owned() };
    let mut window = Window { sdl: &s, window: raw, renderer, texture: std::ptr::null_mut(), texture_key: (0, 0, false, false), fullscreen: false };
    let spec = AudioSpec { format: sdl::AUDIO_F32LE, channels: 2, freq: 48_000 };
    let stream = unsafe { (s.open_audio_device_stream)(sdl::AUDIO_DEVICE_DEFAULT_PLAYBACK, &spec, std::ptr::null_mut(), std::ptr::null_mut()) };
    if !stream.is_null() {
        let tone: Vec<f32> = (0..48_000 * seconds as usize).flat_map(|i| {
            let v = (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.1;
            [v, v]
        }).collect();
        unsafe {
            (s.put_audio_stream_data)(stream, tone.as_ptr().cast(), (tone.len() * 4) as i32);
            (s.resume_audio_stream_device)(stream);
        }
    }
    let (w, h) = (640u32, 360u32);
    let started = Instant::now();
    let mut presented = 0u64;
    let mut event = Event([0; 128]);
    while started.elapsed() < Duration::from_secs(seconds) {
        while unsafe { (s.poll_event)(&mut event) } {}
        // Pola uji: gradasi luma bergeser + chroma kotak-kotak.
        let shift = (started.elapsed().as_millis() / 8) as usize;
        let mut nv12 = vec![0u8; (w * h + w * h / 2) as usize];
        for y in 0..h as usize {
            for x in 0..w as usize {
                nv12[y * w as usize + x] = (16 + ((x + shift) % w as usize) * 219 / w as usize) as u8;
            }
        }
        for (i, v) in nv12[(w * h) as usize..].iter_mut().enumerate() {
            let (row, col) = (i / w as usize, (i % w as usize) / 2);
            *v = if (row / 20 + col / 20) % 2 == 0 { 90 } else { 170 };
        }
        let picture = CpuPicture { width: w, height: h, nv12, bt601: false, full_range: false };
        window.present(&picture, Some("Uji mandiri CloudPC\nGambar + suara 440 Hz\nRenderer SDL"), Some("Ini hanya uji"))?;
        presented += 1;
        std::thread::sleep(Duration::from_millis(16));
    }
    if !stream.is_null() {
        unsafe { (s.destroy_audio_stream)(stream) };
    }
    drop(window);
    unsafe { (s.quit_subsystem)(sdl::INIT_VIDEO | sdl::INIT_AUDIO) };
    Ok((name, presented))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picture_fits_with_bars() {
        let round = |r: FRect| (r.x.round(), r.y.round(), r.w.round(), r.h.round());
        assert_eq!(round(fit((1920, 1080), (1000.0, 1000.0))), (0.0, 219.0, 1000.0, 563.0), "letterbox atas-bawah");
        assert_eq!(round(fit((1280, 720), (2560.0, 1440.0))), (0.0, 0.0, 2560.0, 1440.0));
        assert_eq!(round(fit((1920, 1080), (2560.0, 1080.0))), (320.0, 0.0, 1920.0, 1080.0), "pillarbox kiri-kanan");
    }
}
