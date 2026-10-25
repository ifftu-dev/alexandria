// Chat and sign-in errors as the backend writes them: `code: reason`, where
// the code is one of the generated ChatErrorCode values and the reason is
// for people (or, for `rate_limited` and `http`, a number). This is the one
// place the webview reads a code or picks its message.

import { CHAT_ERROR_CODES, type ChatErrorCode } from '@/generated/chat-vocab'
import type { MessageKey } from '@/i18n/keys'

const CODES: ReadonlySet<string> = new Set(CHAT_ERROR_CODES)

export function isChatErrorCode(value: string): value is ChatErrorCode {
  return CODES.has(value)
}

/** The code at the start of a backend error, and whatever follows its first colon. */
export function parseChatError(value: unknown): { code: ChatErrorCode | null; detail: string } {
  const text = String(value instanceof Error ? value.message : value)
  const colon = text.indexOf(':')
  const head = (colon < 0 ? text : text.slice(0, colon)).trim()
  const detail = colon < 0 ? '' : text.slice(colon + 1).trim()
  return { code: isChatErrorCode(head) ? head : null, detail }
}

/** The code of a backend error, or `generic` when it carries none the webview knows. */
export function chatErrorCode(value: unknown): ChatErrorCode | 'generic' {
  return parseChatError(value).code ?? 'generic'
}

/** What chat shows: a backend code, or one of the two the webview chooses itself. */
export type ChatErrorDisplay = ChatErrorCode | 'generic' | 'memory'

// Every code has a message. Codes without wording of their own fall back to
// the generic one; a new backend code is a type error here until it is placed.
const CHAT_ERROR_MESSAGES: Record<ChatErrorDisplay, MessageKey> = {
  invalid: 'chat.errors.invalid',
  unreachable: 'chat.errors.unreachable',
  timeout: 'chat.errors.timeout',
  interrupted: 'chat.errors.interrupted',
  auth: 'chat.errors.auth',
  rate_limited: 'chat.errors.rate_limited',
  model_unavailable: 'chat.errors.model_unavailable',
  http: 'chat.errors.http',
  malformed: 'chat.errors.malformed',
  too_large: 'chat.errors.too_large',
  empty: 'chat.errors.empty',
  provider: 'chat.errors.provider',
  unsupported: 'chat.errors.unsupported',
  cancelled: 'chat.errors.cancelled',
  usage_limit: 'chat.errors.usage_limit',
  plan_ineligible: 'chat.errors.plan_ineligible',
  assessment_active: 'chat.errors.assessment_active',
  assessment_item: 'chat.errors.assessment_item',
  no_connection: 'chat.errors.no_connection',
  guardian_required: 'chat.errors.guardian_required',
  guardian_pending: 'chat.errors.guardian_pending',
  guardian_declined: 'chat.errors.guardian_declined',
  no_guardian: 'chat.errors.no_guardian',
  under_13: 'chat.errors.under_13',
  adults_only: 'chat.errors.adults_only',
  context_stale: 'chat.errors.context_stale',
  too_many_runs: 'chat.errors.too_many_runs',
  source_unavailable: 'chat.errors.source_unavailable',
  not_enrolled: 'chat.errors.generic',
  no_lesson: 'chat.errors.generic',
  needs_plugins: 'chat.errors.generic',
  open_course: 'chat.errors.generic',
  conflict: 'chat.errors.generic',
  not_found: 'chat.errors.generic',
  invalid_input: 'chat.errors.generic',
  storage_error: 'chat.errors.generic',
  signin_required: 'chat.errors.signin_required',
  signin_cancelled: 'chat.errors.generic',
  signin_timeout: 'chat.errors.generic',
  signin_declined: 'chat.errors.generic',
  plan_not_granted: 'chat.errors.generic',
  different_account: 'chat.errors.generic',
  code_rejected: 'chat.errors.generic',
  signin_expired: 'chat.errors.generic',
  signin_failed: 'chat.errors.generic',
  signin_busy: 'chat.errors.generic',
  generic: 'chat.errors.generic',
  memory: 'chat.errors.memory',
}

/** The translation key for an error shown in the chat panel or on an answer. */
export function chatErrorMessageKey(code: ChatErrorDisplay): MessageKey {
  return CHAT_ERROR_MESSAGES[code]
}

// Sign-in has its own wording for the codes it can end with. Any other code
// gets the generic sign-in message with the code shown.
const SIGN_IN_ERROR_MESSAGES: Partial<Record<ChatErrorCode, MessageKey>> = {
  signin_timeout: 'chat.signin.errors.timeout',
  signin_declined: 'chat.signin.errors.declined',
  plan_not_granted: 'chat.signin.errors.planNotGranted',
  different_account: 'chat.signin.errors.differentAccount',
  signin_busy: 'chat.signin.errors.busy',
  unreachable: 'chat.signin.errors.unreachable',
  under_13: 'chat.errors.under_13',
  adults_only: 'chat.errors.adults_only',
  model_unavailable: 'chat.signin.errors.noModels',
  code_rejected: 'chat.signin.errors.codeRejected',
  signin_expired: 'chat.signin.errors.expired',
}

/** The translation key for a failed provider sign-in, when it has its own. */
export function signInErrorMessageKey(code: ChatErrorCode): MessageKey | null {
  return SIGN_IN_ERROR_MESSAGES[code] ?? null
}
