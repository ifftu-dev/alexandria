// Visual identity for the six credential classes (domain/vc §6).
//
// One source of truth for the colour + icon coding used across the
// credentials page: the derived-credential grid, the input-credential
// drill-down, and the legend. Tailwind classes are written out in full
// (no string interpolation) so the JIT compiler keeps them.

import type { CredentialType } from '@/types'

export type CredentialClass = CredentialType | 'DerivedCredential'

export interface CredentialKindMeta {
  /** Full human label. */
  label: string
  /** Compact label for chips/badges. */
  short: string
  /** Badge classes: background + text + ring. */
  badge: string
  /** Solid accent dot/icon-chip background. */
  dot: string
  /** Foreground text accent. */
  text: string
  /** Single SVG path `d` (24×24, stroke-based). */
  icon: string
}

const UNKNOWN: CredentialKindMeta = {
  label: 'Credential',
  short: 'Other',
  badge: 'bg-muted text-muted-foreground ring-1 ring-border',
  dot: 'bg-muted-foreground/60',
  text: 'text-muted-foreground',
  icon: 'M9 12h6m-6 4h6m2 4H7a2 2 0 01-2-2V5a2 2 0 012-2h6l6 6v11a2 2 0 01-2 2z',
}

export const CREDENTIAL_KINDS: Record<CredentialClass, CredentialKindMeta> = {
  // Official completion cert from an authority.
  FormalCredential: {
    label: 'Formal',
    short: 'Formal',
    badge: 'bg-indigo-100 text-indigo-700 ring-1 ring-indigo-200 dark:bg-indigo-900/30 dark:text-indigo-300 dark:ring-indigo-800',
    dot: 'bg-indigo-500',
    text: 'text-indigo-600 dark:text-indigo-400',
    icon: 'M12 14l9-5-9-5-9 5 9 5zm0 0l6.16-3.42M12 14v7m-6-9.5V17a6 3 0 0012 0v-5.5',
  },
  // Result of a graded evaluation (quiz/exam/codejudge).
  AssessmentCredential: {
    label: 'Assessment',
    short: 'Assess',
    badge: 'bg-amber-100 text-amber-700 ring-1 ring-amber-200 dark:bg-amber-900/30 dark:text-amber-300 dark:ring-amber-800',
    dot: 'bg-amber-500',
    text: 'text-amber-600 dark:text-amber-400',
    icon: 'M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2m-3 7l2 2 4-4',
  },
  // A third party vouches for you (endorsement/attendance/integrity).
  AttestationCredential: {
    label: 'Attestation',
    short: 'Attest',
    badge: 'bg-emerald-100 text-emerald-700 ring-1 ring-emerald-200 dark:bg-emerald-900/30 dark:text-emerald-300 dark:ring-emerald-800',
    dot: 'bg-emerald-500',
    text: 'text-emerald-600 dark:text-emerald-400',
    icon: 'M9 12l2 2 4-4m5.618-4.016A11.955 11.955 0 0112 2.944a11.955 11.955 0 01-8.618 3.04A12.02 12.02 0 003 9c0 5.591 3.824 10.29 9 11.622 5.176-1.332 9-6.03 9-11.622 0-1.042-.133-2.052-.382-3.016z',
  },
  // A role/permission in a context (instructor, DAO member).
  RoleCredential: {
    label: 'Role',
    short: 'Role',
    badge: 'bg-sky-100 text-sky-700 ring-1 ring-sky-200 dark:bg-sky-900/30 dark:text-sky-300 dark:ring-sky-800',
    dot: 'bg-sky-500',
    text: 'text-sky-600 dark:text-sky-400',
    icon: 'M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z',
  },
  // Computed roll-up over evidence (not directly issued).
  DerivedCredential: {
    label: 'Derived',
    short: 'Derived',
    badge: 'bg-violet-100 text-violet-700 ring-1 ring-violet-200 dark:bg-violet-900/30 dark:text-violet-300 dark:ring-violet-800',
    dot: 'bg-violet-500',
    text: 'text-violet-600 dark:text-violet-400',
    icon: 'M13 10V3L4 14h7v7l9-11h-7z',
  },
  // You claim it about yourself (lowest trust).
  SelfAssertion: {
    label: 'Self',
    short: 'Self',
    badge: 'bg-slate-100 text-slate-700 ring-1 ring-slate-200 dark:bg-slate-800/60 dark:text-slate-300 dark:ring-slate-700',
    dot: 'bg-slate-500',
    text: 'text-slate-600 dark:text-slate-400',
    icon: 'M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z',
  },
}

/** Resolve a VC's `type` array to its class meta. */
export function kindOfType(types: string[]): CredentialKindMeta {
  for (const t of types) {
    if (t in CREDENTIAL_KINDS) return CREDENTIAL_KINDS[t as CredentialClass]
  }
  return UNKNOWN
}

/** The bare class name (e.g. `FormalCredential`) of a VC, or `'Other'`. */
export function classNameOf(types: string[]): string {
  return types.find((t) => t in CREDENTIAL_KINDS) ?? 'Other'
}

/** Ordered list for legends / filter chips. */
export const CREDENTIAL_CLASS_ORDER: CredentialClass[] = [
  'FormalCredential',
  'AssessmentCredential',
  'AttestationCredential',
  'RoleCredential',
  'DerivedCredential',
  'SelfAssertion',
]
