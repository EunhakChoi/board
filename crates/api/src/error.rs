use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use board_engine::{EngineError, StoreError};
use serde::Serialize;

/// The one error shape of the API (`Error` in the contract).
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
    detail: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct Body<'a> {
    code: &'a str,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<&'a serde_json::Value>,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            detail: None,
        }
    }

    #[must_use]
    pub fn with_detail(mut self, detail: serde_json::Value) -> Self {
        self.detail = Some(detail);
        self
    }

    pub fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized", "Sign in first")
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }
}

impl From<StoreError> for ApiError {
    fn from(error: StoreError) -> Self {
        // The message stays in the log; clients get a generic one.
        eprintln!("board-server: storage error: {error}");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "Internal error",
        )
    }
}

impl From<EngineError> for ApiError {
    fn from(error: EngineError) -> Self {
        match error {
            EngineError::Forbidden => Self::new(StatusCode::FORBIDDEN, "forbidden", "Not allowed"),
            EngineError::NotFound(what) => Self::not_found(format!("{what} not found")),
            EngineError::Invalid(message) => Self::new(StatusCode::BAD_REQUEST, "invalid", message),
            EngineError::Exists(what) => Self::new(
                StatusCode::CONFLICT,
                "exists",
                format!("{what} already exists"),
            ),
            EngineError::Conflict { head } => Self::new(
                StatusCode::CONFLICT,
                "conflict",
                "main moved since your base; pull and try again",
            )
            .with_detail(serde_json::json!({ "head": head })),
            EngineError::Pending(message) => Self::new(StatusCode::CONFLICT, "pending", message),
            EngineError::Changed(rev) => Self::new(
                StatusCode::CONFLICT,
                "changed",
                "someone changed it meanwhile; read it again",
            )
            .with_detail(serde_json::json!({ "rev": rev })),
            EngineError::Unavailable(message) => {
                Self::new(StatusCode::SERVICE_UNAVAILABLE, "unavailable", message)
            }
            EngineError::Store(error) => error.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Body {
            code: self.code,
            message: &self.message,
            detail: self.detail.as_ref(),
        };
        (self.status, Json(body)).into_response()
    }
}
