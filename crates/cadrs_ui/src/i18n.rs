//! The interface's language (issue #178): message catalogs and [`tr`].
//!
//! - **Catalogs** are files in `crates/cadrs_ui/locales/`, one a language (`de.ftl`), in
//!   Fluent's simple form: `key = text`, `#` lines are notes for translators, and `{ $name }` is
//!   filled in by the app. `en.ftl` is the source: every key the app uses is there. They are
//!   built into the app (a browser page has no files to read), so a translation needs no Rust:
//!   only its file (and a line in [`LANGUAGES`] for a new language). See `locales/README.md`.
//! - [`tr`] gives a key's text in the current language ([`set_language`]), else in English, else
//!   the key itself; [`tr_with`] fills in its `{ $name }`s.
//! - [`check`] lists what is wrong in the catalogs: unparsable lines, keys English doesn't have,
//!   placeholders that differ from English's, keys a language doesn't translate yet (warnings).
//!   A test runs it, so CI catches a broken catalog.
//! - User content (document, part and feature names) is never translated: only the app's own
//!   texts, where it calls [`tr`].

use std::collections::BTreeMap;
use std::sync::{OnceLock, RwLock};

/// The languages cadrs has catalogs for: code (BCP 47), the language's own name, the catalog.
/// [`offered`] are the ones the interface's font can draw.
pub const LANGUAGES: &[(&str, &str, &str)] = &[
    ("en", "English", include_str!("../locales/en.ftl")),
    ("de", "Deutsch", include_str!("../locales/de.ftl")),
    ("fr", "Français", include_str!("../locales/fr.ftl")),
    ("es", "Español", include_str!("../locales/es.ftl")),
    ("it", "Italiano", include_str!("../locales/it.ftl")),
    ("nl", "Nederlands", include_str!("../locales/nl.ftl")),
    (
        "pt-BR",
        "Português (Brasil)",
        include_str!("../locales/pt-BR.ftl"),
    ),
    ("ru", "Русский", include_str!("../locales/ru.ftl")),
    ("zh-CN", "简体中文", include_str!("../locales/zh-CN.ftl")),
    ("ja", "日本語", include_str!("../locales/ja.ftl")),
    ("ko", "한국어", include_str!("../locales/ko.ftl")),
];

/// Languages whose script the interface's font (Inter: Latin, Cyrillic, Greek) can't draw:
/// their catalogs are kept and checked, but they aren't offered until a font for them ships
/// (Bevy's text doesn't fall back to the system's fonts).
const NEEDS_A_FONT: &[&str] = &["zh-CN", "ja", "ko"];

/// The languages to choose from (code, own name): those the interface's font can draw.
pub fn offered() -> impl Iterator<Item = (&'static str, &'static str)> {
    LANGUAGES
        .iter()
        .filter(|(code, ..)| !NEEDS_A_FONT.contains(code))
        .map(|(code, name, _)| (*code, *name))
}

type Catalog = BTreeMap<String, String>;

/// Every catalog, parsed once (lines that don't parse are left out; [`check`] reports them).
fn catalogs() -> &'static BTreeMap<&'static str, Catalog> {
    static ALL: OnceLock<BTreeMap<&'static str, Catalog>> = OnceLock::new();
    ALL.get_or_init(|| {
        LANGUAGES
            .iter()
            .map(|(code, _, text)| (*code, parse(text).0))
            .collect()
    })
}

static CURRENT: RwLock<&'static str> = RwLock::new("en");

/// Shows the interface in the language with this code from now on (an unknown one: English).
/// Texts made from now on follow; [`Translated`] texts already shown change too.
pub fn set_language(code: &str) {
    let code = LANGUAGES
        .iter()
        .map(|(c, ..)| *c)
        .find(|c| *c == code)
        .unwrap_or("en");
    if let Ok(mut current) = CURRENT.write() {
        *current = code;
    }
}

/// The current language's code.
pub fn language() -> &'static str {
    CURRENT.read().map_or("en", |c| *c)
}

/// The text of `key` in the current language, else in English, else the key itself.
pub fn tr(key: &str) -> String {
    let all = catalogs();
    [language(), "en"]
        .iter()
        .find_map(|code| all.get(code)?.get(key).cloned())
        .unwrap_or_else(|| key.to_string())
}

/// [`tr`] with its `{ $name }`s filled in from `args`.
pub fn tr_with(key: &str, args: &[(&str, &str)]) -> String {
    let mut text = tr(key);
    for (name, value) in args {
        text = text.replace(&format!("{{ ${name} }}"), value);
    }
    text
}

/// A UI text shown from a catalog key: it changes when the language does
/// ([`retranslate`], in [`crate::CadrsUiPlugin`]).
#[derive(bevy::prelude::Component, Debug, Clone, Copy)]
pub struct Translated(pub &'static str);

/// Puts the current language's text into every [`Translated`] text when the language changes.
pub fn retranslate(
    mut last: bevy::prelude::Local<&'static str>,
    mut q: bevy::prelude::Query<(&Translated, &mut bevy::prelude::Text)>,
) {
    let now = language();
    if *last == now {
        return;
    }
    *last = now;
    for (t, mut text) in &mut q {
        let want = tr(t.0);
        if text.0 != want {
            text.0 = want;
        }
    }
}

/// A catalog's `key = text` entries, and the lines that aren't one (line number, line).
fn parse(text: &str) -> (Catalog, Vec<(usize, String)>) {
    let mut out = Catalog::new();
    let mut bad = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let ok = t.split_once(" = ").filter(|(k, v)| {
            !v.trim().is_empty()
                && k.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        });
        match ok {
            Some((k, v)) => {
                out.insert(k.to_string(), v.trim().to_string());
            }
            None => bad.push((n + 1, line.to_string())),
        }
    }
    (out, bad)
}

/// The `{ $name }`s of a text, sorted.
fn placeholders(text: &str) -> Vec<&str> {
    let mut out: Vec<&str> = text
        .match_indices("{ $")
        .filter_map(|(i, _)| {
            let rest = &text[i + 3..];
            rest.find(" }").map(|end| &rest[..end])
        })
        .collect();
    out.sort_unstable();
    out
}

/// What is wrong in the catalogs: errors (a line that isn't `key = text`, a key English doesn't
/// have, a text whose placeholders differ from English's) and warnings (keys a language doesn't
/// translate yet, which show in English).
pub fn check() -> (Vec<String>, Vec<String>) {
    let (mut errors, mut warnings) = (Vec::new(), Vec::new());
    let english = &catalogs()["en"];
    for (code, _, text) in LANGUAGES {
        let (catalog, bad) = parse(text);
        for (n, line) in bad {
            errors.push(format!("{code}.ftl:{n}: not `key = text`: {line:?}"));
        }
        for (key, value) in &catalog {
            match english.get(key) {
                None => errors.push(format!("{code}.ftl: {key} is not in en.ftl")),
                Some(en) if placeholders(en) != placeholders(value) => errors.push(format!(
                    "{code}.ftl: {key} has placeholders {:?}, English {:?}",
                    placeholders(value),
                    placeholders(en)
                )),
                Some(_) => {}
            }
        }
        let missing: Vec<&str> = english
            .keys()
            .filter(|k| !catalog.contains_key(*k))
            .map(String::as_str)
            .collect();
        if !missing.is_empty() {
            warnings.push(format!(
                "{code}.ftl: not translated yet: {}",
                missing.join(", ")
            ));
        }
    }
    (errors, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped catalogs: no errors, and (as shipped) every language translates every key.
    #[test]
    fn the_catalogs_are_sound_and_complete() {
        let (errors, warnings) = check();
        assert!(errors.is_empty(), "{errors:#?}");
        assert!(warnings.is_empty(), "{warnings:#?}");
    }

    #[test]
    fn a_text_falls_back_to_english_then_to_its_key() {
        // One test sets the language (the setting is global).
        assert_eq!(tr("share"), "Share");
        set_language("de");
        assert_eq!(tr("share"), "Teilen");
        assert_eq!(tr_with("features-count", &[("count", "4")]), "Features (4)");
        set_language("ja");
        assert_eq!(
            tr_with("features-count", &[("count", "7")]),
            "フィーチャー (7)"
        );
        assert_eq!(tr("no-such-key"), "no-such-key");
        set_language("xx");
        assert_eq!(language(), "en");
        assert_eq!(tr("share"), "Share");
    }

    #[test]
    fn broken_lines_and_placeholders_are_found() {
        let (catalog, bad) = parse("ok = fine\nBad Key = no\nempty =\n# note\n");
        assert_eq!(catalog.len(), 1);
        assert_eq!(bad.len(), 2);
        assert_eq!(
            placeholders("Features ({ $count }) of { $all }"),
            ["all", "count"]
        );
    }
}
