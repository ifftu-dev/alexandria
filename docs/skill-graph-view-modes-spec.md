# Skill graph view modes: Simple and Detailed

Status: active
Updated: 2026-11-14
Triage: ready-for-agent
Repos: alexandria

Follows the implemented [Personal Skill Graph atlas](skill-graph-atlas-spec.md) spec, which is frozen; this spec carries the change.

Interactive preview of the agreed design: [skill-graph-view-modes-preview.html](skill-graph-view-modes-preview.html) (sample data, both themes, phone and desktop widths). The preview is inspiration for layout and emphasis; the acceptance criteria below and the parent spec win where they differ.

## Problem Statement

The personal skill graph now ships as the atlas: subject-field regions, information-dense skill cards, search, Bloom emphasis, zoom and pan, a list and an inspector. That is the right tool for investigating one skill or one field, but it is a poor first glance. A learner opening the graph to answer "where am I, and what is next?" meets a map that needs reading before it says anything.

The earlier circular graph answered that question at a glance: filled and hollow discs joined by prerequisite arrows, nothing else. It was retired because it carried too little evidence for the atlas's other purposes. Learners should not have to choose between the two; both answer real questions.

## Solution

One explorer, two views, switched by the learner from a segmented control in the graph heading:

- **Simple** (the default): a flow of circular nodes and prerequisite connections. Nodes are filled when earned (carrying the earned Bloom number), ringed when ready next, hollow when ready. Field colour identifies the field; there are no field regions, cards, search, Bloom emphasis or zoom controls. Selecting a node traces what it builds on and what it unlocks and quietly dims the rest.
- **Detailed**: today's atlas, unchanged in meaning: field regions with counts and progress, skill cards with evidence lines, search, Bloom emphasis, field focus, zoom and pan, the list and the map tools.

Both views show the same skills, the same evidence and the same inspector. Selection and relationship focus carry across the switch, and nodes move between their two positions rather than reappearing, so the learner sees one graph wearing two lenses. The last chosen view is remembered per profile and applies to both the skills page and the expanded modal.

## User Stories

1. As a learner, I want the graph to open in the Simple view by default, so that my first glance shows what I have earned and what comes next without reading a map.
2. As a learner, I want a clearly labelled Simple / Detailed switch beside the graph title, so that I can find the other view without hunting through menus.
3. As a learner, I want the switch to be a segmented control that shows which view is active, so that I always know which lens I am looking through.
4. As a learner, I want the app to remember the view I last chose, so that I am not switched back to the default every time I open the graph.
5. As a learner with several profiles on one device, I want the remembered view to belong to the profile, so that one profile's choice never leaks into another.
6. As a learner with paired devices, I want my chosen view to follow me across devices like my other preferences, so that the graph feels like mine everywhere.
7. As a learner, I want the skills page and the expanded modal to share one remembered view, so that the graph does not change shape between the two surfaces.
8. As a learner in the Simple view, I want earned skills drawn as filled discs in their field colour carrying my earned Bloom number, so that my evidence is visible without text.
9. As a learner in the Simple view, I want skills whose prerequisites I have all earned drawn with a distinct ring, so that I can see my next steps at a glance.
10. As a learner in the Simple view, I want other unearned skills drawn hollow and labelled Ready in their accessible name, so that nothing is ever presented as locked.
11. As a learner in the Simple view, I want skill names beside the nodes, so that discs are never anonymous.
12. As a learner in the Simple view, I want prerequisite arrows to flow in one reading direction, so that I can follow a path from foundations to advanced skills.
13. As a learner in the Simple view, I want the whole graph to fit the width of the viewport without panning or zooming, so that the view stays simple.
14. As a learner in the Simple view, I want selecting a node to highlight the arrows into and out of it and dim unrelated nodes, so that I can trace one path without losing the rest.
15. As a learner in the Simple view, I want the dimming to clear when I clear the selection or reset, so that the overview comes back.
16. As a learner in the Simple view, I want no field regions, cards, search box, Bloom selector, zoom buttons or pan buttons, so that the view carries only what the glance needs.
17. As a learner in the Simple view, I want a short legend naming filled, ringed and hollow discs and saying that colour means field, so that I never have to guess the encoding.
18. As a learner in the Simple view, I want the field colour chips above the graph to stay, with earned counts per field, so that colour has a name.
19. As a learner in the Detailed view, I want everything the atlas does today, so that switching modes costs me nothing.
20. As a learner, I want the skill I have selected to stay selected when I switch views, so that I can glance in Simple and investigate in Detailed without reselecting.
21. As a learner, I want the inspector to show the same information and actions in both views, so that Open skill and learning actions are always one tap away.
22. As a learner, I want prerequisite and dependent focus from the inspector to work in both views, so that relationship tracing does not depend on the lens.
23. As a learner, I want my Detailed camera, search text and Bloom emphasis to survive a trip through Simple and back, so that switching views never throws away my exploration.
24. As a learner, I want nodes to travel from their Simple position to their Detailed position when I switch, so that I can see it is one graph.
25. As a learner who has asked for reduced motion, I want the switch to be instant with no travel, so that my preference is honoured.
26. As a learner on a phone, I want the Simple view to lay out top to bottom so that it fits a narrow screen without horizontal scrolling.
27. As a learner on a phone, I want the switch to be full width with touch-sized targets, so that I can change views with a thumb.
28. As a learner on a phone, I want the inspector bottom sheet to behave the same in both views, so that selecting a node always leaves room to explore.
29. As a keyboard user, I want the switch operable with the keyboard and announced as a pressed toggle, so that I can change views without a pointer.
30. As a keyboard user, I want every Simple-view node to be a focusable button with the same accessible name as in Detailed, so that I can explore the Simple view without a pointer.
31. As a screen-reader user, I want the view change announced through the existing status region, so that I know which view is active.
32. As a screen-reader user, I want the list of skills to stay available in both views, so that I can reach every skill regardless of layout.
33. As a learner with a large taxonomy, I want the Simple view to stay legible when there are many nodes, so that density never turns it into noise.
34. As a learner with a large taxonomy, I want the labels of earned, ready-next, selected and neighbouring nodes to stay visible even when other labels are thinned for density, so that what matters is always named.
35. As a learner, I want the Simple view to stay still once settled, so that nothing floats or pulses while I read.
36. As a learner who has just earned a skill or improved a level, I want the brief evidence feedback to show in whichever view is open, so that the celebration is not tied to a lens.
37. As a learner, I want switching views never to change what I have earned, what is ready or what is a prerequisite, so that the views differ only in presentation.
38. As a learner, I want my view choice cleared from the screen when I lock the profile, so that the next profile starts from its own preference.
39. As a learner, I want the theme tokens honoured in both views in light and dark appearance, so that the Simple view is as readable at night as the Detailed one.
40. As a learner in any supported language, I want the switch labels, legend and announcements translated, so that the new controls read like the rest of the app.
41. As a learner, I want the sidebar's compact field summary to stay as it is, so that a mode switch in the main graph does not reshape the sidebar.
42. As an instructor viewing another learner's public graph, I want that page unchanged by this work, so that the public graph keeps its current behaviour until it gets its own spec.
43. As a developer, I want a pure layout function for the Simple view that depends only on the taxonomy, so that positions are deterministic and testable without a DOM.
44. As a developer, I want the Simple view to reuse the layered longest-path algorithm already proven in the public graph, so that the two graphs can share one layout later.
45. As a developer, I want the view mode exposed as a prop and event on the explorer rather than hidden inside it, so that surfaces and tests control it the same way.
46. As a developer, I want a fixture parameter that opens the desktop-ui atlas fixture in either view, so that browser checks and screenshots cover both.
47. As a maintainer, I want the remembered view stored as a registered settings key with a documented scope and default, so that sync rejects unknown values and the key appears in the settings documentation.

## Implementation Decisions

**Names and default.** The two modes are called Simple and Detailed in the UI. Simple is the default for a profile that has never chosen. The earlier working names Network and Atlas do not appear in copy; "atlas" stays as the internal name of the model and explorer.

**Where the switch lives.** A segmented control in the explorer heading, next to the title and reset action, present on the skills page graph tab and in the expanded modal. The buttons use pressed state for the active view and carry a small glyph each (linked discs for Simple, a card grid for Detailed) beside the text label. The earlier floating on-canvas variant is dropped.

**One preference, not per-surface view state.** The chosen view is a profile preference, not part of the per-surface atlas view state. It is stored as a new settings key, `ui.skill_graph_view`, registered in the backend settings registry with sync scope, an enum of `simple` and `detailed`, and default `simple`. The frontend reads and writes it through the existing reactive setting accessor. Locking a profile clears the settings cache as it does today, which is what resets the on-screen view. The registry declares the allowed values on the key itself; local writes and inbound sync rows with any other value are refused, and the frontend still reads anything unknown as `simple`. The settings documentation's key table gains a row.

**Explorer contract.** The explorer gains a `view mode` prop with the two values and an `update` event for it. The page and the modal bind both to the setting. The per-surface atlas view state (selection, focus, query, Bloom emphasis, neighbourhood, camera, inspector collapsed) is unchanged in shape and is not reset by a mode switch. Detailed-only fields (camera, query, Bloom emphasis, field focus) are simply not rendered in Simple and are intact when the learner returns. Selecting a skill in Simple sets the selection and opens the inspector without moving the Detailed camera or field focus. A hidden search query does not filter the list in Simple. Field focus buttons name their earned count, as the chips do.

**Simple layout.** A new pure layout function beside the existing atlas layout computes Simple positions from the model alone. Layers come from the longest prerequisite path from roots (the algorithm already used by the public instructor graph; extract it to a shared utility rather than copying it). Within a layer, nodes sort by field slot then name so field colours cluster. The layout adapts to the taxonomy's shape. When every layer fits the fixed wide height at the minimum disc pitch, layers run left to right and each layer's nodes spread evenly down that height. Otherwise, and always below the existing narrow-map threshold, layers run top to bottom and each layer wraps onto as many rows as the viewport width needs, with row capacity derived from the width; alternate discs stagger only when names would otherwise be held short. The layout also decides how wide each name may be, so names stay clear of neighbouring discs and names. The whole graph fits the viewport width; height follows the layer and row count. No camera, no zoom. Selecting a skill from the list or an inspector relation scrolls its node into view when it is off screen.

**Simple node encoding.** One DOM node per skill in both views; the view swaps its skin, not its element. In Simple a node is a disc of a fixed size: earned skills are filled with the field colour and show the earned Bloom number (or nothing when the earned level is unavailable, never a fabricated Remember); ready-next skills have a field-coloured ring and a dashed halo; other ready skills have a muted ring. The name sits under the disc; no evidence line. The accessible name is the same string the presenter already produces for Detailed. A selected node gets the primary-colour focus ring.

**Simple emphasis.** Selecting a node marks edges touching it as active and dims nodes and edges outside the one-hop neighbourhood. Prerequisite and dependent focus from the inspector reuse the existing neighbourhood state and depth. Clearing selection or reset removes dimming. Bloom emphasis and search are not offered in Simple; the controls are hidden, not disabled, and their state is preserved.

**Density.** Labels are always drawn for earned, ready-next, selected and neighbouring nodes. When a row's pitch falls below a legibility threshold, remaining Ready labels are shown only on hover or focus and remain reachable through the list and the accessible name. This threshold is a named constant beside the layout. Tight wrapped rows can reach it at any width: on the bundled taxonomy at desktop width, plain Ready names appear only on hover or focus.

**Legend and strip.** The field chips above the graph stay in both views and gain per-field earned counts. The legend line below the canvas is per view: Simple names filled, ringed and hollow discs and says colour means field; Detailed keeps today's legend.

**Transition.** On a switch, node positions and sizes transition with a motion token duration; edges fade out, redraw at the new positions, and fade back in after the token duration. Only the learner's own switch animates and is announced; a change that arrives from the other surface or when the setting loads applies instantly and silently. Nodes travel only between Simple and the Detailed map, since the Detailed field-card overview has no node elements to travel to. Reduced and off motion settings make the switch instant through the existing root attribute. No literal durations; the two motion guard tests must stay green.

**Inspector and feedback.** Unchanged. The inspector reads the same selected node; the evidence feedback highlight applies to nodes in whichever view is mounted.

**Surfaces.** Skills page graph tab and expanded modal get the switch. The sidebar compact summary is unchanged. The public instructor graph page is unchanged.

**i18n.** New keys under the atlas namespace for the switch group label, the two view names, the Simple legend items, the field earned count and one whole-sentence announcement per view (never a view name composed into a sentence). All locales gain the keys through the existing translation script; parity and no-raw-text checks must pass.

**Fixture.** The desktop-ui atlas fixtures accept a `view` query parameter (`simple` or `detailed`) so browser checks can start in either view; the personal-surfaces fixture seeds it into the first profile's setting. Because Simple is the default, fixture URLs cited by the parent atlas spec open in Simple unless they add `view=detailed`.

## Testing Decisions

A good test drives the explorer through what a learner can see and do: mount it, press the switch, select nodes, read accessible names, count nodes and edges, assert emitted events. It does not read component internals, CSS class names used only for styling, or positions pixel by pixel except through the pure layout function.

Seams, highest first:

1. **Explorer mount tests** (existing harness in the explorer's test file): mount with a model and view, assert Simple is the default skin, switch to Detailed and back, assert node count and every accessible name are identical in both, assert selection and neighbourhood focus survive the switch, assert Simple hides search, Bloom select, zoom and pan controls while the list remains, assert the switch emits the mode update and the pressed state follows the prop, assert the Detailed camera and query props are untouched after a round trip, assert reduced motion makes the switch synchronous.
2. **Pure Simple layout** (new unit test beside the existing layout test style): layers equal longest prerequisite path, nodes sorted by field slot then name within a layer, wide and narrow orientations, stagger rule, determinism across calls, every node placed, cycle protection mirrors the public graph.
3. **Personal surfaces** (existing personal atlas test file): the remembered view is read from the setting on the page and the modal, writing it on one surface changes the other, locking the profile returns the next profile to its own value, Open skill and return keep the view.
4. **Settings registry** (existing Rust registry tests): the key exists, is sync scoped, has default `simple`, rejects other values.
5. **Browser checks** (existing desktop-ui atlas spec): a Simple screenshot pass at desktop and phone widths in light and dark, plus the existing Detailed checks with the fixture parameter.

Prior art: the explorer test file's `button` finder by accessible name and `setProps`-driven view round trips; the layout constants test that pins field palette size; the personal atlas tests for setting-backed state across lock and unlock; the registry tests that look up `ui.theme` and `ui.motion`.

## Out of Scope

- The public instructor graph page and its renderer. It may adopt the shared layer utility later under its own spec.
- The sidebar compact summary; it stays a field list with an explicit expand.
- Any change to the atlas model, credential-derived Bloom, ready-next rules or prerequisite data.
- A per-surface view mode (different views on the page and in the modal).
- Force-directed or physics layouts; the Simple view is a layered DAG.
- Zoom, pan or minimap in Simple.
- New backend commands or schema changes beyond the settings registry entry.
- Native-speaker review of new copy (tracked as before with the parent spec).

## Further Notes

- The preview in this folder was validated at desktop and phone widths in both themes with no overflow and no console errors. Its sample data has 13 skills in 3 fields; real taxonomies are larger, which is why the density rule exists.
- The parent spec's acceptance criteria 2, 3, 5 and 6 (always-visible meaning, Ready never Locked, accessible inspection, stillness and reduced motion) apply to Simple exactly as they do to Detailed.
- "Ready next" in Simple is the existing readiness emphasis, not a new state: it still means every counted prerequisite is loaded and earned, nothing more.
