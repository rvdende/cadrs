//! An editable cell of a spreadsheet-like table (Onshape's Variable Studio, gpui-component's
//! editable `Table` cells): borderless text that shows a blue outline while it is edited.
//!
//! - The cell shows its [`GridCellState`]'s text whenever it isn't being edited; the owner keeps
//!   the state (the cell never changes the owner's data by itself).
//! - Pressing the cell, or tabbing to it, edits it with the whole text selected.
//! - **Enter** commits and leaves the cell; leaving it any other way (Tab to the next cell, a
//!   click elsewhere) commits too, if the text changed. Either triggers [`GridCellCommit`] on
//!   the cell (`enter` tells which). **Esc** puts the state's text back and leaves the cell.
//! - With [`GridCell::placeholder`] an empty cell shows a hint (a table's "Add variable" row).
//! - The cell's root gets the builder's name; its editable text is `<name>-field`.

use std::borrow::Cow;

use bevy::ecs::spawn::SpawnWith;
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusCause, FocusedInput, InputFocus};
use bevy::prelude::*;
use bevy::text::{EditableText, EditableTextFilter, FontWeight, TextCursorStyle, TextEdit};

use crate::input::TextInputField;
use crate::shortcut_key::ShortcutKey;
use crate::theme::Theme;

pub struct GridCellPlugin;

impl Plugin for GridCellPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_cell_key)
            .add_observer(on_cell_press)
            .add_systems(
                PostUpdate,
                (commit_on_blur, sync_cells)
                    .chain()
                    .before(bevy::ui::UiSystems::Prepare),
            );
    }
}

/// What a cell shows while it isn't edited, and whether that is in error (red text).
#[derive(Component, Debug, Clone, PartialEq, Eq, Default)]
pub struct GridCellState {
    pub text: String,
    pub error: bool,
}

/// A cell's edit was committed: Enter, or leaving the cell after changing its text. Targets
/// the cell's root.
#[derive(EntityEvent, Debug, Clone)]
pub struct GridCellCommit {
    pub entity: Entity,
    pub text: String,
    /// Committed with Enter (not by leaving the cell).
    pub enter: bool,
}

/// A cell's editable text; points at the cell's root.
#[derive(Component, Debug, Clone, Copy)]
pub struct GridCellEdit(pub Entity);

/// The hint an empty cell shows.
#[derive(Component, Debug, Clone, Copy)]
struct GridCellHint;

/// Builder for an editable cell.
#[derive(Debug, Clone)]
pub struct GridCell {
    name: Cow<'static, str>,
    text: String,
    placeholder: String,
    width: Val,
    grow: f32,
    height: f32,
    font_size: Option<f32>,
    weight: FontWeight,
    error: bool,
    accept: Option<fn(char) -> bool>,
}

impl GridCell {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            text: String::new(),
            placeholder: String::new(),
            width: Val::Auto,
            grow: 0.0,
            height: 28.0,
            font_size: None,
            weight: FontWeight::NORMAL,
            error: false,
            accept: None,
        }
    }

    /// The text shown (the state's, to start with).
    pub fn text(mut self, t: impl Into<String>) -> Self {
        self.text = t.into();
        self
    }

    /// The hint shown while the cell is empty.
    pub fn placeholder(mut self, p: impl Into<String>) -> Self {
        self.placeholder = p.into();
        self
    }

    /// A fixed width.
    pub fn width(mut self, w: Val) -> Self {
        self.width = w;
        self
    }

    /// A share of the row's free width (as a table column's weight).
    pub fn grow(mut self, weight: f32) -> Self {
        self.grow = weight;
        self
    }

    /// Only these characters can be typed (a name cell: letters, digits and `_`).
    pub fn accept(mut self, f: fn(char) -> bool) -> Self {
        self.accept = Some(f);
        self
    }

    /// Starts in error (red text).
    pub fn error(mut self, e: bool) -> Self {
        self.error = e;
        self
    }

    pub fn height(mut self, h: f32) -> Self {
        self.height = h;
        self
    }

    pub fn font(mut self, size: f32, weight: FontWeight) -> Self {
        self.font_size = Some(size);
        self.weight = weight;
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let t = theme.clone();
        let size = self.font_size.unwrap_or(theme.font_base);
        let font = theme.font(size, self.weight);
        let GridCell {
            name,
            text,
            placeholder,
            width,
            grow,
            height,
            error,
            accept,
            ..
        } = self;
        let edit_name = format!("{name}-field");
        (
            Name::new(name.into_owned()),
            GridCellState {
                text: text.clone(),
                error,
            },
            Node {
                width,
                flex_grow: grow,
                flex_basis: if grow > 0.0 { Val::Px(0.0) } else { Val::Auto },
                min_width: Val::Px(0.0),
                height: Val::Px(height),
                padding: UiRect::horizontal(Val::Px(6.0)),
                border: UiRect::all(Val::Px(2.0)),
                align_items: AlignItems::Center,
                overflow: Overflow::clip(),
                ..default()
            },
            BorderColor::all(Color::NONE),
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                let root = p.target_entity();
                p.spawn((
                    Name::new(edit_name),
                    GridCellEdit(root),
                    TextInputField,
                    EditableText::new(&text),
                    EditableTextFilter::new(move |c: char| {
                        !c.is_control() && accept.is_none_or(|f| f(c))
                    }),
                    font.clone(),
                    TextColor(t.foreground),
                    TextLayout::no_wrap(),
                    TextCursorStyle {
                        color: t.caret,
                        selection_color: t.selection,
                        unfocused_selection_color: Color::NONE,
                        selected_text_color: Some(t.selection_foreground),
                        selection_radius: 0.0,
                    },
                    TabIndex(0),
                    Node {
                        flex_grow: 1.0,
                        ..default()
                    },
                ));
                if !placeholder.is_empty() {
                    p.spawn((
                        GridCellHint,
                        Text::new(placeholder),
                        font,
                        TextColor(t.subtle_foreground),
                        TextLayout::no_wrap(),
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(6.0),
                            ..default()
                        },
                        Pickable::IGNORE,
                    ));
                }
            })),
        )
    }
}

/// Leaving a cell after changing its text commits it (compared with the text when it gained
/// the focus, as `dialog_fields`' number fields: the owner may change the state meanwhile).
/// Runs before [`sync_cells`], which shows the state's text once the cell is left.
fn commit_on_blur(
    focus: Res<InputFocus>,
    mut last: Local<Option<Entity>>,
    mut at_focus: Local<Option<String>>,
    q_edit: Query<(&EditableText, &GridCellEdit)>,
    mut q_state: Query<&mut GridCellState>,
    mut commands: Commands,
) {
    let now = focus.get();
    if *last == now {
        return;
    }
    if let Some(prev) = *last
        && let Ok((text, edit)) = q_edit.get(prev)
        && at_focus.as_deref().is_some_and(|t| text.value() != t)
    {
        let value = text.value().to_string();
        // Shown as typed until the owner says otherwise (no flash of the old text).
        if let Ok(mut st) = q_state.get_mut(edit.0) {
            st.text = value.clone();
        }
        commands.trigger(GridCellCommit {
            entity: edit.0,
            text: value,
            enter: false,
        });
    }
    *at_focus = now
        .and_then(|e| q_edit.get(e).ok())
        .map(|(t, _)| t.value().to_string());
    *last = now;
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn sync_cells(
    theme: Res<Theme>,
    focus: Res<InputFocus>,
    q_state: Query<&GridCellState>,
    mut q_edit: Query<(Entity, &GridCellEdit, &mut EditableText, &mut TextColor)>,
    mut q_root: Query<&mut BorderColor, With<GridCellState>>,
    mut q_hint: Query<(&GridCellHint, &ChildOf, &mut Visibility)>,
    q_children: Query<&Children>,
    // (A cell's text changes only while it is being edited, or by its state: the focus and
    // the state cover it.)
    q_changed: Query<(), Or<(Changed<GridCellState>, Added<GridCellEdit>)>>,
    mut focused_before: Local<Option<Entity>>,
) {
    // Nothing to do while no cell changed and the focus stayed put.
    if !focus.is_changed() && q_changed.is_empty() {
        return;
    }
    let focused = focus.get();
    for (e, edit, mut text, mut color) in &mut q_edit {
        let Ok(state) = q_state.get(edit.0) else {
            continue;
        };
        let is_focused = focused == Some(e);
        if !is_focused && text.value() != state.text.as_str() {
            text.editor_mut().set_text(&state.text);
        }
        if is_focused && *focused_before != Some(e) {
            text.queue_edit(TextEdit::SelectAll);
        }
        let c = if state.error && !is_focused {
            theme.danger
        } else {
            theme.foreground
        };
        color.set_if_neq(TextColor(c));
        if let Ok(mut border) = q_root.get_mut(edit.0) {
            let want = BorderColor::all(if is_focused {
                theme.input_border_focus
            } else {
                Color::NONE
            });
            border.set_if_neq(want);
        }
    }
    // The hints: shown while their cell is empty and not being edited.
    for (_, parent, mut vis) in &mut q_hint {
        let shown = q_children
            .get(parent.parent())
            .into_iter()
            .flatten()
            .filter_map(|c| q_edit.get(*c).ok())
            .any(|(e, _, text, _)| focused != Some(e) && text.value() == "");
        vis.set_if_neq(if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    *focused_before = focused;
}

fn on_cell_key(
    mut ev: On<FocusedInput<KeyboardInput>>,
    mut q_edit: Query<(&mut EditableText, &GridCellEdit)>,
    mut q_state: Query<&mut GridCellState>,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
) {
    let Ok((mut text, edit)) = q_edit.get_mut(ev.focused_entity) else {
        return;
    };
    if ev.input.state != ButtonState::Pressed || text.is_composing() {
        return;
    }
    match ev.input.shortcut_code() {
        KeyCode::Enter | KeyCode::NumpadEnter => {
            ev.propagate(false);
            let value = text.value().to_string();
            if let Ok(mut st) = q_state.get_mut(edit.0) {
                st.text = value.clone();
            }
            // Leaving after Enter doesn't commit a second time.
            focus.clear();
            commands.trigger(GridCellCommit {
                entity: edit.0,
                text: value,
                enter: true,
            });
        }
        KeyCode::Escape => {
            ev.propagate(false);
            if let Ok(st) = q_state.get(edit.0) {
                text.editor_mut().set_text(&st.text);
            }
            focus.clear();
        }
        _ => {}
    }
}

/// Pressing a cell's padding edits it, the caret at the end.
fn on_cell_press(
    mut press: On<PointerPress>,
    mut q_edit: Query<(Entity, &GridCellEdit, &mut EditableText)>,
    q_root: Query<(), With<GridCellState>>,
    mut focus: ResMut<InputFocus>,
) {
    if !q_root.contains(press.entity) {
        return;
    }
    let root = press.entity;
    press.propagate(false);
    if let Some((e, _, mut text)) = q_edit.iter_mut().find(|(_, g, _)| g.0 == root)
        && focus.get() != Some(e)
    {
        focus.set(e, FocusCause::Pressed);
        text.queue_edit(TextEdit::TextEnd(false));
    }
}

/// Edits the cell `root` (its text selected), e.g. the next cell of a row just added.
pub fn focus_cell(world: &mut World, root: Entity) {
    let mut q = world.query::<(Entity, &GridCellEdit)>();
    if let Some((e, _)) = q.iter(world).find(|(_, g)| g.0 == root) {
        world
            .resource_mut::<InputFocus>()
            .set(e, FocusCause::Navigated);
    }
}
