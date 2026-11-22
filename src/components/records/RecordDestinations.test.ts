import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import RecordDestinations from './RecordDestinations.vue'
import type { DestinationPreview, RecordAccessEntry, RecordDestination } from '@/types'

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key, locale: { value: 'en' } }) }))

const STORE = 'https://lrs.example.org/xapi/'

const preview: DestinationPreview = {
  endpoint: STORE,
  location: 'remote',
  needs_confirmation: false,
  pairwise_actor: 'z6MkMadeForThisStore',
  real_did_actor: 'z6MkLearner',
  courses: [{ id: 'c-safety', title: 'Warehouse safety' }],
}

function destination(over: Partial<RecordDestination> = {}): RecordDestination {
  return {
    id: 'd-1', endpoint: STORE, location: 'remote', username: 'learner-key', identifier: 'pairwise',
    actor_name: 'z6MkMadeForThisStore', scope: { course_ids: ['c-safety'], include_interactions: false },
    xapi_version: '2.0.0', created_at: '2026-11-20T09:00:00.000Z',
    status: { queued: 0, failed: 0, sent: 4, last_error: null, next_attempt_at: null, last_sent_at: '2026-11-20T09:01:00.000Z' },
    ...over,
  }
}

const sent: RecordAccessEntry = {
  seq: 1, at: '2026-11-20T09:01:00.000Z', recipient_id: 'd-1', recipient_kind: 'destination', recipient_name: STORE,
  action: 'send', scope: { course_ids: ['c-safety'], include_interactions: false }, record_count: 4,
  identifier: 'z6MkMadeForThisStore', accepted: true, detail: null,
}

let listed: RecordDestination[] = []

function arm() {
  mocks.invoke.mockImplementation(async (command: string) => {
    switch (command) {
      case 'record_destinations': return listed
      case 'record_destination_preview': return preview
      case 'record_destination_add': listed = [destination()]; return listed[0]
      case 'record_destination_remove': listed = []; return { voided: 4, void_error: null }
      case 'record_destinations_send_now': return null
      case 'record_access_log': return [sent]
      default: throw new Error(command)
    }
  })
}

const mountPanel = (showLog = false) =>
  mount(RecordDestinations, {
    props: { showLog },
    global: { mocks: { $t: (key: string) => key }, stubs: { teleport: true } },
  })

const commands = () => mocks.invoke.mock.calls.map((c) => c[0] as string)

beforeEach(() => {
  mocks.invoke.mockReset()
  listed = []
  arm()
})

describe('RecordDestinations', () => {
  it('shows the consent screen before anything is approved, and adds the store only on approval', async () => {
    const wrapper = mountPanel(); await flushPromises()
    await wrapper.get('[data-testid=record-destination-add]').trigger('click')
    const inputs = wrapper.get('[data-testid=record-destination-form]').findAll('input')
    await inputs[0]!.setValue(STORE)
    await inputs[1]!.setValue('learner-key')
    await inputs[2]!.setValue('issued-secret')
    await wrapper.get('[data-testid=record-destination-form]').trigger('submit')
    await flushPromises()

    expect(wrapper.find('[data-testid=record-consent]').exists()).toBe(true)
    expect(commands()).not.toContain('record_destination_add')
    expect(mocks.invoke).toHaveBeenCalledWith('record_destination_preview', { address: STORE, username: 'learner-key' })

    await wrapper.get('[data-testid=consent-course-c-safety]').setValue(true)
    await wrapper.get('[data-testid=consent-approve]').trigger('click')
    await flushPromises()

    expect(mocks.invoke).toHaveBeenCalledWith('record_destination_add', {
      request: {
        address: STORE, username: 'learner-key', secret: 'issued-secret',
        course_ids: ['c-safety'], include_interactions: false, identifier: 'pairwise', confirm_private: false,
      },
    })
    expect(wrapper.findAll('[data-testid=record-destination]')).toHaveLength(1)
    expect(wrapper.find('[data-testid=record-consent]').exists()).toBe(false)
  })

  it('on a supervised profile, says a guardian has to approve the destination instead of showing the refusal', async () => {
    const wrapper = mountPanel(); await flushPromises()
    await wrapper.get('[data-testid=record-destination-add]').trigger('click')
    const inputs = wrapper.get('[data-testid=record-destination-form]').findAll('input')
    await inputs[0]!.setValue(STORE)
    await inputs[1]!.setValue('learner-key')
    await inputs[2]!.setValue('issued-secret')
    await wrapper.get('[data-testid=record-destination-form]').trigger('submit')
    await flushPromises()
    await wrapper.get('[data-testid=consent-course-c-safety]').setValue(true)
    mocks.invoke.mockRejectedValueOnce('guardian_approval_required: a guardian approves where a supervised profile sends records')
    await wrapper.get('[data-testid=consent-approve]').trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('organisations.records.guardianRequired')
    expect(wrapper.text()).not.toContain('guardian_approval_required')
    expect(wrapper.findAll('[data-testid=record-destination]')).toHaveLength(0)
  })

  it('makes a refused send visible with its reason and when it will be retried', async () => {
    listed = [destination({
      status: { queued: 0, failed: 4, sent: 0, last_error: 'refused: the store answered HTTP 401', next_attempt_at: '2026-11-20T09:01:30.000Z', last_sent_at: null },
    })]
    const wrapper = mountPanel(); await flushPromises()
    const status = wrapper.get('[data-testid=record-destination-status]').text()
    expect(status).toContain('organisations.records.status.failed')
    expect(status).toContain('organisations.records.status.error')
    expect(status).toContain('organisations.records.status.retry')
  })

  it('removes a store only after the learner confirms, and says what removal did', async () => {
    listed = [destination()]
    const wrapper = mountPanel(); await flushPromises()
    await wrapper.findAll('button').find((b) => b.text() === 'organisations.records.remove')!.trigger('click')
    expect(commands()).not.toContain('record_destination_remove')
    expect(wrapper.text()).toContain('organisations.records.removeMessage')
    const removeButtons = wrapper.findAll('button').filter((b) => b.text() === 'organisations.records.remove')
    const confirm = removeButtons[removeButtons.length - 1]!
    await confirm.trigger('click')
    await flushPromises()
    expect(mocks.invoke).toHaveBeenCalledWith('record_destination_remove', { id: 'd-1' })
    expect(wrapper.text()).toContain('organisations.records.removed')
    expect(wrapper.findAll('[data-testid=record-destination]')).toHaveLength(0)
  })

  it('lists what this device sent when asked to show the log', async () => {
    const wrapper = mountPanel(true); await flushPromises()
    expect(wrapper.findAll('[data-testid=record-send-entry]')).toHaveLength(1)
  })
})
