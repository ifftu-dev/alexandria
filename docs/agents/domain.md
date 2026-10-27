# Domain Docs

How the engineering skills should consume this workspace's domain documentation when exploring the codebase.

## Before exploring, read these

- **`GLOSSARY-MAP.md`** at the workspace root (`alexandria-mark3/`). It points at each repo's `GLOSSARY.md`. Read the ones for the repos you're touching.
- **`<repo>/docs/adr/`**: ADRs for the repo you're working in.
- **`alexandria/docs/adr/`**: cross-repo decisions (VC format, network profile, credential authority). Always check these for work that crosses repo boundaries.

Each repo is one context: `alexandria`, `alexandria-cloud`, `alexandria-relay`, `alexandria-monitoring`, `website`.

If any of these files don't exist, **proceed silently**. Don't flag their absence; don't suggest creating them upfront. The `/domain-modeling` skill (reached via `/grill-with-docs` and `/improve-codebase-architecture`) creates them lazily when terms or decisions actually get resolved.

## File structure

```
alexandria-mark3/                      ← workspace root, not a git repo
├── GLOSSARY-MAP.md
├── alexandria/
│   ├── GLOSSARY.md
│   └── docs/adr/                      ← app decisions + cross-repo decisions
├── alexandria-cloud/
│   ├── GLOSSARY.md
│   └── docs/adr/
├── alexandria-relay/
│   ├── GLOSSARY.md
│   └── docs/adr/
├── alexandria-monitoring/
│   ├── GLOSSARY.md
│   └── docs/adr/
└── website/
    ├── GLOSSARY.md
    └── docs/adr/
```

## Use the glossary's vocabulary

When your output names a domain concept (in an issue title, a refactor proposal, a hypothesis, a test name), use the term as defined in `GLOSSARY.md`. Don't drift to synonyms the glossary explicitly avoids.

If the concept you need isn't in the glossary yet, that's a signal: either you're inventing language the project doesn't use (reconsider) or there's a real gap (note it for `/domain-modeling`).

## Flag ADR conflicts

If your output contradicts an existing ADR, surface it explicitly rather than silently overriding:

> _Contradicts ADR-0007 (event-sourced orders), but worth reopening because…_
