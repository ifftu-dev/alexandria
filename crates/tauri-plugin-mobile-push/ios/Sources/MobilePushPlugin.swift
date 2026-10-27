import UIKit
import WebKit
import UserNotifications
import Tauri
import ObjectiveC
import FirebaseCore
import FirebaseMessaging

// Alexandria modification: the token this plugin reports is a Firebase Cloud
// Messaging registration token, not the raw APNs device token upstream
// returned. The APNs token is still obtained the same way (method injection
// into the Tao AppDelegate), then handed to FirebaseMessaging, which trades it
// for an FCM token that the one server-side sender can address. Firebase's
// own AppDelegate swizzling is off (`FirebaseAppDelegateProxyEnabled` in the
// app's Info.plist): it would race the injection below on a class Tao
// generates at launch.

// MARK: - Event bridge

/// Bridges a platform event to the Rust side, which fans it out to the JS
/// listeners registered with `register_listener`.
///
/// `Plugin::trigger` cannot be used: it reaches listeners held by the plugin
/// object Tauri's own dispatch instantiated, and this plugin bypasses that
/// dispatch (see the FFI functions below). Rust owns the listener registry
/// instead, and this calls into it.
///
/// The symbol is `mobile_push_emit_event` in `src/ios.rs`, linked here rather
/// than imported because the Rust library is linked into the same binary.
@_silgen_name("mobile_push_emit_event")
private func mobilePushEmitEvent(_ event: UnsafePointer<CChar>, _ payload: UnsafePointer<CChar>)

/// Emits an event, encoding both strings as C strings for the Rust side.
private func emit(_ event: String, _ payload: [String: Any]) {
    guard let data = try? JSONSerialization.data(withJSONObject: payload),
          let json = String(data: data, encoding: .utf8)
    else {
        NSLog("[mobile-push] could not serialize the %@ payload", event)
        return
    }
    event.withCString { eventPtr in
        json.withCString { payloadPtr in
            mobilePushEmitEvent(eventPtr, payloadPtr)
        }
    }
}

// MARK: - Firebase

/// Configures Firebase from the `GoogleService-Info.plist` in the app bundle,
/// once. Without the file the app runs and local notifications work; only
/// remote push is unavailable, and `getToken` says so. Main thread only:
/// FirebaseCore expects it.
@discardableResult
private func configureFirebase() -> Bool {
    if FirebaseApp.app() != nil {
        return true
    }
    guard let path = Bundle.main.path(forResource: "GoogleService-Info", ofType: "plist"),
          let options = FirebaseOptions(contentsOfFile: path)
    else {
        NSLog("[mobile-push] GoogleService-Info.plist is not in the bundle; remote push is off")
        return false
    }
    FirebaseApp.configure(options: options)
    Messaging.messaging().delegate = MessagingHandler.shared
    NSLog("[mobile-push] Firebase configured for %@", options.gcmSenderID)
    return true
}

/// Hears Firebase rotate the registration token. The first token after
/// launch arrives here too, so a profile that registered before is
/// re-registered by the Rust side without a `getToken` call.
private class MessagingHandler: NSObject, MessagingDelegate {
    static let shared = MessagingHandler()

    func messaging(_ messaging: Messaging, didReceiveRegistrationToken fcmToken: String?) {
        guard let token = fcmToken, !token.isEmpty else { return }
        NSLog("[mobile-push] FCM registration token received")
        emit("token-received", ["token": token])
    }
}

// MARK: - Token Fetcher (thread-safe async token retrieval)

/// Fetches an FCM registration token: registers for remote notifications,
/// waits for the AppDelegate callback to hand Firebase the APNs token, then
/// asks Firebase for the FCM one.
private class TokenFetcher {
    let semaphore = DispatchSemaphore(value: 0)
    var token: String?
    var error: String?

    func resolve(_ tokenString: String) {
        self.token = tokenString
        semaphore.signal()
    }

    func reject(_ errorMessage: String) {
        self.error = errorMessage
        semaphore.signal()
    }
}

/// The active token fetcher — set during getDeviceToken, read by AppDelegate callbacks.
/// Access is serialized: only one getToken call can be in-flight at a time (guarded by tokenLock).
private var activeTokenFetcher: TokenFetcher?
private let tokenLock = NSLock()

// MARK: - AppDelegate method injection

/// Whether we've already injected APNs methods into the AppDelegate class.
private var apnsDelegateSetUp = false

/// Configured foreground presentation options. Set via FFI at plugin init.
private var configuredForegroundPresentation: UNNotificationPresentationOptions =
    [.banner, .list, .sound, .badge]

/// UNUserNotificationCenter delegate for foreground notification handling.
/// `willPresent` fires only when the app is in the foreground; backgrounded
/// and locked states bypass this delegate entirely and iOS shows the system
/// banner natively.
private class PushNotificationHandler: NSObject, UNUserNotificationCenterDelegate {
    static let shared = PushNotificationHandler()

    func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        emit("notification-received", Self.eventPayload(for: notification))
        completionHandler(configuredForegroundPresentation)
    }

    func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        if response.actionIdentifier == UNNotificationDefaultActionIdentifier {
            emit("notification-tapped", Self.eventPayload(for: response.notification))
        }
        completionHandler()
    }

    /// Builds the event body from a notification.
    ///
    /// The APNs payload's custom keys are the `data` map, matching what the
    /// Android side reports for the same push, so the JS handler reads one
    /// shape from both platforms. FCM puts its `data` map there, beside
    /// `aps`, plus bookkeeping keys of its own (`gcm.message_id`, `google.*`)
    /// that are passed along unread. `aps.alert` is not part of it — the
    /// visible text is not something a handler navigates on.
    private static func eventPayload(for notification: UNNotification) -> [String: Any] {
        var data: [String: Any] = [:]
        let remote = notification.request.trigger is UNPushNotificationTrigger
        let info = notification.request.content.userInfo
        if remote {
            for key in ["recipient", "eventId", "kind", "id", "route", "title", "body"] {
                if let value = info[key] as? String { data[key] = value }
            }
        } else if let extras = info["__EXTRA__"] as? [String: Any] {
            data["__EXTRA__"] = extras
        }
        var payload: [String: Any] = ["data": data, "local": !remote]
        let content = notification.request.content
        if !content.title.isEmpty {
            payload["title"] = content.title
        }
        if !content.body.isEmpty {
            payload["body"] = content.body
        }
        return payload
    }
}

/// Installs the notification-center delegate, which is what hands this app
/// foreground notifications and notification taps.
///
/// Must run before the app finishes launching: iOS delivers a tap that
/// launched the app only to the delegate that is already set when launch
/// completes, and asks exactly once, so a late install drops that tap for
/// good. Safe to call repeatedly.
private func installNotificationCenterDelegate() {
    if let existing = UNUserNotificationCenter.current().delegate,
        existing is PushNotificationHandler {
        return
    }
    UNUserNotificationCenter.current().delegate = PushNotificationHandler.shared
    NSLog("[mobile-push] Set UNUserNotificationCenter.delegate")
}

/// Injects APNs callback methods into the Tao-generated dynamic AppDelegate
/// class, so registration results reach the waiting `getDeviceToken` call.
///
/// Lazily: it needs `UIApplication.shared.delegate`, which Tao creates during
/// launch, and nothing needs it until a token is requested. Must be called on
/// the main thread.
private func setupApnsDelegateInternal() {
    installNotificationCenterDelegate()
    configureFirebase()

    guard !apnsDelegateSetUp else { return }

    guard let delegate = UIApplication.shared.delegate else {
        NSLog("[mobile-push] setupApnsDelegate: no UIApplication.delegate found")
        return
    }

    let cls: AnyClass = type(of: delegate)
    NSLog("[mobile-push] Injecting APNs methods into %@", NSStringFromClass(cls))

    // application:didRegisterForRemoteNotificationsWithDeviceToken:
    let didRegisterSel = sel_registerName("application:didRegisterForRemoteNotificationsWithDeviceToken:")
    let didRegisterBlock: @convention(block) (AnyObject, UIApplication, NSData) -> Void = { _, _, tokenNSData in
        let tokenData = tokenNSData as Data
        NSLog("[mobile-push] APNs token received (%d bytes)", tokenData.count)
        MobilePushPlugin.instance?.handleApnsToken(tokenData)
    }
    let didRegisterImp = imp_implementationWithBlock(didRegisterBlock as Any)
    if class_addMethod(cls, didRegisterSel, didRegisterImp, "v@:@@") {
        NSLog("[mobile-push] Added didRegisterForRemoteNotifications to AppDelegate")
    } else {
        NSLog("[mobile-push] didRegisterForRemoteNotifications already exists on AppDelegate")
    }

    // application:didFailToRegisterForRemoteNotificationsWithError:
    let didFailSel = sel_registerName("application:didFailToRegisterForRemoteNotificationsWithError:")
    let didFailBlock: @convention(block) (AnyObject, UIApplication, NSError) -> Void = { _, _, error in
        NSLog("[mobile-push] APNs registration failed: %@", error.localizedDescription)
        activeTokenFetcher?.reject(error.localizedDescription)
        MobilePushPlugin.instance?.handleTokenError(error as Error)
    }
    let didFailImp = imp_implementationWithBlock(didFailBlock as Any)
    if class_addMethod(cls, didFailSel, didFailImp, "v@:@@") {
        NSLog("[mobile-push] Added didFailToRegisterForRemoteNotifications to AppDelegate")
    } else {
        NSLog("[mobile-push] didFailToRegisterForRemoteNotifications already exists on AppDelegate")
    }

    apnsDelegateSetUp = true
}

// MARK: - Direct FFI functions (bypass PluginManager dispatch)

/// Configure what iOS shows when a notification arrives while the app is
/// foreground. `options` is the `UNNotificationPresentationOptions` bitmask
/// assembled on the Rust side from `ForegroundPresentationOptions`.
/// Called once from the Rust plugin setup after `register_ios_plugin`.
@_cdecl("mobile_push_set_foreground_presentation")
func setForegroundPresentation(_ options: UInt32) {
    configuredForegroundPresentation =
        UNNotificationPresentationOptions(rawValue: UInt(options))
    NSLog("[mobile-push] Foreground presentation set: %u", options)
}

/// Request notification permission. Blocks until the user responds (30s timeout).
/// Returns 1 if granted, 0 if denied or error.
@_cdecl("mobile_push_request_permission")
func requestPermissionDirect() -> Int32 {
    let sem = DispatchSemaphore(value: 0)
    var granted = false

    UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .badge, .sound]) { result, error in
        if let error = error {
            NSLog("[mobile-push] requestAuthorization error: %@", error.localizedDescription)
        }
        granted = result
        sem.signal()
    }

    let result = sem.wait(timeout: .now() + 30)
    if result == .timedOut {
        NSLog("[mobile-push] requestAuthorization timed out")
        return 0
    }

    NSLog("[mobile-push] Permission %@", granted ? "granted" : "denied")
    return granted ? 1 : 0
}

/// Get the FCM registration token. Blocks until the token is received or timeout.
/// Writes the token string (null-terminated) to buffer.
/// Returns: >0 = token length, -1 = error, -2 = timeout.
///
/// The symbol keeps upstream's name so `src/commands.rs` is unchanged.
@_cdecl("mobile_push_get_device_token")
func getDeviceTokenDirect(_ buffer: UnsafeMutablePointer<CChar>, _ bufferLen: Int32, _ timeoutSecs: Int32) -> Int32 {
    // Serialize concurrent calls
    tokenLock.lock()
    defer { tokenLock.unlock() }

    // Ensure Firebase and the APNs delegate are set up
    var configured = false
    let setup = {
        setupApnsDelegateInternal()
        configured = FirebaseApp.app() != nil
    }
    if Thread.isMainThread {
        setup()
    } else {
        DispatchQueue.main.sync(execute: setup)
    }
    guard configured else {
        NSLog("[mobile-push] No Firebase configuration; cannot issue a token")
        return -1
    }

    let fetcher = TokenFetcher()
    activeTokenFetcher = fetcher

    // Register for remote notifications (must be on main thread). iOS answers
    // through the injected delegate method every time, even with a cached
    // token, and `handleApnsToken` then asks Firebase for the FCM token.
    DispatchQueue.main.async {
        NSLog("[mobile-push] Calling registerForRemoteNotifications...")
        UIApplication.shared.registerForRemoteNotifications()
    }

    // Wait for the token
    let timeout = max(Int(timeoutSecs), 5)
    let result = fetcher.semaphore.wait(timeout: .now() + .seconds(timeout))
    activeTokenFetcher = nil

    if result == .timedOut {
        NSLog("[mobile-push] Token request timed out after %ds", timeout)
        return -2
    }

    if fetcher.error != nil {
        NSLog("[mobile-push] Token request failed")
        return -1
    }

    guard let token = fetcher.token else {
        NSLog("[mobile-push] Token is nil after semaphore signal")
        return -1
    }

    // Write token to buffer
    let bytes = Array(token.utf8)
    guard bytes.count < Int(bufferLen) else {
        NSLog("[mobile-push] Token too long for buffer: %d >= %d", bytes.count, bufferLen)
        return -1
    }
    for (i, b) in bytes.enumerated() {
        buffer[i] = CChar(bitPattern: b)
    }
    buffer[bytes.count] = 0

    NSLog("[mobile-push] Token (%d chars) written to buffer", bytes.count)
    return Int32(bytes.count)
}

// MARK: - Plugin class (kept for lifecycle + event callbacks)

@objc(MobilePushPlugin)
public class MobilePushPlugin: Plugin {
    public static var instance: MobilePushPlugin?

    override public func load(webview: WKWebView) {
        MobilePushPlugin.instance = self
        // The app has launched by now, so this is the earliest safe point to
        // configure Firebase: its token refresh then reaches `MessagingHandler`
        // for a relaunch that never calls `getToken`.
        configureFirebase()
        NSLog("[mobile-push] Plugin loaded (webview ready)")
    }

    // MARK: - Callbacks from AppDelegate injection

    /// Apple issued (or re-issued) the APNs token. Firebase learns it, works
    /// out from the embedded provisioning profile whether it is a sandbox or
    /// production token, and answers with the FCM token that stands for it.
    public func handleApnsToken(_ token: Data) {
        Messaging.messaging().apnsToken = token
        Messaging.messaging().token { fcmToken, error in
            if let error = error {
                NSLog("[mobile-push] FCM token request failed")
                activeTokenFetcher?.reject(error.localizedDescription)
                return
            }
            guard let fcmToken = fcmToken, !fcmToken.isEmpty else {
                activeTokenFetcher?.reject("Firebase returned no token")
                return
            }
            activeTokenFetcher?.resolve(fcmToken)
        }
    }

    public func handleTokenError(_ error: Error) {
        NSLog("[mobile-push] Token callback failed")
    }
}

// MARK: - Plugin entry point

@_cdecl("init_plugin_mobile_push")
func initPlugin() -> Plugin {
    // Set the notification-center delegate here rather than waiting for the
    // first `getDeviceToken`. The plugin is registered while the app is being
    // built, before `UIApplicationMain` runs, so this is the earliest hook
    // this plugin has — and installing the delegate only on demand is what
    // loses a tap that launched the app: iOS delivers that response once, to
    // whichever delegate is set when launch completes, and nothing in a
    // relaunch that already holds a stored device token ever calls
    // `getDeviceToken`.
    //
    // `setupApnsDelegateInternal` still installs it as well, so a command that
    // reaches a process where this early install did not stick behaves exactly
    // as before. The AppDelegate method injection stays lazy there:
    // `UIApplication.shared.delegate` may not exist yet at this point (Tao
    // creates the class during launch), and only a token request needs it.
    // Firebase is not configured here either: it wants a launched app.
    installNotificationCenterDelegate()
    return MobilePushPlugin()
}
