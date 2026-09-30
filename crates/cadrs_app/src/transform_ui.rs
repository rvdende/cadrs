//! The **Transform** feature in the applied-feature dialogs, as Onshape lays it out: the
//! transform type (Translate by line, Translate by distance, Translate by XYZ, Transform by mate
//! connectors, Rotate, Copy in place, Scale uniformly); *Parts to transform*; the type's fields
//! (*Line or axis*; *Direction* and *Distance*; *X*, *Y*, *Z*; *From* and *To* mate connectors
//! with the flip primary / reorient secondary buttons; *Axis of rotation* and *Angle*; *Point to
//! scale about* and *Scale*), the opposite-direction flips; and **Copy part**. The toolbar's
//! Transform button starts it with the selected parts. The result shows while the dialog is
//! open, with the parts' outlines where they were before it.
//!
//! P3H.6 (PCB7.9): in a Part Studio in the context of an assembly, *Parts to transform* also takes
//! the context's parts (listed by their instance names); they can only be copied (Copy in place,
//! or Copy part). A box dragged on empty space adds the parts inside it ([`add_parts`]).

use bevy::prelude::*;
use cadrs_core::document::{AxisRef, DirectionRef};
use cadrs_core::mate::ConnectorRef;
use cadrs_core::transform::{SecondaryAxis, TransformFeature, TransformType};
use cadrs_core::{Feature, FeatureKind, PartId};
use cadrs_ui::OptionRow;
use cadrs_ui::prelude::*;

use crate::applied::AppliedField;
use crate::applied_dialog::{Role, body_column, index_of, list, number, opts, select_row};
use crate::parts::PartCache;
use crate::viewport::Pick;
use crate::ActiveDocument;

/// A new Transform from the selection: its parts, and the field that takes the next picks.
pub fn initial(doc: &cadrs_core::Document, element: cadrs_core::ElementId, picked: &[Pick]) -> (TransformFeature, AppliedField) {
    let mut parts: Vec<PartId> = Vec::new();
    for p in picked {
        if let Pick::Part(id) = p
            && !parts.contains(id)
        {
            parts.push(*id);
        }
    }
    let mut x = TransformFeature::default();
    x.set_picked(doc, element, &parts);
    let field = if x.picked().is_empty() { AppliedField::TransformParts } else { first_field(x.transform_type) };
    (x, field)
}

/// Adds `parts` (a box selection) to the open Transform's Parts to transform.
pub fn add_parts(world: &mut World, parts: &[PartId]) {
    let Some(mut kind) = crate::applied::current(world).map(|f| f.kind) else { return };
    let FeatureKind::Transform(x) = &mut kind else { return };
    let mut all = x.picked();
    for p in parts {
        if !all.contains(p) {
            all.push(*p);
        }
    }
    let Some((d, el)) = world.get_resource::<ActiveDocument>().and_then(|d| Some((d, d.active_element()?.id))) else { return };
    x.set_picked(&d.doc, el, &all);
    crate::applied::set(world, kind, "Select parts");
}

/// The field a type's first reference goes in (the parts' for the types without one).
pub fn first_field(t: TransformType) -> AppliedField {
    match t {
        TransformType::TranslateByLine => AppliedField::TransformLine,
        TransformType::TranslateByDistance => AppliedField::TransformDirection,
        TransformType::MateConnectors => AppliedField::TransformFrom,
        TransformType::Rotate => AppliedField::TransformAxis,
        TransformType::ScaleUniformly => AppliedField::TransformScalePoint,
        TransformType::TranslateXyz | TransformType::CopyInPlace => AppliedField::TransformParts,
    }
}

fn toggle<T: PartialEq>(list: &mut Vec<T>, x: T) {
    if let Some(i) = list.iter().position(|y| *y == x) {
        list.remove(i);
    } else {
        list.push(x);
    }
}

fn set_or_clear<T: PartialEq>(slot: &mut Option<T>, x: T) {
    *slot = if slot.as_ref() == Some(&x) { None } else { Some(x) };
}

/// A pick into the Transform's fields. Returns false if it doesn't fit the field.
pub fn pick(world: &mut World, kind: &mut FeatureKind, field: AppliedField, pick: Pick) -> bool {
    let FeatureKind::Transform(x) = kind else { return false };
    let Some(features) = world.get_resource::<ActiveDocument>().and_then(|d| d.active_element()).map(|e| e.features().to_vec()) else {
        return false;
    };
    let cache = world.resource::<PartCache>();
    let mut next: Option<AppliedField> = None;
    match field {
        AppliedField::TransformParts => match pick.part() {
            Some(p) => {
                // Its own parts and (P3H.6) the assembly context's.
                let mut all = x.picked();
                toggle(&mut all, p);
                let Some((d, el)) = world.get_resource::<ActiveDocument>().and_then(|d| Some((d, d.active_element()?.id))) else { return false };
                x.set_picked(&d.doc, el, &all);
            }
            None => return false,
        },
        AppliedField::TransformLine => match crate::pattern::direction_of(&features, cache, pick) {
            Some(d @ (DirectionRef::Edge(_) | DirectionRef::SketchLine { .. })) => set_or_clear(&mut x.line, d),
            _ => return false,
        },
        AppliedField::TransformDirection => match crate::pattern::direction_of(&features, cache, pick) {
            Some(d) => set_or_clear(&mut x.direction, d),
            None => return false,
        },
        AppliedField::TransformFrom => match crate::pattern::connector_of(&features, cache, pick) {
            Some(c) => {
                set_or_clear(&mut x.from, c);
                // Then the destination.
                if x.from.is_some() && x.to.is_none() {
                    next = Some(AppliedField::TransformTo);
                }
            }
            None => return false,
        },
        AppliedField::TransformTo => match crate::pattern::connector_of(&features, cache, pick) {
            Some(c) => set_or_clear(&mut x.to, c),
            None => return false,
        },
        AppliedField::TransformAxis => match crate::pattern::axis_of(&features, cache, pick) {
            Some(a) => set_or_clear(&mut x.axis, a),
            None => return false,
        },
        AppliedField::TransformScalePoint => match crate::pattern::connector_of(&features, cache, pick) {
            Some(c) => set_or_clear(&mut x.scale_point, c),
            None => return false,
        },
        _ => return false,
    }
    if let (Some(f), Some(mut s)) = (next, world.get_resource_mut::<crate::applied::AppliedSession>()) {
        s.field = f;
    }
    true
}

/// What the Transform's fields refer to, shown selected in the view while its dialog is open.
pub fn references(kind: &FeatureKind, cache: &PartCache) -> Vec<Pick> {
    let FeatureKind::Transform(x) = kind else { return Vec::new() };
    let face = |f: &cadrs_core::FaceRef| cache.parts.iter().find(|p| p.solid.face(&f.face).is_some()).map(|p| Pick::Face(p.id, f.face));
    let edge = |e: &cadrs_core::EdgeRef| cache.parts.iter().find(|p| p.solid.edge(&e.edge).is_some()).map(|p| Pick::Edge(p.id, e.edge));
    let direction = |d: &DirectionRef| match d {
        DirectionRef::Edge(e) => edge(e),
        DirectionRef::FaceNormal(f) => face(f),
        DirectionRef::SketchLine { sketch, curve } => Some(Pick::SketchCurve(*sketch, *curve)),
        DirectionRef::PlaneNormal(p) => crate::viewport::plane_pick(*p),
        DirectionRef::Connector(c) => crate::pattern::connector_pick(c),
    };
    let mut out: Vec<Pick> = x.picked().into_iter().map(Pick::Part).collect();
    match x.transform_type {
        TransformType::TranslateByLine => out.extend(x.line.iter().filter_map(direction)),
        TransformType::TranslateByDistance => out.extend(x.direction.iter().filter_map(direction)),
        TransformType::MateConnectors => out.extend([&x.from, &x.to].into_iter().flatten().filter_map(crate::pattern::connector_pick)),
        TransformType::Rotate => out.extend(x.axis.iter().filter_map(|a| match a {
            AxisRef::SketchCurve { sketch, curve } => Some(Pick::SketchCurve(*sketch, *curve)),
            AxisRef::Edge(e) => edge(e),
            AxisRef::Face(f) => face(f),
            AxisRef::Connector(c) => crate::pattern::connector_pick(c),
        })),
        TransformType::ScaleUniformly => out.extend(x.scale_point.iter().filter_map(crate::pattern::connector_pick)),
        TransformType::TranslateXyz | TransformType::CopyInPlace => {}
    }
    out
}

/// True when the active field takes mate connectors (their hover preview shows).
pub fn takes_connectors(kind: &FeatureKind, field: AppliedField) -> bool {
    matches!(kind, FeatureKind::Transform(_))
        && matches!(
            field,
            AppliedField::TransformFrom | AppliedField::TransformTo | AppliedField::TransformScalePoint | AppliedField::TransformAxis | AppliedField::TransformDirection
        )
}

// ---------------------------------------------------------------------------------------------
// The dialog

pub(crate) fn name(kind: &FeatureKind) -> Option<&'static str> {
    matches!(kind, FeatureKind::Transform(_)).then_some("transform").or_else(|| crate::surfacing_ui::name(kind))
}

pub(crate) fn layout(kind: &FeatureKind) -> Option<String> {
    match kind {
        FeatureKind::Transform(x) => Some(format!("transform {:?} {} {} {} {:?}", x.transform_type, x.copy, x.flip, x.flip_primary, x.secondary)),
        k => crate::surfacing_ui::layout(k),
    }
}

fn op_name(features: &[Feature], op: cadrs_sketch::OpId) -> String {
    features.iter().find(|f| f.id.0 == op).map_or("part".into(), |f| f.name.clone())
}

fn name_of(features: &[Feature], id: cadrs_core::FeatureId) -> String {
    features.iter().find(|f| f.id == id).map_or("sketch".into(), |f| f.name.clone())
}

fn direction_label(features: &[Feature], d: &DirectionRef) -> String {
    match d {
        DirectionRef::Edge(r) => format!("Edge of {}", op_name(features, crate::parts::edge_maker(features, &r.edge))),
        DirectionRef::SketchLine { sketch, .. } => format!("Line of {}", name_of(features, *sketch)),
        DirectionRef::FaceNormal(f) => format!("Face of {}", op_name(features, f.face.op)),
        DirectionRef::PlaneNormal(p) => crate::viewport::plane_label(features, *p),
        DirectionRef::Connector(c) => c.label(features),
    }
}

fn connector_label(features: &[Feature], c: &ConnectorRef) -> String {
    match c {
        ConnectorRef::Implicit(o) => crate::pattern_dialog::origin_label(features, o),
        c => c.label(features),
    }
}

pub(crate) fn items(features: &[Feature], cache: &PartCache, kind: &FeatureKind, role: Role) -> Option<Vec<String>> {
    let FeatureKind::Transform(x) = kind else { return crate::surfacing_ui::items(features, cache, kind, role) };
    Some(match role {
        Role::TransformParts => crate::applied::part_names(cache, &x.parts).into_iter().chain(x.context.iter().map(|c| c.name.clone())).collect(),
        Role::TransformLine => x.line.iter().map(|d| direction_label(features, d)).collect(),
        Role::TransformDirection => x.direction.iter().map(|d| direction_label(features, d)).collect(),
        Role::TransformFrom => x.from.iter().map(|c| connector_label(features, c)).collect(),
        Role::TransformTo => x.to.iter().map(|c| connector_label(features, c)).collect(),
        Role::TransformAxis => x.axis.iter().map(|a| crate::revolve::axis_label(features, a)).collect(),
        Role::TransformScalePoint => x.scale_point.iter().map(|c| connector_label(features, c)).collect(),
        _ => return None,
    })
}

pub(crate) fn list_field(role: Role) -> Option<AppliedField> {
    Some(match role {
        Role::TransformParts => AppliedField::TransformParts,
        Role::TransformLine => AppliedField::TransformLine,
        Role::TransformDirection => AppliedField::TransformDirection,
        Role::TransformFrom => AppliedField::TransformFrom,
        Role::TransformTo => AppliedField::TransformTo,
        Role::TransformAxis => AppliedField::TransformAxis,
        Role::TransformScalePoint => AppliedField::TransformScalePoint,
        r => return crate::surfacing_ui::list_field(r),
    })
}

/// A list with the opposite-direction flip beside it (Translate by line).
#[allow(clippy::too_many_arguments)]
fn list_with_flip(b: &mut ChildSpawner, t: &Theme, name: &str, placeholder: &str, role: Role, items: Vec<String>, active: bool, flip: bool) {
    b.spawn(Node { align_items: AlignItems::FlexStart, column_gap: Val::Px(2.0), ..default() }).with_children(|r| {
        r.spawn(Node { flex_grow: 1.0, flex_basis: Val::Px(0.0), min_width: Val::Px(0.0), flex_direction: FlexDirection::Column, ..default() })
            .with_children(|c| list(c, t, name, placeholder, role, items, active));
        r.spawn(Node { margin: UiRect::top(Val::Px(4.0)), ..default() }).with_children(|c| {
            crate::extrude_dialog::flip_button_any(c, t, &format!("{name}-flip"), Role::Flip, flip, "Opposite direction");
        });
    });
}

pub(crate) fn body(b: &mut ChildSpawner, t: &Theme, kind: &FeatureKind, field: AppliedField, items_of: &dyn Fn(Role) -> Vec<String>) {
    let FeatureKind::Transform(x) = kind else { return };
    body_column(b, |b| {
        let types = opts(&TransformType::ALL, |m| m.label(), |_| true);
        select_row(b, t, "transform-type", "", Role::TransformType, &types, index_of(&TransformType::ALL, &x.transform_type), None);
        list(b, t, "transform-parts-field", "Parts to transform", Role::TransformParts, items_of(Role::TransformParts), field == AppliedField::TransformParts);
        let active = |f: AppliedField| field == f;
        match x.transform_type {
            TransformType::TranslateByLine => list_with_flip(
                b,
                t,
                "transform-line-field",
                "Line or axis",
                Role::TransformLine,
                items_of(Role::TransformLine),
                active(AppliedField::TransformLine),
                x.flip,
            ),
            TransformType::TranslateByDistance => {
                list(b, t, "transform-direction-field", "Direction", Role::TransformDirection, items_of(Role::TransformDirection), active(AppliedField::TransformDirection));
                number(b, t, "transform-distance", "Distance", Role::TransformDistance, &x.distance_expr, Some(x.flip));
            }
            TransformType::TranslateXyz => {
                number(b, t, "transform-x", "X", Role::TransformX, &x.dx_expr, None);
                number(b, t, "transform-y", "Y", Role::TransformY, &x.dy_expr, None);
                number(b, t, "transform-z", "Z", Role::TransformZ, &x.dz_expr, None);
            }
            TransformType::MateConnectors => {
                list(b, t, "transform-from-field", "From mate connector", Role::TransformFrom, items_of(Role::TransformFrom), active(AppliedField::TransformFrom));
                list(b, t, "transform-to-field", "To mate connector", Role::TransformTo, items_of(Role::TransformTo), active(AppliedField::TransformTo));
                b.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(4.0), margin: UiRect::new(Val::Px(2.0), Val::ZERO, Val::Px(5.0), Val::Px(2.0)), ..default() })
                    .with_children(|r| {
                        crate::pattern_dialog::icon_button(r, t, "transform-flip-primary", Role::TransformFlipPrimary, "flip-direction-up", "Flip primary axis", x.flip_primary);
                        let tip = format!("Reorient secondary axis ({})", x.secondary.onshape().replace('_', " ").to_lowercase());
                        crate::pattern_dialog::icon_button(r, t, "transform-reorient", Role::TransformReorient, "revolve", &tip, x.secondary != SecondaryAxis::PlusX);
                    });
            }
            TransformType::Rotate => {
                list(b, t, "transform-axis-field", "Axis of rotation", Role::TransformAxis, items_of(Role::TransformAxis), active(AppliedField::TransformAxis));
                number(b, t, "transform-angle", "Angle", Role::TransformAngle, &x.angle_expr, Some(x.flip));
            }
            TransformType::CopyInPlace => {}
            TransformType::ScaleUniformly => {
                list(
                    b,
                    t,
                    "transform-scale-point-field",
                    "Point to scale about (origin)",
                    Role::TransformScalePoint,
                    items_of(Role::TransformScalePoint),
                    active(AppliedField::TransformScalePoint),
                );
                number(b, t, "transform-scale", "Scale", Role::TransformScale, &x.scale_expr, None);
            }
        }
        if x.transform_type != TransformType::CopyInPlace {
            b.spawn(OptionRow::new("transform-copy", "Copy part").checked(x.copy).build(t));
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Keeping it in step and input

pub(crate) fn number_text(kind: &FeatureKind, role: Role) -> Option<String> {
    let FeatureKind::Transform(x) = kind else { return crate::surfacing_ui::number_text(kind, role) };
    Some(match role {
        Role::TransformDistance => x.distance_expr.clone(),
        Role::TransformX => x.dx_expr.clone(),
        Role::TransformY => x.dy_expr.clone(),
        Role::TransformZ => x.dz_expr.clone(),
        Role::TransformAngle => x.angle_expr.clone(),
        Role::TransformScale => x.scale_expr.clone(),
        _ => return None,
    })
}

pub(crate) fn select_index(kind: &FeatureKind, role: Role) -> Option<usize> {
    match (kind, role) {
        (FeatureKind::Transform(x), Role::TransformType) => Some(index_of(&TransformType::ALL, &x.transform_type)),
        (k, r) => crate::surfacing_ui::select_index(k, r),
    }
}

pub(crate) fn select(k: &mut FeatureKind, role: Role, i: usize) {
    match (k, role) {
        (FeatureKind::Transform(x), Role::TransformType) => x.transform_type = TransformType::ALL[i.min(TransformType::ALL.len() - 1)],
        (k, r) => crate::surfacing_ui::select(k, r, i),
    }
}

pub(crate) fn is_checkbox(name: &str) -> bool {
    name == "transform-copy-checkbox" || crate::surfacing_ui::is_checkbox(name)
}

pub(crate) fn checkbox(k: &mut FeatureKind, name: &str, on: bool) -> Option<&'static str> {
    match (k, name) {
        (FeatureKind::Transform(x), "transform-copy-checkbox") => {
            x.copy = on;
            Some("Copy part")
        }
        (k, n) => crate::surfacing_ui::checkbox(k, n, on),
    }
}

/// Numbers of any sign (lengths, and the angle).
pub(crate) fn is_signed(role: Role) -> bool {
    matches!(role, Role::TransformDistance | Role::TransformX | Role::TransformY | Role::TransformZ | Role::TransformAngle)
        || crate::surfacing_ui::is_signed(role)
}

/// The scale: a plain number greater than zero.
pub(crate) fn is_plain(role: Role) -> bool {
    role == Role::TransformScale || crate::surfacing_ui::is_plain(role)
}

pub(crate) fn is_angle(role: Role) -> bool {
    role == Role::TransformAngle || crate::surfacing_ui::is_angle(role)
}

pub(crate) fn number_label(role: Role) -> Option<&'static str> {
    Some(match role {
        Role::TransformDistance => "Distance",
        Role::TransformX => "X",
        Role::TransformY => "Y",
        Role::TransformZ => "Z",
        Role::TransformAngle => "Angle",
        Role::TransformScale => "Scale",
        r => return crate::surfacing_ui::number_label(r),
    })
}

pub(crate) fn set_number(k: &mut FeatureKind, role: Role, v: f64, expr: String) {
    if !matches!(k, FeatureKind::Transform(_)) {
        crate::surfacing_ui::set_number(k, role, v, expr);
        return;
    }
    let FeatureKind::Transform(x) = k else { return };
    let (value, text) = match role {
        Role::TransformDistance => (&mut x.distance, &mut x.distance_expr),
        Role::TransformX => (&mut x.dx, &mut x.dx_expr),
        Role::TransformY => (&mut x.dy, &mut x.dy_expr),
        Role::TransformZ => (&mut x.dz, &mut x.dz_expr),
        Role::TransformAngle => (&mut x.angle, &mut x.angle_expr),
        Role::TransformScale => (&mut x.scale, &mut x.scale_expr),
        _ => return,
    };
    *value = v;
    *text = expr;
}

/// The opposite-direction flip, the flip primary axis and reorient secondary axis buttons.
pub(crate) fn flip(k: &mut FeatureKind, role: Role) {
    if !matches!(k, FeatureKind::Transform(_)) {
        crate::surfacing_ui::flip(k, role);
        return;
    }
    let FeatureKind::Transform(x) = k else { return };
    match role {
        Role::Flip => x.flip = !x.flip,
        Role::TransformFlipPrimary => x.flip_primary = !x.flip_primary,
        Role::TransformReorient => x.secondary = x.secondary.next(),
        _ => {}
    }
}

/// An item's ✕.
pub(crate) fn remove(k: &mut FeatureKind, role: Role, i: usize) {
    if !matches!(k, FeatureKind::Transform(_)) {
        crate::surfacing_ui::remove(k, role, i);
        return;
    }
    let FeatureKind::Transform(x) = k else { return };
    match role {
        Role::TransformParts if i < x.parts.len() => {
            x.parts.remove(i);
        }
        // P3H.6: a context part (listed after the studio's own; its source snapshot stays, as
        // the other copies index the sources).
        Role::TransformParts if i - x.parts.len() < x.context.len() => {
            x.context.remove(i - x.parts.len());
        }
        Role::TransformLine => x.line = None,
        Role::TransformDirection => x.direction = None,
        Role::TransformFrom => x.from = None,
        Role::TransformTo => x.to = None,
        Role::TransformAxis => x.axis = None,
        Role::TransformScalePoint => x.scale_point = None,
        _ => {}
    }
}

// ---------------------------------------------------------------------------------------------
// Translate by XYZ: the arrows

/// Translate by XYZ's arrow manipulators, one per world axis at the middle of the moved parts
/// (the extrude's arrows, three times): dragging one changes that axis's distance live, snapped
/// like the extrude's depth, and records one undo step on release.
#[derive(Resource, Debug, Default)]
pub struct XyzArrows {
    /// Each axis's base and tip on screen (none when it points at the viewer).
    pub base_tip: [Option<(Vec2, Vec2)>; 3],
    pub hovered: Option<usize>,
    pub drag: Option<XyzDrag>,
}

#[derive(Debug, Clone)]
pub struct XyzDrag {
    axis: usize,
    start: Vec2,
    start_value: f64,
    /// Screen px per mm along the axis.
    dir_px: Vec2,
    /// The feature's parameters as dragged.
    pub kind: FeatureKind,
}

const XYZ_ARROW_LEN: f32 = 48.0;
const XYZ_GRAB: f32 = 8.0;
const AXES: [Vec3; 3] = [Vec3::X, Vec3::Y, Vec3::Z];
const AXIS_NAMES: [&str; 3] = ["x", "y", "z"];

fn axis_value(x: &TransformFeature, axis: usize) -> f64 {
    [x.dx, x.dy, x.dz][axis]
}

fn set_axis_value(x: &mut TransformFeature, axis: usize, v: f64, expr: String) {
    let (value, text) = match axis {
        0 => (&mut x.dx, &mut x.dx_expr),
        1 => (&mut x.dy, &mut x.dy_expr),
        _ => (&mut x.dz, &mut x.dz_expr),
    };
    *value = v;
    *text = expr;
}

/// The arrow under `p`: the nearest one within reach, measured along its outer three quarters
/// (the three share a base).
fn arrow_at(arrows: &XyzArrows, p: Vec2) -> Option<usize> {
    let dist = |(a, b): (Vec2, Vec2)| {
        let a = a + (b - a) * 0.25;
        let ab = b - a;
        let t = ((p - a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0.0, 1.0);
        p.distance(a + ab * t)
    };
    (0..3)
        .filter_map(|i| Some((i, dist(arrows.base_tip[i]?))))
        .filter(|(_, d)| *d <= XYZ_GRAB)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// Grabs, drags and releases the arrows.
#[allow(clippy::too_many_arguments)]
pub(crate) fn xyz_arrow_pointer(
    mut inputs: MessageReader<bevy::picking::pointer::PointerInput>,
    session: Option<Res<crate::applied::AppliedSession>>,
    mut arrows: ResMut<XyzArrows>,
    doc: Option<Res<ActiveDocument>>,
    view: Res<crate::viewport::ViewportView>,
    drag: Res<crate::viewport::ViewportDrag>,
    units: Res<crate::WorkspaceUnits>,
    mut over: ResMut<crate::parts::PartOverride>,
    mut grab: ResMut<crate::assembly::ViewportGrab>,
    mut commands: Commands,
) {
    use bevy::picking::pointer::{PointerAction, PointerButton, PointerId};
    let Some(s) = session.filter(|s| s.kind == crate::applied::AppliedKind::Transform) else {
        inputs.clear();
        return;
    };
    arrows.hovered = arrows.drag.as_ref().map(|d| d.axis).or_else(|| arrow_at(&arrows, drag.pointer()));
    let kind = doc.as_ref().and_then(|d| Some(d.doc.element(s.element)?.feature(s.feature)?.kind.clone()));
    for input in inputs.read() {
        if input.pointer_id != PointerId::Mouse {
            continue;
        }
        let pos = input.location.position;
        match input.action {
            PointerAction::Press(PointerButton::Primary) => {
                if let (Some(axis), Some(k)) = (arrow_at(&arrows, pos), kind.as_ref())
                    && let FeatureKind::Transform(x) = k
                {
                    let dir_px = view.view.project_vector(AXES[axis]);
                    if dir_px.length() > 0.05 {
                        let start_value = axis_value(x, axis);
                        arrows.drag = Some(XyzDrag { axis, start: pos, start_value, dir_px, kind: k.clone() });
                        // Not a click that picks a part.
                        grab.0 = true;
                    }
                }
            }
            PointerAction::Move { .. } => {
                if let Some(d) = arrows.drag.as_mut() {
                    let along = (pos - d.start).dot(d.dir_px.normalize()) / d.dir_px.length();
                    let step = crate::extrude::snap_step(d.dir_px.length());
                    let v = ((d.start_value + along as f64) / step).round() * step;
                    // Keep the snapped value tidy, and never -0.
                    let v = (v * 1e6).round() / 1e6 + 0.0;
                    if let FeatureKind::Transform(x) = &mut d.kind
                        && (axis_value(x, d.axis) - v).abs() > 1e-9
                    {
                        set_axis_value(x, d.axis, v, units.0.with_unit(v, cadrs_sketch::units::Quantity::Length));
                    }
                    let want = Some((s.feature, d.kind.clone()));
                    if over.applied != want {
                        over.applied = want;
                    }
                }
            }
            PointerAction::Release(PointerButton::Primary) if arrows.drag.is_some() => {
                commands.queue(|world: &mut World| {
                    world.resource_mut::<crate::parts::PartOverride>().applied = None;
                    let Some(d) = world.resource_mut::<XyzArrows>().drag.take() else { return };
                    if crate::applied::current(world).is_some_and(|f| f.kind != d.kind) {
                        let label = format!("Drag {}", AXIS_NAMES[d.axis].to_uppercase());
                        crate::applied::set(world, d.kind, &label);
                    }
                });
            }
            PointerAction::Cancel => {
                arrows.drag = None;
                over.applied = None;
            }
            _ => {}
        }
    }
}

/// One arrow (a dark halo and a line in the axis's colour).
#[derive(Component)]
pub(crate) struct XyzArrowNode(usize);

#[derive(Component)]
pub(crate) struct XyzArrowLine(usize);

/// The axes' colours (red X, green Y, blue Z); orange under the pointer or dragged, as the
/// extrude's arrow.
fn axis_color(axis: usize, active: bool) -> Color {
    if active {
        return Color::srgb_u8(0xff, 0xb4, 0x5a);
    }
    match axis {
        0 => Color::srgb_u8(0xf0, 0x5a, 0x5a),
        1 => Color::srgb_u8(0x4c, 0xc0, 0x5c),
        _ => Color::srgb_u8(0x4a, 0x8c, 0xf0),
    }
}

/// Places the arrows at the middle of the parts to transform, where they are moved to (with the
/// dragged value while dragged).
#[allow(clippy::too_many_arguments)]
pub(crate) fn place_xyz_arrows(
    session: Option<Res<crate::applied::AppliedSession>>,
    doc: Option<Res<ActiveDocument>>,
    before: Res<crate::applied::BeforeParts>,
    view: Res<crate::viewport::ViewportView>,
    rect: Res<crate::viewport::ViewportRect>,
    mut arrows: ResMut<XyzArrows>,
    q_area: Query<Entity, With<crate::viewport::ViewportArea>>,
    mut q: Query<(Entity, &XyzArrowNode, &mut Node, &mut bevy::ui::UiTransform, &mut Visibility)>,
    mut q_line: Query<(&XyzArrowLine, &mut ImageNode)>,
    mut commands: Commands,
) {
    let anchor = session.as_ref().filter(|s| s.kind == crate::applied::AppliedKind::Transform).and_then(|s| {
        let kind = match arrows.drag.as_ref() {
            Some(d) => d.kind.clone(),
            None => doc.as_ref()?.doc.element(s.element)?.feature(s.feature)?.kind.clone(),
        };
        let FeatureKind::Transform(x) = kind else { return None };
        if x.transform_type != TransformType::TranslateXyz {
            return None;
        }
        // The middle of the box around the parts before they move.
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for p in before.parts.iter().filter(|p| x.parts.contains(&p.id)) {
            let Some((a, b)) = p.solid.bounds() else { continue };
            lo = lo.min(Vec3::new(a[0] as f32, a[1] as f32, a[2] as f32));
            hi = hi.max(Vec3::new(b[0] as f32, b[1] as f32, b[2] as f32));
        }
        (lo.x <= hi.x).then(|| (lo + hi) / 2.0 + Vec3::new(x.dx as f32, x.dy as f32, x.dz as f32))
    });
    arrows.base_tip = [0, 1, 2].map(|i| {
        let p = anchor?;
        let d = view.view.project_vector(AXES[i]);
        (d.length() >= 0.05).then(|| {
            let b = rect.to_screen(view.view.project(p));
            (b, b + d.normalize() * XYZ_ARROW_LEN)
        })
    });
    if anchor.is_none() {
        for (e, ..) in &q {
            commands.entity(e).try_despawn();
        }
        return;
    }
    if q.is_empty() {
        let Some(area) = q_area.iter().next() else { return };
        for (i, axis) in AXIS_NAMES.iter().enumerate() {
            let e = commands
                .spawn((
                    Name::new(format!("transform-{axis}-arrow")),
                    XyzArrowNode(i),
                    Node { position_type: PositionType::Absolute, width: Val::Px(XYZ_ARROW_LEN), height: Val::Px(XYZ_ARROW_LEN), ..default() },
                    bevy::ui::UiTransform::default(),
                    Visibility::Hidden,
                    Pickable::IGNORE,
                    ZIndex(-3),
                    DespawnOnExit(crate::AppState::Document),
                    children![
                        (
                            cadrs_ui::icon::icon_in(
                                "manipulator-arrow-halo",
                                XYZ_ARROW_LEN,
                                Color::srgba_u8(0x3c, 0x46, 0x4e, 0xb0),
                                Node { position_type: PositionType::Absolute, ..default() },
                            ),
                            Pickable::IGNORE,
                        ),
                        (
                            XyzArrowLine(i),
                            cadrs_ui::icon::icon_in(
                                "manipulator-arrow-line",
                                XYZ_ARROW_LEN,
                                axis_color(i, false),
                                Node { position_type: PositionType::Absolute, ..default() },
                            ),
                            Pickable::IGNORE,
                        ),
                    ],
                ))
                .id();
            commands.entity(area).add_child(e);
        }
        return;
    }
    for (_, a, mut node, mut transform, mut vis) in &mut q {
        let Some((base, tip)) = arrows.base_tip[a.0] else {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        };
        let u = (tip - base).normalize_or_zero();
        let center = (base + tip) / 2.0 - rect.0.min;
        let (l, t) = (Val::Px(center.x - XYZ_ARROW_LEN / 2.0), Val::Px(center.y - XYZ_ARROW_LEN / 2.0));
        if node.left != l || node.top != t {
            node.left = l;
            node.top = t;
        }
        let want = bevy::ui::UiTransform { rotation: Rot2::radians(u.x.atan2(-u.y)), ..default() };
        if *transform != want {
            *transform = want;
        }
        vis.set_if_neq(Visibility::Inherited);
    }
    for (a, mut img) in &mut q_line {
        let c = axis_color(a.0, arrows.hovered == Some(a.0));
        if img.color != c {
            img.color = c;
        }
    }
}
