#!/usr/bin/env node
// Assert every non-English locale has EXACTLY the English key set, and that
// interpolation placeholders + pluralization branches match the source. Fails
// loudly (non-zero exit) so CI blocks broken translations.
//
//   node scripts/i18n/check-parity.mjs

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

const placeholders = (s) => (s.match(/\{[^}]+\}/g) ?? []).sort().join(',')
const pluralBranches = (s) => s.split('|').length

const en = catalog('en')
const locales = readdirSync(localesDir).filter(
  (d) => d !== 'en' && !d.endsWith('.ts') && !d.includes('.'),
)

let failures = 0
for (const loc of locales) {
  const cat = catalog(loc)
  for (const key of en.keys()) {
    if (!cat.has(key)) {
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

if (failures) {
  console.error(`\ncatalog parity FAILED: ${failures} problem(s)`)
  process.exit(1)
}
console.log(`catalog parity OK — ${locales.length} locales match en (${en.size} keys)`)
