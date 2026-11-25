//! The way into the user documentation at <https://cadrs.net/docs/> (#273): the app menu's
//! **Documentation** (F1) opens the book, and the "?" at the foot of a feature's dialog
//! ([`help_icon`]) opens the feature's section of it. [`PAGES`] says where each dialog's section
//! is; the tests check every one of them against the book in `docs/src/`.

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::Button as WidgetButton;
use bevy::ui_widgets::{Activate, observe};
use cadrs_ui::ShortcutKey;
use cadrs_ui::i18n::tr;
use cadrs_ui::prelude::*;

/// The book's home page. A page in it is this followed by its path, such as
/// `features/sketch.html`.
pub const DOCS: &str = "https://cadrs.net/docs/";

/// Each feature dialog's section of the book: (the dialog's name, its page and anchor). The
/// names are the dialogs' own (`applied_dialog`'s, and the others' `Name`s without `-help`); an
/// anchor is the slug mdBook makes of the section's heading.
pub const PAGES: &[(&str, &str)] = &[
    ("sketch", "features/sketch.html"),
    // Part Studio features.
    ("extrude", "features/part-studio.html#extrude"),
    ("revolve", "features/part-studio.html#revolve"),
    ("sweep", "features/part-studio.html#sweep"),
    ("loft", "features/part-studio.html#loft"),
    ("fillet", "features/part-studio.html#fillet"),
    ("chamfer", "features/part-studio.html#chamfer"),
    ("shell", "features/part-studio.html#shell"),
    ("draft", "features/part-studio.html#draft"),
    ("hole", "features/part-studio.html#hole"),
    ("thread", "features/part-studio.html#thread"),
    ("boolean", "features/part-studio.html#boolean"),
    ("split", "features/part-studio.html#split"),
    ("transform", "features/part-studio.html#transform"),
    ("linear-pattern", "features/part-studio.html#linear-pattern"),
    (
        "circular-pattern",
        "features/part-studio.html#circular-pattern",
    ),
    ("curve-pattern", "features/part-studio.html#curve-pattern"),
    ("mirror", "features/part-studio.html#mirror"),
    ("plane", "features/part-studio.html#plane"),
    ("mate-connector", "features/part-studio.html#mate-connector"),
    // Surfaces, direct edits and more.
    ("thicken", "features/more-part-studio.html#thicken"),
    ("helix", "features/more-part-studio.html#helix"),
    ("fill", "features/more-part-studio.html#fill"),
    ("move-face", "features/more-part-studio.html#move-face"),
    ("delete-face", "features/more-part-studio.html#delete-face"),
    ("simplify", "features/more-part-studio.html#simplify"),
    ("gear", "features/more-part-studio.html#gear"),
    ("frame", "features/more-part-studio.html#frame"),
    ("composite", "features/more-part-studio.html#composite-part"),
    ("derived", "features/more-part-studio.html#derived"),
    // Sheet metal.
    (
        "sheet-metal-model",
        "features/sheet-metal.html#sheet-metal-model",
    ),
    ("sheet-metal-flange", "features/sheet-metal.html#flange"),
    ("sheet-metal-hem", "features/sheet-metal.html#hem"),
    (
        "sheet-metal-make-joint",
        "features/sheet-metal.html#make-joint",
    ),
    ("modify-joint", "features/sheet-metal.html#modify-joint"),
    ("sheet-metal-finish", "features/sheet-metal.html#finish"),
    ("sheet-metal-tab", "features/sheet-metal.html#tab"),
    ("sheet-metal-bend", "features/sheet-metal.html#bend"),
    ("sheet-metal-jog", "features/sheet-metal.html#jog"),
    ("sheet-metal-corner", "features/sheet-metal.html#corner"),
    (
        "sheet-metal-bend-relief",
        "features/sheet-metal.html#bend-relief",
    ),
    (
        "sheet-metal-corner-break",
        "features/sheet-metal.html#corner-break",
    ),
    (
        "sheet-metal-loft",
        "features/sheet-metal.html#sheet-metal-loft",
    ),
    ("sheet-metal-form", "features/sheet-metal.html#form"),
    ("tag", "features/sheet-metal.html#tag"),
    ("flat-extrude", "features/sheet-metal.html#flat-extrude"),
    // Assemblies.
    ("mate", "features/assembly.html#mate"),
    (
        "assembly-mate-connector",
        "features/assembly.html#mate-connector",
    ),
    ("relation", "features/assembly.html#relation"),
    ("group", "features/assembly.html#group"),
    ("animate", "features/assembly.html#animate"),
];

/// The page (and anchor) of dialog `dialog`'s section, if [`PAGES`] has one.
pub fn page_of(dialog: &str) -> Option<&'static str> {
    PAGES.iter().find(|(d, _)| *d == dialog).map(|(_, p)| *p)
}

/// Opens `page` of the book in the user's browser ("" for its home page).
pub fn open_docs(page: &str) {
    crate::open_url(&format!("{DOCS}{page}"));
}

/// The "?" at the foot of dialog `dialog`'s panel, named `name`: a click opens the dialog's
/// section of the book ([`PAGES`]), or the book's home page if it has none.
pub fn help_icon(name: impl Into<std::borrow::Cow<'static, str>>, dialog: &str) -> impl Bundle {
    let page = page_of(dialog).unwrap_or_else(|| {
        warn!("no documentation page for the {dialog} dialog");
        ""
    });
    (
        Name::new(name),
        cadrs_ui::icon("help-filled", 14.0, Color::srgb_u8(0xa8, 0xa8, 0xa8)),
        Tooltip::new(tr("common-help")),
        // A button, for the pointing hand and the click.
        WidgetButton,
        Hovered::default(),
        observe(move |_: On<Activate>| open_docs(page)),
    )
}

pub struct HelpPlugin;

impl Plugin for HelpPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, open_on_f1);
    }
}

/// F1 opens the book, as in most programs (also while typing: F1 types nothing).
fn open_on_f1(mut keys_in: MessageReader<KeyboardInput>) {
    if keys_in
        .read()
        .any(|k| k.state == ButtonState::Pressed && !k.repeat && k.shortcut_code() == KeyCode::F1)
    {
        open_docs("");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn src() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/src")
    }

    /// The anchor mdBook gives a heading: lower case, spaces to `-`, only letters, digits, `-`
    /// and `_` kept.
    fn slug(heading: &str) -> String {
        heading
            .trim()
            .chars()
            .filter_map(|c| match c {
                ' ' => Some('-'),
                c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c.to_ascii_lowercase()),
                _ => None,
            })
            .collect()
    }

    /// Every dialog's section is in the book: its page exists and is in the book's contents,
    /// and it has a heading with that anchor. A link to a page that isn't there would land on
    /// the site's 404, which sends the user to the app instead.
    #[test]
    fn every_help_link_has_its_section_in_the_book() {
        let summary = std::fs::read_to_string(src().join("SUMMARY.md")).unwrap();
        for (dialog, link) in PAGES {
            let (page, anchor) = link.split_once('#').unwrap_or((link, ""));
            let md = page.replace(".html", ".md");
            let text = std::fs::read_to_string(src().join(&md))
                .unwrap_or_else(|e| panic!("{dialog}: docs/src/{md}: {e}"));
            assert!(
                summary.contains(&format!("({md})")),
                "{dialog}: {md} isn't in docs/src/SUMMARY.md"
            );
            if !anchor.is_empty() {
                assert!(
                    text.lines()
                        .filter_map(|l| l
                            .trim_start_matches('#')
                            .strip_prefix(' ')
                            .filter(|_| l.starts_with('#')))
                        .any(|h| slug(h) == anchor),
                    "{dialog}: docs/src/{md} has no heading for #{anchor}"
                );
            }
        }
    }

    #[test]
    fn every_dialog_is_listed_once() {
        for (i, (d, _)) in PAGES.iter().enumerate() {
            assert!(
                !PAGES[..i].iter().any(|(e, _)| e == d),
                "{d} is listed twice"
            );
        }
    }

    #[test]
    fn slugs_match_mdbook() {
        assert_eq!(slug("Linear pattern"), "linear-pattern");
        assert_eq!(
            slug("Surfaces, direct edits and more"),
            "surfaces-direct-edits-and-more"
        );
    }
}
