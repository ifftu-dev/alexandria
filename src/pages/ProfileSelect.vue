<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'

import Starfield from '@/components/auth/Starfield.vue'
import BrandMark from '@/components/auth/BrandMark.vue'
import AddProfileTile from '@/components/profile/AddProfileTile.vue'
import ProfileAvatar from '@/components/profile/ProfileAvatar.vue'
import ProfileTile from '@/components/profile/ProfileTile.vue'
import { AppButton, AppInput } from '@/components/ui'
import { useProfiles } from '@/composables/useProfiles'
import {
  biometricCredentialExists,
  biometricSupported,
  getVaultPasswordViaBiometric,
} from '@/composables/useBiometricVault'

const router = useRouter()
const { t } = useI18n()
const { profiles, refreshProfiles, unlockProfile } = useProfiles()

const selectedId = ref<string | null>(null)
const password = ref('')
const unlocking = ref(false)
const error = ref<string | null>(null)
const passwordInput = ref<{ focus: () => void; select: () => void; blur: () => void } | null>(null)
const unlockStatus = ref<string>('')

// Biometric unlock. Credentials are keyed per profile, so availability is
// device-level but whether a credential exists is checked per selected
// profile.
const biometricAvailable = ref(false)
const selectedHasCredential = ref(false)
const biometricLoading = ref(false)
const biometricEnabled = computed(() => biometricAvailable.value && selectedHasCredential.value)

// The backend does not publish unlock progress. Never infer stages from elapsed time.
function startUnlockStages() {
  unlockStatus.value = t('onboarding.profileSelect.stageDecrypting')
}
function clearUnlockStages() {
  unlockStatus.value = ''
}

const selectedProfile = computed(() =>
  profiles.value.find((p) => p.id === selectedId.value) ?? null,
)

// Safety net for the macOS Secure Event Input leak: if the app loses key
// focus while the password field is still focused, release it so global
// hotkey tools (CGEventTaps) keep working in other apps.
function releaseSecureInputOnBlur() {
  passwordInput.value?.blur()
}

onMounted(async () => {
  window.addEventListener('blur', releaseSecureInputOnBlur)
  await refreshProfiles()
  try {
    biometricAvailable.value = await biometricSupported()
  } catch {
    biometricAvailable.value = false
  }
})

onUnmounted(() => {
  window.removeEventListener('blur', releaseSecureInputOnBlur)
})

function onSelect(id: string) {
  selectedId.value = id
  password.value = ''
  error.value = null
  selectedHasCredential.value = false
  // Focus is handled by the watcher below — the password panel is gated
  // mounted conditionally. Watch the ref so keyboard focus follows the
  // available form, independently of decorative transitions.

  // Check this profile's biometric credential, then auto-offer it.
  // Best-effort: a cancel or a vault rejection silently drops to password.
  if (biometricAvailable.value) {
    void offerBiometric(id)
  }
}

async function offerBiometric(id: string) {
  try {
    selectedHasCredential.value = await biometricCredentialExists(id)
  } catch {
    selectedHasCredential.value = false
  }
  if (selectedHasCredential.value && selectedId.value === id) {
    void unlockWithBiometric(true)
  }
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
    unlockStatus.value = t('onboarding.profileSelect.welcomeBack')
    // Blur the password field *before* navigating away. On macOS a focused
    // password input holds Secure Event Input on; WKWebView can leak that
    // state if the field unmounts while focused, which suppresses global
    // hotkey tools (CGEventTaps) app-wide until restart. Explicitly blurring
    // first makes macOS release it.
    passwordInput.value?.blur()
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

async function unlockWithBiometric(auto = false) {
  if (!selectedId.value || biometricLoading.value || unlocking.value) return
  biometricLoading.value = true
  if (!auto) error.value = null

  const profileId = selectedId.value
  let vaultPassword: string
  try {
    vaultPassword = await getVaultPasswordViaBiometric(profileId, 'Unlock Alexandria')
  } catch (e) {
    // Retrieval failed (user cancelled, not enrolled on this device, or
    // missing keychain entitlement). Stay silent on auto; explain on a
    // deliberate tap.
    if (!auto) {
      const msg = e instanceof Error ? e.message : String(e)
      if (/itemnotfound|not found/i.test(msg)) {
        error.value = t('onboarding.profileSelect.biometricNotSetup')
      } else if (/-34018/.test(msg)) {
        error.value = t('onboarding.profileSelect.biometricUnavailable')
      } else if (!/cancel/i.test(msg)) {
        error.value = t('onboarding.profileSelect.biometricFailed', { error: msg })
      }
    }
    biometricLoading.value = false
    return
  }
  biometricLoading.value = false

  // Got a password from the keychain — run the normal unlock against the
  // selected profile, reusing the progress stages.
  unlocking.value = true
  startUnlockStages()
  try {
    await unlockProfile(profileId, vaultPassword)
    unlockStatus.value = t('onboarding.profileSelect.welcomeBack')
    // See onUnlock: release Secure Event Input before unmounting.
    passwordInput.value?.blur()
    router.replace('/home')
  } catch (e) {
    // Stored credential was rejected by the vault. Drop to the password
    // form; only nag on a deliberate tap.
    if (!auto) {
      error.value = t('onboarding.profileSelect.biometricMismatch')
    }
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
    class="min-h-full flex items-center justify-center p-6 relative overflow-hidden"
    @keydown="onKeydown"
  >
    <Starfield />

    <div class="w-full max-w-3xl relative z-10">
      <header class="text-center mb-12">
        <div class="w-12 h-12 mx-auto mb-5" data-launch-target><BrandMark /></div>
        <h1 class="text-3xl font-semibold text-foreground">{{ $t('onboarding.profileSelect.heading') }}</h1>
        <p class="text-muted-foreground mt-2">
          {{ $t('onboarding.profileSelect.subtitle') }}
        </p>
      </header>

      <Transition name="fade" @before-leave="(el: Element) => el.setAttribute('inert', '')">
        <!-- Picker grid -->
        <div
          v-if="!selectedProfile"
          key="grid"
          class="flex flex-wrap gap-4 justify-center"
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
              {{ $t('onboarding.profileSelect.enterPassword') }}
            </div>
          </div>

          <form class="w-full space-y-3" @submit.prevent="onUnlock">
            <AppInput
              ref="passwordInput"
              v-model="password"
              type="password"
              :placeholder="$t('onboarding.profileSelect.passwordPlaceholder')"
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
                <span class="inline-block h-3 w-3 rounded-full border-2 border-current border-e-transparent animate-spin" />
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
                {{ $t('common.actions.back') }}
              </AppButton>
              <AppButton
                type="submit"
                :loading="unlocking"
                :disabled="unlocking || password.length === 0"
                class="flex-1"
              >
                {{ $t('onboarding.profileSelect.unlock') }}
              </AppButton>
            </div>

            <AppButton
              v-if="biometricEnabled"
              type="button"
              variant="secondary"
              class="w-full"
              :loading="biometricLoading"
              :disabled="unlocking || biometricLoading"
              @click="unlockWithBiometric(false)"
            >
              <svg class="me-1.5 h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.8">
                <path stroke-linecap="round" stroke-linejoin="round" d="M12 11c-1.1 0-2 .9-2 2v1m4-3c1.1 0 2 .9 2 2v2m-9-5a7 7 0 0110 0M5 8a10 10 0 0114 0M9 19c-.5-1-1-2.2-1-4m8 3a14 14 0 00.5-5" />
              </svg>
              {{ $t('onboarding.profileSelect.unlockBiometric') }}
            </AppButton>
          </form>
        </div>
      </Transition>
    </div>
  </div>
</template>

<style scoped>
.fade-enter-active { transition: opacity var(--transition-base), transform var(--transition-base); }
.fade-leave-active { position: absolute; width: 100%; transition: opacity var(--transition-fast); pointer-events: none; }
.fade-enter-from,
.fade-leave-to {
  opacity: 0;
  transform: translateY(4px);
}
</style>
