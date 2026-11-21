// A role or credential request whose policy requires `device_attested` is
// taken on a phone that attests itself: a desktop (paired phone or not) only
// reaches `companion_attested`. The backend refuses to start such an attempt
// on a device that cannot reach it, before any item, with the stable code
// `verified_device_required:<phones>: …`; these helpers read that code and
// say up front which phones count.
import type { DeviceSupport } from '@/types'
import { errorMessage } from './errorMessage'

/** Which phones can take the attempt: an iPhone until Android attestation is enabled. */
export type VerifiedPhones = 'iphone' | 'phone'

/** The stable error code (mirrors `sentinel::attestation::VERIFIED_DEVICE_REQUIRED`). */
export const VERIFIED_DEVICE_REQUIRED = 'verified_device_required'

const REFUSAL = new RegExp(`${VERIFIED_DEVICE_REQUIRED}:(iphone|phone)\\b`)

/** The phones the refusal names when `cause` is the verified-device refusal, else null. */
export function verifiedDeviceRefusal(cause: unknown): VerifiedPhones | null {
  if (cause === undefined || cause === null) return null
  const match = REFUSAL.exec(errorMessage(cause))
  return match ? (match[1] as VerifiedPhones) : null
}

/** Whether a required assurance level must be met on a phone that attests itself. */
export function needsVerifiedDevice(level: string | null | undefined): boolean {
  return level === 'device_attested'
}

/** The phones that count on this build; an iPhone while support is unknown. */
export function verifiedPhones(support: DeviceSupport | null): VerifiedPhones {
  return support?.android_attestation ? 'phone' : 'iphone'
}
