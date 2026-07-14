import { readonly, ref } from 'vue'
import { check, type Update } from '@tauri-apps/plugin-updater'
import { relaunch } from '@tauri-apps/plugin-process'
import { invoke } from '@tauri-apps/api/core'
import { getVersion } from '@tauri-apps/api/app'
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

/**
 * True when an update exists but can't be self-installed — i.e. on mobile,
 * where the tauri updater is unsupported and app stores forbid self-updating.
 * The banner then links out to the release page instead of offering Install.
 */
const manualOnly = ref(false)
/** Where to send the user to get the update manually (mobile). */
const manualUrl = ref<string | null>(null)

let pendingUpdate: Update | null = null

/**
 * Desktop Tauri (macOS/Windows/Linux) — where the updater plugin can actually
 * download + install an update. Mobile is handled by the manual-notice path.
 */
function supported(): boolean {
  return !isMobilePlatform && currentPlatform !== 'unknown'
}

/** Compare dotted numeric cores (ignoring any `-prerelease` suffix). */
function isNewer(remote: string, local: string): boolean {
  const core = (v: string) => (v.split('-')[0] ?? '').split('.').map((n) => parseInt(n, 10) || 0)
  const r = core(remote)
  const l = core(local)
  for (let i = 0; i < Math.max(r.length, l.length); i++) {
    const a = r[i] ?? 0
    const b = l[i] ?? 0
    if (a !== b) return a > b
  }
  // Same numeric core: treat a differing string as "not newer" — alpha tracks
  // bump the core on every release, so this avoids nagging within a version.
  return false
}

/**
 * Mobile update notice: fetch the published manifest via the Rust command
 * (the webview CSP blocks a direct fetch to GitHub), compare to the running
 * version, and — if newer — surface a link-out banner. Never self-installs.
 */
async function checkMobileUpdate(): Promise<void> {
  try {
    const info = await invoke<{
      version: string
      notes: string
      releases_url: string
    } | null>('fetch_update_manifest')
    if (!info) return
    const current = await getVersion()
    if (isNewer(info.version, current)) {
      availableVersion.value = info.version
      currentVersion.value = current
      releaseNotes.value = info.notes || null
      manualUrl.value = info.releases_url
      manualOnly.value = true
      bannerDismissed.value = false
      phase.value = 'available'
    }
  } catch (e) {
    console.warn('[update] mobile check failed:', e)
  }
}

/**
 * Query the update endpoint. `silent` suppresses the "you're up to date" and
 * error states so the on-launch check never nags — it only ever reveals the
 * banner when an update genuinely exists.
 */
async function checkForUpdate(opts: { silent?: boolean } = {}): Promise<void> {
  if (phase.value === 'checking' || phase.value === 'downloading') return

  // Mobile can't self-install — fall back to the link-out notice.
  if (!supported()) {
    if (isMobilePlatform) await checkMobileUpdate()
    return
  }

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

/** Open the manual-download page (mobile). */
function openManual(): void {
  if (manualUrl.value) window.open(manualUrl.value, '_blank', 'noopener,noreferrer')
}

/** Fire-and-forget silent check for App start-up (desktop self-update + mobile notice). */
export function initUpdateCheck(): void {
  if (!supported() && !isMobilePlatform) return
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
    manualOnly: readonly(manualOnly),
    manualUrl: readonly(manualUrl),
    supported,
    checkForUpdate,
    downloadAndInstall,
    dismissBanner,
    openManual,
  }
}
