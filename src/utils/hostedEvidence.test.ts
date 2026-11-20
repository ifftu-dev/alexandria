// Who graded a credential and what the issuer verified, from the
// credential's own HostedAttemptEvidence entry (story 2).
import { describe, expect, it } from 'vitest'
import { gradedBy } from '@/utils/hostedEvidence'

const entry = {
  type: 'HostedAttemptEvidence', attemptId: 'hatt', answerCustody: 'issuer', gradedBy: 'issuer',
  score: 0.75, attestation: { outcome: 'verified_credential_bound', platform: 'ios' },
  verifiedAssuranceLevel: 'device_attested',
}

describe('gradedBy', () => {
  it('reads the issuer, its score and its verified level from a hosted attempt entry', () => {
    expect(gradedBy({ evidence: [entry] })).toEqual({ by: 'issuer', score: 0.75, level: 'device_attested' })
  })

  it('says the device graded when the credential names no hosted attempt', () => {
    expect(gradedBy({})).toEqual({ by: 'device', score: null, level: null })
    expect(gradedBy({ evidence: [{ type: 'AssessedCredentialEvidence' }] }))
      .toEqual({ by: 'device', score: null, level: null })
  })

  it('reads nothing from an entry that does not say the issuer graded', () => {
    expect(gradedBy({ evidence: [{ ...entry, gradedBy: 'device' }] }))
      .toEqual({ by: 'device', score: null, level: null })
    expect(gradedBy({ evidence: ['garbage', { ...entry, verifiedAssuranceLevel: 7 }] }))
      .toEqual({ by: 'issuer', score: 0.75, level: null })
  })
})
