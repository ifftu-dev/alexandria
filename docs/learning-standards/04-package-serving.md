# Serving package content in isolation

Triage: ready-for-agent
Repos: alexandria
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/03-package-storage.md

Spec: Serving and isolation (own scheme, launch-scoped addresses, origin storage, content policy, removed interfaces, device capabilities); Serving performance.

- A dedicated scheme for package content, separate from the plugin scheme, registered for desktop and mobile. The app's frame policy allows it.
- Opening a launch returns an unguessable path prefix bound to one package and valid only while that launch is open. Nothing else is served.
- The handler is asynchronous, honours byte-range requests, reads only the requested range and caps whole-file responses.
- Every response carries the package content policy. Every HTML response gets the injected bootstrap that removes the command bridge and the network-capable interfaces the policy does not govern.
- Storage at the package origin is cleared before and after each launch and service worker registration is blocked.
- No camera, microphone, location, clipboard read or full screen. A new-window call is redirected into the player frame.

Done when: handler tests show a ranged read returns only the range, an expired or foreign launch prefix is refused, headers carry the policy, and HTML carries the bootstrap. Browser-level escape attempts are proved in ticket 08.
