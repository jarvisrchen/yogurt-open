//! What core knows about Pro: whether it is on, the free limits, and the hooks Pro fills.
//! How Pro is granted lives only in the `pro` module.

use std::sync::Arc;

use axum::Router;
use tokio::sync::broadcast;
use yogurt_db::dictionary::Entry;
use yogurt_db::providers::Provider;
use yogurt_db::settings::General;
use yogurt_db::style::{Setup, Style};
use yogurt_db::MeetingFilter;
use yogurt_llm::LlmClient;
use yogurt_stt::TranscriptEvent;

use crate::api::ApiError;
use crate::meetings::Meeting;
use crate::state::AppState;
use crate::voice_cleanup::Lists;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Entitlements {
    pub pro: bool,
}

/// `None` is unlimited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub ask_days: Option<u32>,
    pub dictation_hours: Option<u32>,
    /// Caps creating a label only, so labels made with Pro stay usable after it ends.
    pub labels_max: Option<usize>,
}

pub fn limits(ent: &Entitlements) -> Limits {
    if ent.pro {
        Limits {
            ask_days: None,
            dictation_hours: None,
            labels_max: None,
        }
    } else {
        Limits {
            ask_days: Some(30),
            dictation_hours: Some(24),
            labels_max: Some(5),
        }
    }
}

/// Every place core hands off to Pro. Each default is the free behavior, so [`NoPro`] is empty.
#[async_trait::async_trait]
pub trait ProHooks: std::any::Any + Send + Sync {
    /// Computed on every call, so a grant that expires ends on time on a server that runs for weeks.
    fn entitlements(&self, _state: &AppState) -> Entitlements {
        Entitlements::default()
    }

    /// Once at startup, after the listener binds.
    fn started(&self, _state: &AppState) {}

    /// Merged behind the session token.
    fn router(&self, _state: &AppState) -> Router<AppState> {
        Router::new()
    }

    /// `transcript` was subscribed before capture started, so it misses no segment.
    async fn meeting_started(
        &self,
        _state: &AppState,
        _settings: &General,
        _m: Arc<Meeting>,
        _transcript: broadcast::Receiver<TranscriptEvent>,
    ) {
    }

    fn meeting_stopped(&self, _state: &AppState, _m: &Meeting) {}

    /// After Enhance saved its summary.
    fn meeting_enhanced(&self, _m: &Meeting) {}

    /// Resolves once Pro's background work is at rest, and keeps it there until the guard drops.
    async fn pause(&self, _state: &AppState) -> Box<dyn Send> {
        Box::new(())
    }

    /// After a meeting's stored row changed.
    fn meeting_changed(&self, _state: &AppState, _id: &str) {}

    /// Text frames a live-meeting socket gets as soon as it connects.
    fn live_snapshot(&self, _m: &Meeting) -> Vec<String> {
        vec![]
    }

    /// The meeting bubble's Pro part: the live recap's newest bullets and the lemon edge. `None` leaves the card to the transcript.
    fn bubble_card(
        &self,
        _state: &AppState,
        _m: &Meeting,
    ) -> Option<yogurt_voice::bubble::ProCard> {
        None
    }

    /// The Voice dictionary that meeting and dictation transcription hear and dictations are respelled by.
    ///
    /// **Blocking:** async callers wrap it in `tokio::task::spawn_blocking`.
    fn dictionary(&self, _state: &AppState) -> Vec<Entry> {
        vec![]
    }

    /// The snippet a dictation said whole, or what the cleanup prompt lists for it.
    ///
    /// **Blocking:** async callers wrap it in `tokio::task::spawn_blocking`.
    fn voice_lists(
        &self,
        _state: &AppState,
        _dictionary: &[Entry],
        _raw: &str,
        _model_runs: bool,
        _style_of: Box<dyn FnOnce(&Setup) -> Style + Send>,
    ) -> (Option<String>, Lists) {
        (None, Lists::default())
    }

    /// The client for a provider with the CLI adapter.
    fn llm_for(
        &self,
        _state: &AppState,
        provider: &Provider,
    ) -> anyhow::Result<Arc<dyn LlmClient>> {
        Err(coding_agent_refused(provider))
    }

    /// Checks a new CLI provider's program before it is stored.
    fn check_cli_program(&self, _state: &AppState, _program: &str) -> anyhow::Result<()> {
        Err(coding_agents_required())
    }

    /// Whether a CLI program lists its own models now, so Settings offers Refresh.
    fn cli_has_catalog(&self, _state: &AppState, _program: &str) -> bool {
        false
    }

    /// The models a CLI provider's program lists, `None` when the program is unknown or has no catalog.
    async fn cli_models(
        &self,
        _state: &AppState,
        _provider: &Provider,
    ) -> anyhow::Result<Option<Vec<String>>> {
        Err(coding_agents_required())
    }

    /// The filter a meeting listing runs.
    ///
    /// **Blocking:** async callers wrap it in `tokio::task::spawn_blocking`.
    fn list_filters(
        &self,
        _state: &AppState,
        filter: MeetingFilter,
    ) -> anyhow::Result<MeetingFilter> {
        without_archive(filter)
    }
}

/// The archive is part of Pro, so free lists archived meetings with the rest and has no archive listing of its own.
pub(crate) fn without_archive(filter: MeetingFilter) -> anyhow::Result<MeetingFilter> {
    if filter.archived == Some(true) {
        return Err(ApiError::ProRequired { feature: "archive" }.into());
    }
    Ok(MeetingFilter {
        archived: None,
        ..filter
    })
}

pub fn coding_agents_required() -> anyhow::Error {
    ApiError::ProRequired {
        feature: "coding_agents",
    }
    .into()
}

/// Enhance, chat, Ask, recap, and Test show only this text, so it names where to pick another model.
pub fn coding_agent_refused(provider: &Provider) -> anyhow::Error {
    coding_agents_required().context(format!(
        "{} is a coding agent, which needs Yogurt Pro. Pick another model in Settings > Model",
        provider.name
    ))
}

pub struct NoPro;

impl ProHooks for NoPro {}

pub fn hooks() -> Arc<dyn ProHooks> {
    #[cfg(feature = "pro")]
    return Arc::new(crate::pro::Pro::default());
    #[cfg(not(feature = "pro"))]
    return Arc::new(NoPro);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_limits_hold_until_pro() {
        let free = limits(&Entitlements::default());
        assert_eq!(
            (free.ask_days, free.dictation_hours, free.labels_max),
            (Some(30), Some(24), Some(5))
        );
        let pro = limits(&Entitlements { pro: true });
        assert_eq!(
            (pro.ask_days, pro.dictation_hours, pro.labels_max),
            (None, None, None)
        );
    }

    #[test]
    fn a_refused_coding_agent_names_settings_and_answers_402() {
        use axum::response::IntoResponse;
        let p = Provider {
            id: "p".into(),
            name: "Claude Code".into(),
            base_url: String::new(),
            model: "claude".into(),
            is_active: true,
            created_at: 0,
            adapter: yogurt_db::providers::adapter::CLI.into(),
            cli_model: String::new(),
        };
        let err = coding_agent_refused(&p);
        assert!(err.to_string().contains("Settings > Model"), "{err}");
        let res = ApiError::from(err).into_response();
        assert_eq!(res.status(), axum::http::StatusCode::PAYMENT_REQUIRED);
    }
}
