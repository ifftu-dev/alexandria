# Push Notifications — Architecture RFC

**Status:** Research complete, not yet scheduled for implementation
**Date:** 2026-05-18
**Audience:** Alexandria core

> **TL;DR:** Extend `alexandria-relay` into a multi-protocol push gateway (APNs + FCM + UnifiedPush + native WebSocket). Use `tauri-plugin-notifications` v0.4.6 on mobile, a native Rust WebSocket subscriber on desktop. Payloads are end-to-end encrypted on the message plane; the wakeup plane unavoidably touches Apple/Google on their respective platforms. UnifiedPush gives privacy-conscious Android users a Google-free path. Estimated total effort: ~8 engineer-weeks for production-grade cross-platform push.

---

## 1. Centralized stack: APNs + FCM + WNS

**What ships:**
- **APNs (iOS / macOS):** Apple Developer account, `.p8` signing key (KEY_ID + TEAM_ID), HTTP/2 connection to `api.push.apple.com`. Per-device tokens (~64 hex chars) rotate on reinstall/restore.
- **FCM (Android):** Firebase project, OAuth2 service-account JSON, per-install registration tokens. Requires `google-services.json` bundled into the APK.
- **WNS (Windows):** Package Family Name + Azure AD client secret, OAuth2 access token, per-channel URL. Only meaningful for MSIX-packaged apps from the Store; sideloaded Tauri builds typically can't use it.
- **Server-side:** token store keyed by user/device, fan-out worker, retry on `Unregistered` / `InvalidRegistration`, key rotation.

**Privacy implications (the honest version):**

- An APNs token uniquely identifies a device-app pair to Apple. FCM tokens do the same for Google. Even with E2E-encrypted payloads, **both providers learn "this device runs Alexandria" plus delivery timing and frequency** — a strong fingerprint. The EFF documented this metadata exposure ([EFF, 2026](https://www.eff.org/deeplinks/2026/04/how-push-notifications-can-betray-your-privacy-and-what-do-about-it)).
- Apple and Google have honored law-enforcement requests for push metadata (Wyden DOJ letter, 2023).
- FCM is the privacy hotspot: every Android push touches Google servers, even if every other piece of infrastructure is self-hosted.

**Tauri 2 plugin landscape (as of 2026-05):**

| Plugin | Version | Platforms | Status |
|---|---|---|---|
| [`Choochmeque/tauri-plugin-notifications`](https://github.com/Choochmeque/tauri-plugin-notifications) | **0.4.6** (May 4, 2026) | iOS, Android, macOS, Windows, Linux; remote push on iOS + Android (macOS APNs flagged) | Active, 340 commits, MIT. **Best pick.** |
| [`yanqianglu/tauri-plugin-mobile-push`](https://github.com/yanqianglu/tauri-plugin-mobile-push) | 0.1 | iOS 13+, Android 7+ | Newer, no-swizzling iOS path, desktop is stub. |
| [`tauri-plugin-remote-push`](https://crates.io/crates/tauri-plugin-remote-push) | 1.0.5 | iOS, Android | Smaller scope. |
| Upstream `tauri-plugin-notification` | 2.x | local only | Doesn't do remote push — see [tauri#11651](https://github.com/tauri-apps/tauri/issues/11651). |

**Effort:** ~2 engineer-weeks (plugin wiring + Firebase / Apple setup + relay endpoint + token rotation).

---

## 2. Decentralized / open alternatives

**[UnifiedPush](https://unifiedpush.org/users/faq/)** — Android-and-Linux-desktop standard. User picks a "distributor" app (ntfy, NextPush, KUnifiedPush). **No iOS support and none coming** — iOS forbids background services, so no third-party distributor is possible without jailbreak. Linux/KDE desktop works via KUnifiedPush. **Use it as the preferred Android path for privacy-conscious users**, fall back to FCM.

**[ntfy.sh](https://ntfy.sh)** — HTTP / WebSocket pub-sub written in Go, self-hostable, doubles as a UnifiedPush distributor backend ([docs](https://unifiedpush.org/users/distributors/ntfy/)). **`alexandria-relay` can front an ntfy-protocol endpoint** (or embed the equivalent SSE / WS endpoints directly) — the wire protocol is dead simple (POST to publish, GET / WS to subscribe). No mature Rust _server_ port exists (only client crates: [`ntfy`](https://docs.rs/ntfy/0.3.1/ntfy/), [`ntfy-api`](https://github.com/tomaThomas/ntfy-api)), so either run ntfy-go as a sidecar or implement the ~300-LOC subset directly in axum.

**APNs from Rust:** [`a2`](https://github.com/WalletConnect/a2) (now `reown-com/a2`, v0.10.0, May 2024) is the standard. Full HTTP/2 + `.p8` JWT auth + signature caching. MIT, maintenance-mode but functional. Alternative: `apple-apns`.

**Web Push (VAPID) for desktop:** [`web-push` 0.11.0](https://crates.io/crates/web-push) (`pimeys/rust-web-push`) — solid, supports RFC 8188 + VAPID. **The blocker is the client:** Tauri uses WKWebView (macOS), WebView2 (Windows), WebKitGTK (Linux). [WKWebView does not expose Service Worker / Push API without browser entitlement](https://bugs.webkit.org/show_bug.cgi?id=206741) (still true in 2026). WebView2 supports it; WebKitGTK partial. **Verdict: Web Push is not a portable desktop solution via Tauri's webview.** Use a native Rust background task in the Tauri host process instead.

**libp2p / iroh persistent connection:** [iroh](https://www.iroh.computer/) cross-compiles to iOS and Android, but **iOS will not let you hold a QUIC socket while suspended**. `BGAppRefreshTask` gives ~30s opportunistic wakeups; VoIP / CallKit is the only "always on" path and Apple will reject Alexandria for misusing it. Android Doze + battery-optimisation whitelists make it user-hostile. **Not viable as the primary push channel** — useful only as a "warm window" sync when the app is foregrounded.

---

## 3. Hybrid recommendation

Extend **`alexandria-relay`** (existing Fly.io Rust service) into a **push gateway** with pluggable transports:

```
       client device                       relay (Fly.io)              provider
┌─────────────────────────┐  register   ┌──────────────────┐  fan-out  ┌──────┐
│ Tauri app               │────────────▶│ /push/register   │──────────▶│ APNs │
│  - tauri-plugin-notifs  │             │  (transport,     │           │ FCM  │
│  - desktop: native WS   │  publish    │   token, topic)  │           │ ntfy │
│    subscriber           │◀────────────│ /push/publish    │           │ WNS? │
└─────────────────────────┘   payload   └──────────────────┘           └──────┘
```

**Registration handshake (opaque to providers):**

1. Client generates ephemeral `device_id` (random, not tied to user identity).
2. Plugin obtains native push token; client POSTs `{device_id, transport: "apns"|"fcm"|"unifiedpush"|"webpush"|"ws", token, topic_pubkey}` to relay over TLS.
3. Relay stores `topic_pubkey → [device records]`. **Payload content is E2E-encrypted client-side** with the topic key; relay only routes ciphertext + a 1-byte priority flag.
4. Senders POST `{topic_pubkey, ciphertext}`; relay looks up subscribers and dispatches per-transport.

**Unavoidable exposures:**

- **FCM users:** Google learns the device runs Alexandria + push timing. Mitigated only by the user choosing UnifiedPush.
- **APNs users:** Apple learns the same. **No mitigation exists on iOS** — this is the cost of being on the platform.
- **Relay operator (us):** sees `{token, topic_pubkey, ciphertext_size, timing}`. Does **not** see message content or user identity, provided `device_id` stays ephemeral.

---

## 4. Concrete recommendation

**Architecture:**

- **Mobile:** `tauri-plugin-notifications` 0.4.6 (Choochmeque). iOS → APNs, Android → FCM by default, **with a build-time `unifiedpush` feature** offering ntfy as the Android distributor for F-Droid / privacy builds.
- **Desktop:** Native Rust subscriber in the Tauri host using `tokio-tungstenite` against an ntfy-protocol endpoint on `alexandria-relay`. Use OS-native local notifications via the upstream `tauri-plugin-notification` for display. Skip WNS, skip Web Push in webview.
- **Relay (extend `alexandria-relay`):** add `axum` routes `/push/register`, `/push/publish`, `/sub/:topic` (WS). Server-side crates: [`a2`](https://github.com/WalletConnect/a2) for APNs, [`fcm_v1`](https://docs.rs/fcm_v1) (or `oauth_fcm`) for FCM HTTP v1, plain `reqwest` for ntfy / UnifiedPush.
- **Payload:** ciphertext-only, sealed with the topic's libp2p / iroh keypair. Notification display text is generated on-device after decryption.

**Effort estimate:**

| Slice | Weeks |
|---|---|
| Relay gateway + 3 transports + token rotation | 3 |
| Mobile plugin integration + Firebase / Apple cert plumbing + CI signing | 2 |
| Desktop WS subscriber + local notification glue | 1 |
| UnifiedPush opt-in build + F-Droid recipe | 0.5 |
| E2E payload encryption + testing on real devices (5 OS variants) | 1.5 |
| **Total** | **~8** |

**Opinion:** Don't waste time fighting APNs on iOS — it's the price of being on the platform. Spend the saved energy making the **Android + desktop** paths genuinely Google-free, and document the iOS exposure honestly in the privacy policy. The relay-as-gateway design keeps the message plane decentralized (E2E) even when the wakeup plane isn't.

---

## Interaction with multi-user profiles

Push delivery must be **profile-scoped**: a notification destined for Profile A should not surface while Profile B is active. Approach:

- The notification payload (after decryption on-device) carries `profile_id`.
- If the matching profile is the active one → display the notification, route the tap to the in-app screen.
- If a different profile is active → display the notification with **profile name in the title**, tap action locks the current profile and unlocks the target one (password prompt).
- If no profile is active → display the notification, tap action opens the picker pre-selected on the target profile.

This requires multi-profile to land first, which is why this RFC is queued behind `feat/multi-user-profiles`.

---

## Sources

- [Choochmeque/tauri-plugin-notifications](https://github.com/Choochmeque/tauri-plugin-notifications) — v0.4.6, May 2026
- [yanqianglu/tauri-plugin-mobile-push](https://github.com/yanqianglu/tauri-plugin-mobile-push)
- [tauri-plugin-remote-push on crates.io](https://crates.io/crates/tauri-plugin-remote-push/1.0.5)
- [Tauri push notifications meta-issue #11651](https://github.com/tauri-apps/tauri/issues/11651)
- [UnifiedPush FAQ](https://unifiedpush.org/users/faq/) — iOS unsupported
- [ntfy as UnifiedPush distributor](https://unifiedpush.org/users/distributors/ntfy/)
- [ntfy docs](https://ntfy.sh/docs/config/)
- [`a2` APNs crate (reown-com)](https://github.com/WalletConnect/a2) — v0.10.0
- [`web-push` 0.11.0](https://crates.io/crates/web-push)
- [`fcm_v1` crate](https://docs.rs/fcm_v1)
- [WKWebView Service Worker bug (still open 2026)](https://bugs.webkit.org/show_bug.cgi?id=206741)
- [EFF — How Push Notifications Can Betray Your Privacy (April 2026)](https://www.eff.org/deeplinks/2026/04/how-push-notifications-can-betray-your-privacy-and-what-do-about-it)
- [iroh roadmap](https://www.iroh.computer/roadmap)
- [Apple BGTaskScheduler docs](https://developer.apple.com/documentation/backgroundtasks/bgtaskscheduler)
