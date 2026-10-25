// Vendor terms the connection forms point out before a user connects.
// The backend enforces the hard rules (alexandria-ai `terms`) and owns the
// host list; this only warns adults about a provider whose terms restrict
// apps minors may use.

import { RESTRICTS_APPS_FOR_MINORS_HOSTS } from '@/generated/chat-vocab'

function host(endpoint: string): string | null {
  try {
    return new URL(endpoint.trim()).hostname.replace(/\.$/, '').toLowerCase()
  } catch {
    return null
  }
}

/** Whether the provider's terms restrict use in apps people under 18 are likely to use. */
export function restrictsAppsForMinors(endpoint: string): boolean {
  const h = host(endpoint)
  return !!h && RESTRICTS_APPS_FOR_MINORS_HOSTS.some((s) => h === s || h.endsWith(`.${s}`))
}
