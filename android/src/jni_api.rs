//! Fungsi yang dipanggil kelas Kotlin `id.cloudpc.stream.Native` (lihat `android/kotlin/id/cloudpc/stream/Native.kt`).
//!
//! Satu stream = satu `Session`: thread sesi (jaringan, decode, suara) + keadaan bersama. `start` mengembalikan pegangan
//! (pointer sebagai `long`); semua fungsi lain menerimanya; `stop` mengakhiri dan membebaskannya. Sisi Kotlin menjamin
//! tidak ada panggilan dengan pegangan yang sudah di-`stop` (semua lewat `StreamActivity` yang menyerialkannya).

use std::cell::RefCell;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use cloudpc_stream_client::input::{channel, relative_packets, text_packets, InputPacket};
use cloudpc_stream_client::nvhttp::{self, HostAddr};
use cloudpc_stream_client::rtsp::Codec;
use cloudpc_stream_client::session::{self, AudioSink, CodecChoice, Event, Outputs, StreamOptions};
use cloudpc_stream_client::store::Store;
use cloudpc_stream_proto::input as wire;
use jni::objects::{JClass, JObject, JString};
use jni::sys::{jboolean, jfloat, jint, jlong, jstring, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;
use ndk::native_window::NativeWindow;

use crate::audio::AndroidAudio;
use crate::gesture::{Action, Touch, TouchKind};
use crate::overlay::{EditAction, Event as PadEvent, Mode as PadMode, Profile, Route};
use crate::pad::{Axes, PadState, VIRTUAL_DEVICE};
use crate::report::{event_lines, global_stats, Log};
use crate::state::Shared;
use crate::video::AndroidVideo;

struct Session {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

struct Params {
    dir: String,
    host: String,
    app: String,
    size: (u32, u32),
    fps: u32,
    bitrate_kbps: u32,
    codec: CodecChoice,
    hardware: Vec<Codec>,
    log_path: String,
}

fn string(env: &mut JNIEnv, value: &JString) -> String {
    env.get_string(value).map(String::from).unwrap_or_default()
}

fn to_java(env: &mut JNIEnv, text: &str) -> jstring {
    env.new_string(text).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
}

fn flag(value: bool) -> jboolean {
    if value {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

/// Panik tidak boleh menembus batas JNI (itu mematikan seluruh aplikasi tanpa pesan).
fn guarded<T>(fallback: T, body: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(body)).unwrap_or(fallback)
}

fn panic_text(payload: Box<dyn std::any::Any + Send>) -> String {
    payload.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| payload.downcast_ref::<String>().cloned()).unwrap_or_else(|| "galat tak dikenal".into())
}

/// Pegangan -> sesi. Pegangan 0 (belum mulai/sudah berhenti) = tidak ada.
fn session<'a>(handle: jlong) -> Option<&'a Session> {
    // SAFETY: pegangan bukan nol hanya berasal dari `start` (Box::into_raw) dan belum dibebaskan `stop`; lihat catatan modul.
    unsafe { (handle as *const Session).as_ref() }
}

fn parse_codec(text: &str) -> CodecChoice {
    match text {
        "H.264" => CodecChoice::Only(Codec::H264),
        "HEVC" => CodecChoice::Only(Codec::Hevc),
        "AV1" => CodecChoice::Only(Codec::Av1),
        _ => CodecChoice::Auto,
    }
}

/// Bit 1 = H.264, 2 = HEVC, 4 = AV1 (decoder hardware yang ada di HP, dari `MediaCodecList`).
fn hardware_codecs(mask: jint) -> Vec<Codec> {
    [(1, Codec::H264), (2, Codec::Hevc), (4, Codec::Av1)].into_iter().filter(|(bit, _)| mask & bit != 0).map(|(_, c)| c).collect()
}

fn pair(dir: &str, host: &str, pin: &str) -> Result<(), String> {
    let mut store = Store::open(Path::new(dir))?;
    let addr = HostAddr::parse(host)?;
    nvhttp::pair(&addr, &mut store, pin).map(|_| ())
}

/// Pairing dengan PIN (memblokir sampai selesai; panggil dari thread latar). Teks kosong = berhasil, selain itu pesan galat.
#[no_mangle]
pub extern "system" fn Java_id_cloudpc_stream_Native_pair<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>, dir: JString<'l>, host: JString<'l>, pin: JString<'l>) -> jstring {
    let (dir, host, pin) = (string(&mut env, &dir), string(&mut env, &host), string(&mut env, &pin));
    let message = match catch_unwind(|| pair(&dir, &host, &pin)) {
        Ok(Ok(())) => String::new(),
        Ok(Err(e)) => e,
        Err(payload) => format!("Pairing gagal: {}", panic_text(payload)),
    };
    to_java(&mut env, &message)
}

fn stream(shared: &Arc<Shared>, window: NativeWindow, p: &Params, log: &mut Log) -> Result<(), String> {
    let store = Store::open(Path::new(&p.dir))?;
    let addr = HostAddr::parse(&p.host)?;
    let options = StreamOptions {
        app: p.app.clone(),
        width: p.size.0,
        height: p.size.1,
        fps: p.fps,
        bitrate_kbps: p.bitrate_kbps,
        codec: p.codec,
        dump_video: None,
        launch: nvhttp::LaunchExtras::default(),
    };
    let mut video = AndroidVideo::new(Arc::clone(shared), window, p.size, p.fps, p.hardware.clone());
    let mut audio = match AndroidAudio::open() {
        Ok(audio) => Some(audio),
        Err(e) => {
            log.line(&format!("Suara tidak tersedia: {e}"));
            None
        }
    };
    let report = {
        let log = RefCell::new(&mut *log);
        let mut events = |event: Event| {
            for line in event_lines(&event, "MediaCodec") {
                log.borrow_mut().line(&line);
            }
        };
        let outputs = Outputs {
            video: Some(&mut video),
            audio: audio.as_mut().map(|a| a as &mut dyn AudioSink),
            events: Some(&mut events),
            mic: None,
        };
        session::run_with(&addr, &store, &options, outputs, || shared.stopping(), |text| {
            log.borrow_mut().line(&text);
            Shared::set(&shared.progress, text);
        })?
    };
    let shown = video.report();
    log.line(&format!(
        "Ringkasan: decoder {}, {} frame didecode, decode {:.2} ms, dibuang {}, galat decode {}",
        if shown.decoder.is_empty() { "-" } else { &shown.decoder },
        shown.decoded,
        shown.decode_ms,
        shown.skipped,
        shown.decode_errors
    ));
    if let Some(code) = report.termination {
        log.line(&format!("Server notified termination reason: {code:#x}"));
    }
    log.raw(&global_stats(&report, Some(&shown)));
    if let Some(error) = video.error() {
        return Err(format!("Decoder video gagal: {error}"));
    }
    if report.disconnected {
        return Err("Koneksi ke PC terputus.".into());
    }
    if report.termination.is_some() && !shared.stopping() {
        return Err("PC mengakhiri stream.".into());
    }
    Ok(())
}

/// Mulai stream ke `surface`. Mengembalikan pegangan (0 = gagal sebelum mulai; pesannya tidak ada, cek log).
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_id_cloudpc_stream_Native_start<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    dir: JString<'l>,
    host: JString<'l>,
    app: JString<'l>,
    width: jint,
    height: jint,
    fps: jint,
    bitrate_kbps: jint,
    codec: JString<'l>,
    hardware: jint,
    touch_slop: jfloat,
    pointer_speed: jfloat,
    log_path: JString<'l>,
    surface: JObject<'l>,
) -> jlong {
    let params = Params {
        dir: string(&mut env, &dir),
        host: string(&mut env, &host),
        app: string(&mut env, &app),
        size: (width.max(1) as u32, height.max(1) as u32),
        fps: fps.max(1) as u32,
        bitrate_kbps: bitrate_kbps.max(500) as u32,
        codec: parse_codec(&string(&mut env, &codec)),
        hardware: hardware_codecs(hardware),
        log_path: string(&mut env, &log_path),
    };
    // SAFETY: `env` dan `surface` berasal dari panggilan JNI ini; `surface` adalah android.view.Surface yang masih hidup
    // (Kotlin memanggil dari surfaceCreated). ANativeWindow menambah hitungan rujukannya sendiri.
    let Some(window) = (unsafe { NativeWindow::from_surface(env.get_raw().cast(), surface.as_raw().cast()) }) else { return 0 };
    let shared = Arc::new(Shared::new(touch_slop, pointer_speed, Path::new(&params.dir).join("gamepad-layout.json")));
    if let Ok(mut overlay) = shared.overlay.lock() {
        overlay.set_look_speed(pointer_speed);
    }
    let for_thread = Arc::clone(&shared);
    let thread = std::thread::Builder::new().name("cloudpc-stream".into()).spawn(move || {
        let mut log = Log::create(Path::new(&params.log_path));
        let outcome = catch_unwind(AssertUnwindSafe(|| stream(&for_thread, window, &params, &mut log)));
        let error = match outcome {
            Ok(Ok(())) => None,
            Ok(Err(e)) => Some(e),
            Err(payload) => Some(format!("PANIK: {}", panic_text(payload))),
        };
        if let Some(error) = error {
            log.line(&format!("GAGAL: {error}"));
            Shared::set(&for_thread.error, error);
        }
        for_thread.ended.store(true, Ordering::Relaxed);
    });
    match thread {
        Ok(thread) => Box::into_raw(Box::new(Session { shared, thread: Some(thread) })) as jlong,
        Err(_) => 0,
    }
}

/// Akhiri stream dan bebaskan pegangan (memblokir sampai thread sesi selesai; panggil dari thread latar).
#[no_mangle]
pub extern "system" fn Java_id_cloudpc_stream_Native_stop<'l>(_env: JNIEnv<'l>, _class: JClass<'l>, handle: jlong) {
    if handle == 0 {
        return;
    }
    // SAFETY: lihat `session`; setelah ini pegangan tidak dipakai lagi oleh Kotlin.
    let mut owned = unsafe { Box::from_raw(handle as *mut Session) };
    guarded((), || {
        // Layar ditutup di tengah mode atur: perubahan tata letak stik layar tetap disimpan.
        if owned.shared.overlay.lock().is_ok_and(|o| o.editing()) {
            owned.shared.save_overlay();
        }
        // Tombol yang masih ditahan dilepas dulu di PC, lalu beri kanal kontrol waktu mengirimnya.
        let released = owned.shared.keyboard.lock().map(|mut k| k.release_all()).unwrap_or_default();
        if !released.is_empty() && !owned.shared.ended.load(Ordering::Relaxed) {
            owned.shared.push(released);
            std::thread::sleep(Duration::from_millis(60));
        }
        owned.shared.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = owned.thread.take() {
            let _ = thread.join();
        }
    });
}

#[no_mangle]
pub extern "system" fn Java_id_cloudpc_stream_Native_ended<'l>(_env: JNIEnv<'l>, _class: JClass<'l>, handle: jlong) -> jboolean {
    flag(session(handle).is_none_or(|s| s.shared.ended.load(Ordering::Relaxed)))
}

/// Pesan galat stream yang sudah berakhir (kosong = berakhir normal).
#[no_mangle]
pub extern "system" fn Java_id_cloudpc_stream_Native_error<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>, handle: jlong) -> jstring {
    let text = session(handle).map(|s| Shared::get(&s.shared.error)).unwrap_or_default();
    to_java(&mut env, &text)
}

/// Teks lapisan statistik (jaringan + decode); sebelum gambar pertama berisi kalimat kemajuan.
#[no_mangle]
pub extern "system" fn Java_id_cloudpc_stream_Native_status<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>, handle: jlong) -> jstring {
    let text = session(handle)
        .map(|s| {
            let status = Shared::get(&s.shared.status);
            if status.is_empty() {
                Shared::get(&s.shared.progress)
            } else {
                status
            }
        })
        .unwrap_or_default();
    to_java(&mut env, &text)
}

/// Kirim ke PC apa yang dihasilkan stik layar: kejadian keyboard/mouse (profil Minecraft) dan, untuk profil Gamepad,
/// keadaan stik Xbox terbaru.
fn flush_overlay(shared: &Shared) {
    let (events, pad) = match shared.overlay.lock() {
        Ok(mut overlay) => (overlay.take_events(), overlay.uses_gamepad().then(|| overlay.state())),
        Err(_) => return,
    };
    for event in events {
        match event {
            PadEvent::Key { vk, down } => {
                let packets = shared.keyboard.lock().map(|mut k| k.vk(vk, down)).unwrap_or_default();
                shared.push(packets);
            }
            PadEvent::Mouse { button, down } => shared.push([InputPacket { channel: channel::MOUSE, data: wire::mouse_button(down, button) }]),
            PadEvent::Look { dx, dy } => shared.push(relative_packets(dx, dy)),
            PadEvent::Wheel(amount) => shared.push([InputPacket { channel: channel::MOUSE, data: wire::scroll(amount) }]),
            PadEvent::Absolute { x, y, width, height } => {
                shared.push([InputPacket { channel: channel::MOUSE, data: wire::mouse_move_abs(x, y, (width, height)) }])
            }
        }
    }
    if let Some(state) = pad {
        let packet = shared.pads.lock().ok().and_then(|mut p| p.set_state(VIRTUAL_DEVICE, state));
        shared.push(packet);
    }
}

/// Cabut stik Xbox virtual dari PC (stik layar disembunyikan atau pindah ke profil keyboard).
fn unplug_overlay_pad(shared: &Shared) {
    let packet = shared.pads.lock().ok().and_then(|mut p| p.disconnect(VIRTUAL_DEVICE));
    shared.push(packet);
}

/// Baris ringkas untuk pil statistik: "60 FPS · 24 ms · 6 Mbps". Kosong sebelum gambar mengalir.
#[no_mangle]
pub extern "system" fn Java_id_cloudpc_stream_Native_brief<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>, handle: jlong) -> jstring {
    let text = session(handle).map(|s| Shared::get(&s.shared.brief)).unwrap_or_default();
    to_java(&mut env, &text)
}

/// Sentuhan layar. Yang mengenai stik layar menjadi input gamepad; sisanya trackpad. `action`: 0 turun, 1 gerak, 2 naik,
/// 3 batal. Hasil: 0 = tidak ada, 1 = buka/tutup keyboard, 2 = stik layar berubah (gambar ulang).
#[no_mangle]
pub extern "system" fn Java_id_cloudpc_stream_Native_touch<'l>(_env: JNIEnv<'l>, _class: JClass<'l>, handle: jlong, action: jint, id: jint, x: jfloat, y: jfloat, time_ms: jlong) -> jint {
    let Some(session) = session(handle) else { return 0 };
    guarded(0, || {
        let kind = match action {
            0 => TouchKind::Down,
            1 => TouchKind::Move,
            2 => TouchKind::Up,
            _ => TouchKind::Cancel,
        };
        let touch = Touch { kind, id, x, y, time_ms: time_ms.max(0) as u64 };
        match session.shared.overlay.lock().map(|mut o| o.touch(touch)).unwrap_or(Route::Pass) {
            Route::Pad => {
                flush_overlay(&session.shared);
                return 2;
            }
            Route::Edit => return 2,
            Route::Pass => {}
        }
        let Ok(mut trackpad) = session.shared.trackpad.lock() else { return 0 };
        let outcome = trackpad.touch(touch);
        drop(trackpad);
        session.shared.push(outcome.packets);
        match outcome.action {
            Some(Action::ToggleKeyboard) => 1,
            None => 0,
        }
    })
}

/// Tombol keyboard (fisik atau tombol khusus keyboard layar). False = tidak dikenal, biarkan Android menanganinya.
#[no_mangle]
pub extern "system" fn Java_id_cloudpc_stream_Native_key<'l>(_env: JNIEnv<'l>, _class: JClass<'l>, handle: jlong, key_code: jint, down: jboolean) -> jboolean {
    let Some(session) = session(handle) else { return JNI_FALSE };
    guarded(JNI_FALSE, || {
        let packets = session.shared.keyboard.lock().ok().and_then(|mut k| k.key(key_code, down != JNI_FALSE));
        match packets {
            Some(packets) => {
                session.shared.push(packets);
                JNI_TRUE
            }
            None => JNI_FALSE,
        }
    })
}

/// Teks dari keyboard layar (huruf, emoji, hasil prediksi).
#[no_mangle]
pub extern "system" fn Java_id_cloudpc_stream_Native_text<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>, handle: jlong, text: JString<'l>) {
    let text = string(&mut env, &text);
    if let Some(session) = session(handle) {
        session.shared.push(text_packets(&text));
    }
}

/// Tombol gamepad. False = bukan tombol gamepad.
#[no_mangle]
pub extern "system" fn Java_id_cloudpc_stream_Native_padButton<'l>(_env: JNIEnv<'l>, _class: JClass<'l>, handle: jlong, device: jint, key_code: jint, down: jboolean) -> jboolean {
    let Some(session) = session(handle) else { return JNI_FALSE };
    guarded(JNI_FALSE, || {
        let packet = session.shared.pads.lock().ok().and_then(|mut p| p.button(device, key_code, down != JNI_FALSE));
        let handled = packet.is_some();
        session.shared.push(packet);
        flag(handled)
    })
}

/// Sumbu gamepad dari satu `MotionEvent` (semua -1..1, pemicu 0..1).
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_id_cloudpc_stream_Native_padAxes<'l>(
    _env: JNIEnv<'l>,
    _class: JClass<'l>,
    handle: jlong,
    device: jint,
    lx: jfloat,
    ly: jfloat,
    rx: jfloat,
    ry: jfloat,
    lt: jfloat,
    rt: jfloat,
    hat_x: jfloat,
    hat_y: jfloat,
) {
    let Some(session) = session(handle) else { return };
    guarded((), || {
        let packet = session.shared.pads.lock().ok().and_then(|mut p| p.axes(device, Axes { lx, ly, rx, ry, lt, rt, hat_x, hat_y }));
        session.shared.push(packet);
    })
}

#[no_mangle]
pub extern "system" fn Java_id_cloudpc_stream_Native_padGone<'l>(_env: JNIEnv<'l>, _class: JClass<'l>, handle: jlong, device: jint) {
    let Some(session) = session(handle) else { return };
    guarded((), || {
        let packet = session.shared.pads.lock().ok().and_then(|mut p| p.disconnect(device));
        session.shared.push(packet);
    })
}

/// Mouse fisik (USB/Bluetooth) yang ditangkap: gerak relatif.
#[no_mangle]
pub extern "system" fn Java_id_cloudpc_stream_Native_mouseMove<'l>(_env: JNIEnv<'l>, _class: JClass<'l>, handle: jlong, dx: jint, dy: jint) {
    if let Some(session) = session(handle) {
        session.shared.push(relative_packets(dx, dy));
    }
}

/// Tombol mouse fisik: 1 kiri, 2 tengah, 3 kanan, 4 X1, 5 X2.
#[no_mangle]
pub extern "system" fn Java_id_cloudpc_stream_Native_mouseButton<'l>(_env: JNIEnv<'l>, _class: JClass<'l>, handle: jlong, button: jint, down: jboolean) {
    if let Some(session) = session(handle) {
        if (1..=5).contains(&button) {
            session.shared.push([InputPacket { channel: channel::MOUSE, data: wire::mouse_button(down != JNI_FALSE, button as u8) }]);
        }
    }
}

/// Roda mouse fisik (120 = satu takik).
#[no_mangle]
pub extern "system" fn Java_id_cloudpc_stream_Native_scroll<'l>(_env: JNIEnv<'l>, _class: JClass<'l>, handle: jlong, amount: jint) {
    if let Some(session) = session(handle) {
        let amount = amount.clamp(-32767, 32767) as i16;
        if amount != 0 {
            session.shared.push([InputPacket { channel: channel::MOUSE, data: wire::scroll(amount) }]);
        }
    }
}

// ---- stik di layar ---------------------------------------------------------------------------------------------------

/// Setelah `Overlay` melepas semuanya (sembunyi, mode atur, ganti profil): kirim pelepasan keyboard/mouse, dan nolkan stik
/// Xbox virtual bila sudah pernah dipakai (tanpa membuat stik baru di PC).
fn release_overlay(shared: &Shared) {
    let events = shared.overlay.lock().map(|mut o| o.take_events()).unwrap_or_default();
    for event in events {
        match event {
            PadEvent::Key { vk, down } => {
                let packets = shared.keyboard.lock().map(|mut k| k.vk(vk, down)).unwrap_or_default();
                shared.push(packets);
            }
            PadEvent::Mouse { button, down } => shared.push([InputPacket { channel: channel::MOUSE, data: wire::mouse_button(down, button) }]),
            PadEvent::Look { .. } | PadEvent::Wheel(_) | PadEvent::Absolute { .. } => {}
        }
    }
    let packet = shared.pads.lock().ok().and_then(|mut p| if p.has(VIRTUAL_DEVICE) { p.set_state(VIRTUAL_DEVICE, PadState::default()) } else { None });
    shared.push(packet);
}

/// Profil stik layar: 0 = Gamepad (stik Xbox), 1 = Minecraft (keyboard + mouse).
#[no_mangle]
pub extern "system" fn Java_id_cloudpc_stream_Native_padProfile<'l>(_env: JNIEnv<'l>, _class: JClass<'l>, handle: jlong) -> jint {
    session(handle).and_then(|s| s.shared.overlay.lock().ok().map(|o| o.profile().index() as jint)).unwrap_or(0)
}

/// Ganti profil stik layar (ikut tersimpan). Profil keyboard tidak memakai stik Xbox, jadi stik virtualnya dicabut dari PC.
#[no_mangle]
pub extern "system" fn Java_id_cloudpc_stream_Native_padSetProfile<'l>(_env: JNIEnv<'l>, _class: JClass<'l>, handle: jlong, profile: jint) {
    let Some(session) = session(handle) else { return };
    guarded((), || {
        let uses_gamepad = match session.shared.overlay.lock() {
            Ok(mut overlay) => {
                overlay.set_profile(Profile::from_index(profile));
                overlay.uses_gamepad()
            }
            Err(_) => return,
        };
        release_overlay(&session.shared);
        if !uses_gamepad {
            unplug_overlay_pad(&session.shared);
        }
        session.shared.save_overlay();
    })
}

/// Ukuran area gambar stik layar (piksel); dipanggil saat stream mulai dan tiap ukuran tampilan berubah.
#[no_mangle]
pub extern "system" fn Java_id_cloudpc_stream_Native_padView<'l>(_env: JNIEnv<'l>, _class: JClass<'l>, handle: jlong, width: jint, height: jint) {
    if let Some(session) = session(handle) {
        if let Ok(mut overlay) = session.shared.overlay.lock() {
            overlay.set_view(width as f32, height as f32);
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_id_cloudpc_stream_Native_padShown<'l>(_env: JNIEnv<'l>, _class: JClass<'l>, handle: jlong) -> jboolean {
    flag(session(handle).is_some_and(|s| s.shared.overlay.lock().is_ok_and(|o| o.shown())))
}

/// Tampilkan/sembunyikan stik layar (pilihan ini ikut tersimpan). Saat disembunyikan, stik virtualnya dicabut dari PC.
#[no_mangle]
pub extern "system" fn Java_id_cloudpc_stream_Native_padShow<'l>(_env: JNIEnv<'l>, _class: JClass<'l>, handle: jlong, shown: jboolean) {
    let Some(session) = session(handle) else { return };
    guarded((), || {
        let shown = shown != JNI_FALSE;
        if let Ok(mut overlay) = session.shared.overlay.lock() {
            overlay.set_shown(shown);
        }
        if shown {
            flush_overlay(&session.shared);
        } else {
            // Tombol keyboard/mouse yang masih ditahan dilepas dulu, lalu stik virtual dicabut.
            release_overlay(&session.shared);
            unplug_overlay_pad(&session.shared);
        }
        session.shared.save_overlay();
    })
}

/// Masuk/keluar mode atur. Keluar = tata letak disimpan di HP pemain.
#[no_mangle]
pub extern "system" fn Java_id_cloudpc_stream_Native_padEdit<'l>(_env: JNIEnv<'l>, _class: JClass<'l>, handle: jlong, editing: jboolean) {
    let Some(session) = session(handle) else { return };
    guarded((), || {
        let editing = editing != JNI_FALSE;
        if let Ok(mut overlay) = session.shared.overlay.lock() {
            overlay.set_editing(editing);
        }
        // Yang sedang ditekan saat masuk mode atur dilepas di PC.
        release_overlay(&session.shared);
        if !editing {
            session.shared.save_overlay();
        }
    })
}

/// Tombol mode atur: 1 perbesar, 2 perkecil, 3 sembunyikan/tampilkan kontrol terpilih, 4 kembali ke tata letak bawaan,
/// 5 hapus tombol buatan pemain yang terpilih. Langsung disimpan (pemain bisa meninggalkan layar tanpa menekan Selesai).
#[no_mangle]
pub extern "system" fn Java_id_cloudpc_stream_Native_padEditAction<'l>(_env: JNIEnv<'l>, _class: JClass<'l>, handle: jlong, action: jint) {
    let Some(session) = session(handle) else { return };
    guarded((), || {
        if let (Some(action), Ok(mut overlay)) = (EditAction::from_code(action), session.shared.overlay.lock()) {
            overlay.edit(action);
        }
        release_overlay(&session.shared);
        session.shared.save_overlay();
    })
}

/// Pilihan aksi untuk tombol buatan pemain, satu per baris: `kode|nama di daftar|tulisan bawaan` (`overlay::action_choices`).
#[no_mangle]
pub extern "system" fn Java_id_cloudpc_stream_Native_padChoices<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>) -> jstring {
    let text: String = crate::overlay::action_choices().iter().map(|(code, name, label)| format!("{code}|{name}|{label}\n")).collect();
    to_java(&mut env, &text)
}

/// Kontrol terpilih di mode atur (`Overlay::selected_info`); kosong = belum ada yang terpilih.
#[no_mangle]
pub extern "system" fn Java_id_cloudpc_stream_Native_padSelected<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>, handle: jlong) -> jstring {
    let text = session(handle).and_then(|s| s.shared.overlay.lock().ok().map(|o| o.selected_info())).unwrap_or_default();
    to_java(&mut env, &text)
}

/// Tambah tombol buatan pemain (`mode`: 0 tahan, 1 hidup/mati, 2 tahan sambil mengarahkan). Langsung disimpan di HP.
#[no_mangle]
pub extern "system" fn Java_id_cloudpc_stream_Native_padAdd<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>, handle: jlong, label: JString<'l>, action: JString<'l>, mode: jint) -> jboolean {
    let (label, action) = (string(&mut env, &label), string(&mut env, &action));
    let Some(session) = session(handle) else { return JNI_FALSE };
    guarded(JNI_FALSE, || {
        let added = session.shared.overlay.lock().is_ok_and(|mut o| o.add_button(&label, &action, PadMode::from_code(mode)));
        if added {
            session.shared.save_overlay();
        }
        flag(added)
    })
}

/// Ganti aksi, cara kerja, dan tulisan kontrol terpilih. Langsung disimpan di HP.
#[no_mangle]
pub extern "system" fn Java_id_cloudpc_stream_Native_padChange<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>, handle: jlong, label: JString<'l>, action: JString<'l>, mode: jint) -> jboolean {
    let (label, action) = (string(&mut env, &label), string(&mut env, &action));
    let Some(session) = session(handle) else { return JNI_FALSE };
    guarded(JNI_FALSE, || {
        let changed = session.shared.overlay.lock().is_ok_and(|mut o| o.change_selected(&label, &action, PadMode::from_code(mode)));
        if changed {
            release_overlay(&session.shared);
            session.shared.save_overlay();
        }
        flag(changed)
    })
}

/// Daftar kontrol untuk digambar (format di `Overlay::describe`). Kosong = tidak ada yang digambar.
#[no_mangle]
pub extern "system" fn Java_id_cloudpc_stream_Native_padDraw<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>, handle: jlong) -> jstring {
    let text = session(handle).and_then(|s| s.shared.overlay.lock().ok().map(|o| o.describe())).unwrap_or_default();
    to_java(&mut env, &text)
}

/// Area gambar PC di dalam tampilan (piksel), untuk mode sentuh langsung (toko): ketukan dipetakan ke posisi di gambar.
#[no_mangle]
pub extern "system" fn Java_id_cloudpc_stream_Native_padPicture<'l>(_env: JNIEnv<'l>, _class: JClass<'l>, handle: jlong, left: jint, top: jint, width: jint, height: jint) {
    if let Some(session) = session(handle) {
        if let Ok(mut overlay) = session.shared.overlay.lock() {
            overlay.set_picture(left as f32, top as f32, width as f32, height as f32);
        }
    }
}

// ---- gyro (bidik dengan memiringkan HP) --------------------------------------------------------------------------------

#[no_mangle]
pub extern "system" fn Java_id_cloudpc_stream_Native_gyroEnabled<'l>(_env: JNIEnv<'l>, _class: JClass<'l>, handle: jlong) -> jboolean {
    flag(session(handle).is_some_and(|s| s.shared.overlay.lock().is_ok_and(|o| o.gyro())))
}

#[no_mangle]
pub extern "system" fn Java_id_cloudpc_stream_Native_gyroInverted<'l>(_env: JNIEnv<'l>, _class: JClass<'l>, handle: jlong) -> jboolean {
    flag(session(handle).is_some_and(|s| s.shared.overlay.lock().is_ok_and(|o| o.gyro_inverted())))
}

/// Nyalakan/matikan gyro dan arah terbaliknya (ikut tersimpan bersama tata letak stik layar).
#[no_mangle]
pub extern "system" fn Java_id_cloudpc_stream_Native_gyroSet<'l>(_env: JNIEnv<'l>, _class: JClass<'l>, handle: jlong, enabled: jboolean, inverted: jboolean) {
    let Some(session) = session(handle) else { return };
    guarded((), || {
        if let Ok(mut overlay) = session.shared.overlay.lock() {
            overlay.set_gyro(enabled != JNI_FALSE);
            overlay.set_gyro_inverted(inverted != JNI_FALSE);
        }
        if let Ok(mut gyro) = session.shared.gyro.lock() {
            gyro.reset();
        }
        session.shared.save_overlay();
    })
}

/// Satu sampel giroskop dalam sumbu layar (lihat `gyro.rs`): `yaw`/`pitch` rad/dtk, `seconds` jeda sejak sampel
/// sebelumnya. Diabaikan saat gyro mati, saat mengatur tombol, dan saat toko terbuka.
#[no_mangle]
pub extern "system" fn Java_id_cloudpc_stream_Native_gyro<'l>(_env: JNIEnv<'l>, _class: JClass<'l>, handle: jlong, yaw: jfloat, pitch: jfloat, seconds: jfloat) {
    let Some(session) = session(handle) else { return };
    guarded((), || {
        let (active, invert) = match session.shared.overlay.lock() {
            Ok(overlay) => (overlay.gyro_active(), overlay.gyro_inverted()),
            Err(_) => return,
        };
        if !active {
            return;
        }
        let moved = session.shared.gyro.lock().ok().and_then(|mut g| g.update(yaw, pitch, seconds, invert));
        if let Some((dx, dy)) = moved {
            session.shared.push(relative_packets(dx, dy));
        }
    })
}
