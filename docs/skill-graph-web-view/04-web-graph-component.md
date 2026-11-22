# Web graph component and shared Web state

Triage: done
Repos: alexandria
Part of: ../skill-graph-web-view-spec.md
Blocked by: 01-relations.md, 02-web-geometry.md

## Work

The one Web drawing and interaction component used later by the explorer (ticket 05) and the sidebar (ticket 06), and the state they share. Follow the spec's **One Web graph component**, **Web node encoding**, **Web lines**, **Web names**, **Web emphasis**, **Keyboard**, **Camera**, **Dragging and stillness** and **What is remembered** decisions.

- Component: dots by state and earned-Bloom size, prerequisite and dotted relation lines with constant on-screen width, arrowheads on lit prerequisite edges and on all of them with Arrows, names with constant on-screen size for the lit set, past a zoom threshold and with Names, hover and roving-keyboard emphasis, selection, one Tab stop with arrow-key roving, Enter, Space and Escape, wheel and pinch zoom, background-drag pan, dot drag with a live force run that stops within the emphasis motion token, and only the held dot moving under reduced or off motion. A compact flag for the sidebar.
- Shape the component so ticket 05 can keep one DOM node per skill across the explorer's three views (see the spec's decision for the options); choose and document the split in the component's doc comment.
- Shared state in the atlas composable: per-surface Names, Arrows, Related and narrowed flags and a per-surface Web camera in the atlas view state; one set of dragged positions per model shared by all surfaces, cleared on Reset, model change and profile lock. Memoise the layout per model.
- Motion tokens only; both motion guard tests stay green. Theme tokens and field colour tokens only.

## Tests

New mount test for the component covering seam 3 of the spec's Testing Decisions; unit tests for the shared state (drag positions shared across surfaces and cleared on lock).

## Notes

- 2026-11-21: Done. Merged into the integration branch in d2fb4d91, then adjusted in the review-fix round (6dcc53e3). The review round memoised one Web graph and layout per model and passed it to every Web surface.
