//! Local preferences (P3E.3, TD6.2, D2.2): what this user prefers on this machine, kept beside
//! the documents in `<store root>/preferences.ron` rather than in any document. So far the
//! **3D view mouse controls**: which mouse gestures rotate, pan and zoom the view, as one of the
//! presets Onshape offers ([`MousePreset`]). Part Studio and Assembly viewports read the
//! rotate, pan and zoom gestures; a drawing sheet has no rotate, so its rotate gestures pan it
//! ([`MousePreset::sheet_action`]), as Onshape's do (D2.2: right- or middle-drag pans).
//!
//! The wheel zooms in every preset.
//!
//! The **color scheme** ([`ColorScheme`]): light, dark, or whatever the system uses.
//!
//! The **MCP server** ([`McpPreferences`]): whether AI assistants may drive the app over
//! the Model Context Protocol, and on which loopback port.
//!
//! And whether cadrs goes online for sharing as soon as it starts
//! ([`Preferences::autoconnect_p2p`], off unless the user turns it on).

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
    pub const NONE: Modifiers = Modifiers {
        shift: false,
        ctrl: false,
        alt: false,
    };
    pub const SHIFT: Modifiers = Modifiers {
        shift: true,
        ctrl: false,
        alt: false,
    };
    pub const CTRL: Modifiers = Modifiers {
        shift: false,
        ctrl: true,
        alt: false,
    };
    pub const ALT: Modifiers = Modifiers {
        shift: false,
        ctrl: false,
        alt: true,
    };

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
    Binding {
        button,
        modifiers,
        action,
    }
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
    pub const ALL: [MousePreset; 4] = [
        MousePreset::Onshape,
        MousePreset::SolidWorks,
        MousePreset::Inventor,
        MousePreset::Creo,
    ];

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
        Self {
            enabled: false,
            port: DEFAULT_MCP_PORT,
        }
    }
}

/// The interface's color scheme (the top bar's light / dark / system toggle).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum ColorScheme {
    /// Light or dark as the system is.
    #[default]
    System,
    Light,
    Dark,
}

/// The local preferences.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Preferences {
    #[serde(default)]
    pub mouse: MousePreset,
    #[serde(default)]
    pub mcp: McpPreferences,
    /// Assembly rotation stops in degrees; zero disables snapping.
    #[serde(default = "default_rotation_snap_degrees")]
    pub rotation_snap_degrees: f64,
    /// Maximum angular distance from a rotation stop, in degrees.
    #[serde(default = "default_rotation_snap_tolerance_degrees")]
    pub rotation_snap_tolerance_degrees: f64,
    /// How large the interface is drawn, in percent of the display's own scale (issue #12).
    #[serde(default = "default_ui_scale")]
    pub ui_scale: u16,
    #[serde(default)]
    pub color_scheme: ColorScheme,
    /// The last edition of the app's "What's new" notice the user has seen.
    #[serde(default)]
    pub whats_new_seen: u32,
    /// Go online for sharing and collaboration (peer to peer, through iroh's relays) as soon
    /// as cadrs starts. Off, cadrs stays off the network until the user connects (issue #148).
    #[serde(default)]
    pub autoconnect_p2p: bool,
    /// Time every system and schedule, for the frame panel's details (F12) and
    /// `get_frame_stats`. It costs about a fifth of each frame, so it's off unless wanted, and
    /// takes effect at the next start (`cadrs_app::frame_profile::CpuProfiling`).
    #[serde(default)]
    pub profile_cpu: bool,
    /// Vertical sync: frames wait for the display's refresh (on by default). Off, they are drawn
    /// as fast as they are made, which shows what a frame really costs but uses more power.
    #[serde(default = "default_vsync")]
    pub vsync: bool,
}

fn default_vsync() -> bool {
    true
}

fn default_rotation_snap_degrees() -> f64 {
    45.0
}
fn default_rotation_snap_tolerance_degrees() -> f64 {
    10.0
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            mouse: MousePreset::default(),
            mcp: McpPreferences::default(),
            rotation_snap_degrees: default_rotation_snap_degrees(),
            rotation_snap_tolerance_degrees: default_rotation_snap_tolerance_degrees(),
            ui_scale: 100,
            color_scheme: ColorScheme::default(),
            whats_new_seen: 0,
            autoconnect_p2p: false,
            profile_cpu: false,
            vsync: true,
        }
    }
}

/// The interface sizes offered (percent).
pub const UI_SCALES: [u16; 8] = [75, 90, 100, 110, 125, 150, 175, 200];

fn default_ui_scale() -> u16 {
    100
}

impl Preferences {
    /// The interface's scale as a factor (1 is the display's own), kept within the sizes offered.
    pub fn ui_scale_factor(&self) -> f32 {
        f32::from(
            self.ui_scale
                .clamp(UI_SCALES[0], UI_SCALES[UI_SCALES.len() - 1]),
        ) / 100.0
    }
}

impl Preferences {
    pub fn rotation_snap_tolerance_degrees(&self) -> f64 {
        if self.rotation_snap_tolerance_degrees.is_finite()
            && (0.0..=90.0).contains(&self.rotation_snap_tolerance_degrees)
        {
            self.rotation_snap_tolerance_degrees
        } else {
            default_rotation_snap_tolerance_degrees()
        }
    }
    pub fn rotation_snap_degrees(&self) -> f64 {
        if self.rotation_snap_degrees.is_finite()
            && (0.0..=180.0).contains(&self.rotation_snap_degrees)
        {
            self.rotation_snap_degrees
        } else {
            default_rotation_snap_degrees()
        }
    }
    /// The preferences file in a store root.
    pub fn path(root: &Path) -> PathBuf {
        root.join(PREFERENCES_FILE)
    }

    /// Reads the preferences from a store root; the defaults when there is no file or it can't
    /// be read.
    pub fn load(root: &Path) -> Self {
        cadrs_fs::read_to_string(Self::path(root))
            .ok()
            .and_then(|s| ron::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Writes the preferences into a store root.
    pub fn save(&self, root: &Path) -> io::Result<()> {
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(io::Error::other)?;
        cadrs_fs::create_dir_all(root)?;
        let path = Self::path(root);
        let tmp = path.with_extension("tmp");
        cadrs_fs::write(&tmp, text)?;
        cadrs_fs::rename(&tmp, path)
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
        assert_eq!(
            p.sheet_action(Right, Modifiers::NONE),
            Some(SheetAction::Pan)
        );
        assert_eq!(
            p.sheet_action(Middle, Modifiers::NONE),
            Some(SheetAction::Pan)
        );
        assert_eq!(
            p.sheet_action(Middle, Modifiers::SHIFT),
            Some(SheetAction::Zoom)
        );
        let p = MousePreset::SolidWorks;
        assert_eq!(p.sheet_action(Right, Modifiers::NONE), None);
        assert_eq!(
            p.sheet_action(Middle, Modifiers::NONE),
            Some(SheetAction::Pan)
        );
    }

    #[test]
    fn gestures_read_for_the_preferences() {
        assert_eq!(
            MousePreset::Onshape.gestures(Pan),
            vec!["Middle drag", "Ctrl+Right drag"]
        );
        assert_eq!(
            MousePreset::SolidWorks.gestures(Zoom),
            vec!["Scroll wheel", "Shift+Middle drag"]
        );
        assert_eq!(MousePreset::Creo.gestures(Rotate), vec!["Middle drag"]);
    }

    #[test]
    fn invalid_snap_preferences_use_the_default_without_affecting_other_choices() {
        for value in [-1.0, 181.0, f64::NAN, f64::INFINITY] {
            let p = Preferences {
                rotation_snap_degrees: value,
                ..Preferences::default()
            };
            assert_eq!(p.rotation_snap_degrees(), 45.0);
        }
        assert_eq!(
            Preferences {
                rotation_snap_degrees: 0.0,
                ..Preferences::default()
            }
            .rotation_snap_degrees(),
            0.0
        );
        for value in [-1.0, 91.0, f64::NAN, f64::INFINITY] {
            let p = Preferences {
                rotation_snap_tolerance_degrees: value,
                ..Preferences::default()
            };
            assert_eq!(p.rotation_snap_tolerance_degrees(), 10.0);
        }
    }

    #[test]
    fn preferences_round_trip_through_the_store_root() {
        let dir = std::env::temp_dir().join(format!("cadrs-prefs-test-{}", std::process::id()));
        let _ = cadrs_fs::remove_dir_all(&dir);
        assert_eq!(Preferences::load(&dir), Preferences::default());
        let p = Preferences {
            mouse: MousePreset::SolidWorks,
            mcp: McpPreferences {
                enabled: true,
                port: 7700,
            },
            rotation_snap_degrees: 22.5,
            rotation_snap_tolerance_degrees: 15.0,
            ui_scale: 125,
            color_scheme: ColorScheme::Dark,
            whats_new_seen: 1,
            autoconnect_p2p: true,
            profile_cpu: true,
            vsync: false,
        };
        p.save(&dir).unwrap();
        assert_eq!(Preferences::load(&dir), p);
        // A file from before the MCP server (and P2P's switch) reads with both off.
        cadrs_fs::write(Preferences::path(&dir), "(mouse: Creo)").unwrap();
        assert_eq!(
            Preferences::load(&dir),
            Preferences {
                mouse: MousePreset::Creo,
                ..Preferences::default()
            }
        );
        // An unreadable file reads as the defaults.
        cadrs_fs::write(Preferences::path(&dir), "not ron").unwrap();
        assert_eq!(Preferences::load(&dir), Preferences::default());
        let _ = cadrs_fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_older_file_without_a_ui_scale_reads_as_100_percent() {
        let p: Preferences = ron::from_str("(mcp: (enabled: true, port: 7690))").unwrap();
        assert_eq!(p.ui_scale, 100);
        assert_eq!(p.ui_scale_factor(), 1.0);
        assert_eq!(p.color_scheme, ColorScheme::System);
        // From before the vsync switch: vsync on.
        assert!(p.vsync);
        assert_eq!(Preferences { ui_scale: 150, ..p }.ui_scale_factor(), 1.5);
        // Out of range: the nearest size offered.
        assert_eq!(
            Preferences {
                ui_scale: 1000,
                ..p
            }
            .ui_scale_factor(),
            2.0
        );
    }
}
