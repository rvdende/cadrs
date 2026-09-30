//! A collapsible side panel, modeled on gpui-component's `Dock` (left placement) and Onshape's
//! feature list: a fixed-width bordered panel with a small toggle tab on its right edge, about
//! halfway down. Clicking the tab (or triggering [`ToggleDockPanel`] on the panel) collapses the
//! panel to just the tab; the panel then triggers [`DockPanelToggled`].
//!
//! The panel root gets the builder's name, the body `<name>-body` and the tab `<name>-toggle`.

use std::borrow::Cow;

use bevy::ecs::spawn::SpawnWith;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button as WidgetButton};

use crate::icon::icon;
use crate::style::{StateColors, Visuals};
use crate::theme::Theme;

pub struct DockPlugin;

impl Plugin for DockPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_tab_activate)
            .add_observer(on_toggle)
            .add_systems(PostUpdate, sync_docks.before(bevy::ui::UiSystems::Prepare));
    }
}

/// Whether a dock panel is open.
#[derive(Component, Debug, Clone, Copy)]
pub struct DockPanelState {
    pub open: bool,
    width: f32,
}

impl DockPanelState {
    pub fn width(&self) -> f32 {
        self.width
    }

    /// Changes the panel's width (px).
    pub fn set_width(&mut self, w: f32) {
        self.width = w;
    }
}

/// Ask a dock panel to open or close. Trigger it on the panel root.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct ToggleDockPanel {
    pub entity: Entity,
}

/// A dock panel was opened or closed.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct DockPanelToggled {
    pub entity: Entity,
    pub open: bool,
}

#[derive(Component, Debug, Clone, Copy)]
struct DockBody(Entity);

#[derive(Component, Debug, Clone, Copy)]
struct DockTab(Entity);

type SpawnFn = Box<dyn FnOnce(&mut ChildSpawner) + Send + Sync>;

/// Builder for a dock panel.
pub struct DockPanel {
    name: Cow<'static, str>,
    width: f32,
    open: bool,
    tab_icon: Cow<'static, str>,
    content: Option<SpawnFn>,
}

impl DockPanel {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            width: 190.0,
            open: true,
            tab_icon: "list-details".into(),
            content: None,
        }
    }

    pub fn width(mut self, w: f32) -> Self {
        self.width = w;
        self
    }

    pub fn open(mut self, o: bool) -> Self {
        self.open = o;
        self
    }

    /// The icon on the toggle tab.
    pub fn tab_icon(mut self, i: impl Into<Cow<'static, str>>) -> Self {
        self.tab_icon = i.into();
        self
    }

    /// Spawns the panel's content (laid out in a column).
    pub fn content(mut self, f: impl FnOnce(&mut ChildSpawner) + Send + Sync + 'static) -> Self {
        self.content = Some(Box::new(f));
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let theme = theme.clone();
        let DockPanel {
            name,
            width,
            open,
            tab_icon,
            content,
        } = self;
        let body_name = format!("{name}-body");
        let tab_name = format!("{name}-toggle");
        (
            Name::new(name.into_owned()),
            DockPanelState { open, width },
            Node {
                width: Val::Px(if open { width } else { 0.0 }),
                flex_shrink: 0.0,
                ..default()
            },
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                let root = p.target_entity();
                let mut body = p.spawn((
                    Name::new(body_name),
                    DockBody(root),
                    Node {
                        width: Val::Px(width),
                        min_height: Val::Px(0.0),
                        flex_direction: FlexDirection::Column,
                        border: UiRect::new(Val::ZERO, Val::Px(1.0), Val::Px(1.0), Val::ZERO),
                        overflow: Overflow::clip(),
                        display: if open { Display::Flex } else { Display::None },
                        ..default()
                    },
                    BackgroundColor(theme.background),
                    BorderColor::all(theme.panel_border),
                ));
                if let Some(f) = content {
                    body.with_children(|b| f(b));
                }
                p.spawn((
                    Name::new(tab_name),
                    DockTab(root),
                    WidgetButton,
                    Hovered::default(),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Percent(100.0),
                        top: Val::Percent(50.0),
                        width: Val::Px(28.0),
                        height: Val::Px(32.0),
                        margin: UiRect::top(Val::Px(-16.0)),
                        border: UiRect::new(Val::ZERO, Val::Px(1.0), Val::Px(1.0), Val::Px(1.0)),
                        border_radius: BorderRadius::right(Val::Px(theme.radius)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    Visuals {
                        background: StateColors::new(
                            theme.background,
                            theme.ghost_hover,
                            theme.ghost_active,
                            theme.background,
                        ),
                        border: StateColors::all(theme.panel_border),
                        foreground: StateColors::all(theme.tool_foreground),
                        focus_ring: theme.focus_ring,
                    },
                    crate::tooltip::Tooltip::new("Toggle panel"),
                    children![(
                        icon(tab_icon, 18.0, theme.tool_foreground),
                        Pickable::IGNORE
                    )],
                ));
            })),
        )
    }
}

fn on_tab_activate(ev: On<Activate>, q: Query<&DockTab>, mut commands: Commands) {
    if let Ok(tab) = q.get(ev.entity) {
        commands.trigger(ToggleDockPanel { entity: tab.0 });
    }
}

fn on_toggle(ev: On<ToggleDockPanel>, mut q: Query<&mut DockPanelState>, mut commands: Commands) {
    if let Ok(mut s) = q.get_mut(ev.entity) {
        s.open = !s.open;
        commands.trigger(DockPanelToggled {
            entity: ev.entity,
            open: s.open,
        });
    }
}

fn sync_docks(
    mut q: Query<(Entity, &DockPanelState, &mut Node), Changed<DockPanelState>>,
    mut q_body: Query<(&DockBody, &mut Node), Without<DockPanelState>>,
) {
    for (root, state, mut node) in &mut q {
        node.width = Val::Px(if state.open { state.width } else { 0.0 });
        for (b, mut n) in &mut q_body {
            if b.0 == root {
                n.width = Val::Px(state.width);
                n.display = if state.open {
                    Display::Flex
                } else {
                    Display::None
                };
            }
        }
    }
}
