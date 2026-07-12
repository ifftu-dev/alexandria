/**
 * i18n runtime + locale-registry tests.
 *
 * Guards the localization mechanism the whole app depends on: locale
 * resolution, text direction (RTL for Urdu), named interpolation, plurals,
 * and English fallback for missing keys. Uses small inline catalogs so it
 * stays decoupled from the (evolving) real message files — catalog *content*
 * parity is enforced separately by scripts/i18n/check-parity.mjs.
 */
import { describe, expect, it } from 'vitest'
import { createI18n } from 'vue-i18n'

import en from '@/locales/en'
import {
  APP_LOCALES,
  LOCALE_META,
  directionOf,
  isAppLocale,
  resolveSupported,
} from '@/locales/meta'

describe('locale registry (meta)', () => {
  it('every launch locale has metadata', () => {
    for (const code of APP_LOCALES) {
      expect(LOCALE_META[code], `meta for ${code}`).toBeTruthy()
      expect(LOCALE_META[code].endonym.length).toBeGreaterThan(0)
    }
  })

  it('Urdu is right-to-left; all others are left-to-right', () => {
    expect(directionOf('ur')).toBe('rtl')
    for (const code of APP_LOCALES) {
      if (code !== 'ur') expect(directionOf(code)).toBe('ltr')
    }
  })

  it('English is the reviewed source locale', () => {
    expect(LOCALE_META.en.reviewed).toBe(true)
  })

  it('resolveSupported maps BCP-47 tags to the nearest supported locale', () => {
    expect(resolveSupported('zh-Hant-TW')).toBe('zh')
    expect(resolveSupported('es-419')).toBe('es')
    expect(resolveSupported('en-US')).toBe('en')
    expect(resolveSupported('pt-BR')).toBe('en') // unsupported → fallback
    expect(resolveSupported(null)).toBe('en')
  })

  it('isAppLocale is a correct type guard', () => {
    expect(isAppLocale('ur')).toBe(true)
    expect(isAppLocale('xx')).toBe(false)
    expect(isAppLocale(42)).toBe(false)
  })
})

describe('i18n runtime behaviour', () => {
  const i18n = createI18n({
    legacy: false,
    locale: 'en',
    fallbackLocale: 'en',
    messages: {
      en: {
        greeting: 'Good morning, {name}',
        peers: 'no connections | {count} connection | {count} connections',
        onlyInEnglish: 'Fallback works',
      },
      es: {
        greeting: 'Buenos días, {name}',
        peers: 'sin conexiones | {count} conexión | {count} conexiones',
      },
    },
  })
  const t = i18n.global.t

  it('interpolates named params', () => {
    expect(t('greeting', { name: 'Ada' })).toBe('Good morning, Ada')
  })

  it('pluralizes with count', () => {
    expect(t('peers', { count: 0 }, 0)).toBe('no connections')
    expect(t('peers', { count: 1 }, 1)).toBe('1 connection')
    expect(t('peers', { count: 5 }, 5)).toBe('5 connections')
  })

  it('switches locale reactively', () => {
    i18n.global.locale.value = 'es'
    expect(t('greeting', { name: 'Ada' })).toBe('Buenos días, Ada')
    i18n.global.locale.value = 'en'
  })

  it('falls back to English for keys missing in the active locale', () => {
    i18n.global.locale.value = 'es'
    expect(t('onlyInEnglish')).toBe('Fallback works')
    i18n.global.locale.value = 'en'
  })
})

describe('English source catalog', () => {
  it('exposes the core namespaces', () => {
    for (const ns of ['common', 'network', 'onboarding', 'settings', 'credentials']) {
      expect(en, `namespace ${ns}`).toHaveProperty(ns)
    }
  })

  it('common namespace carries the shared vocabulary', () => {
    expect(en.common.actions.save).toBeTruthy()
    expect(en.common.status.connected).toBeTruthy()
    expect(en.common.unreviewedBanner.message).toBeTruthy()
  })
})
