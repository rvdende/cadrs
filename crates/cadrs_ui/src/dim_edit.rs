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
//! - **`#` lists the variables** ([`DimEditVariables`], set by the owner) under the box, as
//!   Onshape does: each `#name` with its value on the right, filtered by what follows the `#`,
//!   then **New variable…** ([`DimEditNewVariable`]). Up/Down move the highlight, Enter or Tab
//!   (or a click) puts the highlighted name in, Esc closes the list.
//!
//! The owner positions the box (absolutely: set `left`/`top`).

use crate::shortcut_key::ShortcutKey;
use std::borrow::Cow;
use std::ops::Range;

use bevy::ecs::lifecycle::HookContext;
use bevy::ecs::world::DeferredWorld;
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{AcquireFocus, FocusCause, FocusedInput, InputFocus};
use bevy::prelude::*;
use bevy::text::{EditableText, EditableTextFilter, FontWeight, TextCursorStyle, TextEdit};

use crate::icon::icon;
use crate::input::TextInputField;
use crate::theme::Theme;

pub struct DimEditPlugin;

impl Plugin for DimEditPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_key)
            .add_observer(on_button_click)
            .add_observer(on_row_click)
            .add_observer(on_row_over)
            .add_observer(keep_focus_in_list)
            .add_systems(Update, update_suggestions)
            .add_systems(
                PostUpdate,
                (style_boxes, show_suggestions).before(bevy::ui::UiSystems::Prepare),
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

/// **New variable…** picked in the `#` list. Targets the editor's text field and bubbles. The
/// owner creates the variable and puts its name in with [`complete_variable`].
#[derive(EntityEvent, Clone, Debug)]
#[entity_event(propagate, auto_propagate)]
pub struct DimEditNewVariable {
    pub entity: Entity,
    /// What follows the `#` being typed (may be empty or not a name yet).
    pub name: String,
}

/// A variable the `#` list offers: its name (without `#`) and its value as shown ("5 mm").
#[derive(Debug, Clone, PartialEq)]
pub struct DimEditVariable {
    pub name: String,
    pub value: String,
}

/// The variables a dimension editor offers when `#` is typed (on the [`DimEditBox`]; the owner
/// may change them while it is open).
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct DimEditVariables(pub Vec<DimEditVariable>);

/// The editor (outer node).
#[derive(Component, Debug, Clone)]
pub struct DimEditBox {
    /// Set by the owner when the value cannot be used: shown under the box until the text
    /// changes.
    pub error: Option<String>,
    /// The text the error was set for.
    error_text: String,
    colors: Colors,
    suggest: Suggest,
}

/// The `#` list's state.
#[derive(Debug, Clone, Default, PartialEq)]
struct Suggest {
    /// The name being typed after a `#`: its byte range in the text (the caret is in it).
    token: Option<Range<usize>>,
    /// The variables listed (indices into [`DimEditVariables`]); New variable… follows them.
    rows: Vec<usize>,
    /// The highlighted row (`rows.len()` is New variable…).
    hi: usize,
    /// Esc closed the list for this text (it opens again when the text changes).
    closed_for: Option<String>,
    /// A name was just put in: the list stays closed on the text that makes.
    closing: bool,
}

impl Suggest {
    fn open(&self) -> bool {
        self.token.is_some() && self.closed_for.is_none() && !self.closing
    }
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
    popover: Color,
    list_border: Color,
    hover: Color,
    muted: Color,
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
    variables: Vec<DimEditVariable>,
}

/// Measured in `screens/16b`: an 11 px handle strip, a 1 px separator and a 32 px value row.
/// The value row ends with a green ✓ and a red ✕ (`dimension-active-field-01-01.png`).
const HANDLE: f32 = 11.0;
const ROW: f32 = 32.0;
/// The editor's minimum width (it widens for a long value or message).
pub const WIDTH: f32 = 176.0;
/// The ✓ and ✕ buttons (`dimension-active-field-01-01.png`).
const BUTTON: f32 = 20.0;
/// The `#` list's minimum width and row height, and the most variables it shows.
const LIST_WIDTH: f32 = 200.0;
const LIST_ROW: f32 = 24.0;
const LIST_MAX: usize = 8;
/// The editor's height (without the error message).
pub const HEIGHT: f32 = HANDLE + 1.0 + ROW + 2.0;

impl DimEdit {
    pub fn new(name: impl Into<Cow<'static, str>>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            select: None,
            variables: Vec::new(),
        }
    }

    /// The variables `#` lists.
    pub fn variables(mut self, variables: Vec<DimEditVariable>) -> Self {
        self.variables = variables;
        self
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
            popover: theme.popover,
            list_border: theme.border,
            hover: theme.menu_hover,
            muted: theme.muted_foreground,
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
                suggest: Suggest::default(),
            },
            DimEditVariables(self.variables),
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
                        padding: UiRect::new(
                            Val::Px(6.0),
                            Val::Px(8.0),
                            Val::Px(0.0),
                            Val::Px(0.0)
                        ),
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
                                selection_radius: 0.0,
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
                        padding: UiRect::new(
                            Val::Px(8.0),
                            Val::Px(8.0),
                            Val::Px(0.0),
                            Val::Px(6.0)
                        ),
                        ..default()
                    },
                ),
                (
                    Name::new(format!("{name}-variables")),
                    DimEditList,
                    Node {
                        display: Display::None,
                        position_type: PositionType::Absolute,
                        top: Val::Percent(100.0),
                        left: Val::Px(-1.0),
                        margin: UiRect::top(Val::Px(2.0)),
                        min_width: Val::Px(LIST_WIDTH),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::vertical(Val::Px(4.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(2.0)),
                        ..default()
                    },
                    BackgroundColor(colors.popover),
                    BorderColor::all(colors.list_border),
                    BoxShadow::new(
                        theme.shadow,
                        Val::Px(0.0),
                        Val::Px(2.0),
                        Val::Px(0.0),
                        Val::Px(6.0),
                    ),
                    ZIndex(1),
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
    click: On<PointerClick>,
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
    mut q_field: Query<&mut EditableText, With<DimEditField>>,
    mut q_box: Query<(&mut DimEditBox, &DimEditVariables)>,
    q_parent: Query<&ChildOf>,
    mut commands: Commands,
) {
    let entity = ev.focused_entity;
    let Ok(mut text) = q_field.get_mut(entity) else {
        return;
    };
    if ev.input.state != ButtonState::Pressed {
        return;
    }
    let code = ev.input.shortcut_code();
    // The `#` list takes the keys that move in it, pick from it or close it.
    if let Some(b) = box_of(entity, &q_parent)
        && let Ok((mut bx, vars)) = q_box.get_mut(b)
        && bx.suggest.open()
    {
        let s = &mut bx.suggest;
        let n = s.rows.len() + 1;
        match code {
            KeyCode::ArrowDown | KeyCode::ArrowUp => {
                ev.propagate(false);
                s.hi = if code == KeyCode::ArrowDown {
                    (s.hi + 1) % n
                } else {
                    (s.hi + n - 1) % n
                };
                // The text input moves the caret to the line's start or end on these: not here.
                commands.queue(move |world: &mut World| {
                    if let Some(mut t) = world.get_mut::<EditableText>(entity) {
                        t.pending_edits
                            .retain(|e| !matches!(e, TextEdit::Up(_) | TextEdit::Down(_)));
                    }
                });
                return;
            }
            KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Tab => {
                ev.propagate(false);
                pick(&mut bx, vars, &mut text, entity, &mut commands);
                return;
            }
            KeyCode::Escape => {
                ev.propagate(false);
                s.closed_for = Some(text.value().to_string());
                return;
            }
            _ => {}
        }
    }
    match code {
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

/// Puts the highlighted variable in place of the `#name` being typed, or asks the owner for a
/// new one.
fn pick(
    bx: &mut DimEditBox,
    vars: &DimEditVariables,
    text: &mut EditableText,
    field: Entity,
    commands: &mut Commands,
) {
    let s = &mut bx.suggest;
    match s.rows.get(s.hi) {
        Some(&i) => {
            if let Some(v) = vars.0.get(i) {
                replace_name(text, &v.name);
            }
        }
        None => {
            let value = text.value().to_string();
            let name = s
                .token
                .clone()
                .and_then(|r| value.get(r))
                .unwrap_or_default()
                .to_string();
            commands.trigger(DimEditNewVariable {
                entity: field,
                name,
            });
        }
    }
    s.token = None;
    s.closing = true;
}

/// The caret's byte index, if nothing is selected.
fn caret(text: &EditableText) -> Option<usize> {
    let sel = text.editor().raw_selection();
    sel.is_collapsed().then(|| sel.focus().index())
}

/// The name being typed after a `#` at `caret`: its byte range (empty right after the `#`).
fn hash_token(text: &str, caret: usize) -> Option<Range<usize>> {
    if !text.is_char_boundary(caret) {
        return None;
    }
    let is_name = |c: char| c.is_alphanumeric() || c == '_';
    let start = text[..caret]
        .char_indices()
        .rev()
        .take_while(|&(_, c)| is_name(c))
        .last()
        .map_or(caret, |(i, _)| i);
    if !text[..start].ends_with('#') {
        return None;
    }
    let end = text[caret..]
        .char_indices()
        .find(|&(_, c)| !is_name(c))
        .map_or(text.len(), |(i, _)| caret + i);
    Some(start..end)
}

/// The variables matching what follows the `#` (ignoring case): those starting with it, then
/// those containing it, at most [`LIST_MAX`].
fn matching(vars: &[DimEditVariable], typed: &str) -> Vec<usize> {
    let typed = typed.to_lowercase();
    let names: Vec<String> = vars.iter().map(|v| v.name.to_lowercase()).collect();
    let starts = (0..vars.len()).filter(|&i| names[i].starts_with(&typed));
    let contains =
        (0..vars.len()).filter(|&i| !names[i].starts_with(&typed) && names[i].contains(&typed));
    starts.chain(contains).take(LIST_MAX).collect()
}

/// Replaces the `#name` at the caret with `#{name}` (or puts `#{name}` at the caret).
fn replace_name(text: &mut EditableText, name: &str) {
    let value = text.value().to_string();
    let Some(c) = caret(text) else {
        text.queue_edit(TextEdit::Insert(format!("#{name}").into()));
        return;
    };
    match hash_token(&value, c) {
        // The whole text replaced at once, so a caret moved meanwhile (a click in the list)
        // can't misplace it.
        Some(r) => {
            let new = format!("{}{name}{}", &value[..r.start], &value[r.end..]);
            text.queue_edit(TextEdit::SelectAll);
            text.queue_edit(TextEdit::Insert(new.into()));
        }
        None => text.queue_edit(TextEdit::Insert(format!("#{name}").into())),
    }
}

/// A press in the `#` list leaves the keyboard with the text field (the press would otherwise
/// look for something focusable up to the window, and the list would close before the click).
fn keep_focus_in_list(mut ev: On<AcquireFocus>, q_list: Query<(), With<DimEditList>>) {
    if q_list.contains(ev.focused_entity) {
        ev.propagate(false);
    }
}

/// Puts `#{name}` in the editor whose text field is `field`, in place of the `#name` being typed,
/// and closes the `#` list: for the owner, once it has made the variable New variable… asked
/// for (see [`DimEditNewVariable`]).
pub fn complete_variable(world: &mut World, field: Entity, name: &str) {
    if let Some(mut text) = world.get_mut::<EditableText>(field) {
        replace_name(&mut text, name);
    }
    let b = {
        let mut q = world.query::<&ChildOf>();
        q.get(world, field)
            .ok()
            .and_then(|row| q.get(world, row.parent()).ok())
            .map(|b| b.parent())
    };
    if let Some(mut bx) = b.and_then(|b| world.get_mut::<DimEditBox>(b)) {
        bx.suggest.token = None;
        bx.suggest.closing = true;
    }
}

/// Follows the caret: opens the `#` list while a name is typed after a `#`, filtered by it.
fn update_suggestions(
    q_field: Query<(Entity, &EditableText), With<DimEditField>>,
    mut q_box: Query<(&mut DimEditBox, &DimEditVariables)>,
    q_parent: Query<&ChildOf>,
    focus: Res<InputFocus>,
) {
    for (field, text) in &q_field {
        let Some(b) = box_of(field, &q_parent) else {
            continue;
        };
        let Ok((mut bx, vars)) = q_box.get_mut(b) else {
            continue;
        };
        let value = text.value().to_string();
        let mut s = bx.suggest.clone();
        // A pick closes the list on the text it made (once its edits are in).
        if s.closing && text.pending_edits.is_empty() {
            s.closing = false;
            s.closed_for = Some(value.clone());
        }
        if s.closed_for.as_ref().is_some_and(|t| *t != value) {
            s.closed_for = None;
        }
        let token = (focus.get() == Some(field) && !text.is_composing())
            .then(|| caret(text))
            .flatten()
            .and_then(|c| hash_token(&value, c));
        let rows = match &token {
            Some(r) => matching(&vars.0, &value[r.clone()]),
            None => Vec::new(),
        };
        if rows != s.rows {
            s.hi = 0;
        }
        s.rows = rows;
        s.token = token;
        s.hi = s.hi.min(s.rows.len());
        if bx.suggest != s {
            bx.suggest = s;
        }
    }
}

/// The `#` list (under the box).
#[derive(Component, Debug, Clone, Copy)]
pub struct DimEditList;

/// A row of the `#` list: its index (the variables' count is New variable…).
#[derive(Component, Debug, Clone, Copy)]
struct DimEditRow(usize);

/// What a `#` list shows, so its rows are rebuilt only when that changes.
#[derive(Component, Debug, Clone, PartialEq)]
struct Shown(Vec<DimEditVariable>);

#[allow(clippy::type_complexity)]
fn show_suggestions(
    q_box: Query<
        (&DimEditBox, &DimEditVariables, &Children, &Name),
        Or<(Changed<DimEditBox>, Changed<DimEditVariables>)>,
    >,
    mut q_list: Query<(Entity, &mut Node, Option<&Shown>), With<DimEditList>>,
    q_children: Query<&Children>,
    mut q_row: Query<(&DimEditRow, &mut BackgroundColor)>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    for (bx, vars, children, name) in &q_box {
        let Some(Ok((list, mut node, shown))) = children
            .iter()
            .find(|&c| q_list.contains(c))
            .map(|c| q_list.get_mut(c))
        else {
            continue;
        };
        let s = &bx.suggest;
        let display = if s.open() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        if !s.open() {
            continue;
        }
        let want: Vec<DimEditVariable> = s.rows.iter().map(|&i| vars.0[i].clone()).collect();
        if shown.is_none_or(|sh| sh.0 != want) {
            commands
                .entity(list)
                .despawn_children()
                .insert(Shown(want.clone()));
            let c = bx.colors;
            let font = theme.font(13.0, FontWeight::NORMAL);
            let row = |i: usize, label: String, value: String, icon_name: &'static str| {
                (
                    Name::new(match i < want.len() {
                        true => format!("{name}-variable-{label}"),
                        false => format!("{name}-new-variable"),
                    }),
                    DimEditRow(i),
                    Node {
                        height: Val::Px(LIST_ROW),
                        padding: UiRect::horizontal(Val::Px(8.0)),
                        column_gap: Val::Px(6.0),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(if i == s.hi { c.hover } else { Color::NONE }),
                    children![
                        (icon(icon_name, 14.0, c.text), Pickable::IGNORE),
                        (
                            Text::new(label),
                            font.clone(),
                            TextColor(c.text),
                            TextLayout::no_wrap(),
                            Node {
                                flex_grow: 1.0,
                                ..default()
                            },
                            Pickable::IGNORE,
                        ),
                        (
                            Text::new(value),
                            font.clone(),
                            TextColor(c.muted),
                            TextLayout::no_wrap(),
                            Pickable::IGNORE,
                        ),
                    ],
                )
            };
            for (i, v) in want.iter().enumerate() {
                commands.spawn((
                    row(i, format!("#{}", v.name), v.value.clone(), "variables"),
                    ChildOf(list),
                ));
            }
            commands.spawn((
                row(want.len(), "New variable…".into(), String::new(), "plus"),
                ChildOf(list),
            ));
            continue;
        }
        // Same rows: only the highlight moved.
        for r in q_children.get(list).into_iter().flatten() {
            if let Ok((row, mut bg)) = q_row.get_mut(*r) {
                let want = if row.0 == s.hi {
                    bx.colors.hover
                } else {
                    Color::NONE
                };
                bg.set_if_neq(BackgroundColor(want));
            }
        }
    }
}

/// The box of a `#` list row, and its text field.
fn row_box(
    row: Entity,
    q_parent: &Query<&ChildOf>,
    q_children: &Query<&Children>,
    q_field: &Query<(), With<DimEditField>>,
) -> Option<(Entity, Entity)> {
    let list = q_parent.get(row).ok()?.parent();
    let b = q_parent.get(list).ok()?.parent();
    let field = q_children
        .iter_descendants(b)
        .find(|&e| q_field.contains(e))?;
    Some((b, field))
}

#[allow(clippy::too_many_arguments)]
fn on_row_click(
    mut click: On<PointerClick>,
    q_row: Query<&DimEditRow>,
    q_parent: Query<&ChildOf>,
    q_children: Query<&Children>,
    q_is_field: Query<(), With<DimEditField>>,
    mut q_box: Query<(&mut DimEditBox, &DimEditVariables)>,
    mut q_text: Query<&mut EditableText>,
    mut commands: Commands,
) {
    let Ok(row) = q_row.get(click.entity) else {
        return;
    };
    click.propagate(false);
    let Some((b, field)) = row_box(click.entity, &q_parent, &q_children, &q_is_field) else {
        return;
    };
    let (Ok((mut bx, vars)), Ok(mut text)) = (q_box.get_mut(b), q_text.get_mut(field)) else {
        return;
    };
    bx.suggest.hi = row.0;
    pick(&mut bx, vars, &mut text, field, &mut commands);
    // The field keeps the keyboard.
    commands.queue(move |world: &mut World| {
        if let Some(mut focus) = world.get_resource_mut::<InputFocus>() {
            focus.set(field, FocusCause::Pressed);
        }
    });
}

fn on_row_over(
    over: On<PointerOver>,
    q_row: Query<&DimEditRow>,
    q_parent: Query<&ChildOf>,
    q_children: Query<&Children>,
    q_is_field: Query<(), With<DimEditField>>,
    mut q_box: Query<&mut DimEditBox>,
) {
    let Ok(row) = q_row.get(over.entity) else {
        return;
    };
    if let Some((b, _)) = row_box(over.entity, &q_parent, &q_children, &q_is_field)
        && let Ok(mut bx) = q_box.get_mut(b)
        && bx.suggest.hi != row.0
    {
        bx.suggest.hi = row.0;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn var(name: &str) -> DimEditVariable {
        DimEditVariable {
            name: name.into(),
            value: "5 mm".into(),
        }
    }

    #[test]
    fn a_name_after_a_hash_is_found_around_the_caret() {
        assert_eq!(hash_token("#", 1), Some(1..1));
        assert_eq!(hash_token("10 mm + #wi", 11), Some(9..11));
        // The caret inside the name: the whole name.
        assert_eq!(hash_token("#width + 2", 3), Some(1..6));
        assert_eq!(hash_token("#width + 2", 8), None);
        assert_eq!(hash_token("width", 5), None);
        assert_eq!(hash_token("", 0), None);
    }

    #[test]
    fn names_starting_with_the_typed_text_come_first() {
        let vars = [var("bore"), var("width"), var("wall"), var("Wheel")];
        assert_eq!(matching(&vars, ""), vec![0, 1, 2, 3]);
        assert_eq!(matching(&vars, "w"), vec![1, 2, 3]);
        assert_eq!(matching(&vars, "WH"), vec![3]);
        assert_eq!(matching(&vars, "e"), vec![0, 3]);
        assert_eq!(matching(&vars, "x"), Vec::<usize>::new());
    }

    #[test]
    fn the_list_shows_at_most_its_maximum() {
        let vars: Vec<_> = (0..20).map(|i| var(&format!("v{i}"))).collect();
        assert_eq!(matching(&vars, "v").len(), LIST_MAX);
    }
}
