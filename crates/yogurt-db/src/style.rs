//! Voice style per kind of app: Casual, Normal, or Formal for each kind, and the apps the user moved to another kind.
//! It stays on this Mac as one `settings` row, since a phone runs other apps and may want other tones.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::Db;

const KEY: &str = "voice.style";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Messages,
    Work,
    Email,
    Other,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Messages, Kind::Work, Kind::Email, Kind::Other];
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Style {
    Casual,
    #[default]
    Normal,
    Formal,
}

/// `(bundle ID, name, kind)` for the apps yogurt sorts without being told.
pub const BUILT_IN: &[(&str, &str, Kind)] = &[
    ("com.apple.MobileSMS", "Messages", Kind::Messages),
    ("net.whatsapp.WhatsApp", "WhatsApp", Kind::Messages),
    (
        "org.whispersystems.signal-desktop",
        "Signal",
        Kind::Messages,
    ),
    ("com.tdesktop.Telegram", "Telegram", Kind::Messages),
    ("com.tinyspeck.slackmacgap", "Slack", Kind::Work),
    ("com.microsoft.teams2", "Microsoft Teams", Kind::Work),
    ("com.hnc.Discord", "Discord", Kind::Work),
    ("com.apple.mail", "Mail", Kind::Email),
    ("com.microsoft.Outlook", "Outlook", Kind::Email),
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Moved {
    /// Kept so the Style page can list a moved app after its dictations are gone.
    pub name: String,
    pub kind: Kind,
}

/// A kind missing from `styles` is Normal.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Setup {
    #[serde(default)]
    pub styles: BTreeMap<Kind, Style>,
    /// By bundle ID.
    #[serde(default)]
    pub moves: BTreeMap<String, Moved>,
}

impl Setup {
    pub fn style(&self, kind: Kind) -> Style {
        self.styles.get(&kind).copied().unwrap_or_default()
    }

    pub fn kind_of(&self, bundle_id: &str) -> Kind {
        self.moves
            .get(bundle_id)
            .map(|m| m.kind)
            .or_else(|| built_in(bundle_id).map(|(_, _, k)| *k))
            .unwrap_or(Kind::Other)
    }

    pub fn style_for(&self, bundle_id: Option<&str>) -> Style {
        self.style(bundle_id.map_or(Kind::Other, |b| self.kind_of(b)))
    }

    /// Moving an app back to its built-in kind forgets the move.
    pub fn move_app(&mut self, bundle_id: &str, name: &str, kind: Kind) {
        if built_in(bundle_id)
            .map(|(_, _, k)| *k)
            .unwrap_or(Kind::Other)
            == kind
        {
            self.moves.remove(bundle_id);
        } else {
            self.moves.insert(
                bundle_id.to_string(),
                Moved {
                    name: name.to_string(),
                    kind,
                },
            );
        }
    }
}

fn built_in(bundle_id: &str) -> Option<&'static (&'static str, &'static str, Kind)> {
    BUILT_IN.iter().find(|(b, _, _)| *b == bundle_id)
}

fn read(conn: &rusqlite::Connection) -> Result<Setup> {
    let raw: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key=?1", [KEY], |r| {
            r.get(0)
        })
        .optional()?;
    raw.map_or(Ok(Setup::default()), |r| {
        serde_json::from_str(&r).context("settings: `voice.style` is not a style setup")
    })
}

pub fn load(db: &Db) -> Result<Setup> {
    db.with_conn(read)
}

/// Reads, changes, and writes under one lock, so two quick clicks on the page never drop each other's change.
/// A row that no longer reads, say from a newer build's kind, is replaced rather than blocking every change.
pub fn update(db: &Db, change: impl FnOnce(&mut Setup)) -> Result<Setup> {
    db.with_conn(|conn| {
        let mut setup = read(conn).unwrap_or_default();
        change(&mut setup);
        conn.execute(
            "INSERT INTO settings(key, value) VALUES(?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![KEY, serde_json::to_string(&setup)?],
        )?;
        Ok(setup)
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct App {
    pub bundle_id: String,
    pub name: String,
    pub kind: Kind,
    pub moved: bool,
}

/// The built-in apps, the moved ones, and every app a saved dictation went to, by name.
pub fn apps(db: &Db, setup: &Setup) -> Result<Vec<App>> {
    let seen: Vec<(String, String)> = db.with_conn(|conn| {
        let mut stmt = conn.prepare_cached(
            "SELECT app_bundle_id, MAX(app_name) FROM dictations \
             WHERE app_bundle_id IS NOT NULL AND app_name IS NOT NULL GROUP BY app_bundle_id",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<rusqlite::Result<_>>()
    })?;
    let mut names: BTreeMap<String, String> = seen.into_iter().collect();
    for (b, n, _) in BUILT_IN {
        names.insert(b.to_string(), n.to_string());
    }
    for (b, m) in &setup.moves {
        names.entry(b.clone()).or_insert_with(|| m.name.clone());
    }
    let mut apps: Vec<App> = names
        .into_iter()
        .map(|(bundle_id, name)| App {
            kind: setup.kind_of(&bundle_id),
            moved: setup.moves.contains_key(&bundle_id),
            bundle_id,
            name,
        })
        .collect();
    apps.sort_by_cached_key(|a| a.name.to_lowercase());
    Ok(apps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apps_sort_by_the_built_in_list_until_moved() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(load(&db).unwrap(), Setup::default());
        let s = update(&db, |s| {
            s.styles.insert(Kind::Messages, Style::Casual);
            s.move_app("com.tinyspeck.slackmacgap", "Slack", Kind::Messages);
            s.move_app("com.apple.Notes", "Notes", Kind::Email);
        })
        .unwrap();
        assert_eq!(load(&db).unwrap(), s);
        assert_eq!(
            s.style_for(Some("com.tinyspeck.slackmacgap")),
            Style::Casual
        );
        assert_eq!(s.style_for(Some("com.apple.MobileSMS")), Style::Casual);
        assert_eq!(s.kind_of("com.apple.Notes"), Kind::Email);
        assert_eq!(s.kind_of("com.example.Unknown"), Kind::Other);
        assert_eq!(s.style_for(None), Style::Normal);

        let s = update(&db, |s| {
            s.move_app("com.tinyspeck.slackmacgap", "Slack", Kind::Work);
            s.move_app("com.apple.Notes", "Notes", Kind::Other);
        })
        .unwrap();
        assert!(s.moves.is_empty());
        assert_eq!(
            s.style_for(Some("com.tinyspeck.slackmacgap")),
            Style::Normal
        );
    }

    #[test]
    fn apps_lists_the_built_in_moved_and_dictated_apps_once() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn(|c| {
            c.execute_batch(
                "INSERT INTO dictations (id, created_at, app_bundle_id, app_name, hands_free, audio_ms, raw_text, text, raw_words, words, stt_engine, stt_ms, cleanup) VALUES
                 ('a', 1, 'com.apple.Notes', 'Notes', 0, 1, 'x', 'x', 1, 1, 'e', 1, 'off'),
                 ('b', 2, 'com.apple.mail', 'Mail', 0, 1, 'x', 'x', 1, 1, 'e', 1, 'off'),
                 ('c', 3, NULL, NULL, 0, 1, 'x', 'x', 1, 1, 'e', 1, 'off')",
            )
        })
        .unwrap();
        let s = update(&db, |s| s.move_app("dev.zed.Zed", "Zed", Kind::Work)).unwrap();
        let apps = apps(&db, &s).unwrap();
        assert_eq!(apps.len(), BUILT_IN.len() + 2);
        let find = |b: &str| apps.iter().find(|a| a.bundle_id == b).unwrap();
        assert_eq!(
            (find("com.apple.Notes").kind, find("com.apple.Notes").moved),
            (Kind::Other, false)
        );
        assert_eq!(
            (find("dev.zed.Zed").kind, find("dev.zed.Zed").moved),
            (Kind::Work, true)
        );
        assert_eq!(find("com.apple.mail").kind, Kind::Email);
    }
}
