<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { useAuth } from '@/composables/useAuth'
import { useTheme } from '@/composables/useTheme'
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
const { theme, toggleTheme } = useTheme()
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
const exportError = ref('')
const exporting = ref(false)
const locking = ref(false)
const biometricAvailable = ref(false)
const biometricEnabled = ref(false)
const biometricBusy = ref(false)
const biometricPassword = ref('')
const biometricMessage = ref('')
const biometricDiagnostics = ref('')

onMounted(() => {
  if (identity.value) {
    displayName.value = identity.value.display_name ?? ''
    bio.value = identity.value.bio ?? ''
  }

  void refreshBiometricState()
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
  exportError.value = ''
}

function closeExportModal() {
  showExportModal.value = false
  exportedMnemonic.value = ''
  exportConfirmed.value = false
}

async function doExport() {
  exportConfirmed.value = true
  exporting.value = true
  exportError.value = ''

  try {
    exportedMnemonic.value = await authExport()
  } catch (e) {
    exportError.value = String(e)
  } finally {
    exporting.value = false
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
        </div>
      </div>

      <!-- Export Recovery Phrase Modal -->
      <AppModal
        :open="showExportModal"
        title="Export Recovery Phrase"
        max-width="28rem"
        @close="closeExportModal"
      >
        <!-- Confirmation step -->
        <div v-if="!exportedMnemonic && !exportConfirmed">
          <AppAlert variant="error" class="mb-4">
            Your recovery phrase gives full access to your identity and credentials.
            Only export it in a private, secure environment.
          </AppAlert>

          <div class="flex gap-2">
            <AppButton variant="ghost" class="flex-1" @click="closeExportModal">
              Cancel
            </AppButton>
            <AppButton variant="danger" class="flex-1" @click="doExport">
              I Understand, Show Phrase
            </AppButton>
          </div>
        </div>

        <!-- Loading -->
        <div v-else-if="exporting" class="text-center py-4">
          <div class="w-6 h-6 border-2 border-primary border-t-transparent rounded-full animate-spin mx-auto mb-2" />
          <p class="text-sm text-muted-foreground">Decrypting...</p>
        </div>

        <!-- Error -->
        <div v-else-if="exportError">
          <AppAlert variant="error" class="mb-3">{{ exportError }}</AppAlert>
          <AppButton variant="outline" class="w-full" @click="closeExportModal">
            Close
          </AppButton>
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
