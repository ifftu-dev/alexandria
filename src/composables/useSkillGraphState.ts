import { ref } from 'vue'
import {
  extractSkillClaim,
  type SkillInfo,
  type SkillGraphEdge,
  type VerifiableCredential,
} from '@/types'

/**
 * Shared state for the sidebar skill graph widget.
 *
 * Data is loaded once by SidebarSkillGraph.vue, and the reactive refs
 * are shared across components via module-level singletons.
 *
 * Post-migration 040: earned state is derived from credentials
 * (a SkillClaim on the credentialSubject) for the subject == local
 * DID. The set of earned skill IDs is the distinct `skillId` across
 * those credentials.
 */

const skills = ref<SkillInfo[]>([])
const edges = ref<SkillGraphEdge[]>([])
const credentials = ref<VerifiableCredential[]>([])
const earnedSkillIds = ref<Set<string>>(new Set())
const earnedCount = ref(0)
const availableCount = ref(0)
const lockedCount = ref(0)
const totalCount = ref(0)
const loaded = ref(false)
const isModalOpen = ref(false)

export type SkillStatus = 'earned' | 'available' | 'locked'

export function useSkillGraphState() {
  return {
    skills,
    edges,
    credentials,
    earnedSkillIds,
    earnedCount,
    availableCount,
    lockedCount,
    totalCount,
    loaded,
    isModalOpen,
  }
}

/**
 * Reduce a credential list to the distinct set of skill IDs
 * represented by skill-kind credentials held by `subjectDid`
 * (or, when no filter is supplied, every skill claim in the list).
 */
export function earnedSkillIdsFromCredentials(
  creds: VerifiableCredential[],
  subjectDid?: string | null,
): Set<string> {
  const out = new Set<string>()
  for (const vc of creds) {
    if (subjectDid && vc.credentialSubject.id !== subjectDid) continue
    const claim = extractSkillClaim(vc.credentialSubject)
    if (!claim) continue
    out.add(claim.skillId)
  }
  return out
}

/**
 * For each skillId, pick the credential with the highest
 * `SkillClaim.level`. Used to render per-skill detail pages.
 */
export function highestLevelBySkill(
  creds: VerifiableCredential[],
  subjectDid?: string | null,
): Map<string, VerifiableCredential> {
  const best = new Map<string, VerifiableCredential>()
  for (const vc of creds) {
    if (subjectDid && vc.credentialSubject.id !== subjectDid) continue
    const claim = extractSkillClaim(vc.credentialSubject)
    if (!claim) continue
    const existing = best.get(claim.skillId)
    if (!existing) {
      best.set(claim.skillId, vc)
      continue
    }
    const existingClaim = extractSkillClaim(existing.credentialSubject)
    if (!existingClaim) continue
    if (claim.level > existingClaim.level) {
      best.set(claim.skillId, vc)
    }
  }
  return best
}
