# Credentials live in a credentials module, not in a commands file

Triage: ready-for-agent
Repos: alexandria
Part of: alexandria/docs/architecture-deepening-spec.md
Blocked by: alexandria/docs/architecture-deepening/07-commands-depend-down.md

## Problem Statement

Credential issuance, status lists, revocation, storage, bundle export and offline verification all live in `src-tauri/src/commands/credentials.rs`. The file is 2,888 lines: about 1,640 of logic, about 1,240 of tests and 11 `#[tauri::command]` handlers. Twenty-one other commands files and `lib.rs` reach into it, most often for `load_issuer_key` (16 uses), `issue_credential_impl` and `IssueCredentialRequest` (9 each), and the status-list functions.

`lib.rs` runs two background jobs through the commands layer: the status-list publisher and `commands::auto_issuance::tick`. `commands/auto_issuance.rs` (492 lines) has no command handler at all and keeps its own copy of `allocate_status_index`.

The derived skill state store's tests are stuck in `commands/aggregation.rs` behind a `cfg(test)` `test_support` module, because their fixture issues credentials through `commands::credentials` (the 07 follow-up). Anyone looking for "how a credential is issued" finds it under a command.

## Solution

A top-level `crate::credentials` module owns everything about a credential that touches the database, the keystore or the network. `domain::vc` keeps the pure types, signing and ids. The commands file becomes thin wrappers over the module, and auto-issuance moves under it. One issuer value replaces the key-and-DID pair.

## User Stories

1. As a maintainer, I want issuance, status lists, revocation and verification callable without a command, so that the background jobs, auto-issuance and every command use one implementation.
2. As a maintainer, I want one `allocate_status_index`, so that a status-list fix cannot land in one copy and miss the other.
3. As a maintainer, I want the issuer's signing key and DID to travel as one value, so that a credential can never be signed with one key and stamped with another DID.
4. As a maintainer, I want the derived skill state store's tests next to the store, so that its tests and code change together and the `test_support` workaround goes away.
5. As an AI agent working on the codebase, I want the module named for credentials to own credentials, so that I never have to search the commands layer for them.

## Implementation Decisions

- New top-level module `src-tauri/src/credentials/` (sibling to `evidence`, `aggregation`, `enrollment`), with:
  - `issue.rs`: `IssuancePolicy`, `SessionFacts`, `build_integrity_assertion`, `IssueCredentialRequest`, `issue()`.
  - `status_list.rs`: ensure, allocate (one copy, replacing the one in auto-issuance), host, pending, signed, mark published, push, publish.
  - `lifecycle.rs`: revoke, suspend, reinstate.
  - `store.rs`: get, list, list filtered, `parse_credential_payload`, `credential_entry`, the payload JSON limits.
  - `bundle.rs`: bundle types, export (plain and signed), offline verification of a credential and of a bundle, `rejection_reasons`.
  - `issuer.rs`: `Issuer { key, did }` with a constructor from the keystore, replacing `load_issuer_key(&State)`. It needs no `State`, only what it reads.
  - `auto.rs`: all of `commands/auto_issuance.rs` (`tick`, `issue_for_observation`, `credential_for_witness_tx`).
- Drop the `_impl` suffix (`credentials::issue`, `credentials::revoke`, `credentials::list`, …).
- `commands/credentials.rs` keeps the 11 handlers and `publish_status_lists_quietly`, which also runs the organisations sync and stays command-side orchestration. Handler names and payloads are unchanged.
- `lib.rs`'s status-list publisher and auto-issuance tick call `crate::credentials` directly.
- The derived skill state store's tests move from `commands/aggregation.rs` to `aggregation::store`. Then delete the `test_support` module and keep `parse_credential_type` private.
- No schema change, no command renamed, no change to signed credential bytes.

## Testing Decisions

- The 45 tests in `commands/credentials.rs` and the auto-issuance tests move with their code, changed only in paths and in the `Issuer` call shape. Each move step must show as a move under `git diff -M --color-moved`.
- Add a test for `Issuer` only if nothing already covers the locked-vault refusal.
- The direction guard in `scripts/check-tauri-commands.mjs` must still pass with nothing new exempted.
- Run fmt, clippy `-D warnings`, the full `alexandria-node` tests (default and `tutoring-video`), `check-tauri-commands` and its tests, and the iOS and Android checks.

## Out of Scope

- Other commands-to-commands imports beyond credentials (holder pull, talent index, organisations sync).
- Moving the other background-task logic `lib.rs` calls (organisations, sync, guardian).
- A crate-wide typed command error.
- Changing the credential format, status-list format or any Cloud contract.

## Further Notes

- Follow-up named by ticket 07. Branch from `refactor/arch-integration`, since this needs `clock::now_rfc3339` and the direction guard.
- Order: status lists, then issuance, lifecycle and store, bundle and verification, `Issuer`, auto-issuance, then the aggregation tests. Commit after each step, with tests green at every step.
