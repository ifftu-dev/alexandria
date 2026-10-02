import { describe, it, expect, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import ThreadComposer from './ThreadComposer.vue'
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: vi.fn() }) }))
describe('thread publication disclosure', () => {
  it('requires content and affirmative disclosure before submitting', async () => {
    const wrapper = mount(ThreadComposer, { global: { mocks: { $t: (key: string) => key } } })
    await wrapper.get('input[maxlength="300"]').setValue('Arrays or objects first?')
    await wrapper.get('textarea').setValue('Start with the operations the learner needs.')
    expect(wrapper.get('button[type="submit"]').attributes('disabled')).toBeDefined()
    await wrapper.get('input[type="checkbox"]').setValue(true)
    expect(wrapper.get('button[type="submit"]').attributes('disabled')).toBeUndefined()
    await wrapper.get('form').trigger('submit')
    expect(wrapper.emitted('submit')?.[0]?.[0]).toEqual({ title: 'Arrays or objects first?', body: 'Start with the operations the learner needs.', post_kind: 'text', url: null, video_cid: null, thumbnail_cid: null })
    wrapper.unmount()
  })
})
