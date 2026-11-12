import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import InterviewSignalFeed from './InterviewSignalFeed.vue'

// On a panel seat (an interview held in Alexandria) the panel can mark an
// unscripted live segment done; anywhere else the control is absent.
const t = (key: string, values?: Record<string, unknown>) => (values ? `${key} ${JSON.stringify(values)}` : key)

describe('InterviewSignalFeed live segments', () => {
  it('offers the live-segment mark on a panel seat and says when it was sent', async () => {
    const wrapper = mount(InterviewSignalFeed, {
      props: { entries: [], live: true, liveMarkedAt: null },
      global: { mocks: { $t: t } },
    })
    await wrapper.get('[data-testid="mark-live-segment"]').trigger('click')
    expect(wrapper.emitted('live')).toHaveLength(1)
    await wrapper.setProps({ liveMarkedAt: Date.UTC(2026, 10, 12, 10, 0, 0) })
    expect(wrapper.text()).toContain('sentinel.trust.live.marked')
  })

  it('has no live-segment mark outside a panel seat', () => {
    const wrapper = mount(InterviewSignalFeed, {
      props: { entries: [] },
      global: { mocks: { $t: t } },
    })
    expect(wrapper.find('[data-testid="mark-live-segment"]').exists()).toBe(false)
  })
})
