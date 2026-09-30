//! Double-click detection and in-place text editing.
//!
//! - [`DoubleClickable`]: two primary clicks on the entity within [`DOUBLE_CLICK_TIME`] trigger
//!   [`DoubleClick`] on it (gpui's `ClickEvent::click_count() == 2`).
//! - [`InlineEdit`]: a node whose [`InlineEditLabel`] text child can be swapped for a
//!   [`TextInput`] in place, like renaming a tab or the document name. Start editing with
//!   [`begin_inline_edit`]. Enter or clicking elsewhere triggers [`InlineEditCommit`] on the node;
//!   Escape triggers [`InlineEditCancel`]. Either way the label comes back.

use std::borrow::Cow;

use bevy::input_focus::InputFocus;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;

use crate::input::{TextCancel, TextInput, TextInputField, TextSubmit};
use crate::theme::Theme;

/// The longest gap between the two clicks of a double click, in seconds.
pub const DOUBLE_CLICK_TIME: f32 = 0.45;

pub struct InlineEditPlugin;

impl Plugin for InlineEditPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(detect_double_click)
            .add_observer(on_submit)
            .add_observer(on_cancel)
            .add_observer(stop_key_bubbling)
            .add_systems(Update, commit_on_blur);
    }
}

/// Double-clicking this entity triggers [`DoubleClick`] on it.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct DoubleClickable;

/// A [`DoubleClickable`] entity was double-clicked with the primary button.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct DoubleClick {
    pub entity: Entity,
}

fn detect_double_click(
    click: On<Pointer<Click>>,
    q: Query<(), With<DoubleClickable>>,
    time: Res<Time>,
    mut last: Local<Option<(Entity, f32)>>,
    mut commands: Commands,
) {
    if click.button != PointerButton::Primary || !q.contains(click.entity) {
        return;
    }
    let now = time.elapsed_secs();
    match *last {
        Some((e, t)) if e == click.entity && now - t <= DOUBLE_CLICK_TIME => {
            *last = None;
            commands.trigger(DoubleClick {
                entity: click.entity,
            });
        }
        _ => *last = Some((click.entity, now)),
    }
}

/// A node with a label that can be edited in place.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct InlineEdit {
    pub editing: bool,
}

/// The text shown while an [`InlineEdit`] is not being edited (hidden while editing).
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct InlineEditLabel;

/// A sibling of the label that steps aside while editing (a tree row's eye toggle), so the
/// field keeps its full width (P3.5: the rename field of a sketch row had shrunk by the eye's
/// 24 px).
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct InlineEditHide;

/// The text input spawned by [`begin_inline_edit`].
#[derive(Component, Debug, Clone, Copy)]
pub struct InlineEditInput {
    /// The [`InlineEdit`] node.
    pub owner: Entity,
    had_focus: bool,
}

/// An inline edit finished with Enter or by clicking elsewhere. Bubbles from the [`InlineEdit`]
/// node.
#[derive(EntityEvent, Clone, Debug)]
#[entity_event(propagate, auto_propagate)]
pub struct InlineEditCommit {
    pub entity: Entity,
    pub value: String,
}

/// An inline edit was cancelled with Escape. Bubbles from the [`InlineEdit`] node.
#[derive(EntityEvent, Clone, Copy, Debug)]
#[entity_event(propagate, auto_propagate)]
pub struct InlineEditCancel {
    pub entity: Entity,
}

/// How [`begin_inline_edit`] sizes its input.
#[derive(Debug, Clone)]
pub struct InlineEditOptions {
    /// The input's `Name` (its field is `<name>-field`).
    pub name: Cow<'static, str>,
    pub width: Val,
    pub height: f32,
    pub font_size: Option<f32>,
    pub weight: bevy::text::FontWeight,
    /// Padding inside the border; default the input's.
    pub padding: Option<f32>,
    /// Focused border (width, color); default the input's 2 px focus border.
    pub focus_border: Option<(f32, Color)>,
}

impl InlineEditOptions {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            width: Val::Percent(100.0),
            height: 22.0,
            font_size: None,
            weight: bevy::text::FontWeight::NORMAL,
            padding: None,
            focus_border: None,
        }
    }
}

/// Swaps the [`InlineEditLabel`] of `owner` (an [`InlineEdit`] node) for a focused text input
/// holding `value`, fully selected.
pub fn begin_inline_edit(
    commands: &mut Commands,
    theme: &Theme,
    owner: Entity,
    value: impl Into<String>,
    opts: InlineEditOptions,
) {
    let mut input = TextInput::new(opts.name.clone())
        .value(value)
        .width(opts.width)
        .height(opts.height)
        .select_all_on_focus()
        .autofocus();
    if let Some(size) = opts.font_size {
        input = input.font(size, opts.weight);
    }
    if let Some(p) = opts.padding {
        input = input.padding(p);
    }
    if let Some((w, c)) = opts.focus_border {
        input = input.focus_border(w, c);
    }
    let bundle = input.build(theme);
    commands.queue(move |world: &mut World| {
        let Ok(mut e) = world.get_entity_mut(owner) else {
            return;
        };
        let Some(mut edit) = e.get_mut::<InlineEdit>() else {
            return;
        };
        if edit.editing {
            return;
        }
        edit.editing = true;
        let children: Vec<Entity> = e
            .get::<Children>()
            .map(|c| c.to_vec())
            .unwrap_or_default();
        let mut index = children.len();
        for (i, c) in children.iter().enumerate() {
            if world.get::<InlineEditLabel>(*c).is_some() {
                index = index.min(i);
                if let Some(mut n) = world.get_mut::<Node>(*c) {
                    n.display = Display::None;
                }
            } else if world.get::<InlineEditHide>(*c).is_some()
                && let Some(mut n) = world.get_mut::<Node>(*c)
            {
                n.display = Display::None;
            }
        }
        let input = world
            .spawn((
                bundle,
                InlineEditInput {
                    owner,
                    had_focus: false,
                },
            ))
            .id();
        world.entity_mut(owner).insert_children(index, &[input]);
    });
}

/// Ends an inline edit on `owner` without triggering anything: removes the input and shows the
/// label again.
pub fn end_inline_edit(world: &mut World, owner: Entity) {
    let mut q = world.query::<(Entity, &InlineEditInput)>();
    let inputs: Vec<Entity> = q
        .iter(world)
        .filter(|(_, i)| i.owner == owner)
        .map(|(e, _)| e)
        .collect();
    for e in inputs {
        if let Ok(e) = world.get_entity_mut(e) {
            e.despawn();
        }
    }
    let Ok(mut e) = world.get_entity_mut(owner) else {
        return;
    };
    if let Some(mut edit) = e.get_mut::<InlineEdit>() {
        edit.editing = false;
    }
    let children: Vec<Entity> = e
        .get::<Children>()
        .map(|c| c.to_vec())
        .unwrap_or_default();
    for c in children {
        if (world.get::<InlineEditLabel>(c).is_some() || world.get::<InlineEditHide>(c).is_some())
            && let Some(mut n) = world.get_mut::<Node>(c)
        {
            n.display = Display::Flex;
        }
    }
}

fn finish(commands: &mut Commands, owner: Entity, value: Option<String>) {
    commands.queue(move |world: &mut World| {
        let editing = world
            .get::<InlineEdit>(owner)
            .is_some_and(|e| e.editing);
        if !editing {
            return;
        }
        end_inline_edit(world, owner);
        match value {
            Some(value) => world.trigger(InlineEditCommit {
                entity: owner,
                value,
            }),
            None => world.trigger(InlineEditCancel { entity: owner }),
        }
    });
}

/// Keys typed into the editor must not reach the edited node (tabs and the document name are
/// buttons, which would activate on Enter or Space).
fn stop_key_bubbling(
    mut ev: On<bevy::input_focus::FocusedInput<bevy::input::keyboard::KeyboardInput>>,
    q: Query<(), With<InlineEditInput>>,
) {
    if q.contains(ev.event_target()) {
        ev.propagate(false);
    }
}

fn on_submit(ev: On<TextSubmit>, q: Query<&InlineEditInput>, mut commands: Commands) {
    if let Ok(input) = q.get(ev.entity) {
        finish(&mut commands, input.owner, Some(ev.value.clone()));
    }
}

fn on_cancel(ev: On<TextCancel>, q: Query<&InlineEditInput>, mut commands: Commands) {
    if let Ok(input) = q.get(ev.entity) {
        finish(&mut commands, input.owner, None);
    }
}

/// Clicking elsewhere (the field loses focus) commits the edit.
fn commit_on_blur(
    mut q: Query<(&mut InlineEditInput, &Children)>,
    q_field: Query<&bevy::text::EditableText, With<TextInputField>>,
    focus: Res<InputFocus>,
    mut commands: Commands,
) {
    for (mut input, children) in &mut q {
        let Some((field, text)) = children
            .iter()
            .find_map(|c| q_field.get(c).ok().map(|t| (c, t)))
        else {
            continue;
        };
        let focused = focus.get() == Some(field);
        if focused {
            input.had_focus = true;
        } else if input.had_focus {
            finish(&mut commands, input.owner, Some(text.value().to_string()));
        }
    }
}
