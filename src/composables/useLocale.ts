import { computed, watch } from 'vue'

import { setActiveLocale } from '@/i18n'
import { loadLocaleMessages } from '@/i18n/loadLocale'
import {
  APP_LOCALES,
  DEFAULT_LOCALE,
  LOCALE_META,
  directionOf,
  isAppLocale,
  resolveSupported,
  type AppLocale,
} from '@/locales/meta'
import { useSetting, useSettings } from './useSettings'

// Persisted preference. `"system"` (the registry default) means "follow the OS
// language". Any other value is a concrete supported locale code. Sync-scoped,
// so the user's language follows them across devices — see
// `src-tauri/src/settings/registry.rs::USER_LANGUAGE`.
const SETTING_KEY = 'user.language'

// Pre-unlock cache — read synchronously by `initLocale()` so the correct
// language and text direction paint before the first render (avoids an
// LTR→RTL flash for Urdu). Mirrors `useTheme`'s pre-unlock cache: stores the
// last *resolved* locale, is never keyed per profile, and once a profile
// unlocks the per-profile `user.language` setting takes over.
const PRE_UNLOCK_CACHE_KEY = 'alexandria-locale-pre-unlock'

let initialized = false
// Cached OS locale resolution (async detection is only needed when the user
// preference is `"system"`).
let osResolved: AppLocale | null = null
// Explicit choice made before a profile settings store exists (onboarding /
// profile picker). Held here until `persistLocaleToProfile()` writes it into
// the unlocked profile's synced `user.language` setting.
let pendingChoice: string | null = null

const setting = useSetting<string>(SETTING_KEY)

function readPreference(): string {
  const v = setting.ref.value
  if (typeof v === 'string' && v.length > 0) return v
  // Pre-unlock: no settings store yet — reflect the in-session choice.
  if (pendingChoice) return pendingChoice
  return 'system'
}

/** Resolve the effective locale from the stored preference + OS fallback. */
function resolveEffective(): AppLocale {
  const pref = readPreference()
  if (pref !== 'system' && isAppLocale(pref)) return pref
  return osResolved ?? DEFAULT_LOCALE
}

function applyDirection(locale: AppLocale) {
  const el = document.documentElement
  el.setAttribute('dir', directionOf(locale))
  el.setAttribute('lang', locale)
}

/**
 * Load (if needed) and activate a locale: swap the vue-i18n active locale,
 * flip text direction, and refresh the pre-unlock cache so the next cold start
 * paints the same language. `en` is always resident, so it applies synchronously.
 */
async function applyLocale(locale: AppLocale): Promise<void> {
  await loadLocaleMessages(locale)
  setActiveLocale(locale)
  applyDirection(locale)
  localStorage.setItem(PRE_UNLOCK_CACHE_KEY, locale)
}

async function detectOsLocale(): Promise<AppLocale> {
  if (osResolved) return osResolved
  try {
    const { locale: osLocale } = await import('@tauri-apps/plugin-os')
    osResolved = resolveSupported(await osLocale())
  } catch {
    osResolved = DEFAULT_LOCALE
  }
  return osResolved
}

/**
 * Apply the last-known locale synchronously before the first render, using the
 * localStorage cache (no IPC / unlocked profile is available yet). If the cache
 * points at a non-English locale, its messages are loaded asynchronously and
 * swapped in as soon as they arrive; direction is set immediately either way.
 */
export function initLocale(): Promise<void> {
  if (initialized) return Promise.resolve()
  initialized = true

  const cached = localStorage.getItem(PRE_UNLOCK_CACHE_KEY)
  const locale = isAppLocale(cached) ? cached : DEFAULT_LOCALE
  applyDirection(locale)
  if (locale === DEFAULT_LOCALE) {
    setActiveLocale(DEFAULT_LOCALE)
    return Promise.resolve()
  }
  // Await the cached locale's messages before first render so setup-time
  // t() calls (e.g. label constants) resolve in the right language on cold
  // boot rather than briefly capturing English.
  return applyLocale(locale)
}

/**
 * Reconcile with the active profile's `user.language` after unlock. Idempotent —
 * call from `App.vue::hydrateProfileScopedState`. When the preference is
 * `"system"`, detects the OS locale first. Never writes the pre-unlock cache
 * back into profile settings (each profile owns its language independently).
 */
export async function initLocaleFromSettings(): Promise<void> {
  await useSettings().initialize()
  if (readPreference() === 'system') await detectOsLocale()
  await applyLocale(resolveEffective())
}

export function useLocale() {
  /** The effective (resolved) locale currently displayed. */
  const locale = computed<AppLocale>(() => resolveEffective())

  /** The raw stored preference, including the `"system"` sentinel. */
  const preference = computed<string>(() => readPreference())

  const available = APP_LOCALES.map((code) => ({ code, ...LOCALE_META[code] }))

  const isRtl = computed(() => directionOf(locale.value) === 'rtl')

  const isReviewed = computed(() => LOCALE_META[locale.value].reviewed)

  // Re-apply on any change to the preference — explicit switch, profile
  // change, or inbound sync from another device.
  watch(
    setting.ref,
    async () => {
      if (readPreference() === 'system') await detectOsLocale()
      await applyLocale(resolveEffective())
    },
    { immediate: false },
  )

  /**
   * Set the display language. `"system"` reverts to OS-follow. Applies
   * immediately (so pre-unlock onboarding reflects the choice even before a
   * profile settings store exists) and persists to the profile setting when
   * one is available.
   */
  async function setLocale(value: AppLocale | 'system'): Promise<void> {
    // Remember the explicit choice so a pre-unlock selection survives into the
    // profile once one exists.
    pendingChoice = value
    if (value === 'system') {
      await detectOsLocale()
      await applyLocale(resolveEffective())
    } else {
      await applyLocale(value)
    }
    // Persist when a profile settings store is loaded (this is where the synced
    // `user.language` gets written, propagating to the user's other devices).
    // Pre-unlock this is a no-op; `persistLocaleToProfile()` seeds it on unlock.
    await setting.set(value).catch(() => {})
  }

  /**
   * Seed the pending language choice into the active profile's synced settings.
   * Called on onboarding completion (and after unlock) so a language chosen
   * before the settings store existed sticks and syncs across devices.
   */
  async function persistLocaleToProfile(): Promise<void> {
    if (pendingChoice) {
      await setting.set(pendingChoice).catch(() => {})
    }
  }

  return {
    locale,
    preference,
    available,
    isRtl,
    isReviewed,
    setLocale,
    persistLocaleToProfile,
  }
}
