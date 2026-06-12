import { ref } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'

/**
 * Resolve `did:key` strings to human display names, app-wide.
 *
 * DIDs are opaque to humans, so the UI shows usernames wherever possible.
 * The backend (`resolve_display_names`) names what it can — the active
 * profile's own DID and built-in plugin authors — and the rest fall back
 * to a shortened DID via [`shortDid`].
 *
 * Results are cached in a module-level map so repeated lookups across
 * components don't re-hit the backend. Call [`ensureNames`] with the DIDs
 * a view needs, then read [`displayName`] in templates.
 */

// Shared across all callers for the session.
const cache = ref<Record<string, string>>({})
const usernameCache = ref<Record<string, string>>({})
const inFlight = new Set<string>()

export function shortDid(did: string | null | undefined): string {
  if (!did) return 'Unknown'
  // did:key:z6Mk… — show a compact, stable fragment.
  const body = did.startsWith('did:key:') ? did.slice('did:key:'.length) : did
  if (body.length <= 14) return body
  return `${body.slice(0, 8)}…${body.slice(-4)}`
}

export function useDisplayNames() {
  const { invoke } = useLocalApi()

  /** Best-effort name for a DID — resolved name, else a short DID. */
  function displayName(did: string | null | undefined): string {
    if (!did) return 'Unknown'
    return cache.value[did] ?? shortDid(did)
  }

  /** Whether we have a real (resolved) name, not just a short-DID fallback. */
  function hasName(did: string | null | undefined): boolean {
    return !!did && !!cache.value[did]
  }

  /** The @username for a DID when known (no fallback). */
  function username(did: string | null | undefined): string | null {
    if (!did) return null
    return usernameCache.value[did] ?? null
  }

  /** "@handle" string for a DID, falling back to a short DID. */
  function handle(did: string | null | undefined): string {
    const u = username(did)
    return u ? `@${u}` : shortDid(did)
  }

  /** Resolve + cache any DIDs not already known. Safe to call repeatedly. */
  async function ensureNames(dids: Array<string | null | undefined>): Promise<void> {
    const want = Array.from(
      new Set(
        dids.filter((d): d is string => !!d && !(d in cache.value) && !inFlight.has(d)),
      ),
    )
    if (want.length === 0) return
    for (const d of want) inFlight.add(d)
    try {
      const [names, profiles] = await Promise.all([
        invoke<Record<string, string>>('resolve_display_names', { dids: want }),
        invoke<Record<string, { username: string | null; display_name: string | null }>>(
          'resolve_profiles',
          { dids: want },
        ).catch(() => ({}) as Record<string, { username: string | null; display_name: string | null }>),
      ])
      cache.value = { ...cache.value, ...names }
      const usernames: Record<string, string> = {}
      for (const [did, r] of Object.entries(profiles)) {
        if (r.username) usernames[did] = r.username
        if (r.display_name && !(did in names)) {
          cache.value = { ...cache.value, [did]: r.display_name }
        }
      }
      usernameCache.value = { ...usernameCache.value, ...usernames }
    } catch {
      // Leave unresolved → callers fall back to shortDid.
    } finally {
      for (const d of want) inFlight.delete(d)
    }
  }

  return { displayName, username, handle, hasName, ensureNames, shortDid }
}
