//! The interface's language (issue #178): message catalogs and [`tr`].
//!
//! - **Catalogs** are files in `crates/cadrs_ui/locales/`, one a language (`de.ftl`), in
//!   Fluent's simple form: `key = text`, `#` lines are notes for translators, and `{ $name }` is
//!   filled in by the app. `en.ftl` is the source: every key the app uses is there. They are
//!   built into the app (a browser page has no files to read), so a translation needs no Rust:
//!   only its file (and a line in [`LANGUAGES`] for a new language). See `locales/README.md`.
//! - [`tr`] gives a key's text in the current language ([`set_language`]), else in English, else
//!   the key itself; [`tr_with`] fills in its `{ $name }`s. A text takes the language it is made
//!   in: what is rebuilt (the feature list, a dialog opened again) follows a change at once,
//!   what is built once (the top bar) at the next window; a text marked [`Translated`] follows
//!   at once too.
//! - Plural forms and Fluent's other selectors aren't supported yet: [`check`] says so for a
//!   catalog that uses one (write the number as a placeholder instead).
//! - [`check`] lists what is wrong in the catalogs: unparsable lines, selectors, duplicate keys,
//!   keys English doesn't have, placeholders that differ from English's (errors), and keys a
//!   language doesn't translate yet (warnings: they show in English). A test fails on errors,
//!   so CI catches a broken catalog.
//! - User content (document, part and feature names) is never translated: only the app's own
//!   texts, where it calls [`tr`].

use std::collections::BTreeMap;
use std::sync::{OnceLock, RwLock};

/// The languages cadrs has catalogs for: code (BCP 47), the language's own name, the catalog;
/// in alphabetical order of their names, as Preferences › Language lists them. [`offered`] are
/// the ones the interface's font can draw.
pub const LANGUAGES: &[(&str, &str, &str)] = &[
    ("af", "Afrikaans", include_str!("../locales/af.ftl")),
    ("de", "Deutsch", include_str!("../locales/de.ftl")),
    ("en", "English", include_str!("../locales/en.ftl")),
    ("es", "Español", include_str!("../locales/es.ftl")),
    ("fr", "Français", include_str!("../locales/fr.ftl")),
    ("it", "Italiano", include_str!("../locales/it.ftl")),
    ("nl", "Nederlands", include_str!("../locales/nl.ftl")),
    (
        "pt-BR",
        "Português (Brasil)",
        include_str!("../locales/pt-BR.ftl"),
    ),
    ("ru", "Русский", include_str!("../locales/ru.ftl")),
    ("ja", "日本語", include_str!("../locales/ja.ftl")),
    ("zh-CN", "简体中文", include_str!("../locales/zh-CN.ftl")),
    ("ko", "한국어", include_str!("../locales/ko.ftl")),
];

/// English's catalog, the source.
const ENGLISH: &str = include_str!("../locales/en.ftl");

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
/// A language that isn't [`offered`] (its script needs a font cadrs doesn't have) is English.
pub fn set_language(code: &str) {
    let code = shown_as(code);
    if let Ok(mut current) = CURRENT.write() {
        *current = code;
    }
}

/// The language a code is shown in: itself if it is [`offered`], else English.
fn shown_as(code: &str) -> &'static str {
    offered()
        .map(|(c, _)| c)
        .find(|c| *c == code)
        .unwrap_or("en")
}

/// The current language's code.
pub fn language() -> &'static str {
    CURRENT.read().map_or("en", |c| *c)
}

/// The text of `key` in the current language, else in English, else the key itself. A `\n` in
/// a catalog's text is a line break.
pub fn tr(key: &str) -> String {
    tr_in(language(), key)
}

/// [`tr`] in the language with this code.
fn tr_in(code: &str, key: &str) -> String {
    let all = catalogs();
    [code, "en"]
        .iter()
        .find_map(|code| all.get(code)?.get(key))
        .map_or_else(|| key.to_string(), |text| text.replace("\\n", "\n"))
}

/// [`tr`] with its `{ $name }`s filled in from `args`.
pub fn tr_with(key: &str, args: &[(&str, &str)]) -> String {
    fill(&tr(key), args)
}

/// `text` with its `{ $name }`s filled in from `args`.
fn fill(text: &str, args: &[(&str, &str)]) -> String {
    // One pass: a value that itself reads `{ $x }` stays as it is.
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("{ $") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 3..];
        match after.find(" }") {
            Some(end) => {
                let name = &after[..end];
                match args.iter().find(|(n, _)| *n == name) {
                    Some((_, value)) => out.push_str(value),
                    None => out.push_str(&rest[at..at + 3 + end + 2]),
                }
                rest = &after[end + 2..];
            }
            None => {
                out.push_str(&rest[at..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
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

/// A catalog's `key = text` entries (the first of a key that comes twice), and its problems
/// (line number, what): lines that aren't `key = text`, selectors (plurals), duplicate keys.
fn parse(text: &str) -> (Catalog, Vec<(usize, String)>) {
    let mut out = Catalog::new();
    let mut bad = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        // (A selector is `{ $x -> …`: an arrow in a text, "A -> B", is just text.)
        if t.contains("{ $") && t.contains("->") || t.starts_with('[') || t.starts_with("*[") {
            bad.push((
                n + 1,
                format!("selectors (plurals) aren't supported yet, use a placeholder: {line:?}"),
            ));
            continue;
        }
        let ok = t.split_once(" = ").filter(|(k, v)| {
            !v.trim().is_empty()
                && !k.starts_with('-')
                && k.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        });
        match ok {
            Some((k, _)) if out.contains_key(k) => {
                bad.push((n + 1, format!("{k} comes twice")));
            }
            Some((k, v)) => {
                out.insert(k.to_string(), v.trim().to_string());
            }
            None => bad.push((n + 1, format!("not `key = text`: {line:?}"))),
        }
    }
    (out, bad)
}

/// A catalog's keys in the file's order.
fn keys_in_order(text: &str) -> Vec<&str> {
    text.lines()
        .filter_map(|l| l.trim().split_once(" = ").map(|(k, _)| k))
        .filter(|k| !k.starts_with('#'))
        .collect()
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
        for (n, what) in bad {
            errors.push(format!("{code}.ftl:{n}: {what}"));
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
        let missing: Vec<&str> = keys_in_order(ENGLISH)
            .into_iter()
            .filter(|k| !catalog.contains_key(*k))
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

    /// The shipped catalogs have no errors; keys not translated yet are listed (they show in
    /// English, so a catalog can grow a few keys at a time).
    #[test]
    fn the_catalogs_are_sound() {
        let (errors, warnings) = check();
        assert!(errors.is_empty(), "{errors:#?}");
        for w in warnings {
            println!("{w}");
        }
    }

    /// Preferences › Language lists the languages in alphabetical order of their own names.
    #[test]
    fn the_languages_are_in_alphabetical_order() {
        let names: Vec<String> = LANGUAGES.iter().map(|(_, n, _)| n.to_lowercase()).collect();
        assert!(names.is_sorted(), "{names:?}");
    }

    /// (The language setting is global, and every test's texts read it: these leave it alone.)
    #[test]
    fn a_text_falls_back_to_english_then_to_its_key() {
        assert_eq!(tr_in("en", "share"), "Share");
        assert_eq!(tr_in("de", "share"), "Teilen");
        assert_eq!(tr_in("af", "parts"), "Onderdele");
        assert_eq!(
            fill(&tr_in("de", "features-count"), &[("count", "4")]),
            "Features (4)"
        );
        // A value is never read as a placeholder; an unknown one stays.
        assert_eq!(
            fill(&tr_in("de", "features-count"), &[("count", "{ $count }")]),
            "Features ({ $count })"
        );
        assert_eq!(fill("{ $a } and { $b }", &[("a", "1")]), "1 and { $b }");
        assert_eq!(tr_in("de", "no-such-key"), "no-such-key");
        // Not offered (no font for its script), or unknown: English.
        assert_eq!(shown_as("ja"), "en");
        assert_eq!(shown_as("xx"), "en");
        assert_eq!(shown_as("af"), "af");
    }

    #[test]
    fn broken_lines_and_placeholders_are_found() {
        let (catalog, bad) = parse(
            "ok = fine\nBad Key = no\nempty =\n# note\nok = again\nn = { $c ->\n *[other] many\narrow = A -> B\n",
        );
        assert_eq!(catalog.len(), 2);
        assert_eq!(catalog["ok"], "fine");
        assert_eq!(catalog["arrow"], "A -> B");
        assert_eq!(bad.len(), 5, "{bad:?}");
        assert!(bad.iter().any(|(_, w)| w.contains("comes twice")));
        assert!(bad.iter().any(|(_, w)| w.contains("selectors")));
        assert_eq!(
            placeholders("Features ({ $count }) of { $all }"),
            ["all", "count"]
        );
    }
}
