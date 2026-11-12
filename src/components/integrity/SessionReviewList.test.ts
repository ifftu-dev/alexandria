import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import SessionReviewList from './SessionReviewList.vue'
import type { RoleSessionReview } from '@/types'

// The sponsor's review queue: sessions in the backend's order (most
// suspicious first), each signal with its certified rates or marked advisory.
function review(id: string, posterior: number, signals: RoleSessionReview['review']['signals']): RoleSessionReview {
  return {
    attempt_id: `att-${id}`, subject_did: `did:key:z6Mk${id}`, integrity_session_id: id, score: 0.8, passed: true,
    review: {
      session_id: id, status: 'completed', trust_profile: 'core', rung: 'process',
      posterior_llr: posterior, threshold: 2, manifest_version: 'cert-1', signals,
    },
  }
}

const t = (key: string, values?: Record<string, unknown>) => (values ? `${key} ${JSON.stringify(values)}` : key)

describe('SessionReviewList', () => {
  it('keeps the ranked order and marks advisory signals apart from certified ones', async () => {
    const wrapper = mount(SessionReviewList, {
      props: {
        reviews: [
          review('high', 3, [{ signal: 'response_idle_burst', channel: 'response_process', windows: 2, advisory: false, certified: { llr: 1.5, precision: 0.8, recall: 0.6 } }]),
          review('low', 0, [{ signal: 'gaze_down_then_response', channel: 'gaze', windows: 1, advisory: true, certified: null }]),
        ],
      },
      global: { mocks: { $t: t } },
    })
    const rows = wrapper.findAll('[data-testid="session-review"]')
    expect(rows.map(r => r.attributes('data-session'))).toEqual(['high', 'low'])
    expect(rows[0]!.text()).toContain('dashboard.sponsor.review.weight {"value":"3.00"}')
    expect(rows[0]!.text()).toContain('dashboard.sponsor.review.rates {"precision":"0.80","recall":"0.60"}')
    expect(rows[0]!.text()).not.toContain('dashboard.sponsor.review.advisory')
    expect(rows[1]!.text()).toContain('dashboard.sponsor.review.advisory')
    expect(rows[1]!.text()).toContain('sentinel.trust.rung.process')

    await rows[1]!.get('button').trigger('click')
    expect(wrapper.emitted('use')?.[0]?.[0]).toMatchObject({ integrity_session_id: 'low' })
  })
})
