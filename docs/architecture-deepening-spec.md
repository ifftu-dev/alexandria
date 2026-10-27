# Architecture deepening: chat, calls, and AI access

Triage: done
Repos: alexandria

Umbrella for the seven deepening opportunities found in the 2026-10-26 architecture review. Each child ticket is one refactor that turns a shallow module into a deep one. Vocabulary: **module** (interface + implementation), **interface** (everything a caller must know), **seam** (where an interface lives), **adapter** (a concrete thing filling a seam), **depth** (behaviour per unit of interface), **locality** (change and tests concentrate in one place), **leverage** (one implementation, many callers).

## Problem Statement

Recent work on the AI chat and on live calls keeps landing as edits spread across several files for one idea. A learner-facing rule (who may use which model, what a chat may write, what is sent to a model) has no single home, so each change is a hunt, and bugs are fixed in one copy and survive in another. Tests for these rules exist but can only run by driving Tauri commands against a real database and, for streaming, a live fake model server.

## Solution

Move each rule behind one deep module with a small interface, placed at a seam the existing tests already cross. Commands become thin wrappers. Platform and transport variance sits behind adapters rather than being copied per platform. Vocabularies the webview needs are generated from the backend rather than retyped.

## Children

| NN | Ticket | Strength | Blocked by |
| --- | --- | --- | --- |
| 01 | `architecture-deepening/01-chat-turn-module.md` | Strong | — |
| 02 | `architecture-deepening/02-call-manager-media-adapters.md` | Strong | — |
| 03 | `architecture-deepening/03-ai-access-policy.md` | Strong | — |
| 04 | `architecture-deepening/04-ai-connect.md` | Worth exploring | — |
| 05 | `architecture-deepening/05-run-module-event-sink.md` | Worth exploring | 01 |
| 06 | `architecture-deepening/06-generated-vocabularies.md` | Worth exploring | — |
| 07 | `architecture-deepening/07-commands-depend-down.md` | Speculative | 01, 03 |
| 08 | `architecture-deepening/08-credentials-module.md` | Follow-up | 07 |

Recommended order: 01, then 03 and 05, then 02 once the pending live-calls retest is done, then 04, 06, 07.

## Implementation Decisions

- One seam per child, chosen at the highest point an existing test already crosses. No new seam is introduced where only one adapter would ever exist.
- Commands keep their names and payloads. The generated command list and the webview are unchanged unless a child says otherwise.
- Every chat decision recorded in the AI chat spec's implementation record stays as recorded; these tickets move code, they do not change behaviour. The one exception, surfaced in 03, is closing gaps where a documented rule is not applied everywhere the record says it is.
- Decisions that look like conflicts but are deliberate: the Optional AI checks client builds its own pinned HTTP client because that crate must stay MIT/Apache licensed; it is not folded into 04.

## Testing Decisions

- A good test crosses the module's interface and asserts on outcomes a learner would notice (what was sent, what was saved, what was refused and why), never on internal call order.
- Existing behaviour tests move with the code and are not rewritten. A child that cannot keep the existing tests green through its new interface has chosen the wrong seam.
- Prior art: the chat command tests that build a migrated SQLite database with a seeded learner and call prepare, start, and complete directly; the Studio crate's gate and learning tests; the AI crate's endpoint and SSE tests against a loopback server; the tutoring lifecycle tests that drive the manager without a camera.

## Out of Scope

- New learner-facing features or copy changes.
- Changing what is stored (no migrations) except where a child says so (none do).
- Cloud, relay, monitoring, and website repos.
- Creating the glossary and ADR directory described in the agent docs; a child may add a glossary term when it names a new module after a concept the glossary lacks.

## Further Notes

- The review report that produced these tickets listed per-candidate before/after diagrams; the tickets carry the decisions, not the diagrams.
- No `GLOSSARY.md` or `docs/adr/` exists yet. The AI chat spec's implementation record is the current decision record for chat.

## Notes

- 2026-10-26: created from the architecture review. All children labelled ready-for-agent; 02 also needs a human for on-device verification (see its notes).
- 2026-10-27: closed (done). All seven children are done and merged into `refactor/arch-integration`; each child's Notes record what landed and where it departed from its text. Two things remain outside these tickets: the on-device call retest in 02 (a human, before merge to `main`), and moving the derived skill state store's tests down once credential issuance leaves the commands layer (07). No glossary or ADR was added. The architecture and AI chat docs describe the new modules.
- 2026-10-27: reopened for one follow-up child, 08 (credential issuance out of the commands layer, which also lets the derived skill state store's tests move down). The umbrella stays done for 01–07.
