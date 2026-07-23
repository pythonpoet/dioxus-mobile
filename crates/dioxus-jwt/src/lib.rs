mod error;
pub use error::AuthError;

#[cfg(feature = "client")]
mod client;
#[cfg(feature = "client")]
pub use client::{provide_jwt, provide_jwt_with, use_jwt, JwtAuth, RequireAuth};

#[cfg(feature = "server")]
mod server;
#[cfg(feature = "server")]
pub use server::{AuthClaims, JwtConfig, JwtLayer};

use serde::de::DeserializeOwned;

/// Decode claims *without* verifying the signature.
///
/// The client uses this to read `sub`/`exp` for UX. Verification always
/// happens server-side — a client can never be its own trust anchor.
pub fn decode_claims_unverified<C: DeserializeOwned>(token: &str) -> Result<C, AuthError> {
    // jsonwebtoken 9.x (in 8.x this was `dangerous_insecure_decode`,
    // in 10.x `dangerous::insecure_decode` again — check your version)
    Ok(jsonwebtoken::dangerous::insecure_decode::<C>(token)?.claims)
}
