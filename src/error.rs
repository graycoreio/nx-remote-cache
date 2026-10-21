use axum::{
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};

/// Every non-2xx response the server produces. Bodies are `text/plain` because the Nx client
/// shows them to the user, and treats a 401 without `text/plain` as a misconfigured server.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{0}")]
    Unauthorized(String),
    #[error("{0}")]
    Forbidden(String),
    #[error("not found")]
    NotFound,
    #[error("cannot override an existing record")]
    Conflict,
    #[error("{0}")]
    BadRequest(String),
    #[error("Content-Length is required")]
    LengthRequired,
    #[error("artifact exceeds the server limit of {0} bytes")]
    PayloadTooLarge(u64),
    #[error("internal server error")]
    Internal,
    #[error("{0}")]
    Unavailable(String),
}

impl ApiError {
    fn status(&self) -> StatusCode {
        match self {
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Conflict => StatusCode::CONFLICT,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::LengthRequired => StatusCode::LENGTH_REQUIRED,
            Self::PayloadTooLarge(_) => StatusCode::PAYLOAD_TOO_LARGE,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = (
            self.status(),
            [(header::CONTENT_TYPE, "text/plain")],
            self.to_string(),
        )
            .into_response();
        if matches!(self, Self::Unauthorized(_)) {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, "Bearer".parse().unwrap());
        }
        response
    }
}
