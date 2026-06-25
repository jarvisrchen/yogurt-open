//! API responses never include a raw API key, only the mask (`••••XXXX`, last 4 chars)
//! via [`ProviderView::api_key_masked`].
//! `tests/settings_api.rs::api_responses_never_include_the_raw_api_key` enforces it.

use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use yogurt_db::{providers, settings};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/settings", get(get_settings).patch(patch_settings))
        .route(
            "/api/settings/providers",
            get(list_providers).post(create_provider),
        )
        .route(
            "/api/settings/providers/{id}",
            axum::routing::patch(update_provider).delete(delete_provider),
        )
        .route(
            "/api/settings/providers/{id}/activate",
            post(activate_provider),
        )
        .route("/api/settings/providers/{id}/key", post(set_provider_key))
        .route("/api/settings/providers/{id}/test", post(test_provider))
        .route(
            "/api/settings/providers/{id}/models",
            post(list_provider_models),
        )
        .route("/api/settings/stt/key", post(set_stt_key))
        .route("/api/settings/stt/test", post(test_stt_key))
        .route("/api/settings/stt/local/test", post(test_local_stt))
        .route("/api/settings/presets", get(list_presets))
}

/// Intentionally has no `api_key` field. The secret is exposed only via [`Self::api_key_masked`].
#[derive(Serialize)]
struct ProviderView {
    id: String,
    name: String,
    base_url: String,
    model: String,
    is_active: bool,
    created_at: i64,
    /// `Some("••••XXXX")` when a key is stored, `None` otherwise. Never the raw key.
    api_key_masked: Option<String>,
    /// `"http"` or `"cli"`, see `yogurt_db::providers::adapter`.
    /// A `cli` row has no base URL, API key, or model catalog.
    adapter: String,
    /// `cli` adapter only: the `--model` value passed to the CLI. Empty means the CLI's default.
    cli_model: String,
    /// Whether [`list_provider_models`] can fetch a live catalog, so the UI shows Refresh.
    has_model_catalog: bool,
}

fn to_view(s: &AppState, p: providers::Provider, masked: Option<String>) -> ProviderView {
    let has_model_catalog =
        p.adapter != providers::adapter::CLI || s.pro.cli_has_catalog(s, &p.model);
    ProviderView {
        id: p.id,
        name: p.name,
        base_url: p.base_url,
        model: p.model,
        is_active: p.is_active,
        created_at: p.created_at,
        api_key_masked: masked,
        adapter: p.adapter,
        cli_model: p.cli_model,
        has_model_catalog,
    }
}

fn masked_views(state: &AppState, rows: Vec<providers::Provider>) -> Vec<ProviderView> {
    rows.into_iter()
        .map(|p| {
            let masked = state.keys.masked(&p.id).ok().flatten();
            to_view(state, p, masked)
        })
        .collect()
}

#[derive(Serialize)]
struct PresetView {
    name: &'static str,
    base_url: &'static str,
    default_model: &'static str,
    models: &'static [&'static str],
    docs_url: &'static str,
    adapter: &'static str,
    default_cli_model: &'static str,
}

#[derive(Serialize)]
struct SettingsView {
    general: settings::General,
    providers: Vec<ProviderView>,
    presets: Vec<PresetView>,
    /// Mask of the `stt-deepgram` key-file entry. Never the raw key.
    deepgram_key_masked: Option<String>,
    paths: PathsView,
}

/// Display strings, with the home directory shortened to `~`.
#[derive(Serialize)]
struct PathsView {
    data_dir: String,
    keys: String,
    notes: String,
}

fn display_path(p: &std::path::Path, home: Option<&std::path::Path>) -> String {
    match home.and_then(|h| p.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.display()),
        None => p.display().to_string(),
    }
}

async fn get_settings(State(s): State<AppState>) -> Result<Json<SettingsView>, Error> {
    let general = settings::load_general(&s.db)?;
    let presets = preset_views();
    let providers = masked_views(&s, providers::list(&s.db)?);
    let deepgram_key_masked = s
        .keys
        .masked(crate::meetings::DEEPGRAM_KEY_ID)
        .ok()
        .flatten();
    let base = directories::BaseDirs::new();
    let home = base.as_ref().map(|b| b.home_dir());
    let paths = PathsView {
        data_dir: display_path(&s.data_dir, home),
        keys: display_path(&s.keys_path, home),
        notes: display_path(s.markdown_exporter.notes_dir(), home),
    };
    Ok(Json(SettingsView {
        general,
        providers,
        presets,
        deepgram_key_masked,
        paths,
    }))
}

/// Reject a PATCH that would leave `stt_provider == "local"` pointed at a model not on disk.
/// Checks the effective settings (current row with `patch` overlaid) so it covers a
/// provider flip, a model flip, and both together.
async fn validate_stt_patch(
    db: &yogurt_db::Db,
    patch: &settings::GeneralPatch,
) -> Result<(), Error> {
    let current = settings::load_general(db)?;
    let effective_provider = patch
        .stt_provider
        .as_deref()
        .unwrap_or(&current.stt_provider);
    if effective_provider != "local" {
        return Ok(());
    }
    let effective_model = patch
        .stt_model
        .clone()
        .unwrap_or_else(|| current.stt_model.clone());

    ensure_downloaded(effective_model, "Transcription").await
}

/// 422 unless `model` is a known local model with verified files, naming the Settings page to download it from.
async fn ensure_downloaded(model: String, page: &str) -> Result<(), Error> {
    // `is_downloaded` can hash a multi-GB file on a legacy marker, so keep it off the reactor.
    let downloaded = tokio::task::spawn_blocking({
        let model = model.clone();
        move || {
            yogurt_stt::models::lookup(&model)
                .map(yogurt_stt::models::is_downloaded)
                .unwrap_or(false)
        }
    })
    .await
    .map_err(|e| Error::Internal(format!("join is_downloaded check: {e}")))?;

    if downloaded {
        Ok(())
    } else {
        Err(Error::Unprocessable(format!(
            "local model {model} is not downloaded - download it in \
             Settings > {page} first"
        )))
    }
}

async fn patch_settings(
    State(s): State<AppState>,
    Json(patch): Json<settings::GeneralPatch>,
) -> Result<Json<settings::General>, Error> {
    validate_stt_patch(&s.db, &patch).await?;
    if let Some(h) = &patch.voice_hotkey {
        h.parse::<yogurt_voice::Hotkey>()
            .map_err(|e| Error::BadRequest(e.to_string()))?;
    }
    // `builtin` is accepted before its download, so the picker can offer the Download button.
    if let Some(p) = patch
        .voice_cleanup_provider
        .as_ref()
        .filter(|p| *p != settings::VOICE_CLEANUP_BUILTIN)
    {
        crate::voice_cleanup::model(&s, p).map_err(Error::BadRequest)?;
    }
    if let Some(m) = &patch.voice_stt_model {
        let mut g = settings::load_general(&s.db)
            .map_err(|e| Error::Internal(format!("load general settings: {e}")))?;
        g.voice_stt_model = m.clone();
        if let Some(stt) = &patch.stt_model {
            g.stt_model = stt.clone();
        }
        if m == settings::VOICE_STT_DEEPGRAM {
            if crate::meetings::deepgram_key(s.keys.as_ref()).is_none() {
                return Err(Error::Unprocessable(
                    "no Deepgram key stored - add one in Settings > Transcription first".into(),
                ));
            }
        } else {
            let page = if m == settings::VOICE_STT_MEETINGS {
                "Transcription"
            } else {
                "Voice"
            };
            ensure_downloaded(g.voice_model().to_string(), page).await?;
        }
    }
    // The cleanup keys too, so the built-in model loads or frees its memory now rather than at the next poll.
    let voice_changed = patch.voice_enabled.is_some()
        || patch.voice_hotkey.is_some()
        || patch.voice_stt_model.is_some()
        || patch.voice_cleanup.is_some()
        || patch.voice_cleanup_provider.is_some();
    let retention_changed = patch.voice_retention.is_some();
    let saved = settings::save_general_patch(&s.db, patch)?;
    if voice_changed {
        s.voice.wake.notify_one();
    }
    if retention_changed {
        crate::voice::trim_history(&s).await;
    }
    Ok(Json(saved))
}

async fn list_providers(State(s): State<AppState>) -> Result<Json<Vec<ProviderView>>, Error> {
    Ok(Json(masked_views(&s, providers::list(&s.db)?)))
}

async fn list_presets() -> Json<Vec<PresetView>> {
    Json(preset_views())
}

fn preset_views() -> Vec<PresetView> {
    providers::PRESETS
        .iter()
        .map(|p| PresetView {
            name: p.name,
            base_url: p.base_url,
            default_model: p.default_model,
            models: p.models,
            docs_url: p.docs_url,
            adapter: p.adapter,
            default_cli_model: p.default_cli_model,
        })
        .collect()
}

/// Reject a malformed `adapter` before it reaches a DB row, so a bad CLI program
/// gets a clear 422 instead of a confusing error from `from_active_provider` later.
fn validate_new_provider(s: &AppState, body: &providers::NewProvider) -> Result<(), Error> {
    match body.adapter.as_str() {
        providers::adapter::HTTP => Ok(()),
        providers::adapter::CLI => s
            .pro
            .check_cli_program(s, &body.model)
            .map_err(|e| Error::pro_or(e, Error::Unprocessable)),
        other => Err(Error::Unprocessable(format!(
            "'{other}' is not a recognized provider adapter (expected \"http\" or \"cli\")"
        ))),
    }
}

async fn create_provider(
    State(s): State<AppState>,
    Json(body): Json<providers::NewProvider>,
) -> Result<Json<ProviderView>, Error> {
    validate_new_provider(&s, &body)?;
    let id = providers::insert(&s.db, body)?;
    let p = providers::list(&s.db)?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| Error::Internal("inserted provider missing".into()))?;
    let view = masked_views(&s, vec![p]).remove(0);
    Ok(Json(view))
}

/// Full replace: every field must be supplied.
#[derive(Deserialize)]
struct UpdateProviderBody {
    name: String,
    base_url: String,
    model: String,
    #[serde(default)]
    cli_model: String,
}

async fn update_provider(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateProviderBody>,
) -> Result<Json<ProviderView>, Error> {
    providers::update(
        &s.db,
        &id,
        &body.name,
        &body.base_url,
        &body.model,
        &body.cli_model,
    )?;
    let p = providers::list(&s.db)?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or(Error::NotFound)?;
    let view = masked_views(&s, vec![p]).remove(0);
    Ok(Json(view))
}

async fn delete_provider(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, Error> {
    // Best-effort: a stale key entry without a DB row is harmless.
    let _ = s.keys.delete(&id);
    providers::delete(&s.db, &id)?;
    if settings::load_general(&s.db)?.voice_cleanup_provider == id {
        settings::save_general_patch(
            &s.db,
            settings::GeneralPatch {
                voice_cleanup_provider: Some(settings::VOICE_CLEANUP_BUILTIN.into()),
                ..Default::default()
            },
        )?;
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn activate_provider(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ProviderView>, Error> {
    providers::set_active(&s.db, &id)?;
    let p = providers::active(&s.db)?
        .ok_or_else(|| Error::Internal("active provider missing after set_active".into()))?;
    let view = masked_views(&s, vec![p]).remove(0);
    Ok(Json(view))
}

/// `api_key` is the only place a raw secret appears in this module. It is inbound only.
#[derive(Deserialize)]
struct SetKeyBody {
    api_key: String,
}

/// The Deepgram key has no `providers` row. The key-file entry under
/// [`crate::meetings::DEEPGRAM_KEY_ID`] is the configuration.
async fn set_stt_key(
    State(s): State<AppState>,
    Json(body): Json<SetKeyBody>,
) -> Result<StatusCode, Error> {
    let key = body.api_key.trim();
    if key.is_empty() {
        return Err(Error::Internal("api_key must not be empty".into()));
    }
    s.keys
        .set(crate::meetings::DEEPGRAM_KEY_ID, key)
        .map_err(|e| Error::Internal(format!("key store set: {e}")))?;
    // Voice on Deepgram starts, or swaps to the new key, now rather than at the next poll.
    s.voice.wake.notify_one();
    Ok(StatusCode::NO_CONTENT)
}

/// Same shape as [`TestProviderBody`]. Absent tests the stored Deepgram key.
#[derive(Deserialize)]
struct TestSttBody {
    #[serde(default)]
    api_key: Option<String>,
}

/// `error` is `null`, not omitted, when `ok` is true.
#[derive(Serialize)]
struct TestSttResult {
    ok: bool,
    error: Option<String>,
}

/// Cheap liveness check against Deepgram (`GET /v1/projects`) with no audio.
/// A rejected key is HTTP 200 with `ok: false`.
async fn test_stt_key(
    State(s): State<AppState>,
    Json(body): Json<TestSttBody>,
) -> Result<Json<TestSttResult>, Error> {
    let draft = body
        .api_key
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty());
    let key = match draft {
        Some(k) => Some(k),
        None => s
            .keys
            .get(crate::meetings::DEEPGRAM_KEY_ID)
            .map_err(|e| Error::Internal(format!("key store read: {e}")))?,
    };
    let Some(key) = key else {
        return Ok(Json(TestSttResult {
            ok: false,
            error: Some("No Deepgram key stored yet - paste one above, then test.".into()),
        }));
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| Error::Internal(format!("http client build: {e}")))?;

    let result = match client
        .get("https://api.deepgram.com/v1/projects")
        .header("Authorization", format!("Token {key}"))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => TestSttResult {
            ok: true,
            error: None,
        },
        Ok(resp) => {
            let status = resp.status();
            // Auth failures get a canned message so upstream text never reaches a response.
            let msg = if status.as_u16() == 401 || status.as_u16() == 403 {
                "Deepgram rejected this key.".to_string()
            } else {
                // Upstreams can quote the key back, so redact unconditionally.
                let body = resp
                    .text()
                    .await
                    .unwrap_or_default()
                    .replace(&key, "<redacted>");
                format!("Deepgram returned {status}: {body}")
            };
            TestSttResult {
                ok: false,
                error: Some(truncate(&msg, MAX_TEST_ERROR_LEN)),
            }
        }
        Err(e) => TestSttResult {
            ok: false,
            error: Some(truncate(&format!("{e:#}"), MAX_TEST_ERROR_LEN)),
        },
    };
    Ok(Json(result))
}

/// `model` defaults to `general.stt_model`.
#[derive(Deserialize)]
struct TestLocalSttBody {
    #[serde(default)]
    model: Option<String>,
}

/// Loads the selected local model and decodes a silent clip, so a broken
/// download is found here and not when a meeting starts.
/// The `{ok, error}` shape matches `test_stt_key` so the frontend's `TestKeyButton` works unchanged.
/// `spawn_blocking` because the model mmap and decode would starve the tokio runtime.
async fn test_local_stt(
    State(s): State<AppState>,
    Json(body): Json<TestLocalSttBody>,
) -> Result<Json<TestProviderResult>, Error> {
    let model_name = match body.model {
        Some(m) if !m.trim().is_empty() => m,
        _ => settings::load_general(&s.db)?.stt_model,
    };

    let result = tokio::task::spawn_blocking(move || -> TestProviderResult {
        let spec = match yogurt_stt::models::lookup(&model_name) {
            Some(spec) => spec,
            None => {
                return TestProviderResult {
                    ok: false,
                    model: None,
                    error: Some(format!("Unknown model: {model_name}")),
                }
            }
        };
        let Some(model_path) = yogurt_stt::models::resolve_model(spec) else {
            return TestProviderResult {
                ok: false,
                model: None,
                error: Some(format!(
                    "{model_name} is not downloaded yet — download it above, then test."
                )),
            };
        };
        let started = std::time::Instant::now();
        match yogurt_stt::LocalModel::self_test(spec, model_path) {
            Ok(()) => TestProviderResult {
                ok: true,
                model: Some(format!(
                    "{model_name} · loaded and ran in {}ms",
                    started.elapsed().as_millis()
                )),
                error: None,
            },
            Err(e) => TestProviderResult {
                ok: false,
                model: None,
                error: Some(truncate(&format!("{e:#}"), MAX_TEST_ERROR_LEN)),
            },
        }
    })
    .await
    .map_err(|e| Error::Internal(format!("test task panicked: {e}")))?;

    Ok(Json(result))
}

async fn set_provider_key(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SetKeyBody>,
) -> Result<StatusCode, Error> {
    // Check first so a typo'd id cannot leave an orphan key entry.
    if providers::list(&s.db)?.iter().all(|p| p.id != id) {
        return Err(Error::NotFound);
    }
    s.keys
        .set(&id, &body.api_key)
        .map_err(|e| Error::Internal(format!("key store set: {e}")))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct TestProviderBody {
    /// Draft key to test without storing it. `None` or empty tests the stored key.
    #[serde(default)]
    api_key: Option<String>,
    /// Unsaved edits to test before Save. `None` or blank tests the saved value.
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    base_url: Option<String>,
}

/// A rejected key is still HTTP 200 with `ok: false`. Non-200 means the test could not run.
#[derive(Serialize)]
struct TestProviderResult {
    ok: bool,
    /// Model name the provider echoed back, which is not always the one we
    /// asked for (aliases, `-latest` tags, OpenRouter routing).
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Provider error bodies can be multi-KB JSON blobs; the actionable part is at the front.
const MAX_TEST_ERROR_LEN: usize = 400;

/// The draft key builds a throwaway client and is never written, logged, or echoed.
/// `OpenAiCompatClient` scrubs it from the provider's error text, since providers
/// quote the offending key back ("Incorrect API key provided: sk-abc123").
async fn test_provider(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<TestProviderBody>,
) -> Result<Json<TestProviderResult>, Error> {
    let provider = find_provider(&s, &id)?;

    use yogurt_llm::LlmClient as _;

    // A `cli` provider has no key: `locate` either finds the program on `$PATH` or not.
    if provider.adapter == providers::adapter::CLI {
        let client = match s.pro.llm_for(&s, &provider) {
            Ok(c) => c,
            Err(e) => {
                // A refusal's own text names Settings > Model, and its chain would only repeat it.
                let msg = if is_pro_required(&e) {
                    e.to_string()
                } else {
                    format!("{e:#}")
                };
                return Ok(Json(TestProviderResult {
                    ok: false,
                    model: None,
                    error: Some(truncate(&msg, MAX_TEST_ERROR_LEN)),
                }));
            }
        };
        let req = yogurt_llm::ChatRequest {
            messages: vec![yogurt_llm::ChatMessage::user("Reply with: ok")],
            stream: false,
        };
        return Ok(Json(match client.complete(req).await {
            Ok(resp) => TestProviderResult {
                ok: true,
                model: Some(resp.model),
                error: None,
            },
            Err(e) => TestProviderResult {
                ok: false,
                model: None,
                error: Some(truncate(&format!("{e:#}"), MAX_TEST_ERROR_LEN)),
            },
        }));
    }

    let key = provider_key(&s, &provider.id, body.api_key)?;
    let key = match key {
        Some(k) => k,
        None => {
            return Ok(Json(TestProviderResult {
                ok: false,
                model: None,
                error: Some(
                    "No key stored for this provider yet - paste one above, \
                     then test."
                        .into(),
                ),
            }))
        }
    };

    // A one-turn completion exercises the path enhance and chat use, so it also catches a
    // wrong model name or a non-OpenAI base URL that `/models` would miss.
    // No `max_tokens` cap: some reasoning models reject a tiny budget.
    let draft = |v: Option<String>| v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let client = crate::llm_openai::OpenAiCompatClient::new(
        draft(body.base_url).unwrap_or(provider.base_url),
        key,
        draft(body.model).unwrap_or(provider.model),
    );
    let req = yogurt_llm::ChatRequest {
        messages: vec![yogurt_llm::ChatMessage::user("Reply with: ok")],
        stream: false,
    };

    Ok(Json(match client.complete(req).await {
        Ok(resp) => TestProviderResult {
            ok: true,
            model: Some(resp.model),
            error: None,
        },
        Err(e) => TestProviderResult {
            ok: false,
            model: None,
            error: Some(truncate(&format!("{e:#}"), MAX_TEST_ERROR_LEN)),
        },
    }))
}

/// A draft `api_key` replaces the stored key, so models can be listed before committing it.
#[derive(Deserialize)]
struct ListModelsBody {
    #[serde(default)]
    api_key: Option<String>,
}

/// Lists `/v1/models` for an `http` provider, `--list-models` on the binary for a `cli` one.
/// A draft key wins over the stored key and is never persisted.
/// No key at all is not an error: local runtimes (Ollama, LM Studio) need none.
/// Upstream rejections map to `Error::BadGateway`.
async fn list_provider_models(
    State(s): State<AppState>,
    Path(id): Path<String>,
    body: Option<Json<ListModelsBody>>,
) -> Result<Json<Vec<String>>, Error> {
    let draft = body.and_then(|b| b.api_key.clone());

    // A `cli` provider has no `/v1/models` and no key, so the catalog comes from the binary
    // itself (`cursor-agent --list-models`, `opencode models`, `codex debug models`).
    let provider = find_provider(&s, &id)?;
    if provider.adapter == providers::adapter::CLI {
        // No catalog (`claude`, `copilot`) is a 422 so the UI keeps the preset's static aliases.
        // Only a spawn, auth, or parse failure is a 502.
        return match s.pro.cli_models(&s, &provider).await {
            Ok(Some(models)) => Ok(Json(models)),
            Ok(None) => Err(Error::Unprocessable(format!(
                "{} has no model catalog to refresh",
                provider.model
            ))),
            Err(e) => Err(Error::pro_or(e, |m| {
                Error::BadGateway(truncate(&m, MAX_TEST_ERROR_LEN))
            })),
        };
    }

    let (provider, key) = provider_and_key(&s, &id, draft)?;

    let client = crate::llm_openai::OpenAiCompatClient::new(
        provider.base_url.clone(),
        key.unwrap_or_default(),
        provider.model.clone(),
    );
    match client.list_models().await {
        Ok(models) => Ok(Json(models)),
        Err(e) => Err(Error::BadGateway(truncate(
            &format!("{e:#}"),
            MAX_TEST_ERROR_LEN,
        ))),
    }
}

fn find_provider(s: &AppState, id: &str) -> Result<providers::Provider, Error> {
    providers::list(&s.db)?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or(Error::NotFound)
}

fn provider_and_key(
    s: &AppState,
    id: &str,
    draft: Option<String>,
) -> Result<(providers::Provider, Option<String>), Error> {
    let provider = find_provider(s, id)?;
    let key = provider_key(s, id, draft)?;
    Ok((provider, key))
}

/// A trimmed non-empty `draft` wins, otherwise the stored key. `None` means neither exists.
fn provider_key(s: &AppState, id: &str, draft: Option<String>) -> Result<Option<String>, Error> {
    let draft = draft
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty());
    match draft {
        Some(k) => Ok(Some(k)),
        None => read_stored_key(s, id),
    }
}

fn read_stored_key(s: &AppState, id: &str) -> Result<Option<String>, Error> {
    s.keys
        .get(id)
        .map_err(|e| Error::Internal(format!("key store read: {e}")))
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    format!("{cut}…")
}

#[derive(Debug)]
enum Error {
    NotFound,
    Internal(String),
    /// 422: valid JSON describing an invalid configuration.
    Unprocessable(String),
    /// 400: a value that is not a coherent configuration attempt.
    BadRequest(String),
    /// 502: a live probe against the provider failed.
    BadGateway(String),
    /// 402 for a coding agent without Pro.
    ProRequired,
}

fn is_pro_required(e: &anyhow::Error) -> bool {
    matches!(
        e.downcast_ref(),
        Some(crate::api::ApiError::ProRequired { .. })
    )
}

impl Error {
    /// [`Error::ProRequired`] when a Pro hook refused, `other` with the error's text otherwise.
    fn pro_or(e: anyhow::Error, other: impl FnOnce(String) -> Error) -> Error {
        if is_pro_required(&e) {
            Error::ProRequired
        } else {
            other(format!("{e:#}"))
        }
    }
}

impl From<anyhow::Error> for Error {
    fn from(e: anyhow::Error) -> Self {
        Self::Internal(format!("{e:#}"))
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Internal(format!("{e}"))
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        match self {
            Error::NotFound => (StatusCode::NOT_FOUND, "not found").into_response(),
            Error::Internal(s) => (StatusCode::INTERNAL_SERVER_ERROR, s).into_response(),
            Error::Unprocessable(msg) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({ "error": msg })),
            )
                .into_response(),
            Error::BadRequest(msg) => (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": msg })),
            )
                .into_response(),
            Error::BadGateway(msg) => (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": msg })),
            )
                .into_response(),
            Error::ProRequired => crate::api::ApiError::ProRequired {
                feature: "coding_agents",
            }
            .into_response(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::display_path;
    use std::path::Path;

    #[test]
    fn display_path_shortens_only_the_home_prefix() {
        let home = Some(Path::new("/Users/a"));
        assert_eq!(
            display_path(Path::new("/Users/a/.yogurt/keys.json"), home),
            "~/.yogurt/keys.json"
        );
        assert_eq!(display_path(Path::new("/Users/a"), home), "~");
        assert_eq!(display_path(Path::new("/Users/ab/x"), home), "/Users/ab/x");
        assert_eq!(display_path(Path::new("/tmp/y"), None), "/tmp/y");
    }

    #[tokio::test]
    async fn voice_stt_model_rejects_a_missing_model() {
        use super::{patch_settings, Error};
        use axum::{extract::State, Json};
        let tmp = tempfile::tempdir().unwrap();
        let state = crate::state::AppState::in_memory(
            crate::Mode::Release,
            std::sync::Arc::new(
                crate::storage::Storage::init_at(&tmp.path().join("db.sqlite")).unwrap(),
            ),
            std::sync::Arc::new(
                crate::session::load_or_create(&tmp.path().join("session-token")).unwrap(),
            ),
            7878,
            tmp.path().join("notes"),
        )
        .unwrap();
        let patch = |m: &str| yogurt_db::settings::GeneralPatch {
            voice_stt_model: Some(m.into()),
            ..Default::default()
        };
        assert!(matches!(
            patch_settings(State(state.clone()), Json(patch("nope"))).await,
            Err(Error::Unprocessable(_))
        ));

        assert!(
            matches!(
                patch_settings(State(state.clone()), Json(patch("deepgram"))).await,
                Err(Error::Unprocessable(_))
            ),
            "Deepgram needs a stored key"
        );
        state
            .keys
            .set(crate::meetings::DEEPGRAM_KEY_ID, "dg_test")
            .unwrap();
        let saved = patch_settings(State(state), Json(patch("deepgram")))
            .await
            .unwrap();
        assert_eq!(saved.voice_model(), "deepgram");
    }
}
