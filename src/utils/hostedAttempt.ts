// An assessment request whose organisation holds the answers (required
// answer custody `issuer`) is taken as a hosted attempt: the organisation's
// service sends one item at a time and grades. The backend's hosted attempt
// commands reject with `hosted_attempt:<code>: <message>`; these helpers read
// that code.
import type { IssuancePolicy } from '@/types'
import { errorMessage } from './errorMessage'

/** A request that only a hosted attempt can meet. */
export function isHostedRequest(policy: IssuancePolicy | null | undefined): boolean {
  return policy?.required_answer_custody === 'issuer'
}

export interface HostedAttemptError {
  code: string
  message: string
}

const ERROR = /^hosted_attempt:([a-z_]+): ?([\s\S]*)$/

/** The code and message of a hosted attempt command's rejection, else null. */
export function hostedAttemptError(cause: unknown): HostedAttemptError | null {
  if (cause === undefined || cause === null) return null
  const match = ERROR.exec(errorMessage(cause).trim())
  return match ? { code: match[1] ?? '', message: match[2] ?? '' } : null
}

/** The service could not be reached: retry, and the attempt resumes at the same item. */
export function isDisconnected(cause: unknown): boolean {
  return hostedAttemptError(cause)?.code === 'disconnected'
}

/** The service no longer serves the attempt: closed, or abandoned after the disconnection window. */
export function isGone(cause: unknown): boolean {
  return hostedAttemptError(cause)?.code === 'not_found'
}
