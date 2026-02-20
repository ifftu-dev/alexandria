<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { useTheme } from '@/composables/useTheme'

const { invoke } = useLocalApi()
const { theme, toggleTheme } = useTheme()
const router = useRouter()

interface Identity {
  stake_address: string
  payment_address: string
  display_name: string | null
  bio: string | null
}

const profile = ref<Identity | null>(null)
const displayName = ref('')
const bio = ref('')
const saving = ref(false)
const message = ref('')

// Security section state
const showExportModal = ref(false)
const exportConfirmed = ref(false)
const exportedMnemonic = ref('')
const exportError = ref('')
const exporting = ref(false)
const locking = ref(false)

onMounted(async () => {
  try {
    profile.value = await invoke<Identity | null>('get_profile')
    if (profile.value) {
      displayName.value = profile.value.display_name ?? ''
      bio.value = profile.value.bio ?? ''
    }
  } catch (e) {
    console.error('Failed to load profile:', e)
  }
})

async function saveProfile() {
  saving.value = true
  message.value = ''

  try {
    const updated = await invoke<Identity>('update_profile', {
      update: {
        display_name: displayName.value || null,
        bio: bio.value || null,
      },
    })
    profile.value = updated
    message.value = 'Profile updated.'
  } catch (e) {
    message.value = `Error: ${e}`
  } finally {
    saving.value = false
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

async function exportMnemonic() {
  exporting.value = true
  exportError.value = ''

  try {
    exportedMnemonic.value = await invoke<string>('export_mnemonic')
  } catch (e) {
    exportError.value = String(e)
  } finally {
    exporting.value = false
  }
}

async function lockWallet() {
  locking.value = true
  try {
    await invoke('lock_vault')
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
        <div>
          <label class="block text-xs font-medium text-[rgb(var(--color-muted-foreground))] mb-1">
            Display Name
          </label>
          <input
            v-model="displayName"
            type="text"
            placeholder="How others see you"
            class="w-full px-3 py-2 text-sm rounded-md border border-[rgb(var(--color-border))] bg-[rgb(var(--color-background))] focus:outline-none focus:ring-2 focus:ring-[rgb(var(--color-ring))]"
          >
        </div>

        <div>
          <label class="block text-xs font-medium text-[rgb(var(--color-muted-foreground))] mb-1">
            Bio
          </label>
          <textarea
            v-model="bio"
            placeholder="A short description about yourself"
            rows="3"
            class="w-full px-3 py-2 text-sm rounded-md border border-[rgb(var(--color-border))] bg-[rgb(var(--color-background))] focus:outline-none focus:ring-2 focus:ring-[rgb(var(--color-ring))] resize-none"
          />
        </div>

        <div class="flex items-center gap-3">
          <button
            class="px-4 py-2 rounded-md text-sm font-medium bg-[rgb(var(--color-primary))] text-[rgb(var(--color-primary-foreground))] hover:bg-[rgb(var(--color-primary-hover))] transition-colors disabled:opacity-50"
            :disabled="saving"
            @click="saveProfile"
          >
            {{ saving ? 'Saving...' : 'Save Profile' }}
          </button>
          <span v-if="message" class="text-xs text-[rgb(var(--color-success))]">{{ message }}</span>
        </div>
      </div>
    </div>

    <!-- Security -->
    <div class="card p-5 mb-6">
      <h2 class="text-base font-semibold mb-3">Security</h2>

      <div class="space-y-3">
        <!-- Export Recovery Phrase -->
        <div class="flex items-center justify-between">
          <div>
            <div class="text-sm">Recovery Phrase</div>
            <div class="text-xs text-[rgb(var(--color-muted-foreground))]">
              Export your 24-word backup phrase
            </div>
          </div>
          <button
            class="px-3 py-1.5 rounded-md text-sm border border-[rgb(var(--color-border))] hover:bg-[rgb(var(--color-muted)/0.5)] transition-colors"
            @click="openExportModal"
          >
            Export
          </button>
        </div>

        <!-- Lock Wallet -->
        <div class="flex items-center justify-between">
          <div>
            <div class="text-sm">Lock Wallet</div>
            <div class="text-xs text-[rgb(var(--color-muted-foreground))]">
              Clear secrets from memory and require password
            </div>
          </div>
          <button
            class="px-3 py-1.5 rounded-md text-sm border border-[rgb(var(--color-border))] hover:bg-[rgb(var(--color-muted)/0.5)] transition-colors disabled:opacity-50"
            :disabled="locking"
            @click="lockWallet"
          >
            {{ locking ? 'Locking...' : 'Lock' }}
          </button>
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
        <button
          class="px-3 py-1.5 rounded-md text-sm border border-[rgb(var(--color-border))] hover:bg-[rgb(var(--color-muted)/0.5)] transition-colors"
          @click="toggleTheme"
        >
          Toggle ({{ theme === 'light' ? 'Dark' : theme === 'dark' ? 'System' : 'Light' }})
        </button>
      </div>
    </div>

    <!-- Identity (read-only) -->
    <div v-if="profile" class="card p-5">
      <h2 class="text-base font-semibold mb-3">Identity</h2>
      <div class="space-y-2 text-sm">
        <div>
          <span class="text-xs text-[rgb(var(--color-muted-foreground))]">Stake Address</span>
          <code class="block font-mono text-xs mt-0.5 break-all">{{ profile.stake_address }}</code>
        </div>
        <div>
          <span class="text-xs text-[rgb(var(--color-muted-foreground))]">Payment Address</span>
          <code class="block font-mono text-xs mt-0.5 break-all">{{ profile.payment_address }}</code>
        </div>
      </div>
    </div>

    <!-- Export Recovery Phrase Modal -->
    <Teleport to="body">
      <div
        v-if="showExportModal"
        class="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
        @click.self="closeExportModal"
      >
        <div class="card p-6 w-full max-w-md mx-4">
          <h3 class="text-base font-semibold mb-3">Export Recovery Phrase</h3>

          <!-- Confirmation step -->
          <div v-if="!exportedMnemonic && !exportConfirmed">
            <div class="card p-4 mb-4 border-[rgb(var(--color-error))] bg-[rgb(var(--color-error)/0.05)]">
              <p class="text-sm text-[rgb(var(--color-error))] font-medium">
                Your recovery phrase gives full access to your identity and credentials.
                Only export it in a private, secure environment.
              </p>
            </div>

            <div class="flex gap-2">
              <button
                class="flex-1 py-2 px-3 rounded-md text-sm border border-[rgb(var(--color-border))] hover:bg-[rgb(var(--color-muted)/0.5)] transition-colors"
                @click="closeExportModal"
              >
                Cancel
              </button>
              <button
                class="flex-1 py-2 px-3 rounded-md text-sm font-medium bg-[rgb(var(--color-error))] text-[rgb(var(--color-error-foreground))] hover:opacity-90 transition-opacity"
                @click="exportConfirmed = true; exportMnemonic()"
              >
                I Understand, Show Phrase
              </button>
            </div>
          </div>

          <!-- Loading -->
          <div v-else-if="exporting" class="text-center py-4">
            <div class="w-6 h-6 border-2 border-[rgb(var(--color-primary))] border-t-transparent rounded-full animate-spin mx-auto mb-2" />
            <p class="text-sm text-[rgb(var(--color-muted-foreground))]">Decrypting...</p>
          </div>

          <!-- Error -->
          <div v-else-if="exportError">
            <p class="text-sm text-[rgb(var(--color-error))] mb-3">{{ exportError }}</p>
            <button
              class="w-full py-2 px-3 rounded-md text-sm border border-[rgb(var(--color-border))] hover:bg-[rgb(var(--color-muted)/0.5)] transition-colors"
              @click="closeExportModal"
            >
              Close
            </button>
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

            <button
              class="w-full py-2 px-3 rounded-md text-sm font-medium bg-[rgb(var(--color-primary))] text-[rgb(var(--color-primary-foreground))] hover:bg-[rgb(var(--color-primary-hover))] transition-colors"
              @click="closeExportModal"
            >
              Done
            </button>
          </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>
