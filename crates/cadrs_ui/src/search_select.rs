//! A dropdown with a search box: one of many names, found by typing (gpui-component's
//! searchable `Select`). It is the Part Studio's material dropdown (the Assign material dialog,
//! PS10), lifted out so that everything that picks a material, or anything else from a long
//! list, is the same control.
//!
//! - Closed: a box with the chosen name (or a placeholder) and a ▾.
//! - Open: under it, over what follows, a search box (focused, empty) and the list of the names
//!   that contain what was typed, any case. The list shows eight rows and scrolls, by the wheel
//!   and its scrollbar; it opens scrolled to the chosen name. A click on a name, or Enter for
//!   the first one, chooses it and closes the list; the box again, Esc, or a press anywhere
//!   else closes it.
//! - [`SearchSelect::adds`]: each choice is something to add (to a list the owner keeps), so the
//!   box goes back to its placeholder instead of showing the choice.
//! - [`SearchSelect::typed`]: what was typed is offered too, when no name is exactly that (a
//!   name the list doesn't have yet).
//!
//! While a list is open, Enter and Esc are its own: a dialog the control is in doesn't take them
//! as its OK and its close ([`crate::Dialog`] doesn't; anything else that reads the keys asks
//! [`any_open`] first, or closes the list itself with [`SearchSelectState::close`]). So the list
//! stays open, for everything that looks, until the frame's `Update` is over.
//!
//! Choosing triggers [`SearchSelectPick`] on the control. The owner may change
//! [`SearchSelectState`] (`options`, `chosen`, `open`) at any time; the control follows.
//!
//! Names, for a control `<name>`: the control `<name>-select`, its box `<name>-choose` with
//! `<name>-choose-label`, the open part `<name>-popup` with `<name>-search` and `<name>-list`,
//! and a row a name, `<name>-item-<slug>` (the name in lower case, anything but letters and
//! digits as one `-`: "Nylon 6/6" is `<name>-item-nylon-6-6`); the typed one is
//! `<name>-item-typed`.

use std::borrow::Cow;

use bevy::ecs::spawn::SpawnWith;
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::text::{EditableText, FontWeight};
use bevy::ui::OverrideClip;
use bevy::ui_widgets::Activate;

use crate::input::{TextCancel, TextInput, TextInputField, TextSubmit};
use crate::list::ListItem;
use crate::shortcut_key::ShortcutKey;
use crate::style::{StateColors, Visuals};
use crate::theme::Theme;

/// A row's height.
pub const ROW: f32 = 22.0;
/// How many rows show before the list scrolls.
const ROWS: f32 = 8.0;

pub struct SearchSelectPlugin;

impl Plugin for SearchSelectPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_choose)
            .add_observer(on_item)
            .add_observer(on_submit)
            .add_observer(on_cancel)
            .init_resource::<Keyed>()
            .add_systems(Update, (read_search, sync).chain())
            .add_systems(Update, (close_on_escape, close_on_press_outside))
            .add_systems(PostUpdate, settle_keys);
    }
}

/// A name was chosen. Targets the control.
#[derive(EntityEvent, Clone, Debug)]
pub struct SearchSelectPick {
    pub entity: Entity,
    pub value: String,
}

/// The control's state.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct SearchSelectState {
    /// The names to choose from, in the order they are listed.
    pub options: Vec<String>,
    pub chosen: Option<String>,
    pub open: bool,
    /// What the search box holds.
    pub query: String,
    name: String,
    placeholder: String,
    none_match: String,
    none_yet: String,
    typed: Option<String>,
    adds: bool,
}

impl SearchSelectState {
    /// The names that contain what was typed (any case), in order.
    pub fn hits(&self) -> Vec<&str> {
        let q = self.query.trim().to_lowercase();
        self.options
            .iter()
            .filter(|o| q.is_empty() || o.to_lowercase().contains(&q))
            .map(String::as_str)
            .collect()
    }

    /// What was typed, when it is offered as a choice of its own: something was typed, the
    /// control takes typed names, and no name is exactly that.
    pub fn typed_choice(&self) -> Option<&str> {
        let typed = self.query.trim();
        (self.typed.is_some()
            && !typed.is_empty()
            && !self.options.iter().any(|o| o.eq_ignore_ascii_case(typed)))
        .then_some(typed)
    }

    /// What Enter chooses: the first name listed, else what was typed if that is offered.
    pub fn first(&self) -> Option<String> {
        self.hits()
            .first()
            .map(|hit| (*hit).to_owned())
            .or_else(|| self.typed_choice().map(str::to_owned))
    }

    /// Closes the list (Esc, handled by whoever reads the keys).
    pub fn close(&mut self) {
        self.open = false;
        self.query.clear();
    }

    fn label(&self) -> &str {
        self.chosen.as_deref().unwrap_or(&self.placeholder)
    }
}

/// Some control's list is open: Enter and Esc are its.
pub fn any_open(selects: &Query<&SearchSelectState>) -> bool {
    selects.iter().any(|s| s.open)
}

/// What the keys asked of open lists this frame: a name to choose, or none to close. Done when
/// `Update` is over, so that everything reading the same key there still sees the list open
/// and leaves the key to it.
#[derive(Resource, Default)]
struct Keyed(Vec<(Entity, Option<String>)>);

/// Esc closes an open list, wherever the focus is.
fn close_on_escape(
    mut keys: MessageReader<KeyboardInput>,
    selects: Query<(Entity, &SearchSelectState)>,
    mut keyed: ResMut<Keyed>,
) {
    for key in keys.read() {
        if key.state == ButtonState::Pressed && key.shortcut_code() == KeyCode::Escape {
            keyed.0.extend(
                selects
                    .iter()
                    .filter(|(_, s)| s.open)
                    .map(|(select, _)| (select, None)),
            );
        }
    }
}

/// A press anywhere but on an open list (its box, its search box, its rows, its scrollbar)
/// closes it: it lies over what follows it, which could not be reached otherwise.
fn close_on_press_outside(
    mut presses: MessageReader<PointerPress>,
    selects: Query<(Entity, &SearchSelectState)>,
    parents: Query<&ChildOf>,
    mut keyed: ResMut<Keyed>,
) {
    let pressed: Vec<Entity> = presses.read().map(|press| press.entity).collect();
    if pressed.is_empty() {
        return;
    }
    for (select, state) in &selects {
        let inside = |entity: Entity| {
            entity == select || parents.iter_ancestors(entity).any(|a| a == select)
        };
        if state.open && !pressed.iter().copied().any(inside) {
            keyed.0.push((select, None));
        }
    }
}

fn settle_keys(world: &mut World) {
    let keyed = std::mem::take(&mut world.resource_mut::<Keyed>().0);
    for (select, value) in keyed {
        match value {
            Some(value) => choose(world, select, value),
            None => set_open(world, select, false),
        }
    }
}

/// A part of a control.
#[derive(Component, Debug, Clone, Copy)]
struct PartOf {
    select: Entity,
    part: Part,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Choose,
    Label,
    Popup,
    List,
}

/// A row of the list: the control, and the name it chooses.
#[derive(Component, Debug, Clone)]
struct Item {
    select: Entity,
    value: String,
}

/// What the list was last built from, and the search it was last scrolled for (choosing a name
/// keeps the scroll).
#[derive(Component, Default)]
struct Built {
    list: Option<(Vec<String>, String, Option<String>)>,
    scrolled_for: Option<String>,
    open: bool,
}

/// `name` as part of an element's name: lower case, anything but letters and digits as one `-`.
pub fn slug(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Builder for a dropdown with a search box.
pub struct SearchSelect {
    name: Cow<'static, str>,
    options: Vec<String>,
    chosen: Option<String>,
    placeholder: String,
    search_placeholder: String,
    none_match: String,
    none_yet: String,
    typed: Option<String>,
    adds: bool,
    open: bool,
}

impl SearchSelect {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            options: Vec::new(),
            chosen: None,
            placeholder: String::new(),
            search_placeholder: String::new(),
            none_match: String::new(),
            none_yet: String::new(),
            typed: None,
            adds: false,
            open: false,
        }
    }

    /// The names to choose from, in the order they are listed.
    pub fn options(mut self, options: impl IntoIterator<Item = String>) -> Self {
        self.options = options.into_iter().collect();
        self
    }

    pub fn chosen(mut self, chosen: Option<String>) -> Self {
        self.chosen = chosen;
        self
    }

    /// What the box says while nothing is chosen ("Select a material").
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = text.into();
        self
    }

    /// What the search box says while it is empty ("Search materials").
    pub fn search_placeholder(mut self, text: impl Into<String>) -> Self {
        self.search_placeholder = text.into();
        self
    }

    /// What the list says when no name contains what was typed, and when there are no names at
    /// all.
    pub fn empty_text(
        mut self,
        none_match: impl Into<String>,
        none_yet: impl Into<String>,
    ) -> Self {
        self.none_match = none_match.into();
        self.none_yet = none_yet.into();
        self
    }

    /// Offers what was typed as a choice of its own when no name is exactly that; `label` goes
    /// before it in its row ("Use" gives: Use “PA 12”).
    pub fn typed(mut self, label: impl Into<String>) -> Self {
        self.typed = Some(label.into());
        self
    }

    /// Each choice is something to add: the box goes back to its placeholder.
    pub fn adds(mut self) -> Self {
        self.adds = true;
        self
    }

    /// Open from the start.
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let t = theme.clone();
        let name = self.name.to_string();
        let state = SearchSelectState {
            options: self.options,
            chosen: self.chosen,
            open: self.open,
            query: String::new(),
            name: name.clone(),
            placeholder: self.placeholder,
            none_match: self.none_match,
            none_yet: self.none_yet,
            typed: self.typed,
            adds: self.adds,
        };
        let search_placeholder = self.search_placeholder;
        let built = state.clone();
        (
            Name::new(format!("{name}-select")),
            state,
            Built::default(),
            Node {
                flex_direction: FlexDirection::Column,
                ..default()
            },
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                let select = p.target_entity();
                let s = &built;
                // Closed: the chosen name and a ▾.
                p.spawn((
                    PartOf {
                        select,
                        part: Part::Choose,
                    },
                    ListItem::new(format!("{name}-choose")).build(&t),
                ))
                .insert(Node {
                    height: Val::Px(26.0),
                    padding: UiRect::horizontal(Val::Px(6.0)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(2.0)),
                    ..default()
                })
                .insert(Visuals {
                    background: StateColors::new(
                        t.background,
                        t.list_hover,
                        t.list_active,
                        t.background,
                    ),
                    border: StateColors::all(Color::srgb_u8(0xc8, 0xc8, 0xc8)),
                    foreground: StateColors::all(t.foreground),
                    focus_ring: t.focus_ring,
                })
                .with_children(|r| {
                    r.spawn((
                        Name::new(format!("{name}-choose-label")),
                        PartOf {
                            select,
                            part: Part::Label,
                        },
                        t.text(s.label().to_owned(), 12.5, FontWeight::NORMAL, t.foreground),
                        Pickable::IGNORE,
                    ));
                    r.spawn((
                        crate::icon::icon("caret-down", 12.0, t.foreground),
                        Pickable::IGNORE,
                    ));
                });
                // Open: the search box and the list, under the box and over what follows.
                p.spawn((
                    Name::new(format!("{name}-popup")),
                    PartOf {
                        select,
                        part: Part::Popup,
                    },
                    Node {
                        position_type: PositionType::Absolute,
                        top: Val::Px(28.0),
                        left: Val::ZERO,
                        right: Val::ZERO,
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(4.0),
                        padding: UiRect::all(Val::Px(4.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(3.0)),
                        display: if s.open { Display::Flex } else { Display::None },
                        ..default()
                    },
                    BackgroundColor(t.popover),
                    BorderColor::all(t.border),
                    BoxShadow::new(t.shadow, Val::ZERO, Val::Px(2.0), Val::ZERO, Val::Px(8.0)),
                    // Over the dialog it is in, but under a menu opened while it is open and
                    // that menu's dismiss layer.
                    GlobalZIndex(crate::z::MENU - 2),
                    // Over a scrolling body, not clipped by it, as a menu.
                    OverrideClip,
                ))
                .with_children(|pop| {
                    pop.spawn(
                        TextInput::new(format!("{name}-search"))
                            .placeholder(search_placeholder)
                            .width(Val::Percent(100.0))
                            .height(24.0)
                            .build(&t),
                    );
                    // The list scrolls with the wheel and its scrollbar (it holds more names
                    // than its rows show).
                    pop.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        ..default()
                    })
                    .with_children(|frame| {
                        let list = frame
                            .spawn((
                                Name::new(format!("{name}-list")),
                                PartOf {
                                    select,
                                    part: Part::List,
                                },
                                crate::WheelScroll,
                                crate::scrollbar::ScrollGutter(12.0),
                                Node {
                                    flex_direction: FlexDirection::Column,
                                    max_height: Val::Px(ROWS * ROW),
                                    overflow: Overflow::scroll_y(),
                                    ..default()
                                },
                                scroll_to_chosen(s),
                            ))
                            .with_children(|l| spawn_list(l, &t, select, s))
                            .id();
                        frame.spawn(crate::vertical_scrollbar(
                            &t,
                            &format!("{name}-list-scrollbar"),
                            list,
                        ));
                    });
                });
            })),
        )
    }
}

/// The list scrolled so the chosen name shows (two rows above it).
fn scroll_to_chosen(s: &SearchSelectState) -> ScrollPosition {
    let i = s
        .hits()
        .iter()
        .position(|hit| s.chosen.as_deref() == Some(*hit))
        .unwrap_or(0);
    ScrollPosition(Vec2::new(0.0, (i as f32 - 2.0).max(0.0) * ROW))
}

fn spawn_list(p: &mut ChildSpawner, t: &Theme, select: Entity, s: &SearchSelectState) {
    let hits = s.hits();
    let typed = s.typed_choice();
    if hits.is_empty() && typed.is_none() {
        let empty = if s.options.is_empty() && s.query.trim().is_empty() {
            &s.none_yet
        } else {
            &s.none_match
        };
        p.spawn(t.text(empty.clone(), 11.5, FontWeight::NORMAL, t.muted_foreground));
    }
    let name = &s.name;
    let mut row = |item_name: String, label: String, value: &str| {
        let selected = s.chosen.as_deref() == Some(value);
        p.spawn((
            ListItem::new(item_name)
                .label(label)
                .height(ROW)
                .padding_left(8.0)
                .selected(selected)
                .build(t),
            Item {
                select,
                value: value.to_owned(),
            },
        ))
        .entry::<Node>()
        .and_modify(|mut n| n.flex_shrink = 0.0);
    };
    for hit in hits {
        row(format!("{name}-item-{}", slug(hit)), hit.to_owned(), hit);
    }
    // What was typed, when no name is exactly that.
    if let (Some(typed), Some(label)) = (typed, &s.typed) {
        row(
            format!("{name}-item-typed"),
            format!("{label} “{typed}”"),
            typed,
        );
    }
}

/// The search box of the control called `name`.
fn search_field(world: &mut World, name: &str) -> Option<Entity> {
    let field = format!("{name}-search-field");
    let mut q = world.query_filtered::<(Entity, &Name), With<TextInputField>>();
    q.iter(world)
        .find(|(_, n)| n.as_str() == field)
        .map(|(e, _)| e)
}

/// Opens the control's list (clearing the search and focusing it), or closes it.
fn set_open(world: &mut World, select: Entity, open: bool) {
    let Some(mut state) = world.get_mut::<SearchSelectState>(select) else {
        return;
    };
    if state.open == open {
        return;
    }
    state.open = open;
    state.query.clear();
    let name = state.name.clone();
    let Some(field) = search_field(world, &name) else {
        return;
    };
    if let Some(mut text) = world.get_mut::<EditableText>(field) {
        text.editor_mut().set_text("");
    }
    if open && let Some(mut focus) = world.get_resource_mut::<InputFocus>() {
        focus.set(field, FocusCause::Pressed);
    }
}

/// Chooses `value`: kept (unless each choice is something to add), the list closed, and the
/// owner told.
fn choose(world: &mut World, select: Entity, value: String) {
    let Some(mut state) = world.get_mut::<SearchSelectState>(select) else {
        return;
    };
    if !state.adds {
        state.chosen = Some(value.clone());
    }
    set_open(world, select, false);
    world.trigger(SearchSelectPick {
        entity: select,
        value,
    });
}

/// The box opens the list, or closes it.
fn on_choose(a: On<Activate>, parts: Query<&PartOf>, mut commands: Commands) {
    let Ok(PartOf {
        select,
        part: Part::Choose,
    }) = parts.get(a.entity).copied()
    else {
        return;
    };
    commands.queue(move |world: &mut World| {
        let open = world
            .get::<SearchSelectState>(select)
            .is_some_and(|s| s.open);
        set_open(world, select, !open);
    });
}

fn on_item(a: On<Activate>, items: Query<&Item>, mut commands: Commands) {
    if let Ok(item) = items.get(a.entity) {
        let (select, value) = (item.select, item.value.clone());
        commands.queue(move |world: &mut World| choose(world, select, value));
    }
}

/// The control whose search box `field` is, if it is one's and that one is open.
fn select_of_field(
    field: Entity,
    names: &Query<&Name, With<TextInputField>>,
    selects: &Query<(Entity, &SearchSelectState)>,
) -> Option<Entity> {
    let name = names.get(field).ok()?;
    selects
        .iter()
        .find(|(_, s)| s.open && name.as_str() == format!("{}-search-field", s.name))
        .map(|(e, _)| e)
}

/// Enter in the search box chooses the first name listed. It goes no further: a dialog the
/// control is in doesn't take it as its own OK.
fn on_submit(
    mut ev: On<TextSubmit>,
    names: Query<&Name, With<TextInputField>>,
    selects: Query<(Entity, &SearchSelectState)>,
    mut keyed: ResMut<Keyed>,
) {
    let Some(select) = select_of_field(ev.entity, &names, &selects) else {
        return;
    };
    ev.propagate(false);
    if let Some(first) = selects.get(select).ok().and_then(|(_, s)| s.first()) {
        keyed.0.push((select, Some(first)));
    }
}

/// Esc in the search box closes the list (`close_on_escape`), and only that: it goes no further.
fn on_cancel(
    mut ev: On<TextCancel>,
    names: Query<&Name, With<TextInputField>>,
    selects: Query<(Entity, &SearchSelectState)>,
) {
    if select_of_field(ev.entity, &names, &selects).is_some() {
        ev.propagate(false);
    }
}

/// The search box filters as you type.
#[allow(clippy::type_complexity)]
fn read_search(
    fields: Query<(&Name, &EditableText), (With<TextInputField>, Changed<EditableText>)>,
    mut selects: Query<&mut SearchSelectState>,
) {
    for (name, text) in &fields {
        for mut state in &mut selects {
            if state.open && name.as_str() == format!("{}-search-field", state.name) {
                let typed = text.value().to_string();
                if state.query != typed {
                    state.query = typed;
                }
            }
        }
    }
}

/// The control shows its state: the box's label, the list open or not, and the names listed.
#[allow(clippy::type_complexity)]
fn sync(
    mut selects: Query<(Entity, &SearchSelectState, &mut Built), Changed<SearchSelectState>>,
    parts: Query<(Entity, &PartOf)>,
    mut nodes: Query<&mut Node>,
    mut texts: Query<&mut Text>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    for (select, state, mut built) in &mut selects {
        let opened = state.open && !built.open;
        built.open = state.open;
        let listed = (
            state.options.clone(),
            state.query.clone(),
            state.chosen.clone(),
        );
        let relist = built.list.as_ref() != Some(&listed);
        for (entity, part) in parts.iter().filter(|(_, p)| p.select == select) {
            match part.part {
                Part::Choose => {}
                Part::Label => {
                    if let Ok(mut text) = texts.get_mut(entity)
                        && text.0 != state.label()
                    {
                        text.0 = state.label().to_owned();
                    }
                }
                Part::Popup => {
                    let want = if state.open {
                        Display::Flex
                    } else {
                        Display::None
                    };
                    if let Ok(mut node) = nodes.get_mut(entity)
                        && node.display != want
                    {
                        node.display = want;
                    }
                }
                Part::List => {
                    // Scrolled to the chosen name when it opens and when the search changes;
                    // choosing a name keeps the scroll.
                    let rescroll =
                        opened || built.scrolled_for.as_deref() != Some(state.query.as_str());
                    if relist {
                        let (t, s) = (theme.clone(), state.clone());
                        commands.entity(entity).despawn_children();
                        commands.queue(move |world: &mut World| {
                            if let Ok(mut list) = world.get_entity_mut(entity) {
                                list.with_children(|l| spawn_list(l, &t, select, &s));
                            }
                        });
                    }
                    if rescroll {
                        commands.entity(entity).try_insert(scroll_to_chosen(state));
                    }
                }
            }
        }
        if relist {
            built.list = Some(listed);
        }
        built.scrolled_for = Some(state.query.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn materials() -> Vec<String> {
        [
            "ABS",
            "Aluminum - 6061",
            "Nylon 6/6",
            "PETG",
            "PLA",
            "Steel - A36",
        ]
        .map(String::from)
        .to_vec()
    }

    fn world_with(select: SearchSelect) -> (World, Entity) {
        let mut world = World::new();
        world.insert_resource(Theme::default());
        world.init_resource::<InputFocus>();
        world.add_observer(on_choose);
        world.add_observer(on_item);
        let entity = world.spawn(select.build(&Theme::default())).id();
        world.flush();
        (world, entity)
    }

    fn named(world: &mut World, name: &str) -> Option<Entity> {
        let mut q = world.query::<(Entity, &Name)>();
        q.iter(world)
            .find(|(_, n)| n.as_str() == name)
            .map(|(e, _)| e)
    }

    fn run_sync(world: &mut World) {
        let sync = world.register_system(sync);
        world.run_system(sync).unwrap();
        world.flush();
    }

    #[test]
    fn typing_narrows_the_names() {
        let (mut world, select) = world_with(SearchSelect::new("material").options(materials()));
        let mut state = world.get_mut::<SearchSelectState>(select).unwrap();
        assert_eq!(state.hits().len(), 6);
        state.query = " pet ".into();
        assert_eq!(state.hits(), ["PETG"]);
        state.query = "AL".into();
        assert_eq!(state.hits(), ["Aluminum - 6061"]);
        state.query = "titanium".into();
        assert!(state.hits().is_empty());
        // Nothing typed is offered unless the control takes typed names.
        assert_eq!(state.typed_choice(), None);
        assert_eq!(state.first(), None);
    }

    #[test]
    fn rows_are_named_by_what_they_choose() {
        assert_eq!(slug("Nylon 6/6"), "nylon-6-6");
        assert_eq!(slug("Aluminum - 6061"), "aluminum-6061");
        let (mut world, _) = world_with(SearchSelect::new("material").options(materials()));
        for name in [
            "material-select",
            "material-choose",
            "material-choose-label",
            "material-popup",
            "material-search",
            "material-list",
            "material-item-nylon-6-6",
            "material-item-petg",
        ] {
            assert!(named(&mut world, name).is_some(), "no {name}");
        }
        // The list scrolls: by the wheel, with a scrollbar beside it.
        let list = named(&mut world, "material-list").unwrap();
        assert!(world.get::<crate::WheelScroll>(list).is_some());
        assert!(named(&mut world, "material-list-scrollbar").is_some());
    }

    #[test]
    fn a_click_chooses_and_closes() {
        #[derive(Resource, Default)]
        struct Picked(Vec<String>);
        let (mut world, select) = world_with(
            SearchSelect::new("material")
                .options(materials())
                .placeholder("Select a material"),
        );
        world.init_resource::<Picked>();
        world.add_observer(|ev: On<SearchSelectPick>, mut picked: ResMut<Picked>| {
            picked.0.push(ev.value.clone());
        });
        let label = named(&mut world, "material-choose-label").unwrap();
        assert_eq!(world.get::<Text>(label).unwrap().0, "Select a material");
        // The box opens the list.
        let choose_box = named(&mut world, "material-choose").unwrap();
        world.trigger(Activate { entity: choose_box });
        world.flush();
        assert!(world.get::<SearchSelectState>(select).unwrap().open);
        run_sync(&mut world);
        let popup = named(&mut world, "material-popup").unwrap();
        assert_eq!(world.get::<Node>(popup).unwrap().display, Display::Flex);
        // A row chooses its name, closes the list and says so.
        let petg = named(&mut world, "material-item-petg").unwrap();
        world.trigger(Activate { entity: petg });
        world.flush();
        run_sync(&mut world);
        let state = world.get::<SearchSelectState>(select).unwrap();
        assert_eq!(state.chosen.as_deref(), Some("PETG"));
        assert!(!state.open);
        assert_eq!(world.resource::<Picked>().0, ["PETG"]);
        assert_eq!(world.get::<Text>(label).unwrap().0, "PETG");
        assert_eq!(world.get::<Node>(popup).unwrap().display, Display::None);
    }

    /// A control that adds keeps its placeholder, and one that takes typed names offers what
    /// was typed when no name is exactly that.
    #[test]
    fn adding_and_typed_names() {
        let (mut world, select) = world_with(
            SearchSelect::new("add")
                .options(materials())
                .placeholder("Add a material…")
                .typed("Use")
                .adds(),
        );
        let mut state = world.get_mut::<SearchSelectState>(select).unwrap();
        state.open = true;
        state.query = "PA 12".into();
        assert_eq!(state.typed_choice(), Some("PA 12"));
        assert_eq!(state.first().as_deref(), Some("PA 12"));
        // A name that is there, in any case, isn't offered again as typed.
        state.query = "petg".into();
        assert_eq!(state.typed_choice(), None);
        assert_eq!(state.first().as_deref(), Some("PETG"));
        state.query = "PA 12".into();
        run_sync(&mut world);
        let typed = named(&mut world, "add-item-typed").expect("the typed name's row");
        world.trigger(Activate { entity: typed });
        world.flush();
        run_sync(&mut world);
        let state = world.get::<SearchSelectState>(select).unwrap();
        assert_eq!(state.chosen, None, "each choice is one to add");
        assert!(!state.open && state.query.is_empty());
        let label = named(&mut world, "add-choose-label").unwrap();
        assert_eq!(world.get::<Text>(label).unwrap().0, "Add a material…");
    }

    /// The owner changes the names (another library) and the list follows; with none, it says so.
    #[test]
    fn the_list_follows_its_names() {
        let (mut world, select) = world_with(
            SearchSelect::new("material")
                .options(materials())
                .empty_text("No material matches", "No materials yet"),
        );
        world.get_mut::<SearchSelectState>(select).unwrap().options = vec!["Oak".into()];
        run_sync(&mut world);
        assert!(named(&mut world, "material-item-oak").is_some());
        assert!(named(&mut world, "material-item-petg").is_none());
        world
            .get_mut::<SearchSelectState>(select)
            .unwrap()
            .options
            .clear();
        run_sync(&mut world);
        let mut texts = world.query::<&Text>();
        assert!(texts.iter(&world).any(|t| t.0 == "No materials yet"));
    }
}
