// SPDX-License-Identifier: LicenseRef-IFFTU-Enterprise
/**
 * Enterprise-edition entitlement provider (IFFTU Enterprise License — see
 * `LICENSE.md` in this directory). Selected when the build sets `VITE_EE=1`.
 *
 * This file is thin **by design**. Everything that decides whether an
 * entitlement is real — signature, issuer resolution, expiry, revocation,
 * suspension, and the compiled-in trusted-issuer allowlist — lives in MIT
 * Rust (`domain/vc/entitlement.rs`, `commands/entitlements.rs`) so a user can
 * audit exactly what gates the software they are running. All this layer does
 * is ask for the answer and translate it.
 *
 * It must satisfy `EntitlementProvider` — the shared MIT contract in
 * `src/types/entitlements.ts`.
 */

import { invoke } from '@tauri-apps/api/core'

import type { EntitlementProvider, EntitlementSnapshot, FeatureKey } from '@/types/entitlements'

/**
 * The backend's reply. Deliberately not `EntitlementSnapshot`: the backend
 * cannot know whether this is an enterprise build (that is decided by `@ee`
 * resolution at build time), and its feature strings are unvalidated.
 */
interface BackendSnapshot {
  features: string[]
  orgId: string | null
  plan: string | null
}

/**
 * Feature keys this build understands. An entitlement from a newer plan may
 * name a key that did not exist when this build shipped; such keys are carried
 * intact through the backend and dropped here, at the last possible moment, so
 * an older client degrades to "feature absent" rather than failing to read the
 * credential at all.
 */
const KNOWN_FEATURES: readonly FeatureKey[] = [
  'talent_index',
  'bulk_verification',
  'skills_intelligence',
  'employer_console',
]

function isKnownFeature(key: string): key is FeatureKey {
  return (KNOWN_FEATURES as readonly string[]).includes(key)
}

/**
 * An enterprise build with no valid entitlement. Distinct from
 * `NO_ENTITLEMENTS` only in `enterpriseBuild`, and that distinction is the
 * point: this build *could* carry an entitlement and does not, whereas the
 * community build could not carry one at all.
 */
const NO_FEATURES: EntitlementSnapshot = {
  features: [],
  orgId: null,
  plan: null,
  enterpriseBuild: true,
}

export const loadEntitlements: EntitlementProvider['loadEntitlements'] =
  async (): Promise<EntitlementSnapshot> => {
    try {
      const snapshot = await invoke<BackendSnapshot>('get_entitlement_snapshot')
      return {
        features: snapshot.features.filter(isKnownFeature),
        orgId: snapshot.orgId,
        plan: snapshot.plan,
        enterpriseBuild: true,
      }
    } catch {
      // Fail closed. An unreachable or erroring backend must never be read as
      // permission — it is indistinguishable from a tampered one, and the
      // composable that calls this treats a rejection the same way.
      return NO_FEATURES
    }
  }
