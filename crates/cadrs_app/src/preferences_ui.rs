//! The local preferences (P3E.3, TD6.2, D2.2): this machine's, not a document's, kept in the
//! store root's `preferences.ron` ([`cadrs_core::preferences`]), so nothing is undone. The app
//! menu's **Preferences…** opens their dialog ([`crate::preferences_dialog`]); this module holds
//! them in force ([`LocalPreferences`]), applies them, and keeps the ones set elsewhere.
//!
//! **UI scale** (issue #12): 75–200 % of the display's own scale ([`apply_ui_scale`]); Ctrl+= and
//! Ctrl+- step through its sizes anywhere, as a browser's zoom does ([`step_ui_scale`]).
//! **Vertical sync** (`vsync`, on by default; [`apply_vsync`]): `CADRS_NO_VSYNC=1` turns it off
//! for one run without saving. **Time systems** (`profile_cpu`): the CPU time of every system in
//! the frame panel's details and `get_frame_stats`, from the next start
//! ([`crate::frame_profile::CpuProfiling`]). **Grid**, **Origin** and **Frame stats** are read
//! where they're drawn (`crate::viewport`, `crate::frame_stats`); the eye on the feature list's
//! Origin row hides and shows the origin ([`toggle_origin`]).
//!
//! **Color scheme**: the top bar's light / dark / system button ([`cadrs_ui::scheme_toggle`])
//! is saved here too, as soon as it's clicked. `CADRS_COLOR_SCHEME=light|dark|system` overrides
//! it for one run without saving (scenarios use it to render in dark).
//!
//! *Keep camera up* (`keep_camera_up`, off by default, issue #97) is also set from the view
//! menu ([`set_keep_camera_up`]): every rotate gesture turns without roll. The top bar's AI
//! assistants button (`crate::mcp_button`) sets the MCP server's switch and port, and the network
//! button's menu *Connect when cadrs starts* (`autoconnect_p2p`).

use bevy::input::keyboard::KeyboardInput;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use cadrs_core::preferences::{
    ColorScheme, FrameStatsMode, GridStyle, Modifiers, MouseButton, MousePreset, OriginStyle,
    Preferences, UI_SCALES,
};
use cadrs_ui::i18n::{tr, tr_with};
use cadrs_ui::{ShortcutKey, Theme};

use crate::DocumentStore;

pub struct PreferencesPlugin;

impl Plugin for PreferencesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LocalPreferences>()
            .add_systems(Startup, load_preferences)
            .add_systems(
                Update,
                (
                    ui_scale_keys,
                    apply_ui_scale,
                    apply_vsync,
                    apply_language,
                    save_color_scheme,
                ),
            );
        crate::preferences_dialog::plugin(app);
    }
}

pub use crate::preferences_dialog::{Section, open_preferences, open_preferences_at};

/// The interface shows the language of the preferences (at start, and when it changes in the
/// Preferences dialog: texts already shown that come from the catalogs change with it).
fn apply_language(prefs: Res<LocalPreferences>, mut commands: Commands) {
    let code = prefs.0.language.code();
    if prefs.is_changed() && cadrs_ui::i18n::language() != code {
        cadrs_ui::i18n::set_language(code);
        // (A document's toolbars and top bar are built once: they follow when it is opened
        // again. The documents page is made again at once.)
        commands.queue(|world: &mut World| {
            let in_document = world
                .get_resource::<State<crate::AppState>>()
                .is_some_and(|s| *s.get() == crate::AppState::Document);
            if in_document && world.contains_resource::<cadrs_ui::Theme>() {
                crate::linked::toast(world, tr("preferences-language-later"));
            }
        });
    }
}

/// The preferences in force (read at startup from the store root).
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct LocalPreferences(pub Preferences);

impl LocalPreferences {
    pub fn mouse(&self) -> MousePreset {
        self.0.mouse
    }
}

/// A wheel scroll as zoom steps (> 0 zooms in): a line per notch, or per 40 px of a touchpad's
/// scroll, turned round when the wheel's zoom is inverted. Without preferences (a test's app),
/// rolling away zooms in.
pub fn wheel_zoom_lines(
    prefs: Option<&LocalPreferences>,
    unit: bevy::input::mouse::MouseScrollUnit,
    y: f32,
) -> f32 {
    let lines = match unit {
        bevy::input::mouse::MouseScrollUnit::Line => y,
        bevy::input::mouse::MouseScrollUnit::Pixel => y / 40.0,
    };
    prefs.map_or(lines, |p| p.0.wheel_zoom(lines))
}

fn load_preferences(
    store: Option<Res<DocumentStore>>,
    mut prefs: ResMut<LocalPreferences>,
    mut appearance: ResMut<cadrs_ui::Appearance>,
) {
    if let Some(store) = store {
        prefs.0 = Preferences::load(store.0.root());
    }
    // Before the first window is made (a document opened at start builds it in Startup).
    cadrs_ui::i18n::set_language(prefs.0.language.code());
    appearance.preference = to_ui(scheme_override().unwrap_or(prefs.0.color_scheme));
}

/// `CADRS_COLOR_SCHEME`, when set to a scheme.
pub(crate) fn scheme_override() -> Option<ColorScheme> {
    match std::env::var("CADRS_COLOR_SCHEME").ok()?.as_str() {
        "light" => Some(ColorScheme::Light),
        "dark" => Some(ColorScheme::Dark),
        "system" => Some(ColorScheme::System),
        _ => None,
    }
}

pub(crate) fn to_ui(c: ColorScheme) -> cadrs_ui::SchemePreference {
    match c {
        ColorScheme::System => cadrs_ui::SchemePreference::System,
        ColorScheme::Light => cadrs_ui::SchemePreference::Light,
        ColorScheme::Dark => cadrs_ui::SchemePreference::Dark,
    }
}

pub(crate) fn from_ui(p: cadrs_ui::SchemePreference) -> ColorScheme {
    match p {
        cadrs_ui::SchemePreference::System => ColorScheme::System,
        cadrs_ui::SchemePreference::Light => ColorScheme::Light,
        cadrs_ui::SchemePreference::Dark => ColorScheme::Dark,
    }
}

/// Saves the scheme the toggle picks (not one `CADRS_COLOR_SCHEME` set).
fn save_color_scheme(world: &mut World) {
    let picked = from_ui(world.resource::<cadrs_ui::Appearance>().preference);
    let prefs = world.resource::<LocalPreferences>().0;
    if prefs.color_scheme == picked || scheme_override() == Some(picked) {
        return;
    }
    world.resource_mut::<LocalPreferences>().0.color_scheme = picked;
    save(world);
}

/// Draws the interface at the preferred size (issue #12): the window's scale factor is the
/// display's own times the preference, so text, panels and the view's handles grow together and
/// pointer positions stay in the same units as the layout.
fn apply_ui_scale(
    prefs: Res<LocalPreferences>,
    mut windows: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
    mut applied: Local<Option<f32>>,
    mut original: Local<Option<Option<f32>>>,
) {
    let k = prefs.0.ui_scale_factor();
    if *applied == Some(k) {
        return;
    }
    let Ok(mut w) = windows.single_mut() else {
        return;
    };
    // The window's own override (a scenario replay's 1.0) is what 100% puts back.
    let own = *original.get_or_insert(w.resolution.scale_factor_override());
    if applied.is_none() && k == 1.0 {
        *applied = Some(k);
        return;
    }
    let want = if k == 1.0 {
        own
    } else {
        Some(own.unwrap_or(w.resolution.base_scale_factor()) * k)
    };
    w.resolution.set_scale_factor_override(want);
    *applied = Some(k);
}

/// Presents frames with vertical sync or without, as the preference says (`CADRS_NO_VSYNC=1`
/// turns it off for the run). On, the window's own present mode is put back.
fn apply_vsync(
    prefs: Res<LocalPreferences>,
    mut windows: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
    mut applied: Local<Option<bool>>,
    mut original: Local<Option<bevy::window::PresentMode>>,
) {
    let on = prefs.0.vsync && std::env::var_os("CADRS_NO_VSYNC").is_none();
    if *applied == Some(on) {
        return;
    }
    let Ok(mut w) = windows.single_mut() else {
        return;
    };
    let own = *original.get_or_insert(w.present_mode);
    let want = if on {
        own
    } else {
        bevy::window::PresentMode::AutoNoVsync
    };
    if w.present_mode != want {
        w.present_mode = want;
    }
    *applied = Some(on);
}

/// Sets and saves vertical sync (the dialog's OK; `set_preferences`).
pub fn set_vsync(world: &mut World, on: bool) {
    world.resource_mut::<LocalPreferences>().0.vsync = on;
    save(world);
}

/// Sets and saves the interface size in percent (the dialog's OK; scenarios too).
pub fn set_ui_scale(world: &mut World, percent: u16) {
    world.resource_mut::<LocalPreferences>().0.ui_scale = percent;
    save(world);
}

/// Ctrl+= (Ctrl++, the numpad's +) and Ctrl+- (the numpad's -): the next UI scale up or down
/// ([`step_ui_scale`]).
fn ui_scale_keys(
    mut keys_in: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
) {
    let m = modifiers(&keys);
    for k in keys_in.read() {
        if !m.ctrl || m.alt || k.state != bevy::input::ButtonState::Pressed {
            continue;
        }
        let by = match k.shortcut_code() {
            KeyCode::Equal | KeyCode::NumpadAdd => 1,
            KeyCode::Minus | KeyCode::NumpadSubtract => -1,
            _ => continue,
        };
        commands.queue(move |world: &mut World| step_ui_scale(world, by));
    }
}

/// Sets and saves the next UI scale up (`by` > 0) or down from the one now ([`next_ui_scale`]),
/// and shows it in a toast that fades.
pub fn step_ui_scale(world: &mut World, by: i32) {
    let now = world.resource::<LocalPreferences>().0.ui_scale;
    let next = next_ui_scale(now, by);
    if next != now {
        set_ui_scale(world, next);
    }
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    cadrs_ui::show_toast_for(
        &mut commands,
        &theme,
        tr_with(
            "shell-prefs-ui-scale-toast",
            &[("percent", &next.to_string())],
        ),
        1.5,
    );
    world.flush();
}

/// The UI scale preset after `percent`, up (`by` > 0) or down; the largest or the smallest stays.
pub fn next_ui_scale(percent: u16, by: i32) -> u16 {
    if by > 0 {
        UI_SCALES
            .iter()
            .copied()
            .find(|&p| p > percent)
            .unwrap_or(UI_SCALES[UI_SCALES.len() - 1])
    } else {
        UI_SCALES
            .iter()
            .rev()
            .copied()
            .find(|&p| p < percent)
            .unwrap_or(UI_SCALES[0])
    }
}

/// The modifier keys held.
pub fn modifiers(keys: &ButtonInput<KeyCode>) -> Modifiers {
    Modifiers {
        shift: keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
        ctrl: keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]),
        alt: keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]),
    }
}

/// A pointer button as the preferences name it.
pub fn mouse_button(b: PointerButton) -> MouseButton {
    match b {
        PointerButton::Primary => MouseButton::Left,
        PointerButton::Secondary => MouseButton::Right,
        PointerButton::Middle => MouseButton::Middle,
    }
}

/// Sets and saves the mouse preset (the dialog's OK; scenarios too).
pub fn set_mouse_preset(world: &mut World, preset: MousePreset) {
    world.resource_mut::<LocalPreferences>().0.mouse = preset;
    save(world);
}

/// Turns the MCP server on or off and saves the choice (the dialog's OK; scenarios too).
pub fn set_mcp_enabled(world: &mut World, on: bool) {
    world.resource_mut::<LocalPreferences>().0.mcp.enabled = on;
    save(world);
}

/// Sets the MCP server's port and saves it (the AI assistants menu; the server restarts on it).
pub fn set_mcp_port(world: &mut World, port: u16) {
    world.resource_mut::<LocalPreferences>().0.mcp.port = port;
    save(world);
}

/// Remembers the slicer parts were sent to, listed first next time (issue #176).
pub fn set_last_slicer(world: &mut World, id: u8) {
    let prefs = &mut world.resource_mut::<LocalPreferences>().0;
    if prefs.last_slicer != Some(id) {
        prefs.last_slicer = Some(id);
        save(world);
    }
}

/// Whether cadrs goes online as soon as it starts, saved (the network button's menu).
pub fn set_autoconnect_p2p(world: &mut World, on: bool) {
    world.resource_mut::<LocalPreferences>().0.autoconnect_p2p = on;
    save(world);
}

/// Whether to take part in public documents while online (Preferences › Collaboration, the
/// Public documents panel's Turn on), saved.
pub fn set_public_documents(world: &mut World, on: bool) {
    world.resource_mut::<LocalPreferences>().0.public_documents = on;
    save(world);
}

/// Keeps the camera up (the view menu's "Keep camera up", issue #97), and saves it.
pub fn set_keep_camera_up(world: &mut World, on: bool) {
    world.resource_mut::<LocalPreferences>().0.keep_camera_up = on;
    save(world);
    // Turned on, a rolled view levels.
    if on && let Some(mut view) = world.get_resource_mut::<crate::viewport::ViewportView>() {
        let mut to = view.target();
        if to.roll != 0.0 {
            to.roll = 0.0;
            view.animate_to(to);
        }
    }
}

/// Sets and saves the ground grid's look (scenarios, `set_preferences`).
pub fn set_grid(world: &mut World, grid: GridStyle) {
    world.resource_mut::<LocalPreferences>().0.grid = grid;
    save(world);
}

/// Sets and saves how the origin is drawn (scenarios, `set_preferences`).
pub fn set_origin(world: &mut World, origin: OriginStyle) {
    world.resource_mut::<LocalPreferences>().0.origin = origin;
    save(world);
}

/// Sets and saves what the frame-stats panel shows (scenarios, `set_preferences`).
pub fn set_frame_stats(world: &mut World, mode: FrameStatsMode) {
    world.resource_mut::<LocalPreferences>().0.frame_stats = mode;
    save(world);
}

/// The eye on the feature list's Origin row: hides the origin, or shows it again as it was
/// drawn before it was hidden (a bullseye if it was hidden from the start), and saves it.
pub fn toggle_origin(world: &mut World) {
    let now = world.resource::<LocalPreferences>().0.origin;
    let next = if now == OriginStyle::Hidden {
        world
            .get_resource::<ShownOrigin>()
            .map_or(OriginStyle::Dot, |s| s.0)
    } else {
        world.insert_resource(ShownOrigin(now));
        OriginStyle::Hidden
    };
    set_origin(world, next);
}

/// How the origin was drawn before the eye hid it.
#[derive(Resource)]
struct ShownOrigin(OriginStyle);

/// Remembers that the user has seen "What's new" up to `edition`
/// ([`crate::notifications::WHATS_NEW`]).
pub(crate) fn mark_whats_new_seen(world: &mut World, edition: u32) {
    if world.resource::<LocalPreferences>().0.whats_new_seen >= edition {
        return;
    }
    world.resource_mut::<LocalPreferences>().0.whats_new_seen = edition;
    save(world);
}

pub(crate) fn save(world: &mut World) {
    let prefs = world.resource::<LocalPreferences>().0;
    if let Some(store) = world.get_resource::<DocumentStore>()
        && let Err(e) = prefs.save(store.0.root())
    {
        warn!("cannot save the preferences: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each language of the preferences has a catalog (`cadrs_ui::i18n`) and the other way
    /// round, in the same order.
    #[test]
    fn every_language_has_a_catalog() {
        let ours: Vec<&str> = cadrs_core::preferences::Language::ALL
            .iter()
            .map(|l| l.code())
            .collect();
        let catalogs: Vec<&str> = cadrs_ui::i18n::LANGUAGES.iter().map(|(c, ..)| *c).collect();
        assert_eq!(ours, catalogs);
        for (code, _) in cadrs_ui::i18n::offered() {
            assert!(cadrs_core::preferences::Language::from_code(code).is_some());
        }
    }

    /// Ctrl+= and Ctrl+- step through the UI scale's sizes, staying at the ends; from a size
    /// between them, to the next one that way.
    #[test]
    fn ui_scale_steps_through_its_sizes() {
        assert_eq!(next_ui_scale(100, 1), 110);
        assert_eq!(next_ui_scale(100, -1), 90);
        assert_eq!(next_ui_scale(200, 1), 200);
        assert_eq!(next_ui_scale(75, -1), 75);
        assert_eq!(next_ui_scale(120, 1), 125);
        assert_eq!(next_ui_scale(120, -1), 110);
        let mut world = World::new();
        world.init_resource::<LocalPreferences>();
        world.init_resource::<Theme>();
        step_ui_scale(&mut world, 1);
        step_ui_scale(&mut world, 1);
        assert_eq!(world.resource::<LocalPreferences>().0.ui_scale, 125);
        // The last one only (each replaces the one before).
        let mut texts = world.query::<(&Name, &Text)>();
        let shown: Vec<String> = texts
            .iter(&world)
            .filter(|(n, _)| n.as_str() == "toast-message")
            .map(|(_, t)| t.0.clone())
            .collect();
        assert_eq!(shown, ["UI scale 125%"]);
    }
}
