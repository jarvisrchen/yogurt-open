//! Voice snippets: a short spoken cue that pastes a longer text.

use anyhow::Result;
use rusqlite::{params, OptionalExtension, Row};
use serde::Serialize;

use crate::Db;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Snippet {
    pub id: String,
    pub cue: String,
    pub text: String,
    /// Unix ms.
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Saved {
    Saved(Snippet),
    /// Another snippet has a cue that reads the same.
    CueTaken,
    NotFound,
}

const COLUMNS: &str = "id, cue, text, created_at, updated_at";

fn row_to_snippet(r: &Row) -> rusqlite::Result<Snippet> {
    Ok(Snippet {
        id: r.get(0)?,
        cue: r.get(1)?,
        text: r.get(2)?,
        created_at: r.get(3)?,
        updated_at: r.get(4)?,
    })
}

/// A cue as it is compared: lowercase letters and digits only, so "My address." said matches the cue "my address".
pub fn key(cue: &str) -> String {
    cue.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Newest first. An edit keeps a snippet's place.
pub fn list(db: &Db) -> Result<Vec<Snippet>> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT {COLUMNS} FROM snippets ORDER BY created_at DESC, id DESC"
        ))?;
        let rows = stmt.query_map([], row_to_snippet)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    })
}

/// Adds a snippet, or with `id` changes that one.
pub fn save(db: &Db, id: Option<&str>, cue: &str, text: &str, now_ms: i64) -> Result<Saved> {
    let k = key(cue);
    db.with_conn(|conn| {
        let taken: Option<String> = conn
            .query_row("SELECT id FROM snippets WHERE key = ?1", [&k], |r| r.get(0))
            .optional()?;
        if taken.is_some_and(|t| Some(t.as_str()) != id) {
            return Ok(Saved::CueTaken);
        }
        let saved = match id {
            None => Some(conn.query_row(
                &format!(
                    "INSERT INTO snippets (key, {COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?5) RETURNING {COLUMNS}"
                ),
                params![k, ulid::Ulid::new().to_string(), cue, text, now_ms],
                row_to_snippet,
            )?),
            Some(id) => conn
                .query_row(
                    &format!(
                        "UPDATE snippets SET key = ?1, cue = ?2, text = ?3, updated_at = ?4 WHERE id = ?5 \
                         RETURNING {COLUMNS}"
                    ),
                    params![k, cue, text, now_ms, id],
                    row_to_snippet,
                )
                .optional()?,
        };
        Ok(saved.map_or(Saved::NotFound, Saved::Saved))
    })
}

/// `false` when no row had that id.
pub fn delete(db: &Db, id: &str) -> Result<bool> {
    Ok(db.with_conn(|conn| conn.execute("DELETE FROM snippets WHERE id = ?1", [id]))? > 0)
}

/// The snippet whose cue is the whole of what was said, ignoring case, spaces, and punctuation.
pub fn whole<'a>(snippets: &'a [Snippet], said: &str) -> Option<&'a Snippet> {
    let said = key(said);
    snippets.iter().find(|s| key(&s.cue) == said)
}

fn words(s: &str) -> impl Iterator<Item = &str> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
}

/// The snippets whose cue was said somewhere in `text` as a run of whole words, compared as [`whole`] does,
/// so "email sig" said finds the cue "e-mail sig".
pub fn said<'a>(snippets: &'a [Snippet], text: &str) -> Vec<&'a Snippet> {
    let said: Vec<String> = words(text).map(key).collect();
    snippets
        .iter()
        .filter(|s| {
            let cue = key(&s.cue);
            // One run longer than the cue's own words covers a space said where the cue has none.
            let longest = words(&s.cue).count() + 1;
            !cue.is_empty() && (1..=longest).any(|n| said.windows(n).any(|w| w.concat() == cue))
        })
        .collect()
}

const MARKER: &str = "[[snippet:";

/// Swaps each `[[snippet:<cue>]]` the cleanup model wrote for that snippet's text, and drops one that names no snippet.
pub fn expand(cleaned: &str, snippets: &[Snippet]) -> String {
    let mut out = String::new();
    let mut rest = cleaned;
    while let Some(start) = rest.find(MARKER) {
        let Some(len) = rest[start..].find("]]") else {
            break;
        };
        let found = whole(snippets, &rest[start + MARKER.len()..start + len]);
        out.push_str(&rest[..start]);
        rest = &rest[start + len + 2..];
        match found {
            Some(s) => out.push_str(&s.text),
            None => {
                // "Thanks [[snippet:x]]." reads "Thanks." and "a [[snippet:x]] b" reads "a b".
                out.truncate(out.trim_end_matches(' ').len());
                rest = rest.trim_start_matches(' ');
                if !out.is_empty()
                    && !out.ends_with('\n')
                    && rest.starts_with(char::is_alphanumeric)
                {
                    out.push(' ');
                }
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snippet(cue: &str, text: &str) -> Snippet {
        Snippet {
            id: cue.into(),
            cue: cue.into(),
            text: text.into(),
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn save_adds_edits_and_refuses_a_cue_another_snippet_has() {
        let db = Db::open_in_memory().unwrap();
        let Saved::Saved(address) =
            save(&db, None, "my address", "221 Pine St\nPortland", 1).unwrap()
        else {
            panic!()
        };
        let Saved::Saved(link) =
            save(&db, None, "calendar link", "https://cal.example.com", 2).unwrap()
        else {
            panic!()
        };
        assert_eq!(
            save(&db, None, "My Address!", "x", 3).unwrap(),
            Saved::CueTaken
        );
        assert_eq!(
            save(&db, Some(&link.id), "my-address", "x", 3).unwrap(),
            Saved::CueTaken
        );
        assert_eq!(
            save(&db, Some("gone"), "new cue", "x", 3).unwrap(),
            Saved::NotFound
        );
        let Saved::Saved(edited) = save(
            &db,
            Some(&address.id),
            "My address",
            "221 Pine St, Apt 4",
            4,
        )
        .unwrap() else {
            panic!()
        };
        assert_eq!(
            (edited.id.as_str(), edited.created_at, edited.updated_at),
            (address.id.as_str(), 1, 4)
        );
        assert_eq!(edited.text, "221 Pine St, Apt 4");
        assert_eq!(
            list(&db)
                .unwrap()
                .iter()
                .map(|s| s.cue.as_str())
                .collect::<Vec<_>>(),
            ["calendar link", "My address"]
        );
        assert!(delete(&db, &link.id).unwrap());
        assert!(!delete(&db, &link.id).unwrap());
        assert_eq!(list(&db).unwrap().len(), 1);
    }

    #[test]
    fn whole_matches_a_cue_said_alone_ignoring_case_and_punctuation() {
        let all = [
            snippet("my address", "221 Pine St"),
            snippet("intro reply", "Thanks!"),
        ];
        assert_eq!(whole(&all, "My address.").unwrap().text, "221 Pine St");
        assert_eq!(whole(&all, "  intro, reply!").unwrap().text, "Thanks!");
        assert!(whole(&all, "send my address").is_none());
        assert!(whole(&all, "address").is_none());
    }

    #[test]
    fn said_finds_cues_spoken_as_whole_words() {
        let all = [
            snippet("my address", "x"),
            snippet("art", "y"),
            snippet("intro reply", "z"),
            snippet("e-mail sig", "w"),
        ];
        let cues = |t: &str| {
            said(&all, t)
                .iter()
                .map(|s| s.cue.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(cues("Send it to My-Address, thanks."), ["my address"]);
        assert_eq!(cues("let's start the intro, reply later"), ["intro reply"]);
        assert!(cues("my new address").is_empty());
        assert_eq!(cues("art"), ["art"]);
        assert_eq!(cues("add my email sig here"), ["e-mail sig"]);
    }

    #[test]
    fn expand_swaps_markers_for_text_and_drops_unknown_ones() {
        let all = [
            snippet("my address", "221 Pine St\nPortland, OR"),
            snippet("sign off", "Best,\nSam"),
        ];
        assert_eq!(
            expand("Ship it to [[snippet:my address]] please.", &all),
            "Ship it to 221 Pine St\nPortland, OR please."
        );
        assert_eq!(
            expand("Thanks!\n[[snippet:Sign Off]]", &all),
            "Thanks!\nBest,\nSam"
        );
        assert_eq!(expand("Thanks [[snippet:nope]].", &all), "Thanks.");
        assert_eq!(expand("a [[snippet:nope]] b", &all), "a b");
        assert_eq!(expand("[[snippet:nope]] Hi", &all), "Hi");
        assert_eq!(expand("No markers here.", &all), "No markers here.");
        assert_eq!(
            expand("Broken [[snippet:my address", &all),
            "Broken [[snippet:my address"
        );
    }
}
