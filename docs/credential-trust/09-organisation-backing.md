# 09 Organisation backing

Triage: ready-for-agent
Type: task
Repos: alexandria
Part of: alexandria/docs/credential-trust-spec.md
Blocked by: 04

Spec sections: *Organisation-issued credentials from the hosted service* (issuer object and backing bullets), stories 6–8, 37.

## What to build

A verifier can see which organisation, if any, stands behind a credential's issuer key.

- The credential envelope accepts `issuer` as a string or as an object with `id` plus `name` and `domain` (VC 2.0 allows both). The proof is checked against `id`. Serialisation of credentials issued by learners stays the string form.
- A function in `alexandria-verify` takes a credential and an optional organisation domain descriptor (resolved by the caller, as the existing org identity contract does) and reports organisation backing: `verified` with name and domain, `key_not_listed`, or `descriptor_not_supplied`. When the issuer object names a domain, the descriptor must be for that domain.
- The provenance classification exposes whether the issuer is the subject (self-issued) and the organisation backing outcome when supplied.
- The app resolves the descriptor through its existing TXT lookup when classifying a stored credential whose issuer names a domain.

## Acceptance criteria

- [ ] Credentials with a string issuer verify byte-for-byte as before.
- [ ] Vectors: object issuer verifies; backing verified; key removed from the descriptor (`key_not_listed`, signature still valid); descriptor for another domain (`key_not_listed` or a distinct code); descriptor not supplied.
- [ ] The app's credential trust command reports backing for an object-issuer credential using a stubbed resolver in tests.

## Pointers

- Org identity contract: `crates/alexandria-verify/src/org.rs` (`OrgDescriptor`, `descriptor_from_txt`).
- App TXT lookup: grep `descriptor_from_txt` under `src-tauri/src/`.
- Classification: `crates/alexandria-verify/src/trust.rs`; app command `get_credential_trust`.
- Envelope: `crates/alexandria-verify/src/vc/mod.rs` (`issuer: Did`).
