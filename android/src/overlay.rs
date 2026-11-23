//! Stik di layar: tata letak bawaan CloudPC yang bisa diatur ulang tiap pemain.
//!
//! Dua tata letak ("profil"), masing-masing punya bawaan dan disimpan terpisah:
//! - `Gamepad`  : stik Xbox (dua stik analog, d-pad, A/B/X/Y, LB/RB/LT/RT, Start/Back, L3/R3) untuk game yang membaca gamepad.
//! - `Minecraft`: keyboard + mouse untuk game yang TIDAK membaca gamepad (Minecraft Java dan game PC sejenis): stik gerak
//!   = W/A/S/D (didorong penuh ke depan = lari), Lompat, Jongkok (hidup/mati), Pukul/Pakai (tombol mouse; selama ditahan,
//!   geseran jari ikut memutar kamera), Tas, Buang, ganti slot, Esc, Chat. Kamera = geser di luar tombol (trackpad).
//!
//! Posisi kontrol disimpan relatif terhadap tepi kiri atau kanan dalam satuan TINGGI layar, jadi bentuknya sama di HP
//! 16:9 maupun 21:9 dan tombol tetap menempel di tepi tempat jempol berada.
//!
//! - Saat main: sentuhan yang mengenai kontrol menjadi keadaan stik Xbox (`state`) dan/atau kejadian keyboard/mouse
//!   (`take_events`); sentuhan di luar kontrol dikembalikan ke pemanggil (`Route::Pass`) supaya tetap jadi trackpad.
//! - Mode atur: geser kontrol untuk memindahkan, lalu perbesar/perkecil/sembunyikan yang terpilih, atau kembali ke bawaan.
//!   Disimpan sebagai JSON di HP pemain; yang disimpan hanya penyimpangan dari bawaan per id kontrol, jadi kontrol baru di
//!   versi aplikasi berikutnya tetap muncul.
//!
//! Murni data, tanpa Android: sisi Kotlin hanya menggambar daftar dari `describe` dan meneruskan sentuhan.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::gesture::{Touch, TouchKind};
use crate::pad::{bit, PadState};

/// Versi format file tata letak pemain. 2 = bawaan digeser dari pita atas (22 Nov 2026); file versi lain diabaikan.
pub const VERSION: u32 = 2;
/// Pita atas layar stream dipakai pil statistik (kiri) dan bilah ikon (kanan): tidak ada tombol bawaan yang masuk ke
/// sini (satuan tinggi layar). Pemain tetap boleh menaruh tombol di sana lewat mode atur.
pub const TOP_BAND: f32 = 0.16;
/// Batas ukuran kontrol (diameter, satuan tinggi layar).
const SIZE_RANGE: (f32, f32) = (0.07, 0.60);
const SIZE_STEP: f32 = 1.12;
/// Jarak gerak kepala stik dari pusat, sebagai bagian dari jari-jari kontrol.
const STICK_TRAVEL: f32 = 0.6;
/// D-pad dan stik gerak keyboard: sentuhan lebih dekat dari ini ke pusat belum menekan arah apa pun.
const DPAD_DEAD: f32 = 0.22;
const KEY_STICK_DEAD: f32 = 0.30;
/// Stik gerak keyboard didorong sejauh ini ke depan = tombol lari ikut ditahan.
const SPRINT_AT: f32 = 0.95;

/// Kode tombol virtual Windows yang dipakai tata letak bawaan.
mod vk {
    pub const SPACE: u16 = 0x20;
    pub const ESCAPE: u16 = 0x1B;
    pub const A: u16 = 0x41;
    pub const D: u16 = 0x44;
    pub const E: u16 = 0x45;
    pub const F: u16 = 0x46;
    pub const Q: u16 = 0x51;
    pub const S: u16 = 0x53;
    pub const T: u16 = 0x54;
    pub const W: u16 = 0x57;
    pub const F5: u16 = 0x74;
    pub const TAB: u16 = 0x09;
    pub const N1: u16 = 0x31;
    pub const B: u16 = 0x42;
    pub const C: u16 = 0x43;
    pub const R: u16 = 0x52;
    pub const V: u16 = 0x56;
    pub const X: u16 = 0x58;
    pub const Z: u16 = 0x5A;
    pub const LSHIFT: u16 = 0xA0;
    pub const LCONTROL: u16 = 0xA2;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    #[default]
    Gamepad,
    Minecraft,
    Deadlock,
}

impl Profile {
    pub const ALL: [Profile; 3] = [Profile::Gamepad, Profile::Minecraft, Profile::Deadlock];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(index: i32) -> Profile {
        match index {
            1 => Profile::Minecraft,
            2 => Profile::Deadlock,
            _ => Profile::Gamepad,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    LeftStick,
    RightStick,
    Dpad,
    /// Tombol bundar gamepad dengan bit `XINPUT_GAMEPAD_*`.
    Button(u32),
    /// Tombol bahu lonjong (LB/RB) dengan bit tombolnya.
    Shoulder(u32),
    LeftTrigger,
    RightTrigger,
    /// Stik gerak keyboard: [atas, kiri, bawah, kanan] = empat tombol; didorong penuh ke atas = `sprint` ikut ditahan.
    KeyStick { keys: [u16; 4], sprint: u16 },
    /// Tombol keyboard, ditahan selama jari menempel.
    Key(u16),
    /// Tombol keyboard yang berganti hidup/mati tiap ketukan (jongkok).
    KeyToggle(u16),
    /// Tombol mouse (1 kiri, 3 kanan); selama ditahan, geseran jari ikut menggerakkan mouse (memutar kamera).
    Mouse(u8),
    /// Satu putaran roda mouse per ketukan (120 = satu takik ke atas).
    Wheel(i16),
    /// Tombol keyboard yang selama ditahan ikut menggerakkan mouse (skill: tahan, geser untuk mengarahkan, lepas).
    KeyAim(u16),
    /// Tombol mouse yang berganti hidup/mati tiap ketukan (bidik/ADS).
    MouseToggle(u8),
    /// Ketuk = tekan tombol keyboard ini sekali DAN masuk/keluar mode sentuh langsung (toko: ketuk layar = klik di
    /// titik itu, bukan trackpad). Selama mode itu hanya tombol ini yang tampil.
    DirectToggle(u16),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Control {
    pub id: &'static str,
    pub label: &'static str,
    pub kind: Kind,
    pub side: Side,
    /// Jarak pusat dari tepi `side`, satuan tinggi layar.
    pub x: f32,
    /// Jarak pusat dari atas, satuan tinggi layar.
    pub y: f32,
    /// Diameter (tinggi untuk tombol lonjong), satuan tinggi layar.
    pub size: f32,
    pub visible: bool,
}

/// Kejadian keyboard/mouse dari kontrol (profil Minecraft); gamepad dilaporkan lewat `state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Key { vk: u16, down: bool },
    Mouse { button: u8, down: bool },
    /// Gerak mouse relatif (piksel layar PC).
    Look { dx: i32, dy: i32 },
    Wheel(i16),
    /// Posisi mouse mutlak di dalam gambar PC berukuran `(width, height)` (mode sentuh langsung).
    Absolute { x: i16, y: i16, width: i16, height: i16 },
}

/// Yang disimpan per kontrol di HP pemain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct SavedControl {
    id: String,
    side: Side,
    x: f32,
    y: f32,
    size: f32,
    visible: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Saved {
    version: u32,
    shown: bool,
    #[serde(default)]
    profile: Profile,
    #[serde(default)]
    gamepad: Vec<SavedControl>,
    #[serde(default)]
    minecraft: Vec<SavedControl>,
    #[serde(default)]
    deadlock: Vec<SavedControl>,
    /// Bidik dengan memiringkan HP (gyro), dan arahnya dibalik.
    #[serde(default)]
    gyro: bool,
    #[serde(default)]
    gyro_invert: bool,
}

/// Tata letak bawaan CloudPC untuk satu profil (posisi mendatar; lihat catatan modul untuk satuannya).
pub fn default_controls(profile: Profile) -> Vec<Control> {
    let c = |id, label, kind, side, x, y, size| Control { id, label, kind, side, x, y, size, visible: true };
    match profile {
        Profile::Gamepad => vec![
            c("ls", "L", Kind::LeftStick, Side::Left, 0.24, 0.70, 0.36),
            c("dpad", "", Kind::Dpad, Side::Left, 0.62, 0.38, 0.30),
            c("lt", "LT", Kind::LeftTrigger, Side::Left, 0.18, 0.22, 0.11),
            c("lb", "LB", Kind::Shoulder(bit::LB), Side::Left, 0.18, 0.36, 0.11),
            c("l3", "L3", Kind::Button(bit::LEFT_STICK), Side::Left, 0.50, 0.92, 0.09),
            c("back", "Back", Kind::Button(bit::BACK), Side::Left, 0.66, 0.92, 0.09),
            c("rs", "R", Kind::RightStick, Side::Right, 0.66, 0.70, 0.30),
            c("a", "A", Kind::Button(bit::A), Side::Right, 0.26, 0.82, 0.15),
            c("b", "B", Kind::Button(bit::B), Side::Right, 0.12, 0.66, 0.15),
            c("x", "X", Kind::Button(bit::X), Side::Right, 0.40, 0.66, 0.15),
            c("y", "Y", Kind::Button(bit::Y), Side::Right, 0.26, 0.50, 0.15),
            c("rt", "RT", Kind::RightTrigger, Side::Right, 0.18, 0.22, 0.11),
            c("rb", "RB", Kind::Shoulder(bit::RB), Side::Right, 0.18, 0.36, 0.11),
            c("r3", "R3", Kind::Button(bit::RIGHT_STICK), Side::Right, 0.47, 0.92, 0.09),
            c("start", "Start", Kind::Button(bit::START), Side::Right, 0.66, 0.92, 0.09),
        ],
        // Tombol bawaan Minecraft Java: WASD, Spasi lompat, Shift jongkok, Ctrl lari, E tas, Q buang, F tangan kiri,
        // klik kiri pukul/hancurkan, klik kanan pakai/pasang, roda = ganti slot, T chat, F5 sudut pandang.
        Profile::Minecraft => vec![
            c("move", "", Kind::KeyStick { keys: [vk::W, vk::A, vk::S, vk::D], sprint: vk::LCONTROL }, Side::Left, 0.24, 0.70, 0.36),
            c("sneak", "Jongkok", Kind::KeyToggle(vk::LSHIFT), Side::Left, 0.56, 0.86, 0.13),
            c("drop", "Buang", Kind::Key(vk::Q), Side::Left, 0.56, 0.62, 0.10),
            c("esc", "Esc", Kind::Key(vk::ESCAPE), Side::Left, 0.10, 0.24, 0.09),
            c("chat", "Chat", Kind::Key(vk::T), Side::Left, 0.26, 0.24, 0.09),
            c("view", "F5", Kind::Key(vk::F5), Side::Left, 0.42, 0.24, 0.09),
            c("jump", "Lompat", Kind::Key(vk::SPACE), Side::Right, 0.18, 0.80, 0.17),
            c("attack", "Pukul", Kind::Mouse(1), Side::Right, 0.42, 0.86, 0.15),
            c("use", "Pakai", Kind::Mouse(3), Side::Right, 0.20, 0.52, 0.15),
            c("next", ">", Kind::Wheel(-120), Side::Right, 0.42, 0.62, 0.10),
            c("prev", "<", Kind::Wheel(120), Side::Right, 0.56, 0.62, 0.10),
            c("bag", "Tas", Kind::Key(vk::E), Side::Right, 0.10, 0.24, 0.09),
            c("swap", "F", Kind::Key(vk::F), Side::Right, 0.26, 0.24, 0.09),
        ],
        // Tombol bawaan Deadlock (ditulis dari ingatan, cocokkan dengan pengaturan game): WASD, Spasi lompat/zipline,
        // Shift dash, Ctrl jongkok/slide, klik kiri tembak, klik kanan bidik, R isi peluru, Q pukul, F tangkis,
        // 1-4 skill, Z/X/C/V item aktif, B toko, Tab papan skor, klik tengah ping. Skill dan item memakai KeyAim:
        // tahan, geser untuk mengarahkan, lepas (cocok dengan cast "saat dilepas" di pengaturan game).
        Profile::Deadlock => vec![
            c("move", "", Kind::KeyStick { keys: [vk::W, vk::A, vk::S, vk::D], sprint: 0 }, Side::Left, 0.25, 0.70, 0.34),
            c("dash", "Dash", Kind::Key(vk::LSHIFT), Side::Left, 0.55, 0.55, 0.14),
            c("slide", "Slide", Kind::Key(vk::LCONTROL), Side::Left, 0.57, 0.80, 0.13),
            c("shop", "Toko", Kind::DirectToggle(vk::B), Side::Left, 0.10, 0.24, 0.10),
            c("score", "Skor", Kind::Key(vk::TAB), Side::Left, 0.25, 0.24, 0.09),
            c("esc", "Esc", Kind::Key(vk::ESCAPE), Side::Left, 0.40, 0.24, 0.09),
            c("item1", "Z", Kind::KeyAim(vk::Z), Side::Left, 0.70, 0.91, 0.09),
            c("item2", "X", Kind::KeyAim(vk::X), Side::Left, 0.81, 0.91, 0.09),
            c("item3", "C", Kind::KeyAim(vk::C), Side::Right, 0.81, 0.91, 0.09),
            c("item4", "V", Kind::KeyAim(vk::V), Side::Right, 0.70, 0.91, 0.09),
            c("fire", "Tembak", Kind::Mouse(1), Side::Right, 0.23, 0.75, 0.22),
            c("ads", "Bidik", Kind::MouseToggle(3), Side::Right, 0.08, 0.92, 0.10),
            c("skill1", "1", Kind::KeyAim(vk::N1), Side::Right, 0.483, 0.852, 0.13),
            c("skill2", "2", Kind::KeyAim(vk::N1 + 1), Side::Right, 0.49, 0.69, 0.13),
            c("skill3", "3", Kind::KeyAim(vk::N1 + 2), Side::Right, 0.403, 0.552, 0.13),
            c("skill4", "4", Kind::KeyAim(vk::N1 + 3), Side::Right, 0.255, 0.49, 0.13),
            c("jump", "Lompat", Kind::Key(vk::SPACE), Side::Right, 0.10, 0.50, 0.14),
            c("reload", "Isi", Kind::Key(vk::R), Side::Right, 0.58, 0.53, 0.10),
            c("parry", "Tangkis", Kind::Key(vk::F), Side::Right, 0.65, 0.65, 0.10),
            c("melee", "Pukul", Kind::Key(vk::Q), Side::Right, 0.662, 0.79, 0.12),
            c("ping", "Ping", Kind::Mouse(2), Side::Right, 0.52, 0.30, 0.09),
        ],
    }
}

/// Ke mana satu sentuhan pergi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// Bukan urusan stik layar: teruskan ke trackpad.
    Pass,
    /// Mengenai kontrol: keadaan stik (`state`) dan/atau kejadian (`take_events`) mungkin berubah.
    Pad,
    /// Mode atur: tata letak/pilihan berubah, cukup gambar ulang.
    Edit,
}

/// Aksi tombol di mode atur (angka yang sama dipakai sisi Kotlin).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditAction {
    Bigger = 1,
    Smaller = 2,
    ToggleVisible = 3,
    Reset = 4,
}

impl EditAction {
    pub fn from_code(code: i32) -> Option<EditAction> {
        Some(match code {
            1 => EditAction::Bigger,
            2 => EditAction::Smaller,
            3 => EditAction::ToggleVisible,
            4 => EditAction::Reset,
            _ => return None,
        })
    }
}

/// Bentuk untuk digambar (satu huruf di `describe`).
fn shape(kind: Kind) -> char {
    match kind {
        Kind::LeftStick | Kind::RightStick | Kind::KeyStick { .. } => 's',
        Kind::Dpad => 'd',
        Kind::Shoulder(_) | Kind::LeftTrigger | Kind::RightTrigger => 'r',
        Kind::Button(_) | Kind::Key(_) | Kind::KeyToggle(_) | Kind::Mouse(_) | Kind::Wheel(_) | Kind::KeyAim(_) | Kind::MouseToggle(_) | Kind::DirectToggle(_) => 'c',
    }
}

/// Satu jari yang sedang memegang kontrol.
struct Held {
    pointer: i32,
    control: usize,
    last: (f32, f32),
}

pub struct Overlay {
    /// Tata letak per profil (indeks = `Profile::index`).
    layouts: [Vec<Control>; 3],
    profile: Profile,
    shown: bool,
    view: (f32, f32),
    held: Vec<Held>,
    /// Simpangan kepala stik kiri/kanan, -1..1, Y positif = ke bawah (seperti layar). Stik gerak keyboard memakai yang kiri.
    knobs: [(f32, f32); 2],
    dpad: u32,
    /// Tombol yang sedang ditahan stik gerak keyboard.
    stick_keys: Vec<u16>,
    /// Indeks kontrol `KeyToggle` yang sedang hidup.
    toggled: Vec<usize>,
    events: Vec<Event>,
    /// Pengali geseran jari -> gerak mouse saat tombol mouse ditahan, dan sisa pecahannya.
    look_speed: f32,
    look_carry: (f32, f32),
    /// Mode sentuh langsung (toko terbuka): sentuhan di luar tombol toko = klik mouse di titik itu.
    direct: bool,
    direct_pointer: Option<i32>,
    /// Area gambar PC di dalam tampilan (kiri, atas, lebar, tinggi) untuk sentuh langsung; lebar 0 = seluruh tampilan.
    picture: (f32, f32, f32, f32),
    gyro: bool,
    gyro_invert: bool,
    editing: bool,
    selected: Option<usize>,
    /// (id jari, indeks kontrol, selisih sentuhan dari pusat kontrol).
    drag: Option<(i32, usize, f32, f32)>,
}

impl Default for Overlay {
    fn default() -> Self {
        Overlay {
            layouts: [default_controls(Profile::Gamepad), default_controls(Profile::Minecraft), default_controls(Profile::Deadlock)],
            profile: Profile::Gamepad,
            shown: true,
            view: (0.0, 0.0),
            held: Vec::new(),
            knobs: [(0.0, 0.0); 2],
            dpad: 0,
            stick_keys: Vec::new(),
            toggled: Vec::new(),
            events: Vec::new(),
            look_speed: 1.0,
            look_carry: (0.0, 0.0),
            direct: false,
            direct_pointer: None,
            picture: (0.0, 0.0, 0.0, 0.0),
            gyro: false,
            gyro_invert: false,
            editing: false,
            selected: None,
            drag: None,
        }
    }
}

fn apply_saved(controls: &mut [Control], saved: Vec<SavedControl>) {
    for s in saved {
        if let Some(c) = controls.iter_mut().find(|c| c.id == s.id) {
            if [s.x, s.y, s.size].iter().all(|v| v.is_finite()) {
                c.side = s.side;
                c.x = s.x.clamp(0.0, 4.0);
                c.y = s.y.clamp(0.0, 1.0);
                c.size = s.size.clamp(SIZE_RANGE.0, SIZE_RANGE.1);
                c.visible = s.visible;
            }
        }
    }
}

fn to_saved(controls: &[Control]) -> Vec<SavedControl> {
    controls.iter().map(|c| SavedControl { id: c.id.to_string(), side: c.side, x: c.x, y: c.y, size: c.size, visible: c.visible }).collect()
}

impl Overlay {
    /// Tata letak tersimpan di atas bawaan. Teks rusak, versi lain, atau id tak dikenal diabaikan (bawaan dipakai).
    pub fn from_json(text: &str) -> Overlay {
        let mut overlay = Overlay::default();
        let Ok(saved) = serde_json::from_str::<Saved>(text) else { return overlay };
        if saved.version != VERSION {
            return overlay;
        }
        overlay.shown = saved.shown;
        overlay.profile = saved.profile;
        apply_saved(&mut overlay.layouts[Profile::Gamepad.index()], saved.gamepad);
        apply_saved(&mut overlay.layouts[Profile::Minecraft.index()], saved.minecraft);
        apply_saved(&mut overlay.layouts[Profile::Deadlock.index()], saved.deadlock);
        overlay.gyro = saved.gyro;
        overlay.gyro_invert = saved.gyro_invert;
        overlay
    }

    pub fn to_json(&self) -> String {
        let saved = Saved {
            version: VERSION,
            shown: self.shown,
            profile: self.profile,
            gamepad: to_saved(&self.layouts[Profile::Gamepad.index()]),
            minecraft: to_saved(&self.layouts[Profile::Minecraft.index()]),
            deadlock: to_saved(&self.layouts[Profile::Deadlock.index()]),
            gyro: self.gyro,
            gyro_invert: self.gyro_invert,
        };
        serde_json::to_string(&saved).unwrap_or_default()
    }

    pub fn load(path: &Path) -> Overlay {
        std::fs::read_to_string(path).map(|text| Overlay::from_json(&text)).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, self.to_json());
    }

    pub fn controls(&self) -> &[Control] {
        &self.layouts[self.profile.index()]
    }

    pub fn set_view(&mut self, width: f32, height: f32) {
        self.view = (width.max(0.0), height.max(0.0));
    }

    pub fn set_look_speed(&mut self, speed: f32) {
        self.look_speed = if speed > 0.0 { speed } else { 1.0 };
    }

    /// Area gambar PC di dalam tampilan (gambar dijaga rasionya, jadi bisa lebih kecil dari layar).
    pub fn set_picture(&mut self, left: f32, top: f32, width: f32, height: f32) {
        self.picture = (left, top, width.max(0.0), height.max(0.0));
    }

    pub fn gyro(&self) -> bool {
        self.gyro
    }

    pub fn set_gyro(&mut self, on: bool) {
        self.gyro = on;
    }

    pub fn gyro_inverted(&self) -> bool {
        self.gyro_invert
    }

    pub fn set_gyro_inverted(&mut self, on: bool) {
        self.gyro_invert = on;
    }

    /// Gyro boleh menggerakkan bidikan sekarang: dinyalakan pemain, dan tidak sedang mengatur tombol atau di toko.
    pub fn gyro_active(&self) -> bool {
        self.gyro && !self.editing && !self.direct
    }

    /// Mode sentuh langsung sedang aktif (toko terbuka).
    pub fn direct(&self) -> bool {
        self.direct
    }

    pub fn shown(&self) -> bool {
        self.shown
    }

    pub fn editing(&self) -> bool {
        self.editing
    }

    pub fn profile(&self) -> Profile {
        self.profile
    }

    /// Profil ini mengirim stik Xbox ke PC (profil Minecraft hanya keyboard + mouse, jadi tidak membuat stik di PC).
    pub fn uses_gamepad(&self) -> bool {
        self.profile == Profile::Gamepad
    }

    /// Tampil/sembunyi. Menyembunyikan melepas semua yang sedang ditekan.
    pub fn set_shown(&mut self, shown: bool) {
        self.shown = shown;
        if !shown {
            self.release_all();
        }
    }

    pub fn set_profile(&mut self, profile: Profile) {
        if profile != self.profile {
            self.release_all();
            self.selected = None;
            self.drag = None;
            self.profile = profile;
        }
    }

    pub fn set_editing(&mut self, editing: bool) {
        self.editing = editing;
        self.selected = None;
        self.drag = None;
        self.release_all();
    }

    /// Lepas semua yang ditekan; tombol keyboard/mouse yang masih ditahan di PC dilepas lewat kejadian.
    fn release_all(&mut self) {
        let held: Vec<usize> = self.held.drain(..).map(|h| h.control).collect();
        for index in held {
            match self.controls()[index].kind {
                Kind::Key(vk) | Kind::KeyAim(vk) => self.events.push(Event::Key { vk, down: false }),
                Kind::Mouse(button) => self.events.push(Event::Mouse { button, down: false }),
                _ => {}
            }
        }
        for index in std::mem::take(&mut self.toggled) {
            match self.controls()[index].kind {
                Kind::KeyToggle(vk) => self.events.push(Event::Key { vk, down: false }),
                Kind::MouseToggle(button) => self.events.push(Event::Mouse { button, down: false }),
                _ => {}
            }
        }
        if self.direct_pointer.take().is_some() {
            self.events.push(Event::Mouse { button: 1, down: false });
        }
        self.direct = false;
        for vk in std::mem::take(&mut self.stick_keys) {
            self.events.push(Event::Key { vk, down: false });
        }
        self.knobs = [(0.0, 0.0); 2];
        self.dpad = 0;
        self.look_carry = (0.0, 0.0);
    }

    /// Kejadian keyboard/mouse yang menunggu dikirim.
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    /// Pusat dan ukuran kontrol dalam piksel: (cx, cy, lebar, tinggi).
    fn geometry(&self, c: &Control) -> (f32, f32, f32, f32) {
        let (w, h) = self.view;
        let cx = match c.side {
            Side::Left => c.x * h,
            Side::Right => w - c.x * h,
        };
        let height = c.size * h;
        let width = if shape(c.kind) == 'r' { height * 1.7 } else { height };
        (cx, c.y * h, width, height)
    }

    fn hit(&self, c: &Control, x: f32, y: f32) -> bool {
        let (cx, cy, w, h) = self.geometry(c);
        if shape(c.kind) == 'r' {
            (x - cx).abs() <= w / 2.0 && (y - cy).abs() <= h / 2.0
        } else {
            // Sedikit lebih longgar dari gambarnya: jempol jarang tepat di tengah.
            (x - cx).hypot(y - cy) <= w / 2.0 * 1.1
        }
    }

    /// Kontrol teratas (terakhir di daftar) yang mengenai titik ini.
    fn find(&self, x: f32, y: f32, include_hidden: bool) -> Option<usize> {
        self.controls().iter().enumerate().rev().find(|(_, c)| (c.visible || include_hidden) && self.hit(c, x, y)).map(|(i, _)| i)
    }

    fn held_on(&self, control: usize) -> bool {
        self.held.iter().any(|h| h.control == control)
    }

    pub fn touch(&mut self, t: Touch) -> Route {
        if self.view.0 <= 0.0 || self.view.1 <= 0.0 {
            return Route::Pass;
        }
        if self.editing {
            self.edit_touch(t);
            return Route::Edit;
        }
        if !self.shown {
            return Route::Pass;
        }
        if self.direct {
            return self.direct_touch(t);
        }
        match t.kind {
            TouchKind::Down => {
                let Some(index) = self.find(t.x, t.y, false) else { return Route::Pass };
                let already = self.held_on(index);
                self.held.retain(|h| h.pointer != t.id);
                self.held.push(Held { pointer: t.id, control: index, last: (t.x, t.y) });
                match self.controls()[index].kind {
                    Kind::Key(vk) | Kind::KeyAim(vk) if !already => self.events.push(Event::Key { vk, down: true }),
                    Kind::MouseToggle(button) => {
                        let on = self.toggled.contains(&index);
                        if on {
                            self.toggled.retain(|i| *i != index);
                        } else {
                            self.toggled.push(index);
                        }
                        self.events.push(Event::Mouse { button, down: !on });
                    }
                    Kind::DirectToggle(vk) => {
                        // Buka toko: tekan tombolnya sekali, lalu layar jadi sentuh langsung sampai tombol ini diketuk lagi.
                        self.events.push(Event::Key { vk, down: true });
                        self.events.push(Event::Key { vk, down: false });
                        self.direct = true;
                    }
                    Kind::Mouse(button) if !already => self.events.push(Event::Mouse { button, down: true }),
                    Kind::Wheel(amount) => self.events.push(Event::Wheel(amount)),
                    Kind::KeyToggle(vk) => {
                        let on = self.toggled.contains(&index);
                        if on {
                            self.toggled.retain(|i| *i != index);
                        } else {
                            self.toggled.push(index);
                        }
                        self.events.push(Event::Key { vk, down: !on });
                    }
                    _ => {}
                }
                self.aim(index, t.x, t.y);
                Route::Pad
            }
            TouchKind::Move => {
                let Some(position) = self.held.iter().position(|h| h.pointer == t.id) else { return Route::Pass };
                let (index, last) = (self.held[position].control, self.held[position].last);
                self.held[position].last = (t.x, t.y);
                if matches!(self.controls()[index].kind, Kind::Mouse(_) | Kind::KeyAim(_)) {
                    // Jempol yang menahan tombol mouse sekaligus memutar kamera.
                    let (fx, fy) = ((t.x - last.0) * self.look_speed + self.look_carry.0, (t.y - last.1) * self.look_speed + self.look_carry.1);
                    let (dx, dy) = (fx.trunc(), fy.trunc());
                    self.look_carry = (fx - dx, fy - dy);
                    if dx != 0.0 || dy != 0.0 {
                        self.events.push(Event::Look { dx: dx as i32, dy: dy as i32 });
                    }
                }
                self.aim(index, t.x, t.y);
                Route::Pad
            }
            TouchKind::Up | TouchKind::Cancel => {
                let Some(position) = self.held.iter().position(|h| h.pointer == t.id) else { return Route::Pass };
                let index = self.held.remove(position).control;
                if !self.held_on(index) {
                    match self.controls()[index].kind {
                        Kind::LeftStick => self.knobs[0] = (0.0, 0.0),
                        Kind::RightStick => self.knobs[1] = (0.0, 0.0),
                        Kind::Dpad => self.dpad = 0,
                        Kind::Key(vk) => self.events.push(Event::Key { vk, down: false }),
                        Kind::KeyAim(vk) => {
                            self.events.push(Event::Key { vk, down: false });
                            self.look_carry = (0.0, 0.0);
                        }
                        Kind::Mouse(button) => {
                            self.events.push(Event::Mouse { button, down: false });
                            self.look_carry = (0.0, 0.0);
                        }
                        Kind::KeyStick { .. } => {
                            self.knobs[0] = (0.0, 0.0);
                            self.set_stick_keys(Vec::new());
                        }
                        _ => {}
                    }
                }
                Route::Pad
            }
        }
    }

    /// Mode sentuh langsung (toko): hanya tombol toko yang tetap bisa diketuk, untuk keluar. Sentuhan lain menjadi mouse
    /// mutlak: jari turun = kursor ke titik itu + klik kiri ditahan, digeser = kursor ikut, diangkat = klik dilepas.
    fn direct_touch(&mut self, t: Touch) -> Route {
        // Jari yang masih menempel di tombol toko sejak mode ini dinyalakan.
        if let Some(position) = self.held.iter().position(|h| h.pointer == t.id) {
            if matches!(t.kind, TouchKind::Up | TouchKind::Cancel) {
                self.held.remove(position);
            }
            return Route::Pad;
        }
        if t.kind == TouchKind::Down {
            if let Some(index) = self.find(t.x, t.y, false) {
                if let Kind::DirectToggle(vk) = self.controls()[index].kind {
                    // Tutup toko: lepas klik yang tertahan, tekan tombolnya sekali lagi, kembali ke tombol biasa.
                    if self.direct_pointer.take().is_some() {
                        self.events.push(Event::Mouse { button: 1, down: false });
                    }
                    self.events.push(Event::Key { vk, down: true });
                    self.events.push(Event::Key { vk, down: false });
                    self.direct = false;
                    // Jari ini dicatat memegang tombol toko supaya saat diangkat tidak diteruskan ke trackpad.
                    self.held.push(Held { pointer: t.id, control: index, last: (t.x, t.y) });
                    return Route::Pad;
                }
            }
        }
        match t.kind {
            TouchKind::Down if self.direct_pointer.is_none() => {
                self.direct_pointer = Some(t.id);
                self.events.push(self.absolute(t.x, t.y));
                self.events.push(Event::Mouse { button: 1, down: true });
            }
            TouchKind::Move if self.direct_pointer == Some(t.id) => self.events.push(self.absolute(t.x, t.y)),
            TouchKind::Up | TouchKind::Cancel if self.direct_pointer == Some(t.id) => {
                self.direct_pointer = None;
                self.events.push(Event::Mouse { button: 1, down: false });
            }
            _ => {} // jari kedua diabaikan
        }
        Route::Pad
    }

    /// Titik sentuh -> posisi mouse mutlak di dalam gambar PC.
    fn absolute(&self, x: f32, y: f32) -> Event {
        let (left, top, w, h) = if self.picture.2 > 1.0 && self.picture.3 > 1.0 { self.picture } else { (0.0, 0.0, self.view.0, self.view.1) };
        let (w, h) = (w.clamp(2.0, 32767.0), h.clamp(2.0, 32767.0));
        Event::Absolute { x: (x - left).clamp(0.0, w - 1.0) as i16, y: (y - top).clamp(0.0, h - 1.0) as i16, width: (w - 1.0) as i16, height: (h - 1.0) as i16 }
    }

    /// Ganti himpunan tombol yang ditahan stik gerak keyboard; hanya selisihnya yang dikirim.
    fn set_stick_keys(&mut self, wanted: Vec<u16>) {
        for vk in self.stick_keys.clone() {
            if !wanted.contains(&vk) {
                self.events.push(Event::Key { vk, down: false });
            }
        }
        for vk in &wanted {
            if !self.stick_keys.contains(vk) {
                self.events.push(Event::Key { vk: *vk, down: true });
            }
        }
        self.stick_keys = wanted;
    }

    /// Stik dan d-pad mengikuti posisi jari; tombol cukup "ditekan" selama jarinya ada di `held`.
    fn aim(&mut self, index: usize, x: f32, y: f32) {
        let control = &self.controls()[index];
        let kind = control.kind;
        let (cx, cy, w, _) = self.geometry(control);
        let radius = (w / 2.0).max(1.0);
        match kind {
            Kind::LeftStick | Kind::RightStick | Kind::KeyStick { .. } => {
                let (mut vx, mut vy) = ((x - cx) / (radius * STICK_TRAVEL), (y - cy) / (radius * STICK_TRAVEL));
                let length = vx.hypot(vy);
                if length > 1.0 {
                    vx /= length;
                    vy /= length;
                }
                self.knobs[usize::from(kind == Kind::RightStick)] = (vx, vy);
                if let Kind::KeyStick { keys, sprint } = kind {
                    let length = length.min(1.0);
                    let mut wanted = Vec::new();
                    if length >= KEY_STICK_DEAD {
                        // Delapan arah seperti d-pad: sumbu ikut ditekan bila bagiannya cukup besar.
                        if vy.abs() > 0.38 * length {
                            wanted.push(if vy < 0.0 { keys[0] } else { keys[2] });
                        }
                        if vx.abs() > 0.38 * length {
                            wanted.push(if vx < 0.0 { keys[1] } else { keys[3] });
                        }
                        if sprint != 0 && length >= SPRINT_AT && wanted.contains(&keys[0]) {
                            wanted.push(sprint);
                        }
                    }
                    self.set_stick_keys(wanted);
                }
            }
            Kind::Dpad => {
                let (vx, vy) = ((x - cx) / radius, (y - cy) / radius);
                let length = vx.hypot(vy);
                self.dpad = 0;
                if length >= DPAD_DEAD {
                    if vx.abs() > 0.38 * length {
                        self.dpad |= if vx < 0.0 { bit::LEFT } else { bit::RIGHT };
                    }
                    if vy.abs() > 0.38 * length {
                        self.dpad |= if vy < 0.0 { bit::UP } else { bit::DOWN };
                    }
                }
            }
            _ => {}
        }
    }

    /// Keadaan stik Xbox dari semua yang sedang ditekan (profil Gamepad).
    pub fn state(&self) -> PadState {
        let mut state = PadState { buttons: self.dpad, ..PadState::default() };
        for held in &self.held {
            match self.controls()[held.control].kind {
                Kind::Button(bit) | Kind::Shoulder(bit) => state.buttons |= bit,
                Kind::LeftTrigger => state.triggers.0 = 255,
                Kind::RightTrigger => state.triggers.1 = 255,
                _ => {}
            }
        }
        if self.uses_gamepad() {
            let axis = |v: f32| (v.clamp(-1.0, 1.0) * 32767.0).round() as i16;
            // XInput: Y positif = ke atas, kebalikan layar.
            state.left = (axis(self.knobs[0].0), axis(-self.knobs[0].1));
            state.right = (axis(self.knobs[1].0), axis(-self.knobs[1].1));
        }
        state
    }

    fn edit_touch(&mut self, t: Touch) {
        match t.kind {
            TouchKind::Down => {
                if self.drag.is_some() {
                    return; // jari kedua diabaikan selama satu kontrol sedang digeser
                }
                self.selected = self.find(t.x, t.y, true);
                if let Some(index) = self.selected {
                    let (cx, cy, _, _) = self.geometry(&self.controls()[index]);
                    self.drag = Some((t.id, index, t.x - cx, t.y - cy));
                }
            }
            TouchKind::Move => {
                let Some((id, index, dx, dy)) = self.drag else { return };
                if id != t.id {
                    return;
                }
                let (w, h) = self.view;
                let (_, _, cw, ch) = self.geometry(&self.controls()[index]);
                // Kontrol tidak boleh keluar layar.
                let cx = (t.x - dx).clamp((cw / 2.0).min(w / 2.0), (w - cw / 2.0).max(w / 2.0));
                let cy = (t.y - dy).clamp((ch / 2.0).min(h / 2.0), (h - ch / 2.0).max(h / 2.0));
                let control = &mut self.layouts[self.profile.index()][index];
                control.side = if cx < w / 2.0 { Side::Left } else { Side::Right };
                control.x = match control.side {
                    Side::Left => cx / h,
                    Side::Right => (w - cx) / h,
                };
                control.y = cy / h;
            }
            TouchKind::Up | TouchKind::Cancel => {
                if self.drag.is_some_and(|(id, ..)| id == t.id) {
                    self.drag = None;
                }
            }
        }
    }

    /// Tombol mode atur. Perbesar/perkecil/sembunyikan berlaku untuk kontrol terpilih; kembali ke bawaan untuk seluruh
    /// profil yang sedang dipakai (profil lain tidak tersentuh).
    pub fn edit(&mut self, action: EditAction) {
        let profile = self.profile;
        if action == EditAction::Reset {
            self.layouts[profile.index()] = default_controls(profile);
            self.selected = None;
            self.drag = None;
            return;
        }
        let Some(control) = self.selected.and_then(|i| self.layouts[profile.index()].get_mut(i)) else { return };
        match action {
            EditAction::Bigger => control.size = (control.size * SIZE_STEP).min(SIZE_RANGE.1),
            EditAction::Smaller => control.size = (control.size / SIZE_STEP).max(SIZE_RANGE.0),
            EditAction::ToggleVisible => control.visible = !control.visible,
            EditAction::Reset => {}
        }
    }

    /// Daftar untuk digambar, satu kontrol per baris:
    /// `id|label|bentuk|cx|cy|lebar|tinggi|tanda|kx|ky` (piksel). Bentuk: s stik, d d-pad, c bundar, r lonjong.
    /// Tanda (bit): 1 terlihat, 2 ditekan (atau hidup, untuk tombol hidup/mati), 4 terpilih (mode atur). `kx,ky`: geseran
    /// kepala stik dari pusat; untuk d-pad `kx` berisi bit arah yang ditekan. Kontrol tersembunyi hanya ikut di mode atur.
    /// Kosong bila tidak perlu digambar.
    pub fn describe(&self) -> String {
        if !(self.shown || self.editing) || self.view.0 <= 0.0 {
            return String::new();
        }
        let mut out = String::new();
        for (index, c) in self.controls().iter().enumerate() {
            if !c.visible && !self.editing {
                continue;
            }
            let shop = matches!(c.kind, Kind::DirectToggle(_));
            if self.direct && !shop {
                continue; // toko terbuka: tombol lain disembunyikan supaya tidak menutupi tokonya
            }
            let (cx, cy, w, h) = self.geometry(c);
            let pressed = self.held_on(index) || self.toggled.contains(&index) || (self.direct && shop);
            let flags = u8::from(c.visible) | u8::from(pressed) << 1 | u8::from(self.selected == Some(index)) << 2;
            let travel = w / 2.0 * STICK_TRAVEL;
            let (kx, ky) = match c.kind {
                Kind::LeftStick | Kind::KeyStick { .. } => (self.knobs[0].0 * travel, self.knobs[0].1 * travel),
                Kind::RightStick => (self.knobs[1].0 * travel, self.knobs[1].1 * travel),
                Kind::Dpad => (self.dpad as f32, 0.0),
                _ => (0.0, 0.0),
            };
            out.push_str(&format!("{}|{}|{}|{cx:.1}|{cy:.1}|{w:.1}|{h:.1}|{flags}|{kx:.1}|{ky:.1}\n", c.id, c.label, shape(c.kind)));
        }
        out
    }

    #[cfg(test)]
    fn center(&self, id: &str) -> (f32, f32) {
        let c = self.controls().iter().find(|c| c.id == id).unwrap();
        let (cx, cy, _, _) = self.geometry(c);
        (cx, cy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(kind: TouchKind, id: i32, (x, y): (f32, f32)) -> Touch {
        Touch { kind, id, x, y, time_ms: 0 }
    }

    fn overlay(view: (f32, f32)) -> Overlay {
        let mut o = Overlay::default();
        o.set_view(view.0, view.1);
        o
    }

    fn minecraft(view: (f32, f32)) -> Overlay {
        let mut o = overlay(view);
        o.set_profile(Profile::Minecraft);
        o
    }

    fn key(vk: u16, down: bool) -> Event {
        Event::Key { vk, down }
    }

    #[test]
    fn default_layouts_have_no_overlapping_controls_on_common_phones() {
        // 16:9, 20:9, dan 21:9: semua kontrol bawaan di dalam layar dan tidak ada dua yang bertumpuk.
        for profile in Profile::ALL {
            for view in [(1920.0, 1080.0), (2400.0, 1080.0), (2520.0, 1080.0)] {
                let mut o = overlay(view);
                o.set_profile(profile);
                for (i, a) in o.controls().iter().enumerate() {
                    let (ax, ay, aw, ah) = o.geometry(a);
                    assert!(ax - aw / 2.0 >= 0.0 && ax + aw / 2.0 <= view.0 && ay - ah / 2.0 >= 0.0 && ay + ah / 2.0 <= view.1, "{profile:?} {} keluar layar {view:?}", a.id);
                    // Pita atas milik pil statistik dan bilah ikon.
                    assert!(ay - ah / 2.0 >= TOP_BAND * view.1, "{profile:?} {} masuk pita atas", a.id);
                    for b in o.controls().iter().skip(i + 1) {
                        let (bx, by, bw, bh) = o.geometry(b);
                        let apart = (ax - bx).abs() >= (aw + bw) / 2.0 || (ay - by).abs() >= (ah + bh) / 2.0;
                        assert!(apart, "{profile:?}: {} dan {} bertumpuk di layar {view:?}", a.id, b.id);
                    }
                }
            }
        }
    }

    #[test]
    fn touch_outside_controls_is_passed_to_the_trackpad() {
        for mut o in [overlay((2400.0, 1080.0)), minecraft((2400.0, 1080.0))] {
            assert_eq!(o.touch(t(TouchKind::Down, 0, (1200.0, 540.0))), Route::Pass);
            assert_eq!(o.touch(t(TouchKind::Move, 0, (1210.0, 540.0))), Route::Pass);
            assert_eq!(o.touch(t(TouchKind::Up, 0, (1210.0, 540.0))), Route::Pass);
            assert_eq!(o.state(), PadState::default());
            assert!(o.take_events().is_empty());
        }
    }

    #[test]
    fn buttons_hold_while_the_finger_is_down_and_combine() {
        let mut o = overlay((2400.0, 1080.0));
        assert_eq!(o.touch(t(TouchKind::Down, 1, o.center("a"))), Route::Pad);
        assert_eq!(o.touch(t(TouchKind::Down, 2, o.center("rb"))), Route::Pad);
        assert_eq!(o.touch(t(TouchKind::Down, 3, o.center("rt"))), Route::Pad);
        let s = o.state();
        assert_eq!(s.buttons, bit::A | bit::RB);
        assert_eq!(s.triggers, (0, 255));
        assert_eq!(o.touch(t(TouchKind::Up, 1, o.center("a"))), Route::Pad);
        assert_eq!(o.state().buttons, bit::RB);
        o.touch(t(TouchKind::Cancel, 2, o.center("rb")));
        o.touch(t(TouchKind::Cancel, 3, o.center("rt")));
        assert_eq!(o.state(), PadState::default());
        assert!(o.take_events().is_empty()); // profil gamepad tidak pernah mengirim keyboard/mouse
    }

    #[test]
    fn stick_follows_the_thumb_clamps_and_recentres() {
        let mut o = overlay((2400.0, 1080.0));
        let (cx, cy) = o.center("ls");
        o.touch(t(TouchKind::Down, 5, (cx, cy)));
        assert_eq!(o.state().left, (0, 0));
        // Jauh ke kanan: penuh. Ke atas di layar = Y XInput positif.
        o.touch(t(TouchKind::Move, 5, (cx + 900.0, cy)));
        assert_eq!(o.state().left, (32767, 0));
        o.touch(t(TouchKind::Move, 5, (cx, cy - 900.0)));
        assert_eq!(o.state().left, (0, 32767));
        // Setengah jalan.
        let travel = 0.36 * 1080.0 / 2.0 * STICK_TRAVEL;
        o.touch(t(TouchKind::Move, 5, (cx - travel / 2.0, cy)));
        assert_eq!(o.state().left.0, -16384);
        // Jari yang sudah memegang stik tetap miliknya walau keluar dari lingkaran.
        assert_eq!(o.touch(t(TouchKind::Move, 5, (cx + 2000.0, cy + 50.0))), Route::Pad);
        o.touch(t(TouchKind::Up, 5, (cx, cy)));
        assert_eq!(o.state().left, (0, 0));
        // Stik kanan terpisah.
        o.touch(t(TouchKind::Down, 6, o.center("rs")));
        let (rx, ry) = o.center("rs");
        o.touch(t(TouchKind::Move, 6, (rx, ry + 900.0)));
        assert_eq!((o.state().left, o.state().right), ((0, 0), (0, -32767)));
    }

    #[test]
    fn dpad_gives_eight_directions_and_slides() {
        let mut o = overlay((2400.0, 1080.0));
        let (cx, cy) = o.center("dpad");
        o.touch(t(TouchKind::Down, 1, (cx, cy)));
        assert_eq!(o.state().buttons, 0); // tepat di tengah: belum ada arah
        o.touch(t(TouchKind::Move, 1, (cx - 100.0, cy)));
        assert_eq!(o.state().buttons, bit::LEFT);
        o.touch(t(TouchKind::Move, 1, (cx - 90.0, cy - 90.0)));
        assert_eq!(o.state().buttons, bit::LEFT | bit::UP);
        o.touch(t(TouchKind::Move, 1, (cx, cy + 120.0)));
        assert_eq!(o.state().buttons, bit::DOWN);
        o.touch(t(TouchKind::Up, 1, (cx, cy + 120.0)));
        assert_eq!(o.state().buttons, 0);
    }

    #[test]
    fn minecraft_move_stick_presses_wasd_and_sprints_when_pushed_fully_forward() {
        let mut o = minecraft((2400.0, 1080.0));
        let (cx, cy) = o.center("move");
        let travel = 0.36 * 1080.0 / 2.0 * STICK_TRAVEL;
        o.touch(t(TouchKind::Down, 1, (cx, cy)));
        assert!(o.take_events().is_empty()); // di tengah: diam
        o.touch(t(TouchKind::Move, 1, (cx, cy - travel * 0.6)));
        assert_eq!(o.take_events(), vec![key(vk::W, true)]);
        // Serong kiri-depan: A ikut, W tidak dikirim ulang.
        o.touch(t(TouchKind::Move, 1, (cx - travel * 0.5, cy - travel * 0.5)));
        assert_eq!(o.take_events(), vec![key(vk::A, true)]);
        // Didorong penuh ke depan: A lepas, Ctrl (lari) ditahan.
        o.touch(t(TouchKind::Move, 1, (cx, cy - travel * 3.0)));
        assert_eq!(o.take_events(), vec![key(vk::A, false), key(vk::LCONTROL, true)]);
        // Mundur: W dan Ctrl lepas, S ditekan.
        o.touch(t(TouchKind::Move, 1, (cx, cy + travel)));
        assert_eq!(o.take_events(), vec![key(vk::W, false), key(vk::LCONTROL, false), key(vk::S, true)]);
        o.touch(t(TouchKind::Up, 1, (cx, cy + travel)));
        assert_eq!(o.take_events(), vec![key(vk::S, false)]);
        // Profil ini tidak membuat stik Xbox.
        assert!(!o.uses_gamepad());
        assert_eq!(o.state(), PadState::default());
    }

    #[test]
    fn minecraft_buttons_send_keys_toggle_sneak_and_scroll_the_hotbar() {
        let mut o = minecraft((2400.0, 1080.0));
        o.touch(t(TouchKind::Down, 1, o.center("jump")));
        assert_eq!(o.take_events(), vec![key(vk::SPACE, true)]);
        o.touch(t(TouchKind::Up, 1, o.center("jump")));
        assert_eq!(o.take_events(), vec![key(vk::SPACE, false)]);
        // Jongkok: ketuk = hidup, ketuk lagi = mati; mengangkat jari tidak melepasnya.
        o.touch(t(TouchKind::Down, 2, o.center("sneak")));
        o.touch(t(TouchKind::Up, 2, o.center("sneak")));
        assert_eq!(o.take_events(), vec![key(vk::LSHIFT, true)]);
        assert!(o.describe().lines().any(|l| l.starts_with("sneak|") && l.split('|').nth(7) == Some("3")));
        o.touch(t(TouchKind::Down, 2, o.center("sneak")));
        o.touch(t(TouchKind::Up, 2, o.center("sneak")));
        assert_eq!(o.take_events(), vec![key(vk::LSHIFT, false)]);
        // Ganti slot: satu takik roda per ketukan.
        o.touch(t(TouchKind::Down, 3, o.center("next")));
        o.touch(t(TouchKind::Up, 3, o.center("next")));
        o.touch(t(TouchKind::Down, 3, o.center("prev")));
        o.touch(t(TouchKind::Up, 3, o.center("prev")));
        assert_eq!(o.take_events(), vec![Event::Wheel(-120), Event::Wheel(120)]);
    }

    #[test]
    fn minecraft_attack_button_holds_the_mouse_and_turns_the_camera_while_dragging() {
        let mut o = minecraft((2400.0, 1080.0));
        o.set_look_speed(0.5);
        let (cx, cy) = o.center("attack");
        o.touch(t(TouchKind::Down, 1, (cx, cy)));
        assert_eq!(o.take_events(), vec![Event::Mouse { button: 1, down: true }]);
        o.touch(t(TouchKind::Move, 1, (cx + 40.0, cy - 10.0)));
        assert_eq!(o.take_events(), vec![Event::Look { dx: 20, dy: -5 }]);
        // Pecahan tidak hilang: 3 x 1 px x 0,5 = 1 px (dua langkah pertama belum cukup).
        for step in 1..=3 {
            o.touch(t(TouchKind::Move, 1, (cx + 40.0 + step as f32, cy - 10.0)));
        }
        assert_eq!(o.take_events(), vec![Event::Look { dx: 1, dy: 0 }]);
        o.touch(t(TouchKind::Up, 1, (cx + 43.0, cy - 10.0)));
        assert_eq!(o.take_events(), vec![Event::Mouse { button: 1, down: false }]);
    }

    fn deadlock(view: (f32, f32)) -> Overlay {
        let mut o = overlay(view);
        o.set_profile(Profile::Deadlock);
        o
    }

    #[test]
    fn deadlock_skill_is_held_aimed_by_dragging_and_cast_on_release() {
        let mut o = deadlock((2400.0, 1080.0));
        let (cx, cy) = o.center("skill2");
        o.touch(t(TouchKind::Down, 1, (cx, cy)));
        assert_eq!(o.take_events(), vec![key(0x32, true)]);
        o.touch(t(TouchKind::Move, 1, (cx - 30.0, cy + 12.0)));
        assert_eq!(o.take_events(), vec![Event::Look { dx: -30, dy: 12 }]);
        o.touch(t(TouchKind::Up, 1, (cx - 30.0, cy + 12.0)));
        assert_eq!(o.take_events(), vec![key(0x32, false)]);
        // Tembak sambil menahan skill lain: dua-duanya ditahan.
        o.touch(t(TouchKind::Down, 1, o.center("fire")));
        o.touch(t(TouchKind::Down, 2, o.center("dash")));
        assert_eq!(o.take_events(), vec![Event::Mouse { button: 1, down: true }, key(vk::LSHIFT, true)]);
        assert!(!o.uses_gamepad());
    }

    #[test]
    fn deadlock_aim_button_toggles_the_right_mouse_button() {
        let mut o = deadlock((2400.0, 1080.0));
        let ads = o.center("ads");
        o.touch(t(TouchKind::Down, 1, ads));
        o.touch(t(TouchKind::Up, 1, ads));
        assert_eq!(o.take_events(), vec![Event::Mouse { button: 3, down: true }]);
        o.touch(t(TouchKind::Down, 1, ads));
        o.touch(t(TouchKind::Up, 1, ads));
        assert_eq!(o.take_events(), vec![Event::Mouse { button: 3, down: false }]);
        // Menyembunyikan stik saat bidik masih menyala melepas klik kanan.
        o.touch(t(TouchKind::Down, 1, ads));
        o.touch(t(TouchKind::Up, 1, ads));
        o.take_events();
        o.set_shown(false);
        assert_eq!(o.take_events(), vec![Event::Mouse { button: 3, down: false }]);
    }

    #[test]
    fn deadlock_shop_switches_the_screen_to_direct_touch_until_tapped_again() {
        let mut o = deadlock((2400.0, 1080.0));
        o.set_picture(240.0, 0.0, 1920.0, 1080.0); // gambar 16:9 di tengah layar 20:9
        let shop = o.center("shop");
        o.touch(t(TouchKind::Down, 1, shop));
        o.touch(t(TouchKind::Up, 1, shop));
        assert_eq!(o.take_events(), vec![key(vk::B, true), key(vk::B, false)]);
        assert!(o.direct() && !o.gyro_active());
        // Hanya tombol toko yang digambar, dalam keadaan menyala.
        let drawn = o.describe();
        assert_eq!(drawn.lines().count(), 1);
        assert!(drawn.starts_with("shop|Toko|c|") && drawn.split('|').nth(7) == Some("3"));
        // Ketuk item di toko: kursor ke titik itu (relatif terhadap gambar) + klik; tombol lain tidak bereaksi.
        let fire = o.center("fire");
        assert_eq!(o.touch(t(TouchKind::Down, 2, fire)), Route::Pad);
        assert_eq!(o.take_events(), vec![Event::Absolute { x: (fire.0 - 240.0) as i16, y: fire.1 as i16, width: 1919, height: 1079 }, Event::Mouse { button: 1, down: true }]);
        o.touch(t(TouchKind::Move, 2, (1000.0, 500.0)));
        assert_eq!(o.take_events(), vec![Event::Absolute { x: 760, y: 500, width: 1919, height: 1079 }]);
        o.touch(t(TouchKind::Up, 2, (1000.0, 500.0)));
        assert_eq!(o.take_events(), vec![Event::Mouse { button: 1, down: false }]);
        // Di luar gambar (pita hitam kiri) dijepit ke tepi gambar.
        o.touch(t(TouchKind::Down, 2, (10.0, 600.0)));
        assert_eq!(o.take_events()[0], Event::Absolute { x: 0, y: 600, width: 1919, height: 1079 });
        // Ketuk Toko lagi selagi jari lain masih menekan: klik dilepas, toko ditutup, tombol kembali.
        o.touch(t(TouchKind::Down, 3, shop));
        assert_eq!(o.take_events(), vec![Event::Mouse { button: 1, down: false }, key(vk::B, true), key(vk::B, false)]);
        assert!(!o.direct());
        assert_eq!(o.touch(t(TouchKind::Up, 3, shop)), Route::Pad); // tidak bocor ke trackpad
        assert!(o.take_events().is_empty());
        assert_eq!(o.describe().lines().count(), default_controls(Profile::Deadlock).len());
    }

    #[test]
    fn gyro_choice_is_saved_and_off_by_default() {
        let mut o = Overlay::default();
        assert!(!o.gyro() && !o.gyro_inverted() && !o.gyro_active());
        o.set_gyro(true);
        o.set_gyro_inverted(true);
        o.set_profile(Profile::Deadlock);
        let loaded = Overlay::from_json(&o.to_json());
        assert!(loaded.gyro() && loaded.gyro_inverted() && loaded.gyro_active());
        assert_eq!(loaded.profile(), Profile::Deadlock);
        // Mengatur tombol menghentikan gyro sementara.
        let mut editing = loaded;
        editing.set_editing(true);
        assert!(!editing.gyro_active());
    }

    #[test]
    fn hiding_switching_profile_or_editing_releases_everything_held_on_the_pc() {
        let mut o = minecraft((2400.0, 1080.0));
        let (cx, cy) = o.center("move");
        o.touch(t(TouchKind::Down, 1, (cx, cy)));
        o.touch(t(TouchKind::Move, 1, (cx, cy - 900.0)));
        o.touch(t(TouchKind::Down, 2, o.center("sneak")));
        o.touch(t(TouchKind::Down, 3, o.center("attack")));
        o.take_events();
        o.set_shown(false);
        let released = o.take_events();
        for expected in [key(vk::W, false), key(vk::LCONTROL, false), key(vk::LSHIFT, false), Event::Mouse { button: 1, down: false }] {
            assert!(released.contains(&expected), "{expected:?} tidak dilepas: {released:?}");
        }
        assert_eq!(released.len(), 4);
        // Tidak ada yang tersisa untuk dilepas dua kali.
        o.set_shown(true);
        o.set_profile(Profile::Gamepad);
        o.set_editing(true);
        assert!(o.take_events().is_empty());
    }

    #[test]
    fn hidden_overlay_or_hidden_control_does_not_take_touches() {
        let mut o = overlay((2400.0, 1080.0));
        let a = o.center("a");
        o.touch(t(TouchKind::Down, 1, a));
        o.set_shown(false);
        assert_eq!(o.state(), PadState::default()); // menyembunyikan melepas yang ditekan
        assert_eq!(o.touch(t(TouchKind::Down, 2, a)), Route::Pass);
        assert!(o.describe().is_empty());
        o.set_shown(true);
        o.set_editing(true);
        o.touch(t(TouchKind::Down, 3, a));
        o.touch(t(TouchKind::Up, 3, a));
        o.edit(EditAction::ToggleVisible);
        o.set_editing(false);
        assert_eq!(o.touch(t(TouchKind::Down, 4, a)), Route::Pass);
        assert!(!o.describe().contains("a|A|"));
    }

    #[test]
    fn edit_mode_moves_resizes_and_survives_save_and_load_per_profile() {
        let mut o = overlay((2400.0, 1080.0));
        o.set_editing(true);
        let from = o.center("a");
        // Geser A dari kanan bawah ke kiri atas: jangkar pindah ke tepi kiri.
        assert_eq!(o.touch(t(TouchKind::Down, 1, from)), Route::Edit);
        assert_eq!(o.touch(t(TouchKind::Move, 1, (600.0, 300.0))), Route::Edit);
        o.touch(t(TouchKind::Up, 1, (600.0, 300.0)));
        assert_eq!(o.center("a"), (600.0, 300.0));
        o.edit(EditAction::Bigger);
        o.edit(EditAction::Bigger);
        let size = o.controls().iter().find(|c| c.id == "a").unwrap().size;
        assert!((size - 0.15 * SIZE_STEP * SIZE_STEP).abs() < 1e-5);
        // Mode atur tidak menghasilkan input game.
        assert_eq!(o.state(), PadState::default());
        o.set_editing(false);
        // Profil Minecraft diatur terpisah.
        o.set_profile(Profile::Minecraft);
        o.set_editing(true);
        o.touch(t(TouchKind::Down, 1, o.center("jump")));
        o.touch(t(TouchKind::Move, 1, (1000.0, 500.0)));
        o.touch(t(TouchKind::Up, 1, (1000.0, 500.0)));
        o.set_editing(false);

        let mut loaded = Overlay::from_json(&o.to_json());
        loaded.set_view(2400.0, 1080.0);
        assert_eq!(loaded.profile(), Profile::Minecraft);
        assert_eq!(loaded.center("jump"), (1000.0, 500.0));
        loaded.set_profile(Profile::Gamepad);
        assert_eq!(loaded.center("a"), (600.0, 300.0));
        assert_eq!(loaded.layouts, o.layouts);
        // Layar lain (16:9): A tetap pada jarak yang sama dari tepi kiri.
        loaded.set_view(1920.0, 1080.0);
        assert_eq!(loaded.center("a"), (600.0, 300.0));

        // Kembali ke bawaan hanya untuk profil yang sedang dipakai.
        loaded.set_editing(true);
        loaded.edit(EditAction::Reset);
        assert_eq!(loaded.controls(), default_controls(Profile::Gamepad).as_slice());
        assert_ne!(loaded.layouts[Profile::Minecraft.index()], default_controls(Profile::Minecraft));
    }

    #[test]
    fn dragging_cannot_push_a_control_off_screen() {
        let mut o = overlay((2400.0, 1080.0));
        o.set_editing(true);
        o.touch(t(TouchKind::Down, 1, o.center("ls")));
        o.touch(t(TouchKind::Move, 1, (-500.0, 5000.0)));
        let radius = 0.36 * 1080.0 / 2.0;
        let (cx, cy) = o.center("ls");
        assert!((cx - radius).abs() < 0.5 && (cy - (1080.0 - radius)).abs() < 0.5, "pusat ({cx}, {cy})");
    }

    #[test]
    fn broken_or_foreign_saved_layout_falls_back_to_default() {
        let gamepad = default_controls(Profile::Gamepad);
        assert_eq!(Overlay::from_json("bukan json").controls(), gamepad.as_slice());
        assert_eq!(Overlay::from_json(r#"{"version":99,"shown":false}"#).controls(), gamepad.as_slice());
        // Id tak dikenal dilewati, nilai gila dijepit, sisanya tetap bawaan.
        let o = Overlay::from_json(r#"{"version":2,"shown":false,"gamepad":[{"id":"zz","side":"left","x":1,"y":1,"size":1,"visible":true},{"id":"a","side":"left","x":0.5,"y":0.5,"size":9.0,"visible":true}]}"#);
        assert!(!o.shown());
        assert_eq!(o.profile(), Profile::Gamepad);
        let a = o.controls().iter().find(|c| c.id == "a").unwrap();
        assert_eq!((a.side, a.x, a.size), (Side::Left, 0.5, SIZE_RANGE.1));
        assert_eq!(o.controls().len(), gamepad.len());
    }

    #[test]
    fn describe_lists_geometry_and_press_state_for_drawing() {
        let mut o = overlay((2400.0, 1080.0));
        let (cx, cy) = o.center("ls");
        o.touch(t(TouchKind::Down, 1, (cx, cy)));
        o.touch(t(TouchKind::Move, 1, (cx + 5000.0, cy)));
        let text = o.describe();
        let line = text.lines().find(|l| l.starts_with("ls|")).unwrap();
        let fields: Vec<&str> = line.split('|').collect();
        assert_eq!(fields.len(), 10);
        assert_eq!((fields[2], fields[7]), ("s", "3")); // stik, terlihat + ditekan
        let travel = 0.36 * 1080.0 / 2.0 * STICK_TRAVEL;
        assert_eq!(fields[8], format!("{travel:.1}"));
        assert_eq!(text.lines().count(), default_controls(Profile::Gamepad).len());
    }
}
