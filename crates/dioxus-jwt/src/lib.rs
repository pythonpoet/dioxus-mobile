#[cfg(feature = "client")]
mod client;
#[cfg(feature = "client")]
pub use client::{provide_jwt,try_use_jwt_diagnostics, try_use_jwt, provide_jwt_with, use_jwt, JwtAuth, RequireAuth};

#[cfg(feature = "client")]
// Re-export the native storage initialization macro so applications using
// dioxus-jwt do not need a separate dioxus-sdk-storage dependency.
pub use dioxus_sdk_storage::set_dir;

// The `server` feature is a thin wrapper over `axum-jwt-auth`: all key
// management, token validation, and axum extraction / error mapping lives in
// that crate. We re-export its public surface so applications can depend on a
// single `dioxus-jwt` crate for both the client and the server.
#[cfg(feature = "server")]
pub use axum_jwt_auth::{
    AuthError, BearerTokenExtractor, Claims, CookieTokenExtractor, Decoder, ExtractorConfig,
    HeaderTokenExtractor, LocalDecoder, RemoteJwksDecoder, RemoteJwksDecoderBuilder,
    RemoteJwksDecoderConfig, RemoteJwksDecoderConfigBuilder, TokenExtractor,
    define_cookie_extractor, define_header_extractor,
};