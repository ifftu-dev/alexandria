// What the learner is told before the first item: the trust profile the
// backend chose for the session, why the signals above it do not run, whether
// the sponsor's floor is met, and what would raise the profile next time.
// The backend decided already (`integrity_select_profile`); this only reads
// its reply.
import type { DeepReadonly } from 'vue'
import type {
  PolicyFloor,
  ProfileExclusionReason,
  ProfileSignal,
  SelectedProfile,
  TrustProfile,
} from '@/types'

/** Something the learner can change to reach a higher profile. */
export type RaiseStep =
  | 'attested_device'
  | 'connect_camera'
  | 'allow_camera'
  | 'faster_device'

export interface SignalNotice {
  signal: ProfileSignal
  reasons: ProfileExclusionReason[]
}

export interface ProfileNotice {
  profile: TrustProfile
  /** Signals the profile does not run, with what kept them out. */
  notRun: SignalNotice[]
  floor: Pick<PolicyFloor, 'required' | 'satisfied' | 'missing'>
  /** In the order the signals are listed; only what the floor misses when it is unmet. */
  raise: RaiseStep[]
}

const SIGNAL_REASONS: Record<ProfileSignal, ProfileExclusionReason[]> = {
  device_attestation: ['not_attested'],
  camera: ['no_camera', 'camera_not_consented', 'not_benchmarked', 'over_budget'],
}

const PROFILE_SIGNALS: Record<TrustProfile, ProfileSignal[]> = {
  core: [],
  attested: ['device_attestation'],
  camera: ['camera'],
  full: ['device_attestation', 'camera'],
}

function step(reason: ProfileExclusionReason): RaiseStep {
  switch (reason) {
    // Only the attempt's own attested device runs device attestation: a
    // paired phone is the companion level and raises nothing.
    case 'not_attested': return 'attested_device'
    case 'no_camera': return 'connect_camera'
    case 'camera_not_consented': return 'allow_camera'
    case 'not_benchmarked':
    case 'over_budget': return 'faster_device'
  }
}

export function profileNotice(selected: DeepReadonly<SelectedProfile>): ProfileNotice {
  const notRun = selected.not_run.map((signal): SignalNotice => {
    const reasons: ProfileExclusionReason[] = []
    for (const exclusion of selected.excluded) {
      if (!PROFILE_SIGNALS[exclusion.profile].includes(signal)) continue
      for (const reason of exclusion.reasons) {
        if (SIGNAL_REASONS[signal].includes(reason) && !reasons.includes(reason)) reasons.push(reason)
      }
    }
    return { signal, reasons }
  })
  const { required, satisfied, missing } = selected.policy_floor
  const raise: RaiseStep[] = []
  for (const { signal, reasons } of notRun) {
    if (!satisfied && !missing.includes(signal)) continue
    for (const reason of reasons) {
      const next = step(reason)
      if (!raise.includes(next)) raise.push(next)
    }
  }
  return {
    profile: selected.profile,
    notRun: satisfied ? notRun : notRun.filter(n => missing.includes(n.signal)),
    floor: { required, satisfied, missing: [...missing] },
    raise,
  }
}
