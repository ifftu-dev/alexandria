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
//
// There is no public notification link. Banner and push taps reach the app
// as `notifications:open` events with an in-app route, never through here, so
// an outside page cannot open, mark read or follow an inbox row.

/** Hosts we accept https app-links from. */
const APP_LINK_HOSTS = new Set(['alexandria.ifftu.dev'])

export type DeepLinkTarget =
  /** Guardian invite acceptance — handled specially (runs a backend command). */
  | { kind: 'guardian-accept'; code: string }
  /** Bounded founding-genesis locator. Opening it only navigates to the
   *  review screen; retrieval and trust pinning remain explicit actions. */
  | { kind: 'governance-genesis-locator'; locator: string }
  /** A panelist's seat on an organisation's interview. The whole link goes to
   *  the backend, which validates it and attaches a local interview session. */
  | { kind: 'interview-panel'; link: string }
  /** A concrete in-app router path. The caller still validates it against the
   *  router's registered routes before navigating (defense in depth). */
  | { kind: 'route'; path: string }

// Resolves relative paths in `isInAppPath`; never fetched or shown.
const PATH_BASE = 'http://in-app.invalid'

/**
 * Is this a same-document absolute path: one leading slash, then nothing a
 * URL parser could read as another origin? Rejects protocol-relative `//host`,
 * backslashes (`/\host` is `//host` to a browser), control characters (parsers
 * strip tabs and newlines, so `/\t/host` would become `//host`), and every
 * scheme. A plain space is allowed (`/search?q=two words`): parsers encode it
 * rather than strip it, so it cannot join slashes. The one check for every
 * in-app route a link, notification or caller hands over; the router still has
 * to know the path.
 */
export function isInAppPath(path: string | null | undefined): path is string {
  if (!path || path[0] !== '/' || path[1] === '/') return false
  for (let i = 0; i < path.length; i++) {
    const code = path.charCodeAt(i)
    if (code < 0x20 || code === 0x7f || code === 0x5c) return false
  }
  try {
    return new URL(path, PATH_BASE).origin === PATH_BASE
  } catch {
    return false
  }
}

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
    case 'interview': {
      // alexandria://interview/join?cloud=…&id=…&panelist=…&token=… — custom
      // scheme only: the token is a credential and has no business in an
      // https URL that browsers and servers log.
      if (url.protocol !== 'alexandria:' || segments[0] !== 'join') return null
      const required = ['cloud', 'id', 'panelist', 'token']
      if (raw.trim().length > 2048 || !required.every(k => url.searchParams.get(k))) return null
      return { kind: 'interview-panel', link: url.href }
    }
    case 'governance': {
      if (segments[0] !== 'genesis' || !segments[1] || raw.trim().length > 2048) {
        return null
      }
      return { kind: 'governance-genesis-locator', locator: url.href }
    }
    case 'open': {
      // alexandria://open?route=/any/in-app/path  (generic fallback)
      const route = url.searchParams.get('route')
      // Must be a same-document absolute path, so this can never become an
      // open redirect to an external origin.
      if (!isInAppPath(route)) return null
      return { kind: 'route', path: route }
    }
    default:
      return null
  }
}
