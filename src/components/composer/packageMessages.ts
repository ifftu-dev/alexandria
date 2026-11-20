// Turns a package command's error into words an instructor can act on.
// The host's errors start with a stable code (`no_manifest: …`); the detail
// after it is English for whoever fixes the package and is shown beneath.

/**
 * Whether a course being edited is or will be public. Every published course
 * is public until private courses ship (course access spec); when they do,
 * this reads the course's visibility instead.
 */
export function courseWillBePublic(): boolean {
  return true
}

const CODES = [
  'not_an_archive',
  'no_manifest',
  'manifest_unparseable',
  'unsupported_standard',
  'launch_resource_missing',
  'launch_resource_remote',
  'unsafe_entry_path',
  'limit_exceeded',
  'package_result_self_reported',
  'not_a_package',
] as const

export type PackageErrorCode = typeof CODES[number]

export interface PackageErrorMessage {
  /** The i18n key of the instructor-facing explanation. */
  key: `instructor.package.refusal.${PackageErrorCode | 'other'}`
  params: Record<string, string>
  /** The host's own detail, when there is one. */
  detail: string | null
  code: PackageErrorCode | null
}

export function packageErrorMessage(error: unknown): PackageErrorMessage {
  const raw = (error instanceof Error ? error.message : String(error)).replace(/^Error:\s*/, '')
  const separator = raw.indexOf(':')
  const head = (separator >= 0 ? raw.slice(0, separator) : raw).trim()
  const code = CODES.find(known => known === head) ?? null
  if (!code) return { key: 'instructor.package.refusal.other', params: { error: raw }, detail: null, code: null }
  const detail = separator >= 0 ? raw.slice(separator + 1).trim() : ''
  return { key: `instructor.package.refusal.${code}`, params: {}, detail: detail || null, code }
}
