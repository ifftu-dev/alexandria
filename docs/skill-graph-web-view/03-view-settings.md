# Settings: Web value and the expanded-graph key

Triage: done
Repos: alexandria
Part of: ../skill-graph-web-view-spec.md

## Work

Backend half of the spec's **Two preferences, one per surface** decision.

- The existing skill graph view key accepts `web` beside `simple` and `detailed`; default stays `simple`.
- New key for the graph expanded from the sidebar: sync scope, the same three allowed values, default `web`, declared the same way as the existing key so the shared validator refuses other values on local write and inbound sync.
- Frontend changes are not part of this ticket; the accessor and bindings change in ticket 05.

## Tests

Extend the existing settings registry and store tests: both keys exist, are sync scoped, carry the three values and their own defaults, and refuse other values on local write and on inbound sync.

## Notes

- 2026-11-21: Done. Merged into the integration branch in a83f5a9e.
