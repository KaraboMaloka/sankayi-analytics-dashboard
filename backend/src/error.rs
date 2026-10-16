//! API error type. Maps internal failures onto the shared error envelope
//! documented in `docs/API_CONTRACTS.md`.

use actix_web::{http::StatusCode, HttpResponse, ResponseError};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("database error")]
    Db(#[from] sqlx::Error),

    #[error("internal error: {0}")]
    Internal(String),
}

impl AppError {
    /// Wrap any error as an opaque internal (500) failure.
    pub fn internal(e: impl std::fmt::Display) -> Self {
        AppError::Internal(e.to_string())
    }

    fn code(&self) -> &'static str {
        match self {
            AppError::NotFound(_) => "not_found",
            AppError::BadRequest(_) => "bad_request",
            AppError::Db(_) | AppError::Internal(_) => "internal",
        }
    }

    fn public_message(&self) -> String {
        match self {
            AppError::NotFound(m) => m.clone(),
            AppError::BadRequest(m) => m.clone(),
            // Never leak internal detail to the client.
            AppError::Db(_) | AppError::Internal(_) => "An internal error occurred".to_string(),
        }
    }
}

impl ResponseError for AppError {
    fn status_code(&self) -> StatusCode {
        match self {
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::Db(_) | AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        // Log the full internal error server-side; return the sanitised envelope.
        match self {
            AppError::Db(e) => log::error!("database error: {e}"),
            AppError::Internal(e) => log::error!("internal error: {e}"),
            _ => {}
        }
        HttpResponse::build(self.status_code()).json(json!({
            "error": {
                "code": self.code(),
                "message": self.public_message(),
            }
        }))
    }
}
