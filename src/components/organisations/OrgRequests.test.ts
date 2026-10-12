import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import OrgRequests from './OrgRequests.vue'
import type {
  AccessRequestItem,
  HiringRecord,
  OrgCheck,
  OrgCredentialRequest,
  OrgInterview,
  OrgOffer,
  OrgProof,
} from '@/types'

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), push: vi.fn() }))
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key, locale: { value: 'en' } }) }))
vi.mock('vue-router', () => ({ useRouter: () => ({ push: mocks.push }) }))

const CLOUD = 'http://127.0.0.1:8787'
const proof: OrgProof = { domain: 'acme.example', verificationMethod: 'did:key:z6MkOrg', created: '2026-10-12T00:00:00Z', jws: 'h..s' }
const verified: OrgCheck = { domain: 'acme.example', name: 'Acme Hiring', verified: true, problem: null }
const unverified: OrgCheck = { domain: 'acme.example', name: null, verified: false, problem: 'acme.example does not publish an organisation file (404)' }
const future = 1_950_000_000

function accessItem(id: string, org: OrgCheck = verified, created = 300): AccessRequestItem {
  return {
    directoryUrl: CLOUD,
    request: {
      id, audience: 'urn:alexandria:org:acme.example', nonce: 'n', organization: 'Totally Real Bank',
      subject_did: 'did:key:z6MkMe', skill_ids: ['skill_big_o', 'skill_graphs'], purpose: 'Backend engineer shortlist',
      network_id: 'preprod', created_at: created, expires_at: future,
    },
    orgProof: proof,
    org,
    skills: [
      { skillId: 'skill_big_o', name: 'Big-O Analysis', hasEvidence: true, bloomLevel: 3, level: 4 },
      { skillId: 'skill_graphs', name: 'Graphs', hasEvidence: false, bloomLevel: null, level: null },
    ],
  }
}

function assessmentItem(org: OrgCheck = verified, created = 200): OrgCredentialRequest {
  return {
    directory_url: CLOUD,
    request: {
      id: 'cr-1', audience: 'a', nonce: 'n', organization: 'Acme', subject_did: 'did:key:z6MkMe', skill_id: 'skill_big_o',
      network_id: 'preprod', taxonomy_digest: 'd', purpose: 'Screening', role_label: 'Backend engineer',
      require_new_assessment: true, created_at: created, expires_at: future,
    },
    orgProof: proof,
    org,
  }
}

function interviewItem(org: OrgCheck = verified, mode: OrgInterview['invite']['mode'] = 'video', created = 100): OrgInterview {
  return {
    directory_url: CLOUD,
    invite: {
      id: 'int-1', audience: 'a', nonce: 'n', organization: 'Acme', subject_did: 'did:key:z6MkMe', role_label: 'Junior engineer',
      message: 'Let us talk.', mode, proposed_slots: [1_900_000_000, 1_900_100_000],
      meeting_url: mode === 'alexandria' ? null : 'https://meet.example/room', run_id: null, created_at: created, expires_at: future,
    },
    orgProof: proof,
    org,
  }
}

function offerItem(org: OrgCheck = verified): OrgOffer {
  return {
    directory_url: CLOUD,
    offer: {
      id: 'off-1', audience: 'a', nonce: 'n', organization: 'Acme', subject_did: 'did:key:z6MkMe', interview_id: 'int-1',
      role_label: 'Junior engineer', terms: 'Full time, Bengaluru.', start_date: '2026-11-09', created_at: 50, expires_at: future,
    },
    orgProof: proof,
    org,
  }
}

function hostedInterview(id: string, chosenSlot: number): HiringRecord {
  return {
    id, kind: 'interview', directory_url: CLOUD, organization: 'Acme Hiring', role_label: 'Junior engineer', decision: 'accept',
    chosen_slot: chosenSlot, meeting_url: null, responded_at: '2026-10-01T10:00:00Z',
    payload_json: JSON.stringify({ response: { invite: { mode: 'alexandria' } } }),
  }
}

interface Inbox {
  access?: AccessRequestItem[]
  assessments?: OrgCredentialRequest[]
  interviews?: OrgInterview[]
  offers?: OrgOffer[]
  history?: HiringRecord[]
  problems?: { directory: string; detail: string }[]
}

function arm(inbox: Inbox) {
  const problems = inbox.problems ?? []
  mocks.invoke.mockImplementation(async (command: string) => {
    switch (command) {
      case 'org_access_requests': return { items: inbox.access ?? [], problems }
      case 'exchange_requests': return { items: inbox.assessments ?? [], problems }
      case 'hiring_interviews': return { items: inbox.interviews ?? [], problems }
      case 'hiring_offers': return { items: inbox.offers ?? [], problems }
      case 'hiring_history': return inbox.history ?? []
      case 'get_skill': return { skill: { name: 'Big-O Analysis' } }
      case 'org_grant': return { grantId: 'g-1', orgName: 'Acme Hiring', orgDomain: 'acme.example' }
      case 'exchange_credentials': return [{ id: 'cred-1', issuer: 'did:key:z6MkIssuer', issued_at: '2026-10-01T00:00:00Z' }]
      case 'exchange_preview': return { type: ['VerifiablePresentation'] }
      case 'hiring_interview_respond':
      case 'hiring_offer_respond': return { received: true, status: 'accepted' }
      default: throw new Error(command)
    }
  })
}

const button = (wrapper: ReturnType<typeof mount>, text: string) =>
  wrapper.findAll('button').find((b) => b.text() === text)!

beforeEach(() => {
  mocks.invoke.mockReset()
  mocks.push.mockReset()
})
afterEach(() => { vi.useRealTimers() })

describe('OrgRequests', () => {
  it('merges every kind of request into one inbox, newest first', async () => {
    arm({ access: [accessItem('ar-1')], assessments: [assessmentItem()], interviews: [interviewItem()], offers: [offerItem()] })
    const wrapper = mount(OrgRequests); await flushPromises()
    const kinds = wrapper.findAll('article').map((a) => a.attributes('data-testid'))
    expect(kinds).toEqual(['request-access', 'request-assessment', 'request-interview', 'request-offer'])
    expect(wrapper.text()).toContain('Acme Hiring')
    expect(wrapper.text()).toContain('organisations.badge.verified')
  })

  it('pre-ticks skills with evidence and shares exactly those, once or kept in sync', async () => {
    const item = accessItem('ar-2')
    arm({ access: [item] })
    const wrapper = mount(OrgRequests); await flushPromises()
    const boxes = wrapper.findAll('[data-testid=request-access] input[type=checkbox]')
    expect((boxes[0]!.element as HTMLInputElement).checked).toBe(true)
    expect(boxes[1]!.attributes('disabled')).toBeDefined()
    expect(wrapper.text()).toContain('organisations.requests.access.noEvidence')

    await button(wrapper, 'organisations.requests.access.share').trigger('click'); await flushPromises()
    expect(mocks.invoke).toHaveBeenCalledWith('org_grant', {
      directoryUrl: CLOUD, request: item.request, orgProof: proof, skillIds: ['skill_big_o'], sync: false,
    })
    expect(wrapper.emitted('shared')).toHaveLength(1)
    // Both panels refresh: this one asks again.
    expect(mocks.invoke.mock.calls.filter((c) => c[0] === 'org_access_requests')).toHaveLength(2)

    await button(wrapper, 'organisations.requests.access.shareSync').trigger('click'); await flushPromises()
    expect(mocks.invoke).toHaveBeenCalledWith('org_grant', expect.objectContaining({ sync: true, orgProof: proof }))
  })

  it('cannot share once nothing is ticked', async () => {
    arm({ access: [accessItem('ar-3')] })
    const wrapper = mount(OrgRequests); await flushPromises()
    await wrapper.find('[data-testid=request-access] input[type=checkbox]').trigger('change')
    expect(button(wrapper, 'organisations.requests.access.share').attributes('disabled')).toBeDefined()
  })

  it('disables every answer to an unverified sender and never shows its claimed name', async () => {
    arm({
      access: [accessItem('ar-4', unverified)], assessments: [assessmentItem(unverified)],
      interviews: [interviewItem(unverified)], offers: [offerItem(unverified)],
    })
    const wrapper = mount(OrgRequests); await flushPromises()
    expect(wrapper.findAll('[data-testid=org-badge-unverified]')).toHaveLength(4)
    expect(wrapper.text()).toContain('organisations.badge.unverified')
    expect(wrapper.text()).not.toContain('Totally Real Bank')
    for (const label of [
      'organisations.requests.access.share', 'organisations.requests.access.shareSync',
      'organisations.requests.assessment.assess', 'organisations.requests.assessment.choose',
      'organisations.requests.interview.accept', 'organisations.requests.interview.decline',
      'organisations.requests.offer.accept', 'organisations.requests.offer.decline',
    ]) {
      expect(button(wrapper, label).attributes('disabled'), label).toBeDefined()
    }
    // Hiding locally is not an answer, so it stays available.
    expect(button(wrapper, 'organisations.requests.access.notNow').attributes('disabled')).toBeUndefined()
  })

  it('previews a credential share with the organisation proof and sends it to the same Cloud', async () => {
    const item = assessmentItem()
    arm({ assessments: [item] })
    const wrapper = mount(OrgRequests); await flushPromises()
    expect(wrapper.text()).toContain('organisations.requests.assessment.skill')
    expect(mocks.invoke).toHaveBeenCalledWith('get_skill', { skillId: 'skill_big_o' })

    await button(wrapper, 'organisations.requests.assessment.choose').trigger('click'); await flushPromises()
    await wrapper.find('[data-testid=credential-picker] select').setValue('cred-1')
    await button(wrapper, 'organisations.requests.assessment.preview').trigger('click'); await flushPromises()
    expect(mocks.invoke).toHaveBeenCalledWith('exchange_preview', { request: item.request, orgProof: proof, credentialId: 'cred-1' })
    expect(wrapper.text()).toContain('organisations.requests.assessment.disclosure')
  })

  it('opens the requested assessment with the request and Cloud in the query', async () => {
    arm({ assessments: [assessmentItem()] })
    const wrapper = mount(OrgRequests); await flushPromises()
    await button(wrapper, 'organisations.requests.assessment.assess').trigger('click')
    expect(mocks.push).toHaveBeenCalledWith({ path: '/assessment/skill_big_o', query: { request: 'cr-1', directory: CLOUD } })
  })

  it('answers interviews and offers with the organisation proof', async () => {
    const interview = interviewItem()
    const offer = offerItem()
    arm({ interviews: [interview], offers: [offer] })
    const wrapper = mount(OrgRequests); await flushPromises()
    const card = wrapper.find('[data-testid=request-interview]')
    expect(button(wrapper, 'organisations.requests.interview.accept').attributes('disabled')).toBeDefined()
    await card.find('select').setValue(1_900_100_000)
    await card.find('input').setValue('See you then')
    await button(wrapper, 'organisations.requests.interview.accept').trigger('click'); await flushPromises()
    expect(mocks.invoke).toHaveBeenCalledWith('hiring_interview_respond', {
      directoryUrl: CLOUD, invite: interview.invite, orgProof: proof, decision: 'accept', chosenSlot: 1_900_100_000, note: 'See you then',
    })
    await button(wrapper, 'organisations.requests.offer.accept').trigger('click'); await flushPromises()
    expect(mocks.invoke).toHaveBeenCalledWith('hiring_offer_respond', {
      directoryUrl: CLOUD, offer: offer.offer, orgProof: proof, decision: 'accept', note: null,
    })
  })

  it('keeps a refused answer on its card', async () => {
    arm({ interviews: [interviewItem()] })
    const wrapper = mount(OrgRequests); await flushPromises()
    const respond = mocks.invoke.getMockImplementation()!
    mocks.invoke.mockImplementation(async (command: string, args?: unknown) => {
      if (command === 'hiring_interview_respond') throw new Error('this organisation could not be verified')
      return respond(command, args)
    })
    await button(wrapper, 'organisations.requests.interview.decline').trigger('click'); await flushPromises()
    expect(wrapper.find('[data-testid=request-interview] [role=alert]').text()).toContain('could not be verified')
  })

  it('says an interview happens in Alexandria instead of linking out', async () => {
    arm({ interviews: [interviewItem(verified, 'alexandria')] })
    const wrapper = mount(OrgRequests); await flushPromises()
    const card = wrapper.find('[data-testid=request-interview]')
    expect(card.text()).toContain('organisations.requests.interview.inAlexandria')
    expect(card.find('a').exists()).toBe(false)
  })

  it('lists accepted Alexandria interviews and opens the room from 15 minutes before', async () => {
    vi.useFakeTimers({ toFake: ['Date'] })
    vi.setSystemTime(new Date(1_900_000_000 * 1000))
    const soon = hostedInterview('int-soon', 1_900_000_000 + 10 * 60)
    const later = hostedInterview('int-later', 1_900_000_000 + 2 * 86_400)
    const elsewhere: HiringRecord = { ...hostedInterview('int-video', 1_900_000_000 + 600), payload_json: JSON.stringify({ response: { invite: { mode: 'video' } } }) }
    const past = hostedInterview('int-past', 1_800_000_000)
    arm({ history: [later, soon, elsewhere, past] })
    const wrapper = mount(OrgRequests); await flushPromises()
    const rows = wrapper.findAll('[data-testid=upcoming-interview]')
    expect(rows).toHaveLength(2)
    expect(rows[1]!.text()).toContain('organisations.requests.upcoming.opensSoon')
    await rows[0]!.find('button').trigger('click')
    expect(mocks.push).toHaveBeenCalledWith({ path: '/interview-room/int-soon', query: { cloud: CLOUD } })
  })

  it('shows each problem once, even when every call reports it', async () => {
    arm({ problems: [{ directory: 'Alexandria Cloud', detail: 'offline' }] })
    const wrapper = mount(OrgRequests); await flushPromises()
    expect(wrapper.findAll('[data-testid=request-problems] [role=alert]').map((p) => p.text())).toEqual(['offline'])
    expect(wrapper.text()).toContain('organisations.requests.empty')
  })

  it('hides a request with "Not now" without telling anyone, for the rest of the session', async () => {
    arm({ access: [accessItem('ar-not-now')] })
    const wrapper = mount(OrgRequests); await flushPromises()
    await button(wrapper, 'organisations.requests.access.notNow').trigger('click')
    expect(wrapper.find('[data-testid=request-access]').exists()).toBe(false)
    expect(mocks.invoke.mock.calls.map((c) => c[0])).not.toContain('org_grant')
    wrapper.unmount()
    const again = mount(OrgRequests); await flushPromises()
    expect(again.find('[data-testid=request-access]').exists()).toBe(false)
  })
})
