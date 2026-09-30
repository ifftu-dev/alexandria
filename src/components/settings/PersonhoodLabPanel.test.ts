import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { PersonhoodLabStatus } from '@/types'

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), detach: vi.fn() }))
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))
vi.mock('@/composables/useProfiles', () => ({ onProfileLocked: () => mocks.detach, useProfiles: () => ({ isUnlocked: { value: false } }) }))

function status(enabled: boolean, phase = 'idle'): PersonhoodLabStatus {
  return { enabled, phase, key_status: 'stored', downloaded_bytes: 612082146,
    total_bytes: 612082146, elapsed_ms: 0, error: null, result: null }
}

describe('PersonhoodLabPanel lifecycle', () => {
  beforeEach(() => { vi.resetModules(); vi.useFakeTimers(); mocks.invoke.mockReset(); mocks.detach.mockReset() })
  afterEach(() => { vi.useRealTimers() })

  it('renders no lab controls in a normal build', async () => {
    mocks.invoke.mockResolvedValue(status(false))
    const component = (await import('./PersonhoodLabPanel.vue')).default
    const wrapper = mount(component)
    await flushPromises()
    expect(wrapper.find('[data-testid="personhood-lab"]').exists()).toBe(false)
    expect(mocks.invoke).toHaveBeenCalledTimes(1)
    wrapper.unmount()
  })

  it('cancels native work and removes hooks when leaving the panel', async () => {
    mocks.invoke.mockResolvedValueOnce(status(true, 'proving')).mockResolvedValueOnce(status(true, 'cancelled'))
    const component = (await import('./PersonhoodLabPanel.vue')).default
    const wrapper = mount(component)
    await flushPromises()
    expect(wrapper.text()).toContain('Generating proof')
    wrapper.unmount()
    await flushPromises()
    expect(mocks.invoke).toHaveBeenLastCalledWith('personhood_lab_action', { action: 'cancel' })
    expect(mocks.detach).toHaveBeenCalledOnce()
    await vi.advanceTimersByTimeAsync(1000)
    expect(mocks.invoke).toHaveBeenCalledTimes(2)
  })
})
