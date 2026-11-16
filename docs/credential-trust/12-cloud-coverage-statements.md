# 12 Cloud coverage statements

Triage: ready-for-agent
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
