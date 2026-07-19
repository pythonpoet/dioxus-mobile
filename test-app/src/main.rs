use dioxus::prelude::*;

use components::Hero;

mod components;

const FAVICON: Asset = asset!("/assets/favicon.ico");
const MAIN_CSS: Asset = asset!("/assets/styling/main.css");
const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    use dioxus_fcm::{
        kotlin_available, request_notification_permission, request_token,
    };

    // Results live in signals so the UI re-renders when they arrive
    let mut kotlin_says = use_signal(|| None::<String>);
    let mut permission = use_signal(|| None::<bool>);
    let mut token = use_signal(|| None::<String>);

    rsx! {
        document::Link { rel: "icon", href: FAVICON }
        document::Link { rel: "stylesheet", href: MAIN_CSS }
        document::Link { rel: "stylesheet", href: TAILWIND_CSS }

        h1 { "Hallo!" }

        // 1. Sync probe — call directly, store the return value
        div { class: "bg-red-100",
            button {
                onclick: move |_| kotlin_says.set(kotlin_available()),
                "Test kotlin!"
            }
            if let Some(s) = kotlin_says() {
                p { "Kotlin says: {s}" }
            }
        }

        // 2. Async permission — handler must be async so we can .await
        div { class: "bg-blue-100",
            button {
                onclick: move |_| async move {
                    permission.set(Some(request_notification_permission().await));
                },
                "Request permission"
            }
            if let Some(granted) = permission() {
                p { if granted { "🔔 granted" } else { "🚫 denied" } }
            }
        }

        // 3. Async token — same pattern
        div { class: "bg-blue-100",
            button {
                onclick: move |_| async move {
                    token.set(request_token().await);
                },
                "Read token!"
            }
            if let Some(t) = token() {
                p { class: "break-all", "token: {t}" }
            }
        }
    }
}
