# 04 Delete key rotation

Triage: ready-for-agent
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

- [ ] No code path can make a `did:key` issuer resolve to anything but its embedded key.
- [ ] A vector: a `did:key` credential verified with stale registry state for that DID supplied still resolves the embedded key and verifies.
- [ ] Inbound `vc-did` announcements write no registry row for a `did:key`.
- [ ] The verifier library's store trait and docs no longer describe rotation.
- [ ] `cargo test` for the verifier library and the touched app modules pass.

## Pointers

- Registry and rotation: `src-tauri/src/crypto/key_registry.rs`, `src-tauri/src/credentials/issuer.rs` (`Issuer::rotated`).
- Gossip: `src-tauri/src/p2p/vc_did.rs`.
- Verifier key resolution: `crates/alexandria-verify/src/vc/verify.rs` (`resolve_issuer_key`), `crates/alexandria-verify/src/did.rs` (`KeyRegistryEntry`).
- Bundles: `src-tauri/src/credentials/bundle.rs`.
- Callers using `Issuer::rotated` in tests: `src-tauri/src/commands/assessment.rs`, `src-tauri/src/commands/adaptive.rs`.
