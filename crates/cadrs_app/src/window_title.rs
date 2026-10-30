//! The window's title says what is open, as a browser's says the page: the Documents page, or
//! the document with its open tab and the tab's type, most specific first so a cut-short title
//! (the taskbar, Alt+Tab) still tells several cadrs windows apart:
//!
//! - `Documents – cadrs`
//! - `Bracket – Part Studio 1 (Part Studio) – cadrs`

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::{ActiveDocument, AppState};

pub struct WindowTitlePlugin;

impl Plugin for WindowTitlePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, sync_window_title);
    }
}

/// The title for the app in `state`, with the open document's name and its tab's name and type.
pub fn title(state: AppState, doc: Option<(&str, Option<(&str, &str)>)>) -> String {
    match (state, doc) {
        (AppState::Document, Some((doc, Some((tab, kind))))) => {
            format!("{doc} – {tab} ({kind}) – cadrs")
        }
        (AppState::Document, Some((doc, None))) => format!("{doc} – cadrs"),
        (AppState::Landing, _) => "Documents – cadrs".into(),
        _ => "cadrs".into(),
    }
}

fn sync_window_title(
    state: Res<State<AppState>>,
    doc: Option<Res<ActiveDocument>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    let doc = doc.as_deref();
    let tab = doc
        .and_then(|d| d.active_element())
        .map(|el| (el.name.as_str(), crate::tab_preview::tab_type(el)));
    let want = title(*state.get(), doc.map(|d| (d.doc.name.as_str(), tab)));
    if window.title != want {
        window.title = want;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_title_names_what_is_open() {
        assert_eq!(title(AppState::Landing, None), "Documents – cadrs");
        assert_eq!(
            title(
                AppState::Document,
                Some(("Bracket", Some(("Part Studio 1", "Part Studio"))))
            ),
            "Bracket – Part Studio 1 (Part Studio) – cadrs"
        );
        assert_eq!(
            title(AppState::Document, Some(("Bracket", None))),
            "Bracket – cadrs"
        );
        assert_eq!(title(AppState::Document, None), "cadrs");
    }
}
