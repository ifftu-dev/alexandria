# 04 Delete key rotation

Triage: done
Type: task
Repos: alexandria
Part of: alexandria/docs/credential-trust-spec.md
Blocked by: none

Spec sections: *Personal key rotation is deleted*, stories 14, 47.

## What to build

A `did:key` resolves to the key in its identifier, always.

- Remove the rotation function, the rotated-issuer constructor, rotation announcement handling on the `vc-did` topic, validity windows and the rotated-by link, in the app and the verifier library.
- The key registry remains only as the lookup for identifiers that are not self-resolving. For a `did:key`, no registry row can change resolution. A row without a key can no longer be written.
- First-sight DID announcements keep their other effect (promoting credentials waiting on that issuer), with no registry write for a `did:key`.
- Bundles stop exporting rotation history; bundle import ignores it.
- Tests that exercised rotation are removed or rewritten to assert the new rule.

## Acceptance criteria

- [x] No code path can make a `did:key` issuer resolve to anything but its embedded key.
- [x] A vector: a `did:key` credential verified with stale registry state for that DID supplied still resolves the embedded key and verifies.
- [x] Inbound `vc-did` announcements write no registry row for a `did:key`.
- [x] The verifier library's store trait and docs no longer describe rotation.
- [x] `cargo test` for the verifier library and the touched app modules pass.

## Pointers

- Registry and rotation: `src-tauri/src/crypto/key_registry.rs`, `src-tauri/src/credentials/issuer.rs` (`Issuer::rotated`).
- Gossip: `src-tauri/src/p2p/vc_did.rs`.
- Verifier key resolution: `crates/alexandria-verify/src/vc/verify.rs` (`resolve_issuer_key`), `crates/alexandria-verify/src/did.rs` (`KeyRegistryEntry`).
- Bundles: `src-tauri/src/credentials/bundle.rs`.
- Callers using `Issuer::rotated` in tests: `src-tauri/src/commands/assessment.rs`, `src-tauri/src/commands/adaptive.rs`.

## Notes

- 2026-11-16: built. Verifier library: `VerificationStore::key_at(did, at)` is now `registered_key(did)`, consulted only for a DID method that is not self-resolving; `KeyRegistryEntry` loses `valid_from`, `valid_until` and `rotated_by`. A `did:key` never asks the store, so an unavailable store no longer adds `IssuerKeyUnavailable` for one. Vector `08-rotated-issuer-key` is replaced by `08-did-key-ignores-registry` (registry state naming another key; accept); the independent JavaScript verifier and the demo stdlib verifier follow the same rule. `sign_credential` still keeps a preset verification method under the issuer's DID; only its rotation wording went. App: `rotate_key`, `resolve_key_at`, `Issuer::rotated`, `CredentialError::NotCurrentKey` and the personhood lab's current-key check are deleted; `key_registry::registered_key` is the only registry function. Migration 41 rebuilds `key_registry` as `(did, key_id, public_key_hex)` with `CHECK (length(public_key_hex) = 64)`, so a row without a key cannot be written. `vc-did` writes nothing and returns `DidIngest::Announced { promoted }`; `rotated_to` is ignored. Decision: because announcements no longer register issuers, the `vc-status` pre-filter now asks whether the issuer is a well-formed `did:key` (the only kind its signature check can authorise) instead of whether a registry row exists. Bundles drop the `key_registry` field and `KeyRegistryRow` (and the TypeScript mirror); an old bundle's field is ignored on import. The scoring fingerprint reads only `key_id` and `public_key_hex` from the registry.
