<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { useTheme } from '@/composables/useTheme'

const { invoke } = useLocalApi()
const { theme, toggleTheme } = useTheme()

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
</script>

<template>
  <div class="max-w-2xl">
    <h1 class="text-xl font-bold mb-1">Settings</h1>
    <p class="text-sm text-[rgb(var(--color-muted-foreground))] mb-6">
      Manage your profile and node settings.
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
  </div>
</template>
