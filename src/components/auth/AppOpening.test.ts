import { mount } from '@vue/test-utils'
import { afterEach, expect, test, vi } from 'vitest'
import AppOpening from './AppOpening.vue'

afterEach(() => { document.body.innerHTML = ''; vi.useRealTimers() })
test('readiness moves the opening to the available logo; input dismisses it immediately', async () => {
  vi.useFakeTimers()
  const logo = document.createElement('div')
  logo.setAttribute('data-launch-target', '')
  logo.getBoundingClientRect = () => new DOMRect(20, 30, 64, 64)
  document.body.append(logo)
  const opening = mount(AppOpening, { props: { ready: false }, attachTo: document.body })
  expect(opening.find('[data-opening]').exists()).toBe(true)
  await opening.setProps({ ready: true })
  expect(opening.get('[data-opening]').attributes('style')).toContain('left: 20px')
  document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab' }))
  await opening.vm.$nextTick()
  expect(opening.find('[data-opening]').exists()).toBe(false)
  opening.unmount()
  expect(vi.getTimerCount()).toBe(0)
})
