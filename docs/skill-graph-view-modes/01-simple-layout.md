# Pure Simple layout and shared layered-DAG utility

Triage: done
Repos: alexandria
Part of: ../skill-graph-view-modes-spec.md

## Work

Build the Simple view's layout as a pure function beside the existing atlas layout, following the parent spec's **Simple layout** and **Density** decisions. No component changes in this ticket.

- Extract the longest-path layering (layer = 1 + max prerequisite layer, cycle protection) out of the public instructor graph component into a shared utility under the skills utils, and make that component use the utility. Its rendering and behaviour must not change.
- Add a Simple layout function that takes the atlas model plus a viewport width and returns, for every node, a centre point and the canvas height. Wide viewports: layers left to right, each layer's nodes spread evenly down a fixed height. Below the existing narrow-map threshold: layers top to bottom, nodes spread across the width, alternate nodes staggered when a layer holds more than three. Within a layer sort by field slot then name. Coordinates depend only on taxonomy membership, IDs and field slots, never on credentials or view state.
- Export named constants for the disc size, the fixed wide height, the row pitch and the label legibility threshold (pitch below which non-essential Ready labels are hidden), and a helper that says whether a layer is dense.

## Tests

Vitest beside the existing layout code: layers equal longest prerequisite path; sort order within a layer; wide and narrow orientation; stagger rule; every node placed exactly once; determinism across calls; cycle protection; the instructor graph's existing tests still pass unchanged.

## Notes

- 2026-11-14: Done. Implemented in e80c86c0 and bcf47bef, then extended by 5de894aa, 81312204, 8b876948 and 0a23ce8d. The fixed wide height turned out to be a floor, not a rule: the final layout uses left-to-right columns only while every layer fits it, and otherwise wraps layers onto full-width rows at any width. The parent spec's Simple layout and Density decisions describe the result.
