import { describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import Inbox from './Inbox.vue'

const mocks = vi.hoisted(() => ({ invoke: vi.fn().mockResolvedValue([]) }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))
vi.mock('vue-router', () => ({ useRouter: () => ({ push: vi.fn() }) }))
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))

describe('portable instructor endorsement requests', () => {
  it('passes the exact supplied document to backend verification and resets review after editing', async () => {
    const wrapper = mount(Inbox, { global: { mocks: { $t: (key: string) => key } } })
    await flushPromises()
    const binding = {
      format_version: 1, network_id: 'demo', subject_did: 'did:key:learner',
      course_id: 'course', course_document_cid: 'ab'.repeat(32), course_document_version: 2,
      completion_root: 'cd'.repeat(32), evidence: [], course_document_json: '{"signed":"public document"}',
    }
    await wrapper.get('textarea').setValue(JSON.stringify(binding))
    await wrapper.findAll('button').find(button => button.text() === 'instructor.inbox.endorsementReview')!.trigger('click')
    const sign = wrapper.findAll('button').find(button => button.text() === 'instructor.inbox.endorsementSign')!
    expect(sign).toBeDefined()
    await sign.trigger('click')
    await flushPromises()
    const { course_document_json, ...signedBinding } = binding
    expect(mocks.invoke).toHaveBeenCalledWith('sign_course_completion_endorsement', {
      binding: signedBinding, courseDocumentJson: course_document_json,
    })
    await wrapper.get('textarea').setValue('{}')
    expect(wrapper.text()).not.toContain('instructor.inbox.endorsementSign')
    wrapper.unmount()
  })
})
