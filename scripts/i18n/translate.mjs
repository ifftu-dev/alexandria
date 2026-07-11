#!/usr/bin/env node
// Machine-translation seeding pipeline. Reads the English catalog
// (src/locales/en/*.json) and drafts each non-English locale key-for-key,
// preserving `{named}` placeholders and `|` plural structure. Output is
// committed and flagged `reviewed: false` in src/locales/meta.ts until a native
// speaker signs off.
//
//   ANTHROPIC_API_KEY=... node scripts/i18n/translate.mjs [locale...]
//
// With no locale args, drafts every non-English launch locale. Re-runnable:
// only keys whose English value changed since the last run are re-translated
// (tracked by a hash manifest), so review effort stays incremental.
//
// Terminology is pinned via GLOSSARY so renamed concepts ("community" not
// "DAO") and the product name stay consistent across languages.

import { readFileSync, writeFileSync, readdirSync, existsSync, mkdirSync } from 'node:fs'
import { resolve, dirname, basename } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createHash } from 'node:crypto'

const here = dirname(fileURLToPath(import.meta.url))
const localesDir = resolve(here, '../../src/locales')
const manifestPath = resolve(here, '.translate-manifest.json')

const LANGS = {
  zh: 'Simplified Chinese (Mandarin)',
  es: 'Spanish',
  fr: 'French',
  hi: 'Hindi',
  ur: 'Urdu',
  te: 'Telugu',
  mr: 'Marathi',
  bn: 'Bengali',
}

// Do-not-translate / pinned terms.
const GLOSSARY = `- "Alexandria" is the product name — never translate it.
- Keep the motif line "I am, because we all are" translated naturally (Ubuntu philosophy).
- Preserve every {placeholder} EXACTLY, including braces and the name inside.
- Preserve the pipe character "|" and the number of plural branches around it.
- "Recovery Phrase" = the wallet backup words; translate as the standard local term.
- The community/governance feature is "Community" (not "DAO"/"blockchain").`

const hash = (s) => createHash('sha1').update(s).digest('hex').slice(0, 12)

function flatten(obj, prefix, acc) {
  for (const [k, v] of Object.entries(obj)) {
    const p = prefix ? `${prefix}.${k}` : k
    if (v && typeof v === 'object' && !Array.isArray(v)) flatten(v, p, acc)
    else acc[p] = String(v)
  }
  return acc
}

function unflatten(flat) {
  const root = {}
  for (const [path, val] of Object.entries(flat)) {
    const parts = path.split('.')
    let node = root
    for (let i = 0; i < parts.length - 1; i++) node = node[parts[i]] ??= {}
    node[parts[parts.length - 1]] = val
  }
  return root
}

async function translateBatch(langName, entries) {
  // entries: [{ key, value }]. Returns { key: translated }.
  const { default: Anthropic } = await import('@anthropic-ai/sdk')
  const client = new Anthropic()
  const payload = Object.fromEntries(entries.map((e) => [e.key, e.value]))
  const msg = await client.messages.create({
    model: 'claude-sonnet-5',
    max_tokens: 8192,
    system: `You are a professional UI localizer. Translate JSON string values into ${langName}.\n${GLOSSARY}\nReturn ONLY a JSON object with the same keys and translated values. Do not add commentary.`,
    messages: [{ role: 'user', content: JSON.stringify(payload, null, 2) }],
  })
  const text = msg.content.find((c) => c.type === 'text')?.text ?? '{}'
  const jsonStart = text.indexOf('{')
  const jsonEnd = text.lastIndexOf('}')
  return JSON.parse(text.slice(jsonStart, jsonEnd + 1))
}

async function main() {
  const targets = process.argv.slice(2).filter((a) => a in LANGS)
  const locales = targets.length ? targets : Object.keys(LANGS)

  // Load English source.
  const enDir = resolve(localesDir, 'en')
  const enFlat = {}
  const nsFiles = readdirSync(enDir).filter((f) => f.endsWith('.json'))
  for (const file of nsFiles) {
    const ns = basename(file, '.json')
    flatten(JSON.parse(readFileSync(resolve(enDir, file), 'utf8')), ns, enFlat)
  }

  const manifest = existsSync(manifestPath)
    ? JSON.parse(readFileSync(manifestPath, 'utf8'))
    : {}

  for (const loc of locales) {
    const dir = resolve(localesDir, loc)
    if (!existsSync(dir)) mkdirSync(dir, { recursive: true })

    // Existing translations, to preserve unchanged keys.
    const existingFlat = {}
    for (const file of nsFiles) {
      const p = resolve(dir, file)
      if (existsSync(p)) {
        flatten(JSON.parse(readFileSync(p, 'utf8')), basename(file, '.json'), existingFlat)
      }
    }

    // Only (re)translate changed or missing keys.
    const stale = []
    for (const [key, value] of Object.entries(enFlat)) {
      const sig = hash(value)
      const prev = manifest[loc]?.[key]
      if (prev !== sig || !(key in existingFlat)) stale.push({ key, value })
    }

    console.log(`[${loc}] ${stale.length} keys to translate (of ${Object.keys(enFlat).length})`)
    const translated = { ...existingFlat }
    const BATCH = 60
    for (let i = 0; i < stale.length; i += BATCH) {
      const chunk = stale.slice(i, i + BATCH)
      const out = await translateBatch(LANGS[loc], chunk)
      Object.assign(translated, out)
    }

    // Write back per-namespace, preserving the en file split.
    manifest[loc] ??= {}
    const grouped = {}
    for (const [key, value] of Object.entries(translated)) {
      const ns = key.split('.')[0]
      const rest = key.slice(ns.length + 1)
      ;(grouped[ns] ??= {})[rest] = value
      if (enFlat[key] !== undefined) manifest[loc][key] = hash(enFlat[key])
    }
    for (const file of nsFiles) {
      const ns = basename(file, '.json')
      const tree = unflatten(
        Object.fromEntries(
          Object.entries(grouped[ns] ?? {}).map(([k, v]) => [`${ns}.${k}`, v]),
        ),
      )[ns] ?? {}
      writeFileSync(resolve(dir, file), JSON.stringify(tree, null, 2) + '\n')
    }
    console.log(`[${loc}] wrote ${nsFiles.length} namespace files`)
  }

  writeFileSync(manifestPath, JSON.stringify(manifest, null, 2))
  console.log('done. Remember: locales stay reviewed:false in meta.ts until reviewed.')
}

main().catch((e) => {
  console.error(e)
  process.exit(1)
})
