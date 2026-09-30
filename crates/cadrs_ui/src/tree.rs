//! Tree rows, modeled on gpui-component's `Tree`/`ListItem` and Onshape's feature list: 24 px
//! rows with an indent, an optional disclosure chevron, a small grey icon and a 13 px label.
//! Clicking a row triggers `bevy::ui_widgets::Activate` on it; clicking its chevron triggers
//! [`TreeToggle`] instead.
//!
//! [`tree_guide`] draws the thin vertical line Onshape shows along a group's children.
//!
//! A row can have a trailing toggle ([`TreeItem::toggle`], Onshape's show/hide eye on a feature
//! row): it shows while the row is hovered, and clicking it triggers [`TreeRowToggled`] on the
//! row instead of activating it. The owner keeps [`TreeRowToggle::on`] up to date.
//!
//! A row can be renamed in place ([`TreeItem::editable`]): its label is an
//! [`crate::InlineEditLabel`] of an [`crate::InlineEdit`] row, so [`crate::begin_inline_edit`]
//! swaps it for a text field.

use std::borrow::Cow;

use bevy::ecs::spawn::SpawnWith;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::Button as WidgetButton;

use crate::icon::icon;
use crate::style::{InheritFg, InitState, StateColors, VisualState, Visuals};
use crate::theme::Theme;

pub struct TreePlugin;

impl Plugin for TreePlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_chevron_press)
            .add_observer(on_toggle_click)
            .add_systems(Update, sync_row_toggles);
    }
}

/// The disclosure chevron of a tree row was clicked. Targets the row.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct TreeToggle {
    pub entity: Entity,
}

/// Marks a row's chevron; points at the row.
#[derive(Component, Debug, Clone, Copy)]
struct TreeChevron(Entity);

/// A row's trailing toggle (the eye): `on` picks its icon (`on_icon` or `off_icon`).
#[derive(Component, Debug, Clone)]
pub struct TreeRowToggle {
    pub row: Entity,
    pub on: bool,
    on_icon: Cow<'static, str>,
    off_icon: Cow<'static, str>,
}

/// A row's trailing toggle was clicked. Targets the row.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct TreeRowToggled {
    pub entity: Entity,
}

/// A row toggle as built: its name, on and off icons, and whether it is on.
type RowToggleSpec = (Cow<'static, str>, Cow<'static, str>, Cow<'static, str>, bool);

/// Builder for a tree row.
pub struct TreeItem {
    name: Cow<'static, str>,
    label: String,
    icon: Option<(Cow<'static, str>, f32)>,
    icon_color: Option<Color>,
    depth: u32,
    disclosure: Option<bool>,
    selected: bool,
    disabled: bool,
    muted: bool,
    force: Option<VisualState>,
    weight: FontWeight,
    height: f32,
    indent: f32,
    left: f32,
    badge: Option<Color>,
    toggle: Option<RowToggleSpec>,
    toggle_tip: Option<String>,
    editable: bool,
    middle: bool,
    strikethrough: bool,
    italic: bool,
    foreground: Option<Color>,
    trailing: Option<(String, Color)>,
}

impl TreeItem {
    pub fn new(name: impl Into<Cow<'static, str>>, label: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            icon: None,
            icon_color: None,
            depth: 0,
            disclosure: None,
            selected: false,
            disabled: false,
            muted: false,
            force: None,
            weight: FontWeight::MEDIUM,
            height: 24.0,
            indent: 20.0,
            left: 4.0,
            badge: None,
            toggle: None,
            toggle_tip: None,
            editable: false,
            middle: false,
            strikethrough: false,
            italic: false,
            foreground: None,
            trailing: None,
        }
    }

    /// The label's colour (default: the theme's foreground, or subtle while muted).
    pub fn foreground(mut self, c: Color) -> Self {
        self.foreground = Some(c);
        self
    }

    /// The label in italics (a feature below the rollback bar or the one being edited,
    /// P3D.1 / IR5.4).
    pub fn italic(mut self, on: bool) -> Self {
        self.italic = on;
        self
    }

    /// The label struck through (a suppressed feature, P3.9).
    pub fn strikethrough(mut self, on: bool) -> Self {
        self.strikethrough = on;
        self
    }

    /// A short right-aligned text after the label, in `color` (a feature's regeneration time,
    /// P3.9), named `<row>-trailing`.
    pub fn trailing(mut self, text: Option<String>, color: Color) -> Self {
        self.trailing = text.map(|t| (t, color));
        self
    }

    /// A tooltip on the trailing toggle ("Show Sketch 1").
    pub fn toggle_tooltip(mut self, tip: impl Into<String>) -> Self {
        self.toggle_tip = Some(tip.into());
        self
    }

    /// A trailing toggle named `name` (shown while the row is hovered), with `on_icon` when
    /// `on` and `off_icon` otherwise; clicking it triggers [`TreeRowToggled`].
    pub fn toggle(
        mut self,
        name: impl Into<Cow<'static, str>>,
        on_icon: impl Into<Cow<'static, str>>,
        off_icon: impl Into<Cow<'static, str>>,
        on: bool,
    ) -> Self {
        self.toggle = Some((name.into(), on_icon.into(), off_icon.into(), on));
        self
    }

    /// A long label is cut in the middle ("Mate connector of Rear…mount").
    pub fn middle_ellipsis(mut self) -> Self {
        self.middle = true;
        self
    }

    /// The label can be edited in place (renaming a feature).
    pub fn editable(mut self) -> Self {
        self.editable = true;
        self
    }

    /// A leading icon at `size` px (Onshape uses 16 px plane icons and a 10 px origin dot).
    pub fn icon(mut self, name: impl Into<Cow<'static, str>>, size: f32) -> Self {
        self.icon = Some((name.into(), size));
        self
    }

    /// The icon's color (default: the muted grey Onshape uses for feature icons).
    pub fn icon_color(mut self, c: Color) -> Self {
        self.icon_color = Some(c);
        self
    }

    /// Nesting level; each level indents by [`TreeItem::indent`].
    pub fn depth(mut self, d: u32) -> Self {
        self.depth = d;
        self
    }

    pub fn indent(mut self, px: f32) -> Self {
        self.indent = px;
        self
    }

    /// An 8 px status badge (a filled circle of `color` with a white "−") at the icon's
    /// bottom right, like Onshape's under-defined sketch badge (`screens/14`).
    pub fn icon_badge(mut self, color: Option<Color>) -> Self {
        self.badge = color;
        self
    }

    /// Left padding before the first level.
    pub fn left(mut self, px: f32) -> Self {
        self.left = px;
        self
    }

    /// Shows a chevron: `Some(true)` expanded (▾), `Some(false)` collapsed (▸).
    pub fn disclosure(mut self, open: Option<bool>) -> Self {
        self.disclosure = open;
        self
    }

    pub fn selected(mut self, s: bool) -> Self {
        self.selected = s;
        self
    }

    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
        self
    }

    /// Grey text, for items such as an assembly's origin.
    pub fn muted(mut self, m: bool) -> Self {
        self.muted = m;
        self
    }

    pub fn weight(mut self, w: FontWeight) -> Self {
        self.weight = w;
        self
    }

    pub fn height(mut self, h: f32) -> Self {
        self.height = h;
        self
    }

    pub fn force_state(mut self, s: VisualState) -> Self {
        self.force = Some(s);
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let TreeItem {
            name,
            label,
            icon: icon_spec,
            icon_color,
            depth,
            disclosure,
            selected,
            disabled,
            muted,
            force,
            weight,
            height,
            indent,
            left,
            badge,
            toggle,
            toggle_tip,
            editable,
            middle,
            strikethrough,
            italic,
            foreground,
            trailing,
        } = self;
        let trailing_name = format!("{name}-trailing");
        let chevron_name = format!("{name}-chevron");
        let fg = foreground.unwrap_or(if muted {
            theme.subtle_foreground
        } else {
            theme.foreground
        });
        let visuals = Visuals {
            background: StateColors::new(
                Color::NONE,
                theme.list_hover,
                theme.list_active,
                Color::NONE,
            )
            .with_selected(theme.list_selected),
            border: StateColors::all(Color::NONE),
            foreground: StateColors::new(fg, fg, fg, theme.disabled_foreground),
            focus_ring: theme.focus_ring,
        };
        let icon_color = icon_color.unwrap_or(theme.feature_icon);
        let theme_icon = theme.foreground;
        let chevron_color = theme.foreground;
        // 12 px: Onshape's feature rows ("Extrude 1" is about 52 px wide in `screens/22`).
        let mut font = theme.font(theme.font_sm, weight);
        if italic {
            font.style = bevy::text::FontStyle::Italic;
        }
        let font_trailing = theme.font(theme.font_xs, FontWeight::MEDIUM);
        let pad = left + indent * depth as f32;
        (
            Name::new(name.into_owned()),
            Node {
                height: Val::Px(height),
                flex_shrink: 0.0,
                padding: UiRect::left(Val::Px(pad)),
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..default()
            },
            WidgetButton,
            Hovered::default(),
            visuals,
            InitState {
                disabled,
                selected,
                force,
            },
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                let row = p.target_entity();
                if let Some(open) = disclosure {
                    p.spawn((
                        Name::new(chevron_name),
                        TreeChevron(row),
                        Node {
                            width: Val::Px(14.0),
                            height: Val::Px(height),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        children![(
                            icon(
                                if open {
                                    "chevron-down-medium"
                                } else {
                                    "chevron-right-medium"
                                },
                                18.0,
                                chevron_color
                            ),
                            Pickable::IGNORE,
                        )],
                    ));
                }
                if let Some((i, size)) = icon_spec {
                    // Icons sit in a 16 px column so labels line up.
                    let mut col = p.spawn((
                        Node {
                            width: Val::Px(16.0_f32.max(size)),
                            justify_content: JustifyContent::Center,
                            ..default()
                        },
                        Pickable::IGNORE,
                        children![(icon(i, size, icon_color), Pickable::IGNORE)],
                    ));
                    if let Some(c) = badge {
                        col.with_child((
                            Name::new("tree-icon-badge"),
                            Node {
                                position_type: PositionType::Absolute,
                                right: Val::Px(-3.0),
                                bottom: Val::Px(-3.0),
                                width: Val::Px(8.0),
                                height: Val::Px(8.0),
                                border_radius: BorderRadius::MAX,
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                ..default()
                            },
                            BackgroundColor(c),
                            Pickable::IGNORE,
                            children![(
                                Node {
                                    width: Val::Px(4.0),
                                    height: Val::Px(1.5),
                                    ..default()
                                },
                                BackgroundColor(Color::WHITE),
                                Pickable::IGNORE,
                            )],
                        ));
                    }
                }
                // A label too long for the row ends in "…" (a hole's callout, P3.6).
                let mut text = p.spawn((
                    Text::new(label),
                    font,
                    TextColor(fg),
                    TextLayout::no_wrap(),
                    InheritFg,
                    Pickable::IGNORE,
                    crate::ellipsis::Ellipsis::node(),
                    if middle { crate::ellipsis::Ellipsis::middle() } else { crate::ellipsis::Ellipsis::default() },
                ));
                if editable {
                    text.insert(crate::InlineEditLabel);
                }
                if strikethrough {
                    text.insert(bevy::text::Strikethrough);
                }
                let has_trailing = trailing.is_some();
                if let Some((t, color)) = trailing {
                    p.spawn((
                        Name::new(trailing_name),
                        Text::new(t),
                        font_trailing.clone(),
                        TextColor(color),
                        TextLayout::no_wrap(),
                        Node {
                            margin: UiRect::new(Val::Auto, Val::Px(6.0), Val::ZERO, Val::ZERO),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        Pickable::IGNORE,
                        crate::inline_edit::InlineEditHide,
                    ));
                }
                if let Some((name, on_icon, off_icon, on)) = toggle {
                    let current = if on { on_icon.clone() } else { off_icon.clone() };
                    let mut toggle_entity = p.spawn((
                        Name::new(name.into_owned()),
                        TreeRowToggle {
                            row,
                            on,
                            on_icon,
                            off_icon,
                        },
                        Node {
                            width: Val::Px(20.0),
                            height: Val::Px(height),
                            margin: UiRect::new(
                                if has_trailing { Val::ZERO } else { Val::Auto },
                                Val::Px(4.0),
                                Val::ZERO,
                                Val::ZERO,
                            ),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            flex_shrink: 0.0,
                            ..default()
                        },
                        Visibility::Hidden,
                        crate::inline_edit::InlineEditHide,
                        children![(icon(current, 16.0, theme_icon), Pickable::IGNORE)],
                    ));
                    if let Some(tip) = toggle_tip {
                        toggle_entity.insert(crate::Tooltip::new(tip));
                    }
                }
            })),
            // Only an editable row's label can be swapped for a field.
            crate::InlineEdit::default(),
        )
    }
}

fn on_toggle_click(
    mut click: On<Pointer<Click>>,
    q: Query<&TreeRowToggle>,
    mut commands: Commands,
) {
    if let Ok(t) = q.get(click.entity) {
        click.propagate(false);
        commands.trigger(TreeRowToggled { entity: t.row });
    }
}

/// Shows each row toggle while its row is hovered, with the icon for its state.
fn sync_row_toggles(
    mut q: Query<(&TreeRowToggle, &mut Visibility, &Children)>,
    q_row: Query<&Hovered>,
    mut q_icon: Query<&mut crate::Icon>,
) {
    for (t, mut vis, children) in &mut q {
        let hovered = q_row.get(t.row).is_ok_and(|h| h.get());
        vis.set_if_neq(if hovered {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        let want = if t.on { &t.on_icon } else { &t.off_icon };
        for c in children.iter() {
            if let Ok(mut i) = q_icon.get_mut(c)
                && i.name != *want
            {
                i.name = want.clone();
            }
        }
    }
}

fn on_chevron_press(
    mut click: On<Pointer<Click>>,
    q: Query<&TreeChevron>,
    mut commands: Commands,
) {
    if let Ok(c) = q.get(click.entity) {
        click.propagate(false);
        commands.trigger(TreeToggle { entity: c.0 });
    }
}

/// The thin vertical guide line along a tree group's children, `left` px from the group's left
/// edge, from `top` to `height` px.
pub fn tree_guide(theme: &Theme, left: f32, top: f32, height: f32) -> impl Bundle {
    (
        Name::new("tree-guide"),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(left),
            top: Val::Px(top),
            width: Val::Px(1.0),
            height: Val::Px(height),
            ..default()
        },
        BackgroundColor(theme.tree_guide),
        Pickable::IGNORE,
    )
}
