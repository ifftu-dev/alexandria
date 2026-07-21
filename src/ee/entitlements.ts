// SPDX-License-Identifier: LicenseRef-IFFTU-Enterprise
/**
 * Enterprise-edition entitlement provider (IFFTU Enterprise License — see
 * `LICENSE.md` in this directory). Selected when the build sets `VITE_EE=1`.
 *
 * Placeholder: resolves the entitlement credential from the local store and
 * verifies it via the MIT `verify_credential` path. Wired in Phase 2
 * alongside the billing service that issues these credentials. Until then
 * it reports an enterprise build that grants no features, so the EE build
 * behaves exactly like the community build rather than silently unlocking
 * anything.
 *
 * It must satisfy `EntitlementProvider` — the shared MIT contract in
 * `src/types/entitlements.ts`.
 */

import type { EntitlementProvider, EntitlementSnapshot } from '@/types/entitlements'

export const loadEntitlements: EntitlementProvider['loadEntitlements'] =
  async (): Promise<EntitlementSnapshot> => ({
    features: [],
    orgId: null,
    plan: null,
    enterpriseBuild: true,
  })
