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
//! ([`Preferences::autoconnect_p2p`]: off unless the user turns it on, but on in a browser,
//! where the page is how people try cadrs, public documents among it).

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

/// A choice saved by its code, so that a code this cadrs doesn't know (a newer one's) reads as
/// the default instead of making the whole file unreadable (which would reset every preference).
macro_rules! coded_choice {
    ($ty:ident { $($variant:ident => $code:literal),+ $(,)? }) => {
        impl $ty {
            pub const ALL: &'static [$ty] = &[$($ty::$variant),+];

            /// Its code, as saved.
            pub fn code(self) -> &'static str {
                match self {
                    $($ty::$variant => $code),+
                }
            }

            /// The choice with this code, if there is one.
            pub fn from_code(code: &str) -> Option<$ty> {
                $ty::ALL.iter().copied().find(|c| c.code() == code)
            }
        }

        impl Serialize for $ty {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(self.code())
            }
        }

        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let code = String::deserialize(d)?;
                Ok($ty::from_code(&code).unwrap_or_default())
            }
        }
    };
}

/// How the ground grid under the 3D view looks (Preferences › Interface › Grid).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GridStyle {
    /// No grid.
    Off,
    /// A faint line on every step and a small cross on every fifth line (the schematic's).
    Crossed,
    /// A faint line on every step and a stronger one on every tenth.
    #[default]
    Lines,
}

coded_choice!(GridStyle {
    Off => "off",
    Crossed => "crossed",
    Lines => "lines",
});

/// Which way of moving a SpaceMouse's cap zooms (Preferences › Navigation › SpaceMouse); the
/// other of the two moves the view up and down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SpaceMouseZoom {
    /// Pulling the cap up and pushing it down.
    #[default]
    UpDown,
    /// Moving the cap forward (away from you) and backward.
    ForwardBackward,
}

coded_choice!(SpaceMouseZoom {
    UpDown => "up-down",
    ForwardBackward => "forward-backward",
});

/// How the origin is drawn in the 3D view (Preferences › Interface › Origin; the eye on the
/// feature list's Origin row hides and shows it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum OriginStyle {
    /// Not drawn, and not picked in the view (the feature list's row still picks it).
    Hidden,
    /// A small bullseye, Onshape's.
    #[default]
    Dot,
    /// Short red, green and blue lines along X, Y and Z.
    Axes,
}

coded_choice!(OriginStyle {
    Hidden => "hidden",
    Dot => "dot",
    Axes => "axes",
});

/// What the frame-stats panel at the 3D view's bottom left shows (Preferences › Interface).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FrameStatsMode {
    /// Nothing.
    #[default]
    Off,
    /// The frame rate line alone.
    Fps,
    /// The frame rate and where the frame's time goes.
    Details,
}

coded_choice!(FrameStatsMode {
    Off => "off",
    Fps => "fps",
    Details => "details",
});

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
    /// Off by default in the app, on in a browser (the page, and the site's embed).
    #[serde(default = "default_autoconnect")]
    pub autoconnect_p2p: bool,
    /// While online, take part in the public documents room (`cadrs_net::public`): find the
    /// documents others published, and help others find them. On unless the user turns it off.
    #[serde(default = "default_true")]
    pub public_documents: bool,
    /// Time every system and schedule, for the frame panel's details and
    /// `get_frame_stats`. It costs about a fifth of each frame, so it's off unless wanted, and
    /// takes effect at the next start (`cadrs_app::frame_profile::CpuProfiling`).
    #[serde(default)]
    pub profile_cpu: bool,
    /// Vertical sync: frames wait for the display's refresh (on by default). Off, they are drawn
    /// as fast as they are made, which shows what a frame really costs but uses more power.
    #[serde(default = "default_vsync")]
    pub vsync: bool,
    /// The interface's language (issue #178; `cadrs_ui::i18n`). One this cadrs doesn't know (a
    /// newer one's) reads as English, the other preferences kept.
    #[serde(default)]
    pub language: Language,
    /// The slicer parts were last sent to ([`crate::slicer::Slicer::id`]): listed first.
    #[serde(default)]
    pub last_slicer: Option<u8>,
    /// Keep the camera up (issue #97): the rotate drag turns about the vertical (world Z) and
    /// tilts, without rolling the view, and so do the arrow keys and the view cube's arrows.
    /// Off, the drag turns freely about the screen (Onshape's), and the no-roll turn is on the
    /// other gesture.
    #[serde(default)]
    pub keep_camera_up: bool,
    /// Turn the view normal to the sketch plane when a sketch starts (once it has its plane) and
    /// when one is opened for editing, as `N` does (issue #265, like SolidWorks's "Auto-rotate
    /// view normal to sketch plane"). Off, the view stays where it is, as in Onshape.
    #[serde(default)]
    pub sketch_view_normal: bool,
    /// The ground grid's look, or none ([`GridStyle`]).
    #[serde(default)]
    pub grid: GridStyle,
    /// What the frame-stats panel shows ([`FrameStatsMode`]); nothing unless asked for.
    #[serde(default)]
    pub frame_stats: FrameStatsMode,
    /// How the origin is drawn ([`OriginStyle`]).
    #[serde(default)]
    pub origin: OriginStyle,
    /// Which way of moving a SpaceMouse's cap zooms ([`SpaceMouseZoom`]).
    #[serde(default)]
    pub space_mouse_zoom: SpaceMouseZoom,
    /// A SpaceMouse's speed, pan, zoom and turn alike, in percent of the default (one of
    /// [`SPACE_MOUSE_SPEEDS`]).
    #[serde(default = "default_space_mouse_speed")]
    pub space_mouse_speed: u16,
}

/// A language cadrs's interface has a catalog for (`cadrs_ui::i18n::LANGUAGES`, in the same
/// order), saved as its code (`"pt-BR"`); a code this cadrs doesn't know (a newer one's) reads as
/// English, the other preferences kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    Afrikaans,
    German,
    #[default]
    English,
    Spanish,
    French,
    Italian,
    Dutch,
    PortugueseBrazil,
    Russian,
    Japanese,
    ChineseSimplified,
    Korean,
}

impl Serialize for Language {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.code())
    }
}

impl<'de> Deserialize<'de> for Language {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let code = String::deserialize(d)?;
        Ok(Language::from_code(&code).unwrap_or_default())
    }
}

impl Language {
    pub const ALL: [Language; 12] = [
        Language::Afrikaans,
        Language::German,
        Language::English,
        Language::Spanish,
        Language::French,
        Language::Italian,
        Language::Dutch,
        Language::PortugueseBrazil,
        Language::Russian,
        Language::Japanese,
        Language::ChineseSimplified,
        Language::Korean,
    ];

    /// The language with this code (BCP 47), if cadrs has it.
    pub fn from_code(code: &str) -> Option<Language> {
        Language::ALL.into_iter().find(|l| l.code() == code)
    }

    /// Its code (BCP 47), the catalog's file name.
    pub fn code(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::German => "de",
            Language::French => "fr",
            Language::Spanish => "es",
            Language::Italian => "it",
            Language::Dutch => "nl",
            Language::PortugueseBrazil => "pt-BR",
            Language::Russian => "ru",
            Language::Afrikaans => "af",
            Language::ChineseSimplified => "zh-CN",
            Language::Japanese => "ja",
            Language::Korean => "ko",
        }
    }
}

fn default_true() -> bool {
    true
}

/// Online at start in a browser only.
fn default_autoconnect() -> bool {
    cfg!(target_arch = "wasm32")
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
            autoconnect_p2p: default_autoconnect(),
            public_documents: true,
            profile_cpu: false,
            vsync: true,
            language: Language::English,
            last_slicer: None,
            keep_camera_up: false,
            sketch_view_normal: false,
            grid: GridStyle::default(),
            frame_stats: FrameStatsMode::default(),
            origin: OriginStyle::default(),
            space_mouse_zoom: SpaceMouseZoom::default(),
            space_mouse_speed: default_space_mouse_speed(),
        }
    }
}

/// The SpaceMouse speeds offered (percent of the default).
pub const SPACE_MOUSE_SPEEDS: [u16; 7] = [25, 50, 75, 100, 150, 200, 300];

fn default_space_mouse_speed() -> u16 {
    100
}

/// The interface sizes offered (percent).
pub const UI_SCALES: [u16; 8] = [75, 90, 100, 110, 125, 150, 175, 200];

fn default_ui_scale() -> u16 {
    100
}

impl Preferences {
    /// The SpaceMouse's speed as a factor (1 is the default), kept within the speeds offered.
    pub fn space_mouse_speed_factor(&self) -> f32 {
        f32::from(self.space_mouse_speed.clamp(
            SPACE_MOUSE_SPEEDS[0],
            SPACE_MOUSE_SPEEDS[SPACE_MOUSE_SPEEDS.len() - 1],
        )) / 100.0
    }

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
            public_documents: false,
            profile_cpu: true,
            vsync: false,
            language: Language::PortugueseBrazil,
            last_slicer: Some(0),
            keep_camera_up: true,
            sketch_view_normal: true,
            grid: GridStyle::Off,
            frame_stats: FrameStatsMode::Fps,
            origin: OriginStyle::Axes,
            space_mouse_zoom: SpaceMouseZoom::ForwardBackward,
            space_mouse_speed: 150,
        };
        p.save(&dir).unwrap();
        assert_eq!(Preferences::load(&dir), p);
        // A language this cadrs doesn't know reads as English, the rest kept.
        let saved = cadrs_fs::read_to_string(Preferences::path(&dir)).unwrap();
        assert!(saved.contains("\"pt-BR\""), "{saved}");
        cadrs_fs::write(
            Preferences::path(&dir),
            saved.replace("\"pt-BR\"", "\"tlh\""),
        )
        .unwrap();
        assert_eq!(
            Preferences::load(&dir),
            Preferences {
                language: Language::English,
                ..p
            }
        );
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

    /// The grid and the frame panel are saved by code; one this cadrs doesn't know (a newer
    /// one's) reads as the default and keeps the other preferences.
    #[test]
    fn grid_and_frame_stats_read_by_code() {
        let p = Preferences::default();
        assert_eq!(p.grid, GridStyle::Lines);
        assert_eq!(p.frame_stats, FrameStatsMode::Off);
        assert_eq!(p.origin, OriginStyle::Dot);
        let text = ron::to_string(&Preferences {
            grid: GridStyle::Crossed,
            frame_stats: FrameStatsMode::Details,
            ..p
        })
        .unwrap();
        assert!(text.contains(r#"grid:"crossed""#), "{text}");
        let back: Preferences = ron::from_str(&text).unwrap();
        assert_eq!(back.grid, GridStyle::Crossed);
        assert_eq!(back.frame_stats, FrameStatsMode::Details);
        let newer: Preferences =
            ron::from_str(r#"(grid: "dots", frame_stats: "graph", ui_scale: 125)"#).unwrap();
        assert_eq!(newer.grid, GridStyle::Lines);
        assert_eq!(newer.frame_stats, FrameStatsMode::Off);
        assert_eq!(newer.ui_scale, 125);
    }
}
