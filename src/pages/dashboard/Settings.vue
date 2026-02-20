<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { useAuth } from '@/composables/useAuth'
import { useTheme } from '@/composables/useTheme'
import { AppButton, AppInput, AppTextarea, AppModal, AppAlert, DataRow } from '@/components/ui'
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

onMounted(() => {
  if (identity.value) {
    displayName.value = identity.value.display_name ?? ''
    bio.value = identity.value.bio ?? ''
  }
})

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
</script>

<template>
  <div class="max-w-2xl">
    <h1 class="text-xl font-bold mb-1">Settings</h1>
    <p class="text-sm text-[rgb(var(--color-muted-foreground))] mb-6">
      Manage your profile, security, and node settings.
    </p>

    <!-- Profile -->
    <div class="card p-5 mb-6">
      <h2 class="text-base font-semibold mb-4">Profile</h2>

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
          <span v-if="message" class="text-xs text-[rgb(var(--color-success))]">{{ message }}</span>
          <span v-if="publishMessage" class="text-xs text-[rgb(var(--color-success))]">{{ publishMessage }}</span>
        </div>

        <div v-if="identity?.profile_hash" class="pt-2 border-t border-[rgb(var(--color-border))]">
          <span class="text-xs text-[rgb(var(--color-muted-foreground))]">Published Profile Hash</span>
          <code class="block font-mono text-xs mt-0.5 break-all text-[rgb(var(--color-muted-foreground))]">{{ identity.profile_hash }}</code>
        </div>
      </div>
    </div>

    <!-- Security -->
    <div class="card p-5 mb-6">
      <h2 class="text-base font-semibold mb-3">Security</h2>

      <div class="space-y-3">
        <div class="flex items-center justify-between">
          <div>
            <div class="text-sm">Recovery Phrase</div>
            <div class="text-xs text-[rgb(var(--color-muted-foreground))]">
              Export your 24-word backup phrase
            </div>
          </div>
          <AppButton variant="outline" size="sm" @click="openExportModal">
            Export
          </AppButton>
        </div>

        <div class="flex items-center justify-between">
          <div>
            <div class="text-sm">Lock Wallet</div>
            <div class="text-xs text-[rgb(var(--color-muted-foreground))]">
              Clear secrets from memory and require password
            </div>
          </div>
          <AppButton variant="outline" size="sm" :loading="locking" @click="lockWallet">
            Lock
          </AppButton>
        </div>
      </div>
    </div>

    <!-- Theme -->
    <div class="card p-5 mb-6">
      <h2 class="text-base font-semibold mb-3">Appearance</h2>
      <div class="flex items-center justify-between">
        <div>
          <div class="text-sm">Theme</div>
          <div class="text-xs text-[rgb(var(--color-muted-foreground))]">
            Current: {{ theme }}
          </div>
        </div>
        <AppButton variant="outline" size="sm" @click="toggleTheme">
          Toggle ({{ theme === 'light' ? 'Dark' : theme === 'dark' ? 'System' : 'Light' }})
        </AppButton>
      </div>
    </div>

    <!-- Identity (read-only) -->
    <div v-if="identity" class="card p-5">
      <h2 class="text-base font-semibold mb-3">Identity</h2>
      <div class="space-y-2">
        <DataRow label="Stake Address" mono>{{ identity.stake_address }}</DataRow>
        <DataRow label="Payment Address" mono>{{ identity.payment_address }}</DataRow>
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
        <div class="w-6 h-6 border-2 border-[rgb(var(--color-primary))] border-t-transparent rounded-full animate-spin mx-auto mb-2" />
        <p class="text-sm text-[rgb(var(--color-muted-foreground))]">Decrypting...</p>
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
        <div class="grid grid-cols-3 gap-2 mb-4">
          <div
            v-for="(word, i) in exportedMnemonic.split(' ')"
            :key="i"
            class="flex items-center gap-2 text-sm py-1.5 px-2.5 rounded bg-[rgb(var(--color-muted)/0.3)]"
          >
            <span class="text-xs text-[rgb(var(--color-muted-foreground))] w-5 text-right">{{ i + 1 }}.</span>
            <span class="font-mono font-medium">{{ word }}</span>
          </div>
        </div>

        <AppButton class="w-full" @click="closeExportModal">
          Done
        </AppButton>
      </div>
    </AppModal>
  </div>
</template>
