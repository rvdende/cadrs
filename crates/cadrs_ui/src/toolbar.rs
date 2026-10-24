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
fn caret_click(mut click: On<Pointer<Click>>, parents: Query<&ChildOf>, mut commands: Commands) {
    click.propagate(false);
    if let Ok(p) = parents.get(click.entity) {
        commands.trigger(ToolDropdown { entity: p.parent() });
    }
}

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
/// end lists them in a menu; choosing one activates its button.
#[derive(Component, Debug, Default)]
pub struct ToolbarOverflow;

/// The » button of a [`ToolbarOverflow`] row.
#[derive(Component)]
struct OverflowButton;

/// A child a [`ToolbarOverflow`] row hid.
#[derive(Component, Debug, Clone, Copy)]
struct Overflowed;

/// The width a child took when last shown.
#[derive(Component, Debug, Clone, Copy)]
struct ShownWidth(f32);

pub(crate) struct ToolbarPlugin;

impl Plugin for ToolbarPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, fit_toolbars.after(bevy::ui::UiSystems::Layout))
            .add_observer(on_overflow);
    }
}

fn px(v: Val) -> f32 {
    match v {
        Val::Px(p) => p,
        _ => 0.0,
    }
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
        // The » goes last (a toolbar rebuilt for another tab drops it).
        if children.last() != button.as_ref() || button.is_none() {
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
                .id();
            commands.entity(row).add_child(b);
            continue;
        }
        let Some(button) = button else { continue };
        let k = computed.inverse_scale_factor();
        let gap = px(row_node.column_gap);
        let available =
            computed.size().x * k - px(row_node.padding.left) - px(row_node.padding.right);
        // Each tool's width: as laid out when shown, else as last shown.
        let mut widths = Vec::new();
        for &c in children.iter().filter(|c| **c != button) {
            let Ok((cn, node, shown, _)) = items.get(c) else {
                continue;
            };
            let w = if node.display == Display::None {
                shown.map_or(0.0, |s| s.0)
            } else {
                cn.size().x * k + px(node.margin.left) + px(node.margin.right)
            };
            if node.display != Display::None && shown.is_none_or(|s| (s.0 - w).abs() > 0.5) {
                commands.entity(c).insert(ShownWidth(w));
            }
            widths.push((c, w));
        }
        let all: f32 = widths.iter().map(|(_, w)| w + gap).sum();
        let fits = all <= available + 0.5;
        let room = available - if fits { 0.0 } else { 32.0 + gap };
        let mut used = 0.0;
        let mut hiding = false;
        for (c, w) in widths {
            hiding |= !fits && used + w > room;
            used += w + gap;
            let Ok((_, mut node, _, _)) = items.get_mut(c) else {
                continue;
            };
            let display = if hiding { Display::None } else { Display::Flex };
            if node.display != display {
                node.display = display;
                if hiding {
                    commands.entity(c).insert(Overflowed);
                } else {
                    commands.entity(c).remove::<Overflowed>();
                }
            }
        }
        if let Ok((_, mut node, _, _)) = items.get_mut(button) {
            let display = if fits { Display::None } else { Display::Flex };
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
    ),
    With<Overflowed>,
>;

/// The »: a menu of the hidden tools (by their tooltips); choosing one activates it.
fn on_overflow(
    a: On<bevy::ui_widgets::Activate>,
    buttons: Query<&ChildOf, With<OverflowButton>>,
    rows: Query<&Children, With<ToolbarOverflow>>,
    hidden: HiddenTools,
    at: Query<(&ComputedNode, &UiGlobalTransform)>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    let Ok(row) = buttons.get(a.entity).map(|p| p.parent()) else {
        return;
    };
    let Ok(children) = rows.get(row) else { return };
    let mut menu = crate::menu::Menu::new("toolbar-overflow-menu").min_width(180.0);
    let mut targets = Vec::new();
    for &c in children {
        let Ok((name, tip, disabled)) = hidden.get(c) else {
            continue;
        };
        // Tools only (the search box is reached with Alt+C).
        let Some(label) = tip.and_then(|t| t.0.clone()) else {
            continue;
        };
        menu = menu.item(
            crate::menu::MenuItem::new(format!("toolbar-overflow-{}", name.as_str()), label)
                .disabled(disabled),
        );
        targets.push((format!("toolbar-overflow-{}", name.as_str()), c));
    }
    let pos = at.get(a.entity).map_or(Vec2::ZERO, |(n, t)| {
        let s = n.inverse_scale_factor();
        let size = n.size() * s;
        t.translation * s + Vec2::new(-size.x / 2.0, size.y / 2.0 + 2.0)
    });
    let anchor = crate::menu::open_context_menu(&mut commands, pos, menu.build(&theme));
    commands.entity(anchor).observe(
        move |ev: On<crate::menu::MenuAction>, mut commands: Commands| {
            if let Some((_, e)) = targets.iter().find(|(n, _)| *n == ev.item) {
                commands.trigger(bevy::ui_widgets::Activate { entity: *e });
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
