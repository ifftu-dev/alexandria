// @vitest-environment node
import { readFileSync } from 'node:fs'
import postcss, { type Declaration, type Root } from 'postcss'
import { parse } from 'vue/compiler-sfc'
import { expect, test } from 'vitest'

// Story 47: every duration comes from the `--duration-*` tokens in main.css
// (or `motionDuration()` in script), so tuning motion is one change. This guard
// reads every component's styles, its template style bindings and main.css,
// and fails on a literal `ms`/`s` duration or delay outside the list below.

const mainCss = readFileSync(new URL('./main.css', import.meta.url), 'utf8')
const sources = import.meta.glob('../../**/*.vue', { query: '?raw', import: 'default', eager: true }) as Record<string, string>

/** Rules that rightly carry a literal, keyed by `file: selector`, with the reason. */
const ALLOWED: Record<string, string> = {
  // The motion policy itself: Reduced and Off hold every transition at a floor.
  'main.css: :root[data-motion=\'reduced\'] *:not([data-progress])': 'the Reduced/Off floor is the policy, not a duration',
  'main.css: :root[data-motion=\'reduced\'] [data-progress]': 'the Reduced/Off floor is the policy, not a duration',
  // Negative delays only offset where a loop starts; they never delay anything.
  'main.css: .sf-delay-05': 'a negative delay offsets the starfield twinkle phase',
  'main.css: .sf-delay-1': 'a negative delay offsets the starfield twinkle phase',
  'main.css: .sf-delay-15': 'a negative delay offsets the starfield twinkle phase',
  'main.css: .sf-delay-2': 'a negative delay offsets the starfield twinkle phase',
  'main.css: .sf-delay-25': 'a negative delay offsets the starfield twinkle phase',
  'main.css: .sf-delay-35': 'a negative delay offsets the starfield twinkle phase',
  'main.css: .sf-delay-4': 'a negative delay offsets the starfield twinkle phase',
  'main.css: .sf-delay-5': 'a negative delay offsets the starfield twinkle phase',
  'components/plugin/PluginHost.vue: .ph-loading__dot:nth-child(2)': 'a negative delay offsets the loading dots phase',
  'components/plugin/PluginHost.vue: .ph-loading__dot:nth-child(3)': 'a negative delay offsets the loading dots phase',
  // A loop's period is its speed; Reduced swaps it for --duration-progress-reduced.
  'main.css: .anim-3s': 'the period of the decorative onboarding orbit loop',
  'main.css: .anim-2s-reverse': 'the period of the decorative onboarding orbit loop',
}

const TIMED = /^(transition|animation)(-duration|-delay)?$/
// A number with a time unit, not part of a token name such as --duration-2s.
const LITERAL = /(?<![\w-])-?(?:\d+\.?\d*|\.\d+)m?s\b/

function literalsIn(file: string, root: Root): string[] {
  const found: string[] = []
  root.walkDecls(TIMED, (decl: Declaration) => {
    if (!LITERAL.test(decl.value)) return
    // A looping animation's literal is its period, a speed rather than a duration.
    if (decl.prop === 'animation' && /\binfinite\b/.test(decl.value)) return
    const rule = decl.parent
    // The OS reduce preference holds transitions at the same floor as Reduced.
    if (rule?.parent?.type === 'atrule' && (rule.parent as postcss.AtRule).params.includes('prefers-reduced-motion')) return
    const selectors = rule?.type === 'rule' ? (rule as postcss.Rule).selectors : ['(no selector)']
    if (selectors.some(selector => `${file}: ${selector}` in ALLOWED)) return
    found.push(`${file}: ${selectors.join(', ')} { ${decl.prop}: ${decl.value} }`)
  })
  return found
}

const components = Object.entries(sources).map(([path, source]) => {
  const file = path.replace(/^\.\.\/\.\.\//, '')
  const { descriptor } = parse(source, { filename: file })
  return { file, descriptor }
})

test('the guard reads every component', () => {
  expect(components.length).toBeGreaterThan(100)
})

test('component styles and main.css take durations from the motion tokens', () => {
  const found = [
    ...literalsIn('main.css', postcss.parse(mainCss)),
    ...components.flatMap(({ file, descriptor }) => descriptor.styles.flatMap(style => literalsIn(file, postcss.parse(style.content)))),
  ]
  expect(found).toEqual([])
})

test('inline style bindings and scripted styles take durations from the motion tokens', () => {
  const found: string[] = []
  for (const { file, descriptor } of components) {
    const template = descriptor.template?.content ?? ''
    for (const [attr] of template.matchAll(/(?:^|\s):?style="[^"]*"/g)) {
      if (/(transition|animation)/i.test(attr) && (LITERAL.test(attr) || /\}m?s`/.test(attr))) found.push(`${file}: ${attr.trim()}`)
    }
    const script = `${descriptor.script?.content ?? ''}\n${descriptor.scriptSetup?.content ?? ''}`
    // A millisecond count written into the script (`${120 + i * 90}ms`), rather
    // than one derived from motionDuration().
    for (const [literal, expr = ''] of script.matchAll(/`[^`\n]*\$\{([^`\n]*)\}ms`/g)) {
      if (/(?<![\w.])\d{2,}/.test(expr)) found.push(`${file}: ${literal}`)
    }
  }
  expect(found).toEqual([])
})

test('Tailwind duration and delay utilities name a motion token', () => {
  // `duration-(--duration-base)` reads the token; `duration-200` repeats a number.
  const found = components.flatMap(({ file, descriptor }) => {
    const markup = `${descriptor.template?.content ?? ''}\n${descriptor.script?.content ?? ''}\n${descriptor.scriptSetup?.content ?? ''}`
    return [...markup.matchAll(/(?<![\w-])(?:duration|delay)-(?:\d+|\[[^\]]*\])(?![\w-])/g)].map(([utility]) => `${file}: ${utility}`)
  })
  expect(found).toEqual([])
})

test('every allowed rule still exists', () => {
  const selectors = new Set<string>()
  const collect = (file: string, root: Root) => root.walkRules(rule => { for (const selector of rule.selectors) selectors.add(`${file}: ${selector}`) })
  collect('main.css', postcss.parse(mainCss))
  for (const { file, descriptor } of components) for (const style of descriptor.styles) collect(file, postcss.parse(style.content))
  expect(Object.keys(ALLOWED).filter(key => !selectors.has(key))).toEqual([])
})
