<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue'
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
const passwordInput = ref<{ focus: () => void; select: () => void } | null>(null)
const unlockStatus = ref<string>('')

// Status messages shown while the unlock IPC runs. The backend doesn't
// emit progress events yet, so we cycle through expected stages on a
// timer — gives the user a sense of motion instead of a frozen spinner.
const UNLOCK_STAGES: { at: number; msg: string }[] = [
  { at: 0, msg: 'Decrypting your vault…' },
  { at: 1200, msg: 'Loading your profile…' },
  { at: 2500, msg: 'Opening the local database…' },
  { at: 4500, msg: 'Spinning up the local node…' },
  { at: 7000, msg: 'Almost there — taking a bit longer than usual…' },
]
let unlockStageTimers: number[] = []
function startUnlockStages() {
  clearUnlockStages()
  unlockStageTimers = UNLOCK_STAGES.map((s) =>
    window.setTimeout(() => { unlockStatus.value = s.msg }, s.at),
  )
}
function clearUnlockStages() {
  for (const id of unlockStageTimers) window.clearTimeout(id)
  unlockStageTimers = []
  unlockStatus.value = ''
}

const selectedProfile = computed(() =>
  profiles.value.find((p) => p.id === selectedId.value) ?? null,
)

onMounted(async () => {
  await refreshProfiles()
})

function onSelect(id: string) {
  selectedId.value = id
  password.value = ''
  error.value = null
  // Focus is handled by the watcher below — the password panel is gated
  // behind a <Transition mode="out-in">, so nextTick alone fires before
  // the new DOM is mounted. Watch the ref instead and focus when it
  // becomes available.
}

// When the password panel mounts (Transition finishes), grab focus.
watch(passwordInput, (el) => {
  if (el) {
    // Two RAFs to clear any in-flight Transition style application.
    requestAnimationFrame(() => requestAnimationFrame(() => el.focus()))
  }
})

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
  startUnlockStages()
  try {
    await unlockProfile(selectedId.value, password.value)
    unlockStatus.value = 'Welcome back — taking you home…'
    router.replace('/home')
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
    password.value = ''
    await nextTick()
    requestAnimationFrame(() => passwordInput.value?.focus())
  } finally {
    unlocking.value = false
    clearUnlockStages()
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
            <Transition
              enter-active-class="transition duration-150 ease-out"
              enter-from-class="opacity-0 -translate-y-1"
              enter-to-class="opacity-100 translate-y-0"
              leave-active-class="transition duration-100 ease-in"
              leave-from-class="opacity-100"
              leave-to-class="opacity-0"
            >
              <div
                v-if="unlocking && unlockStatus"
                class="flex items-center gap-2 text-xs text-muted-foreground"
              >
                <span class="inline-block h-3 w-3 rounded-full border-2 border-current border-r-transparent animate-spin" />
                <span>{{ unlockStatus }}</span>
              </div>
            </Transition>
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
