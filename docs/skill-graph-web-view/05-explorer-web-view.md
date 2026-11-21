# Explorer: Web view, three-way switch and surface bindings

Triage: done
Repos: alexandria
Part of: ../skill-graph-web-view-spec.md
Blocked by: 03-view-settings.md, 04-web-graph-component.md

## Work

Add Web to the atlas explorer and bind the two surfaces to their own settings, following the spec's **Names and order**, **Two preferences, one per surface**, **Explorer contract**, **Narrowing**, **Tools in Web**, **Reset**, **Legend**, **Transition**, **Expanded graph**, **i18n** and **Fixture** decisions.

- Three-value view mode prop and event; switch reads Web, Simple, Detailed in that order with a glyph each; one whole-sentence announcement per view.
- Web view on the explorer's existing node elements using ticket 04's component and state; travel between all three views; tools as listed in the spec, Bloom emphasis hidden with state kept; Whole graph and Around selection with one to three steps; search and field focus fade and fit; Reset also restores dragged dots in Web; Web legend.
- Frontend accessor takes the surface and returns a two-way ref with that surface's default (`simple` for the page, `web` for the expanded graph); the page binds the page key, the modal binds the expanded key.
- Desktop-ui fixtures accept `view=web` and can seed either key; Web browser checks at desktop and phone widths in both themes; the performance check repeated in Web on the scale fixture.
- i18n keys with English text in every locale.

## Tests

Explorer mount tests per seam 1, personal-surface tests per seam 4, browser checks per seam 9 of the spec's Testing Decisions.

## Notes

- 2026-11-21: Done. Merged into the integration branch in f6f1e0cd, then adjusted in the review-fix round (6dcc53e3). The review round moved the explorer's Web-only code into its own adapter and made the tools compact; see the parent spec's Tools in Web decision.
