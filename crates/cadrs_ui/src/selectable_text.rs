//! Read-only text the user can select and copy, modeled on gpui-component's selectable `Label`
//! and its `Clipboard` button (`clipboard.rs`).
//!
//! - Drag to select, double-click for a word, triple-click for everything; Ctrl+C copies the
//!   selection (all from Bevy's `EditableText`).
//! - Typing, Backspace/Delete, paste and IME input are dropped ([`ReadOnlyText`]); Cut copies.
//!   There is no caret.
//! - An optional copy icon after the text (named `<name>-copy`) copies the whole value and
//!   shows a ✓ for a moment. It appears while the pointer is over the text, keeping its space so
//!   nothing moves. A copy triggers [`TextCopied`], which bubbles.
//! - Esc, or a press anywhere else, leaves the text, so the app's shortcuts work again.
//!
//! The outer entity gets the builder's name and holds the value in [`SelectableValue`]: set the
//! value there and the text follows. The inner text entity is named `<name>-field`.

use std::borrow::Cow;
use std::time::Duration;

use bevy::ecs::spawn::SpawnWith;
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusedInput, InputFocus};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::{EditableText, EditableTextSystems, FontWeight, TextCursorStyle, TextEdit};

use crate::shortcut_key::ShortcutKey;
use crate::theme::Theme;
use crate::tooltip::Tooltip;

/// How long the copy icon shows a ✓ after a copy.
const COPIED_FOR: Duration = Duration::from_millis(1500);

pub struct SelectableTextPlugin;

impl Plugin for SelectableTextPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_copy_click)
            .add_observer(on_field_escape)
            .add_systems(
                PreUpdate,
                leave_on_press_elsewhere.after(bevy::picking::PickingSystems::Hover),
            )
            .add_systems(
                PostUpdate,
                (
                    sync_values.before(drop_edits),
                    drop_edits.before(EditableTextSystems),
                    style_copy_icons,
                ),
            );
    }
}

/// The value of a [`SelectableText`]. Changing it replaces the text (and any selection).
#[derive(Component, Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectableValue(pub String);

/// An `EditableText` that can be selected and copied but not changed.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct ReadOnlyText;

/// The text entity of a [`SelectableText`].
#[derive(Component, Debug, Clone, Copy, Default)]
#[require(crate::input::KeyboardField)]
pub struct SelectableTextField;

/// A [`SelectableText`]'s copy icon, and when it last copied.
#[derive(Component, Debug, Clone, Default)]
pub struct CopyButton {
    copied_at: Option<Duration>,
    colors: CopyColors,
}

#[derive(Debug, Clone, Copy, Default)]
struct CopyColors {
    normal: Color,
    hover: Color,
    copied: Color,
}

/// The copy icon of a selectable text copied its value. Targets the outer entity and bubbles.
#[derive(EntityEvent, Clone, Debug)]
#[entity_event(propagate, auto_propagate)]
pub struct TextCopied {
    pub entity: Entity,
    pub value: String,
}

/// Builder for a selectable, read-only text.
#[derive(Debug, Clone)]
pub struct SelectableText {
    name: Cow<'static, str>,
    value: String,
    font: Option<(f32, FontWeight)>,
    color: Option<Color>,
    justify: Justify,
    copy_button: bool,
}

impl SelectableText {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            value: String::new(),
            font: None,
            color: None,
            justify: Justify::Left,
            copy_button: false,
        }
    }

    pub fn value(mut self, v: impl Into<String>) -> Self {
        self.value = v.into();
        self
    }

    /// Overrides the font size and weight (default 13 px regular).
    pub fn font(mut self, size: f32, weight: FontWeight) -> Self {
        self.font = Some((size, weight));
        self
    }

    /// Overrides the text color (default the theme's foreground).
    pub fn color(mut self, c: Color) -> Self {
        self.color = Some(c);
        self
    }

    /// Aligns the text to the right end (numbers in a column).
    pub fn align_right(mut self) -> Self {
        self.justify = Justify::Right;
        self
    }

    /// A copy icon after the text that copies the whole value (gpui-component's `Clipboard`).
    pub fn copy_button(mut self) -> Self {
        self.copy_button = true;
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let (font_size, font_weight) = self.font.unwrap_or((theme.font_base, FontWeight::NORMAL));
        let font = theme.font(font_size, font_weight);
        let fg = self.color.unwrap_or(theme.foreground);
        let cursor = TextCursorStyle {
            color: Color::NONE,
            selection_color: theme.selection,
            unfocused_selection_color: Color::NONE,
            selected_text_color: Some(theme.selection_foreground),
            selection_radius: 0.0,
        };
        let icon_size = (font_size - 1.0).round().max(10.0);
        let colors = CopyColors {
            normal: theme.muted_foreground,
            hover: theme.tool_foreground,
            copied: theme.success,
        };
        let field_name = format!("{}-field", self.name);
        let copy_name = format!("{}-copy", self.name);
        let SelectableText {
            name,
            value,
            justify,
            copy_button,
            ..
        } = self;

        (
            Name::new(name.into_owned()),
            SelectableValue(value.clone()),
            Hovered::default(),
            Node {
                flex_grow: 1.0,
                align_items: AlignItems::Center,
                column_gap: Val::Px(4.0),
                ..default()
            },
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                let mut editable = EditableText::new(&value);
                editable.visible_lines = Some(1.0);
                p.spawn((
                    Name::new(field_name),
                    SelectableTextField,
                    ReadOnlyText,
                    editable,
                    font,
                    TextColor(fg),
                    TextLayout::new(justify, LineBreak::NoWrap),
                    cursor,
                    TabIndex(0),
                    Node {
                        flex_grow: 1.0,
                        min_width: Val::Px(0.0),
                        ..default()
                    },
                ));
                if copy_button {
                    p.spawn((
                        Name::new(copy_name),
                        CopyButton {
                            copied_at: None,
                            colors,
                        },
                        Hovered::default(),
                        Tooltip::new("Copy"),
                        crate::icon::icon("copy", icon_size, colors.normal),
                        Visibility::Hidden,
                    ));
                }
            })),
        )
    }
}

/// Copies a changed [`SelectableValue`] into its text.
fn sync_values(
    q: Query<(&SelectableValue, &Children), Changed<SelectableValue>>,
    mut q_field: Query<&mut EditableText, With<SelectableTextField>>,
) {
    for (value, children) in &q {
        for child in children.iter() {
            if let Ok(mut text) = q_field.get_mut(child)
                && text.value().to_string() != value.0
            {
                text.editor_mut().set_text(&value.0);
            }
        }
    }
}

/// Drops the edits that would change a read-only text; Cut copies instead.
fn drop_edits(mut q: Query<&mut EditableText, With<ReadOnlyText>>) {
    for mut text in &mut q {
        if text.pending_edits.is_empty() {
            continue;
        }
        let edits = std::mem::take(&mut text.bypass_change_detection().pending_edits);
        let kept: Vec<TextEdit> = edits
            .into_iter()
            .filter_map(|e| match e {
                TextEdit::Cut => Some(TextEdit::Copy),
                TextEdit::Paste
                | TextEdit::Insert(_)
                | TextEdit::Backspace
                | TextEdit::BackspaceWord
                | TextEdit::Delete
                | TextEdit::DeleteWord
                | TextEdit::ImeSetCompose { .. }
                | TextEdit::ImeCommit { .. } => None,
                e => Some(e),
            })
            .collect();
        text.pending_edits = kept;
    }
}

/// Esc in a read-only text leaves it.
fn on_field_escape(
    ev: On<FocusedInput<KeyboardInput>>,
    q: Query<(), With<ReadOnlyText>>,
    mut focus: ResMut<InputFocus>,
) {
    if q.contains(ev.focused_entity)
        && ev.input.state == ButtonState::Pressed
        && ev.input.shortcut_code() == KeyCode::Escape
    {
        focus.clear();
    }
}

/// A press anywhere but on the focused read-only text leaves it (it holds no edit to commit,
/// and while it has the focus the app's shortcuts are off).
fn leave_on_press_elsewhere(
    mouse: Res<ButtonInput<MouseButton>>,
    mut focus: ResMut<InputFocus>,
    q: Query<&Hovered, With<ReadOnlyText>>,
) {
    if !mouse.get_just_pressed().any(|_| true) {
        return;
    }
    if let Some(f) = focus.get()
        && let Ok(hovered) = q.get(f)
        && !hovered.get()
    {
        focus.clear();
    }
}

/// A click on a copy icon copies its text's whole value.
fn on_copy_click(
    click: On<PointerClick>,
    mut q_button: Query<(&mut CopyButton, &ChildOf)>,
    q_value: Query<&SelectableValue>,
    time: Res<Time<Real>>,
    mut commands: Commands,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Ok((mut button, parent)) = q_button.get_mut(click.entity) else {
        return;
    };
    let Ok(value) = q_value.get(parent.parent()) else {
        return;
    };
    let value = value.0.clone();
    button.copied_at = Some(time.elapsed());
    let target = parent.parent();
    commands.queue(move |world: &mut World| {
        if let Some(mut clip) = world.get_resource_mut::<bevy::clipboard::Clipboard>()
            && let Err(e) = clip.set_text(value.clone())
        {
            warn!("clipboard: {e:?}");
        }
        world.trigger(TextCopied {
            entity: target,
            value,
        });
    });
}

/// Shows a copy icon while its text is hovered (or it has just copied), as ✓ after a copy.
fn style_copy_icons(
    time: Res<Time<Real>>,
    mut q: Query<(
        &mut CopyButton,
        &ChildOf,
        &Hovered,
        &mut crate::icon::Icon,
        &mut ImageNode,
        &mut Visibility,
    )>,
    q_root: Query<(&Hovered, &SelectableValue)>,
) {
    let now = time.elapsed();
    for (mut button, parent, hovered, mut icon, mut image, mut vis) in &mut q {
        if button
            .copied_at
            .is_some_and(|t| now.saturating_sub(t) >= COPIED_FOR)
        {
            button.copied_at = None;
        }
        let copied = button.copied_at.is_some();
        let shown = copied
            || q_root
                .get(parent.parent())
                .is_ok_and(|(h, v)| h.get() && !v.0.is_empty());
        vis.set_if_neq(if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        let name = if copied { "check" } else { "copy" };
        if icon.name != name {
            icon.name = name.into();
        }
        let c = button.colors;
        let color = if copied {
            c.copied
        } else if hovered.get() {
            c.hover
        } else {
            c.normal
        };
        if image.color != color {
            image.color = color;
        }
    }
}
