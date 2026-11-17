// An organisation's issuance policy, as carried in a credential request, read
// as the requirements a candidate sees before starting, in the order the
// shared rule judges them (status, score, flags, assurance, floor, rung,
// probes). Also what this device cannot run of the policy's trust profile
// floor, so the candidate is told before an attempt that could never issue.
import type { DeviceSupport, IssuancePolicy, ProbePolicy, TrustProfile, VerificationRung } from '@/types'

export type Requirement =
  | { kind: 'clean' }
  | { kind: 'minIntegrity'; percent: number }
  | { kind: 'maxCritical'; count: number }
  | { kind: 'maxWarning'; count: number }
  | { kind: 'assurance'; level: string }
  | { kind: 'floor'; profile: TrustProfile }
  | { kind: 'rung'; rung: VerificationRung }
  | { kind: 'probes'; policy: Exclude<ProbePolicy, 'none'> }

/** What the policy asks for; empty when it asks for nothing. */
export function policyRequirements(policy: IssuancePolicy): Requirement[] {
  const out: Requirement[] = []
  if (policy.require_clean) out.push({ kind: 'clean' })
  if (typeof policy.min_integrity === 'number') {
    out.push({ kind: 'minIntegrity', percent: Math.round(policy.min_integrity * 100) })
  }
  if (typeof policy.max_critical === 'number') out.push({ kind: 'maxCritical', count: policy.max_critical })
  if (typeof policy.max_warning === 'number') out.push({ kind: 'maxWarning', count: policy.max_warning })
  if (policy.required_assurance_level) out.push({ kind: 'assurance', level: policy.required_assurance_level })
  if (policy.min_trust_profile && policy.min_trust_profile !== 'core') {
    out.push({ kind: 'floor', profile: policy.min_trust_profile })
  }
  if (policy.min_verification_rung && policy.min_verification_rung !== 'none') {
    out.push({ kind: 'rung', rung: policy.min_verification_rung })
  }
  if (policy.probe_policy && policy.probe_policy !== 'none') out.push({ kind: 'probes', policy: policy.probe_policy })
  return out
}

/** What is known about this device; `null` while unknown. */
export interface DeviceAbility {
  /** A camera is connected. */
  camera: boolean | null
  support: DeviceSupport | null
}

export type FloorBlocker = 'no_camera' | 'no_attestation'

export interface FloorReach {
  /** What this device lacks for the floor. Unknown capabilities never block. */
  blockers: FloorBlocker[]
  /** Device attestation is missing here but a paired phone can lend it. */
  pairPhone: boolean
}

const needsCamera = (floor: TrustProfile) => floor === 'camera' || floor === 'full'
const needsAttestation = (floor: TrustProfile) => floor === 'attested' || floor === 'full'

/** Whether this device can run the policy's minimum trust profile. */
export function floorReach(floor: TrustProfile | null | undefined, device: DeviceAbility): FloorReach {
  const blockers: FloorBlocker[] = []
  let pairPhone = false
  if (!floor) return { blockers, pairPhone }
  if (needsCamera(floor) && device.camera === false) blockers.push('no_camera')
  if (needsAttestation(floor) && device.support && !device.support.platform_attestation) {
    if (device.support.companion_phone) pairPhone = true
    else blockers.push('no_attestation')
  }
  return { blockers, pairPhone }
}

/** True when the floor asks for something a device may lack. */
export function floorNeedsDevice(floor: TrustProfile | null | undefined): boolean {
  return !!floor && (needsCamera(floor) || needsAttestation(floor))
}
