# dioxus-jwt

Storage-backed JWT auth for [Dioxus](https://dioxuslabs.com).

- **Client:** the token is persisted via
  [dioxus-sdk-storage](https://docs.rs/dioxus-sdk-storage) behind a signal,
  exposed through a `Copy` context handle with login/logout/guard helpers.
- **Server:** a thin re-export of
  [axum-jwt-auth](https://docs.rs/axum-jwt-auth). All key management, token
  validation, and `axum` extraction / error mapping live in that crate; this
  crate just re-exports its public surface so you can depend on a single
  `dioxus-jwt` for both the client and the server.

## How it maps

| axum-jwt-auth | dioxus-jwt |
|---|---|
| `LocalDecoder` (keys + validation) | re-exported as `LocalDecoder` |
| `Decoder<T>` (Arc trait-object, `FromRef`) | re-exported as `Decoder` |
| `Claims<T, E>` extractor (validates a bearer token) | re-exported as `Claims` |
| `BearerTokenExtractor` / header / cookie extractors | re-exported |
| `AuthError` (→ HTTP 401/500) | re-exported as `AuthError` |
| `RemoteJwksDecoder` (JWKS + auto-refresh) | re-exported |

The `server` feature is a pure `pub use axum_jwt_auth::…` — no wrapper types,
no tower layer. Use the extractor `Claims<T>` and the `Decoder<T>: FromRef`
pattern exactly as you would with `axum-jwt-auth` directly.

## Install

```toml
[dependencies]
dioxus-jwt = { version = "0.1" }                 # client only (default)
# dioxus-jwt = { version = "0.1", features = ["server"] }   # re-exports axum-jwt-auth
```

| Feature | Enables | Pulls in | Default |
|---|---|---|---|
| `client` | `JwtAuth`, `provide_jwt`, `use_jwt`, `RequireAuth` | `dioxus`, `dioxus-sdk-storage`, `web-time` | ✔ |
| `server` | `Decoder`, `LocalDecoder`, `Claims`, `AuthError`, extractors | `axum-jwt-auth` | |

## Version compatibility

| dioxus-jwt | dioxus | dioxus-jwt-store | axum | axum-jwt-auth | jsonwebtoken |
|---|---|---|---|---|---|
| 0.1 | 0.8 | 0.8 | 0.8 | 0.7 | 10 |

> Keep the `axum` version aligned with what `dioxus-fullstack` uses internally
> (check with `cargo tree -i axum` if you see trait-mismatch errors).
> `axum-jwt-auth` 0.7 requires `axum` 0.8 and `jsonwebtoken` 10 — the same
> versions dioxus-jwt targets. On sdk versions before 0.7,
> `use_persistent` lives at `dioxus_sdk_storage::storage::use_persistent`
> instead of the crate root.

## Client quickstart

```rust
use dioxus::prelude::*;
use dioxus_jwt::{provide_jwt, use_jwt, RequireAuth};

#[derive(serde::Deserialize, Clone)]
struct Claims {
    sub: String,
    exp: u64,
}

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    provide_jwt(); // token persists under the key "dioxus-jwt:token"

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
                // `login` here is your server function / API call
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

## Server quickstart (axum-jwt-auth)

With the `server` feature, `dioxus_jwt` is just `axum_jwt_auth`, so enable
the extractor with router state.

```rust
use std::sync::Arc;
use axum::{extract::FromRef, routing::get, Json, Router};
use dioxus_jwt::{Claims, Decoder, LocalDecoder};
use jsonwebtoken::{DecodingKey, Algorithm, Validation};
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
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_audience(&["my-app"]);

    let decoder = LocalDecoder::builder()
        .keys(vec![jsonwebtoken::DecodingKey::from_secret(secret.as_bytes())])
        .validation(validation)
        .build()
        .unwrap();

    let state = AppState { decoder: Arc::new(decoder) };

    let app = Router::new()
        .route("/me", get(me))
        .with_state(state);
}
```

`Claims<C>` rejects with `401` on a missing, malformed, expired, or
badly-signed token. Extractors for custom headers or cookies come from
`define_header_extractor!` / `define_cookie_extractor!` (re-exported), and
`RemoteJwksDecoder` handles JWKS with automatic caching and refresh.

Issuing tokens (signing fresh claims with your `EncodingKey` after verifying
credentials) is the caller's job on the server; this crate's client half only
stores and presents whatever token the server hands back.

## Dioxus fullstack

Server functions are plain axum-routable endpoints, so the pragmatic pattern
is: the login function *returns* the token, and protected functions *take*
it as an argument (the default server-fn client doesn't attach
`Authorization` headers).

```rust
use dioxus_fullstack::prelude::*;
use jsonwebtoken::{encode, EncodingKey, Header};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: usize,
}

fn encoding_key() -> EncodingKey {
    EncodingKey::from_secret(std::env::var("JWT_SECRET").unwrap().as_bytes())
}

#[post("/api/login")]
pub async fn login(username: String, password: String) -> Result<String, ServerFnError> {
    // verify credentials against your DB …
    let exp = /* now + 3600 */;
    Ok(encode(&Header::default(), &Claims { sub: username, exp }, &encoding_key())
        .map_err(|e| ServerFnError::new(e.to_string()))?)
}

#[post("/api/me")]
pub async fn me(token: String) -> Result<String, ServerFnError> {
    // verify `token` with a `LocalDecoder` (or mount the `Claims` extractor)
    // then respond …
}
```

```rust
// client side
let mut auth = use_jwt();
auth.login(login(user, pass).await?);                       // persist
let greeting = me(auth.token().unwrap()).await?;            // use
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