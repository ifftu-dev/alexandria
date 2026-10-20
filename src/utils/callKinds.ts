// What each kind of call is for, and where its screens are.
//
// Every call rides the same engine, but a tutoring session, a classroom call
// and an interview are different things to the people in them. The kind is
// recorded with the call (tutoring_sessions.kind) and these helpers turn it
// into routes, so the mini window and the call screen send people back to
// the right place instead of always to Live Tutoring.

import type { CallKind } from '@/types'

export type { CallKind }

export interface CallRecord {
  id: string
  kind?: CallKind | null
  context_id?: string | null
}

/** Only open tutoring offers an invite link or QR code to share. */
export function sharesInvites(kind: CallKind | null | undefined): boolean {
  return (kind ?? 'tutoring') === 'tutoring'
}

/** The screen that shows this call while it is live. */
export function callScreen(call: CallRecord): string {
  const ctx = call.context_id
  switch (call.kind ?? 'tutoring') {
    case 'classroom':
      return ctx ? `/classrooms/${ctx}` : '/classrooms'
    case 'interview_panel':
      return ctx ? `/panel/${ctx}` : '/settings/organisations'
    case 'interview_assistant':
      return ctx ? `/interviews/${ctx}` : '/interviews'
    case 'interview_candidate':
      return `/interview-call/${call.id}`
    case 'plugin':
      return '/'
    default:
      return `/tutoring/${call.id}`
  }
}

/**
 * Where leaving a call takes you: its kind's home, never Live Tutoring for
 * an interview or a classroom. `cloud` is the candidate's Alexandria Cloud
 * address, which their waiting room needs.
 */
export function callHome(call: CallRecord | null | undefined, cloud?: string | null): string {
  const ctx = call?.context_id
  switch (call?.kind ?? 'tutoring') {
    case 'classroom':
      return ctx ? `/classrooms/${ctx}` : '/classrooms'
    case 'interview_panel':
      return ctx ? `/panel/${ctx}` : '/settings/organisations'
    case 'interview_assistant':
      return ctx ? `/interviews/${ctx}` : '/interviews'
    case 'interview_candidate':
      return ctx && cloud ? `/interview-room/${ctx}?cloud=${encodeURIComponent(cloud)}` : '/settings/organisations'
    case 'plugin':
      return '/'
    default:
      return '/tutoring'
  }
}
