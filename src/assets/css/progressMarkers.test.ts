import postcss, { type Rule } from 'postcss'
import { parse } from 'vue/compiler-sfc'
import { expect, test } from 'vitest'

// Spinners and progress that keep moving in Reduced carry `data-progress`
// (main.css exempts the marker from the global reduced-motion rule). These
// guards read every component's template and scoped styles, so an indicator
// added without the marker, or silenced again by a local reduced-motion rule,
// fails here rather than freezing in the app.

const sources = import.meta.glob('../../**/*.vue', { query: '?raw', import: 'default', eager: true }) as Record<string, string>

/** Looping animations that are not progress, so they rightly stop in Reduced. */
const NOT_PROGRESS: Record<string, string> = {
  'chat-caret': 'the streaming caret blinks beside the marked "Writing" spinner',
  'app-spinner__arc': 'the arc holds a fixed gap in Reduced while the marked svg turns',
  'anim-3s': 'the onboarding hero orbit is decorative',
  'anim-2s-reverse': 'the onboarding hero orbit is decorative',
}

interface Tag { file: string; classes: Set<string>; marked: boolean; source: string }

function tagsOf(file: string, template: string): Tag[] {
  return [...template.matchAll(/<([a-zA-Z][\w-]*)\b([^>]*)>/g)].map(([source, , attrs = '']) => {
    const statics = [...attrs.matchAll(/(?:^|\s)class="([^"]*)"/g)].flatMap(([, list = '']) => list.split(/\s+/))
    const bound = [...attrs.matchAll(/(?:^|\s):class="([^"]*)"/g)].flatMap(([, expr = '']) => expr.match(/[\w-]+/g) ?? [])
    return { file, source, classes: new Set([...statics, ...bound].filter(Boolean)), marked: /(?:^|\s)data-progress(?=[\s=/]|$)/.test(attrs) }
  })
}

function targetClasses(selector: string): string[] {
  const last = selector.trim().split(/\s*[\s>+~]\s*/).pop() ?? ''
  return [...last.matchAll(/\.([\w-]+)/g)].map(([, name = '']) => name)
}

const components = Object.entries(sources).map(([file, source]) => {
  const { descriptor } = parse(source, { filename: file })
  return {
    file,
    tags: tagsOf(file, descriptor.template?.content ?? ''),
    styles: descriptor.styles.map(style => postcss.parse(style.content)),
  }
})

test('the guard reads every component', () => {
  expect(components.length).toBeGreaterThan(100)
})

test('every looping animation in a component targets a marked element of its template', () => {
  const problems: string[] = []
  for (const { file, tags, styles } of components) {
    for (const root of styles) {
      root.walkDecls(/^animation(-iteration-count)?$/, decl => {
        if (!/\binfinite\b/.test(decl.value)) return
        const rule = decl.parent as Rule
        for (const selector of rule.selectors) {
          const classes = targetClasses(selector)
          if (!classes.length) { problems.push(`${file}: "${selector}" loops but names no class`); continue }
          if (classes.some(name => name in NOT_PROGRESS)) continue
          const targets = tags.filter(tag => classes.every(name => tag.classes.has(name)))
          if (!targets.length) problems.push(`${file}: "${selector}" loops but nothing in the template carries it`)
          for (const tag of targets.filter(tag => !tag.marked)) problems.push(`${file}: ${tag.source} loops without data-progress`)
        }
      })
    }
  }
  expect(problems).toEqual([])
})

test('every Tailwind spinner is marked', () => {
  const unmarked = components.flatMap(({ tags }) => tags)
    .filter(tag => tag.classes.has('animate-spin') && !tag.marked && ![...tag.classes].some(name => name in NOT_PROGRESS))
    .map(tag => `${tag.file}: ${tag.source}`)
  expect(unmarked).toEqual([])
})

test('no component rule stops a marked indicator under reduced motion', () => {
  const problems: string[] = []
  for (const { file, tags, styles } of components) {
    for (const root of styles) {
      root.walkDecls(/^animation(-name)?$/, decl => {
        if (!/^none\b/.test(decl.value)) return
        const rule = decl.parent as Rule
        const media = rule.parent?.type === 'atrule' ? String((rule.parent as postcss.AtRule).params) : ''
        for (const selector of rule.selectors) {
          if (!media.includes('prefers-reduced-motion') && !selector.includes('data-motion')) continue
          const classes = targetClasses(selector)
          for (const tag of tags.filter(tag => tag.marked && classes.length && classes.every(name => tag.classes.has(name)))) {
            problems.push(`${file}: "${selector}" stops ${tag.source}`)
          }
        }
      })
    }
  }
  expect(problems).toEqual([])
})
