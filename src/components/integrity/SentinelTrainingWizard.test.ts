import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import SentinelTrainingWizard from './SentinelTrainingWizard.vue'

type Sentinel = ReturnType<typeof import('@/composables/useSentinel').useSentinel>
const mocks = vi.hoisted(() => ({
  token: 'profile-A',
  keyboardCleanup: vi.fn(),
  mouseCleanup: vi.fn(),
  startTrainingKeystrokes: vi.fn(),
  startTrainingMouse: vi.fn(),
  clearTrainingBuffers: vi.fn(),
  saveTrainingProfile: vi.fn<Sentinel['saveTrainingProfile']>(),
  resetProfile: vi.fn<Sentinel['resetProfile']>(),
  getProfile: vi.fn<Sentinel['getProfile']>(),
  reportFaceDetection: vi.fn(),
  trainAIModels: vi.fn<Sentinel['trainAIModels']>(),
  enrollFace: vi.fn(),
  getAIModelStatus: vi.fn<Sentinel['getAIModelStatus']>(),
  refreshUserModelsStatus: vi.fn<Sentinel['refreshUserModelsStatus']>(),
  extractGazeFeatures: vi.fn<Sentinel['extractGazeFeatures']>(),
  trainGazeCalibration: vi.fn<Sentinel['trainGazeCalibration']>(),
  getUserMedia: vi.fn<MediaDevices['getUserMedia']>(),
}))

vi.mock('@/composables/useSentinel', () => ({
  useSentinel: () => ({
    ...mocks,
    getTrainingMetrics: () => ({ typing: {}, mouse: {} }),
  }),
}))
vi.mock('@/composables/profileSession', () => ({ getProfileSessionToken: () => mocks.token }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))
vi.mock('@/components/ui', async () => ({
  AppButton: (await import('@/components/ui/AppButton.vue')).default,
}))

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>(res => { resolve = res })
  return { promise, resolve }
}

function camera() {
  const stop = vi.fn()
  return { stream: { getTracks: () => [{ stop }] } as unknown as MediaStream, stop }
}

const mounted: ReturnType<typeof mount<typeof SentinelTrainingWizard>>[] = []
function render() {
  const wrapper = mount(SentinelTrainingWizard, { global: { mocks: { $t: (key: string) => key } } })
  mounted.push(wrapper)
  return wrapper
}
type Wizard = ReturnType<typeof render>
function button(wrapper: Wizard, key: string) {
  const found = wrapper.findAll('button').find(item => item.text() === key)
  if (!found) throw new Error(`Missing button: ${key}`)
  return found
}
async function click(wrapper: Wizard, key: string) {
  await button(wrapper, key).trigger('click')
  await flushPromises()
}
async function toCamera(wrapper: Wizard) {
  await flushPromises()
  await click(wrapper, 'sentinel.wizard.begin')
  await click(wrapper, 'sentinel.wizard.skipKeep')
  await click(wrapper, 'sentinel.wizard.skipKeep')
  await click(wrapper, 'common.actions.continue')
}
async function toGaze(wrapper: Wizard) {
  await toCamera(wrapper)
  await click(wrapper, 'sentinel.wizard.skip')
}
async function toReview(wrapper: Wizard) {
  await toGaze(wrapper)
  await click(wrapper, 'sentinel.wizard.skip')
}

beforeEach(() => {
  vi.resetAllMocks()
  vi.useFakeTimers()
  mocks.token = 'profile-A'
  mocks.startTrainingKeystrokes.mockReturnValue(mocks.keyboardCleanup)
  mocks.startTrainingMouse.mockReturnValue(mocks.mouseCleanup)
  mocks.saveTrainingProfile.mockResolvedValue()
  mocks.resetProfile.mockResolvedValue()
  mocks.getProfile.mockReturnValue(null)
  mocks.refreshUserModelsStatus.mockResolvedValue()
  const trained = { trained: true, epochs: 3, samples: 20, loss: 0.1 }
  mocks.getAIModelStatus.mockReturnValue({ keystrokeAE: trained, mouseCNN: trained, faceEmbedder: null })
  mocks.trainAIModels.mockResolvedValue({
    keystrokeAE: { trained: false, loss: -1, samples: 0 },
    mouseCNN: { trained: false, loss: -1, samples: 0 },
    faceEmbedder: { enrolled: false, progress: 0 },
  })
  mocks.extractGazeFeatures.mockResolvedValue({ yaw: 0, pitch: 0, roll: 0, irisDx: 0, irisDy: 0 })
  mocks.trainGazeCalibration.mockResolvedValue({ train_loss: 0.1, training_samples: 45, trained_epochs: 3 })
  Object.defineProperty(navigator, 'mediaDevices', { configurable: true, value: { getUserMedia: mocks.getUserMedia } })
  vi.spyOn(HTMLMediaElement.prototype, 'play').mockResolvedValue()
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null)
})

afterEach(async () => {
  for (const wrapper of mounted.splice(0)) wrapper.unmount()
  await flushPromises()
  expect(vi.getTimerCount()).toBe(0)
  vi.useRealTimers()
  vi.restoreAllMocks()
})

describe('Sentinel calibration lifecycle', () => {
  it('does not install queued step listeners after unmount', async () => {
    const wrapper = render()
    button(wrapper, 'sentinel.wizard.begin').element.dispatchEvent(new MouseEvent('click'))
    wrapper.unmount()
    await flushPromises()
    expect(mocks.startTrainingKeystrokes).not.toHaveBeenCalled()
  })

  it('releases training listeners and polling when their step ends', async () => {
    const wrapper = render()
    await toCamera(wrapper)
    expect(mocks.startTrainingKeystrokes).toHaveBeenCalledOnce()
    expect(mocks.startTrainingMouse).toHaveBeenCalledOnce()
    expect(mocks.keyboardCleanup).toHaveBeenCalledOnce()
    expect(mocks.mouseCleanup).toHaveBeenCalledOnce()
    expect(vi.getTimerCount()).toBe(0)
  })

  it.each(['navigate', 'unmount'] as const)('stops a late camera grant after %s', async action => {
    const pending = deferred<MediaStream>()
    const grant = camera()
    mocks.getUserMedia.mockReturnValue(pending.promise)
    const wrapper = render()
    await toCamera(wrapper)
    await click(wrapper, 'sentinel.wizard.enableCamera')
    if (action === 'unmount') wrapper.unmount()
    else await click(wrapper, 'sentinel.wizard.skip')
    pending.resolve(grant.stream)
    await flushPromises()
    expect(grant.stop).toHaveBeenCalledOnce()
    expect(HTMLMediaElement.prototype.play).not.toHaveBeenCalled()
    expect(mocks.enrollFace).not.toHaveBeenCalled()
    expect(vi.getTimerCount()).toBe(0)
  })

  it('releases the stream on playback failure and permits a new request', async () => {
    const first = camera()
    const second = camera()
    mocks.getUserMedia.mockResolvedValueOnce(first.stream).mockResolvedValueOnce(second.stream)
    vi.mocked(HTMLMediaElement.prototype.play).mockRejectedValueOnce(new Error('play failed'))
    const wrapper = render()
    await toCamera(wrapper)
    await click(wrapper, 'sentinel.wizard.enableCamera')
    expect(first.stop).toHaveBeenCalledOnce()
    expect(wrapper.text()).toContain('sentinel.wizard.cameraFailed')
    await click(wrapper, 'sentinel.wizard.enableCamera')
    expect(mocks.getUserMedia).toHaveBeenCalledTimes(2)
    wrapper.unmount()
    expect(second.stop).toHaveBeenCalledOnce()
  })

  it('does not let an obsolete permission result replace a newer camera stream', async () => {
    const pending = deferred<MediaStream>()
    const oldGrant = camera()
    const newGrant = camera()
    mocks.getUserMedia.mockReturnValueOnce(pending.promise).mockResolvedValueOnce(newGrant.stream)
    const wrapper = render()
    await toCamera(wrapper)
    await click(wrapper, 'sentinel.wizard.enableCamera')
    await click(wrapper, 'common.actions.back')
    await click(wrapper, 'common.actions.continue')
    await click(wrapper, 'sentinel.wizard.enableCamera')
    pending.resolve(oldGrant.stream)
    await flushPromises()
    expect(oldGrant.stop).toHaveBeenCalledOnce()
    expect(newGrant.stop).not.toHaveBeenCalled()
    expect((wrapper.get('video').element as HTMLVideoElement).srcObject).toBe(newGrant.stream)
    wrapper.unmount()
    expect(newGrant.stop).toHaveBeenCalledOnce()
  })

  it('coalesces gaze permission requests and releases a grant after skipping', async () => {
    const pending = deferred<MediaStream>()
    const grant = camera()
    mocks.getUserMedia.mockReturnValue(pending.promise)
    const wrapper = render()
    await toGaze(wrapper)
    await click(wrapper, 'sentinel.wizard.startCalibration')
    await click(wrapper, 'sentinel.wizard.startCalibration')
    expect(mocks.getUserMedia).toHaveBeenCalledOnce()
    await click(wrapper, 'sentinel.wizard.skip')
    pending.resolve(grant.stream)
    await flushPromises()
    expect(grant.stop).toHaveBeenCalledOnce()
    expect(mocks.extractGazeFeatures).not.toHaveBeenCalled()
    expect(mocks.trainGazeCalibration).not.toHaveBeenCalled()
  })

  it('cancels the gaze settling timer immediately on unmount', async () => {
    const grant = camera()
    mocks.getUserMedia.mockResolvedValue(grant.stream)
    const wrapper = render()
    await toGaze(wrapper)
    await click(wrapper, 'sentinel.wizard.startCalibration')
    expect(vi.getTimerCount()).toBe(1)
    wrapper.unmount()
    await flushPromises()
    expect(vi.getTimerCount()).toBe(0)
    expect(grant.stop).toHaveBeenCalledOnce()
    expect(mocks.extractGazeFeatures).not.toHaveBeenCalled()
  })

  it('discards a late frame and aborts further training after leaving the step', async () => {
    const pending = deferred<Awaited<ReturnType<Sentinel['extractGazeFeatures']>>>()
    const grant = camera()
    mocks.getUserMedia.mockResolvedValue(grant.stream)
    mocks.extractGazeFeatures.mockReturnValueOnce(pending.promise)
    const wrapper = render()
    await toGaze(wrapper)
    await click(wrapper, 'sentinel.wizard.startCalibration')
    await vi.advanceTimersByTimeAsync(700)
    expect(mocks.extractGazeFeatures).toHaveBeenCalledOnce()
    const signal = mocks.extractGazeFeatures.mock.calls[0]![1]!
    await wrapper.findAll('button')[0]!.trigger('click')
    expect(signal.aborted).toBe(true)
    pending.resolve({ yaw: 0, pitch: 0, roll: 0, irisDx: 0, irisDy: 0 })
    await flushPromises()
    expect(grant.stop).toHaveBeenCalledOnce()
    expect(mocks.trainGazeCalibration).not.toHaveBeenCalled()
    expect(vi.getTimerCount()).toBe(0)
    expect(wrapper.text()).toContain('sentinel.wizard.welcomeTitle')
  })

  it('captures all nine dots once and releases the camera on successful calibration', async () => {
    const grant = camera()
    mocks.getUserMedia.mockResolvedValue(grant.stream)
    const wrapper = render()
    await toGaze(wrapper)
    await click(wrapper, 'sentinel.wizard.startCalibration')
    await vi.runAllTimersAsync()
    expect(mocks.extractGazeFeatures).toHaveBeenCalledTimes(45)
    expect(mocks.trainGazeCalibration).toHaveBeenCalledOnce()
    const samples = mocks.trainGazeCalibration.mock.calls[0]![0]
    expect(samples).toHaveLength(45)
    expect(new Set(samples.map(sample => `${sample.targetX},${sample.targetY}`)).size).toBe(9)
    expect(grant.stop).toHaveBeenCalledOnce()
    expect(wrapper.text()).toContain('sentinel.wizard.gazeDone')
  })

  it('releases the camera and offers retry when gaze training fails', async () => {
    const grant = camera()
    mocks.getUserMedia.mockResolvedValue(grant.stream)
    mocks.trainGazeCalibration.mockRejectedValueOnce(new Error('training failed'))
    const wrapper = render()
    await toGaze(wrapper)
    await click(wrapper, 'sentinel.wizard.startCalibration')
    await vi.runAllTimersAsync()
    expect(grant.stop).toHaveBeenCalledOnce()
    expect(wrapper.text()).toContain('sentinel.wizard.gazeTrainFailed')
    expect(button(wrapper, 'sentinel.wizard.startCalibration').exists()).toBe(true)
  })

  it('retains a failed typing save for retry and prevents duplicate submissions', async () => {
    mocks.saveTrainingProfile.mockRejectedValueOnce(new Error('save unavailable'))
    const pending = deferred<void>()
    mocks.saveTrainingProfile.mockReturnValueOnce(pending.promise)
    const wrapper = render()
    await click(wrapper, 'sentinel.wizard.begin')
    await wrapper.get('textarea').setValue('a'.repeat(200))
    await click(wrapper, 'common.actions.continue')
    expect(wrapper.get('[role="alert"]').text()).toContain('save unavailable')
    expect(wrapper.text()).toContain('sentinel.wizard.typingTitle')
    await click(wrapper, 'common.actions.retry')
    await wrapper.get('[role="alert"] button').trigger('click')
    expect(mocks.saveTrainingProfile).toHaveBeenCalledTimes(2)
    pending.resolve()
    await flushPromises()
    expect(wrapper.text()).toContain('sentinel.wizard.mouseTitle')
    expect(wrapper.find('[role="alert"]').exists()).toBe(false)
  })

  it.each(['review', 'finish', 'reset'] as const)('ignores late %s results after unmount', async action => {
    const pending = deferred<void>()
    const wrapper = render()
    if (action === 'review') {
      const result = await mocks.trainAIModels()
      mocks.trainAIModels.mockClear().mockImplementationOnce(async () => { await pending.promise; return result })
    }
    await toReview(wrapper)
    let signal: AbortSignal | undefined
    if (action === 'review') signal = mocks.trainAIModels.mock.calls[0]![0]
    else if (action === 'finish') {
      mocks.saveTrainingProfile.mockReturnValueOnce(pending.promise)
      await click(wrapper, 'sentinel.wizard.finish')
      signal = mocks.saveTrainingProfile.mock.calls[0]![0]
    } else {
      mocks.resetProfile.mockReturnValueOnce(pending.promise)
      await click(wrapper, 'sentinel.wizard.recalibrate')
      signal = mocks.resetProfile.mock.calls[0]![0]
    }
    wrapper.unmount()
    expect(signal?.aborted).toBe(true)
    pending.resolve()
    await flushPromises()
    expect(wrapper.emitted('complete')).toBeUndefined()
    if (action === 'review') expect(mocks.getProfile).not.toHaveBeenCalled()
  })
})
