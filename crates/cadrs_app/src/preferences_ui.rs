//! The local preferences (P3E.3, TD6.2, D2.2): the account menu's **Preferences…** opens a
//! dialog with the **3D view mouse controls** preset (Onshape, SolidWorks, Inventor, Creo) and
//! what its gestures do (Rotate, Rotate without roll, Pan, Zoom). OK keeps the choice in the store root's
//! `preferences.ron` ([`cadrs_core::preferences`]); it is this machine's, not a document's, so
//! nothing is undone. The Part Studio and Assembly viewports (`crate::viewport`) and the
//! drawing sheet (`crate::drawing`) read it for their drags.

use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, observe};
use cadrs_core::preferences::{Modifiers, MouseButton, MousePreset, Preferences, ViewAction};
use cadrs_ui::prelude::*;
use cadrs_ui::{Button, DialogClose, MenuAction, MenuItem, Select, SelectChange, SelectState, form_row, open_menu};

use crate::{AppState, DocumentStore};

pub struct PreferencesPlugin;

impl Plugin for PreferencesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LocalPreferences>()
            .add_systems(Startup, load_preferences)
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
        .item(MenuItem::new("account-sign-out", "Sign out").disabled(true));
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
                // Room for the preset list, which opens down over the rows.
                b.spawn(Node { height: Val::Px(36.0), ..default() });
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

/// OK: the chosen preset is kept.
fn save_preferences(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<PreferencesDialog>>();
    let Some(dialog) = q.iter(world).next() else { return };
    let mut qs = world.query::<(&Name, &SelectState)>();
    let preset = qs.iter(world).find(|(n, _)| n.as_str() == "pref-mouse-preset").and_then(|(_, s)| MousePreset::ALL.get(s.selected).copied());
    if let Some(p) = preset {
        set_mouse_preset(world, p);
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
