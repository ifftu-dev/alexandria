import { mount, flushPromises } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { reactive, ref } from 'vue'
import SentinelDebugPip from './SentinelDebugPip.vue'

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn(), getUserMedia: vi.fn(), setCameraOptedIn: vi.fn(), scoreGaze: vi.fn(), verifyFace: vi.fn() }))
const debug = reactive({ active: true, integrity: 0.85, consistency: 0.9, flags: [] as string[] })
const optedIn = ref(false)
const stakeAddress = ref('stake_test_active')
vi.mock('@/composables/useProfiles', () => ({ useProfiles: () => ({ stakeAddress }) }))
vi.mock('vue-router', () => ({ useRoute: () => ({ path: '/assessment/skill_big_o' }) }))
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))
vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))
vi.mock('@/composables/useSentinel', () => ({ useSentinel: () => ({ debug, cameraOptedIn: optedIn, getSessionId: () => 'session-1', setCameraOptedIn: mocks.setCameraOptedIn, scoreGaze: mocks.scoreGaze, verifyFace: mocks.verifyFace, computeDeviceFingerprint: async () => '0123456789abcdef0123456789abcdef' }) }))

const mountPanel = () => mount(SentinelDebugPip, { props: { initiallyOpen: true }, global: {
  stubs: { Teleport: true }, mocks: { $t: (key: string) => key },
} })

beforeEach(() => {
  vi.clearAllMocks()
  debug.active = true; debug.integrity = 0.85; optedIn.value = false
  Object.defineProperty(navigator, 'mediaDevices', { configurable: true, value: { getUserMedia: mocks.getUserMedia } })
  vi.spyOn(HTMLMediaElement.prototype, 'play').mockResolvedValue()
})
afterEach(() => { vi.restoreAllMocks(); vi.useRealTimers() })

describe('Sentinel Dev panel', () => {
  it('shows live telemetry without diagnostics, camera capture, or backend mutations', async () => {
    const wrapper = mount(SentinelDebugPip, { props: { initiallyOpen: true }, global: {
      stubs: { Teleport: true }, mocks: { $t: (key: string) => key },
    } })
    await flushPromises()
    expect(wrapper.text()).toContain('85%')
    expect(wrapper.text()).toContain('sentinel.debug.valActive')
    expect(wrapper.find('video').exists()).toBe(true)
    expect(mocks.getUserMedia).not.toHaveBeenCalled()
    expect(mocks.listen).not.toHaveBeenCalled()
    expect(mocks.invoke).not.toHaveBeenCalled()
    debug.integrity = 0.72
    await flushPromises()
    expect(wrapper.text()).toContain('72%')
    await wrapper.get('button').trigger('click')
    expect(wrapper.emitted('close')).toHaveLength(1)
    wrapper.unmount()
  })

  it('shows permission errors and never opts a failed camera into the assessment', async () => {
    mocks.getUserMedia.mockRejectedValue(new Error('Camera denied'))
    const wrapper = mountPanel()
    await wrapper.findAll('button')[1]!.trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('Camera denied')
    expect(mocks.setCameraOptedIn).not.toHaveBeenCalled()
    wrapper.unmount()
  })

  it('stops a late camera grant after the panel was closed', async () => {
    let resolve!: (stream: MediaStream) => void
    mocks.getUserMedia.mockReturnValue(new Promise<MediaStream>(done => { resolve = done }))
    const stop = vi.fn()
    const wrapper = mountPanel()
    await wrapper.findAll('button')[1]!.trigger('click')
    await wrapper.findAll('button')[0]!.trigger('click')
    resolve({ getTracks: () => [{ stop }] } as unknown as MediaStream)
    await flushPromises()
    expect(stop).toHaveBeenCalledOnce()
    expect(mocks.setCameraOptedIn).not.toHaveBeenCalled()
    wrapper.unmount()
  })

  it('starts camera monitoring only on opt-in and releases it on close', async () => {
    const stop = vi.fn()
    mocks.getUserMedia.mockResolvedValue({ getTracks: () => [{ stop }] })
    const wrapper = mountPanel()
    await wrapper.findAll('button')[1]!.trigger('click')
    await flushPromises()
    expect(mocks.setCameraOptedIn).toHaveBeenCalledWith(true)
    expect(wrapper.text()).toContain('sentinel.debug.stopCamera')
    wrapper.unmount()
    expect(stop).toHaveBeenCalledOnce()
    expect(mocks.setCameraOptedIn).toHaveBeenLastCalledWith(false)
  })

  it('does not present idle defaults as measured integrity', () => {
    debug.active = false
    const wrapper = mountPanel()
    expect(wrapper.text()).toContain('sentinel.debug.idleNote')
    expect(wrapper.text()).not.toContain('85%')
    wrapper.unmount()
  })

  it('marks preview frames as non-evidence and surfaces inference failures', async () => {
    vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] })
    debug.active = false
    mocks.getUserMedia.mockResolvedValue({ getTracks: () => [{ stop: vi.fn() }] })
    mocks.invoke.mockRejectedValue(new Error('Face model unavailable'))
    vi.spyOn(HTMLMediaElement.prototype, 'readyState', 'get').mockReturnValue(2)
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue({
      drawImage: vi.fn(), getImageData: () => ({ data: new Uint8ClampedArray(4) }),
    } as unknown as CanvasRenderingContext2D)
    const wrapper = mountPanel()
    await wrapper.findAll('button')[1]!.trigger('click')
    await flushPromises()
    await vi.advanceTimersByTimeAsync(700)
    expect(mocks.invoke).toHaveBeenCalledWith('sentinel_score_gaze', expect.objectContaining({
      req: expect.objectContaining({ preview_only: true, user_address: 'stake_test_active', device_fp_prefix: '0123456789abcdef' }),
    }))
    expect(wrapper.text()).toContain('Face model unavailable')
    expect(mocks.setCameraOptedIn).not.toHaveBeenCalled()
    wrapper.unmount()
  })
})
