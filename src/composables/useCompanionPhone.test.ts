import { beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))

import { useCompanionPhone } from './useCompanionPhone'

type Waiter = { resolve: (v: unknown) => void; reject: (e: unknown) => void }

beforeEach(() => {
  mocks.invoke.mockReset()
  useCompanionPhone().reset()
})

/** Answers the offer, and holds the wait open until the test settles it. */
function desktopBackend(): Waiter {
  const waiter = {} as Waiter
  mocks.invoke.mockImplementation((cmd: string) => {
    if (cmd === 'integrity_companion_offer') return Promise.resolve({ code: 'CODE', expires_at: 1 })
    if (cmd === 'integrity_companion_await') {
      return new Promise((resolve, reject) => Object.assign(waiter, { resolve, reject }))
    }
    if (cmd === 'integrity_companion_cancel') {
      waiter.reject(new Error('the companion code was withdrawn'))
      return Promise.resolve()
    }
    return Promise.reject(new Error(`unexpected ${cmd}`))
  })
  return waiter
}

describe('useCompanionPhone', () => {
  it('shows the code while waiting and resolves with what the backend stored', async () => {
    const waiter = desktopBackend()
    const phone = useCompanionPhone()
    const pairing = phone.pairPhone('isess-1')
    await vi.waitFor(() => expect(phone.offer.value?.code).toBe('CODE'))
    expect(mocks.invoke).toHaveBeenCalledWith('integrity_companion_offer', { sessionId: 'isess-1', camera: false })

    waiter.resolve({ assurance_level: 'device_attested', camera: false })
    expect(await pairing).toEqual({ assurance_level: 'device_attested', camera: false })
    expect(phone.paired.value?.assurance_level).toBe('device_attested')
    expect(phone.offer.value).toBeNull()
  })

  it('a skip withdraws the code and continues without the phone or an error', async () => {
    desktopBackend()
    const phone = useCompanionPhone()
    const pairing = phone.pairPhone('isess-2')
    await vi.waitFor(() => expect(phone.offer.value).not.toBeNull())
    await phone.skip()
    expect(await pairing).toBeNull()
    expect(mocks.invoke).toHaveBeenCalledWith('integrity_companion_cancel', { sessionId: 'isess-2' })
    expect(phone.error.value).toBeNull()
    expect(phone.offer.value).toBeNull()
  })

  it('a refused pairing resolves null and says why', async () => {
    const waiter = desktopBackend()
    const phone = useCompanionPhone()
    const pairing = phone.pairPhone('isess-3')
    await vi.waitFor(() => expect(phone.offer.value).not.toBeNull())
    waiter.reject(new Error('the phone is signed in as another learner'))
    expect(await pairing).toBeNull()
    expect(phone.error.value).toBe('the phone is signed in as another learner')
  })

  it('a phone joins with the trimmed code and reports a refusal', async () => {
    const phone = useCompanionPhone()
    mocks.invoke.mockResolvedValueOnce({ session_id: 's', assurance_level: 'device_attested', camera: false })
    expect((await phone.join('  CODE \n'))?.session_id).toBe('s')
    expect(mocks.invoke).toHaveBeenCalledWith('integrity_companion_join', { code: 'CODE' })

    mocks.invoke.mockRejectedValueOnce(new Error('this companion code has expired'))
    expect(await phone.join('OLD')).toBeNull()
    expect(phone.error.value).toBe('this companion code has expired')
  })
})
