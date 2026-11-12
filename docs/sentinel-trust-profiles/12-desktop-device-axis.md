# Desktop device axis: macOS attestation check and paired phone

Triage: ready-for-agent
Repos: alexandria
Part of: docs/sentinel-trust-profiles-spec.md
Blocked by: docs/sentinel-trust-profiles/05-trust-profiles.md

Spec: Implementation Decisions → Device axis.

- First: establish from Apple primary docs whether DCAppAttestService is supported for the macOS build (and Mac Catalyst/Apple silicon caveats). Record the answer in this ticket's Notes. If supported, wire it into the existing attestation module for macOS.
- Paired attested phone: pairing over the existing peer-to-peer transport, DID-bound; phone contributes its attestation to the desktop session's commitment chain; under camera profiles it is the camera, sending only scores.
- Desktop + paired attested phone reaches `device_attested`.
- Runtime on real devices is verified in ticket 16; this ticket proves the protocol with unit/integration tests.

## Notes

2026-11-12, macOS App Attest answer (Apple primary documentation):

- The `DCAppAttestService` reference lists availability "iOS 14.0+, iPadOS 14.0+, Mac Catalyst 14.0+, macOS 11.0+, tvOS 15.0+, visionOS 1.0+, watchOS 9.0+", so the API compiles for macOS. Its `isSupported` reference (developer.apple.com/documentation/devicecheck/dcappattestservice/issupported) says: "If you read `isSupported` from an app running on a Mac device, the value is `false`. This includes Mac Catalyst apps, and iOS or iPadOS apps running on Apple silicon."
- So macOS is not wired. There is no Developer ID versus App Store caveat to weigh: the value is false on every Mac whatever the distribution channel. Desktop reaches `device_attested` only through a paired phone.

2026-11-12, deviations from the bullets above:

- Transport is a new ALPN (`alexandria/sentinel-companion/1`) on the existing iroh endpoint (the content node's router), not the libp2p device-sync pairing. App to app only: no relay or monitoring change.
- Pairing is per session, not a standing device pair. The desktop shows a one-time code (session id and start, learner DID, iroh address, 32-byte secret, 5-minute expiry). The phone joins only if the code names the DID of its own unlocked profile, attests `SHA-256(tag ‖ session nonce ‖ learner DID ‖ phone endpoint id)`, and signs the result with the learner DID key. The desktop checks the secret, that the connection is authenticated as the phone endpoint the binding names, the DID signature, and the platform attestation through `sentinel::attestation::verify` against a nonce it recomputes from its own session row. A failed attestation is refused; Android without response keys is stored at `local`, like a phone's own session.
- Stored on `attestation_json` with a `companion` binding (no migration). Issuance re-verifies it and credits it only when the binding names the profile's learner (`identity.local_did`).
- The pairing is folded into the chain as the link before the selection (`companion|{session}|{attestation_json}`). It must happen before profile selection. Selection without companion consent drops the pairing (assurance back to `local`), so the credential cannot claim it.
- At session end an iOS phone signs the desktop's terminal root. The phone cannot recompute that root, so the assertion shows the phone was still linked at the end, not that the phone produced the camera scores.
- Phone as camera: the protocol, desktop ingestion (the phone's window replaces the desktop client's camera group) and the phone's own benchmark (reported, not attested) are built and tested. The phone-side producer of camera windows in the UI is not built, so the desktop UI pairs with the camera off for now. Follow-up.
- Phone UI: Sentinel dashboard → Join from phone (phones only) takes a pasted code; the desktop shows the code in the assessment runner when "Pair my phone first" is ticked. There is no QR code.
- The transport tests were written after the transport code, not before it.
- Review fix: the desktop read the phone's role (camera, benchmark) right after answering the join, while the transport inserted the phone link only once it received that answer, so the read could miss the link. `Joined::answer` now puts the link in place before it replies; a desktop that answers too late leaves no link.
- Review fix: declining companion consent cleared the stored attestation but left the phone link up, so the phone's benchmark still fed selection and its camera windows still replaced the desktop's. Selection now drops the link first (`CompanionHub::consented_role`).
