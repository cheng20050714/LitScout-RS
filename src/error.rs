use thiserror::Error;

pub type Result<T> = std::result::Result<T, AppError>;

#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum AppError {
    #[error("invalid config: {0}")]
    InvalidConfig(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("{service} HTTP {status}: {body}")]
    HttpStatus {
        service: &'static str,
        status: u16,
        body: String,
    },

    #[error("{service} rate limit exceeded{reset}")]
    RateLimit {
        service: &'static str,
        reset: String,
    },

    #[error("XML parse failed: {0}")]
    Xml(String),

    #[error("workflow failed: {0}")]
    Workflow(String),

    #[error("LLM error: {0}")]
    Llm(String),

    #[error("feature is not implemented yet: {0}")]
    NotImplemented(&'static str),
}

impl axum::response::IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        use axum::http::StatusCode;
        use axum::Json;
        use serde_json::json;

        let status = match &self {
            AppError::InvalidConfig(_) | AppError::Xml(_) | AppError::Json(_) => {
                StatusCode::BAD_REQUEST
            }
            AppError::RateLimit { .. } => StatusCode::TOO_MANY_REQUESTS,
            AppError::HttpStatus { .. } | AppError::Http(_) | AppError::Llm(_) => {
                StatusCode::BAD_GATEWAY
            }
            AppError::NotImplemented(_) => StatusCode::NOT_IMPLEMENTED,
            AppError::Io(_) | AppError::Workflow(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let message = self.to_string();
        (status, Json(json!({ "error": message }))).into_response()
    }
}
