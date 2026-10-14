import { mount, flushPromises } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { reactive, ref } from 'vue'
import SentinelDebugPip from './SentinelDebugPip.vue'

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn(), getUserMedia: vi.fn(), setCameraOptedIn: vi.fn(), scoreGaze: vi.fn(), verifyFace: vi.fn() }))
const probeDefaults = () => ({
  suspended: false, lastSnapshotAt: 0,
  androidEnvSource: '', shieldRequested: false, shieldActive: false, overlayHidingSupported: false,
  developmentSettings: false, adbEnabled: false, foreignAccessibility: [] as string[], obscuredTouches: 0, androidSdk: 0,
  overlaySource: '', overlayScanned: 0, overlayAllowlisted: 0, overlaySuspicious: [] as string[],
  processSource: '', processesScanned: 0, watchedProcesses: [] as string[],
  hotkeysSupported: null as boolean | null, hotkeysSource: '', hotkeysPermission: false, hotkeysRunning: false,
  nativeHotkeys: 0, phantomHotkeys: [] as string[],
})
const debug = reactive({ active: true, integrity: 0.85, consistency: 0.9, flags: [] as string[], ...probeDefaults() })
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
  Object.assign(debug, probeDefaults())
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
  it('shows native probe rows as pending until the first snapshot', () => {
    debug.hotkeysSupported = true
    const wrapper = mountPanel()
    expect(wrapper.text()).not.toContain('0/0')
    expect(wrapper.text()).not.toContain('no-shield')
    expect(wrapper.text()).not.toContain('no-permission')
    wrapper.unmount()
  })

  it('shows n/a for probes the platform does not have, not clean zeros', () => {
    // A phone: no overlay scan, no process scan, no hotkey probe, and no
    // Android report (iOS) even though the shield was requested.
    Object.assign(debug, { lastSnapshotAt: 1, shieldRequested: true, hotkeysSupported: false, hotkeysSource: 'unsupported' })
    const wrapper = mountPanel()
    const text = wrapper.text()
    expect(text.match(/sentinel\.debug\.valNa/g)?.length).toBeGreaterThanOrEqual(4)
    expect(text).not.toContain('0/0')
    expect(text).not.toContain('shield?')
    expect(text).not.toContain('no-permission')
    wrapper.unmount()
  })

  it('names the desktop probe sources and allowlisted overlays', () => {
    Object.assign(debug, {
      lastSnapshotAt: 1,
      overlaySource: 'cgwindow', overlayScanned: 412, overlayAllowlisted: 2,
      processSource: 'nsworkspace', processesScanned: 318,
      hotkeysSupported: true, hotkeysSource: 'cgeventtap', hotkeysPermission: true, hotkeysRunning: true, nativeHotkeys: 3,
    })
    const wrapper = mountPanel()
    const text = wrapper.text()
    expect(text).toContain('0/412 allow×2·cgwindow')
    expect(text).toContain('0/318·nsworkspace')
    expect(text).toContain('0/3·cgeventtap')
    wrapper.unmount()
  })

  it('reports the Android environment details the probe returns', () => {
    Object.assign(debug, {
      lastSnapshotAt: 1, androidEnvSource: 'activity', shieldRequested: true, shieldActive: true,
      overlayHidingSupported: false, developmentSettings: true, androidSdk: 30,
    })
    const wrapper = mountPanel()
    expect(wrapper.text()).toContain('shield no-overlay-hide dev-options api30')
    wrapper.unmount()
  })

  it('shows a suspended session as suspended, not active', () => {
    debug.suspended = true
    const wrapper = mountPanel()
    expect(wrapper.text()).toContain('sentinel.debug.valSuspended')
    expect(wrapper.text()).not.toContain('sentinel.debug.valActive')
    wrapper.unmount()
  })
})
