//! iOS bridge for dioxus-fcm.
//!
//! Binds directly to a Swift Package (`src/ios/plugin`, see `Sources/FcmPlugin.swift`)
//! via the `#[manganis::ffi]` macro. dx extracts the linked `SwiftPackageMetadata`,
//! compiles that package into a dynamic framework, and links it into the app bundle
//! automatically at `dx build --ios` time — there's no Xcode project to hand-edit.
//!
//! Firebase needs `GoogleService-Info.plist`; the consuming app provides it at
//! `<app>/ios/GoogleService-Info.plist` (loaded from the main app bundle at runtime).

use once_cell::sync::Lazy;

#[manganis::ffi("src/ios/plugin")]
unsafe extern "Swift" {
    /// The native FcmPlugin class (see Sources/FcmPlugin.swift).
    pub type FcmPlugin;

    /// Load GoogleService-Info.plist and configure Firebase + the messaging/notification
    /// delegates. Safe to call more than once.
    pub fn native_configure(this: &FcmPlugin) -> ();

    /// Show the system permission dialog; blocks until the user responds (or times out).
    /// Returns "true"/"false" — see the note on the Swift side for why not `Bool`.
    pub fn native_request_permission(this: &FcmPlugin) -> String;

    /// Synchronous permission check — no dialog. Returns "true"/"false".
    pub fn native_has_permission(this: &FcmPlugin) -> String;

    /// Fetch the FCM token; blocks until Firebase delivers it (or times out).
    /// Empty string means "no token" (Firebase not configured, or timeout/error).
    pub fn native_request_token(this: &FcmPlugin) -> String;
    /// Last error from `native_request_token`; empty when the last call succeeded.
    pub fn native_last_token_error(this: &FcmPlugin) -> String;
    /// Probe: is the Swift side reachable? Returns "switft inshallah" on success.
    pub fn native_test_interface(this: &FcmPlugin) -> String;
}

/// One retained plugin instance for the process lifetime. `Messaging.delegate` and
/// `UNUserNotificationCenter.delegate` don't retain their delegate, so the Swift object
/// backing this has to stay alive for callbacks (like token refresh) to keep firing.
static PLUGIN: Lazy<FcmPlugin> =
    Lazy::new(|| FcmPlugin::new().expect("failed to initialize the FcmPlugin Swift bridge"));

pub fn init_fcm() {
    if let Err(e) = native_configure(&*PLUGIN) {
        tracing::error!("dioxus-fcm: native_configure failed: {e}");
    }
}

pub async fn request_notification_permission() -> bool {
    tokio::task::spawn_blocking(|| {
        native_request_permission(&*PLUGIN).unwrap_or_default() == "true"
    })
    .await
    .unwrap_or(false)
}

/// Synchronous — the underlying `getNotificationSettings` completion handler resolves on
/// a private queue almost immediately, so a short blocking wait here is safe (same
/// contract as `android::notifications_enabled`).
pub fn has_notification_permission() -> bool {
    native_has_permission(&*PLUGIN).unwrap_or_default() == "true"
}

pub async fn request_token() -> Result<String, String> {
    tokio::task::spawn_blocking(|| {
        let token = native_request_token(&*PLUGIN).unwrap_or_default();
        if !token.is_empty() {
            return Ok(token);
        }
        let error = native_last_token_error(&*PLUGIN).unwrap_or_default();
        if error.is_empty() {
            Err("FCM token request failed with no native error message".to_string())
        } else {
            Err(error)
        }
    })
    .await
    .unwrap_or_else(|join_err| Err(format!("FCM token worker failed: {join_err}")))
}

/// Probe: is the Swift side reachable? Should return "switft inshallah".
pub fn test_interface() -> Option<String> {
    let msg = native_test_interface(&*PLUGIN).unwrap_or_default();
    if msg.is_empty() { None } else { Some(msg) }
}
