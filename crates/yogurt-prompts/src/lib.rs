//! Bundled LLM prompt templates.
//! `Mode::Release` serves the copy `include_str!` bakes in at build time.
//! `Mode::Dev` re-reads `templates/` on every call so edits show up without rebuilding.

mod ctx;
pub mod thinking;
pub use ctx::EnhanceCtx;

use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use tinytemplate::TinyTemplate;

/// `include_str!` rather than rust-embed, which also bakes in the folder path and file times, so the WASM build is the same on every machine.
const EMBEDDED: [(&str, &str); 12] = [
    ("enhance.md", include_str!("../templates/enhance.md")),
    (
        "chat-system.md",
        include_str!("../templates/chat-system.md"),
    ),
    (
        "library-chat-system.md",
        include_str!("../templates/library-chat-system.md"),
    ),
    ("live-recap.md", include_str!("../templates/live-recap.md")),
    (
        "voice-cleanup.md",
        include_str!("../templates/voice-cleanup.md"),
    ),
    (
        "enhance/general.md",
        include_str!("../templates/enhance/general.md"),
    ),
    (
        "enhance/standup.md",
        include_str!("../templates/enhance/standup.md"),
    ),
    (
        "enhance/one-on-one.md",
        include_str!("../templates/enhance/one-on-one.md"),
    ),
    (
        "enhance/team-meeting.md",
        include_str!("../templates/enhance/team-meeting.md"),
    ),
    (
        "enhance/design-review.md",
        include_str!("../templates/enhance/design-review.md"),
    ),
    (
        "enhance/customer-call.md",
        include_str!("../templates/enhance/customer-call.md"),
    ),
    (
        "enhance/interview.md",
        include_str!("../templates/enhance/interview.md"),
    ),
];

/// In UI order. The first entry is the fallback when auto-detection names nothing recognizable.
/// Each id is the stem of `templates/enhance/<id>.md`.
pub const TEMPLATE_IDS: [&str; 7] = [
    "general",
    "standup",
    "one-on-one",
    "team-meeting",
    "design-review",
    "customer-call",
    "interview",
];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Template {
    pub id: &'static str,
    pub name: String,
    /// Shown as a picker hint and given to the model to choose by.
    pub when: String,
    #[serde(skip)]
    pub body: String,
}

fn dev_template_dir() -> PathBuf {
    // WASM has no file system, and the absolute path would make its build differ per machine.
    if cfg!(target_family = "wasm") {
        return PathBuf::new();
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates")
}

#[derive(Debug, Clone, Copy)]
pub enum Mode {
    Dev,
    Release,
}

pub struct Prompts {
    mode: Mode,
    cached: HashMap<String, String>,
}

impl Prompts {
    pub fn load(mode: Mode) -> Result<Self> {
        let mut p = Self {
            mode,
            cached: HashMap::new(),
        };
        if matches!(mode, Mode::Release) {
            for name in [
                "enhance.md",
                "chat-system.md",
                "library-chat-system.md",
                "live-recap.md",
                "voice-cleanup.md",
            ] {
                p.cached.insert(name.to_string(), read_embedded(name)?);
            }
            for id in TEMPLATE_IDS {
                let name = template_file(id);
                p.cached.insert(name.clone(), read_embedded(&name)?);
            }
        }
        Ok(p)
    }

    /// `None` renders every format with an instruction to pick one, `Some(id)` renders just that one.
    /// An unknown id is an error, since callers validate against [`TEMPLATE_IDS`] first.
    pub fn render_enhance(&self, ctx: &EnhanceCtx<'_>) -> Result<String> {
        let format = match ctx.template {
            Some(id) => {
                let t = self
                    .template(id)?
                    .with_context(|| format!("unknown enhance template: {id}"))?;
                format!(
                    "Note format: {} ({}).\n{}\n\n{}",
                    t.name,
                    t.when,
                    output_contract(ctx.suggest_titles),
                    t.body.trim_end()
                )
            }
            None => {
                let mut s = String::from(
                    "Note format. Pick the format below that best matches what this meeting \
                     actually was, judging by the transcript and the notes.\n",
                );
                s.push_str(&output_contract(ctx.suggest_titles));
                for t in self.templates()? {
                    s.push_str(&format!(
                        "\n\n### {}\nUse when: {}.\n{}",
                        t.id,
                        t.when,
                        t.body.trim_end()
                    ));
                }
                s
            }
        };
        #[derive(Serialize)]
        struct Full<'a> {
            notes: &'a str,
            transcript: &'a str,
            format: String,
        }
        let raw = self.read("enhance.md")?;
        render(
            &raw,
            "enhance",
            &Full {
                notes: ctx.notes,
                transcript: ctx.transcript,
                format,
            },
        )
    }

    /// `earlier` is the recap so far as `- [mm:ss] fact` lines, `lines` the new transcript lines.
    pub fn render_live_recap(&self, earlier: &str, lines: &str) -> Result<String> {
        #[derive(Serialize)]
        struct Ctx<'a> {
            earlier: &'a str,
            lines: &'a str,
        }
        let earlier = if earlier.trim().is_empty() {
            "(no bullets yet)"
        } else {
            earlier
        };
        render(
            &self.read("live-recap.md")?,
            "live-recap",
            &Ctx { earlier, lines },
        )
    }

    pub fn chat_system(&self) -> Result<String> {
        self.read("chat-system.md")
    }

    pub fn library_chat_system(&self) -> Result<String> {
        self.read("library-chat-system.md")
    }

    pub fn voice_cleanup(&self) -> Result<VoiceCleanup> {
        parse_voice_cleanup(&self.read("voice-cleanup.md")?)
    }

    pub fn templates(&self) -> Result<Vec<Template>> {
        TEMPLATE_IDS
            .iter()
            .map(|id| {
                self.template(id)?
                    .with_context(|| format!("template listed but not loadable: {id}"))
            })
            .collect()
    }

    pub fn template(&self, id: &str) -> Result<Option<Template>> {
        let Some(id) = TEMPLATE_IDS.iter().copied().find(|t| *t == id) else {
            return Ok(None);
        };
        let raw = self.read(&template_file(id))?;
        parse_template(id, &raw).map(Some)
    }

    fn read(&self, name: &str) -> Result<String> {
        match self.mode {
            Mode::Dev => std::fs::read_to_string(dev_template_dir().join(name))
                .with_context(|| format!("dev: reading templates/{name}")),
            Mode::Release => Ok(self
                .cached
                .get(name)
                .expect("release mode caches at load")
                .clone()),
        }
    }
}

/// The cleanup instructions, and worked examples that go to the model as earlier chat turns.
/// A 4B model follows a shown example where it skips the same rule stated in prose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceCleanup {
    pub system: String,
    /// The line that lists the dictionary words a dictation needs, with `{words}` where they go.
    pub dictionary: Option<String>,
    /// The line that lists the snippet cues, with `{cues}` where they go.
    pub snippets: Option<String>,
    /// The rules and examples for a dictation into an app set to Casual or Formal. Normal adds no line.
    pub casual: Option<String>,
    pub formal: Option<String>,
    /// The line for a dictation that says a symbol or a break, such as "dash", "comma" or "new line". Every other dictation leaves it out, since Qwen3 4B regressed on unrelated cases with it in the instructions.
    pub symbols: Option<String>,
    /// `(dictation, cleaned)` pairs.
    pub examples: Vec<(String, String)>,
}

/// Expects the instructions with optional `dictionary:`, `snippets:`, `casual:`, `formal:`, and `symbols:` lines among them, then `dictation:` and `cleaned:` line pairs.
fn parse_voice_cleanup(raw: &str) -> Result<VoiceCleanup> {
    let start = raw
        .lines()
        .position(|l| l.trim_start().starts_with("dictation:"))
        .unwrap_or(usize::MAX);
    let mut lines = raw.lines();
    let (mut dictionary, mut snippets, mut casual, mut formal, mut symbols) =
        (None, None, None, None, None);
    let system = lines
        .by_ref()
        .take(start)
        .filter(|l| {
            let l = l.trim_start();
            if let Some(d) = l.strip_prefix("dictionary:") {
                dictionary = Some(d.trim().to_string());
            } else if let Some(s) = l.strip_prefix("snippets:") {
                snippets = Some(s.trim().to_string());
            } else if let Some(s) = l.strip_prefix("casual:") {
                casual = Some(s.trim().to_string());
            } else if let Some(s) = l.strip_prefix("formal:") {
                formal = Some(s.trim().to_string());
            } else if let Some(s) = l.strip_prefix("symbols:") {
                symbols = Some(s.trim().to_string());
            } else {
                return true;
            }
            false
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    if system.is_empty() {
        bail!("voice-cleanup.md: empty instructions");
    }
    if dictionary.as_ref().is_some_and(|d| !d.contains("{words}")) {
        bail!("voice-cleanup.md: the `dictionary:` line has no `{{words}}`");
    }
    if snippets.as_ref().is_some_and(|s| !s.contains("{cues}")) {
        bail!("voice-cleanup.md: the `snippets:` line has no `{{cues}}`");
    }
    let mut lines = lines.map(str::trim).filter(|l| !l.is_empty());
    let mut examples = Vec::new();
    while let Some(line) = lines.next() {
        let dictation = line.strip_prefix("dictation:").with_context(|| {
            format!("voice-cleanup.md: expected `dictation: ...`, got {line:?}")
        })?;
        let cleaned = lines
            .next()
            .and_then(|l| l.strip_prefix("cleaned:"))
            .with_context(|| {
                format!("voice-cleanup.md: `dictation:{dictation}` has no `cleaned:` line after it")
            })?;
        let (dictation, cleaned) = (dictation.trim(), cleaned.trim());
        if dictation.is_empty() || cleaned.is_empty() {
            bail!("voice-cleanup.md: an example has an empty `dictation:` or `cleaned:`");
        }
        examples.push((dictation.to_string(), cleaned.to_string()));
    }
    Ok(VoiceCleanup {
        system,
        dictionary,
        snippets,
        casual,
        formal,
        symbols,
        examples,
    })
}

/// Shared by the pick and forced paths: the header lines [`split_header`] parses, plus the section rules.
fn output_contract(suggest_titles: bool) -> String {
    let header = match suggest_titles {
        false => "Your output's first line must be exactly `template: <id>` on its own line, \
            followed by a blank line, then the document.",
        true => "Your output's first line must be exactly `template: <id>` on its own line. \
            Its second line must be exactly `titles: <name> | <name> | <name>`: three different names \
            for this meeting, best first, each 2 to 6 words in sentence case that say what it was about \
            (`Q3 pricing review`, `Hiring loop for backend roles`), with no date, no quotes, and never a \
            generic name like `Meeting` or `Weekly sync`. Then a blank line, then the document.",
    };
    format!(
        "{header} Keep the user's own headings if they wrote any and put the format's remaining \
        sections after them. Include only the sections that have content and omit empty ones. \
        Headings are `##`."
    )
}

fn template_file(id: &str) -> String {
    format!("enhance/{id}.md")
}

/// Expects a `name:` line, a `when:` line, a blank line, then the body.
fn parse_template(id: &'static str, raw: &str) -> Result<Template> {
    let mut lines = raw.lines();
    let field = |lines: &mut std::str::Lines<'_>, key: &str| -> Result<String> {
        let line = lines
            .next()
            .with_context(|| format!("template {id}: missing `{key}:` line"))?;
        let value = line
            .strip_prefix(key)
            .and_then(|s| s.strip_prefix(':'))
            .with_context(|| format!("template {id}: expected `{key}: ...`, got {line:?}"))?;
        let value = value.trim();
        if value.is_empty() {
            bail!("template {id}: `{key}:` is empty");
        }
        Ok(value.to_string())
    };
    let name = field(&mut lines, "name")?;
    let when = field(&mut lines, "when")?;
    let body = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    if body.is_empty() {
        bail!("template {id}: empty body");
    }
    Ok(Template {
        id,
        name,
        when,
        body,
    })
}

/// The note format an enhance request names. Absent, empty, or `auto` lets the model pick, and an unknown id is an error.
pub fn template_choice(choice: Option<&str>) -> Result<Option<&'static str>> {
    match choice.map(str::trim) {
        None | Some("") | Some("auto") => Ok(None),
        Some(id) => TEMPLATE_IDS
            .iter()
            .copied()
            .find(|t| *t == id)
            .map(Some)
            .with_context(|| {
                format!(
                    "unknown template {id:?}; expected one of: {}",
                    TEMPLATE_IDS.join(", ")
                )
            }),
    }
}

/// The header lines of an enhance reply and the document after them.
#[derive(Debug, PartialEq, Eq)]
pub struct Header<'a> {
    /// The `template:` id as written, unvalidated.
    pub template: Option<&'a str>,
    /// The `titles:` names, at most three, empty when the reply named none.
    pub titles: Vec<String>,
    pub body: &'a str,
}

/// Splits the `template: <id>` line, and a `titles:` line right after it, off the model's output.
/// A reply without the `template:` marker is all body.
/// A streaming prefix of either marker (`"templ"`, `"tit"`) is treated as the marker so a live preview never flashes it.
pub fn split_header(text: &str) -> Header<'_> {
    let Some((template, rest)) = split_marker(text, "template:") else {
        return Header {
            template: None,
            titles: Vec::new(),
            body: text,
        };
    };
    let (titles, body) = match split_marker(rest, "titles:") {
        Some((names, body)) => (names.map(parse_titles).unwrap_or_default(), body),
        None => (Vec::new(), rest),
    };
    Header {
        template,
        titles,
        body,
    }
}

/// [`split_header`] without the titles.
pub fn split_template_line(text: &str) -> (Option<&str>, &str) {
    let h = split_header(text);
    (h.template, h.body)
}

/// `Some((value, rest))` when the first non-blank line is `marker` (any case, backticks allowed) or still a streaming prefix of it.
fn split_marker<'a>(text: &'a str, marker: &str) -> Option<(Option<&'a str>, &'a str)> {
    let start = text.trim_start();
    let (first, rest) = match start.find('\n') {
        Some(i) => (&start[..i], &start[i + 1..]),
        None => (start, ""),
    };
    let first = first.trim().trim_end_matches('`').trim_start_matches('`');
    let lower = first.to_ascii_lowercase();
    if lower.starts_with(marker) {
        let value = first[marker.len()..].trim().trim_matches(['`', '*', '"']);
        let value = (!value.is_empty()).then_some(value);
        return Some((value, rest.trim_start_matches(['\n', '\r'])));
    }
    if rest.is_empty() && !lower.is_empty() && marker.starts_with(&lower) {
        return Some((None, ""));
    }
    None
}

fn parse_titles(names: &str) -> Vec<String> {
    const MAX_CHARS: usize = 80;
    let mut out: Vec<String> = Vec::new();
    for name in names.split('|') {
        let name = name
            .trim()
            .trim_matches(['"', '*', '`', '\u{201c}', '\u{201d}'])
            .trim();
        // Use appends the meeting's date, so a date the model added would show twice.
        let name = match name.rsplit_once(' ') {
            Some((head, last)) if is_short_date(last) => head.trim_end(),
            _ => name,
        };
        if name.is_empty()
            || name.chars().count() > MAX_CHARS
            || ["meeting", "untitled meeting"].contains(&name.to_lowercase().as_str())
            || out.iter().any(|n| n.eq_ignore_ascii_case(name))
        {
            continue;
        }
        out.push(name.to_string());
    }
    out.truncate(3);
    out
}

/// `M/D/YY` or `M/D/YYYY`, as default titles carry.
fn is_short_date(s: &str) -> bool {
    let parts: Vec<&str> = s.split('/').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

fn read_embedded(name: &str) -> Result<String> {
    EMBEDDED
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, body)| body.to_string())
        .with_context(|| format!("embedded asset missing: {name}"))
}

fn render<S: Serialize>(template: &str, name: &str, ctx: &S) -> Result<String> {
    let mut tt = TinyTemplate::new();
    // Output goes into an LLM prompt, not a browser, so skip tinytemplate's HTML escaping.
    tt.set_default_formatter(&tinytemplate::format_unescaped);
    tt.add_template(name, template)?;
    Ok(tt.render(name, ctx)?)
}

/// List prices for the Voice cleanup models. In `templates/` because the Android app bundles that folder too, so both read one table.
pub const CLEANUP_PRICES: &str = include_str!("../templates/cleanup-prices.json");

#[cfg(test)]
mod tests {
    use super::{parse_voice_cleanup, split_header, split_template_line, Mode, Prompts};

    #[test]
    fn splits_marker_and_body() {
        let (id, body) = split_template_line("template: standup\n\n## Updates\n- x");
        assert_eq!(id, Some("standup"));
        assert_eq!(body, "## Updates\n- x");
    }

    #[test]
    fn tolerates_case_backticks_and_leading_blank_lines() {
        let (id, body) = split_template_line("\n\n`Template: Design-Review`\n## Proposal");
        assert_eq!(id, Some("Design-Review"));
        assert_eq!(body, "## Proposal");
    }

    #[test]
    fn passes_through_a_reply_without_marker() {
        let (id, body) = split_template_line("## Decisions\n- ship");
        assert_eq!(id, None);
        assert_eq!(body, "## Decisions\n- ship");
    }

    #[test]
    fn hides_a_streaming_prefix_of_the_marker() {
        assert_eq!(split_template_line("templ"), (None, ""));
        assert_eq!(split_template_line("template: stand"), (Some("stand"), ""));
        assert_eq!(split_template_line("template:"), (None, ""));
        assert_eq!(split_template_line("## Dec"), (None, "## Dec"));
    }

    #[test]
    fn keeps_a_quoted_template_id_intact() {
        assert_eq!(
            split_template_line("template: \"standup\"\n- x"),
            (Some("standup"), "- x")
        );
    }

    #[test]
    fn splits_titles_after_the_template_line() {
        let h = split_header(
            "template: general\ntitles: Q3 pricing review 11/12/26 | \"Pricing sync\" | Meeting | q3 PRICING REVIEW | | Pro tier launch | Extra\n\n## Decisions\n- x",
        );
        assert_eq!(h.template, Some("general"));
        assert_eq!(
            h.titles,
            ["Q3 pricing review", "Pricing sync", "Pro tier launch"]
        );
        assert_eq!(h.body, "## Decisions\n- x");
    }

    #[test]
    fn titles_only_count_right_after_the_template_line() {
        let h = split_header("titles: A | B\n## Notes");
        assert_eq!(
            (h.template, h.titles.len(), h.body),
            (None, 0, "titles: A | B\n## Notes")
        );
        let h = split_header("template: general\n\n## Notes\n- x");
        assert_eq!((h.titles.len(), h.body), (0, "## Notes\n- x"));
    }

    #[test]
    fn hides_a_streaming_titles_line() {
        assert_eq!(split_header("template: general\ntit").body, "");
        assert_eq!(split_header("template: general\ntitles: Q3 pri").body, "");
        assert_eq!(split_header("template: general\n## Dec").body, "## Dec");
    }

    #[test]
    fn voice_cleanup_splits_instructions_from_examples() {
        let p = Prompts::load(Mode::Release)
            .unwrap()
            .voice_cleanup()
            .unwrap();
        assert!(p.system.ends_with("nothing else."), "{}", p.system);
        assert!(p.dictionary.unwrap().ends_with(": {words}"));
        assert!(p.snippets.unwrap().ends_with(": {cues}"));
        assert!(!p.system.contains("snippets:"));
        assert!(!p.system.contains("symbols:"));
        assert!(p
            .symbols
            .unwrap()
            .starts_with("Where the dictation says \"dash\" or \"slash\" between two words"));
        assert!(p.casual.unwrap().starts_with("Write this one casual."));
        assert!(p.formal.unwrap().starts_with("Write this one formal."));
        assert_eq!(
            p.examples[0],
            (
                "Let's meet at like two, no wait, three. Actually, you know what, four.".into(),
                "Let's meet at 4.".into()
            )
        );
    }

    #[test]
    fn voice_cleanup_rejects_an_example_without_its_answer() {
        assert!(parse_voice_cleanup(
            "Clean it.\n\ndictation: um hi\n\ndictation: yo\ncleaned: Yo."
        )
        .is_err());
        assert!(parse_voice_cleanup("dictation: um hi\ncleaned: Hi.").is_err());
        assert!(parse_voice_cleanup("Clean it.\n\ndictation: um\ncleaned:").is_err());
        assert_eq!(parse_voice_cleanup("Clean it.").unwrap().examples, vec![]);
        let indented =
            parse_voice_cleanup("Clean it.\n  dictation: um hi\n  cleaned: Hi.").unwrap();
        assert_eq!(indented.system, "Clean it.");
        assert_eq!(indented.examples.len(), 1);
        assert_eq!(indented.dictionary, None);
        assert!(parse_voice_cleanup("Clean it.\ndictionary: Spell these.").is_err());
        assert!(parse_voice_cleanup("Clean it.\nsnippets: Mark these.").is_err());
    }
}
