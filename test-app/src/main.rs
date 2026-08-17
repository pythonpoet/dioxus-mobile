//! Four-page test app for the dioxus-mobile workspace crates:
//! dioxus-fcm, dioxus-jwt, dioxus-recorder.

#![allow(non_snake_case)]

use dioxus::prelude::*;

mod views;
use views::{Fcm, Init, Jwt, Recorder};

/// Server URL used by native/mobile clients for server-function calls.
/// On a device, replace with the host's LAN IP, e.g. `http://192.168.1.10:8080`.
#[cfg(not(feature = "server"))]
const SERVER_URL: &str = "http://127.0.0.1:8080";

/// HS256 secret shared by the login server function and the auth-check decoder.
/// Override at runtime with the `JWT_SECRET` environment variable.
#[cfg(feature = "server")]
const JWT_SECRET: &str = "dev-only-secret-change-me";

/// Claims carried by our JWTs.
#[cfg(feature = "server")]
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct AuthClaims {
    sub: String,
    exp: u64,
}

/// Axum state exposing the JWT decoder to the auth-check route.
#[cfg(feature = "server")]
#[derive(Clone)]
struct AuthState {
    decoder: dioxus_jwt::Decoder<AuthClaims>,
}

/// Manual impl instead of `#[derive(FromRef)]`: the derive expands to
/// `::axum::extract::FromRef`, which needs `axum` as a direct extern crate.
/// We route through dioxus's re-export instead.
#[cfg(feature = "server")]
impl dioxus::server::axum::extract::FromRef<AuthState> for dioxus_jwt::Decoder<AuthClaims> {
    fn from_ref(state: &AuthState) -> Self {
        state.decoder.clone()
    }
}

/// Bare-bones login server function: fixed credentials, no database.
#[post("/api/login")]
pub async fn login(username: String, password: String) -> ServerFnResult<String> {
    if username == "admin" && password == "password" {
        let secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| JWT_SECRET.to_string());
        let claims = AuthClaims {
            sub: username,
            exp: now_unix() + 86_400, // 24h
        };
        dioxus_jwt::issue_hs256(&secret, &claims)
            .ok_or_else(|| ServerFnError::new("failed to sign token"))
    } else {
        Err(ServerFnError::new("invalid credentials"))
    }
}

/// Server-side check: validate the bearer token against the decoder.
#[cfg(feature = "server")]
async fn auth_check(claims: dioxus_jwt::Claims<AuthClaims>) -> String {
    format!("authenticated as {}", claims.claims.sub)
}

/// Current unix time in seconds, matching the JWT `exp` claim format.
#[cfg(feature = "server")]
fn now_unix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Clone, Routable, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[rustfmt::skip]
enum Route {
    #[layout(Layout)]
        #[route("/")]
        Init {},

        #[route("/fcm")]
        Fcm {},

        #[route("/jwt")]
        Jwt {},

        #[route("/recorder")]
        Recorder {},
}

#[component]
fn Layout() -> Element {
    rsx! {
        nav {
            style: "display: flex; gap: 8px; flex-wrap: wrap; padding: 12px 16px; background: #0f172a; border-bottom: 1px solid rgba(255,255,255,0.08);",
            Link { to: Route::Init {}, style: "color: #93c5fd; text-decoration: none; padding: 6px 10px; border-radius: 8px;", "Init" }
            Link { to: Route::Fcm {}, style: "color: #93c5fd; text-decoration: none; padding: 6px 10px; border-radius: 8px;", "FCM" }
            Link { to: Route::Jwt {}, style: "color: #93c5fd; text-decoration: none; padding: 6px 10px; border-radius: 8px;", "JWT" }
            Link { to: Route::Recorder {}, style: "color: #93c5fd; text-decoration: none; padding: 6px 10px; border-radius: 8px;", "Recorder" }
        }
        Outlet::<Route> {}
    }
}

fn App() -> Element {
    let _auth = dioxus_jwt::init();
    rsx! { Router::<Route> {} }
}

fn main() {
    #[cfg(not(feature = "server"))]
    dioxus::fullstack::set_server_url(SERVER_URL);

    #[cfg(not(feature = "server"))]
    dioxus::launch(App);

    #[cfg(feature = "server")]
    dioxus::serve(|| async move {
        use dioxus::server::axum::routing::get;
        use dioxus::server::axum::Router;

        let secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| JWT_SECRET.to_string());
        let decoder = dioxus_jwt::hs256_decoder::<AuthClaims>(&secret, Some(&["test-app"]));
        let auth_state = AuthState { decoder };

        let auth_router = Router::new()
            .route("/api/check", get(auth_check))
            .with_state(auth_state);

        Ok(dioxus::server::router(App).merge(auth_router))
    });
}
