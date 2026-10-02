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
  it('links examples to their reading pages and filters by topic', async () => {
    const wrapper = mount(OpinionExamples, { props: { examples, subjectFieldId: '' }, global: { mocks: { $t: (key: string) => key }, stubs: { RouterLink: { props: ['to'], template: '<a :href="to"><slot /></a>' } } } })
    expect(wrapper.text()).toContain('opinions.examples.description')
    expect(wrapper.find('[data-video]').exists()).toBe(false)
    expect(wrapper.findAll('a').map(a => a.attributes('href'))).toEqual(['/opinions/examples/one', '/opinions/examples/two'])
    await wrapper.setProps({ subjectFieldId: 'design' })
    expect(wrapper.text()).not.toContain('First viewpoint')
    expect(wrapper.get('a').attributes('href')).toBe('/opinions/examples/two')
    wrapper.unmount()
  })
})
