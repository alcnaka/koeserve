use axum::{extract::State, Json};

use crate::{tts::VoiceInfo, AppState};

#[utoipa::path(
    get,
    path = "/v1/voices",
    responses((status = 200, body = [VoiceInfo])),
    tag = "voices"
)]
pub async fn list_voices(State(state): State<AppState>) -> Json<Vec<VoiceInfo>> {
    Json(state.tts.voices())
}
