<script setup lang="ts">
import { ref, onMounted } from 'vue'
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
import { AppButton, AppInput, AppTextarea, AppModal, AppAlert } from '@/components/ui'
import type { Identity } from '@/types'

const { invoke } = useLocalApi()
const { identity, lockVault: authLock, exportMnemonic: authExport, refreshProfile } = useAuth()
const { status: p2pStatus, refreshStatus: refreshP2pStatus } = useP2P()
const { theme, toggleTheme } = useTheme()
const { shortcuts, updateShortcut, resetShortcut, resetAll: resetAllShortcuts } =
  useKeyboardShortcuts()
const router = useRouter()

const displayName = ref('')
const bio = ref('')
const saving = ref(false)
const message = ref('')

const publishing = ref(false)
const publishMessage = ref('')

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

// Storage management
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

// Keyboard shortcut recording state — when a user clicks "edit" on a
// shortcut row, we put its id here and capture the next keydown.
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

onMounted(() => {
  if (identity.value) {
    displayName.value = identity.value.display_name ?? ''
    bio.value = identity.value.bio ?? ''
  }

  void refreshBiometricState()
  void loadStorageStats()
  void refreshP2pStatus()
})

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
</script>

<template>
  <div>
    <div class="max-w-2xl">
      <div class="mb-8">
        <h1 class="text-3xl font-bold text-foreground">Settings</h1>
        <p class="mt-2 text-muted-foreground">
          Manage your profile, security, and node settings.
        </p>
      </div>

      <!-- Profile -->
      <div class="rounded-xl bg-card shadow-sm p-6 mb-6">
        <h2 class="text-lg font-semibold text-foreground mb-4">Profile</h2>

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

          <div class="flex items-center gap-3">
            <AppButton :loading="saving" @click="saveProfile">
              Save Profile
            </AppButton>
            <AppButton variant="outline" :loading="publishing" @click="publishProfile">
              Publish to Network
            </AppButton>
            <span v-if="message" class="text-xs text-emerald-600 dark:text-emerald-400">{{ message }}</span>
            <span v-if="publishMessage" class="text-xs text-emerald-600 dark:text-emerald-400">{{ publishMessage }}</span>
          </div>

          <div v-if="identity?.profile_hash" class="pt-3 border-t border-border/50">
            <p class="text-xs text-muted-foreground mb-1">Published Profile Hash</p>
            <code class="block font-mono text-xs break-all select-all text-muted-foreground">{{ identity.profile_hash }}</code>
          </div>
        </div>
      </div>

      <!-- Security -->
      <div class="rounded-xl bg-card shadow-sm p-6 mb-6">
        <h2 class="text-lg font-semibold text-foreground mb-4">Security</h2>

        <div class="divide-y divide-border/50">
          <div class="flex items-center justify-between py-4 first:pt-0">
            <div>
              <p class="text-sm font-medium text-foreground">Recovery Phrase</p>
              <p class="text-xs text-muted-foreground">
                Export your 24-word backup phrase
              </p>
            </div>
            <AppButton variant="outline" size="sm" @click="openExportModal">
              Export
            </AppButton>
          </div>

          <div class="flex items-center justify-between py-4">
            <div>
              <p class="text-sm font-medium text-foreground">Lock Wallet</p>
              <p class="text-xs text-muted-foreground">
                Clear secrets from memory and require password
              </p>
            </div>
            <AppButton variant="outline" size="sm" :loading="locking" @click="lockWallet">
              Lock
            </AppButton>
          </div>

          <div class="py-4">
            <div class="flex items-center justify-between gap-3">
              <div>
                <p class="text-sm font-medium text-foreground">Biometric Unlock</p>
                <p class="text-xs text-muted-foreground">
                  Use Touch ID/Face ID to unlock this device vault
                </p>
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

            <div v-if="!biometricAvailable" class="mt-2 text-xs text-muted-foreground">
              Biometrics are not available on this device/runtime.
            </div>

            <div v-else-if="!biometricEnabled" class="mt-3 flex items-end gap-2">
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

            <div v-else class="mt-2 text-xs text-emerald-600 dark:text-emerald-400">
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
      </div>

      <!-- Theme -->
      <div class="rounded-xl bg-card shadow-sm p-6 mb-6">
        <h2 class="text-lg font-semibold text-foreground mb-4">Appearance</h2>
        <div class="flex items-center justify-between">
          <div>
            <p class="text-sm font-medium text-foreground">Theme</p>
            <p class="text-xs text-muted-foreground">
              Current: {{ theme }}
            </p>
          </div>
          <AppButton variant="outline" size="sm" @click="toggleTheme">
            Toggle ({{ theme === 'light' ? 'Dark' : theme === 'dark' ? 'System' : 'Light' }})
          </AppButton>
        </div>
      </div>

      <!-- Keyboard Shortcuts -->
      <div class="rounded-xl bg-card shadow-sm p-6 mb-6">
        <div class="flex items-center justify-between mb-4">
          <h2 class="text-lg font-semibold text-foreground">Keyboard Shortcuts</h2>
          <AppButton variant="ghost" size="sm" @click="resetAllShortcuts">
            Reset all
          </AppButton>
        </div>
        <p class="text-xs text-muted-foreground mb-4">
          Click a shortcut's key binding to record a new one. Press Escape to cancel.
        </p>
        <div class="divide-y divide-border/50">
          <div
            v-for="def in Object.values(shortcuts)"
            :key="def.id"
            class="flex items-center justify-between py-3 first:pt-0 last:pb-0"
          >
            <span class="text-sm text-foreground">{{ def.label }}</span>
            <div class="flex items-center gap-2">
              <!-- Recording mode: capture next keypress -->
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
              <!-- Display mode -->
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

      <!-- Storage -->
      <div class="rounded-xl bg-card shadow-sm p-6 mb-6">
        <h2 class="text-lg font-semibold text-foreground mb-4">Storage</h2>

        <div v-if="storageStats" class="space-y-4">
          <!-- Usage bar -->
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

          <!-- Quota selector -->
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

          <!-- Free space button -->
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

        <div v-else class="text-sm text-muted-foreground">
          Loading storage information...
        </div>
      </div>

      <!-- Identity (read-only) -->
      <div v-if="identity" class="rounded-xl bg-card shadow-sm p-6">
        <h2 class="text-lg font-semibold text-foreground mb-4">Identity</h2>
        <div class="divide-y divide-border/50">
          <div class="py-3 first:pt-0">
            <p class="text-xs text-muted-foreground mb-1">Stake Address</p>
            <code class="block font-mono text-xs break-all select-all text-foreground">{{ identity.stake_address }}</code>
          </div>
          <div class="py-3">
            <p class="text-xs text-muted-foreground mb-1">Payment Address</p>
            <code class="block font-mono text-xs break-all select-all text-foreground">{{ identity.payment_address }}</code>
          </div>
          <div class="py-3 last:pb-0">
            <p class="text-xs text-muted-foreground mb-1">Peer ID</p>
            <code v-if="p2pStatus?.peer_id" class="block font-mono text-xs break-all select-all text-foreground">{{ p2pStatus.peer_id }}</code>
            <p v-else class="text-xs text-muted-foreground italic">Network offline</p>
          </div>
        </div>
      </div>

      <!-- Export Recovery Phrase Modal -->
      <AppModal
        :open="showExportModal"
        title="Export Recovery Phrase"
        max-width="28rem"
        @close="closeExportModal"
      >
        <!-- Warning step -->
        <div v-if="!exportedMnemonic && !exportConfirmed">
          <AppAlert variant="error" class="mb-4">
            Your recovery phrase gives full access to your identity and credentials.
            Only export it in a private, secure environment.
          </AppAlert>

          <div class="flex gap-2">
            <AppButton variant="ghost" class="flex-1" @click="closeExportModal">
              Cancel
            </AppButton>
            <AppButton variant="danger" class="flex-1" @click="confirmExport">
              I Understand, Continue
            </AppButton>
          </div>
        </div>

        <!-- Password re-entry step -->
        <div v-else-if="exportConfirmed && !exportedMnemonic && !exporting && !exportError">
          <p class="text-sm text-muted-foreground mb-3">
            Enter your vault password to confirm.
          </p>
          <AppInput
            v-model="exportPassword"
            type="password"
            placeholder="Vault password"
            class="mb-3"
            @keyup.enter="doExport"
          />
          <div class="flex gap-2">
            <AppButton variant="ghost" class="flex-1" @click="closeExportModal">
              Cancel
            </AppButton>
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

        <!-- Loading -->
        <div v-else-if="exporting" class="text-center py-4">
          <div class="w-6 h-6 border-2 border-primary border-t-transparent rounded-full animate-spin mx-auto mb-2" />
          <p class="text-sm text-muted-foreground">Decrypting...</p>
        </div>

        <!-- Error (allow retry) -->
        <div v-else-if="exportError">
          <AppAlert variant="error" class="mb-3">{{ exportError }}</AppAlert>
          <div class="flex gap-2">
            <AppButton variant="outline" class="flex-1" @click="closeExportModal">
              Close
            </AppButton>
            <AppButton variant="ghost" class="flex-1" @click="exportError = ''">
              Try Again
            </AppButton>
          </div>
        </div>

        <!-- Mnemonic display -->
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

          <AppButton class="w-full" @click="closeExportModal">
            Done
          </AppButton>
        </div>
      </AppModal>
    </div>
  </div>
</template>
