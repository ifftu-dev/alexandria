// Guardian-link state for both sides: a parent's children list and a
// ward's "my guardian" view, plus typed accessors over the mirrored
// activity rows.

import { computed, readonly, ref } from 'vue'

import type { GuardianLinkInfo } from '@/types'

import { useLocalApi } from './useLocalApi'
import { onProfileLocked, onProfileReady } from './useProfiles'

const { invoke } = useLocalApi()

const links = ref<GuardianLinkInfo[]>([])
const loaded = ref(false)

const children = computed(() => links.value.filter(l => l.side === 'guardian' && l.status !== 'revoked'))
const guardians = computed(() => links.value.filter(l => l.side === 'ward' && l.status !== 'revoked'))

async function refreshLinks(): Promise<GuardianLinkInfo[]> {
  try {
    links.value = await invoke<GuardianLinkInfo[]>('guardian_list_links')
  } catch {
    links.value = []
  }
  loaded.value = true
  return links.value
}

async function acceptInvite(code: string): Promise<GuardianLinkInfo> {
  const link = await invoke<GuardianLinkInfo>('guardian_accept_invite', { code })
  await refreshLinks()
  return link
}

async function syncNow(): Promise<number> {
  const rows = await invoke<number>('guardian_sync_now')
  await refreshLinks()
  return rows
}

async function revokeLink(linkId: string): Promise<void> {
  await invoke('guardian_revoke_link', { linkId })
  await refreshLinks()
}

/** Mirrored child activity rows for one table (guardian side). */
async function childActivity<T = Record<string, unknown>>(
  linkId: string,
  table: string,
): Promise<T[]> {
  return invoke<T[]>('guardian_get_child_activity', { linkId, table })
}

/** Age computed from the synced birthdate, if known. */
export function childAge(link: GuardianLinkInfo): number | null {
  if (!link.child_birthdate) return null
  const born = new Date(`${link.child_birthdate}T00:00:00Z`)
  if (Number.isNaN(born.getTime())) return null
  const now = new Date()
  let age = now.getUTCFullYear() - born.getUTCFullYear()
  const monthDay = (d: Date) => (d.getUTCMonth() + 1) * 100 + d.getUTCDate()
  if (monthDay(now) < monthDay(born)) age -= 1
  return age
}

onProfileReady(() => {
  void refreshLinks()
})
onProfileLocked(() => {
  links.value = []
  loaded.value = false
})

export function useGuardian() {
  return {
    links: readonly(links),
    loaded: readonly(loaded),
    children,
    guardians,
    refreshLinks,
    acceptInvite,
    syncNow,
    revokeLink,
    childActivity,
  }
}
