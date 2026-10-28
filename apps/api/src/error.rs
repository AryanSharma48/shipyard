use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub enum ApiError {
    BadRequest(String),
    Unauthorized(String),
    NotFound(String),
    Conflict(String),
    Internal(String),
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiError::BadRequest(msg) => write!(f, "Bad request: {msg}"),
            ApiError::Unauthorized(msg) => write!(f, "Unauthorized: {msg}"),
            ApiError::NotFound(msg) => write!(f, "Not found: {msg}"),
            ApiError::Conflict(msg) => write!(f, "Conflict: {msg}"),
            ApiError::Internal(msg) => write!(f, "Internal error: {msg}"),
        }
    }
}

impl std::error::Error for ApiError {}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            ApiError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg),
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            ApiError::Conflict(msg) => (StatusCode::CONFLICT, msg),
            ApiError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        };

        let body = Json(ErrorResponse { error: message });
        (status, body).into_response()
    }
}
