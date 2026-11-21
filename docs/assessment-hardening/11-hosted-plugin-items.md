# 11 Plugin items in hosted banks

Triage: done
Type: task
Repos: alexandria-cloud, alexandria
Part of: alexandria/docs/assessment-hardening-spec.md
Blocked by: 09, 10

Spec sections: *Item types in hosted banks*, story 37.

## What to build

- A hosted bank item may be a plugin item: the bundled plugin identifier, its public prompt and starter, and hidden test cases the device never receives.
- The device sends the submission; the service runs the exact bundled grader with the hidden cases and grades. Free-text model-graded items stay out.
- Import accepts plugin items; the console lists them.

## Acceptance criteria

- [x] HTTP tests: a plugin item's hidden cases never appear in a candidate response; a correct submission scores, a wrong one does not.
- [x] The service refuses a plugin identifier or grader bytes that do not match the bundled manifest.
- [x] App shows a hosted plugin item with the existing editor and sends the submission.
- [x] Checks pass in both repos.

## Pointers

- Bundled grader rule in the workspace `AGENTS.md` (*Credential authority*); editor plugins and the wasm grader in the app.

## Notes

2026-11-20: built in both repos.

- Cloud, bundled plugins: the four graded editor plugins' manifests and grader bytes are vendored under `vendor/alexandria-plugins/` in the app's `plugins/builtin/<slug>/` layout, recorded in `vendor/checksums.json`; `scripts/check-vendor.py --source <app>/crates` holds them to the app's bytes through a new `sources` map. `bundled_plugins` identifies a plugin as the app does, by the BLAKE3 of its manifest (`plugin_cid`) and the `grader.blake3` the manifest names, and checks the vendored grader against that digest before compiling it. Graders run under Wasmtime 36 with the app's desktop determinism configuration, budgets and ABI v1, copied on purpose like the wire structs.
- Cloud, banks: migration 0060 adds `kind`, `plugin_cid`, `grader_cid`, `starter` and `hidden_tests` to bank items (a plugin item has no options or key) and `submission` and `grader_score` to attempt items. JSON import takes `"kind": "plugin"` with `plugin_cid`, `grader_cid`, `prompt`, `starter`, `hidden_tests` (1 to 50 of `name`, `stdin`, `expected_stdout`) and `time_limit_secs` (15 minutes when left out); a plugin or grader digest that is not the bundled one is refused with the row named, and any kind other than `choice` or `plugin` is refused as free text. Hidden tests are read by administrators only, like answers. The bank page lists plugin items (plugin name, starter, hidden test count, the cases for administrators) and the bundled plugins an item may name.
- Cloud, attempts: a plugin item is sent as `kind` `plugin` with `plugin.cid`, `plugin.starter`, the prompt as `question`, no options and `select` `submission`. The answer carries `submission`; the bundled grader runs over the hidden tests on a blocking thread, and full marks is correct. A grader that traps or runs out of fuel or memory scores the submission 0.
- App: `wire::Item` gained `kind` and `plugin`, `AnswerBody` gained `submission`, and `hosted_attempt_answer` takes it. The client refuses an item naming a plugin the app does not bundle (`plugin_not_bundled`) and holds a plugin item's prompt and starter in the bank guard; the command installs the bundled plugin from the app's own bundle if needed. The page shows a plugin item in the existing editor through `PluginHost` in a new hosted mode, which hands the submission to the page and grades, stores and announces nothing.
- Seams: Cloud HTTP tests for import (accepted, mismatched plugin or grader refused, free text refused) and for a hosted attempt (hidden cases in no candidate reply; a correct submission scores, a wrong one does not, through the real bundled JavaScript grader); a Cloud unit test that every vendored grader matches its manifest; the app client against the stub service (submission sent, nothing stored or logged, bank guard holds prompt and starter; unbundled plugin refused); a Vitest for `PluginHost`'s hosted mode.
- Deviations: CSV import stays choice-only. The plugin's submit is left pending in hosted mode, since the page replaces the item once the service has it. An item whose plugin or grader a later vendoring drops can no longer be graded: answering it fails with a server error, so retire such items before re-vendoring. The vendored graders add about 17 MB to the Cloud repository and binary.
