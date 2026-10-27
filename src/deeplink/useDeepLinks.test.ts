import { beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  currentRoute: { value: { fullPath: '/' } },
  push: vi.fn<(to: unknown) => Promise<void>>(),
  resolve: vi.fn<(path: string) => { matched: unknown[]; fullPath?: string }>(),
  unlocked: { value: false },
  profile: { value: null as string | null },
  readyCallbacks: [] as Array<() => void | Promise<void>>,
}))

vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))

vi.mock('@/router', () => ({ default: { push: mocks.push, resolve: mocks.resolve, currentRoute: mocks.currentRoute, beforeResolve: () => () => {} } }))
vi.mock('@/composables/useProfiles', () => ({
  useProfiles: () => ({ isUnlocked: mocks.unlocked, activeProfileId: mocks.profile }),
  onProfileReady: (cb: () => void | Promise<void>) => {
    mocks.readyCallbacks.push(cb)
    return () => {}
  },
}))

async function fresh() {
  vi.resetModules()
  return (await import('./useDeepLinks')).useDeepLinks()
}

beforeEach(() => {
  mocks.invoke.mockReset().mockResolvedValue(null)
  mocks.unlocked.value = false
  mocks.readyCallbacks.length = 0
  mocks.push.mockReset().mockImplementation(async to => { mocks.currentRoute.value.fullPath = typeof to === 'string' ? to : '/' })
  mocks.resolve.mockReset().mockImplementation((path) => ({ matched: path.startsWith('/nowhere') ? [] : [{}], fullPath: path }))
})

describe('openPath', () => {
  it('navigates at once when a profile is unlocked', async () => {
    const links = await fresh()
    mocks.unlocked.value = true
    await links.openPath('/interview-room/abc')
    expect(mocks.push).toHaveBeenCalledWith('/interview-room/abc')
    expect(links.heldTarget()).toBeNull()
  })

  it('holds a path while locked and replays it on the next unlock', async () => {
    const links = await fresh()
    await links.openPath('/settings/organisations')
    expect(mocks.push).not.toHaveBeenCalled()
    expect(links.heldTarget()).toEqual({ kind: 'route', path: '/settings/organisations' })

    mocks.unlocked.value = true
    for (const cb of mocks.readyCallbacks) await cb()
    expect(mocks.push).toHaveBeenCalledWith('/settings/organisations')
    expect(links.heldTarget()).toBeNull()
  })

  it('keeps only the most recent held target', async () => {
    const links = await fresh()
    await links.openPath('/credentials/1')
    await links.openPath('/credentials/2')
    expect(links.heldTarget()).toEqual({ kind: 'route', path: '/credentials/2' })
  })

  it('refuses a path the router does not know', async () => {
    const links = await fresh()
    mocks.unlocked.value = true
    await links.openPath('/nowhere')
    expect(mocks.push).not.toHaveBeenCalled()
  })

  it('refuses anything that is not a same-document absolute path', async () => {
    const links = await fresh()
    mocks.unlocked.value = true
    await links.openPath('//evil.example/x')
    await links.openPath('skills')
    await links.openPath('')
    expect(mocks.push).not.toHaveBeenCalled()
    expect(links.heldTarget()).toBeNull()
  })
})

describe('openLink', () => {
  it('routes a recognised deep link and ignores a foreign one', async () => {
    const links = await fresh()
    mocks.unlocked.value = true
    await links.openLink('alexandria://notification/n-1')
    expect(mocks.push).toHaveBeenCalledWith('/notifications?open=n-1')
    await links.openLink('https://example.com/notifications')
    expect(mocks.push).toHaveBeenCalledTimes(1)
  })
})

describe('recipient-bound notification taps', () => {
  it('holds A through B unlock, then routes A once despite event and drain delivery', async () => {
    const links = await fresh()
    const tap = { id: 'tap-1', profileId: 'A', networkId: 'network', recipient: 'binding-A', route: '/settings/organisations' }
    await links.openNotification(tap)
    mocks.profile.value = 'B'
    mocks.unlocked.value = true
    for (const cb of mocks.readyCallbacks) await cb()
    expect(mocks.push).not.toHaveBeenCalled()
    mocks.profile.value = 'A'
    for (const cb of mocks.readyCallbacks) await cb()
    await links.openNotification(tap)
    expect(mocks.push).toHaveBeenCalledExactlyOnceWith('/settings/organisations')
    expect(links.hasUnlockDestination()).toBe(true)
  })
})

it('does not navigate after a panel link resolves in an obsolete profile session', async () => {
  const links = await fresh()
  const sessions = await import('@/composables/profileSession')
  sessions.setProfileSessionToken('session-A')
  mocks.unlocked.value = true
  let finish!: (value: unknown) => void
  mocks.invoke.mockImplementation(() => new Promise(resolve => { finish = resolve }))
  const opening = links.openLink('alexandria://interview/join?cloud=https%3A%2F%2Fcloud.example&id=i&panelist=p&token=t')
  await vi.waitFor(() => expect(mocks.invoke).toHaveBeenCalled())
  sessions.setProfileSessionToken('session-B')
  finish({ sessionId: 'private-A-panel' })
  await opening
  expect(mocks.push).not.toHaveBeenCalled()
})

it('resolves an opaque cold tap only after its matching profile unlocks', async () => {
  const links = await fresh()
  const tap = { id: 'opaque-tap', profileId: 'A', networkId: 'network', recipient: null, route: '' }
  await links.openNotification(tap)
  mocks.profile.value = 'B'
  mocks.unlocked.value = true
  for (const cb of mocks.readyCallbacks) await cb()
  expect(mocks.invoke).not.toHaveBeenCalled()
  mocks.profile.value = 'A'
  mocks.invoke.mockResolvedValueOnce({ ...tap, route: '/credentials/secret' })
  for (const cb of mocks.readyCallbacks) await cb()
  expect(mocks.push).toHaveBeenCalledExactlyOnceWith('/credentials/secret')
  expect(mocks.invoke).toHaveBeenCalledWith('notifications_consume_open', { id: 'opaque-tap' })
})

it('does not navigate a decrypted tap returned after the profile session changes', async () => {
  const links = await fresh()
  const sessions = await import('@/composables/profileSession')
  sessions.setProfileSessionToken('A')
  mocks.unlocked.value = true
  mocks.profile.value = 'A'
  let finish!: (value: unknown) => void
  mocks.invoke.mockImplementation(() => new Promise(resolve => { finish = resolve }))
  const tap = { id: 'opaque', profileId: 'A', networkId: 'network', recipient: null, route: '' }
  const opening = links.openNotification(tap)
  await vi.waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith('notifications_pending_open'))
  sessions.setProfileSessionToken('B')
  finish({ ...tap, route: '/credentials/private-A' })
  await opening
  expect(mocks.push).not.toHaveBeenCalled()
})
