import { beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  Classroom,
  TutoringChatMessage,
  TutoringSessionInfo,
  TutoringTranscriptMessage,
} from '@/types'

type ProfileLockCallback = () => void | Promise<void>
type EventCallback = (event: { payload: unknown }) => void

const mocks = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
  listen: vi.fn<(event: string, callback: EventCallback) => Promise<() => void>>(),
  lockCallbacks: [] as ProfileLockCallback[],
  eventCallbacks: new Map<string, EventCallback>(),
}))

vi.mock('../useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))
vi.mock('../useProfiles', () => ({
  onProfileLocked: (callback: ProfileLockCallback) => mocks.lockCallbacks.push(callback),
  onProfileReady: () => () => undefined,
}))
vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }))

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((res) => { resolve = res })
  return { promise, resolve }
}

async function lockProfile(): Promise<void> {
  await Promise.all(mocks.lockCallbacks.map((callback) => callback()))
}

const classroom: Classroom = {
  id: 'classroom-private',
  name: 'Private classroom',
  description: null,
  icon_emoji: null,
  owner_address: 'owner-private',
  invite_code: 'invite-private',
  status: 'active',
  created_at: '2026-09-14T00:00:00Z',
  updated_at: '2026-09-14T00:00:00Z',
  member_count: 1,
  my_role: 'owner',
}

const session: TutoringSessionInfo = {
  id: 'session-private',
  title: 'Private session',
  ticket: 'ticket-private',
  status: 'active',
  created_at: '2026-09-14T00:00:00Z',
  ended_at: null,
}

beforeEach(() => {
  vi.resetModules()
  mocks.invoke.mockReset()
  mocks.listen.mockReset().mockImplementation(async (event, callback) => {
    mocks.eventCallbacks.set(event, callback)
    return () => undefined
  })
  mocks.lockCallbacks.length = 0
  mocks.eventCallbacks.clear()
})

describe('profile-scoped media and classroom caches', () => {
  it('discards a classroom list that resolves after profile locking starts', async () => {
    const pending = deferred<Classroom[]>()
    mocks.invoke.mockImplementation(async (command) => {
      if (command === 'classroom_list') return pending.promise
      return null
    })
    const classroomStore = (await import('../useClassroom')).useClassroom()

    const loading = classroomStore.loadClassrooms()
    await lockProfile()
    pending.resolve([classroom])
    await loading

    expect(classroomStore.classrooms.value).toEqual([])
    expect(classroomStore.loading.value).toBe(false)
  })

  it('discards display names that resolve after lock and clears graph credentials', async () => {
    const pendingNames = deferred<Record<string, string>>()
    const pendingProfiles = deferred<Record<string, { username: string | null; display_name: string | null }>>()
    mocks.invoke.mockImplementation(async (command) => {
      if (command === 'resolve_display_names') return pendingNames.promise
      if (command === 'resolve_profiles') return pendingProfiles.promise
      return null
    })
    const names = (await import('../useDisplayNames')).useDisplayNames()
    const graph = (await import('../useSkillGraphState')).useSkillGraphState()
    graph.credentials.value = [
      {
        '@context': [],
        id: 'credential-private',
        type: ['VerifiableCredential'],
        issuer: 'did:key:issuer-private',
        validFrom: '2026-09-14T00:00:00Z',
        credentialSubject: { id: 'did:key:z6MkLearnerPrivate' },
        proof: {
          type: 'Ed25519Signature2020',
          created: '2026-09-14T00:00:00Z',
          verificationMethod: 'did:key:issuer-private#key-1',
          proofPurpose: 'assertionMethod',
          jws: 'signature-private',
        },
      },
    ]

    const resolving = names.ensureNames(['did:key:z6MkLearnerPrivate'])
    await lockProfile()
    pendingNames.resolve({ 'did:key:z6MkLearnerPrivate': 'Private learner' })
    pendingProfiles.resolve({
      'did:key:z6MkLearnerPrivate': {
        username: 'private-learner',
        display_name: 'Private learner',
      },
    })
    await resolving

    expect(names.displayName('did:key:z6MkLearnerPrivate')).toBe('z6MkLear…vate')
    expect(names.username('did:key:z6MkLearnerPrivate')).toBeNull()
    expect(graph.credentials.value).toEqual([])
  })

  it('clears a stale P2P status and ignores its late response', async () => {
    const pendingStatus = deferred<{
      is_running: boolean
      peer_id: string | null
      listening_addresses: string[]
      connected_peers: number
      subscribed_topics: string[]
      nat_status: string
      relay_addresses: string[]
    }>()
    mocks.invoke.mockImplementation(async (command) => {
      if (command === 'p2p_status') return pendingStatus.promise
      return null
    })
    const p2p = (await import('../useP2P')).useP2P()

    const refreshing = p2p.refreshStatus()
    await lockProfile()
    pendingStatus.resolve({
      is_running: true,
      peer_id: 'peer-private',
      listening_addresses: ['/ip4/127.0.0.1/tcp/1'],
      connected_peers: 1,
      subscribed_topics: ['/private'],
      nat_status: 'public',
      relay_addresses: [],
    })
    await refreshing

    expect(p2p.status.value).toBeNull()
    expect(p2p.lastError.value).toBeNull()
  })

  it('discards late account and guardian state after lock', async () => {
    const pendingAccount = deferred<{
      roles: ['learner']
      role: 'learner'
      birthdate: string | null
      is_minor: boolean
      activation_state: string
    }>()
    const pendingLinks = deferred<
      Array<{
        id: string
        side: 'ward'
        peer_did: string
        peer_display_name: string | null
        status: 'active'
        child_birthdate: string | null
        created_at: string
        last_sync_at: string | null
      }>
    >()
    mocks.invoke.mockImplementation(async (command) => {
      if (command === 'get_account_status') return pendingAccount.promise
      if (command === 'guardian_list_links') return pendingLinks.promise
      return null
    })
    const account = (await import('../useAccountStatus')).useAccountStatus()
    const guardian = (await import('../useGuardian')).useGuardian()

    const refreshing = Promise.all([account.refreshAccountStatus(), guardian.refreshLinks()])
    await lockProfile()
    pendingAccount.resolve({
      roles: ['learner'],
      role: 'learner',
      birthdate: '2010-01-01',
      is_minor: true,
      activation_state: 'pending_guardian',
    })
    pendingLinks.resolve([
      {
        id: 'link-private',
        side: 'ward',
        peer_did: 'did:key:guardian-private',
        peer_display_name: 'Private guardian',
        status: 'active',
        child_birthdate: '2010-01-01',
        created_at: '2026-09-14T00:00:00Z',
        last_sync_at: null,
      },
    ])
    await refreshing

    expect(account.status.value).toBeNull()
    expect(account.loaded.value).toBe(false)
    expect(guardian.links.value).toEqual([])
    expect(guardian.loaded.value).toBe(false)
  })

  it('clears search recents, pending queries, and content-sync notices on lock', async () => {
    window.localStorage.setItem(
      'alexandria:omni-search-recents',
      JSON.stringify([
        {
          id: 'classroom:private',
          type: 'classroom',
          title: 'Private classroom',
          route: '/classrooms/private',
        },
      ]),
    )
    const search = (await import('../useOmniSearch')).useOmniSearch()
    const sync = (await import('../useContentSync')).useContentSync()
    search.open()
    search.setQuery('private')
    sync.startContentSync()
    sync.completeContentSync({
      hydrated: 2,
      beforeCourses: 3,
      afterCourses: 4,
      durationMs: 5,
    })

    await lockProfile()

    expect(search.isOpen.value).toBe(false)
    expect(search.query.value).toBe('')
    expect(search.results.value).toEqual([])
    expect(search.recents.value).toEqual([])
    expect(window.localStorage.getItem('alexandria:omni-search-recents')).toBeNull()
    expect(sync.phase.value).toBe('idle')
    expect(sync.stats.value).toBeNull()
    expect(sync.visible.value).toBe(false)
  })

  it('removes a classroom listener installed after locking and ignores its callback', async () => {
    const lateUnlisten = vi.fn()
    const pendingListener = deferred<() => void>()
    mocks.listen.mockImplementationOnce(async (event, callback) => {
      mocks.eventCallbacks.set(event, callback)
      return pendingListener.promise
    })
    const classroomStore = (await import('../useClassroom')).useClassroom()

    const setup = classroomStore.enterClassroom(classroom.id)
    await vi.waitFor(() => expect(mocks.listen).toHaveBeenCalledOnce())
    await lockProfile()
    pendingListener.resolve(lateUnlisten)
    await setup

    const callback = mocks.eventCallbacks.get('classroom:message')
    expect(callback).toBeDefined()
    callback?.({
      payload: {
        classroom_id: classroom.id,
        channel_id: 'channel-private',
        message: {
          id: 'message-private',
          channel_id: 'channel-private',
          classroom_id: classroom.id,
          sender_address: 'sender-private',
          sender_name: 'Private learner',
          content: 'private message',
          sent_at: '2026-09-14T00:00:00Z',
        },
      },
    })
    expect(lateUnlisten).toHaveBeenCalledOnce()
    expect(classroomStore.messages.value).toEqual({})
  })

  it('discards tutoring sessions and device data that resolve after locking', async () => {
    const pendingSessions = deferred<TutoringSessionInfo[]>()
    const pendingPeers = deferred<[{ node_id: string; display_name: string; broadcasts: string[]; connected: boolean }]>()
    mocks.invoke.mockImplementation(async (command) => {
      if (command === 'tutoring_list_sessions') return pendingSessions.promise
      if (command === 'tutoring_peers') return pendingPeers.promise
      return null
    })
    const tutoringStore = (await import('../useTutoringRoom')).useTutoringRoom()

    const refreshing = tutoringStore.refreshSessions()
    const peers = tutoringStore.getPeers()
    await lockProfile()
    pendingSessions.resolve([session])
    pendingPeers.resolve([
      {
        node_id: 'peer-private',
        display_name: 'Private learner',
        broadcasts: ['audio'],
        connected: true,
      },
    ])

    await refreshing
    await expect(peers).resolves.toEqual([])
    expect(tutoringStore.sessions.value).toEqual([])
  })

  it('clears tutoring event data and detaches all listeners on lock', async () => {
    const unlisteners = new Map<string, ReturnType<typeof vi.fn>>()
    mocks.listen.mockImplementation(async (event, callback) => {
      mocks.eventCallbacks.set(event, callback)
      const unlisten = vi.fn()
      unlisteners.set(event, unlisten)
      return unlisten
    })
    const tutoringStore = (await import('../useTutoringRoom')).useTutoringRoom()
    await tutoringStore.setupEventListeners()

    const chat: TutoringChatMessage = {
      sender: 'peer-private',
      sender_name: 'Private learner',
      text: 'private message',
      timestamp: 1,
    }
    const transcript: TutoringTranscriptMessage = {
      sender: 'peer-private',
      sender_name: 'Private learner',
      text: 'private caption',
      confidence: 0.9,
      timestamp: 2,
    }
    mocks.eventCallbacks.get('tutoring:chat')?.({ payload: chat })
    mocks.eventCallbacks.get('tutoring:transcript')?.({ payload: transcript })
    mocks.eventCallbacks.get('tutoring:peer-name')?.({
      payload: { node_id: 'peer-private', display_name: 'Private learner' },
    })
    expect(tutoringStore.chatMessages.value).toEqual([chat])
    expect(tutoringStore.transcriptMessages.value).toEqual([transcript])
    expect(tutoringStore.peerNames.value).toEqual({ 'peer-private': 'Private learner' })

    await lockProfile()
    mocks.eventCallbacks.get('tutoring:chat')?.({ payload: chat })
    mocks.eventCallbacks.get('tutoring:transcript')?.({ payload: transcript })

    expect(tutoringStore.chatMessages.value).toEqual([])
    expect(tutoringStore.transcriptMessages.value).toEqual([])
    expect(tutoringStore.peerNames.value).toEqual({})
    expect(tutoringStore.videoFrames.value).toEqual({})
    expect(tutoringStore.micLevel.value).toBe(0)
    expect(tutoringStore.outputLevel.value).toBe(0)
    expect([...unlisteners.values()]).toHaveLength(6)
    for (const unlisten of unlisteners.values()) expect(unlisten).toHaveBeenCalledOnce()
  })
})
