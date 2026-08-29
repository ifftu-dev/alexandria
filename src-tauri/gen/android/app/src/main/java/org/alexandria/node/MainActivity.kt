package org.alexandria.node

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.enableEdgeToEdge
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat

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
