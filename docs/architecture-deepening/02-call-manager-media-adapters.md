# One call manager, platform media adapters behind its seam

Triage: ready-for-agent
Repos: alexandria
Part of: alexandria/docs/architecture-deepening-spec.md

## Problem Statement

A call (tutoring session, classroom call, interview seat, interview candidate, interview assistant, plugin experience) is implemented twice: one manager for desktop and Android, one for iOS, each with its own copy of the message types, room lifecycle, call kind handling, session records, chat, transcript, presence, and frame bridge. The seventeen call commands exist three times (desktop, iOS, stub). Most recent call fixes had to be applied to both managers, and some were applied to only one.

## Solution

One **call manager** owns everything about a call that does not depend on the platform. The platform-specific parts (camera source, video encoder and decoder, audio route and session, and the main-thread hop iOS needs) sit behind one **media devices** interface with a desktop adapter, an iOS adapter, an Android adapter, and a synthetic adapter for tests and unsupported targets. One commands file serves all platforms.

## User Stories

1. As a learner on any device, I want leaving a call never to hang on stuck media work, so that I can always get out.
2. As a learner on any device, I want the camera and screen to open without blocking the call, so that the call stays responsive.
3. As a learner, I want the call's kind (tutoring, classroom, interview) remembered the same way on every device, so that leaving takes me to the right place.
4. As a learner on a phone, I want to watch the 720p rendition and never an upscaled one, so that video stays smooth.
5. As a learner, I want my self view smooth and remote tiles never blanking, so that the call feels live.
6. As a learner, I want chat and transcript messages delivered identically on every device, so that nothing is lost between platforms.
7. As a learner, I want presence and peer names to behave the same everywhere, so that I know who is in the call.
8. As a panelist, I want my interview seat to work the same on a Mac and an iPhone, so that I can join from either.
9. As a maintainer, I want a call bug fixed once, so that a fix on the Mac does not leave the iPhone broken.
10. As a maintainer, I want the call lifecycle testable without a camera, so that lifecycle tests run in CI on every platform.
11. As a maintainer, I want the stub target to be the synthetic adapter, so that unsupported platforms share real code.
12. As an AI agent working on the codebase, I want one call manager, so that "what happens when someone joins a call" has one place to read.

## Implementation Decisions

- New **media devices** interface at the seam where the two managers actually differ: enumerate cameras and audio devices, open a camera source producing frames, open a screen source (desktop only; others report unsupported), create a video encoder and decoder, create audio input and output streams, report and switch audio routes, and run a closure on the platform's main thread when required.
- Adapters: desktop (existing camera capturer, software H.264, screen capturer), iOS (AVFoundation capture, VideoToolbox encode and decode, AVAudioSession route and interruption handling, main-thread dispatch), Android (existing NDK Camera2 backend and MediaCodec), synthetic (generated frames and silent audio; also the build for targets without video features).
- One **call manager** holding the active session, task admission, room lifecycle, call kind, session records, chat, transcript, presence, name announcements, frame bridge to the webview, diagnostics, and audio level reporting. It is generic over the media devices adapter and constructed once at app start with the platform's adapter.
- The iOS interruption bridge and audio route snapshot stay inside the iOS adapter; the manager only sees "route changed" and "interrupted / resumed" events.
- One commands file. The three existing files collapse into it; the command names and payloads do not change.
- Feature flags for video stay, but select the adapter, not the manager.
- The in-tree media crates (publish, subscribe, capture, audio) are unchanged.

## Testing Decisions

- Test the call manager through its own interface (create, join, leave, toggle, status, peers, sessions, shutdown) with the synthetic adapter and an in-memory event sink; assert on session state, emitted events, and stored session rows.
- The existing lifecycle tests (shutdown joins partial startup; interrupted shutdown blocks admission until retry) move and run once instead of once per manager.
- Add tests for call kind recording and re-entry refusal, chat and transcript fan-out, and leaving while media work is stuck, all against the synthetic adapter.
- Platform adapters are verified by compiling for every target in CI and by a human on-device retest (Mac to iPhone, Android), which an agent cannot do.
- Prior art: tutoring lifecycle tests; the media crate's slow-receiver end-to-end test.

## Out of Scope

- Admission control (publishers refusing peers not entitled to a call).
- Any change to the MoQ transport, renditions, or codecs.
- Classroom manager internals beyond its use of the call manager.

## Further Notes

- Highest payoff per bug of all children, but the riskiest: the iOS main-thread and audio session handling is where the two managers truly differ. Do this after the pending live-calls retest, and keep the old iOS manager reachable in git for comparison during the on-device retest.
- Agent delivers: the manager, the four adapters, one commands file, green lifecycle tests, green compile on desktop, iOS and Android targets. Human delivers: on-device verification before merge.
