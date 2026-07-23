# dioxus-jwt

Storage-backed JWT auth for [Dioxus](https://dioxuslabs.com) — roughly what
[axum-session](https://docs.rs/axum-session) is for cookies, but for tokens,
with an [axum](https://docs.rs/axum) server integration in the spirit of
[axum-jwt](https://docs.rs/axum-jwt).

- **Client:** the token is persisted via
  [dioxus-sdk-storage](https://docs.rs/dioxus-sdk-storage) behind a signal,
  exposed through a `Copy` context handle with login/logout/guard helpers.
- **Server:** a tower layer plus an axum extractor that validate
  `Authorization: Bearer …` and hand you typed claims.
- **Fullstack:** the two halves share one `JwtConfig`, so your
  `dioxus-fullstack` server functions issue tokens your components consume.

## How it maps

| axum-session | dioxus-jwt |
|---|---|
| `SessionLayer` | `JwtLayer` (installs `JwtConfig` into request extensions) |
| `AxumSession` extractor | `AuthClaims<C>` extractor (validates the bearer token) |
| `SessionConfig` / session store | `JwtConfig` (keys + validation) / `dioxus-sdk-storage` |
| axum-jwt's `JsonWebToken<C>` | same wire format; `AuthClaims<C>` is the analogue |

`AuthClaims` is wire-compatible with axum-jwt: same `Bearer` scheme, same
`jsonwebtoken` underneath.

## Install

```toml
[dependencies]
dioxus-jwt = { version = "0.1" }                 # client only (default)
# dioxus-jwt = { version = "0.1", features = ["fullstack"] }
```

| Feature | Enables | Pulls in | Default |
|---|---|---|---|
| `client` | `JwtAuth`, `provide_jwt`, `use_jwt`, `RequireAuth` | `dioxus`, `dioxus-sdk-storage`, `web-time` | ✔ |
| `server` | `JwtConfig`, `JwtLayer`, `AuthClaims` | `axum`, `http`, `tower-*` | |
| `fullstack` | both of the above | + `dioxus-fullstack` | |

## Version compatibility

| dioxus-jwt | dioxus | dioxus-fullstack | dioxus-sdk-storage | axum | jsonwebtoken |
|---|---|---|---|---|---|
| 0.1 | 0.8 | 0.8 | 0.8 | 0.8 | 9 |

> Keep the axum/http versions aligned with what `dioxus-fullstack` uses
> internally — check with `cargo tree -i axum` if you see trait-mismatch
> errors. On sdk versions before 0.7, `use_persistent` lives at
> `dioxus_sdk_storage::storage::use_persistent` instead of the crate root.

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

## Server quickstart (standalone axum)

```rust
use axum::{extract::Extension, routing::{get, post}, Router};
use dioxus_jwt::{AuthClaims, AuthError, JwtConfig, JwtLayer};

#[derive(serde::Serialize, serde::Deserialize)]
struct Claims {
    sub: String,
    exp: u64,
}

async fn login(Extension(config): Extension<JwtConfig>) -> Result<String, AuthError> {
    // verify credentials against your DB first …
    let exp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap()
        .as_secs() + 3600;
    config.issue(&Claims { sub: "alice".into(), exp })
}

async fn me(AuthClaims(claims): AuthClaims<Claims>) -> String {
    format!("hello {}", claims.sub)
}

#[tokio::main]
async fn main() {
    let config = JwtConfig::hs256(std::env::var("JWT_SECRET").expect("JWT_SECRET must be set"));
    let app = Router::new()
        .route("/login", post(login))
        .route("/me", get(me))
        .layer(JwtLayer::new(config));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

`AuthClaims<C>` rejects with `401` on a missing, malformed, expired, or
badly-signed token. `JwtLayer` is a plain tower layer, so it composes with
the rest of your middleware stack.

## Dioxus fullstack

Server functions are plain axum-routable endpoints, so the pragmatic pattern
is: the login function *returns* the token, and protected functions *take*
it as an argument (the default server-fn client doesn't attach
`Authorization` headers).

```rust
use dioxus_fullstack::prelude::*;
use dioxus_jwt::JwtConfig;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: u64,
}

fn config() -> JwtConfig {
    JwtConfig::hs256(std::env::var("JWT_SECRET").expect("JWT_SECRET must be set"))
}

#[post("/api/login")]
pub async fn login(username: String, password: String) -> Result<String, ServerFnError> {
    // verify credentials against your DB …
    let exp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap()
        .as_secs() + 3600;
    config()
        .issue(&Claims { sub: username, exp })
        .map_err(|e| ServerFnError::new(e.to_string()))
}

#[post("/api/me")]
pub async fn me(token: String) -> Result<String, ServerFnError> {
    let claims = config()
        .validate::<Claims>(&token)
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    Ok(format!("hello {}", claims.sub))
}
```

```rust
// client side
let mut auth = use_jwt();
auth.login(login(user, pass).await?);                       // persist
let greeting = me(auth.token().unwrap()).await?;            // use
```

Because `#[post("/api/...")]` maps to real paths, you can mount
`JwtLayer` wherever `dioxus-fullstack` exposes the axum `Router`, and
non-Dioxus clients (mobile apps, curl) can call the same endpoints with a
bearer header.

## Security notes

- **LocalStorage is XSS-readable.** Any JS running on your page can read
  the token. That's the accepted tradeoff of storage-based auth; keep your
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
