use std::{
    sync::Arc,
    task::{Context, Poll},
};

use axum::extract::FromRequestParts;
use http::{header::AUTHORIZATION, request::Parts, Request, StatusCode};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{de::DeserializeOwned, Serialize};
use tower_layer::Layer;
use tower_service::Service;

use crate::AuthError;

/// Keys + validation rules. Cheap to clone.
#[derive(Clone)]
pub struct JwtConfig {
    encoding_key: Arc<EncodingKey>,
    decoding_key: Arc<DecodingKey>,
    validation: Arc<Validation>,
    header: Arc<Header>,
}

impl JwtConfig {
    /// HMAC-SHA256 symmetric secret — the common case.
    pub fn hs256(secret: impl AsRef<[u8]>) -> Self {
        let secret = secret.as_ref();
        Self {
            encoding_key: Arc::new(EncodingKey::from_secret(secret)),
            decoding_key: Arc::new(DecodingKey::from_secret(secret)),
            validation: Arc::new(Validation::new(Algorithm::HS256)),
            header: Arc::new(Header::default()),
        }
    }

    pub fn with_validation(mut self, validation: Validation) -> Self {
        self.validation = Arc::new(validation);
        self
    }

    /// Sign a fresh token — call this from your login handler.
    pub fn issue<C: Serialize>(&self, claims: &C) -> Result<String, AuthError> {
        encode(&self.header, claims, &self.encoding_key).map_err(AuthError::Issue)
    }

    /// Verify signature + registered claims (`exp`, `nbf`, … per `validation`).
    pub fn validate<C: DeserializeOwned>(&self, token: &str) -> Result<C, AuthError> {
        Ok(decode::<C>(token, &self.decoding_key, &self.validation)?.claims)
    }
}

/// Tower layer that installs [`JwtConfig`] into request extensions —
/// the same role `SessionLayer` plays for axum-session.
#[derive(Clone)]
pub struct JwtLayer {
    config: JwtConfig,
}

impl JwtLayer {
    pub fn new(config: JwtConfig) -> Self {
        Self { config }
    }
}

impl<S> Layer<S> for JwtLayer {
    type Service = JwtService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        JwtService {
            inner,
            config: self.config.clone(),
        }
    }
}

#[derive(Clone)]
pub struct JwtService<S> {
    inner: S,
    config: JwtConfig,
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for JwtService<S>
where
    S: Service<Request<ReqBody>, Response = axum::response::Response<ResBody>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        req.extensions_mut().insert(self.config.clone());
        self.inner.call(req)
    }
}

/// Extractor: validates the bearer token and hands you the claims.
/// Rejects with 401 on missing/invalid/expired tokens.
pub struct AuthClaims<C>(pub C);

#[axum::async_trait]
impl<S, C> FromRequestParts<S> for AuthClaims<C>
where
    S: Send + Sync,
    C: DeserializeOwned + Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let config = parts
            .extensions
            .get::<JwtConfig>()
            .ok_or(AuthError::MissingConfig)?;

        let value = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or(AuthError::MissingToken)?;

        let (scheme, token) = value.split_once(' ').ok_or(AuthError::MissingToken)?;
        if !scheme.eq_ignore_ascii_case("bearer") {
            return Err(AuthError::MissingToken);
        }

        Ok(AuthClaims(config.validate(token)?))
    }
}
