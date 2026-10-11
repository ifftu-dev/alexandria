import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import AssessmentRunner from './AssessmentRunner.vue'
import type { AttemptRoleTarget, GradeResult, StartedAttempt, SubmittedAnswer } from '@/types'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  start: vi.fn<(enrollment: string | null, camera?: boolean) => Promise<void>>(),
  setCameraOptedIn: vi.fn<(on: boolean) => void>(),
  reportCameraDevice: vi.fn<(label: string | null) => void>(),
  openRoles: vi.fn<(skill: string) => Promise<AttemptRoleTarget[]>>(),
  stop: vi.fn<() => Promise<string | undefined>>(),
  startAttempt: vi.fn<(skill: string, session: string, role: string | null) => Promise<StartedAttempt>>(),
  submitAnswers: vi.fn<(attempt: string, answers: SubmittedAnswer[]) => Promise<void>>(),
  saveDraft: vi.fn<(attempt: string, answers: SubmittedAnswer[]) => Promise<void>>(),
  grade: vi.fn<(attempt: string, answers: SubmittedAnswer[]) => Promise<GradeResult>>(),
  active: { value: false },
  push: vi.fn(),
  go: vi.fn(),
}))

vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))

vi.mock('vue-router', () => ({
  useRoute: () => ({ params: { skillId: 'skill_test' }, query: {} }),
  useRouter: () => ({ push: mocks.push, go: mocks.go }),
}))
vi.mock('@/composables/useSentinel', () => ({
  useSentinel: () => ({
    start: mocks.start,
    stop: mocks.stop,
    isActive: mocks.active,
    integrityScore: { value: 1 },
    getSessionId: () => mocks.active.value ? 'session-1' : null,
    setCameraOptedIn: mocks.setCameraOptedIn,
    reportCameraDevice: mocks.reportCameraDevice,
    verifyFace: () => ({ present: true, count: 1, consistency: 1 }),
    scoreGaze: async () => null,
  }),
}))
vi.mock('@/composables/useAssessment', () => ({
  useAssessment: () => ({ openRoles: mocks.openRoles, startAttempt: mocks.startAttempt, saveDraft: mocks.saveDraft, submitAnswers: mocks.submitAnswers, grade: mocks.grade }),
}))
vi.mock('@/composables/useDiagnostics', () => ({
  useDiagnostics: () => ({ registerEntryPreparation: () => () => undefined }),
}))
vi.mock('@/components/ui', async () => ({
  AppButton: (await import('@/components/ui/AppButton.vue')).default,
}))

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>(res => { resolve = res })
  return { promise, resolve }
}

const attempt: StartedAttempt = {
  attempt_id: 'attempt-1', skill_id: 'skill_test', pass_threshold: 0.7,
  questions: [{ id: 'question-1', prompt: 'Question', options: ['One', 'Two'] }],
  draft_answers: [],
}
const result: GradeResult = { score: 0.5, passed: false, credential_id: null }
const mounted: ReturnType<typeof mount>[] = []

function render() {
  const wrapper = mount(AssessmentRunner, { global: { mocks: { $t: (key: string) => key } } })
  mounted.push(wrapper)
  return wrapper
}

beforeEach(() => {
  vi.clearAllMocks()
  mocks.invoke.mockReset().mockResolvedValue(null)
  mocks.active.value = false
  mocks.start.mockReset().mockImplementation(async () => { mocks.active.value = true })
  mocks.stop.mockReset().mockImplementation(async () => { mocks.active.value = false; return 'session-1' })
  mocks.openRoles.mockReset().mockResolvedValue([])
  mocks.startAttempt.mockReset().mockResolvedValue(attempt)
  mocks.grade.mockReset().mockResolvedValue(result)
})

afterEach(async () => {
  for (const wrapper of mounted.splice(0)) wrapper.unmount()
  await flushPromises()
  vi.restoreAllMocks()
})

describe('standalone assessment monitoring', () => {
  it('recovers a finalized submission without starting another monitoring session', async () => {
    mocks.invoke.mockResolvedValueOnce({ score: 1, passed: true, credential_id: 'credential-1' })
    const wrapper = render()
    await flushPromises()
    expect(wrapper.text()).toContain('learn.assessment.passed')
    expect(mocks.start).not.toHaveBeenCalled()
    expect(mocks.startAttempt).not.toHaveBeenCalled()
  })

  it('keeps frozen answers on grading failure and retries without reopening monitoring', async () => {
    mocks.grade.mockRejectedValueOnce(new Error('grading unavailable'))
    const wrapper = render()
    await flushPromises()
    await wrapper.get('input').setValue(true)
    await wrapper.findAll('button').find(button => button.text() === 'learn.assessment.submit')!.trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('grading unavailable')
    expect(mocks.stop).toHaveBeenCalledOnce()
    expect(mocks.active.value).toBe(false)
    expect((wrapper.get('input').element as HTMLInputElement).checked).toBe(true)
    await wrapper.findAll('button').find(button => button.text() === 'learn.assessment.submit')!.trigger('click')
    await flushPromises()
    expect(mocks.startAttempt).toHaveBeenCalledExactlyOnceWith('skill_test', 'session-1', null)
    expect(mocks.grade.mock.calls).toEqual([
      ['attempt-1', [{ question_id: 'question-1', selected: [0] }]],
      ['attempt-1', [{ question_id: 'question-1', selected: [0] }]],
    ])
    expect(mocks.stop).toHaveBeenCalledOnce()
    expect(wrapper.text()).toContain('learn.assessment.notPassedYet')
  })

  it('does not create an attempt after monitoring startup fails', async () => {
    mocks.start.mockRejectedValueOnce(new Error('monitoring unavailable'))
    const wrapper = render()
    await flushPromises()
    expect(wrapper.text()).toContain('monitoring unavailable')
    expect(mocks.startAttempt).not.toHaveBeenCalled()
    expect(mocks.stop).toHaveBeenCalledOnce()
  })

  it('does not create an unmonitored attempt when startup was cancelled', async () => {
    mocks.start.mockResolvedValueOnce()
    const wrapper = render()
    await flushPromises()
    expect(wrapper.text()).toContain('Assessment monitoring is not active')
    expect(wrapper.text()).not.toContain('learn.assessment.integrityOn')
    expect(mocks.startAttempt).not.toHaveBeenCalled()
  })

  it('closes monitoring if attempt creation fails and exposes retryable cleanup', async () => {
    mocks.startAttempt.mockRejectedValueOnce(new Error('attempt unavailable'))
    mocks.stop.mockRejectedValueOnce(new Error('cleanup unavailable'))
    const wrapper = render()
    await flushPromises()
    expect(wrapper.text()).toContain('attempt unavailable')
    expect(wrapper.get('[role="alert"]').text()).toContain('cleanup unavailable')
    await wrapper.get('[role="alert"] button').trigger('click')
    await flushPromises()
    expect(mocks.stop).toHaveBeenCalledTimes(2)
    expect(wrapper.find('[role="alert"]').exists()).toBe(false)
    expect(mocks.startAttempt).toHaveBeenCalledOnce()
  })

  it('cancels component startup when unmounted during monitoring startup', async () => {
    const starting = deferred<void>()
    mocks.start.mockReturnValueOnce(starting.promise)
    const wrapper = render()
    wrapper.unmount()
    starting.resolve()
    await flushPromises()
    expect(mocks.startAttempt).not.toHaveBeenCalled()
    expect(mocks.stop).toHaveBeenCalledOnce()
  })

  it('ignores a late attempt response after unmount without starting more cleanup', async () => {
    const starting = deferred<StartedAttempt>()
    mocks.startAttempt.mockReturnValueOnce(starting.promise)
    const wrapper = render()
    await flushPromises()
    wrapper.unmount()
    starting.resolve(attempt)
    await flushPromises()
    expect(mocks.stop).toHaveBeenCalledOnce()
    expect(mocks.grade).not.toHaveBeenCalled()
  })

  it('ignores late grading after unmount instead of stopping a later session', async () => {
    const grading = deferred<GradeResult>()
    mocks.grade.mockReturnValueOnce(grading.promise)
    const wrapper = render()
    await flushPromises()
    await wrapper.findAll('button').find(button => button.text() === 'learn.assessment.submit')!.trigger('click')
    await flushPromises()
    wrapper.unmount()
    await flushPromises()
    mocks.active.value = true
    grading.resolve(result)
    await flushPromises()
    expect(mocks.stop).toHaveBeenCalledOnce()
    expect(mocks.active.value).toBe(true)
  })

  it('does not grade twice while a submission is in flight', async () => {
    const grading = deferred<GradeResult>()
    mocks.grade.mockReturnValueOnce(grading.promise)
    const wrapper = render()
    await flushPromises()
    const submit = wrapper.findAllComponents({ name: 'AppButton' }).find(button => button.text() === 'learn.assessment.submit')!
    submit.vm.$emit('click')
    submit.vm.$emit('click')
    await flushPromises()
    expect(mocks.grade).toHaveBeenCalledOnce()
    expect((wrapper.get('input').element as HTMLInputElement).disabled).toBe(true)
    grading.resolve(result)
    await flushPromises()
    expect(mocks.stop).toHaveBeenCalledOnce()
  })

  it('freezes answers and waits for cleanup before grading', async () => {
    mocks.stop.mockRejectedValueOnce(new Error('cleanup unavailable'))
    const wrapper = render()
    await flushPromises()
    await wrapper.findAll('button').find(button => button.text() === 'learn.assessment.submit')!.trigger('click')
    await flushPromises()
    expect(mocks.submitAnswers).toHaveBeenCalledOnce()
    expect(mocks.grade).not.toHaveBeenCalled()
    expect(wrapper.get('[role="alert"]').text()).toContain('cleanup unavailable')
    expect((wrapper.get('input').element as HTMLInputElement).disabled).toBe(true)
    await wrapper.get('[role="alert"] button').trigger('click')
    await flushPromises()
    const submit = wrapper.findAll('button').find(button => button.text() === 'learn.assessment.submit')!
    await submit.trigger('click')
    await flushPromises()
    expect(mocks.grade).toHaveBeenCalledOnce()
    expect(mocks.stop).toHaveBeenCalledTimes(2)
    expect(wrapper.text()).toContain('learn.assessment.notPassedYet')
  })
})

describe('role targeting', () => {
  const plainRole: AttemptRoleTarget = {
    role_assessment_id: 'role-plain', role_title: 'Analyst', org_name: 'Acme', camera_required: false,
  }
  const cameraRole: AttemptRoleTarget = {
    role_assessment_id: 'role-cam', role_title: 'SRE', org_name: 'Acme', camera_required: true, min_camera_coverage: 0.9,
  }

  function mockCamera(impl: () => Promise<MediaStream>) {
    const track = { label: 'OBS Virtual Camera', stop: vi.fn(), addEventListener: vi.fn() }
    const stream = {
      getTracks: () => [track],
      getVideoTracks: () => [track],
    } as unknown as MediaStream
    Object.defineProperty(navigator, 'mediaDevices', {
      configurable: true,
      value: { getUserMedia: vi.fn(impl.length ? impl : async () => stream) },
    })
    return { stream, track }
  }

  afterEach(() => {
    Object.defineProperty(navigator, 'mediaDevices', { configurable: true, value: undefined })
  })

  it('starts immediately when no role covers the skill', async () => {
    const wrapper = render()
    await flushPromises()
    expect(wrapper.find('[data-testid="role-chooser"]').exists()).toBe(false)
    expect(mocks.startAttempt).toHaveBeenCalledExactlyOnceWith('skill_test', 'session-1', null)
  })

  it('waits for a role choice and passes the chosen role to the attempt', async () => {
    mocks.openRoles.mockResolvedValue([plainRole, cameraRole])
    mocks.startAttempt.mockResolvedValue({ ...attempt, role: plainRole })
    const wrapper = render()
    await flushPromises()
    expect(wrapper.find('[data-testid="role-chooser"]').exists()).toBe(true)
    expect(mocks.start).not.toHaveBeenCalled()
    expect(mocks.startAttempt).not.toHaveBeenCalled()
    await wrapper.get('[data-testid="role-role-plain"]').setValue(true)
    await wrapper.get('[data-testid="start-attempt"]').trigger('click')
    await flushPromises()
    expect(mocks.start).toHaveBeenCalledExactlyOnceWith(null, false)
    expect(mocks.startAttempt).toHaveBeenCalledExactlyOnceWith('skill_test', 'session-1', 'role-plain')
    expect(wrapper.get('[data-testid="role-banner"]').text()).toContain('learn.assessment.roleBanner')
    expect(mocks.setCameraOptedIn).not.toHaveBeenCalled()
  })

  it('lets the learner decline a role and starts without one', async () => {
    mocks.openRoles.mockResolvedValue([plainRole])
    const wrapper = render()
    await flushPromises()
    await wrapper.get('[data-testid="role-none"]').setValue(true)
    await wrapper.get('[data-testid="start-attempt"]').trigger('click')
    await flushPromises()
    expect(mocks.startAttempt).toHaveBeenCalledExactlyOnceWith('skill_test', 'session-1', null)
  })

  it('blocks a camera-required role until the camera is on, then opts the session in', async () => {
    mocks.openRoles.mockResolvedValue([cameraRole])
    mocks.startAttempt.mockResolvedValue({ ...attempt, role: cameraRole })
    const { track } = mockCamera(() => Promise.reject(new Error('unused')))
    const wrapper = render()
    await flushPromises()
    await wrapper.get('[data-testid="role-role-cam"]').setValue(true)
    await flushPromises()
    const startButton = () => wrapper.get('[data-testid="start-attempt"]').element as HTMLButtonElement
    expect(wrapper.find('[data-testid="camera-gate"]').exists()).toBe(true)
    expect(startButton().disabled).toBe(true)
    await wrapper.get('[data-testid="camera-enable"]').trigger('click')
    await flushPromises()
    await new Promise(resolve => setTimeout(resolve, 0))
    await flushPromises()
    expect(mocks.reportCameraDevice).toHaveBeenCalledWith('OBS Virtual Camera')
    expect(wrapper.text()).toContain('learn.assessment.cameraReady')
    expect(startButton().disabled).toBe(false)
    expect(mocks.setCameraOptedIn).not.toHaveBeenCalled()
    await wrapper.get('[data-testid="start-attempt"]').trigger('click')
    await flushPromises()
    expect(mocks.start).toHaveBeenCalledExactlyOnceWith(null, true)
    expect(mocks.startAttempt).toHaveBeenCalledExactlyOnceWith('skill_test', 'session-1', 'role-cam')
    expect(mocks.setCameraOptedIn).toHaveBeenCalledWith(true)
    expect(wrapper.find('[data-testid="camera-lost"]').exists()).toBe(false)
    wrapper.unmount()
    await flushPromises()
    expect(track.stop).toHaveBeenCalled()
  })

  it('keeps the start blocked and shows the error when the camera is refused', async () => {
    mocks.openRoles.mockResolvedValue([cameraRole])
    Object.defineProperty(navigator, 'mediaDevices', {
      configurable: true,
      value: { getUserMedia: vi.fn(async () => { throw new Error('Permission denied') }) },
    })
    const wrapper = render()
    await flushPromises()
    await wrapper.get('[data-testid="role-role-cam"]').setValue(true)
    await wrapper.get('[data-testid="camera-enable"]').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('Permission denied')
    expect((wrapper.get('[data-testid="start-attempt"]').element as HTMLButtonElement).disabled).toBe(true)
    expect(mocks.start).not.toHaveBeenCalled()
  })

  it('tells a resumed camera-required attempt to turn the camera back on', async () => {
    mocks.openRoles.mockResolvedValue([plainRole])
    mocks.startAttempt.mockResolvedValue({ ...attempt, role: cameraRole })
    const wrapper = render()
    await flushPromises()
    await wrapper.get('[data-testid="role-none"]').setValue(true)
    await wrapper.get('[data-testid="start-attempt"]').trigger('click')
    await flushPromises()
    expect(wrapper.find('[data-testid="camera-lost"]').exists()).toBe(true)
  })
})
