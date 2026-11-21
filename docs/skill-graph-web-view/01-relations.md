# Relations: command, model and inspector

Triage: done
Repos: alexandria
Part of: ../skill-graph-web-view-spec.md

## Work

Bring the taxonomy's non-prerequisite relations (related, complementary, alternative) into the personal skill graph, following the spec's **Relations in the model**, **Inspector** and **Prerequisites are guidance** decisions.

- New Tauri command returning every relation row (skill id, related skill id, relation type), in the taxonomy commands module beside the prerequisite-edges command, registered in the handler list and the command guard's allowlist; a matching frontend type beside the existing graph-edge type.
- The atlas loads relations beside the prerequisite edges; a failed relations load degrades to none and never blocks the graph.
- Each atlas node gains its relations (other skill id and name, type, loaded flag), undirected so a relation appears on both skills, sorted like prerequisites.
- The shared inspector gains a Related group after the dependents, naming each relation's type in words; choosing an entry selects that skill. Works in Simple and Detailed today and needs no change when Web lands.
- The desktop-ui fixture backend serves relations from the bundled taxonomy.
- i18n keys for the group heading and the three relation types, English text in every locale.

## Tests

- Rust: in-memory database with migrations and bundled data; every bundled relation returned once with a known type.
- Atlas model unit tests: relations on both skills, sorted, loaded flags; missing load yields none.
- Explorer mount tests: the inspector lists relations and choosing one selects the skill.

## Notes

- 2026-11-21: Done. Merged into the integration branch in 78ed1dbc, then adjusted in the review-fix round (6dcc53e3). The relation type became a lowercase serde enum; an unknown type reads as related.
