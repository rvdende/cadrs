//! A collapsible section, modeled on gpui-component's `Collapsible`/`Accordion`: a header row
//! with a chevron, an optional icon and a label; clicking it shows or hides the content.
//!
//! The root gets the builder's name, the header `<name>-header` and the content
//! `<name>-content`. Toggling triggers [`CollapsibleToggled`] on the root so the owner can
//! remember the state.

use std::borrow::Cow;

use bevy::ecs::spawn::SpawnWith;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, Button as WidgetButton};

use crate::icon::{Icon, icon};
use crate::style::{InheritFg, StateColors, Visuals};
use crate::theme::Theme;

pub struct CollapsiblePlugin;

impl Plugin for CollapsiblePlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_header_activate)
            .add_systems(PostUpdate, sync_collapsibles.before(crate::icon::resolve_icons));
    }
}

/// Whether a collapsible is open.
#[derive(Component, Debug, Clone, Copy)]
pub struct CollapsibleState {
    pub open: bool,
}

/// A collapsible was opened or closed by the user.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct CollapsibleToggled {
    pub entity: Entity,
    pub open: bool,
}

#[derive(Component, Debug, Clone, Copy)]
struct CollapsibleHeader(Entity);

#[derive(Component, Debug, Clone, Copy)]
struct CollapsibleContent(Entity);

#[derive(Component, Debug, Clone, Copy)]
struct CollapsibleChevron(Entity);

type SpawnFn = Box<dyn FnOnce(&mut ChildSpawner) + Send + Sync>;

/// Builder for a collapsible section.
pub struct Collapsible {
    name: Cow<'static, str>,
    label: String,
    icon: Option<Cow<'static, str>>,
    icon_badge: bool,
    open: bool,
    highlight: bool,
    header_height: f32,
    content: Option<SpawnFn>,
}

impl Collapsible {
    pub fn new(name: impl Into<Cow<'static, str>>, label: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            icon: None,
            icon_badge: false,
            open: false,
            highlight: false,
            header_height: 33.0,
            content: None,
        }
    }

    pub fn icon(mut self, i: impl Into<Cow<'static, str>>) -> Self {
        self.icon = Some(i.into());
        self
    }

    /// Draws the icon white on a round accent-colored badge.
    pub fn icon_badge(mut self) -> Self {
        self.icon_badge = true;
        self
    }

    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Gives the header a tinted, rounded background.
    pub fn highlight(mut self, h: bool) -> Self {
        self.highlight = h;
        self
    }

    pub fn header_height(mut self, h: f32) -> Self {
        self.header_height = h;
        self
    }

    pub fn content(mut self, f: impl FnOnce(&mut ChildSpawner) + Send + Sync + 'static) -> Self {
        self.content = Some(Box::new(f));
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let theme = theme.clone();
        let Collapsible {
            name,
            label,
            icon: icon_name,
            icon_badge,
            open,
            highlight,
            header_height,
            content,
        } = self;
        let header_name = format!("{name}-header");
        let content_name = format!("{name}-content");
        (
            Name::new(name.into_owned()),
            CollapsibleState { open },
            Node {
                flex_direction: FlexDirection::Column,
                flex_shrink: 0.0,
                ..default()
            },
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                let root = p.target_entity();
                let fg = theme.foreground;
                let base_bg = if highlight {
                    theme.list_hover
                } else {
                    Color::NONE
                };
                p.spawn((
                    Name::new(header_name),
                    CollapsibleHeader(root),
                    WidgetButton,
                    Hovered::default(),
                    Node {
                        height: Val::Px(header_height),
                        padding: UiRect::horizontal(Val::Px(theme.space[4])),
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(theme.space[4]),
                        border_radius: BorderRadius::all(Val::Px(theme.radius_lg)),
                        ..default()
                    },
                    Visuals {
                        background: StateColors::new(
                            base_bg,
                            theme.list_hover,
                            theme.list_active,
                            base_bg,
                        ),
                        border: StateColors::all(Color::NONE),
                        foreground: StateColors::all(fg),
                        focus_ring: theme.focus_ring,
                    },
                ))
                .with_children(|h| {
                    h.spawn((
                        icon(
                            if open { "chevron-down" } else { "chevron-right" },
                            14.0,
                            fg,
                        ),
                        CollapsibleChevron(root),
                        Pickable::IGNORE,
                    ));
                    if let Some(i) = icon_name {
                        if icon_badge {
                            h.spawn((
                                Node {
                                    width: Val::Px(20.0),
                                    height: Val::Px(20.0),
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    border_radius: BorderRadius::MAX,
                                    ..default()
                                },
                                BackgroundColor(theme.primary),
                                Pickable::IGNORE,
                            ))
                            .with_child((icon(i, 14.0, Color::WHITE), Pickable::IGNORE));
                        } else {
                            h.spawn((icon(i, 16.0, fg), InheritFg, Pickable::IGNORE));
                        }
                    }
                    h.spawn((
                        theme.text(label, theme.font_base, FontWeight::BOLD, fg),
                        InheritFg,
                        Pickable::IGNORE,
                    ));
                });
                let mut c = p.spawn((
                    Name::new(content_name),
                    CollapsibleContent(root),
                    Node {
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::new(
                            Val::Px(34.0),
                            Val::Px(theme.space[4]),
                            Val::Px(theme.space[2]),
                            Val::Px(theme.space[4]),
                        ),
                        row_gap: Val::Px(theme.space[3]),
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

fn on_header_activate(
    ev: On<Activate>,
    q_header: Query<&CollapsibleHeader>,
    mut q_state: Query<&mut CollapsibleState>,
    mut commands: Commands,
) {
    let Ok(header) = q_header.get(ev.entity) else {
        return;
    };
    let Ok(mut state) = q_state.get_mut(header.0) else {
        return;
    };
    state.open = !state.open;
    commands.trigger(CollapsibleToggled {
        entity: header.0,
        open: state.open,
    });
}

fn sync_collapsibles(
    q_changed: Query<(Entity, &CollapsibleState), Changed<CollapsibleState>>,
    mut q_content: Query<(&CollapsibleContent, &mut Node)>,
    mut q_chevron: Query<(&CollapsibleChevron, &mut Icon)>,
) {
    for (root, state) in &q_changed {
        for (c, mut node) in &mut q_content {
            if c.0 == root {
                node.display = if state.open {
                    Display::Flex
                } else {
                    Display::None
                };
            }
        }
        for (c, mut i) in &mut q_chevron {
            if c.0 == root {
                let name = if state.open {
                    "chevron-down"
                } else {
                    "chevron-right"
                };
                if i.name != name {
                    i.name = name.into();
                }
            }
        }
    }
}
