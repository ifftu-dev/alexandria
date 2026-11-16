# 12 Cloud coverage statements

Triage: done
Type: task
Repos: alexandria-cloud
Part of: alexandria/docs/credential-trust-spec.md
Blocked by: 02

Spec sections: *Coverage statements stay resolvable* (address bullet), story 11.

## What to build

- Sync the vendored `alexandria-verify` from the app integration branch (including ticket 02) and update `vendor/checksums.json`.
- Serve `GET /coverage-statements/{digest}` publicly and cacheably from the vendored registry: the statement's exact bytes as JSON for a known digest, 404 for an unknown one. Long cache lifetime: a digest's content never changes.

## Acceptance criteria

- [ ] An HTTP test fetches a version-1 and a version-2 statement by digest and checks the bytes hash to the digest.
- [ ] Unknown digest returns 404; a malformed digest returns 400.
- [ ] Vendor check, fmt and clippy pass.

## Pointers

- Status list serving for the same shape of route: `src/http/status_lists.rs`.
- Commits need `--signoff` (DCO).

## Notes

2026-11-16: built and merged into the Cloud integration branch (`alexandria-cloud` `feat/credential-trust`, merge `a0f7432`).

- `GET /coverage-statements/{digest}` serves the exact registry bytes for every published version as `application/json`, `Cache-Control: public, max-age=31536000, immutable`, readable cross-site like `/status-lists/`. Unknown digest 404; malformed (not `blake3:` + 64 lowercase hex) 400. Listed among the deliberately open routes.
- The vendor sync needed one Cloud fix: `hosted_status` reads the revocation entry of a multi-entry `credentialStatus`. Share-time suspension is not checked by Cloud yet.
- Statements that embed a certification resolve only when the caller supplies it; Cloud passes none, which holds while no certification is in force.
