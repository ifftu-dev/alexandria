# Sidebar: fully working Web graph

Triage: done
Repos: alexandria
Part of: ../skill-graph-web-view-spec.md
Blocked by: 04-web-graph-component.md

## Work

Replace the sidebar widget's per-field earned skill rows with ticket 04's component in compact form, following the spec's **Sidebar graph**, **Sidebar caption**, **Sidebar and expanded graph share state**, **Sidebar fields**, **Very large taxonomies in the sidebar**, **Sidebar accessibility** and **Reset** decisions.

- Fully working graph in the sidebar: hover, roving keyboard, click to select in place, drag, wheel and pinch zoom, pan, names past the zoom threshold, one Reset button.
- Caption under the graph for the selection with Open skill and Expand; counts line when nothing is selected.
- Reads and writes the expanded modal's surface view state for selection, field focus and toggles; keeps its own Web camera; shares dragged positions.
- Field buttons with earned counts focus a field in place (fade others, fit camera), second press clears; expand action unchanged.
- Node limit and subset from ticket 02, with a short line when only part of the graph shows.
- Collapsed sidebar entry, loading, empty and error states unchanged.
- i18n keys with English text in every locale; a sidebar check in the desktop-ui browser spec.

## Tests

Sidebar tests per seam 5 of the spec's Testing Decisions.

## Notes

- 2026-11-21: Done. Merged into the integration branch in 4bb23de9, then adjusted in the review-fix round (6dcc53e3). The review round strengthened the sidebar graph's lines, tints and dots in both themes.
