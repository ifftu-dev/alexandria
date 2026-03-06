import { platform } from '@tauri-apps/plugin-os'

/**
 * Cached platform detection using Tauri's native `platform()` API.
 *
 * Replaces all `navigator.userAgent` sniffing with a reliable,
 * Tauri-provided platform string.  The value is computed once
 * and reused across all call-sites.
 */

let _platform: string | null = null

function getPlatform(): string {
  if (_platform === null) {
    try {
      _platform = platform()
    } catch {
      // Outside Tauri runtime (e.g. dev server in browser) — fall back
      _platform = 'unknown'
    }
  }
  return _platform
}

/** True on iOS or Android — i.e. a mobile Tauri build. */
export const isMobilePlatform = (() => {
  const p = getPlatform()
  return p === 'ios' || p === 'android'
})()

/** True on macOS (but not iOS). */
export const isMac = getPlatform() === 'macos'

/** True on iOS specifically. */
export const isIOS = getPlatform() === 'ios'

/** True on Android specifically. */
export const isAndroid = getPlatform() === 'android'

/** The raw platform string from Tauri. */
export const currentPlatform = getPlatform()

/**
 * Composable wrapper — useful when importing inside `<script setup>`.
 *
 * ```vue
 * <script setup>
 * import { usePlatform } from '@/composables/usePlatform'
 * const { isMobilePlatform, isMac } = usePlatform()
 * </script>
 * ```
 */
export function usePlatform() {
  return {
    isMobilePlatform,
    isMac,
    isIOS,
    isAndroid,
    platform: currentPlatform,
  } as const
}
