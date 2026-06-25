//! `providers` rows describe an LLM endpoint or CLI the user configured. API keys live in `~/.yogurt/keys.json` via [`crate::keys`], never here.
//! A partial unique index in `V001__initial.sql` enforces a single active LLM provider.

use crate::Db;
use anyhow::Result;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use ulid::Ulid;

/// Built-in provider presets, shown as chips in the Model section's "CLONE A PRESET" row.
pub struct Preset {
    pub name: &'static str,
    pub base_url: &'static str,
    pub default_model: &'static str,
    /// Popular model ids for the Settings UI's MODEL `<datalist>`, so a freshly-cloned provider has suggestions before a key is pasted.
    /// Includes `default_model`; the user can type any model.
    ///
    /// Lists go stale, so the Settings page offers `Refresh` (live `GET {base_url}/models`, needs a stored key) and a `docs_url` link.
    pub models: &'static [&'static str],
    /// Public URL of the provider's "available models" page, shown next to the MODEL field as a Refresh fallback and for models the static list lacks.
    /// Empty for a `cli` preset.
    pub docs_url: &'static str,
    /// `"http"` (`OpenAiCompatClient` against `base_url`, a stored API key) or `"cli"` (`yogurt_llm::CliClient`, no key or base URL, see [`Adapter::CLI`]).
    pub adapter: &'static str,
    /// `cli`-adapter only: the suggested default `--model`. The Settings UI seeds `cli_model` with it the first time a `Test` proves the CLI connects (not on clone).
    /// `"haiku"` for claude since enhance/chat are simple extraction calls.
    /// `"auto"` for cursor-agent, the one model that works on every plan (free tiers get `ActionRequiredError: Named models unavailable` otherwise).
    /// Empty only for `http` presets.
    pub default_cli_model: &'static str,
    /// The `YOGURT_*_API_KEY` env var that seeds this preset on boot, or empty for none.
    pub env_var: &'static str,
}

/// `Preset::adapter` / `Provider::adapter` values. Consts rather than an enum because SQLite has no enum type and the REST DTOs pass `adapter` as a string.
/// They are the one place the literals are spelled, so a typo fails to compile.
pub mod adapter {
    pub const HTTP: &str = "http";
    pub const CLI: &str = "cli";
}

/// Built-in presets, in UI chip order: cloud majors, local runtimes, the aggregator, then local agent CLIs.
///
/// Every `adapter: "http"` entry must speak the OpenAI `/chat/completions` shape, since `OpenAiCompatClient` is the only HTTP client `yogurt-llm` ships.
/// For an `adapter: "cli"` entry `base_url` is empty and `default_model` holds the CLI program id `yogurt_llm::CliProgram::parse` expects ("claude" | "cursor-agent" | "opencode" | "codex" | "copilot").
pub const PRESETS: &[Preset] = &[
    Preset {
        name: "Minimax",
        base_url: "https://api.minimax.io/v1",
        default_model: "MiniMax-M3",
        models: &["MiniMax-M3", "MiniMax-M3.1-Flash"],
        docs_url: "https://platform.MiniMax.io/docs/api-reference",
        adapter: adapter::HTTP,
        default_cli_model: "",
        env_var: "YOGURT_MINIMAX_API_KEY",
    },
    Preset {
        name: "OpenAI",
        base_url: "https://api.openai.com/v1",
        default_model: "gpt-4o-mini",
        models: &[
            "gpt-4o",
            "gpt-4o-mini",
            "gpt-4.1",
            "gpt-4.1-mini",
            "o4-mini",
            "o3",
        ],
        docs_url: "https://platform.openai.com/docs/models",
        adapter: adapter::HTTP,
        default_cli_model: "",
        env_var: "YOGURT_OPENAI_API_KEY",
    },
    // Google exposes Gemini through an OpenAI-compatible shim, not the native `generativeLanguage` REST shape.
    // `OpenAiCompatClient` trims the trailing slash before appending `/chat/completions`.
    Preset {
        name: "Google Gemini",
        base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
        default_model: "gemini-2.5-flash",
        models: &[
            "gemini-2.5-pro",
            "gemini-2.5-flash",
            "gemini-2.0-flash",
            "gemini-2.0-flash-lite",
            "gemini-1.5-pro",
            "gemini-1.5-flash",
        ],
        docs_url: "https://ai.google.dev/gemini/docs/models",
        adapter: adapter::HTTP,
        default_cli_model: "",
        env_var: "YOGURT_GEMINI_API_KEY",
    },
    Preset {
        name: "DeepSeek",
        base_url: "https://api.deepseek.com/v1",
        default_model: "deepseek-chat",
        models: &["deepseek-chat", "deepseek-reasoner"],
        docs_url: "https://api-docs.deepseek.com/quick_start/pricing",
        adapter: adapter::HTTP,
        default_cli_model: "",
        env_var: "YOGURT_DEEPSEEK_API_KEY",
    },
    Preset {
        name: "Celeris",
        base_url: "https://inference.celeris.ai/celeris-1/v1",
        default_model: "celeris-1",
        models: &["celeris-1"],
        docs_url: "https://docs.celeris.ai/models",
        adapter: adapter::HTTP,
        default_cli_model: "",
        env_var: "YOGURT_CELERIS_API_KEY",
    },
    Preset {
        name: "Ollama (local)",
        base_url: "http://localhost:11434/v1",
        default_model: "llama3.2",
        // Hints only: models depend on what the user has pulled locally. Refresh (live `GET /v1/models`) wins once there is a real list.
        models: &["llama3.2", "llama3.1", "mistral", "qwen2.5", "gemma2"],
        docs_url: "https://ollama.com/library",
        adapter: adapter::HTTP,
        default_cli_model: "",
        env_var: "",
    },
    Preset {
        name: "LM Studio (local)",
        base_url: "http://localhost:1234/v1",
        default_model: "",
        // Empty: LM Studio has no well-known model names, so Refresh is the only way this datalist gets populated.
        models: &[],
        docs_url: "https://lmstudio.ai/models",
        adapter: adapter::HTTP,
        default_cli_model: "",
        env_var: "",
    },
    Preset {
        name: "OpenRouter",
        base_url: "https://openrouter.ai/api/v1",
        default_model: "anthropic/claude-3.5-sonnet",
        models: &[
            "anthropic/claude-3.5-sonnet",
            "anthropic/claude-3.5-haiku",
            "openai/gpt-4o-mini",
            "google/gemini-2.5-flash",
            "meta-llama/llama-3.1-70b-instruct",
        ],
        docs_url: "https://openrouter.ai/models",
        adapter: adapter::HTTP,
        default_cli_model: "",
        env_var: "YOGURT_OPENROUTER_API_KEY",
    },
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub is_active: bool,
    pub created_at: i64,
    /// `"http"` or `"cli"`, see [`adapter`].
    pub adapter: String,
    /// `cli`-adapter only: the `--model` value to pass to the CLI. Empty
    /// means "use the CLI's own default model". Meaningless for `http`.
    pub cli_model: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewProvider {
    pub name: String,
    pub base_url: String,
    pub model: String,
    /// `"http"` or `"cli"`. Defaults to `"http"` so older three-field request bodies keep working.
    #[serde(default = "default_adapter")]
    pub adapter: String,
    #[serde(default)]
    pub cli_model: String,
}

fn default_adapter() -> String {
    adapter::HTTP.to_string()
}

/// Insert a new provider (kind='llm', is_active=0) and return its ULID.
pub fn insert(db: &Db, p: NewProvider) -> Result<String> {
    let id = Ulid::new().to_string();
    let now = unix_millis();
    db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO providers (id, name, base_url, model, kind, is_active, created_at, adapter, cli_model) \
             VALUES (?1, ?2, ?3, ?4, 'llm', 0, ?5, ?6, ?7)",
            params![id, p.name, p.base_url, p.model, now, p.adapter, p.cli_model],
        )
    })?;
    Ok(id)
}

/// List all LLM providers ordered by creation time (oldest first).
pub fn list(db: &Db) -> Result<Vec<Provider>> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, base_url, model, is_active, created_at, adapter, cli_model \
             FROM providers WHERE kind='llm' ORDER BY created_at ASC",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Provider {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    base_url: r.get(2)?,
                    model: r.get(3)?,
                    is_active: r.get::<_, i64>(4)? != 0,
                    created_at: r.get(5)?,
                    adapter: r.get(6)?,
                    cli_model: r.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok::<_, rusqlite::Error>(rows)
    })
    .map_err(Into::into)
}

/// List all LLM provider names. Cheaper than `list` when only the names matter
/// (e.g. the bootstrap idempotence check).
pub fn list_names(db: &Db) -> Result<Vec<String>> {
    db.with_conn(|conn| {
        let mut stmt =
            conn.prepare("SELECT name FROM providers WHERE kind='llm' ORDER BY created_at ASC")?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok::<_, rusqlite::Error>(rows)
    })
    .map_err(Into::into)
}

pub fn active(db: &Db) -> Result<Option<Provider>> {
    db.with_conn(|conn| {
        conn.query_row(
            "SELECT id, name, base_url, model, is_active, created_at, adapter, cli_model \
             FROM providers WHERE kind='llm' AND is_active=1",
            [],
            |r| {
                Ok(Provider {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    base_url: r.get(2)?,
                    model: r.get(3)?,
                    is_active: true,
                    created_at: r.get(5)?,
                    adapter: r.get(6)?,
                    cli_model: r.get(7)?,
                })
            },
        )
        .optional()
    })
    .map_err(Into::into)
}

/// Atomically deactivate all LLM providers and activate `id`.
/// Runs in `BEGIN IMMEDIATE` so a failure rolls back; otherwise the partial unique index would reject the second UPDATE and leave no active provider.
pub fn set_active(db: &Db, id: &str) -> Result<()> {
    db.with_conn(|conn| -> rusqlite::Result<()> {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let res: rusqlite::Result<()> = (|| {
            conn.execute("UPDATE providers SET is_active=0 WHERE kind='llm'", [])?;
            let updated = conn.execute(
                "UPDATE providers SET is_active=1 WHERE id=?1 AND kind='llm'",
                params![id],
            )?;
            if updated == 0 {
                return Err(rusqlite::Error::QueryReturnedNoRows);
            }
            Ok(())
        })();
        match res {
            Ok(()) => {
                conn.execute_batch("COMMIT")?;
                Ok(())
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    })?;
    Ok(())
}

/// Update a provider's mutable fields. Does NOT touch `is_active`.
pub fn update(
    db: &Db,
    id: &str,
    name: &str,
    base_url: &str,
    model: &str,
    cli_model: &str,
) -> Result<()> {
    db.with_conn(|conn| {
        conn.execute(
            "UPDATE providers SET name=?2, base_url=?3, model=?4, cli_model=?5 WHERE id=?1",
            params![id, name, base_url, model, cli_model],
        )
    })?;
    Ok(())
}

/// Delete a provider row. Caller is responsible for deleting the matching
/// key-file entry (see `keys::ApiKeyStore::delete`).
pub fn delete(db: &Db, id: &str) -> Result<()> {
    db.with_conn(|conn| conn.execute("DELETE FROM providers WHERE id=?1", params![id]))?;
    Ok(())
}

fn unix_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
