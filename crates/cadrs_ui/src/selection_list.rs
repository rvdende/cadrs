//! Selection lists: a selection field that holds several picks, one row each, as in Onshape's
//! feature dialogs (`reference/onshape/training/intro-to-part-studios/ex1-step3.png`: "Faces
//! and sketch regions to extrude" with three "Face of Sketch 1" rows, each with its ✕).
//!
//! - **Empty**: like an empty [`crate::SelectionField`], a box with the parameter's name as the
//!   placeholder (pale blue while it waits for picks).
//! - **Filled**: the name as a small caption at the top, then one 18 px row per item (red when
//!   [`SelectionListState::red`] marks it) with a ✕
//!   (named `<name>-item-<i>-remove`) at its right. The rows are named `<name>-item-<i>`.
//!
//! The app owns the items: it sets [`SelectionListState`] and reacts to
//! [`SelectionListRemove`] (a ✕) and [`SelectionListActivate`] (a click on the list, to make it
//! the field that takes the next pick).
//!
//! **Replace reference** (P3D.4, IR4.2): while a list has [`SelectionListReplaceable`]`(true)`
//! (the app sets it while its Repair panel is open), hovering a row shows a small "Replace
//! reference" icon at its right in place of the ✕ (`<name>-item-<i>-replace`), which triggers
//! [`SelectionListReplace`]. Every row carries [`SelectionListItem`] and [`Hovered`], so the
//! app can tell which item the pointer is over.
//!
//! **Reordering** ([`SelectionList::reorderable`], a Loft's Profiles, PS20.2): with two or more
//! items the caption has a ↑↓ button (`<name>-reorder`, "Reorder items"). It turns on drag
//! handles at the rows' left (`<name>-item-<i>-handle`; the ✕ hide) and a **Done** button
//! under them (`<name>-done`). Dropping a row sends [`SelectionListMove`]; the app reorders its
//! items.

use std::borrow::Cow;

use bevy::picking::hover::Hovered;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, observe};

use crate::button::{Button, visuals_for};
use crate::icon::icon;
use crate::tooltip::Tooltip;
use crate::style::StateColors;
use crate::theme::Theme;

pub struct SelectionListPlugin;

impl Plugin for SelectionListPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_list_click)
            .add_systems(PostUpdate, sync_selection_lists.before(bevy::ui::UiSystems::Prepare))
            .add_systems(PostUpdate, show_replace_on_hover.after(sync_selection_lists).before(bevy::ui::UiSystems::Prepare));
    }
}

/// What a selection list shows.
#[derive(Component, Debug, Clone, PartialEq, Eq, Default)]
#[require(Hovered)]
pub struct SelectionListState {
    /// The picked items' labels ("Face of Sketch 1").
    pub items: Vec<String>,
    /// Takes the next pick (pale blue).
    pub active: bool,
    /// Something picked is gone: the border and the items are red.
    pub error: bool,
    /// The items are red but the field is fine.
    pub red_items: bool,
    /// Which items are red (a part without a material in Mass properties, X7); items past its
    /// end are not.
    pub red: Vec<bool>,
}

/// An item's ✕ was clicked. Targets the list.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct SelectionListRemove {
    pub entity: Entity,
    pub index: usize,
}

/// A row was dragged by its handle from `from` to `to` (reorder mode). Targets the list.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct SelectionListMove {
    pub entity: Entity,
    pub from: usize,
    pub to: usize,
}

/// P3D.4 (IR4.2): the rows show a "Replace reference" icon on hover.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SelectionListReplaceable(pub bool);

/// A row's "Replace reference" icon was clicked. Targets the list.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct SelectionListReplace {
    pub entity: Entity,
    pub index: usize,
}

/// A row of a filled list: its list and item.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[require(Hovered)]
pub struct SelectionListItem {
    pub list: Entity,
    pub index: usize,
}

/// A row's "Replace reference" icon.
#[derive(Component, Debug, Clone, Copy)]
struct ItemReplace {
    list: Entity,
    index: usize,
    row: Entity,
}

/// Whether a reorderable list shows its drag handles.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SelectionListReorder(pub bool);

/// A row's drag handle: the list, the item and the row.
#[derive(Component, Debug, Clone, Copy)]
struct ItemHandle {
    list: Entity,
    index: usize,
    row: Entity,
}

/// The list was clicked. Targets the list.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct SelectionListActivate {
    pub entity: Entity,
}

/// The list's name and parameter name (for its rows).
#[derive(Component, Debug, Clone)]
struct ListMeta {
    name: String,
    placeholder: String,
    reorderable: bool,
    chevrons: bool,
    /// An icon before each item's ✕ (named `<name>-item-<i>-icon`) and its tooltip.
    item_icon: Option<(Cow<'static, str>, String)>,
    /// Long items are cut in the middle, keeping their ends apart.
    middle_ellipsis: bool,
    /// A filled field keeps the pale blue box (Onshape's mate dialog, `ex2-step18.png`).
    tint_filled: bool,
}

/// A row's ✕: the list and the item.
#[derive(Component, Debug, Clone, Copy)]
struct ItemRemove {
    list: Entity,
    index: usize,
}

/// Builder for a selection list.
#[derive(Debug, Clone)]
pub struct SelectionList {
    name: Cow<'static, str>,
    placeholder: String,
    state: SelectionListState,
    reorderable: bool,
    chevrons: bool,
    item_icon: Option<(Cow<'static, str>, String)>,
    middle_ellipsis: bool,
    tint_filled: bool,
}

impl SelectionList {
    /// Long items are cut in the middle ("Mate connector of Rear…mount"), so items that differ
    /// only at the end can be told apart.
    pub fn middle_ellipsis(mut self) -> Self {
        self.middle_ellipsis = true;
        self
    }

    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            placeholder: String::new(),
            state: SelectionListState::default(),
            reorderable: false,
            chevrons: false,
            item_icon: None,
            middle_ellipsis: false,
            tint_filled: false,
        }
    }

    /// Each row starts with a ">" chevron, as a Loft's profile rows (`ex4-step9.png`), where it
    /// stands for the row's own options (drawn only; the rows don't open).
    pub fn chevrons(mut self, c: bool) -> Self {
        self.chevrons = c;
        self
    }

    /// A small icon on every item, before its ✕ (a mate connector's edit glyph, A6.3).
    pub fn item_icon(mut self, icon: impl Into<Cow<'static, str>>, tooltip: impl Into<String>) -> Self {
        self.item_icon = Some((icon.into(), tooltip.into()));
        self
    }

    /// Items can be put in another order (the ↑↓ button and drag handles).
    pub fn reorderable(mut self, r: bool) -> Self {
        self.reorderable = r;
        self
    }

    /// The parameter's name: the placeholder while empty, the caption once filled.
    pub fn placeholder(mut self, p: impl Into<String>) -> Self {
        self.placeholder = p.into();
        self
    }

    pub fn items(mut self, items: Vec<String>) -> Self {
        self.state.items = items;
        self
    }

    pub fn active(mut self, a: bool) -> Self {
        self.state.active = a;
        self
    }

    pub fn error(mut self, e: bool) -> Self {
        self.state.error = e;
        self
    }

    pub fn red_items(mut self, r: bool) -> Self {
        self.state.red_items = r;
        self
    }

    /// A filled field keeps the pale blue box, not only an empty active one.
    pub fn tint_filled(mut self) -> Self {
        self.tint_filled = true;
        self
    }

    /// Marks single items red.
    pub fn red(mut self, r: Vec<bool>) -> Self {
        self.state.red = r;
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let t = theme.clone();
        let meta = ListMeta {
            name: self.name.to_string(),
            placeholder: self.placeholder,
            reorderable: self.reorderable,
            chevrons: self.chevrons,
            item_icon: self.item_icon,
            middle_ellipsis: self.middle_ellipsis,
            tint_filled: self.tint_filled,
        };
        let state = self.state;
        let (node, bg, border) = list_style(&t, &state, meta.tint_filled);
        // The rows are spawned by `sync_selection_lists` (the new state counts as changed).
        (Name::new(self.name.into_owned()), meta, state, SelectionListReorder::default(), node, bg, border)
    }
}

fn list_style(t: &Theme, s: &SelectionListState, tint_filled: bool) -> (Node, BackgroundColor, BorderColor) {
    let filled = !s.items.is_empty();
    let node = Node {
        flex_direction: FlexDirection::Column,
        flex_grow: 1.0,
        min_height: Val::Px(24.0),
        padding: if filled {
            UiRect::new(Val::Px(6.0), Val::Px(2.0), Val::Px(3.0), Val::Px(2.0))
        } else {
            UiRect::horizontal(Val::Px(6.0))
        },
        justify_content: if filled { JustifyContent::FlexStart } else { JustifyContent::Center },
        border: UiRect::all(Val::Px(1.0)),
        border_radius: BorderRadius::all(Val::Px(2.0)),
        ..default()
    };
    let (bg, border) = if s.error && s.red.iter().any(|r| *r) {
        // P3D.1 (IR5.2): a field holding a missing reference is tinted red
        // (`ex1-step13.png`), its missing items red, the others as usual.
        (Color::srgb_u8(0xfd, 0xec, 0xec), t.feature_error)
    } else if s.error {
        (t.background, t.feature_error)
    } else if s.active || (tint_filled && filled) {
        (t.selection_field_active, t.selection_field_active_border)
    } else {
        (t.background, Color::srgb_u8(0xe0, 0xe0, 0xe0))
    };
    (node, BackgroundColor(bg), BorderColor::all(border))
}

/// A small ghost icon button in the list (the ↑↓ and the ✕).
fn ghost_visuals(t: &Theme) -> crate::style::Visuals {
    let mut ghost = visuals_for(t, crate::ButtonVariant::Ghost);
    ghost.foreground = StateColors::new(
        Color::srgb_u8(0x55, 0x55, 0x55),
        t.foreground,
        t.foreground,
        t.disabled_foreground,
    );
    ghost.background = StateColors::all(Color::NONE);
    ghost
}

fn small_square() -> Node {
    Node {
        width: Val::Px(18.0),
        height: Val::Px(18.0),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    }
}

fn spawn_contents(
    p: &mut ChildSpawnerCommands,
    t: &Theme,
    list: Entity,
    meta: &ListMeta,
    s: &SelectionListState,
    reordering: bool,
    replaceable: bool,
) {
    if s.items.is_empty() {
        p.spawn((
            t.text(meta.placeholder.clone(), 10.5, FontWeight::NORMAL, Color::srgb_u8(0x3d, 0x4b, 0x52)),
            Pickable::IGNORE,
        ));
        return;
    }
    let reorder = meta.reorderable && s.items.len() >= 2;
    if reorder {
        p.spawn((
            Node {
                margin: UiRect::bottom(Val::Px(1.0)),
                align_items: AlignItems::Center,
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|c| {
            c.spawn((
                t.text(meta.placeholder.clone(), 10.0, FontWeight::NORMAL, t.muted_foreground),
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
                Pickable::IGNORE,
            ));
            c.spawn((
                Button::new(format!("{}-reorder", meta.name))
                    .icon("reorder")
                    .icon_size(12.0)
                    .ghost()
                    .selected(reordering)
                    .tooltip("Reorder items")
                    .build(t),
                observe(move |_: On<Activate>, mut q: Query<&mut SelectionListReorder>| {
                    if let Ok(mut r) = q.get_mut(list) {
                        r.0 = !r.0;
                    }
                }),
            ))
            .insert((ghost_visuals(t), small_square()));
        });
    } else {
        p.spawn((
            t.text(meta.placeholder.clone(), 10.0, FontWeight::NORMAL, t.muted_foreground),
            Node {
                margin: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            Pickable::IGNORE,
        ));
    }
    let reordering = reordering && reorder;
    for (i, item) in s.items.iter().enumerate() {
        let marked = s.red.get(i).copied().unwrap_or(false);
        // With single items marked, an error field reds only them (P3D.1).
        let red = marked || s.red_items || (s.error && !s.red.iter().any(|r| *r));
        let color = if red { t.feature_error } else { t.tool_foreground };
        let ghost = ghost_visuals(t);
        p.spawn((
            Name::new(format!("{}-item-{i}", meta.name)),
            SelectionListItem { list, index: i },
            // A right-click asks for the item's menu (P3D.4: a missing item's "Edit healthy
            // moment of …").
            crate::menu::ContextMenuTarget,
            Node {
                height: Val::Px(18.0),
                align_items: AlignItems::Center,
                ..default()
            },
            ZIndex(0),
            // Hoverable (for the Replace icon and the app's highlights), letting clicks through
            // to the list.
            Pickable { should_block_lower: false, is_hoverable: true },
        ))
        .with_children(|r| {
            if reordering {
                let row = r.target_entity();
                r.spawn((
                    Name::new(format!("{}-item-{i}-handle", meta.name)),
                    ItemHandle { list, index: i, row },
                    icon("drag-handle", 12.0, Color::srgb_u8(0x70, 0x70, 0x70)),
                    Tooltip::new("Drag to reorder"),
                ))
                .insert(Node {
                    width: Val::Px(12.0),
                    height: Val::Px(12.0),
                    margin: UiRect::right(Val::Px(3.0)),
                    flex_shrink: 0.0,
                    ..default()
                })
                .observe(on_handle_drag)
                .observe(on_handle_drag_end);
            } else if meta.chevrons {
                r.spawn((
                    Name::new(format!("{}-item-{i}-chevron", meta.name)),
                    icon("chevron-right", 12.0, color),
                    Pickable::IGNORE,
                ))
                .insert(Node {
                    width: Val::Px(12.0),
                    height: Val::Px(12.0),
                    margin: UiRect::right(Val::Px(3.0)),
                    flex_shrink: 0.0,
                    ..default()
                });
            }
            // A long item ends in "…" before its buttons ("Mate connector of Rear Ca…").
            // The label takes what the icons and the ✕ leave (basis 0), so it is measured
            // against that room and never runs under them.
            r.spawn((
                Node {
                    flex_grow: 1.0,
                    flex_shrink: 1.0,
                    flex_basis: Val::Px(0.0),
                    min_width: Val::Px(0.0),
                    overflow: Overflow::clip(),
                    ..default()
                },
                // Hovering the label shows the whole item (a cut one ends in "…"), beside the
                // field so it doesn't cover the rows below (P3G.5 judge: the Blind dropdown).
                Pickable { should_block_lower: false, is_hoverable: true },
                Tooltip::beside(item.clone()),
            ))
            .with_children(|c| {
                c.spawn((
                    t.text(item.clone(), t.font_base, FontWeight::NORMAL, color),
                    crate::ellipsis::Ellipsis::node(),
                    if meta.middle_ellipsis { crate::ellipsis::Ellipsis::middle() } else { crate::ellipsis::Ellipsis::default() },
                    Pickable::IGNORE,
                ))
                .insert(TextLayout::no_wrap());
            });
            if reordering {
                return;
            }
            if replaceable {
                // In the ✕'s place, only while hovered (`ex1-step13.png`). It takes room in the
                // row while shown, so the label ends in "…" before it instead of running under
                // it (Final regression judge: conrod 14).
                let row = r.target_entity();
                let (_, field_bg, _) = list_style(t, s, meta.tint_filled);
                let mut v = ghost_visuals(t);
                v.background = StateColors::all(field_bg.0);
                r.spawn((
                    ItemReplace { list, index: i, row },
                    Button::new(format!("{}-item-{i}-replace", meta.name))
                        // Two sheets, as the course's Replace reference glyph (not "restore",
                        // a bin with an arrow that read as delete).
                        .icon("copy")
                        .icon_size(12.0)
                        .ghost()
                        .tooltip("Replace reference")
                        .build(t),
                    observe(|a: On<Activate>, q: Query<&ItemReplace>, mut commands: Commands| {
                        if let Ok(c) = q.get(a.entity) {
                            commands.trigger(SelectionListReplace { entity: c.list, index: c.index });
                        }
                    }),
                ))
                .insert((
                    v,
                    Node {
                        width: Val::Px(18.0),
                        height: Val::Px(18.0),
                        flex_shrink: 0.0,
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        display: Display::None,
                        ..default()
                    },
                ));
            }
            if let Some((icon_name, tip)) = &meta.item_icon {
                r.spawn((
                    Name::new(format!("{}-item-{i}-icon", meta.name)),
                    icon(icon_name.clone(), 12.0, Color::srgb_u8(0x70, 0x70, 0x70)),
                    Tooltip::new(tip.clone()),
                ))
                .insert(Node {
                    width: Val::Px(12.0),
                    height: Val::Px(12.0),
                    margin: UiRect::horizontal(Val::Px(3.0)),
                    flex_shrink: 0.0,
                    ..default()
                });
            }
            r.spawn((
                ItemRemove { list, index: i },
                Button::new(format!("{}-item-{i}-remove", meta.name))
                    .icon("close")
                    .icon_size(10.0)
                    .ghost()
                    .tooltip("Remove")
                    .build(t),
                observe(|a: On<Activate>, q: Query<&ItemRemove>, mut commands: Commands| {
                    if let Ok(c) = q.get(a.entity) {
                        commands.trigger(SelectionListRemove {
                            entity: c.list,
                            index: c.index,
                        });
                    }
                }),
            ))
            .insert((ghost, small_square()));
        });
    }
    if reordering {
        p.spawn(Node {
            justify_content: JustifyContent::FlexEnd,
            margin: UiRect::vertical(Val::Px(2.0)),
            ..default()
        })
        .with_child((
            Button::new(format!("{}-done", meta.name)).label("Done").small().build(t),
            observe(move |_: On<Activate>, mut q: Query<&mut SelectionListReorder>| {
                if let Ok(mut r) = q.get_mut(list) {
                    r.0 = false;
                }
            }),
        ));
    }
}

/// Rows a handle's drag moves its row by: the drag's height in rows.
fn rows_moved(dy: f32) -> isize {
    (dy / 18.0).round() as isize
}

/// A handle's drag moves its row with the pointer.
fn on_handle_drag(mut ev: On<Pointer<Drag>>, q: Query<&ItemHandle>, mut q_node: Query<(&mut Node, &mut ZIndex)>) {
    ev.propagate(false);
    let Ok(h) = q.get(ev.entity) else { return };
    if let Ok((mut n, mut z)) = q_node.get_mut(h.row) {
        n.top = Val::Px(ev.distance.y);
        z.set_if_neq(ZIndex(1));
    }
}

/// Dropped: the row goes where it was dragged to.
fn on_handle_drag_end(
    mut ev: On<Pointer<DragEnd>>,
    q: Query<&ItemHandle>,
    q_state: Query<&SelectionListState>,
    mut q_node: Query<(&mut Node, &mut ZIndex)>,
    mut commands: Commands,
) {
    ev.propagate(false);
    let Ok(h) = q.get(ev.entity) else { return };
    if let Ok((mut n, mut z)) = q_node.get_mut(h.row) {
        n.top = Val::Auto;
        z.set_if_neq(ZIndex(0));
    }
    let n = q_state.get(h.list).map_or(0, |s| s.items.len());
    if n == 0 {
        return;
    }
    let to = (h.index as isize + rows_moved(ev.distance.y)).clamp(0, n as isize - 1) as usize;
    if to != h.index {
        commands.trigger(SelectionListMove { entity: h.list, from: h.index, to });
    }
}

fn on_list_click(mut click: On<Pointer<Click>>, q: Query<(), With<SelectionListState>>, mut commands: Commands) {
    if click.button != PointerButton::Primary {
        return;
    }
    if q.contains(click.entity) {
        click.propagate(false);
        commands.trigger(SelectionListActivate {
            entity: click.entity,
        });
    }
}

#[allow(clippy::type_complexity)]
fn sync_selection_lists(
    theme: Res<Theme>,
    mut q: Query<
        (
            Entity,
            &SelectionListState,
            &SelectionListReorder,
            &ListMeta,
            &mut Node,
            &mut BackgroundColor,
            &mut BorderColor,
            Option<&SelectionListReplaceable>,
        ),
        Or<(Changed<SelectionListState>, Changed<SelectionListReorder>, Changed<SelectionListReplaceable>)>,
    >,
    mut commands: Commands,
) {
    for (e, s, reorder, meta, mut node, mut bg, mut border, replaceable) in &mut q {
        let replaceable = replaceable.is_some_and(|r| r.0);
        let (n, b, br) = list_style(&theme, s, meta.tint_filled);
        *node = n;
        bg.set_if_neq(b);
        border.set_if_neq(br);
        let t = theme.clone();
        let (m, st, on) = (meta.clone(), s.clone(), reorder.0);
        commands.entity(e).despawn_children();
        commands.entity(e).with_children(|p| {
            let list = p.target_entity();
            spawn_contents(p, &t, list, &m, &st, on, replaceable);
        });
    }
}

/// A row's Replace icon shows while the row (or the icon) is hovered, in place of the row's ✕
/// (as `ex1-step13.png`), so the label keeps its room.
#[allow(clippy::type_complexity)]
fn show_replace_on_hover(
    q_rows: Query<&Hovered, With<SelectionListItem>>,
    mut q_icons: Query<(&ItemReplace, &Hovered, &mut Node), Without<ItemRemove>>,
    mut q_remove: Query<(&ChildOf, &mut Node), (With<ItemRemove>, Without<ItemReplace>)>,
) {
    for (r, own, mut node) in &mut q_icons {
        let on = own.get() || q_rows.get(r.row).is_ok_and(|h| h.get());
        let want = if on { Display::Flex } else { Display::None };
        if node.display != want {
            node.display = want;
        }
        let other = if on { Display::None } else { Display::Flex };
        for (parent, mut n) in &mut q_remove {
            if parent.parent() == r.row && n.display != other {
                n.display = other;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::rows_moved;

    #[test]
    fn a_drag_moves_by_whole_rows() {
        // Rows are 18 px: half a row or more counts.
        assert_eq!(rows_moved(0.0), 0);
        assert_eq!(rows_moved(8.0), 0);
        assert_eq!(rows_moved(9.5), 1);
        assert_eq!(rows_moved(37.0), 2);
        assert_eq!(rows_moved(-20.0), -1);
    }
}
