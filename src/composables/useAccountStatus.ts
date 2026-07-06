import { computed, readonly, ref } from 'vue'

import type { AccountRole, AccountStatus } from '@/types'

import { useLocalApi } from './useLocalApi'
import { onProfileLocked, onProfileReady } from './useProfiles'

const { invoke } = useLocalApi()

// Module-level singleton — one active profile at a time.
const status = ref<AccountStatus | null>(null)
const loaded = ref(false)

const role = computed<AccountRole>(() => status.value?.role ?? 'learner')
const isMinor = computed(() => status.value?.is_minor ?? false)
const activationState = computed(() => status.value?.activation_state ?? 'active')
const isPendingGuardian = computed(() => activationState.value === 'pending_guardian')

async function refreshAccountStatus(): Promise<AccountStatus | null> {
  try {
    status.value = await invoke<AccountStatus | null>('get_account_status')
  } catch {
    status.value = null
  }
  loaded.value = true
  return status.value
}

async function setAccountRole(newRole: AccountRole): Promise<void> {
  await invoke('set_account_role', { role: newRole })
  await refreshAccountStatus()
}

onProfileReady(() => {
  void refreshAccountStatus()
})
onProfileLocked(() => {
  status.value = null
  loaded.value = false
})

export function useAccountStatus() {
  return {
    status: readonly(status),
    loaded: readonly(loaded),
    role,
    isMinor,
    activationState,
    isPendingGuardian,
    refreshAccountStatus,
    setAccountRole,
  }
}
