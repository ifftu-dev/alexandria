# Contributing to Alexandria

Thanks for wanting to work on this. Alexandria is learning infrastructure —
free for learners, permanently — and contributions are welcome.

## Licensing: read this first

Alexandria is **open core**. Almost everything is MIT Expat. Two small
subtrees are not:

- `src/ee/` — enterprise frontend
- `src-tauri/src/ee/` — enterprise backend

Those are licensed under the IFFTU Enterprise License (see `LICENSE.md`
inside each, and the carve-out in the root `LICENSE.md`).

**External contributions are accepted to MIT paths only.** CI rejects pull
requests that add or modify files under either `ee/` directory. This is not
a judgement about the contribution — it is that we cannot accept
externally-authored code into a proprietary tree without a contributor
agreement we do not currently have.

If you want to work on something and are not sure which side of the line it
falls on, read `docs/enterprise-boundary.md` or open an issue and ask before
writing code. The short version: anything a learner needs in order to learn,
be assessed, or hold and verify their own credentials is MIT and always will
be. That includes the assessment engine, the grader plugins, credential
issuance, and all verification logic.

## Before you push

Run the local mirror of CI:

```bash
./scripts/check.sh          # everything
./scripts/check.sh --fast   # skip the Rust test suite (slowest step)
```

This runs the same gates CI does: `cargo fmt --check`, `cargo clippy -D
warnings`, the Rust test suite, both build configurations, the enterprise
boundary guard, the Tauri command guard, i18n parity, `vue-tsc`, and vitest.

## Code style

Enforced by CI on every push and pull request:

- **Rust** — `cargo fmt --check` and `cargo clippy -- -D warnings` must pass.
- **Vue / TypeScript** — `vue-tsc -b --noEmit` must pass in strict mode
  (`noUnusedLocals`, `noUnusedParameters`, `noFallthroughCasesInSwitch`,
  `noUncheckedSideEffectImports`).

Match the surrounding code — its naming, its comment density, its idiom.
Comments here tend to explain *why*, not *what*.

## Things that are load-bearing

A few invariants are enforced by tests because breaking them silently would
be bad. If a change makes one of these fail, that is a signal to reconsider
the change, not to update the test:

- **Privacy.** Published profiles must not carry account-private fields;
  guardian data never rides gossip or device sync; `SYNCABLE_TABLES` is
  pinned at exactly three tables; a private profile queried by username
  answers `NotOwner`, not `Private`.
- **Answer keys never leave the host.** Assessment grading happens
  backend-side; the client never receives `correct_indices`.
- **Grader determinism.** `wasm_runtime.rs::grader_config()` and the ABI v1
  contract are frozen. Every historical score must stay re-derivable —
  third-party re-derivation is a product, not just a nicety. Extend via
  manifest budgets and new plugins, never by changing the runtime config.
- **The community build never depends on `ee/`.** A checkout with both
  `ee/` directories deleted must build, typecheck, and pass tests.

## Commits and pull requests

- Keep unrelated changes in separate commits.
- Explain *why* in the commit body when the reason isn't obvious from the
  diff.
- If your change touches documented behaviour, say so in the PR — docs
  updates are usually wanted, but we would rather discuss than have them
  drift.
