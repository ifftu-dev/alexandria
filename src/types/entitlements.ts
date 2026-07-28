// SPDX-License-Identifier: MIT
/**
 * Entitlement contract shared by the community and enterprise builds.
 *
 * An entitlement says which enterprise features an organisation has paid
 * for. It is delivered as an ordinary Verifiable Credential
 * (`CredentialType::EntitlementCredential`), so validity — signature,
 * expiry, status-list revocation — is checked by the same MIT verification
 * path as any other VC, offline and with no license server.
 *
 * This file is MIT on purpose: a user must be able to read what gates the
 * software they are running. See `docs/enterprise-boundary.md`.
 */

/** Feature keys an entitlement can grant. Extend as EE features land. */
export type FeatureKey =
  | 'talent_index'
  | 'bulk_verification'
  | 'skills_intelligence'
  | 'employer_console'

export interface EntitlementSnapshot {
  /** Feature keys granted by a currently valid entitlement. */
  readonly features: readonly FeatureKey[]
  /** Organisation the entitlement was issued to, when there is one. */
  readonly orgId: string | null
  /** Plan identifier, when there is one. */
  readonly plan: string | null
  /**
   * True only in a build that can carry entitlements at all. The community
   * build reports false and grants nothing — it is not a degraded
   * enterprise build, it is the whole product minus the commercial layer.
   */
  readonly enterpriseBuild: boolean
}

/**
 * Resolves the current entitlement. Both `src/ee-stub` (MIT, community)
 * and `src/ee` (enterprise) implement this; `@ee` resolves to one or the
 * other at build time.
 */
export interface EntitlementProvider {
  loadEntitlements(): Promise<EntitlementSnapshot>
}

/** The empty entitlement: valid, and grants nothing. */
export const NO_ENTITLEMENTS: EntitlementSnapshot = {
  features: [],
  orgId: null,
  plan: null,
  enterpriseBuild: false,
}
