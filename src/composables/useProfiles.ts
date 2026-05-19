import { computed, readonly, ref } from 'vue'

import type {
  Avatar,
  CreateProfileResponse,
  Identity,
  ProfileSummary,
  UnlockProfileResponse,
  WalletInfo,
} from '@/types'

import { useLocalApi } from './useLocalApi'

const { invoke } = useLocalApi()

// Module-level singleton state — mirrors the backend ProfileManager.
const profiles = ref<ProfileSummary[]>([])
const activeProfileId = ref<string | null>(null)
const activeWallet = ref<WalletInfo | null>(null)
const activeIdentity = ref<Identity | null>(null)
const loading = ref(false)
const initialized = ref(false)

const activeProfile = computed<ProfileSummary | null>(() => {
  const id = activeProfileId.value
  if (!id) return null
  return profiles.value.find((p) => p.id === id) ?? null
})

const isUnlocked = computed(() => activeProfileId.value !== null)
const displayName = computed(
  () => activeIdentity.value?.display_name ?? activeProfile.value?.display_name ?? null,
)
const stakeAddress = computed(() => activeIdentity.value?.stake_address ?? null)

async function refreshProfiles(): Promise<void> {
  profiles.value = await invoke<ProfileSummary[]>('list_profiles')
}

async function refreshActiveIdentity(): Promise<void> {
  try {
    activeIdentity.value = await invoke<Identity | null>('get_profile')
  } catch {
    activeIdentity.value = null
  }
}

async function refreshActiveWallet(): Promise<void> {
  try {
    activeWallet.value = await invoke<WalletInfo | null>('get_wallet_info')
  } catch {
    activeWallet.value = null
  }
}

async function initialize(): Promise<'onboarding' | 'picker' | 'ready'> {
  if (initialized.value) {
    if (isUnlocked.value) return 'ready'
    return profiles.value.length === 0 ? 'onboarding' : 'picker'
  }

  loading.value = true
  try {
    await refreshProfiles()
    const id = await invoke<string | null>('get_active_profile_id')
    activeProfileId.value = id
    if (id) {
      await Promise.all([refreshActiveIdentity(), refreshActiveWallet()])
    }
    initialized.value = true

    if (isUnlocked.value) return 'ready'
    return profiles.value.length === 0 ? 'onboarding' : 'picker'
  } finally {
    loading.value = false
  }
}

async function createProfile(
  display_name: string,
  password: string,
  avatar?: Avatar,
): Promise<CreateProfileResponse> {
  const result = await invoke<CreateProfileResponse>('create_profile', {
    displayName: display_name,
    password,
    avatar,
  })
  activeProfileId.value = result.summary.id
  activeWallet.value = result.wallet
  activeIdentity.value = result.profile
  await refreshProfiles()
  await runProfileReadyCallbacks()
  return result
}

async function restoreProfileWithMnemonic(
  display_name: string,
  mnemonic: string,
  password: string,
  avatar?: Avatar,
): Promise<UnlockProfileResponse> {
  const result = await invoke<UnlockProfileResponse>('restore_profile_with_mnemonic', {
    displayName: display_name,
    mnemonic,
    password,
    avatar,
  })
  activeWallet.value = result.wallet
  activeIdentity.value = result.profile
  await refreshProfiles()
  const id = await invoke<string | null>('get_active_profile_id')
  activeProfileId.value = id
  await runProfileReadyCallbacks()
  return result
}

async function unlockProfile(id: string, password: string): Promise<UnlockProfileResponse> {
  const result = await invoke<UnlockProfileResponse>('unlock_profile', { id, password })
  activeProfileId.value = id
  activeWallet.value = result.wallet
  activeIdentity.value = result.profile
  await refreshProfiles()
  await runProfileReadyCallbacks()
  return result
}

async function lockProfile(): Promise<void> {
  await invoke('lock_profile')
  activeProfileId.value = null
  activeWallet.value = null
  activeIdentity.value = null
}

// ── onProfileReady hook ─────────────────────────────────────────
//
// Other singletons (settings store, theme, keyboard shortcuts,
// sentinel flags) need to re-hydrate from the per-profile DB the
// moment a profile becomes active. They register here so the
// lifecycle commands fan out without coupling useProfiles to those
// modules.

type ProfileReadyCallback = () => void | Promise<void>
const profileReadyCallbacks = new Set<ProfileReadyCallback>()

export function onProfileReady(cb: ProfileReadyCallback): () => void {
  profileReadyCallbacks.add(cb)
  return () => profileReadyCallbacks.delete(cb)
}

async function runProfileReadyCallbacks(): Promise<void> {
  for (const cb of profileReadyCallbacks) {
    try {
      await cb()
    } catch (e) {
      console.warn('[useProfiles] onProfileReady callback failed:', e)
    }
  }
}

async function renameProfile(id: string, display_name: string): Promise<ProfileSummary> {
  const summary = await invoke<ProfileSummary>('rename_profile', {
    id,
    displayName: display_name,
  })
  await refreshProfiles()
  return summary
}

async function setProfileAvatar(id: string, avatar: Avatar): Promise<ProfileSummary> {
  const summary = await invoke<ProfileSummary>('set_profile_avatar', { id, avatar })
  await refreshProfiles()
  return summary
}

async function deleteProfile(id: string, password: string): Promise<void> {
  await invoke('delete_profile', { id, password })
  await refreshProfiles()
}

export function useProfiles() {
  return {
    profiles: readonly(profiles),
    activeProfile,
    activeProfileId: readonly(activeProfileId),
    activeWallet: readonly(activeWallet),
    activeIdentity: readonly(activeIdentity),
    loading: readonly(loading),
    initialized: readonly(initialized),

    isUnlocked,
    displayName,
    stakeAddress,

    initialize,
    refreshProfiles,
    refreshActiveIdentity,
    refreshActiveWallet,
    createProfile,
    restoreProfileWithMnemonic,
    unlockProfile,
    lockProfile,
    renameProfile,
    setProfileAvatar,
    deleteProfile,
  }
}
