# 07 Hosted item banks

Triage: ready-for-agent
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

- [ ] HTTP tests against the real database: import (both formats, malformed rows rejected with row numbers), list, retire; tenant isolation; audit rows.
- [ ] HTTP test: publish refused under the floor with the skill and shortfall, allowed at the floor; retired items do not count.
- [ ] Migrations, fmt, clippy, console checks pass.

## Pointers

- Roles and assessment requests: `src/http/assessment.rs`, `src/hiring.rs`, `web/src/views/RolesView.vue`; tenancy `src/db/tenant.rs`; audit `src/db/audit.rs`; tests in `tests/` (`assessment_exchange.rs`, `hiring.rs`, `common/`).
- The hosted service holds no private-course content: nothing here reads or links courses.
