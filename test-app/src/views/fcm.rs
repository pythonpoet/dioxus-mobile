use dioxus::prelude::*;

use super::{BTN, CARD, PAGE_STYLE};

/// FCM test page: notification permission and token fetch.
#[component]
pub fn Fcm() -> Element {
    let mut test_interface = use_signal(|| None::<String>);
    let mut permission = use_signal(|| None::<bool>);
    let mut token = use_signal(|| None::<String>);
    let mut token_error = use_signal(String::new);

    // Initialize Firebase once and probe the Kotlin bridge (Android only).
    use_effect(move || {
        dioxus_fcm::init_fcm();
        test_interface.set(dioxus_fcm::test_interface());
    });

    rsx! {
        div { style: PAGE_STYLE,
            h1 { "dioxus-fcm" }

            div { style: CARD,
                if let Some(k) = test_interface() {
                    p { "Native bridge: {k}" }
                } else {
                    p { "Native bridge: unavailable on this platform" }
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
                },
                "Read FCM token"
            }
            if let Some(t) = token() {
                p { style: "word-break: break-all; font-family: monospace; font-size: 12px;", "Token: {t}" }
            }
            if !token_error().is_empty() {
                p {
                    style: "word-break: break-all; font-family: monospace; font-size: 12px; color: #f87171;",
                    "Token error: {token_error}"
                }
            }
        }
    }
}
