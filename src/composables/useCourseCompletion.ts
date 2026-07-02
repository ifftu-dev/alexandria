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

interface CompletionPayload {
  courseTitle: string
  courseId: string
  skillIds: string[]
  txHash: string | null
  credentialIds: string[]
  isTutorial?: boolean
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

let pollTimer: ReturnType<typeof setInterval> | null = null
let revealTimers: ReturnType<typeof setTimeout>[] = []
let skillCache: SkillInfo[] | null = null

const REVEAL_INTERVAL_MS = 450

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
    for (const t of revealTimers) clearTimeout(t)
    revealTimers = []
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
  function revealBatch(hasAnchor: boolean) {
    items.value.forEach((_, i) => {
      const t = setTimeout(() => {
        const it = items.value[i]
        if (it) it.status = 'minted'
        const allMinted = items.value.every((x) => x.status === 'minted')
        if (allMinted) mintStage.value = hasAnchor ? 'anchoring' : 'issued'
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
    items.value = []
    isOpen.value = true

    const hasAnchor = !!p.txHash // non-empty tx → on-chain anchor in flight

    if (p.credentialIds.length === 0 && !hasAnchor) {
      mintStage.value = 'unavailable'
      return
    }

    mintStage.value = 'minting'
    items.value = await resolveItems(p.credentialIds)

    if (items.value.length === 0) {
      // Anchored but locals not resolvable yet — fall back to polling.
      mintStage.value = hasAnchor ? 'anchoring' : 'issued'
    } else {
      revealBatch(hasAnchor)
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
    open,
    close,
  }
}
