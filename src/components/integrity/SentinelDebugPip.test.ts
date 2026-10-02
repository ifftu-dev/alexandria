import { mount, flushPromises } from '@vue/test-utils'
import { describe, expect, it, vi } from 'vitest'
import { reactive } from 'vue'
import SentinelDebugPip from './SentinelDebugPip.vue'

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }))
const debug = reactive({ active: true, integrity: 0.85, consistency: 0.9, flags: [] as string[] })
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))
vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))
vi.mock('@/composables/useSentinel', () => ({ useSentinel: () => ({ debug }) }))

describe('passive Sentinel Dev panel', () => {
  it('shows live telemetry without diagnostics, camera capture, or backend mutations', async () => {
    const wrapper = mount(SentinelDebugPip, { props: { readOnly: true }, global: {
      stubs: { Teleport: true }, mocks: { $t: (key: string) => key },
    } })
    await flushPromises()
    expect(wrapper.text()).toContain('85%')
    expect(wrapper.text()).toContain('sentinel.debug.valActive')
    expect(wrapper.find('video').exists()).toBe(false)
    expect(mocks.listen).not.toHaveBeenCalled()
    expect(mocks.invoke).not.toHaveBeenCalled()
    debug.integrity = 0.72
    await flushPromises()
    expect(wrapper.text()).toContain('72%')
    await wrapper.get('button').trigger('click')
    expect(wrapper.emitted('close')).toHaveLength(1)
    wrapper.unmount()
  })
})
