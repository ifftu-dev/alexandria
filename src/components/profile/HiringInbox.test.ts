import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import HiringInbox from './HiringInbox.vue'
import type { DirectoryInterview, DirectoryOffer, HiringRecord } from '@/types'

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))

const invite: DirectoryInterview = {
  directory_url: 'http://127.0.0.1:8787',
  invite: {
    id: 'int-1', audience: 'urn:alexandria:organization:org', nonce: 'n', organization: 'Alexandria Demo',
    subject_did: 'did:key:z6MkMe', role_label: 'Junior engineer', message: 'Let us talk.', mode: 'video',
    proposed_slots: [1_900_000_000, 1_900_100_000], meeting_url: 'https://meet.example/room', run_id: 'run-9',
    created_at: 1, expires_at: 1_950_000_000,
  },
}
const offer: DirectoryOffer = {
  directory_url: 'http://127.0.0.1:8787',
  offer: {
    id: 'off-1', audience: 'a', nonce: 'n', organization: 'Alexandria Demo', subject_did: 'did:key:z6MkMe', interview_id: 'int-1',
    role_label: 'Junior engineer', terms: 'Full time, Bengaluru.', start_date: '2026-11-09', created_at: 1, expires_at: 1_950_000_000,
  },
}
const record: HiringRecord = {
  id: 'int-0', kind: 'interview', directory_url: 'http://127.0.0.1:8787', organization: 'Earlier Org', role_label: 'Intern',
  decision: 'accept', chosen_slot: 1_890_000_000, meeting_url: 'https://meet.example/earlier', responded_at: '2026-10-01T10:00:00Z', payload_json: '{}',
}

function arm(interviews: DirectoryInterview[], offers: DirectoryOffer[], history: HiringRecord[]) {
  mocks.invoke.mockImplementation(async (command: string) => {
    if (command === 'hiring_interviews') return { items: interviews, problems: [] }
    if (command === 'hiring_offers') return { items: offers, problems: [{ directory: 'Local demo', detail: 'offline' }] }
    if (command === 'hiring_history') return history
    if (command === 'hiring_interview_respond' || command === 'hiring_offer_respond') return { received: true, status: 'accepted' }
    throw new Error(command)
  })
}

beforeEach(() => { mocks.invoke.mockReset() })

describe('HiringInbox', () => {
  it('lists invitations, offers, directory problems and the learner’s own answers', async () => {
    arm([invite], [offer], [record])
    const wrapper = mount(HiringInbox); await flushPromises()
    expect(wrapper.findAll('[data-testid=interview]')).toHaveLength(1)
    expect(wrapper.findAll('[data-testid=offer]')).toHaveLength(1)
    expect(wrapper.text()).toContain('Let us talk.')
    expect(wrapper.text()).toContain('Full time, Bengaluru.')
    expect(wrapper.find('[role=alert]').text()).toBe('Local demo: offline')
    expect(wrapper.find('[data-testid=history]').text()).toContain('Earlier Org')
    expect(wrapper.find('[data-testid=interview] a').attributes('href')).toBe('https://meet.example/room')
  })

  it('accepts only with a chosen time and sends the exact invitation back signed', async () => {
    arm([invite], [], [])
    const wrapper = mount(HiringInbox); await flushPromises()
    const card = wrapper.find('[data-testid=interview]')
    const accept = card.findAll('button').find(b => b.text() === 'profile.hiring.accept')!
    expect(accept.attributes('disabled')).toBeDefined()
    await card.find('select').setValue(1_900_100_000)
    await card.find('input').setValue('See you then')
    expect(accept.attributes('disabled')).toBeUndefined()
    arm([], [], [{ ...record, id: 'int-1' }])
    await accept.trigger('click'); await flushPromises()
    expect(mocks.invoke).toHaveBeenCalledWith('hiring_interview_respond', {
      directoryUrl: 'http://127.0.0.1:8787', invite: invite.invite, decision: 'accept', chosenSlot: 1_900_100_000, note: 'See you then',
    })
    expect(wrapper.text()).toContain('profile.hiring.empty')
  })

  it('declines without a time and answers offers with a signed decision', async () => {
    arm([invite], [offer], [])
    const wrapper = mount(HiringInbox); await flushPromises()
    const decline = wrapper.find('[data-testid=interview]').findAll('button').find(b => b.text() === 'profile.hiring.decline')!
    await decline.trigger('click'); await flushPromises()
    expect(mocks.invoke).toHaveBeenCalledWith('hiring_interview_respond', expect.objectContaining({ decision: 'decline', chosenSlot: null }))
    const acceptOffer = wrapper.find('[data-testid=offer]').findAll('button').find(b => b.text() === 'profile.hiring.acceptOffer')!
    await acceptOffer.trigger('click'); await flushPromises()
    expect(mocks.invoke).toHaveBeenCalledWith('hiring_offer_respond', {
      directoryUrl: 'http://127.0.0.1:8787', offer: offer.offer, decision: 'accept', note: null,
    })
  })

  it('shows a refusal from the directory and keeps the invitation open', async () => {
    arm([invite], [], [])
    mocks.invoke.mockImplementationOnce(async () => ({ items: [invite], problems: [] }))
    const wrapper = mount(HiringInbox); await flushPromises()
    const card = wrapper.find('[data-testid=interview]')
    await card.find('select').setValue(1_900_000_000)
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === 'hiring_interview_respond') throw new Error('directory returned 400: this invitation is expired')
      if (command === 'hiring_interviews') return { items: [invite], problems: [] }
      if (command === 'hiring_offers') return { items: [], problems: [] }
      return []
    })
    await card.findAll('button').find(b => b.text() === 'profile.hiring.accept')!.trigger('click'); await flushPromises()
    expect(wrapper.find('[role=alert]').text()).toContain('this invitation is expired')
    expect(wrapper.find('[data-testid=interview] select').exists()).toBe(true)
  })
})
