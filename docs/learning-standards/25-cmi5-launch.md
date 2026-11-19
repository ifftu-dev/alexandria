# cmi5 launch, sessions and completion

Triage: ready-for-agent
Repos: alexandria
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/14-learning-records.md, docs/learning-standards/23-cmi5-transport-prototype.md, docs/learning-standards/24-cmi5-import.md

Spec: cmi5 (launching system's duties, actor, on-device endpoint); Attempts and progress → cmi5 completion; Learning records → document stores.

- The app performs the launching system's duties: launch parameters, the one-time authorisation token, launch data and learner preference documents, the launched statement, validation of a unit's statements against the session rules, satisfied statements for blocks and the course, the abandoned statement on abnormal end, and completion criteria evaluation.
- Content talks xAPI to an on-device endpoint that exists only while a session is open, accepts only that session's token and implements only the resources cmi5 requires, backed by the local record tables and the document stores cmi5 needs. It is not claimed as a conformant record store.
- The transport is the one ticket 23 settled.
- The actor given to a unit is the pseudonymous identifier other package content receives. Records a unit writes are rewritten to the recipient identifier when sent anywhere.
- A package element is complete when the course's completion criteria are satisfied.

Done when: a published cmi5 package launches, runs a session and completes in the harness; host-seam tests cover token single use, a statement breaking the session rules being refused, satisfied and abandoned statements, and the endpoint refusing requests outside an open session.
