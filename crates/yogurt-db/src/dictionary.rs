//! The Voice dictionary: words spelled the user's way, which cleanup is told to keep as written.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use crate::Db;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// Typed on the Dictionary page.
    Added,
    /// Picked from a dictation.
    Learned,
}

impl Source {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Source::Added => "added",
            Source::Learned => "learned",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub id: String,
    pub word: String,
    pub sounds_like: Option<String>,
    pub source: Source,
    /// Unix ms.
    pub created_at: i64,
    pub updated_at: i64,
}

const COLUMNS: &str = "id, word, sounds_like, source, created_at, updated_at";

fn row_to_entry(r: &Row) -> rusqlite::Result<Entry> {
    let source: String = r.get(3)?;
    Ok(Entry {
        id: r.get(0)?,
        word: r.get(1)?,
        sounds_like: r.get(2)?,
        source: match source.as_str() {
            "added" => Source::Added,
            "learned" => Source::Learned,
            _ => {
                return Err(rusqlite::Error::InvalidColumnType(
                    3,
                    format!("source {source:?}"),
                    rusqlite::types::Type::Text,
                ))
            }
        },
        created_at: r.get(4)?,
        updated_at: r.get(5)?,
    })
}

/// Most recently added or changed first.
pub fn list(db: &Db) -> Result<Vec<Entry>> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT {COLUMNS} FROM dictionary ORDER BY updated_at DESC, id DESC"
        ))?;
        let rows = stmt.query_map([], row_to_entry)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    })
}

/// A word already there in any case takes the new spelling, and the new sounds like when one is given, but keeps its source.
pub fn add(
    db: &Db,
    word: &str,
    sounds_like: Option<&str>,
    source: Source,
    now_ms: i64,
) -> Result<Entry> {
    db.with_conn(|conn| {
        Ok(conn.query_row(
            &format!(
                "INSERT INTO dictionary (key, {COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6) \
                 ON CONFLICT (key) DO UPDATE SET word = excluded.word, \
                 sounds_like = COALESCE(excluded.sounds_like, sounds_like), updated_at = excluded.updated_at \
                 RETURNING {COLUMNS}"
            ),
            params![
                word.to_lowercase(),
                ulid::Ulid::new().to_string(),
                word,
                sounds_like,
                source.as_str(),
                now_ms
            ],
            row_to_entry,
        )?)
    })
}

#[derive(Debug, PartialEq, Eq)]
pub enum Updated {
    Updated(Entry),
    /// Another entry is already spelled that way in some case.
    WordTaken,
    NotFound,
}

/// Respells a word and sets or clears its sounds like, keeping its source.
pub fn update(
    db: &Db,
    id: &str,
    word: &str,
    sounds_like: Option<&str>,
    now_ms: i64,
) -> Result<Updated> {
    let key = word.to_lowercase();
    db.with_conn(|conn| {
        let taken: Option<String> = conn
            .query_row("SELECT id FROM dictionary WHERE key = ?1", [&key], |r| {
                r.get(0)
            })
            .optional()?;
        if taken.is_some_and(|t| t != id) {
            return Ok(Updated::WordTaken);
        }
        let updated = conn
            .query_row(
                &format!(
                    "UPDATE dictionary SET key = ?1, word = ?2, sounds_like = ?3, updated_at = ?4 \
                     WHERE id = ?5 RETURNING {COLUMNS}"
                ),
                params![key, word, sounds_like, now_ms, id],
                row_to_entry,
            )
            .optional()?;
        Ok(updated.map_or(Updated::NotFound, Updated::Updated))
    })
}

/// `false` when no row had that id.
pub fn delete(db: &Db, id: &str) -> Result<bool> {
    Ok(db.with_conn(|conn| conn.execute("DELETE FROM dictionary WHERE id = ?1", [id]))? > 0)
}

/// The words a transcription engine is given, Learned first since each came from a dictation that came out wrong, then newest first.
/// An engine takes as many from the front as it can fit.
pub fn engine_words(entries: &[Entry]) -> Vec<String> {
    let mut sorted: Vec<&Entry> = entries.iter().collect();
    sorted.sort_by_key(|e| {
        (
            e.source != Source::Learned,
            std::cmp::Reverse((e.updated_at, e.id.as_str())),
        )
    });
    sorted.into_iter().map(|e| e.word.clone()).collect()
}

/// Lowercase runs of letters and digits, so "sherpa-onnx" and "Sherpa ONNX" both read as `sherpa onnx`.
fn tokens(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn distance(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(ca != cb))
                .min(prev[j + 1] + 1)
                .min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// `text` with each sounds like said in it written as its entry's word, so the paste never depends on a cleanup model following the dictionary.
/// A sounds like matches whole words in any case, said with or without its spaces, so "Zorb Ex" also catches "zorbex" and "Zorb-ex".
/// The longest match wins, then the first entry, and an entry's word already written stays as it is, so respelling twice changes nothing more.
pub fn respell(entries: &[Entry], text: &str) -> String {
    let forms: Vec<(&str, String, usize)> = entries
        .iter()
        .filter_map(|e| {
            let parts = tokens(e.sounds_like.as_deref()?);
            (!parts.is_empty()).then(|| (e.word.as_str(), parts.concat(), parts.len()))
        })
        .collect();
    let mut words: Vec<(usize, usize, String)> = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices().chain([(text.len(), ' ')]) {
        match (c.is_alphanumeric(), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                words.push((s, i, text[s..i].to_lowercase()));
                start = None;
            }
            _ => {}
        }
    }
    // A run spans only spaces and hyphens, so a form never matches across a sentence or a comma.
    let joined = |a: usize, b: usize| {
        text[words[a].1..words[b].0]
            .chars()
            .all(|c| c == ' ' || c == '-')
    };
    // The byte after `word` when the text at `at` reads it in any case, so "Node.js" is never read as "node" plus more.
    let reads = |at: usize, word: &str| {
        let end = text[at..]
            .char_indices()
            .nth(word.chars().count())
            .map_or(text.len(), |(b, _)| at + b);
        (text[at..end].to_lowercase() == word.to_lowercase()
            && !text[end..].starts_with(char::is_alphanumeric))
        .then_some(end)
    };
    let (mut out, mut copied, mut i) = (String::with_capacity(text.len()), 0, 0);
    while i < words.len() {
        if let Some(end) = forms.iter().find_map(|(word, ..)| reads(words[i].0, word)) {
            while i < words.len() && words[i].0 < end {
                i += 1;
            }
            continue;
        }
        let hit = forms
            .iter()
            .enumerate()
            .filter_map(|(k, (word, form, parts))| {
                let mut run = String::new();
                (i..words.len().min(i + parts + 1))
                    .take_while(|&j| j == i || joined(j - 1, j))
                    .find_map(|j| {
                        run.push_str(&words[j].2);
                        (run == *form).then_some((j, k, *word))
                    })
            })
            .min_by_key(|(j, k, _)| (std::cmp::Reverse(*j), *k));
        match hit {
            Some((last, _, word)) => {
                out.push_str(&text[copied..words[i].0]);
                out.push_str(word);
                copied = words[last].1;
                i = last + 1;
            }
            None => i += 1,
        }
    }
    out.push_str(&text[copied..]);
    out
}

/// The entries whose word or sounds like is within a few edits of some run of words in `text`, closest first, at most `cap`.
/// Words and runs compare with their spaces and punctuation gone, so "bell view" said matches "Bellevue" and "sherpa onyx" matches "sherpa-onnx".
/// Up to 4 letters a match must be exact, so "Mark" never catches "park". Longer ones allow one edit per 4 letters, up to 3.
pub fn close_to<'a>(entries: &'a [Entry], text: &str, cap: usize) -> Vec<&'a Entry> {
    let said = tokens(text);
    let forms = |e: &'a Entry| {
        [Some(e.word.as_str()), e.sounds_like.as_deref()]
            .into_iter()
            .flatten()
    };
    // A run of one more or one fewer word than a form covers a space said or dropped.
    let sizes = |parts: usize| parts.saturating_sub(1).max(1)..=parts + 1;
    let runs: HashMap<usize, HashSet<Vec<char>>> = entries
        .iter()
        .flat_map(forms)
        .flat_map(|f| sizes(tokens(f).len()))
        .collect::<HashSet<_>>()
        .into_iter()
        .map(|n| {
            let joined = said.windows(n).map(|w| w.concat().chars().collect());
            (n, joined.collect())
        })
        .collect();
    let mut hits: Vec<(usize, usize)> = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        let best = forms(e)
            .filter_map(|form| {
                let parts = tokens(form);
                let joined: Vec<char> = parts.concat().chars().collect();
                let allowed = if joined.len() <= 4 {
                    0
                } else {
                    (joined.len() / 4).min(3)
                };
                sizes(parts.len())
                    .flat_map(|n| &runs[&n])
                    .filter(|run| run.len().abs_diff(joined.len()) <= allowed)
                    .map(|run| distance(&joined, run))
                    .filter(|d| *d <= allowed)
                    .min()
            })
            .min();
        if let Some(d) = best {
            hits.push((d, i));
        }
    }
    hits.sort();
    hits.into_iter()
        .take(cap)
        .map(|(_, i)| &entries[i])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(word: &str, sounds_like: Option<&str>) -> Entry {
        Entry {
            id: word.into(),
            word: word.into(),
            sounds_like: sounds_like.map(str::to_string),
            source: Source::Added,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn words(hits: Vec<&Entry>) -> Vec<&str> {
        hits.into_iter().map(|e| e.word.as_str()).collect()
    }

    #[test]
    fn re_adding_a_word_in_another_case_respells_it_and_keeps_its_source_and_sounds_like() {
        let db = Db::open_in_memory().unwrap();
        let first = add(&db, "Iphone", Some("eye phone"), Source::Learned, 1).unwrap();
        let again = add(&db, "iPhone", None, Source::Added, 2).unwrap();
        let emile = add(&db, "Émile", None, Source::Added, 2).unwrap();
        assert_eq!(
            add(&db, "émile", None, Source::Added, 2).unwrap().id,
            emile.id
        );
        delete(&db, &emile.id).unwrap();
        assert_eq!(again.id, first.id);
        assert_eq!(again.word, "iPhone");
        assert_eq!(again.sounds_like.as_deref(), Some("eye phone"));
        assert_eq!(again.source, Source::Learned);
        assert_eq!((again.created_at, again.updated_at), (1, 2));
        add(&db, "Niamh", None, Source::Added, 3).unwrap();
        assert_eq!(
            list(&db)
                .unwrap()
                .iter()
                .map(|e| e.word.as_str())
                .collect::<Vec<_>>(),
            ["Niamh", "iPhone"]
        );
        assert!(delete(&db, &first.id).unwrap());
        assert!(!delete(&db, &first.id).unwrap());
        assert_eq!(list(&db).unwrap().len(), 1);
    }

    #[test]
    fn update_respells_clears_sounds_like_and_refuses_another_entrys_spelling() {
        let db = Db::open_in_memory().unwrap();
        let niamh = add(&db, "Niamh", Some("neeve"), Source::Learned, 1).unwrap();
        add(&db, "Kubrick", None, Source::Added, 2).unwrap();
        let Updated::Updated(e) = update(&db, &niamh.id, "Neve", None, 3).unwrap() else {
            panic!("not updated");
        };
        assert_eq!((e.word.as_str(), e.sounds_like), ("Neve", None));
        assert_eq!(e.source, Source::Learned);
        assert_eq!((e.created_at, e.updated_at), (1, 3));
        assert!(matches!(
            update(&db, &niamh.id, "NEVE", Some("nev"), 4).unwrap(),
            Updated::Updated(Entry { ref word, .. }) if word == "NEVE"
        ));
        assert_eq!(
            update(&db, &niamh.id, "kubrick", None, 5).unwrap(),
            Updated::WordTaken
        );
        assert_eq!(
            update(&db, "nope", "Zed", None, 5).unwrap(),
            Updated::NotFound
        );
        assert_ne!(
            add(&db, "Niamh", None, Source::Added, 6).unwrap().id,
            niamh.id
        );
        assert_eq!(
            add(&db, "neve", None, Source::Added, 7).unwrap().id,
            niamh.id
        );
        assert_eq!(list(&db).unwrap().len(), 3);
    }

    #[test]
    fn close_to_matches_a_spelling_or_a_sounds_like_near_what_was_said() {
        let dict = [
            entry("Kubrick", Some("cubic")),
            entry("Niamh", Some("neeve")),
            entry("Bellevue", Some("bell view")),
            entry("sherpa-onnx", Some("sherpa onyx")),
            entry("Yogurt Cloud", None),
            entry("Al", None),
        ];
        assert_eq!(
            words(close_to(&dict, "Loved the cubrick reference.", 10)),
            ["Kubrick"]
        );
        assert_eq!(
            words(close_to(&dict, "ask neve about the bell view office", 10)),
            ["Bellevue", "Niamh"]
        );
        assert_eq!(
            words(close_to(
                &dict,
                "we moved to Sherpa onyx and yogurtcloud",
                10
            )),
            ["sherpa-onnx", "Yogurt Cloud"]
        );
        assert_eq!(words(close_to(&dict, "a pal said al", 10)), ["Al"]);
        assert!(close_to(&dict, "a pal said hi", 10).is_empty());
        let mark = [entry("Mark", None)];
        assert!(close_to(&mark, "park the car", 10).is_empty());
        assert_eq!(close_to(&mark, "ask mark", 10).len(), 1);
        let supabase = [entry("Supabase", None)];
        assert_eq!(close_to(&supabase, "it lives in super base", 10).len(), 1);
        assert!(close_to(&dict, "let's meet on Tuesday at three", 10).is_empty());
        assert_eq!(close_to(&dict, "cubic neeve bellevue", 2).len(), 2);
    }

    #[test]
    fn respell_writes_each_sounds_like_said_as_its_word() {
        let dict = [
            entry("Zorbex Lab", Some("Zorb Ex")),
            entry("Niamh", Some("neeve")),
            entry("Tavimek", None),
            entry("Qorv.js", Some("qorv")),
            entry("C++", Some("see plus plus")),
            entry("Will", Some("wil")),
        ];
        let r = |text: &str| respell(&dict, text);
        assert_eq!(r("ask Zorb Ex to check it."), "ask Zorbex Lab to check it.");
        assert_eq!(r("zorbex, then zorb-ex"), "Zorbex Lab, then Zorbex Lab");
        assert_eq!(r("Neeve's deck, NEEVE?"), "Niamh's deck, Niamh?");
        assert_eq!(r("zorb. Ex"), "zorb. Ex");
        assert_eq!(r("zorbexy neevey"), "zorbexy neevey");
        assert_eq!(r("tavimek"), "tavimek");
        assert_eq!(r("send it to zorbex lab"), "send it to zorbex lab");
        assert_eq!(
            r("use qorv, see plus plus, or plan c"),
            "use Qorv.js, C++, or plan c"
        );
        assert_eq!(r("I will ask wil"), "I will ask Will");
        for said in ["zorbex, Zorbex Lab", "qorv and Qorv.js", "see plus plus"] {
            assert_eq!(r(&r(said)), r(said));
        }
        let ties = [
            entry("Q", Some("zorb")),
            entry("Zorbex Lab", Some("zorb ex")),
            entry("Neve", Some("neeve")),
            entry("Niamh", Some("neeve")),
        ];
        assert_eq!(respell(&ties, "zorb ex and zorb"), "Zorbex Lab and Q");
        assert_eq!(respell(&ties, "ask neeve"), "ask Neve");
    }

    #[test]
    fn engine_words_put_learned_first_then_newest() {
        let at = |word: &str, source, updated_at| Entry {
            source,
            updated_at,
            ..entry(word, None)
        };
        let dict = [
            at("Zorbal", Source::Added, 3),
            at("Tavimek", Source::Learned, 1),
            at("Marivex", Source::Added, 1),
            at("Telian", Source::Learned, 2),
        ];
        assert_eq!(
            engine_words(&dict),
            ["Telian", "Tavimek", "Zorbal", "Marivex"]
        );
    }

    #[test]
    fn distance_counts_edits() {
        let c = |s: &str| s.chars().collect::<Vec<_>>();
        assert_eq!(distance(&c("kitten"), &c("sitting")), 3);
        assert_eq!(distance(&c(""), &c("abc")), 3);
        assert_eq!(distance(&c("same"), &c("same")), 0);
    }
}
