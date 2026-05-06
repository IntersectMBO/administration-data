//! Structured JSON error responses
//!
//! Every non-2xx response shares the shape:
//!
//! ```json
//! {
//!   "error": { "code": "<short_token>", "message": "<human readable>", "details": {...}? },
//!   "meta":  { "timestamp": "<ISO 8601>" }
//! }
//! ```
//!
//! Handlers return `Result<T, ApiError>`. `ApiError`'s `IntoResponse` impl
//! converts each variant into the appropriate HTTP status + JSON body.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::models::v1::ResponseMeta;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ApiErrorBody {
    pub error: ApiErrorDetail,
    pub meta: ResponseMeta,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ApiErrorDetail {
    /// Short machine-readable code, e.g. `not_found`, `bad_request`, `internal`.
    pub code: String,
    /// Human-readable message.
    pub message: String,
    /// Optional structured detail (validation errors, internal context).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

/// Error type returned by every v1 handler.
#[derive(Debug)]
#[allow(dead_code)]
pub enum ApiError {
    NotFound(String),
    BadRequest(String),
    Internal(String),
    Database(sqlx::Error),
}

impl ApiError {
    fn status_and_code(&self) -> (StatusCode, &'static str) {
        match self {
            ApiError::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            ApiError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            ApiError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
            ApiError::Database(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        }
    }

    fn message(&self) -> String {
        match self {
            ApiError::NotFound(m) | ApiError::BadRequest(m) | ApiError::Internal(m) => m.clone(),
            ApiError::Database(_) => "database error".to_string(),
        }
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        // Log the full error server-side; clients only see "database error".
        tracing::error!("Database error: {}", e);
        ApiError::Database(e)
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        tracing::error!("Internal error: {:?}", e);
        ApiError::Internal(e.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = self.status_and_code();
        let body = ApiErrorBody {
            error: ApiErrorDetail {
                code: code.to_string(),
                message: self.message(),
                details: None,
            },
            meta: ResponseMeta::default(),
        };
        (status, Json(body)).into_response()
    }
}
