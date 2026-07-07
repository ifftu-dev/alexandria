// Deep-link URL parser. Pure (no Tauri / no router imports) so it is
// trivially unit-testable. Accepts both link forms and normalizes them to a
// single canonical target:
//
//   custom scheme   alexandria://<action>/<rest…>?<query>
//   https app-link  https://alexandria.ifftu.dev/<action>/<rest…>?<query>
//
// For the custom scheme the URL *host* is the action; for https the first
// path segment is the action (the domain host is stripped). Anything that
// doesn't match a known action returns null — callers must treat null as
// "ignore this link", never navigate to a raw string.

/** Hosts we accept https app-links from. */
const APP_LINK_HOSTS = new Set(['alexandria.ifftu.dev'])

export type DeepLinkTarget =
  /** Guardian invite acceptance — handled specially (runs a backend command). */
  | { kind: 'guardian-accept'; code: string }
  /** A concrete in-app router path. The caller still validates it against the
   *  router's registered routes before navigating (defense in depth). */
  | { kind: 'route'; path: string }

/**
 * Parse a raw deep-link URL into a canonical {@link DeepLinkTarget}, or null
 * if it is not a recognized Alexandria link.
 */
export function parseDeepLink(raw: string): DeepLinkTarget | null {
  let url: URL
  try {
    url = new URL(raw.trim())
  } catch {
    return null
  }

  let action: string
  let segments: string[]

  if (url.protocol === 'alexandria:') {
    action = url.hostname
    segments = url.pathname.split('/').filter(Boolean)
  } else if (
    (url.protocol === 'https:' || url.protocol === 'http:') &&
    APP_LINK_HOSTS.has(url.hostname)
  ) {
    const parts = url.pathname.split('/').filter(Boolean)
    action = parts.shift() ?? ''
    segments = parts
  } else {
    return null
  }

  switch (action) {
    case 'guardian': {
      // alexandria://guardian/accept?code=<invite>
      if (segments[0] !== 'accept') return null
      const code = url.searchParams.get('code')?.trim()
      return code ? { kind: 'guardian-accept', code } : null
    }
    case 'course': {
      // alexandria://course/<id>  →  /courses/:id  (segment stays as the URL
      // already percent-encoded it — re-encoding would double-escape).
      const id = segments[0]
      return id ? { kind: 'route', path: `/courses/${id}` } : null
    }
    case 'classroom': {
      // alexandria://classroom/<id>  →  /classrooms/:id
      const id = segments[0]
      return id ? { kind: 'route', path: `/classrooms/${id}` } : null
    }
    case 'open': {
      // alexandria://open?route=/any/in-app/path  (generic fallback)
      const route = url.searchParams.get('route')
      // Must be a same-document absolute path. Reject protocol-relative
      // (`//host`) and anything not starting with a single slash so this can
      // never become an open redirect to an external origin.
      if (!route || !route.startsWith('/') || route.startsWith('//')) return null
      return { kind: 'route', path: route }
    }
    default:
      return null
  }
}
