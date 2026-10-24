//! Toolbar pieces, modeled on Onshape's feature toolbar and gpui-component's `Button` in
//! toolbar use: [`ToolButton`] (an icon, optionally followed by a ▾ for tools with variants),
//! [`toolbar_separator`] (the thin rule between groups) and [`Kbd`] (a keycap, as in
//! "Search tools… alt c").
//!
//! Tool buttons trigger `bevy::ui_widgets::Activate` like every other button. A split button's
//! ▾ ([`ToolButton::split`]) triggers [`ToolDropdown`] instead (Onshape's: the button starts the
//! tool, its ▾ lists the variants); on other buttons the ▾ is part of the button.

use std::borrow::Cow;

use bevy::ecs::spawn::SpawnWith;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::Button as WidgetButton;

use crate::button::ButtonTooltip;
use crate::icon::icon;
use crate::style::{InheritFg, InitState, StateColors, VisualState, Visuals};
use crate::theme::Theme;

/// A [`ToolButton`]'s ▾ was clicked (the button isn't activated): its menu of variants should
/// open. Targets the button.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct ToolDropdown {
    pub entity: Entity,
}

/// The ▾'s click: the button's [`ToolDropdown`] instead of its `Activate`.
fn caret_click(
    mut click: On<Pointer<Click>>,
    parents: Query<&ChildOf>,
    disabled: Query<(), With<bevy::ui::InteractionDisabled>>,
    mut commands: Commands,
) {
    click.propagate(false);
    if let Ok(p) = parents.get(click.entity)
        && !disabled.contains(p.parent())
    {
        commands.trigger(ToolDropdown { entity: p.parent() });
    }
}

/// A split [`ToolButton`] (its ▾ triggers [`ToolDropdown`]): from a [`ToolbarOverflow`]'s » it
/// opens its menu of variants rather than starting the tool.
#[derive(Component, Debug, Clone, Copy)]
pub struct SplitTool(pub bool);

/// Builder for a toolbar button: a 20 px icon in a 28 px square, or a wider button when it has a
/// dropdown caret or a label (like "✎ Sketch").
#[derive(Debug, Clone)]
pub struct ToolButton {
    name: Cow<'static, str>,
    icon: Cow<'static, str>,
    label: Option<String>,
    dropdown: bool,
    split: bool,
    disabled: bool,
    selected: bool,
    force: Option<VisualState>,
    tooltip: Option<String>,
    icon_size: f32,
}

impl ToolButton {
    pub fn new(name: impl Into<Cow<'static, str>>, icon: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            icon: icon.into(),
            label: None,
            dropdown: false,
            split: false,
            disabled: false,
            selected: false,
            force: None,
            tooltip: None,
            icon_size: 24.0,
        }
    }

    /// A text label after the icon (Onshape shows one only on Sketch and Insert).
    pub fn label(mut self, l: impl Into<String>) -> Self {
        self.label = Some(l.into());
        self
    }

    /// Adds a small ▾ after the icon, for tools with a menu of variants.
    pub fn dropdown(mut self, d: bool) -> Self {
        self.dropdown = d;
        self
    }

    /// A ▾ of its own: clicking it triggers [`ToolDropdown`] rather than `Activate` (the tool's
    /// menu of variants, while the button starts the tool).
    pub fn split(mut self) -> Self {
        self.dropdown = true;
        self.split = true;
        self
    }

    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
        self
    }

    pub fn selected(mut self, s: bool) -> Self {
        self.selected = s;
        self
    }

    pub fn force_state(mut self, s: VisualState) -> Self {
        self.force = Some(s);
        self
    }

    pub fn tooltip(mut self, t: impl Into<String>) -> Self {
        self.tooltip = Some(t.into());
        self
    }

    pub fn icon_size(mut self, px: f32) -> Self {
        self.icon_size = px;
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let visuals = Visuals {
            background: StateColors::new(
                Color::NONE,
                theme.ghost_hover,
                theme.ghost_active,
                Color::NONE,
            )
            .with_selected(theme.list_selected),
            border: StateColors::all(Color::NONE),
            foreground: StateColors::new(
                theme.tool_foreground,
                theme.foreground,
                theme.foreground,
                theme.tool_disabled_foreground,
            )
            .with_selected(theme.link),
            focus_ring: theme.focus_ring,
        };
        let fg = visuals.foreground.normal;
        let icon_name = self.icon;
        let icon_size = self.icon_size;
        let dropdown = self.dropdown;
        let split = self.split;
        let label = self.label.clone();
        // The caret gets its own name so scenarios can open the dropdown without coordinates.
        let caret_name = format!("{}-caret", self.name);
        // Onshape's text tool buttons (Sketch, Insert) are semibold (`screens/05a`).
        let font = theme.font(theme.font_base, FontWeight::SEMIBOLD);
        let pad = if label.is_some() {
            6.0
        } else if dropdown {
            4.0
        } else {
            0.0
        };
        (
            Name::new(self.name.into_owned()),
            Node {
                height: Val::Px(32.0),
                min_width: Val::Px(32.0),
                padding: UiRect::horizontal(Val::Px(pad)),
                border_radius: BorderRadius::all(Val::Px(theme.radius)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                column_gap: Val::Px(if label.is_some() { 5.0 } else { 3.0 }),
                flex_shrink: 0.0,
                ..default()
            },
            WidgetButton,
            Hovered::default(),
            TabIndex(0),
            visuals,
            InitState {
                disabled: self.disabled,
                selected: self.selected,
                force: self.force,
            },
            ButtonTooltip(self.tooltip),
            SplitTool(split),
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                p.spawn((icon(icon_name, icon_size, fg), InheritFg, Pickable::IGNORE));
                if let Some(l) = label {
                    p.spawn((
                        Text::new(l),
                        font,
                        TextColor(fg),
                        TextLayout::no_wrap(),
                        InheritFg,
                        Pickable::IGNORE,
                    ));
                }
                if split {
                    p.spawn((
                        Name::new(caret_name),
                        icon("chevron-down", 14.0, fg),
                        InheritFg,
                    ))
                    .observe(caret_click);
                } else if dropdown {
                    p.spawn((
                        Name::new(caret_name),
                        icon("chevron-down", 14.0, fg),
                        InheritFg,
                        Pickable::IGNORE,
                    ));
                }
            })),
        )
    }
}

/// A toolbar row that fits its width (#22: at 1224 px the Part Studio's tools ran off the
/// right edge): the children that don't fit are hidden, from the end, and a » button at the
/// end lists them in a menu; choosing one activates its button (a split tool's opens its
/// variants), its menu opening under the ». Children marked [`OverflowKeep`] stay (the search
/// box); one marked [`OverflowWithPrevious`] goes with the child before it (a separate ▾);
/// children something else hid are left alone.
#[derive(Component, Debug, Default)]
pub struct ToolbarOverflow;

/// A [`ToolbarOverflow`] child that is never hidden.
#[derive(Component, Debug, Default)]
pub struct OverflowKeep;

/// A [`ToolbarOverflow`] child hidden and shown with the one before it.
#[derive(Component, Debug, Default)]
pub struct OverflowWithPrevious;

/// Where a tool chosen from a [`ToolbarOverflow`]'s » opens its menu (its own button is
/// hidden): under the ». Read with [`tool_menu_at`].
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct OverflowOrigin(pub Option<Vec2>);

/// Where a toolbar button's menu opens: under its lower left corner, nudged by `offset`, or,
/// for a button the » hid (no size), under the ».
pub fn tool_menu_at(
    node: Option<(&ComputedNode, &UiGlobalTransform)>,
    offset: Vec2,
    origin: &OverflowOrigin,
) -> Vec2 {
    match node {
        Some((n, t)) if n.size().x > 0.0 => {
            let s = n.inverse_scale_factor();
            let size = n.size() * s;
            t.translation * s + Vec2::new(-size.x / 2.0, size.y / 2.0 + 2.0) + offset
        }
        _ => origin.0.unwrap_or_default(),
    }
}

/// The » button of a [`ToolbarOverflow`] row.
#[derive(Component)]
struct OverflowButton;

/// A child a [`ToolbarOverflow`] row hid.
#[derive(Component, Debug, Clone, Copy)]
struct Overflowed;

/// The width a child took when last shown (logical px, with its margins).
#[derive(Component, Debug, Clone, Copy)]
struct ShownWidth(f32);

/// A tool chosen from a »: activated the frame after (closing the » menu closes every menu,
/// so the tool's own would close with it); `true` for a split tool's ▾.
#[derive(Resource, Debug, Default)]
struct OverflowChosen(Option<(Entity, bool)>);

fn activate_chosen(mut chosen: ResMut<OverflowChosen>, mut commands: Commands) {
    if let Some((entity, split)) = chosen.0.take() {
        if split {
            commands.trigger(ToolDropdown { entity });
        } else {
            commands.trigger(bevy::ui_widgets::Activate { entity });
        }
    }
}

/// The »'s width.
const MORE: f32 = 32.0;

pub(crate) struct ToolbarPlugin;

impl Plugin for ToolbarPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OverflowOrigin>()
            .init_resource::<OverflowChosen>()
            .add_systems(PreUpdate, activate_chosen)
            .add_systems(PostUpdate, fit_toolbars.after(bevy::ui::UiSystems::Layout))
            .add_observer(on_overflow);
    }
}

fn px(v: Val) -> f32 {
    match v {
        Val::Px(p) => p,
        _ => 0.0,
    }
}

/// Which units of a row to hide: each unit's width and whether it stays; `gap` between shown
/// units, `available` across. `None` when everything fits (no »); else the units that don't
/// fit, from the first that doesn't on, with room for the » kept.
fn fit(units: &[(f32, bool)], gap: f32, available: f32) -> Option<Vec<bool>> {
    let span = |ws: &mut dyn Iterator<Item = f32>| {
        let (sum, n) = ws.fold((0.0, 0), |(s, n), w| (s + w, n + 1));
        sum + gap * (n.max(1) - 1) as f32
    };
    if span(&mut units.iter().map(|u| u.0)) <= available + 0.5 {
        return None;
    }
    // Room for the tools that may go: what the kept ones and the » leave.
    let kept: f32 = units.iter().filter(|u| u.1).map(|u| u.0 + gap).sum();
    // The » and the gap before it.
    let room = available - kept - MORE - gap;
    let mut used = 0.0;
    let mut hiding = false;
    Some(
        units
            .iter()
            .map(|&(w, keep)| {
                if keep {
                    return false;
                }
                hiding |= used + w > room + 0.5;
                used += w + gap;
                hiding
            })
            .collect(),
    )
}

/// Hides what doesn't fit and shows the » when anything is hidden (the layout catches up next
/// frame).
#[allow(clippy::type_complexity)]
fn fit_toolbars(
    rows: Query<(Entity, &ComputedNode, &Node, Option<&Children>), With<ToolbarOverflow>>,
    mut items: Query<
        (
            &ComputedNode,
            &mut Node,
            Option<&ShownWidth>,
            Has<OverflowButton>,
            Has<Overflowed>,
            Has<OverflowKeep>,
            Has<OverflowWithPrevious>,
        ),
        Without<ToolbarOverflow>,
    >,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    for (row, computed, row_node, children) in &rows {
        let children: Vec<Entity> = children.map_or(Vec::new(), |c| c.to_vec());
        let button = children
            .iter()
            .copied()
            .find(|c| items.get(*c).is_ok_and(|i| i.3));
        // The » goes last (a toolbar rebuilt for another tab drops it), hidden until needed.
        if button.is_none() || children.last() != button.as_ref() {
            if let Some(b) = button {
                commands.entity(b).despawn();
            }
            let b = commands
                .spawn((
                    OverflowButton,
                    ToolButton::new("toolbar-overflow", "chevron-right")
                        .tooltip("More tools")
                        .build(&theme),
                ))
                .insert(Node {
                    display: Display::None,
                    height: Val::Px(32.0),
                    min_width: Val::Px(MORE),
                    border_radius: BorderRadius::all(Val::Px(theme.radius)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    flex_shrink: 0.0,
                    ..default()
                })
                .id();
            commands.entity(row).add_child(b);
            continue;
        }
        let Some(button) = button else { continue };
        let k = computed.inverse_scale_factor();
        let gap = px(row_node.column_gap);
        let available =
            computed.size().x * k - px(row_node.padding.left) - px(row_node.padding.right);
        // Units (a child and those going with it), their widths: as laid out when shown, else
        // as last shown. Children hidden by something else take no room and aren't touched.
        let mut units: Vec<(Vec<Entity>, f32, bool)> = Vec::new();
        for &c in children.iter().filter(|c| **c != button) {
            let Ok((cn, node, shown, _, overflowed, keep, with_previous)) = items.get(c) else {
                continue;
            };
            if node.display == Display::None && !overflowed {
                continue;
            }
            // Shown again by its owner while hidden here (a button that shows itself): measured
            // as shown, and hidden again below if it still doesn't fit.
            let overflowed = overflowed && node.display == Display::None;
            let w = if overflowed {
                shown.map_or(0.0, |s| s.0)
            } else {
                cn.size().x * k + px(node.margin.left) + px(node.margin.right)
            };
            if !overflowed && shown.is_none_or(|s| (s.0 - w).abs() > 0.5) {
                commands.entity(c).insert(ShownWidth(w));
            }
            match units.last_mut() {
                Some(u) if with_previous => {
                    u.0.push(c);
                    u.1 += w + gap;
                }
                _ => units.push((vec![c], w, keep)),
            }
        }
        let widths: Vec<(f32, bool)> = units.iter().map(|u| (u.1, u.2)).collect();
        let hide = fit(&widths, gap, available);
        for (i, (members, _, _)) in units.iter().enumerate() {
            let hidden = hide.as_ref().is_some_and(|h| h[i]);
            for &c in members {
                let Ok((_, mut node, _, _, overflowed, _, _)) = items.get_mut(c) else {
                    continue;
                };
                let overflowed = overflowed && node.display == Display::None;
                if hidden && !overflowed {
                    node.display = Display::None;
                    commands.entity(c).insert(Overflowed);
                } else if !hidden && overflowed {
                    node.display = Display::Flex;
                    commands.entity(c).remove::<Overflowed>();
                }
            }
        }
        if let Ok((_, mut node, ..)) = items.get_mut(button) {
            let display = if hide.is_some() {
                Display::Flex
            } else {
                Display::None
            };
            if node.display != display {
                node.display = display;
            }
        }
    }
}

type HiddenTools<'w, 's> = Query<
    'w,
    's,
    (
        &'static Name,
        Option<&'static ButtonTooltip>,
        Has<bevy::ui::InteractionDisabled>,
        Option<&'static SplitTool>,
    ),
    With<Overflowed>,
>;

/// The »: a menu of the hidden tools (by their tooltips); choosing one activates it (a split
/// tool's ▾), its own menu opening under the ».
#[allow(clippy::too_many_arguments)]
fn on_overflow(
    a: On<bevy::ui_widgets::Activate>,
    buttons: Query<&ChildOf, With<OverflowButton>>,
    rows: Query<&Children, With<ToolbarOverflow>>,
    hidden: HiddenTools,
    at: Query<(&ComputedNode, &UiGlobalTransform)>,
    theme: Res<Theme>,
    mut origin: ResMut<OverflowOrigin>,
    mut commands: Commands,
) {
    let Ok(row) = buttons.get(a.entity).map(|p| p.parent()) else {
        return;
    };
    let Ok(children) = rows.get(row) else { return };
    let mut menu = crate::menu::Menu::new("toolbar-overflow-menu").min_width(180.0);
    let mut targets = Vec::new();
    for &c in children {
        let Ok((name, tip, disabled, split)) = hidden.get(c) else {
            continue;
        };
        // Tools only (a hidden separator has no tooltip).
        let Some(label) = tip.and_then(|t| t.0.clone()) else {
            continue;
        };
        let item = format!("toolbar-overflow-{}", name.as_str());
        menu = menu.item(crate::menu::MenuItem::new(item.clone(), label).disabled(disabled));
        targets.push((item, c, split.is_some_and(|s| s.0)));
    }
    let pos = tool_menu_at(at.get(a.entity).ok(), Vec2::ZERO, &OverflowOrigin(None));
    origin.0 = Some(pos);
    let anchor = crate::menu::open_context_menu(&mut commands, pos, menu.build(&theme));
    commands.entity(anchor).observe(
        move |ev: On<crate::menu::MenuAction>, mut chosen: ResMut<OverflowChosen>| {
            if let Some(&(_, entity, split)) = targets.iter().find(|(n, ..)| *n == ev.item) {
                chosen.0 = Some((entity, split));
            }
        },
    );
}

/// The thin vertical rule between toolbar groups.
pub fn toolbar_separator(theme: &Theme) -> impl Bundle {
    (
        Name::new("toolbar-separator"),
        Node {
            width: Val::Px(1.0),
            height: Val::Px(26.0),
            margin: UiRect::horizontal(Val::Px(4.0)),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(theme.toolbar_separator),
        Pickable::IGNORE,
    )
}

/// A keycap, like gpui-component's `Kbd`: small text in a bordered box.
pub struct Kbd {
    text: String,
}

impl Kbd {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        (
            Node {
                height: Val::Px(14.0),
                padding: UiRect::horizontal(Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(2.0)),
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                ..default()
            },
            BorderColor::all(theme.kbd_border),
            BackgroundColor(theme.background),
            Pickable::IGNORE,
            children![(
                theme.text(self.text, 10.0, FontWeight::NORMAL, theme.muted_foreground),
                Pickable::IGNORE,
            )],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::fit;

    #[test]
    fn everything_fits_without_the_more_button() {
        // 3 × 30 with 2 gaps of 3 is 96 across.
        assert_eq!(fit(&[(30.0, false); 3], 3.0, 96.0), None);
    }

    #[test]
    fn the_tools_past_the_room_go_but_kept_ones_stay() {
        // 200 across, a kept 60 (the search box): 200 − 63 − 32 − 3 = 102 for tools of 30 + 3:
        // three fit (99), the fourth and fifth go, the kept one stays.
        let units = [
            (30.0, false),
            (30.0, false),
            (30.0, false),
            (30.0, false),
            (30.0, false),
            (60.0, true),
        ];
        assert_eq!(
            fit(&units, 3.0, 200.0),
            Some(vec![false, false, false, true, true, false])
        );
    }

    #[test]
    fn the_more_button_and_its_gap_fit_after_the_last_tool() {
        // 4 × 30 with gaps of 3 (129) in 128: the » (32) and the gap before it must fit after the
        // tools left, so two stay (30 + 3 + 30 + 3 + 32 = 98); three would be 131.
        assert_eq!(
            fit(&[(30.0, false); 4], 3.0, 128.0),
            Some(vec![false, false, true, true])
        );
    }
}
