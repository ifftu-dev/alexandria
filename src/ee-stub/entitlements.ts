// SPDX-License-Identifier: MIT
/**
 * Community-edition entitlement provider.
 *
 * `@ee` resolves here unless the build sets `VITE_EE=1`. It grants nothing,
 * and that is the intended steady state: every learner-facing capability —
 * learning, assessment, credential issuance, verification, presentation —
 * is MIT and works without an entitlement. Nothing gated here is needed to
 * use Alexandria.
 */

import type { EntitlementProvider, EntitlementSnapshot } from '@/types/entitlements'
import { NO_ENTITLEMENTS } from '@/types/entitlements'

export const loadEntitlements: EntitlementProvider['loadEntitlements'] =
  async (): Promise<EntitlementSnapshot> => NO_ENTITLEMENTS
