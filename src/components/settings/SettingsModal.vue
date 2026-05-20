<script setup lang="ts">
import { ref, watch, onMounted, onUnmounted } from 'vue'
import { useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { useAuth } from '@/composables/useAuth'
import { useP2P } from '@/composables/useP2P'
import { useTheme } from '@/composables/useTheme'
import {
  useKeyboardShortcuts,
  formatCombo,
  comboFromEvent,
} from '@/composables/useKeyboardShortcuts'
import {
  biometricCredentialExists,
  biometricSupported,
  clearBiometricVaultPassword,
  getBiometricStatus,
  storeVaultPasswordForBiometric,
} from '@/composables/useBiometricVault'
import { useSettingsModal, type SettingsSectionId } from '@/composables/useSettingsModal'
import { AppButton, AppInput, AppTextarea, AppModal, AppAlert } from '@/components/ui'
import AdvancedSettingsPanel from '@/components/settings/AdvancedSettingsPanel.vue'
import type { Identity } from '@/types'

const { invoke } = useLocalApi()
const { identity, lockVault: authLock, exportMnemonic: authExport, refreshProfile } = useAuth()
const { status: p2pStatus, refreshStatus: refreshP2pStatus } = useP2P()
const { theme, setTheme } = useTheme()
const { shortcuts, updateShortcut, resetShortcut, resetAll: resetAllShortcuts } =
  useKeyboardShortcuts()
const { isOpen, activeSection, close, setSection } = useSettingsModal()
const router = useRouter()

// ---- Profile ----
const displayName = ref('')
const bio = ref('')
const saving = ref(false)
const message = ref('')

const publishing = ref(false)
const publishMessage = ref('')

// ---- Security ----
const showExportModal = ref(false)
const exportConfirmed = ref(false)
const exportedMnemonic = ref('')
const exportPassword = ref('')
const exportError = ref('')
const exporting = ref(false)
const locking = ref(false)
const biometricAvailable = ref(false)
const biometricEnabled = ref(false)
const biometricBusy = ref(false)
const biometricPassword = ref('')
const biometricMessage = ref('')
const biometricDiagnostics = ref('')

// ---- Storage ----
interface StorageStats {
  total_pinned_bytes: number
  quota_bytes: number
  evictable_bytes: number
  pin_count: number
  usage_percent: number | null
}

const QUOTA_OPTIONS = [
  { label: '1 GB', bytes: 1_073_741_824 },
  { label: '2 GB', bytes: 2_147_483_648 },
  { label: '5 GB', bytes: 5_368_709_120 },
  { label: '10 GB', bytes: 10_737_418_240 },
  { label: '25 GB', bytes: 26_843_545_600 },
  { label: 'Unlimited', bytes: 0 },
]

const storageStats = ref<StorageStats | null>(null)
const quotaBytes = ref(0)
const evicting = ref(false)
const storageMessage = ref('')

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  const i = Math.floor(Math.log(bytes) / Math.log(1024))
  return `${(bytes / Math.pow(1024, i)).toFixed(i > 0 ? 1 : 0)} ${units[i]}`
}

function usageColor(percent: number | null): string {
  if (percent === null) return 'bg-primary'
  if (percent < 70) return 'bg-emerald-500'
  if (percent < 90) return 'bg-amber-500'
  return 'bg-red-500'
}

async function loadStorageStats() {
  try {
    storageStats.value = await invoke<StorageStats>('storage_stats')
    quotaBytes.value = await invoke<number>('storage_get_quota')
  } catch (e) {
    console.error('Failed to load storage stats:', e)
  }
}

async function setQuota(bytes: number) {
  try {
    await invoke('storage_set_quota', { bytes })
    quotaBytes.value = bytes
    storageMessage.value = ''
    await loadStorageStats()
  } catch (e) {
    storageMessage.value = `Error: ${e}`
  }
}

async function freeSpace() {
  evicting.value = true
  storageMessage.value = ''
  try {
    const result = await invoke<{ blobs_evicted: number; bytes_freed: number }>('storage_evict_now')
    if (result.blobs_evicted > 0) {
      storageMessage.value = `Freed ${formatBytes(result.bytes_freed)} from ${result.blobs_evicted} item${result.blobs_evicted === 1 ? '' : 's'}.`
    } else {
      storageMessage.value = 'Nothing to free.'
    }
    await loadStorageStats()
  } catch (e) {
    storageMessage.value = `Error: ${e}`
  } finally {
    evicting.value = false
  }
}

// ---- Keyboard shortcut recording ----
const recordingShortcutId = ref<string | null>(null)

function startRecording(id: string) {
  recordingShortcutId.value = id
}

function cancelRecording() {
  recordingShortcutId.value = null
}

function onShortcutKeydown(e: KeyboardEvent) {
  e.preventDefault()
  e.stopPropagation()
  const combo = comboFromEvent(e)
  if (!combo || !recordingShortcutId.value) return
  updateShortcut(recordingShortcutId.value, combo)
  recordingShortcutId.value = null
}

// ---- Lifecycle: refresh when opened ----
async function hydrate() {
  if (identity.value) {
    displayName.value = identity.value.display_name ?? ''
    bio.value = identity.value.bio ?? ''
  }
  void refreshBiometricState()
  void loadStorageStats()
  void refreshP2pStatus()
}

watch(isOpen, (val) => {
  if (val) void hydrate()
  else {
    // reset transient state on close
    message.value = ''
    publishMessage.value = ''
    storageMessage.value = ''
    biometricMessage.value = ''
    biometricPassword.value = ''
    recordingShortcutId.value = null
  }
})

function onBackdropKeydown(e: KeyboardEvent) {
  if (!isOpen.value) return
  if (e.key === 'Escape' && !showExportModal.value) close()
}

onMounted(() => {
  document.addEventListener('keydown', onBackdropKeydown)
})
onUnmounted(() => {
  document.removeEventListener('keydown', onBackdropKeydown)
})

watch(isOpen, (val) => {
  if (typeof document === 'undefined') return
  document.body.style.overflow = val ? 'hidden' : ''
})

// ---- Section actions ----
async function refreshBiometricState() {
  try {
    const [status, enabled] = await Promise.all([
      getBiometricStatus(),
      biometricCredentialExists(),
    ])
    biometricAvailable.value = status.isAvailable
    biometricEnabled.value = enabled
    biometricDiagnostics.value = status.error
      ? `Status error${status.errorCode ? ` (${status.errorCode})` : ''}: ${status.error}`
      : ''
  } catch {
    biometricAvailable.value = false
    biometricEnabled.value = false
    biometricDiagnostics.value = ''
  }
}

async function saveProfile() {
  saving.value = true
  message.value = ''
  try {
    await invoke<Identity>('update_profile', {
      update: {
        display_name: displayName.value || null,
        bio: bio.value || null,
      },
    })
    await refreshProfile()
    message.value = 'Profile updated.'
  } catch (e) {
    message.value = `Error: ${e}`
  } finally {
    saving.value = false
  }
}

async function publishProfile() {
  publishing.value = true
  publishMessage.value = ''
  try {
    await invoke('publish_profile')
    publishMessage.value = 'Published!'
    await refreshProfile()
  } catch (e) {
    publishMessage.value = `Error: ${e}`
  } finally {
    publishing.value = false
  }
}

function openExportModal() {
  showExportModal.value = true
  exportConfirmed.value = false
  exportedMnemonic.value = ''
  exportPassword.value = ''
  exportError.value = ''
}

function closeExportModal() {
  showExportModal.value = false
  exportedMnemonic.value = ''
  exportPassword.value = ''
  exportConfirmed.value = false
}

function confirmExport() {
  exportConfirmed.value = true
  exportError.value = ''
}

async function doExport() {
  exporting.value = true
  exportError.value = ''
  try {
    exportedMnemonic.value = await authExport(exportPassword.value)
  } catch (e) {
    exportError.value = String(e)
  } finally {
    exporting.value = false
    exportPassword.value = ''
  }
}

async function lockWallet() {
  locking.value = true
  try {
    await authLock()
    close()
    router.replace('/unlock')
  } catch (e) {
    console.error('Failed to lock:', e)
  } finally {
    locking.value = false
  }
}

async function enableBiometric() {
  if (!biometricPassword.value) {
    biometricMessage.value = 'Enter your vault password to enable biometric unlock.'
    return
  }
  biometricBusy.value = true
  biometricMessage.value = ''
  try {
    const supported = await biometricSupported()
    if (!supported) {
      biometricMessage.value = 'Biometric support is unavailable right now on this runtime.'
      return
    }
    const mode = await storeVaultPasswordForBiometric(biometricPassword.value)
    biometricEnabled.value = true
    biometricPassword.value = ''
    biometricMessage.value = mode === 'secure'
      ? 'Biometric unlock enabled.'
      : 'Biometric unlock enabled for this app session only (dev runtime keychain entitlement limitation).'
  } catch (e) {
    const msg = String(e)
    if (msg.includes('-34018')) {
      biometricMessage.value = 'macOS keychain entitlement is missing for this runtime (-34018). Use a bundled/signed app build, then enable biometrics again.'
    } else {
      biometricMessage.value = `Failed to enable biometric unlock: ${msg}`
    }
  } finally {
    biometricBusy.value = false
    await refreshBiometricState()
  }
}

async function disableBiometric() {
  biometricBusy.value = true
  biometricMessage.value = ''
  try {
    await clearBiometricVaultPassword()
    biometricEnabled.value = false
    biometricMessage.value = 'Biometric credential cleared for this device.'
  } catch (e) {
    biometricMessage.value = `Failed to clear biometric credential: ${String(e)}`
  } finally {
    biometricBusy.value = false
    await refreshBiometricState()
  }
}

function gotoNetwork() {
  close()
  router.push('/dashboard/network')
}

async function copyText(value: string | undefined | null) {
  if (!value) return
  try {
    await navigator.clipboard.writeText(value)
  } catch {
    // clipboard may be unavailable — silently ignore
  }
}

// ---- Section nav metadata ----
interface SectionMeta {
  id: SettingsSectionId
  label: string
  desc: string
}
const SECTIONS: SectionMeta[] = [
  { id: 'account', label: 'Account & Identity', desc: 'Profile, addresses, peer' },
  { id: 'security', label: 'Security & Privacy', desc: 'Recovery, lock, biometric' },
  { id: 'personalization', label: 'Personalization', desc: 'Theme, shortcuts' },
  { id: 'system', label: 'System', desc: 'Storage, network' },
  { id: 'advanced', label: 'All settings', desc: 'Every per-profile setting' },
]
</script>

<template>
  <Teleport to="body">
    <Transition
      enter-active-class="transition duration-200 ease-out"
      enter-from-class="opacity-0"
      enter-to-class="opacity-100"
      leave-active-class="transition duration-150 ease-in"
      leave-from-class="opacity-100"
      leave-to-class="opacity-0"
    >
      <div
        v-if="isOpen"
        class="fixed inset-0 z-40 flex items-center justify-center bg-black/50 p-4"
        @mousedown.self.stop
        @click.self="close"
      >
        <Transition
          enter-active-class="transition duration-200 ease-out"
          enter-from-class="opacity-0 scale-95"
          enter-to-class="opacity-100 scale-100"
          leave-active-class="transition duration-150 ease-in"
          leave-from-class="opacity-100 scale-100"
          leave-to-class="opacity-0 scale-95"
        >
          <div
            v-if="isOpen"
            class="settings-shell w-full max-w-4xl flex flex-col sm:flex-row overflow-hidden rounded-2xl border border-border bg-card shadow-2xl"
            role="dialog"
            aria-modal="true"
            aria-labelledby="settings-title"
          >
            <!-- Sidebar nav -->
            <aside class="settings-sidebar shrink-0 sm:w-60 border-b sm:border-b-0 sm:border-r border-border bg-muted/20">
              <div class="px-4 pt-5 pb-3 flex items-center justify-between">
                <h2 id="settings-title" class="text-sm font-semibold tracking-wide uppercase text-muted-foreground">
                  Settings
                </h2>
                <button
                  class="sm:hidden p-1 rounded-md text-muted-foreground hover:bg-muted/50"
                  aria-label="Close settings"
                  @click="close"
                >
                  <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
                  </svg>
                </button>
              </div>

              <nav class="px-2 pb-4 flex sm:block overflow-x-auto sm:overflow-visible gap-1 sm:gap-0">
                <button
                  v-for="s in SECTIONS"
                  :key="s.id"
                  class="settings-nav-item"
                  :class="{ 'settings-nav-item--active': activeSection === s.id }"
                  @click="setSection(s.id)"
                >
                  <span class="settings-nav-icon" aria-hidden="true">
                    <!-- Account -->
                    <svg v-if="s.id === 'account'" class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.75">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14c-4.418 0-8 2.239-8 5v1h16v-1c0-2.761-3.582-5-8-5z" />
                    </svg>
                    <!-- Security -->
                    <svg v-else-if="s.id === 'security'" class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.75">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M12 11c-1.105 0-2 .895-2 2 0 .738.4 1.38 1 1.723V17h2v-2.277c.6-.343 1-.985 1-1.723 0-1.105-.895-2-2-2zM6 10V8a6 6 0 1112 0v2M5 10h14v10H5V10z" />
                    </svg>
                    <!-- Personalization -->
                    <svg v-else-if="s.id === 'personalization'" class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.75">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M7 21a4 4 0 01-4-4 4 4 0 014-4h1m11-7l-7 7m0 0l-3-3m3 3v6a4 4 0 11-4-4" />
                    </svg>
                    <!-- System -->
                    <svg v-else class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.75">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M4 7h16M4 12h16M4 17h16" />
                    </svg>
                  </span>
                  <span class="flex flex-col text-left min-w-0">
                    <span class="text-sm font-medium truncate">{{ s.label }}</span>
                    <span class="hidden sm:block text-[11px] text-muted-foreground truncate">{{ s.desc }}</span>
                  </span>
                </button>
              </nav>
            </aside>

            <!-- Content panel -->
            <section class="settings-content flex-1 flex flex-col min-w-0">
              <header class="flex items-center justify-between px-6 py-4 border-b border-border">
                <h3 class="text-base font-semibold text-foreground">
                  {{ SECTIONS.find(s => s.id === activeSection)?.label }}
                </h3>
                <button
                  class="hidden sm:inline-flex p-1 rounded-md text-muted-foreground hover:bg-muted/50 transition-colors"
                  aria-label="Close settings"
                  @click="close"
                >
                  <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
                  </svg>
                </button>
              </header>

              <div class="flex-1 overflow-y-auto px-6 py-5 space-y-8">
                <!-- ──────────── Account & Identity ──────────── -->
                <template v-if="activeSection === 'account'">
                  <div>
                    <h4 class="settings-group-title">Profile</h4>
                    <div class="space-y-4">
                      <AppInput
                        v-model="displayName"
                        label="Display Name"
                        placeholder="How others see you"
                      />
                      <AppTextarea
                        v-model="bio"
                        label="Bio"
                        placeholder="A short description about yourself"
                      />
                      <div class="flex flex-wrap items-center gap-3">
                        <AppButton :loading="saving" @click="saveProfile">
                          Save Profile
                        </AppButton>
                        <AppButton variant="outline" :loading="publishing" @click="publishProfile">
                          Publish to Network
                        </AppButton>
                        <span v-if="message" class="text-xs text-emerald-600 dark:text-emerald-400">{{ message }}</span>
                        <span v-if="publishMessage" class="text-xs text-emerald-600 dark:text-emerald-400">{{ publishMessage }}</span>
                      </div>
                    </div>
                  </div>

                  <div v-if="identity">
                    <h4 class="settings-group-title">Identity</h4>
                    <div class="space-y-3">
                      <div class="settings-row-stack">
                        <div class="flex items-center justify-between">
                          <p class="text-xs text-muted-foreground">Stake Address</p>
                          <button class="settings-copy-btn" @click="copyText(identity.stake_address)">Copy</button>
                        </div>
                        <code class="settings-code">{{ identity.stake_address }}</code>
                      </div>

                      <div class="settings-row-stack">
                        <div class="flex items-center justify-between">
                          <p class="text-xs text-muted-foreground">Payment Address</p>
                          <button class="settings-copy-btn" @click="copyText(identity.payment_address)">Copy</button>
                        </div>
                        <code class="settings-code">{{ identity.payment_address }}</code>
                      </div>

                      <div class="settings-row-stack">
                        <div class="flex items-center justify-between">
                          <p class="text-xs text-muted-foreground">Peer ID</p>
                          <button
                            v-if="p2pStatus?.peer_id"
                            class="settings-copy-btn"
                            @click="copyText(p2pStatus.peer_id)"
                          >
                            Copy
                          </button>
                        </div>
                        <code v-if="p2pStatus?.peer_id" class="settings-code">{{ p2pStatus.peer_id }}</code>
                        <p v-else class="text-xs text-muted-foreground italic">Network offline</p>
                      </div>

                      <div v-if="identity.profile_hash" class="settings-row-stack">
                        <div class="flex items-center justify-between">
                          <p class="text-xs text-muted-foreground">Published Profile Hash</p>
                          <button class="settings-copy-btn" @click="copyText(identity.profile_hash)">Copy</button>
                        </div>
                        <code class="settings-code">{{ identity.profile_hash }}</code>
                      </div>
                    </div>
                  </div>
                </template>

                <!-- ──────────── Security & Privacy ──────────── -->
                <template v-else-if="activeSection === 'security'">
                  <div>
                    <h4 class="settings-group-title">Wallet</h4>
                    <div class="divide-y divide-border/50 rounded-lg border border-border">
                      <div class="flex items-center justify-between gap-4 p-4">
                        <div>
                          <p class="text-sm font-medium text-foreground">Recovery Phrase</p>
                          <p class="text-xs text-muted-foreground">Export your 24-word backup phrase</p>
                        </div>
                        <AppButton variant="outline" size="sm" @click="openExportModal">
                          Export
                        </AppButton>
                      </div>
                      <div class="flex items-center justify-between gap-4 p-4">
                        <div>
                          <p class="text-sm font-medium text-foreground">Lock Wallet</p>
                          <p class="text-xs text-muted-foreground">Clear secrets from memory and require password</p>
                        </div>
                        <AppButton variant="outline" size="sm" :loading="locking" @click="lockWallet">
                          Lock
                        </AppButton>
                      </div>
                    </div>
                  </div>

                  <div>
                    <h4 class="settings-group-title">Device</h4>
                    <div class="rounded-lg border border-border p-4">
                      <div class="flex items-center justify-between gap-4">
                        <div>
                          <p class="text-sm font-medium text-foreground">Biometric Unlock</p>
                          <p class="text-xs text-muted-foreground">Use Touch ID/Face ID to unlock this device vault</p>
                        </div>
                        <AppButton
                          v-if="biometricEnabled"
                          variant="outline"
                          size="sm"
                          :loading="biometricBusy"
                          @click="disableBiometric"
                        >
                          Disable
                        </AppButton>
                      </div>

                      <div v-if="!biometricAvailable" class="mt-3 text-xs text-muted-foreground">
                        Biometrics are not available on this device/runtime.
                      </div>

                      <div v-else-if="!biometricEnabled" class="mt-3 flex flex-col sm:flex-row sm:items-end gap-2">
                        <div class="flex-1">
                          <AppInput
                            v-model="biometricPassword"
                            label="Vault Password"
                            type="password"
                            placeholder="Enter current vault password"
                          />
                        </div>
                        <AppButton size="sm" :loading="biometricBusy" @click="enableBiometric">
                          Enable
                        </AppButton>
                      </div>

                      <div v-else class="mt-3 text-xs text-emerald-600 dark:text-emerald-400">
                        Biometric unlock is enabled.
                      </div>

                      <p v-if="biometricMessage" class="mt-2 text-xs text-muted-foreground">
                        {{ biometricMessage }}
                      </p>
                      <p v-if="biometricDiagnostics" class="mt-1 text-xs text-muted-foreground/80">
                        {{ biometricDiagnostics }}
                      </p>
                    </div>
                  </div>
                </template>

                <!-- ──────────── Personalization ──────────── -->
                <template v-else-if="activeSection === 'personalization'">
                  <div>
                    <h4 class="settings-group-title">Theme</h4>
                    <div class="grid grid-cols-3 gap-2">
                      <button
                        v-for="opt in (['light', 'dark', 'system'] as const)"
                        :key="opt"
                        class="theme-card"
                        :class="{ 'theme-card--active': theme === opt }"
                        @click="setTheme(opt)"
                      >
                        <span class="theme-card-swatch" :class="`theme-card-swatch--${opt}`" aria-hidden="true" />
                        <span class="capitalize text-sm">{{ opt }}</span>
                      </button>
                    </div>
                  </div>

                  <div>
                    <div class="flex items-center justify-between mb-3">
                      <h4 class="settings-group-title mb-0">Keyboard Shortcuts</h4>
                      <AppButton variant="ghost" size="sm" @click="resetAllShortcuts">
                        Reset all
                      </AppButton>
                    </div>
                    <p class="text-xs text-muted-foreground mb-3">
                      Click a shortcut's key binding to record a new one. Press Escape to cancel.
                    </p>
                    <div class="divide-y divide-border/50 rounded-lg border border-border">
                      <div
                        v-for="def in Object.values(shortcuts)"
                        :key="def.id"
                        class="flex items-center justify-between px-4 py-3"
                      >
                        <span class="text-sm text-foreground">{{ def.label }}</span>
                        <div class="flex items-center gap-2">
                          <template v-if="recordingShortcutId === def.id">
                            <kbd
                              class="inline-flex min-w-[60px] items-center justify-center rounded-md border border-primary bg-primary/10 px-2 py-1 font-mono text-xs text-primary animate-pulse"
                              tabindex="0"
                              autofocus
                              @keydown="onShortcutKeydown"
                              @blur="cancelRecording"
                            >
                              Press keys…
                            </kbd>
                          </template>
                          <template v-else>
                            <button
                              class="inline-flex min-w-[60px] items-center justify-center rounded-md border border-border bg-muted/30 px-2 py-1 font-mono text-xs text-foreground transition-colors hover:bg-muted/60"
                              @click="startRecording(def.id)"
                            >
                              {{ formatCombo(def.keys) }}
                            </button>
                          </template>
                          <button
                            v-if="def.keys.key !== def.defaultKeys.key || def.keys.mod !== def.defaultKeys.mod || def.keys.shift !== def.defaultKeys.shift || def.keys.alt !== def.defaultKeys.alt"
                            class="text-xs text-muted-foreground hover:text-foreground transition-colors"
                            title="Reset to default"
                            @click="resetShortcut(def.id)"
                          >
                            reset
                          </button>
                        </div>
                      </div>
                    </div>
                  </div>
                </template>

                <!-- ──────────── System ──────────── -->
                <template v-else-if="activeSection === 'system'">
                  <div>
                    <h4 class="settings-group-title">Storage</h4>
                    <div v-if="storageStats" class="space-y-5 rounded-lg border border-border p-4">
                      <div>
                        <div class="flex items-center justify-between mb-1.5">
                          <p class="text-sm font-medium text-foreground">Content Cache</p>
                          <p class="text-xs text-muted-foreground">
                            {{ formatBytes(storageStats.total_pinned_bytes) }}
                            <template v-if="storageStats.quota_bytes > 0">
                              of {{ formatBytes(storageStats.quota_bytes) }}
                            </template>
                            <template v-else>
                              (unlimited)
                            </template>
                          </p>
                        </div>
                        <div class="h-2 rounded-full bg-muted/50 overflow-hidden">
                          <div
                            class="h-full rounded-full transition-all duration-300"
                            :class="usageColor(storageStats.usage_percent)"
                            :style="{ width: storageStats.usage_percent !== null ? `${Math.min(storageStats.usage_percent, 100)}%` : '0%' }"
                          />
                        </div>
                        <p class="mt-1 text-xs text-muted-foreground">
                          {{ storageStats.pin_count }} item{{ storageStats.pin_count === 1 ? '' : 's' }} cached
                          <template v-if="storageStats.evictable_bytes > 0">
                            &middot; {{ formatBytes(storageStats.evictable_bytes) }} can be freed
                          </template>
                        </p>
                      </div>

                      <div>
                        <p class="text-sm font-medium text-foreground mb-2">Disk Quota</p>
                        <div class="flex flex-wrap gap-2">
                          <button
                            v-for="option in QUOTA_OPTIONS"
                            :key="option.bytes"
                            class="px-3 py-1.5 text-xs font-medium rounded-lg border transition-colors"
                            :class="quotaBytes === option.bytes
                              ? 'bg-primary text-primary-foreground border-primary'
                              : 'bg-background text-foreground border-border hover:bg-muted/50'"
                            @click="setQuota(option.bytes)"
                          >
                            {{ option.label }}
                          </button>
                        </div>
                      </div>

                      <div class="flex items-center gap-3">
                        <AppButton
                          variant="outline"
                          size="sm"
                          :loading="evicting"
                          :disabled="storageStats.evictable_bytes === 0"
                          @click="freeSpace"
                        >
                          Free Space
                        </AppButton>
                        <span v-if="storageMessage" class="text-xs text-muted-foreground">{{ storageMessage }}</span>
                      </div>
                    </div>
                    <div v-else class="rounded-lg border border-border p-4 text-sm text-muted-foreground">
                      Loading storage information…
                    </div>
                  </div>

                  <div>
                    <h4 class="settings-group-title">Network</h4>
                    <div class="rounded-lg border border-border p-4">
                      <div class="flex items-center justify-between gap-4">
                        <div>
                          <p class="text-sm font-medium text-foreground">P2P Node</p>
                          <p class="text-xs text-muted-foreground">
                            <template v-if="p2pStatus?.is_running">
                              Connected · peer ID
                              <code class="font-mono text-[11px]">{{ p2pStatus.peer_id?.slice(0, 12) }}…</code>
                            </template>
                            <template v-else-if="p2pStatus">
                              Offline
                            </template>
                            <template v-else>
                              Starting…
                            </template>
                          </p>
                        </div>
                        <AppButton variant="outline" size="sm" @click="gotoNetwork">
                          Open network →
                        </AppButton>
                      </div>
                    </div>
                  </div>
                </template>

                <!-- ──────────── Advanced — every registered setting ──────────── -->
                <template v-else-if="activeSection === 'advanced'">
                  <AdvancedSettingsPanel />
                </template>
              </div>
            </section>
          </div>
        </Transition>

        <!-- Nested Export Recovery Phrase modal — sits above the settings shell -->
        <AppModal
          :open="showExportModal"
          title="Export Recovery Phrase"
          max-width="28rem"
          @close="closeExportModal"
        >
          <div v-if="!exportedMnemonic && !exportConfirmed">
            <AppAlert variant="error" class="mb-4">
              Your recovery phrase gives full access to your identity and credentials.
              Only export it in a private, secure environment.
            </AppAlert>
            <div class="flex gap-2">
              <AppButton variant="ghost" class="flex-1" @click="closeExportModal">Cancel</AppButton>
              <AppButton variant="danger" class="flex-1" @click="confirmExport">
                I Understand, Continue
              </AppButton>
            </div>
          </div>

          <div v-else-if="exportConfirmed && !exportedMnemonic && !exporting && !exportError">
            <p class="text-sm text-muted-foreground mb-3">Enter your vault password to confirm.</p>
            <AppInput
              v-model="exportPassword"
              type="password"
              placeholder="Vault password"
              class="mb-3"
              @keyup.enter="doExport"
            />
            <div class="flex gap-2">
              <AppButton variant="ghost" class="flex-1" @click="closeExportModal">Cancel</AppButton>
              <AppButton
                variant="danger"
                class="flex-1"
                :disabled="!exportPassword"
                @click="doExport"
              >
                Show Phrase
              </AppButton>
            </div>
          </div>

          <div v-else-if="exporting" class="text-center py-4">
            <div class="w-6 h-6 border-2 border-primary border-t-transparent rounded-full animate-spin mx-auto mb-2" />
            <p class="text-sm text-muted-foreground">Decrypting…</p>
          </div>

          <div v-else-if="exportError">
            <AppAlert variant="error" class="mb-3">{{ exportError }}</AppAlert>
            <div class="flex gap-2">
              <AppButton variant="outline" class="flex-1" @click="closeExportModal">Close</AppButton>
              <AppButton variant="ghost" class="flex-1" @click="exportError = ''">Try Again</AppButton>
            </div>
          </div>

          <div v-else-if="exportedMnemonic">
            <div class="grid grid-cols-2 sm:grid-cols-3 gap-2 mb-4">
              <div
                v-for="(word, i) in exportedMnemonic.split(' ')"
                :key="i"
                class="flex items-center gap-2 text-sm py-1.5 px-2.5 rounded bg-muted/30"
              >
                <span class="text-xs text-muted-foreground w-5 text-right">{{ i + 1 }}.</span>
                <span class="font-mono font-medium">{{ word }}</span>
              </div>
            </div>
            <AppButton class="w-full" @click="closeExportModal">Done</AppButton>
          </div>
        </AppModal>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.settings-shell {
  height: min(85vh, 720px);
  max-height: 85vh;
}

@media (max-width: 640px) {
  .settings-shell {
    height: 100%;
    max-height: 100%;
  }
}

.settings-sidebar {
  flex-shrink: 0;
}

.settings-nav-item {
  display: flex;
  align-items: center;
  gap: 0.625rem;
  width: 100%;
  padding: 0.5rem 0.75rem;
  border-radius: 0.5rem;
  color: var(--app-foreground);
  background: transparent;
  border: none;
  cursor: pointer;
  text-align: left;
  transition: background 0.15s, color 0.15s;
  flex-shrink: 0;
}

.settings-nav-item:hover {
  background: color-mix(in srgb, var(--app-muted) 50%, transparent);
}

.settings-nav-item--active {
  background: color-mix(in srgb, var(--app-primary) 10%, transparent);
  color: var(--app-primary);
}

.settings-nav-item--active .settings-nav-icon {
  color: var(--app-primary);
}

.settings-nav-icon {
  display: inline-flex;
  flex-shrink: 0;
  width: 1.5rem;
  height: 1.5rem;
  align-items: center;
  justify-content: center;
  color: var(--app-muted-foreground);
}

.settings-group-title {
  font-size: 0.75rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--app-muted-foreground);
  margin-bottom: 0.75rem;
}

.settings-row-stack {
  padding: 0.75rem;
  border-radius: 0.5rem;
  border: 1px solid var(--app-border);
  background: color-mix(in srgb, var(--app-muted) 15%, transparent);
}

.settings-code {
  display: block;
  margin-top: 0.375rem;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: 0.6875rem;
  word-break: break-all;
  user-select: all;
  color: var(--app-foreground);
}

.settings-copy-btn {
  font-size: 0.6875rem;
  color: var(--app-muted-foreground);
  background: transparent;
  border: none;
  padding: 0.125rem 0.375rem;
  border-radius: 0.25rem;
  cursor: pointer;
  transition: color 0.15s, background 0.15s;
}

.settings-copy-btn:hover {
  color: var(--app-foreground);
  background: color-mix(in srgb, var(--app-muted) 50%, transparent);
}

/* Theme picker cards */
.theme-card {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.5rem;
  padding: 0.75rem;
  border-radius: 0.5rem;
  border: 1px solid var(--app-border);
  background: var(--app-background);
  color: var(--app-foreground);
  cursor: pointer;
  transition: border-color 0.15s, background 0.15s;
}

.theme-card:hover {
  background: color-mix(in srgb, var(--app-muted) 30%, transparent);
}

.theme-card--active {
  border-color: var(--app-primary);
  background: color-mix(in srgb, var(--app-primary) 8%, transparent);
}

.theme-card-swatch {
  width: 100%;
  height: 2.5rem;
  border-radius: 0.375rem;
  border: 1px solid var(--app-border);
}

.theme-card-swatch--light {
  background: linear-gradient(135deg, #ffffff 50%, #f3f4f6 50%);
}

.theme-card-swatch--dark {
  background: linear-gradient(135deg, #0b0b0e 50%, #1f1f25 50%);
}

.theme-card-swatch--system {
  background: linear-gradient(135deg, #ffffff 50%, #0b0b0e 50%);
}
</style>
