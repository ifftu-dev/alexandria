import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { CompletionWitnessState } from '@/types'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
  locked: null as (() => void) | null,
  session: 'profile-1' as string | null,
}))

vi.mock('../useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))
vi.mock('../profileSession', () => ({ getProfileSessionToken: () => mocks.session }))
vi.mock('../useProfiles', () => ({ onProfileLocked: (callback: () => void) => { mocks.locked = callback } }))

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>(res => { resolve = res })
  return { promise, resolve }
}

const credential = { id: 'vc', type: ['VerifiableCredential', 'SelfAssertion'], credentialSubject: { id: 'subject' } }
const payload = {
  courseTitle: 'Private course', courseId: 'course', skillIds: [], credentialIds: ['vc'],
  txHash: null, claimId: 'claim', witnessStatus: 'pending' as const,
}

async function fresh() {
  vi.resetModules()
  return (await import('../useCourseCompletion')).useCourseCompletion()
}

beforeEach(() => {
  vi.useFakeTimers()
  mocks.session = 'profile-1'
  mocks.locked = null
  mocks.invoke.mockReset().mockImplementation(async command => {
    if (command === 'list_credentials') return [credential]
    if (command === 'list_skills') return []
    if (command === 'get_completion_witness_status') return { status: 'pending', tx_hash: null }
    throw new Error(`Unexpected command ${command}`)
  })
})

afterEach(() => { mocks.locked?.(); vi.useRealTimers() })

describe('durable completion witness display', () => {
  it('finishes local claims while a witness without a transaction remains pending', async () => {
    const service = await fresh()
    await service.open(payload)
    await vi.advanceTimersByTimeAsync(600)
    expect(service.mintStage.value).toBe('issued')
    expect(service.progressPct.value).toBe(100)
    expect(service.witnessStatus.value).toBe('pending')
    expect(service.txHash.value).toBeNull()
    await vi.advanceTimersByTimeAsync(120_000)
    expect(service.witnessStatus.value).toBe('pending')
    expect(mocks.invoke.mock.calls.filter(([name]) => name === 'list_credentials')).toHaveLength(1)
  })

  it('distinguishes uncertain and submitted transactions from confirmed receipts', async () => {
    const service = await fresh()
    await service.open(payload)
    let receipt: CompletionWitnessState = { status: 'outcome_unknown', tx_hash: 'original' }
    mocks.invoke.mockImplementation(async command => command === 'get_completion_witness_status' ? receipt : [])
    await vi.advanceTimersByTimeAsync(5000)
    expect(service.witnessStatus.value).toBe('outcome_unknown')
    expect(service.txHash.value).toBe('original')
    receipt = { status: 'submitted', tx_hash: 'original' }
    await vi.advanceTimersByTimeAsync(5000)
    expect(service.witnessStatus.value).toBe('submitted')
    receipt = { status: 'confirmed', tx_hash: 'original' }
    await vi.advanceTimersByTimeAsync(5000)
    expect(service.witnessStatus.value).toBe('confirmed')
    const calls = mocks.invoke.mock.calls.length
    await vi.advanceTimersByTimeAsync(60_000)
    expect(mocks.invoke.mock.calls).toHaveLength(calls)
  })

  it('does not overlap status reads and ignores a late result after lock', async () => {
    const response = deferred<CompletionWitnessState>()
    mocks.invoke.mockImplementation(async command => {
      if (command === 'get_completion_witness_status') return response.promise
      return command === 'list_credentials' ? [credential] : []
    })
    const service = await fresh()
    await service.open(payload)
    await vi.advanceTimersByTimeAsync(30_000)
    expect(mocks.invoke.mock.calls.filter(([name]) => name === 'get_completion_witness_status')).toHaveLength(1)
    mocks.session = null
    mocks.locked?.()
    response.resolve({ status: 'confirmed', tx_hash: 'private-transaction' })
    await vi.advanceTimersByTimeAsync(30_000)
    expect(service.isOpen.value).toBe(false)
    expect(service.courseTitle.value).toBe('')
    expect(service.items.value).toEqual([])
    expect(service.primaryCredentialId.value).toBeNull()
    expect(service.txHash.value).toBeNull()
    expect(vi.getTimerCount()).toBe(0)
  })

  it('does not let a stale credential list overwrite a new celebration', async () => {
    const response = deferred<unknown[]>()
    const service = await fresh()
    mocks.invoke.mockImplementation(async command => command === 'list_credentials' ? response.promise : [])
    const first = service.open({ ...payload, claimId: undefined, witnessStatus: 'not_requested' })
    service.close()
    mocks.invoke.mockImplementation(async command => command === 'list_credentials' ? [{ ...credential, id: 'new-vc' }] : [])
    await service.open({ ...payload, courseTitle: 'New course', credentialIds: ['new-vc'], claimId: undefined, witnessStatus: 'not_requested' })
    response.resolve([credential])
    await first
    await vi.advanceTimersByTimeAsync(600)
    expect(service.courseTitle.value).toBe('New course')
    expect(service.items.value.map(item => item.id)).toEqual(['new-vc'])
  })

  it('preserves local success when the optional witness fails', async () => {
    const service = await fresh()
    mocks.invoke.mockImplementation(async command => {
      if (command === 'get_completion_witness_status') return { status: 'failed_on_chain', tx_hash: 'failed' }
      return command === 'list_credentials' ? [credential] : []
    })
    await service.open(payload)
    await vi.advanceTimersByTimeAsync(600)
    expect(service.witnessStatus.value).toBe('failed_on_chain')
    expect(service.mintStage.value).toBe('issued')
    expect(service.items.value).toHaveLength(1)
    expect(vi.getTimerCount()).toBe(0)
  })
})
