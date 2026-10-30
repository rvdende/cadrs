//! The `#` list of a value field, as Onshape has it (`dimension-active-field-01-01.png`): typing
//! `#` in a field that takes expressions lists the variables under it, each `#name` with its
//! value on the right, filtered by what follows the `#`, then **New variable…**. Up/Down move the
//! highlight, Enter or Tab (or a click) puts the highlighted name in, Esc closes the list. The
//! dimension editor ([`crate::DimEdit`]) and the dialogs' number fields
//! ([`crate::NumberField::variables`]) have one.
//!
//! - [`VariableList`] goes on the field (the entity with the `EditableText`): the variables
//!   listed, whether New variable… is offered, and which of the field's ancestors the list opens
//!   under. The owner's key handler gives the list its keys first ([`list_key`]).
//! - New variable… triggers [`NewVariable`] on the field; the owner makes the variable and puts
//!   its name in with [`complete_variable`].
//! - The list is a popover over everything (not clipped by a dialog's scroll area), open only
//!   while a name is typed after a `#`.

use std::ops::Range;

use bevy::input_focus::{AcquireFocus, FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::text::{EditableText, FontWeight, TextEdit};
use bevy::ui_widgets::popover::{Popover, PopoverAlign, PopoverPlacement, PopoverSide};

use crate::icon::icon;
use crate::theme::Theme;
use crate::z;

pub struct VariableListPlugin;

impl Plugin for VariableListPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_row_click)
            .add_observer(on_row_over)
            .add_observer(keep_focus_in_list)
            .add_systems(Update, update_lists)
            .add_systems(PostUpdate, show_lists.before(bevy::ui::UiSystems::Prepare));
    }
}

/// **New variable…** picked in a `#` list. Targets the field and bubbles. The owner creates the
/// variable and puts its name in with [`complete_variable`].
#[derive(EntityEvent, Clone, Debug)]
#[entity_event(propagate, auto_propagate)]
pub struct NewVariable {
    pub entity: Entity,
    /// What follows the `#` being typed (may be empty or not a name yet).
    pub name: String,
}

/// A variable a `#` list offers: its name (without `#`) and its value as shown ("5 mm").
#[derive(Debug, Clone, PartialEq)]
pub struct ListedVariable {
    pub name: String,
    pub value: String,
}

/// A field's `#` list (on the entity with its `EditableText`).
#[derive(Component, Debug, Clone, PartialEq)]
pub struct VariableList {
    /// The variables listed (the owner may change them while the field is being edited).
    pub variables: Vec<ListedVariable>,
    /// New variable… follows them: the owner makes one on [`NewVariable`].
    pub new_variable: bool,
    /// The list opens under the field's ancestor this many levels up (0: the field).
    under: usize,
    /// Its nodes' names start with this (`{name}-variables`, `{name}-variable-#width`,
    /// `{name}-new-variable`); the field's name if empty.
    name: String,
    suggest: Suggest,
    /// The list's node, while it is open.
    node: Option<Entity>,
}

/// The `#` list's state.
#[derive(Debug, Clone, Default, PartialEq)]
struct Suggest {
    /// The name being typed after a `#`: its byte range in the text (the caret is in it).
    token: Option<Range<usize>>,
    /// The variables listed (indices into [`VariableList::variables`]); New variable… follows.
    rows: Vec<usize>,
    /// The highlighted row (`rows.len()` is New variable…).
    hi: usize,
    /// Esc closed the list for this text (it opens again when the text changes).
    closed_for: Option<String>,
    /// A name was just put in: the list stays closed on the text that makes.
    closing: bool,
}

impl VariableList {
    pub fn new(variables: Vec<ListedVariable>) -> Self {
        Self {
            variables,
            new_variable: false,
            under: 0,
            name: String::new(),
            suggest: Suggest::default(),
            node: None,
        }
    }

    /// Offers New variable… after the variables.
    pub fn new_variable(mut self, on: bool) -> Self {
        self.new_variable = on;
        self
    }

    /// Opens the list under the field's ancestor `levels` up (1: its parent).
    pub fn under(mut self, levels: usize) -> Self {
        self.under = levels;
        self
    }

    /// Names the list's nodes `{name}-variables`, `{name}-variable-#width`, `{name}-new-variable`
    /// (the field's name by default).
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// The list is showing (the keys that move in it, pick from it or close it go to it).
    pub fn is_open(&self) -> bool {
        let s = &self.suggest;
        s.token.is_some()
            && s.closed_for.is_none()
            && !s.closing
            && (!s.rows.is_empty() || self.new_variable)
    }

    /// The rows shown: the variables', then New variable… if offered.
    fn row_count(&self) -> usize {
        self.suggest.rows.len() + usize::from(self.new_variable)
    }
}

/// The most variables a list shows, its minimum width and its rows' height.
const LIST_MAX: usize = 8;
const LIST_WIDTH: f32 = 200.0;
const LIST_ROW: f32 = 24.0;

/// Gives a key pressed in `field` to its `#` list while it is open: Up/Down move the highlight,
/// Enter or Tab put the highlighted name in, Esc closes the list. True if the list took the key
/// (the owner then leaves it alone and stops its propagation). For the owner's key handler,
/// before its own keys.
pub fn list_key(
    code: KeyCode,
    field: Entity,
    list: &mut VariableList,
    text: &mut EditableText,
    commands: &mut Commands,
) -> bool {
    if !list.is_open() {
        return false;
    }
    let n = list.row_count();
    let s = &mut list.suggest;
    match code {
        KeyCode::ArrowDown | KeyCode::ArrowUp => {
            s.hi = if code == KeyCode::ArrowDown {
                (s.hi + 1) % n
            } else {
                (s.hi + n - 1) % n
            };
            // The text input moves the caret to the line's start or end on these: not here.
            commands.queue(move |world: &mut World| {
                if let Some(mut t) = world.get_mut::<EditableText>(field) {
                    t.pending_edits
                        .retain(|e| !matches!(e, TextEdit::Up(_) | TextEdit::Down(_)));
                }
            });
            true
        }
        KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Tab => {
            pick(list, text, field, commands);
            true
        }
        KeyCode::Escape => {
            s.closed_for = Some(text.value().to_string());
            true
        }
        _ => false,
    }
}

/// Puts the highlighted variable in place of the `#name` being typed, or asks the owner for a
/// new one.
fn pick(list: &mut VariableList, text: &mut EditableText, field: Entity, commands: &mut Commands) {
    let s = &mut list.suggest;
    match s.rows.get(s.hi) {
        Some(&i) => {
            if let Some(v) = list.variables.get(i) {
                replace_name(text, &v.name);
            }
        }
        None if list.new_variable => {
            let value = text.value().to_string();
            let name = s
                .token
                .clone()
                .and_then(|r| value.get(r))
                .unwrap_or_default()
                .to_string();
            commands.trigger(NewVariable {
                entity: field,
                name,
            });
        }
        None => {}
    }
    s.token = None;
    s.closing = true;
}

/// Puts `#{name}` in `field`, in place of the `#name` being typed, and closes its `#` list: for
/// the owner, once it has made the variable New variable… asked for (see [`NewVariable`]).
pub fn complete_variable(world: &mut World, field: Entity, name: &str) {
    if let Some(mut text) = world.get_mut::<EditableText>(field) {
        replace_name(&mut text, name);
    }
    if let Some(mut list) = world.get_mut::<VariableList>(field) {
        list.suggest.token = None;
        list.suggest.closing = true;
    }
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
fn matching(vars: &[ListedVariable], typed: &str) -> Vec<usize> {
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

/// Follows the caret: opens a field's `#` list while a name is typed after a `#` in it, filtered
/// by that name.
fn update_lists(
    mut q_field: Query<(Entity, &EditableText, &mut VariableList)>,
    focus: Res<InputFocus>,
) {
    for (field, text, mut list) in &mut q_field {
        let value = text.value().to_string();
        let mut s = list.suggest.clone();
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
            Some(r) => matching(&list.variables, &value[r.clone()]),
            None => Vec::new(),
        };
        if rows != s.rows {
            s.hi = 0;
        }
        s.rows = rows;
        s.token = token;
        s.hi = s.hi.min(list.row_count().saturating_sub(1));
        if list.suggest != s {
            list.suggest = s;
        }
    }
}

/// A `#` list's node; points at its field.
#[derive(Component, Debug, Clone, Copy)]
pub struct VariableListNode(pub Entity);

/// A row of a `#` list: its field and index (after the variables', New variable…).
#[derive(Component, Debug, Clone, Copy)]
struct VariableRow {
    field: Entity,
    index: usize,
}

/// What a `#` list shows, so its rows are rebuilt only when that changes.
#[derive(Component, Debug, Clone, PartialEq)]
struct Shown(Vec<ListedVariable>, bool);

/// Opens, fills and closes the `#` lists of the fields whose lists changed.
fn show_lists(
    mut q_field: Query<(Entity, &mut VariableList, Option<&Name>), Changed<VariableList>>,
    q_parent: Query<&ChildOf>,
    q_children: Query<&Children>,
    q_shown: Query<&Shown>,
    mut q_row: Query<(&VariableRow, &mut BackgroundColor)>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    for (field, mut list, name) in &mut q_field {
        if !list.is_open() {
            if let Some(node) = list.bypass_change_detection().node.take() {
                commands.entity(node).try_despawn();
            }
            continue;
        }
        let label = match (&list.name, name) {
            (n, _) if !n.is_empty() => n.clone(),
            (_, Some(n)) => n.as_str().to_string(),
            _ => "field".to_string(),
        };
        let node = match list.node {
            Some(n) => n,
            None => {
                let mut under = field;
                for _ in 0..list.under {
                    under = q_parent.get(under).map_or(under, ChildOf::parent);
                }
                let n = commands
                    .spawn((list_node(&theme, &label, field), ChildOf(under)))
                    .id();
                list.bypass_change_detection().node = Some(n);
                n
            }
        };
        let want: Vec<ListedVariable> = list
            .suggest
            .rows
            .iter()
            .map(|&i| list.variables[i].clone())
            .collect();
        let hi = list.suggest.hi;
        let shown = Shown(want.clone(), list.new_variable);
        if q_shown.get(node).ok() != Some(&shown) {
            commands.entity(node).despawn_children().insert(shown);
            for (i, v) in want.iter().enumerate() {
                commands.spawn((
                    row(
                        &theme,
                        format!("{label}-variable-#{}", v.name),
                        VariableRow { field, index: i },
                        i == hi,
                        format!("#{}", v.name),
                        v.value.clone(),
                        "variables",
                    ),
                    ChildOf(node),
                ));
            }
            if list.new_variable {
                let i = want.len();
                commands.spawn((
                    row(
                        &theme,
                        format!("{label}-new-variable"),
                        VariableRow { field, index: i },
                        i == hi,
                        "New variable…".into(),
                        String::new(),
                        "plus",
                    ),
                    ChildOf(node),
                ));
            }
            continue;
        }
        // Same rows: only the highlight moved.
        for r in q_children.get(node).into_iter().flatten() {
            if let Ok((row, mut bg)) = q_row.get_mut(*r) {
                let want = if row.index == hi {
                    theme.menu_hover
                } else {
                    Color::NONE
                };
                bg.set_if_neq(BackgroundColor(want));
            }
        }
    }
}

/// A `#` list's node: a popover under its parent (above it when there's no room below), over
/// the dialogs and not clipped by their scroll areas.
fn list_node(theme: &Theme, label: &str, field: Entity) -> impl Bundle {
    let placement = |side| PopoverPlacement {
        side,
        align: PopoverAlign::Start,
        gap: 2.0,
    };
    (
        Name::new(format!("{label}-variables")),
        VariableListNode(field),
        OverrideClip,
        Node {
            position_type: PositionType::Absolute,
            // Laid out up and left from the parent's bottom-right corner, so it doesn't make a
            // scroll area it sits in scrollable; the popover then moves it to its place.
            right: Val::Px(0.0),
            bottom: Val::Px(0.0),
            min_width: Val::Px(LIST_WIDTH),
            flex_direction: FlexDirection::Column,
            padding: UiRect::vertical(Val::Px(4.0)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(2.0)),
            ..default()
        },
        Popover {
            positions: vec![placement(PopoverSide::Bottom), placement(PopoverSide::Top)],
            window_margin: 4.0,
        },
        BackgroundColor(theme.popover),
        BorderColor::all(theme.border),
        BoxShadow::new(
            theme.shadow,
            Val::Px(0.0),
            Val::Px(2.0),
            Val::Px(0.0),
            Val::Px(6.0),
        ),
        GlobalZIndex(z::MENU),
    )
}

fn row(
    theme: &Theme,
    name: String,
    row: VariableRow,
    hi: bool,
    label: String,
    value: String,
    icon_name: &'static str,
) -> impl Bundle {
    let font = theme.font(13.0, FontWeight::NORMAL);
    (
        Name::new(name),
        row,
        Node {
            height: Val::Px(LIST_ROW),
            padding: UiRect::horizontal(Val::Px(8.0)),
            column_gap: Val::Px(6.0),
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(if hi { theme.menu_hover } else { Color::NONE }),
        children![
            (icon(icon_name, 14.0, theme.foreground), Pickable::IGNORE),
            (
                Text::new(label),
                font.clone(),
                TextColor(theme.foreground),
                TextLayout::no_wrap(),
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
                Pickable::IGNORE,
            ),
            (
                Text::new(value),
                font,
                TextColor(theme.muted_foreground),
                TextLayout::no_wrap(),
                Pickable::IGNORE,
            ),
        ],
    )
}

/// A press in a `#` list leaves the keyboard with the text field (the press would otherwise
/// look for something focusable up to the window, and the list would close before the click).
fn keep_focus_in_list(mut ev: On<AcquireFocus>, q_list: Query<(), With<VariableListNode>>) {
    if q_list.contains(ev.focused_entity) {
        ev.propagate(false);
    }
}

fn on_row_click(
    mut click: On<PointerClick>,
    q_row: Query<&VariableRow>,
    mut q_field: Query<(&mut VariableList, &mut EditableText)>,
    mut commands: Commands,
) {
    let Ok(&VariableRow { field, index }) = q_row.get(click.entity) else {
        return;
    };
    click.propagate(false);
    let Ok((mut list, mut text)) = q_field.get_mut(field) else {
        return;
    };
    list.suggest.hi = index;
    pick(&mut list, &mut text, field, &mut commands);
    // The field keeps the keyboard.
    commands.queue(move |world: &mut World| {
        if let Some(mut focus) = world.get_resource_mut::<InputFocus>() {
            focus.set(field, FocusCause::Pressed);
        }
    });
}

fn on_row_over(
    over: On<PointerOver>,
    q_row: Query<&VariableRow>,
    mut q_list: Query<&mut VariableList>,
) {
    let Ok(&VariableRow { field, index }) = q_row.get(over.entity) else {
        return;
    };
    if let Ok(mut list) = q_list.get_mut(field)
        && list.suggest.hi != index
    {
        list.suggest.hi = index;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn var(name: &str) -> ListedVariable {
        ListedVariable {
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

    /// A number field's list opens under its line (the field's parent), with the variables then
    /// New variable…, and goes when it closes.
    #[test]
    fn an_open_list_hangs_under_its_ancestor_and_goes_when_it_closes() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<Theme>()
            .add_systems(Update, show_lists);
        let line = app.world_mut().spawn(Node::default()).id();
        let mut list = VariableList::new(vec![var("foot_height"), var("width")])
            .new_variable(true)
            .under(1)
            .named("extrude-depth");
        list.suggest.token = Some(1..1);
        list.suggest.rows = vec![0, 1];
        let field = app
            .world_mut()
            .spawn((Name::new("depth-field"), list, ChildOf(line)))
            .id();
        app.update();
        let node = app
            .world()
            .get::<VariableList>(field)
            .unwrap()
            .node
            .unwrap();
        assert_eq!(app.world().get::<ChildOf>(node).unwrap().parent(), line);
        let rows: Vec<String> = app
            .world()
            .get::<Children>(node)
            .unwrap()
            .iter()
            .map(|r| app.world().get::<Name>(r).unwrap().to_string())
            .collect();
        assert_eq!(
            rows,
            [
                "extrude-depth-variable-#foot_height",
                "extrude-depth-variable-#width",
                "extrude-depth-new-variable"
            ]
        );
        app.world_mut()
            .get_mut::<VariableList>(field)
            .unwrap()
            .suggest
            .token = None;
        app.update();
        assert!(app.world().get_entity(node).is_err());
        assert!(
            app.world()
                .get::<VariableList>(field)
                .unwrap()
                .node
                .is_none()
        );
    }

    #[test]
    fn without_new_variable_a_list_with_no_match_stays_closed() {
        let mut list = VariableList::new(vec![var("width")]);
        list.suggest.token = Some(1..2);
        assert!(!list.is_open());
        list.suggest.rows = vec![0];
        assert!(list.is_open());
        list.suggest.rows.clear();
        assert!(list.new_variable(true).is_open());
    }
}
