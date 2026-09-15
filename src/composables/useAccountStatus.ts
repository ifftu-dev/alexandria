import { computed, readonly, ref } from 'vue'

import type { AccountRole, AccountStatus } from '@/types'

import { useLocalApi } from './useLocalApi'
import { onProfileLocked, onProfileReady } from './useProfiles'

const { invoke } = useLocalApi()

// Module-level singleton — one active profile at a time.
const status = ref<AccountStatus | null>(null)
const loaded = ref(false)
let profileGeneration = 0

/** Canonical role set. Everybody is a learner, so this is never empty. */
const roles = computed<AccountRole[]>(() => status.value?.roles ?? ['learner'])
const hasRole = (r: AccountRole) => roles.value.includes(r)
const isInstructor = computed(() => hasRole('instructor'))
const isParent = computed(() => hasRole('parent'))
/** Legacy single-valued view: the first extra role, or 'learner'. */
const role = computed<AccountRole>(() => status.value?.role ?? 'learner')
const isMinor = computed(() => status.value?.is_minor ?? false)
const activationState = computed(() => status.value?.activation_state ?? 'active')
const isPendingGuardian = computed(() => activationState.value === 'pending_guardian')

async function refreshAccountStatus(): Promise<AccountStatus | null> {
  const generation = profileGeneration
  try {
    const refreshed = await invoke<AccountStatus | null>('get_account_status')
    if (generation === profileGeneration) status.value = refreshed
  } catch {
    if (generation === profileGeneration) status.value = null
  }
  if (generation === profileGeneration) loaded.value = true
  return generation === profileGeneration ? status.value : null
}

/** Replace the extra roles. Learner cannot be removed; the backend puts it back. */
async function setAccountRoles(next: AccountRole[]): Promise<void> {
  const generation = profileGeneration
  await invoke('set_account_roles', { roles: next })
  if (generation === profileGeneration) await refreshAccountStatus()
}

onProfileReady(() => {
  void refreshAccountStatus()
})
onProfileLocked(() => {
  profileGeneration += 1
  status.value = null
  loaded.value = false
})

export function useAccountStatus() {
  return {
    status: readonly(status),
    loaded: readonly(loaded),
    roles,
    hasRole,
    isInstructor,
    isParent,
    role,
    isMinor,
    activationState,
    isPendingGuardian,
    refreshAccountStatus,
    setAccountRoles,
  }
}
