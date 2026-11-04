# Remove public notification deep links

Triage: ready-for-agent
Repos: alexandria
Part of: alexandria/docs/review-remediation-spec.md

## Task

Remove the public `alexandria://notifications` and `alexandria://notification/<id>` links, and the inbox's open-on-query behaviour.

Do this in order:

1. Confirm that native push taps and held-tap replay route through the internal route only, so that removing the links breaks nothing. If any native path depends on them, move it to the internal route first.
2. List the debug-only notification emitter in the parent spec's decisions as debug-only. It stays refused outside debug builds.

Covers spec stories 14–16.

## Acceptance

- In the desktop UI suite, the removed links neither navigate nor mark anything read.
- Push-tap and held-tap tests still land on the right screen.
- The deep-link parser tests are updated.
- A test shows the debug emitter is refused in release configuration.
