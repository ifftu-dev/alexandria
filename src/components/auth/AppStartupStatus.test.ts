import { mount } from '@vue/test-utils'
import { afterEach, expect, test, vi } from 'vitest'
import { i18n } from '@/i18n'
import AppStartupStatus from './AppStartupStatus.vue'

afterEach(() => { vi.useRealTimers() })

test('a slow start adds "taking longer" under the current step after about two seconds', async () => {
  vi.useFakeTimers()
  const startup = mount(AppStartupStatus, { global: { plugins: [i18n] } })
  const status = startup.get('[role="status"]')
  expect(status.text()).toBe('Initializing…')
  await vi.advanceTimersByTimeAsync(1500)
  expect(status.text()).not.toContain('Taking longer than expected')
  await vi.advanceTimersByTimeAsync(600)
  expect(status.text()).toContain('Initializing…')
  expect(status.text()).toContain('Taking longer than expected')
  startup.unmount()
  expect(vi.getTimerCount()).toBe(0)
})
