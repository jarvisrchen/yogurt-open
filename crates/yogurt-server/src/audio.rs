//! HTTP surface for `yogurt-audio` capture.
//!
//! The screen-recording prompt is fired explicitly from the Welcome button because it
//! otherwise only appeared when `start_capture` opened an SCK stream, which the Welcome CTA
//! is gated behind, so first-run users deadlocked.
//!
//! [`AudioErrorWrap`] maps [`AudioError`] variants onto HTTP status codes.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde::Serialize;
use yogurt_audio::{
    has_microphone_permission, has_screen_recording_permission, list_input_devices,
    permission_owner, request_microphone_permission, request_screen_recording_permission,
    AudioError, DeviceInfo, PermissionStatus,
};

/// Body of the permission and both request endpoints. All three return the full snapshot
/// so the SPA polls one cache key.
///
/// ```json
/// {
///   "screen_recording": "granted" | "denied" | "not_required",
///   "microphone":       "granted" | "denied" | "not_determined" | "not_required",
///   "owner":            "Ghostty" | null
/// }
/// ```
#[derive(Serialize)]
pub struct PermissionResponse {
    pub screen_recording: PermissionStatus,
    pub microphone: PermissionStatus,
    pub owner: Option<String>,
}

impl PermissionResponse {
    /// Both queries are thread-safe and idempotent, so nothing is cached.
    fn snapshot() -> Self {
        Self {
            screen_recording: has_screen_recording_permission(),
            microphone: has_microphone_permission(),
            owner: permission_owner(),
        }
    }
}

pub async fn get_permission() -> Json<PermissionResponse> {
    Json(PermissionResponse::snapshot())
}

/// macOS asks once per app, so a repeat call after a denial does not re-prompt.
pub async fn request_microphone() -> Json<PermissionResponse> {
    // The dialog appears asynchronously and the SPA's polling picks up the result.
    let _ = request_microphone_permission();
    Json(PermissionResponse::snapshot())
}

/// After a denial macOS will not re-prompt, so the SPA offers a System Settings deep link.
pub async fn request_screen_recording() -> Json<PermissionResponse> {
    let _ = request_screen_recording_permission();
    Json(PermissionResponse::snapshot())
}

pub async fn get_devices() -> Result<Json<Vec<DeviceInfo>>, (StatusCode, String)> {
    list_input_devices()
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

/// Newtype giving [`AudioError`] an [`IntoResponse`] impl past the orphan rule.
pub struct AudioErrorWrap(pub AudioError);

impl From<AudioError> for AudioErrorWrap {
    fn from(e: AudioError) -> Self {
        Self(e)
    }
}

impl IntoResponse for AudioErrorWrap {
    fn into_response(self) -> axum::response::Response {
        let (status, body) = match self.0 {
            AudioError::PermissionDenied => (
                StatusCode::FORBIDDEN,
                serde_json::json!({
                    "error": "permission_denied",
                    "message": "macOS Screen Recording permission is required",
                    "recovery": "open System Settings → Privacy & Security → Screen Recording",
                }),
            ),
            AudioError::UnsupportedPlatform => (
                StatusCode::NOT_IMPLEMENTED,
                serde_json::json!({
                    "error": "unsupported_platform",
                    "message": "system audio capture requires macOS 13+",
                }),
            ),
            other => (
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({ "error": "audio", "message": other.to_string() }),
            ),
        };
        (status, Json(body)).into_response()
    }
}
