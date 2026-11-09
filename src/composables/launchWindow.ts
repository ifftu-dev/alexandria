/**
 * Desktop launch handoff. The main window is created hidden with the theme
 * background (`tauri.conf.json`, `launch_window.rs`) and shown here once the
 * themed first frame is ready, so it never appears blank or in the wrong colour.
 */

/**
 * A hidden webview may never schedule an animation frame, so a short timer
 * shows the window anyway. A webview that never runs script at all is covered
 * by the Rust-side fallback in `launch_window.rs`.
 */
export const FIRST_PAINT_FALLBACK_MS = 150

export interface FirstPaintOptions {
  requestFrame?: (callback: FrameRequestCallback) => number
  fallbackMs?: number
}

/** Calls `show` exactly once: on the first frame, or after the fallback. */
export function showOnFirstPaint(show: () => void, options: FirstPaintOptions = {}): void {
  const requestFrame = options.requestFrame ?? (cb => window.requestAnimationFrame(cb))
  let shown = false
  const reveal = (): void => {
    if (shown) return
    shown = true
    clearTimeout(fallback)
    show()
  }
  const fallback = setTimeout(reveal, options.fallbackMs ?? FIRST_PAINT_FALLBACK_MS)
  requestFrame(() => reveal())
}
