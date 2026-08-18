use dioxus::prelude::*;

use super::{CARD, PAGE_STYLE};

/// Landing page: quick map of the other pages and how to run the auth server.
#[component]
pub fn Init() -> Element {
    rsx! {
        div { style: PAGE_STYLE,
            h1 { "Init" }
            p { "Landing page for the dioxus-mobile test app. Use the nav above to switch pages." }

            div { style: CARD,
                p { "FCM" }
                p { "Notification permission and token fetch against dioxus-fcm." }
            }
            div { style: CARD,
                p { "JWT" }
                p { "Bare-bones axum auth router login check. Login as admin / password, close the app, reopen: still authenticated." }
            }
            div { style: CARD,
                p { "Recorder" }
                p { "Denoised microphone recording and local playback." }
            }

            div { style: CARD,
                p { "Auth server" }
                p { "Run on the host with:" }
                pre { "cargo run --no-default-features --features server,tracing" }
                p { "On a device, point SERVER_URL in src/main.rs at the host's LAN IP." }
            }
        }
    }
}
