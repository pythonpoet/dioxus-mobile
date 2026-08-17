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
    private var configured = false
    private let stateLock = NSLock()

    override init() {
        super.init()
    }

    /// Load `GoogleService-Info.plist` bundled inside this Swift package (see
    /// `Sources/Resources/`) and configure Firebase from it. Safe to call more than once.
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
        // `.copy("Resources")` in Package.swift may either flatten the file to the bundle
        // root or preserve the "Resources/" subpath depending on the SwiftPM toolchain —
        // try both so this keeps working across versions.
        let plistURL = Bundle.module.url(
            forResource: "GoogleService-Info", withExtension: "plist", subdirectory: "Resources"
        ) ?? Bundle.module.url(forResource: "GoogleService-Info", withExtension: "plist")

        guard let plistURL, let options = FirebaseOptions(contentsOfFile: plistURL.path) else {
            NSLog(
                "dioxus-fcm: GoogleService-Info.plist not found in the FcmPlugin package "
                    + "(crates/dioxus-fcm/src/ios/plugin/Sources/Resources/) — Firebase not configured"
            )
            return nil
        }

        if options.googleAppID == "REPLACE_ME" {
            NSLog(
                "dioxus-fcm: GoogleService-Info.plist is still the placeholder — download the "
                    + "real file from the Firebase console (Project settings → your iOS app) and "
                    + "replace crates/dioxus-fcm/src/ios/plugin/Sources/Resources/GoogleService-Info.plist"
            )
            return nil
        }

        return options
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
    /// Returns an empty string on error/timeout — Rust maps that to `None`.
    @objc public func nativeRequestToken() -> String {
        stateLock.lock()
        let cached = latestToken
        stateLock.unlock()
        if let cached, !cached.isEmpty { return cached }

        guard FirebaseApp.app() != nil else {
            NSLog("dioxus-fcm: requestToken called before Firebase was configured")
            return ""
        }

        let semaphore = DispatchSemaphore(value: 0)
        var result = ""

        Messaging.messaging().token { token, error in
            if let token {
                result = token
            } else if let error {
                NSLog("dioxus-fcm: token fetch failed: \(error.localizedDescription)")
            }
            semaphore.signal()
        }

        _ = semaphore.wait(timeout: .now() + 15)
        return result
    }
}
