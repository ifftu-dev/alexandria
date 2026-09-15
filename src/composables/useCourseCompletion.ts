import { ref, readonly } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { onProfileLocked } from '@/composables/useProfiles'
import { getProfileSessionToken } from '@/composables/profileSession'
import { extractSkillClaim, type VerifiableCredential, type SkillInfo, type CompletionWitnessStatus, type CompletionWitnessState } from '@/types'
import { classNameOf } from '@/components/credential/credentialKind'

/**
 * Global "course completed" celebration + credential-mint tracker.
 *
 * Module-singleton refs (mirrors useSkillGraphState) so the Player can fire the
 * celebration and then navigate away while the modal — mounted once in
 * AppLayout — stays up and keeps animating.
 *
 * Local self-claims are already durable when opened. Their reveal is separate
 * from the optional, durable background witness; elapsed time never implies
 * chain confirmation. Instructor endorsements are issued separately.
 */
export type MintStage = 'minting' | 'issued' | 'unavailable'

export interface MintItem {
  id: string
  /** Skill name, or a label for the witnessed completion VC. */
  label: string
  /** Credential class name (drives the icon/colour). */
  kind: string
  status: 'minting' | 'minted'
}

/** A gradeable element the learner hasn't passed yet — shown in the modal
 *  when a credential can't be earned. */
export interface UnmetElement {
  element_id: string
  title: string
  element_type: string
  /** Best score so far (0..1), or null if never attempted. */
  best_score: number | null
  /** Passing score (0..1). */
  required_score: number
}

interface CompletionPayload {
  courseTitle: string
  courseId: string
  skillIds: string[]
  txHash: string | null
  credentialIds: string[]
  claimId?: string
  witnessStatus?: CompletionWitnessStatus
  isTutorial?: boolean
  unmetElements?: UnmetElement[]
}

const isOpen = ref(false)
const courseTitle = ref('')
const courseId = ref('')
const isTutorial = ref(false)
const txHash = ref<string | null>(null)
const witnessStatus = ref<CompletionWitnessStatus>('not_requested')
const mintStage = ref<MintStage>('minting')
const items = ref<MintItem[]>([])
/** First credential id — the target of "View credential" when unambiguous. */
const primaryCredentialId = ref<string | null>(null)
/** Gradeable elements not yet passed — populated when no credential is earned. */
const unmetElements = ref<UnmetElement[]>([])
/** Live elapsed time since the mint started (ms). */
const elapsedMs = ref(0)
/** Estimated time remaining until the batch finishes (ms). */
const etaMs = ref(0)
/** Local credential reveal progress only; not chain confirmation progress. */
const progressPct = ref(0)

let pollTimer: ReturnType<typeof setTimeout> | null = null
let tickTimer: ReturnType<typeof setInterval> | null = null
let revealTimers: ReturnType<typeof setTimeout>[] = []
let generation = 0

const REVEAL_INTERVAL_MS = 450
function stopTimers() {
  if (pollTimer) clearTimeout(pollTimer)
  if (tickTimer) clearInterval(tickTimer)
  for (const timer of revealTimers) clearTimeout(timer)
  pollTimer = null
  tickTimer = null
  revealTimers = []
}

onProfileLocked(() => {
  generation += 1
  stopTimers()
  isOpen.value = false
  courseTitle.value = ''
  courseId.value = ''
  isTutorial.value = false
  txHash.value = null
  witnessStatus.value = 'not_requested'
  mintStage.value = 'unavailable'
  primaryCredentialId.value = null
  items.value = []
  unmetElements.value = []
  elapsedMs.value = 0
  etaMs.value = 0
  progressPct.value = 0
})

const awaitingWitness = (status: CompletionWitnessStatus) =>
  status === 'pending' || status === 'submitted' || status === 'outcome_unknown'

export function useCourseCompletion() {
  const { invoke } = useLocalApi()

  async function skillNameMap(): Promise<Map<string, string>> {
    const skills = (await invoke<SkillInfo[]>('list_skills', {}).catch(() => [])) ?? []
    return new Map(skills.map((skill) => [skill.id, skill.name]))
  }

  /** Freeze the ETA clock (keeps the final elapsed time on screen). */
  function stopTicker() {
    if (tickTimer) {
      clearInterval(tickTimer)
      tickTimer = null
    }
    etaMs.value = 0
  }

  /** Drive the local reveal's elapsed time, ETA and progress bar. */
  function startTicker(startTs: number) {
    tickTimer = setInterval(() => {
      elapsedMs.value = Date.now() - startTs

      const total = items.value.length
      const minted = items.value.filter((x) => x.status === 'minted').length
      const localFrac = total ? minted / total : mintStage.value === 'issued' ? 1 : 0

      if (mintStage.value === 'issued') {
        progressPct.value = 100
        etaMs.value = 0
        stopTicker()
        return
      }
      if (mintStage.value === 'unavailable') {
        stopTicker()
        return
      }

      progressPct.value = Math.round(localFrac * 100)
      etaMs.value = items.value.filter((x) => x.status !== 'minted').length * REVEAL_INTERVAL_MS
    }, 100)
  }

  function labelFor(c: VerifiableCredential, skills: Map<string, string>): string {
    const skill = extractSkillClaim(c.credentialSubject)
    if (skill) return skills.get(skill.skillId) ?? skill.skillId
    // Witnessed completion VC carries a custom course_completion claim.
    return 'Course completion'
  }

  /** Build the batch list from the issued credential ids. */
  async function resolveItems(ids: string[]): Promise<MintItem[]> {
    if (!ids.length) return []
    const [creds, skills] = await Promise.all([
      invoke<VerifiableCredential[]>('list_credentials', {}).catch(() => []),
      skillNameMap(),
    ])
    const byId = new Map((creds ?? []).filter((c) => c.id).map((c) => [c.id as string, c]))
    const out: MintItem[] = []
    for (const id of ids) {
      const c = byId.get(id)
      if (!c) continue
      out.push({
        id,
        label: labelFor(c, skills),
        kind: classNameOf(c.type),
        status: 'minting',
      })
    }
    return out
  }

  /** Flip each item to "minted" on a stagger; resolve the overall stage. */
  function revealBatch() {
    items.value.forEach((_, i) => {
      const t = setTimeout(() => {
        const it = items.value[i]
        if (it) it.status = 'minted'
        const allMinted = items.value.every((x) => x.status === 'minted')
        if (allMinted && mintStage.value === 'minting') {
          mintStage.value = 'issued'
        }
      }, REVEAL_INTERVAL_MS * (i + 1))
      revealTimers.push(t)
    })
  }

  async function open(p: CompletionPayload) {
    stopTimers()
    const current = ++generation
    const session = getProfileSessionToken()
    if (!session) return
    const active = () => current === generation && session === getProfileSessionToken() && isOpen.value
    courseTitle.value = p.courseTitle
    courseId.value = p.courseId
    isTutorial.value = !!p.isTutorial
    txHash.value = p.txHash
    witnessStatus.value = p.witnessStatus ?? 'not_requested'
    primaryCredentialId.value = p.credentialIds[0] ?? null
    unmetElements.value = p.unmetElements ?? []
    items.value = []
    elapsedMs.value = 0
    etaMs.value = 0
    progressPct.value = 0
    isOpen.value = true
    mintStage.value = p.credentialIds.length ? 'minting' : 'unavailable'

    // Poll one indexed local receipt, never the entire credential collection.
    // Recursive timeout avoids overlapping requests when the backend is busy.
    async function pollWitness() {
      if (!active() || !p.claimId || !awaitingWitness(witnessStatus.value)) return
      try {
        const state = await invoke<CompletionWitnessState>('get_completion_witness_status', { claimId: p.claimId })
        if (!active()) return
        witnessStatus.value = state.status
        txHash.value = state.tx_hash
      } catch {
        // A transient read failure does not imply submission or confirmation.
      }
      if (active() && awaitingWitness(witnessStatus.value)) {
        pollTimer = setTimeout(() => { void pollWitness() }, 5000)
      }
    }
    void pollWitness()
    if (!p.credentialIds.length) return
    startTicker(Date.now())
    const resolved = await resolveItems(p.credentialIds)
    if (!active()) return
    items.value = resolved
    if (items.value.length === 0) {
      mintStage.value = 'unavailable'
      stopTicker()
    } else {
      revealBatch()
    }
  }

  function close() {
    generation += 1
    isOpen.value = false
    stopTimers()
  }

  return {
    isOpen: readonly(isOpen),
    courseTitle: readonly(courseTitle),
    courseId: readonly(courseId),
    isTutorial: readonly(isTutorial),
    txHash: readonly(txHash),
    witnessStatus: readonly(witnessStatus),
    mintStage: readonly(mintStage),
    items: readonly(items),
    primaryCredentialId: readonly(primaryCredentialId),
    unmetElements: readonly(unmetElements),
    elapsedMs: readonly(elapsedMs),
    etaMs: readonly(etaMs),
    progressPct: readonly(progressPct),
    open,
    close,
  }
}
