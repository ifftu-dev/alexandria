# Explorer: Simple and Detailed views

Triage: ready-for-agent
Repos: alexandria
Part of: ../skill-graph-view-modes-spec.md
Blocked by: 01-simple-layout.md

## Work

Give the atlas explorer its two views, following the parent spec's **Explorer contract**, **Simple node encoding**, **Simple emphasis**, **Density**, **Legend and strip**, **Transition**, **Where the switch lives** and **i18n** decisions. The preview HTML in the docs folder shows the intended look; the spec wins where they differ.

- Add a view-mode prop (`simple` | `detailed`, type from ticket 02's accessor module or a shared type module if 02 has not merged yet) and an update event. Default rendering when the prop is `simple`.
- Segmented control in the explorer heading with pressed state and glyphs; emits the update. Announce the change through the existing status region.
- Simple skin on the same node elements: disc with earned Bloom number for earned (nothing for an unavailable level), ring plus dashed halo for ready next, muted ring for ready; name under the disc; accessible name unchanged. Hide field regions, cards, search, Bloom select, field focus nav, zoom and pan tools in Simple; keep the list, reset and the inspector. Hidden controls keep their state.
- Simple emphasis: edges touching the selection are active; nodes and edges outside one hop dim; inspector relationship focus and depth still work; clearing selection or reset restores all.
- Density: always label earned, ready next, selected and neighbours; below the legibility threshold other Ready labels appear only on hover or focus.
- Field chips gain per-field earned counts in both views. Legend line is per view.
- Transition on switch using motion tokens only; edges fade out, redraw, fade in; reduced and off motion make it instant. Both motion guard tests must stay green.
- i18n keys under the atlas namespace for the group label, the two view names, Simple legend items and the announcement; run the translation and parity scripts.

## Tests

Extend the explorer's existing mount tests: default skin, switch round trip keeps node count, accessible names, selection, neighbourhood focus, camera and query; Simple hides the listed controls while the list remains; the switch emits and follows the prop; reduced motion makes the switch synchronous; evidence feedback highlights in Simple.
