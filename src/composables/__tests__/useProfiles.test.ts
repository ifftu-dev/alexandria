import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
}))

vi.mock('../useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (error: Error) => void
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej })
  return { promise, resolve, reject }
}

const unlocked = {
  wallet: { stake_address: 'private-wallet' },
  profile: { stake_address: 'private-identity', display_name: 'Learner' },
}

async function freshProfiles() {
  vi.resetModules()
  return import('../useProfiles')
}

beforeEach(() => {
  mocks.invoke.mockReset().mockImplementation(async command => {
    if (command === 'unlock_profile') return unlocked
    if (command === 'get_profile_session_token') return 'profile-session-1'
    if (command === 'list_profiles') return [{ id: 'profile-1', display_name: 'Learner' }]
    if (command === 'get_profile') return unlocked.profile
    if (command === 'get_wallet_info') return unlocked.wallet
    return null
  })
})

afterEach(() => vi.restoreAllMocks())

describe('profile lock lifecycle', () => {
  it('shows retryable cleanup failure after backend startup rollback fails', async () => {
    const service = (await freshProfiles()).useProfiles()
    mocks.invoke.mockImplementation(async command => {
      if (command === 'unlock_profile') throw new Error('startup failed')
      if (command === 'get_profile_cleanup_required') return true
      return null
    })
    await expect(service.unlockProfile('profile-1', 'password')).rejects.toThrow('startup failed')
    expect(service.lockState.value).toBe('failed')
    expect(service.activeProfileId.value).toBeNull()
    await expect(service.unlockProfile('profile-2', 'password')).rejects.toThrow('Finish locking')
    await service.lockProfile()
    expect(service.lockState.value).toBe('idle')
  })

  it('allows another unlock after backend rollback succeeds', async () => {
    const service = (await freshProfiles()).useProfiles()
    mocks.invoke.mockImplementation(async command => {
      if (command === 'unlock_profile') throw new Error('incorrect password')
      if (command === 'get_profile_cleanup_required') return false
      return null
    })
    await expect(service.unlockProfile('profile-1', 'password')).rejects.toThrow('incorrect password')
    expect(service.isLockBlocked.value).toBe(false)
    expect(service.activeProfileId.value).toBeNull()
  })

  it('restores the blocked cleanup screen when the frontend reloads', async () => {
    const service = (await freshProfiles()).useProfiles()
    mocks.invoke.mockImplementation(async command => {
      if (command === 'get_profile_cleanup_required') return true
      if (command === 'list_profiles') return []
      throw new Error(`Unexpected command: ${command}`)
    })
    await expect(service.initialize()).resolves.toBe('picker')
    expect(service.lockState.value).toBe('failed')
    expect(service.initialized.value).toBe(true)
    expect(service.isUnlocked.value).toBe(false)
  })

  it('requires an explicit completed lock before switching an active profile', async () => {
    const service = (await freshProfiles()).useProfiles()
    await service.unlockProfile('profile-1', 'password')
    await expect(service.unlockProfile('profile-2', 'password')).rejects.toThrow('Lock the current profile')
    expect(service.activeProfileId.value).toBe('profile-1')
    expect(mocks.invoke.mock.calls.filter(([command]) => command === 'unlock_profile')).toHaveLength(1)
  })

  it('hides private state immediately and blocks activation until backend cleanup succeeds', async () => {
    const service = (await freshProfiles()).useProfiles()
    await service.unlockProfile('profile-1', 'password')
    const cleanup = deferred<void>()
    mocks.invoke.mockImplementation(async command => {
      if (command === 'lock_profile') return cleanup.promise
      return null
    })
    const locking = service.lockProfile()
    expect(service.lockProfile()).toBe(locking)
    expect(service.lockState.value).toBe('locking')
    expect(service.isUnlocked.value).toBe(false)
    expect(service.activeIdentity.value).toBeNull()
    expect(service.activeWallet.value).toBeNull()
    await expect(service.unlockProfile('profile-2', 'password')).rejects.toThrow('Finish locking')
    await expect(service.createProfile('new', 'New', 'password')).rejects.toThrow('Finish locking')
    await expect(service.restoreProfileWithMnemonic('old', 'Old', 'words', 'password')).rejects.toThrow('Finish locking')
    cleanup.resolve()
    await locking
    expect(service.lockState.value).toBe('idle')
    expect(mocks.invoke.mock.calls.filter(([command]) => command === 'lock_profile')).toHaveLength(1)
  })

  it('keeps failed cleanup private and retryable without enabling the next unlock', async () => {
    const service = (await freshProfiles()).useProfiles()
    await service.unlockProfile('profile-1', 'password')
    mocks.invoke.mockRejectedValueOnce(new Error('cleanup failed'))
    await expect(service.lockProfile()).rejects.toThrow('cleanup failed')
    expect(service.lockState.value).toBe('failed')
    expect(service.lockError.value).toBe('cleanup failed')
    expect(service.isUnlocked.value).toBe(false)
    await expect(service.unlockProfile('profile-2', 'password')).rejects.toThrow('Finish locking')
    await service.lockProfile()
    expect(service.isLockBlocked.value).toBe(false)
    await service.unlockProfile('profile-2', 'password')
    expect(service.activeProfileId.value).toBe('profile-2')
  })

  it('waits for in-flight activation, discards its late result, then tears down its resources', async () => {
    const { useProfiles, onProfileReady } = await freshProfiles()
    const service = useProfiles()
    const ready = vi.fn()
    onProfileReady(ready)
    const startup = deferred<typeof unlocked>()
    mocks.invoke.mockImplementation(async command => {
      if (command === 'unlock_profile') return startup.promise
      return null
    })
    const unlocking = service.unlockProfile('profile-1', 'password')
    const cancelled = expect(unlocking).rejects.toThrow('cancelled by locking')
    const locking = service.lockProfile()
    expect(mocks.invoke).not.toHaveBeenCalledWith('lock_profile')
    startup.resolve(unlocked)
    await cancelled
    await locking
    expect(mocks.invoke).toHaveBeenCalledWith('lock_profile')
    expect(service.activeIdentity.value).toBeNull()
    expect(service.activeProfileId.value).toBeNull()
    expect(ready).not.toHaveBeenCalled()
  })

  it('rejects overlapping activations before the second IPC call', async () => {
    const service = (await freshProfiles()).useProfiles()
    const startup = deferred<typeof unlocked>()
    mocks.invoke.mockImplementation(async command => command === 'unlock_profile' ? startup.promise : [])
    const first = service.unlockProfile('profile-1', 'password')
    await expect(service.unlockProfile('profile-2', 'password')).rejects.toThrow('already being unlocked')
    startup.resolve(unlocked)
    await first
    expect(mocks.invoke.mock.calls.filter(([command]) => command === 'unlock_profile')).toHaveLength(1)
  })

  it('ignores a stale active-profile response from initialization after lock', async () => {
    const service = (await freshProfiles()).useProfiles()
    const activeId = deferred<string>()
    const requested = deferred<void>()
    mocks.invoke.mockImplementation(async command => {
      if (command === 'list_profiles') return []
      if (command === 'get_active_profile_id') {
        requested.resolve()
        return activeId.promise
      }
      return null
    })
    const initializing = service.initialize()
    const cancelled = expect(initializing).rejects.toThrow('cancelled by locking')
    await requested.promise
    await service.lockProfile()
    activeId.resolve('profile-1')
    await cancelled
    expect(service.isUnlocked.value).toBe(false)
  })

  it('does not restore identity or wallet from requests started before locking', async () => {
    const service = (await freshProfiles()).useProfiles()
    await service.unlockProfile('profile-1', 'password')
    const identity = deferred<typeof unlocked.profile>()
    const wallet = deferred<typeof unlocked.wallet>()
    mocks.invoke.mockImplementation(async command => {
      if (command === 'get_profile') return identity.promise
      if (command === 'get_wallet_info') return wallet.promise
      return null
    })
    const refreshing = Promise.all([service.refreshActiveIdentity(), service.refreshActiveWallet()])
    await service.lockProfile()
    identity.resolve(unlocked.profile)
    wallet.resolve(unlocked.wallet)
    await refreshing
    expect(service.activeIdentity.value).toBeNull()
    expect(service.activeWallet.value).toBeNull()
  })

  it('runs all cleanup hooks but does not claim success when a hook fails', async () => {
    const { useProfiles, onProfileLocked } = await freshProfiles()
    const service = useProfiles()
    const failing = vi.fn().mockRejectedValueOnce(new Error('listener cleanup failed')).mockResolvedValue(undefined)
    const later = vi.fn()
    onProfileLocked(failing)
    onProfileLocked(later)
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    await expect(service.lockProfile()).rejects.toThrow('Profile cleanup did not finish')
    expect(later).toHaveBeenCalledOnce()
    expect(service.lockState.value).toBe('failed')
    await service.lockProfile()
    expect(service.lockState.value).toBe('idle')
    expect(failing).toHaveBeenCalledTimes(2)
  })
})
