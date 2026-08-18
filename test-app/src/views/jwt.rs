use dioxus::prelude::*;
use dioxus_jwt::use_jwt;

use super::{BTN, CARD, PAGE_STYLE};

/// dioxus-jwt test page.
///
/// Flow: enter `admin` / `password`, tap Login. The server function issues an
/// HS256 token which is persisted locally. Close the app, reopen: still
/// authenticated (client-side UX check backed by the persisted token).
#[component]
pub fn Jwt() -> Element {
    let auth = use_jwt();
    let mut username = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut status = use_signal(String::new);

    let authenticated = auth.is_authenticated();
    let safe = auth.safe_status();

    rsx! {
        div { style: PAGE_STYLE,
            h1 { "dioxus-jwt" }

            div { style: CARD,
                p {
                    "Status: "
                    if authenticated { "authenticated" } else { "not authenticated" }
                }
                p { "Storage: {safe.storage_kind}" }
                if let Some(len) = safe.token_length {
                    p { "Token length: {len}" }
                }
                if let Some(exp) = safe.expires_at {
                    p { "Expires at (unix): {exp}" }
                }
            }

            if !authenticated {
                input {
                    placeholder: "username (admin)",
                    value: username,
                    oninput: move |e| username.set(e.value()),
                }
                input {
                    placeholder: "password (password)",
                    value: password,
                    oninput: move |e| password.set(e.value()),
                }
                button {
                    style: BTN,
                    onclick: move |_| async move {
                        let user = username();
                        let pass = password();
                        match crate::login(user, pass).await {
                            Ok(token) => {
                                let mut auth = auth;
                                auth.login(token);
                                status.set("logged in".to_string());
                            }
                            Err(e) => {
                                status.set(format!("login failed: {e}"));
                            }
                        }
                    },
                    "Login"
                }
            } else {
                button {
                    style: BTN,
                    onclick: move |_| {
                        let mut auth = auth;
                        auth.logout();
                        status.set("logged out".to_string());
                    },
                    "Logout"
                }
            }

            p { "{status}" }
        }
    }
}
