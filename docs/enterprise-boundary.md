# The enterprise boundary

Alexandria is AGPL-3.0-or-later in full. The commercial Enterprise Edition is a **shell
around** this application, in a separate repository, and nothing here references
or depends on it.

This document exists to keep it that way.

## The rule, stated once

> The Enterprise Edition contains only what exists because an organisation has
> multiple people, an IT department, or a compliance obligation. Never anything
> that exists because a learner wants to learn or prove something.

## The guarantee

> Credential verification is free, offline-capable, and permanent, forever. No
> commercial feature may be required to verify a credential.

An implementation that makes verification depend on a hosted service being
reachable is wrong even if it passes tests.

## What "shell" means in practice

Core is complete and unaware. It has no feature flags naming the commercial
edition, no licence checks, no stub modules standing in for proprietary ones,
and no conditional compilation that changes what a user gets. A build you
compile yourself is the whole product.

That constrains what can be sold, deliberately: **nothing inside the app is ever
withheld.** Organisations pay for running Alexandria *as an organisation* —
identity and provisioning, policy and access control, audit and retention,
hosted services the app talks to, and the operational guarantees around them.

The honest answer to "what stops a company just using the free app?" is
*nothing*. A five-person team should. A five-hundred-person company will want
SSO, provisioning, an audit trail and someone to call — none of which exists for
an individual, which is exactly why charging for it is fair.

## The test

When unsure which side something belongs on:

> Would a single individual with no employer ever want this?

If yes, it is core, and it is AGPL. There is no third category.

## Worked examples

| Feature | Verdict | Reasoning |
|---|---|---|
| Adaptive assessment / IRT | **Core** | It is how a score is produced |
| Attempt policy and cooldowns | **Core** | A learner-side integrity rule |
| Artifact grader plugins | **Core** | Scores must be re-derivable by anyone |
| Credential issuance, holding, presentation | **Core** | The product |
| Credential verification logic | **Core**, and permissively licensed | Permanently — see the guarantee above. `alexandria-verify` is I/O-free and MIT OR Apache-2.0, so anyone may embed it, including in closed software. An AGPL verifier would make checking a credential legally expensive for the registrars and employers the guarantee exists for |
| Selective disclosure | **Core** | Every learner gets it |
| Sentinel on-device integrity | **Core** | Runs on the learner's machine |
| Telling a learner they were flagged | **Core** | An accusation the subject cannot see is not something they can answer |
| Appeal-evidence retention and its consent prompt | **Core** | It decides what is kept about a learner, on their device, by their choice |
| Importing a credential someone handed you | **Core** | Receiving is not a commercial act |
| Talent-index consent UI, publish client, wire schema | **Core** | A learner must audit what leaves their device |
| SSO / SAML / OIDC, SCIM provisioning | **Enterprise** | Exists only because an org has many people |
| Role-based access control, delegated admin | **Enterprise** | Same |
| Immutable audit log, retention policy, data residency | **Enterprise** | Compliance obligation |
| Org-operated relay with auth and quotas | **Enterprise** | Org infrastructure |
| Talent index service and storage | **Enterprise** | Multi-tenant server-side state about other people |
| Employer search console | **Enterprise** | Scoped to an organisation |
| Hosted verification API — rate limits, keys, SLA | **Enterprise** | The hosted *operation*; the logic underneath stays core |
| Bulk verification | **Enterprise** | A metered enterprise operation |
| Human review queue over Sentinel flags | **Enterprise** | An org process with an adjudication workflow — but it may not request evidence; see `sentinel.md` |
| Cohort analytics, skills intelligence | **Enterprise** | Cross-user server-side aggregation |
| Seat accounting and billing | **Enterprise** | Definitionally multi-tenant |

Note the pattern in the split cases: the *client* is core and the *service* is
enterprise. The talent-index publish client is core because it decides what
leaves a learner's device; the index that receives it is enterprise because it
holds data about other people. The verification logic is core; the hosted API
with an SLA in front of it is enterprise.

## How the boundary is enforced

Structurally, not by policy:

- The commercial code is **not in this repository**. It cannot leak in, because
  there is nowhere for it to go.
- Core has no build flag, licence check, or module that names it. Nothing to
  mis-set, nothing to strip, nothing to audit.
- The Enterprise Edition depends on core through published, versioned
  interfaces only — never a fork, never a patch, never reaching into internals.
  Anything it needs goes upstream into core as a general-purpose interface,
  which means every user gets it.

## Adding a feature

1. Apply the test above.
2. If it is core, build it here. That is the default and most things are.
3. If it is enterprise, it does not belong in this repository at all — not
   behind a flag, not as a stub.
4. If it seems to need a hook *into* core, that hook is a general-purpose
   interface other people would also want. Design it that way, or reconsider.
5. Add a row to the worked examples above so the next person does not
   re-litigate it.

## History

Between 2026-07 and 2026-08 this repository briefly carried an in-core seam: an
`ee` cargo feature, `src/ee` and `src/ee-stub` trees, an `@ee` alias, and a
compiled-in trusted-issuer allowlist that gated features client-side.

It was removed. A client-side gate is bypassable by anyone who can patch a
binary — and with a publicly readable codebase, that is anyone who wants
to. It also could not enforce the one thing the pricing model needed, seat
counting, because a device cannot see the other members of an organisation. It
took the reputational cost of visibly gating an open-source product while
providing none of the protection.

What survived the removal, because it was always core: the `allowed_types`
verification fix, `CredentialType::as_str`, `import_credential`, iroh credential
delivery, and the talent-index consent client with signed records. Holder
binding went with it — it bound an *entitlement* to a device, so with
entitlements gone it had no subject.
