import { describe, expect, it } from 'vitest'
import { needsVerifiedDevice, verifiedDeviceRefusal, verifiedPhones } from '../verifiedDevice'

describe('verified device requirement', () => {
  it('reads the stable refusal code from a start error, whatever wraps it', () => {
    const message = 'verified_device_required:iphone: this requires a verified device; take the attempt in Alexandria on an iPhone'
    expect(verifiedDeviceRefusal(message)).toBe('iphone')
    expect(verifiedDeviceRefusal(new Error(message.replace(':iphone:', ':phone:')))).toBe('phone')
    expect(verifiedDeviceRefusal('policy floor not met: the policy requires the camera')).toBeNull()
    expect(verifiedDeviceRefusal(undefined)).toBeNull()
  })

  it('needs a verified device only for device_attested; a paired phone meets companion_attested', () => {
    expect(needsVerifiedDevice('device_attested')).toBe(true)
    expect(needsVerifiedDevice('companion_attested')).toBe(false)
    expect(needsVerifiedDevice('local')).toBe(false)
    expect(needsVerifiedDevice(null)).toBe(false)
  })

  it('names an iPhone until Android attestation is enabled', () => {
    expect(verifiedPhones(null)).toBe('iphone')
    expect(verifiedPhones({ platform_attestation: false, companion_phone: true, android_attestation: false, verified_device: false })).toBe('iphone')
    expect(verifiedPhones({ platform_attestation: false, companion_phone: true, android_attestation: true, verified_device: false })).toBe('phone')
  })
})
