# Dioxus FCM

A Firebase Cloud Messaging (FCM) crate for Dioxus mobile apps. Android and iOS are both
supported; the project is still early-stage — do not use it in production.

For more information about FCM, visit: https://firebase.google.com/docs/cloud-messaging/

> **Privacy note:** If you use Dioxus Fullstack to fulfill data integrity
> requirements, be aware that messages are subject to FCM's terms of use and
> pass through Google's servers. If your project has to be GDPR-compliant,
> using a managed solution from big tech is a liability:
> https://www.activemind.legal/guides/us-cloud-act/
>
> There are projects working on self-hosted alternatives, like Namma Push:
> https://blacklovertech.medium.com/building-namma-push-the-open-source-pure-rust-alternative-to-firebase-cloud-messaging-9507b3f1bf4f
>
> Let me know if you know of another alternative to FCM.

## License

[![Hippocratic License HL3-BDS-ECO-EXTR-FFD-MIL-SV](https://img.shields.io/static/v1?label=Hippocratic%20License&message=HL3-BDS-ECO-EXTR-FFD-MIL-SV&labelColor=5e2751&color=bc8c3d)](https://firstdonoharm.dev/version/3/0/bds-eco-extr-ffd-mil-sv.html)

## Setup

1. Create an `android` folder in the root of your Dioxus project.
2. Create a Firebase project at https://console.firebase.google.com
3. Add an Android app to the project → download `google-services.json` and
   copy it into your `android` folder.
4. Copy `test-app/android/AndroidManifest.xml` into your `android` folder and
   reference it in your `Dioxus.toml`:

   ```toml
   [android]
   custom_manifest = "android/AndroidManifest.xml"

   [android.permissions]
   "android.permission.INTERNET" = { description = "Network access for push notifications" }
   "android.permission.POST_NOTIFICATIONS" = { description = "Show push notifications" }
   ```

   The manifest must declare `FcmService` (see the test-app manifest) —
   without it, token refresh callbacks never fire.

5. Add `dioxus-fcm` to your `Cargo.toml`.

> **Note:** FCM also requires the `google-services` Gradle plugin and the
> Firebase dependencies in your Android build. See `test-app` for a working
> configuration.

### iOS setup

The iOS bridge (`src/ios/plugin`) is a Swift Package bound to Rust via
`#[manganis::ffi]` — `dx build --ios` compiles and links it into the app bundle
automatically. There's no Xcode project to hand-edit, but you do need a few things
from your Apple/Firebase accounts before push actually works:

1. Add an iOS app to your Firebase project (Firebase Console → Project settings →
   your apps → Add app → iOS). Use the same bundle identifier as `[bundle] identifier`
   / `[ios] identifier` in your app's `Dioxus.toml`.
2. Download the generated `GoogleService-Info.plist` and place it at
   `<app>/ios/GoogleService-Info.plist` in your app's source tree. `dioxus-fcm`
   stages that file via its build script and declares it as an unhashed manganis asset, so
   `dx build --ios` copies it into `App.app/assets/GoogleService-Info.plist`; the
   Swift bridge reads it from there (with the bundle root as a fallback).
   (`init_fcm()` detects a placeholder with `GOOGLE_APP_ID = "REPLACE_ME"` and logs a
   warning instead of configuring Firebase, so the app doesn't crash if you forget
   this step, but `request_token()` will always return `Err(<message>)`.)
3. Create an APNs Auth Key (.p8) in your Apple Developer account (Certificates,
   Identifiers & Profiles → Keys) and upload it in Firebase Console → Project
   Settings → Cloud Messaging → Apple app configuration → APNs Authentication Key.
4. Enable the **Push Notifications** capability for your App ID in the Apple
   Developer portal, and make sure your provisioning profile / entitlements include
   it — add this to your app's `Dioxus.toml`:

   ```toml
   [ios]
   deployment_target = "14.0"

   [ios.entitlements]
   aps-environment = "development"  # "production" for a TestFlight/App Store build
   ```

5. Add `dioxus-fcm` to your `Cargo.toml`.

The first `dx build --ios` will fetch and compile the Firebase iOS SDK via Swift
Package Manager, which is slow (network + a large Swift build) — subsequent builds
are cached.

## Usage

The request functions are **async** — they wait for a callback from the
native layer (Firebase response / permission dialog). Always call them from
an async context (`async` event handler, `spawn`, `use_future`) and never
block the main thread.

```rust
// Initialize once at startup
dioxus_fcm::init_fcm();

// Test the Rust ↔ Kotlin bridge (returns Some("inshallah"))
let probe = dioxus_fcm::kotlin_available();

// Request notification permission — resolves with the user's choice
let granted: bool = dioxus_fcm::request_notification_permission().await;

// Fetch the FCM token — resolves with the token or an error message
match dioxus_fcm::request_token().await {
    Ok(token) => {
        // send the token to your server
    }
    Err(e) => {
        // surface the native error (e.g. token_error.set(e))
    }
}
```

Example button in Dioxus:

```rust
let mut token = use_signal(|| None::<String>);
let mut token_error = use_signal(String::new);

button {
    onclick: move |_| async move {
        if dioxus_fcm::request_notification_permission().await {
            match dioxus_fcm::request_token().await {
                Ok(t) => {
                    token.set(Some(t));
                    token_error.set(String::new());
                }
                Err(e) => {
                    token.set(None);
                    token_error.set(e);
                }
            }
        }
    },
    "Enable notifications"
}
```

Helpers:

```rust
dioxus_fcm::cached_token();              // last known token, non-blocking
dioxus_fcm::has_notification_permission(); // synchronous check, no dialog
```

Listen for background events (token rotation, errors):

```rust
use_future(move || async move {
    let rx = dioxus_fcm::events();
    loop {
        // crossbeam's recv() blocks, so poll with a timeout and yield
        // to keep the async executor responsive
        match rx.recv_timeout(std::time::Duration::from_millis(500)) {
            Ok(dioxus_fcm::TokenEvent::NewToken(t)) => { /* token rotated */ }
            Ok(dioxus_fcm::TokenEvent::Error(e)) => { /* log the error */ }
            Ok(dioxus_fcm::TokenEvent::PermissionResult(g)) => { /* ... */ }
            Err(_) => tokio::task::yield_now().await,
        }
    }
});
```

## iOS limitations

Unlike Android's JNI bridge (which calls back into Rust unprompted whenever Firebase
rotates a token), the iOS Swift plugin only *caches* a rotated token internally — it has
no way to push an unsolicited `TokenEvent::NewToken` into Rust. On iOS, `events()` will
only ever emit what you already knew from a `request_token()` call. If you need to notice
a rotated token, call `request_token()` again (e.g. on app foreground) rather than relying
on the event channel.

## Disclaimer

This is a work-in-progress project — do not use it in production.

There may eventually be an icon generator for push notification icons.
