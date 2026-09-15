import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import AssessmentRunner from './AssessmentRunner.vue'
import type { GradeResult, StartedAttempt, SubmittedAnswer } from '@/types'

const mocks = vi.hoisted(() => ({
  start: vi.fn<(enrollment: string | null) => Promise<void>>(),
  stop: vi.fn<() => Promise<string | undefined>>(),
  startAttempt: vi.fn<(skill: string, session: string) => Promise<StartedAttempt>>(),
  saveDraft: vi.fn<(attempt: string, answers: SubmittedAnswer[]) => Promise<void>>(),
  grade: vi.fn<(attempt: string, answers: SubmittedAnswer[]) => Promise<GradeResult>>(),
  active: { value: false },
  push: vi.fn(),
  go: vi.fn(),
}))

vi.mock('vue-router', () => ({
  useRoute: () => ({ params: { skillId: 'skill_test' } }),
  useRouter: () => ({ push: mocks.push, go: mocks.go }),
}))
vi.mock('@/composables/useSentinel', () => ({
  useSentinel: () => ({
    start: mocks.start,
    stop: mocks.stop,
    isActive: mocks.active,
    integrityScore: { value: 1 },
    getSessionId: () => mocks.active.value ? 'session-1' : null,
  }),
}))
vi.mock('@/composables/useAssessment', () => ({
  useAssessment: () => ({ startAttempt: mocks.startAttempt, saveDraft: mocks.saveDraft, grade: mocks.grade }),
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
  mocks.active.value = false
  mocks.start.mockReset().mockImplementation(async () => { mocks.active.value = true })
  mocks.stop.mockReset().mockImplementation(async () => { mocks.active.value = false; return 'session-1' })
  mocks.startAttempt.mockReset().mockResolvedValue(attempt)
  mocks.grade.mockReset().mockResolvedValue(result)
})

afterEach(async () => {
  for (const wrapper of mounted.splice(0)) wrapper.unmount()
  await flushPromises()
  vi.restoreAllMocks()
})

describe('standalone assessment monitoring', () => {
  it('keeps monitoring and selected answers on grading failure, then closes on successful retry', async () => {
    mocks.grade.mockRejectedValueOnce(new Error('grading unavailable'))
    const wrapper = render()
    await flushPromises()
    await wrapper.get('input').setValue(true)
    await wrapper.get('button').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('grading unavailable')
    expect(mocks.stop).not.toHaveBeenCalled()
    expect(mocks.active.value).toBe(true)
    expect((wrapper.get('input').element as HTMLInputElement).checked).toBe(true)
    await wrapper.get('button').trigger('click')
    await flushPromises()
    expect(mocks.startAttempt).toHaveBeenCalledExactlyOnceWith('skill_test', 'session-1')
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
    await wrapper.get('button').trigger('click')
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
    const submit = wrapper.getComponent({ name: 'AppButton' })
    submit.vm.$emit('click')
    submit.vm.$emit('click')
    await flushPromises()
    expect(mocks.grade).toHaveBeenCalledOnce()
    expect((wrapper.get('input').element as HTMLInputElement).disabled).toBe(true)
    grading.resolve(result)
    await flushPromises()
    expect(mocks.stop).toHaveBeenCalledOnce()
  })

  it('retains a successful grade when cleanup fails and retries only cleanup', async () => {
    mocks.stop.mockRejectedValueOnce(new Error('cleanup unavailable'))
    const wrapper = render()
    await flushPromises()
    await wrapper.get('button').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('learn.assessment.notPassedYet')
    expect(wrapper.get('[role="alert"]').text()).toContain('cleanup unavailable')
    const retake = () => wrapper.findAll('button').find(button => button.text() === 'learn.assessment.retake')!
    expect((retake().element as HTMLButtonElement).disabled).toBe(true)
    await wrapper.get('[role="alert"] button').trigger('click')
    await flushPromises()
    expect(mocks.grade).toHaveBeenCalledOnce()
    expect(mocks.stop).toHaveBeenCalledTimes(2)
    expect(wrapper.find('[role="alert"]').exists()).toBe(false)
    expect((retake().element as HTMLButtonElement).disabled).toBe(false)
  })
})
