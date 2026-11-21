import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { RecordAccessEntry } from '@/types'

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), locked: [] as (() => void)[] }))
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))
vi.mock('@/composables/useProfiles', () => ({ onProfileLocked: (run: () => void) => { mocks.locked.push(run) } }))

const sent: RecordAccessEntry = {
  seq: 1, at: '2026-11-20T09:01:00.000Z', recipient_id: 'd-1', recipient_kind: 'destination', recipient_name: 'store',
  action: 'send', scope: { course_ids: ['c-safety'], include_interactions: false }, record_count: 4,
  identifier: 'z6MkMadeForThisStore', accepted: true, detail: null,
}

describe('useRecordSendLog', () => {
  beforeEach(() => {
    vi.resetModules()
    mocks.invoke.mockReset()
    mocks.locked.length = 0
  })

  it('is one log for every view: a refresh from one is seen by the others', async () => {
    const { useRecordSendLog } = await import('../useRecordSendLog')
    mocks.invoke.mockResolvedValue([sent])
    const destinationsPanel = useRecordSendLog()
    const organisationAccess = useRecordSendLog()
    await destinationsPanel.refresh()
    expect(mocks.invoke).toHaveBeenCalledWith('record_access_log')
    expect(organisationAccess.entries.value).toEqual([sent])
  })

  it('keeps the reason a read failed, and clears it on the next good read', async () => {
    const { useRecordSendLog } = await import('../useRecordSendLog')
    const log = useRecordSendLog()
    mocks.invoke.mockRejectedValueOnce('profile session is locked')
    await log.refresh()
    expect(log.error.value).toBe('profile session is locked')
    mocks.invoke.mockResolvedValueOnce(null)
    await log.refresh()
    expect(log.error.value).toBe('')
    expect(log.entries.value).toEqual([])
  })

  it('forgets what was sent when the profile locks', async () => {
    const { useRecordSendLog } = await import('../useRecordSendLog')
    mocks.invoke.mockResolvedValue([sent])
    const log = useRecordSendLog()
    await log.refresh()
    for (const run of mocks.locked) run()
    expect(log.entries.value).toEqual([])
  })
})
