//! Dropdown menus (popover + menu items), modeled on gpui-component's `PopupMenu`
//! (`menu/popup_menu.rs`): items with an optional icon and shortcut, separators, disabled items,
//! and submenus marked with an arrow that open on hover.
//!
//! Open a menu with [`open_menu`], which spawns it below an anchor (usually the button that was
//! clicked) together with an invisible layer that closes it on any click outside. Choosing an
//! item triggers [`MenuAction`] on the item; it bubbles to the menu and the anchor, so observe it
//! on the anchor or globally. Escape closes the innermost menu.
//!
//! Context menus: mark an entity [`ContextMenuTarget`] and a right-click on it triggers
//! [`ContextMenuRequested`] with the pointer position. Answer it with [`open_context_menu`],
//! which opens a menu at that position; its invisible anchor entity receives the bubbling
//! [`MenuAction`], so put whatever the handler needs (such as the clicked row's id) on it.

use std::borrow::Cow;

use bevy::ecs::spawn::SpawnWith;
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::picking::hover::Hovered;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::popover::{Popover, PopoverAlign, PopoverPlacement, PopoverSide};
use bevy::ui_widgets::{Activate, Button as WidgetButton};

use crate::icon::icon;
use crate::style::{InheritFg, InitState, StateColors, VisualState, Visuals};
use crate::theme::Theme;
use crate::z;

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LastPointerButton>()
            .init_resource::<MenuInsets>()
            .add_observer(record_pointer_button)
            .add_observer(on_item_activate)
            .add_observer(on_dismiss_press)
            .add_observer(on_secondary_click)
            .add_systems(Update, (update_submenus, close_on_escape))
            .add_systems(PostUpdate, fit_menus_to_window.before(bevy::ui::UiSystems::Layout));
    }
}

/// A menu's own height cap ([`Menu::max_height`]) and which way it opens, for
/// [`fit_menus_to_window`].
#[derive(Component, Debug, Clone, Copy)]
struct MenuFit {
    cap: Option<f32>,
    vertical: bool,
    /// Capped to the window already (it stays capped: see [`menu_cap`]).
    fitted: Option<f32>,
}

/// A menu's height cap: its own `cap` when that fits the `room`; else, when its `natural` height
/// doesn't fit (or it was capped before, `fitted`: a capped menu measures no taller than its cap,
/// so re-deciding from that would let it spring back every other frame), the room. `None`: no
/// cap. (Final part 4: the flip-flop moved the rows under a still pointer, so the hover lagged a
/// row behind.)
pub fn menu_cap(cap: Option<f32>, natural: f32, room: f32, fitted: Option<f32>) -> Option<f32> {
    let room = room.max(60.0);
    match cap {
        Some(c) if c <= room => Some(c),
        _ if fitted.is_some() || natural > room + 0.5 => Some(room),
        other => other,
    }
}

/// Margin (px) a menu keeps from the window's edges.
const WINDOW_MARGIN: f32 = 4.0;

/// Bands at the window's top and bottom that a long menu keeps clear of when it is capped (the
/// app's tab strip at the bottom).
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub struct MenuInsets {
    pub top: f32,
    pub bottom: f32,
}

/// A menu taller than the room above or below its anchor (a 30-row instance menu near the
/// bottom of the window) is capped to the larger room and scrolls, so the popover finds a
/// placement inside the window instead of running off it or over the tab strip (Final part 3:
/// `course_asm_triad` 08, `course_asm_std_bulk_edit` 02).
fn fit_menus_to_window(
    windows: Query<&Window>,
    insets: Res<MenuInsets>,
    mut q: Query<(&mut MenuFit, &ChildOf, &ComputedNode, &mut Node), With<MenuPopup>>,
    q_parent: Query<(&ComputedNode, &UiGlobalTransform, Has<ContextMenuAnchor>)>,
    mut q_anchor: Query<&mut Node, (With<ContextMenuAnchor>, Without<MenuPopup>)>,
) {
    let Some(w) = windows.iter().next() else { return };
    let h = w.height() - insets.bottom;
    for (mut fit, parent, computed, mut node) in &mut q {
        let Ok((pn, pt, context)) = q_parent.get(parent.parent()) else { continue };
        let s = pn.inverse_scale_factor();
        let (top, bottom) = (pt.translation.y * s - pn.size().y * s / 2.0, pt.translation.y * s + pn.size().y * s / 2.0);
        // The room on either side of the anchor (a submenu beside its item: from the item's top
        // or bottom to the far edge).
        let room = if fit.vertical {
            (h - WINDOW_MARGIN - bottom - 2.0).max(top - insets.top - WINDOW_MARGIN - 2.0)
        } else {
            // Beside the anchor, its top level with the anchor's: only the room below counts
            // (Final part 4: with the room above too, the capped menu didn't fit on the right
            // and opened left, over the triad, `course_asm_ex1_start` 07/09).
            h - WINDOW_MARGIN - top
        };
        let is = computed.inverse_scale_factor();
        let natural = (computed.content_size().y * is).max(computed.size().y * is);
        // Before the first layout the menu has no size yet: nothing to decide.
        if computed.size().y <= 0.0 {
            continue;
        }
        // A context menu (opened at a point) that fits the window but neither below nor above
        // the point moves up, whole, as Onshape's do, rather than scrolling its last rows out of sight (Final
        // part 4: a drawing view's Delete was scrolled away). Only a menu taller than the
        // window is capped.
        if context && fit.vertical && fit.fitted.is_none() {
            let height = fit.cap.map_or(natural, |c| natural.min(c));
            let below = h - WINDOW_MARGIN - bottom - 2.0;
            let above = top - insets.top - WINDOW_MARGIN - 2.0;
            let full = h - insets.top - 2.0 * WINDOW_MARGIN - 2.0;
            // (One that fits above opens there, flipped by the popover, as before.)
            if height > below + 0.5 && height > above + 0.5 && height <= full
                && let Ok(mut a) = q_anchor.get_mut(parent.parent())
                && let Val::Px(t) = a.top
            {
                a.top = Val::Px(t - (height - below).ceil());
                continue;
            }
        }
        let cap = menu_cap(fit.cap, natural, room, fit.fitted);
        // Only the window fit is sticky, not the menu's own cap.
        let fitted = cap.filter(|c| fit.cap != Some(*c));
        if fit.fitted != fitted {
            fit.fitted = fitted;
        }
        // Rounded, so sub-pixel changes in the room don't re-lay the menu out every frame.
        let cap = cap.map(|c| c.floor());
        let want = cap.map_or(Val::Auto, Val::Px);
        if node.max_height != want {
            node.max_height = want;
            node.overflow = if cap.is_some() { Overflow::scroll_y() } else { Overflow::DEFAULT };
        }
    }
}

/// The pointer button of the last press anywhere. Bevy's buttons activate on any button, so a
/// right-click on a [`ContextMenuTarget`] row also activates it; a handler that should only react
/// to left clicks checks this (P3.5: right-clicking a part row doesn't select the part).
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct LastPointerButton(pub PointerButton);

impl Default for LastPointerButton {
    fn default() -> Self {
        Self(PointerButton::Primary)
    }
}

fn record_pointer_button(press: On<Pointer<Press>>, mut last: ResMut<LastPointerButton>) {
    if last.0 != press.button {
        last.0 = press.button;
    }
}

/// A menu item was chosen. Targets the item entity and bubbles up through the menu to the anchor.
#[derive(EntityEvent, Clone, Debug)]
#[entity_event(propagate, auto_propagate)]
pub struct MenuAction {
    pub entity: Entity,
    /// The item's name (as given to [`MenuItem::new`]).
    pub item: String,
}

/// One row of a menu.
#[derive(Debug, Clone)]
pub enum MenuEntry {
    Item(MenuItem),
    Separator,
}

impl From<MenuItem> for MenuEntry {
    fn from(i: MenuItem) -> Self {
        MenuEntry::Item(i)
    }
}

/// Builder for a menu item.
#[derive(Debug, Clone)]
pub struct MenuItem {
    name: Cow<'static, str>,
    label: String,
    icon: Option<Cow<'static, str>>,
    shortcut: Option<String>,
    disabled: bool,
    submenu: Option<Vec<MenuEntry>>,
    force: Option<VisualState>,
    checked: bool,
    tooltip: Option<String>,
    /// A colour dot in the icon column (a label's colour, P3E.2).
    dot: Option<Color>,
}

impl MenuItem {
    pub fn new(name: impl Into<Cow<'static, str>>, label: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            icon: None,
            shortcut: None,
            disabled: false,
            submenu: None,
            force: None,
            checked: false,
            tooltip: None,
            dot: None,
        }
    }

    /// Shows a colour dot in the icon column instead of an icon (a document label's colour).
    pub fn dot(mut self, c: Color) -> Self {
        self.dot = Some(c);
        self
    }

    /// A tooltip card beside the row (e.g. why a disabled item is disabled: "Needs drawings
    /// (P3C.1)"). Shown while the menu is open.
    pub fn tooltip(mut self, text: impl Into<String>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    /// Marks the current choice (gpui-component's `PopupMenu` `checked`): a check mark at the
    /// row's end and the selected highlight.
    pub fn checked(mut self, c: bool) -> Self {
        self.checked = c;
        self
    }

    pub fn icon(mut self, icon: impl Into<Cow<'static, str>>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn shortcut(mut self, s: impl Into<String>) -> Self {
        self.shortcut = Some(s.into());
        self
    }

    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
        self
    }

    /// Makes this item open a submenu (shown with an arrow).
    pub fn submenu(mut self, entries: Vec<MenuEntry>) -> Self {
        self.submenu = Some(entries);
        self
    }

    pub fn force_state(mut self, s: VisualState) -> Self {
        self.force = Some(s);
        self
    }
}

/// Builder for a menu popup.
#[derive(Debug, Clone)]
pub struct Menu {
    name: Cow<'static, str>,
    entries: Vec<MenuEntry>,
    min_width: f32,
    side: PopoverSide,
    /// Right-align the menu with its anchor (for anchors at the right edge of the window).
    align_end: bool,
    item_height: Option<f32>,
    icon_column: bool,
    look: ItemLook,
    /// The tallest the menu gets; longer lists scroll (the Tab manager's 40 tabs).
    max_height: Option<f32>,
}

/// How a menu's rows look.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ItemLook {
    icon_size: f32,
    /// Icons in the foreground colour instead of muted.
    strong_icons: bool,
    /// Shortcuts as keycap chips ("shift" "o") instead of text ("Shift+O").
    keycaps: bool,
}

impl Default for ItemLook {
    fn default() -> Self {
        Self {
            icon_size: 16.0,
            strong_icons: false,
            keycaps: false,
        }
    }
}

impl Menu {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            entries: Vec::new(),
            min_width: 136.0,
            side: PopoverSide::Bottom,
            align_end: false,
            item_height: None,
            icon_column: true,
            look: ItemLook::default(),
            max_height: None,
        }
    }

    /// Caps the menu's height; a longer list scrolls.
    pub fn max_height(mut self, h: f32) -> Self {
        self.max_height = Some(h);
        self
    }

    /// Row height (default [`Theme::menu_item_height`]).
    pub fn item_height(mut self, h: f32) -> Self {
        self.item_height = Some(h);
        self
    }

    /// Plain text rows without the icon column (like Onshape's tab menu).
    /// Icon size (default 16 px).
    pub fn icon_size(mut self, px: f32) -> Self {
        self.look.icon_size = px;
        self
    }

    /// Draws icons in the foreground colour (the sketch Constraints ▾ menu,
    /// `reference/onshape/constraints/constraints-01.png`).
    pub fn strong_icons(mut self) -> Self {
        self.look.strong_icons = true;
        self
    }

    /// Shows shortcuts as keycap chips, one per key (`Shift+O` → `shift` `o`), like the search
    /// box hint.
    pub fn keycap_shortcuts(mut self) -> Self {
        self.look.keycaps = true;
        self
    }

    pub fn text_only(mut self) -> Self {
        self.icon_column = false;
        self
    }

    pub fn item(mut self, item: MenuItem) -> Self {
        self.entries.push(MenuEntry::Item(item));
        self
    }

    pub fn separator(mut self) -> Self {
        self.entries.push(MenuEntry::Separator);
        self
    }

    pub fn entries(mut self, entries: Vec<MenuEntry>) -> Self {
        self.entries = entries;
        self
    }

    pub fn min_width(mut self, w: f32) -> Self {
        self.min_width = w;
        self
    }

    /// Lines the menu's right edge up with its anchor's (it opens toward the left).
    pub fn align_end(mut self) -> Self {
        self.align_end = true;
        self
    }

    /// Where the menu opens relative to its anchor (default below).
    pub fn side(mut self, side: PopoverSide) -> Self {
        self.side = side;
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let theme = theme.clone();
        let align = if self.align_end {
            PopoverAlign::End
        } else {
            PopoverAlign::Start
        };
        let placements = match self.side {
            PopoverSide::Right => vec![
                placement(PopoverSide::Right, PopoverAlign::Start, -2.0),
                placement(PopoverSide::Left, PopoverAlign::Start, -2.0),
            ],
            // Flipped to the other side, then to the other end, when it doesn't fit in the window
            // (a context menu opened at the viewport's right edge opens toward the left, P3B.1
            // judge).
            side => {
                let other = if self.align_end { PopoverAlign::Start } else { PopoverAlign::End };
                vec![
                    placement(side, align, 2.0),
                    placement(side.mirror(), align, 2.0),
                    placement(side, other, 2.0),
                    placement(side.mirror(), other, 2.0),
                ]
            }
        };
        let entries = self.entries;
        let item_height = self.item_height.unwrap_or(theme.menu_item_height);
        let icon_column = self.icon_column;
        let look = self.look;
        let max_height = self.max_height;
        let vertical = matches!(self.side, PopoverSide::Top | PopoverSide::Bottom);
        (
            Name::new(self.name.into_owned()),
            MenuPopup,
            MenuFit { cap: max_height, vertical, fitted: None },
            // Wheel scrolling for a capped menu (`max_height`).
            bevy::ui_widgets::ScrollArea,
            Node {
                position_type: PositionType::Absolute,
                flex_direction: FlexDirection::Column,
                max_height: max_height.map_or(Val::Auto, Val::Px),
                overflow: if max_height.is_some() { Overflow::scroll_y() } else { Overflow::DEFAULT },
                min_width: Val::Px(self.min_width),
                padding: UiRect::vertical(Val::Px(theme.space[2])),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(theme.radius)),
                ..default()
            },
            Popover {
                positions: placements,
                window_margin: 4.0,
            },
            BackgroundColor(theme.popover),
            BorderColor::all(theme.border),
            BoxShadow::new(
                theme.shadow,
                Val::Px(0.0),
                Val::Px(2.0),
                Val::Px(0.0),
                Val::Px(8.0),
            ),
            GlobalZIndex(z::MENU),
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                for entry in entries {
                    match entry {
                        MenuEntry::Separator => {
                            p.spawn((
                                Node {
                                    height: Val::Px(1.0),
                                    flex_shrink: 0.0,
                                    margin: UiRect::vertical(Val::Px(theme.space[2])),
                                    ..default()
                                },
                                BackgroundColor(theme.separator),
                                Pickable::IGNORE,
                            ));
                        }
                        MenuEntry::Item(mut item) => {
                            let tip = item.tooltip.take();
                            let mut e = p.spawn(item_bundle(&theme, item, item_height, icon_column, look));
                            if let Some(t) = tip {
                                e.insert(crate::tooltip::Tooltip::card(t));
                            }
                        }
                    }
                }
            })),
        )
    }
}

fn placement(side: PopoverSide, align: PopoverAlign, gap: f32) -> PopoverPlacement {
    PopoverPlacement { side, align, gap }
}

/// Marks a menu popup.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct MenuPopup;

/// Marks a menu item. Holds the submenu entries, if any.
#[derive(Component, Debug, Clone)]
pub struct MenuItemState {
    pub item: String,
    submenu: Option<Vec<MenuEntry>>,
    theme: Box<Theme>,
}

/// The click-outside layer behind open menus.
#[derive(Component, Debug, Clone, Copy)]
pub struct MenuDismissLayer;

fn item_bundle(
    theme: &Theme,
    item: MenuItem,
    height: f32,
    icon_column: bool,
    look: ItemLook,
) -> impl Bundle {
    let fg = theme.foreground;
    let muted = theme.subtle_foreground;
    let icon_color = if look.strong_icons && !item.disabled {
        // `#333`, as measured in `constraints/constraints-01.png`.
        Color::srgb_u8(0x33, 0x33, 0x33)
    } else {
        muted
    };
    let t = theme.clone();
    let font = theme.font(theme.font_base, FontWeight::NORMAL);
    let small = theme.font(theme.font_sm, FontWeight::NORMAL);
    let has_submenu = item.submenu.is_some();
    let MenuItem {
        name,
        label,
        icon: icon_name,
        shortcut,
        disabled,
        submenu,
        force,
        checked,
        tooltip: _,
        dot,
    } = item;
    let check_color = theme.primary;
    let check_name = format!("{name}-check");
    // Onshape's menus indent icons 16 px (`screens/02`).
    let pad = if icon_column { theme.space[6] } else { 14.0 };
    (
        Name::new(name.to_string()),
        MenuItemState {
            item: name.into_owned(),
            submenu,
            theme: Box::new(theme.clone()),
        },
        Node {
            height: Val::Px(height),
            // A capped menu scrolls; its rows keep their height (Final part 4: they squashed).
            flex_shrink: 0.0,
            padding: UiRect::new(Val::Px(pad), Val::Px(theme.space[5]), Val::ZERO, Val::ZERO),
            align_items: AlignItems::Center,
            column_gap: Val::Px(theme.space[4]),
            ..default()
        },
        WidgetButton,
        Hovered::default(),
        Visuals {
            background: StateColors::new(
                Color::NONE,
                theme.menu_hover,
                theme.list_active,
                Color::NONE,
            )
            .with_selected(theme.menu_hover),
            border: StateColors::all(Color::NONE),
            foreground: StateColors::new(fg, fg, fg, theme.disabled_foreground),
            focus_ring: theme.focus_ring,
        },
        InitState {
            disabled,
            selected: checked,
            force,
        },
        Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
            // Reserve the icon column so labels line up, as Onshape does.
            match icon_name {
                _ if !icon_column => {}
                _ if dot.is_some() => {
                    p.spawn((Node { width: Val::Px(look.icon_size), justify_content: JustifyContent::Center, ..default() }, Pickable::IGNORE)).with_child((
                        Node { width: Val::Px(10.0), height: Val::Px(10.0), border_radius: BorderRadius::MAX, ..default() },
                        BackgroundColor(dot.unwrap_or(Color::NONE)),
                        Pickable::IGNORE,
                    ));
                }
                Some(n) => {
                    p.spawn((icon(n, look.icon_size, icon_color), Pickable::IGNORE));
                }
                None => {
                    p.spawn((
                        Node {
                            width: Val::Px(look.icon_size),
                            ..default()
                        },
                        Pickable::IGNORE,
                    ));
                }
            }
            p.spawn((
                Text::new(label),
                font,
                TextColor(fg),
                TextLayout::no_wrap(),
                InheritFg,
                Pickable::IGNORE,
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
            ));
            if let Some(s) = shortcut.as_ref().filter(|_| look.keycaps) {
                p.spawn((
                    Node {
                        column_gap: Val::Px(2.0),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    Pickable::IGNORE,
                ))
                .with_children(|k| {
                    for key in s.split('+') {
                        k.spawn(crate::toolbar::Kbd::new(key.to_lowercase()).build(&t));
                    }
                });
            } else if let Some(s) = shortcut {
                p.spawn((
                    Text::new(s),
                    small,
                    TextColor(muted),
                    TextLayout::no_wrap(),
                    Pickable::IGNORE,
                ));
            }
            if has_submenu {
                p.spawn((icon("caret-right", 12.0, muted), Pickable::IGNORE));
            }
            if checked {
                p.spawn((
                    Name::new(check_name),
                    icon("check", 14.0, check_color),
                    Pickable::IGNORE,
                ));
            }
        })),
    )
}

/// Opens `menu` (a [`Menu::build`] bundle) anchored to `anchor`, closing any other open menus.
/// Returns the menu entity.
pub fn open_menu(commands: &mut Commands, anchor: Entity, menu: impl Bundle) -> Entity {
    commands.queue(close_all_menus);
    let menu = commands.spawn(menu).insert(ChildOf(anchor)).id();
    // Show the anchor as "open" (selected) while its menu is up.
    commands.queue(move |world: &mut World| {
        if let Ok(mut e) = world.get_entity_mut(anchor)
            && !e.contains::<crate::style::Selected>()
        {
            e.insert((crate::style::Selected, MenuOpenAnchor));
        }
    });
    commands.spawn((
        Name::new("menu-dismiss-layer"),
        MenuDismissLayer,
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        GlobalZIndex(z::MENU - 1),
    ));
    menu
}

/// Right-clicking this entity triggers [`ContextMenuRequested`] on it.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct ContextMenuTarget;

/// A [`ContextMenuTarget`] was right-clicked at `position` (logical px).
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct ContextMenuRequested {
    pub entity: Entity,
    pub position: Vec2,
}

/// The invisible anchor of a context menu; despawned with the menu.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct ContextMenuAnchor;

fn on_secondary_click(
    mut click: On<Pointer<Click>>,
    q: Query<(), With<ContextMenuTarget>>,
    mut commands: Commands,
) {
    if click.button != PointerButton::Secondary || !q.contains(click.entity) {
        return;
    }
    click.propagate(false);
    commands.trigger(ContextMenuRequested {
        entity: click.entity,
        position: click.pointer_location.position,
    });
}

/// Opens `menu` (a [`Menu::build`] bundle) with its top-left corner at `position`, closing any
/// other open menu. Returns the anchor entity, which receives the menu's [`MenuAction`]s.
pub fn open_context_menu(commands: &mut Commands, position: Vec2, menu: impl Bundle) -> Entity {
    commands.queue(close_all_menus);
    let anchor = commands
        .spawn((
            Name::new("context-menu-anchor"),
            ContextMenuAnchor,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(position.x),
                top: Val::Px(position.y - 2.0),
                width: Val::Px(0.0),
                height: Val::Px(0.0),
                ..default()
            },
            GlobalZIndex(z::MENU),
            Pickable::IGNORE,
        ))
        .id();
    commands.spawn(menu).insert(ChildOf(anchor));
    commands.spawn((
        Name::new("menu-dismiss-layer"),
        MenuDismissLayer,
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        GlobalZIndex(z::MENU - 1),
    ));
    anchor
}

/// Marks a menu anchor that is shown selected only because its menu is open.
#[derive(Component, Debug, Clone, Copy)]
pub struct MenuOpenAnchor;

/// Closes every open menu.
pub fn close_all_menus(world: &mut World) {
    let mut q_open = world.query_filtered::<Entity, With<MenuOpenAnchor>>();
    let open: Vec<Entity> = q_open.iter(world).collect();
    for e in open {
        world
            .entity_mut(e)
            .remove::<(crate::style::Selected, MenuOpenAnchor)>();
    }
    let mut q = world.query_filtered::<Entity, Or<(
        With<MenuPopup>,
        With<MenuDismissLayer>,
        With<ContextMenuAnchor>,
    )>>();
    let entities: Vec<Entity> = q.iter(world).collect();
    for e in entities {
        if let Ok(e) = world.get_entity_mut(e) {
            e.despawn();
        }
    }
}

/// True while any menu is open.
pub fn any_menu_open(q: &Query<(), With<MenuDismissLayer>>) -> bool {
    !q.is_empty()
}

fn on_dismiss_press(
    press: On<Pointer<Press>>,
    q: Query<(), With<MenuDismissLayer>>,
    mut commands: Commands,
) {
    if q.contains(press.entity) {
        commands.queue(close_all_menus);
    }
}

fn on_item_activate(
    ev: On<Activate>,
    q_item: Query<(&MenuItemState, Has<InteractionDisabled>)>,
    mut commands: Commands,
) {
    let Ok((item, disabled)) = q_item.get(ev.entity) else {
        return;
    };
    if disabled || item.submenu.is_some() {
        return;
    }
    commands.trigger(MenuAction {
        entity: ev.entity,
        item: item.item.clone(),
    });
    commands.queue(close_all_menus);
}

/// A submenu is open exactly while its item is hovered (hovering the submenu keeps the item
/// hovered, because the submenu is the item's child).
#[allow(clippy::type_complexity)]
fn update_submenus(
    mut commands: Commands,
    q_items: Query<(
        Entity,
        &MenuItemState,
        &Hovered,
        Option<&Children>,
        Has<InteractionDisabled>,
    )>,
    q_popups: Query<(), With<MenuPopup>>,
) {
    for (entity, item, hovered, children, disabled) in &q_items {
        let Some(entries) = &item.submenu else {
            continue;
        };
        let open: Vec<Entity> = children
            .map(|c| c.iter().filter(|e| q_popups.contains(*e)).collect())
            .unwrap_or_default();
        let want = hovered.get() && !disabled;
        if want && open.is_empty() {
            let menu = Menu::new(format!("{}-submenu", item.item))
                .entries(entries.clone())
                .side(PopoverSide::Right)
                .build(&item.theme);
            commands.spawn(menu).insert(ChildOf(entity));
        } else if !want {
            for e in open {
                commands.entity(e).try_despawn();
            }
        }
    }
}

fn close_on_escape(
    mut keys: MessageReader<KeyboardInput>,
    q_layer: Query<(), With<MenuDismissLayer>>,
    mut commands: Commands,
) {
    for k in keys.read() {
        if k.state == ButtonState::Pressed && k.key_code == KeyCode::Escape && !q_layer.is_empty() {
            commands.queue(close_all_menus);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::menu_cap;

    #[test]
    fn a_long_menu_is_capped_and_stays_capped() {
        // 680 px of rows, 600 px of room: capped to the room.
        let first = menu_cap(None, 680.0, 600.0, None);
        assert_eq!(first, Some(600.0));
        // Capped, it measures no taller than the room: it must not spring back to its natural
        // height (the flip-flop that made the hover lag a row).
        assert_eq!(menu_cap(None, 598.0, 600.0, first), Some(600.0));
        // A short menu isn't capped; the menu's own cap wins when it fits.
        assert_eq!(menu_cap(None, 300.0, 600.0, None), None);
        assert_eq!(menu_cap(Some(250.0), 680.0, 600.0, None), Some(250.0));
        // Its own cap taller than the room: the room.
        assert_eq!(menu_cap(Some(900.0), 680.0, 600.0, None), Some(600.0));
    }
}
