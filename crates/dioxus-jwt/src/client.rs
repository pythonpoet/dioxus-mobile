//! Client-side JWT state for Dioxus apps.
//!
//! The token is persisted through [`dioxus-sdk-storage`] (LocalStorage on
//! web, the platform equivalent on native) behind a signal, so every
//! component that reads it re-renders automatically on login and logout.
//!
//! The claims type is erased at the context level and decoded on demand, so
//! [`JwtAuth`] is a single context type — guards and hooks compose no matter
//! what your claims look like.

use dioxus::prelude::*;
use dioxus_sdk_storage::use_persistent;
use serde::de::DeserializeOwned;

/// Default key under which the token is persisted.
pub const DEFAULT_STORAGE_KEY: &str = "dioxus-jwt:token";

/// Handle to the stored JWT.
///
/// `Copy` — capture it in event handlers and async blocks freely. Reads are
/// signal-backed: components calling [`token`](Self::token) or
/// [`is_authenticated`](Self::is_authenticated) re-render whenever the
/// stored token changes.
#[derive(Clone, Copy)]
pub struct JwtAuth {
    stored: Signal<Option<String>>,
}

// Never leak the token into logs.
impl std::fmt::Debug for JwtAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JwtAuth")
            .field("authenticated", &self.is_authenticated())
            .field("token", &self.token().as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

/// Create the auth state and provide it via context.
///
/// Call once near the app root; retrieve the handle in any descendant with
/// [`use_jwt`].
pub fn provide_jwt() -> JwtAuth {
    provide_jwt_with(DEFAULT_STORAGE_KEY)
}

/// Like [`provide_jwt`], but with a custom storage key — useful when one app
/// talks to several backends and keeps a token per backend.
pub fn provide_jwt_with(storage_key: &str) -> JwtAuth {
    let stored = use_persistent(storage_key, || Option::<String>::None);
    use_context_provider(|| JwtAuth { stored })
}

/// Consume the [`JwtAuth`] provided by an ancestor component.
///
/// # Panics
///
/// Panics if no ancestor called [`provide_jwt`].
pub fn use_jwt() -> JwtAuth {
    try_use_context::<JwtAuth>()
        .expect("no JwtAuth in context — call provide_jwt() in a parent component")
}

impl JwtAuth {
    /// The raw token, if one is stored.
    pub fn token(&self) -> Option<String> {
        self.stored.cloned()
    }

    /// `Bearer <token>`, ready for an `Authorization` header.
    pub fn bearer(&self) -> Option<String> {
        self.token().map(|t| format!("Bearer {t}"))
    }

    /// Persist a new token. Call this after a successful login.
    pub fn login(&mut self, token: impl Into<String>) {
        *self.stored.write() = Some(token.into());
    }

    /// Drop the token from state *and* storage.
    pub fn logout(&mut self) {
        *self.stored.write() = None;
    }

    /// `true` if a token exists and is not expired.
    ///
    /// **UX only.** "Not expired" means the token decodes and either has no
    /// `exp` claim or its `exp` is in the future *according to the client
    /// clock* — checked without verifying the signature. The server
    /// re-validates everything; a client can never be its own trust anchor.
    pub fn is_authenticated(&self) -> bool {
        self.token().is_some_and(|t| !is_expired(&t))
    }

    /// Decode the claims *without* verifying the signature.
    ///
    /// Returns `None` when no token is stored or decoding fails. Same
    /// trust caveat as [`is_authenticated`](Self::is_authenticated).
    pub fn claims<C: DeserializeOwned>(&self) -> Option<C> {
        self.token()
            .and_then(|t| crate::decode_claims_unverified(&t).ok())
    }

    /// The `exp` claim as seconds since the Unix epoch, if the token has
    /// one. Handy for "session expires in …" UI.
    pub fn expires_at(&self) -> Option<u64> {
        self.token()
            .and_then(|t| crate::decode_claims_unverified::<ExpProbe>(&t).ok())
            .and_then(|probe| probe.exp)
    }
}

/// Minimal projection used to peek at `exp` without knowing the concrete
/// claims type.
#[derive(serde::Deserialize)]
struct ExpProbe {
    exp: Option<u64>,
}

fn is_expired(token: &str) -> bool {
    let Ok(probe) = crate::decode_claims_unverified::<ExpProbe>(token) else {
        return true; // unparseable → treat as expired
    };
    match probe.exp {
        Some(exp) => now_unix() >= exp,
        None => false,
    }
}

fn now_unix() -> u64 {
    // web-time is a std-compatible shim that also works on wasm.
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

/// Renders `children` only when [`JwtAuth::is_authenticated`].
///
/// ```rust,ignore
/// rsx! {
///     RequireAuth {
///         fallback: rsx! { Login {} },
///         Dashboard {}
///     }
/// }
/// ```
#[component]
pub fn RequireAuth(children: Element, fallback: Option<Element>) -> Element {
    let auth = use_jwt();
    if auth.is_authenticated() {
        children
    } else if let Some(fallback) = fallback {
        fallback
    } else {
        rsx! {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};

    #[derive(serde::Serialize)]
    struct TestClaims {
        #[serde(skip_serializing_if = "Option::is_none")]
        exp: Option<u64>,
    }

    fn token_with_exp(exp: Option<u64>) -> String {
        encode(
            &Header::default(),
            &TestClaims { exp },
            &EncodingKey::from_secret(b"test"),
        )
        .unwrap()
    }

    #[test]
    fn future_exp_is_not_expired() {
        assert!(!is_expired(&token_with_exp(Some(now_unix() + 3600))));
    }

    #[test]
    fn past_exp_is_expired() {
        assert!(is_expired(&token_with_exp(Some(now_unix().saturating_sub(1)))));
    }

    #[test]
    fn missing_exp_is_not_expired() {
        assert!(!is_expired(&token_with_exp(None)));
    }

    #[test]
    fn garbage_is_expired() {
        assert!(is_expired("not-a-jwt"));
    }
}
