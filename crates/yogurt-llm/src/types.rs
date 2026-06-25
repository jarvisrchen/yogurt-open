//! Public chat types plus private OpenAI wire types.
//! Keeping the wire shape private lets a future non-OpenAI impl of `LlmClient` skip mirroring it.

use serde::{Deserialize, Serialize};

/// One chat message. `role` is a free-form string because OpenAI-compatible providers accept extra roles (`tool`, `developer`, ...).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(s: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: s.into(),
        }
    }

    pub fn user(s: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: s.into(),
        }
    }

    pub fn assistant(s: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: s.into(),
        }
    }
}

/// `stream` is descriptive only: `complete` always sends `stream=false` and `stream` always sends `stream=true`.
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    pub stream: bool,
}

/// `model` echoes the upstream-reported name, which can differ from the requested one (e.g. a dated snapshot).
#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub content: String,
    pub model: String,
    pub usage: Option<Usage>,
}

/// `done` is true for the terminal `[DONE]` chunk and for chunks whose upstream `finish_reason` is non-null, so accumulators must treat either as end-of-stream.
#[derive(Debug, Clone, Default)]
pub struct ChatChunk {
    pub delta: String,
    pub done: bool,
    /// What the provider reported the whole call used. OpenAI-style providers send it in a chunk of its own after the `finish_reason` one.
    pub usage: Option<Usage>,
}

/// Defaults to 0 for a field a provider leaves out, so an odd `usage` never fails the chunk that carries the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub struct Usage {
    #[serde(rename = "prompt_tokens", default)]
    pub input_tokens: i64,
    #[serde(rename = "completion_tokens", default)]
    pub output_tokens: i64,
}

#[derive(Serialize)]
pub(crate) struct OpenAiRequest<'a> {
    pub model: &'a str,
    pub messages: &'a [ChatMessage],
    pub stream: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub reasoning_split: bool,
    /// MiniMax-only `thinking` control. `None` omits the field so plain
    /// OpenAI-compatible providers never see an unknown key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<Thinking>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<StreamOptions>,
}

#[derive(Serialize)]
pub(crate) struct StreamOptions {
    pub include_usage: bool,
}

#[derive(Serialize)]
pub(crate) struct Thinking {
    pub r#type: &'static str,
}

#[derive(Deserialize)]
pub(crate) struct OpenAiResponse {
    pub model: String,
    pub choices: Vec<OpenAiChoice>,
    #[serde(default)]
    pub usage: Option<Usage>,
}

#[derive(Deserialize)]
pub(crate) struct OpenAiChoice {
    pub message: ChatMessage,
}

#[derive(Deserialize)]
pub(crate) struct OpenAiStreamChunk {
    #[serde(default)]
    pub choices: Vec<OpenAiStreamChoice>,
    #[serde(default)]
    pub usage: Option<Usage>,
}

#[derive(Deserialize)]
pub(crate) struct OpenAiStreamChoice {
    pub delta: OpenAiDelta,
    /// `null` mid-stream; non-null on the last content-bearing chunk, which maps to `done=true`.
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct OpenAiDelta {
    /// Absent on role-only and tool-call-only chunks.
    #[serde(default)]
    pub content: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct ModelsResponse {
    pub data: Vec<ModelDescriptor>,
}

#[derive(Deserialize)]
pub(crate) struct ModelDescriptor {
    pub id: String,
}
