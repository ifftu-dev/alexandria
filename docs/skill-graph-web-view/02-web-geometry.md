# Web geometry: layout, sizes, keyboard and sidebar subset

Triage: done
Repos: alexandria
Part of: ../skill-graph-web-view-spec.md

## Work

Pure, DOM-free functions for the Web view, beside the existing Simple layout module, following the spec's **Web layout**, **Web node encoding**, **Keyboard** and **Very large taxonomies in the sidebar** decisions. The preview HTML in the docs folder holds a working sketch of the force layout and field pull; its constants are a starting point, not a requirement.

- Web layout: deterministic force layout run to rest inside the function. Input is plain data derived from the atlas model (node ids with field slot, prerequisite pairs, relation pairs) so it does not depend on ticket 01; take relation pairs as an input that may be empty. Seeded ring start by field slot, repulsion with a spatial grid or cut-off, prerequisite springs, weaker longer relation springs, field-centroid pull, centre pull, named constants. Also a function that continues the same forces from given positions with some nodes pinned, for live dragging in ticket 04.
- Size rule: radius for an earned Bloom rank (six even area steps) and the smaller base size for unearned or unknown level.
- Nearest shown dot in a direction (up, down, left, right) from a given dot, for keyboard roving.
- Sidebar subset: whole graph up to a named node limit; above it, earned skills plus one step in either direction; when nothing is earned, skills with no prerequisites.

## Tests

New unit test file in the style of the Simple layout test, covering everything listed under seam 2 of the spec's Testing Decisions, including a time budget on a taxonomy of the scale fixture's size.

## Notes

- 2026-11-21: Done. Merged into the integration branch in 670f9135, then adjusted in the review-fix round (6dcc53e3). The review round made name placement one shared function for every Web surface and timed the layout budget in process CPU time.
