//! A row of text tabs with a blue underline on the selected one, like gpui-component's
//! `TabBar` in its underline variant and the tabs of Onshape's keyboard shortcuts dialog
//! (`reference/onshape/shortcuts/keyboard-shortcut-dialog-03.png`: General | Part Studio | …,
//! the active tab in blue with a 2 px underline).
//!
//! Each tab is a button named `<name>-<index>`. Clicking one selects it and triggers
//! [`TabStripSelect`] on the strip.
//!
//! [`TabStrip::compact`] gives the denser style of Onshape's feature dialogs
//! (`reference/onshape/screens/22`: Solid | Surface | Thin, New | Add | Remove | Intersect):
//! 25 px rows, 12 px text, tabs sharing the width, a 1 px divider under the row. Tabs can be
//! disabled (greyed, not clickable).

use std::borrow::Cow;

use bevy::ecs::spawn::SpawnWith;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, Button as WidgetButton};

use crate::style::{InheritFg, InitState, Selected, StateColors, Visuals};
use crate::theme::Theme;

pub struct TabStripPlugin;

impl Plugin for TabStripPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_tab_activate);
    }
}

/// A tab of a strip was chosen. Targets the strip.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct TabStripSelect {
    pub entity: Entity,
    pub index: usize,
}

/// A tab strip and its selected tab.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabStripState {
    pub selected: usize,
}

/// A tab's text (semibold while selected).
#[derive(Component)]
struct StripTabLabel;

/// One tab of a strip.
#[derive(Component, Debug, Clone, Copy)]
struct StripTab {
    strip: Entity,
    index: usize,
}

/// Builder for a tab strip.
pub struct TabStrip {
    name: Cow<'static, str>,
    labels: Vec<String>,
    selected: usize,
    disabled: Vec<usize>,
    compact: bool,
    equal: bool,
}

impl TabStrip {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            labels: Vec::new(),
            selected: 0,
            disabled: Vec::new(),
            compact: false,
            equal: false,
        }
    }

    /// Every tab the same width (P3G.4: the Move to document and Derived dialogs' source tabs).
    pub fn equal(mut self) -> Self {
        self.equal = true;
        self
    }

    /// Greys out a tab (it cannot be chosen).
    pub fn disabled(mut self, index: usize) -> Self {
        self.disabled.push(index);
        self
    }

    /// The feature-dialog style: short rows, small text, tabs sharing the width and a divider
    /// line under them.
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    pub fn tab(mut self, label: impl Into<String>) -> Self {
        self.labels.push(label.into());
        self
    }

    pub fn selected(mut self, i: usize) -> Self {
        self.selected = i;
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let t = theme.clone();
        let TabStrip {
            name,
            labels,
            selected,
            disabled,
            compact,
            equal,
        } = self;
        let prefix = name.to_string();
        let (height, gap, font_size) = if compact { (25.0, 0.0, 12.0) } else { (36.0, 8.0, 14.0) };
        (
            Name::new(name.into_owned()),
            TabStripState { selected },
            Node {
                height: Val::Px(height),
                flex_shrink: 0.0,
                column_gap: Val::Px(gap),
                align_items: AlignItems::Stretch,
                border: UiRect::bottom(Val::Px(if compact { 1.0 } else { 0.0 })),
                ..default()
            },
            BorderColor::all(Color::srgb_u8(0xe1, 0xe1, 0xe1)),
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                let strip = p.target_entity();
                for (index, label) in labels.into_iter().enumerate() {
                    let normal_fg = if compact { t.tool_foreground } else { t.foreground };
                    let visuals = Visuals {
                        background: StateColors::all(Color::NONE),
                        border: StateColors::all(Color::NONE).with_selected(t.link),
                        // Compact (dialog) tabs that are not available keep their dark
                        // text, as Onshape draws them (`screens/22`); they just do nothing.
                        foreground: StateColors::new(
                            normal_fg,
                            t.link,
                            t.link,
                            if compact { normal_fg } else { t.disabled_foreground },
                        )
                        .with_selected(t.link),
                        focus_ring: t.focus_ring,
                    };
                    let mut tab = p.spawn((
                        Name::new(format!("{prefix}-{index}")),
                        StripTab { strip, index },
                        Node {
                            padding: UiRect::new(
                                Val::Px(5.0),
                                Val::Px(5.0),
                                Val::Px(0.0),
                                Val::Px(0.0),
                            ),
                            border: UiRect::bottom(Val::Px(2.0)),
                            // The compact rows sit on the divider line.
                            margin: UiRect::bottom(Val::Px(if compact { -1.0 } else { 0.0 })),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            flex_grow: if compact || equal { 1.0 } else { 0.0 },
                            flex_basis: if equal { Val::Px(0.0) } else { Val::Auto },
                            ..default()
                        },
                        WidgetButton,
                        Hovered::default(),
                        visuals,
                        InitState {
                            disabled: disabled.contains(&index),
                            selected: index == selected,
                            force: None,
                        },
                        children![(
                            StripTabLabel,
                            Text::new(label),
                            t.font(
                                font_size,
                                if index == selected {
                                    FontWeight::SEMIBOLD
                                } else {
                                    FontWeight::NORMAL
                                },
                            ),
                            TextColor(t.foreground),
                            TextLayout::no_wrap(),
                            InheritFg,
                            Pickable::IGNORE,
                        )],
                    ));
                    if disabled.contains(&index) {
                        tab.insert(crate::Tooltip::new("Not available yet"));
                    }
                }
            })),
        )
    }
}

fn on_tab_activate(
    ev: On<Activate>,
    q_tab: Query<&StripTab>,
    q_all: Query<(Entity, &StripTab, Has<Selected>, &Children)>,
    mut q_state: Query<&mut TabStripState>,
    mut q_label: Query<&mut TextFont, With<StripTabLabel>>,
    mut commands: Commands,
) {
    let Ok(tab) = q_tab.get(ev.entity).copied() else {
        return;
    };
    for (e, t, selected, children) in &q_all {
        if t.strip != tab.strip {
            continue;
        }
        let weight = if t.index == tab.index {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        };
        for c in children.iter() {
            if let Ok(mut f) = q_label.get_mut(c)
                && f.weight != weight
            {
                f.weight = weight;
            }
        }
        if t.index == tab.index && !selected {
            commands.entity(e).insert(Selected);
        } else if t.index != tab.index && selected {
            commands.entity(e).remove::<Selected>();
        }
    }
    if let Ok(mut s) = q_state.get_mut(tab.strip)
        && s.selected != tab.index
    {
        s.selected = tab.index;
        commands.trigger(TabStripSelect {
            entity: tab.strip,
            index: tab.index,
        });
    }
}
