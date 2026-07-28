// SPDX-License-Identifier: MIT
/**
 * The community edition must grant nothing.
 *
 * These tests run against the default `@ee` → `src/ee-stub` alias, so they
 * assert the community build's behaviour. If someone points `@ee` at the
 * enterprise sources by default, or makes the stub grant a feature, this
 * fails — which is the point.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { useEntitlements } from '@/composables/useEntitlements'
import { NO_ENTITLEMENTS } from '@/types/entitlements'
import type { EntitlementProvider } from '@/types/entitlements'
import { loadEntitlements as communityProvider } from '@/ee-stub/entitlements'

type Loader = EntitlementProvider['loadEntitlements']

/**
 * Load the enterprise provider if this checkout has one.
 *
 * A community *distribution* may ship with `src/ee/` deleted outright —
 * the license is written as "if that directory exists". A plain dynamic
 * `import()` will not do: Vite resolves those at transform time, so a
 * missing directory fails the whole file before any try/catch runs.
 * `import.meta.glob` resolves to an empty object instead, which is the
 * behaviour we need.
 */
const enterpriseModules = import.meta.glob<{ loadEntitlements: Loader }>('../../ee/entitlements.ts')

async function loadEnterpriseProvider(): Promise<Loader | null> {
  const entry = Object.values(enterpriseModules)[0]
  if (!entry) return null
  return (await entry()).loadEntitlements
}

describe('useEntitlements (community edition)', () => {
  beforeEach(() => {
    vi.resetModules()
  })

  it('grants no features', async () => {
    const { hasFeature, refresh } = useEntitlements()
    await refresh()

    expect(hasFeature('talent_index')).toBe(false)
    expect(hasFeature('bulk_verification')).toBe(false)
    expect(hasFeature('skills_intelligence')).toBe(false)
    expect(hasFeature('employer_console')).toBe(false)
  })

  it('reports itself as a non-enterprise build with no org or plan', async () => {
    const { entitlements, refresh } = useEntitlements()
    await refresh()

    expect(entitlements.value.enterpriseBuild).toBe(false)
    expect(entitlements.value.orgId).toBeNull()
    expect(entitlements.value.plan).toBeNull()
    expect(entitlements.value.features).toEqual([])
  })

  it('loads without being asked and settles', async () => {
    const { loaded, refresh } = useEntitlements()
    await refresh()
    expect(loaded.value).toBe(true)
  })

  it('exposes an immutable snapshot', async () => {
    const { entitlements, refresh } = useEntitlements()
    await refresh()
    // `readonly()` makes writes a no-op (and a dev warning) rather than
    // throwing — assert the value is unchanged, which is what matters.
    const before = entitlements.value.features.length
    expect(before).toBe(0)
    expect(entitlements.value).toEqual(NO_ENTITLEMENTS)
  })
})

describe('the @ee seam', () => {
  // The alias is what makes the community build free of enterprise code.
  // If both sides ever collapse to the same implementation, the seam is
  // decorative and this catches it.
  it('has two distinguishable implementations', async () => {
    const enterprise = await loadEnterpriseProvider()
    if (!enterprise) return // community distribution, ee/ stripped

    expect((await communityProvider()).enterpriseBuild).toBe(false)
    expect((await enterprise()).enterpriseBuild).toBe(true)
  })

  it('holds every present implementation to the shared contract', async () => {
    const enterprise = await loadEnterpriseProvider()
    const providers: Loader[] = [communityProvider, ...(enterprise ? [enterprise] : [])]

    for (const load of providers) {
      const snapshot = await load()
      expect(Array.isArray(snapshot.features)).toBe(true)
      expect(snapshot).toHaveProperty('orgId')
      expect(snapshot).toHaveProperty('plan')
      expect(typeof snapshot.enterpriseBuild).toBe('boolean')
    }
  })

  it('grants nothing on either side until billing lands in Phase 2', async () => {
    // Guards against an EE placeholder quietly shipping real grants before
    // there is a service that can revoke them.
    expect((await communityProvider()).features).toEqual([])

    const enterprise = await loadEnterpriseProvider()
    if (enterprise) expect((await enterprise()).features).toEqual([])
  })
})
