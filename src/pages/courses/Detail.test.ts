import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import Detail from './Detail.vue'
import type { Course } from '@/types'

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), push: vi.fn() }))
vi.mock('vue-router', () => ({ useRoute: () => ({ params: { id: 'course-1' } }), useRouter: () => ({ push: mocks.push }) }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))
vi.mock('@/composables/useProfiles', () => ({ useProfiles: () => ({ stakeAddress: { value: 'author' } }) }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }))
vi.mock('@/components/course/EnrollPluginDialog.vue', () => ({ default: { template: '<div />' } }))

let course: Course
const mounted: ReturnType<typeof mount>[] = []
const render = () => {
  const wrapper = mount(Detail, { global: { mocks: { $t: (key: string) => key } } })
  mounted.push(wrapper)
  return wrapper
}
beforeEach(() => {
  course = { id: 'course-1', title: 'Draft', description: null, author_address: 'author', author_name: null,
    content_cid: null, thumbnail_cid: null, thumbnail_svg: null, tags: null, skill_ids: null,
    version: 1, status: 'draft', published_at: null, on_chain_tx: null, created_at: '', updated_at: '', kind: 'course' }
  mocks.invoke.mockReset().mockImplementation(async (command: string) => {
    switch (command) {
      case 'get_course': return { ...course }
      case 'list_chapters': case 'list_enrollments': case 'course_required_plugins': return []
      case 'prepare_local_course': course.content_cid = 'a'.repeat(64); return {}
      case 'enroll': return { id: 'enrollment', course_id: course.id, status: 'active' }
      default: throw new Error(`Unexpected command: ${command}`)
    }
  })
})
afterEach(() => { mounted.splice(0).forEach(w => w.unmount()); vi.clearAllMocks() })

describe('course enrollment', () => {
  it('prepares an owned draft before enrolling and offers Continue learning', async () => {
    const wrapper = render(); await flushPromises()
    const button = wrapper.findAll('button').find(b => b.text().includes('prepareAndEnroll'))!
    await button.trigger('click'); await flushPromises()
    const commands = mocks.invoke.mock.calls.map(c => c[0])
    expect(commands.indexOf('prepare_local_course')).toBeLessThan(commands.indexOf('enroll'))
    expect(mocks.invoke).toHaveBeenCalledWith('prepare_local_course', { courseId: course.id })
    expect(wrapper.text()).toContain('courses.detail.continueLearning')
    expect(mocks.invoke).not.toHaveBeenCalledWith('publish_course', expect.anything())
  })
  it('shows preparation errors instead of silently failing', async () => {
    const handler = mocks.invoke.getMockImplementation()!
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === 'prepare_local_course') throw new Error('Signing failed')
      return handler(command)
    })
    const wrapper = render(); await flushPromises()
    await wrapper.findAll('button').find(b => b.text().includes('prepareAndEnroll'))!.trigger('click')
    await flushPromises()
    expect(wrapper.get('[role=alert]').text()).toContain('Signing failed')
    expect(mocks.invoke.mock.calls.some(c => c[0] === 'enroll')).toBe(false)
  })
  it('does not prepare another author’s unpublished draft', async () => {
    course.author_address = 'someone-else'
    const wrapper = render(); await flushPromises()
    const button = wrapper.findAll('button').find(b => b.text() === 'courses.detail.enroll')!
    expect(button.attributes('disabled')).toBeDefined()
    expect(wrapper.text()).toContain('courses.detail.awaitingPublication')
    expect(mocks.invoke.mock.calls.some(c => c[0] === 'prepare_local_course')).toBe(false)
  })
  it('does not bypass a failed plugin pre-flight', async () => {
    course.status = 'published'; course.content_cid = 'b'.repeat(64)
    const handler = mocks.invoke.getMockImplementation()!
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === 'course_required_plugins') throw new Error('Plugin lookup failed')
      return handler(command)
    })
    const wrapper = render(); await flushPromises()
    await wrapper.findAll('button').find(b => b.text() === 'courses.detail.enroll')!.trigger('click')
    await flushPromises()
    expect(wrapper.get('[role=alert]').text()).toContain('Plugin lookup failed')
    expect(mocks.invoke.mock.calls.some(c => c[0] === 'enroll')).toBe(false)
  })
})
