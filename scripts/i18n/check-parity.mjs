#!/usr/bin/env node
// Assert every non-English locale has EXACTLY the English key set, and that
// interpolation placeholders + pluralization branches match the source. Fails
// loudly (non-zero exit) so CI blocks broken translations.
//
//   node scripts/i18n/check-parity.mjs
//
// A missing key is NOT a runtime error: `src/i18n/index.ts` sets
// `fallbackLocale` to English and eager-loads the English catalog, so an
// untranslated string renders in English rather than as a raw key. This check
// exists to stop that fallback becoming invisible and permanent, not to
// pretend the app breaks without it.
//
// PENDING_TRANSLATION below acknowledges key prefixes that are shipped
// English-only on purpose. They are reported as pending and do not fail the
// run; everything else still does, so a NEW accidental gap is caught. Shrink
// this list as translations land — an entry that no longer has gaps is
// reported as stale so it cannot linger after the work is done.

import { readFileSync, readdirSync } from 'node:fs'
import { resolve, dirname, basename } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const localesDir = resolve(here, '../../src/locales')

function leaves(obj, prefix, acc) {
  for (const [k, v] of Object.entries(obj)) {
    const path = prefix ? `${prefix}.${k}` : k
    if (v && typeof v === 'object' && !Array.isArray(v)) leaves(v, path, acc)
    else acc.set(path, String(v))
  }
}

function catalog(loc) {
  const dir = resolve(localesDir, loc)
  const map = new Map()
  for (const file of readdirSync(dir)) {
    if (!file.endsWith('.json')) continue
    const ns = basename(file, '.json')
    leaves(JSON.parse(readFileSync(resolve(dir, file), 'utf8')), ns, map)
  }
  return map
}

// Key prefixes shipped English-only for now. Each needs a reason and an owner
// in the comment, so "temporary" stays visible.
const PENDING_TRANSLATION = [
  // Talent-index consent surface (EE Phase 2). Shipped English-only pending a
  // `npm run i18n:translate` pass; renders in English via the i18n fallback.
  'profile.talentIndex.',
  // Keyboard-shortcut help sheet. English-only pending a translate pass;
  // renders in English via the i18n fallback.
  'settings.personalization.shortcutsModal',
  'settings.personalization.shortcutNames.',
  // Sentinel appeal-evidence consent + integrity history. English-only pending
  // a translate pass; renders in English via the i18n fallback. This one is
  // worth prioritising — it is a consent surface, and consent a learner cannot
  // read in their own language is not consent.
  'sentinel.evidence.',
  // Credential import surface. English-only pending a translate pass.
  'credentials.import.',
  'credentials.page.import',
]

const isPending = (key) => PENDING_TRANSLATION.some((p) => key.startsWith(p))

const placeholders = (s) => (s.match(/\{[^}]+\}/g) ?? []).sort().join(',')
const pluralBranches = (s) => s.split('|').length

const en = catalog('en')
const locales = readdirSync(localesDir).filter(
  (d) => d !== 'en' && !d.endsWith('.ts') && !d.includes('.'),
)

let failures = 0
const pendingHits = new Set()
for (const loc of locales) {
  const cat = catalog(loc)
  for (const key of en.keys()) {
    if (!cat.has(key)) {
      if (isPending(key)) {
        pendingHits.add(key)
        continue
      }
      console.error(`✘ [${loc}] missing key: ${key}`)
      failures++
      continue
    }
    const a = en.get(key)
    const b = cat.get(key)
    if (placeholders(a) !== placeholders(b)) {
      console.error(`✘ [${loc}] placeholder mismatch @ ${key}: "${a}" vs "${b}"`)
      failures++
    }
    if (pluralBranches(a) !== pluralBranches(b)) {
      console.error(`✘ [${loc}] plural-branch mismatch @ ${key}`)
      failures++
    }
  }
  for (const key of cat.keys()) {
    if (!en.has(key)) console.warn(`⚠ [${loc}] extra key not in en: ${key}`)
  }
}

// A prefix with nothing left to translate is stale. Flagging it keeps the
// acknowledgement list from outliving the gap it was meant to cover.
const stale = PENDING_TRANSLATION.filter(
  (p) => ![...pendingHits].some((k) => k.startsWith(p)),
)
for (const p of stale) {
  console.error(`✘ stale PENDING_TRANSLATION prefix (nothing missing): ${p}`)
  failures++
}

if (failures) {
  console.error(`\ncatalog parity FAILED: ${failures} problem(s)`)
  process.exit(1)
}

if (pendingHits.size) {
  console.log(
    `⏳ ${pendingHits.size} key(s) pending translation — shown in English via fallback:`,
  )
  for (const p of PENDING_TRANSLATION) console.log(`   ${p}*`)
}
console.log(`catalog parity OK — ${locales.length} locales match en (${en.size} keys)`)
