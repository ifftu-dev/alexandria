import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { PersonhoodLabStatus } from '@/types'

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))

function state(phase = 'idle', enabled = true): PersonhoodLabStatus {
  return { enabled, phase, key_status: 'missing', downloaded_bytes: 0,
    total_bytes: 612082146, elapsed_ms: 0, error: null, result: null }
}

function deferred() {
  let resolve: (value: PersonhoodLabStatus) => void = () => undefined
  const promise = new Promise<PersonhoodLabStatus>((done) => { resolve = done })
  return { promise, resolve }
}

describe('developer synthetic personhood flow', () => {
  beforeEach(() => { vi.resetModules(); mocks.invoke.mockReset() })

  it('checks availability without downloading or proving automatically', async () => {
    mocks.invoke.mockResolvedValue(state('idle', false))
    const lab = (await import('./usePersonhoodLab')).usePersonhoodLab()
    await lab.refresh()
    expect(lab.status.value?.enabled).toBe(false)
    expect(mocks.invoke.mock.calls).toEqual([['personhood_lab_status']])
  })

  it('does not let an old poll overwrite an explicit action', async () => {
    const poll = deferred()
    mocks.invoke.mockReturnValueOnce(poll.promise).mockResolvedValueOnce(state('downloading'))
    const lab = (await import('./usePersonhoodLab')).usePersonhoodLab()
    const refresh = lab.refresh()
    await lab.act('download')
    poll.resolve(state())
    await refresh
    expect(lab.status.value?.phase).toBe('downloading')
    expect(lab.busy.value).toBe(true)
  })

  it('allows cancel during an in-flight action and ignores its late result', async () => {
    const start = deferred()
    mocks.invoke.mockResolvedValueOnce(state())
      .mockReturnValueOnce(start.promise).mockResolvedValueOnce(state('cancelled'))
    const lab = (await import('./usePersonhoodLab')).usePersonhoodLab()
    await lab.refresh()
    const work = lab.act('prove')
    const cancellation = lab.cancel()
    start.resolve(state('proving'))
    await work
    await cancellation
    expect(lab.status.value?.phase).toBe('cancelled')
    expect(lab.pending.value).toBe(false)
    expect(mocks.invoke.mock.calls.slice(1)).toEqual([
      ['personhood_lab_action', { action: 'prove' }],
      ['personhood_lab_action', { action: 'cancel' }],
    ])
  })

  it('reports IPC failures and allows retry', async () => {
    mocks.invoke.mockRejectedValueOnce(new Error('unavailable')).mockResolvedValueOnce(state('checking'))
    const lab = (await import('./usePersonhoodLab')).usePersonhoodLab()
    await lab.act('prove')
    expect(lab.error.value).toContain('unavailable')
    expect(lab.pending.value).toBe(false)
    await lab.act('prove')
    expect(lab.error.value).toBeNull()
    expect(lab.status.value?.phase).toBe('checking')
  })
})
