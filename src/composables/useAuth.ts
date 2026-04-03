import { ref, computed, readonly } from 'vue'
import type { Identity, WalletInfo } from '@/types'
import { useLocalApi } from './useLocalApi'

const { invoke } = useLocalApi()

// Module-level singleton state
const identity = ref<Identity | null>(null)
const walletInfo = ref<WalletInfo | null>(null)
const vaultUnlocked = ref(false)
const loading = ref(false)
const initialized = ref(false)

const isAuthenticated = computed(() => vaultUnlocked.value && !!identity.value)
const displayName = computed(() => identity.value?.display_name ?? null)
const stakeAddress = computed(() => identity.value?.stake_address ?? null)

async function checkVaultExists(): Promise<boolean> {
  try {
    return await invoke<boolean>('check_vault_exists')
  } catch {
    return false
  }
}

async function unlockVault(password: string): Promise<WalletInfo> {
  // unlock_vault returns both wallet info and profile in a single IPC call
  const response = await invoke<{ wallet: WalletInfo; profile: Identity | null }>('unlock_vault', { password })
  walletInfo.value = response.wallet
  identity.value = response.profile
  vaultUnlocked.value = true
  return response.wallet
}

async function generateWallet(password: string): Promise<{ mnemonic: string; stake_address: string; payment_address: string }> {
  const result = await invoke<{ mnemonic: string; stake_address: string; payment_address: string }>('generate_wallet', { password })
  vaultUnlocked.value = true
  // Profile was just created — fetch it immediately (fast DB read, no crypto)
  await refreshProfile()
  return result
}

async function restoreWallet(mnemonic: string, password: string): Promise<WalletInfo> {
  const info = await invoke<WalletInfo>('restore_wallet', { mnemonic, password })
  walletInfo.value = info
  vaultUnlocked.value = true
  await refreshProfile()
  return info
}

async function refreshProfile(): Promise<void> {
  try {
    identity.value = await invoke<Identity | null>('get_profile')
  } catch (e) {
    console.warn('[useAuth] refreshProfile failed:', e)
    identity.value = null
  }
}

async function lockVault(): Promise<void> {
  await invoke('lock_vault')
  vaultUnlocked.value = false
  identity.value = null
  walletInfo.value = null
  initialized.value = false
}

async function resetLocalWallet(): Promise<void> {
  await invoke('reset_local_wallet')
  vaultUnlocked.value = false
  identity.value = null
  walletInfo.value = null
  initialized.value = false
}

async function exportMnemonic(password: string): Promise<string> {
  return invoke<string>('export_mnemonic', { password })
}

async function initialize(): Promise<'onboarding' | 'unlock' | 'ready'> {
  if (initialized.value) {
    return vaultUnlocked.value ? 'ready' : 'unlock'
  }

  loading.value = true
  try {
    const exists = await checkVaultExists()

    if (!exists) {
      initialized.value = true
      return 'onboarding'
    }

    // Vault file exists — the user needs to unlock.
    // We intentionally do NOT check get_wallet_info/get_profile here
    // because those are DB reads that succeed even when the in-memory
    // keystore is None. The only way to populate the keystore (required
    // for P2P, export mnemonic, etc.) is via unlock_vault IPC.
    initialized.value = true
    return 'unlock'
  } finally {
    loading.value = false
  }
}

export function useAuth() {
  return {
    // State (readonly)
    identity: readonly(identity),
    walletInfo: readonly(walletInfo),
    vaultUnlocked: readonly(vaultUnlocked),
    loading: readonly(loading),
    initialized: readonly(initialized),

    // Computed
    isAuthenticated,
    displayName,
    stakeAddress,

    // Actions
    checkVaultExists,
    unlockVault,
    generateWallet,
    restoreWallet,
    refreshProfile,
    lockVault,
    resetLocalWallet,
    exportMnemonic,
    initialize,
  }
}
