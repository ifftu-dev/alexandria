import { mount } from '@vue/test-utils'
import { afterEach, expect, test } from 'vitest'
import { i18n } from '@/i18n'
import AppSpinner from './AppSpinner.vue'

afterEach(() => { delete document.documentElement.dataset.motion })

test('a spinner is a marked progress indicator that always carries text, even with motion off', () => {
  document.documentElement.dataset.motion = 'off'
  const uncaptioned = mount(AppSpinner, { global: { plugins: [i18n] } })
  expect(uncaptioned.get('[role="status"]').text()).toBe('Loading…')
  expect(uncaptioned.find('[data-progress]').exists()).toBe(true)
  const captioned = mount(AppSpinner, { props: { label: 'Saving draft' }, global: { plugins: [i18n] } })
  expect(captioned.get('[role="status"]').text()).toBe('Saving draft')
})
