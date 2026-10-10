import { beforeEach, describe, expect, it, vi } from 'vitest'
const mocks = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))
function deferred<T>() {
  let resolve: (value: T) => void = () => undefined
  const promise = new Promise<T>(done => { resolve = done })
  return { promise, resolve }
}
describe('private receipt lifecycle', () => {
  beforeEach(() => { vi.resetModules(); mocks.invoke.mockReset() })
  it('cancels an acknowledged challenge without proving after an earlier cancel', async () => {
    const prepared = deferred<string>()
    mocks.invoke.mockReturnValueOnce(prepared.promise).mockResolvedValueOnce(undefined)
    const lab = (await import('./usePersonhoodReceipts')).usePersonhoodReceipts()
    const work = lab.start()
    await lab.cancel()
    prepared.resolve('challenge')
    await work
    expect(mocks.invoke.mock.calls).toEqual([
      ['personhood_receipt_prepare'], ['personhood_receipt_cancel', { challengeId: 'challenge' }],
    ])
    expect(lab.pending.value).toBe(false)
  })
  it('can cancel while proof IPC remains in flight', async () => {
    const proof = deferred<unknown>()
    mocks.invoke.mockResolvedValueOnce('challenge').mockReturnValueOnce(proof.promise).mockResolvedValueOnce(undefined)
    const lab = (await import('./usePersonhoodReceipts')).usePersonhoodReceipts()
    const work = lab.start()
    await Promise.resolve()
    await lab.cancel(true)
    expect(mocks.invoke).toHaveBeenLastCalledWith('personhood_receipt_cancel', { challengeId: 'challenge' })
    proof.resolve({ id: 'late' })
    await work
    expect(lab.receipts.value).toEqual([])
    expect(lab.pending.value).toBe(false)
    expect(mocks.invoke).toHaveBeenCalledTimes(3)
  })
  it('does not restore a prior profile list after cleanup', async () => {
    const list = deferred<unknown[]>()
    mocks.invoke.mockReturnValueOnce(list.promise)
    const lab = (await import('./usePersonhoodReceipts')).usePersonhoodReceipts()
    const refreshing = lab.refresh()
    await lab.cancel(true)
    list.resolve([{ id: 'old-profile' }])
    await refreshing
    expect(lab.receipts.value).toEqual([])
  })
  it('refreshes receipts only after the backend accepts the proof', async () => {
    mocks.invoke.mockResolvedValueOnce('challenge').mockResolvedValueOnce({ id: 'receipt' }).mockResolvedValueOnce([{ id: 'receipt' }])
    const lab = (await import('./usePersonhoodReceipts')).usePersonhoodReceipts()
    await lab.start()
    expect(mocks.invoke.mock.calls.map(call => call[0])).toEqual(['personhood_receipt_prepare', 'personhood_receipt_prove', 'personhood_receipt_list'])
    expect(lab.receipts.value[0]?.id).toBe('receipt')
  })
})
