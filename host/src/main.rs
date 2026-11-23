//! Baris perintah host streaming CloudPC.
//!
//! `run` menjalankan host. Perintah lain (`pin`, `clients`, `unpair`, `stop`) hanya menyentuh folder state dan
//! dipakai agent CloudPC untuk mengatur host yang sedang berjalan; host memantau folder itu.

use std::net::IpAddr;
use std::path::PathBuf;
use std::time::Duration;

use cloudpc_stream_host::nvhttp::{self, HostConfig};
use cloudpc_stream_host::state::{self, Store};
use cloudpc_stream_proto::ports::{Ports, BASE_PORT};
use serde_json::json;

const USAGE: &str = "Perintah:
  cloudpc-stream-host run --state <folder> [--base-port N] [--bind ALAMAT] [--name NAMA] [--uniqueid ID] [--log-file FILE]
  cloudpc-stream-host supervise --state <folder> [opsi run]   jalankan + awasi host; sebagai SYSTEM host masuk ke sesi pemain
  cloudpc-stream-host pin <4 angka> --state <folder> [--name NAMA_PERANGKAT]   titipkan PIN sekali pakai
  cloudpc-stream-host pin --clear --state <folder>                             buang PIN yang belum terpakai
  cloudpc-stream-host clients --state <folder>                                 daftar perangkat terpairing
  cloudpc-stream-host unpair --state <folder> (--name NAMA | --prefix AWALAN)  cabut perangkat (sesinya ikut diakhiri)
  cloudpc-stream-host stop --state <folder>                                    akhiri sesi stream yang berjalan
  cloudpc-stream-host check-audio
  cloudpc-stream-host mic-check [--port COM7] [--tone DETIK]     cari perangkat CloudPC Mic, baca laporannya; --tone = kirim nada 440 Hz
  cloudpc-stream-host mic-bootloader [--port COM7]               perangkat CloudPC Mic masuk mode isi firmware (drive RPI-RP2)
  cloudpc-stream-host gpu-profile [--remove | --show PROFIL EXE]   profil driver NVIDIA \"prefer maximum performance\" untuk host
  cloudpc-stream-host ports [port_dasar]
  cloudpc-stream-host --version
Tambahkan --json pada pin/clients/unpair/stop untuk keluaran yang dibaca program.
Bawaan --bind 127.0.0.1 (hanya PC ini). run/supervise memasang profil driver NVIDIA host (seperti Sunshine) kecuali --no-gpu-profile.
run --mic pico[:COM7]: terima suara mikrofon pemain dan keluarkan lewat perangkat CloudPC Mic (mikrofon USB);
tanpa --mic host tidak menawarkan mikrofon ke client. --mic wav:<berkas> = tulis ke berkas (uji).
Hanya untuk uji (perintah run): --test-drop-video N [--test-drop-burst B], --test-drop-audio N,
--test-silent-audio dan --test-record-input (keduanya WAJIB bila client berjalan di mesin yang sama).";

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(2);
}

/// Ambil nilai `--nama nilai` dari argumen.
fn option(args: &[String], name: &str) -> Option<String> {
    let at = args.iter().position(|a| a == name)?;
    match args.get(at + 1) {
        Some(value) => Some(value.clone()),
        None => fail(&format!("{name} butuh nilai")),
    }
}

fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn number(args: &[String], name: &str, default: u32) -> u32 {
    match option(args, name) {
        None => default,
        Some(text) => text.parse().unwrap_or_else(|_| fail(&format!("{name} butuh angka"))),
    }
}

fn parse_base_port(text: Option<String>) -> u16 {
    match text {
        None => BASE_PORT,
        Some(text) => text.parse().unwrap_or_else(|_| fail(&format!("Port dasar tidak valid: {text}"))),
    }
}

/// Folder state untuk perintah pengatur: harus sudah ada (dibuat oleh `run`), supaya salah ketik tidak membuat
/// identitas host baru di tempat lain.
fn existing_state_dir(args: &[String]) -> PathBuf {
    let dir = state_dir(args);
    if !dir.is_dir() {
        fail(&format!("Folder state tidak ada: {}", dir.display()));
    }
    dir
}

fn state_dir(args: &[String]) -> PathBuf {
    PathBuf::from(option(args, "--state").unwrap_or_else(|| fail("--state <folder> wajib diisi")))
}

/// Alihkan stdout dan stderr proses ini ke file (ditambahkan di akhir). Dipakai saat berjalan tanpa konsol
/// (sebagai SYSTEM). File di atas 5 MB disisihkan dulu ke `.old`.
fn redirect_output(path: &std::path::Path) {
    if std::fs::metadata(path).is_ok_and(|m| m.len() > 5 * 1024 * 1024) {
        let _ = std::fs::rename(path, path.with_extension("log.old"));
    }
    let Ok(file) = std::fs::OpenOptions::new().create(true).append(true).open(path) else { return };
    #[cfg(windows)]
    {
        use std::os::windows::io::IntoRawHandle;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::Console::{SetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE};
        // Pegangan sengaja tidak pernah ditutup: dipakai sampai proses selesai.
        let handle = HANDLE(file.into_raw_handle());
        // SAFETY: pegangan file yang sah dan tetap terbuka seumur proses.
        unsafe {
            let _ = SetStdHandle(STD_OUTPUT_HANDLE, handle);
            let _ = SetStdHandle(STD_ERROR_HANDLE, handle);
        }
    }
    #[cfg(not(windows))]
    drop(file);
}

/// Pasang profil driver NVIDIA "prefer maximum performance" untuk program ini (lihat `nvprefs`). Tidak fatal: tanpa hak
/// Administrator/SYSTEM atau tanpa GPU NVIDIA host tetap jalan, hanya GPU bisa tetap di clock rendah.
#[cfg(windows)]
fn apply_gpu_profile(args: &[String]) {
    use cloudpc_stream_host::nvprefs;
    if flag(args, "--no-gpu-profile") {
        return;
    }
    let exe = nvprefs::current_exe_name();
    match nvprefs::ensure(&exe) {
        Ok(nvprefs::Outcome::AlreadySet) => println!("Profil driver NVIDIA {} untuk {exe}: sudah \"prefer maximum performance\".", nvprefs::PROFILE_NAME),
        Ok(nvprefs::Outcome::Changed) => println!("Profil driver NVIDIA {} untuk {exe} dipasang: \"prefer maximum performance\" (berlaku untuk proses host berikutnya).", nvprefs::PROFILE_NAME),
        Err(why) => println!("Profil driver NVIDIA tidak dipasang ({why}); GPU bisa tetap di clock rendah selama stream ringan."),
    }
}

#[cfg(not(windows))]
fn apply_gpu_profile(_args: &[String]) {}

/// Cetak hasil perintah pengatur: JSON satu baris dengan `--json`, kalimat tanpa itu.
fn report(args: &[String], machine: serde_json::Value, human: String) {
    if flag(args, "--json") {
        println!("{machine}");
    } else {
        println!("{human}");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") => println!("cloudpc-stream-host {}", env!("CARGO_PKG_VERSION")),
        Some("ports") => {
            let base = parse_base_port(args.get(1).cloned());
            let Some(p) = Ports::from_base(base) else { fail(&format!("Port dasar {base} membuat port lain keluar rentang")) };
            println!("TCP {} (HTTPS), {} (HTTP), {} (RTSP)", p.https, p.http, p.rtsp);
            println!("UDP {} (video), {} (kontrol), {} (audio)", p.video, p.control, p.audio);
        }
        Some("pin") => {
            let dir = existing_state_dir(&args);
            if flag(&args, "--clear") {
                let cleared = u8::from(state::clear_pin(&dir));
                return report(&args, json!({ "cleared": cleared }), format!("PIN yang menunggu dibuang: {cleared}."));
            }
            let pin = args.get(1).cloned().unwrap_or_default();
            let name = option(&args, "--name").unwrap_or_else(|| "perangkat".into());
            if let Err(e) = state::write_pin(&dir, &pin, &name) {
                fail(&e);
            }
            report(&args, json!({ "ok": true }), format!("PIN dititipkan untuk \"{name}\" (sekali pakai, berlaku {} detik).", state::PIN_MAX_AGE_SECS));
        }
        Some("clients") => {
            let store = Store::load(&existing_state_dir(&args)).unwrap_or_else(|e| fail(&e));
            let clients: Vec<_> = store.clients().iter().map(|c| json!({ "name": c.name, "paired_at": c.paired_at })).collect();
            let human = match store.clients() {
                [] => "Belum ada perangkat terpairing.".to_string(),
                list => list.iter().map(|c| format!("{} (sejak {})", c.name, c.paired_at)).collect::<Vec<_>>().join("\n"),
            };
            report(&args, json!({ "clients": clients }), human);
        }
        Some("unpair") => {
            let (name, prefix) = (option(&args, "--name"), option(&args, "--prefix"));
            if name.is_some() == prefix.is_some() {
                fail("unpair butuh tepat satu dari --name NAMA atau --prefix AWALAN");
            }
            if prefix.as_deref() == Some("") {
                fail("--prefix kosong akan mencabut semua perangkat; pakai awalan yang jelas");
            }
            let mut store = Store::load(&existing_state_dir(&args)).unwrap_or_else(|e| fail(&e));
            let removed = store
                .remove_clients(|candidate| match (&name, &prefix) {
                    (Some(name), _) => candidate == name,
                    (_, Some(prefix)) => candidate.starts_with(prefix.as_str()),
                    _ => false,
                })
                .unwrap_or_else(|e| fail(&e));
            report(&args, json!({ "removed": removed }), format!("{removed} perangkat dicabut."));
        }
        Some("stop") => {
            state::request_stop(&existing_state_dir(&args)).unwrap_or_else(|e| fail(&e));
            report(&args, json!({ "ok": true }), "Permintaan mengakhiri sesi dititipkan.".into());
        }
        Some("check-audio") => match cloudpc_stream_host::audio::check_capture(Duration::from_secs(2)) {
            Ok((0, _)) => println!("Tangkapan suara terbuka, tetapi tidak ada yang sedang berbunyi (0 sampel dalam 2 detik)."),
            Ok((samples, peak)) => println!("Tangkapan suara OK: {samples} sampel dalam 2 detik, puncak {:.1}% dari maksimum.", peak * 100.0),
            Err(why) => fail(&format!("Tangkapan suara gagal: {why}")),
        },
        Some("mic-check") => {
            let seconds = u64::from(number(&args, "--tone", 0));
            if seconds > 0 {
                println!("Mengirim nada 440 Hz selama {seconds} detik. Rekam \"CloudPC Mic\" di Windows (Pengaturan > Suara > Input) untuk mendengarnya.");
            }
            match cloudpc_stream_host::mic::check_pico(option(&args, "--port").as_deref(), seconds, |line| println!("{line}")) {
                Ok(()) => println!("Perangkat CloudPC Mic menjawab dengan benar."),
                Err(why) => fail(&format!("Perangkat CloudPC Mic bermasalah: {why}")),
            }
        }
        Some("mic-bootloader") => {
            let result = cloudpc_stream_host::mic::PicoOutput::open(option(&args, "--port").as_deref()).and_then(|pico| pico.enter_bootloader());
            match result {
                Ok(()) => println!("Perintah terkirim. Perangkat muncul sebagai drive RPI-RP2; salin berkas .uf2 baru ke sana."),
                Err(why) => fail(&format!("Gagal: {why}")),
            }
        }
        #[cfg(windows)]
        Some("gpu-profile") => {
            use cloudpc_stream_host::nvprefs;
            if flag(&args, "--remove") {
                match nvprefs::remove() {
                    Ok(true) => println!("Profil {} dihapus.", nvprefs::PROFILE_NAME),
                    Ok(false) => println!("Profil {} tidak ada.", nvprefs::PROFILE_NAME),
                    Err(why) => fail(&why),
                }
            } else if let Some(name) = option(&args, "--show") {
                let exe = args.iter().skip_while(|a| *a != "--show").nth(2).cloned().unwrap_or_else(nvprefs::current_exe_name);
                match nvprefs::inspect(&name, &exe) {
                    Ok(None) => println!("Profil {name} tidak ada."),
                    Ok(Some((has_exe, pstate))) => println!("Profil {name}: memuat {exe}: {has_exe}; PREFERRED_PSTATE milik profil: {pstate:?} (1 = prefer maximum performance)"),
                    Err(why) => fail(&why),
                }
            } else {
                let exe = nvprefs::current_exe_name();
                match nvprefs::ensure(&exe) {
                    Ok(outcome) => println!("Profil {} untuk {exe}: {outcome:?}", nvprefs::PROFILE_NAME),
                    Err(why) => fail(&why),
                }
            }
        }
        #[cfg(windows)]
        Some("supervise") => {
            // Semua argumen selain kata `supervise` diteruskan ke `run`; log pengawas dan host masuk ke folder state.
            let dir = state_dir(&args);
            let mut run_args: Vec<String> = args[1..].to_vec();
            if option(&args, "--log-file").is_none() {
                run_args.extend(["--log-file".to_string(), dir.join("host.log").to_string_lossy().into_owned()]);
            }
            if std::fs::create_dir_all(&dir).is_ok() {
                redirect_output(&dir.join("supervisor.log"));
            }
            // Sebelum host pertama dijalankan, supaya host memuat driver grafis dengan profil ini.
            apply_gpu_profile(&args);
            if let Err(why) = cloudpc_stream_host::supervise::supervise(&dir, &run_args) {
                fail(&why);
            }
        }
        Some("run") => {
            if let Some(path) = option(&args, "--log-file") {
                redirect_output(std::path::Path::new(&path));
            }
            #[cfg(windows)]
            if let Some(pid) = option(&args, "--parent-pid") {
                cloudpc_stream_host::supervise::exit_when_parent_dies(pid.parse().unwrap_or_else(|_| fail("--parent-pid butuh angka")));
            }
            // Sebelum device D3D pertama (pemeriksaan codec di `nvhttp::start`). Di bawah `supervise` profil sudah dipasang.
            if option(&args, "--parent-pid").is_none() {
                apply_gpu_profile(&args);
            }
            let bind = option(&args, "--bind").unwrap_or_else(|| "127.0.0.1".into());
            let bind: IpAddr = bind.parse().unwrap_or_else(|_| fail(&format!("Alamat --bind tidak valid: {bind}")));
            let cfg = HostConfig {
                bind,
                base_port: parse_base_port(option(&args, "--base-port")),
                name: option(&args, "--name").unwrap_or_else(|| "CloudPC".into()),
                state_dir: state_dir(&args),
                uniqueid: option(&args, "--uniqueid"),
                pin_wait: Duration::from_secs(state::PIN_MAX_AGE_SECS),
                verbose: true,
                test_drop_video_every: number(&args, "--test-drop-video", 0),
                test_drop_video_burst: number(&args, "--test-drop-burst", 1),
                test_silent_audio: flag(&args, "--test-silent-audio"),
                test_drop_audio_every: number(&args, "--test-drop-audio", 0),
                test_record_input: flag(&args, "--test-record-input"),
                test_rumble: flag(&args, "--test-rumble"),
                mic: option(&args, "--mic").map(|text| cloudpc_stream_host::mic::MicTarget::parse(&text).unwrap_or_else(|e| fail(&e))),
            };
            let host = nvhttp::start(cfg).unwrap_or_else(|e| fail(&e));
            let p = host.ports;
            println!("cloudpc-stream-host (pid {}) berjalan di {bind}: HTTP {}, HTTPS {}, RTSP {}, kontrol UDP {} (uniqueid {})", std::process::id(), p.http, p.https, p.rtsp, p.control, host.uniqueid());
            println!("Perangkat terpairing: {}. Video, audio, dan input aktif.", host.paired_names().len());
            loop {
                std::thread::park();
            }
        }
        _ => fail(USAGE),
    }
}
