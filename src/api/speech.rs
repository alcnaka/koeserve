use axum::{
    Json,
    body::Body,
    extract::State,
    http::{StatusCode, header},
    response::Response,
};

use crate::{AppState, error::ApiError, tts::SynthesisRequest};

#[utoipa::path(
    post,
    path = "/v1/speech",
    request_body = SynthesisRequest,
    responses(
        (status = 200, description = "Synthesized WAV audio", content_type = "audio/wav"),
        (status = 400, description = "Invalid request or unknown voice", body = crate::error::ApiErrorBody),
        (status = 503, description = "TTS service unavailable or queue full", body = crate::error::ApiErrorBody),
        (status = 500, description = "Synthesis failed", body = crate::error::ApiErrorBody)
    ),
    tag = "speech"
)]
pub async fn synthesize(
    State(state): State<AppState>,
    Json(request): Json<SynthesisRequest>,
) -> Result<Response, ApiError> {
    let wav = state.tts.synthesize(request).await?;

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "audio/wav")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(wav))
        .map_err(|_| ApiError::Internal)
}
