use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

#[derive(Debug)]
pub enum AppError {
    BadRequest(String),
    Unauthorized(String),
    Forbidden(String),
    NotFound(String),
    Conflict(String),
    ServiceUnavailable(String),
    Internal(String),
}

impl AppError {
    fn status_and_message(&self) -> (StatusCode, &str) {
        match self {
            AppError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            AppError::Unauthorized(m) => (StatusCode::UNAUTHORIZED, m),
            AppError::Forbidden(m) => (StatusCode::FORBIDDEN, m),
            AppError::NotFound(m) => (StatusCode::NOT_FOUND, m),
            AppError::Conflict(m) => (StatusCode::CONFLICT, m),
            AppError::ServiceUnavailable(m) => (StatusCode::SERVICE_UNAVAILABLE, m),
            AppError::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, m),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = self.status_and_message();
        let body = Json(json!({ "error": message }));
        (status, body).into_response()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::RowNotFound => AppError::NotFound("resource not found".into()),
            sqlx::Error::Database(db_err) if db_err.is_unique_violation() => {
                AppError::Conflict("resource already exists".into())
            }
            _ => {
                tracing::error!("Database error: {:?}", e);
                AppError::Internal("internal server error".into())
            }
        }
    }
}

impl From<jsonwebtoken::errors::Error> for AppError {
    fn from(_: jsonwebtoken::errors::Error) -> Self {
        AppError::Unauthorized("invalid token".into())
    }
}

impl From<argon2::password_hash::Error> for AppError {
    fn from(e: argon2::password_hash::Error) -> Self {
        tracing::error!("Password hashing error: {:?}", e);
        AppError::Internal("internal server error".into())
    }
}
