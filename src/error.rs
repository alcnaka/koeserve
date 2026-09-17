use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub struct ApiErrorBody {
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{0}")]
    InvalidRequest(String),
    #[error("unknown voice: {0}")]
    UnknownVoice(String),
    #[error("TTS engine is not ready")]
    NotReady,
    #[error("TTS queue is full")]
    QueueFull,
    #[error("speech synthesis failed")]
    SynthesisFailed,
    #[error("internal server error")]
    Internal,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            Self::InvalidRequest(_) => (StatusCode::BAD_REQUEST, "invalid_request"),
            Self::UnknownVoice(_) => (StatusCode::BAD_REQUEST, "unknown_voice"),
            Self::NotReady => (StatusCode::SERVICE_UNAVAILABLE, "tts_not_ready"),
            Self::QueueFull => (StatusCode::SERVICE_UNAVAILABLE, "queue_full"),
            Self::SynthesisFailed => (StatusCode::INTERNAL_SERVER_ERROR, "synthesis_failed"),
            Self::Internal => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
        };

        let body = ApiErrorBody {
            code,
            message: self.to_string(),
        };

        (status, Json(body)).into_response()
    }
}
