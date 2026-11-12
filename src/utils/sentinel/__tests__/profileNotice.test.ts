// Seam 5 for the learner's pre-attempt notice: from the backend's profile
// choice, what the learner is told before the first item — which signals do
// not run and why, whether the sponsor's floor is met, and what would raise
// the profile. Nothing here decides anything; the backend chose already.
import { describe, expect, it } from 'vitest'
import type { PolicyFloor, ProfileExclusion, SelectedProfile, TrustProfile } from '@/types'
import { profileNotice } from '@/utils/sentinel/profileNotice'

const budget = { detect_ms: 250, liveness_ms: 60, track_ms: 15, detect_interval: 3000 }

function selected(
  profile: TrustProfile,
  excluded: ProfileExclusion[],
  floor: Partial<PolicyFloor> = {},
): SelectedProfile {
  const signals = { core: [], attested: ['device_attestation'], camera: ['camera'], full: ['device_attestation', 'camera'] } as const
  const run: readonly string[] = signals[profile]
  return {
    profile,
    inputs: {
      device_attested: false,
      capabilities: { camera: true, microphone: false, companion_phone: false },
      consent: { camera: false, microphone: false, companion_phone: false },
      budget: null,
      floor: floor.required ?? null,
    },
    excluded,
    not_run: (['device_attestation', 'camera'] as const).filter(s => !run.includes(s)),
    benchmark: null,
    camera_budget: budget,
    full_budget: budget,
    policy_floor: { required: null, profile, satisfied: true, missing: [], ...floor },
  }
}

describe('profileNotice', () => {
  it('says why a desktop session runs core and how to raise it', () => {
    const notice = profileNotice(selected('core', [
      { profile: 'attested', reasons: ['not_attested'] },
      { profile: 'camera', reasons: ['camera_not_consented'] },
      { profile: 'full', reasons: ['not_attested', 'camera_not_consented'] },
    ]), { mobile: false })
    expect(notice.profile).toBe('core')
    expect(notice.notRun).toEqual([
      { signal: 'device_attestation', reasons: ['not_attested'] },
      { signal: 'camera', reasons: ['camera_not_consented'] },
    ])
    expect(notice.floor).toEqual({ required: null, satisfied: true, missing: [] })
    expect(notice.raise).toEqual(['pair_phone', 'allow_camera'])
  })

  it('names only what the unmet floor is missing', () => {
    const notice = profileNotice(selected('camera', [
      { profile: 'attested', reasons: ['not_attested'] },
      { profile: 'full', reasons: ['not_attested', 'over_budget'] },
    ], { required: 'attested', satisfied: false, missing: ['device_attestation'] }), { mobile: true })
    expect(notice.floor).toEqual({ required: 'attested', satisfied: false, missing: ['device_attestation'] })
    expect(notice.notRun).toEqual([{ signal: 'device_attestation', reasons: ['not_attested'] }])
    // A phone without attestation cannot pair another phone: it needs an
    // attested device. The camera budget is beside the point here.
    expect(notice.raise).toEqual(['attested_device'])
  })

  it('points a device that cannot sustain the camera at a faster one', () => {
    const notice = profileNotice(selected('attested', [
      { profile: 'camera', reasons: ['over_budget'] },
      { profile: 'full', reasons: ['not_benchmarked'] },
    ]), { mobile: true })
    expect(notice.notRun).toEqual([{ signal: 'camera', reasons: ['over_budget', 'not_benchmarked'] }])
    expect(notice.raise).toEqual(['faster_device'])
  })

  it('has nothing to raise on the full profile', () => {
    const notice = profileNotice(selected('full', []), { mobile: true })
    expect(notice.notRun).toEqual([])
    expect(notice.raise).toEqual([])
  })
})
