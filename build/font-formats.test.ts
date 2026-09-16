// @vitest-environment node
import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import { fontsourceWoff2Css } from './font-formats'

const require = createRequire(import.meta.url)
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const id = '/app/node_modules/@fontsource/noto-sans-sc/400.css'
const source = `@font-face {
  font-family: 'Noto Sans SC';
  font-style: normal;
  font-weight: 400;
  font-display: swap;
  src: url(./files/noto-sans-sc-4-400-normal.woff2) format('woff2'), url(./files/noto-sans-sc-4-400-normal.woff) format('woff');
  unicode-range: U+4E00-9FFF;
}`

describe('bundled font formats', () => {
  it('removes only the equivalent WOFF fallback, preserving every other descriptor', () => {
    const expected = source.replace(", url(./files/noto-sans-sc-4-400-normal.woff) format('woff')", '')
    expect(fontsourceWoff2Css(source, id)).toBe(expected)
    expect(fontsourceWoff2Css(source, id.replaceAll('/', '\\'))).toBe(expected)
  })

  it('does not transform app CSS, other packages, raw imports, or unrelated files', () => {
    for (const path of ['/app/src/assets/fonts.css', '/app/node_modules/other/400.css',
      `${id}?raw`, `${id}.js`, '/app/node_modules/@fontsource/other/400.css']) {
      expect(fontsourceWoff2Css(source, path)).toBeNull()
    }
  })

  it('rejects missing sources, unexpected formats and mismatched fallback names', () => {
    for (const css of ['', '@font-face { font-family: Missing; }',
      source.replace("format('woff2')", "format('truetype')"),
      source.replace('normal.woff)', 'different.woff)'),
      source.replace('.woff2)', '.ttf)')]) {
      expect(() => fontsourceWoff2Css(css, id)).toThrow()
    }
  })

  it('preserves all installed subsets and weights imported by the application', () => {
    const main = readFileSync(resolve(root, 'src/main.ts'), 'utf8')
    const imports = [...main.matchAll(/import '(@fontsource\/[^']+\.css)'/g)].map(match => match[1]!)
    expect(imports).toHaveLength(10)
    let faces = 0
    for (const specifier of imports) {
      const path = require.resolve(specifier)
      const css = readFileSync(path, 'utf8')
      const output = fontsourceWoff2Css(css, path)
      expect(output).not.toBeNull()
      const withoutSources = (text: string) => text.replace(/\bsrc:[^;]+;/g, '')
      expect(withoutSources(output!)).toBe(withoutSources(css))
      const originalUrls = [...css.matchAll(/url\(([^)]+\.woff2)\)/g)].map(match => match[1]!)
      const retainedUrls = [...output!.matchAll(/url\(([^)]+\.woff2)\)/g)].map(match => match[1]!)
      expect(retainedUrls).toEqual(originalUrls)
      expect(output).not.toMatch(/\.woff[)'"]/)
      faces += originalUrls.length
      for (const font of retainedUrls) {
        expect(readFileSync(resolve(dirname(path), font)).subarray(0, 4).toString()).toBe('wOF2')
      }
    }
    expect(faces).toBe(226)
  })
})
