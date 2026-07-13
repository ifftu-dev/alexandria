import { readonly, ref } from 'vue'
import { check, type Update } from '@tauri-apps/plugin-updater'
import { relaunch } from '@tauri-apps/plugin-process'
import { isMobilePlatform, currentPlatform } from '@/composables/usePlatform'

/**
 * In-app auto-update flow (desktop only).
 *
 * The Rust side registers `tauri-plugin-updater` (pubkey + GitHub `latest.json`
 * endpoint in `tauri.conf.json`) and `tauri-plugin-process` (for relaunch).
 * This composable is the frontend half: a silent check on launch that surfaces
 * a banner when a signed update is available, plus a manual "Check for updates"
 * control in Settings. Nothing downloads or installs without the user asking.
 *
 * Guarded so it is inert outside a desktop Tauri runtime — the browser dev
 * server and mobile builds (which update through their app stores) never call
 * the plugin.
 */

export type UpdatePhase =
  | 'idle'
  | 'checking'
  | 'available'
  | 'uptodate'
  | 'downloading'
  | 'ready'
  | 'error'

const phase = ref<UpdatePhase>('idle')
const availableVersion = ref<string | null>(null)
const currentVersion = ref<string | null>(null)
const releaseNotes = ref<string | null>(null)
/** 0–1 download progress, or null before download begins / when size unknown. */
const downloadProgress = ref<number | null>(null)
const errorMessage = ref<string | null>(null)
/** Dismissing the banner hides it until the next check finds a newer version. */
const bannerDismissed = ref(false)

let pendingUpdate: Update | null = null

/** Desktop Tauri only — the updater plugin is absent everywhere else. */
function supported(): boolean {
  return !isMobilePlatform && currentPlatform !== 'unknown'
}

/**
 * Query the update endpoint. `silent` suppresses the "you're up to date" and
 * error states so the on-launch check never nags — it only ever reveals the
 * banner when an update genuinely exists.
 */
async function checkForUpdate(opts: { silent?: boolean } = {}): Promise<void> {
  if (!supported() || phase.value === 'checking' || phase.value === 'downloading') return

  phase.value = 'checking'
  errorMessage.value = null
  try {
    const update = await check()
    if (update) {
      pendingUpdate = update
      availableVersion.value = update.version
      currentVersion.value = update.currentVersion
      releaseNotes.value = update.body ?? null
      bannerDismissed.value = false
      phase.value = 'available'
    } else {
      pendingUpdate = null
      availableVersion.value = null
      phase.value = opts.silent ? 'idle' : 'uptodate'
    }
  } catch (e) {
    // A silent launch check must fail closed — no update is available if we
    // cannot reach or verify the endpoint, and the user sees nothing.
    console.warn('[update] check failed:', e)
    if (opts.silent) {
      phase.value = 'idle'
    } else {
      errorMessage.value = e instanceof Error ? e.message : String(e)
      phase.value = 'error'
    }
  }
}

/**
 * Download the pending update (signature-verified by the plugin against the
 * configured pubkey) and install it, then relaunch into the new version.
 */
async function downloadAndInstall(): Promise<void> {
  if (!pendingUpdate || phase.value === 'downloading') return

  phase.value = 'downloading'
  downloadProgress.value = null
  errorMessage.value = null

  let total = 0
  let received = 0
  try {
    await pendingUpdate.downloadAndInstall((event) => {
      switch (event.event) {
        case 'Started':
          total = event.data.contentLength ?? 0
          received = 0
          downloadProgress.value = total > 0 ? 0 : null
          break
        case 'Progress':
          received += event.data.chunkLength
          if (total > 0) downloadProgress.value = Math.min(1, received / total)
          break
        case 'Finished':
          downloadProgress.value = 1
          break
      }
    })
    phase.value = 'ready'
    await relaunch()
  } catch (e) {
    console.error('[update] install failed:', e)
    errorMessage.value = e instanceof Error ? e.message : String(e)
    phase.value = 'error'
  }
}

function dismissBanner(): void {
  bannerDismissed.value = true
}

/** Fire-and-forget silent check for App start-up. */
export function initUpdateCheck(): void {
  if (!supported()) return
  void checkForUpdate({ silent: true })
}

export function useAppUpdate() {
  return {
    phase: readonly(phase),
    availableVersion: readonly(availableVersion),
    currentVersion: readonly(currentVersion),
    releaseNotes: readonly(releaseNotes),
    downloadProgress: readonly(downloadProgress),
    errorMessage: readonly(errorMessage),
    bannerDismissed: readonly(bannerDismissed),
    supported,
    checkForUpdate,
    downloadAndInstall,
    dismissBanner,
  }
}
