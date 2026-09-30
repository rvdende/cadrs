//! Checkboxes, modeled on gpui-component's `Checkbox` (`checkbox.rs`) and the ones in Onshape's
//! feature dialogs (`reference/onshape/screens/08c`): a 12 px box with a 1 px grey border that
//! fills blue with a white ✓ when checked, followed by a 13 px label.
//!
//! Clicking anywhere on the row toggles it and triggers [`CheckboxChange`] on the row. The
//! current value is the row's [`CheckboxState`]; change it from code to update the box.
//! Inserting or removing `InteractionDisabled` on the row greys or restores its box and label
//! (pair it with a [`crate::Tooltip`] saying why).

use std::borrow::Cow;

use bevy::picking::hover::Hovered;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui::InteractionDisabled;

use crate::icon::icon;
use crate::theme::Theme;

pub struct CheckboxPlugin;

impl Plugin for CheckboxPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_click)
            .add_systems(PostUpdate, (sync_disabled, sync_checkboxes).chain().before(bevy::ui::UiSystems::Prepare));
    }
}

/// Whether a checkbox row is checked.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[require(Hovered)]
pub struct CheckboxState {
    pub checked: bool,
    /// Not available at the moment (a disabled row only): the box is filled grey, as Onshape
    /// shows Mass properties' Center of mass and inertia Override before a material exists
    /// (`ex1-step6.png`).
    pub unavailable: bool,
}

/// A checkbox was toggled by the user. Targets the checkbox row.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct CheckboxChange {
    pub entity: Entity,
    pub checked: bool,
}

/// The box of a checkbox row; points at the row.
#[derive(Component, Debug, Clone, Copy)]
struct CheckboxBox(Entity);

/// The ✓ inside the box; points at the row.
#[derive(Component, Debug, Clone, Copy)]
struct CheckboxMark(Entity);

/// The label of a checkbox row; points at the row.
#[derive(Component, Debug, Clone, Copy)]
struct CheckboxLabel(Entity);

/// Whether the row was drawn disabled (to follow `InteractionDisabled` added or removed later).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct CheckboxDrawnDisabled(bool);

/// Builder for a checkbox with a label.
#[derive(Debug, Clone)]
pub struct Checkbox {
    name: Cow<'static, str>,
    label: String,
    checked: bool,
    disabled: bool,
    height: f32,
}

impl Checkbox {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            label: String::new(),
            checked: false,
            disabled: false,
            height: 25.0,
        }
    }

    pub fn label(mut self, l: impl Into<String>) -> Self {
        self.label = l.into();
        self
    }

    pub fn checked(mut self, c: bool) -> Self {
        self.checked = c;
        self
    }

    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
        self
    }

    /// Row height (default 25 px, the spacing of Onshape's dialog checkboxes).
    pub fn height(mut self, h: f32) -> Self {
        self.height = h;
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let t = theme.clone();
        let label = self.label;
        let checked = self.checked;
        let disabled = self.disabled;
        (
            Name::new(self.name.into_owned()),
            CheckboxState { checked, unavailable: false },
            CheckboxDrawnDisabled(disabled),
            Node {
                height: Val::Px(self.height),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                column_gap: Val::Px(4.0),
                ..default()
            },
            crate::style::InitState {
                disabled: self.disabled,
                selected: false,
                force: None,
            },
            Children::spawn(bevy::ecs::spawn::SpawnWith(move |p: &mut ChildSpawner| {
                let row = p.target_entity();
                p.spawn((
                    CheckboxBox(row),
                    Node {
                        width: Val::Px(12.0),
                        height: Val::Px(12.0),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(2.0)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        flex_shrink: 0.0,
                        ..default()
                    },
                    box_colors(&t, checked, disabled, false),
                    Pickable::IGNORE,
                ))
                .with_child((
                    CheckboxMark(row),
                    icon("check-bold", 10.0, Color::WHITE),
                    if checked {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    },
                    Pickable::IGNORE,
                ));
                // A disabled option's label is greyed like a disabled tool (Onshape's greyed Draft).
                let fg = if disabled { t.tool_disabled_foreground } else { t.tool_foreground };
                p.spawn((CheckboxLabel(row), t.text(label, t.font_sm, FontWeight::MEDIUM, fg), Pickable::IGNORE));
            })),
        )
    }
}

fn box_colors(t: &Theme, checked: bool, disabled: bool, unavailable: bool) -> (BackgroundColor, BorderColor) {
    if disabled && unavailable {
        let grey = Color::srgb_u8(0xa8, 0xa8, 0xa8);
        return (BackgroundColor(grey), BorderColor::all(grey));
    }
    if disabled {
        // A muted box: a light grey fill and a pale border (checked or not).
        let fill = if checked { t.primary_disabled } else { t.input_disabled_background };
        return (BackgroundColor(fill), BorderColor::all(t.disabled_foreground));
    }
    if checked {
        (
            BackgroundColor(t.checkbox_checked),
            BorderColor::all(t.checkbox_checked),
        )
    } else {
        (
            BackgroundColor(t.background),
            BorderColor::all(t.checkbox_border),
        )
    }
}

fn on_click(
    mut click: On<Pointer<Click>>,
    mut q: Query<(&mut CheckboxState, Has<InteractionDisabled>)>,
    mut commands: Commands,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let e = click.entity;
    let Ok((mut state, disabled)) = q.get_mut(e) else {
        return;
    };
    click.propagate(false);
    if disabled {
        return;
    }
    state.checked = !state.checked;
    commands.trigger(CheckboxChange {
        entity: e,
        checked: state.checked,
    });
}

/// A row disabled or enabled after it was built: redraw its box (through
/// [`sync_checkboxes`], by touching its state) and its label.
fn sync_disabled(
    theme: Res<Theme>,
    mut q_rows: Query<(Entity, &mut CheckboxState, &mut CheckboxDrawnDisabled, Has<InteractionDisabled>)>,
    mut q_label: Query<(&CheckboxLabel, &mut TextColor)>,
) {
    let mut changed = Vec::new();
    for (e, mut state, mut drawn, disabled) in &mut q_rows {
        if drawn.0 != disabled {
            drawn.0 = disabled;
            state.set_changed();
            changed.push((e, disabled));
        }
    }
    if changed.is_empty() {
        return;
    }
    for (l, mut color) in &mut q_label {
        if let Some((_, disabled)) = changed.iter().find(|(e, _)| *e == l.0) {
            color.0 = if *disabled { theme.tool_disabled_foreground } else { theme.tool_foreground };
        }
    }
}

fn sync_checkboxes(
    theme: Res<Theme>,
    q_rows: Query<(&CheckboxState, Has<InteractionDisabled>), Changed<CheckboxState>>,
    mut q_box: Query<(&CheckboxBox, &mut BackgroundColor, &mut BorderColor)>,
    mut q_mark: Query<(&CheckboxMark, &mut Visibility)>,
) {
    if q_rows.is_empty() {
        return;
    }
    for (b, mut bg, mut border) in &mut q_box {
        if let Ok((s, disabled)) = q_rows.get(b.0) {
            let (nb, nborder) = box_colors(&theme, s.checked, disabled, s.unavailable);
            bg.set_if_neq(nb);
            border.set_if_neq(nborder);
        }
    }
    for (m, mut vis) in &mut q_mark {
        if let Ok((s, _)) = q_rows.get(m.0) {
            vis.set_if_neq(if s.checked {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
    }
}
