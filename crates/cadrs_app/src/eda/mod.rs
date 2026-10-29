//! A native board's Schematic and Layout views in the PCB Studio (docs/PLAN.md, GS2–GS26).
//!
//! A board designed in cadrs (it has a `cadrs_eda::Design`) shows a **Schematic | Layout |
//! 3D** switch at the top of the view. 3D is the studio's own view; Schematic and Layout are
//! drawn here, flat, by a 2D camera on [`EDA_LAYER`] over the 3D scene:
//! `cadrs_eda::render` turns the design into coloured lines and triangles, the triangles become
//! meshes (rebuilt when the design or the selection changes) and the lines gizmos (redrawn each
//! frame, their pixel width following the zoom).
//!
//! Navigation (GS2): the wheel zooms about the pointer, a right- or middle-drag pans, **F**
//! fits. Each board keeps a view per mode. The tools live in [`schematic_tools`] and
//! [`layout_tools`].
//!
//! Names: `eda-mode-schematic`, `eda-mode-layout`, `eda-mode-3d` (the switch).

pub mod browser;
pub(crate) mod lay_dialogs;
pub mod layers_panel;
pub mod layout_tools;
pub mod libraries;
#[cfg(feature = "easyeda")]
pub mod online;
mod part_dialogs;
pub mod part_tools;
mod sch_dialogs;
pub mod schematic_tools;
pub mod ui;

use cadrs_ui::ShortcutKey;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use crate::GizmoGroups;
use bevy::camera::ScalingMode;
use bevy::camera::visibility::RenderLayers;
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::input::mouse::MouseScrollUnit;
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use cadrs_core::ElementId;
use cadrs_core::pcb::BoardId;
use cadrs_eda::Design;
use cadrs_eda::render::{self, DrawList};
use cadrs_eda::units::Bounds;
use cadrs_eda::view::View;
use cadrs_ui::RenderSurface;
use cadrs_ui::prelude::*;

use crate::pcb::{PcbUi, PcbView, active_studio};
use crate::viewport::{ActiveKind, ViewportArea, ViewportRect};
use crate::{ActiveDocument, AppState};

/// The render layer the 2D views draw on.
pub const EDA_LAYER: usize = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Mode {
    #[default]
    Schematic,
    Layout,
    ThreeD,
    /// A component's symbol editor.
    Symbol,
    /// A component's footprint editor.
    Footprint,
}

/// What a view shows: a native board, or a component being edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Subject {
    Board(BoardId),
    Component(cadrs_core::pcb::ComponentId),
}

/// The board or component whose view is shown this frame, and in which mode (`None`: not a
/// native board, or not the PCB Studio). Read by the 3D viewport to keep its hands off in 2D.
/// The second field is set when the Layout's tools run **on a board in the 3D view** instead
/// (a derived board's layout edited in a Part Studio): the
/// view stays 3D and the pointer is mapped onto the board's face
/// ([`crate::board_in_studio`]).
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq)]
pub struct Eda2d(pub Option<(ElementId, Subject, Mode)>, pub LayoutHost);

/// Where the Layout's tools work: the PCB Studio's flat
/// view, or a board in a 3D view (a Part Studio's derived board, an assembly's board instance),
/// the pointer mapped onto its face. Either way they commit `SetDesign` on the PCB Studio.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum LayoutHost {
    #[default]
    Flat,
    Plane(PlaneHost),
}

/// A board copy in a Part Studio whose layout the Layout's tools edit: the copy, its board
/// frame (the layout's origin and axes on its bottom face) and its thickness (mm).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaneHost {
    pub target: crate::board_in_studio::BoardTarget,
    pub frame: cadrs_sketch::PlaneFrame,
    pub thickness: f64,
}

impl Eda2d {
    /// A 2D view (not the 3D one) is on screen.
    pub fn flat(&self) -> bool {
        self.1 == LayoutHost::Flat && matches!(self.0, Some((_, _, m)) if m != Mode::ThreeD)
    }

    /// The Layout's tools run on a board in a Part Studio's 3D view.
    pub fn plane(&self) -> Option<&PlaneHost> {
        match &self.1 {
            LayoutHost::Plane(h) => Some(h),
            LayoutHost::Flat => None,
        }
    }

    /// The shown native board: (tab, board, mode).
    pub fn board(&self) -> Option<(ElementId, BoardId, Mode)> {
        match self.0? {
            (el, Subject::Board(b), m) => Some((el, b, m)),
            _ => None,
        }
    }

    /// The component being edited: (tab, component, mode).
    pub fn component(&self) -> Option<(ElementId, cadrs_core::pcb::ComponentId, Mode)> {
        match self.0? {
            (el, Subject::Component(c), m) => Some((el, c, m)),
            _ => None,
        }
    }
}

/// View state of the 2D views (not saved, not undone).
#[derive(Resource, Default, Debug)]
pub struct EdaUi {
    pub modes: HashMap<(ElementId, Subject), Mode>,
    /// Per subject and mode: the view, and whether it is still the fitted one.
    pub views: HashMap<(ElementId, Subject, Mode), (View, bool)>,
    /// The box each fitted schematic view was fitted to: it stays fitted to that box (as the
    /// area resizes) while what is drawn grows, until fit (F) takes the drawing's box again.
    fit_boxes: HashMap<(ElementId, Subject, Mode), cadrs_eda::units::Bounds>,
    /// The component being edited in a studio (a click on it under Components); a click on a
    /// board goes back to the board.
    pub editing: HashMap<ElementId, cadrs_core::pcb::ComponentId>,
    /// A right- or middle-drag in progress: where the pointer was.
    pan_from: Option<Vec2>,
}

impl EdaUi {
    pub fn mode(&self, el: ElementId, s: Subject) -> Mode {
        self.modes.get(&(el, s)).copied().unwrap_or(match s {
            Subject::Board(_) => Mode::Schematic,
            Subject::Component(_) => Mode::Symbol,
        })
    }
}

/// The marker of the 2D camera.
#[derive(Component)]
pub struct EdaCamera;

/// An entity of the drawn scene (meshes, the ground), despawned on rebuild.
#[derive(Component)]
struct EdaEntity;

/// The lines of the drawn scene and what they were built from.
#[derive(Resource, Default)]
pub struct EdaScene {
    key: Option<u64>,
    pub list: DrawList,
    /// The schematic grid points (in grid steps) a drawn line runs over: the grid isn't drawn
    /// there, so lines don't look dashed with its marks over them.
    covered: std::collections::HashSet<(i64, i64)>,
}

/// The grid points (in steps of `step`) within `reach` of one of `lines`.
fn covered_points(
    lines: &[render::Line],
    step: i64,
    reach: i64,
) -> std::collections::HashSet<(i64, i64)> {
    use cadrs_eda::units::Pt;
    let mut out = std::collections::HashSet::new();
    for l in lines {
        for w in l.pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            let (x0, x1) = (a.x.min(b.x) - reach, a.x.max(b.x) + reach);
            let (y0, y1) = (a.y.min(b.y) - reach, a.y.max(b.y) + reach);
            let (i0, i1) = (x0.div_euclid(step), x1.div_euclid(step));
            let (j0, j1) = (y0.div_euclid(step), y1.div_euclid(step));
            // A long diagonal's box is mostly far from it: skip what would take too long.
            if (i1 - i0 + 1) * (j1 - j0 + 1) > 100_000 {
                continue;
            }
            for i in i0..=i1 {
                for j in j0..=j1 {
                    let p = Pt::new(i * step, j * step);
                    if cadrs_eda::sch_edit::seg_dist(p, a, b) <= reach as f64 {
                        out.insert((i, j));
                    }
                }
            }
        }
    }
    out
}

/// The scene's lines of one width (nm; 0 is the thinnest the canvas draws), as a retained
/// gizmo whose pixel width follows the zoom.
#[derive(Component)]
struct EdaLines(i64);

/// The schematic grid's dots (2 px squares).
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct EdaDots;
/// The placing crosshair (1 px).
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct EdaCross;

/// Where a placing tool will put its point (snapped to the grid), and whether that point
/// connects (it is on a wire or a pin); drawn as a crosshair with a small square, filled when
/// it connects. Set by the tools each frame, `None` when nothing is being placed.
#[derive(Resource, Default, Debug, PartialEq)]
pub struct Crosshair(pub Option<(cadrs_eda::units::Pt, bool)>);

pub struct EdaPlugin;

impl Plugin for EdaPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Eda2d>()
            .init_resource::<EdaUi>()
            .init_resource::<EdaScene>()
            .init_resource::<SceneInputs>()
            .init_resource::<Preview>()
            .init_resource::<EdaPointer>()
            .add_message::<EdaClick>()
            .add_message::<ui::StripAction>()
            .add_observer(ui::on_strip_button)
            .gizmo_group::<EdaDots>()
            .gizmo_group::<EdaCross>()
            .init_resource::<Crosshair>()
            .add_systems(Startup, (spawn_camera, configure_gizmos))
            .add_systems(
                Update,
                (
                    track_context,
                    sync_mode_bar,
                    navigate,
                    fit_views,
                    place_camera,
                    rebuild_scene,
                    set_line_widths,
                    draw_grid_mesh,
                    draw_overlay,
                )
                    .chain()
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(AppState::Document), reset)
            .add_systems(
                Update,
                run_script_commands.run_if(in_state(AppState::Document)),
            )
            .add_observer(on_mode_button);
        schematic_tools::register(app);
        layout_tools::register(app);
        part_tools::register(app);
        libraries::register(app);
        browser::register(app);
        layers_panel::register(app);
    }
}

fn spawn_camera(mut commands: Commands, surface: Res<RenderSurface>) {
    commands.spawn((
        Name::new("eda-camera"),
        EdaCamera,
        Camera2d,
        surface.target.clone(),
        Camera {
            order: 1,
            is_active: false,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::WindowSize,
            ..OrthographicProjection::default_2d()
        }),
        RenderLayers::layer(EDA_LAYER),
        Tonemapping::None,
        DebandDither::Disabled,
    ));
}

fn configure_gizmos(mut store: ResMut<GizmoConfigStore>) {
    fn set(c: &mut GizmoConfig) {
        c.render_layers = RenderLayers::layer(EDA_LAYER);
        c.line.joints = GizmoLineJoint::Round(6);
    }
    set(store.config_mut::<EdaDots>().0);
    set(store.config_mut::<EdaCross>().0);
    store.config_mut::<EdaDots>().0.line.width = 2.0;
    store.config_mut::<EdaCross>().0.line.width = 1.5;
}

fn reset(
    mut ui: ResMut<EdaUi>,
    mut scene: ResMut<EdaScene>,
    mut eda: ResMut<Eda2d>,
    q: Query<Entity, With<EdaEntity>>,
    mut commands: Commands,
) {
    *ui = EdaUi::default();
    *scene = EdaScene::default();
    eda.0 = None;
    for e in &q {
        commands.entity(e).despawn();
    }
}

/// The design of the board a view shows.
pub fn design(doc: &ActiveDocument, el: ElementId, b: BoardId) -> Option<&Design> {
    doc.doc.element(el)?.pcb()?.board(b)?.design()
}

/// The shown native board (or the component being edited) and its mode, this frame; or the
/// derived board copy whose layout is edited in the active Part Studio.
#[allow(clippy::too_many_arguments)]
fn track_context(
    doc: Option<Res<ActiveDocument>>,
    kind: Res<ActiveKind>,
    pcb: Res<PcbUi>,
    ui: Res<EdaUi>,
    editing: Res<crate::board_in_studio::BoardEditing>,
    cache: Res<crate::parts::PartCache>,
    mut eda: ResMut<Eda2d>,
) {
    let plane = doc
        .as_deref()
        .filter(|_| matches!(*kind, ActiveKind::PartStudio | ActiveKind::Assembly))
        .and_then(|doc| crate::board_in_studio::edited_board(doc, &editing, &cache));
    let now = match plane {
        Some((el, b, _)) => Some((el, Subject::Board(b), Mode::Layout)),
        None => doc
            .as_deref()
            .filter(|_| *kind == ActiveKind::PcbStudio && pcb.view == PcbView::Board)
            .and_then(|doc| {
                let (el, s) = active_studio(doc)?;
                if let Some(c) = ui.editing.get(&el).filter(|c| s.component(**c).is_some()) {
                    let subject = Subject::Component(*c);
                    return Some((el, subject, ui.mode(el, subject)));
                }
                let b = pcb.shown_board(el, s)?;
                s.board(b)?.design()?;
                let subject = Subject::Board(b);
                Some((el, subject, ui.mode(el, subject)))
            }),
    };
    let host = plane.map_or(LayoutHost::Flat, |(_, _, h)| LayoutHost::Plane(h));
    if eda.0 != now || eda.1 != host {
        eda.0 = now;
        eda.1 = host;
    }
}

/// Opens a component's editor (a click on it under Components).
pub fn edit_component(world: &mut World, el: ElementId, c: cadrs_core::pcb::ComponentId) {
    world.resource_mut::<EdaUi>().editing.insert(el, c);
}

/// Leaves the component editor (a click on a board).
pub fn stop_editing(world: &mut World, el: ElementId) {
    world.resource_mut::<EdaUi>().editing.remove(&el);
}

// ---------------------------------------------------------------------------------------------
// The mode switch

/// The mode bar, for a board (`false`) or a component (`true`).
#[derive(Component)]
struct ModeBar(bool);

#[derive(Component, Clone, Copy)]
struct ModeButton(Mode);

fn sync_mode_bar(
    eda: Res<Eda2d>,
    q_area: Query<Entity, With<ViewportArea>>,
    q_bar: Query<(Entity, &ModeBar)>,
    q_btn: Query<(Entity, &ModeButton, Has<cadrs_ui::style::Selected>)>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    // No mode bar on a board edited in a Part Studio: it stays 3D.
    let want = eda
        .0
        .filter(|_| eda.1 == LayoutHost::Flat)
        .map(|(_, s, _)| matches!(s, Subject::Component(_)));
    let mut bar = q_bar.single().ok().map(|(e, b)| (e, b.0));
    // The other kind's bar goes.
    if let Some((e, kind)) = bar
        && want != Some(kind)
    {
        commands.entity(e).despawn();
        bar = None;
    }
    let modes: &[(Mode, &str, &str)] = if want == Some(true) {
        &[
            (Mode::Symbol, "eda-mode-symbol", "Symbol"),
            (Mode::Footprint, "eda-mode-footprint", "Footprint"),
            (Mode::ThreeD, "eda-mode-3d", "3D"),
        ]
    } else {
        &[
            (Mode::Schematic, "eda-mode-schematic", "Schematic"),
            (Mode::Layout, "eda-mode-layout", "Layout"),
            (Mode::ThreeD, "eda-mode-3d", "3D"),
        ]
    };
    match (want, bar) {
        (Some(kind), None) => {
            let Ok(area) = q_area.single() else { return };
            let t = theme.clone();
            commands.entity(area).with_children(|vp| {
                vp.spawn((
                    Name::new("eda-mode-bar"),
                    ModeBar(kind),
                    Node {
                        position_type: PositionType::Absolute,
                        top: Val::Px(8.0),
                        left: Val::Percent(50.0),
                        margin: UiRect::left(Val::Px(-120.0)),
                        column_gap: Val::Px(2.0),
                        padding: UiRect::all(Val::Px(3.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(t.radius_panel)),
                        ..default()
                    },
                    cadrs_ui::glass::panel(&t),
                ))
                .with_children(|b| {
                    for (mode, name, label) in modes.iter().copied() {
                        // A segmented control's segments: no outlines, the mode shown tinted.
                        b.spawn((
                            cadrs_ui::Button::new(name)
                                .label(label)
                                .small()
                                .ghost()
                                .build(&t),
                            ModeButton(mode),
                        ))
                        .entry::<Node>()
                        .and_modify(|mut n| n.border = UiRect::ZERO);
                    }
                });
            });
        }
        (None, Some((e, _))) => commands.entity(e).despawn(),
        _ => {}
    }
    if let Some((_, _, mode)) = eda.0 {
        for (e, b, selected) in &q_btn {
            let on = b.0 == mode;
            if on && !selected {
                commands.entity(e).insert(cadrs_ui::style::Selected);
            } else if !on && selected {
                commands.entity(e).remove::<cadrs_ui::style::Selected>();
            }
        }
    }
}

fn on_mode_button(a: On<Activate>, q: Query<&ModeButton>, eda: Res<Eda2d>, mut ui: ResMut<EdaUi>) {
    let Ok(b) = q.get(a.entity) else { return };
    if let Some((el, board, _)) = eda.0 {
        ui.modes.insert((el, board), b.0);
    }
}

/// Switches a board's view (also the scenario command `eda-mode schematic|layout|3d`).
/// Shows `mode` for the board `b` of the PCB Studio `el` (when it is next shown).
pub fn set_mode_of(world: &mut World, el: ElementId, b: BoardId, mode: Mode) {
    world
        .resource_mut::<EdaUi>()
        .modes
        .insert((el, Subject::Board(b)), mode);
}

pub fn set_mode(world: &mut World, mode: Mode) {
    let ctx = world.resource::<Eda2d>().0;
    if let Some((el, b, _)) = ctx {
        world.resource_mut::<EdaUi>().modes.insert((el, b), mode);
    }
}

// ---------------------------------------------------------------------------------------------
// The view: fitting, panning, zooming, the camera

fn view_size(rect: &ViewportRect) -> [f64; 2] {
    [rect.0.width() as f64, rect.0.height() as f64]
}

/// The draw list's box for "fit".
fn fit_box(list: &DrawList) -> Option<Bounds> {
    list.bounds
}

fn current_view(ui: &EdaUi, eda: &Eda2d) -> Option<View> {
    let (el, b, m) = eda.0?;
    ui.views.get(&(el, b, m)).map(|v| v.0)
}

/// A view shown the first time is fitted; a fitted view stays fitted when the area resizes
/// (a schematic's to the box it was first fitted to: it doesn't jump as it is drawn on).
fn fit_views(
    eda: Res<Eda2d>,
    rect: Res<ViewportRect>,
    scene: Res<EdaScene>,
    mut ui: ResMut<EdaUi>,
) {
    let Some(key @ (_, _, mode)) = eda.0 else {
        return;
    };
    if mode == Mode::ThreeD || scene.key.is_none() {
        return;
    }
    let size = view_size(&rect);
    let drawn = fit_box(&scene.list);
    let ui = &mut *ui;
    let entry = ui.views.entry(key).or_insert((View::new(size), true));
    entry.0.size = size;
    if !entry.1 || mode != Mode::Schematic {
        ui.fit_boxes.remove(&key);
    }
    if entry.1
        && let Some(b) = ui.fit_boxes.get(&key).copied().or(drawn)
    {
        // (The schematic is unbounded: its view keeps the box it was fitted to.)
        if mode == Mode::Schematic {
            ui.fit_boxes.insert(key, b);
        }
        let mut v = entry.0;
        v.fit(b, 0.04);
        if v != entry.0 {
            entry.0 = v;
        }
    }
}

/// Fits the shown 2D view (F).
pub fn fit(world: &mut World) {
    let Some(key) = world.resource::<Eda2d>().0 else {
        return;
    };
    let size = view_size(world.resource::<ViewportRect>());
    let b = fit_box(&world.resource::<EdaScene>().list);
    let mut ui = world.resource_mut::<EdaUi>();
    let entry = ui.views.entry(key).or_insert((View::new(size), true));
    if let Some(b) = b {
        entry.0.fit(b, 0.04);
    }
    entry.1 = true;
    match b {
        Some(b) if key.2 == Mode::Schematic => ui.fit_boxes.insert(key, b),
        _ => ui.fit_boxes.remove(&key),
    };
}

/// Where the pointer is on the shown 2D view (updated by [`navigate`]).
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct EdaPointer {
    /// Screen position (logical px).
    pub screen: Vec2,
    /// Design point under it (nm).
    pub at: cadrs_eda::units::Pt,
    pub over: bool,
}

/// The last left press (when, where: a second one soon after is a double-click) and where the
/// right button went down (let go there: a right-click, not a pan).
type Presses<'a> = (Local<'a, Option<(f64, Vec2)>>, Local<'a, Option<Vec2>>);

/// A left-button event on the 2D view, for the tools.
#[derive(Message, Clone, Copy, Debug)]
pub enum EdaClick {
    Press {
        at: cadrs_eda::units::Pt,
        shift: bool,
        ctrl: bool,
    },
    Release {
        at: cadrs_eda::units::Pt,
    },
    /// A second press soon after the first, at the same place (with Shift: adds to what is selected).
    Double {
        at: cadrs_eda::units::Pt,
        shift: bool,
    },
    /// A right-click (pressed and let go without panning): ends what is being drawn.
    Secondary {
        at: cadrs_eda::units::Pt,
    },
}

/// Pans (right or middle drag), zooms about the pointer (wheel) and fits (F); records the
/// pointer and passes left-button events on as [`EdaClick`]s.
#[allow(clippy::too_many_arguments)]
pub(crate) fn navigate(
    eda: Res<Eda2d>,
    rect: Res<ViewportRect>,
    hover: Res<HoverMap>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut inputs: MessageReader<bevy::picking::pointer::PointerInput>,
    mut keys_in: MessageReader<bevy::input::keyboard::KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<bevy::input_focus::InputFocus>,
    q_dialogs: Query<(), With<cadrs_ui::DialogRoot>>,
    mut pointer: ResMut<EdaPointer>,
    mut clicks: MessageWriter<EdaClick>,
    (mut last_press, mut right_press): Presses,
    (time, live): (Res<Time>, Option<Res<crate::collab::live::Live>>),
    layout: Res<layout_tools::LayoutState>,
    mut ui: ResMut<EdaUi>,
    mut commands: Commands,
) {
    use bevy::picking::pointer::{PointerAction, PointerButton};
    let events: Vec<_> = inputs.read().cloned().collect();
    let key_events: Vec<_> = keys_in.read().cloned().collect();
    if !eda.flat() {
        ui.pan_from = None;
        return;
    }
    let key = eda.0.unwrap();
    let Some((mut view, mut fitted)) = ui.views.get(&key).copied() else {
        return;
    };
    let over = crate::viewport::pointer_over_viewport(&hover, &q_area);
    let local = |c: Vec2| [(c.x - rect.0.min.x) as f64, (c.y - rect.0.min.y) as f64];
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    // Alt+click in a shared document: "look here" (`collab::pointers::ping`), not a click.
    let pings = live.is_some() && keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    for input in events {
        let pos = input.location.position;
        match input.action {
            PointerAction::Move { .. } => {
                if let Some(from) = ui.pan_from {
                    let d = pos - from;
                    if d != Vec2::ZERO {
                        view.pan([d.x as f64, d.y as f64]);
                        fitted = false;
                    }
                    ui.pan_from = Some(pos);
                }
            }
            PointerAction::Press(b) => match b {
                PointerButton::Secondary | PointerButton::Middle if over => {
                    ui.pan_from = Some(pos);
                    if b == PointerButton::Secondary {
                        *right_press = Some(pos);
                    }
                }
                PointerButton::Primary if over && pings => {
                    commands.queue(|world: &mut World| {
                        crate::collab::pointers::ping(world);
                    });
                }
                PointerButton::Primary if over => {
                    let at = view.to_design_pt(local(pos));
                    let now = time.elapsed_secs_f64();
                    let double =
                        last_press.is_some_and(|(t, p)| now - t < 0.4 && p.distance(pos) < 4.0);
                    *last_press = Some((now, pos));
                    clicks.write(EdaClick::Press { at, shift, ctrl });
                    if double {
                        clicks.write(EdaClick::Double { at, shift });
                        *last_press = None;
                    }
                }
                _ => {}
            },
            PointerAction::Release(b) => match b {
                PointerButton::Secondary | PointerButton::Middle => {
                    ui.pan_from = None;
                    // Let go where it was pressed (no pan): a right-click.
                    if b == PointerButton::Secondary
                        && right_press.take().is_some_and(|p| p.distance(pos) < 4.0)
                    {
                        clicks.write(EdaClick::Secondary {
                            at: view.to_design_pt(local(pos)),
                        });
                    }
                }
                PointerButton::Primary if !pings => {
                    clicks.write(EdaClick::Release {
                        at: view.to_design_pt(local(pos)),
                    });
                }
                // A ping's release.
                PointerButton::Primary => {}
            },
            PointerAction::Scroll { unit, y, .. } if over => {
                let lines = match unit {
                    MouseScrollUnit::Line => y,
                    MouseScrollUnit::Pixel => y / 40.0,
                };
                if lines != 0.0 {
                    view.zoom_at(local(pos), 1.2f64.powf(lines as f64));
                    fitted = false;
                }
            }
            PointerAction::Cancel => ui.pan_from = None,
            _ => {}
        }
        pointer.screen = pos;
        pointer.at = view.to_design_pt(local(pos));
        pointer.over = over;
    }
    ui.views.insert(key, (view, fitted));
    let typing = focus.get().is_some() || !q_dialogs.is_empty();
    for k in key_events {
        // In the Layout, F flips what is selected (`layout_tools`); with nothing, it fits.
        let flips = key.2 == Mode::Layout && !layout.selection.is_empty();
        if k.state == bevy::input::ButtonState::Pressed
            && !typing
            && !flips
            && k.shortcut_code() == KeyCode::KeyF
            && !ctrl
            && !shift
        {
            commands.queue(fit);
        }
    }
}

fn place_camera(
    eda: Res<Eda2d>,
    ui: Res<EdaUi>,
    rect: Res<ViewportRect>,
    surface: Res<RenderSurface>,
    mut q: Query<(&mut Camera, &mut Transform, &mut Projection), With<EdaCamera>>,
) {
    let view = current_view(&ui, &eda).filter(|_| eda.flat());
    for (mut cam, mut t, mut proj) in &mut q {
        let active = view.is_some();
        if cam.is_active != active {
            cam.is_active = active;
        }
        let Some(v) = view else { continue };
        let window = cam
            .logical_viewport_size()
            .unwrap_or(surface.size.as_vec2());
        // The design point at the window's centre (the view is centred on the viewport area).
        let win_center = [
            (window.x / 2.0 - rect.0.min.x) as f64,
            (window.y / 2.0 - rect.0.min.y) as f64,
        ];
        let c = v.to_design(win_center);
        let want = Vec3::new((c[0] / 1e6) as f32, (c[1] / 1e6) as f32, 0.0);
        if t.translation != want {
            t.translation = want;
        }
        if let Projection::Orthographic(o) = &mut *proj {
            let s = 1.0 / v.scale as f32;
            if o.scale != s {
                o.scale = s;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The scene

/// What a view's picture depends on beyond the design: selections, layers. Tools add to it.
#[derive(Resource, Default, Clone, Debug, PartialEq, Hash)]
pub struct SceneInputs {
    pub sch_highlight: Vec<cadrs_eda::sch_edit::SchItem>,
    /// The net of the wire under the pointer, drawn lit.
    pub sch_hover: Vec<cadrs_eda::sch_edit::SchItem>,
    /// The schematic's pin ends marked where wires join, and the one a wire would join now.
    pub sch_pin_ends: render::PinEnds,
    pub sch_hot_pin: Option<cadrs_eda::units::Pt>,
    pub board_selected: Vec<uuid::Uuid>,
    /// The Layout's item under the pointer (drawn lighter), the zone or polygon corner under
    /// it, and the corner handles' half size.
    pub board_hover: Option<uuid::Uuid>,
    pub board_hot_corner: Option<cadrs_eda::units::Pt>,
    pub board_handle: cadrs_eda::units::Nm,
    /// The symbol editor's selection.
    pub sym_selected: Vec<cadrs_eda::lib_edit::SymbolPart>,
    pub hidden_layers: Vec<cadrs_eda::layer::Layer>,
    pub active_layer: Option<cadrs_eda::layer::Layer>,
    /// Layers other than the active one drawn faint.
    pub dim_inactive: bool,
    /// The net highlighted in the Layout.
    pub board_net: Option<String>,
    /// A design being edited (a drag in progress) drawn instead of the document's.
    pub preview: Option<u64>,
}

/// The design being previewed by a tool (shown instead of the stored one while dragging).
#[derive(Resource, Default)]
pub struct Preview {
    design: Option<Box<Design>>,
    component: Option<Box<cadrs_eda::Component>>,
    generation: u64,
}

impl Preview {
    /// Shows `d` instead of the stored design (or the stored one again with `None`).
    pub fn set(&mut self, d: Option<Design>) {
        self.design = d.map(Box::new);
        self.generation += 1;
    }

    pub fn get(&self) -> Option<&Design> {
        self.design.as_deref()
    }

    /// Takes the shown design out to change it (put it back with [`Preview::set`]).
    pub fn take(&mut self) -> Option<Design> {
        self.design.take().map(|d| *d)
    }

    /// Shows `c` instead of the stored component (the component editors).
    pub fn set_component(&mut self, c: Option<cadrs_eda::Component>) {
        self.component = c.map(Box::new);
        self.generation += 1;
    }

    pub fn component(&self) -> Option<&cadrs_eda::Component> {
        self.component.as_deref()
    }

    /// Bumped by every change of what is previewed.
    pub fn generation(&self) -> u64 {
        self.generation
    }
}

fn rgba(c: render::Rgba) -> Color {
    Color::srgba_u8(c[0], c[1], c[2], c[3])
}

/// A filled area's colour and depth: areas sharing one share a mesh.
type AreaKey = (render::Rgba, i32);

/// The schematic's colours: KiCad's on its cream paper in light; in dark, Horizon EDA's look
/// ([`render::SchematicTheme::dark`]: no paper, hairlines, flag labels).
fn schematic_theme() -> render::SchematicTheme {
    if cadrs_ui::dark_in_force() {
        render::SchematicTheme::dark()
    } else {
        render::SchematicTheme::default()
    }
}

#[allow(clippy::too_many_arguments)]
fn rebuild_scene(
    eda: Res<Eda2d>,
    doc: Option<Res<ActiveDocument>>,
    inputs: Option<Res<SceneInputs>>,
    preview: Option<Res<Preview>>,
    mut scene: ResMut<EdaScene>,
    q: Query<Entity, With<EdaEntity>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut gizmo_assets: ResMut<Assets<GizmoAsset>>,
    mut commands: Commands,
) {
    let flat = eda.flat();
    let Some(doc) = doc.filter(|_| flat) else {
        if scene.key.is_some() {
            scene.key = None;
            scene.list = DrawList::default();
            for e in &q {
                commands.entity(e).despawn();
            }
        }
        return;
    };
    let (el, subject, mode) = eda.0.unwrap();
    let inputs = inputs.map(|i| i.clone()).unwrap_or_default();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (
        el,
        subject,
        mode,
        doc.history.undo_len(),
        doc.history.redo_len(),
        &inputs,
        cadrs_ui::dark_in_force(),
    )
        .hash(&mut h);
    let preview_design = preview.as_ref().and_then(|p| p.get());
    let preview_component = preview.as_ref().and_then(|p| p.component());
    if preview_design.is_some() || preview_component.is_some() {
        preview.as_ref().map(|p| p.generation).hash(&mut h);
    }
    let key = h.finish();
    if scene.key == Some(key) {
        return;
    }
    let list = match (subject, mode) {
        (Subject::Board(b), _) => {
            let Some(d) = preview_design.or_else(|| design(&doc, el, b)) else {
                return;
            };
            match mode {
                Mode::Schematic => {
                    let hl = render::Highlight {
                        items: inputs.sch_highlight.clone(),
                        hover: inputs.sch_hover.clone(),
                        pin_ends: inputs.sch_pin_ends,
                        hot_pin: inputs.sch_hot_pin,
                    };
                    if d.schematic.sheets.is_empty() {
                        // A board imported alone: its (first) sheet is blank until edited.
                        render::schematic(&Design::new().schematic, 0, &schematic_theme(), &hl)
                    } else {
                        render::schematic(&d.schematic, 0, &schematic_theme(), &hl)
                    }
                }
                Mode::Layout => {
                    let mut v = render::BoardView::all(&d.board);
                    v.visible.retain(|l| !inputs.hidden_layers.contains(l));
                    v.dim_inactive = inputs.dim_inactive;
                    v.highlight_net = inputs.board_net.clone();
                    if let Some(a) = inputs.active_layer {
                        v.active = a;
                    }
                    v.selected = inputs.board_selected.clone();
                    v.hovered = inputs.board_hover;
                    v.hot_corner = inputs.board_hot_corner;
                    if inputs.board_handle > 0 {
                        v.handle = inputs.board_handle;
                    }
                    render::board(&d.board, &render::BoardTheme::default(), &v)
                }
                _ => DrawList::default(),
            }
        }
        (Subject::Component(c), _) => {
            let comp = preview_component.or_else(|| {
                doc.doc
                    .element(el)
                    .and_then(|e| e.pcb())
                    .and_then(|s| s.component(c))
                    .map(|c| &c.component)
            });
            let Some(comp) = comp else { return };
            match mode {
                Mode::Footprint => render::footprint_view(
                    comp.footprint.as_ref(),
                    &render::BoardTheme::default(),
                    &inputs.board_selected,
                ),
                _ => render::symbol_view(
                    comp.symbol.as_ref(),
                    &schematic_theme(),
                    &inputs.sym_selected,
                ),
            }
        }
    };
    for e in &q {
        commands.entity(e).despawn();
    }
    let layer = RenderLayers::layer(EDA_LAYER);
    // The ground: the camera draws over the 3D view, so it paints its own background.
    commands.spawn((
        Name::new("eda-ground"),
        EdaEntity,
        Sprite::from_color(rgba(list.background), Vec2::splat(1.0e6)),
        Transform::from_xyz(0.0, 0.0, -100.0),
        layer.clone(),
    ));
    // One mesh per colour and depth.
    let mut groups: Vec<(AreaKey, Vec<[f32; 3]>)> = vec![];
    for a in &list.areas {
        let pts = a
            .tris
            .iter()
            .map(|t| [(t[0] / 1e6) as f32, (t[1] / 1e6) as f32, 0.0]);
        match groups.iter_mut().find(|(k, _)| *k == (a.color, a.z)) {
            Some((_, v)) => v.extend(pts),
            None => groups.push(((a.color, a.z), pts.collect())),
        }
    }
    for ((color, z), positions) in groups {
        let n = positions.len();
        let mut mesh = Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; n]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; n]);
        commands.spawn((
            Name::new("eda-area"),
            EdaEntity,
            Mesh2d(meshes.add(mesh)),
            MeshMaterial2d(materials.add(ColorMaterial::from(rgba(color)))),
            Transform::from_xyz(0.0, 0.0, -50.0 + z as f32 * 0.1),
            layer.clone(),
        ));
    }
    // One retained gizmo per line width ([`set_line_widths`] sizes them to the zoom).
    let mut by_width: Vec<(i64, GizmoAsset)> = vec![];
    for l in &list.lines {
        let i = match by_width.iter().position(|(w, _)| *w == l.width) {
            Some(i) => i,
            None => {
                by_width.push((l.width, GizmoAsset::default()));
                by_width.len() - 1
            }
        };
        // A closed outline runs on past its start, so its first corner gets a joint too.
        let closed = l.pts.len() > 2 && l.pts.first() == l.pts.last();
        let pts = l
            .pts
            .iter()
            .chain(l.pts.get(1).filter(|_| closed))
            .map(|p| Vec2::new((p.x as f64 / 1e6) as f32, (p.y as f64 / 1e6) as f32));
        by_width[i].1.linestrip_2d(pts, rgba(l.color));
    }
    for (width, asset) in by_width {
        commands.spawn((
            Name::new("eda-lines"),
            EdaEntity,
            EdaLines(width),
            Gizmo {
                handle: gizmo_assets.add(asset),
                // Round joints: without them a thick curve (an arc, a stroke-font glyph) is a
                // row of separate quads with gaps on the outside of every bend.
                line_config: GizmoLineConfig {
                    width: 1.0,
                    joints: GizmoLineJoint::Round(6),
                    ..default()
                },
                ..default()
            },
            layer.clone(),
        ));
    }
    scene.covered = if matches!(mode, Mode::Schematic | Mode::Symbol) {
        covered_points(&list.lines, cadrs_eda::units::SCHEMATIC_GRID, 300_000)
    } else {
        Default::default()
    };
    scene.list = list;
    scene.key = Some(key);
}

/// Each line width's pixel width at the current zoom (at least a pixel): a line is as wide as
/// it is in the design, so small stroke text stays in proportion. Width 0 is a hairline of
/// [`HAIRLINE_PX`] whatever the zoom.
fn set_line_widths(eda: Res<Eda2d>, ui: Res<EdaUi>, mut q: Query<(&EdaLines, &mut Gizmo)>) {
    let Some(v) = current_view(&ui, &eda).filter(|_| eda.flat()) else {
        return;
    };
    for (lines, mut g) in &mut q {
        let px = if lines.0 == 0 {
            HAIRLINE_PX
        } else {
            ((lines.0 as f64 / 1e6 * v.scale) as f32).max(1.0)
        };
        if g.line_config.width != px {
            g.line_config.width = px;
        }
    }
}

/// A hairline's width in pixels (Horizon EDA's 1.5).
const HAIRLINE_PX: f32 = 1.5;

/// The Horizon theme's grid, as meshes under everything drawn: faint lines on every grid step
/// (every 2nd, 4th, … when they'd be closer than 12 px), and a cross on every fifth line, its
/// arms a fixed share of a step, so it keeps its size against the drawing as the view zooms.
#[derive(Component)]
struct GridMesh(bool);

#[allow(clippy::too_many_arguments)]
fn draw_grid_mesh(
    eda: Res<Eda2d>,
    ui: Res<EdaUi>,
    rect: Res<ViewportRect>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut q: Query<(
        &GridMesh,
        &Mesh2d,
        &MeshMaterial2d<ColorMaterial>,
        &mut Visibility,
    )>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut last: Local<Option<(u64, bool)>>,
    mut commands: Commands,
) {
    let th = schematic_theme();
    let shown =
        th.horizon && matches!(eda.0, Some((_, _, Mode::Schematic | Mode::Symbol))) && eda.flat();
    let view = current_view(&ui, &eda).filter(|_| shown);
    if q.is_empty() {
        // The two meshes: lines below crosses, both over the ground (z -100), under the areas.
        for (cross, color, z) in [(false, th.grid_line, -90.0), (true, th.grid, -89.0)] {
            let mut m = ColorMaterial::from(rgba(color));
            m.alpha_mode = bevy::sprite_render::AlphaMode2d::Blend;
            commands.spawn((
                Name::new(if cross {
                    "eda-grid-crosses"
                } else {
                    "eda-grid-lines"
                }),
                GridMesh(cross),
                Mesh2d(meshes.add(Mesh::new(
                    bevy::mesh::PrimitiveTopology::TriangleList,
                    bevy::asset::RenderAssetUsages::default(),
                ))),
                MeshMaterial2d(materials.add(m)),
                Transform::from_xyz(0.0, 0.0, z),
                Visibility::Hidden,
                RenderLayers::layer(EDA_LAYER),
            ));
        }
        return;
    }
    let Some(v) = view else {
        for (.., mut vis) in &mut q {
            if *vis != Visibility::Hidden {
                *vis = Visibility::Hidden;
            }
        }
        *last = None;
        return;
    };
    let window = window_size(&windows);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (
        v.center.map(f64::to_bits),
        v.scale.to_bits(),
        v.size.map(f64::to_bits),
        window.map(|w| (w.x.to_bits(), w.y.to_bits())),
        (rect.0.min.x.to_bits(), rect.0.min.y.to_bits()),
    )
        .hash(&mut h);
    let key = (h.finish(), true);
    if *last == Some(key) {
        return;
    }
    *last = Some(key);
    let mut step = cadrs_eda::units::SCHEMATIC_GRID;
    while (step as f64 / 1e6) * v.scale < 12.0 {
        step *= 2;
    }
    let vis = window_bounds(&v, &rect, window);
    let (x0, x1) = (vis.min.x.div_euclid(step), vis.max.x.div_euclid(step) + 1);
    let (y0, y1) = (vis.min.y.div_euclid(step), vis.max.y.div_euclid(step) + 1);
    let mm = |nm: i64| nm as f32 / 1e6;
    let px = (1.0 / v.scale) as f32;
    // A quad from `a` to `b`, `w` wide.
    let quad = |out: &mut Vec<[f32; 3]>, a: Vec2, b: Vec2, w: f32| {
        let n = (b - a).normalize_or_zero().perp() * (w / 2.0);
        for p in [a - n, b - n, b + n, a - n, b + n, a + n] {
            out.push([p.x, p.y, 0.0]);
        }
    };
    let (bottom, top) = (mm(y0 * step), mm(y1 * step));
    let (left, right) = (mm(x0 * step), mm(x1 * step));
    let mut lines = Vec::new();
    for i in x0..=x1 {
        let x = mm(i * step);
        quad(&mut lines, Vec2::new(x, bottom), Vec2::new(x, top), px);
    }
    for j in y0..=y1 {
        let y = mm(j * step);
        quad(&mut lines, Vec2::new(left, y), Vec2::new(right, y), px);
    }
    let mut crosses = Vec::new();
    let arm = mm(step) * 0.3;
    let major = 5;
    for i in x0.div_euclid(major)..=x1.div_euclid(major) + 1 {
        for j in y0.div_euclid(major)..=y1.div_euclid(major) + 1 {
            let c = Vec2::new(mm(i * major * step), mm(j * major * step));
            quad(&mut crosses, c - Vec2::X * arm, c + Vec2::X * arm, 1.5 * px);
            quad(&mut crosses, c - Vec2::Y * arm, c + Vec2::Y * arm, 1.5 * px);
        }
    }
    for (g, mesh, material, mut vis) in &mut q {
        let positions = if g.0 { &crosses } else { &lines };
        let color = rgba(if g.0 { th.grid } else { th.grid_line });
        if let Some(mut m) = materials.get_mut(&material.0)
            && m.color != color
        {
            m.color = color;
        }
        if let Some(mut m) = meshes.get_mut(&mesh.0) {
            let n = positions.len();
            m.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions.clone());
            m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; n]);
            m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; n]);
        }
        if *vis != Visibility::Visible {
            *vis = Visibility::Visible;
        }
    }
}

/// What the window shows of `v`: its own rect and the gaps and panels around it, since the
/// canvas runs on under the floating panels (`crate::document`).
fn window_bounds(v: &View, rect: &ViewportRect, window: Option<Vec2>) -> Bounds {
    let Some(w) = window else {
        return v.visible();
    };
    let o = rect.0.min;
    let a = v.to_design([-o.x as f64, -o.y as f64]);
    let b = v.to_design([(w.x - o.x) as f64, (w.y - o.y) as f64]);
    Bounds {
        min: cadrs_eda::units::Pt::new(a[0].min(b[0]) as i64, a[1].min(b[1]) as i64),
        max: cadrs_eda::units::Pt::new(a[0].max(b[0]) as i64, a[1].max(b[1]) as i64),
    }
}

/// The primary window's logical size.
fn window_size(q: &Query<&Window, With<bevy::window::PrimaryWindow>>) -> Option<Vec2> {
    q.iter().next().map(|w| Vec2::new(w.width(), w.height()))
}

/// The schematic grid as dots (every grid point, or every 2nd, 4th, … when they'd be closer
/// than 10 px), and the placing crosshair ([`Crosshair`]).
#[allow(clippy::too_many_arguments)]
fn draw_overlay(
    eda: Res<Eda2d>,
    ui: Res<EdaUi>,
    scene: Res<EdaScene>,
    cross: Res<Crosshair>,
    rect: Res<ViewportRect>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut dots: Gizmos<EdaDots>,
    mut cg: Gizmos<EdaCross>,
) {
    let Some((_, _, mode)) = eda.0 else { return };
    if !matches!(mode, Mode::Schematic | Mode::Symbol) {
        return;
    }
    let Some(v) = current_view(&ui, &eda) else {
        return;
    };
    let mm = |nm: i64| nm as f32 / 1e6;
    let mut step = cadrs_eda::units::SCHEMATIC_GRID;
    while (step as f64 / 1e6) * v.scale < 10.0 {
        step *= 2;
    }
    let vis = window_bounds(&v, &rect, window_size(&windows));
    let (x0, x1) = (vis.min.x.div_euclid(step), vis.max.x.div_euclid(step) + 1);
    let (y0, y1) = (vis.min.y.div_euclid(step), vis.max.y.div_euclid(step) + 1);
    // A dot is a 2 px stroke a pixel and a half long.
    let len = (1.5 / v.scale) as f32;
    let th = schematic_theme();
    let dot = rgba(th.grid);
    let base = cadrs_eda::units::SCHEMATIC_GRID;
    // Filled symbol bodies hide the grid under them (KiCad's do): their boxes, in nm.
    let body = th.body_fill;
    let filled: Vec<[f64; 4]> = scene
        .list
        .areas
        .iter()
        .filter(|a| a.color == body && !a.tris.is_empty())
        .map(|a| {
            a.tris
                .iter()
                .fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |b, t| {
                    [
                        b[0].min(t[0]),
                        b[1].min(t[1]),
                        b[2].max(t[0]),
                        b[3].max(t[1]),
                    ]
                })
        })
        .collect();
    let hidden = |x: i64, y: i64| {
        filled.iter().any(|b| {
            (x as f64) > b[0] && (x as f64) < b[2] && (y as f64) > b[1] && (y as f64) < b[3]
        })
    };
    // Horizon's grid is a mesh of its own ([`draw_grid_mesh`]).
    if !th.horizon && (x1 - x0) * (y1 - y0) < 40_000 {
        for i in x0..=x1 {
            for j in y0..=y1 {
                if hidden(i * step, j * step)
                    || scene.covered.contains(&(i * step / base, j * step / base))
                {
                    continue;
                }
                let (x, y) = (mm(i * step), mm(j * step));
                dots.line_2d(
                    Vec2::new(x - len / 2.0, y),
                    Vec2::new(x + len / 2.0, y),
                    dot,
                );
            }
        }
    }
    if let Some((at, connects)) = cross.0 {
        let c = Vec2::new(mm(at.x), mm(at.y));
        let px = 1.0 / v.scale as f32;
        let ink = rgba(th.cursor);
        cg.line_2d(c - Vec2::X * 36.0 * px, c + Vec2::X * 36.0 * px, ink);
        cg.line_2d(c - Vec2::Y * 36.0 * px, c + Vec2::Y * 36.0 * px, ink);
        let sq = if connects {
            Color::srgb(0.1, 0.6, 0.2)
        } else {
            Color::srgb(0.75, 0.55, 0.1)
        };
        cg.rect_2d(Isometry2d::from_translation(c), Vec2::splat(11.0 * px), sq);
        if connects {
            cg.rect_2d(Isometry2d::from_translation(c), Vec2::splat(8.0 * px), sq);
            cg.rect_2d(Isometry2d::from_translation(c), Vec2::splat(5.0 * px), sq);
            cg.rect_2d(Isometry2d::from_translation(c), Vec2::splat(2.0 * px), sq);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Scenario commands

/// The `eda-…` scenario commands:
/// - `eda-example <stage>`: adds an example as a board of the active PCB Studio and shows it:
///   the "Getting Started" course's design at a step (`gs04`, `gs07`, `gs11`, `gs14`, `gs16`,
///   `gs17`, `gs18`, `gs25`), `wire-editing` ([`cadrs_eda::getting_started::wire_editing`]), or
///   `power-monitor` (the LoRa board redrawn, with its project parts as components);
/// - `eda-redraw power-monitor`: the shown board becomes the LoRa board (see
///   [`redraw_power_monitor`]);
/// - `eda-mode schematic|layout|3d`; `eda-fit`; `eda-zoom x0 y0 x1 y1` (shows that box, mm);
/// - `eda-lcsc-fixtures <dir>`: the online part search answers from the saved EasyEDA parts in
///   `<dir>` instead of the network ([`online`]).
pub fn is_script_command(s: &str) -> bool {
    s.starts_with("eda-")
}

/// An example board for scenarios (`eda-example <stage>`): the board's name, its design, and
/// the components of its project library (made in the studio's Components).
fn course_stage(stage: &str) -> Option<(&'static str, Design, Vec<cadrs_eda::Component>)> {
    use cadrs_eda::getting_started as gs;
    let lib = cadrs_eda::library::LibraryTable::builtin();
    let course = |d: Design| Some(("getting-started", d, vec![]));
    match stage {
        "gs04" => course(gs::gs04(&lib)),
        "gs07" => course(gs::gs07(&lib)),
        "gs11" => course(gs::gs11(&lib)),
        "gs14" => course(gs::gs14(&lib).0),
        "gs16" => course(gs::gs16(&lib)),
        "gs17" => course(gs::gs17(&lib)),
        "gs18" => course(gs::gs18(&lib)),
        "gs25" => course(gs::gs25().0),
        "wire-editing" => course(gs::wire_editing(&lib)),
        "power-monitor" => {
            use cadrs_eda::power_monitor as pm;
            let (d, lib) = pm::design();
            let header = lib
                .footprint("Connector_PinHeader_2.54mm:PinHeader_2x10_P2.54mm_Vertical")
                .cloned();
            let parts = vec![
                cadrs_eda::Component {
                    name: "RA-01SH".into(),
                    symbol: Some(pm::ra01sh_symbol()),
                    footprint: Some(pm::ra01sh_footprint()),
                },
                cadrs_eda::Component {
                    name: "ESP32 header left".into(),
                    symbol: Some(pm::esp32_header_symbol(false)),
                    footprint: header.clone(),
                },
                cadrs_eda::Component {
                    name: "ESP32 header right".into(),
                    symbol: Some(pm::esp32_header_symbol(true)),
                    footprint: header,
                },
                cadrs_eda::Component {
                    name: "PCB antenna".into(),
                    symbol: lib.symbol("Device:Antenna").cloned(),
                    footprint: Some(pm::swra416_footprint()),
                },
            ];
            Some(("power-monitor", d, parts))
        }
        _ => None,
    }
}

/// `eda-redraw power-monitor`: the shown board becomes the desk power monitor's LoRa board
/// (the redraw, [`cadrs_eda::power_monitor`]), its Ra-01SH the JLCPCB catalogue's (symbol,
/// footprint and STEP model, from the user's library) when it's there; the studio's Components
/// become the parts it uses. One undo step.
fn redraw_power_monitor(w: &mut World) {
    use cadrs_eda::power_monitor as pm;
    let lib = ui::libraries(w);
    let module = lib
        .symbol("LCSC:RA-01SH")
        .cloned()
        .zip(lib.footprint("LCSC:WIRELM-SMD_RA-01SH").cloned());
    if module.is_none() {
        warn!("eda-redraw: no LCSC:RA-01SH in the libraries; the drawn module is used");
    }
    let (design, plib) = pm::design_from(pm::libraries_with(module));
    let Some((element, studio)) = w
        .get_resource::<ActiveDocument>()
        .and_then(|d| active_studio(d).map(|(el, s)| (el, s.clone())))
    else {
        return;
    };
    let board = w.resource::<PcbUi>().shown_board(element, &studio);
    let mark = w.resource::<ActiveDocument>().history.undo_len();
    let mut doc = w.resource_mut::<ActiveDocument>();
    let r = match board {
        Some(board) => doc.execute(&cadrs_core::pcb::SetDesign {
            element,
            board,
            design: Box::new(design.clone()),
            label: "Redraw".into(),
        }),
        None => doc.execute(&cadrs_core::pcb::AddBoard {
            element,
            name: Some("power-monitor".into()),
            design: Box::new(design.clone()),
            imported_from: None,
        }),
    };
    if let Err(e) = r {
        warn!("eda-redraw: {e}");
        return;
    }
    // Components: one per part the schematic uses (power ports aside), with its footprint.
    for c in &studio.components {
        let _ = doc.execute(&cadrs_core::pcb::DeleteComponent {
            element,
            component: c.id,
        });
    }
    let sch = &design.schematic;
    let mut seen = vec![];
    for s in sch.sheets.iter().flat_map(|sh| &sh.symbols) {
        let Some(def) = sch.symbol(&s.symbol) else {
            continue;
        };
        if def.power || seen.contains(&def.id) {
            continue;
        }
        seen.push(def.id.clone());
        let fp_id = s
            .field(cadrs_eda::symbol::fields::FOOTPRINT)
            .map(|f| f.value().to_string())
            .unwrap_or_default();
        let footprint = plib.footprint(&fp_id).cloned();
        let value = cadrs_eda::Component {
            name: def.name().to_string(),
            symbol: Some(def.clone()),
            footprint,
        };
        let _ = doc.execute(&cadrs_core::pcb::AddComponent {
            element,
            name: None,
            value: Some(Box::new(value)),
        });
    }
    doc.squash_element_since(mark, element, "Redraw the power monitor board");
}

fn run_script_commands(mut msgs: MessageReader<cadrs_ui::ScriptCommand>, mut commands: Commands) {
    for m in msgs.read() {
        let s = m.0.trim().to_string();
        commands.queue(move |w: &mut World| {
            if let Some(stage) = s.strip_prefix("eda-example ") {
                let Some((name, design, parts)) = course_stage(stage.trim()) else {
                    warn!("eda-example: no stage {stage}");
                    return;
                };
                let Some(mut doc) = w.get_resource_mut::<ActiveDocument>() else {
                    return;
                };
                let Some(element) = doc
                    .active_element()
                    .filter(|e| e.pcb().is_some())
                    .map(|e| e.id)
                else {
                    return;
                };
                let cmd = cadrs_core::pcb::AddBoard {
                    element,
                    name: Some(name.into()),
                    design: Box::new(design),
                    imported_from: None,
                };
                if let Err(e) = doc.execute(&cmd) {
                    warn!("eda-example: {e}");
                }
                // The project library's parts as components.
                for part in parts {
                    let added = doc.execute(&cadrs_core::pcb::AddComponent {
                        element,
                        name: Some(part.name.clone()),
                        value: None,
                    });
                    let id = doc
                        .doc
                        .element(element)
                        .and_then(|e| e.pcb())
                        .and_then(|s| s.components.last())
                        .map(|c| c.id);
                    if let (Ok(()), Some(component)) = (added, id) {
                        let set = cadrs_core::pcb::SetComponent {
                            element,
                            component,
                            value: Box::new(part),
                            label: "Example part".into(),
                        };
                        if let Err(e) = doc.execute(&set) {
                            warn!("eda-example: {e}");
                        }
                    }
                }
            } else if s
                .strip_prefix("eda-redraw ")
                .is_some_and(|x| x.trim() == "power-monitor")
            {
                redraw_power_monitor(w);
            } else if let Some(m) = s.strip_prefix("eda-mode ") {
                let mode = match m.trim() {
                    "schematic" => Mode::Schematic,
                    "layout" => Mode::Layout,
                    _ => Mode::ThreeD,
                };
                // The board may only just have been added: set the mode for the shown one.
                let doc = w.resource::<ActiveDocument>();
                let pcb = w.resource::<PcbUi>();
                let target = active_studio(doc)
                    .and_then(|(el, st)| Some((el, Subject::Board(pcb.shown_board(el, st)?))));
                if let Some(key) = target {
                    w.resource_mut::<EdaUi>().modes.insert(key, mode);
                }
            } else if let Some(_dir) = s.strip_prefix("eda-lcsc-fixtures ") {
                #[cfg(feature = "easyeda")]
                if let Some(mut o) = w.get_resource_mut::<online::Online>() {
                    o.fixtures = Some(std::path::PathBuf::from(_dir.trim()));
                }
            } else if s == "eda-fit" {
                fit(w);
            } else if let Some(args) = s.strip_prefix("eda-zoom ") {
                // Shows the box x0 y0 x1 y1 (mm).
                let v: Vec<f64> = args
                    .split_whitespace()
                    .filter_map(|x| x.parse().ok())
                    .collect();
                let Some(key) = w.resource::<Eda2d>().0 else {
                    return;
                };
                if v.len() == 4 {
                    let size = view_size(w.resource::<ViewportRect>());
                    let b = Bounds {
                        min: cadrs_eda::units::Pt::mm(v[0], v[1]),
                        max: cadrs_eda::units::Pt::mm(v[2], v[3]),
                    };
                    let mut view = View::new(size);
                    view.fit(b, 0.0);
                    w.resource_mut::<EdaUi>().views.insert(key, (view, false));
                }
            }
        });
    }
}
