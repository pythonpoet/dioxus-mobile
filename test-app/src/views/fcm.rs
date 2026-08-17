use dioxus::prelude::*;

use super::{BTN, CARD, PAGE_STYLE};

/// FCM test page: notification permission and token fetch.
#[component]
pub fn Fcm() -> Element {
    let mut kotlin = use_signal(|| None::<String>);
    let mut permission = use_signal(|| None::<bool>);
    let mut token = use_signal(|| None::<String>);

    // Initialize Firebase once and probe the Kotlin bridge (Android only).
    use_effect(move || {
        dioxus_fcm::init_fcm();
        kotlin.set(dioxus_fcm::kotlin_available());
    });

    rsx! {
        div { style: PAGE_STYLE,
            h1 { "dioxus-fcm" }

            div { style: CARD,
                if let Some(k) = kotlin() {
                    p { "Kotlin bridge: {k}" }
                } else {
                    p { "Kotlin bridge: unavailable on this platform" }
                }
            }

            button {
                style: BTN,
                onclick: move |_| async move {
                    permission.set(Some(dioxus_fcm::request_notification_permission().await));
                },
                "Request notification permission"
            }
            if let Some(granted) = permission() {
                p { if granted { "Permission: granted" } else { "Permission: denied" } }
            }

            button {
                style: BTN,
                onclick: move |_| async move {
                    token.set(dioxus_fcm::request_token().await);
                },
                "Read FCM token"
            }
            if let Some(t) = token() {
                p { style: "word-break: break-all; font-family: monospace; font-size: 12px;", "Token: {t}" }
            }
        }
    }
}
