# Dioxus FCM
Crate for Firebase Cloud Messaging for dioxus mobile.

What it should be able to do?
get fcm token + update it on andorid and ios

Steps to setup.

## Part 1: Firebase Project Setup
1. Create a Firebase project at console.firebase.google.com
2. Add an Android app → download google-services.json
3. Add an iOS app → download GoogleService-Info.plist
4. For iOS: upload your APNs key
    - Go to Apple Developer → create an APNs Auth Key (.p8)
    - Upload it in Firebase Console → Project Settings → Cloud Messaging → APNs
5. Create a service account for backend sending:
    - Firebase Console → Project Settings → Service Accounts → Generate new private key
    - Downloads a JSON file — keep this secret (it's your backend credential)


fcm-token-bridge/
├── Cargo.toml
├── build.rs                    # link native libs
├── src/
│   ├── lib.rs                  # public API + platform dispatch
│   ├── android.rs              # JNI bridge
│   ├── ios.rs                  # FFI bridge
│   └── token_store.rs          # in-memory + callback registry
├── android/
│   └── FcmService.kt           # Kotlin FirebaseMessagingService
└── ios/
    └── FcmBridge.swift         # Swift APNs/FCM registration
