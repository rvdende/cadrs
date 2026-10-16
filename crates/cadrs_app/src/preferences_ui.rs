//! The local preferences (P3E.3, TD6.2, D2.2): the account menu's **Preferences…** opens a
//! dialog with the **3D view mouse controls** preset (Onshape, SolidWorks, Inventor, Creo) and
//! what its gestures do (Rotate, Rotate without roll, Pan, Zoom). OK keeps the choice in the store root's
//! `preferences.ron` ([`cadrs_core::preferences`]); it is this machine's, not a document's, so
//! nothing is undone. The Part Studio and Assembly viewports (`crate::viewport`) and the
//! drawing sheet (`crate::drawing`) read it for their drags.
//!
//! **Interface › UI scale** (issue #12): 75–200 % of the display's own scale, for large or
//! high-resolution screens; applied to the whole window as soon as OK is pressed.
//!
//! Below it, **AI assistants (MCP)**: the *Enable MCP server* switch and the endpoint it serves
//! (`crate::mcp` starts and stops the server as OK changes it).

use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, observe};
use cadrs_core::preferences::{Modifiers, MouseButton, MousePreset, Preferences, UI_SCALES, ViewAction};
use cadrs_ui::prelude::*;
use cadrs_ui::{Button, DialogClose, MenuAction, MenuItem, Select, SelectChange, SelectState, Switch, SwitchState, form_row, open_menu};

use crate::{AppState, DocumentStore};

pub struct PreferencesPlugin;

impl Plugin for PreferencesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LocalPreferences>()
            .add_systems(Startup, load_preferences)
            .add_systems(Update, apply_ui_scale)
            .add_observer(on_account)
            .add_observer(on_account_action)
            .add_observer(on_preset_change);
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

fn load_preferences(store: Option<Res<DocumentStore>>, mut prefs: ResMut<LocalPreferences>) {
    if let Some(store) = store {
        prefs.0 = Preferences::load(store.0.root());
    }
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
    let Ok(mut w) = windows.single_mut() else { return };
    // The window's own override (a scenario replay's 1.0) is what 100% puts back.
    let own = *original.get_or_insert(w.resolution.scale_factor_override());
    if applied.is_none() && k == 1.0 {
        *applied = Some(k);
        return;
    }
    let want = if k == 1.0 { own } else { Some(own.unwrap_or(w.resolution.base_scale_factor()) * k) };
    w.resolution.set_scale_factor_override(want);
    *applied = Some(k);
}

/// Sets and saves the interface size in percent (the dialog's OK; scenarios too).
pub fn set_ui_scale(world: &mut World, percent: u16) {
    world.resource_mut::<LocalPreferences>().0.ui_scale = percent;
    save(world);
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

fn save(world: &mut World) {
    let prefs = world.resource::<LocalPreferences>().0;
    if let Some(store) = world.get_resource::<DocumentStore>()
        && let Err(e) = prefs.save(store.0.root())
    {
        warn!("cannot save the preferences: {e}");
    }
}

// ---------------------------------------------------------------------------------------------
// The account menu and the dialog

/// The account button (top right, on the documents page and in a document): its menu.
fn on_account(a: On<Activate>, q: Query<&Name>, theme: Res<Theme>, mut commands: Commands) {
    if q.get(a.entity).map(|n| n.as_str()) != Ok("account") {
        return;
    }
    let menu = cadrs_ui::Menu::new("account-menu")
        .align_end()
        .min_width(180.0)
        .item(MenuItem::new("account-preferences", "Preferences…").icon("settings"))
        .separator()
        .item(MenuItem::new("account-sign-out", "Sign out").planned());
    open_menu(&mut commands, a.entity, menu.build(&theme));
}

fn on_account_action(ev: On<MenuAction>, q: Query<&Name>, mut commands: Commands) {
    if q.get(ev.entity).map(|n| n.as_str()) != Ok("account") {
        return;
    }
    if ev.item == "account-preferences" {
        commands.queue(open_preferences);
    }
}

/// The dialog's root.
#[derive(Component)]
struct PreferencesDialog;

/// A gesture row's text (its action).
#[derive(Component)]
struct GestureText(ViewAction);

const ACTIONS: [(ViewAction, &str); 4] = [
    (ViewAction::Rotate, "pref-mouse-rotate"),
    (ViewAction::RotateTurntable, "pref-mouse-turntable"),
    (ViewAction::Pan, "pref-mouse-pan"),
    (ViewAction::Zoom, "pref-mouse-zoom"),
];

fn gesture_text(preset: MousePreset, action: ViewAction) -> String {
    let g = preset.gestures(action);
    if g.is_empty() { "—".into() } else { g.join(", ") }
}

/// Opens the Preferences dialog with the preferences in force.
pub fn open_preferences(world: &mut World) {
    let preset = world.resource::<LocalPreferences>().mouse();
    let mcp = world.resource::<LocalPreferences>().0.mcp;
    let ui_scale = world.resource::<LocalPreferences>().0.ui_scale;
    let endpoint = cadrs_mcp::url(std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, mcp.port)));
    let state = *world.resource::<State<AppState>>().get();
    let theme = world.resource::<Theme>().clone();
    let (tb, tf) = (theme.clone(), theme.clone());
    world.commands().spawn((
        Dialog::new("preferences-dialog")
            .title("Preferences")
            .width(440.0)
            .body(move |b| {
                let t = &tb;
                b.spawn((
                    Name::new("pref-mouse-heading"),
                    t.text("3D view mouse controls", t.font_base, FontWeight::BOLD, t.foreground),
                    Node { margin: UiRect::bottom(Val::Px(4.0)), ..default() },
                ));
                let mut select = Select::new("pref-mouse-preset").width(Val::Px(200.0));
                for p in MousePreset::ALL {
                    select = select.option(p.label(), true);
                }
                let selected = MousePreset::ALL.iter().position(|p| *p == preset).unwrap_or(0);
                b.spawn(form_row(t, "pref-mouse-row", "Mouse controls", 150.0)).with_child(select.selected(selected).build(t));
                for (action, name) in ACTIONS {
                    b.spawn(form_row(t, format!("{name}-row"), action.label(), 150.0)).with_child((
                        Name::new(name),
                        GestureText(action),
                        t.text(gesture_text(preset, action), t.font_base, FontWeight::NORMAL, t.muted_foreground),
                    ));
                }
                b.spawn((
                    Name::new("pref-interface-heading"),
                    t.text("Interface", t.font_base, FontWeight::BOLD, t.foreground),
                    Node { margin: UiRect::new(Val::ZERO, Val::ZERO, Val::Px(14.0), Val::Px(4.0)), ..default() },
                ));
                let mut sizes = Select::new("pref-ui-scale").width(Val::Px(200.0));
                for p in UI_SCALES {
                    sizes = sizes.option(if p == 100 { "100% (display default)".to_string() } else { format!("{p}%") }, true);
                }
                let at = UI_SCALES.iter().position(|p| *p == ui_scale).unwrap_or(2);
                b.spawn(form_row(t, "pref-ui-scale-row", "UI scale", 150.0)).with_child(sizes.selected(at).build(t));
                b.spawn((
                    Name::new("pref-mcp-heading"),
                    t.text("AI assistants (MCP)", t.font_base, FontWeight::BOLD, t.foreground),
                    Node { margin: UiRect::new(Val::ZERO, Val::ZERO, Val::Px(14.0), Val::Px(4.0)), ..default() },
                ));
                b.spawn(form_row(t, "pref-mcp-row", "MCP server", 150.0))
                    .with_child(Switch::new("pref-mcp-enabled").label("Enabled").on(mcp.enabled).build(t));
                b.spawn(form_row(t, "pref-mcp-endpoint-row", "Endpoint", 150.0)).with_child((
                    Name::new("pref-mcp-endpoint"),
                    t.text(endpoint, t.font_base, FontWeight::NORMAL, t.muted_foreground),
                ));
                b.spawn((
                    Name::new("pref-mcp-note"),
                    t.text(
                        "While it is on, AI assistants on this computer (Claude, Cursor, …) can create and edit documents.",
                        t.font_sm,
                        FontWeight::NORMAL,
                        t.muted_foreground,
                    ),
                    Node { width: Val::Px(400.0), margin: UiRect::top(Val::Px(6.0)), ..default() },
                ))
                // Wrapped (the theme's text doesn't wrap).
                .insert(TextLayout::default());
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn((
                    Button::new("pref-ok").label("OK").primary().build(t),
                    observe(|_: On<Activate>, mut commands: Commands| commands.queue(save_preferences)),
                ));
                f.spawn((
                    Button::new("pref-cancel").label("Cancel").build(t),
                    observe(|_: On<Activate>, q: Query<Entity, With<PreferencesDialog>>, mut commands: Commands| {
                        for e in &q {
                            commands.trigger(DialogClose { entity: e });
                        }
                    }),
                ));
            })
            .build(&theme),
        PreferencesDialog,
        DespawnOnExit(state),
    ));
    world.flush();
}

/// The preset changed in the dialog: its gestures follow.
fn on_preset_change(ev: On<SelectChange>, q: Query<&Name>, mut q_text: Query<(&GestureText, &mut Text)>) {
    if q.get(ev.entity).map(|n| n.as_str()) != Ok("pref-mouse-preset") {
        return;
    }
    let Some(preset) = MousePreset::ALL.get(ev.index).copied() else { return };
    for (g, mut t) in &mut q_text {
        t.0 = gesture_text(preset, g.0);
    }
}

/// OK: the chosen preset, interface size and MCP switch are kept.
fn save_preferences(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<PreferencesDialog>>();
    let Some(dialog) = q.iter(world).next() else { return };
    let mut qs = world.query::<(&Name, &SelectState)>();
    let preset = qs.iter(world).find(|(n, _)| n.as_str() == "pref-mouse-preset").and_then(|(_, s)| MousePreset::ALL.get(s.selected).copied());
    if let Some(p) = preset {
        set_mouse_preset(world, p);
    }
    let size = qs.iter(world).find(|(n, _)| n.as_str() == "pref-ui-scale").and_then(|(_, s)| UI_SCALES.get(s.selected).copied());
    if let Some(p) = size {
        set_ui_scale(world, p);
    }
    let mut qm = world.query::<(&Name, &SwitchState)>();
    let mcp = qm.iter(world).find(|(n, _)| n.as_str() == "pref-mcp-enabled").map(|(_, s)| s.on);
    if let Some(on) = mcp {
        set_mcp_enabled(world, on);
    }
    world.trigger(DialogClose { entity: dialog });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gesture_rows_read_like_the_table() {
        assert_eq!(gesture_text(MousePreset::Onshape, ViewAction::Rotate), "Right drag");
        assert_eq!(gesture_text(MousePreset::Onshape, ViewAction::RotateTurntable), "Alt+Right drag");
        assert_eq!(gesture_text(MousePreset::SolidWorks, ViewAction::RotateTurntable), "—");
        assert_eq!(gesture_text(MousePreset::Onshape, ViewAction::Pan), "Middle drag, Ctrl+Right drag");
        assert_eq!(gesture_text(MousePreset::SolidWorks, ViewAction::Zoom), "Scroll wheel, Shift+Middle drag");
    }
}
