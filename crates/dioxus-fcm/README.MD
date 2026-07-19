# Dioxus FCM

A Firebase Cloud Messaging (FCM) crate for Dioxus mobile apps. The project is at an early stage — only Android is supported at the moment.

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

### Future iOS support (not implemented yet)

1. Add an iOS app to your Firebase project → download `GoogleService-Info.plist`.
2. Upload your APNs key:
   - Go to Apple Developer → create an APNs Auth Key (.p8)
   - Upload it in Firebase Console → Project Settings → Cloud Messaging → APNs

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

// Fetch the FCM token — resolves once Firebase delivers it (None on error/timeout)
if let Some(token) = dioxus_fcm::request_token().await {
    // send the token to your server
}
```

Example button in Dioxus:

```rust
button {
    onclick: move |_| async move {
        if dioxus_fcm::request_notification_permission().await {
            token.set(dioxus_fcm::request_token().await);
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

## Disclaimer

This is a work-in-progress project — do not use it in production.

In the future, iOS might also be supported, and there may be an icon
generator for push notification icons.
