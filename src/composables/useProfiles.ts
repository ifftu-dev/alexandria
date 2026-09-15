import { computed, readonly, ref } from 'vue'

import type {
  AccountRole,
  Avatar,
  CreateProfileResponse,
  Identity,
  ProfileSummary,
  UnlockProfileResponse,
  WalletInfo,
} from '@/types'

import { useLocalApi } from './useLocalApi'
import { setProfileSessionToken } from './profileSession'

const { invoke } = useLocalApi()

// Module-level singleton state — mirrors the backend ProfileManager.
const profiles = ref<ProfileSummary[]>([])
const activeProfileId = ref<string | null>(null)
const activeWallet = ref<WalletInfo | null>(null)
const activeIdentity = ref<Identity | null>(null)
const loading = ref(false)
const initialized = ref(false)
const lockState = ref<'idle' | 'locking' | 'failed'>('idle')
const lockError = ref<string | null>(null)
const isLockBlocked = computed(() => lockState.value !== 'idle')
let profileGeneration = 0
let activation: Promise<unknown> | null = null
let lockOperation: Promise<void> | null = null

function requireCurrentGeneration(generation: number): void {
  if (generation !== profileGeneration || isLockBlocked.value) {
    throw new Error('Profile activation was cancelled by locking')
  }
}

async function refreshSessionToken(generation: number): Promise<void> {
  const token = await invoke<string | null>('get_profile_session_token')
  requireCurrentGeneration(generation)
  if (!token) throw new Error('Profile session is not active')
  setProfileSessionToken(token)
}

function runActivation<T>(operation: (generation: number) => Promise<T>): Promise<T> {
  if (isLockBlocked.value) return Promise.reject(new Error('Finish locking before unlocking a profile'))
  if (activation) return Promise.reject(new Error('A profile is already being unlocked'))
  if (activeProfileId.value) return Promise.reject(new Error('Lock the current profile before opening another'))
  const generation = profileGeneration
  const result = operation(generation).catch(async (error: unknown) => {
    if (generation === profileGeneration && !isLockBlocked.value) {
      const cleanupRequired = await invoke<boolean>('get_profile_cleanup_required').catch(() => true)
      if (cleanupRequired && generation === profileGeneration && !isLockBlocked.value) {
        profileGeneration++
        setProfileSessionToken(null)
        activeProfileId.value = null
        activeWallet.value = null
        activeIdentity.value = null
        lockError.value = 'Profile cleanup did not finish'
        lockState.value = 'failed'
      }
    }
    throw error
  })
  const tracked = result.finally(() => { activation = null })
  activation = tracked
  return tracked
}

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
  const generation = profileGeneration
  try {
    const identity = await invoke<Identity | null>('get_profile')
    if (generation === profileGeneration && !isLockBlocked.value) activeIdentity.value = identity
  } catch {
    if (generation === profileGeneration) activeIdentity.value = null
  }
}

async function refreshActiveWallet(): Promise<void> {
  const generation = profileGeneration
  try {
    const wallet = await invoke<WalletInfo | null>('get_wallet_info')
    if (generation === profileGeneration && !isLockBlocked.value) activeWallet.value = wallet
  } catch {
    if (generation === profileGeneration) activeWallet.value = null
  }
}

async function initialize(): Promise<'onboarding' | 'picker' | 'ready'> {
  if (initialized.value) {
    if (isUnlocked.value) return 'ready'
    return profiles.value.length === 0 ? 'onboarding' : 'picker'
  }

  loading.value = true
  const generation = profileGeneration
  try {
    await refreshProfiles()
    const cleanupRequired = await invoke<boolean>('get_profile_cleanup_required').catch(() => true)
    requireCurrentGeneration(generation)
    if (cleanupRequired) {
      activeProfileId.value = null
      setProfileSessionToken(null)
      activeWallet.value = null
      activeIdentity.value = null
      lockError.value = 'Profile cleanup did not finish'
      lockState.value = 'failed'
      initialized.value = true
      return 'picker'
    }
    const id = await invoke<string | null>('get_active_profile_id')
    requireCurrentGeneration(generation)
    activeProfileId.value = id
    if (id) {
      await refreshSessionToken(generation)
      await Promise.all([refreshActiveIdentity(), refreshActiveWallet()])
    }
    requireCurrentGeneration(generation)
    initialized.value = true

    if (isUnlocked.value) return 'ready'
    return profiles.value.length === 0 ? 'onboarding' : 'picker'
  } finally {
    loading.value = false
  }
}

async function createProfile(
  username: string,
  display_name: string,
  password: string,
  avatar?: Avatar,
  account?: { roles?: AccountRole[]; birthdate?: string },
): Promise<CreateProfileResponse> {
  return runActivation(async generation => {
    const result = await invoke<CreateProfileResponse>('create_profile', {
      username,
      displayName: display_name,
      password,
      avatar,
      roles: account?.roles,
      birthdate: account?.birthdate,
    })
    requireCurrentGeneration(generation)
    await refreshSessionToken(generation)
    activeProfileId.value = result.summary.id
    activeWallet.value = result.wallet
    activeIdentity.value = result.profile
    await refreshProfiles()
    requireCurrentGeneration(generation)
    await runProfileReadyCallbacks(generation)
    return result
  })
}

async function restoreProfileWithMnemonic(
  username: string,
  display_name: string,
  mnemonic: string,
  password: string,
  avatar?: Avatar,
  account?: { roles?: AccountRole[]; birthdate?: string },
): Promise<UnlockProfileResponse> {
  return runActivation(async generation => {
    const result = await invoke<UnlockProfileResponse>('restore_profile_with_mnemonic', {
      username,
      displayName: display_name,
      mnemonic,
      password,
      avatar,
      roles: account?.roles,
      birthdate: account?.birthdate,
    })
    requireCurrentGeneration(generation)
    await refreshSessionToken(generation)
    activeWallet.value = result.wallet
    activeIdentity.value = result.profile
    await refreshProfiles()
    const id = await invoke<string | null>('get_active_profile_id')
    requireCurrentGeneration(generation)
    activeProfileId.value = id
    await runProfileReadyCallbacks(generation)
    return result
  })
}

async function unlockProfile(id: string, password: string): Promise<UnlockProfileResponse> {
  return runActivation(async generation => {
    const result = await invoke<UnlockProfileResponse>('unlock_profile', { id, password })
    requireCurrentGeneration(generation)
    await refreshSessionToken(generation)
    activeProfileId.value = id
    activeWallet.value = result.wallet
    activeIdentity.value = result.profile
    await refreshProfiles()
    requireCurrentGeneration(generation)
    await runProfileReadyCallbacks(generation)
    return result
  })
}

function lockProfile(): Promise<void> {
  if (lockOperation) return lockOperation
  profileGeneration++
  lockState.value = 'locking'
  lockError.value = null
  activeProfileId.value = null
  activeWallet.value = null
  activeIdentity.value = null
  const pendingActivation = activation
  lockOperation = (async () => {
    try {
      // An in-flight activation may have opened backend resources. Wait for
      // it before teardown, but never let its late result restore the UI.
      if (pendingActivation) await pendingActivation.catch(() => undefined)
      await runProfileLockedCallbacks()
      setProfileSessionToken(null)
      await invoke('lock_profile')
      lockState.value = 'idle'
    } catch (error) {
      lockState.value = 'failed'
      lockError.value = error instanceof Error ? error.message : String(error)
      throw error
    } finally {
      lockOperation = null
    }
  })()
  return lockOperation
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
const profileLockedCallbacks = new Set<ProfileReadyCallback>()

export function onProfileReady(cb: ProfileReadyCallback): () => void {
  profileReadyCallbacks.add(cb)
  return () => profileReadyCallbacks.delete(cb)
}

export function onProfileLocked(cb: ProfileReadyCallback): () => void {
  profileLockedCallbacks.add(cb)
  return () => profileLockedCallbacks.delete(cb)
}

async function runProfileReadyCallbacks(generation: number): Promise<void> {
  for (const cb of profileReadyCallbacks) {
    requireCurrentGeneration(generation)
    try {
      await cb()
    } catch (e) {
      console.warn('[useProfiles] onProfileReady callback failed:', e)
    }
  }
  requireCurrentGeneration(generation)
}

async function runProfileLockedCallbacks(): Promise<void> {
  const errors: unknown[] = []
  for (const cb of profileLockedCallbacks) {
    try {
      await cb()
    } catch (e) {
      console.warn('[useProfiles] onProfileLocked callback failed:', e)
      errors.push(e)
    }
  }
  if (errors.length > 0) throw new Error('Profile cleanup did not finish')
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
    lockState: readonly(lockState),
    lockError: readonly(lockError),
    isLockBlocked,

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
