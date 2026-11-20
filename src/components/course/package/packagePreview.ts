// Preview of a package element: the player and its session drive the
// attempt commands as usual, and this adapter sends them to the host's
// preview commands instead. A preview has no enrolment and stores nothing;
// launches, extraction and package lookups are the same as for a learner.

import type { useLocalApi } from '@/composables/useLocalApi'
import type { PackageInvoke } from './packageSession'

type LocalInvoke = ReturnType<typeof useLocalApi>['invoke']

export function previewInvoke(invoke: LocalInvoke, elementId: string): PackageInvoke {
  return <T>(command: string, args: Record<string, unknown> = {}): Promise<T> => {
    switch (command) {
      case 'package_attempt_open': return invoke<T>('package_preview_open', { elementId })
      case 'package_attempt_fresh': return invoke<T>('package_preview_fresh', { elementId })
      case 'package_attempt_initialise': return invoke<T>('package_preview_initialise', args)
      case 'package_attempt_commit': return invoke<T>('package_preview_commit', args)
      case 'package_attempt_activities': return invoke<T>('package_preview_activities', args)
      case 'package_attempt_terminate': return invoke<T>('package_preview_terminate', args)
      case 'package_attempt_sequencing': return invoke<T>('package_preview_sequencing', args)
      case 'package_attempt_sequencing_save': return invoke<T>('package_preview_sequencing_save', args)
      default: return invoke<T>(command as Parameters<LocalInvoke>[0], args)
    }
  }
}
