import { describe, expect, it } from 'vitest'
import { floorNeedsDevice, floorReach, policyRequirements } from '../requestPolicy'

describe('request policy requirements', () => {
  it('lists what is set, in the order the shared rule judges it', () => {
    expect(policyRequirements({
      probe_policy: 'authored', min_verification_rung: 'probed', min_trust_profile: 'camera',
      required_assurance_level: 'device_attested', max_warning: 3, max_critical: 0, min_integrity: 0.755,
      require_clean: true,
    })).toEqual([
      { kind: 'clean' },
      { kind: 'minIntegrity', percent: 76 },
      { kind: 'maxCritical', count: 0 },
      { kind: 'maxWarning', count: 3 },
      { kind: 'assurance', level: 'device_attested' },
      { kind: 'floor', profile: 'camera' },
      { kind: 'rung', rung: 'probed' },
      { kind: 'probes', policy: 'authored' },
    ])
  })

  it('leaves out what asks for nothing', () => {
    expect(policyRequirements({})).toEqual([])
    expect(policyRequirements({
      require_clean: false, min_integrity: null, min_trust_profile: 'core', min_verification_rung: 'none', probe_policy: 'none',
    })).toEqual([])
  })
})

describe('floor reach', () => {
  const known = { platform_attestation: false, companion_phone: false, android_attestation: false, verified_device: false }

  it('names a missing camera and missing attestation only when known', () => {
    expect(floorReach('full', { camera: false, support: known })).toEqual({ blockers: ['no_camera', 'no_attestation'], pairPhone: false })
    expect(floorReach('full', { camera: null, support: null })).toEqual({ blockers: [], pairPhone: false })
    expect(floorReach('camera', { camera: true, support: known })).toEqual({ blockers: [], pairPhone: false })
  })

  it('offers a paired phone where one can lend the attestation', () => {
    expect(floorReach('attested', { camera: false, support: { platform_attestation: false, companion_phone: true, android_attestation: false, verified_device: false } }))
      .toEqual({ blockers: [], pairPhone: true })
    expect(floorReach('attested', { camera: false, support: { platform_attestation: true, companion_phone: false, android_attestation: false, verified_device: true } }))
      .toEqual({ blockers: [], pairPhone: false })
  })

  it('asks about the device only for a floor beyond core', () => {
    expect(floorNeedsDevice(null)).toBe(false)
    expect(floorNeedsDevice('core')).toBe(false)
    expect(floorNeedsDevice('attested')).toBe(true)
    expect(floorNeedsDevice('camera')).toBe(true)
  })
})
