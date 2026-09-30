//! The dimension value editor: the popup Onshape opens on a dimension just placed with the
//! Dimension tool, or double-clicked (`reference/onshape/screens/16b`,
//! `dimension/dimension-active-field-01-01.png`). Modeled on gpui-component's `NumberInput`
//! inside a small `Popover`.
//!
//! - A white popup with a 1 px `#cccccc` border and a soft shadow: a thin drag-handle strip on
//!   top (three small squares), a separator, then the value row: a grey `›` and the value,
//!   right-aligned, **fully selected** (blue highlight) with keyboard focus.
//! - **Enter** triggers [`DimEditCommit`] with the text; **Esc** [`DimEditCancel`]. Both bubble.
//!   The owner validates the text: for a value it cannot use it sets [`DimEditBox::error`], which
//!   turns the border and text red and shows the message under the box. Typing clears it.
//!
//! The owner positions the box (absolutely: set `left`/`top`).

use std::borrow::Cow;

use bevy::ecs::lifecycle::HookContext;
use bevy::ecs::world::DeferredWorld;
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusCause, FocusedInput, InputFocus};
use bevy::prelude::*;
use bevy::text::{EditableText, EditableTextFilter, FontWeight, TextCursorStyle, TextEdit};

use crate::icon::icon;
use crate::input::TextInputField;
use crate::theme::Theme;

pub struct DimEditPlugin;

impl Plugin for DimEditPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_key).add_observer(on_button_click).add_systems(
            PostUpdate,
            style_boxes.before(bevy::ui::UiSystems::Prepare),
        );
    }
}

/// Enter in a dimension editor. Targets the editor's text field and bubbles.
#[derive(EntityEvent, Clone, Debug)]
#[entity_event(propagate, auto_propagate)]
pub struct DimEditCommit {
    pub entity: Entity,
    pub value: String,
}

/// Esc in a dimension editor. Bubbles like [`DimEditCommit`].
#[derive(EntityEvent, Clone, Debug)]
#[entity_event(propagate, auto_propagate)]
pub struct DimEditCancel {
    pub entity: Entity,
}

/// The editor (outer node).
#[derive(Component, Debug, Clone)]
pub struct DimEditBox {
    /// Set by the owner when the value cannot be used: shown under the box until the text
    /// changes.
    pub error: Option<String>,
    /// The text the error was set for.
    error_text: String,
    colors: Colors,
}

impl DimEditBox {
    /// Marks the current text as invalid, with a message.
    pub fn set_error(&mut self, message: impl Into<String>, text: impl Into<String>) {
        self.error = Some(message.into());
        self.error_text = text.into();
    }
}

#[derive(Debug, Clone, Copy)]
struct Colors {
    border: Color,
    text: Color,
    error: Color,
    selection: Color,
    caret: Color,
}

/// The editable text inside a [`DimEditBox`].
#[derive(Component, Debug, Clone, Copy)]
pub struct DimEditField;

/// The error message under a [`DimEditBox`].
#[derive(Component, Debug, Clone, Copy)]
pub struct DimEditMessage;

/// Builder for a dimension editor.
#[derive(Debug, Clone)]
pub struct DimEdit {
    name: Cow<'static, str>,
    value: String,
    select: Option<usize>,
}

/// Measured in `screens/16b`: an 11 px handle strip, a 1 px separator and a 32 px value row.
/// The value row ends with a green ✓ and a red ✕ (`dimension-active-field-01-01.png`).
const HANDLE: f32 = 11.0;
const ROW: f32 = 32.0;
/// The editor's minimum width (it widens for a long value or message).
pub const WIDTH: f32 = 176.0;
/// The ✓ and ✕ buttons (`dimension-active-field-01-01.png`).
const BUTTON: f32 = 20.0;
/// The editor's height (without the error message).
pub const HEIGHT: f32 = HANDLE + 1.0 + ROW + 2.0;

impl DimEdit {
    pub fn new(name: impl Into<Cow<'static, str>>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            select: None,
        }
    }

    /// Selects only the first `chars` characters when it opens (the number of "50 mm"), rather
    /// than the whole value.
    pub fn select_prefix(mut self, chars: usize) -> Self {
        self.select = Some(chars);
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let colors = Colors {
            border: Color::srgb_u8(0xcc, 0xcc, 0xcc),
            text: theme.foreground,
            error: theme.danger,
            selection: theme.selection,
            caret: theme.caret,
        };
        let font = theme.font(14.0, FontWeight::NORMAL);
        let name = self.name.into_owned();
        // Room for the value it opens with (a live "40.03378 mm" was clipped at both ends in the
        // minimum width, Final re-audit): about 9 px a character at 14 px.
        let field_min = self.value.chars().count() as f32 * 9.0 + 6.0;
        let handle_square = |i: usize| {
            (
                Name::new(format!("{name}-handle-{i}")),
                Node {
                    width: Val::Px(4.0),
                    height: Val::Px(4.0),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BorderColor::all(Color::srgb_u8(0xb4, 0xb8, 0xbb)),
            )
        };
        (
            Name::new(name.clone()),
            DimEditBox {
                error: None,
                error_text: String::new(),
                colors,
            },
            Node {
                position_type: PositionType::Absolute,
                min_width: Val::Px(WIDTH),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(Color::WHITE),
            BorderColor::all(colors.border),
            BoxShadow::new(
                theme.shadow,
                Val::Px(0.0),
                Val::Px(2.0),
                Val::Px(0.0),
                Val::Px(6.0),
            ),
            children![
                (
                    Name::new(format!("{name}-handle")),
                    Node {
                        height: Val::Px(HANDLE),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(2.0),
                        border: UiRect::bottom(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::all(Color::srgb_u8(0xe3, 0xe5, 0xe7)),
                    children![handle_square(0), handle_square(1), handle_square(2)],
                ),
                (
                    Name::new(format!("{name}-row")),
                    Node {
                        height: Val::Px(ROW),
                        padding: UiRect::new(Val::Px(6.0), Val::Px(8.0), Val::Px(0.0), Val::Px(0.0)),
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(6.0),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    children![
                        (
                            Name::new(format!("{name}-chevron")),
                            icon("chevron-right", 12.0, Color::srgb_u8(0x9a, 0xa0, 0xa6)),
                        ),
                        (
                            Name::new(format!("{name}-field")),
                            DimEditField,
                            TextInputField,
                            EditableText::new(&self.value),
                            EditableTextFilter::new(|c: char| !c.is_control()),
                            font.clone(),
                            TextColor(colors.text),
                            TextLayout::new(Justify::Right, LineBreak::NoWrap),
                            TextCursorStyle {
                                color: colors.caret,
                                selection_color: colors.selection,
                                unfocused_selection_color: colors.selection,
                                selected_text_color: Some(Color::WHITE),
                            },
                            TabIndex(0),
                            Node {
                                flex_grow: 1.0,
                                min_width: Val::Px(field_min),
                                height: Val::Px(18.0),
                                ..default()
                            },
                            FocusAndSelect(self.select),
                        ),
                        (
                            Name::new(format!("{name}-accept")),
                            DimEditButton::Accept,
                            Node {
                                width: Val::Px(BUTTON),
                                height: Val::Px(BUTTON),
                                flex_shrink: 0.0,
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                border_radius: BorderRadius::all(Val::Px(2.0)),
                                ..default()
                            },
                            BackgroundColor(Color::srgb_u8(0x03, 0x92, 0x03)),
                            children![(icon("check-bold", 14.0, Color::WHITE), Pickable::IGNORE)],
                        ),
                        (
                            Name::new(format!("{name}-cancel")),
                            DimEditButton::Cancel,
                            Node {
                                width: Val::Px(BUTTON),
                                height: Val::Px(BUTTON),
                                flex_shrink: 0.0,
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                            children![(
                                icon("x-bold", 14.0, Color::srgb_u8(0xd0, 0x1f, 0x1f)),
                                Pickable::IGNORE
                            )],
                        ),
                    ],
                ),
                (
                    Name::new(format!("{name}-message")),
                    DimEditMessage,
                    Text::new(""),
                    theme.font(12.0, FontWeight::NORMAL),
                    TextColor(colors.error),
                    TextLayout::no_wrap(),
                    Node {
                        display: Display::None,
                        padding: UiRect::new(Val::Px(8.0), Val::Px(8.0), Val::Px(0.0), Val::Px(6.0)),
                        ..default()
                    },
                ),
            ],
        )
    }
}

/// The ✓ (commit) and ✕ (cancel) buttons.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum DimEditButton {
    Accept,
    Cancel,
}

fn on_button_click(
    click: On<Pointer<Click>>,
    q_button: Query<(&DimEditButton, &ChildOf)>,
    q_children: Query<&Children>,
    q_field: Query<&EditableText, With<DimEditField>>,
    mut commands: Commands,
) {
    let Ok((button, parent)) = q_button.get(click.entity) else {
        return;
    };
    let Ok(children) = q_children.get(parent.parent()) else {
        return;
    };
    let Some((field, text)) = children
        .iter()
        .find_map(|c| q_field.get(c).ok().map(|t| (c, t.value().to_string())))
    else {
        return;
    };
    match button {
        DimEditButton::Accept => commands.trigger(DimEditCommit {
            entity: field,
            value: text,
        }),
        DimEditButton::Cancel => commands.trigger(DimEditCancel { entity: field }),
    }
}

/// Focuses the field and selects its value as soon as it exists.
#[derive(Component, Debug, Clone, Copy)]
#[component(on_insert = on_focus_and_select)]
struct FocusAndSelect(Option<usize>);

fn on_focus_and_select(mut world: DeferredWorld, ctx: HookContext) {
    let e = ctx.entity;
    let prefix = world.get::<FocusAndSelect>(e).and_then(|f| f.0);
    world.commands().queue(move |world: &mut World| {
        if let Some(mut text) = world.get_mut::<EditableText>(e) {
            match prefix {
                Some(n) => {
                    text.queue_edit(TextEdit::TextStart(false));
                    for _ in 0..n {
                        text.queue_edit(TextEdit::Right(true));
                    }
                }
                None => text.queue_edit(TextEdit::SelectAll),
            }
        }
        if let Some(mut focus) = world.get_resource_mut::<InputFocus>() {
            focus.set(e, FocusCause::Pressed);
        }
    });
}

fn on_key(
    mut ev: On<FocusedInput<KeyboardInput>>,
    q_field: Query<&EditableText, With<DimEditField>>,
    mut commands: Commands,
) {
    let entity = ev.focused_entity;
    let Ok(text) = q_field.get(entity) else {
        return;
    };
    if ev.input.state != ButtonState::Pressed {
        return;
    }
    match ev.input.key_code {
        KeyCode::Enter | KeyCode::NumpadEnter => {
            ev.propagate(false);
            commands.trigger(DimEditCommit {
                entity,
                value: text.value().to_string(),
            });
        }
        KeyCode::Escape => {
            ev.propagate(false);
            commands.trigger(DimEditCancel { entity });
        }
        _ => {}
    }
}

/// The DimEditBox of a field (its grandparent).
fn box_of(field: Entity, q_parent: &Query<&ChildOf>) -> Option<Entity> {
    let row = q_parent.get(field).ok()?.parent();
    Some(q_parent.get(row).ok()?.parent())
}

#[allow(clippy::type_complexity)]
fn style_boxes(
    mut q_box: Query<(Entity, &mut DimEditBox, &mut BorderColor, &Children)>,
    mut q_field: Query<(Entity, &EditableText, &mut TextColor), With<DimEditField>>,
    mut q_msg: Query<(&mut Text, &mut Node), (With<DimEditMessage>, Without<DimEditField>)>,
    q_parent: Query<&ChildOf>,
) {
    for (field, text, mut color) in &mut q_field {
        let Some(b) = box_of(field, &q_parent) else {
            continue;
        };
        let Ok((_, mut bx, mut border, children)) = q_box.get_mut(b) else {
            continue;
        };
        // Typing clears the error.
        if bx.error.is_some() && text.value().to_string() != bx.error_text {
            bx.error = None;
        }
        let bad = bx.error.is_some();
        let c = bx.colors;
        border.set_if_neq(BorderColor::all(if bad { c.error } else { c.border }));
        color.set_if_neq(TextColor(if bad { c.error } else { c.text }));
        for child in children.iter() {
            if let Ok((mut msg, mut node)) = q_msg.get_mut(child) {
                let want = bx.error.clone().unwrap_or_default();
                if msg.0 != want {
                    msg.0 = want;
                }
                let display = if bad { Display::Flex } else { Display::None };
                if node.display != display {
                    node.display = display;
                }
            }
        }
    }
}
