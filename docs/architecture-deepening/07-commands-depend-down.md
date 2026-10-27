# Commands depend down, never sideways

Triage: ready-for-agent
Repos: alexandria
Part of: alexandria/docs/architecture-deepening-spec.md
Blocked by: alexandria/docs/architecture-deepening/01-chat-turn-module.md, alexandria/docs/architecture-deepening/03-ai-access-policy.md

## Problem Statement

Shared behaviour lives wherever its first command did, so domain modules reach into the commands layer: chat actions call course note insertion and course enrolment out of commands files; goal handling calls the plugin cid lookup out of a commands file; Studio commands call the chat commands file for the age rule. Each commands file declares its own database helper and its own error prefix convention. An AI agent or maintainer looking for "how a learner enrols" finds it under a command, not under enrolment.

## Solution

Commands depend on domain modules; domain modules never depend on commands. Behaviour that other modules need moves under the module named for it. One shared database helper and one error convention serve every commands file. A cheap check keeps the direction from regressing.

## User Stories

1. As a maintainer, I want enrolment, course notes, and plugin lookups callable without a command, so that chat actions and future callers use one implementation.
2. As a maintainer, I want one database helper for commands, so that lease, rate limit, and error handling are identical everywhere.
3. As a maintainer, I want a check that domain modules do not import the commands layer, so that the direction cannot silently regress.
4. As an AI agent working on the codebase, I want the module named for a concept to own it, so that I never have to search the commands layer for domain behaviour.
5. As a maintainer, I want the handler registration list to stop being the file every feature edits, so that merge conflicts in app wiring are rare.

## Implementation Decisions

- Move out of commands files into their domain modules: course note insertion (into a course notes module), course enrolment (into enrolment), plugin cid lookup for a course (into plugins), and anything tickets 01 and 03 leave behind. Commands become wrappers over them.
- One shared commands database helper (profile lease, activity touch, rate limit label, executor run, error mapping) replaces the per-file copies; per-file copies are deleted.
- One error convention for command results: a typed error with a stable string code, mapped once in the shared helper. Existing string codes are preserved.
- A dependency direction check: a test or script that fails if any non-commands module imports the commands layer. Lightweight (grep-level) is acceptable.
- Handler registration: investigate whether each commands module can expose its own handler list that the app's registration composes; adopt only if the Tauri macro allows it without losing the generated command list check. If not, record why in this ticket's notes and leave registration as is.
- No schema change; no command renames.

## Testing Decisions

- The moved behaviour keeps its existing tests, now under the domain module (prior art: chat action tests that enrol and insert course notes).
- The dependency direction check runs in CI alongside the commit message and command list checks.
- No new behaviour tests; this ticket is structure.

## Out of Scope

- Splitting every large commands file; only the sideways dependencies and the shared helper.
- Changing command payloads or names.

## Further Notes

- Speculative in the review; most of its value falls out of tickets 01 and 03. Do the remainder only if the direction check finds more than a handful of sideways imports after those land.
