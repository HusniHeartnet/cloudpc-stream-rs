// Client streaming CloudPC untuk Android. GPL-3.0-only (lihat LICENSE di akar repo stream-rs).
package id.cloudpc.stream

import android.view.Surface

/**
 * Pustaka Rust `libcloudpc_stream.so` (crate `stream-rs/android`, modul `jni_api`). Semua fungsi yang menerima
 * `handle` hanya boleh dipanggil selama pegangan itu belum di-`stop`; `StreamActivity` yang menjaganya.
 */
object Native {
    init {
        System.loadLibrary("cloudpc_stream")
    }

    /** Pairing dengan PIN; memblokir. Teks kosong = berhasil, selain itu pesan galat. */
    @JvmStatic external fun pair(dataDir: String, host: String, pin: String): String

    /** Mulai stream ke `surface`. 0 = gagal sebelum mulai. `hardwareCodecs`: bit 1 H.264, 2 HEVC, 4 AV1. */
    @JvmStatic external fun start(
        dataDir: String, host: String, app: String, width: Int, height: Int, fps: Int, bitrateKbps: Int,
        codec: String, hardwareCodecs: Int, touchSlop: Float, pointerSpeed: Float, logPath: String, surface: Surface,
    ): Long

    /** Akhiri stream dan bebaskan pegangan; memblokir sampai thread sesi selesai. */
    @JvmStatic external fun stop(handle: Long)

    @JvmStatic external fun ended(handle: Long): Boolean
    @JvmStatic external fun error(handle: Long): String
    @JvmStatic external fun status(handle: Long): String

    /** Baris ringkas untuk pil statistik: "60 FPS · 24 ms · 6 Mbps". Kosong sebelum gambar mengalir. */
    @JvmStatic external fun brief(handle: Long): String

    /** `action`: 0 turun, 1 gerak, 2 naik, 3 batal. Hasil 1 = buka/tutup keyboard layar. */
    @JvmStatic external fun touch(handle: Long, action: Int, id: Int, x: Float, y: Float, timeMs: Long): Int

    @JvmStatic external fun key(handle: Long, keyCode: Int, down: Boolean): Boolean
    @JvmStatic external fun text(handle: Long, text: String)
    @JvmStatic external fun padButton(handle: Long, device: Int, keyCode: Int, down: Boolean): Boolean
    @JvmStatic external fun padAxes(
        handle: Long, device: Int, lx: Float, ly: Float, rx: Float, ry: Float, lt: Float, rt: Float, hatX: Float, hatY: Float,
    )
    @JvmStatic external fun padGone(handle: Long, device: Int)
    @JvmStatic external fun mouseMove(handle: Long, dx: Int, dy: Int)
    @JvmStatic external fun mouseButton(handle: Long, button: Int, down: Boolean)
    @JvmStatic external fun scroll(handle: Long, amount: Int)

    // Stik di layar (logika dan tata letaknya di overlay.rs; Kotlin hanya menggambar). `touch` mengembalikan 2 bila
    // stik layar berubah dan perlu digambar ulang.
    @JvmStatic external fun padView(handle: Long, width: Int, height: Int)
    @JvmStatic external fun padShown(handle: Long): Boolean
    @JvmStatic external fun padShow(handle: Long, shown: Boolean)
    /** Area gambar PC di dalam tampilan (piksel), untuk mode sentuh langsung saat toko terbuka. */
    @JvmStatic external fun padPicture(handle: Long, left: Int, top: Int, width: Int, height: Int)

    // Gyro: bidik dengan memiringkan HP. `gyro` menerima kecepatan putar dalam sumbu LAYAR (lihat gyro.rs).
    @JvmStatic external fun gyroEnabled(handle: Long): Boolean
    @JvmStatic external fun gyroInverted(handle: Long): Boolean
    @JvmStatic external fun gyroSet(handle: Long, enabled: Boolean, inverted: Boolean)
    @JvmStatic external fun gyro(handle: Long, yaw: Float, pitch: Float, seconds: Float)

    /** 0 = Gamepad (stik Xbox), 1 = Minecraft, 2 = Deadlock (keduanya keyboard + mouse). */
    @JvmStatic external fun padProfile(handle: Long): Int
    @JvmStatic external fun padSetProfile(handle: Long, profile: Int)
    @JvmStatic external fun padEdit(handle: Long, editing: Boolean)
    /** 1 perbesar, 2 perkecil, 3 sembunyikan/tampilkan kontrol terpilih, 4 kembali ke bawaan, 5 hapus tombol buatan pemain. */
    @JvmStatic external fun padEditAction(handle: Long, action: Int)

    // Tombol buatan pemain (mode atur). Semua perubahan langsung disimpan di HP, per profil.
    /** Pilihan aksi, satu per baris: `kode|nama di daftar|tulisan bawaan` (tombol keyboard, tombol mouse, roda mouse). */
    @JvmStatic external fun padChoices(): String
    /** Kontrol terpilih: `id|tulisan|kode aksi|cara kerja|buatan pemain(0/1)|bisa diubah(0/1)`; kosong = belum ada. */
    @JvmStatic external fun padSelected(handle: Long): String
    /** Tambah tombol di tengah layar. `mode`: 0 tahan, 1 hidup/mati, 2 tahan sambil mengarahkan. */
    @JvmStatic external fun padAdd(handle: Long, label: String, action: String, mode: Int): Boolean
    /** Ganti aksi, cara kerja, dan tulisan kontrol terpilih. */
    @JvmStatic external fun padChange(handle: Long, label: String, action: String, mode: Int): Boolean
    /** Baris `id|label|bentuk|cx|cy|lebar|tinggi|tanda|kx|ky` per kontrol (lihat `Overlay::describe`). */
    @JvmStatic external fun padDraw(handle: Long): String
}
