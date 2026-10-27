import { computed, readonly, ref, watch } from 'vue'
import type { Router } from 'vue-router'

import { useLocalApi } from '@/composables/useLocalApi'
import { onProfileLocked, onProfileReady, useProfiles } from '@/composables/useProfiles'
import { CREDENTIALS_CHANGED } from '@/composables/credentialEvents'
import type { SkillGraphEdge, SkillInfo, SubjectFieldInfo, VerifiableCredential } from '@/types'
import { buildSkillAtlas, type AtlasModel, type BloomRank } from '@/utils/skillAtlas'

export type AtlasSurface = 'page' | 'modal'
export type AtlasTab = 'credentials' | 'graph' | 'browse'
export type AtlasPhase = 'idle' | 'loading' | 'ready' | 'error'
export type AtlasRefreshReason = 'navigation' | 'credentials' | 'retry'

export interface AtlasCamera { x: number; y: number; zoom: number }
export interface AtlasViewState {
  selectedSkillId: string | null
  focusedFieldId: string | null
  unassignedFocused: boolean
  query: string
  bloomEmphasis: BloomRank | null
  neighborhood: 'none' | 'prerequisites' | 'dependents'
  neighborhoodDepth: 1 | 2
  camera: AtlasCamera | null
  inspectorCollapsed: boolean
}
export interface AtlasFeedback {
  revision: number
  earnedIds: readonly string[]
  improvedIds: readonly string[]
}

const emptyView = (): AtlasViewState => ({
  selectedSkillId: null, focusedFieldId: null, unassignedFocused: false, query: '', bloomEmphasis: null,
  neighborhood: 'none', neighborhoodDepth: 1, camera: null, inspectorCollapsed: false,
})
const model = ref<AtlasModel | null>(null)
const phase = ref<AtlasPhase>('idle')
const error = ref<string | null>(null)
const refreshing = ref(false)
const pageView = ref<AtlasViewState>(emptyView())
const modalView = ref<AtlasViewState>(emptyView())
const skillsTab = ref<AtlasTab>('credentials')
const modalOpen = ref(false)
const feedback = ref<AtlasFeedback | null>(null)
const isLoading = computed(() => phase.value === 'loading')

let generation = 0
let feedbackRevision = 0
let baselineEstablished = false
let inFlight: Promise<void> | null = null
let queuedForced = false
let lastNavigationAt = -Infinity
let installedBridges = 0
const routerBridges = new Map<Router, { owners: number; remove: () => void }>()
const activeSurfaces = new Map<symbol, AtlasSurface>()
let feedbackTimer: ReturnType<typeof setTimeout> | undefined

function clearFeedback(): void {
  if (feedbackTimer !== undefined) clearTimeout(feedbackTimer)
  feedbackTimer = undefined
  feedback.value = null
}

export function activateSurface(surface: AtlasSurface): () => void {
  const token = Symbol(surface)
  clearFeedback()
  activeSurfaces.set(token, surface)
  return () => {
    activeSurfaces.delete(token)
    if (activeSurfaces.size === 0) clearFeedback()
  }
}

function current(generationAtStart: number, profileId: string | null): boolean {
  const profiles = useProfiles()
  return generationAtStart === generation && profileId !== null && profiles.activeProfileId.value === profileId && !profiles.isLockBlocked.value
}

function clearProfileState(): void {
  generation++
  inFlight = null
  lastNavigationAt = -Infinity
  model.value = null
  phase.value = 'idle'
  error.value = null
  refreshing.value = false
  pageView.value = emptyView()
  modalView.value = emptyView()
  skillsTab.value = 'credentials'
  modalOpen.value = false
  clearFeedback()
  baselineEstablished = false
  queuedForced = false
}

async function load(): Promise<void> {
  const profiles = useProfiles()
  const profileId = profiles.activeProfileId.value
  if (!profileId || profiles.isLockBlocked.value) return
  const ticket = generation
  const hadModel = model.value !== null
  refreshing.value = hadModel
  if (!hadModel) phase.value = 'loading'
  error.value = null
  const { invoke } = useLocalApi()
  try {
    const did = await invoke<string | null>('get_local_did')
    if (!current(ticket, profileId)) return
    const [skills, edges, credentials, subjectFields] = await Promise.all([
      invoke<SkillInfo[]>('list_skills'),
      invoke<SkillGraphEdge[]>('list_skill_graph_edges'),
      did ? invoke<VerifiableCredential[]>('list_credentials', { subject: did, activeOnly: true }) : Promise.resolve([]),
      invoke<SubjectFieldInfo[]>('list_subject_fields'),
    ])
    if (!current(ticket, profileId)) return
    const nextModel = buildSkillAtlas({ skills, edges, activeCredentials: credentials, subjectDid: did, subjectFields })
    const improvedIds: string[] = []
    const newlyEarned: string[] = []
    clearFeedback()
    if (baselineEstablished && model.value && activeSurfaces.size > 0 && (typeof document === 'undefined' || document.visibilityState !== 'hidden')) {
      for (const node of nextModel.nodes) {
        const previous = model.value.nodes.find((item) => item.skill.id === node.skill.id)
        if (node.status === 'earned' && previous?.status !== 'earned') newlyEarned.push(node.skill.id)
        if (node.earnedBloom && previous?.earnedBloom && node.earnedBloom.rank > previous.earnedBloom.rank) improvedIds.push(node.skill.id)
      }
      if (newlyEarned.length || improvedIds.length) {
        feedback.value = { revision: ++feedbackRevision, earnedIds: newlyEarned.sort(), improvedIds: improvedIds.sort() }
        feedbackTimer = setTimeout(clearFeedback, 3000)
      }
    }
    model.value = nextModel
    baselineEstablished = true
    phase.value = 'ready'
    error.value = null
  } catch (cause) {
    if (!current(ticket, profileId)) return
    phase.value = hadModel ? 'ready' : 'error'
    error.value = cause instanceof Error ? cause.message : String(cause)
  } finally {
    if (current(ticket, profileId)) refreshing.value = false
  }
}

async function refresh(reason: AtlasRefreshReason): Promise<void> {
  if (reason === 'navigation') {
    const now = Date.now()
    if (now - lastNavigationAt < 1500) return
    lastNavigationAt = now
  }
  if (inFlight) {
    if (reason !== 'navigation') queuedForced = true
    await inFlight
    return
  }
  const ticket = generation
  const operation = (async () => {
    do {
      queuedForced = false
      await load()
    } while (ticket === generation && queuedForced)
  })()
  inFlight = operation
  try { await operation } finally { if (inFlight === operation) inFlight = null }
}

async function ensureLoaded(): Promise<void> {
  if (model.value || phase.value === 'loading') return inFlight ?? Promise.resolve()
  await refresh('retry')
}

function updateView(surface: AtlasSurface, patch: Partial<AtlasViewState>): void {
  const target = surface === 'page' ? pageView : modalView
  target.value = { ...target.value, ...patch, ...(patch.camera ? { camera: { ...patch.camera } } : {}) }
}

function resetView(surface: AtlasSurface): void {
  ;(surface === 'page' ? pageView : modalView).value = emptyView()
}

function setSkillsTab(tab: AtlasTab): void { skillsTab.value = tab }
function setModalOpen(open: boolean): void { modalOpen.value = open }
function acknowledgeFeedback(revision: number): void {
  if (feedback.value?.revision === revision) clearFeedback()
}

const profiles = useProfiles()
watch([profiles.activeProfileId, profiles.isLockBlocked, () => profiles.activeIdentity.value?.stake_address], clearProfileState, { flush: 'sync' })
onProfileReady(() => { void ensureLoaded() })
onProfileLocked(clearProfileState)

export function installSkillAtlasRefresh(router: Router): () => void {
  installedBridges++
  if (installedBridges === 1 && typeof window !== 'undefined') {
    window.addEventListener(CREDENTIALS_CHANGED, credentialsChanged)
    document.addEventListener('visibilitychange', visibilityChanged)
  }
  let bridge = routerBridges.get(router)
  if (!bridge) {
    bridge = {
      owners: 0,
      remove: router.afterEach((_to, _from, failure) => {
        if (!failure) void refresh('navigation')
      }),
    }
    routerBridges.set(router, bridge)
  }
  bridge.owners++
  let disposed = false
  return () => {
    if (disposed) return
    disposed = true
    bridge.owners--
    if (bridge.owners === 0) {
      bridge.remove()
      routerBridges.delete(router)
    }
    installedBridges = Math.max(0, installedBridges - 1)
    if (installedBridges === 0 && typeof window !== 'undefined') {
      window.removeEventListener(CREDENTIALS_CHANGED, credentialsChanged)
      document.removeEventListener('visibilitychange', visibilityChanged)
      clearFeedback()
    }
  }
}

function visibilityChanged(): void {
  if (document.visibilityState === 'hidden') clearFeedback()
}

function credentialsChanged(): void { void refresh('credentials') }

export function useSkillAtlas() {
  return {
    model: readonly(model), phase: readonly(phase), error: readonly(error), refreshing: readonly(refreshing), isLoading: readonly(isLoading),
    pageView: readonly(pageView), modalView: readonly(modalView), skillsTab: readonly(skillsTab), modalOpen: readonly(modalOpen), feedback: readonly(feedback),
    ensureLoaded, refresh, activateSurface, updateView, resetView, setSkillsTab, setModalOpen, acknowledgeFeedback, reset: clearProfileState,
  }
}
