pub mod chat;
pub mod labels;
pub mod library_chat;
pub mod meetings;
pub mod settings;
pub mod stt_models;
pub mod voice;

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;

#[derive(Debug)]
pub(crate) enum ApiError {
    NotFound,
    BadRequest(String),
    Conflict(String),
    /// 402 with `code: "pro_required"`, so the web can show what unlocks it.
    ProRequired {
        feature: &'static str,
    },
    Internal(anyhow::Error),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::NotFound => f.write_str("not found"),
            ApiError::BadRequest(m) | ApiError::Conflict(m) => f.write_str(m),
            ApiError::ProRequired { .. } => f.write_str("this needs Yogurt Pro"),
            ApiError::Internal(e) => write!(f, "{e:#}"),
        }
    }
}

impl std::error::Error for ApiError {}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let code = match &self {
            ApiError::NotFound => StatusCode::NOT_FOUND,
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::Conflict(_) => StatusCode::CONFLICT,
            ApiError::ProRequired { .. } => StatusCode::PAYMENT_REQUIRED,
            ApiError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let mut body = json!({ "error": self.to_string() });
        if let ApiError::ProRequired { feature } = self {
            body["code"] = "pro_required".into();
            body["feature"] = feature.into();
        }
        (code, Json(body)).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        let e = match e.downcast::<ApiError>() {
            Ok(api) => return api,
            Err(e) => e,
        };
        // "label not found" is 400 here because meeting PATCH treats an unknown `label_ids`
        // entry as a bad request. The labels router maps its own unknown id to 404 first.
        let s = e.to_string();
        if s.contains("label not found")
            || s.contains("invalid color")
            || s.contains("already exists")
            || s.contains("empty")
            || s.contains("40 characters")
        {
            ApiError::BadRequest(s)
        } else if s.contains("not found") {
            ApiError::NotFound
        } else {
            ApiError::Internal(e)
        }
    }
}

pub(crate) async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ApiError::Internal(anyhow::Error::new(e)))?
        .map_err(ApiError::Internal)
}

#[derive(Debug, Deserialize)]
pub(crate) struct StatsQuery {
    #[serde(default)]
    pub(crate) range: yogurt_db::stats_range::Range,
}
