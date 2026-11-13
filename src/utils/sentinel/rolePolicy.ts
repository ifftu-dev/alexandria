// A sponsor role's integrity minimums as the policy form holds them, and as
// the issuance policy stores them. Empty means no minimum.
import type { IssuancePolicy, ProbePolicy, TrustProfile, VerificationRung } from '@/types'

export const TRUST_PROFILES: TrustProfile[] = ['core', 'attested', 'camera', 'full']
export const VERIFICATION_RUNGS: VerificationRung[] = ['process', 'probed', 'consistent', 'live']
export const PROBE_POLICIES: Exclude<ProbePolicy, 'none'>[] = ['reseed', 'authored', 'plugin', 'llm']

export interface RoleIntegrityChoice {
  min_trust_profile: TrustProfile | ''
  min_verification_rung: VerificationRung | ''
  probe_policy: ProbePolicy | ''
  /** The saved model connection that writes `llm` follow-ups; empty otherwise. */
  probe_connection_id: string
}

type IntegrityPolicyFields = Pick<IssuancePolicy, 'min_trust_profile' | 'min_verification_rung' | 'probe_policy' | 'probe_connection_id'>

/** A form with no minimums and no follow-ups. */
export function noIntegrityMinimums(): RoleIntegrityChoice {
  return { min_trust_profile: '', min_verification_rung: '', probe_policy: '', probe_connection_id: '' }
}

/** False while `llm` follow-ups are chosen without a model connection to write them. */
export function integrityChoiceReady(choice: RoleIntegrityChoice): boolean {
  return choice.probe_policy !== 'llm' || choice.probe_connection_id !== ''
}

/** The issuance policy fields for `choice`, leaving out what is unset. */
export function integrityPolicyFields(choice: RoleIntegrityChoice): IntegrityPolicyFields {
  const fields: IntegrityPolicyFields = {}
  if (choice.min_trust_profile) fields.min_trust_profile = choice.min_trust_profile
  if (choice.min_verification_rung) fields.min_verification_rung = choice.min_verification_rung
  if (choice.probe_policy) fields.probe_policy = choice.probe_policy
  if (choice.probe_policy === 'llm' && choice.probe_connection_id) fields.probe_connection_id = choice.probe_connection_id
  return fields
}
