//! Use (S20, U) and Pierce (S12.11, Shift+G): picking geometry outside the sketch.
//!
//! - **Use:** hovering a part edge, another sketch's curve, a planar face (all its edges) or a
//!   curved face (its silhouettes seen along the sketch normal, or the one under the pointer)
//!   highlights the source and previews its projection in the sketch plane
//!   (`reference/onshape/t5/useexample1.png`: "a preview of the projected lines will appear").
//!   Clicking projects it ([`SketchOp::Use`]); the projected curves are fixed, drawn black,
//!   with a light-blue link glyph that can be deleted.
//! - **Pierce:** with the Pierce constraint tool, a sketch point and a part edge (or another
//!   sketch's curve) that crosses the sketch plane, in either order: the point is held where
//!   the curve pierces the plane.
//! - **Part vertices:** selecting, or with a constraint tool, a part corner under the pointer
//!   is hovered and picked (Shift adds it) like a sketch point. A constraint to it uses it
//!   first: a point held where the vertex projects, with a Use link, constrained to as any
//!   point ([`cadrs_sketch::constraint::fit_op`]).
//!
//! Only parts and sketches made before the edited sketch can be used, as in Onshape.

use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::{PointerAction, PointerButton, PointerId, PointerInput};
use bevy::prelude::*;
use cadrs_core::links::{LinkContext, face_edges, silhouettes};
use cadrs_core::{Feature, FeatureId, Solid};
use cadrs_sketch::projection::Projected;
use cadrs_sketch::{
    ConstraintKind, ConstraintOf, Link, PlaneFrame, PointRef, SketchEntity, SketchOp,
};

use crate::camera::ViewState;
use crate::parts::{FaceOutlineGizmos, PartCache, pick_face, pick_vertex};
use crate::sketch::{ActiveSketchTool, PartStudioMode, SketchSession, SketchTool};
use crate::sketch_tools::{SketchSelection, SketchToolsSet, execute, over_viewport};
use crate::viewport::{ViewportArea, ViewportRect, ViewportView};
use crate::{ActiveDocument, AppState};

pub struct SketchLinksPlugin;

impl Plugin for SketchLinksPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LinkPick>()
            .add_systems(
                Update,
                link_pointer
                    .in_set(SketchToolsSet)
                    .after(crate::sketch_tools::sketch_pointer)
                    .run_if(in_state(PartStudioMode::Sketching))
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(
                Update,
                draw_link_hover
                    .after(crate::sketch_draw::SketchDrawSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(
                Update,
                draw_vertex_picks
                    .after(crate::sketch_draw::SketchDrawSet)
                    .run_if(in_state(PartStudioMode::Sketching))
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(
                Update,
                shade_hovered_face
                    .after(crate::sketch_draw::SketchDrawSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(PartStudioMode::Sketching), reset);
    }
}

/// What the pointer would use or pierce.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkHover {
    /// The projected shapes and their links (Use).
    pub items: Vec<(Projected, Link)>,
    /// The source in space, as polylines (highlighted).
    pub source: Vec<Vec<Vec3>>,
    /// The sketch plane (for drawing the preview).
    pub frame: PlaneFrame,
    /// A hovered face (Use of a whole face): shaded translucent orange.
    pub face: Option<(FeatureId, cadrs_sketch::FaceName)>,
}

#[derive(Resource, Debug, Default)]
pub struct LinkPick {
    pub hover: Option<LinkHover>,
    /// The Pierce tool's curve, when it was picked before the point.
    pub pending: Option<Link>,
    press: Option<Vec2>,
    cursor: Option<Vec2>,
    last_tool: SketchTool,
}

fn reset(mut pick: ResMut<LinkPick>) {
    *pick = LinkPick::default();
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
}

/// Pixels within which an edge or curve is under the pointer.
const EDGE_PX: f32 = 6.0;

fn dist_to_polyline(p: Vec2, pts: &[Vec2]) -> f32 {
    pts.windows(2)
        .map(|w| {
            let (a, b) = (w[0], w[1]);
            let d = b - a;
            let t = ((p - a).dot(d) / d.length_squared().max(1e-9)).clamp(0.0, 1.0);
            p.distance(a + d * t)
        })
        .fold(f32::INFINITY, f32::min)
}

/// The parts and features the edited sketch may use: those before it.
fn before<'a>(features: &'a [Feature], sketch: FeatureId, cache: &'a PartCache) -> (&'a [Feature], Vec<(FeatureId, &'a Solid)>) {
    let i = features.iter().position(|f| f.id == sketch).unwrap_or(features.len());
    let earlier = &features[..i];
    let solids = cache
        .parts
        .iter()
        // P3B.9: the assembly context's parts are references too.
        .filter(|p| earlier.iter().any(|f| f.id == p.feature) || cadrs_core::assembly::context::is_context(p.feature))
        .map(|p| (p.feature, &*p.solid))
        .collect();
    (earlier, solids)
}

/// The edge or curve under the pointer: its link and its polyline in space.
fn curve_under(
    earlier: &[Feature],
    solids: &[(FeatureId, &Solid)],
    cache: &PartCache,
    view: &ViewState,
    rect: &ViewportRect,
    cursor: Vec2,
    accept: &dyn Fn(Link) -> bool,
) -> Option<(Link, Vec<Vec3>)> {
    let screen = |p: Vec3| rect.to_screen(view.project(p));
    let back = view.back();
    // (distance, depth toward the viewer, link, polyline)
    let mut best: Option<(f32, f32, Link, Vec<Vec3>)> = None;
    let mut consider = |link: Link, pts: Vec<Vec3>| {
        if !accept(link) {
            return;
        }
        let sp: Vec<Vec2> = pts.iter().map(|p| screen(*p)).collect();
        let d = dist_to_polyline(cursor, &sp);
        if d > EDGE_PX {
            return;
        }
        let depth = pts.iter().map(|p| p.dot(back)).fold(f32::MIN, f32::max);
        let better = match &best {
            None => true,
            Some((bd, bdepth, ..)) => d < bd - 1.0 || (d <= bd + 1.0 && depth > *bdepth + 1e-3),
        };
        if better {
            best = Some((d, depth, link, pts));
        }
    };
    for (feature, solid) in solids {
        for e in &solid.edges {
            consider(
                Link::Edge {
                    feature: feature.0,
                    edge: e.name,
                },
                e.points.iter().map(|p| v3(*p)).collect(),
            );
        }
    }
    for f in earlier {
        let Some(sk) = f.sketch() else { continue };
        let Some(plane) = sk.plane else { continue };
        if cache.hidden_sketches.contains(&f.id) {
            continue;
        }
        let frame = plane.frame();
        for c in sk.geometry.curves.keys() {
            let pts = cadrs_sketch::hit::curve_polyline(&sk.geometry, c)
                .into_iter()
                .map(|p| v3(frame.to_world(p)))
                .collect();
            consider(
                Link::SketchCurve {
                    feature: f.id.0,
                    curve: c,
                },
                pts,
            );
        }
    }
    best.map(|(_, _, l, p)| (l, p))
}

/// What Use would take under the pointer.
#[allow(clippy::too_many_arguments)]
fn use_hover(
    features: &[Feature],
    sketch: FeatureId,
    frame: PlaneFrame,
    cache: &PartCache,
    view: &ViewState,
    rect: &ViewportRect,
    cursor: Vec2,
) -> Option<LinkHover> {
    let (earlier, solids) = before(features, sketch, cache);
    let ctx = LinkContext {
        solids: solids.clone(),
        features: earlier,
    };
    let usable = |l: Link| ctx.shape(l, &frame).is_some();
    if let Some((link, pts)) = curve_under(earlier, &solids, cache, view, rect, cursor, &usable) {
        let shape = ctx.shape(link, &frame)?;
        return Some(LinkHover {
            items: vec![(shape, link)],
            source: vec![pts],
            frame,
            face: None,
        });
    }
    // A face: its whole outline (all its edges), and a curved face's silhouettes too.
    let (part, tag, _) = pick_face(cache, view, rect.offset(cursor))?;
    let feature = part.feature;
    let solid = cache.part(part)?.solid.as_ref();
    solids.iter().find(|(f, _)| *f == feature)?;
    let face = solid.face(&tag)?;
    let mut items: Vec<(Projected, Link)> = Vec::new();
    let mut source = Vec::new();
    for edge in face_edges(solid, &tag) {
        let link = Link::Edge {
            feature: feature.0,
            edge,
        };
        if let Some(shape) = ctx.shape(link, &frame) {
            items.push((shape, link));
            if let Some(e) = solid.edge(&edge) {
                source.push(e.points.iter().map(|p| v3(*p)).collect());
            }
        }
    }
    if face.plane.is_none() {
        for (i, (a, b)) in silhouettes(solid, &tag, frame.normal()).iter().enumerate() {
            let link = Link::Silhouette {
                feature: feature.0,
                face: tag,
                index: i as u8,
            };
            // A silhouette along one of the face's edges (a cylinder's seam seen side on) is
            // that edge.
            if let Some(shape) = ctx.shape(link, &frame)
                && !items.iter().any(|(k, _)| same_shape(k, &shape))
            {
                items.push((shape, link));
                source.push(vec![v3(*a), v3(*b)]);
            }
        }
    }
    (!items.is_empty()).then_some(LinkHover {
        items,
        source,
        frame,
        face: Some((feature, tag)),
    })
}

/// True if two projected shapes are the same curve (a line either way round).
fn same_shape(a: &Projected, b: &Projected) -> bool {
    const EPS: f64 = 1e-6;
    match (a, b) {
        (&Projected::Line(p, q), &Projected::Line(r, s)) => {
            (p.distance(r) < EPS && q.distance(s) < EPS) || (p.distance(s) < EPS && q.distance(r) < EPS)
        }
        _ => a == b,
    }
}

/// The part vertex under the pointer, for a constraint to take (as Onshape lets a sketch
/// constrain to a model vertex): a visible vertex of a part made before the sketch.
pub fn vertex_under(
    features: &[Feature],
    sketch: FeatureId,
    cache: &PartCache,
    view: &ViewState,
    rect: &ViewportRect,
    cursor: Vec2,
) -> Option<Link> {
    let (part, vertex, _) = pick_vertex(cache, view, rect.offset(cursor))?;
    let feature = cache.part(part)?.feature;
    let (_, solids) = before(features, sketch, cache);
    solids
        .iter()
        .any(|(f, _)| *f == feature)
        .then_some(Link::Vertex { feature: feature.0, vertex })
}

/// Where a part vertex is in the sketch plane (seen along its normal), and in space; `None` if
/// it is gone.
pub fn vertex_at(
    features: &[Feature],
    sketch: FeatureId,
    frame: &PlaneFrame,
    cache: &PartCache,
    link: Link,
) -> Option<(cadrs_sketch::Vec2, Vec3)> {
    let Link::Vertex { feature, vertex } = link else { return None };
    let (earlier, solids) = before(features, sketch, cache);
    let ctx = LinkContext {
        solids: solids.clone(),
        features: earlier,
    };
    let at = ctx.pierce(link, frame, cadrs_sketch::Vec2::ZERO)?;
    let point = solids
        .iter()
        .filter(|(f, _)| f.0 == feature)
        .find_map(|(_, s)| s.vertex(&vertex))
        .or_else(|| solids.iter().find_map(|(_, s)| s.vertex(&vertex)))?
        .point;
    Some((at, v3(point)))
}

/// [`vertex_at`] in the plane of the edited sketch, for
/// [`cadrs_sketch::constraint::fit_op`].
pub fn sketch_vertex_at<'a>(
    doc: &'a ActiveDocument,
    s: &SketchSession,
    cache: &'a PartCache,
) -> impl Fn(Link) -> Option<cadrs_sketch::Vec2> + 'a {
    let features = doc.doc.element(s.element).map_or(&[][..], |el| el.features());
    let sketch = s.feature;
    let frame = features
        .iter()
        .find(|f| f.id == sketch)
        .and_then(|f| f.sketch()?.plane)
        .map(|p| p.frame());
    move |l| vertex_at(features, sketch, &frame?, cache, l).map(|(p, _)| p)
}

/// What Pierce would take under the pointer: an edge or curve that crosses the plane.
fn pierce_hover(
    features: &[Feature],
    sketch: FeatureId,
    frame: PlaneFrame,
    cache: &PartCache,
    view: &ViewState,
    rect: &ViewportRect,
    cursor: Vec2,
) -> Option<(Link, LinkHover)> {
    let (earlier, solids) = before(features, sketch, cache);
    let ctx = LinkContext {
        solids: solids.clone(),
        features: earlier,
    };
    let crosses = |l: Link| ctx.pierce(l, &frame, cadrs_sketch::Vec2::ZERO).is_some();
    let (link, pts) = curve_under(earlier, &solids, cache, view, rect, cursor, &crosses)?;
    let at = ctx.pierce(link, &frame, cadrs_sketch::Vec2::ZERO)?;
    Some((
        link,
        LinkHover {
            items: vec![(Projected::Point(at), link)],
            source: vec![pts],
            frame,
            face: None,
        },
    ))
}

#[allow(clippy::too_many_arguments)]
fn link_pointer(
    mut inputs: MessageReader<PointerInput>,
    hover_map: Res<HoverMap>,
    q_area: Query<Entity, With<ViewportArea>>,
    session: Option<Res<SketchSession>>,
    doc: Option<Res<ActiveDocument>>,
    tool: Res<ActiveSketchTool>,
    cache: Res<PartCache>,
    view: Res<ViewportView>,
    rect: Res<ViewportRect>,
    mut selection: ResMut<SketchSelection>,
    mut pick: ResMut<LinkPick>,
    mut commands: Commands,
) {
    let pierce = tool.tool == SketchTool::Constrain(ConstraintKind::Pierce);
    let using = tool.tool == SketchTool::Use;
    if pick.last_tool != tool.tool {
        pick.last_tool = tool.tool;
        pick.pending = None;
    }
    let (Some(s), Some(doc)) = (session.as_deref(), doc.as_deref()) else {
        inputs.clear();
        return;
    };
    if !(pierce || using) || s.waiting_for_plane {
        inputs.clear();
        if pick.hover.is_some() {
            pick.hover = None;
        }
        return;
    }
    let Some(el) = doc.doc.element(s.element) else {
        return;
    };
    let features = el.features();
    let Some(frame) = features
        .iter()
        .find(|f| f.id == s.feature)
        .and_then(|f| f.sketch()?.plane)
        .map(|p| p.frame())
    else {
        return;
    };
    let target = (s.element, s.feature);
    let over = over_viewport(&hover_map, &q_area);
    let mut clicked = None;
    for input in inputs.read() {
        if input.pointer_id != PointerId::Mouse {
            continue;
        }
        let pos = input.location.position;
        match input.action {
            PointerAction::Move { .. } => pick.cursor = Some(pos),
            PointerAction::Press(PointerButton::Primary) => {
                pick.press = over.then_some(pos);
            }
            PointerAction::Release(PointerButton::Primary) => {
                if let Some(p) = pick.press.take()
                    && p.distance(pos) <= crate::sketch_tools::DRAG_THRESHOLD
                {
                    pick.cursor = Some(pos);
                    clicked = Some(pos);
                }
            }
            _ => {}
        }
    }
    // The sketch's own points win under the pointer (Pierce picks a point there).
    let sketch = &features.iter().find(|f| f.id == s.feature).and_then(|f| f.sketch());
    let own_point = |c: Vec2| {
        let map = crate::sketch_tools::ScreenMap::new(
            features.iter().find(|f| f.id == s.feature).and_then(|f| f.sketch()?.plane).unwrap_or_default(),
            &view.view,
            &rect,
        );
        sketch.and_then(|sk| {
            cadrs_sketch::hit::hit_test(&sk.geometry, cadrs_sketch::Vec2::new(c.x as f64, c.y as f64), |p| {
                map.to_screen64(p)
            })
        })
    };
    let hover = match pick.cursor.filter(|_| over) {
        Some(c) if using => use_hover(features, s.feature, frame, &cache, &view.view, &rect, c),
        Some(c) if own_point(c).is_none() => {
            pierce_hover(features, s.feature, frame, &cache, &view.view, &rect, c).map(|(_, h)| h)
        }
        _ => None,
    };
    if pick.hover != hover {
        pick.hover = hover;
    }
    let point = match selection.0.as_slice() {
        [SketchEntity::Point(p)] => Some(*p),
        _ => None,
    };
    if let Some(c) = clicked {
        if using {
            if let Some(h) = pick.hover.clone() {
                execute(&mut commands, target, SketchOp::Use { items: h.items });
            }
        } else if own_point(c).is_none()
            && let Some((link, _)) =
                pierce_hover(features, s.feature, frame, &cache, &view.view, &rect, c)
        {
            match point {
                Some(p) => {
                    add_pierce(&mut commands, target, p, link);
                    selection.0.clear();
                }
                None => pick.pending = Some(link),
            }
            return;
        }
    }
    // The curve was picked first: the point completes it.
    if pierce
        && let (Some(link), Some(p)) = (pick.pending, point)
    {
        add_pierce(&mut commands, target, p, link);
        selection.0.clear();
        pick.pending = None;
    }
}

fn add_pierce(
    commands: &mut Commands,
    target: (cadrs_core::ElementId, FeatureId),
    p: cadrs_sketch::PointId,
    link: Link,
) {
    execute(
        commands,
        target,
        SketchOp::AddConstraint {
            constraints: vec![ConstraintOf::Pierce(PointRef::Point(p), link)],
            label: ConstraintKind::Pierce.undo_label(),
        },
    );
}

/// The hovered source (3 px, over everything) and its projection in the sketch plane.
fn draw_link_hover(pick: Res<LinkPick>, view: Res<ViewportView>, mut g: Gizmos<FaceOutlineGizmos>) {
    let Some(h) = &pick.hover else { return };
    // World units per screen pixel.
    let px = view.view.scale;
    let color = Color::srgb_u8(0xf2, 0xb1, 0x3a);
    for pl in &h.source {
        g.linestrip(pl.iter().copied(), color);
    }
    let w = |p: cadrs_sketch::Vec2| v3(h.frame.to_world(p));
    for (shape, _) in &h.items {
        match *shape {
            Projected::Line(a, b) => g.line(w(a), w(b), color),
            Projected::Circle(c, r) => {
                let pts: Vec<Vec3> = (0..=72)
                    .map(|i| w(c + cadrs_sketch::Vec2::from_angle(i as f64 * std::f64::consts::TAU / 72.0) * r))
                    .collect();
                g.linestrip(pts, color);
            }
            Projected::Arc { center, start, end } => {
                let a = cadrs_sketch::ArcGeom::ccw(center, start, end);
                g.linestrip(a.tessellate(0.05, 8).into_iter().map(w), color);
            }
            Projected::Ellipse { center, major, minor } => {
                let e = cadrs_sketch::geom::EllipseGeom::new(center, major, minor);
                g.linestrip(e.tessellate(0.05, 32).into_iter().map(w), color);
            }
            Projected::EllipseOffset { center, major, minor, distance } => {
                let e = cadrs_sketch::geom::EllipseGeom::new(center, major, minor).with_offset(distance);
                g.linestrip(e.tessellate(0.05, 32).into_iter().map(w), color);
            }
            Projected::EllipseArc { center, major, minor, start, end } => {
                let e = cadrs_sketch::EllipseArcGeom::ccw(center, major, minor, start, end);
                g.linestrip(e.tessellate(0.05, 8).into_iter().map(w), color);
            }
            Projected::Spline { ref points, closed } => {
                let spans = cadrs_sketch::spline::spans(points, closed, None, None);
                g.linestrip(cadrs_sketch::spline::tessellate(&spans, 8).into_iter().map(w), color);
            }
            Projected::Point(p) => {
                // Where the edge pierces the plane: an orange disc about 9 px across, facing the
                // viewer (clear at any zoom).
                let rot = Quat::from_rotation_arc(Vec3::Z, view.view.back());
                for r in [1.0, 2.0, 3.0, 4.5] {
                    g.circle(Isometry3d::new(w(p), rot), r * px, color).resolution(24);
                }
            }
        }
    }
}

/// A hovered or selected part vertex (a constraint takes it): an orange disc on the vertex,
/// facing the viewer, larger when selected.
#[allow(clippy::too_many_arguments)]
fn draw_vertex_picks(
    hover: Res<crate::sketch_tools::SketchHover>,
    selection: Res<SketchSelection>,
    session: Option<Res<SketchSession>>,
    doc: Option<Res<ActiveDocument>>,
    cache: Res<PartCache>,
    view: Res<ViewportView>,
    mut g: Gizmos<FaceOutlineGizmos>,
) {
    let (Some(s), Some(doc)) = (session.as_deref(), doc.as_deref()) else { return };
    let links = selection.0.iter().map(|e| (*e, true)).chain(hover.0.map(|e| (e, false)));
    let features = doc.doc.element(s.element).map_or(&[][..], |el| el.features());
    let Some(frame) = features
        .iter()
        .find(|f| f.id == s.feature)
        .and_then(|f| f.sketch()?.plane)
        .map(|p| p.frame())
    else {
        return;
    };
    let px = view.view.scale;
    let rot = Quat::from_rotation_arc(Vec3::Z, view.view.back());
    for (e, selected) in links {
        let SketchEntity::Link(link) = e else { continue };
        let Some((_, at)) = vertex_at(features, s.feature, &frame, &cache, link) else { continue };
        let (color, radii): (Color, &[f32]) = if selected {
            (Color::srgb_u8(0xf6, 0xbc, 0x1a), &[1.0, 2.0, 3.0, 4.0, 5.0])
        } else {
            (Color::srgb_u8(0xf2, 0xb1, 0x3a), &[1.0, 2.0, 3.0, 4.0])
        };
        for r in radii {
            g.circle(Isometry3d::new(at, rot), r * px, color).resolution(24);
        }
    }
}

/// The translucent orange over a face that Use would take whole (like a picked part face).
#[derive(Component)]
struct UseFaceShade;

fn shade_hovered_face(
    pick: Res<LinkPick>,
    cache: Res<PartCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut shown: Local<Option<(FeatureId, cadrs_sketch::FaceName)>>,
    q: Query<Entity, With<UseFaceShade>>,
    mut commands: Commands,
) {
    let want = pick.hover.as_ref().and_then(|h| h.face);
    if *shown == want && want.is_some() != q.is_empty() {
        return;
    }
    *shown = want;
    for e in &q {
        commands.entity(e).try_despawn();
    }
    let Some((feature, tag)) = want else { return };
    let Some(solid) = cache.part_with_face(feature, &tag).map(|p| &p.solid) else {
        return;
    };
    let Some(face) = solid.face(&tag) else { return };
    let idx = &solid.indices[3 * face.first_triangle..3 * (face.first_triangle + face.triangle_count)];
    let positions: Vec<[f32; 3]> = idx.iter().map(|i| v3(solid.positions[*i as usize]).to_array()).collect();
    let indices: Vec<u32> = (0..positions.len() as u32).collect();
    let mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_indices(bevy::mesh::Indices::U32(indices));
    let material = materials.add(StandardMaterial {
        base_color: Color::srgba_u8(0xff, 0xc2, 0x7a, 0x80),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        double_sided: true,
        // Over the part's own face.
        depth_bias: 1000.0,
        ..default()
    });
    commands.spawn((
        Name::new("use-face-shade"),
        UseFaceShade,
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(material),
        Transform::IDENTITY,
        DespawnOnExit(AppState::Document),
    ));
}
