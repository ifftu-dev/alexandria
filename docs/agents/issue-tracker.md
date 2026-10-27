# Issue tracker: spec docs

Work lives as Markdown spec docs in the repo that owns the change. GitHub Issues are not used.

## Where specs live

- One file per feature: `<repo>/docs/<feature>-spec.md`. Existing `-rfc.md`, `-plan.md`, and `-proposal.md` files are specs too.
- `<repo>` is the one whose code changes: `alexandria`, `alexandria-cloud`, `alexandria-relay`, `alexandria-monitoring`, `website`.
- A feature that spans repos keeps its spec in `alexandria/docs/`. Other repos link to it rather than copying it.
- Specs are committed and the repos are open source: no secrets, credentials, customer data, or personal names/emails.

## Header

Every doc under `docs/` starts with a `Status:` line directly under the title; specs and tickets may use `Triage:` alone. Specs put the triage lines below any `Status:` line:

```
Status: draft | active | live | implemented | superseded | retired
Updated: YYYY-MM-DD
Superseded-by: <path>                          (when superseded or retired)
Triage: needs-triage
Repos: alexandria, alexandria-cloud
Part of: alexandria/docs/<parent>-spec.md      (optional)
Blocked by: <path>, <path>                     (optional)
Assignee: <who>                                (optional, once claimed)
```

## Conventions

- **Create an issue**: write a new `docs/<feature>-spec.md` (kebab-case) with the header.
- **Read an issue**: read the file, plus its `Part of:` parent if one is set.
- **List issues**: from the workspace root, `grep -l '^Triage:' */docs/*.md */docs/*/*.md`, then filter on the `Triage:` value.
- **Comment on an issue**: append a dated entry under a `## Notes` section at the bottom of the file.
- **Apply / remove labels**: edit the `Triage:` line.
- **Close**: set `Triage: done` (or `wontfix`), set `Status: implemented` (or `retired`), and add a dated closing note. Never delete the file. An implemented spec is frozen: later changes start a new spec.
- **Sub-tickets**: `docs/<feature>/NN-<slug>.md`, each with `Part of:` pointing at the parent.

## Pull requests as a triage surface

**PRs as a request surface: no.** _(Set to `yes` if external PRs should be treated as feature requests; `/triage` reads this flag.)_

## When a skill says "publish to the issue tracker"

Create a spec file as described in **Create an issue**.

## When a skill says "fetch the relevant ticket"

Look for a spec path in commit messages, then a spec whose name matches the branch (`feat/push-notifications` → `push-notifications-*.md`), then grep the owning repo's `docs/`. If nothing turns up, ask the user.

## Wayfinding operations

Used by `/wayfinder`.

- **Map**: `docs/<feature>-map.md`, with Notes / Decisions-so-far / Fog sections.
- **Child ticket**: `docs/<feature>/NN-<slug>.md` with `Part of:` set to the map and a `Type: research|prototype|grilling|task` header line.
- **Blocking**: a `Blocked by:` header line. A ticket is unblocked once every blocker is `done` or `wontfix`.
- **Frontier query**: open children with no open blocker and no `Assignee:`. Lowest `NN` wins.
- **Claim**: set `Assignee:`. This is the session's first write.
- **Resolve**: append the answer, set `Triage: done`, and add a gist + path pointer to the map's Decisions-so-far.
