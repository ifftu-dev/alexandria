package org.alexandria.node

import android.Manifest
import android.content.Context
import android.content.pm.ApplicationInfo
import android.content.pm.PackageManager
import android.hardware.display.DisplayManager
import android.os.Build
import android.os.Bundle
import android.provider.Settings
import android.view.MotionEvent
import android.view.WindowManager
import android.view.accessibility.AccessibilityManager
import android.accessibilityservice.AccessibilityServiceInfo
import androidx.activity.enableEdgeToEdge
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import org.json.JSONArray
import org.json.JSONObject
import java.util.concurrent.atomic.AtomicInteger

class MainActivity : TauriActivity() {
  companion object {
    private const val AV_PERMISSION_REQUEST = 0x4156

    /** Runtime permissions the native A/V pipeline needs. */
    private val AV_PERMISSIONS = arrayOf(
      Manifest.permission.CAMERA,
      Manifest.permission.RECORD_AUDIO,
    )

    /**
     * The foreground activity, for the Rust side to reach.
     *
     * `ndk_context` hands Rust the *Application*, and a runtime permission
     * request needs an Activity. Rust loads this class through the app's
     * class loader and calls the two static methods below.
     */
    @Volatile private var current: MainActivity? = null

    /** True between `requestAvPermissions()` and the user's answer. */
    @Volatile private var pending = false

    /**
     * 1 = camera and microphone both granted; 0 = a request is in flight;
     * 2 = not granted and nothing pending (denied, or never asked).
     */
    @JvmStatic
    fun avPermissionState(): Int {
      val a = current ?: return 2
      val granted = AV_PERMISSIONS.all {
        ContextCompat.checkSelfPermission(a, it) == PackageManager.PERMISSION_GRANTED
      }
      return if (granted) 1 else if (pending) 0 else 2
    }

    /**
     * Ask for whatever is still missing. Called by Rust at the moment a
     * tutoring session is about to open the camera and microphone — the first
     * time those are consumed, not at app launch.
     *
     * Why Rust drives this rather than the WebView: the WebView's own
     * permission handler only fires when a *page* calls getUserMedia. Live
     * tutoring opens both devices natively — cpal for the mic, the NDK Camera2
     * API for video — and those calls never reach that handler; a denied mic
     * surfaced as a cpal BackendSpecific("Internal") error on stream build,
     * which aborted the process under panic=abort. So the session start asks
     * first and refuses cleanly if the answer is no.
     */
    @JvmStatic
    fun requestAvPermissions() {
      val a = current ?: return
      if (Build.VERSION.SDK_INT < Build.VERSION_CODES.M) return
      val missing = AV_PERMISSIONS.filter {
        ContextCompat.checkSelfPermission(a, it) != PackageManager.PERMISSION_GRANTED
      }
      if (missing.isEmpty()) return
      pending = true
      a.runOnUiThread {
        ActivityCompat.requestPermissions(a, missing.toTypedArray(), AV_PERMISSION_REQUEST)
      }
    }

    /**
     * Display arrangement for Sentinel, as JSON. Read by Rust
     * (`sentinel::display_topology`) once per integrity snapshot.
     *
     * `display_count` counts every display the DisplayManager drives,
     * `presentation_count` the secondary / wireless ones (a cast session
     * shows up here), and the two booleans say whether this activity shares
     * the screen with another app. Every key is optional on the Rust side,
     * so an empty object means "nothing observed".
     */
    @JvmStatic
    fun displayTopology(): String {
      val a = current ?: return "{}"
      val json = JSONObject()
      try {
        val dm = a.getSystemService(Context.DISPLAY_SERVICE) as DisplayManager
        json.put("display_count", dm.displays.size)
        json.put(
          "presentation_count",
          dm.getDisplays(DisplayManager.DISPLAY_CATEGORY_PRESENTATION).size,
        )
      } catch (_: Exception) {
        // DisplayManager unavailable: leave the counts out.
      }
      json.put("multi_window", a.isInMultiWindowMode)
      json.put("picture_in_picture", a.isInPictureInPictureMode)
      json.put("native_transitions", multiWindowTransitions.get())
      return json.toString()
    }

    /**
     * Cumulative count of multi-window / PiP mode transitions since process
     * start. A transition that begins and ends between two Sentinel samples
     * would otherwise go unseen; the delta between samples surfaces it.
     */
    private val multiWindowTransitions = java.util.concurrent.atomic.AtomicInteger(0)

    /**
     * Whether the assessment shield is engaged. Remembered here so a
     * recreated activity (rotation, process restore) re-applies it in
     * `onCreate`; Rust owns the on/off decisions.
     */
    @Volatile private var shieldOn = false

    /**
     * Touches that arrived while another window was drawn over ours
     * (`FLAG_WINDOW_IS_OBSCURED` / `FLAG_WINDOW_IS_PARTIALLY_OBSCURED`).
     * Counted in `dispatchTouchEvent`; drained by `takeObscuredTouches`.
     */
    private val obscuredTouches = AtomicInteger(0)

    /**
     * Engage or release the assessment shield.
     *
     * On: `FLAG_SECURE` keeps the window out of screenshots, screen
     * recordings, casts and the recents thumbnail; on API 31+
     * `setHideOverlayWindows(true)` hides every non-system overlay drawn
     * over the app (needs `HIDE_OVERLAY_WINDOWS`, a normal permission).
     * Off reverses both. Idempotent; safe from any thread.
     */
    @JvmStatic
    fun setAssessmentShield(on: Boolean) {
      shieldOn = on
      val a = current ?: return
      // Apply the latest requested state, not this call's argument: two
      // posts from different threads may run in either order.
      a.runOnUiThread { a.applyShield(shieldOn) }
    }

    /**
     * Returns the obscured-touch count accumulated since the last call and
     * resets it. Rust decides what a non-zero count means for the window.
     */
    @JvmStatic
    fun takeObscuredTouches(): Int = obscuredTouches.getAndSet(0)

    /**
     * Environment facts Sentinel cannot see from the WebView, as JSON. Each
     * read is isolated: a failing one leaves its key out, and the Rust side
     * treats every key as optional.
     *
     * `accessibility_services` lists enabled services as
     * `{"id": "<package>/<class>", "system": bool}`; a non-system service
     * can read the screen on the app's behalf. `obscured_touches` is the
     * running count (not reset here).
     */
    @JvmStatic
    fun environmentReport(): String {
      val a = current ?: return "{}"
      val json = JSONObject()
      try {
        val am = a.getSystemService(Context.ACCESSIBILITY_SERVICE) as AccessibilityManager
        val services = JSONArray()
        for (info in am.getEnabledAccessibilityServiceList(AccessibilityServiceInfo.FEEDBACK_ALL_MASK)) {
          val si = info.resolveInfo?.serviceInfo ?: continue
          val flags = si.applicationInfo?.flags ?: 0
          val system =
            flags and (ApplicationInfo.FLAG_SYSTEM or ApplicationInfo.FLAG_UPDATED_SYSTEM_APP) != 0
          services.put(JSONObject().put("id", si.packageName + "/" + si.name).put("system", system))
        }
        json.put("accessibility_services", services)
      } catch (_: Exception) {
        // AccessibilityManager unavailable: leave the list out.
      }
      try {
        json.put("adb_enabled", Settings.Global.getInt(a.contentResolver, Settings.Global.ADB_ENABLED, 0) == 1)
      } catch (_: Exception) {
      }
      try {
        json.put(
          "development_settings_enabled",
          Settings.Global.getInt(a.contentResolver, Settings.Global.DEVELOPMENT_SETTINGS_ENABLED, 0) == 1,
        )
      } catch (_: Exception) {
      }
      json.put("shield_active", shieldOn)
      json.put("overlay_hiding_supported", Build.VERSION.SDK_INT >= Build.VERSION_CODES.S)
      try {
        json.put("multi_window", a.isInMultiWindowMode)
      } catch (_: Exception) {
      }
      json.put("obscured_touches", obscuredTouches.get())
      json.put("sdk_int", Build.VERSION.SDK_INT)
      return json.toString()
    }
  }

  /** UI-thread half of [setAssessmentShield]. */
  private fun applyShield(on: Boolean) {
    if (on) {
      window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
    } else {
      window.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
    }
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
      window.setHideOverlayWindows(on)
    }
  }

  override fun dispatchTouchEvent(event: MotionEvent): Boolean {
    val obscured = MotionEvent.FLAG_WINDOW_IS_OBSCURED or MotionEvent.FLAG_WINDOW_IS_PARTIALLY_OBSCURED
    // Only while the shield is up: the counter is an assessment signal,
    // not a lifetime tally of every overlay tap on the device.
    if (shieldOn && event.actionMasked == MotionEvent.ACTION_DOWN && event.flags and obscured != 0) {
      obscuredTouches.incrementAndGet()
    }
    return super.dispatchTouchEvent(event)
  }

  override fun onMultiWindowModeChanged(isInMultiWindowMode: Boolean, newConfig: android.content.res.Configuration) {
    super.onMultiWindowModeChanged(isInMultiWindowMode, newConfig)
    multiWindowTransitions.incrementAndGet()
  }

  override fun onPictureInPictureModeChanged(isInPictureInPictureMode: Boolean, newConfig: android.content.res.Configuration) {
    super.onPictureInPictureModeChanged(isInPictureInPictureMode, newConfig)
    multiWindowTransitions.incrementAndGet()
  }

  override fun onRequestPermissionsResult(
    requestCode: Int,
    permissions: Array<out String>,
    grantResults: IntArray,
  ) {
    super.onRequestPermissionsResult(requestCode, permissions, grantResults)
    if (requestCode == AV_PERMISSION_REQUEST) pending = false
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    current = this
    // A recreated activity (rotation, process restore) must keep the
    // assessment shield the Rust side asked for.
    if (shieldOn) applyShield(true)
    // Hide the native OS status bar (clock/battery) so the app owns the full
    // screen height. It can still be revealed with a swipe from the top edge.
    WindowCompat.getInsetsController(window, window.decorView).apply {
      hide(WindowInsetsCompat.Type.statusBars())
      systemBarsBehavior =
        WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
    }
    // Keep the app's process alive in the background so the Rust libp2p
    // task keeps its peer connections instead of being killed by Doze /
    // battery optimisation. See P2pForegroundService for details.
    P2pForegroundService.start(this)
  }

  override fun onDestroy() {
    if (current === this) current = null
    super.onDestroy()
  }
}
