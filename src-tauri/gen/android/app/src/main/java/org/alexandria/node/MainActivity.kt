package org.alexandria.node

import android.os.Bundle
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    // Keep the app's process alive in the background so the Rust libp2p
    // task keeps its peer connections instead of being killed by Doze /
    // battery optimisation. See P2pForegroundService for details.
    P2pForegroundService.start(this)
  }
}
