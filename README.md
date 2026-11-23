# CloudPC Stream (Rust)

Host dan client streaming game/desktop yang ditulis ulang dalam Rust, **kompatibel dengan protokol GameStream**
(Sunshine / Moonlight). Host bisa dipakai dengan client Moonlight biasa, dan client bisa dipakai dengan host Sunshine.

*English: a Rust re-implementation of a GameStream host (Sunshine-compatible) and client (Moonlight-compatible) for
Windows, with a macOS client. Licensed GPL-3.0. Built for CloudPC, a cloud gaming service in Indonesia.*

## Isi

| Crate | Fungsi |
|---|---|
| `proto` | Format kawat GameStream: RTP, paket video + FEC, kanal kontrol, input, pairing, RTSP |
| `common` | Kripto, identitas sertifikat, mode streaming Windows (timer, prioritas, Wi-Fi, MMCSS) |
| `host` | Host Windows: pairing PIN, RTSP, kanal kontrol ENet, tangkap layar (Desktop Duplication) -> NVENC (H.264/HEVC/AV1), audio WASAPI -> Opus, input SendInput + gamepad ViGEm, pengawas sebagai SYSTEM |
| `client` | Client Windows/macOS: decode FFmpeg (D3D11VA/VideoToolbox, AV1 lewat dav1d), tampil D3D11/SDL3, audio Opus, keyboard/mouse/gamepad (XInput/SDL3), getar, statistik |
| `android` | Client Android: pustaka JNI `libcloudpc_stream.so` (decode NDK MediaCodec langsung ke Surface, suara AAudio, layar sentuh sebagai trackpad, stik layar yang bisa diatur dengan profil Gamepad / Minecraft / Deadlock, bidik gyro) + layar stream Kotlin di `android/kotlin` |

Semua aliran wajib terenkripsi (RTSP, kontrol, video, audio), tanpa pengecualian LAN.

## Build

Butuh Rust stabil. Client Windows butuh FFmpeg 8.0 shared (`winget install --id BtbN.FFmpeg.GPL.Shared.8.0`, atau
set `FFMPEG_DIR`); host butuh GPU NVIDIA (NVENC) dan driver NVIDIA.

```
cargo test --workspace
cargo build --release -p cloudpc-stream-host
cargo build --release -p cloudpc-stream-client
```

FFmpeg ramping (hanya decoder yang dipakai, LGPL) dan build macOS: lihat `ci/ffmpeg-slim.yml` dan `ci/stream-rs-mac.yml`.

Android (butuh NDK dan `cargo-ndk`; tidak butuh FFmpeg maupun SDL, minimal Android 8.0):

```
cargo test -p cloudpc-stream-android                       # logika murni: gestur, stik layar, gyro
cargo ndk -t arm64-v8a -P 26 build -p cloudpc-stream-android --release
```

Pustaka itu dipakai aplikasi CloudPC untuk Android (sumbernya di https://github.com/HusniHeartnet/cloudpc-app), yang
mengompilasi `android/kotlin` langsung dari repo ini.

## Pemakaian singkat

```
cloudpc-stream-host run --state <folder> [--bind 0.0.0.0]
cloudpc-stream-host pin 1234 --state <folder>          # titipkan PIN untuk pairing berikutnya
cloudpc-stream-client pair <host> --pin 1234
cloudpc-stream-client stream <host> Desktop --resolution 1920x1080 --fps 60 --bitrate 20000
```

Argumen client mengikuti Moonlight (`moonlight-qt`), jadi bisa menggantikan Moonlight di aplikasi yang memanggilnya.

## Mikrofon (ekstensi di luar GameStream)

Host yang dijalankan dengan `--mic` menerima suara mikrofon dari client (`stream ... --microphone`): Opus mono 48 kHz
dalam pesan kontrol terenkripsi (`proto/src/mic.rs`). Host menyatakan dukungannya di DESCRIBE (`a=x-cloudpc-mic:1`),
jadi Sunshine dan Moonlight asli tidak terpengaruh. Keluaran host: perangkat mikrofon USB lewat port serial
(`--mic pico`) atau berkas WAV (`--mic wav:<berkas>`).

## Lisensi

GPL-3.0-only (lihat `LICENSE`). Kode ini ditulis sambil merujuk Sunshine dan Moonlight (keduanya GPL-3.0), jadi
seluruhnya berlisensi GPL-3.0. Atribusi dan pustaka pihak ketiga: `NOTICE.md`.

Proyek ini tidak berafiliasi dengan LizardByte (Sunshine), Moonlight Game Streaming Project, maupun NVIDIA.
