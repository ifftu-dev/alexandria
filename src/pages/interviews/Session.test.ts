import { flushPromises, shallowMount } from '@vue/test-utils'
import { afterEach, expect, it, vi } from 'vitest'
import { createI18n } from 'vue-i18n'
import Session from './Session.vue'
import { AppButton } from '@/components/ui'

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), lock: null as (() => void) | null }))
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))
vi.mock('@/composables/useProfiles', () => ({ onProfileLocked: (callback: () => void) => { mocks.lock = callback } }))
vi.mock('@/composables/useAuth', () => ({ useAuth: () => ({ stakeAddress: null }) }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => undefined) }))
vi.mock('vue-router', () => ({ useRoute: () => ({ params: { id: 'seat' }, path: '/panel/seat', query: {} }), useRouter: () => ({ push: vi.fn() }) }))

afterEach(() => { mocks.lock?.(); vi.useRealTimers(); mocks.invoke.mockReset() })

it('disposes a losing local publication and joins the authoritative winner after 409', async () => {
  vi.useFakeTimers()
  const bundle = { session: { id: 'seat', title: 'Panel', status: 'draft', sentinel_enabled: false }, participants: [], notes: [], transcript: [], followups: [], criteria: [] }
  const panel = { interviewId: 'interview', status: 'accepted', organization: 'Org', panel: [], criteria: [], candidate: { did: 'candidate' }, candidateEvidence: [], room: { ticket: null, openingId: 'old-opening', live: false } }
  const winner = { ...panel, room: { ticket: 'winner-ticket', openingId: 'winner-opening', live: true } }
  let contested = false
  mocks.invoke.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
    if (command === 'interview_get' || command === 'interview_set_status') return bundle
    if (command === 'interview_panel_status') return contested ? winner : panel
    if (command === 'tutoring_status') return null
    if (command === 'tutoring_list_devices') return { audio_inputs: [], audio_outputs: [], cameras: [] }
    if (command === 'tutoring_create_room') return { id: 'loser', ticket: 'loser-ticket' }
    if (command === 'tutoring_join_room') return { id: 'joined', ticket: 'winner-ticket' }
    if (command === 'interview_panel_set_room') {
      if (args?.ticket === 'loser-ticket') { contested = true; throw new Error('directory returned 409: newer opening') }
      return winner
    }
    return undefined
  })
  const wrapper = shallowMount(Session, { global: {
    plugins: [createI18n({ legacy: false, locale: 'en', messages: { en: {} }, missingWarn: false, fallbackWarn: false })],
    stubs: { AppButton: false },
  } })
  await flushPromises()
  const start = wrapper.findAllComponents(AppButton).find(button => button.text() === 'interviews.session.start')
  expect(start).toBeDefined()
  await start!.trigger('click'); await flushPromises()
  expect(mocks.invoke).toHaveBeenCalledWith('tutoring_leave_room')
  expect(mocks.invoke).toHaveBeenCalledWith('tutoring_join_room', expect.objectContaining({ ticket: 'winner-ticket' }))
  expect(mocks.invoke).toHaveBeenCalledWith('interview_panel_set_room', expect.objectContaining({ sessionId: 'seat', roomSessionId: 'joined', openingId: 'winner-opening' }))
  const leave = mocks.invoke.mock.calls.findIndex(([cmd]) => cmd === 'tutoring_leave_room')
  const join = mocks.invoke.mock.calls.findIndex(([cmd]) => cmd === 'tutoring_join_room')
  expect(leave).toBeLessThan(join)
  expect(mocks.invoke).toHaveBeenCalledWith('interview_panel_set_room', expect.objectContaining({ roomSessionId: 'loser', openingId: null, previousOpeningId: 'old-opening' }))
  const leaves = mocks.invoke.mock.calls.filter(([cmd]) => cmd === 'tutoring_leave_room').length
  wrapper.unmount()
  expect(mocks.invoke.mock.calls.filter(([cmd]) => cmd === 'tutoring_leave_room')).toHaveLength(leaves)
})

it('does not replace an unreachable room while another panelist remains live', async () => {
  vi.useFakeTimers()
  const bundle = { session: { id: 'seat', title: 'Panel', status: 'draft', sentinel_enabled: false }, participants: [], notes: [], transcript: [], followups: [], criteria: [] }
  mocks.invoke.mockImplementation(async (command: string) => {
    if (command === 'interview_get') return bundle
    if (command === 'interview_panel_status') return { status: 'accepted', panel: [], candidate: {}, candidateEvidence: [], room: { ticket: 'still-live', openingId: 'current', live: true } }
    if (command === 'tutoring_status') return null
    if (command === 'tutoring_list_devices') return { audio_inputs: [], audio_outputs: [], cameras: [] }
    if (command === 'tutoring_join_room') throw new Error('SESSION_UNAVAILABLE: temporarily unreachable')
    return undefined
  })
  const wrapper = shallowMount(Session, { global: {
    plugins: [createI18n({ legacy: false, locale: 'en', messages: { en: {} }, missingWarn: false, fallbackWarn: false })],
    stubs: { AppButton: false },
  } })
  await flushPromises()
  const start = wrapper.findAllComponents(AppButton).find(button => button.text() === 'interviews.session.start')
  await start!.trigger('click'); await flushPromises()
  expect(mocks.invoke.mock.calls.some(([cmd]) => cmd === 'tutoring_create_room')).toBe(false)
  expect(mocks.invoke.mock.calls.some(([cmd]) => cmd === 'interview_panel_set_room')).toBe(false)
  expect(mocks.invoke.mock.calls.filter(([cmd]) => cmd === 'interview_panel_status')).toHaveLength(3)
  wrapper.unmount()
})
