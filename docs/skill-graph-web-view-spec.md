# Skill graph: Web view, three view modes and the prerequisite model

Status: implemented
Updated: 2026-11-21
Triage: done
Repos: alexandria

Follows the implemented [Simple and Detailed view modes](skill-graph-view-modes-spec.md) spec, which is frozen; this spec carries the change. Reference research: [skill graph UI research](skill-graph-ui-research.md) and the earlier [skill graph visual research](skill-graph-visual-research.md).

Interactive preview of the Web view: [skill-graph-web-view-preview.html](skill-graph-web-view-preview.html) (the bundled public taxonomy with an invented learner, both themes). The preview is inspiration for layout, encoding and emphasis; the decisions below and the parent specs win where they differ.

## Problem Statement

The personal skill graph has two views. Simple answers "what have I earned and what comes next?" as a layered flow, and Detailed is the atlas for investigating a field or a skill. Neither shows the whole graph at once: on the bundled taxonomy Simple is about 2,600px tall at desktop width, and Detailed is built for studying one field or one skill at a time. Neither shows how skills relate beyond prerequisites, although the taxonomy already records related, complementary and alternative skills.

The sidebar's skill graph is not a graph. It is a list of fields with a few earned skill names, and a learner has to expand it to see any shape at all.

Behind both sits a wider question: the taxonomy stores prerequisites as a directed acyclic graph, and a strict reading of that graph does not match how people learn. People learn out of order, come back to the same idea at greater depth, and reach the same skill by different routes. The graph is useful as a map; it must not be read, or drawn, as a set of gates.

## Solution

A third view, **Web**, joins Simple and Detailed. It draws every skill as a dot and every connection as a thin line, laid out by attraction and repulsion so that connected skills sit together and each subject field forms a loose cluster. The whole taxonomy fits one screen. Hovering or selecting a dot lights its neighbours and fades the rest; names appear for what is lit, and for everything once the learner zooms in. The learner can narrow the view to the skills around the selection, one to three steps out.

The Web view is the first to draw the taxonomy's non-prerequisite relations, as dotted lines the learner can turn off. The inspector, shared by all three views, gains a Related group that names them.

The switch in the graph heading offers Web, Simple and Detailed, in that order. The sidebar's skill graph becomes a small, fully working Web view: hover, select, drag, zoom and pan work right in the sidebar, and it shares its selection, toggles and dragged dots with the graph expanded from it. That expanded graph opens in Web unless the learner has chosen otherwise there. The skills page keeps Simple as its default. Each of the two surfaces remembers its own last view.

In the Web view a dot's size is the Bloom level the learner has earned for that skill, so the web is a picture of the learner's own progress.

The prerequisite graph stays as it is stored, and this spec fixes how it is to be read: prerequisites are guidance, never a gate. The larger changes to the model that learning research points to are recorded here as a recommendation and deferred to their own spec.

## User Stories

1. As a learner, I want a Web view that shows every skill on one screen, so that I can see the shape of the whole taxonomy without scrolling.
2. As a learner, I want the switch to offer Web, Simple and Detailed with the active view marked, so that I always know which view I am in and what the others are.
3. As a learner, I want the sidebar's skill graph to be a small picture of my graph, so that I can see my progress as a shape without opening anything.
4. As a learner, I want the graph I expand from the sidebar to open in the Web view by default, so that expanding the small picture shows the same picture, larger.
5. As a learner, I want the skills page to keep opening in Simple by default, so that the page built for "what comes next" still answers that first.
6. As a learner, I want the expanded graph and the skills page to each remember the view I last chose there, so that a choice on one does not undo the default I rely on in the other.
7. As a learner with several profiles on one device, I want both remembered views to belong to the profile, so that one profile's choices never leak into another.
8. As a learner with paired devices, I want both remembered views to follow me across devices, so that the graph opens the same way everywhere.
9. As a learner in the Web view, I want connected skills drawn near each other, so that position tells me what belongs together.
10. As a learner in the Web view, I want each subject field's skills to gather into a loose cluster in the field colour, so that I can find a field by eye.
11. As a learner in the Web view, I want earned skills drawn as filled dots, ready-next skills as rings and other skills as pale dots, so that I can read my progress without text.
12. As a learner in the Web view, I want no skill ever presented as locked, so that the graph invites me in rather than shutting me out.
13. As a learner in the Web view, I want a dot's size to show the Bloom level I have earned for that skill, so that the web shows how deep my progress goes, not only how wide.
14. As a learner in the Web view, I want a short legend naming the three dot states, saying that size is the Bloom level earned and naming the dotted lines, so that I never have to guess the encoding.
15. As a learner in the Web view, I want hovering a dot to light it, its connections and its neighbours and fade the rest, so that I can read one skill's surroundings in a crowded graph.
16. As a learner in the Web view, I want selecting a dot to keep that emphasis until I clear it, so that I can study a neighbourhood without holding the pointer still.
17. As a learner in the Web view, I want names shown for the selected skill and its neighbours, so that the dots I am looking at are never anonymous.
18. As a learner in the Web view, I want every name shown once I zoom in far enough, and a Names toggle to show them at any zoom, so that I can read the graph when I choose to.
19. As a learner in the Web view, I want names to stay the same size on screen as I zoom, so that zooming in gives me room, not bigger text.
20. As a learner in the Web view, I want prerequisite lines to show their direction whenever they are lit, so that I can tell what a skill builds on from what it leads to.
21. As a learner in the Web view, I want an Arrows toggle that shows direction on every prerequisite line, so that I can read order across the whole graph when I need it.
22. As a learner in the Web view, I want related, complementary and alternative skills joined by dotted lines, so that I can see connections that are not prerequisites.
23. As a learner in the Web view, I want a toggle that hides the dotted lines, so that I can look at prerequisites alone.
24. As a learner in the Web view, I want the dots to stay where they are when I toggle lines, names or arrows, so that I do not lose my place.
25. As a learner in the Web view, I want the same layout every time I open the graph, so that I can learn where things are.
26. As a learner in the Web view, I want to narrow the graph to the skills around my selection, one, two or three steps out, so that I can study a neighbourhood without the rest of the taxonomy.
27. As a learner in the Web view, I want the narrowed graph to fill the canvas and show all its names, so that a small neighbourhood is easy to read.
28. As a learner in the Web view, I want one action that returns to the whole graph, so that narrowing is never a dead end.
29. As a learner in the Web view, I want to zoom and pan with buttons, the pointer, touch gestures and the keyboard, so that I can move around on any device.
30. As a learner in the Web view, I want a Fit action that brings everything shown back into view, so that I can recover from any zoom or pan.
31. As a learner in the Web view, I want search to light the skills that match and fade the rest, so that I can find a skill by name in the picture.
32. As a learner in the Web view, I want choosing a search result or a skill in the list to select it and bring it into view, so that finding and looking are one step.
33. As a learner in the Web view, I want choosing a field chip to fit that field's cluster and fade the others, so that I can look at one field at a time.
34. As a learner in the Web view, I want to drag a dot aside and have its neighbours follow and settle, so that I can untangle a crowded spot.
35. As a learner in the Web view, I want Reset to put every dot back in its usual place, so that rearranging never costs me the layout I know.
36. As a learner in the Web view, I want the graph to stay still once settled, so that nothing drifts while I read.
37. As a learner who has asked for reduced motion, I want the Web view to appear already settled and dragging to move only the dot I hold, so that my preference is honoured.
38. As a learner, I want the skill I have selected to stay selected when I switch between any two views, so that I can find a skill in Web and study it in Detailed without reselecting.
39. As a learner, I want nodes to travel between their positions when I switch views, so that I can see it is one graph.
40. As a learner, I want my Web zoom and position to survive a trip through another view and back, so that switching never throws away my exploration.
41. As a learner, I want the inspector to show the same information and actions in the Web view as in the others, so that Open skill and the learning actions are always one step away.
42. As a learner, I want the inspector to list related, complementary and alternative skills under their own heading, so that I can follow those connections from any view.
43. As a learner, I want choosing a related skill in the inspector to select it, so that I can walk the graph from the inspector.
44. As a learner, I want prerequisites described as what a skill builds on, never as requirements I have failed, so that I feel free to start where my interest is.
45. As a learner in the sidebar, I want the small graph to use the same dots, colours, sizes and states as the Web view, so that the small picture and the large one read the same way.
46. As a learner in the sidebar, I want hovering or focusing a dot to light its neighbours and show their names, so that the small graph is readable without opening anything.
47. As a learner in the sidebar, I want clicking a dot to select it in place and show a one-line caption with its name, its state, Open skill and Expand, so that I can act on a skill straight from the sidebar.
48. As a learner in the sidebar, I want to drag dots, zoom with the wheel or a pinch and pan by dragging the background, so that the small graph works like a real graph.
49. As a learner in the sidebar, I want every name shown once I zoom in far enough, so that I can read a corner of the graph without expanding it.
50. As a learner in the sidebar, I want one Reset button on the small graph that restores the zoom and every dot's usual place, so that I can always get back to the picture I know.
51. As a learner, I want the sidebar graph and the expanded graph to share the selected skill, the Names, Arrows and Related toggles and any dots I have dragged, while each keeps its own zoom and position, so that they feel like one graph at two sizes.
52. As a learner in the sidebar, I want the field names and earned counts to stay under the small graph as buttons that dim the other fields, so that colour has a name and I can pick out one field.
53. As a learner in the sidebar, I want the expand action to stay, so that I can open the full graph without aiming at a dot.
54. As a learner with a very large taxonomy, I want the sidebar graph to show my earned skills and the skills one step from them, so that the small graph stays readable.
55. As a new learner with a very large taxonomy, I want the sidebar graph to show the skills that build on nothing, so that I see places to start instead of an empty picture.
56. As a learner with a collapsed sidebar, I want the skill graph entry to behave as it does today, so that this work does not change the collapsed layout.
57. As a learner on a phone, I want the Web view to fit the screen, respond to pinch and drag, and leave the page scrollable outside the canvas, so that it works under a thumb.
58. As a learner on a phone, I want the three-way switch to fit the width with touch-sized targets, so that I can change views easily.
59. As a keyboard user, I want the Web graph to be one stop in the Tab order, with the arrow keys moving to the nearest dot in that direction and Enter selecting it, so that a hundred dots never stand between me and the rest of the page.
60. As a keyboard user, I want the dot I move to with the arrow keys to light its neighbourhood as hovering does and carry the same accessible name as in the other views, so that I get the same emphasis and information as a pointer user.
61. As a screen-reader user, I want each view change announced in a whole sentence, so that I know which view is active.
62. As a screen-reader user, I want the list of skills to stay available in the expanded Web view, so that I can reach every skill regardless of layout.
63. As a screen-reader user, I want the sidebar graph to have a text summary and real buttons for its fields, so that the picture never hides information from me.
64. As a learner with a large taxonomy, I want the Web view to open promptly and stay responsive, so that growth in the taxonomy does not make the sidebar slow.
65. As a learner, I want the theme tokens honoured in the Web view in light and dark appearance, so that it is as readable at night as the other views.
66. As a learner in any supported language, I want the new view name, toggles, legend, caption and announcements translated, so that the Web view reads like the rest of the app.
67. As a learner, I want switching views never to change what I have earned, what is ready or what builds on what, so that the views differ only in presentation.
68. As an instructor viewing another learner's public graph, I want that page unchanged by this work, so that the public graph keeps its current behaviour until it gets its own spec.
69. As a developer, I want a pure layout function for the Web view that depends only on the taxonomy, so that positions are deterministic and testable without a DOM.
70. As a developer, I want one Web graph component used by the explorer and the sidebar, so that the two sizes cannot drift apart.
71. As a developer, I want one command that returns every non-prerequisite relation, so that the graph does not ask for relations one skill at a time.
72. As a developer, I want the view mode to remain a prop and event on the explorer, now with three values, so that surfaces and tests control it the same way as before.
73. As a developer, I want the fixture parameter to accept the Web view, so that browser checks and screenshots cover all three.
74. As a maintainer, I want both remembered views stored as registered settings keys with documented scope, allowed values and defaults, so that sync refuses unknown values and the keys appear in the settings documentation.
75. As a maintainer, I want the rule that prerequisites are guidance written into the skills documentation, so that later work on any surface does not turn the graph into a gate.

## Implementation Decisions

**Names and order.** The three modes are called Web, Simple and Detailed in the UI, in that order in the switch. The stored value for the new mode is `web`. The reference product that inspired the view is never named in copy. "Atlas" stays as the internal name of the model and explorer.

**Two preferences, one per surface.** This reverses the parent spec's "one preference shared by every surface" decision, because the two surfaces now have different defaults. The existing key `ui.skill_graph_view` becomes the skills page's remembered view: sync scope, allowed values `web`, `simple` and `detailed`, default `simple`. A new key, `ui.skill_graph_expanded_view`, is the remembered view of the graph expanded from the sidebar: sync scope, the same allowed values, default `web`. Both declare their allowed values on the key, so local writes and inbound sync rows with any other value are refused by the shared validator. The frontend accessor takes the surface and returns a two-way ref that reads the surface's default for anything missing or unknown. A value already saved under the existing key keeps its meaning for the page; the expanded graph starts from its own default. No migration.

**Explorer contract.** The explorer's view mode prop and update event take three values. The page binds the page key and the expanded modal binds the expanded key. The per-surface atlas view state gains what the Web view needs and nothing is reset by a mode switch: a Web camera separate from the Detailed camera, the Names, Arrows and Related toggles, and whether the graph is narrowed to the selection. Narrowing depth reuses the existing neighbourhood depth. Selection, field focus, search query and inspector state are shared with the other views as today.

**What is remembered.** Only the two view choices are stored. Web cameras, the toggles, narrowing and dragged positions live in the atlas composable for as long as the app runs and reset on restart or profile lock, as the Detailed camera and search do today.

**One Web graph component.** The Web drawing and interaction (dots, lines, names, emphasis, keyboard roving, drag, wheel, pinch and pan) is one component used by the sidebar and by the explorer. It takes the model, the positions, the selection and emphasis inputs, the toggles, a camera and a compact flag, and emits selection, active-dot, camera and drag events; the explorer and the sidebar own the tools around it. The explorer keeps one DOM node per skill across its three views by rendering its existing node layer through this component's geometry when the view is Web, so nodes can travel between views; the implementer chooses the exact split (for example a node slot, or shared geometry and interaction composables used by both the component and the explorer's node layer) as long as the sidebar and the explorer cannot drift apart and the travel transition works.

**Relations in the model.** A new backend command returns every row of the non-prerequisite relation table as skill id, related skill id and relation type, mirrored by a frontend type and registered with the command guard. The atlas loads it beside the prerequisite edges. Each atlas node gains its relations: the other skill's id and name, the relation type, and whether that skill is loaded, sorted like prerequisites. A relation is undirected and appears on both skills. A failure to load relations degrades to no relations and does not block the graph.

**Web layout.** A new pure function beside the existing layouts computes Web positions from the model alone. It is a force layout run to rest inside the function, not an animation: seeded starting positions on a ring by field slot, repulsion between all nodes, springs along prerequisite edges, weaker and longer springs along relations, a pull toward each field's centroid so fields cluster, and a pull to the centre so unconnected skills stay in frame. The seed, tick count and force constants are named constants beside the layout. The same model always yields the same positions. Relations always take part in the layout, whether or not they are drawn, so toggling them never moves a dot. Repulsion uses a spatial grid or an equivalent cut-off so cost grows close to linearly; the layout must meet the existing atlas performance budget on the scale fixture. The result is memoised per model. No new dependency is added for this.

**Web node encoding.** One DOM node per skill in every explorer view; the view swaps its skin, not its element. In Web a node is a dot in the field colour: filled when earned, a ring on the page background when ready next, a pale tint of the field colour otherwise; the pale state is called Ready, never Later or Locked. Dot size is the Bloom level earned: six sizes for levels 1 to 6 whose areas grow in even steps, and one smaller base size for a skill not yet earned and for an earned skill whose level is unavailable (drawn filled, so no level is ever invented). The sizes are named constants. The earned Bloom number is not drawn on the dot; it stays in the accessible name and the inspector. The accessible name is the string the presenter already produces. A selected node gets the primary-colour focus ring used elsewhere. Size never feeds the layout, so earning a level never moves a dot.

**Brand-new learners.** Ready next keeps its current rule, which needs at least one earned prerequisite, so a learner with nothing earned sees a web of pale base-size dots with no rings. Marking skills that build on nothing as starting points belongs to the prerequisite-model follow-up, not to this spec.

**Web lines.** Prerequisite edges are thin neutral lines drawn between dot centres, under the dots, so only the part between two dots shows. Relations are dotted neutral lines. Lines keep a constant width on screen at any zoom. A lit prerequisite edge always shows an arrowhead at the dependent dot's edge; the Arrows toggle, off by default, shows arrowheads on all prerequisite edges. The Related toggle, on by default, shows or hides relation lines and the relation part of a neighbourhood.

**Web names.** At rest in the whole graph, names are hidden. A name is shown for the selected node, the hovered or focused node and their one-step neighbours; for every shown node when the camera scale passes a named threshold; for every shown node when the graph is narrowed to the selection; and for every node when the Names toggle is on. Names keep a constant size on screen and sit by the dot with a background-coloured outline so lines do not cut through them. Names never overlap each other and never cover a firm dot such as the selected one: a name sits under its dot when there is room, otherwise on the first free side (above, then beside), stepping further out when no side is free. In the compact sidebar graph a neighbour's name with no room is left out instead.

**Web emphasis.** Hover and keyboard focus light the one-step neighbourhood for as long as they last. Selection keeps it. Everything outside the lit set fades; nothing glows. Prerequisite and dependent focus from the inspector reuse the existing neighbourhood state. Search fades nodes that do not match the query. Field focus fades other fields and fits the camera to the focused field. Clearing selection or Reset removes all fading.

**Narrowing.** "Whole graph" and "Around selection" are two pressed buttons in the Web tools. Around selection is disabled until a skill is selected. It shows only nodes within the chosen number of steps, one to three, following prerequisites, dependents and, when the Related toggle is on, relations; positions are unchanged and the camera fits what is shown. Choosing another skill while narrowed re-centres the narrowing on it. Narrowed, nothing fades at rest, because what is shown is already the neighbourhood; hover and focus still light their own neighbourhood. The steps setting is the neighbourhood depth the inspector uses, so changing it in either place changes both.

**Camera.** The Web view has zoom in, zoom out and Fit controls, drag to pan, pinch on touch, and the pan controls the Detailed map already has, sharing that map's interaction code where it can. Wheel zoom follows the Detailed map's rule: in the explorer a plain wheel scrolls the page and ctrl or cmd with the wheel (a trackpad pinch) zooms. The compact sidebar graph zooms with any wheel while the pointer is over it. On touch the canvas takes pinch and drag, and the page scrolls outside it.

**Keyboard.** The Web graph is a single stop in the Tab order, in the explorer and in the sidebar. While it has focus, the arrow keys move a roving active dot to the nearest shown dot in that direction, the active dot lights its neighbourhood as hover does and is announced by its accessible name, Enter or Space selects it, and Escape clears the selection. The active dot starts at the selection, or the dot nearest the centre. The explorer's skill list keeps every skill reachable as a separate route. The arrow keys never pan while the graph has focus; panning stays on the pan controls.

**Dragging and stillness.** A dot can be dragged, in the explorer and in the sidebar. While it is held, a live run of the same forces lets its neighbours follow, and the run stops within the emphasis motion duration after release. Dragged positions are one set per graph, shared by every Web surface, and last until Reset, a change of graph, a profile lock or restart; they are not stored. A refresh that only changes the learner's earned states, such as a new credential, keeps the graph and so keeps the dragged dots. With reduced or off motion there is no live run: only the held dot moves. At rest nothing moves. The layout appears already settled when the view opens; settling is never shown.

**Tools in Web.** Shown: the switch, search, field chips, Whole graph and Around selection with steps, Names, Arrows, Related, zoom and Fit, Reset, the list and the inspector. Hidden with their state kept: Bloom emphasis and the Detailed map tools that have no meaning here. The Web tools sit on one row; the field chips, led by an All fields chip that clears field focus, sit on one scrolling row; zoom in, zoom out, Fit and the pan controls sit in a small group in the graph's corner so the graph starts high on the page.

**Reset.** In the explorer, Reset keeps its current meaning and in Web also restores every dragged dot and fits the camera. It keeps the Names, Arrows and Related toggles, which are display preferences rather than view state. The sidebar's Reset restores the sidebar camera and every dragged dot; it does not clear the selection.

**Legend.** The Web legend names filled (earned), ringed (ready next) and pale (ready) dots, says that colour means field and size means the Bloom level earned, and names the dotted line.

**Transition.** Switching between any two views follows the parent spec's transition rules: nodes travel between positions and sizes on a motion token duration, edges fade out and back, only the learner's own switch animates and is announced, and reduced or off motion makes it instant. No literal durations; the two motion guard tests stay green.

**Inspector.** Unchanged in the three views apart from a new Related group after the dependents, listing each relation with its type in words (related, goes well with, an alternative to). Choosing an entry selects that skill. The inspector keeps its existing "Prerequisites" heading and never says required, locked or missing.

**Sidebar graph.** The sidebar widget keeps its expand action and counts line and replaces the per-field earned skill rows with the Web graph component in compact form, sized to the widget's width. It is fully working: hover and keyboard emphasis with names for the lit set, click to select, drag, wheel and pinch zoom while the pointer is over it, pan by dragging the background, names for every shown dot past the zoom threshold, and one small Reset button. Its lines follow the shared Names, Arrows and Related toggles; it has no toolbar of its own, so search, narrowing and the toggles themselves live in the expanded graph. While the pointer is over the graph the wheel zooms it instead of scrolling the sidebar.

**Sidebar caption.** Selecting a dot in the sidebar keeps the selection in place and shows a short caption under the graph: the skill's name and its state in words, wrapping as needed, with an Open skill action and an Expand action on the row below. Open skill uses the existing open-skill action, and coming back does not reopen the expanded graph; Expand opens the expanded graph with the selection kept. With nothing selected the caption is the counts line.

**Sidebar and expanded graph share state.** The sidebar reads and writes the expanded modal's per-surface view state for selection, field focus and the three toggles, so expanding continues from what the learner was looking at and a toggle changed in the expanded graph applies in the sidebar. Each keeps its own Web camera. Dragged positions are shared as described under Dragging.

**Sidebar fields.** Under the graph, each field keeps its name button and earned count. Choosing a field focuses it in place: the other fields fade and the sidebar camera fits the field. Choosing it again clears the focus. The expand action still opens the expanded graph.

**Very large taxonomies in the sidebar.** Up to a named node limit the sidebar shows the whole graph. Above it, the sidebar shows the learner's earned skills and every skill one step from them in either direction; when nothing is earned it shows the skills that build on nothing. The limit is tuned during the build so the bundled taxonomy always shows in full and the scale fixture does not. The expanded graph always shows every skill. A short line under the sidebar graph says when it is showing a part of the graph.

**Sidebar accessibility.** The graph has a text summary as its accessible description, the fields are real buttons, the caption actions are real buttons, and the graph follows the keyboard decision above. The sidebar graph is drawn only when the sidebar is expanded and the model is loaded, and reuses the memoised layout. Loading, empty and error states stay as they are. The collapsed sidebar entry is unchanged.

**Expanded graph.** The modal opened from the sidebar binds the expanded key, so it opens in Web by default. Because it shares the sidebar's view state, it opens with the sidebar's selection and field focus.

**Prerequisites are guidance.** This spec writes down what the atlas model already does and makes it a rule for every skill graph surface: a skill is earned or ready, and nothing else. "Ready next" is a suggestion computed from prerequisites; it never blocks opening, learning or earning a skill, and no view may draw an unearned skill as locked or unavailable. The skills and reputation document's skill graph section gains this rule.

**i18n.** New keys under the atlas namespace for the Web view name, its whole-sentence announcement, the Web tools, the Web legend, the Related group and the three relation types, the sidebar graph's label, summary, caption, Reset and partial-graph line. All locales gain the keys through the existing translation script; parity and no-raw-text checks must pass.

**Fixture.** The desktop-ui atlas fixtures accept `view=web` beside the two existing values, and the personal-surfaces fixture can seed either key. The fixture backend serves relations from the bundled taxonomy.

**Documentation.** The settings document gains the new key and the changed meaning of the existing one. The skills and reputation document's normative skill graph section gains the guidance rule and names the relation types; no separate decision record is written until the prerequisite-model follow-up starts. The frontend area notes gain a line on the Web layout's determinism if the implementer finds one is needed.

## Testing Decisions

A good test drives the explorer through what a learner can see and do: mount it, press the switch, hover and select nodes, read accessible names, count nodes and lines, assert emitted events. It does not read component internals, styling-only class names, or positions pixel by pixel except through the pure layout function.

Seams, highest first:

1. **Explorer mount tests** (existing harness in the explorer's test file): the switch offers three views and its pressed state follows the prop; node count and every accessible name are identical in all three; selection survives every pair of switches; in Web, hover and focus light the one-step neighbourhood and selection keeps it; names appear for the lit set, past the zoom threshold, when narrowed and with the Names toggle; the Related toggle hides relation lines without changing any node position; Around selection is disabled without a selection and shows only the neighbourhood with one to three steps; search and field focus fade the right nodes; Bloom emphasis is hidden in Web and intact on return; the inspector lists relations and choosing one selects it; reduced motion makes the switch and the drag synchronous.
2. **Pure Web geometry** (new unit test in the style of the Simple layout test): layout determinism across calls; every node placed with finite coordinates inside a bounded extent; no two nodes closer than the minimum distance; linked nodes closer on average than unlinked ones; nodes of one field closer on average than nodes of different fields; identical output with and without relations being drawn; a time budget on the scale taxonomy. Also the size rule (six even area steps, base size for unearned and unknown level), the nearest-dot-in-a-direction rule used by the keyboard, and the sidebar subset rule (whole graph up to the limit, earned plus one step above it, starting points when nothing is earned).
3. **Web graph component** (new mount test): one Tab stop; arrow keys move the active dot and light its neighbourhood; Enter selects and emits; Escape clears; hover lights and leaving restores; drag emits and, with reduced motion, moves only the held dot; names follow the lit set, the zoom threshold and the Names toggle; the Related toggle hides dotted lines without moving dots.
4. **Personal surfaces** (existing personal atlas test file): the page reads its key with default Simple and the expanded graph reads its key with default Web; writing one does not change the other; both follow a profile lock and unlock; Open skill and return keep the view.
5. **Sidebar graph** (existing sidebar test file): the graph is one Tab stop and its dots carry the presenter's names; clicking a dot selects it in place and shows the caption, whose Open skill and Expand work; Expand opens the expanded graph with the same selection; a toggle set in the expanded graph applies in the sidebar; a field button fades the other fields and a second press restores them; Reset restores dragged dots; above the node limit only earned skills and their neighbours show, and starting points show when nothing is earned; loading, empty and error states are unchanged.
6. **Settings registry** (existing Rust registry and store tests): both keys exist, are sync scoped, hold the three allowed values and their own defaults, and refuse other values on local write and on inbound sync.
7. **Relations command** (new Rust test in the style of the taxonomy command tests, in-memory database with migrations and bundled data): every bundled relation is returned once with a known type.
8. **Atlas model** (existing model unit tests): relations appear on both skills, sorted, with loaded flags; a missing relations load yields none.
9. **Browser checks** (existing desktop-ui atlas spec): Web screenshots at desktop and phone widths in light and dark; the sidebar graph; the existing performance check repeated in Web on the scale fixture.

Prior art: the explorer test file's finder by accessible name and its prop-driven view round trips; the Simple layout test's determinism and placement cases; the personal atlas tests for setting-backed state across lock and unlock; the registry tests for keys with allowed values; the desktop-ui atlas performance spec.

## Out of Scope

- Any change to how Simple and Detailed look or behave, beyond the three-way switch, the Related group in the shared inspector and travel to and from Web. The line-diagram and ledger directions in the UI research are candidates for a later spec.
- Level-aware prerequisites and either-or prerequisite groups (see the recommendation below). They change the taxonomy's stored and gossiped shape and need their own spec and decision record.
- Any change to the prerequisite or relation data in the bundled taxonomy, to ready-next rules, or to credential-derived Bloom.
- Storing dragged positions, per profile or otherwise.
- A canvas or WebGL renderer. The Web view uses the same DOM nodes as the other views.
- Dot size or colour driven by anything other than the earned Bloom level and the field.
- Changing the ready-next rule, including marking starting points for new learners.
- A toolbar, search or narrowing inside the sidebar graph.
- The public instructor graph page and its renderer.
- The collapsed sidebar entry and the mobile tab bar.
- Native-speaker review of new copy (tracked as before with the parent specs).

## Further Notes

**Recommendation on the prerequisite model.** A directed acyclic graph is a sound model of curriculum dependency and a poor model of learning. It is right for ordering a learning path, computing ready next, guaranteeing a layout with no cycles and keeping the taxonomy simple enough to govern. It falls short in five ways. Most prerequisites help rather than strictly gate, but an edge says "required". Learners reach a skill by different routes, which knowledge space theory models with either-or prerequisite sets and a plain graph cannot. Learning loops back to the same idea at greater depth, which edges between whole skills forbid. Mastery is graded and fades, so a skill often needs only an earlier level of its prerequisite. And experts who author edges misjudge what novices find hard, so authored edges need checking against outcomes. This paragraph is the author's synthesis and was not source-checked in the way the research documents were.

The recommendation is to keep the graph as the store of hard prerequisites and change what surrounds it, in this order:

1. *In this spec.* Read prerequisites as guidance on every surface, and show the relations the taxonomy already holds.
2. *Next, own spec and decision record.* Let a prerequisite edge name the Bloom level it needs. The graph of skill-and-level pairs stays acyclic while whole skills may then depend on each other at different levels, and ready next can be computed against the level a learner actually holds.
3. *With it or after.* Let a skill declare alternative prerequisite sets, so that "any one of these" is expressible. The existing alternative relation is the seed of this.
4. *Later.* Compare authored edges with attempt outcomes and flag edges the data does not support, for the taxonomy's governance to review.

Items 2 and 3 change the bundled taxonomy, its digest, the taxonomy gossip payload and the hosted taxonomy sync, which is why they are not folded into a view-mode spec.

**What the Web view is for.** It answers "what is near this?" and "what does the whole thing look like?". It does not answer "what comes first?": position carries no order, and direction is shown only on lit lines unless Arrows is on. Simple remains the view for order, which is why the skills page keeps it as the default.

**Sparse data.** The bundled taxonomy has 103 skills, 89 prerequisite edges and 24 relations, under one prerequisite per skill, and six skills with no prerequisite edge at all. A plain force layout scatters such a graph; the field pull is what keeps it legible, and it is the main thing to tune against real taxonomies.

**Dot size.** The reference product sizes dots by incoming links, which the earlier research found does not carry over. Size here is the Bloom level earned, chosen so the web shows the depth of the learner's own progress; the cost is that size no longer marks the skills that lead to the most others, and a new learner sees dots of one size.

**Interaction model.** The sidebar graph follows the reference product's sidebar graph, which offers hover emphasis, drag, zoom and pan directly on the small graph and keeps filters elsewhere. Unlike the reference product, a click selects in place rather than navigating, because a skill page is a bigger jump than opening a note.

**Mock.** The preview in this folder predates the decisions on dot size, keyboard focus and the sidebar graph; it sizes dots by dependents, gives every dot its own Tab stop and has no sidebar. The decisions above win.

**Parent criteria.** The atlas spec's criteria on always-visible meaning, Ready never Locked, accessible inspection, stillness and reduced motion apply to the Web view and the sidebar preview as they do to the other views.

## Notes

- 2026-11-21: Implemented through tickets 01 to 06 and one review-fix round; the Implementation Decisions above were updated to match what was built. Gates passed after merging the latest main: Rust fmt and clippy, strict vue-tsc, 814 Vitest tests, i18n parity and no-raw-text, the Tauri command guard and CSP hashes; 189 desktop-ui browser checks passed twice before that merge. The review round brought the top of the expanded graph at 1280 by 800 from 529px to 244px, overlapping names to zero, and Web pan from about 68ms to about 35ms of main-thread time per frame against the 50ms budget; the pan budget checks now run alone and measure main-thread CPU time. CI machines measured that pan at about 70ms, so the camera was taken off the render path: a pan moves one layer, a zoom writes each dot's box and size directly, and no dot re-renders; the 1,500-skill pan-and-zoom check now measures about 18ms locally. Outstanding: machine translation of the new keys for the eight non-English locales, native-speaker review of that copy, and validation in the native WebView. The prerequisite-model follow-up (level-aware edges, either-or prerequisite sets, starting points) needs its own spec and decision record. Later changes start a new spec.
