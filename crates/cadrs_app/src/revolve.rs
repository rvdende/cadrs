//! The Revolve feature (P3.4, PS7), like Onshape's (`training/intro-to-part-studios/
//! ex2-step5.png`; the dialog is [`crate::revolve_dialog`]):
//!
//! 1. **Revolve** (the toolbar button after Extrude, or Shift+W) inserts "Revolve N" and opens
//!    its dialog, the "Faces and sketch regions to revolve" field waiting for picks: sketch
//!    regions, or a whole sketch from the feature list (PS1.6).
//! 2. The **Revolve axis** field takes a sketch line (a construction centreline) or circle, a
//!    straight or circular part edge, or a cylindrical or conical face (PS7.2).
//! 3. The revolve type: **Full** (the default), **Blind** (an angle, with a flip arrow and a
//!    manipulator arrow at the end that drags the angle), **Symmetric**, **Up to next / face /
//!    part / vertex** with an offset angle, and a **Second end position** (PS7.3); Solid,
//!    Surface and Thin (PS7.4); New, Add, Remove, Intersect with Merge with all (PS7.5, picked
//!    by itself as for an extrude).
//! 4. ✓ or Enter accepts ("Insert Revolve 1", one undo step), ✕ or Esc cancels.
//!
//! The dialog shares the Extrude dialog's session ([`ExtrudeSession`]): the same undo floor,
//! pick filter, feature-list picks and accept/cancel; its own field is
//! [`ExtrudeField::Axis`].

use bevy::picking::pointer::{PointerAction, PointerButton, PointerId, PointerInput};
use bevy::prelude::*;
use bevy::ui::UiTransform;
use cadrs_core::commands::{AddRevolve, SetRevolve};
use cadrs_core::document::{AxisRef, BooleanOp, EdgeRef, EndType, FaceRef, RevolveType, UpTo, VertexRef};
use cadrs_core::{ElementKind, FeatureId, RegionRef, RevolveFeature};
use cadrs_sketch::units::Quantity;

use crate::extrude::{ExtrudeField, ExtrudeSession};
use crate::parts::{PartCache, PartOverride};
use crate::viewport::{Pick, PickRequest, Selection, ViewportArea, ViewportRect, ViewportView};
use crate::{ActiveDocument, AppState};

pub struct RevolvePlugin;

impl Plugin for RevolvePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AngleArrow>()
            .add_systems(
                Update,
                (revolve_picks, angle_arrow_pointer, crate::revolve_dialog::sync_revolve_dialog)
                    .chain()
                    .after(crate::viewport::apply_view_to_camera)
                    .before(crate::parts::PartsSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(
                PostUpdate,
                place_angle_arrow
                    .before(bevy::ui::UiSystems::Layout)
                    .run_if(in_state(AppState::Document)),
            )
            .add_plugins(crate::revolve_dialog::RevolveDialogPlugin)
            .add_systems(
                Update,
                (automatic_operation, show_references, draw_sketch_curve_picks)
                    .after(crate::parts::PartsSet)
                    .run_if(in_state(AppState::Document)),
            );
    }
}

// ---------------------------------------------------------------------------------------------
// Lifecycle

/// Starts a new revolve in the active Part Studio (the Revolve button and Shift+W).
pub fn begin_revolve(world: &mut World) {
    if world.contains_resource::<ExtrudeSession>() || world.contains_resource::<crate::sketch::SketchSession>() {
        return;
    }
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let Some(element) = doc
        .active_element()
        .filter(|e| matches!(e.kind, ElementKind::PartStudio { .. }))
        .map(|e| e.id)
    else {
        return;
    };
    let feature = FeatureId::new();
    let mark = doc.history.undo_len();
    if let Err(e) = doc.execute(&AddRevolve {
        element,
        feature,
        revolve: RevolveFeature::default(),
    }) {
        warn!("cannot insert a revolve: {e}");
        return;
    }
    crate::extrude::start_session(world, element, feature, true, mark, None);
}

/// Opens an existing revolve for editing (double-click in the feature list, or Edit).
pub fn edit_revolve(world: &mut World, feature: FeatureId) {
    if let Some(s) = world.get_resource::<ExtrudeSession>() {
        if s.feature == feature {
            return;
        }
        crate::extrude::finish_session(world);
    }
    if world.contains_resource::<crate::sketch::SketchSession>() {
        crate::sketch::finish_session(world);
    }
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let Some(el) = doc.active_element() else {
        return;
    };
    let element = el.id;
    let Some(before) = el.feature(feature).filter(|f| f.revolve().is_some()).cloned() else {
        return;
    };
    let mark = doc.history.undo_len();
    doc.discard_since(mark);
    crate::extrude::start_session(world, element, feature, false, mark, Some(before));
}

/// The open revolve's parameters.
pub(crate) fn rparams(world: &World) -> Option<RevolveFeature> {
    let s = world.get_resource::<ExtrudeSession>()?;
    crate::extrude::current(world, s)?.revolve().cloned()
}

/// Records a parameter change as an undo step.
pub(crate) fn set_rparams(world: &mut World, revolve: RevolveFeature, label: impl Into<String>) -> bool {
    let Some(s) = world.get_resource::<ExtrudeSession>().cloned() else {
        return false;
    };
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return false;
    };
    match doc.execute(&SetRevolve {
        element: s.element,
        feature: s.feature,
        revolve,
        label: label.into(),
    }) {
        Ok(()) => true,
        Err(e) => {
            warn!("cannot change the revolve: {e}");
            false
        }
    }
}

fn sketch_name(world: &World, sketch: FeatureId) -> Option<String> {
    let d = world.get_resource::<ActiveDocument>()?;
    let f = d.active_element()?.feature(sketch)?;
    f.sketch()?;
    Some(f.name.clone())
}

/// A picked region toggles in the "Faces and sketch regions to revolve" field.
fn toggle_region(world: &mut World, sketch: FeatureId, index: u32) {
    let Some(mut r) = rparams(world) else { return };
    let cache = world.resource::<PartCache>();
    let Some(region) = cache
        .sketch_regions(sketch)
        .and_then(|r| r.regions.get(index as usize))
        .cloned()
    else {
        return;
    };
    let name = sketch_name(world, sketch).unwrap_or_default();
    let rr = RegionRef::new(sketch, &region);
    let same = |x: &RegionRef| x.sketch == sketch && x.curves == rr.curves && region.contains(x.seed);
    let label = if let Some(i) = r.regions.iter().position(same) {
        r.regions.remove(i);
        format!("Deselect Face of {name}")
    } else {
        r.regions.push(rr);
        format!("Select Face of {name}")
    };
    set_rparams(world, r, label);
}

/// A sketch picked in the feature list toggles as a whole (PS1.6).
fn toggle_sketch(world: &mut World, sketch: FeatureId) {
    let Some(mut r) = rparams(world) else { return };
    let Some(name) = sketch_name(world, sketch) else { return };
    let label = if let Some(i) = r.sketches.iter().position(|s| *s == sketch) {
        r.sketches.remove(i);
        format!("Deselect {name}")
    } else {
        r.regions.retain(|x| x.sketch != sketch);
        r.sketches.push(sketch);
        format!("Select {name}")
    };
    set_rparams(world, r, label);
}

// ---------------------------------------------------------------------------------------------
// Picks

fn revolve_picks(
    mut picks: MessageReader<PickRequest>,
    session: Option<Res<ExtrudeSession>>,
    doc: Option<Res<ActiveDocument>>,
    mut arrow: ResMut<AngleArrow>,
    replace: Option<Res<crate::replace_reference::ReplaceSession>>,
    mut commands: Commands,
) {
    let (Some(s), None) = (session, replace) else {
        picks.clear();
        return;
    };
    let is_revolve = doc
        .as_ref()
        .and_then(|d| d.doc.element(s.element)?.feature(s.feature))
        .is_some_and(|f| f.revolve().is_some());
    for p in picks.read() {
        if std::mem::take(&mut arrow.swallow_click) || !is_revolve {
            continue;
        }
        let Some(pick) = p.0 else { continue };
        let field = s.field;
        commands.queue(move |world: &mut World| {
            let _ = take_pick(world, field, pick);
        });
    }
}

/// A pick goes to the active selection field.
fn take_pick(world: &mut World, field: ExtrudeField, pick: Pick) -> Option<()> {
    let s = world.get_resource::<ExtrudeSession>().cloned()?;
    let op = rparams(world).map(|r| r.op);
    let (face, vertex, edge, auto_scope) = {
        let cache = world.resource::<PartCache>();
        let face = match pick {
            Pick::Face(part, face) => (|| {
                let p = cache.part(part)?;
                let i = p.solid.faces.iter().position(|f| f.name == face)?;
                let seed = p.solid.face_point(i)?;
                Some((FaceRef { part, face, seed }, p.solid.faces[i].plane.is_some()))
            })(),
            _ => None,
        };
        let vertex = match pick {
            Pick::Vertex(part, vertex) => cache
                .part(part)
                .and_then(|p| p.solid.vertex(&vertex))
                .map(|v| VertexRef { part, vertex, point: v.point }),
            _ => None,
        };
        let edge = match pick {
            Pick::Edge(part, edge) => cache
                .part(part)
                .and_then(|p| p.solid.edge(&edge))
                .map(|e| EdgeRef { part, edge, seed: e.midpoint() }),
            _ => None,
        };
        let auto_scope = cache
            .contacts
            .get(&s.feature)
            .map(|c| match op {
                Some(BooleanOp::Add) => c.touches.clone(),
                _ => c.overlaps.clone(),
            })
            .unwrap_or_default();
        (face, vertex, edge, auto_scope)
    };
    let own = |op: cadrs_sketch::OpId| op == s.feature.0;
    match (field, pick) {
        (ExtrudeField::Input, Pick::Region(sketch, index)) => toggle_region(world, sketch, index),
        (ExtrudeField::Input, Pick::Feature(sketch)) => toggle_sketch(world, sketch),
        // A planar part face (PS7.1).
        (ExtrudeField::Input, Pick::Face(_, f)) if !own(f.op) => {
            let (r_face, true) = face? else { return None };
            let mut r = rparams(world)?;
            let label = if let Some(i) = r.faces.iter().position(|x| x.face == f) {
                r.faces.remove(i);
                "Deselect face"
            } else {
                r.faces.push(r_face);
                "Select face"
            };
            set_rparams(world, r, label);
        }
        // P3.10 (PS7.2): the mate connector button: a connector's Z axis.
        (ExtrudeField::AxisConnector, pick) => {
            let features = world.get_resource::<ActiveDocument>()?.active_element()?.features().to_vec();
            let c = crate::pattern::connector_of(&features, world.resource::<PartCache>(), pick)?;
            let mut r = rparams(world)?;
            r.axis = Some(AxisRef::Connector(c));
            set_rparams(world, r, "Revolve axis");
            if let Some(mut s) = world.get_resource_mut::<ExtrudeSession>() {
                s.field = ExtrudeField::Axis;
            }
        }
        (ExtrudeField::Axis, pick) => {
            let is_connector = match pick {
                Pick::Feature(f) => world
                    .get_resource::<ActiveDocument>()
                    .and_then(|d| d.active_element())
                    .is_some_and(|e| crate::pattern::is_connector(e.features(), f)),
                _ => false,
            };
            let axis = match pick {
                // An explicit mate connector's Z axis (P3.8, PS7.2).
                Pick::Feature(f) if is_connector => Some(AxisRef::Connector(cadrs_core::mate::ConnectorRef::Feature(f))),
                Pick::SketchCurve(sketch, curve) => Some(AxisRef::SketchCurve { sketch, curve }),
                Pick::Edge(_, e) if !own(e.op()) => edge.map(AxisRef::Edge),
                // A curved face: its axis (a planar face has none).
                Pick::Face(_, f) if !own(f.op) => match face {
                    Some((r, false)) => Some(AxisRef::Face(r)),
                    _ => None,
                },
                _ => None,
            }?;
            let mut r = rparams(world)?;
            r.axis = Some(axis);
            set_rparams(world, r, "Revolve axis");
        }
        (ExtrudeField::UpTo | ExtrudeField::SecondUpTo, pick) => {
            let mut r = rparams(world)?;
            let second = field == ExtrudeField::SecondUpTo;
            let end = if second { r.second.as_ref().map(|s| s.end) } else { Some(r.kind.end_type()) };
            let target = match (end, pick) {
                (Some(EndType::UpToFace), Pick::Face(_, f)) if !own(f.op) => face.map(|(x, _)| UpTo::Face(x)),
                (Some(EndType::UpToPart), p) => p.part().map(UpTo::Part),
                (Some(EndType::UpToVertex), Pick::Vertex(..)) => vertex.map(UpTo::Vertex),
                _ => None,
            }?;
            if second {
                if let Some(s) = &mut r.second {
                    s.up_to = Some(target);
                }
            } else {
                r.up_to = Some(target);
            }
            set_rparams(world, r, "Select up to");
        }
        (ExtrudeField::MergeScope, pick) => {
            let part = pick.part()?;
            let mut r = rparams(world)?;
            if r.merge_scope.is_empty() {
                r.merge_scope = auto_scope;
            }
            if let Some(i) = r.merge_scope.iter().position(|p| *p == part) {
                r.merge_scope.remove(i);
            } else {
                r.merge_scope.push(part);
            }
            set_rparams(world, r, "Merge scope");
        }
        _ => {}
    }
    Some(())
}

/// A new revolve picks New or Add by itself (PS5.2, PS7.5), as an extrude does.
fn automatic_operation(session: Option<Res<ExtrudeSession>>, cache: Res<PartCache>, mut commands: Commands) {
    let Some(s) = session.filter(|s| s.op_auto) else {
        return;
    };
    if cache.rebuilding {
        return;
    }
    let touches = cache.contacts.get(&s.feature).is_some_and(|c| !c.touches.is_empty());
    let feature = s.feature;
    commands.queue(move |world: &mut World| {
        let Some(mut r) = rparams(world) else { return };
        if !matches!(r.op, BooleanOp::New | BooleanOp::Add) || r.body == cadrs_core::BodyType::Surface {
            return;
        }
        let want = if touches { BooleanOp::Add } else { BooleanOp::New };
        let known = world.resource::<PartCache>().contacts.contains_key(&feature);
        if r.op != want && (known || want == BooleanOp::New) && !r.is_empty() {
            r.op = want;
            set_rparams(world, r, want.label());
        }
    });
}

/// The dialog's references (the axis, the Up to targets) show as selected in the view.
fn show_references(
    session: Option<Res<ExtrudeSession>>,
    doc: Option<Res<ActiveDocument>>,
    cache: Res<PartCache>,
    mut selection: ResMut<Selection>,
) {
    let (Some(s), Some(doc)) = (session, doc) else {
        return;
    };
    let Some(r) = doc.doc.element(s.element).and_then(|el| el.feature(s.feature)).and_then(|f| f.revolve()) else {
        return;
    };
    let face_pick = |f: &FaceRef| {
        let part = cache
            .part(f.part)
            .filter(|p| p.solid.face(&f.face).is_some())
            .or_else(|| cache.parts.iter().find(|p| p.solid.face(&f.face).is_some()))?;
        Some(Pick::Face(part.id, f.face))
    };
    let mut want: Vec<Pick> = r.faces.iter().filter_map(face_pick).collect();
    match r.axis {
        Some(AxisRef::SketchCurve { sketch, curve }) => want.push(Pick::SketchCurve(sketch, curve)),
        Some(AxisRef::Edge(e)) => want.extend(
            cache
                .parts
                .iter()
                .find(|p| p.solid.edge(&e.edge).is_some())
                .map(|p| Pick::Edge(p.id, e.edge)),
        ),
        Some(AxisRef::Face(f)) => want.extend(face_pick(&f)),
        Some(AxisRef::Connector(c)) => want.extend(crate::pattern::connector_pick(&c)),
        None => {}
    }
    let ends = std::iter::once(r.up_to).chain(r.second.as_ref().map(|s| s.up_to));
    for u in ends.flatten() {
        let p = match u {
            UpTo::Face(f) => face_pick(&f),
            UpTo::Part(p) => cache.part(p).map(|_| Pick::Part(p)),
            UpTo::Vertex(v) => cache
                .parts
                .iter()
                .find(|p| p.solid.vertex(&v.vertex).is_some())
                .map(|p| Pick::Vertex(p.id, v.vertex)),
        };
        want.extend(p);
    }
    if selection.0 != want {
        selection.0 = want;
    }
}

/// A hovered or selected sketch curve (a revolve axis being picked, or picked): drawn over its
/// sketch in the hover orange, or the selection orange.
fn draw_sketch_curve_picks(
    cache: Res<PartCache>,
    highlight: Res<crate::viewport::PlaneHighlight>,
    selection: Res<Selection>,
    mut g: Gizmos<crate::sketch_draw::SketchHoverGizmos>,
) {
    for sc in &cache.sketch_curves {
        for (id, pts) in &sc.curves {
            let pick = Pick::SketchCurve(sc.sketch, *id);
            let color = if highlight.is_hovered(pick) {
                crate::parts::HOVER
            } else if selection.contains(pick) {
                crate::parts::SELECTED
            } else {
                continue;
            };
            g.linestrip(pts.iter().map(|p| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)), color);
        }
    }
}

/// The name a selection field shows for a revolve axis: "Edge of Sketch 2", "Edge of Extrude
/// 1", "Face of Extrude 1".
pub fn axis_label(features: &[cadrs_core::Feature], a: &AxisRef) -> String {
    let name = |op: cadrs_sketch::OpId| {
        features
            .iter()
            .find(|f| f.id.0 == op)
            .map_or("part".to_string(), |f| f.name.clone())
    };
    match a {
        AxisRef::SketchCurve { sketch, .. } => format!("Edge of {}", name(sketch.0)),
        AxisRef::Edge(e) => format!("Edge of {}", name(e.edge.op())),
        AxisRef::Face(f) => format!("Face of {}", name(f.face.op)),
        AxisRef::Connector(c) => c.label(features),
    }
}

// ---------------------------------------------------------------------------------------------
// The angle manipulator

/// The revolve's arrow manipulator: at the middle of its end face, pointing the way it turns;
/// dragging it along that way changes the angle (P3.4).
#[derive(Resource, Debug, Clone, Default)]
pub struct AngleArrow {
    /// Screen positions (logical px) of the arrow's base and tip, while it is shown.
    pub base_tip: Option<(Vec2, Vec2)>,
    pub hovered: bool,
    pub drag: Option<AngleDrag>,
    swallow_click: bool,
    /// Screen px per degree along the arrow, and the angle now.
    scale: Option<(Vec2, f64)>,
    /// The second end's arrow (P3.5): at the middle of its end face, turning the other way.
    pub second_base_tip: Option<(Vec2, Vec2)>,
    pub second_hovered: bool,
    second_scale: Option<(Vec2, f64)>,
}

#[derive(Debug, Clone)]
pub struct AngleDrag {
    start: Vec2,
    start_signed: f64,
    /// Screen px per degree along the turn.
    px_per_deg: Vec2,
    /// The flip when the drag began (dragging back through the start flips it).
    flip0: bool,
    pub revolve: RevolveFeature,
    /// The second end's arrow is dragged.
    second: bool,
}

/// An angle arrow: the first end's (`0`) or the second end's (`1`).
#[derive(Component, Clone, Copy)]
struct AngleArrowNode(usize);

#[derive(Component, Clone, Copy)]
struct AngleArrowLine(usize);

const ARROW_LEN: f32 = 48.0;
const ARROW_GRAB: f32 = 8.0;

/// The angle a drag of `delta` px gives: in whole degrees, never zero, at most a whole turn;
/// negative means flipped.
pub fn dragged_angle(start_signed: f64, px_per_deg: Vec2, delta: Vec2) -> (f64, bool) {
    let len2 = px_per_deg.length_squared().max(1e-9);
    let signed = start_signed + (delta.dot(px_per_deg) / len2) as f64;
    let angle = signed.abs().round().clamp(1.0, 360.0);
    (angle, signed < 0.0)
}

/// Ends an angle drag: with `keep`, records the dragged angle as one undo step.
pub fn end_angle_drag(world: &mut World, keep: bool) {
    let Some(d) = world.resource_mut::<AngleArrow>().drag.take() else {
        return;
    };
    let changed = rparams(world).is_some_and(|p| p != d.revolve);
    if keep && changed {
        set_rparams(world, d.revolve, "Drag angle");
    }
    world.resource_mut::<PartOverride>().revolve = None;
}

fn segment_distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

#[allow(clippy::too_many_arguments)]
fn angle_arrow_pointer(
    mut inputs: MessageReader<PointerInput>,
    session: Option<Res<ExtrudeSession>>,
    mut arrow: ResMut<AngleArrow>,
    mut over: ResMut<PartOverride>,
    doc: Option<Res<ActiveDocument>>,
    drag: Res<crate::viewport::ViewportDrag>,
    units: Res<crate::WorkspaceUnits>,
    mut commands: Commands,
) {
    let Some(s) = session else {
        inputs.clear();
        return;
    };
    let pointer = drag.pointer();
    arrow.hovered = arrow.base_tip.is_some_and(|(a, b)| segment_distance(pointer, a, b) <= ARROW_GRAB);
    arrow.second_hovered =
        !arrow.hovered && arrow.second_base_tip.is_some_and(|(a, b)| segment_distance(pointer, a, b) <= ARROW_GRAB);
    let params = doc
        .as_ref()
        .and_then(|d| d.doc.element(s.element)?.feature(s.feature)?.revolve().cloned());
    for input in inputs.read() {
        if input.pointer_id != PointerId::Mouse {
            continue;
        }
        let pos = input.location.position;
        match input.action {
            PointerAction::Press(PointerButton::Primary) => {
                let hit = arrow.base_tip.is_some_and(|(a, b)| segment_distance(pos, a, b) <= ARROW_GRAB);
                let hit2 = !hit
                    && arrow.second_base_tip.is_some_and(|(a, b)| segment_distance(pos, a, b) <= ARROW_GRAB);
                let scale = if hit { arrow.scale } else { arrow.second_scale };
                if let (true, Some(p), Some((px_per_deg, signed))) = (hit || hit2, params.clone(), scale)
                    && px_per_deg.length() > 1e-3
                {
                    arrow.drag = Some(AngleDrag {
                        start: pos,
                        start_signed: signed,
                        px_per_deg,
                        flip0: p.flip,
                        revolve: p,
                        second: hit2,
                    });
                }
            }
            PointerAction::Move { .. } => {
                if let Some(d) = arrow.drag.as_mut()
                    && d.second
                {
                    // The second end: its angle, in whole degrees (back through its start
                    // stops at 1°).
                    let (angle, crossed) = dragged_angle(d.start_signed, d.px_per_deg, pos - d.start);
                    let angle = if crossed { 1.0 } else { angle };
                    if let Some(end) = d.revolve.second.as_mut()
                        && (end.depth != angle || end.end != cadrs_core::EndType::Blind)
                    {
                        end.end = cadrs_core::EndType::Blind;
                        end.up_to = None;
                        end.offset = None;
                        end.depth = angle;
                        end.depth_expr = units.0.with_unit(angle, Quantity::Angle);
                    }
                    let want = Some((s.feature, d.revolve.clone()));
                    if over.revolve != want {
                        over.revolve = want;
                    }
                } else if let Some(d) = arrow.drag.as_mut() {
                    let (angle, crossed) = dragged_angle(d.start_signed, d.px_per_deg, pos - d.start);
                    let flip = d.flip0 ^ crossed;
                    let r = &mut d.revolve;
                    if (angle, flip) != (r.angle, r.flip) || !r.kind.has_angle() {
                        // Dragging an "Up to" end makes it Blind at the dragged angle.
                        if !r.kind.has_angle() {
                            r.kind = RevolveType::Blind;
                            r.up_to = None;
                            r.offset = None;
                        }
                        r.angle = angle;
                        if r.kind == RevolveType::Blind {
                            r.flip = flip;
                        }
                        r.angle_expr = units.0.with_unit(angle, Quantity::Angle);
                    }
                    let want = Some((s.feature, d.revolve.clone()));
                    if over.revolve != want {
                        over.revolve = want;
                    }
                }
            }
            PointerAction::Release(PointerButton::Primary) if arrow.drag.is_some() => {
                arrow.swallow_click = true;
                commands.queue(|world: &mut World| end_angle_drag(world, true));
            }
            PointerAction::Cancel => {
                commands.queue(|world: &mut World| end_angle_drag(world, false));
            }
            _ => {}
        }
    }
}

/// Places the arrow at the middle of the revolve's end face, pointing the way it turns there.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn place_angle_arrow(
    session: Option<Res<ExtrudeSession>>,
    cache: Res<PartCache>,
    doc: Option<Res<ActiveDocument>>,
    view: Res<ViewportView>,
    rect: Res<ViewportRect>,
    mut arrow: ResMut<AngleArrow>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut q: Query<(Entity, &mut Node, &mut UiTransform, &mut Visibility, &AngleArrowNode)>,
    mut q_line: Query<(&AngleArrowLine, &mut ImageNode)>,
    mut commands: Commands,
) {
    let target = session.as_ref().and_then(|s| {
        let r = arrow
            .drag
            .as_ref()
            .map(|d| d.revolve.clone())
            .or_else(|| doc.as_ref()?.doc.element(s.element)?.feature(s.feature)?.revolve().cloned())?;
        if r.kind == RevolveType::Full || cache.errors.contains_key(&s.feature) {
            return None;
        }
        // The middle of the end face(s) this revolve made.
        let mut sum = Vec3::ZERO;
        let mut n = 0.0;
        let faces = cache
            .parts
            .iter()
            .chain(cache.tool.iter())
            .filter(|p| p.features.contains(&s.feature))
            .flat_map(|p| p.solid.faces.iter());
        for f in faces {
            if let cadrs_sketch::FaceOrigin::Cap { end: true, .. } = f.name.origin
                && f.name.op == s.feature.0
                && let Some(l) = crate::parts::outer_loop(&f.loops)
            {
                for p in l {
                    sum += Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
                    n += 1.0;
                }
            }
        }
        if n == 0.0 {
            return None;
        }
        let anchor = sum / n;
        // The way the end turns there: about the axis, d × (anchor − origin); a degree moves
        // the anchor π/180 of its distance from the axis.
        let (o, d) = cache.axes.get(&s.feature)?;
        let o = Vec3::new(o[0] as f32, o[1] as f32, o[2] as f32);
        let d = Vec3::new(d[0] as f32, d[1] as f32, d[2] as f32);
        let turn = d.cross(anchor - o);
        if turn.length() < 1e-6 {
            return None;
        }
        let per_deg = turn * std::f32::consts::PI / 180.0;
        // The angle now (signed: negative when flipped); an "Up to" end's own angle isn't
        // known here, and a drag starts it from a quarter turn.
        let signed = if r.kind.has_angle() { r.angle } else { 90.0 };
        Some((anchor, turn.normalize(), per_deg, signed))
    });
    // The second end (P3.5): the middle of its end face (the start cap), turning the other way.
    let second = session.as_ref().and_then(|s| {
        let r = arrow
            .drag
            .as_ref()
            .map(|d| d.revolve.clone())
            .or_else(|| doc.as_ref()?.doc.element(s.element)?.feature(s.feature)?.revolve().cloned())?;
        let end = r.second.as_ref()?;
        if !r.kind.one_sided() || cache.errors.contains_key(&s.feature) {
            return None;
        }
        let mut sum = Vec3::ZERO;
        let mut n = 0.0;
        for p in cache.parts.iter().chain(cache.tool.iter()).filter(|p| p.features.contains(&s.feature)) {
            for f in &p.solid.faces {
                if let cadrs_sketch::FaceOrigin::Cap { end: false, .. } = f.name.origin
                    && f.name.op == s.feature.0
                    && let Some(l) = crate::parts::outer_loop(&f.loops)
                {
                    for q in l {
                        sum += Vec3::new(q[0] as f32, q[1] as f32, q[2] as f32);
                        n += 1.0;
                    }
                }
            }
        }
        if n == 0.0 {
            return None;
        }
        let anchor = sum / n;
        let (o, d) = cache.axes.get(&s.feature)?;
        let o = Vec3::new(o[0] as f32, o[1] as f32, o[2] as f32);
        let d = Vec3::new(d[0] as f32, d[1] as f32, d[2] as f32);
        let turn = -d.cross(anchor - o);
        if turn.length() < 1e-6 {
            return None;
        }
        let per_deg = turn * std::f32::consts::PI / 180.0;
        let signed = if end.end == cadrs_core::EndType::Blind { end.depth } else { 90.0 };
        Some((anchor, turn.normalize(), per_deg, signed))
    });
    arrow.second_scale = second.map(|(_, _, per_deg, signed)| (view.view.project_vector(per_deg), signed));
    arrow.second_base_tip = second.and_then(|(anchor, dir, _, _)| {
        let p = view.view.project_vector(dir);
        (p.length() >= 0.05).then(|| {
            let b = rect.to_screen(view.view.project(anchor));
            (b, b + p.normalize() * ARROW_LEN)
        })
    });
    arrow.scale = target.map(|(_, _, per_deg, signed)| {
        let px = view.view.project_vector(per_deg);
        (px, signed)
    });
    let Some((anchor, dir, _, _)) = target else {
        arrow.base_tip = None;
        arrow.second_base_tip = None;
        for (e, ..) in &q {
            commands.entity(e).try_despawn();
        }
        return;
    };
    let base = rect.to_screen(view.view.project(anchor));
    let d = view.view.project_vector(dir);
    arrow.base_tip = (d.length() >= 0.05).then(|| (base, base + d.normalize() * ARROW_LEN));
    let arrows = [
        (arrow.base_tip, arrow.hovered || arrow.drag.as_ref().is_some_and(|d| !d.second)),
        (arrow.second_base_tip, arrow.second_hovered || arrow.drag.as_ref().is_some_and(|d| d.second)),
    ];
    if q.iter().count() < 2 {
        let Some(area) = q_area.iter().next() else {
            return;
        };
        for (e, ..) in &q {
            commands.entity(e).try_despawn();
        }
        for i in 0..2 {
            let e = commands.spawn(angle_arrow_node(i)).id();
            commands.entity(area).add_child(e);
        }
        return;
    }
    for (_, mut node, mut transform, mut vis, which) in &mut q {
        let (bt, active) = arrows[which.0];
        let Some((base, tip)) = bt else {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        };
        let u = (tip - base).normalize_or_zero();
        let center = (base + tip) / 2.0 - rect.0.min;
        let (l, t) = (Val::Px(center.x - ARROW_LEN / 2.0), Val::Px(center.y - ARROW_LEN / 2.0));
        if node.left != l || node.top != t {
            node.left = l;
            node.top = t;
        }
        let want = UiTransform {
            rotation: Rot2::radians(u.x.atan2(-u.y)),
            ..default()
        };
        if *transform != want {
            *transform = want;
        }
        vis.set_if_neq(Visibility::Inherited);
        let c = if active { Color::srgb_u8(0xff, 0xb4, 0x5a) } else { Color::WHITE };
        for (line, mut img) in &mut q_line {
            if line.0 == which.0 && img.color != c {
                img.color = c;
            }
        }
    }
}

/// An angle arrow's node: `revolve-arrow` (the first end) or `revolve-arrow-2`.
fn angle_arrow_node(i: usize) -> impl Bundle {
    (
        Name::new(if i == 0 { "revolve-arrow".to_string() } else { "revolve-arrow-2".to_string() }),
        AngleArrowNode(i),
        Node {
            position_type: PositionType::Absolute,
            width: Val::Px(ARROW_LEN),
            height: Val::Px(ARROW_LEN),
            ..default()
        },
        UiTransform::default(),
        Visibility::Hidden,
        Pickable::IGNORE,
        ZIndex(-3),
        DespawnOnExit(AppState::Document),
        children![
            (
                cadrs_ui::icon::icon_in(
                    "manipulator-arrow-halo",
                    ARROW_LEN,
                    Color::srgba_u8(0x3c, 0x46, 0x4e, 0xb0),
                    Node {
                        position_type: PositionType::Absolute,
                        ..default()
                    },
                ),
                Pickable::IGNORE,
            ),
            (
                AngleArrowLine(i),
                cadrs_ui::icon::icon_in(
                    "manipulator-arrow-line",
                    ARROW_LEN,
                    Color::WHITE,
                    Node {
                        position_type: PositionType::Absolute,
                        ..default()
                    },
                ),
                Pickable::IGNORE,
            ),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dragging_the_angle_snaps_and_flips() {
        // 2 px per degree to the right.
        let k = Vec2::new(2.0, 0.0);
        assert_eq!(dragged_angle(90.0, k, Vec2::new(20.0, 0.0)), (100.0, false));
        assert_eq!(dragged_angle(90.0, k, Vec2::new(21.0, 3.0)), (101.0, false));
        // Back through zero: flipped.
        assert_eq!(dragged_angle(10.0, k, Vec2::new(-60.0, 0.0)), (20.0, true));
        // Never zero, never past a whole turn.
        assert_eq!(dragged_angle(10.0, k, Vec2::new(-20.0, 0.0)).0, 1.0);
        assert_eq!(dragged_angle(350.0, k, Vec2::new(100.0, 0.0)).0, 360.0);
    }
}
