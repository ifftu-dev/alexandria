import { mount } from '@vue/test-utils'
import { afterEach, expect, test, vi } from 'vitest'
import AppOpening from './AppOpening.vue'

vi.mock('@/composables/usePlatform', () => ({ isAndroid: true, isIOS: false, isMobilePlatform: true }))

afterEach(() => { document.body.innerHTML = ''; vi.useRealTimers() })
test('on Android the opening continues the system splash: the mark starts centred and still, then docks', async () => {
  vi.useFakeTimers()
  const logo = document.createElement('div')
  logo.setAttribute('data-launch-target', '')
  logo.getBoundingClientRect = () => new DOMRect(20, 30, 64, 64)
  document.body.append(logo)
  const opening = mount(AppOpening, { props: { ready: false }, attachTo: document.body })
  const shell = opening.get('[data-opening]')
  // Same place as the splash icon: the centre of the window, no reveal replay.
  expect(shell.attributes('style')).toContain('top: calc(50% - 48px)')
  expect(shell.get('svg').classes()).not.toContain('brand-mark--opening')
  await opening.setProps({ ready: true })
  expect(opening.get('[data-opening]').attributes('style')).toContain('left: 20px')
  opening.unmount()
  expect(vi.getTimerCount()).toBe(0)
})
