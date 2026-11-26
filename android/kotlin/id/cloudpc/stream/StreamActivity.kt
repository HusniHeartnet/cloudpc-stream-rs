// Client streaming CloudPC untuk Android. GPL-3.0-only (lihat LICENSE di akar repo stream-rs).
package id.cloudpc.stream

import android.app.Activity
import android.app.AlertDialog
import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.DashPathEffect
import android.graphics.Paint
import android.graphics.RectF
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import android.hardware.input.InputManager
import android.media.MediaCodecList
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.text.InputFilter
import android.text.InputType
import android.util.TypedValue
import android.view.Gravity
import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.View
import android.view.ViewConfiguration
import android.view.ViewGroup
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.view.WindowManager
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputMethodManager
import android.widget.AdapterView
import android.widget.ArrayAdapter
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.HorizontalScrollView
import android.widget.LinearLayout
import android.widget.PopupMenu
import android.widget.ScrollView
import android.widget.Spinner
import android.widget.TextView
import android.widget.Toast
import android.window.OnBackInvokedCallback
import android.window.OnBackInvokedDispatcher
import java.io.File
import java.lang.ref.WeakReference
import java.util.concurrent.Executors
import java.util.concurrent.Future
import java.util.concurrent.TimeUnit
import kotlin.math.abs
import kotlin.math.max
import kotlin.math.roundToInt

/**
 * Layar PC di HP: satu `SurfaceView` layar penuh (mendatar) yang digambar langsung oleh decoder, ditambah input.
 *
 * - Layar sentuh = trackpad (geser kursor, ketuk klik, dua jari klik kanan/scroll, tiga jari keyboard); aturannya di
 *   `gesture.rs`. Di atasnya ada stik layar (`overlay.rs`): tata letak Gamepad atau Minecraft, bisa diatur pemain.
 * - Gamepad dan keyboard fisik diteruskan apa adanya; mouse fisik ditangkap setelah diklik di layar.
 * - Layar ini ditutup begitu ditinggalkan (menu "Perkecil", tombol kembali, Home, layar dikunci). Sesi di PC tidak ikut
 *   berakhir: aplikasi CloudPC menampilkan "Sambung lagi". Dulu layar ini tetap di atas aplikasi, sehingga membuka
 *   CloudPC lagi langsung jatuh ke sini dan menyambung ulang (pemain tidak bisa kembali ke aplikasi).
 */
class StreamActivity : Activity(), SurfaceHolder.Callback, InputManager.InputDeviceListener, SensorEventListener {
    companion object {
        const val EXTRA_HOST = "host"
        const val EXTRA_APP = "app"
        const val EXTRA_WIDTH = "width"
        const val EXTRA_HEIGHT = "height"
        const val EXTRA_FPS = "fps"
        const val EXTRA_BITRATE_KBPS = "bitrateKbps"
        const val EXTRA_CODEC = "codec"
        const val EXTRA_LOG_PATH = "logPath"

        /** Layar stream sedang ada (dibaca aplikasi CloudPC untuk tahu apakah stream masih terbuka). */
        @Volatile
        var running = false
            private set

        /** Pesan galat stream terakhir; kosong = ditutup normal. */
        @Volatile
        var lastError = ""
            private set

        private var current = WeakReference<StreamActivity>(null)

        /** Dipanggil tepat sebelum `startActivity`, supaya `running` sudah benar sebelum `onCreate` sempat jalan. */
        fun markStarting() {
            running = true
            lastError = ""
        }

        /** Tutup layar stream dari luar (aplikasi mengakhiri sesi). */
        fun close() {
            val activity = current.get()
            if (activity == null) running = false else activity.runOnUiThread { activity.finish() }
        }

        fun dataDir(context: Context): String = File(context.filesDir, "stream-client").absolutePath
    }

    private val worker = Executors.newSingleThreadExecutor()
    private val ui = Handler(Looper.getMainLooper())
    private var handle = 0L
    private var pendingStop: Future<*>? = null
    private var streamSize = Pair(1280, 720)
    private var showStats = false
    private var keyboardShown = false
    private var editing = false
    private var backCallback: OnBackInvokedCallback? = null
    /** Area gambar PC di dalam layar (kiri, atas, lebar, tinggi), dikirim ke stik layar untuk mode sentuh langsung. */
    private var picture = intArrayOf(0, 0, 0, 0)
    private var gyroListening = false
    private var lastGyroNanos = 0L
    /** Mouse fisik ditangkap untuk PC (bawaan). Ctrl+Alt+Shift+Z melepas/menangkap lagi, seperti di client desktop. */
    private var mouseCaptureWanted = true
    private var mouseCarry = floatArrayOf(0f, 0f)
    private var touchpadLast: FloatArray? = null

    private lateinit var root: FrameLayout
    private lateinit var surfaceView: SurfaceView
    private lateinit var keyboardView: KeyboardView
    private lateinit var statsView: TextView
    private lateinit var messageView: TextView
    private lateinit var padView: PadOverlayView
    private lateinit var briefView: TextView
    private lateinit var iconBar: LinearLayout
    private lateinit var keyboardIcon: IconView
    private lateinit var mouseIcon: IconView
    private lateinit var padIcon: IconView
    private lateinit var editBar: LinearLayout

    private fun dp(value: Float): Int = TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_DIP, value, resources.displayMetrics).roundToInt()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        current = WeakReference(this)
        running = true
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        if (Build.VERSION.SDK_INT >= 28) {
            window.attributes = window.attributes.apply {
                layoutInDisplayCutoutMode = WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES
            }
        }
        streamSize = Pair(intent.getIntExtra(EXTRA_WIDTH, 1280).coerceAtLeast(16), intent.getIntExtra(EXTRA_HEIGHT, 720).coerceAtLeast(16))

        root = FrameLayout(this).apply {
            setBackgroundColor(Color.BLACK)
            // Pointer capture (mouse fisik) hanya diberikan ke view yang bisa dan sedang memegang fokus.
            isFocusable = true
            isFocusableInTouchMode = true
            defaultFocusHighlightEnabled = false
        }
        surfaceView = SurfaceView(this)
        // Penyangga gambar seukuran stream; penyusun layar Android yang memperbesarnya ke ukuran tampilan.
        surfaceView.holder.setFixedSize(streamSize.first, streamSize.second)
        surfaceView.holder.addCallback(this)
        root.addView(surfaceView, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT, Gravity.CENTER))

        padView = PadOverlayView(this)
        root.addView(padView, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))

        keyboardView = KeyboardView(this)
        root.addView(keyboardView, FrameLayout.LayoutParams(1, 1))

        messageView = TextView(this).apply {
            setTextColor(Color.WHITE)
            textSize = 16f
            gravity = Gravity.CENTER
            text = "Menghubungkan ke PC…"
        }
        root.addView(messageView, FrameLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT, Gravity.CENTER))

        // Kiri atas: pil statistik yang selalu tampil ("60 FPS · 24 ms · 6 Mbps"). Diketuk = rincian di bawahnya.
        briefView = TextView(this).apply {
            setTextColor(Color.WHITE)
            textSize = 12f
            text = "…"
            background = pill(0xB30E1014.toInt())
            setPadding(dp(10f), dp(5f), dp(10f), dp(5f))
            setOnClickListener {
                showStats = !showStats
                statsView.visibility = if (showStats) View.VISIBLE else View.GONE
            }
        }
        root.addView(briefView, FrameLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT, Gravity.TOP or Gravity.START).apply {
            setMargins(dp(14f), dp(10f), 0, 0)
        })
        statsView = TextView(this).apply {
            setTextColor(Color.WHITE)
            textSize = 11f
            background = pill(0xB30E1014.toInt())
            setPadding(dp(10f), dp(6f), dp(10f), dp(6f))
            visibility = View.GONE
        }
        root.addView(statsView, FrameLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT, Gravity.TOP or Gravity.START).apply {
            setMargins(dp(14f), dp(44f), 0, 0)
        })

        // Kanan atas: bilah ikon. Keyboard layar · mode mouse (layar = trackpad saja) · stik layar · kembali ke aplikasi.
        keyboardIcon = IconView(this, Icon.KEYBOARD) { toggleKeyboard() }
        mouseIcon = IconView(this, Icon.MOUSE) { setPad(false, null) }
        padIcon = IconView(this, Icon.GAMEPAD) { showPadMenu() }
        iconBar = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            background = pill(0xB30E1014.toInt())
            setPadding(dp(6f), dp(2f), dp(6f), dp(2f))
        }
        iconBar.addView(keyboardIcon)
        iconBar.addView(mouseIcon)
        iconBar.addView(padIcon)
        iconBar.addView(IconView(this, Icon.CLOSE) { finish() })
        root.addView(iconBar, FrameLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT, Gravity.TOP or Gravity.END).apply {
            setMargins(0, dp(8f), dp(14f), 0)
        })

        // Mode atur stik layar: geser tombol untuk memindahkan, lalu ubah yang terpilih dengan tombol-tombol ini.
        val editRow = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        editRow.addView(barButton("+ Tombol") { showButtonDialog(false) })
        editRow.addView(barButton("Ubah") { showButtonDialog(true) })
        editRow.addView(barButton("Kecilkan") { editAction(2) })
        editRow.addView(barButton("Besarkan") { editAction(1) })
        editRow.addView(barButton("Sembunyi / tampil") { editAction(3) })
        editRow.addView(barButton("Hapus") { deleteSelected() })
        editRow.addView(barButton("Bawaan") { confirmReset() })
        editRow.addView(barButton("Selesai") { setEditing(false) })
        // Di HP yang layarnya sempit deretan ini bisa lebih lebar dari layar: bisa digeser ke samping.
        val editScroll = HorizontalScrollView(this).apply {
            isHorizontalScrollBarEnabled = false
            addView(editRow)
        }
        editBar = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER_HORIZONTAL
            visibility = View.GONE
        }
        editBar.addView(editScroll)
        editBar.addView(TextView(this).apply {
            text = "\"+ Tombol\" menambah tombol sendiri. Geser tombol untuk memindahkan; ketuk untuk memilihnya, lalu ubah, besarkan, kecilkan, sembunyikan, atau hapus."
            setTextColor(Color.WHITE)
            textSize = 12f
            gravity = Gravity.CENTER
            background = pill(0x99000000.toInt())
            setPadding(dp(10f), dp(4f), dp(10f), dp(4f))
            layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { topMargin = dp(6f) }
        })
        root.addView(editBar, FrameLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT, Gravity.TOP or Gravity.CENTER_HORIZONTAL).apply {
            setMargins(0, dp(8f), 0, 0)
        })

        root.setOnTouchListener { _, event -> onStreamTouch(event) }
        root.setOnCapturedPointerListener { _, event -> onCapturedMouse(event) }
        // Saat keyboard layar terbuka fokus ada di keyboardView, jadi mouse yang tertangkap dikirim ke sana.
        keyboardView.setOnCapturedPointerListener { _, event -> onCapturedMouse(event) }
        root.addOnLayoutChangeListener { _, left, top, right, bottom, _, _, _, _ -> fitSurface(right - left, bottom - top) }
        setContentView(root)

        (getSystemService(Context.INPUT_SERVICE) as InputManager).registerInputDeviceListener(this, ui)
        if (Build.VERSION.SDK_INT >= 33) {
            val callback = OnBackInvokedCallback { confirmClose() }
            onBackInvokedDispatcher.registerOnBackInvokedCallback(OnBackInvokedDispatcher.PRIORITY_DEFAULT, callback)
            backCallback = callback
        }
        hideSystemBars()
    }

    private fun pill(color: Int) = GradientDrawable().apply {
        setColor(color)
        cornerRadius = dp(8f).toFloat()
    }

    private fun barButton(label: String, action: () -> Unit) = TextView(this).apply {
        text = label
        setTextColor(Color.WHITE)
        textSize = 12f
        alpha = 0.55f
        background = pill(0x99000000.toInt())
        setPadding(dp(10f), dp(6f), dp(10f), dp(6f))
        layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { marginStart = dp(6f) }
        setOnClickListener { action() }
    }

    /** Gambar dijaga 16:9 (atau rasio stream apa pun) di tengah layar; sisanya hitam. */
    private fun fitSurface(width: Int, height: Int) {
        if (width <= 0 || height <= 0) return
        val aspect = streamSize.first.toFloat() / streamSize.second
        var w = width
        var h = (width / aspect).roundToInt()
        if (h > height) {
            h = height
            w = (height * aspect).roundToInt()
        }
        picture = intArrayOf((width - w) / 2, (height - h) / 2, w, h)
        val current = handle
        if (current != 0L) Native.padPicture(current, picture[0], picture[1], picture[2], picture[3])
        val params = surfaceView.layoutParams as FrameLayout.LayoutParams
        if (params.width != w || params.height != h) {
            params.width = w
            params.height = h
            params.gravity = Gravity.CENTER
            surfaceView.post { surfaceView.layoutParams = params }
        }
    }

    @Suppress("DEPRECATION")
    private fun hideSystemBars() {
        if (Build.VERSION.SDK_INT >= 30) {
            window.setDecorFitsSystemWindows(false)
            window.insetsController?.let {
                it.hide(WindowInsets.Type.systemBars())
                it.systemBarsBehavior = WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            }
        } else {
            window.decorView.systemUiVisibility = (View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY or View.SYSTEM_UI_FLAG_FULLSCREEN
                or View.SYSTEM_UI_FLAG_HIDE_NAVIGATION or View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                or View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN or View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION)
        }
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) {
            hideSystemBars()
            ensureMouseCaptured()  // Android melepas tangkapan mouse tiap jendela kehilangan fokus (menu, dialog)
        }
    }

    // ---- hidup-matinya stream mengikuti Surface ----------------------------------------------------------------

    override fun surfaceCreated(holder: SurfaceHolder) {
        startStream(holder)
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {}

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        stopStream()
    }

    private fun startStream(holder: SurfaceHolder) {
        if (handle != 0L) return
        // Stream sebelumnya (layar ditinggalkan lalu dibuka lagi) harus selesai dulu: keduanya menulis log yang sama.
        try {
            pendingStop?.get(3, TimeUnit.SECONDS)
        } catch (_: Exception) {
        }
        pendingStop = null
        val metrics = resources.displayMetrics
        val longSide = max(metrics.widthPixels, metrics.heightPixels).coerceAtLeast(1)
        // Gerak jari (piksel layar HP) -> gerak kursor (piksel layar PC), sedikit dipercepat supaya tidak perlu banyak sapuan.
        val pointerSpeed = streamSize.first.toFloat() / longSide * 1.5f
        val log = intent.getStringExtra(EXTRA_LOG_PATH) ?: File(cacheDir, "moonlight-stream.log").absolutePath
        handle = Native.start(
            dataDir(this),
            intent.getStringExtra(EXTRA_HOST) ?: "",
            intent.getStringExtra(EXTRA_APP) ?: "Desktop",
            streamSize.first, streamSize.second,
            intent.getIntExtra(EXTRA_FPS, 60),
            intent.getIntExtra(EXTRA_BITRATE_KBPS, 8000),
            intent.getStringExtra(EXTRA_CODEC) ?: "auto",
            hardwareCodecs(),
            ViewConfiguration.get(this).scaledTouchSlop.toFloat(),
            pointerSpeed,
            log,
            holder.surface,
        )
        if (handle == 0L) {
            endWith("Layar PC tidak bisa dibuka.")
            return
        }
        messageView.text = "Menghubungkan ke PC…"
        messageView.visibility = View.VISIBLE
        // Stream baru = keadaan stik layar baru (tata letak dimuat lagi dari file); mode atur tidak dibawa.
        editing = false
        editBar.visibility = View.GONE
        iconBar.visibility = View.VISIBLE
        briefView.text = "…"
        Native.padView(handle, padView.width, padView.height)
        Native.padPicture(handle, picture[0], picture[1], picture[2], picture[3])
        padView.invalidate()
        syncIcons()
        syncGyro()
        ensureMouseCaptured()
        ui.removeCallbacks(poll)
        ui.postDelayed(poll, 300)
    }

    private fun stopStream() {
        ui.removeCallbacks(poll)
        stopGyro()
        val old = handle
        if (old == 0L) return
        handle = 0L
        // Di thread latar: berhenti bisa menunggu jaringan beberapa detik, dan thread UI tidak boleh ikut menunggu.
        pendingStop = worker.submit { Native.stop(old) }
    }

    private val poll = object : Runnable {
        override fun run() {
            val current = handle
            if (current == 0L) return
            if (Native.ended(current)) {
                endWith(Native.error(current))
                return
            }
            val status = Native.status(current)
            if (status.contains("\nDecode ")) {
                // Statistik pertama tiba = gambar sudah mengalir.
                messageView.visibility = View.GONE
                statsView.text = status
                val brief = Native.brief(current)
                if (brief.isNotEmpty()) briefView.text = brief
            } else if (status.isNotEmpty()) {
                messageView.text = status
            }
            ui.postDelayed(this, 500)
        }
    }

    private fun endWith(error: String) {
        lastError = error
        if (error.isNotEmpty()) Toast.makeText(this, error, Toast.LENGTH_LONG).show()
        finish()
    }

    /**
     * Layar ditinggalkan (Home, pindah aplikasi, layar dikunci): tutup layar ini, jangan tertinggal di atas aplikasi.
     * Dengan begitu membuka CloudPC lagi menampilkan aplikasinya ("Sambung lagi" / "Akhiri sesi"), bukan langsung
     * menyambung ulang. Menu dan dialog milik layar ini tidak memicu onStop.
     */
    override fun onStop() {
        super.onStop()
        if (!isFinishing && !isChangingConfigurations) finish()
    }

    override fun onDestroy() {
        stopStream()
        (getSystemService(Context.INPUT_SERVICE) as InputManager).unregisterInputDeviceListener(this)
        if (Build.VERSION.SDK_INT >= 33) backCallback?.let { onBackInvokedDispatcher.unregisterOnBackInvokedCallback(it) }
        if (current.get() === this) {
            current = WeakReference(null)
            // `running` baru dimatikan SETELAH stream benar-benar berhenti (tugas ini antre di belakang Native.stop):
            // aplikasi membaca statistik dari log begitu melihat layar tertutup, dan log baru lengkap saat itu.
            worker.execute { running = false }
        }
        worker.shutdown()
        super.onDestroy()
    }

    private fun confirmClose() {
        AlertDialog.Builder(this)
            .setMessage("Kembali ke aplikasi CloudPC? Sesi tetap berjalan dan kuota tetap terpakai. Dari aplikasi kamu bisa menyambung lagi atau mengakhiri sesi.")
            .setPositiveButton("Kembali ke aplikasi") { _, _ -> finish() }
            .setNegativeButton("Batal", null)
            .setOnDismissListener { hideSystemBars() }
            .show()
    }

    @Deprecated("Android 12 ke bawah; versi baru memakai OnBackInvokedCallback di onCreate")
    @Suppress("DEPRECATION")
    override fun onBackPressed() {
        confirmClose()
    }

    // ---- layar sentuh dan mouse ------------------------------------------------------------------------------

    private fun onStreamTouch(event: MotionEvent): Boolean {
        val current = handle
        if (current == 0L) return true
        if (event.getToolType(0) == MotionEvent.TOOL_TYPE_MOUSE) {
            // Mouse fisik yang belum tertangkap (mis. baru dilepas dengan Ctrl+Alt+Shift+Z): klik di layar menangkapnya
            // lagi; setelah itu gerak relatif datang lewat onCapturedMouse.
            if (event.actionMasked == MotionEvent.ACTION_DOWN) {
                mouseCaptureWanted = true
                ensureMouseCaptured()
            }
            return true
        }
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN -> touch(current, 0, event, event.actionIndex)
            MotionEvent.ACTION_MOVE -> for (i in 0 until event.pointerCount) touch(current, 1, event, i)
            MotionEvent.ACTION_UP, MotionEvent.ACTION_POINTER_UP -> touch(current, 2, event, event.actionIndex)
            MotionEvent.ACTION_CANCEL -> for (i in 0 until event.pointerCount) touch(current, 3, event, i)
        }
        return true
    }

    private fun touch(current: Long, action: Int, event: MotionEvent, index: Int) {
        when (Native.touch(current, action, event.getPointerId(index), event.getX(index), event.getY(index), event.eventTime)) {
            1 -> toggleKeyboard()
            2 -> padView.invalidate()
        }
    }

    // ---- bilah ikon dan stik layar ----------------------------------------------------------------------------

    /** Ikon stik: pilih tata letak stik layar (Gamepad atau joystick Minecraft) atau atur posisinya. */
    private fun showPadMenu() {
        val current = handle
        if (current == 0L) return
        val shown = Native.padShown(current)
        val profile = Native.padProfile(current)
        fun mark(on: Boolean) = if (on) "✓ " else ""
        val menu = PopupMenu(this, padIcon)
        val gyro = Native.gyroEnabled(current)
        val inverted = Native.gyroInverted(current)
        menu.menu.add(0, 1, 0, mark(shown && profile == 0) + "Gamepad (stik Xbox)")
        menu.menu.add(0, 2, 0, mark(shown && profile == 1) + "Minecraft (joystick + tombol)")
        menu.menu.add(0, 4, 0, mark(shown && profile == 2) + "Deadlock (tembak + skill)")
        menu.menu.add(0, 3, 0, "Atur tombol: tambah, ubah, pindah, ukuran")
        menu.menu.add(0, 5, 0, mark(gyro) + "Gyro: bidik dengan memiringkan HP")
        if (gyro) menu.menu.add(0, 6, 0, mark(inverted) + "Balik arah gyro")
        menu.setOnMenuItemClickListener { item ->
            when (item.itemId) {
                1 -> setPad(true, 0)
                2 -> setPad(true, 1)
                4 -> setPad(true, 2)
                3 -> setEditing(true)
                5 -> setGyro(!gyro, inverted)
                6 -> setGyro(true, !inverted)
            }
            true
        }
        menu.setOnDismissListener { hideSystemBars() }
        menu.show()
    }

    /** Tampilkan stik layar dengan `profile` (null = profil terakhir), atau sembunyikan (mode mouse: layar = trackpad). */
    private fun setPad(shown: Boolean, profile: Int?) {
        val current = handle
        if (current == 0L) return
        if (profile != null) Native.padSetProfile(current, profile)
        Native.padShow(current, shown)
        padView.invalidate()
        syncIcons()
    }

    // ---- gyro: bidik dengan memiringkan HP ---------------------------------------------------------------------

    private fun setGyro(enabled: Boolean, inverted: Boolean) {
        val current = handle
        if (current == 0L) return
        val manager = getSystemService(Context.SENSOR_SERVICE) as SensorManager
        if (enabled && manager.getDefaultSensor(Sensor.TYPE_GYROSCOPE) == null) {
            Toast.makeText(this, "HP ini tidak punya sensor gyro.", Toast.LENGTH_LONG).show()
            return
        }
        Native.gyroSet(current, enabled, inverted)
        syncGyro()
    }

    /** Sensor hanya didengarkan selama stream berjalan dan gyro dinyalakan pemain (hemat baterai). */
    private fun syncGyro() {
        val current = handle
        val wanted = current != 0L && Native.gyroEnabled(current)
        if (wanted == gyroListening) return
        val manager = getSystemService(Context.SENSOR_SERVICE) as SensorManager
        if (wanted) {
            val sensor = manager.getDefaultSensor(Sensor.TYPE_GYROSCOPE) ?: return
            lastGyroNanos = 0L
            // 5 ms antar sampel: cukup halus untuk membidik, masih ringan.
            gyroListening = manager.registerListener(this, sensor, 5000)
        } else {
            stopGyro()
        }
    }

    private fun stopGyro() {
        if (!gyroListening) return
        (getSystemService(Context.SENSOR_SERVICE) as SensorManager).unregisterListener(this)
        gyroListening = false
    }

    override fun onAccuracyChanged(sensor: Sensor?, accuracy: Int) {}

    /**
     * Kecepatan putar dari sensor memakai sumbu HP dalam posisi tegak (X ke kanan, Y ke atas). Di sini dipetakan ke sumbu
     * LAYAR sesuai rotasi tampilan: `yaw` = putaran mengelilingi "atas layar", `pitch` = mengelilingi "kanan layar".
     * Mendatar dengan bagian atas HP di kiri (ROTATION_90): atas layar = +X HP, kanan layar = -Y HP; arah sebaliknya
     * (ROTATION_270) kebalikannya. Bila di suatu HP terasa terbalik, ada "Balik arah gyro" di menu.
     */
    override fun onSensorChanged(event: SensorEvent) {
        val current = handle
        if (current == 0L || event.sensor.type != Sensor.TYPE_GYROSCOPE) return
        val previous = lastGyroNanos
        lastGyroNanos = event.timestamp
        if (previous == 0L) return
        val seconds = (event.timestamp - previous) / 1e9f
        @Suppress("DEPRECATION")
        val rotation = if (Build.VERSION.SDK_INT >= 30) display?.rotation ?: Surface.ROTATION_90 else windowManager.defaultDisplay.rotation
        val x = event.values[0]
        val y = event.values[1]
        val (yaw, pitch) = when (rotation) {
            Surface.ROTATION_90 -> Pair(x, -y)
            Surface.ROTATION_270 -> Pair(-x, y)
            Surface.ROTATION_180 -> Pair(-y, -x)
            else -> Pair(y, x)
        }
        Native.gyro(current, yaw, pitch, seconds)
    }

    /** Ikon yang menyala = mode yang sedang dipakai: mouse (stik layar tersembunyi) atau stik layar; keyboard bila terbuka. */
    private fun syncIcons() {
        val current = handle
        val pad = current != 0L && Native.padShown(current)
        padIcon.active = pad
        mouseIcon.active = !pad
        keyboardIcon.active = keyboardShown
    }

    private fun setEditing(on: Boolean) {
        val current = handle
        if (current == 0L) return
        if (on && !Native.padShown(current)) Native.padShow(current, true)
        Native.padEdit(current, on)
        editing = on
        editBar.visibility = if (on) View.VISIBLE else View.GONE
        iconBar.visibility = if (on) View.GONE else View.VISIBLE
        briefView.visibility = if (on) View.GONE else View.VISIBLE
        if (on) statsView.visibility = View.GONE else if (showStats) statsView.visibility = View.VISIBLE
        padView.invalidate()
        syncIcons()
    }

    private enum class Icon { KEYBOARD, MOUSE, GAMEPAD, CLOSE }

    /** Ikon garis di bilah kanan atas (digambar sendiri, tanpa berkas gambar). `active` = disorot hijau. */
    private inner class IconView(context: Context, private val icon: Icon, action: () -> Unit) : View(context) {
        var active = false
            set(value) {
                if (field != value) {
                    field = value
                    invalidate()
                }
            }
        private val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            style = Paint.Style.STROKE
            strokeWidth = dp(1.6f).toFloat()
            strokeCap = Paint.Cap.ROUND
            strokeJoin = Paint.Join.ROUND
        }
        private val dot = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.FILL }

        init {
            layoutParams = LinearLayout.LayoutParams(dp(40f), dp(36f))
            isClickable = true
            setOnClickListener { action() }
        }

        override fun onDraw(canvas: Canvas) {
            val color = when {
                icon == Icon.CLOSE -> 0xFFFF6B6B.toInt()
                active -> 0xFF3DDC97.toInt()
                else -> 0xFFE9ECF2.toInt()
            }
            paint.color = color
            dot.color = color
            val s = minOf(width, height).toFloat() * 0.9f
            val left = (width - s) / 2
            val top = (height - s) / 2
            fun x(v: Float) = left + v * s
            fun y(v: Float) = top + v * s
            when (icon) {
                Icon.KEYBOARD -> {
                    canvas.drawRoundRect(x(0.12f), y(0.27f), x(0.88f), y(0.73f), s * 0.07f, s * 0.07f, paint)
                    for (i in 0..3) canvas.drawCircle(x(0.29f + i * 0.14f), y(0.42f), s * 0.03f, dot)
                    canvas.drawLine(x(0.34f), y(0.58f), x(0.66f), y(0.58f), paint)
                }
                Icon.MOUSE -> {
                    canvas.drawRoundRect(x(0.33f), y(0.15f), x(0.67f), y(0.85f), s * 0.17f, s * 0.17f, paint)
                    canvas.drawLine(x(0.5f), y(0.28f), x(0.5f), y(0.42f), paint)
                }
                Icon.GAMEPAD -> {
                    canvas.drawRoundRect(x(0.10f), y(0.30f), x(0.90f), y(0.72f), s * 0.20f, s * 0.20f, paint)
                    canvas.drawLine(x(0.25f), y(0.51f), x(0.41f), y(0.51f), paint)
                    canvas.drawLine(x(0.33f), y(0.43f), x(0.33f), y(0.59f), paint)
                    canvas.drawCircle(x(0.63f), y(0.55f), s * 0.035f, dot)
                    canvas.drawCircle(x(0.74f), y(0.46f), s * 0.035f, dot)
                }
                Icon.CLOSE -> {
                    canvas.drawLine(x(0.30f), y(0.30f), x(0.70f), y(0.70f), paint)
                    canvas.drawLine(x(0.70f), y(0.30f), x(0.30f), y(0.70f), paint)
                }
            }
        }
    }

    private fun editAction(action: Int) {
        val current = handle
        if (current == 0L) return
        Native.padEditAction(current, action)
        padView.invalidate()
    }

    private fun toast(text: String) = Toast.makeText(this, text, Toast.LENGTH_LONG).show()

    /** Pilihan aksi tombol buatan pemain dari `overlay.rs`: (kode, nama di daftar, tulisan bawaan). */
    private val padChoices: List<Triple<String, String, String>> by lazy {
        Native.padChoices().lines().mapNotNull { line ->
            val fields = line.split('|')
            if (fields.size >= 3) Triple(fields[0], fields[1], fields[2]) else null
        }
    }

    /**
     * Mode atur: tambah tombol sendiri (`change` = false) atau ubah tombol yang terpilih. Pemain memilih tombol keyboard/
     * mouse yang ditekan, cara kerjanya, dan tulisan di tombol; semuanya langsung tersimpan di HP ini, per profil.
     */
    private fun showButtonDialog(change: Boolean) {
        val current = handle
        if (current == 0L || padChoices.isEmpty()) return
        var action = "k69" // E: tombol "pakai/ambil" di banyak game
        var mode = 0
        var label: String? = null
        if (change) {
            val info = Native.padSelected(current).split('|')
            if (info.size < 6) {
                toast("Ketuk dulu tombol yang mau diubah.")
                return
            }
            if (info[5] != "1") {
                toast("Stik, d-pad, dan tombol gamepad tidak bisa diubah. Sembunyikan saja, lalu tambah tombol baru dengan \"+ Tombol\".")
                return
            }
            label = info[1]
            action = info[2]
            mode = info[3].toIntOrNull() ?: 0
        }
        val builder = AlertDialog.Builder(this, android.R.style.Theme_DeviceDefault_Dialog_Alert)
        val context = builder.context
        val start = padChoices.indexOfFirst { it.first == action }.coerceAtLeast(0)
        fun caption(text: String) = TextView(context).apply {
            this.text = text
            textSize = 13f
            setPadding(0, dp(10f), 0, dp(2f))
        }
        val actionSpinner = Spinner(context).apply {
            adapter = ArrayAdapter(context, android.R.layout.simple_spinner_dropdown_item, padChoices.map { it.second })
            setSelection(start)
        }
        val modeSpinner = Spinner(context).apply {
            adapter = ArrayAdapter(
                context, android.R.layout.simple_spinner_dropdown_item,
                listOf("Tahan: ditekan selama jari menempel", "Hidup/mati: ketuk sekali nyala, ketuk lagi mati", "Tahan sambil mengarahkan: geser jari = gerakkan mouse"),
            )
            setSelection(mode.coerceIn(0, 2))
        }
        val labelInput = EditText(context).apply {
            setSingleLine()
            filters = arrayOf(InputFilter.LengthFilter(12))
            hint = "kosong = nama tombolnya"
            imeOptions = EditorInfo.IME_FLAG_NO_EXTRACT_UI or EditorInfo.IME_ACTION_DONE
            setText(label ?: padChoices[start].third)
        }
        // Tulisan mengikuti tombol yang dipilih selama pemain belum menulis sendiri.
        var suggested = padChoices[start].third
        actionSpinner.onItemSelectedListener = object : AdapterView.OnItemSelectedListener {
            override fun onItemSelected(parent: AdapterView<*>?, view: View?, position: Int, id: Long) {
                val next = padChoices.getOrNull(position)?.third ?: return
                val typed = labelInput.text.toString()
                if (typed.isBlank() || typed == suggested) labelInput.setText(next)
                suggested = next
            }

            override fun onNothingSelected(parent: AdapterView<*>?) {}
        }
        val box = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(22f), dp(4f), dp(22f), 0)
            addView(caption("Tombol yang ditekan di PC"))
            addView(actionSpinner)
            addView(caption("Cara kerja"))
            addView(modeSpinner)
            addView(caption("Tulisan di tombol (paling banyak 12 huruf)"))
            addView(labelInput)
        }
        builder.setTitle(if (change) "Ubah tombol" else "Tambah tombol")
            .setView(ScrollView(context).apply { addView(box) })
            .setPositiveButton(if (change) "Simpan" else "Tambah") { _, _ ->
                val now = handle
                val choice = padChoices.getOrNull(actionSpinner.selectedItemPosition)
                if (now != 0L && choice != null) {
                    val text = labelInput.text.toString()
                    val picked = modeSpinner.selectedItemPosition
                    val done = if (change) Native.padChange(now, text, choice.first, picked) else Native.padAdd(now, text, choice.first, picked)
                    padView.invalidate()
                    toast(
                        when {
                            done && change -> "Tombol diubah menjadi ${choice.second}."
                            done -> "Tombol ${choice.second} ditambahkan di tengah layar. Geser ke tempat yang kamu mau, lalu Selesai."
                            change -> "Tombol ini tidak bisa diubah."
                            else -> "Tombol tidak bisa ditambah: tombol buatan di profil ini sudah terlalu banyak."
                        },
                    )
                }
            }
            .setNegativeButton("Batal", null)
            .setOnDismissListener { hideSystemBars() }
            .show()
    }

    /** Mode atur: hapus tombol buatan pemain yang terpilih (tombol bawaan hanya disembunyikan). */
    private fun deleteSelected() {
        val current = handle
        if (current == 0L) return
        val info = Native.padSelected(current).split('|')
        if (info.size < 6) {
            toast("Ketuk dulu tombol yang mau dihapus.")
            return
        }
        Native.padEditAction(current, 5)
        padView.invalidate()
        toast(if (info[4] == "1") "Tombol \"${info[1]}\" dihapus." else "Tombol bawaan tidak dihapus, hanya disembunyikan. Ketuk lagi lalu \"Sembunyi / tampil\" untuk memunculkannya.")
    }

    /** Mode atur: kembali ke bawaan membuang juga tombol buatan pemain di profil ini, jadi ditanya dulu. */
    private fun confirmReset() {
        AlertDialog.Builder(this)
            .setMessage("Kembalikan semua tombol di profil ini ke bawaan CloudPC? Posisi, ukuran, dan tombol yang kamu tambah sendiri di profil ini akan hilang.")
            .setPositiveButton("Kembalikan") { _, _ -> editAction(4) }
            .setNegativeButton("Batal", null)
            .setOnDismissListener { hideSystemBars() }
            .show()
    }

    /**
     * Gambar stik layar. Bentuk, posisi, dan keadaan tiap kontrol datang dari `Native.padDraw` (dihitung `overlay.rs`), jadi
     * yang digambar selalu sama dengan area sentuhnya. View ini tidak menerima sentuhan sendiri; sentuhan lewat `root`.
     */
    private inner class PadOverlayView(context: Context) : View(context) {
        private val stroke = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            style = Paint.Style.STROKE
            color = Color.WHITE
            strokeWidth = dp(1.5f).toFloat()
        }
        private val fill = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            style = Paint.Style.FILL
            color = Color.WHITE
        }
        private val label = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = Color.WHITE
            textAlign = Paint.Align.CENTER
            typeface = Typeface.DEFAULT_BOLD
        }
        private val pick = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            style = Paint.Style.STROKE
            color = 0xFF3DDC97.toInt()
            strokeWidth = dp(2f).toFloat()
            pathEffect = DashPathEffect(floatArrayOf(dp(6f).toFloat(), dp(4f).toFloat()), 0f)
        }
        private val ring = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            style = Paint.Style.STROKE
            color = Color.WHITE
            strokeWidth = dp(1.5f).toFloat()
            pathEffect = DashPathEffect(floatArrayOf(dp(5f).toFloat(), dp(5f).toFloat()), 0f)
        }
        private val box = RectF()

        override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
            super.onSizeChanged(w, h, oldw, oldh)
            val current = handle
            if (current != 0L) Native.padView(current, w, h)
        }

        override fun onDraw(canvas: Canvas) {
            val current = handle
            if (current == 0L) return
            val text = Native.padDraw(current)
            if (text.isEmpty()) return
            for (row in text.lineSequence()) {
                val f = row.split('|')
                if (f.size < 10) continue
                val cx = f[3].toFloatOrNull() ?: continue
                val cy = f[4].toFloatOrNull() ?: continue
                val w = f[5].toFloatOrNull() ?: continue
                val h = f[6].toFloatOrNull() ?: continue
                val flags = f[7].toIntOrNull() ?: 0
                val kx = f[8].toFloatOrNull() ?: 0f
                val ky = f[9].toFloatOrNull() ?: 0f
                val visible = flags and 1 != 0
                val pressed = flags and 2 != 0
                // Tersembunyi (hanya tampil di mode atur) digambar sangat samar; ditekan lebih terang.
                val edge = if (!visible) 45 else if (pressed) 235 else 125
                val body = if (!visible) 12 else if (pressed) 95 else 38
                stroke.alpha = edge
                label.alpha = edge
                fill.alpha = body
                box.set(cx - w / 2, cy - h / 2, cx + w / 2, cy + h / 2)
                when (f[2]) {
                    "s" -> {
                        // Cincin dash (overlay.rs STICK_DASH_AT): jari didorong melewati garis ini = dash.
                        if (flags and 8 != 0) {
                            ring.alpha = if (!visible) 40 else 110
                            canvas.drawCircle(cx, cy, w / 2 * 1.25f, ring)
                        }
                        canvas.drawCircle(cx, cy, w / 2, fill)
                        canvas.drawCircle(cx, cy, w / 2, stroke)
                        fill.alpha = if (!visible) 30 else if (pressed) 210 else 120
                        canvas.drawCircle(cx + kx, cy + ky, w * 0.2f, fill)
                    }
                    "d" -> {
                        val arm = w * 0.17f
                        val round = arm * 0.5f
                        val bits = kx.toInt()
                        canvas.drawRoundRect(cx - arm, cy - h / 2, cx + arm, cy + h / 2, round, round, fill)
                        canvas.drawRoundRect(cx - w / 2, cy - arm, cx + w / 2, cy + arm, round, round, fill)
                        canvas.drawRoundRect(cx - arm, cy - h / 2, cx + arm, cy + h / 2, round, round, stroke)
                        canvas.drawRoundRect(cx - w / 2, cy - arm, cx + w / 2, cy + arm, round, round, stroke)
                        fill.alpha = 170
                        if (bits and 1 != 0) canvas.drawRoundRect(cx - arm, cy - h / 2, cx + arm, cy - arm, round, round, fill)
                        if (bits and 2 != 0) canvas.drawRoundRect(cx - arm, cy + arm, cx + arm, cy + h / 2, round, round, fill)
                        if (bits and 4 != 0) canvas.drawRoundRect(cx - w / 2, cy - arm, cx - arm, cy + arm, round, round, fill)
                        if (bits and 8 != 0) canvas.drawRoundRect(cx + arm, cy - arm, cx + w / 2, cy + arm, round, round, fill)
                    }
                    "r" -> {
                        canvas.drawRoundRect(box, h / 2, h / 2, fill)
                        canvas.drawRoundRect(box, h / 2, h / 2, stroke)
                    }
                    else -> {
                        canvas.drawCircle(cx, cy, w / 2, fill)
                        canvas.drawCircle(cx, cy, w / 2, stroke)
                    }
                }
                if (f[1].isNotEmpty() && f[2] != "s") {
                    label.textSize = h * 0.34f
                    val wide = label.measureText(f[1])
                    if (wide > w * 0.78f) label.textSize *= w * 0.78f / wide
                    canvas.drawText(f[1], cx, cy - (label.descent() + label.ascent()) / 2, label)
                }
                if (flags and 4 != 0) {
                    val pad = dp(5f).toFloat()
                    box.inset(-pad, -pad)
                    canvas.drawRoundRect(box, pad * 2, pad * 2, pick)
                }
            }
        }
    }

    private fun mouseButtonOf(androidButton: Int): Int = when (androidButton) {
        MotionEvent.BUTTON_PRIMARY -> 1
        MotionEvent.BUTTON_TERTIARY -> 2
        MotionEvent.BUTTON_SECONDARY -> 3
        MotionEvent.BUTTON_BACK -> 4
        MotionEvent.BUTTON_FORWARD -> 5
        else -> 0
    }

    private fun mousePresent(): Boolean = InputDevice.getDeviceIds().any { id ->
        val device = InputDevice.getDevice(id)
        device != null && !device.isVirtual && (device.supportsSource(InputDevice.SOURCE_MOUSE) || device.supportsSource(InputDevice.SOURCE_TOUCHPAD))
    }

    /**
     * Mouse OTG/Bluetooth: begitu ada, penunjuknya ditangkap untuk PC (kursor Android hilang, gerak dikirim relatif seperti
     * mouse game). Tombol-tombol di layar tetap bisa disentuh jari. Dipanggil ulang tiap fokus kembali dan tiap mouse dicolok.
     */
    private fun ensureMouseCaptured() {
        if (!mouseCaptureWanted || handle == 0L || !hasWindowFocus() || !mousePresent()) return
        val target: View = if (keyboardShown) keyboardView else root
        if (target.hasPointerCapture()) return
        if (!target.isFocused) target.requestFocus()
        target.requestPointerCapture()
    }

    private fun releaseMouse() {
        root.releasePointerCapture()
        keyboardView.releasePointerCapture()
    }

    /** Gerak relatif -> PC; sisa pecahan disimpan supaya gerak pelan tidak hilang. */
    private fun moveMouse(current: Long, dx: Float, dy: Float) {
        val fx = dx + mouseCarry[0]
        val fy = dy + mouseCarry[1]
        val ix = fx.toInt()
        val iy = fy.toInt()
        mouseCarry[0] = fx - ix
        mouseCarry[1] = fy - iy
        if (ix != 0 || iy != 0) Native.mouseMove(current, ix, iy)
    }

    private fun onCapturedMouse(event: MotionEvent): Boolean {
        val current = handle
        if (current == 0L) return true
        when (event.actionMasked) {
            MotionEvent.ACTION_MOVE, MotionEvent.ACTION_HOVER_MOVE -> {
                if (event.isFromSource(InputDevice.SOURCE_TOUCHPAD)) {
                    // Touchpad (keyboard ber-touchpad, laptop dok): yang dilaporkan posisi jari, jadi selisihnya dihitung sendiri.
                    val last = touchpadLast
                    if (last != null) moveMouse(current, event.x - last[0], event.y - last[1])
                    touchpadLast = floatArrayOf(event.x, event.y)
                } else {
                    // Mouse: tiap sampel berisi gerak relatif. Android menggabungkan beberapa sampel dalam satu kejadian;
                    // yang lama ada di "riwayat" dan HARUS ikut dijumlah, kalau tidak gerak cepat terasa patah/pendek.
                    var dx = event.x
                    var dy = event.y
                    for (i in 0 until event.historySize) {
                        dx += event.getHistoricalX(i)
                        dy += event.getHistoricalY(i)
                    }
                    moveMouse(current, dx, dy)
                }
            }
            MotionEvent.ACTION_DOWN -> touchpadLast = if (event.isFromSource(InputDevice.SOURCE_TOUCHPAD)) floatArrayOf(event.x, event.y) else null
            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> touchpadLast = null
            MotionEvent.ACTION_BUTTON_PRESS -> Native.mouseButton(current, mouseButtonOf(event.actionButton), true)
            MotionEvent.ACTION_BUTTON_RELEASE -> Native.mouseButton(current, mouseButtonOf(event.actionButton), false)
            MotionEvent.ACTION_SCROLL -> Native.scroll(current, (event.getAxisValue(MotionEvent.AXIS_VSCROLL) * 120f).roundToInt())
        }
        return true
    }

    // ---- keyboard dan gamepad --------------------------------------------------------------------------------

    override fun dispatchKeyEvent(event: KeyEvent): Boolean = handleKey(event) || super.dispatchKeyEvent(event)

    private fun handleKey(event: KeyEvent): Boolean {
        val current = handle
        if (current == 0L) return false
        val down = when (event.action) {
            KeyEvent.ACTION_DOWN -> true
            KeyEvent.ACTION_UP -> false
            else -> return false
        }
        val code = event.keyCode
        // Pintasan keyboard fisik (sama dengan client desktop): Ctrl+Alt+Shift+Z lepas/tangkap mouse, +Q kembali ke aplikasi.
        if (event.isCtrlPressed && event.isAltPressed && event.isShiftPressed && (code == KeyEvent.KEYCODE_Z || code == KeyEvent.KEYCODE_Q)) {
            if (down && event.repeatCount == 0) {
                if (code == KeyEvent.KEYCODE_Q) finish() else {
                    mouseCaptureWanted = !mouseCaptureWanted
                    if (mouseCaptureWanted) ensureMouseCaptured() else releaseMouse()
                    Toast.makeText(this, if (mouseCaptureWanted) "Mouse dipakai untuk PC" else "Mouse dilepas (Ctrl+Alt+Shift+Z untuk memakai lagi)", Toast.LENGTH_SHORT).show()
                }
            }
            return true
        }
        val pad = event.isFromSource(InputDevice.SOURCE_GAMEPAD) || event.isFromSource(InputDevice.SOURCE_JOYSTICK) || KeyEvent.isGamepadButton(code)
        if (pad) {
            if (event.repeatCount > 0) return true
            if (Native.padButton(current, event.deviceId, code, down)) return true
            // Tombol gamepad yang tidak dipetakan jangan sampai jadi "kembali"/"pilih" di Android.
            if (KeyEvent.isGamepadButton(code)) return true
        }
        if (code == KeyEvent.KEYCODE_BACK) {
            // Klik kanan mouse fisik dilaporkan sebagian HP sebagai tombol kembali.
            if (event.isFromSource(InputDevice.SOURCE_MOUSE)) {
                Native.mouseButton(current, 3, down)
                return true
            }
            return false
        }
        return Native.key(current, code, down)
    }

    override fun dispatchGenericMotionEvent(event: MotionEvent): Boolean {
        val current = handle
        if (current != 0L && event.isFromSource(InputDevice.SOURCE_JOYSTICK) && event.actionMasked == MotionEvent.ACTION_MOVE) {
            fun axis(id: Int): Float {
                val value = event.getAxisValue(id)
                return if (abs(value) < 0.08f) 0f else value
            }
            val device = event.device
            // Kebanyakan stik: stik kanan di Z/RZ. Sebagian (mis. beberapa stik generik) memakai RX/RY dan Z/RZ untuk pemicu.
            val rightOnRx = device != null && device.getMotionRange(MotionEvent.AXIS_RX) != null && device.getMotionRange(MotionEvent.AXIS_RY) != null
            val rx = if (rightOnRx) axis(MotionEvent.AXIS_RX) else axis(MotionEvent.AXIS_Z)
            val ry = if (rightOnRx) axis(MotionEvent.AXIS_RY) else axis(MotionEvent.AXIS_RZ)
            var lt = max(event.getAxisValue(MotionEvent.AXIS_LTRIGGER), event.getAxisValue(MotionEvent.AXIS_BRAKE))
            var rt = max(event.getAxisValue(MotionEvent.AXIS_RTRIGGER), event.getAxisValue(MotionEvent.AXIS_GAS))
            if (rightOnRx && lt == 0f && rt == 0f) {
                // Pemicu di Z/RZ dengan rentang -1..1 (lepas = -1).
                lt = (event.getAxisValue(MotionEvent.AXIS_Z) + 1f) / 2f
                rt = (event.getAxisValue(MotionEvent.AXIS_RZ) + 1f) / 2f
                if (device?.getMotionRange(MotionEvent.AXIS_Z) == null) lt = 0f
                if (device?.getMotionRange(MotionEvent.AXIS_RZ) == null) rt = 0f
            }
            Native.padAxes(
                current, event.deviceId, axis(MotionEvent.AXIS_X), axis(MotionEvent.AXIS_Y), rx, ry, lt, rt,
                event.getAxisValue(MotionEvent.AXIS_HAT_X), event.getAxisValue(MotionEvent.AXIS_HAT_Y),
            )
            return true
        }
        if (current != 0L && event.isFromSource(InputDevice.SOURCE_MOUSE)) {
            when (event.actionMasked) {
                // Mouse baru bergerak dan belum tertangkap (mis. fokus baru kembali): tangkap sekarang.
                MotionEvent.ACTION_HOVER_MOVE, MotionEvent.ACTION_HOVER_ENTER -> ensureMouseCaptured()
                MotionEvent.ACTION_SCROLL -> {
                    Native.scroll(current, (event.getAxisValue(MotionEvent.AXIS_VSCROLL) * 120f).roundToInt())
                    return true
                }
            }
        }
        return super.dispatchGenericMotionEvent(event)
    }

    override fun onInputDeviceAdded(deviceId: Int) {
        ensureMouseCaptured()
    }

    override fun onInputDeviceChanged(deviceId: Int) {}

    override fun onInputDeviceRemoved(deviceId: Int) {
        val current = handle
        if (current != 0L) Native.padGone(current, deviceId)
    }

    // ---- keyboard layar ------------------------------------------------------------------------------------

    private fun toggleKeyboard() {
        val manager = getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager
        if (keyboardShown) {
            manager.hideSoftInputFromWindow(keyboardView.windowToken, 0)
        } else {
            keyboardView.requestFocus()
            manager.showSoftInput(keyboardView, 0)
        }
        keyboardShown = !keyboardShown
        // Fokus pindah antara keyboardView dan root; tangkapan mouse ikut dipindah.
        if (!keyboardShown) root.requestFocus()
        ensureMouseCaptured()
        syncIcons()
    }

    private fun tapKey(code: Int) {
        val current = handle
        if (current == 0L) return
        Native.key(current, code, true)
        Native.key(current, code, false)
    }

    /** View tak terlihat yang menerima ketikan keyboard layar dan meneruskannya ke PC sebagai teks/tombol. */
    private inner class KeyboardView(context: Context) : View(context) {
        init {
            isFocusable = true
            isFocusableInTouchMode = true
        }

        override fun onCheckIsTextEditor() = true

        override fun onCreateInputConnection(outAttrs: EditorInfo): InputConnection {
            // "Password terlihat" + tanpa saran: keyboard mengirim tiap huruf langsung, tidak menahan kata untuk prediksi.
            outAttrs.inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
            // Tanpa mode layar penuh: di posisi mendatar keyboard Android biasanya menutupi seluruh layar dengan kotak teks.
            outAttrs.imeOptions = EditorInfo.IME_FLAG_NO_FULLSCREEN or EditorInfo.IME_FLAG_NO_EXTRACT_UI or EditorInfo.IME_ACTION_NONE
            return object : BaseInputConnection(this, false) {
                override fun commitText(text: CharSequence?, newCursorPosition: Int): Boolean {
                    val current = handle
                    if (current != 0L && !text.isNullOrEmpty()) {
                        if (text.toString() == "\n") tapKey(KeyEvent.KEYCODE_ENTER) else Native.text(current, text.toString())
                    }
                    return true
                }

                override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
                    repeat(beforeLength.coerceIn(1, 32)) { tapKey(KeyEvent.KEYCODE_DEL) }
                    return true
                }

                override fun sendKeyEvent(event: KeyEvent): Boolean = handleKey(event) || super.sendKeyEvent(event)
            }
        }
    }

    // ---- kemampuan decoder HP --------------------------------------------------------------------------------

    /** Bit 1 = H.264, 2 = HEVC, 4 = AV1: codec yang punya decoder HARDWARE di HP ini. */
    private fun hardwareCodecs(): Int {
        var mask = 0
        for (info in MediaCodecList(MediaCodecList.REGULAR_CODECS).codecInfos) {
            if (info.isEncoder) continue
            val hardware = if (Build.VERSION.SDK_INT >= 29) info.isHardwareAccelerated
            else !(info.name.startsWith("OMX.google.") || info.name.startsWith("c2.android."))
            if (!hardware) continue
            for (type in info.supportedTypes) {
                when (type.lowercase()) {
                    "video/avc" -> mask = mask or 1
                    "video/hevc" -> mask = mask or 2
                    "video/av01" -> mask = mask or 4
                }
            }
        }
        return mask
    }
}
