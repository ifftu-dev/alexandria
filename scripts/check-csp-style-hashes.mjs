#!/usr/bin/env node

// Keep the CSP's style-src hashes in step with the libraries they were
// computed from.
//
// The CSP in src-tauri/tauri.conf.json has no 'unsafe-inline' for styles,
// deliberately: course content is rendered with v-html, and an inline style
// attribute in somebody else's markdown is exactly the CSS injection that
// policy exists to refuse. Two libraries we depend on inject a <style> block
// at import time, though — force-graph and its float-tooltip dependency — and
// a <style> block is inline style as far as CSP is concerned. Those two are
// allowed by content hash, which is the narrowest allowance there is: that
// CSS text and nothing else.
//
// A hash is only as current as the CSS it was taken from. Bump either
// library and the text changes, the hash stops matching, and the skill graph
// renders unstyled with no error anywhere but the webview console. So this
// recomputes the hashes from what is actually installed and compares them to
// the config. Fails (exit 1) on any difference, and prints the value to paste.
//
// Tauri's own nonce mechanism does not cover this case. It nonces <style>
// elements present in the HTML asset at serve time; a <style> created by a
// script later has no nonce and is refused. (A literal
// 'nonce-__TAURI_CSP_NONCE__' in the config is not a placeholder Tauri
// substitutes — it reaches the browser verbatim and allows nothing.)

import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const read = (p) => readFileSync(join(root, p), 'utf8')

// Where each library's injected CSS lives. rollup-plugin-postcss emits
//   var css_x = "…";
//   styleInject(css_x);
// and the hash must be over the exact string passed in.
const INJECTORS = [
  'node_modules/force-graph/dist/force-graph.mjs',
  'node_modules/float-tooltip/dist/float-tooltip.mjs',
]

function injectedCss(source) {
  const out = []
  const re = /var (\w+) = "((?:\\.|[^"\\])*)";\s*\n\s*styleInject\(\1\)/g
  for (const m of source.matchAll(re)) {
    // The literal is a JS string; evaluating it is the only faithful unescape.
    out.push(JSON.parse(`"${m[2]}"`))
  }
  return out
}

const sha256 = (text) =>
  `'sha256-${createHash('sha256').update(text, 'utf8').digest('base64')}'`

const expected = new Map()
for (const file of INJECTORS) {
  const blocks = injectedCss(read(file))
  if (blocks.length === 0) {
    console.error(`no styleInject() call found in ${file} — the library changed how it ships CSS; update this script`)
    process.exit(1)
  }
  for (const css of blocks) expected.set(sha256(css), file)
}

const conf = JSON.parse(read('src-tauri/tauri.conf.json'))
const csp = conf.app.security.csp
const styleSrc = csp.split(';').map((d) => d.trim()).find((d) => d.startsWith('style-src '))
if (!styleSrc) {
  console.error('no style-src directive in the CSP')
  process.exit(1)
}
const present = new Set(styleSrc.split(/\s+/).slice(1).filter((s) => s.startsWith("'sha256-")))

let failed = false
for (const [hash, file] of expected) {
  if (!present.has(hash)) {
    console.error(`missing from style-src: ${hash}   (${file})`)
    failed = true
  }
}
for (const hash of present) {
  if (!expected.has(hash)) {
    console.error(`stale in style-src, matches nothing installed: ${hash}`)
    failed = true
  }
}
if (present.size === 0 && expected.size > 0) {
  console.error("style-src has no sha256 sources at all — force-graph's styles will be refused")
  failed = true
}

if (failed) {
  console.error('\nstyle-src should carry exactly:')
  for (const hash of expected.keys()) console.error(`  ${hash}`)
  process.exit(1)
}
console.log(`CSP style hashes: ${expected.size} match`)
