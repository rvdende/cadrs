//! A collapsible side panel, modeled on gpui-component's `Dock` (left placement): a floating
//! glass panel ([`crate::glass`]) with a ‹ in its top right corner that collapses it (as
//! triggering [`ToggleDockPanel`] on the panel does); the panel then triggers
//! [`DockPanelToggled`], and whoever shows a way to open it again (an icon on a rail) listens.
//!
//! A grip in the gap on its right, a short line, is the one line drawn between panels: hovering
//! it shows the column-resize cursor and lights it, and dragging it sets the panel's width
//! (within its minimum and maximum), so long names fit; a finished drag triggers
//! [`DockPanelResized`].
//!
//! The panel root gets the builder's name, the body `<name>-body`, the ‹ `<name>-collapse` and
//! the grip `<name>-resize`.

use std::borrow::Cow;

use bevy::ecs::spawn::SpawnWith;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use crate::theme::Theme;

pub struct DockPlugin;

impl Plugin for DockPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_collapse)
            .add_observer(on_toggle)
            .add_observer(on_grip_drag_start)
            .add_observer(on_grip_drag)
            .add_observer(on_grip_drag_end)
            .add_systems(Update, light_grips)
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

/// The ‹ that collapses the panel.
#[derive(Component, Debug, Clone, Copy)]
struct DockCollapse(Entity);

/// The grip's line.
#[derive(Component, Debug, Clone, Copy)]
struct GripLine;

type SpawnFn = Box<dyn FnOnce(&mut ChildSpawner) + Send + Sync>;

/// Builder for a dock panel.
pub struct DockPanel {
    name: Cow<'static, str>,
    width: f32,
    open: bool,
    gap: f32,
    content: Option<SpawnFn>,
}

impl DockPanel {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            width: 190.0,
            open: true,
            gap: 0.0,
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

    /// The gap kept on the panel's right while it is open (the grip sits in it).
    pub fn gap(mut self, px: f32) -> Self {
        self.gap = px;
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
            gap,
            content,
        } = self;
        let body_name = format!("{name}-body");
        let collapse_name = format!("{name}-collapse");
        let grip_name = format!("{name}-resize");
        (
            Name::new(name.into_owned()),
            DockPanelState { open, width },
            DockGap(gap),
            Node {
                width: Val::Px(if open { width } else { 0.0 }),
                margin: UiRect::right(Val::Px(if open { gap } else { 0.0 })),
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
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(theme.radius_panel)),
                        overflow: Overflow::clip(),
                        display: if open { Display::Flex } else { Display::None },
                        ..default()
                    },
                    crate::glass::panel(&theme),
                ));
                body.with_children(|b| {
                    if let Some(f) = content {
                        f(b);
                    }
                    // The ‹ in the top right corner, over the content's first row (a panel's
                    // filter leaves it room).
                    let mut collapse = b.spawn((
                        crate::button::IconButton::new(collapse_name, "chevron-left")
                            .icon_size(18.0)
                            .tooltip("Hide panel")
                            .build(&theme),
                        DockCollapse(root),
                        ZIndex(5),
                    ));
                    collapse.insert(Node {
                        position_type: PositionType::Absolute,
                        top: Val::Px(COLLAPSE_TOP),
                        right: Val::Px(6.0),
                        width: Val::Px(COLLAPSE_SIZE),
                        height: Val::Px(COLLAPSE_SIZE),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border_radius: BorderRadius::all(Val::Px(theme.radius)),
                        ..default()
                    });
                });
                // The grip, just outside the panel's right edge (its line against the edge), a
                // few px wider than the line to grab.
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
                        right: Val::Px(-GRIP_WIDTH),
                        width: Val::Px(GRIP_WIDTH),
                        padding: UiRect::left(Val::Px(2.0)),
                        justify_content: JustifyContent::FlexStart,
                        align_items: AlignItems::Center,
                        display: if open { Display::Flex } else { Display::None },
                        ..default()
                    },
                    // Transparent, but picked (a node without a background is still hit).
                    Pickable::default(),
                    ZIndex(10),
                    children![(
                        GripLine,
                        Node {
                            width: Val::Px(3.0),
                            height: Val::Px(40.0),
                            border_radius: BorderRadius::all(Val::Px(1.5)),
                            ..default()
                        },
                        BackgroundColor(theme.scrollbar_thumb),
                        Pickable::IGNORE,
                    )],
                ));
            })),
        )
    }
}

/// The ‹'s size and its distance from the panel's top, so it lines up with a 28 px field in a
/// 34 px first row.
pub const COLLAPSE_SIZE: f32 = 26.0;
pub const COLLAPSE_TOP: f32 = 5.0;
/// The room a panel's first row leaves for the ‹ at its right end.
pub const COLLAPSE_ROOM: f32 = COLLAPSE_SIZE + 10.0;

const GRIP_WIDTH: f32 = 8.0;

/// The gap a dock panel keeps on its right while open.
#[derive(Component, Debug, Clone, Copy)]
struct DockGap(f32);

fn on_collapse(ev: On<Activate>, q: Query<&DockCollapse>, mut commands: Commands) {
    if let Ok(c) = q.get(ev.entity) {
        commands.trigger(ToggleDockPanel { entity: c.0 });
    }
}

/// A grip's line lights in the accent while hovered or dragged.
fn light_grips(
    theme: Res<Theme>,
    q: Query<(&DockGrip, &Hovered, &Children)>,
    mut q_line: Query<&mut BackgroundColor, With<GripLine>>,
) {
    for (g, hovered, children) in &q {
        let want = if g.dragging || hovered.get() {
            theme.primary
        } else {
            theme.scrollbar_thumb
        };
        for c in children.iter() {
            if let Ok(mut bg) = q_line.get_mut(c)
                && bg.0 != want
            {
                bg.0 = want;
            }
        }
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
    mut ev: On<PointerDragStart>,
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
    mut ev: On<PointerDrag>,
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
    mut ev: On<PointerDragEnd>,
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
    mut q: Query<(Entity, &DockPanelState, &DockGap, &mut Node), Changed<DockPanelState>>,
    mut q_body: Query<(&DockBody, &mut Node), Without<DockPanelState>>,
    mut q_grip: GripNodes,
) {
    for (root, state, gap, mut node) in &mut q {
        node.width = Val::Px(if state.open { state.width } else { 0.0 });
        node.margin.right = Val::Px(if state.open { gap.0 } else { 0.0 });
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
