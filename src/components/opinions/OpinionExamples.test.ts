import { describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import OpinionExamples from './OpinionExamples.vue'
import type { OpinionExample } from '@/types'

vi.mock('./ThreadThumbnail.vue', () => ({ default: { template: '<div />' } }))

vi.mock('@/components/course/VideoPlayer.vue', () => ({ default: {
  props: ['contentCid', 'title'], template: '<div data-video>{{ contentCid }}</div>',
} }))

const examples: OpinionExample[] = [
  { id: 'one', subject_field_id: 'cs', title: 'First viewpoint', summary: 'A discussion prompt', thumbnail_cid: null, video_cid: 'local-one', duration_seconds: 30 },
  { id: 'two', subject_field_id: 'design', title: 'Second viewpoint', summary: 'Another prompt', thumbnail_cid: null, video_cid: 'local-two', duration_seconds: 45 },
]

describe('bundled opinion examples', () => {
  it('opens the selected local video and unmounts it when the topic changes', async () => {
    const wrapper = mount(OpinionExamples, { props: { examples, subjectFieldId: '' }, global: { mocks: { $t: (key: string) => key } } })
    expect(wrapper.text()).toContain('opinions.examples.description')
    expect(wrapper.find('[data-video]').exists()).toBe(false)
    await wrapper.findAll('button')[0]!.trigger('click')
    expect(wrapper.get('[data-video]').text()).toBe('local-one')
    await wrapper.setProps({ subjectFieldId: 'design' })
    expect(wrapper.find('[data-video]').exists()).toBe(false)
    expect(wrapper.text()).not.toContain('First viewpoint')
    await wrapper.findAll('button')[0]!.trigger('click')
    expect(wrapper.get('[data-video]').text()).toBe('local-two')
    await wrapper.findAll('button').find(b => b.text() === 'common.actions.close')!.trigger('click')
    expect(wrapper.find('[data-video]').exists()).toBe(false)
    wrapper.unmount()
  })
})
