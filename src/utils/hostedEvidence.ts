// Who computed a credential's score (story 2), read from the credential's
// own `HostedAttemptEvidence` entry: the issuer, with its score and the
// assurance level it verified, when the entry says the issuer graded; the
// candidate's device otherwise. Verification has already judged the entry
// (`custodyDefects`); this only reads it for display.
import type { VerifiedAssuranceLevel } from '@/types'

export interface GradedBy {
  by: 'issuer' | 'device'
  /** The issuer's score, a fraction in [0,1]; null when the device graded. */
  score: number | null
  /** The assurance level the issuer verified; null when the device graded. */
  level: VerifiedAssuranceLevel | null
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

const LEVELS: readonly VerifiedAssuranceLevel[] = ['local', 'companion_attested', 'device_attested']

function isLevel(value: unknown): value is VerifiedAssuranceLevel {
  return LEVELS.some(level => level === value)
}

export function gradedBy(credential: { evidence?: readonly unknown[] | null }): GradedBy {
  const entry = (credential.evidence ?? []).find(
    (e): e is Record<string, unknown> => isRecord(e) && e.type === 'HostedAttemptEvidence',
  )
  if (!entry || entry.gradedBy !== 'issuer' || entry.answerCustody !== 'issuer') {
    return { by: 'device', score: null, level: null }
  }
  return {
    by: 'issuer',
    score: typeof entry.score === 'number' ? entry.score : null,
    level: isLevel(entry.verifiedAssuranceLevel) ? entry.verifiedAssuranceLevel : null,
  }
}
