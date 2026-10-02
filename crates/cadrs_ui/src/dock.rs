//! A collapsible side panel, modeled on gpui-component's `Dock` (left placement) and Onshape's
//! feature list: a fixed-width bordered panel with a small toggle tab on its right edge, about
//! halfway down. Clicking the tab (or triggering [`ToggleDockPanel`] on the panel) collapses the
//! panel to just the tab; the panel then triggers [`DockPanelToggled`].
//!
//! Its right edge is a divider: hovering it shows the column-resize cursor and dragging it sets
//! the panel's width (within its minimum and maximum), so long names fit; a finished drag
//! triggers [`DockPanelResized`].
//!
//! The panel root gets the builder's name, the body `<name>-body`, the tab `<name>-toggle` and
//! the divider `<name>-resize`.

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
            .add_observer(on_grip_drag_start)
            .add_observer(on_grip_drag)
            .add_observer(on_grip_drag_end)
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

/// A dock panel's width was dragged to `width` px. Triggered on the panel root.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct DockPanelResized {
    pub entity: Entity,
    pub width: f32,
}

/// The least and greatest width (px) a dock panel can be dragged to.
pub const MIN_WIDTH: f32 = 150.0;
pub const MAX_WIDTH: f32 = 640.0;

/// The divider on a dock panel's right edge: the panel it resizes, and the drag in progress.
#[derive(Component, Debug, Clone, Copy)]
pub struct DockGrip {
    panel: Entity,
    start: f32,
    dragging: bool,
}

impl DockGrip {
    /// Whether the divider is being dragged.
    pub fn dragging(&self) -> bool {
        self.dragging
    }
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
        let grip_name = format!("{name}-resize");
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
                // The divider over the body's right border (1 px), a few px wider to grab.
                {
                    p.spawn((
                        Name::new(grip_name),
                        DockGrip {
                            panel: root,
                            start: 0.0,
                            dragging: false,
                        },
                        crate::splitter::HoverCursor(crate::cursor::CursorKind::ColResize),
                        Hovered::default(),
                        Node {
                            position_type: PositionType::Absolute,
                            top: Val::Px(0.0),
                            bottom: Val::Px(0.0),
                            right: Val::Px(-4.0),
                            width: Val::Px(7.0),
                            display: if open { Display::Flex } else { Display::None },
                            ..default()
                        },
                        // Transparent, but picked (a node without a background is still hit).
                        Pickable::default(),
                        ZIndex(10),
                    ));
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

fn on_grip_drag_start(
    mut ev: On<Pointer<DragStart>>,
    mut q: Query<&mut DockGrip>,
    q_state: Query<&DockPanelState>,
) {
    let Ok(mut g) = q.get_mut(ev.entity) else {
        return;
    };
    ev.propagate(false);
    let Ok(s) = q_state.get(g.panel) else { return };
    g.start = s.width;
    g.dragging = true;
}

fn on_grip_drag(
    mut ev: On<Pointer<Drag>>,
    q: Query<&DockGrip>,
    mut q_state: Query<&mut DockPanelState>,
) {
    let Ok(g) = q.get(ev.entity) else { return };
    ev.propagate(false);
    if !g.dragging {
        return;
    }
    let w = (g.start + ev.distance.x)
        .clamp(MIN_WIDTH, MAX_WIDTH)
        .round();
    if let Ok(mut s) = q_state.get_mut(g.panel)
        && s.width != w
    {
        s.width = w;
    }
}

fn on_grip_drag_end(
    mut ev: On<Pointer<DragEnd>>,
    mut q: Query<&mut DockGrip>,
    q_state: Query<&DockPanelState>,
    mut commands: Commands,
) {
    let Ok(mut g) = q.get_mut(ev.entity) else {
        return;
    };
    ev.propagate(false);
    if !g.dragging {
        return;
    }
    g.dragging = false;
    if let Ok(s) = q_state.get(g.panel) {
        commands.trigger(DockPanelResized {
            entity: g.panel,
            width: s.width,
        });
    }
}

/// A grip's node, kept apart from the panel's and the body's.
type GripNodes<'w, 's> = Query<
    'w,
    's,
    (&'static DockGrip, &'static mut Node),
    (Without<DockPanelState>, Without<DockBody>),
>;

fn sync_docks(
    mut q: Query<(Entity, &DockPanelState, &mut Node), Changed<DockPanelState>>,
    mut q_body: Query<(&DockBody, &mut Node), Without<DockPanelState>>,
    mut q_grip: GripNodes,
) {
    for (root, state, mut node) in &mut q {
        node.width = Val::Px(if state.open { state.width } else { 0.0 });
        for (g, mut n) in &mut q_grip {
            if g.panel == root {
                n.display = if state.open {
                    Display::Flex
                } else {
                    Display::None
                };
            }
        }
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
