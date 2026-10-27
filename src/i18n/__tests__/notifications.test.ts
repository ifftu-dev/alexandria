import { expect, it } from 'vitest'
import contract from './fixtures/notification-copy-contract.json'
const resources = import.meta.glob<{ default: Record<string, unknown> }>('../../locales/*/notifications.json', { eager: true })

it.each(Object.entries(contract.locales))('shares the provider generic copy contract for %s', (locale, kinds) => {
  const resource = resources[`../../locales/${locale}/notifications.json`]!.default
  expect(resource.kinds).toMatchObject(kinds)
})

function leaves(value: unknown, prefix = ''): Record<string, string> {
  if (typeof value === 'string') return { [prefix]: value }
  return Object.fromEntries(Object.entries(value as Record<string, unknown>)
    .flatMap(([key, child]) => Object.entries(leaves(child, `${prefix}.${key}`))))
}

it.each(Object.keys(contract.locales).filter(locale => locale !== 'en'))('translates controls, local events and pending states in %s with key/token parity', locale => {
  const english = leaves(resources['../../locales/en/notifications.json']!.default)
  const translated = leaves(resources[`../../locales/${locale}/notifications.json`]!.default)
  expect(Object.keys(translated).sort()).toEqual(Object.keys(english).sort())
  for (const [key, value] of Object.entries(translated)) {
    expect(value.trim(), key).not.toBe('')
    expect(value.match(/\{\w+\}/g) ?? [], key).toEqual(english[key]!.match(/\{\w+\}/g) ?? [])
    // French uses the same word for Notifications; no other English placeholders remain.
    if (locale === 'fr' && ['.title', '.bell', '.settings.title'].includes(key)) continue
    expect(value, key).not.toBe(english[key])
  }
})
