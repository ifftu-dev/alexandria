# 07 Hosted item banks

Triage: done
Type: task
Repos: alexandria-cloud
Part of: alexandria/docs/assessment-hardening-spec.md
Blocked by: none

Spec sections: *Hosted item banks*, stories 26–30, 36.

## What to build

- A hosted item bank per organisation and role: items with skill, difficulty, question, options, correct options and an optional time limit. Choice items only.
- Import from JSON or CSV, list and retire, through the API and a console page on the role. Import states that the hosted service will hold the items and answers. Every action is audited and tenant-scoped like the rest of the service.
- A role draws a configured number of items per skill. Publishing a role with a hosted bank is refused unless each skill holds at least three times that number of live (unretired) items; the refusal names the skill and the shortfall.
- An exposure counter per item, incremented when an item is shown (ticket 08 calls it); the console shows it and flags overexposed items. Advisory only.
- No answer key ever appears in a response readable by a candidate. Only administrators read keys.

## Acceptance criteria

- [x] HTTP tests against the real database: import (both formats, malformed rows rejected with row numbers), list, retire; tenant isolation; audit rows.
- [x] HTTP test: publish refused under the floor with the skill and shortfall, allowed at the floor; retired items do not count.
- [x] Migrations, fmt, clippy, console checks pass.

## Pointers

- Roles and assessment requests: `src/http/assessment.rs`, `src/hiring.rs`, `web/src/views/RolesView.vue`; tenancy `src/db/tenant.rs`; audit `src/db/audit.rs`; tests in `tests/` (`assessment_exchange.rs`, `hiring.rs`, `common/`).
- The hosted service holds no private-course content: nothing here reads or links courses.

## Notes

2026-11-20: built in the Cloud repo (`fec2f74`). Migration 0057 adds `item_bank_items` (kept on retire, never deleted; `exposures` counter), `item_bank_draws` (items per skill per attempt), `role_definitions.published_at` and six audit actions. Routes under `/api/roles/{id}`: `GET item-bank` (answers for administrators only, each such read audited), `POST item-bank/import` (JSON or CSV; refused without accepting the custody statement; all or nothing, 422 names malformed rows), `POST item-bank/items/{item}/retire`, `PUT item-bank/draws`, `POST publish` (409 names each skill and its shortfall when under three times the draw in live items) and `POST unpublish`. Console page at `/roles/:id/bank`. Interface for 08 in `src/item_bank.rs`: `draw_plan`, `live_items`, `item`, `record_exposure`, `shortfalls`, `published`; `BankItem` is not `Serialize`.

- Roles gained draft and published; a role has a hosted bank when its draw plan is non-empty.
- Retiring may take a published role under the floor; the console shows the shortfall and ticket 08 must cope.
- Overexposed means shown at least 20 times and at least twice its skill's average. Provisional.
- CSV answers are letters; JSON answers are 0-based indexes. No new dependency.
