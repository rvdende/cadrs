//! The Frame feature's dialog (`cadrs_core::frame`, issue #266), in the applied-feature dialogs'
//! chain between [`crate::thread_ui`] and [`crate::gear_ui`]:
//!
//! - *Sketch curves or edges*: the members' path. Picking a sketch's row in the feature list
//!   takes all of its curves.
//! - **Profile**: the section's family ([`ProfileFamily`]) and **Size**; Custom takes a *Profile
//!   sketch* (picked by one of its regions or its row), its origin on the path.
//! - **Alignment** (which point of the section's box sits on the path), *Angle* about the path,
//!   **Mirror profile**, **Merge tangent segments**.
//! - **Corners** Miter, Butt, Coped butt or None, with a flip (which member runs on).
//! - *Corner overrides*: sketch points or part vertices at corners joined otherwise; each picked
//!   point gets a row of its own with its treatment and flip.
//! - *Trim to parts*: parts the members are cut back by (a wall, another frame's members).
//! - *Trim to planes*: default planes, Plane features or planar faces the members are cut at.
//! - **Keep the other side** of the trims (all but the largest piece).
//! - **End caps** with their *Thickness* (plates closing hollow members' free ends; **Set into
//!   the end**: inside it, the member stopping short by their thickness) and
//!   **Gussets** with their *Size*, *Thickness*, *Chamfer* and *Position* (middle, top or
//!   bottom of the section): triangular plates in the corners (at a corner of three or more,
//!   in the pair joined; at a tee, either side). An override's row has a gusset checkbox of its
//!   own, its flip (which member runs on) and, at a corner of three or more, a button that
//!   joins the next pair.
//!
//! The toolbar's Frame button starts it with the selected sketch curves, edges and sketches.
//! The cut list of every frame in the Part Studio is the Cut list panel ([`crate::cut_list`]).

use bevy::prelude::*;
use cadrs_core::advanced::PathRef;
use cadrs_core::frame::{
    Anchor, CornerOverride, CornerType, FrameFeature, GussetPosition, ProfileFamily, SketchPointRef,
};
use cadrs_core::{Feature, FeatureKind};
use cadrs_ui::i18n::{tr, tr_with};
use cadrs_ui::prelude::*;
use cadrs_ui::{OptionRow, Select};

use crate::applied::{AppliedField, AppliedKind};
use crate::applied_dialog::{Role, body_column, index_of, list, number, opts, select_row};
use crate::parts::PartCache;
use crate::viewport::Pick;

fn is_sketch(features: &[Feature], f: cadrs_core::FeatureId) -> bool {
    features
        .iter()
        .any(|x| x.id == f && matches!(x.kind, FeatureKind::Sketch(_)))
}

fn is_curve_feature(features: &[Feature], f: cadrs_core::FeatureId) -> bool {
    features.iter().any(|x| {
        x.id == f && matches!(x.kind, FeatureKind::Helix(_) | FeatureKind::ProjectCurve(_))
    })
}

fn features_of(world: &World) -> Vec<Feature> {
    world
        .get_resource::<crate::ActiveDocument>()
        .and_then(|d| d.active_element())
        .map(|e| e.features().to_vec())
        .unwrap_or_default()
}

/// A pick as a member's path: a part edge, a sketch curve, a whole sketch or a curve feature.
fn path_of(features: &[Feature], cache: &PartCache, pick: Pick) -> Option<PathRef> {
    Some(match pick {
        Pick::Edge(..) => match crate::applied::entity_of(cache, pick) {
            Some(cadrs_core::applied::EdgeOrFace::Edge(e)) => PathRef::Edge(e),
            _ => return None,
        },
        Pick::SketchCurve(sketch, curve) => PathRef::SketchCurve { sketch, curve },
        Pick::Feature(f) if is_sketch(features, f) => PathRef::Sketch(f),
        Pick::Feature(f) if is_curve_feature(features, f) => PathRef::Curve(f),
        _ => return None,
    })
}

/// The same trim plane: a face by its name, a Plane feature by its feature (their frames as of
/// the last rebuild aside).
fn same_plane(a: &cadrs_sketch::PlaneRef, b: &cadrs_sketch::PlaneRef) -> bool {
    use cadrs_sketch::PlaneRef as R;
    match (a, b) {
        (R::Face(a), R::Face(b)) => a.feature == b.feature && a.face == b.face,
        (R::Feature(a), R::Feature(b)) => a.feature == b.feature,
        (a, b) => a == b,
    }
}

fn same_path(a: &PathRef, b: &PathRef) -> bool {
    match (a, b) {
        (PathRef::Edge(a), PathRef::Edge(b)) => a.edge == b.edge,
        (a, b) => a == b,
    }
}

/// Where a sketch point is in the model.
fn point_at(
    features: &[Feature],
    sketch: cadrs_core::FeatureId,
    point: cadrs_sketch::PointId,
) -> Option<[f64; 3]> {
    let sk = features.iter().find(|f| f.id == sketch)?.sketch()?;
    let p = sk.geometry.points.get(point)?;
    Some(sk.plane?.frame().to_world(p.pos))
}

/// A new feature's base name, parameters and first field, from the selection.
pub fn initial(
    world: &World,
    kind: AppliedKind,
    picked: &[Pick],
) -> Option<(&'static str, FeatureKind, AppliedField)> {
    if kind != AppliedKind::Frame {
        return None;
    }
    let features = features_of(world);
    let cache = world.resource::<PartCache>();
    let mut x = FrameFeature::default();
    for p in picked {
        if let Some(r) = path_of(&features, cache, *p)
            && !x.path.iter().any(|q| same_path(q, &r))
        {
            x.path.push(r);
        }
    }
    Some(("Frame", FeatureKind::Frame(x), AppliedField::FramePath))
}

/// A pick into the path, the custom profile's sketch or the corner overrides. Returns false if
/// it doesn't fit.
pub fn pick(world: &mut World, kind: &mut FeatureKind, field: AppliedField, pick: Pick) -> bool {
    let FeatureKind::Frame(x) = kind else {
        return false;
    };
    let features = features_of(world);
    match field {
        AppliedField::FramePath => {
            let cache = world.resource::<PartCache>();
            let Some(r) = path_of(&features, cache, pick) else {
                return false;
            };
            match x.path.iter().position(|q| same_path(q, &r)) {
                Some(i) => {
                    x.path.remove(i);
                }
                None => x.path.push(r),
            }
            true
        }
        AppliedField::FrameSketch => {
            let s = match pick {
                Pick::Region(s, _) => s,
                Pick::Feature(s) if is_sketch(&features, s) => s,
                _ => return false,
            };
            x.sketch = if x.sketch == Some(s) { None } else { Some(s) };
            true
        }
        AppliedField::FrameMembers => {
            let cache = world.resource::<PartCache>();
            let r = match pick {
                Pick::SketchCurve(sketch, curve) => PathRef::SketchCurve { sketch, curve },
                Pick::Edge(..) => match crate::applied::entity_of(cache, pick) {
                    Some(cadrs_core::applied::EdgeOrFace::Edge(e)) => PathRef::Edge(e),
                    _ => return false,
                },
                _ => return false,
            };
            match x
                .member_overrides
                .iter()
                .position(|o| same_path(&o.curve, &r))
            {
                Some(i) => {
                    x.member_overrides.remove(i);
                }
                None => x.member_overrides.push(cadrs_core::frame::MemberOverride {
                    curve: r,
                    angle: 90.0,
                    angle_expr: "90 deg".into(),
                }),
            }
            true
        }
        AppliedField::FrameTrim => {
            let Some(p) = pick.part() else {
                return false;
            };
            match x.trim.iter().position(|q| *q == p) {
                Some(i) => {
                    x.trim.remove(i);
                }
                None => x.trim.push(p),
            }
            true
        }
        AppliedField::FrameTrimPlanes => {
            let plane = match pick {
                Pick::Plane(k) => Some(k.plane_ref()),
                Pick::Feature(f) => cadrs_core::parts::plane_feature_ref(&features, f),
                Pick::Face(part, face) => {
                    world.get_resource::<crate::ActiveDocument>().and_then(|d| {
                        crate::parts::face_plane_of(d.active_element()?, part.feature, face)
                    })
                }
                _ => None,
            };
            let Some(plane) = plane else {
                return false;
            };
            match x.trim_planes.iter().position(|q| same_plane(q, &plane)) {
                Some(i) => {
                    x.trim_planes.remove(i);
                }
                None => x.trim_planes.push(plane),
            }
            true
        }
        AppliedField::FrameCorners => {
            let (at, point) = match pick {
                Pick::SketchPoint(sketch, point) => {
                    let Some(at) = point_at(&features, sketch, point) else {
                        return false;
                    };
                    (at, Some(SketchPointRef { sketch, point }))
                }
                Pick::Vertex(part, v) => {
                    let cache = world.resource::<PartCache>();
                    let Some(at) = cache
                        .parts
                        .iter()
                        .find(|p| p.id == part)
                        .and_then(|p| p.solid.vertex(&v))
                        .map(|q| q.point)
                    else {
                        return false;
                    };
                    (at, None)
                }
                _ => return false,
            };
            let same = |o: &CornerOverride| {
                (point.is_some() && o.point == point)
                    || (0..3)
                        .map(|k| (o.at[k] - at[k]).powi(2))
                        .sum::<f64>()
                        .sqrt()
                        < 1e-6
            };
            match x.overrides.iter().position(same) {
                Some(i) => {
                    x.overrides.remove(i);
                }
                None => x.overrides.push(CornerOverride {
                    point,
                    at,
                    // The other treatment than the frame's (else why override).
                    corner: if x.corner == CornerType::Butt {
                        CornerType::Miter
                    } else {
                        CornerType::Butt
                    },
                    flip: false,
                    turn: 0,
                    gusset: None,
                }),
            }
            true
        }
        _ => false,
    }
}

/// The path, the profile sketch and the overridden corners, shown selected in the view while
/// the dialog is open.
pub fn references(kind: &FeatureKind, cache: &PartCache) -> Vec<Pick> {
    let FeatureKind::Frame(x) = kind else {
        return Vec::new();
    };
    let edge = |e: &cadrs_core::document::EdgeRef| {
        cache
            .parts
            .iter()
            .find(|p| p.solid.edge(&e.edge).is_some())
            .map(|p| Pick::Edge(p.id, e.edge))
    };
    let mut out: Vec<Pick> = x
        .path
        .iter()
        .filter_map(|p| match p {
            PathRef::Edge(e) => edge(e),
            PathRef::SketchCurve { sketch, curve } => Some(Pick::SketchCurve(*sketch, *curve)),
            PathRef::Sketch(s) | PathRef::Curve(s) => Some(Pick::Feature(*s)),
        })
        .collect();
    out.extend(x.sketch.map(Pick::Feature));
    out.extend(
        x.overrides
            .iter()
            .filter_map(|o| o.point.map(|p| Pick::SketchPoint(p.sketch, p.point))),
    );
    out.extend(x.trim.iter().map(|p| Pick::Part(*p)));
    out.extend(x.trim_planes.iter().filter_map(|p| {
        match p {
            cadrs_sketch::PlaneRef::Face(f) => cache
                .parts
                .iter()
                .find(|q| q.id.feature.0 == f.feature && q.solid.face(&f.face).is_some())
                .or_else(|| cache.parts.iter().find(|q| q.solid.face(&f.face).is_some()))
                .map(|q| Pick::Face(q.id, f.face)),
            p => crate::viewport::plane_pick(*p),
        }
    }));
    out.extend(x.member_overrides.iter().filter_map(|o| match &o.curve {
        PathRef::Edge(e) => edge(e),
        PathRef::SketchCurve { sketch, curve } => Some(Pick::SketchCurve(*sketch, *curve)),
        _ => None,
    }));
    out
}

// ---------------------------------------------------------------------------------------------
// The dialog

pub(crate) fn name(kind: &FeatureKind) -> Option<&'static str> {
    match kind {
        FeatureKind::Frame(_) => Some("frame"),
        k => crate::gear_ui::name(k),
    }
}

pub(crate) fn layout(kind: &FeatureKind) -> Option<String> {
    match kind {
        FeatureKind::Frame(x) => Some(
            format!(
                "frame {:?} {} {:?} {} {} {:?} {} {:?} {} {}",
                x.family,
                x.size,
                x.anchor,
                x.mirror,
                x.merge_tangent,
                x.corner,
                x.flip,
                x.overrides
                    .iter()
                    .map(|o| (o.corner, o.flip))
                    .collect::<Vec<_>>(),
                x.end_caps,
                x.gussets,
            ) + &format!(
                " {} {:?} {:?}",
                x.trim.is_empty() && x.trim_planes.is_empty(),
                x.trim_flip,
                x.overrides.iter().map(|o| o.gusset).collect::<Vec<_>>()
            ) + &format!(" {} {}", x.member_overrides.len(), x.cap_inward),
        ),
        k => crate::gear_ui::layout(k),
    }
}

fn name_of(features: &[Feature], id: cadrs_core::FeatureId) -> String {
    features
        .iter()
        .find(|f| f.id == id)
        .map_or_else(|| tr("model-unknown-part"), |f| f.name.clone())
}

/// A sketch line's, arc's or circle's length ("400", "78.5"), to tell the path's curves apart.
fn curve_length(
    features: &[Feature],
    sketch: cadrs_core::FeatureId,
    curve: cadrs_sketch::CurveId,
) -> Option<String> {
    use cadrs_sketch::CurveKind;
    let g = &features.iter().find(|f| f.id == sketch)?.sketch()?.geometry;
    let at = |p| g.points.get(p).map(|q| q.pos);
    let l = match g.curves.get(curve)?.kind {
        CurveKind::Line { a, b } => {
            let (a, b) = (at(a)?, at(b)?);
            ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt()
        }
        CurveKind::Circle { radius, .. } => std::f64::consts::TAU * radius,
        CurveKind::Arc { center, start, end } => {
            let (c, a, b) = (at(center)?, at(start)?, at(end)?);
            let r = ((a.x - c.x).powi(2) + (a.y - c.y).powi(2)).sqrt();
            let sweep = ((b.y - c.y).atan2(b.x - c.x) - (a.y - c.y).atan2(a.x - c.x))
                .rem_euclid(std::f64::consts::TAU);
            r * sweep
        }
        _ => return None,
    };
    let s = format!("{l:.1}");
    Some(s.trim_end_matches('0').trim_end_matches('.').to_string())
}

/// A path entry's label: "Curve of Sketch 1 · 400 mm", "Edge of Extrude 1", "Sketch 2".
fn path_label(features: &[Feature], p: &PathRef) -> String {
    match p {
        PathRef::Edge(e) => tr_with(
            "common-edge-of",
            &[(
                "name",
                &op_name(features, crate::parts::edge_maker(features, &e.edge)),
            )],
        ),
        PathRef::SketchCurve { sketch, curve } => {
            let name = tr_with("common-curve-of", &[("name", &name_of(features, *sketch))]);
            match curve_length(features, *sketch, *curve) {
                Some(l) => format!("{l} mm · {name}"),
                None => name,
            }
        }
        PathRef::Sketch(s) | PathRef::Curve(s) => name_of(features, *s),
    }
}

fn op_name(features: &[Feature], op: cadrs_sketch::OpId) -> String {
    name_of(features, cadrs_core::FeatureId(op))
}

pub(crate) fn items(
    features: &[Feature],
    cache: &PartCache,
    kind: &FeatureKind,
    role: Role,
) -> Option<Vec<String>> {
    match (kind, role) {
        (FeatureKind::Frame(x), Role::FramePath) => {
            Some(x.path.iter().map(|p| path_label(features, p)).collect())
        }
        (FeatureKind::Frame(x), Role::FrameMembers) => Some(
            x.member_overrides
                .iter()
                .map(|o| path_label(features, &o.curve))
                .collect(),
        ),
        (FeatureKind::Frame(x), Role::FrameSketch) => {
            Some(x.sketch.iter().map(|s| name_of(features, *s)).collect())
        }
        (FeatureKind::Frame(x), Role::FrameCorners) => Some(
            x.overrides
                .iter()
                .enumerate()
                .map(|(i, o)| {
                    let n = |v: f64| {
                        let s = format!("{v:.1}");
                        s.trim_end_matches('0').trim_end_matches('.').to_string()
                    };
                    format!(
                        "{} ({}, {}, {})",
                        tr_with("model-frame-corner-n", &[("n", &(i + 1).to_string())]),
                        n(o.at[0]),
                        n(o.at[1]),
                        n(o.at[2])
                    )
                })
                .collect(),
        ),
        (FeatureKind::Frame(x), Role::FrameTrimPlanes) => Some(
            x.trim_planes
                .iter()
                .map(|p| match p {
                    cadrs_sketch::PlaneRef::Face(f) => tr_with(
                        "common-face-of",
                        &[("name", &name_of(features, cadrs_core::FeatureId(f.feature)))],
                    ),
                    p => crate::viewport::plane_label(features, *p),
                })
                .collect(),
        ),
        (FeatureKind::Frame(x), Role::FrameTrim) => Some(
            x.trim
                .iter()
                .map(|p| {
                    cache
                        .parts
                        .iter()
                        .find(|q| q.id == *p)
                        .map(|q| cadrs_core::parts::display_name(q, &cache.props).to_string())
                        .unwrap_or_else(|| tr("model-unknown-part"))
                })
                .collect(),
        ),
        (k, r) => crate::gear_ui::items(features, cache, k, r),
    }
}

pub(crate) fn list_field(role: Role) -> Option<AppliedField> {
    match role {
        Role::FramePath => Some(AppliedField::FramePath),
        Role::FrameSketch => Some(AppliedField::FrameSketch),
        Role::FrameCorners => Some(AppliedField::FrameCorners),
        Role::FrameTrim => Some(AppliedField::FrameTrim),
        Role::FrameTrimPlanes => Some(AppliedField::FrameTrimPlanes),
        Role::FrameMembers => Some(AppliedField::FrameMembers),
        r => crate::gear_ui::list_field(r),
    }
}

/// A family's name in the current language.
pub(crate) fn family_text(f: ProfileFamily) -> String {
    tr(match f {
        ProfileFamily::TSlot => "model-frame-family-tslot",
        ProfileFamily::SquareTube => "model-frame-family-square-tube",
        ProfileFamily::RectTube => "model-frame-family-rect-tube",
        ProfileFamily::RoundTube => "model-frame-family-round-tube",
        ProfileFamily::SquareBar => "model-frame-family-square-bar",
        ProfileFamily::RoundBar => "model-frame-family-round-bar",
        ProfileFamily::FlatBar => "model-frame-family-flat-bar",
        ProfileFamily::Angle => "model-frame-family-angle",
        ProfileFamily::Channel => "model-frame-family-channel",
        ProfileFamily::IBeam => "model-frame-family-ibeam",
        ProfileFamily::Custom => "model-frame-family-custom",
    })
}

fn anchor_text(a: Anchor) -> String {
    tr(match a {
        Anchor::TopLeft => "model-frame-anchor-top-left",
        Anchor::Top => "model-frame-anchor-top",
        Anchor::TopRight => "model-frame-anchor-top-right",
        Anchor::Left => "model-frame-anchor-left",
        Anchor::Center => "model-frame-anchor-center",
        Anchor::Right => "model-frame-anchor-right",
        Anchor::BottomLeft => "model-frame-anchor-bottom-left",
        Anchor::Bottom => "model-frame-anchor-bottom",
        Anchor::BottomRight => "model-frame-anchor-bottom-right",
        Anchor::Origin => "model-frame-anchor-origin",
    })
}

fn position_text(p: GussetPosition) -> String {
    tr(match p {
        GussetPosition::Center => "model-frame-gusset-center",
        GussetPosition::Top => "model-frame-gusset-top",
        GussetPosition::Bottom => "model-frame-gusset-bottom",
    })
}

fn corner_text(c: CornerType) -> String {
    tr(match c {
        CornerType::Miter => "model-frame-corner-miter",
        CornerType::Butt => "model-frame-corner-butt",
        CornerType::CopedButt => "model-frame-corner-coped",
        CornerType::None => "common-none",
    })
}

/// The corner types, with a flip for those where one member runs on.
#[allow(clippy::too_many_arguments)]
fn corner_row(
    b: &mut ChildSpawner,
    t: &Theme,
    name: &str,
    label: &str,
    role: Role,
    corner: CornerType,
    flip: Option<(Role, bool)>,
    turn: Option<Role>,
) {
    let options = opts(&CornerType::ALL, corner_text, |_| true);
    let mut select = Select::new(name.to_string());
    for (o, enabled) in &options {
        select = select.option(o.clone(), *enabled);
    }
    b.spawn((
        Name::new(format!("{name}-row")),
        Node {
            height: Val::Px(28.0),
            margin: UiRect::top(Val::Px(2.0)),
            align_items: AlignItems::Center,
            column_gap: Val::Px(4.0),
            ..default()
        },
    ))
    .with_children(|r| {
        r.spawn((
            Name::new(format!("{name}-label")),
            t.text(
                label,
                t.font_sm,
                bevy::text::FontWeight::NORMAL,
                t.muted_foreground,
            ),
            Node {
                width: Val::Px(80.0),
                flex_shrink: 0.0,
                ..default()
            },
        ));
        r.spawn((
            role,
            select
                .selected(index_of(&CornerType::ALL, &corner))
                .build(t),
        ))
        .entry::<Node>()
        .and_modify(|mut n| n.flex_grow = 1.0);
        if let Some((flip_role, on)) = flip
            && matches!(corner, CornerType::Butt | CornerType::CopedButt)
        {
            crate::extrude_dialog::flip_button_any(
                r,
                t,
                &format!("{name}-flip"),
                flip_role,
                on,
                &tr("model-frame-flip-corner"),
            );
        }
        // At a corner of three or more: the next pair is the one joined.
        if let Some(role) = turn {
            r.spawn((
                role,
                IconButton::new(format!("{name}-turn"), "repeat")
                    .icon_size(16.0)
                    .tooltip(tr("model-frame-next-pair"))
                    .build(t),
            ));
        }
    });
}

pub(crate) fn body(
    b: &mut ChildSpawner,
    t: &Theme,
    kind: &FeatureKind,
    field: AppliedField,
    items_of: &dyn Fn(Role) -> Vec<String>,
) {
    let FeatureKind::Frame(x) = kind else {
        crate::gear_ui::body(b, t, kind, field, items_of);
        return;
    };
    body_column(b, |b| {
        list(
            b,
            t,
            "frame-path-field",
            &tr("model-frame-path"),
            Role::FramePath,
            items_of(Role::FramePath),
            field == AppliedField::FramePath,
        );
        let families = opts(&ProfileFamily::ALL, family_text, |_| true);
        select_row(
            b,
            t,
            "frame-family",
            &tr("model-frame-profile"),
            Role::FrameFamily,
            &families,
            index_of(&ProfileFamily::ALL, &x.family),
            None,
        );
        if x.family == ProfileFamily::Custom {
            list(
                b,
                t,
                "frame-sketch-field",
                &tr("model-frame-profile-sketch"),
                Role::FrameSketch,
                items_of(Role::FrameSketch),
                field == AppliedField::FrameSketch,
            );
        } else {
            let sizes: Vec<(String, bool)> = cadrs_core::frame::size_names(x.family)
                .into_iter()
                .map(|s| (s.to_string(), true))
                .collect();
            select_row(
                b,
                t,
                "frame-size",
                &tr("common-size"),
                Role::FrameSize,
                &sizes,
                size_index(x),
                None,
            );
        }
        let anchors = opts(&Anchor::ALL, anchor_text, |_| true);
        select_row(
            b,
            t,
            "frame-anchor",
            &tr("model-frame-alignment"),
            Role::FrameAnchor,
            &anchors,
            index_of(&Anchor::ALL, &x.anchor),
            None,
        );
        number(
            b,
            t,
            "frame-angle",
            &tr("common-angle"),
            Role::FrameAngle,
            &x.angle_expr,
            None,
        );
        b.spawn(
            OptionRow::new("frame-mirror", tr("model-frame-mirror"))
                .checked(x.mirror)
                .build(t),
        );
        b.spawn(
            OptionRow::new("frame-merge", tr("model-frame-merge-tangent"))
                .checked(x.merge_tangent)
                .build(t),
        );
        number(
            b,
            t,
            "frame-end-offset",
            &tr("model-frame-end-offset"),
            Role::FrameEndOffset,
            &x.end_offset_expr,
            None,
        );
        list(
            b,
            t,
            "frame-members-field",
            &tr("model-frame-members"),
            Role::FrameMembers,
            items_of(Role::FrameMembers),
            field == AppliedField::FrameMembers,
        );
        for (i, o) in x.member_overrides.iter().enumerate() {
            let i = i.min(u8::MAX as usize) as u8;
            number(
                b,
                t,
                &format!("frame-member-{i}-angle"),
                &tr_with("model-frame-member-angle", &[("n", &(i + 1).to_string())]),
                Role::FrameMemberAngle(i),
                &o.angle_expr,
                None,
            );
        }
        corner_row(
            b,
            t,
            "frame-corner",
            &tr("model-frame-corners"),
            Role::FrameCorner,
            x.corner,
            Some((Role::Flip, x.flip)),
            None,
        );
        list(
            b,
            t,
            "frame-corners-field",
            &tr("model-frame-overrides"),
            Role::FrameCorners,
            items_of(Role::FrameCorners),
            field == AppliedField::FrameCorners,
        );
        // Each override's treatment, its flip (which member runs on), where three or more meet
        // the next pair, and its gusset; named as its list item, with where it is.
        let corners = items_of(Role::FrameCorners);
        for (i, o) in x.overrides.iter().enumerate() {
            let label = corners
                .get(i)
                .cloned()
                .unwrap_or_else(|| tr_with("model-frame-corner-n", &[("n", &(i + 1).to_string())]));
            let i = i.min(u8::MAX as usize) as u8;
            corner_row(
                b,
                t,
                &format!("frame-override-{i}"),
                &label,
                Role::FrameOverride(i),
                o.corner,
                Some((Role::FrameOverrideFlip(i), o.flip)),
                (!items_of(Role::FrameOverrideTurn(i)).is_empty())
                    .then_some(Role::FrameOverrideTurn(i)),
            );
            b.spawn(
                OptionRow::new(
                    format!("frame-override-{i}-gusset"),
                    tr("model-frame-override-gusset"),
                )
                .checked(o.gusset.unwrap_or(x.gussets))
                .build(t),
            );
        }
        list(
            b,
            t,
            "frame-trim-field",
            &tr("model-frame-trim"),
            Role::FrameTrim,
            items_of(Role::FrameTrim),
            field == AppliedField::FrameTrim,
        );
        list(
            b,
            t,
            "frame-trim-planes-field",
            &tr("model-frame-trim-planes"),
            Role::FrameTrimPlanes,
            items_of(Role::FrameTrimPlanes),
            field == AppliedField::FrameTrimPlanes,
        );
        if !x.trim.is_empty() || !x.trim_planes.is_empty() {
            b.spawn(
                OptionRow::new("frame-trim-flip", tr("model-frame-trim-flip"))
                    .checked(x.trim_flip)
                    .build(t),
            );
        }
        b.spawn(
            OptionRow::new("frame-caps", tr("model-frame-end-caps"))
                .checked(x.end_caps)
                .build(t),
        );
        if x.end_caps {
            number(
                b,
                t,
                "frame-cap-thickness",
                &tr("model-frame-thickness"),
                Role::FrameCapThickness,
                &x.cap_thickness_expr,
                None,
            );
            b.spawn(
                OptionRow::new("frame-cap-inward", tr("model-frame-cap-inward"))
                    .checked(x.cap_inward)
                    .build(t),
            );
        }
        b.spawn(
            OptionRow::new("frame-gussets", tr("model-frame-gussets"))
                .checked(x.gussets)
                .build(t),
        );
        if x.gussets || x.overrides.iter().any(|o| o.gusset == Some(true)) {
            number(
                b,
                t,
                "frame-gusset-size",
                &tr("model-frame-gusset-size"),
                Role::FrameGussetSize,
                &x.gusset_size_expr,
                None,
            );
            number(
                b,
                t,
                "frame-gusset-thickness",
                &tr("model-frame-thickness"),
                Role::FrameGussetThickness,
                &x.gusset_thickness_expr,
                None,
            );
            number(
                b,
                t,
                "frame-gusset-chamfer",
                &tr("model-frame-gusset-chamfer"),
                Role::FrameGussetChamfer,
                &x.gusset_chamfer_expr,
                None,
            );
            let positions = opts(&GussetPosition::ALL, position_text, |_| true);
            select_row(
                b,
                t,
                "frame-gusset-position",
                &tr("model-frame-gusset-position"),
                Role::FrameGussetPosition,
                &positions,
                index_of(&GussetPosition::ALL, &x.gusset_position),
                None,
            );
        }
    });
}

fn size_index(x: &FrameFeature) -> usize {
    cadrs_core::frame::size_names(x.family)
        .iter()
        .position(|s| *s == x.size)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------------------------
// Keeping it in step and input

pub(crate) fn number_text(kind: &FeatureKind, role: Role) -> Option<String> {
    match (kind, role) {
        (FeatureKind::Frame(x), Role::FrameAngle) => Some(x.angle_expr.clone()),
        (FeatureKind::Frame(x), Role::FrameCapThickness) => Some(x.cap_thickness_expr.clone()),
        (FeatureKind::Frame(x), Role::FrameGussetSize) => Some(x.gusset_size_expr.clone()),
        (FeatureKind::Frame(x), Role::FrameGussetThickness) => {
            Some(x.gusset_thickness_expr.clone())
        }
        (FeatureKind::Frame(x), Role::FrameGussetChamfer) => Some(x.gusset_chamfer_expr.clone()),
        (FeatureKind::Frame(x), Role::FrameEndOffset) => Some(x.end_offset_expr.clone()),
        (FeatureKind::Frame(x), Role::FrameMemberAngle(i)) => {
            Some(x.member_overrides.get(i as usize)?.angle_expr.clone())
        }
        (k, r) => crate::gear_ui::number_text(k, r),
    }
}

pub(crate) fn select_index(kind: &FeatureKind, role: Role) -> Option<usize> {
    let FeatureKind::Frame(x) = kind else {
        return crate::gear_ui::select_index(kind, role);
    };
    Some(match role {
        Role::FrameFamily => index_of(&ProfileFamily::ALL, &x.family),
        Role::FrameSize => size_index(x),
        Role::FrameAnchor => index_of(&Anchor::ALL, &x.anchor),
        Role::FrameCorner => index_of(&CornerType::ALL, &x.corner),
        Role::FrameOverride(i) => index_of(&CornerType::ALL, &x.overrides.get(i as usize)?.corner),
        Role::FrameGussetPosition => index_of(&GussetPosition::ALL, &x.gusset_position),
        _ => return None,
    })
}

pub(crate) fn select(k: &mut FeatureKind, role: Role, i: usize) {
    let FeatureKind::Frame(x) = k else {
        crate::gear_ui::select(k, role, i);
        return;
    };
    let corner = CornerType::ALL[i.min(CornerType::ALL.len() - 1)];
    match role {
        Role::FrameFamily => x.set_family(ProfileFamily::ALL[i.min(ProfileFamily::ALL.len() - 1)]),
        Role::FrameSize => {
            if let Some(s) = cadrs_core::frame::size_names(x.family).get(i) {
                x.size = s.to_string();
            }
        }
        Role::FrameAnchor => x.anchor = Anchor::ALL[i.min(Anchor::ALL.len() - 1)],
        Role::FrameCorner => x.corner = corner,
        Role::FrameOverride(j) => {
            if let Some(o) = x.overrides.get_mut(j as usize) {
                o.corner = corner;
            }
        }
        Role::FrameGussetPosition => {
            x.gusset_position = GussetPosition::ALL[i.min(GussetPosition::ALL.len() - 1)]
        }
        _ => {}
    }
}

/// Picking Custom moves the picks to the profile sketch's field.
pub(crate) fn field_after_select(role: Role, i: usize) -> Option<AppliedField> {
    (role == Role::FrameFamily && ProfileFamily::ALL.get(i) == Some(&ProfileFamily::Custom))
        .then_some(AppliedField::FrameSketch)
}

pub(crate) fn tab(k: &mut FeatureKind, role: Role, i: usize) {
    crate::gear_ui::tab(k, role, i)
}

pub(crate) fn is_checkbox(name: &str) -> bool {
    name == "frame-mirror-checkbox"
        || name == "frame-merge-checkbox"
        || name == "frame-caps-checkbox"
        || name == "frame-gussets-checkbox"
        || name == "frame-trim-flip-checkbox"
        || name == "frame-cap-inward-checkbox"
        || override_gusset(name).is_some()
        || crate::gear_ui::is_checkbox(name)
}

pub(crate) fn checkbox(k: &mut FeatureKind, name: &str, on: bool) -> Option<&'static str> {
    match (k, name) {
        (FeatureKind::Frame(x), "frame-mirror-checkbox") => {
            x.mirror = on;
            Some("Mirror profile")
        }
        (FeatureKind::Frame(x), "frame-merge-checkbox") => {
            x.merge_tangent = on;
            Some("Merge tangent segments")
        }
        (FeatureKind::Frame(x), "frame-caps-checkbox") => {
            x.end_caps = on;
            Some("End caps")
        }
        (FeatureKind::Frame(x), "frame-gussets-checkbox") => {
            x.gussets = on;
            Some("Gussets")
        }
        (FeatureKind::Frame(x), "frame-cap-inward-checkbox") => {
            x.cap_inward = on;
            Some("Set caps into the ends")
        }
        (FeatureKind::Frame(x), "frame-trim-flip-checkbox") => {
            x.trim_flip = on;
            Some("Keep the other side")
        }
        (FeatureKind::Frame(x), n) if override_gusset(n).is_some() => {
            let o = x.overrides.get_mut(override_gusset(n)?)?;
            // Like the frame's: back to following it.
            o.gusset = (on != x.gussets).then_some(on);
            Some("Corner gusset")
        }
        (k, n) => crate::gear_ui::checkbox(k, n, on),
    }
}

/// An override's gusset checkbox ("frame-override-2-gusset-checkbox"): its override.
fn override_gusset(name: &str) -> Option<usize> {
    name.strip_prefix("frame-override-")?
        .strip_suffix("-gusset-checkbox")?
        .parse()
        .ok()
}

pub(crate) fn is_signed(role: Role) -> bool {
    matches!(
        role,
        Role::FrameAngle | Role::FrameEndOffset | Role::FrameMemberAngle(_)
    ) || crate::gear_ui::is_signed(role)
}

pub(crate) fn is_plain(role: Role) -> bool {
    crate::gear_ui::is_plain(role)
}

pub(crate) fn is_angle(role: Role) -> bool {
    matches!(role, Role::FrameAngle | Role::FrameMemberAngle(_)) || crate::gear_ui::is_angle(role)
}

pub(crate) fn number_label(role: Role) -> Option<&'static str> {
    match role {
        Role::FrameAngle => Some("Angle"),
        Role::FrameCapThickness => Some("Cap thickness"),
        Role::FrameGussetSize => Some("Gusset size"),
        Role::FrameGussetThickness => Some("Gusset thickness"),
        Role::FrameGussetChamfer => Some("Gusset chamfer"),
        Role::FrameEndOffset => Some("End offset"),
        Role::FrameMemberAngle(_) => Some("Member angle"),
        r => crate::gear_ui::number_label(r),
    }
}

pub(crate) fn set_number(k: &mut FeatureKind, role: Role, v: f64, expr: String) {
    match (k, role) {
        (FeatureKind::Frame(x), Role::FrameAngle) => {
            x.angle = v;
            x.angle_expr = expr;
        }
        (FeatureKind::Frame(x), Role::FrameCapThickness) => {
            x.cap_thickness = v;
            x.cap_thickness_expr = expr;
        }
        (FeatureKind::Frame(x), Role::FrameGussetSize) => {
            x.gusset_size = v;
            x.gusset_size_expr = expr;
        }
        (FeatureKind::Frame(x), Role::FrameGussetThickness) => {
            x.gusset_thickness = v;
            x.gusset_thickness_expr = expr;
        }
        (FeatureKind::Frame(x), Role::FrameGussetChamfer) => {
            x.gusset_chamfer = v;
            x.gusset_chamfer_expr = expr;
        }
        (FeatureKind::Frame(x), Role::FrameEndOffset) => {
            x.end_offset = v;
            x.end_offset_expr = expr;
        }
        (FeatureKind::Frame(x), Role::FrameMemberAngle(i)) => {
            if let Some(o) = x.member_overrides.get_mut(i as usize) {
                o.angle = v;
                o.angle_expr = expr;
            }
        }
        (k, r) => crate::gear_ui::set_number(k, r, v, expr),
    }
}

pub(crate) fn flip(k: &mut FeatureKind, role: Role) {
    match (k, role) {
        (FeatureKind::Frame(x), Role::Flip) => x.flip = !x.flip,
        (FeatureKind::Frame(x), Role::FrameOverrideFlip(i)) => {
            if let Some(o) = x.overrides.get_mut(i as usize) {
                o.flip = !o.flip;
            }
        }
        (FeatureKind::Frame(x), Role::FrameOverrideTurn(i)) => {
            // The next pair (a corner of three members has three, of four six, … of seven 21:
            // 210 a multiple of each, so the turns go round evenly).
            if let Some(o) = x.overrides.get_mut(i as usize) {
                o.turn = (o.turn + 1) % 210;
            }
        }
        (k, r) => crate::gear_ui::flip(k, r),
    }
}

/// An item's ✕.
pub(crate) fn remove(k: &mut FeatureKind, role: Role, i: usize) {
    match (k, role) {
        (FeatureKind::Frame(x), Role::FramePath) => {
            if i < x.path.len() {
                x.path.remove(i);
            }
        }
        (FeatureKind::Frame(x), Role::FrameSketch) => x.sketch = None,
        (FeatureKind::Frame(x), Role::FrameTrim) => {
            if i < x.trim.len() {
                x.trim.remove(i);
            }
        }
        (FeatureKind::Frame(x), Role::FrameTrimPlanes) => {
            if i < x.trim_planes.len() {
                x.trim_planes.remove(i);
            }
        }
        (FeatureKind::Frame(x), Role::FrameMembers) => {
            if i < x.member_overrides.len() {
                x.member_overrides.remove(i);
            }
        }
        (FeatureKind::Frame(x), Role::FrameCorners) => {
            if i < x.overrides.len() {
                x.overrides.remove(i);
            }
        }
        (k, r) => crate::gear_ui::remove(k, r, i),
    }
}
