# The package player

Triage: ready-for-agent
Repos: alexandria
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/01-runtime-library-check.md, docs/learning-standards/04-package-serving.md, docs/learning-standards/05-attempts-and-progress.md

Spec: Runtime; Serving and isolation (host-drawn label, focus mode, degrade message); Credentials → labelling; user stories 2 to 16.

- The package element renders in a player. Nodes that do not know the element type keep showing the existing unknown-element notice.
- A wrapper page served at the package origin holds the SCORM 1.2 API object from the vendored library; content in a nested frame finds it and calls it synchronously.
- Run-time state lives in the wrapper. Commits are coalesced and handed to the host at most once per short interval, and always on termination, frame unload and when the app is backgrounded. A persistence failure shows a player-level error and the attempt stays resumable.
- A permanent label outside the frame names the content as third-party with its publisher and says Alexandria never asks for a recovery phrase there.
- Focus mode fills the app window and keeps the label. A full-screen request from content becomes a focus-mode request.
- A table of contents for packages with several lessons; score and status shown and labelled self-reported; a control to start a fresh attempt; auto-advance suppressed.
- When content needs something blocked, the player says which thing.

Done when: in the browser harness a real SCORM 1.2 package launches, saves, resumes and completes through the player with a fake host bridge, and the label stays visible in and out of focus mode.
