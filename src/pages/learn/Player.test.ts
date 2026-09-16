import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import Player from './Player.vue'
import type { Course, Element, ElementSkillTag, Enrollment } from '@/types'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
  start: vi.fn<(enrollment: string | null) => Promise<void>>(),
  stop: vi.fn<() => Promise<void>>(),
  active: { value: false },
  setElement: vi.fn(),
  setCameraOptedIn: vi.fn(),
  scoreGaze: vi.fn(),
  verifyFace: vi.fn(),
  getUserMedia: vi.fn<() => Promise<MediaStream>>(),
}))

vi.mock('vue-router', () => ({
  useRoute: () => ({ params: { id: 'course-1' } }),
  useRouter: () => ({ push: vi.fn() }),
}))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))
vi.mock('@/composables/profileSession', () => ({ getProfileSessionToken: () => 'profile-1' }))
vi.mock('@/composables/useCourseCompletion', () => ({ useCourseCompletion: () => ({ open: vi.fn() }) }))
vi.mock('@/components/course/elementRegistry', () => ({ resolveElementBinding: () => null }))
vi.mock('@/components/ui/InfoTip.vue', () => ({ default: { template: '<span />' } }))
vi.mock('@/components/ui', async () => ({
  AppButton: (await import('@/components/ui/AppButton.vue')).default,
  // The lesson feedback dialog; stubbed because these tests cover monitoring
  // ownership, not feedback.
  AppModal: { template: '<div><slot /></div>' },
  AppTextarea: { template: '<textarea />' },
  ProvenanceBadge: { template: '<span />' },
}))
vi.mock('@/composables/useSentinel', () => ({
  useSentinel: () => ({
    start: mocks.start, stop: mocks.stop, isActive: mocks.active,
    integrityScore: { value: 1 }, getSessionId: () => 'session-1',
    isAssessmentElement: (type: string) => type === 'quiz',
    setElement: mocks.setElement, setCameraOptedIn: mocks.setCameraOptedIn,
    scoreGaze: mocks.scoreGaze, verifyFace: mocks.verifyFace,
  }),
}))

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>(res => { resolve = res })
  return { promise, resolve }
}

const course: Course = {
  id: 'course-1', title: 'Course', description: null, author_address: 'author', author_name: null,
  content_cid: null, thumbnail_cid: null, thumbnail_svg: null, tags: null, skill_ids: null,
  version: 1, status: 'published', published_at: null, on_chain_tx: null,
  created_at: '', updated_at: '', kind: 'course',
}
const enrollment: Enrollment = {
  id: 'enrollment-1', course_id: course.id, enrolled_at: '', completed_at: null,
  status: 'active', updated_at: '', course_document_cid: null,
  course_document_version: null, completion_policy_json: null,
}
const elements: Element[] = ['quiz', 'text'].map((type, index) => ({
  id: `element-${index}`, chapter_id: 'chapter-1', title: type === 'quiz' ? 'Quiz' : 'Reading',
  element_type: type, content_cid: null, content_inline: null, position: index, duration_seconds: null,
}))
const mounted: ReturnType<typeof mount>[] = []
const mediaDescriptor = Object.getOwnPropertyDescriptor(navigator, 'mediaDevices')
let elementSkillTags: ElementSkillTag[] = []

function render() {
  const wrapper = mount(Player, {
    global: {
      mocks: { $t: (key: string) => key },
      stubs: {
        RouterLink: {
          props: ['to'],
          template: '<a :href="to"><slot /></a>',
        },
      },
    },
  })
  mounted.push(wrapper)
  return wrapper
}

async function leaveAssessment() {
  window.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight' }))
  await flushPromises()
}

function cameraStream() {
  const stop = vi.fn()
  return { stream: { getTracks: () => [{ stop }] } as unknown as MediaStream, stop }
}

beforeEach(() => {
  vi.useFakeTimers()
  vi.clearAllMocks()
  mocks.active.value = false
  mocks.start.mockReset().mockImplementation(async () => { mocks.active.value = true })
  mocks.stop.mockReset().mockImplementation(async () => { mocks.active.value = false })
  mocks.getUserMedia.mockReset()
  mocks.scoreGaze.mockReset().mockResolvedValue(null)
  mocks.verifyFace.mockReset().mockReturnValue({ present: true })
  elementSkillTags = []
  mocks.invoke.mockReset().mockImplementation(async command => {
    switch (command) {
      case 'get_course': return course
      case 'list_chapters': return [{ id: 'chapter-1', course_id: course.id, title: 'Chapter', position: 0 }]
      case 'list_enrollments': return [enrollment]
      case 'list_elements': return elements
      case 'get_progress': return []
      case 'get_course_completion_status': return { ready: false, missing_elements: ['element-0'], required_count: 1, preview: null }
      case 'list_element_skill_tags': return elementSkillTags
      default: return null
    }
  })
  Object.defineProperty(navigator, 'mediaDevices', { configurable: true, value: { getUserMedia: mocks.getUserMedia } })
  vi.spyOn(HTMLMediaElement.prototype, 'play').mockResolvedValue()
})

afterEach(async () => {
  for (const wrapper of mounted.splice(0)) wrapper.unmount()
  await flushPromises()
  vi.clearAllTimers()
  vi.useRealTimers()
  vi.restoreAllMocks()
  if (mediaDescriptor) Object.defineProperty(navigator, 'mediaDevices', mediaDescriptor)
  else Reflect.deleteProperty(navigator, 'mediaDevices')
})

describe('course player monitoring ownership', () => {
  it('loads current-element skill tags from their typed command', async () => {
    elementSkillTags = [{
      skill_id: 'skill-1',
      skill_name: 'Typed boundaries',
      bloom_level: 'apply',
      weight: 1,
    }]
    const wrapper = render()
    await flushPromises()

    expect(mocks.invoke).toHaveBeenCalledWith('list_element_skill_tags', {
      elementId: 'element-0',
    })
    const skillLink = wrapper.get('a[href="/skills/skill-1"]')
    expect(skillLink.text()).toBe('Typed boundaries')
  })

  it('stops pending startup when navigating to reading and ignores its late response', async () => {
    const starting = deferred<void>()
    mocks.start.mockReturnValueOnce(starting.promise)
    const wrapper = render()
    await flushPromises()
    expect(mocks.start).toHaveBeenCalledExactlyOnceWith(enrollment.id)
    const stopped = mocks.stop.mock.calls.length
    await leaveAssessment()
    expect(mocks.stop).toHaveBeenCalledTimes(stopped + 1)
    mocks.active.value = true
    starting.resolve()
    await flushPromises()
    expect(mocks.setElement).not.toHaveBeenCalled()
    expect(wrapper.text()).not.toContain('learn.player.integrityMonitoring')
  })

  it('stops pending startup on unmount without publishing the late session', async () => {
    const starting = deferred<void>()
    mocks.start.mockReturnValueOnce(starting.promise)
    const wrapper = render()
    await flushPromises()
    const stopped = mocks.stop.mock.calls.length
    wrapper.unmount()
    mocks.active.value = true
    starting.resolve()
    await flushPromises()
    expect(mocks.stop).toHaveBeenCalledTimes(stopped + 1)
    expect(mocks.setElement).not.toHaveBeenCalled()
  })

  it('reports a startup error and retries through the same watcher', async () => {
    mocks.start.mockRejectedValueOnce(new Error('monitoring unavailable'))
    const wrapper = render()
    await flushPromises()
    expect(wrapper.get('[role="alert"]').text()).toContain('monitoring unavailable')
    expect(wrapper.text()).not.toContain('learn.player.integrityMonitoring')
    await wrapper.get('[role="alert"] button').trigger('click')
    await flushPromises()
    expect(mocks.start).toHaveBeenCalledTimes(2)
    expect(mocks.setElement).toHaveBeenCalledExactlyOnceWith('element-0', 'quiz')
    expect(wrapper.find('[role="alert"]').exists()).toBe(false)
  })

  it('shows failed cleanup on a reading element and retries without starting another session', async () => {
    const wrapper = render()
    await flushPromises()
    mocks.stop.mockRejectedValueOnce(new Error('cleanup unavailable'))
    await leaveAssessment()
    expect(wrapper.get('[role="alert"]').text()).toContain('cleanup unavailable')
    expect(wrapper.text()).not.toContain('learn.player.integrityMonitoring')
    await wrapper.get('[role="alert"] button').trigger('click')
    await flushPromises()
    expect(wrapper.find('[role="alert"]').exists()).toBe(false)
    expect(mocks.start).toHaveBeenCalledOnce()
  })

  it('does not let an old camera grant replace or stop a newer stream', async () => {
    const oldGrant = deferred<MediaStream>()
    const oldCamera = cameraStream()
    const newCamera = cameraStream()
    mocks.getUserMedia.mockReturnValueOnce(oldGrant.promise).mockResolvedValueOnce(newCamera.stream)
    const wrapper = render()
    await flushPromises()
    const enable = () => wrapper.findAll('button').find(button => button.text() === 'learn.player.cameraEnable')!
    await enable().trigger('click')
    await leaveAssessment()
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft' }))
    await flushPromises()
    await enable().trigger('click')
    await vi.advanceTimersByTimeAsync(1)
    expect(mocks.setCameraOptedIn).toHaveBeenCalledWith(true)
    oldGrant.resolve(oldCamera.stream)
    await flushPromises()
    expect(oldCamera.stop).toHaveBeenCalledOnce()
    expect(newCamera.stop).not.toHaveBeenCalled()
    expect(wrapper.get('video').element.srcObject).toBe(newCamera.stream)
    await leaveAssessment()
    expect(newCamera.stop).toHaveBeenCalledOnce()
  })

  it.each(['reading', 'unmount'])('releases a camera granted after leaving for %s', async destination => {
    const media = deferred<MediaStream>()
    const camera = cameraStream()
    mocks.getUserMedia.mockReturnValueOnce(media.promise)
    const wrapper = render()
    await flushPromises()
    const enable = wrapper.findAll('button').find(button => button.text() === 'learn.player.cameraEnable')!
    await enable.trigger('click')
    expect(mocks.getUserMedia).toHaveBeenCalledOnce()
    if (destination === 'reading') await leaveAssessment()
    else wrapper.unmount()
    media.resolve(camera.stream)
    await flushPromises()
    expect(camera.stop).toHaveBeenCalledOnce()
    expect(mocks.setCameraOptedIn).not.toHaveBeenCalledWith(true)
    expect(mocks.scoreGaze).not.toHaveBeenCalled()
  })

  it('releases an active camera and stops both sampling loops when leaving an assessment', async () => {
    const camera = cameraStream()
    mocks.getUserMedia.mockResolvedValueOnce(camera.stream)
    const wrapper = render()
    await flushPromises()
    const enable = wrapper.findAll('button').find(button => button.text() === 'learn.player.cameraEnable')!
    await enable.trigger('click')
    await vi.advanceTimersByTimeAsync(3001)
    expect(mocks.setCameraOptedIn).toHaveBeenCalledWith(true)
    expect(mocks.scoreGaze).toHaveBeenCalled()
    expect(mocks.verifyFace).toHaveBeenCalledOnce()
    await leaveAssessment()
    expect(camera.stop).toHaveBeenCalledOnce()
    // Preserve the accumulated camera evidence for the owner's final score.
    expect(mocks.setCameraOptedIn).not.toHaveBeenCalledWith(false)
    const gazeChecks = mocks.scoreGaze.mock.calls.length
    await vi.advanceTimersByTimeAsync(6000)
    expect(mocks.scoreGaze).toHaveBeenCalledTimes(gazeChecks)
    expect(mocks.verifyFace).toHaveBeenCalledOnce()
  })
})
