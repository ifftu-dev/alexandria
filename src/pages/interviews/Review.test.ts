import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import Review from './Review.vue'

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), push: vi.fn() }))
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))
vi.mock('vue-router', () => ({
  useRoute: () => ({ params: { id: 'local-1' }, path: '/panel/local-1/review' }),
  useRouter: () => ({ push: mocks.push }),
}))

const bundle = {
  session: {
    id: 'local-1', title: 'Engineering Manager · Demo Learner', objective: 'IFFTU interview', role_assessment_id: null,
    tutoring_session_id: null, status: 'completed', duration_minutes: 45, retention_days: 30, record_audio: false,
    record_video: false, sentinel_enabled: false, integrity_session_id: null, summary: 'Draft', conclusion: null,
    created_at: '2026-10-12T00:00:00Z', started_at: null, ended_at: null, expires_at: '2026-11-11T00:00:00Z',
  },
  participants: [],
  criteria: [
    { id: 'c1', session_id: 'local-1', label: 'Communication', position: 0, status: 'covered', notes: null },
    { id: 'c2', session_id: 'local-1', label: 'Technical depth', position: 1, status: 'partial', notes: null },
  ],
  transcript: [], notes: [], followups: [],
}
const panel = {
  interviewId: 'int-1', organization: 'IFFTU', orgDomain: 'alexandria.ifftu.dev', roleLabel: 'Engineering Manager',
  status: 'accepted', chosenSlot: null, candidate: { did: 'did:key:z6Mk', displayName: 'Demo Learner' },
  criteria: ['Communication', 'Technical depth'], me: { id: 'p1', name: 'Asha' }, panel: [{ id: 'p1', name: 'Asha' }],
  room: { ticket: null, openedAt: null }, candidateEvidence: [],
}

function arm(isPanel: boolean) {
  mocks.invoke.mockImplementation(async (command: string) => {
    if (command === 'interview_get') return bundle
    if (command === 'interview_panel_status') {
      if (isPanel) return panel
      throw new Error('this interview is not linked to an organisation\'s panel')
    }
    if (command === 'interview_panel_scorecard') return { ok: true }
    throw new Error(command)
  })
}

const mountReview = () => mount(Review, { global: { mocks: { $t: (key: string) => key } } })

beforeEach(() => { mocks.invoke.mockReset(); mocks.push.mockReset() })

describe('Review scorecard', () => {
  it('sends ratings, the chosen recommendation and notes for a panel seat', async () => {
    arm(true)
    const wrapper = mountReview(); await flushPromises()
    const card = wrapper.get('[data-testid=scorecard]')
    const send = card.findAll('button').at(-1)!
    expect(send.attributes('disabled')).toBeDefined()

    await card.findAll('select')[0]!.setValue('4')
    await card.find('input[value=yes]').setValue(true)
    await card.find('textarea').setValue('Clear and specific.')
    await send.trigger('click'); await flushPromises()

    expect(mocks.invoke).toHaveBeenCalledWith('interview_panel_scorecard', {
      sessionId: 'local-1',
      ratings: [
        { criterion: 'Communication', rating: 4 },
        { criterion: 'Technical depth', rating: null },
      ],
      recommendation: 'yes',
      notes: 'Clear and specific.',
    })
    expect(card.text()).toContain('interviews.panel.scorecardSent')
  })

  it('shows no scorecard for a local interview', async () => {
    arm(false)
    const wrapper = mountReview(); await flushPromises()
    expect(wrapper.find('[data-testid=scorecard]').exists()).toBe(false)
  })
})
