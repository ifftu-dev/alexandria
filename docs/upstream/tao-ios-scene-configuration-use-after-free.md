# Draft issue — tao

**Repo:** https://github.com/tauri-apps/tao
**Status:** drafted, not filed. Reviewed by: —
**Affects:** 0.35.3 (iOS, scene lifecycle mode)
**Our workaround:** `patches/tao`, via `[patch.crates-io]`

---

**Title:** iOS: `configurationForConnectingSceneSession:` returns a freed `UISceneConfiguration`; app crashes in `objc_retain` before its first frame

**Body:**

With `UIApplicationSceneManifest` / `UIApplicationSupportsMultipleScenes: true`
in `Info.plist` — which is what puts tao into scene mode (`multiple_scenes_enabled`)
— an app built against the iOS 26+ SDK dies on launch, before any window is
shown:

```
EXC_BAD_ACCESS (SIGSEGV), KERN_INVALID_ADDRESS
Thread 0 (main):
  libobjc.A.dylib   objc_retain
  UIKitCore         -[UIApplication _connectUISceneFromFBSScene:transitionContext:]
  UIKitCore         -[UIApplication workspace:didCreateScene:withTransitionContext:completion:]
  UIKitCore         -[UIApplicationSceneClientAgent scene:didInitializeWithEvent:completion:]
  FrontBoardServices …
```

### Cause

`src/platform_impl/ios/view.rs`, `configuration_for_connecting_scene_session`:

```rust
let config = UISceneConfiguration::configurationWithName_sessionRole(
    Some(&NSString::from_str("TaoScene")),
    &NSString::from_str("UIWindowSceneSessionRoleApplication"),
    mtm,
);
config.setDelegateClass(Some(super::scene::TaoSceneDelegate::class()));
Retained::as_ptr(&config) as _
```

`Retained::as_ptr` borrows. The `Retained` still owns the configuration and
releases it when the function returns. objc2 claims the autoreleased return of
`configurationWithName_sessionRole` with `objc_retainAutoreleasedReturnValue`,
which takes it *out* of the autorelease pool, so that release is the last one
and the object is freed on the way out of the delegate method.

The selector does not begin with `new`/`alloc`/`copy`/`mutableCopy`, so under
Cocoa's ownership rules the caller does not own the return value and UIKit
retains it itself — in `_connectUISceneFromFBSScene:`, a moment later, on
memory that has already been freed.

### Fix

Hand the +1 to the autorelease pool instead of dropping it:

```rust
Retained::autorelease_return(config) as _
```

That keeps the object alive across the return and lets UIKit take its own
reference, which is the contract for a non-owning return.

### Why this was not visible before

Two things had to line up. Scene mode is opt-in through `Info.plist`, and the
Tauri iOS template does not set `UIApplicationSceneManifest`, so most apps never
ran this code path. Building against the iOS 26+ SDK changes that: UIKit now
traps an app with no scene manifest at all
(`_UIApplicationEvaluateRuntimeIssueForNoSceneLifecycleAdoption`, previously a
logged runtime issue), so adopting scenes stops being optional — and the first
thing an app hits after adopting them is this.

`UIApplicationSupportsMultipleScenes: false` is not a way around it. tao reads
that key to decide whether it is in scene mode at all, and with `false` it
stays on the legacy path: UIKit still runs the scene lifecycle, but the
`UIWindow` is created with no `windowScene` and never becomes visible. Only
`true` works, and `true` hits the freed return.

### Reproduction

- Tauri 2.x iOS app, tao 0.35.3, Xcode 27 beta / iOS 27 beta device
- Add to `Info.plist`:
  ```xml
  <key>UIApplicationSceneManifest</key>
  <dict>
    <key>UIApplicationSupportsMultipleScenes</key>
    <true/>
  </dict>
  ```
- Launch. Crash on the main thread inside scene connection, no app code on
  the stack.

Applying the one-line change above, the app launches and renders.
