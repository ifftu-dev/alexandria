#!/usr/bin/env node
// Guard against i18n regressions: flag user-visible HARDCODED text in .vue
// templates that isn't routed through i18n ($t / t()). Keeps new untranslated
// strings from silently re-entering the surface after the localization pass.
//
//   node scripts/i18n/check-no-raw-text.mjs
//
// Heuristic, not perfect: reports raw template text nodes and static
// title/aria-label/placeholder/alt attribute values containing real words.
// An inline allowlist covers deliberate non-translatable literals. Exits 1 on
// findings so CI blocks regressions.

import { readFileSync } from 'node:fs'
import { resolve, dirname, relative } from 'node:path'
import { fileURLToPath } from 'node:url'
import { globSync } from 'node:fs'
import { parse } from 'vue/compiler-sfc'

const here = dirname(fileURLToPath(import.meta.url))
const root = resolve(here, '../..')

// Files exempt entirely — dev-only debug surfaces never shown in production.
const FILE_SKIP = ['SentinelDebugPip.vue']

// Brand / proper nouns — matched as substrings.
const ALLOW_SUBSTR = [
  'Alexandria', 'Sentinel', 'IFFTU', 'Blockfrost', 'Cardano', 'GitHub',
]
// Non-translatable literals — matched against the exact trimmed text: units,
// abbreviations, keyboard keys, and the fixed footer attribution fragments.
const ALLOW_EXACT = new Set([
  'ms', 'wpm', 'px/ms', 'px/ms²', 'CC', 'PiP', 'DEV', 'esc', 'WPM', 'JSON',
  'TPR=', 'FPR=', 'diag.log', 'Built with', 'by',
])

const ATTRS = new Set(['title', 'aria-label', 'placeholder', 'alt'])
// Real words: at least two consecutive letters somewhere.
const HAS_WORD = /\p{L}{2,}/u
// Code/format-looking values (input placeholders showing a required format):
// URLs, JSON, ids with underscores/colons/braces/ellipsis. Not human copy.
const CODE_LIKE = /:\/\/|[{}[\]<>_]|\.\.\.|…|\w+:\w+|\w+\.\w+/

function isAllowed(text) {
  const t = text.trim()
  if (!HAS_WORD.test(t)) return true
  if (/^[\d\s.,:;!?%×·—–\-+/()[\]{}#@*]+$/u.test(t)) return true
  if (ALLOW_EXACT.has(t)) return true
  return ALLOW_SUBSTR.some((a) => t.includes(a))
}

const findings = []

function walk(node, file, insideRaw) {
  if (!node) return
  const tag = node.tag
  const raw = insideRaw || tag === 'pre' || tag === 'code' || tag === 'script' || tag === 'style'

  // Text node (type 2): raw content between tags.
  if (node.type === 2 && !raw) {
    const text = node.content
    if (text && HAS_WORD.test(text) && !isAllowed(text)) {
      findings.push({ file, line: node.loc?.start?.line ?? 0, text: text.trim().slice(0, 60) })
    }
  }

  // Static attributes on elements.
  if (node.type === 1 && Array.isArray(node.props)) {
    for (const p of node.props) {
      // ATTRIBUTE (type 6) = static; DIRECTIVE (7) like :title is dynamic → skip.
      if (p.type === 6 && ATTRS.has(p.name) && p.value?.content) {
        const v = p.value.content
        if (HAS_WORD.test(v) && !isAllowed(v) && !CODE_LIKE.test(v)) {
          findings.push({ file, line: p.loc?.start?.line ?? 0, text: `${p.name}="${v.slice(0, 50)}"` })
        }
      }
    }
  }

  for (const child of node.children ?? []) walk(child, file, raw)
}

const files = globSync('src/**/*.vue', { cwd: root })
  .filter((f) => !FILE_SKIP.some((s) => f.endsWith(s)))
  .map((f) => resolve(root, f))
for (const file of files) {
  const src = readFileSync(file, 'utf8')
  let descriptor
  try {
    descriptor = parse(src, { filename: file }).descriptor
  } catch {
    continue
  }
  const ast = descriptor.template?.ast
  if (ast) walk(ast, relative(root, file), false)
}

if (findings.length) {
  console.error(`no-raw-text: ${findings.length} hardcoded user-visible string(s) found:\n`)
  for (const f of findings) console.error(`  ${f.file}:${f.line}  ${f.text}`)
  console.error('\nWrap them in $t(...) / t(...) with a key in src/locales/en, or add a genuine literal to ALLOW.')
  process.exit(1)
}
console.log(`no-raw-text OK — ${files.length} .vue files clean`)
