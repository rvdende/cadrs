//! Local preferences (P3E.3, TD6.2, D2.2): what this user prefers on this machine, kept beside
//! the documents in `<store root>/preferences.ron` rather than in any document. So far the
//! **3D view mouse controls**: which mouse gestures rotate, pan and zoom the view, as one of the
//! presets Onshape offers ([`MousePreset`]). Part Studio and Assembly viewports read the
//! rotate, pan and zoom gestures; a drawing sheet has no rotate, so its rotate gestures pan it
//! ([`MousePreset::sheet_action`]), as Onshape's do (D2.2: right- or middle-drag pans).
//!
//! The wheel zooms in every preset.
//!
//! And the **MCP server** ([`McpPreferences`]): whether AI assistants may drive the app over
//! the Model Context Protocol, and on which loopback port.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The file the preferences live in, in the document store's root.
pub const PREFERENCES_FILE: &str = "preferences.ron";

/// A mouse button that can drive the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

impl MouseButton {
    pub fn label(self) -> &'static str {
        match self {
            MouseButton::Left => "Left",
            MouseButton::Right => "Right",
            MouseButton::Middle => "Middle",
        }
    }
}

/// The modifier keys held during a drag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

impl Modifiers {
    pub const NONE: Modifiers = Modifiers { shift: false, ctrl: false, alt: false };
    pub const SHIFT: Modifiers = Modifiers { shift: true, ctrl: false, alt: false };
    pub const CTRL: Modifiers = Modifiers { shift: false, ctrl: true, alt: false };
    pub const ALT: Modifiers = Modifiers { shift: false, ctrl: false, alt: true };

    /// True if every key `self` names is held in `held`.
    fn held_in(self, held: Modifiers) -> bool {
        (!self.shift || held.shift) && (!self.ctrl || held.ctrl) && (!self.alt || held.alt)
    }

    fn count(self) -> usize {
        self.shift as usize + self.ctrl as usize + self.alt as usize
    }

    /// "Ctrl+", "Shift+", "" …
    fn prefix(self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s += "Ctrl+";
        }
        if self.shift {
            s += "Shift+";
        }
        if self.alt {
            s += "Alt+";
        }
        s
    }
}

/// What a drag does to the 3D view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewAction {
    /// Free rotation about the screen axes.
    Rotate,
    /// Rotation that keeps Z up (Onshape's Alt+right-drag).
    RotateTurntable,
    Pan,
    /// A vertical drag zooms (up zooms in) about where it started.
    Zoom,
}

impl ViewAction {
    pub fn label(self) -> &'static str {
        match self {
            ViewAction::Rotate => "Rotate",
            ViewAction::RotateTurntable => "Rotate without roll",
            ViewAction::Pan => "Pan",
            ViewAction::Zoom => "Zoom",
        }
    }
}

/// What a drag does to a drawing sheet (it has no rotate).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SheetAction {
    Pan,
    Zoom,
}

/// One binding: a button with modifiers held does an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub button: MouseButton,
    pub modifiers: Modifiers,
    pub action: ViewAction,
}

const fn bind(button: MouseButton, modifiers: Modifiers, action: ViewAction) -> Binding {
    Binding { button, modifiers, action }
}

use MouseButton::{Middle, Right};
use ViewAction::{Pan, Rotate, RotateTurntable, Zoom};

const ONSHAPE: &[Binding] = &[
    bind(Right, Modifiers::NONE, Rotate),
    bind(Right, Modifiers::ALT, RotateTurntable),
    bind(Middle, Modifiers::NONE, Pan),
    bind(Right, Modifiers::CTRL, Pan),
    bind(Middle, Modifiers::SHIFT, Zoom),
];
const SOLIDWORKS: &[Binding] = &[
    bind(Middle, Modifiers::NONE, Rotate),
    bind(Middle, Modifiers::CTRL, Pan),
    bind(Middle, Modifiers::SHIFT, Zoom),
];
const INVENTOR: &[Binding] = &[
    bind(Middle, Modifiers::SHIFT, Rotate),
    bind(Middle, Modifiers::NONE, Pan),
    bind(Middle, Modifiers::CTRL, Zoom),
];
const CREO: &[Binding] = &[
    bind(Middle, Modifiers::NONE, Rotate),
    bind(Middle, Modifiers::SHIFT, Pan),
    bind(Middle, Modifiers::CTRL, Zoom),
];

/// The 3D view mouse controls: a preset like Onshape's (My account → Preferences → 3D view
/// mouse controls).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum MousePreset {
    /// Right-drag rotates (Alt: without roll), middle- or Ctrl+right-drag pans,
    /// Shift+middle-drag zooms.
    #[default]
    Onshape,
    /// Middle-drag rotates, Ctrl+middle pans, Shift+middle zooms.
    SolidWorks,
    /// Shift+middle-drag rotates, middle pans, Ctrl+middle zooms.
    Inventor,
    /// Middle-drag rotates, Shift+middle pans, Ctrl+middle zooms.
    Creo,
}

impl MousePreset {
    pub const ALL: [MousePreset; 4] = [MousePreset::Onshape, MousePreset::SolidWorks, MousePreset::Inventor, MousePreset::Creo];

    pub fn label(self) -> &'static str {
        match self {
            MousePreset::Onshape => "Onshape",
            MousePreset::SolidWorks => "SolidWorks",
            MousePreset::Inventor => "Inventor",
            MousePreset::Creo => "Creo",
        }
    }

    /// The preset's bindings.
    pub fn bindings(self) -> &'static [Binding] {
        match self {
            MousePreset::Onshape => ONSHAPE,
            MousePreset::SolidWorks => SOLIDWORKS,
            MousePreset::Inventor => INVENTOR,
            MousePreset::Creo => CREO,
        }
    }

    /// What a drag with `button` and `held` modifiers does: the binding of that button whose
    /// modifiers are all held, the one with the most modifiers winning (so Ctrl+right pans in
    /// Onshape while right rotates). `None`: the drag does nothing to the view.
    pub fn action(self, button: MouseButton, held: Modifiers) -> Option<ViewAction> {
        self.bindings()
            .iter()
            .filter(|b| b.button == button && b.modifiers.held_in(held))
            .max_by_key(|b| b.modifiers.count())
            .map(|b| b.action)
    }

    /// True if a press of `button` can start a view drag (with some modifiers).
    pub fn navigates_with(self, button: MouseButton) -> bool {
        self.bindings().iter().any(|b| b.button == button)
    }

    /// What a drag does on a drawing sheet: its rotate and pan gestures pan, its zoom gestures
    /// zoom.
    pub fn sheet_action(self, button: MouseButton, held: Modifiers) -> Option<SheetAction> {
        Some(match self.action(button, held)? {
            Rotate | RotateTurntable | Pan => SheetAction::Pan,
            Zoom => SheetAction::Zoom,
        })
    }

    /// How an action is done, for the preferences: "Right drag", "Ctrl+Middle drag", …, with
    /// "Scroll wheel" for zoom.
    pub fn gestures(self, action: ViewAction) -> Vec<String> {
        let mut out: Vec<String> = self
            .bindings()
            .iter()
            .filter(|b| b.action == action)
            .map(|b| format!("{}{} drag", b.modifiers.prefix(), b.button.label()))
            .collect();
        if action == Zoom {
            out.insert(0, "Scroll wheel".into());
        }
        out
    }
}

/// The MCP server (Model Context Protocol, `cadrs_mcp`): off unless turned on; when on, the app
/// serves `http://127.0.0.1:<port>/mcp` for AI assistants on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpPreferences {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_mcp_port")]
    pub port: u16,
}

/// `cadrs_mcp::DEFAULT_PORT` (this crate doesn't depend on the server).
pub const DEFAULT_MCP_PORT: u16 = 7680;

fn default_mcp_port() -> u16 {
    DEFAULT_MCP_PORT
}

impl Default for McpPreferences {
    fn default() -> Self {
        Self { enabled: false, port: DEFAULT_MCP_PORT }
    }
}

/// The local preferences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Preferences {
    #[serde(default)]
    pub mouse: MousePreset,
    #[serde(default)]
    pub mcp: McpPreferences,
}

impl Preferences {
    /// The preferences file in a store root.
    pub fn path(root: &Path) -> PathBuf {
        root.join(PREFERENCES_FILE)
    }

    /// Reads the preferences from a store root; the defaults when there is no file or it can't
    /// be read.
    pub fn load(root: &Path) -> Self {
        std::fs::read_to_string(Self::path(root)).ok().and_then(|s| ron::from_str(&s).ok()).unwrap_or_default()
    }

    /// Writes the preferences into a store root.
    pub fn save(&self, root: &Path) -> io::Result<()> {
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).map_err(io::Error::other)?;
        std::fs::create_dir_all(root)?;
        let path = Self::path(root);
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use MouseButton::Left;

    #[test]
    fn the_mapping_table() {
        let none = Modifiers::NONE;
        let (shift, ctrl, alt) = (Modifiers::SHIFT, Modifiers::CTRL, Modifiers::ALT);
        let p = MousePreset::Onshape;
        assert_eq!(p.action(Right, none), Some(Rotate));
        assert_eq!(p.action(Right, alt), Some(RotateTurntable));
        assert_eq!(p.action(Right, ctrl), Some(Pan));
        assert_eq!(p.action(Middle, none), Some(Pan));
        assert_eq!(p.action(Middle, shift), Some(Zoom));
        // Shift adds nothing to a right drag: it still rotates.
        assert_eq!(p.action(Right, shift), Some(Rotate));
        assert_eq!(p.action(Left, none), None);

        let p = MousePreset::SolidWorks;
        assert_eq!(p.action(Middle, none), Some(Rotate));
        assert_eq!(p.action(Middle, ctrl), Some(Pan));
        assert_eq!(p.action(Middle, shift), Some(Zoom));
        assert_eq!(p.action(Right, none), None);
        assert!(!p.navigates_with(Right));

        let p = MousePreset::Inventor;
        assert_eq!(p.action(Middle, none), Some(Pan));
        assert_eq!(p.action(Middle, shift), Some(Rotate));
        assert_eq!(p.action(Middle, ctrl), Some(Zoom));

        let p = MousePreset::Creo;
        assert_eq!(p.action(Middle, none), Some(Rotate));
        assert_eq!(p.action(Middle, shift), Some(Pan));
        assert_eq!(p.action(Middle, ctrl), Some(Zoom));
        assert_eq!(p.action(Right, none), None);
    }

    #[test]
    fn sheets_pan_with_the_rotate_and_pan_gestures() {
        // D2.2: Onshape's sheet pans with a right or a middle drag.
        let p = MousePreset::Onshape;
        assert_eq!(p.sheet_action(Right, Modifiers::NONE), Some(SheetAction::Pan));
        assert_eq!(p.sheet_action(Middle, Modifiers::NONE), Some(SheetAction::Pan));
        assert_eq!(p.sheet_action(Middle, Modifiers::SHIFT), Some(SheetAction::Zoom));
        let p = MousePreset::SolidWorks;
        assert_eq!(p.sheet_action(Right, Modifiers::NONE), None);
        assert_eq!(p.sheet_action(Middle, Modifiers::NONE), Some(SheetAction::Pan));
    }

    #[test]
    fn gestures_read_for_the_preferences() {
        assert_eq!(MousePreset::Onshape.gestures(Pan), vec!["Middle drag", "Ctrl+Right drag"]);
        assert_eq!(MousePreset::SolidWorks.gestures(Zoom), vec!["Scroll wheel", "Shift+Middle drag"]);
        assert_eq!(MousePreset::Creo.gestures(Rotate), vec!["Middle drag"]);
    }

    #[test]
    fn preferences_round_trip_through_the_store_root() {
        let dir = std::env::temp_dir().join(format!("cadrs-prefs-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(Preferences::load(&dir), Preferences::default());
        let p = Preferences { mouse: MousePreset::SolidWorks, mcp: McpPreferences { enabled: true, port: 7700 } };
        p.save(&dir).unwrap();
        assert_eq!(Preferences::load(&dir), p);
        // A file from before the MCP server reads with it off.
        std::fs::write(Preferences::path(&dir), "(mouse: Creo)").unwrap();
        assert_eq!(Preferences::load(&dir), Preferences { mouse: MousePreset::Creo, mcp: McpPreferences::default() });
        // An unreadable file reads as the defaults.
        std::fs::write(Preferences::path(&dir), "not ron").unwrap();
        assert_eq!(Preferences::load(&dir), Preferences::default());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
