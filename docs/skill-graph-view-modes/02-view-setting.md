# Remembered view: settings key

Triage: done
Repos: alexandria
Part of: ../skill-graph-view-modes-spec.md

## Work

Register the profile preference that remembers the chosen view, following the parent spec's **One preference, not per-surface view state** decision.

- Add `ui.skill_graph_view` to the backend settings registry: sync scope, enum of `simple` and `detailed`, default `simple`. Place it beside the other `ui.*` keys.
- Add the row to the key table in the settings doc, in the UI section.
- Add a small typed frontend accessor beside the existing graph preference composable: a two-way ref over the setting, narrowing unknown values to `simple`, exporting the mode type and a type guard. No consumer wiring in this ticket.

## Tests

Rust registry tests in the existing style: key present, sync scope, default `simple`, invalid value rejected. Vitest for the accessor: unknown or missing value reads as `simple`, set writes through.

## Notes

- 2026-11-14: Done. Implemented in 5af81cf8 and 8821a53e; b0c00a2d moved the allowed values onto the key definition and made inbound sync use the same validator. The key's row sits in the settings doc's skill-graph table, because that doc has no UI section.
