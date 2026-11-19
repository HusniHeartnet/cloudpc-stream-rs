# Atribusi dan pustaka pihak ketiga

## Karya yang dirujuk (GPL-3.0)

Kode di repositori ini ditulis dengan merujuk sumber proyek berikut, sehingga merupakan karya turunan dan dirilis di
bawah lisensi yang sama, GPL-3.0:

- **Sunshine**, Copyright (C) LizardByte dan kontributor, GPL-3.0. https://github.com/LizardByte/Sunshine
- **moonlight-common-c**, Copyright (C) Moonlight Game Streaming Project dan kontributor, GPL-3.0.
  https://github.com/moonlight-stream/moonlight-common-c
- **moonlight-qt**, Copyright (C) Moonlight Game Streaming Project dan kontributor, GPL-3.0.
  https://github.com/moonlight-stream/moonlight-qt

Pengkodean Reed-Solomon (`proto/src/fec.rs`) mengikuti pendekatan nanors (GF(2^8), matriks Cauchy) seperti yang dipakai
Sunshine/moonlight-common-c. Matriks paritas audio adalah matriks tetap protokol GameStream.

## Pustaka yang dipakai saat build atau dibundel

| Pustaka | Lisensi | Catatan |
|---|---|---|
| FFmpeg (libavcodec, libavutil) | LGPL-2.1+ (build ramping) | Ditautkan dinamis. Build ramping dari sumber resmi tag n8.0.1 (https://github.com/FFmpeg/FFmpeg), konfigurasi di `ci/ffmpeg-slim.yml` dan `ci/stream-rs-mac.yml` |
| dav1d | BSD-2-Clause | Decoder AV1 (CPU) lewat FFmpeg |
| SDL3 | zlib | Opsional, dimuat saat berjalan (stik PlayStation/Switch; tampilan macOS) |
| libopus (`unsafe-libopus`) | BSD-3-Clause | Codec audio |
| ENet (`rusty_enet`) | MIT | Kanal kontrol |
| rustls, ring | ISC / Apache-2.0 AND ISC | TLS dan kripto |
| crate Rust lain | lihat `Cargo.lock` | Lisensi masing-masing crate |
| NVIDIA Video Codec SDK (header lewat crate `moq-nvenc`) | MIT (header) | NVENC dimuat dari driver NVIDIA saat berjalan |
| ViGEmBus (`vigem-client`) | MIT (crate) | Gamepad virtual di host (driver dipasang terpisah) |
