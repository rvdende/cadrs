//! The features of P3.8 in the app: **Linear**, **Circular** and **Curve pattern** (PS22–PS25),
//! **Mirror** (PS26) and the **Mate connector** (X11). They run in the applied features'
//! session ([`crate::applied`]: the toolbar button inserts "Linear pattern 1" (…) and opens its
//! dialog, one selection field at a time takes the view's picks, ✓/✕, one undo step), with
//! their dialogs in [`crate::pattern_dialog`].
//!
//! - **Mate connectors** are drawn as Onshape draws them (`ex5-step2.png`): a white disc with a
//!   dark rim in the connector's XY plane, a quarter of it light, and its X (red), Y (green) and
//!   Z (blue) axes. **K** shows or hides them (PS27.7). A connector field (a pattern's axis or
//!   direction, a mirror plane, a hole's places, a connector's origin) takes an explicit
//!   connector (clicked in the view or the feature list) or an implicit one: the connector of
//!   the face or edge under the pointer, drawn there while hovered (a flat face's centroid, a
//!   circular edge's centre).
//! - **Skip instances** (PS22.5): while the pattern's dialog is open with Skip instances on,
//!   every instance but the seed shows a grey dot where the seed's centre is copied to; a click
//!   on a dot skips it (it turns light blue and is listed as "(2, 0)"), a second click brings it
//!   back.
//!
//! What a click adds, by field: the parts, features (in the list, or any face they made) or
//! faces to pattern or mirror; a direction (an edge, a sketch line, a flat face's or a plane's
//! normal, a connector's Z); an axis (a circular edge, a face of revolution, a sketch circle or
//! line, a connector's Z, the origin); a path (edges, sketch curves, a whole sketch); a mirror
//! plane (a default or Plane feature, a flat face, a connector's XY plane).

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use cadrs_core::advanced::PathRef;
use cadrs_core::document::{AxisRef, DirectionRef, FaceRef};
use cadrs_core::mate::{ConnectorOrigin, ConnectorRef, MateConnectorFeature};
use cadrs_core::pattern::{MirrorFeature, MirrorPlane, PatternFeature, PatternKind, PatternType};
use cadrs_core::{Feature, FeatureId, FeatureKind, VertexRef};
use cadrs_sketch::{PlaneFrame, PlaneRef};

use crate::applied::{AppliedField, AppliedKind, AppliedSession, entity_of};
use crate::parts::PartCache;
use crate::viewport::{Pick, PlaneHighlight, Selection, ViewportView};
use crate::{ActiveDocument, AppState};

pub struct PatternPlugin;

impl Plugin for PatternPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(MateConnectorsShown(true)).add_systems(
            Update,
            (connector_key, sync_connectors_shown, draw_connectors, draw_instance_dots, draw_connector_hover, draw_connector_fields, draw_skip_box, draw_direction_arrows)
                .after(crate::parts::PartsSet)
                .run_if(in_state(AppState::Document)),
        );
    }
}

/// Whether mate connectors are drawn (K toggles it, PS27.7).
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MateConnectorsShown(pub bool);

/// True if `f` is a Mate connector feature.
pub fn is_connector(features: &[Feature], f: FeatureId) -> bool {
    features.iter().any(|x| x.id == f && matches!(x.kind, FeatureKind::MateConnector(_)))
}

fn is_sketch(features: &[Feature], f: FeatureId) -> bool {
    features.iter().any(|x| x.id == f && x.sketch().is_some())
}

fn plane_of(features: &[Feature], pick: Pick) -> Option<PlaneRef> {
    match pick {
        Pick::Plane(k) => Some(k.plane_ref()),
        Pick::Feature(f) => cadrs_core::parts::plane_feature_ref(features, f),
        _ => None,
    }
}

/// The pick that shows a connector reference in the view.
pub fn connector_pick(c: &ConnectorRef) -> Option<Pick> {
    match c {
        ConnectorRef::Feature(f) => Some(Pick::Feature(*f)),
        ConnectorRef::Implicit(o) => match o {
            ConnectorOrigin::Origin => Some(Pick::Origin),
            ConnectorOrigin::Face(f) => Some(Pick::Face(f.part, f.face)),
            ConnectorOrigin::Edge(e) => Some(Pick::Edge(e.part, e.edge)),
            ConnectorOrigin::Vertex(v) => Some(Pick::Vertex(v.part, v.vertex)),
            ConnectorOrigin::SketchPoint { sketch, point } => Some(Pick::SketchPoint(*sketch, *point)),
            ConnectorOrigin::SketchCurve { sketch, curve } => Some(Pick::SketchCurve(*sketch, *curve)),
        },
    }
}

/// The implicit connector a pick stands for (a face's, an edge's, a vertex's, a sketch point's
/// or the origin's).
pub fn implicit_origin(cache: &PartCache, pick: Pick) -> Option<ConnectorOrigin> {
    Some(match pick {
        Pick::Origin => ConnectorOrigin::Origin,
        Pick::Face(..) | Pick::Edge(..) => match entity_of(cache, pick)? {
            cadrs_core::applied::EdgeOrFace::Face(f) => ConnectorOrigin::Face(f),
            cadrs_core::applied::EdgeOrFace::Edge(e) => ConnectorOrigin::Edge(e),
        },
        Pick::Vertex(part, vertex) => {
            let point = cache.part(part)?.solid.vertex(&vertex)?.point;
            ConnectorOrigin::Vertex(VertexRef { part, vertex, point })
        }
        Pick::SketchPoint(sketch, point) => ConnectorOrigin::SketchPoint { sketch, point },
        // P3B.7 (A22.8): a sketch circle's centre, a line's midpoint.
        Pick::SketchCurve(sketch, curve) => ConnectorOrigin::SketchCurve { sketch, curve },
        _ => return None,
    })
}

/// A connector reference for a pick: an explicit connector clicked, else an implicit one.
pub fn connector_of(features: &[Feature], cache: &PartCache, pick: Pick) -> Option<ConnectorRef> {
    match pick {
        Pick::Feature(f) if is_connector(features, f) => Some(ConnectorRef::Feature(f)),
        p => implicit_origin(cache, p).map(ConnectorRef::Implicit),
    }
}

pub fn face_ref(cache: &PartCache, pick: Pick) -> Option<FaceRef> {
    match entity_of(cache, pick)? {
        cadrs_core::applied::EdgeOrFace::Face(f) => Some(f),
        _ => None,
    }
}

fn toggle<T: PartialEq>(list: &mut Vec<T>, x: T) {
    if let Some(i) = list.iter().position(|y| *y == x) {
        list.remove(i);
    } else {
        list.push(x);
    }
}

fn toggle_face(list: &mut Vec<FaceRef>, f: FaceRef) {
    match list.iter().position(|g| g.face == f.face) {
        Some(i) => {
            list.remove(i);
        }
        None => list.push(f),
    }
}

/// The pattern's or mirror's seeds for a pick, by its type: a part, a feature (a part feature
/// in the list, or the feature that made a clicked face), or a face.
fn toggle_seed(
    features: &[Feature],
    cache: &PartCache,
    ty: PatternType,
    parts: &mut Vec<cadrs_core::PartId>,
    feats: &mut Vec<FeatureId>,
    faces: &mut Vec<FaceRef>,
    pick: Pick,
) -> bool {
    match ty {
        PatternType::Part => match pick.part() {
            Some(p) => toggle(parts, p),
            None => return false,
        },
        PatternType::Feature => {
            let f = match pick {
                Pick::Feature(f) if features.iter().any(|x| x.id == f && x.is_part_feature()) => f,
                Pick::Face(_, face) if features.iter().any(|x| x.id.0 == face.op && x.is_part_feature()) => FeatureId(face.op),
                _ => return false,
            };
            toggle(feats, f);
            // In list order.
            feats.sort_by_key(|f| features.iter().position(|x| x.id == *f));
        }
        PatternType::Face => match face_ref(cache, pick) {
            Some(f) => toggle_face(faces, f),
            None => return false,
        },
    }
    true
}

/// A direction for a pick (PS23.1).
pub(crate) fn direction_of(features: &[Feature], cache: &PartCache, pick: Pick) -> Option<DirectionRef> {
    match pick {
        Pick::Plane(_) => plane_of(features, pick).map(DirectionRef::PlaneNormal),
        Pick::Feature(f) if is_connector(features, f) => Some(DirectionRef::Connector(ConnectorRef::Feature(f))),
        Pick::Feature(_) => plane_of(features, pick).map(DirectionRef::PlaneNormal),
        Pick::Face(..) => face_ref(cache, pick).map(DirectionRef::FaceNormal),
        Pick::Edge(..) => match entity_of(cache, pick)? {
            cadrs_core::applied::EdgeOrFace::Edge(e) => Some(DirectionRef::Edge(e)),
            _ => None,
        },
        Pick::SketchCurve(sketch, curve) => Some(DirectionRef::SketchLine { sketch, curve }),
        _ => None,
    }
}

/// An axis for a pick (PS24.1).
pub(crate) fn axis_of(features: &[Feature], cache: &PartCache, pick: Pick) -> Option<AxisRef> {
    match pick {
        Pick::Feature(f) if is_connector(features, f) => Some(AxisRef::Connector(ConnectorRef::Feature(f))),
        Pick::Origin => Some(AxisRef::Connector(ConnectorRef::Implicit(ConnectorOrigin::Origin))),
        Pick::Face(..) => face_ref(cache, pick).map(AxisRef::Face),
        Pick::Edge(..) => match entity_of(cache, pick)? {
            cadrs_core::applied::EdgeOrFace::Edge(e) => Some(AxisRef::Edge(e)),
            _ => None,
        },
        Pick::SketchCurve(sketch, curve) => Some(AxisRef::SketchCurve { sketch, curve }),
        _ => None,
    }
}

/// A mirror plane for a pick (PS26.1).
pub fn mirror_plane_of(features: &[Feature], cache: &PartCache, pick: Pick) -> Option<MirrorPlane> {
    match pick {
        Pick::Plane(_) => plane_of(features, pick).map(MirrorPlane::Plane),
        Pick::Feature(f) if is_connector(features, f) => Some(MirrorPlane::Connector(ConnectorRef::Feature(f))),
        Pick::Feature(_) => plane_of(features, pick).map(MirrorPlane::Plane),
        Pick::Face(..) => face_ref(cache, pick).map(MirrorPlane::Face),
        _ => None,
    }
}

/// A new feature's name, parameters and first field, from what was selected when its button
/// was clicked.
pub fn initial(world: &World, kind: AppliedKind, picked: &[Pick]) -> Option<(&'static str, FeatureKind, AppliedField)> {
    let doc = world.get_resource::<ActiveDocument>()?;
    let features = doc.active_element()?.features();
    let cache = world.resource::<PartCache>();
    // What the selection holds: parts, features or faces.
    let seeds = |parts: &mut Vec<cadrs_core::PartId>, feats: &mut Vec<FeatureId>, faces: &mut Vec<FaceRef>| -> PatternType {
        let non_sketch: Vec<FeatureId> = picked
            .iter()
            .filter_map(|p| match p {
                Pick::Feature(f) if features.iter().any(|x| x.id == *f && x.is_part_feature()) => Some(*f),
                _ => None,
            })
            .collect();
        if !non_sketch.is_empty() {
            *feats = non_sketch;
            return PatternType::Feature;
        }
        let f: Vec<FaceRef> = picked.iter().filter_map(|p| face_ref(cache, *p)).collect();
        if !f.is_empty() {
            *faces = f;
            return PatternType::Face;
        }
        for p in picked {
            if let Pick::Part(id) = p
                && !parts.contains(id)
            {
                parts.push(*id);
            }
        }
        PatternType::Part
    };
    Some(match kind {
        AppliedKind::Pattern(k) => {
            let mut x = PatternFeature::new(k);
            x.pattern_type = seeds(&mut x.parts, &mut x.features, &mut x.faces);
            let base = match k {
                PatternKind::Linear => "Linear pattern",
                PatternKind::Circular => "Circular pattern",
                PatternKind::Curve => "Curve pattern",
            };
            (base, FeatureKind::Pattern(x), AppliedField::PatternEntities)
        }
        AppliedKind::Mirror => {
            let mut x = MirrorFeature::default();
            x.mirror_type = seeds(&mut x.parts, &mut x.features, &mut x.faces);
            ("Mirror", FeatureKind::Mirror(x), AppliedField::PatternEntities)
        }
        AppliedKind::MateConnector => {
            let origin = picked.iter().find_map(|p| implicit_origin(cache, *p));
            let owner = picked.iter().find(|p| implicit_origin(cache, **p).is_some()).and_then(|p| p.part());
            let field = AppliedField::ConnectorOrigin;
            // A new connector: Move off, Owner entity on (filled from the origin's part, A22.5,
            // P3.11).
            let owner = owner.or_else(|| origin.and_then(|o| o.part()));
            let x = MateConnectorFeature { origin, move_on: false, owner_on: true, owner, ..MateConnectorFeature::default() };
            ("Mate connector", FeatureKind::MateConnector(x), field)
        }
        _ => return None,
    })
}

/// A pick into one of these features' fields (see the module docs). Returns false if the pick
/// doesn't fit the field.
pub fn pick(world: &mut World, kind: &mut FeatureKind, field: AppliedField, pick: Pick) -> bool {
    let Some(features) = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.active_element())
        .map(|e| e.features().to_vec())
    else {
        return false;
    };
    let cache = world.resource::<PartCache>();
    // A skip dot, whatever the field (PS22.5).
    if let (FeatureKind::Pattern(x), Pick::Instance(_, i, j)) = (&mut *kind, pick) {
        x.skip_on = true;
        toggle(&mut x.skipped, [i, j]);
        x.skipped.sort();
        return true;
    }
    match (kind, field) {
        (FeatureKind::Pattern(x), AppliedField::PatternEntities) => {
            toggle_seed(&features, cache, x.pattern_type, &mut x.parts, &mut x.features, &mut x.faces, pick)
        }
        (FeatureKind::Mirror(x), AppliedField::PatternEntities) => {
            toggle_seed(&features, cache, x.mirror_type, &mut x.parts, &mut x.features, &mut x.faces, pick)
        }
        (FeatureKind::Pattern(x), AppliedField::PatternDirection | AppliedField::PatternDirection2) => {
            let Some(d) = direction_of(&features, cache, pick) else { return false };
            let slot = if field == AppliedField::PatternDirection { &mut x.first.direction } else { &mut x.second.direction };
            *slot = if *slot == Some(d) { None } else { Some(d) };
            true
        }
        (FeatureKind::Pattern(x), AppliedField::PatternAxis) => {
            let Some(a) = axis_of(&features, cache, pick) else { return false };
            x.axis = if x.axis == Some(a) { None } else { Some(a) };
            true
        }
        (FeatureKind::Pattern(x), AppliedField::PatternPath) => {
            let p = match pick {
                Pick::Edge(..) => match entity_of(cache, pick) {
                    Some(cadrs_core::applied::EdgeOrFace::Edge(e)) => PathRef::Edge(e),
                    _ => return false,
                },
                Pick::SketchCurve(sketch, curve) => PathRef::SketchCurve { sketch, curve },
                Pick::Feature(f) if is_sketch(&features, f) => PathRef::Sketch(f),
                Pick::Feature(f) if features.iter().any(|y| y.id == f && matches!(y.kind, FeatureKind::Helix(_))) => PathRef::Curve(f),
                _ => return false,
            };
            match x.path.iter().position(|q| match (q, &p) {
                (PathRef::Edge(a), PathRef::Edge(b)) => a.edge == b.edge,
                (a, b) => a == b,
            }) {
                Some(i) => {
                    x.path.remove(i);
                }
                None => x.path.push(p),
            }
            true
        }
        (FeatureKind::Pattern(x), AppliedField::MergeScope) => match pick.part() {
            Some(p) => {
                toggle(&mut x.merge_scope, p);
                true
            }
            None => false,
        },
        (FeatureKind::Mirror(x), AppliedField::MirrorPlane) => {
            let Some(p) = mirror_plane_of(&features, cache, pick) else { return false };
            x.plane = if x.plane == Some(p) { None } else { Some(p) };
            true
        }
        (FeatureKind::Mirror(x), AppliedField::MergeScope) => match pick.part() {
            Some(p) => {
                toggle(&mut x.merge_scope, p);
                true
            }
            None => false,
        },
        (FeatureKind::MateConnector(x), AppliedField::ConnectorOrigin) => {
            let Some(o) = implicit_origin(cache, pick) else { return false };
            x.origin = if x.origin == Some(o) { None } else { Some(o) };
            // The owner follows the first pick (A22.5, P3.11: check it, and change it if needed).
            if x.owner_on && x.owner.is_none() {
                x.owner = pick.part().or_else(|| x.origin.and_then(|o| o.part()));
            }
            true
        }
        // P3.11 (P3.8 judge): the Alignment's direction.
        (FeatureKind::MateConnector(x), AppliedField::ConnectorAlignment) => {
            let Some(d) = direction_of(&features, cache, pick) else { return false };
            x.alignment = if x.alignment == Some(d) { None } else { Some(d) };
            true
        }
        // P3B.7 (A22.4, A22.6, A22.5): the Between entity, the Realign axes, the Owner part.
        (FeatureKind::MateConnector(x), AppliedField::ConnectorBetween) => {
            let Some(o) = implicit_origin(cache, pick) else { return false };
            x.between = if x.between == Some(o) { None } else { Some(o) };
            true
        }
        (FeatureKind::MateConnector(x), f @ (AppliedField::ConnectorPrimary | AppliedField::ConnectorSecondary)) => {
            let Some(o) = implicit_origin(cache, pick).filter(|o| !matches!(o, ConnectorOrigin::Origin | ConnectorOrigin::Vertex(_) | ConnectorOrigin::SketchPoint { .. })) else {
                return false;
            };
            let slot = if f == AppliedField::ConnectorPrimary { &mut x.primary_axis } else { &mut x.secondary_axis };
            *slot = if *slot == Some(o) { None } else { Some(o) };
            true
        }
        (FeatureKind::MateConnector(x), AppliedField::ConnectorOwner) => match pick.part() {
            Some(p) => {
                x.owner = if x.owner == Some(p) { None } else { Some(p) };
                true
            }
            None => false,
        },
        (FeatureKind::Hole(x), AppliedField::HoleConnectors) => {
            let Some(c) = connector_of(&features, cache, pick) else { return false };
            let same = |a: &ConnectorRef| match (a, &c) {
                (ConnectorRef::Implicit(ConnectorOrigin::Face(p)), ConnectorRef::Implicit(ConnectorOrigin::Face(q))) => p.face == q.face,
                (ConnectorRef::Implicit(ConnectorOrigin::Edge(p)), ConnectorRef::Implicit(ConnectorOrigin::Edge(q))) => p.edge == q.edge,
                (a, b) => a == b,
            };
            match x.connectors.iter().position(same) {
                Some(i) => {
                    x.connectors.remove(i);
                }
                None => x.connectors.push(c),
            }
            true
        }
        _ => false,
    }
}

/// What these features' fields refer to, shown selected in the view while the dialog is open.
pub fn references(kind: &FeatureKind, cache: &PartCache) -> Vec<Pick> {
    let mut out = Vec::new();
    let face = |f: &FaceRef| cache.parts.iter().find(|p| p.solid.face(&f.face).is_some()).map(|p| Pick::Face(p.id, f.face));
    let edge = |e: &cadrs_core::EdgeRef| cache.parts.iter().find(|p| p.solid.edge(&e.edge).is_some()).map(|p| Pick::Edge(p.id, e.edge));
    let direction = |d: &DirectionRef| match d {
        DirectionRef::Edge(e) => edge(e),
        DirectionRef::FaceNormal(f) => face(f),
        DirectionRef::SketchLine { sketch, curve } => Some(Pick::SketchCurve(*sketch, *curve)),
        DirectionRef::PlaneNormal(p) => crate::viewport::plane_pick(*p),
        DirectionRef::Connector(c) => connector_pick(c),
    };
    match kind {
        FeatureKind::Pattern(x) => {
            match x.pattern_type {
                PatternType::Part => out.extend(x.parts.iter().map(|p| Pick::Part(*p))),
                PatternType::Feature => out.extend(x.features.iter().map(|f| Pick::Feature(*f))),
                PatternType::Face => out.extend(x.faces.iter().filter_map(face)),
            }
            match x.kind {
                PatternKind::Linear => {
                    out.extend(x.first.direction.iter().filter_map(direction));
                    if x.second_on {
                        out.extend(x.second.direction.iter().filter_map(direction));
                    }
                }
                PatternKind::Circular => out.extend(x.axis.iter().filter_map(|a| match a {
                    AxisRef::SketchCurve { sketch, curve } => Some(Pick::SketchCurve(*sketch, *curve)),
                    AxisRef::Edge(e) => edge(e),
                    AxisRef::Face(f) => face(f),
                    AxisRef::Connector(c) => connector_pick(c),
                })),
                PatternKind::Curve => out.extend(x.path.iter().filter_map(|p| match p {
                    PathRef::Edge(e) => edge(e),
                    PathRef::SketchCurve { sketch, curve } => Some(Pick::SketchCurve(*sketch, *curve)),
                    PathRef::Sketch(s) | PathRef::Curve(s) => Some(Pick::Feature(*s)),
                })),
            }
        }
        FeatureKind::Mirror(x) => {
            match x.mirror_type {
                PatternType::Part => out.extend(x.parts.iter().map(|p| Pick::Part(*p))),
                PatternType::Feature => out.extend(x.features.iter().map(|f| Pick::Feature(*f))),
                PatternType::Face => out.extend(x.faces.iter().filter_map(face)),
            }
            out.extend(x.plane.iter().filter_map(|p| match p {
                MirrorPlane::Plane(p) => crate::viewport::plane_pick(*p),
                MirrorPlane::Face(f) => face(f),
                MirrorPlane::Connector(c) => connector_pick(c),
            }));
        }
        FeatureKind::MateConnector(x) => {
            let between = if x.origin_type == cadrs_core::mate::OriginType::BetweenEntities { x.between } else { None };
            let axes = if x.realign { [x.primary_axis, x.secondary_axis] } else { [None, None] };
            for o in [x.origin, between].into_iter().chain(axes).flatten() {
                out.extend(connector_pick(&ConnectorRef::Implicit(o)));
            }
            out.extend(x.alignment.iter().filter_map(direction));
        }
        _ => {}
    }
    out
}

/// True when the active field takes mate connectors (their hover preview shows).
pub fn takes_connectors(kind: &FeatureKind, field: AppliedField) -> bool {
    match (kind, field) {
        (FeatureKind::Pattern(x), AppliedField::PatternAxis) => x.kind == PatternKind::Circular,
        (FeatureKind::Pattern(_), AppliedField::PatternDirection | AppliedField::PatternDirection2) => true,
        (FeatureKind::Mirror(_), AppliedField::MirrorPlane) => true,
        (FeatureKind::MateConnector(_), AppliedField::ConnectorOrigin | AppliedField::ConnectorBetween) => true,
        (FeatureKind::Hole(_), AppliedField::HoleConnectors) => true,
        (k, f) => crate::transform_ui::takes_connectors(k, f),
    }
}

// ---------------------------------------------------------------------------------------------
// Drawing

const CONNECTOR_PX: f32 = 13.0;

/// K: show or hide the mate connectors (in Part Studios and assemblies, X10). Ctrl+M: the Mate
/// connector tool (P3B.7, A22.3: a Mate connector feature in a Part Studio, an assembly's own
/// connector in an assembly).
#[allow(clippy::too_many_arguments)]
fn connector_key(
    mut keys: MessageReader<KeyboardInput>,
    focus: Res<bevy::input_focus::InputFocus>,
    q_dialogs: Query<(), With<cadrs_ui::DialogRoot>>,
    kind: Res<crate::viewport::ActiveKind>,
    sketch: Option<Res<crate::sketch::SketchSession>>,
    buttons: Res<ButtonInput<KeyCode>>,
    mut shown: ResMut<MateConnectorsShown>,
    mut commands: Commands,
) {
    for k in keys.read() {
        if k.state != ButtonState::Pressed || !matches!(k.key_code, KeyCode::KeyK | KeyCode::KeyM) {
            continue;
        }
        let shift_alt = buttons.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight, KeyCode::AltLeft, KeyCode::AltRight]);
        let ctrl = buttons.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
        if shift_alt || focus.get().is_some() || !q_dialogs.is_empty() || sketch.is_some() {
            continue;
        }
        let studio = *kind == crate::viewport::ActiveKind::PartStudio;
        let assembly = *kind == crate::viewport::ActiveKind::Assembly;
        match (k.key_code, ctrl) {
            (KeyCode::KeyK, false) if studio || assembly => shown.0 = !shown.0,
            (KeyCode::KeyM, true) if studio => {
                commands.queue(|world: &mut World| crate::applied::begin(world, AppliedKind::MateConnector));
            }
            (KeyCode::KeyM, true) if assembly => commands.queue(crate::assembly::connector_tool::open_new),
            _ => {}
        }
    }
}

/// The part cache knows whether connectors are shown (only shown ones are picked).
fn sync_connectors_shown(shown: Res<MateConnectorsShown>, mut cache: ResMut<PartCache>) {
    if cache.connectors_shown != shown.0 {
        cache.connectors_shown = shown.0;
    }
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
}

/// A connector glyph: the disc (rings, a light quarter) in its XY plane and its three axes.
pub(crate) fn connector_glyph(g: &mut Gizmos<crate::parts::VertexGizmos>, f: &PlaneFrame, scale: f32, highlight: Option<Color>) {
    let (o, x, y, z) = (v3(f.origin), v3(f.u).normalize_or_zero(), v3(f.v).normalize_or_zero(), v3(f.normal()).normalize_or_zero());
    let r = CONNECTOR_PX * scale;
    let rot = Quat::from_mat3(&Mat3::from_cols(x, y, z));
    let iso = Isometry3d::new(o, rot);
    let dark = Color::srgb_u8(0x3a, 0x3f, 0x45);
    let ring = highlight.unwrap_or(Color::WHITE);
    for k in 0..6 {
        g.circle(iso, r * (0.62 + 0.06 * k as f32), ring).resolution(32);
    }
    g.circle(iso, r, dark).resolution(32);
    g.circle(iso, r * 0.58, dark).resolution(32);
    // A light quarter between X and Y.
    for k in 0..=8 {
        let a = std::f32::consts::FRAC_PI_2 * k as f32 / 8.0;
        let d = x * a.cos() + y * a.sin();
        g.line(o, o + d * r * 0.58, Color::srgb_u8(0xee, 0xee, 0xee));
    }
    g.line(o, o + x * r * 1.9, Color::srgb_u8(0xe0, 0x30, 0x30));
    g.line(o, o + y * r * 1.4, Color::srgb_u8(0x3c, 0xb0, 0x3c));
    g.line(o, o + z * r * 1.9, Color::srgb_u8(0x30, 0x50, 0xe0));
}

/// The explicit mate connectors (unless K hid them); hovered or selected ones in orange.
fn draw_connectors(
    cache: Res<PartCache>,
    shown: Res<MateConnectorsShown>,
    view: Res<ViewportView>,
    highlight: Res<PlaneHighlight>,
    selection: Res<Selection>,
    sketch: Option<Res<crate::sketch::SketchSession>>,
    mut g: Gizmos<crate::parts::VertexGizmos>,
) {
    if sketch.is_some() {
        return;
    }
    let scale = view.view.scale;
    for (id, f) in &cache.connectors {
        let p = Pick::Feature(*id);
        // Hidden (K), a connector still shows while its feature-list row is hovered.
        if !shown.0 && highlight.list != Some(p) {
            continue;
        }
        let hl = if highlight.is_hovered(p) {
            Some(crate::parts::HOVER)
        } else if selection.contains(p) {
            Some(crate::parts::SELECTED)
        } else {
            None
        };
        connector_glyph(&mut g, f, scale, hl);
    }
}

/// While a field takes connectors: the implicit connector of the face or edge under the pointer.
fn draw_connector_hover(
    session: Option<Res<AppliedSession>>,
    doc: Option<Res<ActiveDocument>>,
    cache: Res<PartCache>,
    highlight: Res<PlaneHighlight>,
    view: Res<ViewportView>,
    mut g: Gizmos<crate::parts::VertexGizmos>,
) {
    let (Some(s), Some(doc)) = (session, doc) else { return };
    let Some(el) = doc.doc.element(s.element) else { return };
    let Some(f) = el.feature(s.feature) else { return };
    if !takes_connectors(&f.kind, s.field) {
        return;
    }
    let Some(pick) = highlight.viewport else { return };
    if matches!(pick, Pick::Feature(_)) {
        return;
    }
    // P3.11 (P3.8 judge): the hovered face shows a dot at every implicit connector on it (its
    // centre, its vertices, its edges' midpoints and circle centres), as Onshape's; hovering one
    // of those edges or vertices picks its connector.
    if let Pick::Face(part, face) = pick
        && let Some(p) = cache.part(part)
    {
        let s = &p.solid;
        let mut dots: Vec<[f64; 3]> = Vec::new();
        if let Some(f) = s.face(&face)
            && let Some(c) = f.center
        {
            dots.push(c);
        }
        for e in s.edges.iter().filter(|e| e.name.touches(&face) && e.name.faces[0] != e.name.faces[1]) {
            dots.push(e.circle.map_or_else(|| e.midpoint(), |c| c.center));
        }
        dots.extend(s.vertices.iter().filter(|v| v.name.faces.contains(&face)).map(|v| v.point));
        let v = view.view;
        let rot = Quat::from_rotation_arc(Vec3::Z, v.back());
        for d in dots {
            let at = v3(d);
            for r in [0.8, 1.6, 2.4] {
                g.circle(Isometry3d::new(at, rot), r * v.scale, Color::srgb_u8(0xf4, 0xf4, 0xf4)).resolution(16);
            }
            g.circle(Isometry3d::new(at, rot), 3.2 * v.scale, Color::srgb_u8(0x5a, 0x60, 0x68)).resolution(16);
        }
    }
    let Some(o) = implicit_origin(&cache, pick) else { return };
    let Ok(frame) = cadrs_core::mate::origin_frame(&o, el.features(), &cache.parts) else { return };
    connector_glyph(&mut g, &frame, view.view.scale, Some(crate::parts::HOVER));
}

/// A connector field outside the applied dialogs (P3.10 judge): the Revolve axis's mate
/// connector button and Mass properties' reference frame. While the field takes a pick, the
/// connector under the pointer shows its whole glyph (disc and triad) in the hover orange; once
/// picked, its glyph stays in the selection orange, and a revolve axis is drawn along its Z.
#[allow(clippy::too_many_arguments)]
fn draw_connector_fields(
    extrude: Option<Res<crate::extrude::ExtrudeSession>>,
    mass: Option<Res<crate::mass_props::MassPanel>>,
    doc: Option<Res<ActiveDocument>>,
    cache: Res<PartCache>,
    highlight: Res<PlaneHighlight>,
    view: Res<ViewportView>,
    mut g: Gizmos<crate::parts::VertexGizmos>,
) {
    let Some(el) = doc.as_ref().and_then(|d| d.active_element()) else { return };
    let features = el.features();
    // Larger than the connectors on the parts: the field's one connector, its triad readable.
    let scale = view.view.scale * 2.2;
    let frame_of = |c: &ConnectorRef| cadrs_core::mate::frame(c, features, &cache.parts, &cache.connectors).ok();
    let mut hover = false;
    let mut picked: Vec<(PlaneFrame, bool)> = Vec::new();
    if let Some(s) = extrude.as_ref()
        && let Some(r) = el.feature(s.feature).and_then(|f| f.revolve())
    {
        hover |= s.field == crate::extrude::ExtrudeField::AxisConnector;
        if let Some(AxisRef::Connector(c)) = &r.axis {
            picked.extend(frame_of(c).map(|f| (f, true)));
        }
    }
    if let Some(m) = mass.as_ref().filter(|m| !m.face_tab) {
        hover |= m.reference_active;
        if let Some(c) = &m.reference {
            picked.extend(frame_of(c).map(|f| (f, false)));
        }
    }
    for (f, axis) in &picked {
        connector_glyph(&mut g, f, scale, Some(crate::parts::SELECTED));
        if *axis {
            // The revolve axis: the connector's Z, well past the part either way.
            let (o, z) = (v3(f.origin), v3(f.normal()).normalize_or_zero());
            let reach = 120.0 * scale;
            g.line(o - z * reach, o + z * reach, crate::parts::SELECTED);
        }
    }
    if !hover {
        return;
    }
    let Some(pick) = highlight.viewport else { return };
    let Some(c) = connector_of(features, &cache, pick) else { return };
    if let Some(f) = frame_of(&c) {
        connector_glyph(&mut g, &f, scale, Some(crate::parts::HOVER));
    }
}

/// P3.11 (PS20.4): while a loft with a Normal direction or Tangent direction condition is
/// edited, its picked direction as an arrow from the middle of the first or last profile, along
/// the loft (as the kernel uses it).
fn draw_direction_arrows(
    session: Option<Res<AppliedSession>>,
    cache: Res<PartCache>,
    view: Res<ViewportView>,
    mut g: Gizmos<crate::parts::PickedEdgeGizmos>,
) {
    let Some(s) = session else { return };
    let Some(arrows) = cache.arrows.get(&s.feature) else { return };
    let len = 90.0 * view.view.scale;
    for (o, d) in arrows {
        let (o, d) = (v3(*o), v3(*d).normalize_or_zero());
        g.arrow(o, o + d * len, crate::parts::SELECTED).with_tip_length(14.0 * view.view.scale);
    }
}

/// Skip instances (PS22.5): a dot per instance of the pattern being edited, grey, light blue
/// when skipped, orange when hovered.
fn draw_instance_dots(
    session: Option<Res<AppliedSession>>,
    doc: Option<Res<ActiveDocument>>,
    cache: Res<PartCache>,
    highlight: Res<PlaneHighlight>,
    view: Res<ViewportView>,
    mut g: Gizmos<crate::parts::VertexGizmos>,
) {
    let (Some(s), Some(doc)) = (session, doc) else { return };
    let Some(FeatureKind::Pattern(x)) = doc.doc.element(s.element).and_then(|e| e.feature(s.feature)).map(|f| f.kind.clone()) else {
        return;
    };
    if !x.skip_on {
        return;
    }
    let Some(dots) = cache.dots.get(&s.feature) else { return };
    let v = view.view;
    let rot = Quat::from_rotation_arc(Vec3::Z, v.back());
    for d in dots {
        let at = v3(d.at);
        let skipped = x.is_skipped(d.index[0], d.index[1]);
        let hovered = highlight.is_hovered(Pick::Instance(s.feature, d.index[0], d.index[1]));
        let fill = if hovered {
            crate::parts::HOVER
        } else if skipped {
            Color::srgb_u8(0x9c, 0xd2, 0xf4)
        } else {
            Color::srgb_u8(0xd8, 0xdc, 0xe0)
        };
        for r in [0.8, 1.6, 2.4, 3.2, 4.0] {
            g.circle(Isometry3d::new(at, rot), r * v.scale, fill).resolution(20);
        }
        g.circle(Isometry3d::new(at, rot), 4.8 * v.scale, Color::srgb_u8(0x5a, 0x60, 0x68)).resolution(20);
    }
}

/// P3.11 (P3.8 judge): the pattern being edited has Skip instances on: every instance dot inside
/// the box from `a` to `b` (viewport offsets) is toggled, as clicking each would (one undo step).
pub fn skip_dots_in_box(world: &mut World, a: Vec2, b: Vec2) {
    let Some(s) = world.get_resource::<AppliedSession>() else { return };
    let pattern = s.feature;
    let view = world.resource::<ViewportView>().view;
    let (lo, hi) = (a.min(b), a.max(b));
    let inside: Vec<[u32; 2]> = world
        .resource::<PartCache>()
        .dots
        .get(&pattern)
        .map(|dots| {
            dots.iter()
                .filter(|d| {
                    let p = view.project(v3(d.at));
                    p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y
                })
                .map(|d| d.index)
                .collect()
        })
        .unwrap_or_default();
    if inside.is_empty() {
        return;
    }
    crate::applied::change_kind(world, "Skip instances", |k| {
        if let FeatureKind::Pattern(x) = k
            && x.skip_on
        {
            for i in &inside {
                toggle(&mut x.skipped, *i);
            }
            x.skipped.sort();
        }
    });
}

/// The box being dragged over a pattern's Skip dots.
#[derive(Component)]
struct SkipBox;

/// Draws the box while it is dragged (only while a pattern with Skip instances is edited).
fn draw_skip_box(
    session: Option<Res<AppliedSession>>,
    doc: Option<Res<ActiveDocument>>,
    drag: Res<crate::viewport::ViewportDrag>,
    mut q: Query<(Entity, &mut Node), With<SkipBox>>,
    mut commands: Commands,
) {
    let skipping = session.as_ref().is_some_and(|s| {
        doc.as_ref()
            .and_then(|d| d.doc.element(s.element))
            .and_then(|e| e.feature(s.feature))
            .is_some_and(|f| matches!(&f.kind, FeatureKind::Pattern(x) if x.skip_on))
    });
    let rect = drag.primary_down().filter(|d| skipping && d.distance(drag.pointer()) >= 4.0).map(|d| (d.min(drag.pointer()), d.max(drag.pointer())));
    let Some((lo, hi)) = rect else {
        for (e, _) in &q {
            commands.entity(e).try_despawn();
        }
        return;
    };
    let (left, top, width, height) = (Val::Px(lo.x), Val::Px(lo.y), Val::Px(hi.x - lo.x), Val::Px(hi.y - lo.y));
    match q.iter_mut().next() {
        Some((_, mut n)) => {
            if n.left != left || n.top != top || n.width != width || n.height != height {
                n.left = left;
                n.top = top;
                n.width = width;
                n.height = height;
            }
        }
        None => {
            commands.spawn((
                Name::new("skip-box"),
                SkipBox,
                Node { position_type: PositionType::Absolute, left, top, width, height, border: UiRect::all(Val::Px(1.0)), ..default() },
                BackgroundColor(Color::srgba(0.17, 0.39, 0.75, 0.08)),
                BorderColor::all(Color::srgb_u8(0x2b, 0x64, 0xc0)),
                GlobalZIndex(cadrs_ui::z::DIALOG - 30),
                Pickable::IGNORE,
                DespawnOnExit(crate::AppState::Document),
            ));
        }
    }
}

/// The instance dot under a screen offset (within 8 px), for picking.
pub fn pick_dot(cache: &PartCache, view: &crate::camera::ViewState, offset: Vec2, pattern: FeatureId) -> Option<Pick> {
    let dots = cache.dots.get(&pattern)?;
    dots.iter()
        .map(|d| (view.project(v3(d.at)).distance(offset), d))
        .filter(|(dist, _)| *dist <= crate::parts::VERTEX_PICK_PX)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, d)| Pick::Instance(pattern, d.index[0], d.index[1]))
}

/// The explicit connector under a screen offset (within 8 px of its origin), for picking.
pub fn pick_connector(cache: &PartCache, view: &crate::camera::ViewState, offset: Vec2) -> Option<Pick> {
    cache
        .connectors
        .iter()
        .map(|(id, f)| (view.project(v3(f.origin)).distance(offset), *id))
        .filter(|(d, _)| *d <= crate::parts::VERTEX_PICK_PX + 2.0)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, id)| Pick::Feature(id))
}
