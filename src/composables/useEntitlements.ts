// SPDX-License-Identifier: MIT
/**
 * Entitlement state for the running build.
 *
 * `hasFeature(key)` is the single question the UI should ask before showing
 * an enterprise surface. In a community build it is always false, because
 * `@ee` resolves to the MIT stub that grants nothing.
 *
 * This composable is deliberately MIT and deliberately dumb: it reports
 * what the entitlement says, and never decides policy itself. The
 * authoritative check for anything that matters happens server-side —
 * a client-side boolean gates UI, not access.
 */

import { readonly, ref } from 'vue'
import { loadEntitlements } from '@ee/entitlements'
import type { EntitlementSnapshot, FeatureKey } from '@/types/entitlements'
import { NO_ENTITLEMENTS } from '@/types/entitlements'

const snapshot = ref<EntitlementSnapshot>(NO_ENTITLEMENTS)
const loaded = ref(false)
let inflight: Promise<void> | null = null

async function refresh(): Promise<void> {
  snapshot.value = await loadEntitlements()
  loaded.value = true
}

export function useEntitlements() {
  // Single-flight: many components may mount at once and all of them are
  // entitled to ask. Only the first triggers a load.
  if (!loaded.value && !inflight) {
    inflight = refresh()
      .catch(() => {
        // Fail closed. An entitlement that cannot be read is not an
        // entitlement, and the community path is always the safe default.
        snapshot.value = NO_ENTITLEMENTS
        loaded.value = true
      })
      .finally(() => {
        inflight = null
      })
  }

  const hasFeature = (key: FeatureKey): boolean => snapshot.value.features.includes(key)

  return {
    entitlements: readonly(snapshot),
    loaded: readonly(loaded),
    hasFeature,
    refresh,
  }
}
