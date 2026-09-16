import type { Plugin } from 'vite'

const bundledFontsourceCss = /\/node_modules\/@fontsource\/(?:noto-sans-sc|noto-sans-devanagari|noto-sans-bengali|noto-sans-telugu|noto-nastaliq-urdu)\/\d+\.css$/

export function fontsourceWoff2Css(source: string, id: string): string | null {
  if (!bundledFontsourceCss.test(id.replaceAll('\\', '/'))) return null

  let sources = 0
  // This is deliberately restricted to Fontsource's generated two-format
  // declarations, not a general CSS rewrite. Unexpected upstream syntax must
  // fail for review rather than silently dropping a subset or fallback.
  const result = source.replace(/\bsrc:\s*([^;]+);/g, (_declaration: string, value: string) => {
    const pair = /^url\((\.\/files\/[-\w]+)\.woff2\) format\('woff2'\),\s*url\(\1\.woff\) format\('woff'\)$/.exec(value.trim())
    if (!pair) throw new Error(`Unsupported Fontsource font declaration in ${id}`)
    sources++
    return `src: url(${pair[1]}.woff2) format('woff2');`
  })
  const faces = [...source.matchAll(/@font-face\s*\{/g)].length
  if (faces === 0 || sources !== faces) {
    throw new Error(`Incomplete Fontsource font declarations in ${id}`)
  }
  return result
}

export function bundledFontFormats(): Plugin {
  return {
    name: 'bundled-font-formats',
    enforce: 'pre',
    transform(source, id) {
      const code = fontsourceWoff2Css(source, id)
      return code === null ? null : { code, map: null }
    },
  }
}
