import { flushPromises } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Identity, SkillInfo, SubjectFieldInfo, VerifiableCredential } from '@/types'

const ipc = vi.hoisted(() => vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>())
vi.mock('@tauri-apps/api/core', () => ({ invoke: ipc }))

const skill: SkillInfo = {
  id: 's', name: 'Skill', description: null, subject_id: 'sub', subject_name: 'Subject',
  subject_field_id: 'field', subject_field_name: null, bloom_level: 'create',
  prerequisite_count: 0, dependent_count: 0, created_at: null,
}
const field: SubjectFieldInfo = {
  id: 'field', name: 'Field', description: null, icon_emoji: null, subject_count: 1, skill_count: 1, created_at: null,
}
const identity: Identity = {
  stake_address: 'stake-a', payment_address: 'payment-a', username: null, display_name: 'A',
  bio: null, avatar_cid: null, visibility: null, profile_hash: null, created_at: '', updated_at: '',
}
const vc = (level: unknown, subject = 'did:a'): VerifiableCredential => ({
  '@context': [], type: ['VerifiableCredential'], issuer: 'issuer', validFrom: '2026-01-01T00:00:00Z',
  credentialSubject: { id: subject, skillId: 's', level },
  proof: { type: '', cryptosuite: '', created: '', verificationMethod: '', proofPurpose: '', proofValue: '' },
})

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (cause: Error) => void
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej })
  return { promise, resolve, reject }
}

let credentials: VerifiableCredential[]
let did: string | null
let session: string
let cleanups: Array<() => void>

async function backend(command: string, args?: Record<string, unknown>): Promise<unknown> {
  switch (command) {
    case 'get_incompatible_profile_data': return null
    case 'get_profile_cleanup_required': return false
    case 'list_profiles': return []
    case 'get_active_profile_id': return 'profile-a'
    case 'get_profile_session_token': return session
    case 'get_profile': return identity
    case 'get_wallet_info': return { stake_address: 'stake-a', payment_address: 'payment-a', has_mnemonic_backup: true }
    case 'unlock_profile':
      session = String(args?.id)
      return { wallet: await backend('get_wallet_info'), profile: identity }
    case 'lock_profile': return null
    case 'get_local_did': return did
    case 'list_skills': return [skill]
    case 'list_subject_fields': return [field]
    case 'list_skill_graph_edges': return []
    case 'list_credentials':
      if (args?.subject !== did || args?.activeOnly !== true) throw new Error('Expected subject-bound active listing')
      return credentials
    default: throw new Error(`Unexpected IPC: ${command}`)
  }
}

async function setup() {
  const { useSkillAtlas } = await import('./useSkillAtlas')
  const profiles = (await import('./useProfiles')).useProfiles()
  await profiles.initialize()
  return { atlas: useSkillAtlas(), profiles, useSkillAtlas }
}

beforeEach(() => {
  vi.resetModules()
  credentials = [vc(1)]
  did = 'did:a'
  session = 'session-a'
  cleanups = []
  ipc.mockReset().mockImplementation(backend)
})
afterEach(() => {
  cleanups.forEach(cleanup => cleanup())
  vi.restoreAllMocks()
  vi.useRealTimers()
})

describe('shared skill atlas state', () => {
  it('loads a subject-bound snapshot at startup, including incomplete field metadata', async () => {
    const { atlas } = await setup()
    await atlas.ensureLoaded()
    expect(atlas.phase.value).toBe('ready')
    expect(atlas.model.value?.nodes[0]?.earnedBloom).toEqual({ rank: 1, name: 'understand' })
    expect(atlas.model.value?.fields[0]?.name).toBe('Field')
    expect(atlas.feedback.value).toBeNull()
  })
  it('lets a new profile load while an old credential request is still pending', async () => {
    const { atlas, profiles } = await setup()
    const pending = deferred<VerifiableCredential[]>()
    ipc.mockImplementation((command, args) => command === 'list_credentials' ? pending.promise : backend(command, args))
    atlas.updateView('page', { selectedSkillId: 's', camera: { x: 8, y: 4, zoom: 2 } })
    atlas.setSkillsTab('graph')
    atlas.setModalOpen(true)
    const oldLoad = atlas.ensureLoaded()
    await flushPromises()
    const locking = profiles.lockProfile()
    expect(atlas.model.value).toBeNull()
    expect(atlas.pageView.value.selectedSkillId).toBeNull()
    expect(atlas.pageView.value.camera).toBeNull()
    expect(atlas.skillsTab.value).toBe('credentials')
    expect(atlas.modalOpen.value).toBe(false)
    await locking
    did = 'did:b'
    credentials = [vc(4, 'did:b')]
    ipc.mockImplementation(backend)
    await profiles.unlockProfile('profile-b', 'password')
    await flushPromises()
    try {
      expect(atlas.model.value?.nodes[0]?.earnedBloom?.rank).toBe(4)
      expect(atlas.feedback.value).toBeNull()
    } finally {
      pending.resolve([vc(5)])
      await oldLoad
    }
    expect(atlas.model.value?.nodes[0]?.earnedBloom?.rank).toBe(4)
    expect(atlas.error.value).toBeNull()
    expect(atlas.refreshing.value).toBe(false)
  })
  it('refreshes level-only credential events without moving the page or modal view', async () => {
    const { atlas, useSkillAtlas } = await setup()
    const { installSkillAtlasRefresh } = await import('./useSkillAtlas')
    const { announceCredentialsChanged } = await import('./credentialEvents')
    const router = createRouter({ history: createMemoryHistory(), routes: [{ path: '/:pathMatch(.*)*', component: {} }] })
    cleanups.push(installSkillAtlasRefresh(router))
    await atlas.ensureLoaded()
    atlas.updateView('page', { selectedSkillId: 's', query: 'Skill', focusedFieldId: 'field', camera: { x: 8, y: 4, zoom: 2 } })
    atlas.updateView('modal', { camera: { x: 1, y: 2, zoom: 0.5 } })
    atlas.setSkillsTab('graph')
    credentials = [vc(4)]
    announceCredentialsChanged()
    await flushPromises()
    const remounted = useSkillAtlas()
    expect(remounted.model.value?.nodes[0]?.earnedBloom?.rank).toBe(4)
    expect(remounted.pageView.value).toMatchObject({ selectedSkillId: 's', query: 'Skill', focusedFieldId: 'field', camera: { x: 8, y: 4, zoom: 2 } })
    expect(remounted.modalView.value.camera).toEqual({ x: 1, y: 2, zoom: 0.5 })
    expect(remounted.skillsTab.value).toBe('graph')
    atlas.resetView('page')
    expect(atlas.pageView.value.camera).toBeNull()
    expect(atlas.modalView.value.camera).toEqual({ x: 1, y: 2, zoom: 0.5 })
  })
  it('discards hidden feedback and consumes or expires visible improvements without replay on remount', async () => {
    vi.useFakeTimers()
    const { atlas } = await setup()
    await atlas.ensureLoaded()
    credentials = [vc(2)]
    await atlas.refresh('credentials')
    expect(atlas.feedback.value).toBeNull()
    const deactivate = atlas.activateSurface('page')
    cleanups.push(deactivate)
    credentials = [vc(3)]
    await atlas.refresh('credentials')
    expect(atlas.feedback.value?.improvedIds).toEqual(['s'])
    const revision = atlas.feedback.value!.revision
    atlas.acknowledgeFeedback(revision)
    expect(atlas.feedback.value).toBeNull()
    credentials = [vc(4)]
    await atlas.refresh('credentials')
    atlas.acknowledgeFeedback(revision)
    expect(atlas.feedback.value?.improvedIds).toEqual(['s'])
    await vi.advanceTimersByTimeAsync(3000)
    expect(atlas.feedback.value).toBeNull()
    credentials = [vc(5)]
    await atlas.refresh('credentials')
    expect(atlas.feedback.value).not.toBeNull()
    deactivate()
    cleanups.push(atlas.activateSurface('page'))
    expect(atlas.feedback.value).toBeNull()
    credentials = []
    await atlas.refresh('credentials')
    expect(atlas.model.value?.nodes[0]?.status).toBe('ready')
    expect(atlas.feedback.value).toBeNull()
    credentials = [vc(null)]
    await atlas.refresh('credentials')
    expect(atlas.feedback.value?.earnedIds).toEqual(['s'])
    atlas.acknowledgeFeedback(atlas.feedback.value!.revision)
    credentials = [vc(1)]
    await atlas.refresh('credentials')
    expect(atlas.model.value?.nodes[0]?.earnedBloom?.rank).toBe(1)
    expect(atlas.feedback.value).toBeNull()
  })
  it('assigns each improvement to the foreground surface and discards it when that surface closes', async () => {
    const { atlas } = await setup()
    await atlas.ensureLoaded()
    cleanups.push(atlas.activateSurface('page'))
    atlas.setModalOpen(true)
    const closeModal = atlas.activateSurface('modal')
    cleanups.push(closeModal)
    credentials = [vc(2)]
    await atlas.refresh('credentials')
    expect(atlas.feedbackFor('modal')?.improvedIds).toEqual(['s'])
    expect(atlas.feedbackFor('page')).toBeNull()
    closeModal()
    atlas.setModalOpen(false)
    expect(atlas.feedbackFor('page')).toBeNull()
    credentials = [vc(3)]
    await atlas.refresh('credentials')
    expect(atlas.feedbackFor('page')?.improvedIds).toEqual(['s'])
    expect(atlas.feedbackFor('modal')).toBeNull()
  })
  it('does not replay atlas feedback when a course celebration closes', async () => {
    const { atlas } = await setup()
    const completion = (await import('./useCourseCompletion')).useCourseCompletion()
    await atlas.ensureLoaded()
    cleanups.push(atlas.activateSurface('page'), completion.close)
    await completion.open({ courseTitle: 'Course', courseId: 'course', skillIds: [], credentialIds: [], txHash: null })
    credentials = [vc(2)]
    await atlas.refresh('credentials')
    expect(atlas.feedbackFor('page')).toBeNull()
    completion.close()
    expect(atlas.feedbackFor('page')).toBeNull()
    await atlas.refresh('credentials')
    expect(atlas.feedbackFor('page')).toBeNull()
  })
  it('invalidates a snapshot on identity changes without treating profile display edits as a new identity', async () => {
    const { atlas, profiles } = await setup()
    await atlas.ensureLoaded()
    atlas.updateView('page', { selectedSkillId: 's' })
    ipc.mockImplementation((command, args) => command === 'get_profile' ? Promise.resolve({ ...identity, display_name: 'Renamed' }) : backend(command, args))
    await profiles.refreshActiveIdentity()
    expect(atlas.pageView.value.selectedSkillId).toBe('s')
    ipc.mockImplementation((command, args) => command === 'get_profile' ? Promise.resolve({ ...identity, stake_address: 'stake-b' }) : backend(command, args))
    await profiles.refreshActiveIdentity()
    expect(atlas.model.value).toBeNull()
    expect(atlas.pageView.value.selectedSkillId).toBeNull()
    did = 'did:b'
    credentials = [vc(5, 'did:b')]
    await atlas.ensureLoaded()
    expect(atlas.model.value?.nodes[0]?.earnedBloom?.rank).toBe(5)
    expect(atlas.feedback.value).toBeNull()
  })
  it('coalesces concurrent loads and queues one trailing forced refresh that all callers await', async () => {
    const { atlas } = await setup()
    const first = deferred<VerifiableCredential[]>()
    const trailing = deferred<VerifiableCredential[]>()
    let reads = 0
    ipc.mockImplementation((command, args) => {
      if (command === 'list_credentials') return ++reads === 1 ? first.promise : trailing.promise
      return backend(command, args)
    })
    const initial = atlas.ensureLoaded()
    const duplicate = atlas.ensureLoaded()
    await flushPromises()
    expect(reads).toBe(1)
    let finished = false
    const forced = atlas.refresh('credentials').then(() => { finished = true })
    const forcedAgain = atlas.refresh('retry')
    first.resolve([vc(1)])
    await flushPromises()
    expect(reads).toBe(2)
    expect(finished).toBe(false)
    expect(atlas.refreshing.value).toBe(true)
    trailing.resolve([vc(4)])
    await Promise.all([initial, duplicate, forced, forcedAgain])
    expect(atlas.model.value?.nodes[0]?.earnedBloom?.rank).toBe(4)
    expect(atlas.refreshing.value).toBe(false)
    expect(reads).toBe(2)
  })
  it('shares navigation/event subscriptions until the last owner leaves and ignores cancelled navigation', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(0)
    const { atlas } = await setup()
    const { installSkillAtlasRefresh } = await import('./useSkillAtlas')
    const { announceCredentialsChanged } = await import('./credentialEvents')
    const router = createRouter({ history: createMemoryHistory(), routes: [{ path: '/:pathMatch(.*)*', component: {} }] })
    router.beforeEach(to => to.path !== '/blocked')
    const stopOne = installSkillAtlasRefresh(router)
    const stopTwo = installSkillAtlasRefresh(router)
    cleanups.push(stopOne, stopTwo)
    await atlas.ensureLoaded()
    credentials = [vc(2)]
    await router.push('/first')
    await flushPromises()
    expect(atlas.model.value?.nodes[0]?.earnedBloom?.rank).toBe(2)
    stopOne()
    credentials = [vc(3)]
    await router.push('/rapid')
    await flushPromises()
    expect(atlas.model.value?.nodes[0]?.earnedBloom?.rank).toBe(2)
    vi.setSystemTime(1600)
    await router.push('/blocked')
    await flushPromises()
    expect(atlas.model.value?.nodes[0]?.earnedBloom?.rank).toBe(2)
    await router.push('/later')
    await flushPromises()
    expect(atlas.model.value?.nodes[0]?.earnedBloom?.rank).toBe(3)
    credentials = [vc(4)]
    announceCredentialsChanged()
    await flushPromises()
    expect(atlas.model.value?.nodes[0]?.earnedBloom?.rank).toBe(4)
    stopTwo()
    credentials = [vc(5)]
    announceCredentialsChanged()
    vi.setSystemTime(3200)
    await router.push('/detached')
    await flushPromises()
    expect(atlas.model.value?.nodes[0]?.earnedBloom?.rank).toBe(4)
  })
  it('drops feedback when the document becomes hidden instead of replaying it on return', async () => {
    const { atlas } = await setup()
    const { installSkillAtlasRefresh } = await import('./useSkillAtlas')
    const router = createRouter({ history: createMemoryHistory(), routes: [] })
    cleanups.push(installSkillAtlasRefresh(router), atlas.activateSurface('page'))
    await atlas.ensureLoaded()
    credentials = [vc(3)]
    await atlas.refresh('credentials')
    expect(atlas.feedback.value?.improvedIds).toEqual(['s'])
    const visibility = vi.spyOn(document, 'visibilityState', 'get').mockReturnValue('hidden')
    document.dispatchEvent(new Event('visibilitychange'))
    expect(atlas.feedback.value).toBeNull()
    credentials = [vc(4)]
    await atlas.refresh('credentials')
    expect(atlas.feedback.value).toBeNull()
    visibility.mockReturnValue('visible')
    document.dispatchEvent(new Event('visibilitychange'))
    expect(atlas.feedback.value).toBeNull()
  })
  it('retains the last good snapshot on refresh failure and recovers from an initial failure', async () => {
    const { atlas } = await setup()
    ipc.mockImplementation((command, args) => command === 'get_local_did' ? Promise.reject(new Error('DID unavailable')) : backend(command, args))
    await atlas.ensureLoaded()
    expect(atlas.phase.value).toBe('error')
    expect(atlas.error.value).toBe('DID unavailable')
    expect(atlas.model.value).toBeNull()
    ipc.mockImplementation(backend)
    await atlas.refresh('retry')
    expect(atlas.phase.value).toBe('ready')
    expect(atlas.error.value).toBeNull()
    atlas.updateView('page', { selectedSkillId: 's' })
    ipc.mockImplementation((command, args) => command === 'list_credentials' ? Promise.reject(new Error('Offline')) : backend(command, args))
    await atlas.refresh('credentials')
    expect(atlas.phase.value).toBe('ready')
    expect(atlas.model.value?.nodes[0]?.earnedBloom?.rank).toBe(1)
    expect(atlas.pageView.value.selectedSkillId).toBe('s')
    expect(atlas.error.value).toBe('Offline')
    expect(atlas.refreshing.value).toBe(false)
    ipc.mockImplementation(backend)
    credentials = []
    await atlas.refresh('retry')
    expect(atlas.model.value?.nodes[0]?.status).toBe('ready')
    expect(atlas.error.value).toBeNull()
  })
  it('shows Ready without a DID and rejects foreign achievements even if the listing includes them', async () => {
    const { atlas } = await setup()
    did = null
    await atlas.ensureLoaded()
    expect(atlas.model.value?.counts).toEqual({ total: 1, earned: 0, ready: 1 })
    did = 'did:a'
    credentials = [vc(5, 'did:b')]
    await atlas.refresh('credentials')
    expect(atlas.model.value?.nodes[0]?.status).toBe('ready')
    credentials = [vc(null)]
    await atlas.refresh('credentials')
    expect(atlas.model.value?.nodes[0]?.status).toBe('earned')
    expect(atlas.model.value?.nodes[0]?.earnedBloom).toBeNull()
  })
  it('rejects stale errors and queued work without altering a new profile load', async () => {
    const { atlas, profiles } = await setup()
    const oldCredentials = deferred<VerifiableCredential[]>()
    const newCredentials = deferred<VerifiableCredential[]>()
    let reads = 0
    ipc.mockImplementation((command, args) => {
      if (command === 'list_credentials') return ++reads === 1 ? oldCredentials.promise : newCredentials.promise
      return backend(command, args)
    })
    const oldLoad = atlas.ensureLoaded()
    await flushPromises()
    const queuedOld = atlas.refresh('credentials')
    await profiles.lockProfile()
    did = 'did:b'
    await profiles.unlockProfile('profile-b', 'password')
    await flushPromises()
    oldCredentials.reject(new Error('Old session failed'))
    await Promise.all([oldLoad, queuedOld])
    expect(atlas.phase.value).toBe('loading')
    expect(atlas.error.value).toBeNull()
    expect(atlas.model.value).toBeNull()
    newCredentials.resolve([vc(3, 'did:b')])
    await atlas.ensureLoaded()
    expect(atlas.model.value?.nodes[0]?.earnedBloom?.rank).toBe(3)
    expect(atlas.phase.value).toBe('ready')
    expect(atlas.feedback.value).toBeNull()
    expect(reads).toBe(2)
  })
  it('stores a camera snapshot independent of the caller and clears all session controls on reset', async () => {
    const { atlas } = await setup()
    await atlas.ensureLoaded()
    const camera = { x: 8, y: 4, zoom: 2 }
    atlas.updateView('page', { camera, query: 'Skill', selectedSkillId: 's' })
    camera.x = 100
    expect(atlas.pageView.value.camera?.x).toBe(8)
    atlas.updateView('modal', { query: 'modal', unassignedFocused: true })
    atlas.setSkillsTab('graph')
    atlas.setModalOpen(true)
    atlas.reset()
    expect(atlas.model.value).toBeNull()
    expect(atlas.pageView.value).toMatchObject({ camera: null, query: '', selectedSkillId: null })
    expect(atlas.modalView.value).toMatchObject({ query: '', unassignedFocused: false })
    expect(atlas.skillsTab.value).toBe('credentials')
    expect(atlas.modalOpen.value).toBe(false)
    expect(atlas.feedback.value).toBeNull()
  })
  it('clears private data synchronously before delayed lock hooks and rejects a late DID lookup', async () => {
    const { useProfiles, onProfileLocked } = await import('./useProfiles')
    const barrier = deferred<void>()
    cleanups.push(onProfileLocked(() => barrier.promise))
    const { useSkillAtlas } = await import('./useSkillAtlas')
    const profiles = useProfiles()
    await profiles.initialize()
    const atlas = useSkillAtlas()
    await atlas.ensureLoaded()
    const pendingDid = deferred<string>()
    ipc.mockImplementation((command, args) => command === 'get_local_did' ? pendingDid.promise : backend(command, args))
    const pending = atlas.refresh('credentials')
    const locking = profiles.lockProfile()
    expect(atlas.model.value).toBeNull()
    expect(atlas.phase.value).toBe('idle')
    pendingDid.resolve('did:a')
    await pending
    expect(atlas.model.value).toBeNull()
    expect(atlas.phase.value).toBe('idle')
    expect(atlas.error.value).toBeNull()
    barrier.resolve()
    await locking
  })
})
