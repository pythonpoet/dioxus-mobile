//! Server-side JWT helpers, generic over the claims type.
//!
//! All cryptographic work is delegated to [`axum_jwt_auth`] (verification /
//! extraction) and [`jsonwebtoken`] (signing). This module only adds thin,
//! generic glue so applications don't need to depend on either crate directly:
//!
//! - [`issue_hs256`] signs a token from any `Serialize` claims.
//! - [`hs256_decoder`] builds an `axum_jwt_auth::Decoder<T>` from a secret and
//!   (optionally) an audience.
//!
//! Claim *values* remain application-defined: pass your own claims struct.

use std::sync::Arc;

use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{de::DeserializeOwned, Serialize};

/// Issue an HS256 JWT.
///
/// # Errors
///
/// Returns `None` if encoding fails (e.g. an un-serialisable claim).
pub fn issue_hs256<C: Serialize>(secret: &str, claims: &C) -> Option<String> {
    let key = EncodingKey::from_secret(secret.as_bytes());
    let header = Header::new(Algorithm::HS256);
    jsonwebtoken::encode(&header, claims, &key).ok()
}

/// Build an HS256 [`axum_jwt_auth::Decoder`] validating the given audience.
///
/// `audience` is optional; pass `Some(&["taalbubbl"])` to enforce the `aud`
/// claim. The returned `Decoder<T>` is `Arc`-backed and `Clone`, ready to store
/// on axum router state so the `Claims<T>` extractor (re-exported by this crate)
/// resolves it via `FromRef`.
pub fn hs256_decoder<C: DeserializeOwned>(
    secret: &str,
    audience: Option<&[&str]>,
) -> axum_jwt_auth::Decoder<C> {
    let mut validation = Validation::new(Algorithm::HS256);
    if let Some(aud) = audience {
        validation.set_audience(aud);
    }

    let decoder = axum_jwt_auth::LocalDecoder::builder()
        .keys(vec![DecodingKey::from_secret(secret.as_bytes())])
        .validation(validation)
        .build()
        .expect("invalid JWT decoding key");

    Arc::new(decoder)
}
