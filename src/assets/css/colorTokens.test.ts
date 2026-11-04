// @vitest-environment node
import { readFileSync } from 'node:fs'
import { expect, test } from 'vitest'

const read = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8')
const block = (css: string, selector: string) => css.match(new RegExp(`^${selector} \\{([\\s\\S]*?)^\\}`, 'm'))?.[1] ?? ''
// A theme token with a literal fallback paints one theme's colour into the other.
const literalFallback = /var\(--[\w-]+,\s*(#|rgba?\()/

test('the atlas field palette is a design token with light and dark values', () => {
  const css = read('./main.css')
  for (const theme of [block(css, ':root'), block(css, '\\.dark')]) {
    for (let slot = 0; slot < 8; slot++) expect(theme).toMatch(new RegExp(`--atlas-field-${slot}:`))
  }
})

test('the notification badge and course completion colours come only from theme tokens', () => {
  const badge = read('../../components/layout/AppTopBar.vue').match(/\.topbar-bell-count \{([^}]*)\}/)?.[1] ?? ''
  expect(badge).toContain('background: var(--app-error);')
  expect(badge).toContain('color: var(--app-error-foreground);')
  expect(read('../../components/course/CourseCompletionModal.vue')).not.toMatch(literalFallback)
})
