use crossbeam_channel::{Receiver, Sender, unbounded};
use once_cell::sync::Lazy;
use std::sync::Mutex;

mod android_artifact;

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "ios")]
mod ios;

/// Events the native layer pushes to Rust.
#[derive(Debug, Clone)]
pub enum TokenEvent {
    /// First token or refreshed token
    NewToken(String),
    /// Registration failed
    Error(String),
}

/// Global channel so native callbacks can hand tokens to Rust.
static TOKEN_CHANNEL: Lazy<(Sender<TokenEvent>, Receiver<TokenEvent>)> = Lazy::new(unbounded);

/// Cache the last known token.
static CURRENT_TOKEN: Lazy<Mutex<Option<String>>> = Lazy::new(|| Mutex::new(None));

/// Called by the native bridge (JNI/FFI) when a token arrives.
pub(crate) fn on_native_token(token: String) {
    *CURRENT_TOKEN.lock().unwrap() = Some(token.clone());
    let _ = TOKEN_CHANNEL.0.send(TokenEvent::NewToken(token));
}

pub(crate) fn on_native_error(msg: String) {
    let _ = TOKEN_CHANNEL.0.send(TokenEvent::Error(msg));
}

// ---- Public API used by your Dioxus app ----

// ---- Public API used by your Dioxus app ----

/// Fetch the FCM token. Resolves once Firebase delivers (or fails).
#[cfg(target_os = "android")]
pub async fn request_token() -> Option<String> {
    android::request_token().await
}

#[cfg(target_os = "ios")]
pub async fn request_token() -> Option<String> {
    todo!("FCM token fetch is not implemented for iOS yet")
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub async fn request_token() -> Option<String> {
    None
}

/// Request notification permission. Resolves with the user's choice.
#[cfg(target_os = "android")]
pub async fn request_notification_permission() -> bool {
    android::request_notification_permission().await
}

#[cfg(target_os = "ios")]
pub async fn request_notification_permission() -> bool {
    todo!("notification permission request is not implemented for iOS yet")
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub async fn request_notification_permission() -> bool {
    false
}

/// Synchronous permission check — no dialog.
#[cfg(target_os = "android")]
pub fn has_notification_permission() -> bool {
    android::notifications_enabled()
}

#[cfg(target_os = "ios")]
pub fn has_notification_permission() -> bool {
    todo!("notification permission check is not implemented for iOS yet")
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn has_notification_permission() -> bool {
    false
}

/// Initialize Firebase. Call once at startup.
#[cfg(target_os = "android")]
pub fn init_fcm() {
    android::init_fcm();
}

#[cfg(target_os = "ios")]
pub fn init_fcm() {
    todo!("Firebase init is not implemented for iOS yet")
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn init_fcm() {}

/// Probe: is the Kotlin side reachable? Android-only by nature.
#[cfg(target_os = "android")]
pub fn kotlin_available() -> Option<String> {
    android::kotlin_available()
}

#[cfg(not(target_os = "android"))]
pub fn kotlin_available() -> Option<String> {
    None // there's no Kotlin on iOS/desktop — None is honest here
}
