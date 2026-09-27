// Barcode and QR code scanner for the Zaklon phone app: a full-screen camera
// view (CameraX) whose frames zxing-cpp reads on the phone itself. Nothing
// here uses Google Play services or the network.

package com.zaklon.scanner

import android.Manifest
import android.app.Activity
import android.content.Context
import android.content.pm.PackageManager
import android.content.res.ColorStateList
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.RectF
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.RippleDrawable
import android.util.Size
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.ViewGroup.LayoutParams.MATCH_PARENT
import android.view.ViewGroup.LayoutParams.WRAP_CONTENT
import android.webkit.WebView
import android.widget.Button
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView
import androidx.activity.ComponentActivity
import androidx.activity.OnBackPressedCallback
import androidx.appcompat.app.AppCompatActivity
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.Preview
import androidx.camera.core.resolutionselector.ResolutionSelector
import androidx.camera.core.resolutionselector.ResolutionStrategy
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.core.content.ContextCompat
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import app.tauri.Logger
import app.tauri.PermissionState
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.roundToInt
import zxingcpp.BarcodeReader
import zxingcpp.BarcodeReader.Format

private const val CAMERA = "camera"

/** What is read when the interface names no formats: the codes on products, and QR codes. */
private val DEFAULT_FORMATS = setOf(
    Format.EAN_13, Format.EAN_8, Format.UPC_A, Format.UPC_E,
    Format.CODE_128, Format.CODE_39, Format.ITF, Format.QR_CODE,
)

/** Square codes: a scan for these alone shows a square frame instead of a wide one. */
private val SQUARE_FORMATS = setOf(Format.QR_CODE, Format.MICRO_QR_CODE, Format.DATA_MATRIX, Format.AZTEC)

@InvokeArg
class ScanArgs {
    /** zxing-cpp format names, e.g. "EAN_13" or "QR_CODE"; names it does not know are skipped. */
    var formats: Array<String>? = null
    /** The cancel button's label, in the interface's language. */
    var cancelLabel: String? = null
    /** The line above the cancel button, in the interface's language. */
    var hint: String? = null
}

/** One scan on screen: the interface's call waiting for an answer, and what to undo when it ends. */
private class Session(val invoke: Invoke, val overlay: View) {
    /** Frames are read here, away from the main thread. */
    val analysisThread: ExecutorService = Executors.newSingleThreadExecutor()
    /** Set once a code was read (or the scan ended), so only one answer is sent. */
    val done = AtomicBoolean(false)
    var back: OnBackPressedCallback? = null
    var cameras: ProcessCameraProvider? = null
}

@TauriPlugin(permissions = [Permission(strings = [Manifest.permission.CAMERA], alias = CAMERA)])
class ScannerPlugin(private val activity: Activity) : Plugin(activity) {
    private var webView: WebView? = null

    /** The scan on screen, if any. Used on the main thread only. */
    private var session: Session? = null

    override fun load(webView: WebView) {
        super.load(webView)
        this.webView = webView
    }

    /**
     * Opens the camera and resolves with `{ text, format }` for the first code
     * read, or `{ canceled: true }` when the person closes the camera (the
     * button or Back). Asks for camera access first when it is not allowed yet.
     */
    @Command
    fun scan(invoke: Invoke) {
        if (!activity.packageManager.hasSystemFeature(PackageManager.FEATURE_CAMERA_ANY)) {
            invoke.reject("This phone has no camera.", "unavailable")
        } else if (getPermissionState(CAMERA) == PermissionState.GRANTED) {
            activity.runOnUiThread { open(invoke) }
        } else {
            // Android asks the person; the answer arrives in cameraPermissionResult.
            requestPermissionForAlias(CAMERA, invoke, "cameraPermissionResult")
        }
    }

    @PermissionCallback
    private fun cameraPermissionResult(invoke: Invoke) {
        if (getPermissionState(CAMERA) == PermissionState.GRANTED) {
            activity.runOnUiThread { open(invoke) }
        } else {
            invoke.reject("Camera access was not allowed.", "denied")
        }
    }

    override fun onDestroy(activity: AppCompatActivity) {
        session?.let { finish(it, null, null) }
    }

    private fun open(invoke: Invoke) {
        val owner = activity as? ComponentActivity
        val root = (activity.findViewById<View>(android.R.id.content) as? ViewGroup) ?: (webView?.parent as? ViewGroup)
        if (session != null) {
            invoke.reject("The camera is already open.", "busy")
            return
        }
        if (owner == null || root == null) {
            invoke.reject("The camera cannot be shown here.", "unavailable")
            return
        }
        val args = invoke.parseArgs(ScanArgs::class.java)
        val formats = args.formats.orEmpty()
            .mapNotNull { name -> Format.values().firstOrNull { it.name == name } }
            .toSet()
            .ifEmpty { DEFAULT_FORMATS }

        val preview = PreviewView(activity).apply {
            // A TextureView, so the frame and the button always draw over the picture.
            implementationMode = PreviewView.ImplementationMode.COMPATIBLE
            scaleType = PreviewView.ScaleType.FILL_CENTER
        }
        val hint = TextView(activity).apply {
            text = args.hint?.takeUnless { it.isBlank() } ?: "Hold the code inside the frame."
            setTextColor(Color.WHITE)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 16f)
            gravity = Gravity.CENTER
            setShadowLayer(dp(4).toFloat(), 0f, 0f, Color.BLACK)
        }
        val cancel = Button(activity).apply {
            text = args.cancelLabel?.takeUnless { it.isBlank() } ?: "Cancel"
            isAllCaps = false
            setTextColor(Color.WHITE)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 17f)
            minWidth = dp(160)
            minHeight = dp(48)
            setPadding(dp(32), dp(12), dp(32), dp(12))
            stateListAnimator = null
            val shape = GradientDrawable().apply {
                cornerRadius = dp(28).toFloat()
                setColor(Color.argb(210, 28, 32, 42))
                setStroke(dp(1), Color.argb(120, 255, 255, 255))
            }
            background = RippleDrawable(ColorStateList.valueOf(Color.argb(70, 255, 255, 255)), shape, null)
        }
        val bottom = LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER_HORIZONTAL
            setPadding(dp(24), 0, dp(24), dp(48))
            addView(hint, LinearLayout.LayoutParams(WRAP_CONTENT, WRAP_CONTENT).apply { bottomMargin = dp(20) })
            addView(cancel, LinearLayout.LayoutParams(WRAP_CONTENT, WRAP_CONTENT))
        }
        val overlay = FrameLayout(activity).apply {
            setBackgroundColor(Color.BLACK)
            // Taps must not reach the page under the camera view.
            isClickable = true
            keepScreenOn = true
            addView(preview, FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT))
            addView(Viewfinder(activity, formats.all { it in SQUARE_FORMATS }), FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT))
            addView(bottom, FrameLayout.LayoutParams(MATCH_PARENT, WRAP_CONTENT, Gravity.BOTTOM))
        }
        // The app draws under the status and navigation bars: keep the button clear of them.
        ViewCompat.setOnApplyWindowInsetsListener(overlay) { _, insets ->
            val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
            bottom.setPadding(bars.left + dp(24), 0, bars.right + dp(24), bars.bottom + dp(32))
            insets
        }

        val s = Session(invoke, overlay)
        session = s
        cancel.setOnClickListener { finish(s, null, null) }
        val back = object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() {
                finish(s, null, null)
            }
        }
        // Added after the web view's own handler, so it comes first while the camera is open.
        owner.onBackPressedDispatcher.addCallback(owner, back)
        s.back = back

        root.addView(overlay, MATCH_PARENT, MATCH_PARENT)
        ViewCompat.requestApplyInsets(overlay)
        WindowCompat.getInsetsController(activity.window, overlay).hide(WindowInsetsCompat.Type.ime())
        webView?.importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS

        val cameras = ProcessCameraProvider.getInstance(activity)
        cameras.addListener({
            if (session !== s) return@addListener
            try {
                startCamera(s, cameras.get(), owner, preview, formats)
            } catch (e: Exception) {
                Logger.error("Scanner: the camera could not be started", e)
                if (close(s)) s.invoke.reject("The camera could not be started.", "unavailable")
            }
        }, ContextCompat.getMainExecutor(activity))
    }

    private fun startCamera(
        s: Session,
        cameras: ProcessCameraProvider,
        owner: ComponentActivity,
        preview: PreviewView,
        formats: Set<Format>,
    ) {
        s.cameras = cameras
        val picture = Preview.Builder().build()
        picture.setSurfaceProvider(preview.surfaceProvider)
        val analysis = ImageAnalysis.Builder()
            .setResolutionSelector(
                ResolutionSelector.Builder()
                    .setResolutionStrategy(
                        ResolutionStrategy(Size(1280, 720), ResolutionStrategy.FALLBACK_RULE_CLOSEST_HIGHER_THEN_LOWER)
                    )
                    .build()
            )
            .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
            .build()
        val reader = BarcodeReader(
            BarcodeReader.Options(formats = formats, tryHarder = true, tryRotate = true, tryInvert = true, tryDownscale = true)
        )
        analysis.setAnalyzer(s.analysisThread) { image ->
            try {
                if (!s.done.get()) {
                    val code = reader.read(image).firstOrNull { !it.text.isNullOrEmpty() }
                    if (code != null && s.done.compareAndSet(false, true)) {
                        val text = code.text.orEmpty()
                        val format = code.format.name
                        activity.runOnUiThread { finish(s, text, format) }
                    }
                }
            } catch (e: Exception) {
                Logger.error("Scanner: a frame could not be read", e)
            } finally {
                image.close()
            }
        }
        val lens = if (cameras.hasCamera(CameraSelector.DEFAULT_BACK_CAMERA)) {
            CameraSelector.DEFAULT_BACK_CAMERA
        } else {
            CameraSelector.DEFAULT_FRONT_CAMERA
        }
        cameras.unbindAll()
        cameras.bindToLifecycle(owner, lens, picture, analysis)
    }

    /** Closes the camera view and answers the interface: the code read, or canceled when [text] is null. */
    private fun finish(s: Session, text: String?, format: String?) {
        if (!close(s)) return
        val result = JSObject()
        if (text != null) {
            result.put("text", text)
            result.put("format", format)
        } else {
            result.put("canceled", true)
        }
        s.invoke.resolve(result)
    }

    /** Puts the app back as it was before the scan; false when [s] had already ended. */
    private fun close(s: Session): Boolean {
        if (session !== s) return false
        session = null
        s.done.set(true)
        s.back?.remove()
        try {
            s.cameras?.unbindAll()
        } catch (e: Exception) {
            Logger.error("Scanner: the camera could not be closed", e)
        }
        s.analysisThread.shutdown()
        (s.overlay.parent as? ViewGroup)?.removeView(s.overlay)
        webView?.importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_AUTO
        return true
    }

    private fun dp(value: Int): Int = (value * activity.resources.displayMetrics.density).roundToInt()
}

/** Dims the camera picture around the frame the code should be held in. */
private class Viewfinder(context: Context, private val square: Boolean) : View(context) {
    private val density = resources.displayMetrics.density
    private val dim = Paint().apply { color = Color.argb(140, 0, 0, 0) }
    private val border = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeWidth = 3 * density
        color = Color.WHITE
    }
    private val frame = RectF()

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        val w = width.toFloat()
        val h = height.toFloat()
        val frameWidth = minOf(w, h) * if (square) 0.7f else 0.85f
        val frameHeight = if (square) frameWidth else frameWidth * 0.55f
        val left = (w - frameWidth) / 2
        // A little above the middle, leaving room for the hint and the button.
        val top = (h - frameHeight) * 0.42f
        frame.set(left, top, left + frameWidth, top + frameHeight)
        canvas.drawRect(0f, 0f, w, frame.top, dim)
        canvas.drawRect(0f, frame.bottom, w, h, dim)
        canvas.drawRect(0f, frame.top, frame.left, frame.bottom, dim)
        canvas.drawRect(frame.right, frame.top, w, frame.bottom, dim)
        val radius = 12 * density
        canvas.drawRoundRect(frame, radius, radius, border)
    }
}
