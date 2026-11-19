//! Menautkan FFmpeg 8.0 (libavcodec + libavutil, DLL) untuk decode video di Windows (fase C3).
//!
//! Folder FFmpeg: variabel `FFMPEG_DIR` (berisi `lib\avcodec.lib` dan `bin\avcodec-62.dll`), atau paket winget
//! `BtbN.FFmpeg.GPL.Shared.8.0` bila terpasang. DLL disalin ke folder target supaya `cargo run`/`cargo test` jalan.
//! Binding Rust sudah di-commit (`src/video/ffi.rs`), jadi build biasa tidak butuh libclang.

use std::path::{Path, PathBuf};

/// avcodec-62 juga mengimpor swresample-6 (build BtbN).
const DLLS: [&str; 3] = ["avcodec-62.dll", "avutil-60.dll", "swresample-6.dll"];

fn main() {
    println!("cargo:rerun-if-env-changed=FFMPEG_DIR");
    println!("cargo:rerun-if-changed=build.rs");
    match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("windows") => {
            // Ikon CloudPC di exe (Explorer, taskbar) dan di jendela stream (`render.rs` memuat ikon ID 1).
            println!("cargo:rerun-if-changed=assets/cloudpc-stream.rc");
            println!("cargo:rerun-if-changed=assets/cloudpc.ico");
            embed_resource::compile("assets/cloudpc-stream.rc", embed_resource::NONE).manifest_optional().expect("resource ikon");
        }
        Ok("macos") => return link_macos(),
        _ => return,
    }
    let dir = find_ffmpeg().unwrap_or_else(|| {
        panic!(
            "FFmpeg 8.0 (shared) tidak ditemukan. Pasang: winget install --id BtbN.FFmpeg.GPL.Shared.8.0, \
             atau set FFMPEG_DIR ke folder yang berisi lib\\avcodec.lib dan bin\\avcodec-62.dll."
        )
    });
    println!("cargo:rustc-link-search=native={}", dir.join("lib").display());
    println!("cargo:rustc-link-lib=dylib=avcodec");
    println!("cargo:rustc-link-lib=dylib=avutil");

    // SDL3 (stik PlayStation/Switch, opsional): folder `SDL3_DIR`, atau target/deps/SDL3-* di workspace.
    println!("cargo:rerun-if-env-changed=SDL3_DIR");
    let sdl3 = find_sdl3();

    // OUT_DIR = target/<profil>/build/<paket>/out -> target/<profil>.
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    if let Some(profile) = out.ancestors().nth(3) {
        for target in [profile.to_path_buf(), profile.join("deps"), profile.join("examples")] {
            let _ = std::fs::create_dir_all(&target);
            for dll in DLLS {
                let to = target.join(dll);
                if !to.exists() {
                    let _ = std::fs::copy(dir.join("bin").join(dll), to);
                }
            }
            if let Some(sdl3) = &sdl3 {
                let to = target.join("SDL3.dll");
                if !to.exists() {
                    let _ = std::fs::copy(sdl3, to);
                }
            }
        }
    }
}

/// macOS: FFmpeg dari `FFMPEG_DIR` (build ramping CI, nama instal `@rpath/...`) atau Homebrew. Program mencari dylib di
/// foldernya sendiri (bundel) lalu di folder FFmpeg saat build (untuk `cargo test`/`cargo run`).
fn link_macos() {
    let dir = std::env::var("FFMPEG_DIR").map(PathBuf::from).ok().or_else(|| {
        ["/opt/homebrew/opt/ffmpeg", "/usr/local/opt/ffmpeg"].into_iter().map(PathBuf::from).find(|p| p.join("lib").join("libavcodec.dylib").exists())
    });
    let dir = dir.unwrap_or_else(|| panic!("FFmpeg 8.0 tidak ditemukan: set FFMPEG_DIR ke folder berisi lib/libavcodec.dylib (atau brew install ffmpeg)"));
    let lib = dir.join("lib");
    println!("cargo:rustc-link-search=native={}", lib.display());
    println!("cargo:rustc-link-lib=dylib=avcodec");
    println!("cargo:rustc-link-lib=dylib=avutil");
    println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib.display());
}

fn find_sdl3() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("SDL3_DIR") {
        let dll = PathBuf::from(dir).join("SDL3.dll");
        return dll.exists().then_some(dll);
    }
    let deps = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").ok()?).parent()?.join("target").join("deps");
    std::fs::read_dir(deps).ok()?.flatten().map(|e| e.path().join("SDL3.dll")).find(|p| p.exists())
}

fn find_ffmpeg() -> Option<PathBuf> {
    let valid = |p: &Path| p.join("lib").join("avcodec.lib").exists() && p.join("bin").join(DLLS[0]).exists();
    if let Ok(dir) = std::env::var("FFMPEG_DIR") {
        let dir = PathBuf::from(dir);
        return valid(&dir).then_some(dir);
    }
    let packages = PathBuf::from(std::env::var("LOCALAPPDATA").ok()?).join("Microsoft").join("WinGet").join("Packages");
    for package in std::fs::read_dir(packages).ok()?.flatten() {
        if !package.file_name().to_string_lossy().starts_with("BtbN.FFmpeg.GPL.Shared.8.0") {
            continue;
        }
        for inner in std::fs::read_dir(package.path()).ok()?.flatten() {
            if valid(&inner.path()) {
                return Some(inner.path());
            }
        }
    }
    None
}
