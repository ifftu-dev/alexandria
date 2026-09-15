<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watchEffect } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { useLocalApi } from '@/composables/useLocalApi'
import AppLayout from '@/layouts/AppLayout.vue'
import BlankLayout from '@/layouts/BlankLayout.vue'
import { useProfiles, onProfileLocked, onProfileReady } from '@/composables/useProfiles'
import { useAccountStatus } from '@/composables/useAccountStatus'
import { initTheme, initThemeFromSettings } from '@/composables/useTheme'
import { initLocaleFromSettings } from '@/composables/useLocale'
import { initShortcutsFromSettings } from '@/composables/useKeyboardShortcuts'
import { initOmniRecentsFromSettings } from '@/composables/useOmniSearch'
import { initSentinelFlagsFromSettings, useSentinel } from '@/composables/useSentinel'
import SentinelDebugPip from '@/components/integrity/SentinelDebugPip.vue'
import EvidenceConsentModal from '@/components/integrity/EvidenceConsentModal.vue'
import FlaggedRunNotice from '@/components/integrity/FlaggedRunNotice.vue'
import SentinelLiveIndicator from '@/components/integrity/SentinelLiveIndicator.vue'
import InstallCliDialog from '@/components/developer/InstallCliDialog.vue'
import DiagnosticsControls from '@/components/developer/DiagnosticsControls.vue'
import UpdateBanner from '@/components/update/UpdateBanner.vue'
import { initUpdateCheck } from '@/composables/useAppUpdate'
import { useDeepLinks } from '@/deeplink/useDeepLinks'


import { clearSettingsCache, useSettings } from '@/composables/useSettings'
import { isMac } from '@/composables/usePlatform'
import { useDiagnostics } from '@/composables/useDiagnostics'

const { invoke } = useLocalApi()

/** Matches `APPEAL_WINDOW_DAYS` in `sentinel::evidence`. */
const EVIDENCE_RETENTION_DAYS = 14
const {
  pendingEvidenceConsent,
  clearPendingEvidenceConsent,
  stopForProfileLock,
  hydrateBehavioralProfile,
} = useSentinel()
const diagnostics = useDiagnostics()

// Apply stored theme immediately (before first render)
initTheme()

// Cmd/Ctrl + vertical scroll → horizontal scroll in overflow-x containers.
function onWheel(e: WheelEvent) {
  const mod = isMac ? e.metaKey : e.ctrlKey
  if (!mod) return
  // Only act on vertical wheel deltas.
  if (e.deltaY === 0) return
  const target = e.target as HTMLElement | null
  if (!target) return
  const scroller = target.closest('.overflow-x-auto, .scrollbar-thin') as HTMLElement | null
  if (!scroller) return
  // If the container can scroll horizontally, redirect.
  if (scroller.scrollWidth <= scroller.clientWidth) return
  e.preventDefault()
  scroller.scrollLeft += e.deltaY
}

const route = useRoute()
const router = useRouter()
const { initialize, isUnlocked, isLockBlocked, lockState, lockProfile } = useProfiles()
const { refreshAccountStatus } = useAccountStatus()

const ready = ref(false)
const isPublicRoute = computed(() => route.name === 'profiles' || route.name === 'onboarding')
const showLockScreen = computed(() => isLockBlocked.value || (ready.value && !isUnlocked.value && !isPublicRoute.value))

watchEffect(() => {
  if (ready.value && !isUnlocked.value && !isLockBlocked.value && !isPublicRoute.value) {
    void router.replace('/profiles')
  }
})

async function retryLock() {
  try {
    await lockProfile()
  } catch {
    // The lock screen keeps the failure visible and permits another retry.
  }
}

const layout = computed(() => {
  const meta = route.meta?.layout as string | undefined
  if (meta === 'blank') return BlankLayout
  return AppLayout
})

/**
 * Hydrate all per-profile settings consumers from the backend.
 * Called once a profile is unlocked. Safe to call repeatedly —
 * each `initXFromSettings` is idempotent.
 */
onProfileReady(async () => {
  await hydrateProfileScopedState()
  await diagnostics.initialize().catch(e => console.warn('[App] diagnostics hydration failed:', e))
  // Unlock just blurred/destroyed the password field; clear any leaked
  // Secure Event Input now (no focus event fires since the window is
  // already key).
  void invoke('release_secure_input').catch(() => {})
  // Anything this device still owes a service: a learner who deleted evidence
  // they had released, while offline or with the service down, asked for a
  // copy on somebody else's disk to be destroyed. That request is queued here
  // and is owed until it lands. Retrying it only when the Integrity settings
  // page happens to be opened would make it depend on the person going back to
  // check up on a deletion they were already told had been requested.
  //
  // Needs the vault, which is why it is here rather than in the Rust unlock
  // path: the withdrawal is signed with the learner's own key.
  void invoke('holder_retry_withdrawals').catch(() => {})
})
onProfileLocked(async () => {
  // Drop the in-memory settings cache so the picker (and the next
  // profile that unlocks) does not flash the previously-active
  // profile's preferences.
  try {
    await stopForProfileLock()
  } finally {
    diagnostics.resetForProfileLock()
    clearPendingEvidenceConsent()
    clearSettingsCache()
  }
})

async function hydrateProfileScopedState() {
  try {
    await useSettings().initialize()
    await Promise.all([
      initThemeFromSettings(),
      initLocaleFromSettings(),
      initShortcutsFromSettings(),
      initOmniRecentsFromSettings(),
      initSentinelFlagsFromSettings(),
      hydrateBehavioralProfile(),
    ])
  } catch (e) {
    console.warn('[App] settings hydration failed:', e)
  }
}

// macOS WKWebView leaks Secure Event Input after a password field is
// focused then navigated away from, which suppresses global hotkey tools
// (CGEventTaps) while Alexandria is foreground. WebKit re-asserts the
// leaked state each time the window becomes key, so clear it on every
// focus — but never while a password field is genuinely focused, so real
// password entry stays protected.
function onWindowFocus() {
  const el = document.activeElement as HTMLInputElement | null
  if (el && el.tagName === 'INPUT' && el.type === 'password') return
  void invoke('release_secure_input').catch(() => {})
}

// Same leak, intra-window variant. `onWindowFocus` only fires on a window
// focus transition, so it misses the case where a password field is blurred
// while the window stays key — e.g. entering a vault password in Settings or
// Onboarding, then clicking elsewhere in the app without ever switching away.
// `focusout` bubbles (unlike `blur`), so one document listener covers every
// password field. Release when focus leaves a password field for anything
// that is not itself a password field.
function onFocusOut(e: FocusEvent) {
  const from = e.target as HTMLElement | null
  if (!(from instanceof HTMLInputElement) || from.type !== 'password') return
  const to = e.relatedTarget as HTMLElement | null
  if (to instanceof HTMLInputElement && to.type === 'password') return
  void invoke('release_secure_input').catch(() => {})
}

onMounted(async () => {
  document.addEventListener('wheel', onWheel, { passive: false })
  window.addEventListener('focus', onWindowFocus)
  document.addEventListener('focusout', onFocusOut)

  try {
    const state = await initialize()

    if (state === 'onboarding' && route.name !== 'onboarding') {
      router.replace('/onboarding')
    } else if (state === 'picker' && route.name !== 'profiles' && route.name !== 'onboarding') {
      router.replace('/profiles')
    } else if (state === 'ready') {
      await hydrateProfileScopedState()
      await diagnostics.initialize().catch(e => console.warn('[App] diagnostics hydration failed:', e))
      // A gated minor profile (awaiting guardian activation) must land on
      // the gate, not the app. The router guard covers later navigations;
      // this covers the initial one, which resolves before initialize().
      const account = await refreshAccountStatus()
      if (account?.activation_state === 'pending_guardian' && route.name !== 'guardian-gate') {
        router.replace('/guardian-gate')
      }
    }
  } catch {
    if (route.name !== 'onboarding' && route.name !== 'profiles') {
      router.replace('/profiles')
    }
  }

  ready.value = true

  // Start deep-link handling after the boot route is resolved: a cold-start
  // URL dispatches now (navigating if a profile is already unlocked, else
  // queuing until one unlocks). Warm opens are handled for the app's lifetime.
  void useDeepLinks().init()

  // Show the window now that the frontend is rendered and themed
  getCurrentWebviewWindow().show()

  // Silent, fail-closed check for a signed update — reveals the banner only
  // when one is genuinely available. Desktop Tauri only; inert elsewhere.
  initUpdateCheck()
})

onUnmounted(() => {
  document.removeEventListener('wheel', onWheel)
  window.removeEventListener('focus', onWindowFocus)
  document.removeEventListener('focusout', onFocusOut)
})
</script>

<template>
  <div v-if="showLockScreen" class="flex items-center justify-center h-full bg-background safe-area-top" role="status" aria-live="polite">
    <div class="text-center max-w-sm p-6">
      <template v-if="lockState === 'failed'">
        <p class="text-sm text-muted-foreground">{{ $t('common.app.lockFailed') }}</p>
        <button type="button" class="mt-4 rounded-lg bg-primary px-4 py-2 text-primary-foreground" @click="retryLock">
          {{ $t('common.actions.retry') }}
        </button>
      </template>
      <template v-else>
        <div class="w-8 h-8 border-2 border-primary border-t-transparent rounded-full animate-spin mx-auto mb-3" />
        <p class="text-sm text-muted-foreground">{{ $t('common.app.locking') }}</p>
      </template>
    </div>
  </div>
  <div v-else-if="!ready" class="flex items-center justify-center h-full bg-background safe-area-top">
    <div class="text-center">
      <div class="w-8 h-8 border-2 border-primary border-t-transparent rounded-full animate-spin mx-auto mb-3" />
      <p class="text-sm text-muted-foreground">{{ $t('common.app.initializing') }}</p>
    </div>
  </div>
  <component v-else :is="layout">
    <router-view />
  </component>

  <!-- Signed-update prompt; self-hides until a check finds an update. -->
  <UpdateBanner />

  <!-- Live Sentinel observability PiP. It is not mounted outside explicit
       diagnostics mode, so its listeners and camera cannot survive exit. -->
  <SentinelDebugPip v-if="isUnlocked && diagnostics.enabled.value" />

  <!-- Shows what Sentinel can see while a session is running, so avoidable
       flags can be avoided. Self-hides when no session is active. -->
  <SentinelLiveIndicator v-if="isUnlocked" />

  <!-- Offers the evidence decision when a session ends flagged. Mounted at the
       root because the assessment view that was running is usually gone by the
       time the session ends, and a learner must not miss this — it is the only
       moment the decision is theirs to make. -->
  <EvidenceConsentModal
    v-if="isUnlocked && pendingEvidenceConsent"
    :open="true"
    :session-id="pendingEvidenceConsent.sessionId"
    :reasons="pendingEvidenceConsent.reasons"
    :retention-days="EVIDENCE_RETENTION_DAYS"
    @close="clearPendingEvidenceConsent()"
    @decided="clearPendingEvidenceConsent()"
  />

  <!-- Says so when a service has flagged one of this person's assessments.
       Mounted at the root, and not on the Integrity settings page, because
       finding out you have been accused of something by happening to open a
       settings page is not being told. Self-hides when there is nothing new. -->
  <FlaggedRunNotice v-if="isUnlocked" />

  <!-- CLI installer — available only from explicit diagnostics mode. -->
  <InstallCliDialog v-if="!showLockScreen && diagnostics.enabled.value" />

  <DiagnosticsControls v-if="isUnlocked" />
</template>
