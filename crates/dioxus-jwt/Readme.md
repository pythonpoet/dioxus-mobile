# dioxus-jwt

Storage-backed JWT auth for [Dioxus](https://dioxuslabs.com).

- **Client:** the token is persisted via
  [dioxus-sdk-storage](https://docs.rs/dioxus-sdk-storage) behind a signal,
  exposed through a `Copy` context handle with login/logout/guard helpers, and
  mirrored into `Authorization: Bearer` request headers so Dioxus server
  functions authenticate transparently.
- **Server:** delegating to
  [axum-jwt-auth](https://docs.rs/axum-jwt-auth) for all
  validation/extraction, plus two thin generic helpers (`issue_hs256`,
  `hs256_decoder`) so applications never need to import `jsonwebtoken` or
  `axum-jwt-auth` directly.

A single `init()` call bootstraps everything on the client.

## What dioxus-jwt provides

### Client

| Function | Purpose |
|---|---|
| `init()` | **One-shot bootstrap**: configure the storage directory (once), provide the `JwtAuth` context, and mirror the current token into request headers. Call from your root component. |
| `provide_jwt()` / `provide_jwt_with(key)` | Provide a persistent `JwtAuth` context (default storage key `dioxus-jwt:token`). |
| `use_jwt()` | Read/write the current `JwtAuth` context. |
| `use_auth_headers(&JwtAuth)` | `use_effect` that mirrors the token into `Authorization: Bearer` for every subsequent server-function call. Called internally by `init()`. |
| `RequireAuth` | Render a component only when authenticated, else a fallback. |
| `JwtAuth` | `Copy` handle: `token()`, `bearer()`, `login()`, `logout()`, `is_authenticated()`, `claims::<C>()`. |

### Server

| Function | Purpose |
|---|---|
| `issue_hs256<C: Serialize>(secret, &claims)` | Sign an HS256 JWT from generic claims. |
| `hs256_decoder<C>(secret, audience)` | Build an `Arc<Decoder<C>>` (HS256) ready for router state / the `Claims` extractor. |
| `Decoder`, `LocalDecoder`, `Claims`, `AuthError`, extractors | Re-exported verbatim from `axum-jwt-auth`. |

## Install

```toml
[dependencies]
dioxus-jwt = { git = "https://github.com/pythonpoet/dioxus-mobile", features = ["client", "server"] }
```

| Feature | Enables | Pulls in | Default |
|---|---|---|---|
| `client` | `JwtAuth`, `init`, `provide_jwt`, `use_jwt`, `use_auth_headers`, `RequireAuth` | `dioxus` (+ `fullstack`), `dioxus-sdk-storage`, `web-time`, `tracing` | ✔ |
| `server` | `issue_hs256`, `hs256_decoder`, `Decoder`, `LocalDecoder`, `Claims`, `AuthError`, extractors | `axum-jwt-auth`, `jsonwebtoken` | |

The `client` feature enables `dioxus/fullstack` because `use_auth_headers`
uses `dioxus_fullstack::set_request_headers`. If your application already
enables it, this is a no-op.

## Platform support

- **Web (wasm32)** — the `client` feature works on the web: the token
  persists to `localStorage` (via `dioxus-sdk-storage`'s wasm backend),
  `use_auth_headers` sets the `Authorization` header on server-function
  calls, and `set_dir!` is a no-op. The crate enables `getrandom/js` on
  wasm so jsonWebToken-based claim decoding compiles for
  `wasm32-unknown-unknown`.
- **Native / mobile (Android, desktop)** — the `client` feature persists via
  the file system under the NDK/data dir, which `set_dir!()` configures on
  first render.
- **The `server` feature is host-only.** It pulls in `axum`/`tokio` (the
  tower/IO stack), which does not compile for wasm. Do not enable `server`
  in a web build — use `features = ["client"]` there, and toggle `server`
  only for the native/back-end build.

## Client quickstart

```rust
use dioxus::prelude::*;
use dioxus_jwt::{init, use_jwt, RequireAuth};

#[derive(serde::Deserialize, Clone)]
struct Claims {
    sub: String,
    exp: u64,
}

fn main() {
    // Server functions (dioxus-fullstack):
    // dioxus::serve(|| async move { dioxus::server::router(App).layer(...) });
    // Client (web / desktop / mobile):
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    init(); // storage dir + provider + auth headers, all at once

    rsx! {
        RequireAuth {
            fallback: rsx! { Login {} },
            Dashboard {}
        }
    }
}

#[component]
fn Login() -> Element {
    let mut auth = use_jwt();
    let mut username = use_signal(String::new);
    let mut password = use_signal(String::new);

    rsx! {
        form {
            onsubmit: move |_| async move {
                // `login` here is a server function / API call
                if let Ok(token) = login(username(), password()).await {
                    auth.login(token); // persisted to storage automatically
                }
            },
            input { oninput: move |e| username.set(e.value()), placeholder: "user" }
            input { oninput: move |e| password.set(e.value()), r#type: "password" }
            button { "Log in" }
        }
    }
}

#[component]
fn Dashboard() -> Element {
    let mut auth = use_jwt();
    let name = auth.claims::<Claims>().map(|c| c.sub).unwrap_or_default();

    rsx! {
        p { "Hello {name}" }
        button { onclick: move |_| auth.logout(), "Log out" }
        button {
            onclick: move |_| async move {
                let res = reqwest::Client::new()
                    .get("https://api.example.com/me")
                    .header(reqwest::header::AUTHORIZATION, auth.bearer().unwrap())
                    .send()
                    .await;
                // …
            },
            "Call API"
        }
    }
}
```

`JwtAuth` is `Copy` and signal-backed: anything that called `token()` or
`is_authenticated()` re-renders the moment you `login()`/`logout()`.

## Server quickstart

`issue_hs256` / `hs256_decoder` keep the signing and validation setup generic:
pass your own claims type, and the encoder/decoder match on the same HS256
secret and audience.

```rust
use std::sync::Arc;
use axum::{extract::FromRef, routing::get, Json, Router};
use dioxus_jwt::{hs256_decoder, issue_hs256, Claims, Decoder};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct MyClaims {
    sub: String,
    exp: usize,
}

#[derive(Clone, FromRef)]
struct AppState {
    decoder: Decoder<MyClaims>,
}

async fn me(Claims { claims, .. }: Claims<MyClaims>) -> Json<MyClaims> {
    Json(claims)
}

#[tokio::main]
async fn main() {
    let secret = std::env::var("JWT_SECRET").expect("JWT_SECRET must be set");

    // Same secret + audience as the encoder, so tokens round-trip.
    let decoder = hs256_decoder::<MyClaims>(&secret, Some(&["my-app"]));
    let state = AppState { decoder };

    let app = Router::new().route("/me", get(me)).with_state(state);

    // issue:
    //   let token = issue_hs256(&secret, &MyClaims { sub, exp });
}
```

`Claims<C>` rejects with `401` on a missing, malformed, expired, or
badly-signed token. Extractors for custom headers or cookies come from
`define_header_extractor!` / `define_cookie_extractor!` (re-exported), and
`RemoteJwksDecoder` handles JWKS with automatic caching and refresh.

## Dioxus fullstack

`init()` makes server functions token-aware automatically: it mirrors the
current token into `Authorization: Bearer` headers via
`dioxus_fullstack::set_request_headers`, so protected server functions can
verify the caller from the header instead of taking the token as a serialized
argument.

On the server, gate your protected functions with a server-only argument
(`name: Type` after the route) whose extractor reads the bearer header and
validates it against your `Decoder`. With the `server` feature you can build
that extractor from the re-exported `axum-jwt-auth` machinery (or the generic
`hs256_decoder` helper):

```rust
use dioxus::prelude::*;

// A server-only argument: present in the handler but absent from the client
// stub. Custom extractors implement `FromRequestParts<FullstackContext>` and
// read the `Authorization: Bearer` header that `init()` set.
#[cfg(feature = "server")]
use axum::{http::request::Parts, extract::FromRequestParts};
#[cfg(feature = "server")]
use dioxus::prelude::dioxus_fullstack::FullstackContext;

// … define `Auth` / `from_request_parts` that decode the bearer token …

#[post("/api/me", auth: my_extractor::Auth)]
pub async fn me() -> Result<String, ServerFnError> {
    // `auth.claims` is in scope (server-only argument)
    // …
}
```

Because `#[post("/api/...")]` maps to real paths, you can mount
`axum-jwt-auth`'s extractor / state wherever `dioxus-fullstack` exposes the
axum `Router`, and non-Dioxus clients (mobile apps, curl) can call the same
endpoints with a bearer header.

## Security notes

- **LocalStorage is XSS-readable.** Any JS running on your page can read the
  token. That's the accepted tradeoff of storage-based auth; keep your
  dependencies audited and use a CSP.
- **`is_authenticated()` and `claims()` do not verify the signature.** They
  exist to drive UI. All trust decisions belong on the server, which
  re-validates on every request.
- **Keep the secret server-side and load it at runtime**
  (`std::env::var`, not `env!`, which bakes it into the binary).
- Use short `exp` times and serve everything over HTTPS.

## Roadmap

- Refresh-token flow (second storage key + a `refresh()` hook)
- `SessionStorage` backing via the sdk's `new_storage::<SessionStorage, _>`
- A `RequireAuth` variant that redirects with `use_navigator()` instead of
  rendering a fallback

## License

Licensed under either of [Apache-2.0](LICENSE-APACHE) or
[MIT](LICENSE-MIT) at your option.