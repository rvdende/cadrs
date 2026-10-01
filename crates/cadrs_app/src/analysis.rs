//! The **Analysis tools** (P3E.3, TD6.6, PS2.11, A1.9), like Onshape's: the bottom-right
//! Analysis tool's menu turns them on and off in a Part Studio or an Assembly.
//!
//! - **Draft analysis…** opens the **Draft analysis** dialog: the **Pull direction** (a default
//!   plane, a plane feature or a planar face's normal, a straight edge, a cylinder's axis;
//!   picked in the view, Top by default), the flip button, and the **Draft angle** needed
//!   (3° by default). The parts are coloured by how much draft each point of their faces has
//!   against the pull direction, in six bands ([`cadrs_core::analysis::DraftBand`]: greens with
//!   enough positive draft, yellows with too little, reds with negative draft), on the GPU
//!   (`part_shading.wgsl`), with a legend at the right of the view (marked "(flipped)" when
//!   the pull is) and the pull direction's blue arrow at its reference. ✓ keeps the analysis while
//!   you work; ✕ (or the menu's Exit draft analysis) ends it.
//! - **Curvature** draws curvature combs on the parts' curved edges (the selected edges when
//!   any are): a tooth at each point of the edge, away from its centre of curvature, as long as
//!   the curvature, joined at their tips ([`cadrs_core::analysis::curvature_comb`]). The longest
//!   tooth is a fixed length on screen, so the combs stay readable as you zoom.
//! - **Zebra stripes** paints the parts with black and white bands of the view ray reflected
//!   off them ([`cadrs_core::analysis::zebra_phase`], per pixel in `part_shading.wgsl`): a flat
//!   face is one shade (straight bands in perspective), curved faces show the bands, and the
//!   bands kink where faces meet without curvature continuity.
//! - Draft analysis and zebra stripes both colour the faces, so turning one on turns the other
//!   off. Each is a view of the tab, kept per tab: nothing in the document changes and nothing
//!   is undone. A selected face keeps its orange.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, observe};
use cadrs_core::analysis::{DraftBand, curvature_comb};
use cadrs_core::{ElementId, FeatureId, PartId};
use cadrs_sketch::{EdgeName, FaceName};
use cadrs_ui::prelude::*;
use cadrs_ui::{FeatureDialogAccept, FeatureDialogCancel, MenuAction, MenuItem, NumberField, NumberFieldCommit, SelectionList, open_menu};

use crate::parts::{PartCache, PickFilter};
use crate::viewport::{ActiveKind, Pick, PickFilterOverride, PickRequest, PlaneKind, PlanesVisible, Selection, ViewportArea, ViewportView};
use crate::{ActiveDocument, AppState};

pub struct AnalysisPlugin;

impl Plugin for AnalysisPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AnalysisViews>()
            .init_resource::<ShadingAnalysis>()
            .init_gizmo_group::<CurvatureGizmos>()
            .add_systems(Startup, configure_gizmos)
            .add_systems(
                Update,
                (take_picks, follow_selection, sync_dialog, sync_legend, sync_shading_analysis, draw_combs, sync_pull_arrow)
                    .chain()
                    .after(crate::parts::PartsSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(AppState::Document), |mut views: ResMut<AnalysisViews>, mut over: ResMut<PickFilterOverride>| {
                if views.dialog.is_some() {
                    over.0 = None;
                }
                *views = AnalysisViews::default();
            })
            .add_observer(on_tool)
            .add_observer(on_menu)
            .add_observer(on_accept)
            .add_observer(on_cancel)
            .add_observer(on_angle);
    }
}

/// The draft analysis's default required angle (degrees).
pub const DEFAULT_DRAFT_ANGLE: f32 = 3.0;

/// What a pull direction was picked from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PullRef {
    Plane(PlaneKind),
    Face(PartId, FaceName),
    Edge(PartId, EdgeName),
    Feature(FeatureId),
}

/// A tab's draft analysis: the pull direction (where it came from and the direction), its flip
/// and the draft angle needed (degrees).
#[derive(Debug, Clone, PartialEq)]
pub struct DraftState {
    pub reference: Option<PullRef>,
    pub label: String,
    pub pull: Vec3,
    /// Where the pull direction's arrow stands (on the reference).
    pub anchor: Vec3,
    pub flip: bool,
    pub angle: f32,
}

impl Default for DraftState {
    fn default() -> Self {
        Self { reference: Some(PullRef::Plane(PlaneKind::Top)), label: "Top plane".into(), pull: Vec3::Z, anchor: Vec3::ZERO, flip: false, angle: DEFAULT_DRAFT_ANGLE }
    }
}

impl DraftState {
    /// The pull direction, flipped if asked.
    pub fn direction(&self) -> Vec3 {
        let d = self.pull.normalize_or_zero();
        if self.flip { -d } else { d }
    }
}

/// A tab's analyses.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AnalysisState {
    pub draft: Option<DraftState>,
    pub zebra: bool,
    pub curvature: bool,
}

/// The analyses of each tab, and the tab whose Draft analysis dialog is open.
#[derive(Resource, Debug, Default)]
pub struct AnalysisViews {
    pub per: HashMap<ElementId, AnalysisState>,
    pub dialog: Option<ElementId>,
}

/// The active tab's face colouring for the part material (`crate::section_view::sync_shading`
/// writes it into the material uniforms): mode (0 none, 1 zebra, 2 draft), the draft angle
/// (degrees) and the pull direction.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct ShadingAnalysis {
    pub mode: u32,
    pub angle: f32,
    pub pull: Vec3,
}

/// The number of zebra bands over half a turn of the reflected view ray.
pub const ZEBRA_STRIPES: f32 = cadrs_core::analysis::ZEBRA_BANDS as f32;

impl ShadingAnalysis {
    /// The material's `analysis` and `pull` uniforms.
    pub fn uniforms(&self) -> (Vec4, Vec4) {
        (Vec4::new(self.mode as f32, self.angle, ZEBRA_STRIPES, 0.0), self.pull.extend(0.0))
    }
}

/// The draft bands' colours for the material (linear, top band first).
pub fn band_colors() -> [Vec4; 6] {
    DraftBand::ALL.map(|b| {
        let [r, g, bl] = b.color();
        Color::srgb_u8(r, g, bl).to_linear().to_vec4()
    })
}

fn modeling_tab(world: &World) -> Option<ElementId> {
    let kind = *world.resource::<ActiveKind>();
    if !matches!(kind, ActiveKind::PartStudio | ActiveKind::Assembly) {
        return None;
    }
    world.get_resource::<ActiveDocument>()?.active
}

/// The active tab's analyses.
pub fn active_state(world: &World) -> AnalysisState {
    modeling_tab(world).and_then(|el| world.resource::<AnalysisViews>().per.get(&el).cloned()).unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------
// The tool, its menu, and turning the analyses on and off

/// The bottom-right Analysis tool: its menu.
fn on_tool(a: On<Activate>, q: Query<&Name>, mut commands: Commands) {
    if q.get(a.entity).map(|n| n.as_str()) != Ok("view-analysis") {
        return;
    }
    let anchor = a.entity;
    commands.queue(move |world: &mut World| {
        if modeling_tab(world).is_none() {
            return;
        }
        let st = active_state(world);
        let theme = world.resource::<Theme>().clone();
        let check = |on: bool, item: MenuItem| if on { item.icon("check") } else { item };
        let menu = cadrs_ui::Menu::new("analysis-menu")
            .align_end()
            .side(bevy::ui_widgets::popover::PopoverSide::Top)
            .min_width(190.0)
            .item(MenuItem::new("analysis-draft", if st.draft.is_some() { "Exit draft analysis" } else { "Draft analysis…" }).icon("draft"))
            .separator()
            .item(check(st.curvature, MenuItem::new("analysis-curvature", "Curvature")))
            .item(check(st.zebra, MenuItem::new("analysis-zebra", "Zebra stripes")));
        let mut commands = world.commands();
        open_menu(&mut commands, anchor, menu.build(&theme));
        world.flush();
    });
}

fn on_menu(ev: On<MenuAction>, q: Query<&Name>, mut commands: Commands) {
    if q.get(ev.entity).map(|n| n.as_str()) != Ok("view-analysis") {
        return;
    }
    match ev.item.as_str() {
        "analysis-draft" => commands.queue(toggle_draft),
        "analysis-curvature" => commands.queue(|world: &mut World| {
            let on = !active_state(world).curvature;
            set_curvature(world, on);
        }),
        "analysis-zebra" => commands.queue(|world: &mut World| {
            let on = !active_state(world).zebra;
            set_zebra(world, on);
        }),
        _ => {}
    }
}

fn with_state(world: &mut World, f: impl FnOnce(&mut AnalysisState)) {
    let Some(el) = modeling_tab(world) else { return };
    let mut views = world.resource_mut::<AnalysisViews>();
    let s = views.per.entry(el).or_default();
    f(s);
    if *s == AnalysisState::default() {
        views.per.remove(&el);
    }
}

/// Turns the curvature combs on or off on the active tab.
pub fn set_curvature(world: &mut World, on: bool) {
    with_state(world, |s| s.curvature = on);
}

/// Turns the zebra stripes on or off on the active tab (on ends a draft analysis).
pub fn set_zebra(world: &mut World, on: bool) {
    if on && active_state(world).draft.is_some() {
        exit_draft(world);
    }
    with_state(world, |s| s.zebra = on);
}

/// The menu's Draft analysis… (opens the dialog) or Exit draft analysis.
pub fn toggle_draft(world: &mut World) {
    let Some(el) = modeling_tab(world) else { return };
    if world.resource::<AnalysisViews>().dialog == Some(el) {
        return;
    }
    if active_state(world).draft.is_some() {
        exit_draft(world);
    } else {
        open_draft(world);
    }
}

/// Opens the Draft analysis dialog on the active tab (the analysis shows at once, pulled along
/// the Top plane's normal, or along what is selected if it gives a direction).
pub fn open_draft(world: &mut World) {
    let Some(el) = modeling_tab(world) else { return };
    let picked = world.resource::<Selection>().0.iter().rev().find_map(|p| resolve(world, *p));
    let took = picked.is_some();
    with_state(world, |s| {
        s.zebra = false;
        let d = s.draft.get_or_insert_with(DraftState::default);
        if let Some(p) = picked {
            *d = DraftState { flip: false, angle: d.angle, ..p };
        }
    });
    if took {
        world.resource_mut::<Selection>().0.clear();
    }
    world.resource_mut::<AnalysisViews>().dialog = Some(el);
    let kind = *world.resource::<ActiveKind>();
    let planes = world.resource::<PlanesVisible>().0;
    let filter = if kind == ActiveKind::Assembly {
        PickFilter { faces: true, edges: true, ..PickFilter::none() }
    } else {
        PickFilter { planes, faces: true, edges: true, plane_features: true, ..PickFilter::none() }
    };
    world.resource_mut::<PickFilterOverride>().0 = Some(filter);
}

fn close_dialog(world: &mut World) {
    if world.resource_mut::<AnalysisViews>().dialog.take().is_some() {
        world.resource_mut::<PickFilterOverride>().0 = None;
    }
}

/// Ends the active tab's draft analysis.
pub fn exit_draft(world: &mut World) {
    close_dialog(world);
    with_state(world, |s| s.draft = None);
}

/// The pull direction a pick gives, if it gives one: a plane's or a planar face's normal, a
/// straight edge's direction, a cylinder's or cone's axis.
fn resolve(world: &World, pick: Pick) -> Option<DraftState> {
    let cache = world.resource::<PartCache>();
    let v3 = |p: [f64; 3]| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
    let label_of = |pick: Pick| {
        let doc = world.get_resource::<ActiveDocument>();
        let features = doc.as_ref().and_then(|d| d.active_element()).map(|e| e.features()).unwrap_or(&[]);
        crate::parts::pick_label(features, cache, pick)
    };
    let (reference, label, pull, anchor) = match pick {
        Pick::Plane(k) => (PullRef::Plane(k), format!("{} plane", k.name()), k.normal(), Vec3::ZERO),
        Pick::Face(part, name) => {
            let p = cache.part(part)?;
            let i = p.solid.faces.iter().position(|f| f.name == name)?;
            let f = &p.solid.faces[i];
            let dir = if f.plane.is_some() { v3(p.solid.face_normal(i)?) } else { v3(f.axis?.1) };
            let at = f.center.or_else(|| p.solid.face_point(i)).map(v3).unwrap_or(Vec3::ZERO);
            let label = label_of(pick).filter(|l| l != "Face").or_else(|| cache.part_name(part).map(|n| format!("Face of {n}"))).unwrap_or_else(|| "Face".into());
            (PullRef::Face(part, name), label, dir, at)
        }
        Pick::Edge(part, name) => {
            let e = cache.part(part)?.solid.edge(&name)?;
            let (a, b) = (v3(*e.points.first()?), v3(*e.points.last()?));
            let dir = b - a;
            let straight = dir.length() > 1e-4 && e.points.iter().all(|p| (v3(*p) - a).cross(dir).length() / dir.length() < 1e-3);
            if !straight {
                return None;
            }
            (PullRef::Edge(part, name), label_of(pick).unwrap_or_else(|| "Edge".into()), dir, (a + b) / 2.0)
        }
        Pick::Feature(f) => {
            let doc = world.get_resource::<ActiveDocument>()?;
            let feature = doc.active_element()?.feature(f)?;
            let frame = match cache.planes.get(&f) {
                Some(frame) => *frame,
                None => feature.sketch()?.plane?.frame(),
            };
            (PullRef::Feature(f), feature.name.clone(), v3(frame.normal()), v3(frame.origin))
        }
        _ => return None,
    };
    let pull = pull.normalize_or_zero();
    (pull != Vec3::ZERO).then(|| DraftState { reference: Some(reference), label, pull, anchor, ..default() })
}

/// The dialog's picks in the view (the pick filter is overridden, so they don't reach the
/// selection on their own).
fn take_picks(mut picks: MessageReader<PickRequest>, views: Res<AnalysisViews>, mut selection: ResMut<Selection>) {
    if views.dialog.is_none() {
        return;
    }
    if let Some(pick) = picks.read().filter_map(|p| p.0).last() {
        selection.0 = vec![pick];
    }
}

/// While the dialog is open, a pick (in the view, or a plane clicked in the feature list) sets
/// the pull direction; it is not left selected (the field names it).
fn follow_selection(selection: Res<Selection>, views: Res<AnalysisViews>, mut commands: Commands) {
    if views.dialog.is_none() || !selection.is_changed() || selection.0.is_empty() {
        return;
    }
    let picks = selection.0.clone();
    commands.queue(move |world: &mut World| {
        let Some(el) = world.resource::<AnalysisViews>().dialog else { return };
        let Some(state) = picks.iter().rev().find_map(|p| resolve(world, *p)) else { return };
        let mut views = world.resource_mut::<AnalysisViews>();
        if let Some(d) = views.per.entry(el).or_default().draft.as_mut() {
            *d = DraftState { flip: d.flip, angle: d.angle, ..state };
        }
        world.resource_mut::<Selection>().0.clear();
    });
}

fn on_accept(ev: On<FeatureDialogAccept>, q: Query<(), With<DraftDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(close_dialog);
    }
}

fn on_cancel(ev: On<FeatureDialogCancel>, q: Query<(), With<DraftDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(exit_draft);
    }
}

fn on_angle(ev: On<NumberFieldCommit>, q: Query<&Name>, mut commands: Commands) {
    if q.get(ev.entity).map(|n| n.as_str()) != Ok("draft-angle") {
        return;
    }
    let text = ev.text.clone();
    commands.queue(move |world: &mut World| {
        let Some(deg) = parse_degrees(&text) else { return };
        let mut views = world.resource_mut::<AnalysisViews>();
        let Some(el) = views.dialog else { return };
        if let Some(d) = views.per.get_mut(&el).and_then(|s| s.draft.as_mut()) {
            d.angle = deg.clamp(0.0, 89.0);
        }
    });
}

/// "3", "3 deg", "3°" → 3.
fn parse_degrees(text: &str) -> Option<f32> {
    let t = text.trim().trim_end_matches("deg").trim_end_matches('°').trim();
    t.parse::<f32>().ok().filter(|v| v.is_finite())
}

fn flip(world: &mut World) {
    let mut views = world.resource_mut::<AnalysisViews>();
    let Some(el) = views.dialog else { return };
    if let Some(d) = views.per.get_mut(&el).and_then(|s| s.draft.as_mut()) {
        d.flip = !d.flip;
    }
}

// ---------------------------------------------------------------------------------------------
// The dialog and the legend

#[derive(Component)]
struct DraftDialog(String);

/// An angle as the dialog and legend show it ("3°", "2.5°").
fn degrees_text(v: f32) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{s}°")
}

fn draft_dialog(t: &Theme, d: &DraftState) -> impl Bundle {
    let tb = t.clone();
    let items = if d.reference.is_some() { vec![d.label.clone()] } else { Vec::new() };
    let (flipped, angle) = (d.flip, degrees_text(d.angle));
    FeatureDialog::new("draft-analysis-dialog")
        .title("Draft analysis")
        .valid(true)
        .width(240.0)
        .body(move |b| {
            b.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(2.0), ..default() }).with_children(|r| {
                r.spawn(SelectionList::new("draft-pull").placeholder("Pull direction").items(items).active(true).build(&tb))
                    .entry::<Node>()
                    .and_modify(|mut n| {
                        n.flex_grow = 1.0;
                        n.margin = UiRect::vertical(Val::Px(2.0));
                    });
                r.spawn((
                    IconButton::new("draft-flip", crate::extrude_dialog::flip_icon(flipped)).icon_size(20.0).tooltip("Flip the pull direction").build(&tb),
                    observe(|_: On<Activate>, mut commands: Commands| commands.queue(flip)),
                ))
                .entry::<Node>()
                .and_modify(|mut n| {
                    n.width = Val::Px(22.0);
                    n.height = Val::Px(22.0);
                    n.flex_shrink = 0.0;
                });
            });
            b.spawn(NumberField::new("draft-angle", "Draft angle").text(angle).label_width(80.0).build(&tb));
        })
        .build(t)
}

fn sync_dialog(views: Res<AnalysisViews>, doc: Option<Res<ActiveDocument>>, theme: Res<Theme>, q_dialog: Query<(Entity, &DraftDialog)>, q_area: Query<Entity, With<ViewportArea>>, mut commands: Commands) {
    let open = views.dialog.filter(|el| doc.as_ref().and_then(|d| d.active) == Some(*el));
    let state = open.and_then(|el| views.per.get(&el)).and_then(|s| s.draft.clone());
    let Some(d) = state else {
        for (e, _) in &q_dialog {
            commands.entity(e).try_despawn();
        }
        return;
    };
    let key = format!("{}|{}|{}|{}", d.reference.is_some(), d.label, d.flip, d.angle);
    if let Some((e, dd)) = q_dialog.iter().next() {
        if dd.0 == key {
            return;
        }
        commands.entity(e).try_despawn();
    }
    let Some(area) = q_area.iter().next() else { return };
    let e = commands.spawn((DraftDialog(key), DespawnOnExit(AppState::Document), draft_dialog(&theme, &d))).id();
    commands.entity(area).add_child(e);
}

/// The draft analysis's legend at the right of the view.
#[derive(Component)]
struct Legend(String);

fn sync_legend(views: Res<AnalysisViews>, doc: Option<Res<ActiveDocument>>, kind: Res<ActiveKind>, theme: Res<Theme>, q: Query<(Entity, &Legend)>, q_area: Query<Entity, With<ViewportArea>>, mut commands: Commands) {
    let modeling = matches!(*kind, ActiveKind::PartStudio | ActiveKind::Assembly);
    let el = doc.as_ref().and_then(|d| d.active).filter(|_| modeling);
    let draft = el.and_then(|el| views.per.get(&el)).and_then(|s| s.draft.clone());
    let Some(d) = draft else {
        for (e, _) in &q {
            commands.entity(e).try_despawn();
        }
        return;
    };
    let key = format!("{}|{}|{}", d.label, d.flip, d.angle);
    if let Some((e, l)) = q.iter().next() {
        if l.0 == key {
            return;
        }
        commands.entity(e).try_despawn();
    }
    let Some(area) = q_area.iter().next() else { return };
    let t = theme.clone();
    let required = d.angle as f64;
    let subtitle = format!("{}{} · {}", d.label, if d.flip { " (flipped)" } else { "" }, degrees_text(d.angle));
    let legend = commands
        .spawn((
            Name::new("draft-legend"),
            Legend(key),
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(14.0),
                top: Val::Px(230.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(3.0),
                padding: UiRect::all(Val::Px(8.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(t.background.with_alpha(0.94)),
            BorderColor::all(t.separator),
            Pickable::IGNORE,
            DespawnOnExit(AppState::Document),
        ))
        .with_children(|p| {
            p.spawn((Name::new("draft-legend-title"), t.text("Draft analysis", t.font_sm, FontWeight::BOLD, t.foreground), Pickable::IGNORE));
            p.spawn((
                Name::new("draft-legend-pull"),
                t.text(subtitle, 11.0, FontWeight::NORMAL, t.muted_foreground),
                Node { margin: UiRect::bottom(Val::Px(3.0)), ..default() },
                Pickable::IGNORE,
            ));
            for (i, band) in DraftBand::ALL.into_iter().enumerate() {
                let [r, g, b] = band.color();
                p.spawn((Name::new(format!("draft-legend-{i}")), Node { align_items: AlignItems::Center, column_gap: Val::Px(6.0), ..default() }, Pickable::IGNORE)).with_children(|row| {
                    row.spawn((
                        Node { width: Val::Px(14.0), height: Val::Px(12.0), border: UiRect::all(Val::Px(1.0)), ..default() },
                        BackgroundColor(Color::srgb_u8(r, g, b)),
                        BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.25)),
                        Pickable::IGNORE,
                    ));
                    row.spawn((t.text(band.label(required), 11.5, FontWeight::NORMAL, t.foreground), Pickable::IGNORE));
                });
            }
        })
        .id();
    commands.entity(area).add_child(legend);
}

/// What the part material colours the faces by on the active tab.
fn sync_shading_analysis(views: Res<AnalysisViews>, doc: Option<Res<ActiveDocument>>, kind: Res<ActiveKind>, mut out: ResMut<ShadingAnalysis>) {
    let modeling = matches!(*kind, ActiveKind::PartStudio | ActiveKind::Assembly);
    let st = doc.as_ref().and_then(|d| d.active).filter(|_| modeling).and_then(|el| views.per.get(&el));
    let want = match st {
        Some(AnalysisState { draft: Some(d), .. }) => ShadingAnalysis { mode: 2, angle: d.angle, pull: d.direction() },
        Some(AnalysisState { zebra: true, .. }) => ShadingAnalysis { mode: 1, ..default() },
        _ => ShadingAnalysis::default(),
    };
    if *out != want {
        *out = want;
    }
}

/// The pull direction's arrow (the shared 3D drag arrow, [`crate::manipulator`]) at its
/// reference while the draft analysis is on; it turns round with the flip.
#[derive(Component)]
struct PullArrow;

/// The pull arrow's length on screen (logical px).
const PULL_ARROW_PX: f32 = 72.0;

fn sync_pull_arrow(views: Res<AnalysisViews>, doc: Option<Res<ActiveDocument>>, kind: Res<ActiveKind>, mut q: Query<(Entity, &mut crate::manipulator::Arrow3d), With<PullArrow>>, mut commands: Commands) {
    let modeling = matches!(*kind, ActiveKind::PartStudio | ActiveKind::Assembly);
    let draft = doc.as_ref().and_then(|d| d.active).filter(|_| modeling).and_then(|el| views.per.get(&el)).and_then(|s| s.draft.clone());
    let Some(d) = draft else {
        for (e, _) in &q {
            commands.entity(e).try_despawn();
        }
        return;
    };
    let a = crate::manipulator::Arrow3d { base: d.anchor, dir: d.direction(), length_px: PULL_ARROW_PX, hot: false };
    match q.iter_mut().next() {
        Some((_, mut cur)) => {
            if *cur != a {
                *cur = a;
            }
        }
        None => {
            commands.spawn((crate::manipulator::arrow("draft-pull-arrow", a), PullArrow, DespawnOnExit(AppState::Document)));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Curvature combs

#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct CurvatureGizmos;

fn configure_gizmos(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<CurvatureGizmos>();
    config.line.width = 1.5;
    config.depth_bias = -0.5;
}

/// The longest tooth, in logical px.
const COMB_PX: f32 = 46.0;
/// The spacing of the teeth along an edge on screen (logical px).
const TOOTH_GAP_PX: f32 = 7.0;
/// A cap on the edges combed (a part with many curved edges).
const MAX_COMBED_EDGES: usize = 400;

/// The comb's teeth and the line through their tips.
const TOOTH: Color = Color::srgb(0.42, 0.28, 0.82);
const ENVELOPE: Color = Color::srgb(0.30, 0.16, 0.66);

fn draw_combs(views: Res<AnalysisViews>, doc: Option<Res<ActiveDocument>>, kind: Res<ActiveKind>, cache: Res<PartCache>, selection: Res<Selection>, view: Res<ViewportView>, mut g: Gizmos<CurvatureGizmos>) {
    let modeling = matches!(*kind, ActiveKind::PartStudio | ActiveKind::Assembly);
    let on = doc.as_ref().and_then(|d| d.active).filter(|_| modeling).and_then(|el| views.per.get(&el)).is_some_and(|s| s.curvature);
    if !on {
        return;
    }
    // The selected edges, or every curved edge of the parts shown.
    let picked: Vec<(PartId, EdgeName)> = selection.0.iter().filter_map(|p| if let Pick::Edge(part, e) = p { Some((*part, *e)) } else { None }).collect();
    let mut combs = Vec::new();
    if picked.is_empty() {
        for p in cache.shown() {
            for e in &p.solid.edges {
                if combs.len() >= MAX_COMBED_EDGES {
                    break;
                }
                let c = curvature_comb(&e.points);
                if c.iter().any(|t| t.curvature > 1e-9) {
                    combs.push(c);
                }
            }
        }
    } else {
        for (part, name) in picked {
            if let Some(e) = cache.part(part).and_then(|p| p.solid.edge(&name)) {
                combs.push(curvature_comb(&e.points));
            }
        }
    }
    let max = combs.iter().flatten().map(|t| t.curvature).fold(0.0_f64, f64::max);
    if max <= 1e-9 {
        return;
    }
    // mm of tooth per unit of curvature, so the longest is COMB_PX on screen.
    let scale = (COMB_PX * view.view.scale) as f64 / max;
    let v3 = |p: [f64; 3]| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
    for comb in &combs {
        let tips: Vec<Vec3> = comb.iter().map(|t| v3(t.tip(scale))).collect();
        // A tooth every TOOTH_GAP_PX or so along the edge on screen, so a finely tessellated
        // edge's comb stays readable (the envelope keeps every point).
        let mut since: Option<Vec2> = None;
        for (i, (t, tip)) in comb.iter().zip(&tips).enumerate() {
            let at = v3(t.at);
            let px = view.view.project(at);
            let due = since.is_none_or(|p| p.distance(px) >= TOOTH_GAP_PX) || i + 1 == comb.len();
            if t.curvature > 1e-9 && due {
                g.line(at, *tip, TOOTH);
                since = Some(px);
            }
        }
        g.linestrip(tips, ENVELOPE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angles_read_and_parse() {
        assert_eq!(degrees_text(3.0), "3°");
        assert_eq!(degrees_text(2.5), "2.5°");
        assert_eq!(parse_degrees("5 deg"), Some(5.0));
        assert_eq!(parse_degrees("4.5°"), Some(4.5));
        assert_eq!(parse_degrees("x"), None);
    }

    #[test]
    fn the_shading_uniforms() {
        let s = ShadingAnalysis { mode: 2, angle: 3.0, pull: Vec3::Z };
        let (a, p) = s.uniforms();
        assert_eq!(a.x, 2.0);
        assert_eq!(a.y, 3.0);
        assert_eq!(p.truncate(), Vec3::Z);
        // A flipped pull points the other way.
        let d = DraftState { flip: true, ..default() };
        assert_eq!(d.direction(), -Vec3::Z);
        assert_eq!(band_colors().len(), 6);
    }
}
