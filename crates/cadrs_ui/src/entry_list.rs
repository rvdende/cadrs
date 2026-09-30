//! Entry groups: a bordered box of expandable entries, each with its own parameters nested
//! under it, as Onshape's Fillet dialog lays out a variable fillet's *Vertices* and *Points on
//! edges* (`reference/onshape/training/intro-to-part-studios/lesson-fillet-and-chamfer.png`:
//! "⌄ [0.5 in] Vertex of Extrude 1 ✕" with Radius and Magnitude under it; the Points group's
//! **CLEAR** link at its top right).
//!
//! - [`EntryGroup`]: the box, its caption ("Vertices") and an optional link at the right of the
//!   caption (`<name>-action`, triggering [`EntryGroupAction`]). It is pale blue while it takes
//!   the next pick ([`EntryGroupState::active`]); a click anywhere in it triggers
//!   [`EntryGroupActivate`]. The owner spawns the entries (and anything else, such as an "Add"
//!   button) as its children after the caption.
//! - [`Entry`]: a header row with a chevron (`<name>-toggle`) that opens and closes the entry,
//!   the title (`<name>-title`, cut with "…" where it doesn't fit) and a ✕ (`<name>-remove`,
//!   triggering [`EntryRemove`]), then the content (`<name>-content`), indented under the title.
//!   The owner keeps [`EntryState::title`] up to date ("[2 mm] Vertex of Extrude 1").
//!
//! Modeled on gpui-component's `Accordion` items (a chevron header over its content) inside a
//! bordered `List` group.

use std::borrow::Cow;

use bevy::ecs::spawn::SpawnWith;
use bevy::picking::hover::Hovered;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, Button as WidgetButton};

use crate::button::{Button, visuals_for};
use crate::ellipsis::Ellipsis;
use crate::icon::{Icon, icon};
use crate::style::StateColors;
use crate::theme::Theme;

pub struct EntryListPlugin;

impl Plugin for EntryListPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_group_click)
            .add_observer(on_entry_button)
            .add_systems(PostUpdate, sync_entries.before(crate::icon::resolve_icons));
    }
}

/// Whether a group takes the next pick (pale blue, like an active selection list).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EntryGroupState {
    pub active: bool,
}

/// A click in the group. Targets the group.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct EntryGroupActivate {
    pub entity: Entity,
}

/// The group's link (CLEAR) was clicked. Targets the group.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct EntryGroupAction {
    pub entity: Entity,
}

/// An entry's title and whether it is open.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct EntryState {
    pub title: String,
    pub open: bool,
}

/// An entry's ✕ was clicked. Targets the entry.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct EntryRemove {
    pub entity: Entity,
}

/// An entry was opened or closed. Targets the entry.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct EntryToggled {
    pub entity: Entity,
    pub open: bool,
}

/// A button in an entry's header: its entry and what it does.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum EntryButton {
    Toggle(Entity),
    Remove(Entity),
    Action(Entity),
}

#[derive(Component, Debug, Clone, Copy)]
struct EntryTitle(Entity);

#[derive(Component, Debug, Clone, Copy)]
struct EntryChevron(Entity);

#[derive(Component, Debug, Clone, Copy)]
struct EntryContent(Entity);

type SpawnFn = Box<dyn FnOnce(&mut ChildSpawner) + Send + Sync>;

fn group_colors(t: &Theme, active: bool) -> (Color, Color) {
    if active {
        (t.selection_field_active, t.selection_field_active_border)
    } else {
        (t.background, Color::srgb_u8(0xe0, 0xe0, 0xe0))
    }
}

/// A small grey icon button's look (the chevron and the ✕), inserted over the button's own.
fn small_button(t: &Theme) -> impl Bundle {
    let mut ghost = visuals_for(t, crate::ButtonVariant::Ghost);
    ghost.foreground = StateColors::new(Color::srgb_u8(0x55, 0x55, 0x55), t.foreground, t.foreground, t.disabled_foreground);
    ghost.background = StateColors::all(Color::NONE);
    (
        ghost,
        Node {
            width: Val::Px(16.0),
            height: Val::Px(18.0),
            flex_shrink: 0.0,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
    )
}

/// Builder for an entry group.
pub struct EntryGroup {
    name: Cow<'static, str>,
    caption: String,
    action: Option<String>,
    active: bool,
    content: Option<SpawnFn>,
}

impl EntryGroup {
    pub fn new(name: impl Into<Cow<'static, str>>, caption: impl Into<String>) -> Self {
        Self { name: name.into(), caption: caption.into(), action: None, active: false, content: None }
    }

    /// A link at the caption's right ("CLEAR").
    pub fn action(mut self, label: impl Into<String>) -> Self {
        self.action = Some(label.into());
        self
    }

    pub fn active(mut self, a: bool) -> Self {
        self.active = a;
        self
    }

    /// The entries (and anything after them).
    pub fn content(mut self, f: impl FnOnce(&mut ChildSpawner) + Send + Sync + 'static) -> Self {
        self.content = Some(Box::new(f));
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let t = theme.clone();
        let EntryGroup { name, caption, action, active, content } = self;
        let action_name = format!("{name}-action");
        let (bg, border) = group_colors(&t, active);
        (
            Name::new(name.into_owned()),
            EntryGroupState { active },
            Node {
                flex_direction: FlexDirection::Column,
                flex_shrink: 0.0,
                padding: UiRect::new(Val::Px(4.0), Val::Px(2.0), Val::Px(3.0), Val::Px(4.0)),
                row_gap: Val::Px(2.0),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(bg),
            BorderColor::all(border),
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                let group = p.target_entity();
                p.spawn((
                    Node { height: Val::Px(16.0), align_items: AlignItems::Center, padding: UiRect::left(Val::Px(2.0)), ..default() },
                    Pickable::IGNORE,
                ))
                .with_children(|h| {
                    h.spawn((
                        t.text(caption, 10.0, FontWeight::NORMAL, t.muted_foreground),
                        Node { flex_grow: 1.0, ..default() },
                        Pickable::IGNORE,
                    ));
                    if let Some(label) = action {
                        h.spawn((
                            Name::new(action_name),
                            EntryButton::Action(group),
                            WidgetButton,
                            Hovered::default(),
                            Node { padding: UiRect::horizontal(Val::Px(4.0)), ..default() },
                        ))
                        .with_child((t.text(label, 9.5, FontWeight::SEMIBOLD, t.muted_foreground), Pickable::IGNORE));
                    }
                });
                if let Some(f) = content {
                    f(p);
                }
            })),
        )
    }
}

/// Builder for an entry.
pub struct Entry {
    name: Cow<'static, str>,
    title: String,
    open: bool,
    content: Option<SpawnFn>,
}

impl Entry {
    pub fn new(name: impl Into<Cow<'static, str>>, title: impl Into<String>) -> Self {
        Self { name: name.into(), title: title.into(), open: true, content: None }
    }

    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// The entry's parameters, shown while it is open.
    pub fn content(mut self, f: impl FnOnce(&mut ChildSpawner) + Send + Sync + 'static) -> Self {
        self.content = Some(Box::new(f));
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let t = theme.clone();
        let Entry { name, title, open, content } = self;
        let (toggle, title_name, remove, content_name) =
            (format!("{name}-toggle"), format!("{name}-title"), format!("{name}-remove"), format!("{name}-content"));
        (
            Name::new(name.into_owned()),
            EntryState { title: title.clone(), open },
            Node { flex_direction: FlexDirection::Column, flex_shrink: 0.0, ..default() },
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                let entry = p.target_entity();
                p.spawn((Node { height: Val::Px(20.0), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE))
                    .with_children(|h| {
                        h.spawn((Button::new(toggle).ghost().tooltip(if open { "Collapse" } else { "Expand" }).build(&t), EntryButton::Toggle(entry)))
                            .insert(small_button(&t))
                            .with_child((
                            EntryChevron(entry),
                            icon(if open { "chevron-down" } else { "chevron-right" }, 11.0, Color::srgb_u8(0x55, 0x55, 0x55)),
                            Pickable::IGNORE,
                        ));
                        h.spawn((
                            Name::new(title_name),
                            EntryTitle(entry),
                            Ellipsis::default(),
                            t.text(title, t.font_base, FontWeight::NORMAL, t.tool_foreground),
                            Node { flex_grow: 1.0, margin: UiRect::left(Val::Px(2.0)), ..Ellipsis::node() },
                            Pickable::IGNORE,
                        ))
                        .insert(TextLayout::new(Justify::Left, LineBreak::NoWrap));
                        h.spawn((Button::new(remove).icon("close").icon_size(10.0).ghost().tooltip("Remove").build(&t), EntryButton::Remove(entry)))
                            .insert(small_button(&t));
                    });
                let mut c = p.spawn((
                    Name::new(content_name),
                    EntryContent(entry),
                    Node {
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::left(Val::Px(16.0)),
                        display: if open { Display::Flex } else { Display::None },
                        ..default()
                    },
                ));
                if let Some(f) = content {
                    c.with_children(|c| f(c));
                }
            })),
        )
    }
}

fn on_entry_button(ev: On<Activate>, q: Query<&EntryButton>, mut q_state: Query<&mut EntryState>, mut commands: Commands) {
    match q.get(ev.entity) {
        Ok(EntryButton::Toggle(e)) => {
            if let Ok(mut s) = q_state.get_mut(*e) {
                s.open = !s.open;
                commands.trigger(EntryToggled { entity: *e, open: s.open });
            }
        }
        Ok(EntryButton::Remove(e)) => commands.trigger(EntryRemove { entity: *e }),
        Ok(EntryButton::Action(g)) => commands.trigger(EntryGroupAction { entity: *g }),
        Err(_) => {}
    }
}

/// A click in a group (bubbled up from its rows) makes it take the next pick.
fn on_group_click(click: On<Pointer<Click>>, q: Query<(), With<EntryGroupState>>, mut commands: Commands) {
    if click.button == PointerButton::Primary && q.contains(click.entity) {
        commands.trigger(EntryGroupActivate { entity: click.entity });
    }
}

#[allow(clippy::type_complexity)]
fn sync_entries(
    theme: Res<Theme>,
    q_groups: Query<(&EntryGroupState, &mut BackgroundColor, &mut BorderColor), Changed<EntryGroupState>>,
    q_entries: Query<(Entity, &EntryState), Changed<EntryState>>,
    mut q_title: Query<(&EntryTitle, &mut Text)>,
    mut q_chevron: Query<(&EntryChevron, &mut Icon)>,
    mut q_content: Query<(&EntryContent, &mut Node)>,
) {
    for (s, mut bg, mut border) in q_groups {
        let (b, br) = group_colors(&theme, s.active);
        bg.set_if_neq(BackgroundColor(b));
        border.set_if_neq(BorderColor::all(br));
    }
    for (e, s) in &q_entries {
        for (t, mut text) in &mut q_title {
            if t.0 == e && text.0 != s.title {
                text.0 = s.title.clone();
            }
        }
        for (c, mut i) in &mut q_chevron {
            if c.0 == e {
                let want = if s.open { "chevron-down" } else { "chevron-right" };
                if i.name != want {
                    i.name = want.into();
                }
            }
        }
        for (c, mut n) in &mut q_content {
            if c.0 == e {
                let d = if s.open { Display::Flex } else { Display::None };
                if n.display != d {
                    n.display = d;
                }
            }
        }
    }
}
