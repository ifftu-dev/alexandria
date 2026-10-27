# Run module with an event sink instead of a Tauri channel

Triage: ready-for-agent
Repos: alexandria
Part of: alexandria/docs/architecture-deepening-spec.md
Blocked by: alexandria/docs/architecture-deepening/01-chat-turn-module.md

## Problem Statement

The streaming run that turns a begun turn into an answer is a private struct in the commands file that depends on the Tauri IPC channel, the database executor and lease, the run registry, the resolved endpoint, and the adapter's stream, and at the end it also parses the reply into proposals and writes them. Tests of proposal behaviour (a flashcard set waits to be saved, a goal change applies only when allowed) must start a fake TCP model server to exercise string parsing. The run is testable at all only because the Tauri channel type happens to accept a closure.

## Solution

A **run** module with a small interface: it takes a started turn, an event sink, a stream source, and a store, and drives the answer: checkpoints, cancellation, ordering, and completion. Turning the finished reply into an outcome is a pure operation on the chat turn (ticket 01); committing the outcome is one store transaction. Tests of proposal behaviour run on a database alone; streaming tests keep the loopback server but no Tauri type.

## User Stories

1. As a learner, I want an answer to stream in order and be saved with its usage, so that nothing is lost or reordered.
2. As a learner, I want stopping to keep the partial answer and record the stop, so that I do not lose what arrived.
3. As a learner, I want an assessment starting mid-answer to stop the answer, so that integrity holds.
4. As a learner, I want an unreachable model to fail without text and with a reason I can act on, so that I am not left waiting.
5. As a learner, I want a crashed app to recover a run that was streaming, so that the thread is never stuck "generating".
6. As a learner, I want a reply's proposals (note, flashcards, goal, goal change, course note, enrolment) to appear as cards only after the answer is complete and checked, so that I never see a half-parsed block.
7. As a maintainer, I want proposal parsing testable without a socket, so that those tests are fast and deterministic.
8. As a maintainer, I want the run free of Tauri types, so that it can be driven from any host.
9. As an AI agent working on the codebase, I want the run and the turn to be two named modules, so that "streaming" and "what a reply becomes" have separate places to read.

## Implementation Decisions

- New **run** module inside the AI chat backend module. Interface: construct with a started turn, an event sink (a callback that receives event bodies; the sink assigns no sequence numbers, the run does), a stream source (anything that yields text deltas and a final completion or error), a run store (checkpoint, finish, and the gate check per checkpoint), and a cancellation receiver. One operation drives it to completion.
- Two adapters at the stream source seam: the AI crate adapter in production; an in-memory scripted stream in tests. Two adapters at the sink: the Tauri IPC channel in production; a vector in tests.
- The reply-to-outcome step moves to the chat turn's `complete` as a pure operation (mode and reply in; content, note, question, memory suggestion, draft proposal, and refusals out). Committing the outcome is one store transaction. The run calls `complete` then commits.
- The run registry (concurrency bound, cancel, recovery) is already a deep module and is unchanged; the run holds its guard as today.
- Event bodies, sequence numbering, checkpoint cadence, the drain-after-end subtlety, and error codes are preserved exactly.
- No schema change.

## Testing Decisions

- Streaming tests (in order and saved with usage; stop keeps partial; assessment mid-answer stops; unreachable fails without text) run the run module with the scripted stream and a vector sink, asserting on the event sequence and stored rows. The loopback fake model server remains only for the adapter protocol tests (ChatGPT sign-in route, Anthropic instructions lift).
- Proposal tests (flashcards wait to be saved; goal added only when allowed; goal change resolves names; course note needs an attached lesson; enrolment must be on the goal's path; malformed practice block left as written; memory suggestion waits for the learner) call the turn's `complete` with a literal reply and assert on the outcome and stored proposals.
- Prior art: the chat command streaming and proposal tests; the run registry's unit tests.

## Out of Scope

- Changing event shapes, sequence semantics, or checkpoint cadence.
- Provider adapters.

## Further Notes

- Pairs with ticket 01; do it immediately after, while the turn's tests are fresh.
