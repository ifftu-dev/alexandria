# Prototype the cmi5 on-device endpoint's transport

Triage: ready-for-agent
Repos: alexandria
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/06-player.md

Spec: cmi5 → transport bullet and own-window bullet.

- A throwaway prototype that answers one question: how does unmodified cmi5 content, running in the package frame, send xAPI requests with bodies to an endpoint on the device, on every supported platform, and does a session survive the app being backgrounded and resumed.
- A loopback listener is the expected answer. A custom scheme is acceptable only where request bodies reach the handler.
- Settle how the package content policy admits that one endpoint and nothing else.
- Settle what a unit that requires its own window does on desktop and on mobile.

Done when: the answer for desktop is demonstrated and recorded here under Notes, with what was tried and what failed; anything that can only be confirmed on a physical phone is listed for ticket 28. The prototype code is not merged.
