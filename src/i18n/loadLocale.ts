// Lazy per-locale catalog loader. `import.meta.glob` lets Vite code-split each
// `locales/<loc>/index.ts` into its own chunk, so non-English message payloads
// (including the large non-Latin catalogs) are only fetched when the user
// actually selects that language.

import { i18n, type AppLocale } from './index'
import type { LocaleMessages } from '@/locales/en'

const loaders = import.meta.glob<LocaleMessages>('../locales/*/index.ts', {
  import: 'default',
})

const loaded = new Set<AppLocale>(['en'])

export function isLocaleLoaded(locale: AppLocale): boolean {
  return loaded.has(locale)
}

export async function loadLocaleMessages(locale: AppLocale): Promise<void> {
  if (loaded.has(locale)) return
  const load = loaders[`../locales/${locale}/index.ts`]
  if (!load) return
  const messages = await load()
  i18n.global.setLocaleMessage(locale, messages)
  loaded.add(locale)
}
