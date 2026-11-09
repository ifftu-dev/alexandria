import type { AnchorStatus } from '@/types'
import { assertNever } from '@/utils/exhaustive'

type BadgeVariant = 'secondary' | 'success' | 'warning' | 'error'

/** How an anchor state reads on a badge: tone and the label key. */
export function anchorBadge(status: AnchorStatus): { variant: BadgeVariant; label: string } {
  switch (status) {
    case 'confirmed':
      return { variant: 'success', label: 'credentials.anchor.status.confirmed' }
    case 'pending':
      return { variant: 'secondary', label: 'credentials.anchor.status.pending' }
    case 'submitted':
      return { variant: 'warning', label: 'credentials.anchor.status.submitted' }
    case 'outcome_unknown':
      return { variant: 'warning', label: 'credentials.anchor.status.outcomeUnknown' }
    case 'failed':
      return { variant: 'error', label: 'credentials.anchor.status.failed' }
    default:
      return assertNever(status)
  }
}
