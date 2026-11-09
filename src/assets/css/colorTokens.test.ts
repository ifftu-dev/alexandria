// @vitest-environment node
import { readFileSync } from 'node:fs'
import { expect, test } from 'vitest'
import { ATLAS_PALETTE_SIZE } from '@/components/skills/atlasPresentation'

const read = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8')
const block = (css: string, selector: string) => css.match(new RegExp(`^${selector} \\{([\\s\\S]*?)^\\}`, 'm'))?.[1] ?? ''
// A theme token with a literal fallback paints one theme's colour into the other.
const literalFallback = /var\(--[\w-]+,\s*(#|rgba?\()/

test('the atlas field palette is a design token with light and dark values', () => {
  const css = read('./main.css')
  for (const theme of [block(css, ':root'), block(css, '\\.dark')]) {
    for (let slot = 0; slot < ATLAS_PALETTE_SIZE; slot++) expect(theme).toMatch(new RegExp(`--atlas-field-${slot}:`))
    expect(theme).not.toMatch(new RegExp(`--atlas-field-${ATLAS_PALETTE_SIZE}:`))
  }
})

test('every bundled subject field has a colour of its own', () => {
  const fields = JSON.parse(read('../../../bootstrap/public_taxonomy.json')).subject_fields as { atlas_slot: number }[]
  // Adding a bundled field beyond the palette means adding a light and dark colour first.
  expect(fields.length).toBeLessThanOrEqual(ATLAS_PALETTE_SIZE)
  expect(new Set(fields.map(field => field.atlas_slot % ATLAS_PALETTE_SIZE)).size).toBe(fields.length)
})

test('the notification badge and course completion colours come only from theme tokens', () => {
  const badge = read('../../components/layout/AppTopBar.vue').match(/\.topbar-bell-count \{([^}]*)\}/)?.[1] ?? ''
  expect(badge).toContain('background: var(--app-error);')
  expect(badge).toContain('color: var(--app-error-foreground);')
  expect(read('../../components/course/CourseCompletionModal.vue')).not.toMatch(literalFallback)
})
