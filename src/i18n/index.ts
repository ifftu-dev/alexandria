// vue-i18n bootstrap.
//
// Composition-API mode, English eager (source + guaranteed fallback), all other
// locales lazy-loaded on demand (see `loadLocale.ts`). Catalogs are precompiled
// to render functions by `@intlify/unplugin-vue-i18n` at build time so the
// runtime message compiler (forbidden by the Tauri `script-src 'self'` CSP) is
// never shipped.

import { createI18n } from 'vue-i18n'
import type { WritableComputedRef } from 'vue'

import en from '@/locales/en'
import { APP_LOCALES, DEFAULT_LOCALE, type AppLocale } from '@/locales/meta'

export { APP_LOCALES, DEFAULT_LOCALE }
export type { AppLocale }

export const i18n = createI18n({
  legacy: false,
  globalInjection: true,
  locale: DEFAULT_LOCALE,
  fallbackLocale: DEFAULT_LOCALE,
  messages: { en },
  missingWarn: import.meta.env.DEV,
  fallbackWarn: false,
})

// vue-i18n narrows `global.locale`'s type to the locales present at creation
// (only `en`, since the rest lazy-load). Set the active locale through this
// helper so callers aren't fighting that narrowed type.
export function setActiveLocale(locale: AppLocale): void {
  ;(i18n.global.locale as WritableComputedRef<string>).value = locale
}
