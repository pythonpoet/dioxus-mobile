#[cfg(feature = "client")]
mod client;
#[cfg(feature = "client")]
pub use client::{provide_jwt,try_use_jwt_diagnostics, try_use_jwt, provide_jwt_with, use_jwt, use_auth_headers, set_bearer_header, init, JwtAuth, RequireAuth};

#[cfg(feature = "client")]
// Re-export the native storage initialization macro so applications using
// dioxus-jwt do not need a separate dioxus-sdk-storage dependency.
pub use dioxus_sdk_storage::set_dir;

// The `server` feature offloads all validation/authentication logic to
// `axum-jwt-auth`. dioxus-jwt simply re-exports its API and adds generic
// HS256 issue/verify helpers in [`server`].
#[cfg(feature = "server")]
pub use axum_jwt_auth::{
    AuthError, BearerTokenExtractor, Claims, CookieTokenExtractor, Decoder, ExtractorConfig,
    HeaderTokenExtractor, LocalDecoder, RemoteJwksDecoder, RemoteJwksDecoderBuilder,
    RemoteJwksDecoderConfig, RemoteJwksDecoderConfigBuilder, TokenExtractor,
    define_cookie_extractor, define_header_extractor,
};
#[cfg(feature = "server")]
pub mod server;
#[cfg(feature = "server")]
pub use server::{hs256_decoder, issue_hs256};