mod health;
mod speech;
mod voices;

use axum::{
    Router,
    routing::{get, post},
};
use utoipa::OpenApi;

use crate::{
    AppState,
    error::ApiErrorBody,
    tts::{SynthesisRequest, VoiceDefaults, VoiceInfo},
};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "KoeServe API",
        version = "0.1.0",
        description = "Lightweight Japanese TTS API powered by JPreprocess and JBonsai"
    ),
    paths(
        health::health,
        voices::list_voices,
        speech::synthesize
    ),
    components(schemas(
        SynthesisRequest,
        VoiceDefaults,
        VoiceInfo,
        ApiErrorBody
    )),
    tags(
        (name = "system", description = "Service status"),
        (name = "voices", description = "Available HTS voices"),
        (name = "speech", description = "Speech synthesis")
    )
)]
pub struct ApiDoc;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health::health))
        .route("/v1/voices", get(voices::list_voices))
        .route("/v1/speech", post(speech::synthesize))
}
