//! The documentation's reference pages made from the code (#273): `docs/src/reference/`'s
//! keyboard shortcuts ([`crate::shortcuts::reference_markdown`]) and preferences
//! ([`crate::preferences_dialog::reference_markdown`]), in English. The pages are committed, so
//! the book builds without cargo; the tests here fail when one no longer matches what the code
//! makes, and `CADRS_BLESS=1 cargo test -p cadrs_app --lib docs_reference` writes them again.

use std::path::PathBuf;

/// The line every generated page starts with.
pub const GENERATED: &str = "<!-- Made from the code: CADRS_BLESS=1 cargo test -p cadrs_app --lib \
                             docs_reference writes this page again. Don't edit it by hand. -->\n\n";

/// `text` as Markdown text: what Markdown would read as markup is escaped.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '*' | '_' | '`' | '|' | '[' | ']' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// A shortcut's keys as keycaps: ` / ` between alternatives, `+` in a chord.
pub fn keycaps(keys: &str) -> String {
    keys.split(" / ")
        .map(|alt| {
            alt.split('+')
                .map(|k| format!("<kbd>{}</kbd>", escape(k)))
                .collect::<Vec<_>>()
                .join("+")
        })
        .collect::<Vec<_>>()
        .join(" or ")
}

fn page(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/src/reference")
        .join(file)
}

/// Checks the committed page against `now`, or writes it with `CADRS_BLESS`.
fn check(file: &str, now: &str) {
    let path = page(file);
    if std::env::var_os("CADRS_BLESS").is_some() {
        std::fs::write(&path, now).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        return;
    }
    let was = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        was == now,
        "{} doesn't match the code: run `CADRS_BLESS=1 cargo test -p cadrs_app --lib \
         docs_reference` and commit the page",
        path.display()
    );
}

#[test]
fn the_keyboard_shortcuts_page_is_up_to_date() {
    check(
        "keyboard-shortcuts.md",
        &crate::shortcuts::reference_markdown(),
    );
}

#[test]
fn the_preferences_page_is_up_to_date() {
    check(
        "preferences.md",
        &crate::preferences_dialog::reference_markdown(),
    );
}

#[test]
fn keycaps_split_alternatives_and_chords() {
    assert_eq!(
        keycaps("Ctrl+Y / Ctrl+Shift+Z"),
        "<kbd>Ctrl</kbd>+<kbd>Y</kbd> or <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>Z</kbd>"
    );
    assert_eq!(keycaps("`"), "<kbd>\\`</kbd>");
}
