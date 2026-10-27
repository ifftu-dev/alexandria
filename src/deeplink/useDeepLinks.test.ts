import { beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  push: vi.fn<(to: unknown) => Promise<void>>(),
  resolve: vi.fn<(path: string) => { matched: unknown[] }>(),
  unlocked: { value: false },
  readyCallbacks: [] as Array<() => void | Promise<void>>,
}))

vi.mock('@/router', () => ({ default: { push: mocks.push, resolve: mocks.resolve } }))
vi.mock('@/composables/useProfiles', () => ({
  useProfiles: () => ({ isUnlocked: mocks.unlocked }),
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
  mocks.unlocked.value = false
  mocks.readyCallbacks.length = 0
  mocks.push.mockReset().mockResolvedValue()
  mocks.resolve.mockReset().mockImplementation((path) => ({ matched: path.startsWith('/nowhere') ? [] : [{}] }))
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
