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

test('tabs are a tablist that takes the shared press feedback and marks the chosen tab as selected', async () => {
  const tabs = mount(AppTabs, { props: { modelValue: 'overview', tabs: [{ key: 'overview', label: 'Overview' }, { key: 'courses', label: 'Courses' }] } })
  expect(tabs.get('.tabs').attributes('role')).toBe('tablist')
  const [overview, courses] = tabs.findAll('button')
  for (const tab of [overview!, courses!]) {
    expect(tab.classes()).toContain('pressable')
    expect(tab.attributes('role')).toBe('tab')
    expect(tab.attributes('aria-pressed')).toBeUndefined()
  }
  expect(overview!.attributes('aria-selected')).toBe('true')
  expect(courses!.attributes('aria-selected')).toBe('false')
  // Roving focus: only the selected tab is in the tab order.
  expect([overview!.attributes('tabindex'), courses!.attributes('tabindex')]).toEqual(['0', '-1'])
  await courses!.trigger('click')
  expect(tabs.emitted('update:modelValue')).toEqual([['courses']])
})

test('arrow keys, Home and End move focus and selection across the tabs, wrapping at the ends', async () => {
  const keys = [{ key: 'a', label: 'A' }, { key: 'b', label: 'B' }, { key: 'c', label: 'C' }]
  const tabs = mount(AppTabs, {
    attachTo: document.body,
    props: { modelValue: 'a', tabs: keys, 'onUpdate:modelValue': (key: string) => { void tabs.setProps({ modelValue: key }) } },
  })
  const buttons = () => tabs.findAll('[role="tab"]')
  await buttons()[0]!.trigger('keydown', { key: 'ArrowRight' })
  expect(document.activeElement).toBe(buttons()[1]!.element)
  await buttons()[1]!.trigger('keydown', { key: 'End' })
  expect(document.activeElement).toBe(buttons()[2]!.element)
  await buttons()[2]!.trigger('keydown', { key: 'ArrowRight' })
  expect(document.activeElement).toBe(buttons()[0]!.element)
  await buttons()[0]!.trigger('keydown', { key: 'ArrowLeft' })
  await buttons()[2]!.trigger('keydown', { key: 'Home' })
  expect(tabs.emitted('update:modelValue')).toEqual([['b'], ['c'], ['a'], ['c'], ['a']])
  expect(buttons().map(tab => tab.attributes('aria-selected'))).toEqual(['true', 'false', 'false'])
  tabs.unmount()
})

test('in a right-to-left layout the left arrow moves to the next tab', async () => {
  document.documentElement.dir = 'rtl'
  const tabs = mount(AppTabs, { attachTo: document.body, props: { modelValue: 'a', tabs: [{ key: 'a', label: 'A' }, { key: 'b', label: 'B' }] } })
  await tabs.findAll('[role="tab"]')[0]!.trigger('keydown', { key: 'ArrowLeft' })
  expect(tabs.emitted('update:modelValue')).toEqual([['b']])
  tabs.unmount()
  document.documentElement.dir = ''
})
