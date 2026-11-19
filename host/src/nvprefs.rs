//! Profil driver NVIDIA untuk program host: "prefer maximum performance" (PREFERRED_PSTATE = PREFER_MAX).
//!
//! Tanpa ini driver membiarkan GPU di clock rendah selama stream desktop ringan (terukur di PC uji 19 Nov 2026: ±450 MHz,
//! Sunshine 2490 MHz), sehingga tangkap + encode lebih lambat (latensi host 3,1 vs 1,7 ms). Sunshine melakukan hal yang sama
//! untuk sunshine.exe (`src/platform/windows/nvprefs/driver_settings.cpp`, profil "SunshineStream"). Seperti Sunshine:
//! hanya profil aplikasi milik program ini yang dibuat/diubah (pengaturan global driver tidak disentuh), profil dibiarkan
//! terpasang, dan menyimpan butuh hak Administrator/SYSTEM. Driver membaca profil saat proses memuat driver grafis, jadi
//! panggil sebelum device D3D pertama dibuat; profil yang baru dibuat berlaku untuk proses host berikutnya.
//!
//! NVAPI dimuat dari System32 (`nvapi64.dll`) lewat `nvapi_QueryInterface`; nomor fungsi dan tata letak struktur dari
//! header NVAPI R5xx publik NVIDIA.

use std::ffi::c_void;

use libloading::Library;

/// Nama profil driver yang dibuat untuk host.
pub const PROFILE_NAME: &str = "CloudPCStreamHost";
const PREFERRED_PSTATE_ID: u32 = 0x1057_EB71;
const PREFERRED_PSTATE_PREFER_MAX: u32 = 1;

const UNICODE_MAX: usize = 2048;
type UnicodeString = [u16; UNICODE_MAX];
type Handle = *mut c_void;
type Status = i32;
const NVAPI_OK: Status = 0;

#[repr(C)]
struct Profile {
    version: u32,
    profile_name: UnicodeString,
    gpu_support: u32,
    is_predefined: u32,
    num_of_apps: u32,
    num_of_settings: u32,
}

#[repr(C)]
struct Application {
    version: u32,
    is_predefined: u32,
    app_name: UnicodeString,
    user_friendly_name: UnicodeString,
    launcher: UnicodeString,
}

/// `NVDRS_SETTING_V1`; kedua nilai (bawaan dan sekarang) berupa union selebar `NVDRS_BINARY_SETTING` (4 + 4096 byte).
#[repr(C)]
struct Setting {
    version: u32,
    setting_name: UnicodeString,
    setting_id: u32,
    setting_type: u32,
    setting_location: u32,
    is_current_predefined: u32,
    is_predefined_valid: u32,
    predefined_value: [u32; 1025],
    current_value: [u32; 1025],
}

const fn version<T>(v: u32) -> u32 {
    std::mem::size_of::<T>() as u32 | (v << 16)
}

fn wide(text: &str) -> UnicodeString {
    let mut out = [0u16; UNICODE_MAX];
    for (slot, unit) in out.iter_mut().zip(text.encode_utf16().take(UNICODE_MAX - 1)) {
        *slot = unit;
    }
    out
}

fn zeroed<T>() -> Box<T> {
    // SAFETY: semua struktur NVAPI di sini hanya berisi bilangan bulat dan larik bilangan bulat; nol adalah nilai sah.
    unsafe { Box::new(std::mem::zeroed()) }
}

/// Hasil `ensure`: apa yang dilakukan.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Profil sudah benar; tidak ada yang diubah.
    AlreadySet,
    /// Profil/aplikasi/pengaturan dibuat atau diperbaiki dan disimpan.
    Changed,
}

type Query = unsafe extern "C" fn(u32) -> *mut c_void;

struct Api {
    _lib: Library,
    query: Query,
}

impl Api {
    fn load() -> Result<Api, String> {
        // SAFETY: nvapi64.dll dari System32 milik driver NVIDIA; ekspor `nvapi_QueryInterface` bertanda tangan seperti ini.
        unsafe {
            let lib = Library::new(system32("nvapi64.dll")).map_err(|e| format!("nvapi64.dll tidak bisa dimuat (bukan GPU NVIDIA?): {e}"))?;
            let query: Query = *lib.get::<Query>(b"nvapi_QueryInterface\0").map_err(|e| format!("nvapi_QueryInterface: {e}"))?;
            Ok(Api { _lib: lib, query })
        }
    }

    /// Penunjuk fungsi untuk nomor antarmuka `id`, ditafsirkan sebagai `F`.
    ///
    /// # Safety
    /// `F` harus tanda tangan yang benar untuk `id` menurut header NVAPI.
    unsafe fn func<F: Copy>(&self, id: u32, name: &str) -> Result<F, String> {
        let p = (self.query)(id);
        if p.is_null() {
            return Err(format!("NVAPI tidak punya {name}"));
        }
        Ok(std::mem::transmute_copy::<*mut c_void, F>(&p))
    }
}

fn system32(name: &str) -> std::path::PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    std::path::Path::new(&root).join("System32").join(name)
}

/// Sesi DRS terbuka (dengan pengaturan driver termuat); ditutup saat dilepas.
struct Session {
    api: Api,
    handle: Handle,
}

impl Session {
    fn open() -> Result<Session, String> {
        let api = Api::load()?;
        // SAFETY: tanda tangan sesuai header NVAPI; semua hasil diperiksa.
        unsafe {
            let init: unsafe extern "C" fn() -> Status = api.func(0x0150_E828, "NvAPI_Initialize")?;
            check(&api, init(), "NvAPI_Initialize")?;
            let create: unsafe extern "C" fn(*mut Handle) -> Status = api.func(0x0694_D52E, "NvAPI_DRS_CreateSession")?;
            let mut handle: Handle = std::ptr::null_mut();
            check(&api, create(&mut handle), "NvAPI_DRS_CreateSession")?;
            let session = Session { api, handle };
            let load: unsafe extern "C" fn(Handle) -> Status = session.api.func(0x375D_BD6B, "NvAPI_DRS_LoadSettings")?;
            check(&session.api, load(session.handle), "NvAPI_DRS_LoadSettings")?;
            Ok(session)
        }
    }

    fn find_profile(&self, name: &str) -> Result<Option<Handle>, String> {
        // SAFETY: lihat `open`.
        unsafe {
            let find: unsafe extern "C" fn(Handle, *const u16, *mut Handle) -> Status = self.api.func(0x7E4A_9A0B, "NvAPI_DRS_FindProfileByName")?;
            let name = wide(name);
            let mut profile: Handle = std::ptr::null_mut();
            Ok((find(self.handle, name.as_ptr(), &mut profile) == NVAPI_OK && !profile.is_null()).then_some(profile))
        }
    }

    /// Nilai DWORD `setting_id` yang ditetapkan LANGSUNG di profil (bukan warisan/bawaan), bila ada.
    fn own_dword(&self, profile: Handle, setting_id: u32) -> Result<Option<u32>, String> {
        // SAFETY: lihat `open`; `Setting` cukup besar untuk struktur V1.
        unsafe {
            let get: unsafe extern "C" fn(Handle, Handle, u32, *mut Setting) -> Status = self.api.func(0x73BF_8338, "NvAPI_DRS_GetSetting")?;
            let mut setting = zeroed::<Setting>();
            setting.version = version::<Setting>(1);
            if get(self.handle, profile, setting_id, &mut *setting) != NVAPI_OK || setting.setting_location != 0 {
                return Ok(None);
            }
            Ok(Some(setting.current_value[0]))
        }
    }

    fn has_application(&self, profile: Handle, exe: &str) -> Result<bool, String> {
        // SAFETY: lihat `open`.
        unsafe {
            let info: unsafe extern "C" fn(Handle, Handle, *const u16, *mut Application) -> Status = self.api.func(0xED1F_8C69, "NvAPI_DRS_GetApplicationInfo")?;
            let mut app = zeroed::<Application>();
            app.version = version::<Application>(1);
            let name = wide(exe);
            Ok(info(self.handle, profile, name.as_ptr(), &mut *app) == NVAPI_OK)
        }
    }

    fn save(&self) -> Result<(), String> {
        // SAFETY: lihat `open`.
        unsafe {
            let save: unsafe extern "C" fn(Handle) -> Status = self.api.func(0xFCBC_7E14, "NvAPI_DRS_SaveSettings")?;
            check(&self.api, save(self.handle), "NvAPI_DRS_SaveSettings (butuh hak Administrator/SYSTEM)")
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: sesi dibuat oleh `open` dan dihancurkan tepat sekali.
        unsafe {
            if let Ok(destroy) = self.api.func::<unsafe extern "C" fn(Handle) -> Status>(0xDAD9_CFF8, "NvAPI_DRS_DestroySession") {
                destroy(self.handle);
            }
        }
    }
}

fn check(api: &Api, status: Status, what: &str) -> Result<(), String> {
    if status == NVAPI_OK {
        return Ok(());
    }
    let mut message = [0u8; 64];
    // SAFETY: NvAPI_ShortString = char[64].
    let text = unsafe {
        match api.func::<unsafe extern "C" fn(Status, *mut u8) -> Status>(0x6C2D_048C, "NvAPI_GetErrorMessage") {
            Ok(get) if get(status, message.as_mut_ptr()) == NVAPI_OK => String::from_utf8_lossy(&message[..message.iter().position(|&b| b == 0).unwrap_or(64)]).into_owned(),
            _ => String::new(),
        }
    };
    Err(format!("{what} gagal: {status} {text}"))
}

/// Nama berkas program host yang sedang berjalan (yang dicocokkan driver), mis. `cloudpc-stream-host.exe`.
pub fn current_exe_name() -> String {
    std::env::current_exe().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_else(|| "cloudpc-stream-host.exe".into())
}

/// Pastikan profil `PROFILE_NAME` ada, memuat `exe`, dan menetapkan PREFERRED_PSTATE = PREFER_MAX.
pub fn ensure(exe: &str) -> Result<Outcome, String> {
    let session = Session::open()?;
    let mut changed = false;
    // SAFETY: tanda tangan sesuai header NVAPI; struktur berversi V1 dan cukup besar.
    unsafe {
        let profile = match session.find_profile(PROFILE_NAME)? {
            Some(p) => p,
            None => {
                let create: unsafe extern "C" fn(Handle, *mut Profile, *mut Handle) -> Status = session.api.func(0xCC17_6068, "NvAPI_DRS_CreateProfile")?;
                let mut desc = zeroed::<Profile>();
                desc.version = version::<Profile>(1);
                desc.profile_name = wide(PROFILE_NAME);
                let mut handle: Handle = std::ptr::null_mut();
                check(&session.api, create(session.handle, &mut *desc, &mut handle), "NvAPI_DRS_CreateProfile")?;
                changed = true;
                handle
            }
        };
        if !session.has_application(profile, exe)? {
            let create: unsafe extern "C" fn(Handle, Handle, *mut Application) -> Status = session.api.func(0x4347_A9DE, "NvAPI_DRS_CreateApplication")?;
            let mut app = zeroed::<Application>();
            app.version = version::<Application>(1);
            app.app_name = wide(exe);
            app.user_friendly_name = wide(exe);
            check(&session.api, create(session.handle, profile, &mut *app), "NvAPI_DRS_CreateApplication")?;
            changed = true;
        }
        if session.own_dword(profile, PREFERRED_PSTATE_ID)? != Some(PREFERRED_PSTATE_PREFER_MAX) {
            let set: unsafe extern "C" fn(Handle, Handle, *mut Setting) -> Status = session.api.func(0x577D_D202, "NvAPI_DRS_SetSetting")?;
            let mut setting = zeroed::<Setting>();
            setting.version = version::<Setting>(1);
            setting.setting_id = PREFERRED_PSTATE_ID;
            setting.setting_type = 0; // NVDRS_DWORD_TYPE
            setting.setting_location = 0; // NVDRS_CURRENT_PROFILE_LOCATION
            setting.current_value[0] = PREFERRED_PSTATE_PREFER_MAX;
            check(&session.api, set(session.handle, profile, &mut *setting), "NvAPI_DRS_SetSetting PREFERRED_PSTATE")?;
            changed = true;
        }
    }
    if !changed {
        return Ok(Outcome::AlreadySet);
    }
    session.save()?;
    Ok(Outcome::Changed)
}

/// Hapus profil `PROFILE_NAME` (bila ada). `true` = ada dan dihapus.
pub fn remove() -> Result<bool, String> {
    let session = Session::open()?;
    let Some(profile) = session.find_profile(PROFILE_NAME)? else { return Ok(false) };
    // SAFETY: lihat `ensure`.
    unsafe {
        let delete: unsafe extern "C" fn(Handle, Handle) -> Status = session.api.func(0x1709_3206, "NvAPI_DRS_DeleteProfile")?;
        check(&session.api, delete(session.handle, profile), "NvAPI_DRS_DeleteProfile")?;
    }
    session.save()?;
    Ok(true)
}

/// Keadaan profil bernama `name`: `None` = tidak ada; `Some((punya exe, nilai PREFERRED_PSTATE milik profil))`.
pub fn inspect(name: &str, exe: &str) -> Result<Option<(bool, Option<u32>)>, String> {
    let session = Session::open()?;
    let Some(profile) = session.find_profile(name)? else { return Ok(None) };
    Ok(Some((session.has_application(profile, exe)?, session.own_dword(profile, PREFERRED_PSTATE_ID)?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn struct_sizes_match_the_nvapi_header() {
        // NVDRS_SETTING_V1 = 12320, NVDRS_PROFILE_V1 = 4116, NVDRS_APPLICATION_V1 = 12296 byte (header NVAPI R5xx, x64).
        assert_eq!(std::mem::size_of::<Setting>(), 12320);
        assert_eq!(std::mem::size_of::<Profile>(), 4116);
        assert_eq!(std::mem::size_of::<Application>(), 12296);
        assert_eq!(version::<Setting>(1), 0x0001_3020);
    }

    #[test]
    fn names_are_null_terminated_utf16() {
        let w = wide("cloudpc-stream-host.exe");
        assert_eq!(w[0], u16::from(b'c'));
        assert_eq!(w["cloudpc-stream-host.exe".len()], 0);
    }
}
