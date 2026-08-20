import Foundation
import FirebaseCore
import FirebaseMessaging
import UserNotifications
#if canImport(UIKit)
import UIKit
#endif

/// Native iOS bridge for dioxus-fcm, called directly from Rust via `#[manganis::ffi]`
/// (see `../../ios.rs`). dx compiles this Swift package into a framework and links it
/// into the app bundle automatically at `dx build --ios` time — nothing here needs to
/// be wired into an Xcode project by hand.
///
/// Every method here blocks the calling thread until the native callback fires, mirroring
/// how the Firebase/UserNotifications completion-handler APIs work. Those completion
/// handlers run on their own private queues (not the calling thread), so blocking with a
/// semaphore here is safe. The Rust side always calls the slow ones (permission + token)
/// via `tokio::task::spawn_blocking` so the async runtime never stalls.
@objc(FcmPlugin)
public class FcmPlugin: NSObject, MessagingDelegate, UNUserNotificationCenterDelegate {
    private var latestToken: String?
    private var lastTokenError: String?
    private var configured = false
    private let stateLock = NSLock()

    override init() {
        super.init()
    }

    /// Load `GoogleService-Info.plist` from the main app bundle and configure Firebase from
    /// it. The plist comes from the consuming app's `<app>/ios/GoogleService-Info.plist`, not
    /// from a copy bundled inside this Swift package. Safe to call more than once.
    @objc public func nativeConfigure() {
        stateLock.lock()
        let alreadyConfigured = configured
        stateLock.unlock()
        if alreadyConfigured { return }

        guard let options = loadFirebaseOptions() else {
            return
        }

        if FirebaseApp.app() == nil {
            FirebaseApp.configure(options: options)
        }
        Messaging.messaging().delegate = self
        UNUserNotificationCenter.current().delegate = self

        stateLock.lock()
        configured = true
        stateLock.unlock()
    }

    private func loadFirebaseOptions() -> FirebaseOptions? {
        // Firebase expects the plist in the *app* bundle, not the Swift package bundle:
        // dx installs the FcmPlugin dylib without its SwiftPM resources, so `Bundle.module`
        // is empty at runtime. The consuming app's `<app>/ios/GoogleService-Info.plist` is the
        // authoritative source and belongs at the main bundle root.
        guard let plistURL = Bundle.main.url(
            forResource: "GoogleService-Info", withExtension: "plist"
        ), let options = FirebaseOptions(contentsOfFile: plistURL.path) else {
            NSLog(
                "dioxus-fcm: GoogleService-Info.plist not found in the app bundle "
                    + "(<app>/ios/GoogleService-Info.plist) — Firebase not configured"
            )
            return nil
        }

        if options.googleAppID == "REPLACE_ME" {
            NSLog(
                "dioxus-fcm: GoogleService-Info.plist is still the placeholder — download the "
                    + "real file from the Firebase console (Project settings → your iOS app) and "
                    + "place it at <app>/ios/GoogleService-Info.plist"
            )
            return nil
        }

        return options
    }

    // MARK: Probe

    /// Confirm the Swift bridge is reachable from Rust. Returns "switft inshallah".
    @objc public func nativeTestInterface() -> String {
        return "switft inshallah"
    }

    // MARK: MessagingDelegate

    public func messaging(_ messaging: Messaging, didReceiveRegistrationToken fcmToken: String?) {
        stateLock.lock()
        latestToken = fcmToken
        stateLock.unlock()
    }

    // MARK: UNUserNotificationCenterDelegate

    /// Show the banner/sound/list even while the app is in the foreground — otherwise iOS
    /// silently swallows notifications for an active app.
    public func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) ->
            Void
    ) {
        completionHandler([.banner, .sound, .list])
    }

    // MARK: Permission
    //
    // These report booleans as "true"/"false" strings rather than Bool: the pinned
    // manganis ffi macro generates `result.as_bool()` on a raw `*mut AnyObject` for
    // Bool-returning bridged functions, which doesn't compile. String is the
    // confirmed-working return path (used throughout the geolocation plugin example),
    // so Rust parses these back into bool on its side.

    /// Show the system permission dialog; blocks until the user responds (or times out).
    @objc public func nativeRequestPermission() -> String {
        let semaphore = DispatchSemaphore(value: 0)
        var granted = false

        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .badge, .sound])
        { result, error in
            if let error {
                NSLog("dioxus-fcm: permission request failed: \(error.localizedDescription)")
            }
            granted = result
            semaphore.signal()
        }

        _ = semaphore.wait(timeout: .now() + 60)

        if granted {
            DispatchQueue.main.async {
                #if canImport(UIKit)
                    UIApplication.shared.registerForRemoteNotifications()
                #endif
            }
        }
        return granted ? "true" : "false"
    }

    /// Synchronous permission check — no dialog.
    @objc public func nativeHasPermission() -> String {
        let semaphore = DispatchSemaphore(value: 0)
        var enabled = false

        UNUserNotificationCenter.current().getNotificationSettings { settings in
            enabled =
                settings.authorizationStatus == .authorized
                || settings.authorizationStatus == .provisional
            semaphore.signal()
        }

        _ = semaphore.wait(timeout: .now() + 5)
        return enabled ? "true" : "false"
    }

    // MARK: Token

    /// Fetch the FCM token; blocks until Firebase delivers it (or times out).
    /// Returns an empty string on error/timeout; `nativeLastTokenError()` exposes the
    /// underlying native error for the Rust side to forward.
    @objc public func nativeRequestToken() -> String {
        stateLock.lock()
        let cached = latestToken
        stateLock.unlock()
        if let cached, !cached.isEmpty {
            lastTokenError = nil
            return cached
        }

        guard FirebaseApp.app() != nil else {
            let message = "Firebase is not configured. Add <app>/ios/GoogleService-Info.plist and call init_fcm()."
            NSLog("dioxus-fcm: requestToken called before Firebase was configured")
            lastTokenError = message
            return ""
        }

        let semaphore = DispatchSemaphore(value: 0)
        var result = ""

        Messaging.messaging().token { token, error in
            if let token {
                result = token
                lastTokenError = nil
            } else if let error {
                NSLog("dioxus-fcm: token fetch failed: \(error.localizedDescription)")
                lastTokenError = error.localizedDescription
            } else {
                lastTokenError = "FCM returned neither a token nor an error"
            }
            semaphore.signal()
        }

        let waitResult = semaphore.wait(timeout: .now() + 15)
        if result.isEmpty && waitResult == .timedOut {
            lastTokenError = "Timed out waiting for FCM token (15s)"
        }
        return result
    }

    /// Last error recorded by `nativeRequestToken()`; empty when the last call succeeded.
    @objc public func nativeLastTokenError() -> String {
        return lastTokenError ?? ""
    }
}
