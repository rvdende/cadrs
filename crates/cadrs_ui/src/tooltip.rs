//! Tooltips, modeled on gpui-component's `Tooltip` (which can carry a key binding) and styled
//! like Onshape's: a small light blue-grey label (`#d3dae4`, `#333` text, 2 px radius, 12 px
//! text) that appears below the hovered element after [`Theme::tooltip_delay`]. A shortcut is
//! shown after the text as keycaps (`Undo` `ctrl` `z`).
//!
//! - `Tooltip::new("Line (L)")` splits a trailing shortcut in parentheses into keycaps, so
//!   builders can keep writing tooltips the way they read.
//! - Pressing the element hides its tooltip until the pointer leaves it (as in GPUI).
//! - The delay starts over whenever the element under the pointer changes, including when the
//!   UI changes under a pointer that stays still (a toolbar rebuilt after a click).
//! - [`Tooltip::error`] explains why something failed (a feature-list row in error): white,
//!   with a red accent and dark text, left-aligned under its element and wrapped to the
//!   element's width, so it stays inside the panel the element is in.

use std::time::Duration;

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui::{InteractionDisabled, Pressed, UiGlobalTransform, UiTransform, Val2};

use crate::theme::Theme;
use crate::z;

pub struct TooltipPlugin;

impl Plugin for TooltipPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TooltipState>()
            .add_systems(PostUpdate, update_tooltips.before(bevy::ui::UiSystems::Prepare))
            .add_systems(PostUpdate, keep_on_screen.after(bevy::ui::UiSystems::Layout));
    }
}

/// Shows `text` (and `shortcut` as keycaps) in a tooltip while the entity is hovered.
#[derive(Component, Clone, Debug, PartialEq)]
#[require(Hovered)]
pub struct Tooltip {
    pub text: String,
    /// Keys like `Ctrl+Z` or `Shift+S`, shown as keycaps.
    pub shortcut: Option<String>,
    pub style: TooltipStyle,
}

/// How a tooltip looks and where it goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TooltipStyle {
    /// A small label centered below the element.
    #[default]
    Label,
    /// An error message: left-aligned below the element, wrapped to its width, white with a red
    /// accent.
    Error,
    /// An information card (a list row's details, several lines): white with a border and a
    /// shadow, beside the element (to its right, top-aligned), so it doesn't cover the rows below.
    Card,
    /// A small label beside the element (to its right, centred on it), so it doesn't cover the
    /// rows below: a feature-list row's status ("Sketch 4 (Hidden) is not fully defined").
    Beside,
    /// A help card (P3.9: the feature filter's prefixes): white, left-aligned below the element.
    /// The first line of the text is its title; each later line is a row (not wrapped), and a
    /// tab splits a row into a bold term and its description.
    Help,
}

impl Tooltip {
    /// A tooltip; a trailing shortcut in parentheses ("Line (L)", "Undo (Ctrl+Z)") becomes its
    /// keycaps.
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        match split_shortcut(&text) {
            Some((t, k)) => Self {
                text: t.to_string(),
                shortcut: Some(k.to_string()),
                style: TooltipStyle::Label,
            },
            None => Self {
                text,
                shortcut: None,
                style: TooltipStyle::Label,
            },
        }
    }

    /// An error message (see [`TooltipStyle::Error`]).
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            shortcut: None,
            style: TooltipStyle::Error,
        }
    }

    /// An information card (see [`TooltipStyle::Card`]).
    pub fn card(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            shortcut: None,
            style: TooltipStyle::Card,
        }
    }

    /// A label beside its element (see [`TooltipStyle::Beside`]).
    pub fn beside(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            shortcut: None,
            style: TooltipStyle::Beside,
        }
    }

    /// A help card (see [`TooltipStyle::Help`]).
    pub fn help(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            shortcut: None,
            style: TooltipStyle::Help,
        }
    }

    /// Sets the shortcut explicitly.
    pub fn shortcut(mut self, keys: impl Into<String>) -> Self {
        self.shortcut = Some(keys.into());
        self
    }

    /// The keycaps of the shortcut: `Shift+S` → `["shift", "s"]`.
    pub fn keycaps(&self) -> Vec<String> {
        self.shortcut
            .as_deref()
            .map(keycaps)
            .unwrap_or_default()
    }
}

/// The keycaps for a shortcut: `Ctrl+Shift+Z` → `ctrl`, `shift`, `z`; `Alt+C` → `alt`, `c`.
pub fn keycaps(shortcut: &str) -> Vec<String> {
    shortcut
        .split('+')
        .map(|k| match k.trim() {
            "Esc" | "Escape" => "escape".to_string(),
            "Del" | "Delete" => "delete".to_string(),
            k => k.to_lowercase(),
        })
        .filter(|k| !k.is_empty())
        .collect()
}

/// Splits "Line (L)" into ("Line", "L") when the parenthesized part looks like a shortcut.
fn split_shortcut(text: &str) -> Option<(&str, &str)> {
    let body = text.strip_suffix(')')?;
    let open = body.rfind(" (")?;
    let (label, keys) = (&body[..open], &body[open + 2..]);
    const NAMED: [&str; 16] = [
        "Ctrl", "Shift", "Alt", "Enter", "Esc", "Escape", "Delete", "Del", "Space", "Tab",
        "Backspace", "Up", "Down", "Left", "Right", "Cmd",
    ];
    let ok = !keys.is_empty()
        && keys.split('+').all(|k| {
            let k = k.trim();
            (k.chars().count() == 1 && !k.chars().any(char::is_whitespace))
                || NAMED.contains(&k)
                || (k.starts_with('F') && k[1..].parse::<u8>().is_ok())
        });
    ok.then_some((label, keys))
}

/// The tooltip bubble currently on screen.
#[derive(Component)]
pub struct TooltipBubble;

/// The left edge (logical px) of the element a label bubble belongs to: a bubble that would run
/// off the window's left edge lines up with it instead (P3C.7: the drawing Update button's tooltip
/// sat at x = 5, over the left rail).
#[derive(Component, Clone, Copy)]
struct TooltipOwnerLeft(f32);

#[derive(Resource, Default)]
struct TooltipState {
    owner: Option<Entity>,
    hovered_for: Duration,
    bubble: Option<Entity>,
    /// Pressed while hovered: no tooltip until the pointer leaves.
    suppressed: Option<Entity>,
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn update_tooltips(
    mut commands: Commands,
    mut state: ResMut<TooltipState>,
    time: Res<Time>,
    theme: Res<Theme>,
    q_owners: Query<(
        Entity,
        &Tooltip,
        &Hovered,
        Has<InteractionDisabled>,
        Has<Pressed>,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    q_menus: Query<(), With<crate::menu::MenuDismissLayer>>,
    q_menu_items: Query<(), With<crate::menu::MenuItemState>>,
) {
    let menus_open = !q_menus.is_empty();
    let hovered = q_owners
        .iter()
        // While a menu is open only its own items' tooltips show.
        .filter(|(e, ..)| !menus_open || q_menu_items.contains(*e))
        .filter(|(_, _, h, ..)| h.get())
        // Prefer the smallest hovered element (the innermost one).
        .min_by(|a, b| {
            let area = |n: &ComputedNode| n.size().x * n.size().y;
            area(a.5).total_cmp(&area(b.5))
        });

    let owner = hovered.as_ref().map(|h| h.0);
    if owner != state.owner {
        if let Some(b) = state.bubble.take() {
            commands.entity(b).try_despawn();
        }
        state.owner = owner;
        state.hovered_for = Duration::ZERO;
        state.suppressed = None;
    }
    let Some((e, tooltip, _, _, pressed, node, transform)) = hovered else {
        return;
    };
    if pressed {
        state.suppressed = Some(e);
    }
    if state.suppressed == Some(e) {
        if let Some(b) = state.bubble.take() {
            commands.entity(b).try_despawn();
        }
        return;
    }
    state.hovered_for += time.delta();
    if state.bubble.is_some() || state.hovered_for < theme.tooltip_delay {
        return;
    }

    let scale = node.inverse_scale_factor();
    let center = transform.translation * scale;
    let size = node.size() * scale;
    let t = &*theme;
    let text = tooltip.text.clone();
    let caps = tooltip.keycaps();
    if tooltip.style == TooltipStyle::Card {
        let bubble = commands
            .spawn((
                Name::new("tooltip"),
                TooltipBubble,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(center.x + size.x / 2.0 + 8.0),
                    top: Val::Px(center.y - size.y / 2.0),
                    padding: UiRect::new(Val::Px(9.0), Val::Px(9.0), Val::Px(6.0), Val::Px(6.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(t.radius_sm)),
                    ..default()
                },
                BackgroundColor(Color::WHITE),
                BorderColor::all(Color::srgb_u8(0xc3, 0xca, 0xd4)),
                BoxShadow::new(
                    Color::srgba(0.0, 0.0, 0.0, 0.22),
                    Val::Px(0.0),
                    Val::Px(2.0),
                    Val::Px(0.0),
                    Val::Px(8.0),
                ),
                GlobalZIndex(z::TOOLTIP),
                Pickable::IGNORE,
            ))
            .with_children(|b| {
                b.spawn((
                    Text::new(text),
                    t.font(t.font_sm, FontWeight::NORMAL),
                    TextColor(Color::srgb_u8(0x1f, 0x23, 0x28)),
                    TextLayout::new(bevy::text::Justify::Left, bevy::text::LineBreak::NoWrap),
                    Pickable::IGNORE,
                ));
            })
            .id();
        state.bubble = Some(bubble);
        return;
    }
    if tooltip.style == TooltipStyle::Help {
        let (left, top) = (center.x - size.x / 2.0, center.y + size.y / 2.0 + 4.0);
        let mut lines = text.lines();
        let title = lines.next().unwrap_or_default().to_string();
        let rows: Vec<(String, String)> = lines
            .map(|l| match l.split_once('\t') {
                Some((a, b)) => (a.to_string(), b.to_string()),
                None => (String::new(), l.to_string()),
            })
            .collect();
        let ink = Color::srgb_u8(0x1f, 0x23, 0x28);
        let muted = t.muted_foreground;
        let bubble = commands
            .spawn((
                Name::new("tooltip"),
                TooltipBubble,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(left),
                    top: Val::Px(top),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(3.0),
                    padding: UiRect::new(Val::Px(10.0), Val::Px(10.0), Val::Px(7.0), Val::Px(8.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(t.radius_sm)),
                    ..default()
                },
                BackgroundColor(Color::WHITE),
                BorderColor::all(Color::srgb_u8(0xc3, 0xca, 0xd4)),
                BoxShadow::new(
                    Color::srgba(0.0, 0.0, 0.0, 0.18),
                    Val::Px(0.0),
                    Val::Px(2.0),
                    Val::Px(0.0),
                    Val::Px(6.0),
                ),
                GlobalZIndex(z::TOOLTIP),
                Pickable::IGNORE,
            ))
            .with_children(|b| {
                b.spawn((
                    Text::new(title),
                    t.font(t.font_sm, FontWeight::BOLD),
                    TextColor(ink),
                    Node { margin: UiRect::bottom(Val::Px(2.0)), ..default() },
                    Pickable::IGNORE,
                ));
                for (term, what) in rows {
                    b.spawn((Node { column_gap: Val::Px(8.0), ..default() }, Pickable::IGNORE)).with_children(|r| {
                        r.spawn((
                            Text::new(term),
                            t.font(t.font_sm, FontWeight::SEMIBOLD),
                            TextColor(ink),
                            TextLayout::no_wrap(),
                            Node { width: Val::Px(104.0), flex_shrink: 0.0, ..default() },
                            Pickable::IGNORE,
                        ));
                        r.spawn((
                            Text::new(what),
                            t.font(t.font_sm, FontWeight::NORMAL),
                            TextColor(muted),
                            TextLayout::no_wrap(),
                            Pickable::IGNORE,
                        ));
                    });
                }
            })
            .id();
        state.bubble = Some(bubble);
        return;
    }
    if tooltip.style == TooltipStyle::Error {
        // Left-aligned under the element, a little in from its left edge, and no wider. A small
        // element, such as an icon, gets a 220 px bubble below and to the right of it, clear of
        // the icon itself (gpui-component's tooltips sit offset from what they explain).
        let small = size.x < 40.0;
        let inset = 20.0f32.min(size.x / 4.0);
        let (left, top) = if small {
            (center.x + size.x / 2.0 + 6.0, center.y + size.y / 2.0 + 8.0)
        } else {
            (center.x - size.x / 2.0 + inset, center.y + size.y / 2.0 + 4.0)
        };
        let width = if small { 220.0 } else { (size.x - inset).max(120.0) };
        let bubble = commands
            .spawn((
                Name::new("tooltip"),
                TooltipBubble,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(left),
                    top: Val::Px(top),
                    width: Val::Px(width),
                    padding: UiRect::new(Val::Px(8.0), Val::Px(8.0), Val::Px(5.0), Val::Px(5.0)),
                    border: UiRect::new(Val::Px(3.0), Val::Px(1.0), Val::Px(1.0), Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(t.radius_sm)),
                    ..default()
                },
                BackgroundColor(Color::WHITE),
                BorderColor {
                    left: t.feature_error,
                    ..BorderColor::all(Color::srgb_u8(0xc3, 0xca, 0xd4))
                },
                BoxShadow::new(
                    Color::srgba(0.0, 0.0, 0.0, 0.18),
                    Val::Px(0.0),
                    Val::Px(2.0),
                    Val::Px(0.0),
                    Val::Px(6.0),
                ),
                GlobalZIndex(z::TOOLTIP),
                Pickable::IGNORE,
            ))
            .with_children(|b| {
                b.spawn((
                    Text::new(text),
                    t.font(t.font_sm, FontWeight::MEDIUM),
                    TextColor(Color::srgb_u8(0x1f, 0x23, 0x28)),
                    // Wrapped to the bubble's width (the element's).
                    TextLayout::new(bevy::text::Justify::Left, bevy::text::LineBreak::WordBoundary),
                    Pickable::IGNORE,
                ));
            })
            .id();
        state.bubble = Some(bubble);
        return;
    }
    // Below the element, centred; or beside it, centred on its height.
    let beside = tooltip.style == TooltipStyle::Beside;
    let (left, top, shift) = if beside {
        (center.x + size.x / 2.0 + 4.0, center.y, Val2::new(Val::Px(0.0), Val::Percent(-50.0)))
    } else {
        (center.x, center.y + size.y / 2.0 + 6.0, Val2::new(Val::Percent(-50.0), Val::Px(0.0)))
    };
    let bubble = commands
        .spawn((
            Name::new("tooltip"),
            TooltipBubble,
            TooltipOwnerLeft(center.x - size.x / 2.0),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(left),
                top: Val::Px(top),
                padding: UiRect::axes(Val::Px(7.0), Val::Px(4.0)),
                border_radius: BorderRadius::all(Val::Px(t.radius_sm)),
                align_items: AlignItems::Center,
                column_gap: Val::Px(3.0),
                ..default()
            },
            UiTransform::from_translation(shift),
            BackgroundColor(t.tooltip_background),
            GlobalZIndex(z::TOOLTIP),
            Pickable::IGNORE,
        ))
        .with_children(|b| {
            b.spawn((
                t.text(text, t.font_sm, FontWeight::NORMAL, t.tooltip_foreground),
                Node {
                    margin: UiRect::right(Val::Px(if caps.is_empty() { 0.0 } else { 3.0 })),
                    ..default()
                },
                Pickable::IGNORE,
            ));
            for cap in caps {
                b.spawn((tooltip_keycap(t, cap), Pickable::IGNORE));
            }
        })
        .id();
    state.bubble = Some(bubble);
}

/// A bubble that runs off the window's right edge moves left onto it (P3.6: the right-hand
/// panel strip's tooltips).
fn keep_on_screen(
    windows: Query<&Window>,
    mut q: Query<(&mut Node, &ComputedNode, &UiGlobalTransform, Option<&TooltipOwnerLeft>), With<TooltipBubble>>,
) {
    let Some(w) = windows.iter().next() else { return };
    let width = w.width();
    for (mut node, computed, t, owner_left) in &mut q {
        let scale = computed.inverse_scale_factor();
        let right = (t.translation.x + computed.size().x / 2.0) * scale;
        let over = right - (width - 4.0);
        if over > 0.5
            && let Val::Px(left) = node.left
        {
            node.left = Val::Px(left - over);
        }
        // Nor past the left edge (a long tooltip under a button near it).
        let left_edge = (t.translation.x - computed.size().x / 2.0) * scale;
        // Lined up with its element's left edge when that is on screen, else at the edge.
        let under = match owner_left {
            Some(o) if left_edge < 4.0 && o.0 >= 4.0 => o.0 - left_edge,
            _ => 4.0 - left_edge,
        };
        if under > 0.5
            && over <= 0.5
            && let Val::Px(left) = node.left
        {
            node.left = Val::Px(left + under);
        }
    }
}

/// A keycap in a tooltip: lowercase key text in a small bordered box, a shade lighter than the
/// tooltip.
fn tooltip_keycap(t: &Theme, key: String) -> impl Bundle {
    (
        Name::new("tooltip-keycap"),
        Node {
            height: Val::Px(16.0),
            min_width: Val::Px(16.0),
            padding: UiRect::horizontal(Val::Px(3.0)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(2.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::srgb_u8(0xee, 0xf1, 0xf5)),
        BorderColor::all(Color::srgb_u8(0x8e, 0x99, 0xa8)),
        children![(
            t.text(key, 10.0, FontWeight::MEDIUM, t.tooltip_foreground),
            Pickable::IGNORE,
        )],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_split_into_keycaps() {
        let t = Tooltip::new("Undo (Ctrl+Z)");
        assert_eq!(t.text, "Undo");
        assert_eq!(t.keycaps(), ["ctrl", "z"]);
        assert_eq!(Tooltip::new("Point (Shift+S)").keycaps(), ["shift", "s"]);
        assert_eq!(Tooltip::new("Line (L)").keycaps(), ["l"]);
        // Not a shortcut: kept as text.
        let t = Tooltip::new("Linear pattern (beta)");
        assert_eq!(t.text, "Linear pattern (beta)");
        assert!(t.shortcut.is_none());
        assert!(Tooltip::new("Documents").shortcut.is_none());
        assert_eq!(Tooltip::new("Keyboard shortcuts").shortcut("Shift+/").keycaps(), ["shift", "/"]);
    }
}
