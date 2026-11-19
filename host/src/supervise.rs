//! Fase H5b. Peluncur host di PC sungguhan.
//!
//! Tangkap layar, input, dan suara hanya jalan di sesi desktop pemain, tetapi host tidak boleh berjalan SEBAGAI
//! akun pemain: pemain bisa membaca kunci host, menitipkan PIN sendiri, atau mematikannya. Jalan keluarnya sama
//! dengan layanan Sunshine: `supervise` berjalan sebagai SYSTEM di sesi 0 (tugas terjadwal saat boot atau dari
//! agent), lalu menjalankan `run` dengan token SYSTEM yang dipindahkan ke sesi konsol tempat pemain login.
//! Pengawas menjalankan ulang host bila berhenti atau bila sesi konsol berganti (logoff, login, restart).
//!
//! Bila TIDAK dijalankan sebagai SYSTEM (uji di PC pengembang), host dijalankan apa adanya di sesi dan akun
//! pemanggil, dengan peringatan.

use std::ffi::c_void;
use std::path::Path;
use std::time::{Duration, Instant};

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, GENERIC_ALL, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Security::{DuplicateTokenEx, GetTokenInformation, IsWellKnownSid, SecurityImpersonation, SetTokenInformation, TokenPrimary, TokenSessionId, TokenUser, WinLocalSystemSid, TOKEN_ALL_ACCESS, TOKEN_QUERY, TOKEN_USER};
use windows::Win32::System::RemoteDesktop::{WTSFreeMemory, WTSGetActiveConsoleSessionId, WTSQuerySessionInformationW, WTSUserName};
use windows::Win32::System::StationsAndDesktops::{CloseDesktop, OpenInputDesktop, SetThreadDesktop, DESKTOP_ACCESS_FLAGS, DESKTOP_CONTROL_FLAGS};
use windows::Win32::System::Threading::{
    CreateProcessAsUserW, GetCurrentProcess, OpenProcess, OpenProcessToken, TerminateProcess, WaitForSingleObject, CREATE_NO_WINDOW, INFINITE, PROCESS_INFORMATION, PROCESS_SYNCHRONIZE, STARTUPINFOW,
};

/// Host yang bertahan lebih lama dari ini dianggap sehat; jeda mulai ulang kembali ke awal.
const HEALTHY_AFTER: Duration = Duration::from_secs(30);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Pindahkan thread ini ke desktop yang sedang menerima input (desktop biasa, layar kunci, atau UAC).
/// Hanya berhasil untuk proses SYSTEM; untuk proses biasa tidak mengubah apa pun.
pub fn sync_thread_desktop() {
    // SAFETY: pegangan desktop ditutup setelah dipasang; gagal di salah satu langkah berarti tidak ada perubahan.
    unsafe {
        if let Ok(desktop) = OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_ACCESS_FLAGS(GENERIC_ALL.0)) {
            let _ = SetThreadDesktop(desktop);
            let _ = CloseDesktop(desktop);
        }
    }
}

/// Proses ini berjalan sebagai akun SYSTEM?
pub fn is_system() -> bool {
    // SAFETY: penyangga u64 cukup besar dan sejajar untuk TOKEN_USER + SID-nya.
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut buffer = [0u64; 32];
        let mut length = 0u32;
        let read = GetTokenInformation(token, TokenUser, Some(buffer.as_mut_ptr() as *mut c_void), std::mem::size_of_val(&buffer) as u32, &mut length).is_ok();
        let _ = CloseHandle(token);
        read && IsWellKnownSid((*(buffer.as_ptr() as *const TOKEN_USER)).User.Sid, WinLocalSystemSid).as_bool()
    }
}

/// Sesi konsol yang sedang aktif DAN sudah ada pengguna yang login di sana.
pub fn logged_on_console_session() -> Option<u32> {
    // SAFETY: penyangga dari WTS dibebaskan dengan WTSFreeMemory.
    unsafe {
        let session = WTSGetActiveConsoleSessionId();
        if session == u32::MAX {
            return None;
        }
        let mut name = PWSTR::null();
        let mut bytes = 0u32;
        WTSQuerySessionInformationW(None, session, WTSUserName, &mut name, &mut bytes).ok()?;
        let logged_on = !name.is_null() && *name.0 != 0;
        WTSFreeMemory(name.0 as *mut c_void);
        logged_on.then_some(session)
    }
}

/// Kutip satu argumen menurut aturan baris perintah Windows (`CommandLineToArgvW`).
pub fn quote_argument(argument: &str) -> String {
    if !argument.is_empty() && !argument.contains([' ', '\t', '"']) {
        return argument.to_string();
    }
    let mut out = String::from('"');
    let mut backslashes = 0;
    for c in argument.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                // Garis miring terbalik sebelum tanda kutip digandakan, lalu tanda kutipnya di-escape.
                out.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                backslashes = 0;
                out.push('"');
                continue;
            }
            _ => {
                out.extend(std::iter::repeat_n('\\', backslashes));
                backslashes = 0;
            }
        }
        if c != '\\' {
            out.push(c);
        }
    }
    // Garis miring terbalik di akhir digandakan supaya tidak meng-escape tanda kutip penutup.
    out.extend(std::iter::repeat_n('\\', backslashes * 2));
    out.push('"');
    out
}

enum Child {
    Plain(std::process::Child),
    InSession(HANDLE),
}

impl Child {
    fn exited(&mut self) -> bool {
        match self {
            Child::Plain(child) => !matches!(child.try_wait(), Ok(None)),
            // SAFETY: pegangan proses milik kita dan masih terbuka.
            Child::InSession(process) => unsafe { WaitForSingleObject(*process, 0) == WAIT_OBJECT_0 },
        }
    }

    fn kill(&mut self) {
        match self {
            Child::Plain(child) => {
                let _ = child.kill();
                let _ = child.wait();
            }
            // SAFETY: lihat `exited`.
            Child::InSession(process) => unsafe {
                let _ = TerminateProcess(*process, 1);
                WaitForSingleObject(*process, 5000);
            },
        }
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        if let Child::InSession(process) = self {
            // SAFETY: ditutup tepat sekali.
            unsafe {
                let _ = CloseHandle(*process);
            }
        }
    }
}

/// Jalankan `exe args...` dengan token SYSTEM proses ini yang dipindahkan ke `session`, di desktop interaktif.
fn spawn_in_session(session: u32, exe: &Path, args: &[String]) -> Result<Child, String> {
    let command_line = std::iter::once(exe.to_string_lossy().into_owned()).chain(args.iter().cloned()).map(|a| quote_argument(&a)).collect::<Vec<_>>().join(" ");
    let mut command_line: Vec<u16> = command_line.encode_utf16().chain([0]).collect();
    let mut desktop: Vec<u16> = "winsta0\\default".encode_utf16().chain([0]).collect();
    let describe = |what: &str, e: windows::core::Error| format!("{what}: {e}");
    // SAFETY: semua pegangan yang dibuka ditutup; penyangga UTF-16 hidup sampai CreateProcessAsUserW kembali.
    unsafe {
        let mut own = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_ALL_ACCESS, &mut own).map_err(|e| describe("OpenProcessToken", e))?;
        let mut primary = HANDLE::default();
        let duplicated = DuplicateTokenEx(own, TOKEN_ALL_ACCESS, None, SecurityImpersonation, TokenPrimary, &mut primary);
        let _ = CloseHandle(own);
        duplicated.map_err(|e| describe("DuplicateTokenEx", e))?;

        let result = (|| {
            // Butuh hak SeTcbPrivilege, yang hanya dimiliki SYSTEM.
            SetTokenInformation(primary, TokenSessionId, &session as *const u32 as *const c_void, 4).map_err(|e| describe("SetTokenInformation(sesi)", e))?;
            let startup = STARTUPINFOW { cb: std::mem::size_of::<STARTUPINFOW>() as u32, lpDesktop: PWSTR(desktop.as_mut_ptr()), ..Default::default() };
            let mut info = PROCESS_INFORMATION::default();
            CreateProcessAsUserW(Some(primary), PCWSTR::null(), Some(PWSTR(command_line.as_mut_ptr())), None, None, false, CREATE_NO_WINDOW, None, PCWSTR::null(), &startup, &mut info)
                .map_err(|e| describe("CreateProcessAsUserW", e))?;
            let _ = CloseHandle(info.hThread);
            Ok(Child::InSession(info.hProcess))
        })();
        let _ = CloseHandle(primary);
        result
    }
}

/// Kunci folder state supaya hanya SYSTEM dan Administrators yang bisa membacanya (kunci privat host ada di sana).
/// Memakai SID, jadi tidak bergantung bahasa Windows.
pub fn lock_state_dir(dir: &Path) -> Result<(), String> {
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    let output = std::process::Command::new(format!("{system_root}\\System32\\icacls.exe"))
        .arg(dir)
        .args(["/inheritance:r", "/grant:r", "*S-1-5-18:(OI)(CI)F", "*S-1-5-32-544:(OI)(CI)F"])
        .output()
        .map_err(|e| format!("icacls tidak bisa dijalankan: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!("icacls gagal: {}", String::from_utf8_lossy(&output.stdout).trim()))
    }
}

/// Host berhenti sendiri bila proses pengawasnya hilang, supaya tidak ada host yatim yang menahan port.
pub fn exit_when_parent_dies(parent_pid: u32) {
    std::thread::spawn(move || {
        // SAFETY: pegangan hanya dipakai untuk menunggu; proses ini keluar sesudahnya.
        unsafe {
            if let Ok(parent) = OpenProcess(PROCESS_SYNCHRONIZE, false, parent_pid) {
                WaitForSingleObject(parent, INFINITE);
            }
        }
        eprintln!("pengawas (pid {parent_pid}) sudah tidak ada: host berhenti");
        std::process::exit(0);
    });
}

/// Jalankan dan awasi host selamanya. `run_args`: argumen untuk `cloudpc-stream-host run`, tanpa kata `run`.
pub fn supervise(state_dir: &Path, run_args: &[String]) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("jalur program tidak diketahui: {e}"))?;
    let system = is_system();
    std::fs::create_dir_all(state_dir).map_err(|e| format!("gagal membuat folder state {}: {e}", state_dir.display()))?;
    if system {
        match lock_state_dir(state_dir) {
            Ok(()) => println!("pengawas: folder state dikunci untuk SYSTEM dan Administrators"),
            Err(why) => println!("pengawas: PERINGATAN, folder state tidak terkunci ({why})"),
        }
    } else {
        println!("pengawas: PERINGATAN, bukan SYSTEM. Host dijalankan di sesi dan akun ini (hanya untuk uji); folder state tidak dikunci.");
    }

    let mut args = vec!["run".to_string()];
    args.extend(run_args.iter().cloned());
    args.extend(["--parent-pid".to_string(), std::process::id().to_string()]);
    let mut backoff = Duration::from_secs(1);
    loop {
        let session = match (system, logged_on_console_session()) {
            (false, _) => None,
            (true, Some(session)) => Some(session),
            (true, None) => {
                // Belum ada yang login di konsol: tidak ada desktop untuk ditangkap.
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
        };
        let started = Instant::now();
        let spawned = match session {
            Some(session) => spawn_in_session(session, &exe, &args),
            None => std::process::Command::new(&exe).args(&args).spawn().map(Child::Plain).map_err(|e| format!("gagal menjalankan host: {e}")),
        };
        match spawned {
            Ok(mut child) => {
                println!("pengawas: host dijalankan{}", session.map(|s| format!(" di sesi {s}")).unwrap_or_default());
                loop {
                    std::thread::sleep(Duration::from_millis(500));
                    if child.exited() {
                        println!("pengawas: host berhenti setelah {} detik", started.elapsed().as_secs());
                        break;
                    }
                    if system && logged_on_console_session() != session {
                        println!("pengawas: sesi konsol berganti, host dihentikan untuk dijalankan ulang");
                        child.kill();
                        break;
                    }
                }
            }
            Err(why) => println!("pengawas: {why}"),
        }
        if started.elapsed() >= HEALTHY_AFTER {
            backoff = Duration::from_secs(1);
        }
        std::thread::sleep(backoff);
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_are_quoted_like_windows_expects() {
        assert_eq!(quote_argument("run"), "run");
        assert_eq!(quote_argument("C:\\CloudPC\\stream-host"), "C:\\CloudPC\\stream-host");
        assert_eq!(quote_argument(""), "\"\"");
        assert_eq!(quote_argument("CloudPC Rust"), "\"CloudPC Rust\"");
        assert_eq!(quote_argument("C:\\Program Files\\x\\"), "\"C:\\Program Files\\x\\\\\"", "garis miring di akhir digandakan");
        assert_eq!(quote_argument("a\"b"), "\"a\\\"b\"");
        assert_eq!(quote_argument("a\\\"b c"), "\"a\\\\\\\"b c\"");
    }

    #[test]
    fn a_normal_test_process_is_not_system_and_has_a_console_user() {
        // Tes dijalankan pengembang yang sedang login, bukan oleh SYSTEM.
        assert!(!is_system());
        assert!(logged_on_console_session().is_some());
        // Untuk proses biasa ini tidak mengubah apa pun dan tidak boleh gagal.
        sync_thread_desktop();
    }
}
