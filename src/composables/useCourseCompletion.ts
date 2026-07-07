import { ref } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { extractSkillClaim, type VerifiableCredential, type SkillInfo } from '@/types'
import { classNameOf } from '@/components/credential/credentialKind'

/**
 * Global "course completed" celebration + credential-mint tracker.
 *
 * Module-singleton refs (mirrors useSkillGraphState) so the Player can fire the
 * celebration and then navigate away while the modal — mounted once in
 * AppLayout — stays up and keeps animating.
 *
 * Completion issues a *batch* of credentials locally (per skill: the learner's
 * self-claim + the instructor attestation, plus a witnessed VC when anchored).
 * They already exist in `list_credentials` by the time we open, so the
 * per-credential "minting" is a staggered reveal for the achievement feel;
 * for an anchored completion the witnessed VC additionally polls on-chain.
 *
 * Mint stages:
 *   - minting     : revealing the batch, credential by credential.
 *   - anchoring   : locals revealed; waiting on the on-chain witness VC.
 *   - issued      : the whole batch is minted.
 *   - unavailable : nothing was issued (claim failed) — still worth a cheer.
 */
export type MintStage = 'minting' | 'anchoring' | 'issued' | 'unavailable'

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
  isTutorial?: boolean
  unmetElements?: UnmetElement[]
}

const isOpen = ref(false)
const courseTitle = ref('')
const courseId = ref('')
const isTutorial = ref(false)
const txHash = ref<string | null>(null)
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
/** Overall completion 0..100, spanning local minting AND on-chain anchoring. */
const progressPct = ref(0)

let pollTimer: ReturnType<typeof setInterval> | null = null
let tickTimer: ReturnType<typeof setInterval> | null = null
let revealTimers: ReturnType<typeof setTimeout>[] = []
let skillCache: SkillInfo[] | null = null
let hasAnchor = false
let anchorStartTs = 0

const REVEAL_INTERVAL_MS = 450
// Soft estimate for an on-chain anchor to confirm (Cardano block cadence).
const ANCHOR_ETA_MS = 60_000
// When a completion anchors on-chain, local minting fills this fraction of the
// bar and the anchoring phase fills the rest.
const LOCAL_SHARE = 0.5

export function useCourseCompletion() {
  const { invoke } = useLocalApi()

  async function skillNameMap(): Promise<Map<string, string>> {
    if (!skillCache) {
      skillCache = (await invoke<SkillInfo[]>('list_skills', {}).catch(() => [])) ?? []
    }
    return new Map(skillCache.map((s) => [s.id, s.name]))
  }

  function stopTimers() {
    if (pollTimer) {
      clearInterval(pollTimer)
      pollTimer = null
    }
    if (tickTimer) {
      clearInterval(tickTimer)
      tickTimer = null
    }
    for (const t of revealTimers) clearTimeout(t)
    revealTimers = []
  }

  /** Freeze the ETA clock (keeps the final elapsed time on screen). */
  function stopTicker() {
    if (tickTimer) {
      clearInterval(tickTimer)
      tickTimer = null
    }
    etaMs.value = 0
  }

  /** Drive the live elapsed / ETA readout + the phase-spanning progress bar. */
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

      if (!hasAnchor) {
        // Local-only: the bar is just the batch reveal.
        progressPct.value = Math.round(localFrac * 100)
        etaMs.value = items.value.filter((x) => x.status !== 'minted').length * REVEAL_INTERVAL_MS
        return
      }

      // Anchored: local minting fills LOCAL_SHARE, anchoring fills the rest
      // over ANCHOR_ETA_MS (creeping to 98% until the tx actually confirms).
      if (mintStage.value === 'anchoring') {
        const anchorElapsed = Date.now() - anchorStartTs
        const anchorFrac = Math.min(anchorElapsed / ANCHOR_ETA_MS, 0.98)
        progressPct.value = Math.round((LOCAL_SHARE + (1 - LOCAL_SHARE) * anchorFrac) * 100)
        etaMs.value = Math.max(0, ANCHOR_ETA_MS - anchorElapsed)
      } else {
        progressPct.value = Math.round(LOCAL_SHARE * localFrac * 100)
        const remaining = items.value.filter((x) => x.status !== 'minted').length
        etaMs.value = remaining * REVEAL_INTERVAL_MS + ANCHOR_ETA_MS
      }
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
          if (hasAnchor) {
            anchorStartTs = Date.now()
            mintStage.value = 'anchoring'
          } else {
            mintStage.value = 'issued'
          }
        }
      }, REVEAL_INTERVAL_MS * (i + 1))
      revealTimers.push(t)
    })
  }

  /** Poll for the witnessed VC carrying our tx hash (anchored path only). */
  async function checkAnchored(): Promise<boolean> {
    if (!txHash.value) return false
    const creds = (await invoke<VerifiableCredential[]>('list_credentials', {}).catch(() => [])) ?? []
    const hit = creds.find((c) => c.witness?.tx_hash === txHash.value)
    if (hit) {
      mintStage.value = 'issued'
      progressPct.value = 100
      etaMs.value = 0
      stopTimers()
      return true
    }
    return false
  }

  async function open(p: CompletionPayload) {
    stopTimers()
    courseTitle.value = p.courseTitle
    courseId.value = p.courseId
    isTutorial.value = !!p.isTutorial
    txHash.value = p.txHash
    primaryCredentialId.value = p.credentialIds[0] ?? null
    unmetElements.value = p.unmetElements ?? []
    items.value = []
    elapsedMs.value = 0
    etaMs.value = 0
    progressPct.value = 0
    anchorStartTs = 0
    hasAnchor = !!p.txHash // non-empty tx → on-chain anchor in flight
    isOpen.value = true

    if (p.credentialIds.length === 0 && !hasAnchor) {
      mintStage.value = 'unavailable'
      return
    }

    mintStage.value = 'minting'
    startTicker(Date.now())
    items.value = await resolveItems(p.credentialIds)

    if (items.value.length === 0) {
      // Anchored but locals not resolvable yet — jump straight to anchoring.
      if (hasAnchor) {
        anchorStartTs = Date.now()
        mintStage.value = 'anchoring'
      } else {
        mintStage.value = 'issued'
      }
    } else {
      revealBatch()
    }

    if (hasAnchor) {
      // Begin polling once the local reveal is underway.
      if (!(await checkAnchored())) {
        let tries = 0
        pollTimer = setInterval(() => {
          tries += 1
          void checkAnchored()
          if (mintStage.value === 'issued' || tries > 40) stopTimers() // ~3.5 min @ 5s
        }, 5000)
      }
    }
  }

  function close() {
    isOpen.value = false
    stopTimers()
  }

  return {
    isOpen,
    courseTitle,
    courseId,
    isTutorial,
    txHash,
    mintStage,
    items,
    primaryCredentialId,
    unmetElements,
    elapsedMs,
    etaMs,
    progressPct,
    open,
    close,
  }
}
