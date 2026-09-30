//! The quick-dimension box: the small value box Onshape opens on a just-drawn rectangle's width
//! (`reference/onshape/screens/12`–`14`), modeled on gpui-component's `NumberInput` in a
//! compact, single-purpose form.
//!
//! - It opens **at rest**: a white box with a grey 1 px border showing the current value in grey,
//!   keyboard focus inside and the whole value selected (but not highlighted, as in Onshape).
//! - Typing replaces the value and switches it to **editing**: a wider box (at least 100 px)
//!   with a blue border, a 3 px blue bottom edge, dark right-aligned text and a caret.
//! - **Tab** appends the default unit (`"5"` → `"5 mm"`, all selected) instead of moving focus.
//! - **Enter** triggers [`QuickDimCommit`], **Esc** [`QuickDimCancel`]. Both bubble. The owner
//!   decides what to do (usually despawn the box and open the next one).
//! - A **passive** box ([`QuickDim::passive`], the line tool's length box) opens without
//!   keyboard focus, so the tool's keys keep working while it shows; the owner calls
//!   [`start_typing`] when a digit is typed, which focuses it with that digit.
//!
//! The owner positions the box: its node is absolutely positioned; set `left`/`top`.

use std::borrow::Cow;

use bevy::ecs::lifecycle::HookContext;
use bevy::ecs::world::DeferredWorld;
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusCause, FocusedInput, InputFocus};
use bevy::prelude::*;
use bevy::text::{EditableText, EditableTextFilter, FontWeight, TextCursorStyle, TextEdit};

use crate::input::TextInputField;
use crate::theme::Theme;

pub struct QuickDimPlugin;

impl Plugin for QuickDimPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_key).add_systems(
            PostUpdate,
            style_boxes.before(bevy::ui::UiSystems::Prepare),
        );
    }
}

/// Enter in a quick-dimension box. Targets the box's text field and bubbles.
#[derive(EntityEvent, Clone, Debug)]
#[entity_event(propagate, auto_propagate)]
pub struct QuickDimCommit {
    pub entity: Entity,
    pub value: String,
}

/// Esc in a quick-dimension box. Bubbles like [`QuickDimCommit`].
#[derive(EntityEvent, Clone, Debug)]
#[entity_event(propagate, auto_propagate)]
pub struct QuickDimCancel {
    pub entity: Entity,
}

/// The box (outer node).
#[derive(Component, Debug, Clone)]
pub struct QuickDimBox {
    /// The value it opened with; editing starts when the text differs.
    initial: String,
    /// The unit Tab appends.
    unit: String,
    /// True once the user has typed.
    pub editing: bool,
    colors: Colors,
}

#[derive(Debug, Clone, Copy)]
struct Colors {
    rest_border: Color,
    rest_text: Color,
    edit_border: Color,
    edit_text: Color,
    selection: Color,
    caret: Color,
}

/// The editable text inside a [`QuickDimBox`].
#[derive(Component, Debug, Clone, Copy)]
pub struct QuickDimField;

/// Builder for a quick-dimension box.
#[derive(Debug, Clone)]
pub struct QuickDim {
    name: Cow<'static, str>,
    value: String,
    unit: String,
    passive: bool,
}

/// Rest height and editing minimum width, measured in `screens/12` and `13`.
const HEIGHT: f32 = 26.0;
const EDIT_MIN_WIDTH: f32 = 100.0;

impl QuickDim {
    pub fn new(name: impl Into<Cow<'static, str>>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            unit: "mm".into(),
            passive: false,
        }
    }

    /// Opens without keyboard focus (see [`start_typing`]).
    pub fn passive(mut self, p: bool) -> Self {
        self.passive = p;
        self
    }

    /// The unit Tab appends (default "mm").
    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = unit.into();
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let colors = Colors {
            rest_border: Color::srgb_u8(0xa8, 0xab, 0xad),
            rest_text: Color::srgb_u8(0x55, 0x59, 0x5a),
            edit_border: Color::srgb_u8(0x16, 0x51, 0xb0),
            edit_text: theme.foreground,
            selection: theme.selection,
            caret: theme.caret,
        };
        let font = theme.font(13.0, FontWeight::NORMAL);
        let field_name = format!("{}-field", self.name);
        let value = self.value.clone();
        let passive = self.passive;
        (
            Name::new(self.name.into_owned()),
            QuickDimBox {
                initial: self.value,
                unit: self.unit,
                editing: false,
                colors,
            },
            Node {
                position_type: PositionType::Absolute,
                height: Val::Px(HEIGHT),
                padding: UiRect::horizontal(Val::Px(6.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::FlexEnd,
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::WHITE),
            BorderColor::all(colors.rest_border),
            // A passive box lets clicks through to the drawing under it.
            pickable(passive),
            children![(
                pickable(passive),
                FocusOnSpawn(!passive),
                Name::new(field_name),
                QuickDimField,
                TextInputField,
                EditableText::new(&value),
                EditableTextFilter::new(|c: char| !c.is_control()),
                font,
                TextColor(colors.rest_text),
                TextLayout::new(Justify::Right, LineBreak::NoWrap),
                TextCursorStyle {
                    color: Color::NONE,
                    selection_color: Color::NONE,
                    unfocused_selection_color: Color::NONE,
                    selected_text_color: None,
                },
                TabIndex(0),
                Node {
                    flex_grow: 1.0,
                    height: Val::Px(16.0),
                    ..default()
                },
            )],
        )
    }
}

fn pickable(passive: bool) -> Pickable {
    if passive {
        Pickable::IGNORE
    } else {
        Pickable::default()
    }
}

/// A passive box's first typed character: focuses its field and replaces the value with `text`,
/// as if the box had been focused when it was typed.
pub fn start_typing(world: &mut World, quick_dim: Entity, text: &str) {
    let field = world
        .get::<Children>(quick_dim)
        .and_then(|c| c.iter().find(|c| world.get::<QuickDimField>(*c).is_some()));
    let Some(field) = field else {
        return;
    };
    if let Some(mut t) = world.get_mut::<EditableText>(field) {
        t.queue_edit(TextEdit::SelectAll);
        t.queue_edit(TextEdit::Insert(text.to_string().into()));
    }
    if let Some(mut b) = world.get_mut::<QuickDimBox>(quick_dim) {
        b.editing = true;
    }
    if let Some(mut focus) = world.get_resource_mut::<InputFocus>() {
        focus.set(field, FocusCause::Pressed);
    }
}

/// With `true`, focuses the field and selects its value as soon as it exists (a passive box
/// has `false`).
#[derive(Component, Debug, Clone, Copy)]
#[component(on_insert = on_focus_on_spawn)]
struct FocusOnSpawn(bool);

fn on_focus_on_spawn(mut world: DeferredWorld, ctx: HookContext) {
    let e = ctx.entity;
    if !world.get::<FocusOnSpawn>(e).is_some_and(|f| f.0) {
        return;
    }
    world.commands().queue(move |world: &mut World| {
        if let Some(mut text) = world.get_mut::<EditableText>(e) {
            text.queue_edit(TextEdit::SelectAll);
        }
        if let Some(mut focus) = world.get_resource_mut::<InputFocus>() {
            focus.set(e, FocusCause::Pressed);
        }
    });
}

fn on_key(
    mut ev: On<FocusedInput<KeyboardInput>>,
    mut q_field: Query<(&mut EditableText, &ChildOf), With<QuickDimField>>,
    mut q_box: Query<&mut QuickDimBox>,
    mut commands: Commands,
) {
    let entity = ev.focused_entity;
    let Ok((mut text, parent)) = q_field.get_mut(entity) else {
        return;
    };
    if ev.input.state != ButtonState::Pressed {
        return;
    }
    match ev.input.key_code {
        KeyCode::Enter | KeyCode::NumpadEnter => {
            ev.propagate(false);
            commands.trigger(QuickDimCommit {
                entity,
                value: text.value().to_string(),
            });
        }
        KeyCode::Escape => {
            ev.propagate(false);
            commands.trigger(QuickDimCancel { entity });
        }
        KeyCode::Tab => {
            // Onshape appends the unit rather than moving on.
            ev.propagate(false);
            let Ok(mut b) = q_box.get_mut(parent.parent()) else {
                return;
            };
            let v = text.value().to_string();
            let trimmed = v.trim_end();
            let has_unit = trimmed.ends_with(|c: char| c.is_alphabetic());
            if !trimmed.is_empty() && !has_unit {
                text.queue_edit(TextEdit::TextEnd(false));
                let pad = if v.ends_with(' ') { "" } else { " " };
                text.queue_edit(TextEdit::Insert(format!("{pad}{}", b.unit).into()));
            }
            text.queue_edit(TextEdit::SelectAll);
            b.editing = true;
        }
        _ => {}
    }
}

#[allow(clippy::type_complexity)]
fn style_boxes(
    mut q_box: Query<(&mut QuickDimBox, &Children, &mut BorderColor, &mut Node)>,
    mut q_field: Query<
        (
            &EditableText,
            &mut TextColor,
            &mut TextCursorStyle,
            &bevy::text::TextLayoutInfo,
            &ComputedNode,
        ),
        With<QuickDimField>,
    >,
) {
    for (mut b, children, mut border, mut node) in &mut q_box {
        let Some(child) = children.first().copied() else {
            continue;
        };
        let Ok((text, mut color, mut cursor, layout, computed)) = q_field.get_mut(child) else {
            continue;
        };
        // The box fits its text (plus padding and room for the caret).
        let text_w = layout.size.x * computed.inverse_scale_factor();
        if !b.editing && text.value().to_string() != b.initial {
            b.editing = true;
        }
        let c = b.colors;
        let (border_color, fg, bw, bottom, min_w) = if b.editing {
            (c.edit_border, c.edit_text, 1.0, 3.0, EDIT_MIN_WIDTH)
        } else {
            (c.rest_border, c.rest_text, 1.0, 1.0, 0.0)
        };
        border.set_if_neq(BorderColor::all(border_color));
        color.set_if_neq(TextColor(fg));
        let want_cursor = if b.editing {
            TextCursorStyle {
                color: c.caret,
                selection_color: c.selection,
                unfocused_selection_color: Color::NONE,
                selected_text_color: Some(Color::WHITE),
            }
        } else {
            TextCursorStyle {
                color: Color::NONE,
                selection_color: Color::NONE,
                unfocused_selection_color: Color::NONE,
                selected_text_color: None,
            }
        };
        if cursor.color != want_cursor.color || cursor.selection_color != want_cursor.selection_color
        {
            *cursor = want_cursor;
        }
        let want_border = UiRect::new(
            Val::Px(bw),
            Val::Px(bw),
            Val::Px(bw),
            Val::Px(bottom),
        );
        if node.border != want_border {
            node.border = want_border;
        }
        let width = Val::Px((text_w + 16.0).max(min_w).round());
        if node.width != width {
            node.width = width;
        }
    }
}
