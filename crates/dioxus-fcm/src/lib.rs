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

/// Kick off registration. Native layer will call back with a token.
pub fn request_token() {
    #[cfg(target_os = "android")]
    android::request_token();
    #[cfg(target_os = "ios")]
    ios::request_token();
}
pub fn kotlin_available() {
    #[cfg(target_os = "android")]
    android::kotlin_available();
    // #[cfg(target_os = "ios")]
    // ios::request_token();
}

/// Non-blocking: get the cached token if we already have one.
pub fn cached_token() -> Option<String> {
    CURRENT_TOKEN.lock().unwrap().clone()
}

/// Receive token events (new token / refresh / error) in an async loop.
pub fn events() -> Receiver<TokenEvent> {
    TOKEN_CHANNEL.1.clone()
}
