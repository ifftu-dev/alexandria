import { beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
  listen: vi.fn<(event: string, callback: (event: { payload: boolean }) => void) => Promise<() => void>>(),
  stop: vi.fn<() => Promise<string | undefined>>(),
  active: { value: true },
}))

vi.mock('../useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))
vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }))
vi.mock('../useSentinel', () => ({
  useSentinel: () => ({ isActive: mocks.active, stop: mocks.stop }),
}))

async function freshDiagnostics() {
  vi.resetModules()
  return (await import('../useDiagnostics')).useDiagnostics()
}

beforeEach(() => {
  mocks.active.value = true
  mocks.stop.mockReset().mockResolvedValue('session-1')
  mocks.listen.mockReset().mockResolvedValue(() => undefined)
  mocks.invoke.mockReset().mockImplementation(async command => {
    if (command === 'diagnostics_status') return { enabled: false, open_assessment_count: 1 }
    if (command === 'diagnostics_enter') return { enabled: true, open_assessment_count: 0 }
    return null
  })
})

describe('explicit diagnostics lifecycle', () => {
  it('does not enable diagnostics when assessment cleanup fails', async () => {
    const diagnostics = await freshDiagnostics()
    mocks.stop.mockRejectedValueOnce(new Error('cleanup failed'))

    await expect(diagnostics.confirmEntry()).rejects.toThrow('cleanup failed')

    expect(mocks.invoke).not.toHaveBeenCalledWith('diagnostics_enter')
    expect(diagnostics.enabled.value).toBe(false)
    expect(diagnostics.error.value).toContain('cleanup failed')
  })

  it('saves registered work before monitoring cleanup and backend entry', async () => {
    const diagnostics = await freshDiagnostics()
    const order: string[] = []
    diagnostics.registerEntryPreparation(() => { order.push('draft') })
    mocks.stop.mockImplementationOnce(async () => { order.push('cleanup'); return 'session-1' })
    mocks.invoke.mockImplementation(async command => {
      if (command === 'diagnostics_enter') {
        order.push('enter')
        return { enabled: true, open_assessment_count: 0 }
      }
      return null
    })

    await diagnostics.confirmEntry()

    expect(order).toEqual(['draft', 'cleanup', 'enter'])
    expect(diagnostics.enabled.value).toBe(true)
  })

  it('stops before cleanup when saving work fails', async () => {
    const diagnostics = await freshDiagnostics()
    diagnostics.registerEntryPreparation(() => { throw new Error('draft failed') })

    await expect(diagnostics.confirmEntry()).rejects.toThrow('draft failed')

    expect(mocks.stop).not.toHaveBeenCalled()
    expect(mocks.invoke).not.toHaveBeenCalledWith('diagnostics_enter')
  })
})
