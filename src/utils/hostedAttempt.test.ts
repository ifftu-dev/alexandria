import { describe, expect, it } from 'vitest'
import { hostedAttemptError, isDisconnected, isGone, isHostedRequest } from './hostedAttempt'

describe('hostedAttempt', () => {
  it('reads the code of a hosted attempt rejection', () => {
    expect(hostedAttemptError('hosted_attempt:retake_wait: The next attempt can open from 2026-11-27.')).toEqual({
      code: 'retake_wait',
      message: 'The next attempt can open from 2026-11-27.',
    })
    expect(isDisconnected(new Error('hosted_attempt:disconnected: the organisation\'s service could not be reached'))).toBe(true)
    expect(isGone('hosted_attempt:not_found: There is no hosted attempt here for this key.')).toBe(true)
    expect(hostedAttemptError('verified_device_required:iphone: take it on an iPhone')).toBeNull()
  })

  it('treats only issuer custody as a hosted request', () => {
    expect(isHostedRequest({ required_answer_custody: 'issuer' })).toBe(true)
    expect(isHostedRequest({ required_answer_custody: 'device' })).toBe(false)
    expect(isHostedRequest(null)).toBe(false)
  })
})
