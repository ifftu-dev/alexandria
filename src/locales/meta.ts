// Locale registry — the single source of truth for which languages the app
// ships, how their names render (endonym = native name), text direction, and
// whether a native speaker has reviewed the (machine-seeded) translation.
//
// `en` is the source catalog and is always considered reviewed. Every other
// locale is machine-drafted from `en`; flip `reviewed` to `true` in a one-line
// change once a native speaker signs off — that clears the in-app
// "unreviewed translation" banner for that language.

export const APP_LOCALES = ['en', 'zh', 'es', 'fr', 'hi', 'ur', 'te', 'mr', 'bn'] as const

export type AppLocale = (typeof APP_LOCALES)[number]

export const DEFAULT_LOCALE: AppLocale = 'en'

export type Direction = 'ltr' | 'rtl'

export interface LocaleMeta {
  /** Native name of the language, shown in the switcher. */
  endonym: string
  /** English name, for accessibility labels and search keywords. */
  englishName: string
  dir: Direction
  /** `false` for machine-seeded catalogs awaiting native-speaker review. */
  reviewed: boolean
}

export const LOCALE_META: Record<AppLocale, LocaleMeta> = {
  en: { endonym: 'English', englishName: 'English', dir: 'ltr', reviewed: true },
  zh: { endonym: '中文', englishName: 'Chinese', dir: 'ltr', reviewed: false },
  es: { endonym: 'Español', englishName: 'Spanish', dir: 'ltr', reviewed: false },
  fr: { endonym: 'Français', englishName: 'French', dir: 'ltr', reviewed: false },
  hi: { endonym: 'हिन्दी', englishName: 'Hindi', dir: 'ltr', reviewed: false },
  ur: { endonym: 'اردو', englishName: 'Urdu', dir: 'rtl', reviewed: false },
  te: { endonym: 'తెలుగు', englishName: 'Telugu', dir: 'ltr', reviewed: false },
  mr: { endonym: 'मराठी', englishName: 'Marathi', dir: 'ltr', reviewed: false },
  bn: { endonym: 'বাংলা', englishName: 'Bengali', dir: 'ltr', reviewed: false },
}

export function isAppLocale(v: unknown): v is AppLocale {
  return typeof v === 'string' && (APP_LOCALES as readonly string[]).includes(v)
}

/**
 * Resolve any BCP-47 tag (e.g. from the OS) to the nearest supported locale.
 * Matches on the primary subtag; unknown tags fall back to `en`.
 */
export function resolveSupported(tag: string | null | undefined): AppLocale {
  if (!tag) return 'en'
  const lower = tag.toLowerCase()
  const primary = lower.split(/[-_]/)[0]
  if (isAppLocale(lower)) return lower
  if (isAppLocale(primary)) return primary
  return 'en'
}

export function directionOf(locale: AppLocale): Direction {
  return LOCALE_META[locale].dir
}
