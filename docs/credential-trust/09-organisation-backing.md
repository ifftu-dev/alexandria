# 09 Organisation backing

Triage: done
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

- [x] Credentials with a string issuer verify byte-for-byte as before.
- [x] Vectors: object issuer verifies; backing verified; key removed from the descriptor (`key_not_listed`, signature still valid); descriptor for another domain (`key_not_listed` or a distinct code); descriptor not supplied.
- [x] The app's credential trust command reports backing for an object-issuer credential using a stubbed resolver in tests.

## Pointers

- Org identity contract: `crates/alexandria-verify/src/org.rs` (`OrgDescriptor`, `descriptor_from_txt`).
- App TXT lookup: grep `descriptor_from_txt` under `src-tauri/src/`.
- Classification: `crates/alexandria-verify/src/trust.rs`; app command `get_credential_trust`.
- Envelope: `crates/alexandria-verify/src/vc/mod.rs` (`issuer: Did`).

## Notes

2026-11-16: built.

- Envelope: `VerifiableCredential.issuer` is `vc::CredentialIssuer`, an untagged enum of a `Did` string or an `IssuerObject` (`id` plus every other property, kept as received so the proof covers them). `id()` and `as_str()` return the DID; `name()` and `domain()` read string properties; `CredentialIssuer::organisation(id, name, domain)` builds the object; `From<Did>` and `PartialEq<Did>` keep call sites mechanical (`did.into()`, `issuer.id()`). `sign_credential` keeps a preset issuer object whose `id` is the signing DID and otherwise writes the string form, so learner credentials and vectors 01–18 are byte-identical.
- `org::organisation_backing(credential, Option<&OrgDescriptor>)` returns `OrganisationBacking` (tag `outcome`): `verified { name, domain }` with the descriptor's published name and domain (not the issuer object's), `key_not_listed`, `wrong_domain`, `invalid_descriptor` (fails `check_descriptor`), `descriptor_not_supplied`. Decision: a descriptor for another domain is the distinct code `wrong_domain`, and a malformed descriptor is `invalid_descriptor`, so neither reads as the organisation withdrawing a key. A string issuer names no domain, so a supplied descriptor is judged on its key list alone. Backing never changes the verification result.
- Vectors 19–23: object issuer (backing `descriptor_not_supplied`), backing verified, key not listed, wrong domain, invalid descriptor. The vector format gains optional `organisationDescriptor` and `expectOrganisationBacking`; the independent verifier and the demo stdlib verifier read the issuer object's `id`, and the independent verifier checks backing.
- Classification: `trust::classify_credential_with_organisation` wraps `classify_credential` (unchanged) and sets `organisation` on `verified_issuer_signed` when a descriptor is supplied or the issuer names a domain. Self-claims carry none. `CredentialTrust::self_issued()` gives `Some(true)` for self-claim states, `Some(false)` for issuer-signed, `None` for invalid or pending. `TRUST_CALCULATION_VERSION` is 3.
- App: `get_credential_trust` reads the stored issuer's domain, refreshes the per-domain descriptor cache (`org_trust::refresh_descriptor`: fresh cached copy, else one TXT lookup), then classifies with `org_trust::fresh_descriptor` as the resolver. A stale or unresolvable descriptor reads `descriptor_not_supplied`. `credential_trust_impl` takes the resolver as a closure. TypeScript mirrors `CredentialIssuer` (with `issuerDid`) and `OrganisationBacking`.
