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

## Pemakaian singkat

```
cloudpc-stream-host run --state <folder> [--bind 0.0.0.0]
cloudpc-stream-host pin 1234 --state <folder>          # titipkan PIN untuk pairing berikutnya
cloudpc-stream-client pair <host> --pin 1234
cloudpc-stream-client stream <host> Desktop --resolution 1920x1080 --fps 60 --bitrate 20000
```

Argumen client mengikuti Moonlight (`moonlight-qt`), jadi bisa menggantikan Moonlight di aplikasi yang memanggilnya.

## Lisensi

GPL-3.0-only (lihat `LICENSE`). Kode ini ditulis sambil merujuk Sunshine dan Moonlight (keduanya GPL-3.0), jadi
seluruhnya berlisensi GPL-3.0. Atribusi dan pustaka pihak ketiga: `NOTICE.md`.

Proyek ini tidak berafiliasi dengan LizardByte (Sunshine), Moonlight Game Streaming Project, maupun NVIDIA.
