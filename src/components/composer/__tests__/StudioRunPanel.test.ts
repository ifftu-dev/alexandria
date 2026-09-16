import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import StudioRunPanel from '../StudioRunPanel.vue'
import type { StudioDocument, StudioRun } from '@/types'
import en from '@/locales/en'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke }) }))
const mounted: ReturnType<typeof mount>[] = []
function fixture(status = 'prepared'): StudioDocument<StudioRun> {
  return { id: 'run', revision: 1, value: { id: 'run', course_id: 'course', element_id: 'lesson', workflow_name: 'Draft and review', workflow_revision: 1, target_fingerprint: 'original', original_content: 'Original', context: 'Selected reference only', status, steps: [
    { role: 'draft', connection: { id: 'local', name: 'Local writer', endpoint: 'http://localhost/v1', model: 'writer', location: 'local', capability: 'text', enabled: true, has_key: false }, effective_prompt: 'Instructor custom prompt', output: status === 'prepared' ? null : 'Generated draft' },
    { role: 'review', connection: { id: 'local', name: 'Local reviewer', endpoint: 'http://localhost/v1', model: 'reviewer', location: 'local', capability: 'text', enabled: true, has_key: false }, effective_prompt: 'Review carefully', output: status === 'prepared' ? null : 'Review comments' },
  ], error: null, created_at: '2026-09-15T12:00:00Z', applied_fingerprint: null } }
}
function render(draftDirty = false) {
  const wrapper = mount(StudioRunPanel, { props: { runId: 'run', draftDirty }, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })] } })
  mounted.push(wrapper)
  return wrapper
}
beforeEach(() => { invoke.mockReset() })
afterEach(() => { for (const wrapper of mounted.splice(0)) wrapper.unmount() })
describe('instructor approval boundary', () => {
  it('shows frozen instructions and selected context without automatically starting', async () => {
    invoke.mockResolvedValue(fixture())
    const wrapper = render(); await flushPromises()
    expect(wrapper.text()).toContain('Instructor custom prompt')
    expect(wrapper.text()).toContain('Selected reference only')
    expect(invoke.mock.calls.map(call => call[0])).toEqual(['studio_get_run'])
    const start = wrapper.findAll('button').find(button => button.text().includes('Start'))!
    invoke.mockResolvedValue(fixture('running'))
    await start.trigger('click'); await flushPromises()
    expect(invoke).toHaveBeenCalledWith('studio_start_run', expect.objectContaining({ runId: 'run', revision: 1 }))
  })
  it('requires a saved lesson and applies the instructor-edited draft, not review comments', async () => {
    invoke.mockResolvedValue(fixture('review'))
    const wrapper = render(true); await flushPromises()
    expect(wrapper.get('textarea').element.value).toBe('Generated draft')
    const approve = wrapper.findAll('button').find(button => button.text().includes('Approve'))!
    expect(approve.attributes('disabled')).toBeDefined()
    await wrapper.setProps({ draftDirty: false })
    await wrapper.get('textarea').setValue('Instructor correction')
    invoke.mockResolvedValue({ ...fixture('applied'), revision: 2 })
    await approve.trigger('click'); await flushPromises()
    expect(invoke).toHaveBeenCalledWith('studio_apply_run', { runId: 'run', revision: 1, text: 'Instructor correction' })
    expect(wrapper.emitted('applied')).toHaveLength(1)
  })
  it('keeps the proposal editable when a stale revision conflicts', async () => {
    invoke.mockResolvedValue(fixture('review'))
    const wrapper = render(); await flushPromises()
    await wrapper.get('textarea').setValue('Keep my correction')
    invoke.mockRejectedValue('conflict: lesson changed')
    await wrapper.findAll('button').find(button => button.text().includes('Approve'))!.trigger('click')
    await flushPromises()
    expect(wrapper.get('[role="alert"]').text()).toContain('lesson changed')
    expect(wrapper.get('textarea').element.value).toBe('Keep my correction')
    expect(wrapper.emitted('applied')).toBeUndefined()
  })
})
