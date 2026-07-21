package org.alexandria.node

import android.os.Bundle
import androidx.activity.enableEdgeToEdge
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
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
