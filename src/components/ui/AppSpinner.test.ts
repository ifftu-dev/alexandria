import { mount } from '@vue/test-utils'
import { compileStyle, parse } from 'vue/compiler-sfc'
import { afterEach, expect, test } from 'vitest'
import { i18n } from '@/i18n'
import AppSpinner from './AppSpinner.vue'
import source from './AppSpinner.vue?raw'

afterEach(() => { delete document.documentElement.dataset.motion })

test('a spinner is a marked progress indicator that always carries text, even with motion off', () => {
  document.documentElement.dataset.motion = 'off'
  const uncaptioned = mount(AppSpinner, { global: { plugins: [i18n] } })
  expect(uncaptioned.get('[role="status"]').text()).toBe('Loading…')
  expect(uncaptioned.find('[data-progress]').exists()).toBe(true)
  const captioned = mount(AppSpinner, { props: { label: 'Saving draft' }, global: { plugins: [i18n] } })
  expect(captioned.get('[role="status"]').text()).toBe('Saving draft')
})

test('the Reduced and Off arc rule styles only the spinner arc, never the whole document', () => {
  const id = 'data-v-spinner'
  const css = parse(source).descriptor.styles
    .map(style => compileStyle({ source: style.content, filename: 'AppSpinner.vue', id, scoped: style.scoped }).code)
    .join('\n')
    .replace(/\/\*[\s\S]*?\*\//g, '')
  const selectors = [...css.matchAll(/([^{}]+)\{/g)].flatMap(([, list = '']) => list.split(',').map(s => s.trim()))
  for (const mode of ['reduced', 'off']) {
    const ruled = selectors.filter(s => s.includes(`data-motion='${mode}'`) || s.includes(`data-motion=${mode}`))
    expect(ruled).not.toEqual([])
    for (const selector of ruled) expect(selector).toMatch(new RegExp(`^:root\\[data-motion=.?${mode}.?\\] \\.app-spinner__arc\\[${id}\\]$`))
  }
})
