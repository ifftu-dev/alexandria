# Surfaces: bind the remembered view, fixtures and browser checks

Triage: done
Repos: alexandria
Part of: ../skill-graph-view-modes-spec.md
Blocked by: 02-view-setting.md, 03-explorer-modes.md

## Work

Wire the explorer's view mode to the remembered preference on both personal surfaces, following the parent spec's **Surfaces** and **Fixture** decisions.

- Skills page graph tab and the expanded modal bind the explorer's view-mode prop and event to ticket 02's accessor. One preference for both surfaces. Locking a profile already clears the settings cache; confirm the next profile starts from its own value.
- Open skill and return keep the view.
- Desktop-ui atlas fixtures accept a `view` query parameter (`simple` | `detailed`) that seeds the setting in the fixture backend.
- Add browser checks for the Simple view at desktop and phone widths in light and dark alongside the existing Detailed checks.
- Sidebar compact summary and the public instructor graph page are unchanged.

## Tests

Extend the personal surfaces tests: the view is read from the setting on page and modal; writing it on one surface changes the other; lock then unlock another profile returns to that profile's value; Open skill and return keep the view. Run the desktop-ui suite.

## Notes

- 2026-11-14: Done. Implemented in 90c26a91 and e305d20c, with fixes e3de48b4 and c7564026. Page and modal share the remembered view; fixtures take `view=simple|detailed`; browser checks cover Simple at 1280 and 390 in light and dark.
