package org.alexandria.node

import android.os.Bundle
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
  }

  override fun onWebViewCreate(webView: WebView) {
    // Android WebView does not populate CSS env(safe-area-inset-*) even with
    // enableEdgeToEdge().  Read the actual window insets from the native side
    // and inject them as CSS custom properties so the safe-area utility classes
    // in main.css can use them via var(--sat, env(safe-area-inset-top)) etc.
    ViewCompat.setOnApplyWindowInsetsListener(webView) { view, insets ->
      val systemBars = insets.getInsets(WindowInsetsCompat.Type.systemBars())
      val density = view.resources.displayMetrics.density
      // Convert px to CSS px (device-independent) by dividing by density
      val top = systemBars.top / density
      val bottom = systemBars.bottom / density
      val left = systemBars.left / density
      val right = systemBars.right / density
      val js = """
        (function() {
          var s = document.documentElement.style;
          s.setProperty('--sat', '${top}px');
          s.setProperty('--sab', '${bottom}px');
          s.setProperty('--sal', '${left}px');
          s.setProperty('--sar', '${right}px');
        })();
      """.trimIndent()
      webView.evaluateJavascript(js, null)
      // Return insets unmodified — CSS handles the padding
      insets
    }
    // Request initial insets application
    ViewCompat.requestApplyInsets(webView)
  }
}
