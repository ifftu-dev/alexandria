# 11 Plugin items in hosted banks

Triage: ready-for-agent
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

- [ ] HTTP tests: a plugin item's hidden cases never appear in a candidate response; a correct submission scores, a wrong one does not.
- [ ] The service refuses a plugin identifier or grader bytes that do not match the bundled manifest.
- [ ] App shows a hosted plugin item with the existing editor and sends the submission.
- [ ] Checks pass in both repos.

## Pointers

- Bundled grader rule in the workspace `AGENTS.md` (*Credential authority*); editor plugins and the wasm grader in the app.
