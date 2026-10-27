import { beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
  listen: vi.fn<(event: string, callback: (event: { payload: unknown }) => void) => Promise<() => void>>(),
  openPath: vi.fn<(path: string | null | undefined) => Promise<void>>(),
  openNotification: vi.fn<(intent: unknown) => Promise<void>>(),
  openLink: vi.fn<(raw: string) => Promise<void>>(),
}))

vi.mock('../useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))
vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }))
vi.mock('../usePlatform', () => ({ isMobilePlatform: true }))
vi.mock('@/deeplink/useDeepLinks', () => ({
  useDeepLinks: () => ({ openPath: mocks.openPath, openLink: mocks.openLink, openNotification: mocks.openNotification }),
}))

async function fresh() {
  vi.resetModules()
  return (await import('../useNotifications')).useNotifications()
}

beforeEach(() => {
  mocks.openPath.mockReset().mockResolvedValue()
  mocks.openLink.mockReset().mockResolvedValue()
  mocks.listen.mockReset().mockResolvedValue(() => undefined)
  mocks.invoke.mockReset().mockImplementation(async (command) => {
    if (command === 'notifications_list') return []
    if (command === 'notifications_unread_count') return 0
    return null
  })
})

describe('notification routes', () => {
  it('hands an in-app path to the deep-link runtime', async () => {
    const notifications = await fresh()
    await notifications.openRoute('/interview-room/abc')
    expect(mocks.openPath).toHaveBeenCalledWith('/interview-room/abc')
    expect(mocks.openLink).not.toHaveBeenCalled()
  })

  it('hands a whole deep link to the parser instead', async () => {
    const notifications = await fresh()
    await notifications.openRoute('alexandria://notification/n-1')
    expect(mocks.openLink).toHaveBeenCalledWith('alexandria://notification/n-1')
    expect(mocks.openPath).not.toHaveBeenCalled()
  })

  it('ignores a row without a route', async () => {
    const notifications = await fresh()
    await notifications.openRoute(null)
    expect(mocks.openPath).not.toHaveBeenCalled()
    expect(mocks.openLink).not.toHaveBeenCalled()
  })

  it('follows a tap the backend held from before the listener existed', async () => {
    mocks.invoke.mockImplementation(async (command) => {
      if (command === 'notifications_list') return []
      if (command === 'notifications_unread_count') return 0
      if (command === 'notifications_pending_open') return { id: 'tap', profileId: 'A', networkId: 'network', recipient: 'r', route: '/settings/organisations' }
      return null
    })
    const notifications = await fresh()
    await notifications.initOpens()
    expect(mocks.openNotification).toHaveBeenCalledWith({ id: 'tap', profileId: 'A', networkId: 'network', recipient: 'r', route: '/settings/organisations' })
  })

  it('routes an open event from the backend', async () => {
    const notifications = await fresh()
    const handlers = new Map<string, (event: { payload: unknown }) => void>()
    mocks.listen.mockImplementation(async (event, callback) => {
      handlers.set(event, callback)
      return () => undefined
    })
    await notifications.initOpens()
    handlers.get('notifications:open')!({ payload: { id: 'tap', profileId: 'A', networkId: 'network', recipient: null, route: '/credentials/1' } })
    await Promise.resolve()
    expect(mocks.openNotification).toHaveBeenCalledWith({ id: 'tap', profileId: 'A', networkId: 'network', recipient: null, route: '/credentials/1' })
  })
})

describe('inbox state', () => {
  it('counts a new unread row once and marks it read locally', async () => {
    const notifications = await fresh()
    const handlers = new Map<string, (event: { payload: unknown }) => void>()
    mocks.listen.mockImplementation(async (event, callback) => {
      handlers.set(event, callback)
      return () => undefined
    })
    await notifications.init()
    const row = { id: 'n1', kind: 'offer_sent', title: 'New offer', body: '', route: null, source: 'push', createdAt: 'now', readAt: null, payload: null }
    handlers.get('notifications:new')!({ payload: { session: null, row } })
    handlers.get('notifications:new')!({ payload: { session: null, row } })
    expect(notifications.items.value).toHaveLength(1)
    expect(notifications.unread.value).toBe(1)

    await notifications.markRead(['n1'])
    expect(mocks.invoke).toHaveBeenCalledWith('notifications_mark_read', { ids: ['n1'] })
    expect(notifications.unread.value).toBe(0)
    expect(notifications.items.value[0]!.readAt).not.toBeNull()

    notifications.teardown()
    expect(notifications.items.value).toHaveLength(0)
  })

  it('opening a row marks it read, then follows its route', async () => {
    const notifications = await fresh()
    const row = { id: 'n2', kind: 'credential_issued', title: 'Credential', body: '', route: '/credentials/2', source: 'local' as const, createdAt: 'now', readAt: null, payload: null }
    await notifications.open(row)
    expect(mocks.invoke).toHaveBeenCalledWith('notifications_mark_read', { ids: ['n2'] })
    expect(mocks.openPath).toHaveBeenCalledWith('/credentials/2')
  })
})

describe('profile changes during actions', () => {
  it('does not follow an old row after mark-read rejects on profile lock', async () => {
    const notifications = await fresh()
    let rejectRead!: (cause: Error) => void
    mocks.invoke.mockImplementation(() => new Promise((_resolve, reject) => { rejectRead = reject }))
    const opening = notifications.open({ id: 'old', kind: 'offer_sent', title: '', body: '', route: '/settings/organisations', source: 'push', createdAt: '', readAt: null, payload: null })
    notifications.teardown()
    rejectRead(new Error('Profile session changed'))
    await opening
    expect(mocks.openPath).not.toHaveBeenCalled()
  })
})

it('ignores a queued inbox event from the previous session after new listeners attach', async () => {
  const notifications = await fresh()
  const session = await import('../profileSession')
  session.setProfileSessionToken('session-B')
  const handlers = new Map<string, (event: { payload: unknown }) => void>()
  mocks.listen.mockImplementation(async (event, callback) => { handlers.set(event, callback); return () => {} })
  await notifications.init()
  handlers.get('notifications:new')!({ payload: { session: 'session-A', row: { id: 'private-A', readAt: null } } })
  expect(notifications.items.value).toEqual([])
  handlers.get('notifications:changed')!({ payload: { session: 'session-A', unread: 9 } })
  expect(notifications.unread.value).toBe(0)
})

it.each(['clear', 'markAllRead', 'markRead', 'refreshPushStatus', 'registerPush', 'unregisterPush'] as const)('ignores late %s completion after profile teardown', async action => {
  const notifications = await fresh()
  let finish!: (result: unknown) => void
  mocks.invoke.mockImplementation(() => new Promise(resolve => { finish = resolve }))
  const pending = action === 'markRead' ? notifications.markRead(['A']) : notifications[action]()
  notifications.teardown()
  mocks.invoke.mockImplementation(async command => command === 'notifications_list' ? [{ id: 'B', readAt: null }] : command === 'notifications_unread_count' ? 1 : null)
  await notifications.load()
  finish({ registered: true, cloudOrigin: 'https://old-profile.example' })
  await pending
  expect(notifications.items.value).toEqual([{ id: 'B', readAt: null }])
  expect(notifications.unread.value).toBe(1)
  expect(notifications.pushStatus.value).toBeNull()
})

it('reconciles opted-out registrations on unlock so offline cleanup is retried', async () => {
  const notifications = await fresh()
  await notifications.reconcilePush()
  expect(mocks.invoke).toHaveBeenCalledWith('push_register')
})

it('changes desired push state through one durable registration command', async () => {
  const notifications = await fresh()
  const result = { enabled: false, registered: false, pendingCleanup: true }
  mocks.invoke.mockResolvedValue(result)
  await notifications.setPushEnabled(false)
  expect(mocks.invoke).toHaveBeenCalledExactlyOnceWith('push_set_enabled', { enabled: false })
  expect(notifications.pushStatus.value).toEqual(result)
})
