# 02 Coverage statement registry

Triage: done
Type: task
Repos: alexandria
Part of: alexandria/docs/credential-trust-spec.md
Blocked by: none

Spec sections: *Coverage statements stay resolvable*, stories 11–13, 48.

## What to build

Every coverage statement ever issued stays resolvable from its digest, offline, from the verifier library.

- Move the coverage statement text and generation from `alexandria-studio` into `alexandria-verify`, as a registry that holds every published text version. `alexandria-studio` and the app use the registry from the verifier library. Version `1` (today's text) stays byte-identical, so every existing digest still resolves.
- Add version `2` with the attestation line reworded: the issuing device verified the attestation, and a reader relies on it only as far as the credential's attestation evidence verifies. New sessions use version `2`. Keep the English-only, quoted-verbatim rule.
- A lookup by digest returns the statement for any version. A test pins that no published version's digest changes.
- A credential issued with an integrity assertion carries the standard VC 2.0 `relatedResource` property naming its statement: `id` = `{host}/coverage-statements/{digest}` when the issuer's profile has a status host configured, else `urn:alexandria:coverage-statement:{digest}`; plus a digest of the statement's bytes in a standard `relatedResource` digest form. The integrity assertion keeps `coverageStatementDigest`.
- The verifier checks, when `relatedResource` names a statement, that its digest matches `coverageStatementDigest`; a mismatch rejects.
- Bundles keep carrying the statements their credentials name.

## Acceptance criteria

- [x] Every digest a version-1 statement ever produced resolves from the verifier library with no app code.
- [x] New credentials name a version-2 statement whose attestation line says who verified.
- [x] `relatedResource` is present on credentials with an integrity assertion and absent otherwise; URL form with a status host, URN form without.
- [x] Interoperability vectors added for: a credential naming a version-1 statement, a version-2 statement, and a `relatedResource` digest mismatch (reject).
- [x] The verifier library still passes its no-I/O dependency test.

## Pointers

- Statement generation today: `crates/alexandria-studio/src/integrity/coverage.rs` (`COVERAGE_VERSION`).
- Assertion building: `src-tauri/src/credentials/issue.rs` (`build_integrity_assertion`).
- Status host and URN fallback: `src-tauri/src/credentials/status_list.rs` (`host`, `ensure`).
- Vectors: `crates/alexandria-verify/tests/vectors.rs` and its regeneration switch.
- Credential envelope: `crates/alexandria-verify/src/vc/mod.rs`. Hosting is ticket 12 (Cloud).

## Notes

2026-11-16: built.

- The registry is `alexandria_verify::coverage`: `statement`, `statements`, `resolve` (every version), `CURRENT_VERSION = "2"`, `VERSIONS`. `alexandria-studio` keeps its typed `coverage_statement` / `coverage_statements` / `resolve_coverage` as an adapter over it; `CertifiedSignal` and `CertificationRef` moved into the verifier library.
- `CoverageStatement.profile` and `.rung` are strings in the verifier library rather than the studio enums. The JSON bytes are unchanged, and a verifier can read a statement from a bundle without knowing every profile name.
- A test pins all 40 published digests (20 for version 1, captured before the move, and 20 for version 2). Version 1 bytes are unchanged.
- Version 2 attestation line: "A platform attestation that an unmodified build of the app on a genuine device ran the session, verified by the issuing device when the credential was issued. A reader relies on it only as far as the credential's attestation evidence verifies." No other line changed.
- `relatedResource` is an array of `{ id, digestSRI, mediaType }`, with `digestSRI` set to `sha256-<base64>` of the statement bytes and `mediaType` set to `application/json`. Only an assertion that names a statement gets one, so an assertion without a profile gets none.
- Verifier check: the digest embedded in a statement address (URN, or a URL path ending in `/coverage-statements/{digest}`) must equal `coverageStatementDigest`. The check needs no registry lookup and no BLAKE3, so the independent JS verifier implements it too. A mismatch sets `coverageStatementMismatch` (left out of the JSON while false, so existing vectors are byte-identical) and rejects. `digestSRI` is for readers who fetch the bytes; the verifier does not resolve and compare it.
- Vectors 13 (version 1, digest only), 14 (version 2, URL form) and 15 (mismatch, URN form, reject).
