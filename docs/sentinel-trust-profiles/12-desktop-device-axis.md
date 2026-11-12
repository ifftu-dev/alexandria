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
