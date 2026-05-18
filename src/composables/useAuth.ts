// Compatibility shim — the canonical surface lives in `useProfiles`.
//
// `useAuth` predates multi-user support. It is kept so existing
// components keep compiling, but new code should consume `useProfiles`
// directly. Legacy methods that no longer make sense in a multi-user
// world (`unlockVault`, `generateWallet`, `restoreWallet`,
// `resetLocalWallet`, `checkVaultExists`) throw to flush out stale
// call sites.

import { computed, readonly } from 'vue'

import { useLocalApi } from './useLocalApi'
import { useProfiles } from './useProfiles'

const { invoke } = useLocalApi()

const profilesApi = useProfiles()

const isAuthenticated = computed(() => profilesApi.isUnlocked.value)

async function exportMnemonic(password: string): Promise<string> {
  return invoke<string>('export_mnemonic', { password })
}

async function initialize(): Promise<'onboarding' | 'unlock' | 'ready'> {
  const state = await profilesApi.initialize()
  // Map the picker state back onto the legacy three-way enum so
  // existing callers (`App.vue`) keep routing correctly.
  return state === 'picker' ? 'unlock' : state
}

function unsupported(name: string): never {
  throw new Error(
    `useAuth.${name} is removed — multi-user profiles use useProfiles.${name} (and a profile id).`,
  )
}

export function useAuth() {
  return {
    // State (readonly)
    identity: profilesApi.activeIdentity,
    walletInfo: profilesApi.activeWallet,
    vaultUnlocked: profilesApi.isUnlocked,
    loading: profilesApi.loading,
    initialized: profilesApi.initialized,

    // Computed
    isAuthenticated: readonly(isAuthenticated),
    displayName: profilesApi.displayName,
    stakeAddress: profilesApi.stakeAddress,

    // Actions still meaningful in multi-user
    refreshProfile: profilesApi.refreshActiveIdentity,
    lockVault: profilesApi.lockProfile,
    exportMnemonic,
    initialize,

    // Removed — kept as guards so a stale caller fails loudly
    checkVaultExists: () => unsupported('checkVaultExists'),
    unlockVault: () => unsupported('unlockVault'),
    generateWallet: () => unsupported('generateWallet'),
    restoreWallet: () => unsupported('restoreWallet'),
    resetLocalWallet: () => unsupported('resetLocalWallet'),
  }
}
