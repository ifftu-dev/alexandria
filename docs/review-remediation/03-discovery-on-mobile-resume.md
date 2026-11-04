# Discovery on mobile resume

Triage: ready-for-agent
Repos: alexandria
Part of: alexandria/docs/review-remediation-spec.md

## Task

Add a command that runs one Cloud discovery pass for the current profile. On mobile, the webview's existing resume handler calls it alongside push re-registration.

Discovery passes are serialised. A resume pass that overlaps the background loop's pass joins that pass rather than starting a second one. Results go through the existing de-duplication, so push and discovery can't double-record. Register the command in the handler list and the command check.

Covers spec stories 9–10.

## Acceptance

- After resume, a missed interview invitation is recorded without waiting for the 60-second loop.
- A resume during an active pass doesn't send duplicate requests and doesn't create duplicate rows.
- Rust tests against the fake Cloud server, the notifications composable Vitest, and `check:tauri-commands` pass.
