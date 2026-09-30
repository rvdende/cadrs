//! A small floating "name it" popup (Onshape's "Folder name" box of Add selection to folder…,
//! `reference/onshape/training/intro-to-assemblies/lesson-assembly-folders.png`): a title, a
//! green ✓ and a red ✗ on one row, and a text field under them, focused with its text selected.
//! In the spirit of gpui-component's `Popover` with a form inside.
//!
//! Enter or ✓ triggers [`NamePopupCommit`] on the popup with the text; Escape or ✗ triggers
//! [`NamePopupCancel`]. Either way the popup then despawns itself.
//!
//! Names: the popup `<name>`, its field `<name>-input` (`<name>-input-field`), and the buttons
//! `<name>-accept` and `<name>-cancel`. With [`NamePopup::description`] (P3D.3: Create
//! version…) a second, optional field `<name>-description` follows the name; the commit carries
//! both.

use std::borrow::Cow;

use bevy::prelude::*;
use bevy::text::{EditableText, FontWeight};
use bevy::ui_widgets::Activate;

use crate::input::{TextCancel, TextInput, TextSubmit};
use crate::theme::Theme;

pub struct NamePopupPlugin;

impl Plugin for NamePopupPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_submit).add_observer(on_cancel).add_observer(on_button);
    }
}

/// ✓ or Enter: the name typed. Targets the popup.
#[derive(EntityEvent, Clone, Debug)]
pub struct NamePopupCommit {
    pub entity: Entity,
    pub value: String,
    /// The second field's text (empty without one).
    pub description: String,
}

/// ✗ or Escape. Targets the popup.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct NamePopupCancel {
    pub entity: Entity,
}

/// The popup root.
#[derive(Component, Debug, Clone)]
pub struct NamePopupRoot;

/// One of the popup's buttons, and the popup it belongs to.
#[derive(Component, Debug, Clone, Copy)]
struct PopupButton {
    popup: Entity,
    accept: bool,
}

/// The builder.
pub struct NamePopup {
    name: Cow<'static, str>,
    title: String,
    value: String,
    at: Vec2,
    description: Option<String>,
}

impl NamePopup {
    /// `at`: its top-left corner, in window coordinates.
    pub fn new(name: impl Into<Cow<'static, str>>, title: impl Into<String>, at: Vec2) -> Self {
        Self { name: name.into(), title: title.into(), value: String::new(), at, description: None }
    }

    /// A second, optional field under the name, with this placeholder.
    pub fn description(mut self, placeholder: impl Into<String>) -> Self {
        self.description = Some(placeholder.into());
        self
    }

    pub fn value(mut self, v: impl Into<String>) -> Self {
        self.value = v.into();
        self
    }

    /// Spawns it (a root node, over the other UI).
    pub fn spawn(self, commands: &mut Commands, theme: &Theme) -> Entity {
        let t = theme.clone();
        let popup = commands
            .spawn((
                Name::new(self.name.to_string()),
                NamePopupRoot,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(self.at.x),
                    top: Val::Px(self.at.y),
                    width: Val::Px(if self.description.is_some() { 220.0 } else { 150.0 }),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(5.0),
                    padding: UiRect::all(Val::Px(6.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(t.popover),
                BorderColor::all(t.border),
                BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.18), Val::ZERO, Val::Px(2.0), Val::ZERO, Val::Px(6.0)),
                GlobalZIndex(crate::z::DIALOG),
            ))
            .id();
        let name = self.name.to_string();
        commands.entity(popup).with_children(|p| {
            p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(2.0), ..default() }).with_children(|r| {
                r.spawn((t.text(self.title.clone(), t.font_sm, FontWeight::MEDIUM, t.foreground), Node { flex_grow: 1.0, ..default() }));
                // A white check on green, a red cross (as Onshape's).
                for (accept, icon, bg, fg) in [
                    (true, "check", Color::srgb_u8(0x2e, 0x9e, 0x44), Color::WHITE),
                    (false, "close", Color::NONE, Color::srgb_u8(0xd0, 0x2c, 0x2c)),
                ] {
                    let suffix = if accept { "accept" } else { "cancel" };
                    r.spawn((
                        Name::new(format!("{name}-{suffix}")),
                        bevy::ui_widgets::Button,
                        PopupButton { popup, accept },
                        Node { width: Val::Px(20.0), height: Val::Px(20.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
                        BackgroundColor(bg),
                        crate::tooltip::Tooltip::new(if accept { "Accept" } else { "Cancel" }),
                        children![(crate::icon::icon(icon, 15.0, fg), Pickable::IGNORE)],
                    ));
                }
            });
            p.spawn(
                TextInput::new(format!("{name}-input"))
                    .value(self.value.clone())
                    .height(24.0)
                    .width(Val::Percent(100.0))
                    .autofocus()
                    .select_all_on_focus()
                    .build(&t),
            );
            if let Some(placeholder) = &self.description {
                p.spawn(
                    TextInput::new(format!("{name}-description"))
                        .placeholder(placeholder.clone())
                        .height(24.0)
                        .width(Val::Percent(100.0))
                        .build(&t),
                );
            }
        });
        popup
    }
}

fn popup_of(mut e: Entity, q_parent: &Query<&ChildOf>, q_root: &Query<(), With<NamePopupRoot>>) -> Option<Entity> {
    for _ in 0..12 {
        if q_root.contains(e) {
            return Some(e);
        }
        e = q_parent.get(e).ok()?.parent();
    }
    None
}

/// The fields' texts, in order (the name first).
fn field_values(popup: Entity, q_children: &Query<&Children>, q_text: &Query<&EditableText>) -> Vec<String> {
    fn walk(e: Entity, q_children: &Query<&Children>, q_text: &Query<&EditableText>, out: &mut Vec<String>) {
        if let Ok(t) = q_text.get(e) {
            out.push(t.value().to_string());
            return;
        }
        if let Ok(c) = q_children.get(e) {
            for child in c.iter() {
                walk(child, q_children, q_text, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(popup, q_children, q_text, &mut out);
    out
}

fn on_submit(
    ev: On<TextSubmit>,
    q_parent: Query<&ChildOf>,
    q_root: Query<(), With<NamePopupRoot>>,
    q_children: Query<&Children>,
    q_text: Query<&EditableText>,
    mut commands: Commands,
) {
    if let Some(popup) = popup_of(ev.entity, &q_parent, &q_root) {
        let v = field_values(popup, &q_children, &q_text);
        let value = v.first().cloned().unwrap_or_else(|| ev.value.clone());
        let description = v.get(1).cloned().unwrap_or_default();
        commands.trigger(NamePopupCommit { entity: popup, value, description });
        commands.entity(popup).try_despawn();
    }
}

fn on_cancel(ev: On<TextCancel>, q_parent: Query<&ChildOf>, q_root: Query<(), With<NamePopupRoot>>, mut commands: Commands) {
    if let Some(popup) = popup_of(ev.entity, &q_parent, &q_root) {
        commands.trigger(NamePopupCancel { entity: popup });
        commands.entity(popup).try_despawn();
    }
}

fn on_button(a: On<Activate>, q: Query<&PopupButton>, q_children: Query<&Children>, q_text: Query<&EditableText>, mut commands: Commands) {
    let Ok(b) = q.get(a.entity).copied() else { return };
    if b.accept {
        let v = field_values(b.popup, &q_children, &q_text);
        let value = v.first().cloned().unwrap_or_default();
        let description = v.get(1).cloned().unwrap_or_default();
        commands.trigger(NamePopupCommit { entity: b.popup, value, description });
    } else {
        commands.trigger(NamePopupCancel { entity: b.popup });
    }
    commands.entity(b.popup).try_despawn();
}
