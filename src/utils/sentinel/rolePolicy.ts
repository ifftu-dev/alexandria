// A sponsor role's integrity minimums as the policy form holds them, and as
// the issuance policy stores them. Empty means no minimum.
import type { IssuancePolicy, ProbePolicy, TrustProfile, VerificationRung } from '@/types'

export const TRUST_PROFILES: TrustProfile[] = ['core', 'attested', 'camera', 'full']
export const VERIFICATION_RUNGS: VerificationRung[] = ['process', 'probed', 'consistent', 'live']
export const PROBE_POLICIES: Exclude<ProbePolicy, 'none'>[] = ['reseed', 'authored', 'plugin']

export interface RoleIntegrityChoice {
  min_trust_profile: TrustProfile | ''
  min_verification_rung: VerificationRung | ''
  probe_policy: ProbePolicy | ''
}

/** The issuance policy fields for `choice`, leaving out what is unset. */
export function integrityPolicyFields(choice: RoleIntegrityChoice): Pick<IssuancePolicy, 'min_trust_profile' | 'min_verification_rung' | 'probe_policy'> {
  const fields: Pick<IssuancePolicy, 'min_trust_profile' | 'min_verification_rung' | 'probe_policy'> = {}
  if (choice.min_trust_profile) fields.min_trust_profile = choice.min_trust_profile
  if (choice.min_verification_rung) fields.min_verification_rung = choice.min_verification_rung
  if (choice.probe_policy) fields.probe_policy = choice.probe_policy
  return fields
}
