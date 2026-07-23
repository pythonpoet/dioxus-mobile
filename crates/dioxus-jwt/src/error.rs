use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("no bearer token in the `Authorization` header")]
    MissingToken,
    #[error("invalid token: {0}")]
    InvalidToken(#[from] jsonwebtoken::errors::Error),
    #[error("`JwtConfig` missing from request extensions — did you add `JwtLayer`?")]
    MissingConfig,
    #[error("failed to issue token: {0}")]
    Issue(#[source] jsonwebtoken::errors::Error),
}

#[cfg(feature = "server")]
impl axum::response::IntoResponse for AuthError {
    fn into_response(self) -> axum::response::Response {
        use http::StatusCode;
        let status = match &self {
            AuthError::MissingToken | AuthError::InvalidToken(_) => StatusCode::UNAUTHORIZED,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, self.to_string()).into_response()
    }
}
