//! Client-side JWT state for Dioxus applications.
//!
//! The token is normally persisted through `dioxus-sdk-storage` and exposed
//! through a Dioxus signal.
//!
//! Security notes:
//!
//! - JWT contents are never written to diagnostic logs.
//! - Client-side decoding does not verify the token signature.
//! - Authentication and authorization must always be validated by the server.

use std::fmt;

use dioxus::prelude::*;
use dioxus_sdk_storage::use_persistent;

use serde::de::DeserializeOwned;

/// Subscribe to the current token and mirror it into the global `Authorization`
/// request header. Call once, inside a component that has access to `auth`.
///
/// On login/logout the signal updates, the effect re-runs, and the header is
/// re-set (or cleared) for subsequent server-function calls.
pub fn use_auth_headers(auth: JwtAuth) {
    use_effect(move || {
        #[cfg(not(feature = "server"))]
        {
            let mut headers = dioxus::prelude::dioxus_fullstack::HeaderMap::new();
            if let Some(token) = auth.token() {
                if let Ok(value) = format!("Bearer {token}").parse() {
                    headers.insert("authorization", value);
                }
            }
            dioxus::prelude::dioxus_fullstack::set_request_headers(headers);
        }
    });
}

/// Decode claims *without* verifying the signature. The client uses this to
/// read `sub`/`exp` for UX. Verification always happens server-side — a
/// client can never be its own trust anchor.
fn decode_claims_unverified<C: DeserializeOwned>(
    token: &str,
) -> Result<C, jsonwebtoken::errors::Error> {
    Ok(jsonwebtoken::dangerous::insecure_decode::<C>(token)?.claims)
}


/// Default key under which the token is persisted.
pub const DEFAULT_STORAGE_KEY: &str = "dioxus-jwt:token";

/// Environment information useful when diagnosing storage initialization.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JwtEnvironment {
    pub os: &'static str,
    pub architecture: &'static str,
    pub android: bool,
    pub wasm: bool,
    pub debug_build: bool,
}

impl JwtEnvironment {
    pub fn current() -> Self {
        Self {
            os: std::env::consts::OS,
            architecture: std::env::consts::ARCH,
            android: cfg!(target_os = "android"),
            wasm: cfg!(target_arch = "wasm32"),
            debug_build: cfg!(debug_assertions),
        }
    }
}

impl fmt::Display for JwtEnvironment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "os={}, architecture={}, android={}, wasm={}, debug={}",
            self.os,
            self.architecture,
            self.android,
            self.wasm,
            self.debug_build,
        )
    }
}

/// Describes which storage implementation backs a [`JwtAuth`] handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JwtStorageKind {
    /// Storage supplied by `dioxus-sdk-storage`.
    Persistent,

    /// A signal that is not persisted.
    ///
    /// This is useful for diagnosing native storage initialization failures.
    Memory,
}

impl fmt::Display for JwtStorageKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Persistent => formatter.write_str("persistent"),
            Self::Memory => formatter.write_str("memory"),
        }
    }
}

/// Initialization information made available through Dioxus context.
#[derive(Clone, Debug)]
pub struct JwtDiagnostics {
    pub storage_key: String,
    pub storage_kind: JwtStorageKind,
    pub environment: JwtEnvironment,
}

impl JwtDiagnostics {
    fn new(storage_key: &str, storage_kind: JwtStorageKind) -> Self {
        Self {
            storage_key: storage_key.to_string(),
            storage_kind,
            environment: JwtEnvironment::current(),
        }
    }
}

/// Errors returned by the non-panicking JWT APIs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JwtError {
    /// No [`JwtAuth`] exists in the current component's context.
    MissingContext,

    /// The storage key was empty or otherwise invalid.
    InvalidStorageKey(String),

    /// No token is currently stored.
    MissingToken,

    /// The JWT could not be decoded.
    InvalidToken(String),
}

impl fmt::Display for JwtError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingContext => {
                formatter.write_str(
                    "no JwtAuth exists in context; call provide_jwt() \
                     in an ancestor component",
                )
            }

            Self::InvalidStorageKey(reason) => {
                write!(formatter, "invalid JWT storage key: {reason}")
            }

            Self::MissingToken => {
                formatter.write_str("no JWT token is currently stored")
            }

            Self::InvalidToken(reason) => {
                write!(formatter, "JWT could not be decoded: {reason}")
            }
        }
    }
}

impl std::error::Error for JwtError {}

/// Handle to the stored JWT.
///
/// This type is `Copy`, so it can be captured in Dioxus event handlers and
/// asynchronous tasks.
#[derive(Clone, Copy)]
pub struct JwtAuth {
    stored: Signal<Option<String>>,
    storage_kind: JwtStorageKind,
}

impl fmt::Debug for JwtAuth {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let token = self.token();

        formatter
            .debug_struct("JwtAuth")
            .field("storage_kind", &self.storage_kind)
            .field("has_token", &token.is_some())
            .field(
                "token_length",
                &token.as_ref().map(String::len),
            )
            .field("authenticated", &self.is_authenticated())
            .field("token", &token.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

/// Print a diagnostic checkpoint without printing the JWT.
///
/// `eprintln!` is used as well as `tracing` because early native startup
/// failures can happen before a tracing subscriber is completely configured.
fn diagnostic(message: impl fmt::Display) {
    eprintln!("[dioxus-jwt] {message}");

    #[cfg(feature = "tracing")]
    tracing::debug!(target: "dioxus_jwt", "{message}");
}

/// Print an error diagnostic without printing the JWT.
fn diagnostic_error(message: impl fmt::Display) {
    eprintln!("[dioxus-jwt] ERROR: {message}");

    #[cfg(feature = "tracing")]
    tracing::error!(target: "dioxus_jwt", "{message}");
}

fn validate_storage_key(storage_key: &str) -> Result<(), JwtError> {
    if storage_key.trim().is_empty() {
        return Err(JwtError::InvalidStorageKey(
            "the key must not be empty".to_string(),
        ));
    }

    if storage_key.contains('\0') {
        return Err(JwtError::InvalidStorageKey(
            "the key must not contain a NUL character".to_string(),
        ));
    }

    Ok(())
}

/// Create persistent JWT state and provide it through Dioxus context.
///
/// Call this exactly once in a component above every component that uses
/// [`use_jwt`] or [`try_use_jwt`].
///
/// # Important
///
/// `dioxus_sdk_storage::use_persistent` does not expose initialization as a
/// `Result`. If its platform backend panics, this function cannot convert that
/// panic into a `JwtError`. The diagnostic checkpoints identify whether the
/// panic occurred before, during, or after persistent storage initialization.
///
/// Do not wrap this hook in `catch_unwind`; unwinding across Dioxus hooks can
/// leave the hook state inconsistent.
pub fn provide_jwt() -> JwtAuth {
    provide_jwt_with(DEFAULT_STORAGE_KEY)
}

/// Create persistent JWT state using a custom storage key.
pub fn provide_jwt_with(storage_key: &str) -> JwtAuth {
    let key_for_log = storage_key.to_owned();

    use_hook(move || {
        eprintln!(
            "[dioxus-jwt] Initializing persistent JWT storage; key={key_for_log:?}"
        );
    });

    let stored = use_persistent(
        storage_key,
        || Option::<String>::None,
    );

    let auth = JwtAuth {
        stored,
        storage_kind: JwtStorageKind::Persistent,
    };

    let diagnostics = JwtDiagnostics::new(
        storage_key,
        JwtStorageKind::Persistent,
    );

    use_context_provider(|| diagnostics);
    use_context_provider(|| auth)
}
/// Create a non-persistent JWT context.
///
/// Use this temporarily to determine whether a panic comes from
/// `dioxus-sdk-storage`.
///
/// If this provider works while [`provide_jwt`] panics, the problem is in the
/// persistent storage backend rather than Dioxus context handling.
pub fn provide_jwt_in_memory() -> JwtAuth {
    provide_jwt_in_memory_with(DEFAULT_STORAGE_KEY)
}

/// Create a non-persistent JWT context with a diagnostic key.
pub fn provide_jwt_in_memory_with(storage_key: &str) -> JwtAuth {
    let environment = JwtEnvironment::current();

    diagnostic(format!(
        "provide_jwt_in_memory_with: entering; \
         key={storage_key:?}; {environment}"
    ));

    if let Err(error) = validate_storage_key(storage_key) {
        diagnostic_error(format!(
            "provide_jwt_in_memory_with: storage key validation failed: \
             {error}"
        ));

        panic!("{error}");
    }

    diagnostic(
        "provide_jwt_in_memory_with: creating in-memory signal",
    );

    let stored = use_signal(|| Option::<String>::None);

    diagnostic(
        "provide_jwt_in_memory_with: signal created successfully",
    );

    let auth = JwtAuth {
        stored,
        storage_kind: JwtStorageKind::Memory,
    };

    let diagnostics = JwtDiagnostics::new(
        storage_key,
        JwtStorageKind::Memory,
    );

    use_context_provider(|| diagnostics);

    let provided = use_context_provider(|| auth);

    diagnostic(
        "provide_jwt_in_memory_with: initialization completed",
    );

    provided
}

/// Attempt to consume the JWT context without panicking.
pub fn try_use_jwt() -> Result<JwtAuth, JwtError> {
    match try_use_context::<JwtAuth>() {
        Some(auth) => {
            diagnostic(format!(
                "try_use_jwt: context found; storage_kind={}",
                auth.storage_kind()
            ));

            Ok(auth)
        }

        None => {
            let environment = JwtEnvironment::current();

            diagnostic_error(format!(
                "try_use_jwt: context is missing; {environment}"
            ));

            Err(JwtError::MissingContext)
        }
    }
}

/// Consume the [`JwtAuth`] provided by an ancestor component.
///
/// Prefer [`try_use_jwt`] while diagnosing initialization errors.
///
/// # Panics
///
/// Panics if no ancestor called [`provide_jwt`] or
/// [`provide_jwt_in_memory`].
pub fn use_jwt() -> JwtAuth {
    try_use_jwt().unwrap_or_else(|error| {
        diagnostic_error(format!("use_jwt: {error}"));
        panic!("{error}");
    })
}

/// Retrieve JWT diagnostics from context.
///
/// This does not panic when diagnostics are unavailable.
pub fn try_use_jwt_diagnostics() -> Option<JwtDiagnostics> {
    try_use_context::<JwtDiagnostics>()
}

impl JwtAuth {
    /// Return the storage implementation backing this handle.
    pub fn storage_kind(&self) -> JwtStorageKind {
        self.storage_kind
    }

    /// Return the raw token, if one is stored.
    ///
    /// Do not write this value to logs.
    pub fn token(&self) -> Option<String> {
        self.stored.cloned()
    }

    /// Return whether a token is currently stored.
    pub fn has_token(&self) -> bool {
        self.stored.read().is_some()
    }

    /// Return token length without exposing the token itself.
    pub fn token_length(&self) -> Option<usize> {
        self.stored.read().as_ref().map(String::len)
    }

    /// Return `Bearer <token>` for an HTTP Authorization header.
    pub fn bearer(&self) -> Option<String> {
        self.token().map(|token| format!("Bearer {token}"))
    }

    /// Persist a new token.
    pub fn login(&mut self, token: impl Into<String>) {
        let token = token.into();
        let token_length = token.len();

        diagnostic(format!(
            "JwtAuth::login: storing token; \
             token_length={token_length}; \
             storage_kind={}",
            self.storage_kind
        ));

        *self.stored.write() = Some(token);

        diagnostic("JwtAuth::login: signal write completed");
    }

    /// Remove the token from state and persistent storage.
    pub fn logout(&mut self) {
        diagnostic(format!(
            "JwtAuth::logout: removing token; storage_kind={}",
            self.storage_kind
        ));

        *self.stored.write() = None;

        diagnostic("JwtAuth::logout: signal write completed");
    }

    /// Return `true` when a token exists and is not expired.
    ///
    /// This is only a client-side UX check. The signature is not verified.
    pub fn is_authenticated(&self) -> bool {
        self.token()
            .is_some_and(|token| !is_expired(&token))
    }

    /// Decode claims without verifying the signature.
    pub fn claims<C: DeserializeOwned>(&self) -> Option<C> {
        self.try_claims().ok()
    }

    /// Decode claims without verifying the signature and preserve the error.
    pub fn try_claims<C: DeserializeOwned>(&self) -> Result<C, JwtError> {
        let token = self.token().ok_or(JwtError::MissingToken)?;

        decode_claims_unverified::<C>(&token).map_err(|error| {
            JwtError::InvalidToken(error.to_string())
        })
    }

    /// Return the `exp` claim in Unix seconds, if present.
    pub fn expires_at(&self) -> Option<u64> {
        self.try_claims::<ExpProbe>()
            .ok()
            .and_then(|probe| probe.exp)
    }

    /// Return a safe diagnostic description that does not contain the token.
    pub fn safe_status(&self) -> JwtSafeStatus {
        let token = self.token();
        let token_length = token.as_ref().map(String::len);
        let expires_at = token
            .as_deref()
            .and_then(decode_expiration);

        JwtSafeStatus {
            storage_kind: self.storage_kind,
            has_token: token.is_some(),
            token_length,
            expires_at,
            authenticated: token
                .as_deref()
                .is_some_and(|value| !is_expired(value)),
        }
    }
}

/// Token status that is safe to display or write to logs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JwtSafeStatus {
    pub storage_kind: JwtStorageKind,
    pub has_token: bool,
    pub token_length: Option<usize>,
    pub expires_at: Option<u64>,
    pub authenticated: bool,
}

/// Minimal projection used to inspect the `exp` claim.
#[derive(serde::Deserialize)]
struct ExpProbe {
    exp: Option<u64>,
}

fn decode_expiration(token: &str) -> Option<u64> {
    decode_claims_unverified::<ExpProbe>(token)
        .ok()
        .and_then(|probe| probe.exp)
}

fn is_expired(token: &str) -> bool {
    let Ok(probe) =
        decode_claims_unverified::<ExpProbe>(token)
    else {
        diagnostic(
            "is_expired: token could not be decoded; \
             treating it as expired",
        );

        return true;
    };

    match probe.exp {
        Some(expiration) => {
            let now = now_unix();
            let expired = now >= expiration;

            diagnostic(format!(
                "is_expired: exp={expiration}; now={now}; \
                 expired={expired}"
            ));

            expired
        }

        None => {
            diagnostic(
                "is_expired: token has no exp claim; \
                 treating it as not expired",
            );

            false
        }
    }
}

fn now_unix() -> u64 {
    match web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
    {
        Ok(duration) => duration.as_secs(),

        Err(error) => {
            diagnostic_error(format!(
                "now_unix: system clock is before Unix epoch: {error}"
            ));

            0
        }
    }
}

/// Render children only when the user appears authenticated.
///
/// This component does not panic when the provider is missing. It renders the
/// fallback instead and reports the context error.
#[component]
pub fn RequireAuth(
    children: Element,
    fallback: Option<Element>,
) -> Element {
    match try_use_jwt() {
        Ok(auth) if auth.is_authenticated() => children,

        Ok(_) => fallback.unwrap_or_else(|| rsx! {}),

        Err(error) => {
            diagnostic_error(format!(
                "RequireAuth: JWT context unavailable: {error}"
            ));

            fallback.unwrap_or_else(|| rsx! {})
        }
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
        .expect("test token must encode")
    }

    #[test]
    fn future_exp_is_not_expired() {
        assert!(!is_expired(&token_with_exp(Some(
            now_unix() + 3600,
        ))));
    }

    #[test]
    fn past_exp_is_expired() {
        assert!(is_expired(&token_with_exp(Some(
            now_unix().saturating_sub(1),
        ))));
    }

    #[test]
    fn missing_exp_is_not_expired() {
        assert!(!is_expired(&token_with_exp(None)));
    }

    #[test]
    fn garbage_is_expired() {
        assert!(is_expired("not-a-jwt"));
    }

    #[test]
    fn empty_storage_key_is_invalid() {
        assert!(matches!(
            validate_storage_key(""),
            Err(JwtError::InvalidStorageKey(_))
        ));
    }

    #[test]
    fn whitespace_storage_key_is_invalid() {
        assert!(matches!(
            validate_storage_key("   "),
            Err(JwtError::InvalidStorageKey(_))
        ));
    }

    #[test]
    fn nul_storage_key_is_invalid() {
        assert!(matches!(
            validate_storage_key("jwt\0token"),
            Err(JwtError::InvalidStorageKey(_))
        ));
    }

    #[test]
    fn default_storage_key_is_valid() {
        assert!(validate_storage_key(DEFAULT_STORAGE_KEY).is_ok());
    }
}
