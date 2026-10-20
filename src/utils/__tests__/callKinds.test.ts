import { describe, expect, it } from 'vitest'
import { callHome, callScreen, sharesInvites } from '../callKinds'

describe('call kinds', () => {
  it('offers invites only for open tutoring', () => {
    expect(sharesInvites('tutoring')).toBe(true)
    expect(sharesInvites(undefined)).toBe(true)
    for (const kind of ['classroom', 'interview_panel', 'interview_candidate', 'interview_assistant', 'plugin'] as const) {
      expect(sharesInvites(kind)).toBe(false)
    }
  })

  it('opens each kind on its own screen', () => {
    expect(callScreen({ id: 's1', kind: 'tutoring' })).toBe('/tutoring/s1')
    expect(callScreen({ id: 's1', kind: 'classroom', context_id: 'c1' })).toBe('/classrooms/c1')
    expect(callScreen({ id: 's1', kind: 'interview_panel', context_id: 'i1' })).toBe('/panel/i1')
    expect(callScreen({ id: 's1', kind: 'interview_assistant', context_id: 'i1' })).toBe('/interviews/i1')
    expect(callScreen({ id: 's1', kind: 'interview_candidate', context_id: 'i1' })).toBe('/interview-call/s1')
  })

  it('never sends an interview or classroom back to Live Tutoring', () => {
    expect(callHome({ id: 's1', kind: 'tutoring' })).toBe('/tutoring')
    expect(callHome({ id: 's1', kind: 'classroom', context_id: 'c1' })).toBe('/classrooms/c1')
    expect(callHome({ id: 's1', kind: 'interview_panel', context_id: 'i1' })).toBe('/panel/i1')
    expect(callHome({ id: 's1', kind: 'interview_candidate', context_id: 'i1' }, 'https://cloud.example'))
      .toBe('/interview-room/i1?cloud=https%3A%2F%2Fcloud.example')
    // Without its Cloud address the waiting room cannot load; fall back to
    // the organisations list, where the interview is listed.
    expect(callHome({ id: 's1', kind: 'interview_candidate', context_id: 'i1' })).toBe('/settings/organisations')
  })
})
