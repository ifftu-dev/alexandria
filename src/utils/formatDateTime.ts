/**
 * An ISO 8601 timestamp as a medium date and short time in `locale`. Text
 * that does not parse is returned unchanged; a missing value is empty.
 */
export function formatDateTime(iso: string | null | undefined, locale: string): string {
  if (!iso) return ''
  const parsed = new Date(iso)
  if (Number.isNaN(parsed.getTime())) return iso
  return new Intl.DateTimeFormat(locale, { dateStyle: 'medium', timeStyle: 'short' }).format(parsed)
}
