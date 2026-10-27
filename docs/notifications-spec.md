# Notifications: profile inbox and mobile push

Triage: ready-for-agent
Repos: alexandria, alexandria-cloud

Status: Implementation and local review complete; external device/provider release acceptance remains open.
Date: 2026-10-26

## Problem Statement

Learners need to hear about interview invitations, offers, requests, classroom activity, credentials, and guardian links without repeatedly opening the relevant screen. A phone may be asleep when a panel opens an interview room. Desktop learners also need an inbox that retains activity after an OS banner disappears.

The existing notifications work implements much of this flow, but remains uncommitted across the app and Cloud. It has drifted behind the main branches, lacks deployment-specific mobile configuration, and has unresolved recipient isolation and offline opt-out behavior. The earlier push RFC describes a different architecture and is no longer sufficient as an implementation guide.

## Solution

Provide a persistent notification inbox for each learner profile, with an unread badge, read and clear actions, and navigation to the relevant screen. Record each event once even when both background discovery and push delivery report it. Show OS banners according to platform and permission state without duplicating a foreground push banner.

Use Alexandria Cloud to send generic mobile notifications through Firebase Cloud Messaging (FCM). FCM delivers directly to Android and forwards to Apple Push Notification service (APNs) for iOS. The running desktop app discovers Cloud activity through authenticated background requests and receives local activity through its existing event producers. Mobile background discovery also recovers eligible activity missed by push when the app resumes.

Keep notification delivery subordinate to profile isolation, verified domain actions, and the learner's preferences. A notification is a prompt to inspect an event, not proof that the event is trustworthy or authority to act on it.

This spec captures the implemented direction and the remaining acceptance requirements. Requirements described as remaining work are not claims about current behavior.

## User Stories

1. As a learner, I want a persistent inbox, so that I can find activity after dismissing an OS banner.
2. As a learner, I want an unread badge, so that I can see whether anything needs my attention.
3. As a learner, I want the newest notifications first, so that recent activity is easy to find.
4. As a learner, I want to mark a notification read, so that the badge reflects what I have reviewed.
5. As a learner, I want to mark all notifications read, so that I can acknowledge a backlog in one action.
6. As a learner, I want to clear my inbox after confirmation, so that I can remove activity I no longer need.
7. As a learner, I want cleared notifications to stay cleared, so that background discovery does not recreate them.
8. As a learner, I want a push and a background fetch about the same event to produce one inbox entry, so that I am not repeatedly interrupted.
9. As a learner, I want a notification tap to open the relevant screen, so that I can respond without searching through the app.
10. As a learner, I want a tap that launches the app to survive startup, so that my intended destination is not lost.
11. As a learner, I want a pending destination to wait for the correct profile to unlock, so that navigation respects authentication.
12. As a learner with several profiles, I want each inbox to contain only that profile's activity, so that notifications do not mix identities.
13. As a learner with several profiles, I want a push for an inactive profile to remain associated with its recipient, so that it cannot be written into the active profile's inbox.
14. As a learner, I want expired or already answered requests to be excluded from background discovery, so that notifications remain useful.
15. As an interview candidate, I want to hear about an invitation, so that I can respond before it expires.
16. As an interview candidate, I want to know when an accepted interview's room opens, so that I can join when the panel is ready.
17. As an interview candidate using a desktop or declining push, I want room availability to be discoverable while the app is running, so that I do not depend on mobile push.
18. As a learner, I want to hear about an offer, so that I can review it promptly.
19. As a learner, I want to hear about a credential access request, so that I can decide whether to share my credentials.
20. As a learner, I want to hear about an assessment request, so that I can decide whether to take it.
21. As a classroom member, I want to hear when somebody else starts a call, so that I can join it.
22. As a classroom moderator, I want to hear about a request to join my classroom, so that I can review membership requests.
23. As a learner, I want to hear when a credential is issued to me, so that I can inspect the new credential.
24. As a ward, I want to hear when a guardian link is completed, so that I know who has oversight of my account.
25. As a learner, I want to turn off in-app notification recording, so that my inbox and badge do not keep filling while that preference is off.
26. As a mobile learner, I want a separate push preference for this device, so that changing one device does not change all my devices.
27. As a learner, I want OS permission requests to happen after unlock, so that startup is not interrupted before I choose a profile.
28. As a learner declining OS permission, I want the inbox to remain usable, so that refusing banners does not remove in-app access to activity.
29. As a learner, I want registration status and failures to be understandable in Settings, so that I can tell whether this device can receive push.
30. As a learner disabling push while offline, I want my opt-out to be remembered and retried, so that a failed request does not silently leave delivery enabled indefinitely.
31. As a learner, I want push token rotation to preserve my preference and recipient association, so that registration remains correct after a token changes.
32. As a learner with several devices, I want eligible devices to receive notifications for my profile, so that delivery does not depend on one installation.
33. As a learner, I want provider-visible notifications to omit names, organisations, DIDs, and credential contents, so that lock-screen delivery reveals as little as this architecture permits.
34. As a learner, I want notification controls and supported notification copy in my selected language, so that I can understand them.
35. As a desktop learner, I want the inbox to update without a banner while the app is focused, so that the same activity does not interrupt me twice.
36. As a learner, I want mobile foreground delivery to produce at most one app-requested banner for an event, so that native and Rust handling do not duplicate it.
37. As a Cloud operator, I want committed business events to queue delivery atomically, so that an event is not lost between persistence and delivery scheduling.
38. As a Cloud operator, I want bounded retries and invalid-token cleanup, so that provider failures do not create an endless delivery loop.
39. As a Cloud operator, I want delivery metrics and exhaustion alerts, so that I can detect failures without inspecting sensitive tokens.
40. As a contributor, I want builds without Firebase configuration to remain usable and report push unavailability, so that local development does not require production credentials.

## Implementation Decisions

- **Continue the existing app/Cloud implementation.** The current architecture is a profile inbox plus Cloud-mediated FCM delivery on both mobile platforms. The earlier relay-gateway recommendation is historical context, not the target of this work.
- **Integrate without losing the working implementation.** Preserve the uncommitted changes in both repositories before integrating their current main branches. Reconcile generated IPC bindings, capability declarations, profile command policies, native project configuration, and vendored plugin changes with the integrated app.
- **Resolve the app migration collision.** The worktree allocates migration 18 to notifications, while current main already uses 18 for call kinds. Allocate an available migration after integration. Inspect migration identity handling and define safe handling for any development database that already applied the provisional notification migration; do not silently treat a different migration with the same number as applied, drop learner data, or rewrite a shipped migration. Cloud currently adds migration 0043; confirm that number remains available when integrating.
- **Keep the profile database authoritative for the inbox.** Persist notification identity, kind, copy, destination, source, timestamps, read state, and optional local payload. Keep deduplication separately from visible rows. Clearing rows must preserve deduplication. An inbox read or clear action does not accept an invitation, change a credential, or otherwise mutate the underlying domain event.
- **Use one delivery policy.** Local producers, Cloud discovery, and received push feed the same recording and announcement behavior. The in-app preference prevents new recording and app-generated banners; it does not erase historical rows. Native remote push has its own device preference and OS permission state, which the UI must distinguish from the in-app preference.
- **Retain the existing IPC and events.** The app exposes list, unread count, mark-read, mark-all-read, clear, permission, pending-open, and push registration/status operations through its local API bridge. New-row, unread-change, and open-destination events update the singleton frontend state. Profile lock tears down listeners and clears cached profile data. In-flight results must not populate a later profile's state.
- **Keep background discovery authenticated and bounded.** Query Cloud through the configured network service origin using existing holder proofs. Recover live interview invitations, offers, access requests, and assessment requests. Filter recipient mismatches, malformed items, expired items, and applicable locally answered items. Failure of one list must not suppress other successful lists. Desktop polling requires the app to be running; mobile polling requires execution time granted to the app.
- **Close the interview-room recovery gap.** Existing discovery does not recover room-open notifications. Extend the authenticated interview read surface and app discovery as needed so a candidate with an accepted interview can discover a currently open room without push. Do not generate a room-open notification for a room already closed or an interview no longer joinable. Use the existing interview authority and lifecycle, not a second source of truth in notifications.
- **Preserve local producers.** Classroom calls notify other members rather than the initiator; membership requests notify eligible moderators. Credential auto-issuance and completed guardian links produce notifications only for the appropriate local learner. Apply existing domain validation before recording these events.
- **Use stable event deduplication.** Cloud push and polling must agree on the event identity. Retry, repeated fetch, and duplicate receipt must not create additional inbox entries. Reopening a room needs a defined identity based on the existing room lifecycle, so a genuinely new opening is distinguishable from redelivery of the same opening.
- **Retain signed device registration.** The existing holder API registers or updates a device with `PUT /api/push/devices/for/{did}` and unregisters it with `DELETE /api/push/devices/for/{did}/{device_id}`. The registration body includes `deviceId`, `platform`, `token`, and optional `appVersion` and `locale`; the supported platform is `fcm`. Re-registration is idempotent for the DID/device pair, and unregistering an absent device succeeds. Preserve proof verification, request limits, and token access restrictions.
- **Bind receipt and navigation to the recipient.** A shared platform token is insufficient to identify a profile. Add an opaque registration-specific recipient binding to outgoing push and resolve it against local registration state before recording or navigating. Reuse the per-profile installation registration identity where appropriate; do not put the DID or profile name into provider-visible data. Unknown or legacy payloads without a resolvable binding must not be guessed into the active profile. Recover their underlying activity through authenticated discovery instead.
- **Handle inactive and locked recipients safely.** A push for another profile must not write into the active profile's database or navigate inside that profile. A tap retains an opaque recipient and destination until the recipient is selected and unlocked through the existing profile flow. Do not unlock or replace a profile session implicitly. Guard the entire asynchronous registration/receipt operation against profile switching, not only individual database jobs.
- **Make opt-out durable.** Remember a pending unregister until Cloud acknowledges it, and retry when connectivity and the required unlocked profile are available. Report pending cleanup honestly in Settings. Do not discard the only registration identity needed to retry, assume the token expires automatically, or let token-refresh callbacks re-enable an opted-out registration. Already queued provider banners cannot be recalled while offline; the UI must not promise immediate remote cessation before acknowledgement.
- **Scope token refresh correctly.** Rotation updates an existing opted-in registration using the same device identity. Refresh the relevant registration again on profile unlock so inactive profiles can reconcile after a rotation. A change of configured Cloud origin must not silently orphan a previously active registration or mix network identities.
- **Keep a transactional Cloud outbox.** Enqueue invitations, offers, access requests, assessment requests, and room openings in the same transaction as their business event. Worker replicas claim due rows with row locking. The current policy polls every five seconds in batches of twenty, retries transient failures up to five attempts with delays of 30 seconds, 120 seconds, 600 seconds, and 3,600 seconds, and abandons messages older than 24 hours. Invalid tokens are removed; permanent failures terminate delivery. Preserve these defaults unless a demonstrated correctness issue requires a documented adjustment.
- **Treat provider delivery as retryable, not exactly once.** A worker crash after a successful send can cause redelivery. Collapse identifiers and inbox deduplication reduce duplicates, but must not be presented as an exactly-once provider guarantee. Exercise partial multi-device failure so retry behavior does not unnecessarily resend to devices already known to have succeeded.
- **Keep push payloads generic and bounded.** Provider-visible fields contain generic copy, event kind, reference ID, destination, and the opaque recipient binding. No organisation name, person name, DID, credential content, or signed domain envelope belongs there. Google and Apple still observe tokens, timing, event metadata, and generic text; this design does not provide end-to-end encryption of the push payload. Validate externally supplied kinds and destinations, and fetch authoritative details before domain actions.
- **Respect platform presentation.** Desktop banners are appropriate when the app is unfocused. iOS already presents configured foreground remote banners; do not add another local banner. Android foreground remote delivery uses the app's local notification path. Retain the shared Android channel and native tap integration, and verify behavior for foreground, background, and cold launch separately.
- **Use one routing boundary.** Inbox rows, local banners, and remote taps use the existing deep-link runtime. Allow only supported app links or known same-document routes. Hold cold-start destinations until the matching profile is ready. Reject foreign URLs and malformed destinations. A destination is never authorization to access another profile's resource.
- **Finish localization.** Translate notification UI resources for all supported locales; the eight non-English resources currently duplicate English. Use locale-appropriate generic copy for app-rendered notifications and provider banners based on the registration's locale, with an English fallback. Keep the app and Cloud kind/copy contracts aligned without inserting identifying content.
- **Complete configuration and observability.** Keep deployment-specific Firebase app configuration and service-account credentials out of source control. Configure the Firebase project's APNs integration for iOS. Without provider configuration, Cloud accepts registrations but does not spawn a sender; builds without app configuration remain usable and surface a useful unavailable/error state. Preserve delivery, failure, exhaustion, and worker-run metrics and the exhaustion alert; avoid logging tokens or service-account material.

## Testing Decisions

- **Agreed boundaries.** The user confirmed testing app notification/deep-link behavior through the existing IPC/event boundary, Cloud registration and delivery through its HTTP router with PostgreSQL and a fake push sender, and real-device checks for native delivery and taps. Prefer these existing boundaries over introducing new testing interfaces.
- **Test observable behavior.** Assert inbox contents, unread counts, destination/profile selection, registration state, HTTP responses, outgoing provider messages, and committed database effects. Do not couple tests to private helper names, internal call sequences, or exact implementation structure. Use focused pure tests only when they cover meaningful parsing or provider error behavior that is expensive to exercise at the higher boundary.
- **App prior art.** Extend the existing notification composable and deep-link tests, notification store/push parsing tests, and in-memory database migration tests. Add behavioral coverage through the existing local API and event contract rather than a second frontend state model.
- **Inbox acceptance.** Verify ordering and bounds, unread counts, idempotent read operations, clear-without-resurrection, and push/pull deduplication. Verify that disabled recording adds nothing and that permission denial leaves the inbox usable. Exercise received events and delayed IPC results across lock/unlock and profile switches.
- **Recipient acceptance.** Register two profiles sharing one platform token. Deliver a push for profile A while B is active and assert no row, badge change, or navigation in B. Repeat while locked, after process startup, after token rotation, and with an unknown recipient binding. Tapping an A notification must wait for A's unlock and then navigate once. Registration started in A must not be saved into B after a switch.
- **Preference acceptance.** Simulate Cloud refusing or timing out an unregister. Confirm local push remains opted out, pending cleanup survives restart, refresh does not re-register it, and a later authenticated retry removes the remote device. Test idempotent unregister and changing the configured Cloud origin without mixing registrations.
- **Producer and recovery acceptance.** Exercise each supported event kind through its owning boundary. Confirm self-started classroom calls and non-moderator join requests do not notify the wrong learner. Test expired/answered requests, malformed and foreign-recipient items, independent list failures, and room-open discovery when push is absent or missed. Verify that viewing a notification does not bypass domain proof checks.
- **Cloud prior art.** Extend the existing push HTTP/database integration suite and its fake `Sender`. Use real PostgreSQL transactions and restricted/owner database roles. Cover valid and invalid holder proofs, cross-DID unregister attempts, registration replacement, rollback without queued delivery, all five business-event hooks, recipient binding, and token/outbox privilege restrictions.
- **Worker acceptance.** Use the existing drain boundary with controlled fake outcomes to test fan-out, transient backoff, permanent failures, dead-token removal, expiry, no-device delivery, mixed device outcomes, and concurrent workers. Assert observable retries and final state; advance due times or use existing controls rather than sleeping through backoff intervals. Include crash/redelivery semantics without assuming exactly-once provider delivery.
- **Contract and localization acceptance.** Check agreement on kind vocabulary, event identity, recipient binding, routes, Android channel, and generic payload constraints across app and Cloud. Validate translation key parity and actual translated copy. Inspect provider payloads to ensure prohibited identifying fields are absent.
- **Migration acceptance.** Test upgrades from current main and from the provisional notification schema if any development installations used it. Verify notification/read/deduplication data survives and that the call-kind and notification migrations cannot be mistaken for each other.
- **Native release checks.** On actual iOS and Android devices with deployment configuration, verify permission grant/denial, token acquisition, foreground/background/terminated delivery, notification taps, cold start, profile isolation, token refresh, and offline opt-out. Check focused/unfocused desktop presentation on supported desktop platforms. Record the tested OS/build configurations and results; mocked tests alone do not establish native readiness.
- **Required gates.** After implementation, run Rust formatting and Clippy with warnings denied, strict Vue/TypeScript checking, targeted frontend and Rust tests, Cloud database integration tests, and relevant native builds. A skipped database suite is not a passing integration result.

## Out of Scope

- A relay-hosted push gateway, relay subscriptions, or new P2P notification protocols.
- UnifiedPush/ntfy, Windows Notification Service, browser Web Push, or a desktop WebSocket push service.
- End-to-end encrypted provider payloads or claims that generic copy removes provider metadata exposure.
- Waking a fully closed desktop app or maintaining mobile P2P connections while the OS suspends the app.
- Remote push for classroom, credential, or guardian events that Cloud does not currently own; these remain local notification producers.
- Synchronizing inbox rows or read state between installations. Existing synchronized preferences retain their documented scope; push preference and registration remain device-specific.
- Notification digests, email/SMS delivery, quiet hours, per-kind subscriptions, rich media, or notification action buttons.
- Replacing profile authentication, holder proof verification, credential authority, interview authorization, or the existing deep-link system.
- Deploying services, provisioning credentials, publishing app-store builds, or asserting production delivery from the existence of this spec.

## Further Notes

- This is the cross-repository issue-tracker spec, owned by the app repository. It supersedes the implementation direction in the earlier push RFC, which the docs overhaul removed; it remains in git history as historical research. No GitHub Issue is needed under this workspace's tracker convention.
- The inspected app checkout was at commit `386052945c3621f4527409687d3a9b710b89f1cc` plus uncommitted changes. It was 98 commits behind local main during the status review.
- The inspected Cloud checkout was at commit `434d0cc3a44cd4535a380bf78c446846d57ddaee` plus uncommitted changes. It was 12 commits behind local main and one ahead; the extra commit was a license correction, not notification implementation.
- Existing work includes the app inbox/store, local producers, Cloud pull, push registration and receipt, frontend inbox/settings/badge, deep-link handling, native plugin changes, Cloud endpoints, outbox/FCM sender, metrics, and tests. The requirements above identify the behavior to retain and the gaps to close; the feature is not starting from scratch.
- Verified during the 2026-10-26 status review: 39 focused frontend tests, 14 app Rust notification tests, six Cloud push unit tests, strict Vue/TypeScript checking, and Rust formatting in both repositories passed. These results describe the pre-integration worktree, not a future integrated implementation.
- Cloud database integration tests were not run because their database environment was not configured in the review shell. Full workspace tests, Clippy, native builds, and real-device delivery remain unverified. Both deployment-specific Firebase app configuration files were absent from the inspected app checkout; deployed Cloud/Firebase configuration was not inspected.
- Recommended execution order: preserve the existing work; integrate main and reconcile migrations; close recipient, preference, and recovery gaps; finish localization and native configuration; run the agreed behavioral and device checks. Completion requires evidence for the acceptance behavior, not just a successful compile or an updated status label.

### 2026-10-27 — integrated implementation; release validation remains open

Tickets 01–05 are complete. Independent app and Cloud standards/spec reviews are clear after fixes and separate mergers. App workspace and frontend suites, real PostgreSQL Cloud tests, pinned-toolchain checks, Android APK/AAB and iOS simulator builds passed; final tap-handoff regressions also passed. [Ticket06](notifications/06-integration-review-and-release-validation.md) records exact evidence, exceptions and remaining acceptance.

Release requires configured Firebase/APNs/Cloud delivery, matching Apple signing/provisioning, connected iOS/Android devices and supported-desktop presentation/tap checks. All Cloud registration writers must enforce the durable `versioned-v1` barrier before app opt-out acknowledgement is reliable; old direct-writing binaries cannot safely coexist. Incompatible prelaunch plaintext held-tap sidecars are discarded, while profile inbox/read/deduplication data is retained. No deployment or production delivery claim is made. This spec remains open until the external checks are evidenced.
