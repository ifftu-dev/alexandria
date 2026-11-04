# Atlas: accessibility, honest wording and stable fields

Triage: done
Repos: alexandria
Part of: alexandria/docs/review-remediation-spec.md

## Task

- **Accessible names.** Nodes, list items and previews build their accessible name from the skill name, its earned or not-yet-earned state, and its Bloom level number and name. The checkmark becomes decorative.
- **Focus indicator.** Add an on-map indicator, visible whatever the inspector state, showing the active prerequisites/dependents focus, its depth and its skill, with a clear action.
- **Prerequisite wording.** "Prerequisites satisfied" means at least one prerequisite, all earned. Skills with none show "No prerequisites" and get no readiness emphasis.
- **Stable field slots.**
  - Each subject field gets an explicit, stable atlas slot in the bundled taxonomy, exposed on the subject-field domain type in Rust and TypeScript.
  - Colour and region order come from the slot. New fields take the next unused slot.
  - The palette has at least as many distinct light/dark colours as there are bundled fields, plus a documented wrap rule.
  - Check whether the slot affects the taxonomy digest used for Cloud sync, and update the digest if it does.
- **Shared presenter.** One atlas presentation module owns field name, field colour and Bloom label for the sidebar graph and the explorer. Layout spacing becomes named constants, the skill-return shape gets a named type, and guards replace the tab, Bloom-rank and field-ID casts.

Covers spec stories 48–53 and 60–62.

## Acceptance

- Component Vitests assert the accessible names and the indicator with the inspector collapsed.
- A test adds a field and shows the existing fields keep their colours and regions.
- The desktop UI atlas specs pass on Chromium and WebKit.
- The Rust seed tests cover the slots.

## Completion evidence — 2026-11-04

What changed:

- **Accessible names.** A shared atlas presenter (`atlasPresentation.ts`) names each skill from its name, earned or not yet earned, and a Bloom level number and name. Earned skills give their current earned level, Ready skills their learning-outcome level, for example "Big-O Analysis, earned, Bloom level 2 Understand". Map nodes, list items, overview previews and the sidebar graph's earned previews use it. Every checkmark is `aria-hidden`.
- **Focus indicator.** While a prerequisites or dependents focus is active, the map shows "Showing prerequisites of {skill} · {depth}" in a live status line, with a Clear focus button. It sits with the map, so it shows whatever the inspector state.
- **Prerequisite wording.** `prerequisitesSatisfied` needs at least one prerequisite, all earned. A skill with none says "No prerequisites". The atlas adds no readiness emphasis beyond that inspector line.
- **Stable slots.** `bootstrap/public_taxonomy.json` gives each subject field an `atlas_slot`. The slots keep the colours fields already had. Migration 32 adds `subject_fields.atlas_slot` with a unique index, the bundled upsert writes it, and `SubjectFieldInfo` carries it in Rust and TypeScript. The atlas puts each field in the map cell of its slot and colours it with `slot % 8`. A field the taxonomy does not place takes the slot after every placed one. The wrap rule is documented on `ATLAS_PALETTE_SIZE`. The palette has 8 light and 8 dark colours for 8 bundled fields.
- **Digest.** `TaxonomySnapshot::from_reference` hashes only skills and prerequisites, so the slot leaves the Cloud exchange digest and `TAXONOMY_SNAPSHOT_REVISION` unchanged.
- **Presenter, constants, guards.** The explorer, sidebar graph and skills page take field name, field colour and Bloom label from the presenter. Layout spacing is named constants derived from node and region size. One `regionCamera()` replaces the `+400/+300` and `+144/+400, +290` offsets. `SkillReturn` is a named type. `isAtlasTab`, `isBloomRank` and a field-ID type predicate replace the casts.
- New strings are translated by hand into all nine locales. `docs/database-schema.md` is regenerated.

Red → green:

- `skillAtlas.test.ts`: zero-prerequisite skills are not satisfied, unplaced fields take the next slot, and `isBloomRank` rejects non-ranks.
- `SkillAtlasExplorer.test.ts`: "No prerequisites"; accessible names in previews, the list and the map with decorative checkmarks; the focus indicator visible with the inspector collapsed and cleared from the map; adding a field keeps every existing field's colour and region.
- `SidebarSkillGraph.test.ts`: the earned preview's accessible name, with a decorative checkmark.
- `atlasPresentation.test.ts`: colour from slot, wrap past the palette, muted for unassigned.
- `colorTokens.test.ts`: the palette matches `ATLAS_PALETTE_SIZE` in both themes, and no two bundled fields share a colour.
- `useSkillAtlas.test.ts`: `isAtlasTab`.
- Rust `db::bundled::every_bundled_field_keeps_its_atlas_slot` (pinned slots), `a_new_bundled_field_takes_the_next_unused_atlas_slot` (distinct, no gaps) and `commands::taxonomy::subject_fields_carry_their_bundled_atlas_slot`. These failed (`atlas_slot` missing), then passed.
- `skill-atlas.spec.ts` asserts the focus line with the inspector collapsed. `personal-atlas.spec.ts` asserts the new accessible names.

Checks: the targeted Vitest files, `npx playwright test` on the skill-atlas and personal-atlas specs (Chromium and WebKit, 31 passed, 1 skipped as before), `npx vue-tsc -b --noEmit`, `npm run i18n:keys`, `npm run i18n:parity`, `npm run i18n:no-raw-text`, `npm run check:tauri-commands`, `npm run check:csp`, `cargo test -p alexandria-node --lib` for `db::`, `commands::taxonomy` and `p2p::taxonomy` (73 passed), `cargo fmt --check`, `cargo clippy -p alexandria-node --all-targets -- -D warnings`, `scripts/check-docs.sh`.

Deviations:

- The atlas had no readiness emphasis to remove. Zero-prerequisite skills now lose the inspector's "Prerequisite evidence available" line. Spec story 51's stronger emphasis for ready-next skills is not added.
- Existing tests were updated, not just extended: they now find skills by the new accessible names instead of `name · status`, and the pinned schema counts include migration 32 and its index.
- The default and field-focus cameras now share one framing. The default view sits 10 map units higher and, on narrow screens, frames a region's first column.
- The inspector's relation buttons keep their "Inspect {name}" labels. Their visible earned or Ready text is not part of the name.

