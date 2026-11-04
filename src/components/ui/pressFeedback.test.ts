import { mount } from '@vue/test-utils'
import { expect, test } from 'vitest'
import AppSwitch from './AppSwitch.vue'
import AppTabs from './AppTabs.vue'

// jsdom does not apply main.css, so these check the opt-in contract: the
// shared controls take the `pressable` press feedback and expose their state,
// and a press changes that state at once. The desktop UI suite proves the
// cascade itself in Chromium and WebKit.

test('a switch takes the shared press feedback and reports its new position on the first click', async () => {
  const toggle = mount(AppSwitch, { props: { modelValue: true, label: 'Show notifications' } })
  const button = toggle.get('[role="switch"]')
  expect(button.classes()).toContain('pressable')
  expect(button.attributes('aria-checked')).toBe('true')
  await button.trigger('click')
  expect(toggle.emitted('update:modelValue')).toEqual([[false]])
})

test('tabs take the shared press feedback and mark the chosen one as pressed', async () => {
  const tabs = mount(AppTabs, { props: { modelValue: 'overview', tabs: [{ key: 'overview', label: 'Overview' }, { key: 'courses', label: 'Courses' }] } })
  const [overview, courses] = tabs.findAll('button')
  for (const tab of [overview!, courses!]) expect(tab.classes()).toContain('pressable')
  expect(overview!.attributes('aria-pressed')).toBe('true')
  expect(courses!.attributes('aria-pressed')).toBe('false')
  await courses!.trigger('click')
  expect(tabs.emitted('update:modelValue')).toEqual([['courses']])
})
