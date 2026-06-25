//! A device's Voice setup as Yogurt Cloud holds it: the dictionary and snippets, without ids or times.
//! Copying one in adds what this device lacks and takes the copy's version of a word or cue both have.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::dictionary::{self, Source};
use crate::snippets;
use crate::Db;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Word {
    pub word: String,
    #[serde(default)]
    pub sounds_like: Option<String>,
    pub source: Source,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cue {
    pub cue: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Setup {
    pub v: u32,
    pub dictionary: Vec<Word>,
    pub snippets: Vec<Cue>,
}

/// What a copy changes. The counts of what stays cover entries only on this device.
#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub struct Plan {
    pub add_words: usize,
    pub add_snippets: usize,
    /// The copy's version of each word this device spells or hears differently.
    pub replace_words: Vec<Word>,
    /// The copy's cue for each snippet whose cue or text differs here.
    pub replace_snippets: Vec<String>,
    pub keep_words: usize,
    pub keep_snippets: usize,
}

pub fn export(db: &Db) -> Result<Setup> {
    Ok(Setup {
        v: 1,
        dictionary: dictionary::list(db)?
            .into_iter()
            .map(|e| Word {
                word: e.word,
                sounds_like: e.sounds_like,
                source: e.source,
            })
            .collect(),
        snippets: snippets::list(db)?
            .into_iter()
            .map(|s| Cue {
                cue: s.cue,
                text: s.text,
            })
            .collect(),
    })
}

/// The words and snippets to write, keyed as the tables key them. A key the copy repeats counts once, as its first entry.
struct Changes<'a> {
    plan: Plan,
    words: Vec<(String, &'a Word)>,
    snippets: Vec<(String, &'a Cue)>,
}

fn changes<'a>(db: &Db, setup: &'a Setup) -> Result<Changes<'a>> {
    let local_words: HashMap<String, (String, Option<String>)> = dictionary::list(db)?
        .into_iter()
        .map(|e| (e.word.to_lowercase(), (e.word, e.sounds_like)))
        .collect();
    let local_snippets: HashMap<String, (String, String)> = snippets::list(db)?
        .into_iter()
        .map(|s| (snippets::key(&s.cue), (s.cue, s.text)))
        .collect();
    let mut plan = Plan::default();
    let mut words = Vec::new();
    let mut seen = HashSet::new();
    for w in &setup.dictionary {
        let key = w.word.to_lowercase();
        if !seen.insert(key.clone()) {
            continue;
        }
        match local_words.get(&key) {
            None => plan.add_words += 1,
            Some((word, sounds_like)) if *word == w.word && *sounds_like == w.sounds_like => {
                continue
            }
            Some(_) => plan.replace_words.push(w.clone()),
        }
        words.push((key, w));
    }
    plan.keep_words = local_words.keys().filter(|k| !seen.contains(*k)).count();
    let mut cues = Vec::new();
    seen.clear();
    for s in &setup.snippets {
        let key = snippets::key(&s.cue);
        if !seen.insert(key.clone()) {
            continue;
        }
        match local_snippets.get(&key) {
            None => plan.add_snippets += 1,
            Some((cue, text)) if *cue == s.cue && *text == s.text => continue,
            Some(_) => plan.replace_snippets.push(s.cue.clone()),
        }
        cues.push((key, s));
    }
    plan.keep_snippets = local_snippets.keys().filter(|k| !seen.contains(*k)).count();
    Ok(Changes {
        plan,
        words,
        snippets: cues,
    })
}

pub fn preview(db: &Db, setup: &Setup) -> Result<Plan> {
    Ok(changes(db, setup)?.plan)
}

/// Applies [`preview`]'s plan in one transaction. A replaced entry keeps its id and its source.
pub fn copy(db: &Db, setup: &Setup, now_ms: i64) -> Result<Plan> {
    let Changes {
        plan,
        words,
        snippets,
    } = changes(db, setup)?;
    db.with_conn(|conn| {
        let tx = conn.unchecked_transaction()?;
        // One ms apart in the copy's order, newest first, so the lists read in the order the uploading device showed them.
        for (i, (key, w)) in words.into_iter().enumerate() {
            tx.execute(
                "INSERT INTO dictionary (key, id, word, sounds_like, source, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6) \
                 ON CONFLICT (key) DO UPDATE SET word = excluded.word, sounds_like = excluded.sounds_like, \
                 updated_at = excluded.updated_at",
                params![
                    key,
                    ulid::Ulid::new().to_string(),
                    w.word,
                    w.sounds_like,
                    w.source.as_str(),
                    now_ms - i as i64
                ],
            )?;
        }
        for (i, (key, s)) in snippets.into_iter().enumerate() {
            tx.execute(
                "INSERT INTO snippets (key, id, cue, text, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5) \
                 ON CONFLICT (key) DO UPDATE SET cue = excluded.cue, text = excluded.text, updated_at = excluded.updated_at",
                params![key, ulid::Ulid::new().to_string(), s.cue, s.text, now_ms - i as i64],
            )?;
        }
        tx.commit()?;
        Ok(plan)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(w: &str, sounds_like: Option<&str>) -> Word {
        Word {
            word: w.into(),
            sounds_like: sounds_like.map(str::to_string),
            source: Source::Learned,
        }
    }

    fn cue(c: &str, t: &str) -> Cue {
        Cue {
            cue: c.into(),
            text: t.into(),
        }
    }

    #[test]
    fn copy_adds_what_is_missing_takes_the_copys_version_and_keeps_the_rest() {
        let db = Db::open_in_memory().unwrap();
        let kept = dictionary::add(&db, "Bellevue", None, Source::Added, 1).unwrap();
        dictionary::add(&db, "xiaoyu", Some("show you"), Source::Added, 1).unwrap();
        dictionary::add(&db, "Niamh", Some("neeve"), Source::Added, 1).unwrap();
        snippets::save(&db, None, "my address", "221 Pine St", 1).unwrap();
        snippets::save(&db, None, "sig", "Old", 1).unwrap();
        snippets::save(&db, None, "only here", "x", 1).unwrap();

        let setup = Setup {
            v: 1,
            dictionary: vec![
                word("Xiaoyu", Some("shao yoo")),
                word("Niamh", Some("neeve")),
                word("Deepgram", None),
                word("DEEPGRAM", Some("ignored, a repeat")),
            ],
            snippets: vec![
                cue("My address", "221 Pine St"),
                cue("sig", "Thanks,\nR"),
                cue("intro reply", "Hi!"),
            ],
        };
        let want = Plan {
            add_words: 1,
            add_snippets: 1,
            replace_words: vec![word("Xiaoyu", Some("shao yoo"))],
            replace_snippets: vec!["My address".into(), "sig".into()],
            keep_words: 1,
            keep_snippets: 1,
        };
        assert_eq!(preview(&db, &setup).unwrap(), want);
        assert_eq!(copy(&db, &setup, 9).unwrap(), want);

        let mut words: Vec<_> = dictionary::list(&db)
            .unwrap()
            .into_iter()
            .map(|e| (e.word, e.sounds_like, e.source))
            .collect();
        words.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            words,
            [
                ("Bellevue".into(), None, Source::Added),
                ("Deepgram".into(), None, Source::Learned),
                ("Niamh".into(), Some("neeve".into()), Source::Added),
                ("Xiaoyu".into(), Some("shao yoo".into()), Source::Added),
            ]
        );
        assert!(dictionary::list(&db).unwrap().contains(&kept));
        assert_eq!(
            dictionary::list(&db).unwrap()[..2]
                .iter()
                .map(|e| e.word.as_str())
                .collect::<Vec<_>>(),
            ["Xiaoyu", "Deepgram"],
            "copied words keep the copy's order"
        );
        let mut snips: Vec<_> = snippets::list(&db)
            .unwrap()
            .into_iter()
            .map(|s| (s.cue, s.text))
            .collect();
        snips.sort();
        assert_eq!(
            snips,
            [
                ("My address".into(), "221 Pine St".into()),
                ("intro reply".into(), "Hi!".into()),
                ("only here".into(), "x".into()),
                ("sig".into(), "Thanks,\nR".into()),
            ]
        );
        assert_eq!(
            preview(&db, &setup).unwrap(),
            Plan {
                keep_words: 1,
                keep_snippets: 1,
                ..Plan::default()
            },
            "a second copy changes nothing"
        );
    }

    #[test]
    fn a_copy_without_a_sounds_like_clears_this_devices() {
        let db = Db::open_in_memory().unwrap();
        dictionary::add(&db, "Niamh", Some("neeve"), Source::Added, 1).unwrap();
        let setup = Setup {
            v: 1,
            dictionary: vec![word("Niamh", None)],
            snippets: vec![],
        };
        copy(&db, &setup, 2).unwrap();
        assert_eq!(dictionary::list(&db).unwrap()[0].sounds_like, None);
    }

    #[test]
    fn export_reads_back_as_the_setup_json_the_cloud_holds() {
        let db = Db::open_in_memory().unwrap();
        dictionary::add(&db, "Niamh", Some("neeve"), Source::Learned, 1).unwrap();
        snippets::save(&db, None, "sig", "Thanks", 1).unwrap();
        assert_eq!(
            serde_json::to_value(export(&db).unwrap()).unwrap(),
            serde_json::json!({
                "v": 1,
                "dictionary": [{ "word": "Niamh", "sounds_like": "neeve", "source": "learned" }],
                "snippets": [{ "cue": "sig", "text": "Thanks" }],
            })
        );
    }
}
