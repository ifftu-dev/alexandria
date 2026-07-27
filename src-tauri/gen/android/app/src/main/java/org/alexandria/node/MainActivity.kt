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
  }

  /**
   * Ask for camera and microphone up front.
   *
   * The WebView's own permission handler only fires when a *page* calls
   * getUserMedia. Live tutoring opens both devices from Rust instead — cpal for
   * the mic, the NDK Camera2 API for video — and those calls never reach that
   * handler: they simply fail. A denied mic surfaced as a cpal
   * BackendSpecific("Internal") error on stream build, which aborted the
   * process under panic=abort, so the permission is requested here rather than
   * discovered at session start.
   */
  private fun requestAvPermissionsIfNeeded() {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.M) return
    val missing = AV_PERMISSIONS.filter {
      ContextCompat.checkSelfPermission(this, it) != PackageManager.PERMISSION_GRANTED
    }
    if (missing.isNotEmpty()) {
      ActivityCompat.requestPermissions(this, missing.toTypedArray(), AV_PERMISSION_REQUEST)
    }
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    requestAvPermissionsIfNeeded()
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
}
