<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'

import AddProfileTile from '@/components/profile/AddProfileTile.vue'
import ProfileAvatar from '@/components/profile/ProfileAvatar.vue'
import ProfileTile from '@/components/profile/ProfileTile.vue'
import { AppButton, AppInput } from '@/components/ui'
import { useProfiles } from '@/composables/useProfiles'

const router = useRouter()
const { profiles, refreshProfiles, unlockProfile } = useProfiles()

const selectedId = ref<string | null>(null)
const password = ref('')
const unlocking = ref(false)
const error = ref<string | null>(null)
const passwordInput = ref<HTMLInputElement | null>(null)

const selectedProfile = computed(() =>
  profiles.value.find((p) => p.id === selectedId.value) ?? null,
)

onMounted(async () => {
  await refreshProfiles()
})

async function onSelect(id: string) {
  selectedId.value = id
  password.value = ''
  error.value = null
  await nextTick()
  passwordInput.value?.focus()
}

function onBack() {
  selectedId.value = null
  password.value = ''
  error.value = null
}

function onAddProfile() {
  router.push({ path: '/onboarding', query: { from: 'picker' } })
}

async function onUnlock() {
  if (!selectedId.value || password.value.length === 0) return
  unlocking.value = true
  error.value = null
  try {
    await unlockProfile(selectedId.value, password.value)
    router.replace('/home')
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
    password.value = ''
    await nextTick()
    passwordInput.value?.focus()
  } finally {
    unlocking.value = false
  }
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape' && selectedId.value) {
    event.preventDefault()
    onBack()
  }
}
</script>

<template>
  <div
    class="min-h-screen flex items-center justify-center p-6 safe-area-top"
    @keydown="onKeydown"
  >
    <div class="w-full max-w-3xl">
      <header class="text-center mb-12">
        <h1 class="text-3xl font-semibold text-foreground">Who's learning today?</h1>
        <p class="text-muted-foreground mt-2">
          Pick a profile to continue, or add a new one.
        </p>
      </header>

      <Transition name="fade" mode="out-in">
        <!-- Picker grid -->
        <div
          v-if="!selectedProfile"
          key="grid"
          class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 gap-4 justify-items-center"
        >
          <ProfileTile
            v-for="profile in profiles"
            :key="profile.id"
            :profile="profile"
            @select="onSelect"
          />
          <AddProfileTile @click="onAddProfile" />
        </div>

        <!-- Password panel -->
        <div
          v-else
          key="unlock"
          class="flex flex-col items-center gap-6 max-w-sm mx-auto"
        >
          <ProfileAvatar
            :avatar="selectedProfile.avatar"
            :fallback-name="selectedProfile.display_name"
            :color="selectedProfile.color"
            :size="112"
          />
          <div class="text-center">
            <div class="text-xl font-medium text-foreground">
              {{ selectedProfile.display_name }}
            </div>
            <div class="text-xs text-muted-foreground mt-1">
              Enter password to continue
            </div>
          </div>

          <form class="w-full space-y-3" @submit.prevent="onUnlock">
            <AppInput
              ref="passwordInput"
              v-model="password"
              type="password"
              placeholder="Vault password"
              :error="error ?? ''"
              :disabled="unlocking"
            />
            <div class="flex items-center gap-2">
              <AppButton
                type="button"
                variant="ghost"
                :disabled="unlocking"
                @click="onBack"
              >
                Back
              </AppButton>
              <AppButton
                type="submit"
                :loading="unlocking"
                :disabled="unlocking || password.length === 0"
                class="flex-1"
              >
                Unlock
              </AppButton>
            </div>
          </form>
        </div>
      </Transition>
    </div>
  </div>
</template>

<style scoped>
.fade-enter-active,
.fade-leave-active {
  transition: opacity 180ms ease, transform 180ms ease;
}
.fade-enter-from,
.fade-leave-to {
  opacity: 0;
  transform: translateY(4px);
}
</style>
