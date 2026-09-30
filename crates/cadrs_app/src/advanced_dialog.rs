//! The dialogs of the P3.7 features, laid out as Onshape's, built and kept in step by
//! [`crate::applied_dialog`] (which calls these for the Plane, Sweep, Loft and Split kinds):
//!
//! - **Plane** (`ex4-step5.png`): *Entities*; the plane type (Offset, Plane point, Line angle,
//!   Point normal, Three point, Mid plane, Curve point, Fit); *Offset distance* with its flip
//!   (Offset), *Angle* with its flip and *Flip alignment* (Line angle); *Flip normal*.
//! - **Sweep** (`ex4-step15.png`): **Solid | Surface | Thin**, **New | Add | Remove |
//!   Intersect**; *Faces and sketch regions to sweep*; *Sweep path*; the profile control (None,
//!   Keep profile orientation, Lock profile direction with its *Direction* field); Thin's
//!   thicknesses; *Merge with all* and *Merge scope*.
//! - **Loft** (`ex4-step9.png`): the tabs; *Profiles* in order, with the ↑↓ reorder button and
//!   drag handles (PS20.2); **End conditions**: *Start profile condition* and *Start magnitude*,
//!   *End profile condition* and *End magnitude* (PS20.4); Thin; merge.
//! - **Split** (PS18.5): *Parts or surfaces to split*, *Entity to split with*; P3.8: the Part / Face
//!   tabs, *Keep tools*, *Trim to face boundaries* and *Keep both sides* with its flip.

use bevy::prelude::*;
use cadrs_core::advanced::{LoftCondition, LoftProfile, PathRef, ProfileControl, SplitToolRef, SplitType};
use cadrs_core::document::{BodyType, BooleanOp, DirectionRef, ThinWall};
use cadrs_core::plane::{PlaneEntity, PlaneType};
use cadrs_core::{Feature, FeatureId, FeatureKind};
use cadrs_ui::prelude::*;
use cadrs_ui::{NumberField, OptionRow, SelectionList, TabStrip};

use crate::applied::AppliedField;
use crate::applied_dialog::{Role, body_column, index_of, list, number, opts, select_row};
use crate::extrude_dialog::flip_button_any;
use crate::parts::PartCache;

/// The dialog's name ("plane" → `plane-dialog`, `plane-entities-field`, …).
pub(crate) fn name(kind: &FeatureKind) -> Option<&'static str> {
    Some(match kind {
        FeatureKind::Plane(_) => "plane",
        FeatureKind::Sweep(_) => "sweep",
        FeatureKind::Loft(_) => "loft",
        FeatureKind::Split(_) => "split",
        k => return crate::pattern_dialog::name(k),
    })
}

/// What decides the rows.
pub(crate) fn layout(kind: &FeatureKind) -> Option<String> {
    Some(match kind {
        FeatureKind::Plane(x) => format!("plane {:?} {}", x.kind, x.flip),
        FeatureKind::Sweep(x) => format!(
            "sweep {:?} {:?} {:?} {} {} {}",
            x.body, x.op, x.control, x.merge_all, x.thin.mid_plane, x.thin.flip_wall
        ),
        FeatureKind::Loft(x) => format!(
            "loft {:?} {:?} {:?} {:?} {} {} {}",
            x.body, x.op, x.start, x.end, x.merge_all, x.thin.mid_plane, x.thin.flip_wall
        ),
        FeatureKind::Split(x) => format!(
            "split {:?} {} {} {}",
            x.split_type,
            x.keep_both,
            matches!(x.tool, Some(SplitToolRef::Face(_))),
            x.flip
        ),
        k => return crate::pattern_dialog::layout(k),
    })
}

fn name_of(features: &[Feature], id: FeatureId) -> String {
    features.iter().find(|f| f.id == id).map_or("sketch".into(), |f| f.name.clone())
}

fn op_name(features: &[Feature], op: cadrs_sketch::OpId) -> String {
    features.iter().find(|f| f.id.0 == op).map_or("part".into(), |f| f.name.clone())
}

fn edge_label(features: &[Feature], e: &cadrs_sketch::EdgeName) -> String {
    format!("Edge of {}", op_name(features, crate::parts::edge_maker(features, e)))
}

/// A whole sketch as a profile: "Face of Sketch 3" or "Faces of Sketch 2".
fn sketch_label(features: &[Feature], s: FeatureId) -> String {
    crate::extrude_dialog::whole_sketch_label(features, s, BodyType::Solid)
}

/// The items of a list.
pub(crate) fn items(features: &[Feature], cache: &PartCache, kind: &FeatureKind, role: Role) -> Option<Vec<String>> {
    let parts = |ids: &[cadrs_core::PartId]| crate::applied::part_names(cache, ids);
    Some(match (kind, role) {
        (FeatureKind::Plane(x), Role::PlaneEntities) => x
            .entities
            .iter()
            .map(|e| match e {
                PlaneEntity::Plane(p) => crate::viewport::plane_label(features, *p),
                PlaneEntity::Face(f) => format!("Face of {}", op_name(features, f.face.op)),
                PlaneEntity::Edge(r) => edge_label(features, &r.edge),
                PlaneEntity::Vertex(v) => format!("Vertex of {}", op_name(features, v.vertex.faces[0].op)),
                PlaneEntity::SketchPoint { sketch, .. } => format!("Vertex of {}", name_of(features, *sketch)),
                PlaneEntity::SketchCurve { sketch, .. } => format!("Curve of {}", name_of(features, *sketch)),
                PlaneEntity::Origin => "Origin".into(),
            })
            .collect(),
        (FeatureKind::Sweep(x), Role::SweepProfile) => {
            let mut v: Vec<String> = x.regions.iter().map(|r| format!("Face of {}", name_of(features, r.sketch))).collect();
            v.extend(x.sketches.iter().map(|s| sketch_label(features, *s)));
            v.extend(x.faces.iter().map(|f| format!("Face of {}", op_name(features, f.face.op))));
            v
        }
        (FeatureKind::Sweep(x), Role::SweepPath) => x
            .path
            .iter()
            .map(|p| match p {
                PathRef::Edge(e) => edge_label(features, &e.edge),
                PathRef::SketchCurve { sketch, .. } => format!("Curve of {}", name_of(features, *sketch)),
                PathRef::Sketch(s) | PathRef::Curve(s) => name_of(features, *s),
            })
            .collect(),
        (FeatureKind::Sweep(x), Role::LockDirection) => x.lock_direction.iter().map(|d| direction_label(features, d)).collect(),
        (FeatureKind::Loft(x), Role::LoftStartDirection) => x.start_direction.iter().map(|d| direction_label(features, d)).collect(),
        (FeatureKind::Loft(x), Role::LoftEndDirection) => x.end_direction.iter().map(|d| direction_label(features, d)).collect(),
        (FeatureKind::Sweep(x), Role::MergeScope) => parts(&x.merge_scope),
        (FeatureKind::Loft(x), Role::LoftProfiles) => x
            .profiles
            .iter()
            .map(|p| match p {
                LoftProfile::Regions { sketch, regions } if regions.len() > 1 => format!("Faces of {}", name_of(features, *sketch)),
                LoftProfile::Regions { sketch, .. } => format!("Face of {}", name_of(features, *sketch)),
                LoftProfile::Sketch(s) => sketch_label(features, *s),
                LoftProfile::Face(f) => format!("Face of {}", op_name(features, f.face.op)),
                LoftProfile::SketchPoint { sketch, .. } => format!("Vertex of {}", name_of(features, *sketch)),
                LoftProfile::Vertex(v) => format!("Vertex of {}", op_name(features, v.vertex.faces[0].op)),
            })
            .collect(),
        (FeatureKind::Loft(x), Role::MergeScope) => parts(&x.merge_scope),
        (FeatureKind::Split(x), Role::SplitParts) => match x.split_type {
            SplitType::Part => parts(&x.parts),
            SplitType::Face => x.faces.iter().map(|f| format!("Face of {}", op_name(features, f.face.op))).collect(),
        },
        (FeatureKind::Split(x), Role::SplitTool) => x
            .tool
            .iter()
            .map(|t| match t {
                SplitToolRef::Plane(p) => crate::viewport::plane_label(features, *p),
                SplitToolRef::Face(f) => format!("Face of {}", op_name(features, f.face.op)),
                SplitToolRef::Sketch(s) => name_of(features, *s),
            })
            .collect(),
        (k, r) => return crate::pattern_dialog::items(features, cache, k, r),
    })
}

/// A direction's item in its field ("Edge of Extrude 1", "Top plane").
fn direction_label(features: &[Feature], d: &DirectionRef) -> String {
    match d {
        DirectionRef::Edge(r) => edge_label(features, &r.edge),
        DirectionRef::SketchLine { sketch, .. } => format!("Line of {}", name_of(features, *sketch)),
        DirectionRef::FaceNormal(f) => format!("Face of {}", op_name(features, f.face.op)),
        DirectionRef::PlaneNormal(p) => crate::viewport::plane_label(features, *p),
        DirectionRef::Connector(c) => c.label(features),
    }
}

/// The field a list stands for.
pub(crate) fn list_field(role: Role) -> Option<AppliedField> {
    Some(match role {
        Role::PlaneEntities => AppliedField::PlaneEntities,
        Role::SweepProfile => AppliedField::Profile,
        Role::SweepPath => AppliedField::Path,
        Role::LockDirection => AppliedField::LockDirection,
        Role::LoftProfiles => AppliedField::Profiles,
        Role::LoftStartDirection => AppliedField::LoftStartDirection,
        Role::LoftEndDirection => AppliedField::LoftEndDirection,
        Role::SplitParts => AppliedField::SplitParts,
        Role::SplitTool => AppliedField::SplitTool,
        r => return crate::pattern_dialog::list_field(r),
    })
}

// ---------------------------------------------------------------------------------------------
// Building

/// Solid | Surface | Thin, then New | Add | Remove | Intersect (not for surfaces).
fn body_tabs(b: &mut ChildSpawner, t: &Theme, name: &str, body: BodyType, op: BooleanOp) {
    let i = match body {
        BodyType::Solid => 0,
        BodyType::Surface => 1,
        BodyType::Thin => 2,
    };
    b.spawn((
        Role::BodyTab,
        TabStrip::new(format!("{name}-body-type")).compact().tab("Solid").tab("Surface").tab("Thin").selected(i).build(t),
    ));
    if body != BodyType::Surface {
        let mut strip = TabStrip::new(format!("{name}-operation")).compact();
        for o in BooleanOp::ALL {
            strip = strip.tab(o.label());
        }
        b.spawn((Role::OpTab, strip.selected(index_of(&BooleanOp::ALL, &op)).build(t)));
    }
}

/// Thin's rows: Thickness 1 with the Flip wall button, Mid plane, Thickness 2.
fn thin_rows(b: &mut ChildSpawner, t: &Theme, name: &str, thin: &ThinWall) {
    b.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(4.0), ..default() }).with_children(|r| {
        r.spawn((
            Role::Thickness1,
            NumberField::new(format!("{name}-thickness1"), "Thickness 1").text(thin.thickness1_expr.clone()).label_width(70.0).build(t),
        ))
        .entry::<Node>()
        .and_modify(|mut n| n.flex_grow = 1.0);
        flip_button_any(r, t, &format!("{name}-flip-wall"), Role::FlipWall, thin.flip_wall, "Flip wall");
    });
    b.spawn(OptionRow::new(format!("{name}-mid-plane"), "Mid plane").checked(thin.mid_plane).build(t));
    if !thin.mid_plane {
        b.spawn((
            Role::Thickness2,
            NumberField::new(format!("{name}-thickness2"), "Thickness 2").text(thin.thickness2_expr.clone()).label_width(70.0).build(t),
        ));
    }
}

/// Merge with all and the Merge scope (for Add, Remove and Intersect).
fn merge_rows(b: &mut ChildSpawner, t: &Theme, name: &str, merge_all: bool, items: Vec<String>, active: bool) {
    b.spawn(OptionRow::new(format!("{name}-merge-all"), "Merge with all").checked(merge_all).build(t));
    if !merge_all {
        list(b, t, &format!("{name}-merge-scope-field"), "Merge scope", Role::MergeScope, items, active);
    }
}

/// A select with its label above it ("Start profile condition").
fn stacked_select(b: &mut ChildSpawner, t: &Theme, name: &str, label: &str, role: Role, options: &[(String, bool)], selected: usize) {
    b.spawn((
        t.text(label, t.font_sm, bevy::text::FontWeight::NORMAL, t.muted_foreground),
        Node { margin: UiRect::top(Val::Px(4.0)), ..default() },
    ));
    select_row(b, t, name, "", role, options, selected, None);
}

/// The rows of the dialog's body.
pub(crate) fn body(b: &mut ChildSpawner, t: &Theme, kind: &FeatureKind, field: AppliedField, items_of: &dyn Fn(Role) -> Vec<String>) {
    match kind {
        FeatureKind::Plane(x) => body_column(b, |b| {
            list(b, t, "plane-entities-field", "Entities", Role::PlaneEntities, items_of(Role::PlaneEntities), field == AppliedField::PlaneEntities);
            select_row(b, t, "plane-type", "", Role::PlaneType, &opts(&PlaneType::ALL, |m| m.label(), |_| true), index_of(&PlaneType::ALL, &x.kind), None);
            match x.kind {
                PlaneType::Offset => number(b, t, "plane-offset", "Offset distance", Role::PlaneOffset, &x.offset_expr, Some(x.flip)),
                // Tangent: the angle round the face's axis (without a point), and which of the two
                // tangents through a point outside it.
                PlaneType::LineAngle | PlaneType::Tangent => {
                    number(b, t, "plane-angle", "Angle", Role::PlaneAngle, &x.angle_expr, Some(x.flip));
                    b.spawn(OptionRow::new("plane-flip-alignment", "Flip alignment").checked(x.flip_alignment).build(t));
                }
                // Mid plane of two planes at an angle: which bisector.
                PlaneType::MidPlane => {
                    b.spawn(OptionRow::new("plane-flip-alignment", "Flip alignment").checked(x.flip_alignment).build(t));
                }
                _ => {}
            }
            b.spawn(OptionRow::new("plane-flip-normal", "Flip normal").checked(x.flip_normal).build(t));
        }),
        FeatureKind::Sweep(x) => {
            body_tabs(b, t, "sweep", x.body, x.op);
            body_column(b, |b| {
                let what = if x.body == BodyType::Surface { "Edges and sketch curves to sweep" } else { "Faces and sketch regions to sweep" };
                list(b, t, "sweep-profile-field", what, Role::SweepProfile, items_of(Role::SweepProfile), field == AppliedField::Profile);
                list(b, t, "sweep-path-field", "Sweep path", Role::SweepPath, items_of(Role::SweepPath), field == AppliedField::Path);
                if x.body == BodyType::Thin {
                    thin_rows(b, t, "sweep", &x.thin);
                }
                select_row(b, t, "sweep-profile-control", "", Role::ProfileControl, &opts(&ProfileControl::ALL, |m| m.label(), |_| true), index_of(&ProfileControl::ALL, &x.control), None);
                if x.control == ProfileControl::LockDirection {
                    list(b, t, "sweep-direction-field", "Direction", Role::LockDirection, items_of(Role::LockDirection), field == AppliedField::LockDirection);
                }
                if x.op != BooleanOp::New && x.body != BodyType::Surface {
                    merge_rows(b, t, "sweep", x.merge_all, items_of(Role::MergeScope), field == AppliedField::MergeScope);
                }
            });
        }
        FeatureKind::Loft(x) => {
            body_tabs(b, t, "loft", x.body, x.op);
            body_column(b, |b| {
                b.spawn((
                    Role::LoftProfiles,
                    SelectionList::new("loft-profiles-field")
                        .placeholder("Profiles")
                        .items(items_of(Role::LoftProfiles))
                        .active(field == AppliedField::Profiles)
                        .reorderable(true)
                        .chevrons(true)
                        .build(t),
                ))
                .entry::<Node>()
                .and_modify(|mut n| {
                    n.flex_grow = 0.0;
                    n.margin = UiRect::vertical(Val::Px(2.0));
                });
                if x.body == BodyType::Thin {
                    thin_rows(b, t, "loft", &x.thin);
                }
                b.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(4.0), margin: UiRect::top(Val::Px(6.0)), ..default() })
                    .with_children(|r| {
                        r.spawn(icon("chevron-down", 12.0, t.foreground));
                        r.spawn(t.text("End conditions", t.font_base, bevy::text::FontWeight::NORMAL, t.foreground));
                    });
                let conditions = opts(&LoftCondition::ALL, |c| c.label(), |c| c.available());
                stacked_select(b, t, "loft-start-condition", "Start profile condition", Role::StartCondition, &conditions, index_of(&LoftCondition::ALL, &x.start));
                // P3.11 (PS20.4): Normal direction and Tangent direction take a picked vector.
                if x.start.takes_direction() {
                    list(b, t, "loft-start-direction-field", "Start direction", Role::LoftStartDirection, items_of(Role::LoftStartDirection), field == AppliedField::LoftStartDirection);
                }
                if x.start.has_magnitude() {
                    number(b, t, "loft-start-magnitude", "Start magnitude", Role::StartMagnitude, &x.start_magnitude_expr, None);
                }
                stacked_select(b, t, "loft-end-condition", "End profile condition", Role::EndCondition, &conditions, index_of(&LoftCondition::ALL, &x.end));
                if x.end.takes_direction() {
                    list(b, t, "loft-end-direction-field", "End direction", Role::LoftEndDirection, items_of(Role::LoftEndDirection), field == AppliedField::LoftEndDirection);
                }
                if x.end.has_magnitude() {
                    number(b, t, "loft-end-magnitude", "End magnitude", Role::EndMagnitude, &x.end_magnitude_expr, None);
                }
                if x.op != BooleanOp::New && x.body != BodyType::Surface {
                    merge_rows(b, t, "loft", x.merge_all, items_of(Role::MergeScope), field == AppliedField::MergeScope);
                }
            });
        }
        // As Onshape's Split: Part or Face; Keep tools; for parts, Trim to face boundaries (a
        // face tool) and Keep both sides (off: the side kept, with its flip).
        FeatureKind::Split(x) => {
            b.spawn((
                Role::SplitTypeTab,
                TabStrip::new("split-type").compact().tab("Part").tab("Face").selected(index_of(&SplitType::ALL, &x.split_type)).build(t),
            ));
            body_column(b, |b| {
                let what = match x.split_type {
                    SplitType::Part => "Parts or surfaces to split",
                    SplitType::Face => "Faces to split",
                };
                list(b, t, "split-parts-field", what, Role::SplitParts, items_of(Role::SplitParts), field == AppliedField::SplitParts);
                list(b, t, "split-tool-field", "Entity to split with", Role::SplitTool, items_of(Role::SplitTool), field == AppliedField::SplitTool);
                match x.split_type {
                    SplitType::Part => {
                        b.spawn(OptionRow::new("split-keep-tools", "Keep tools").checked(x.keep_tools).build(t));
                        if matches!(x.tool, Some(SplitToolRef::Face(_))) {
                            b.spawn(OptionRow::new("split-trim", "Trim to face boundaries").checked(x.trim).build(t));
                        }
                        b.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(4.0), ..default() }).with_children(|r| {
                            r.spawn(OptionRow::new("split-keep-both", "Keep both sides").checked(x.keep_both).build(t))
                                .entry::<Node>()
                                .and_modify(|mut n| n.flex_grow = 1.0);
                            if !x.keep_both {
                                flip_button_any(r, t, "split-flip", Role::Flip, x.flip, "Opposite direction");
                            }
                        });
                    }
                    SplitType::Face => {
                        b.spawn(OptionRow::new("split-keep-tools", "Keep tool surfaces and curves").checked(x.keep_tools).build(t));
                    }
                }
            });
        }
        k => crate::pattern_dialog::body(b, t, k, field, items_of),
    }
}

// ---------------------------------------------------------------------------------------------
// Keeping it in step

pub(crate) fn number_text(kind: &FeatureKind, role: Role) -> Option<String> {
    let thin = |t: &ThinWall| match role {
        Role::Thickness1 => Some(t.thickness1_expr.clone()),
        Role::Thickness2 => Some(t.thickness2_expr.clone()),
        _ => None,
    };
    match (kind, role) {
        (FeatureKind::Plane(x), Role::PlaneOffset) => Some(x.offset_expr.clone()),
        (FeatureKind::Plane(x), Role::PlaneAngle) => Some(x.angle_expr.clone()),
        (FeatureKind::Loft(x), Role::StartMagnitude) => Some(x.start_magnitude_expr.clone()),
        (FeatureKind::Loft(x), Role::EndMagnitude) => Some(x.end_magnitude_expr.clone()),
        (FeatureKind::Loft(x), _) => thin(&x.thin),
        (FeatureKind::Sweep(x), _) => thin(&x.thin),
        (k, r) => crate::pattern_dialog::number_text(k, r),
    }
}

pub(crate) fn select_index(kind: &FeatureKind, role: Role) -> Option<usize> {
    Some(match (kind, role) {
        (FeatureKind::Plane(x), Role::PlaneType) => index_of(&PlaneType::ALL, &x.kind),
        (FeatureKind::Sweep(x), Role::ProfileControl) => index_of(&ProfileControl::ALL, &x.control),
        (FeatureKind::Loft(x), Role::StartCondition) => index_of(&LoftCondition::ALL, &x.start),
        (FeatureKind::Loft(x), Role::EndCondition) => index_of(&LoftCondition::ALL, &x.end),
        (k, r) => return crate::pattern_dialog::select_index(k, r),
    })
}

// ---------------------------------------------------------------------------------------------
// Input (each changes a copy of the feature's parameters; the caller makes it a command)

fn body_op(k: &mut FeatureKind) -> Option<(&mut BodyType, &mut BooleanOp)> {
    match k {
        FeatureKind::Sweep(x) => Some((&mut x.body, &mut x.op)),
        FeatureKind::Loft(x) => Some((&mut x.body, &mut x.op)),
        _ => None,
    }
}

fn thin_of(k: &mut FeatureKind) -> Option<&mut ThinWall> {
    match k {
        FeatureKind::Sweep(x) => Some(&mut x.thin),
        FeatureKind::Loft(x) => Some(&mut x.thin),
        _ => None,
    }
}

/// A tab: the body type or the operation.
pub(crate) fn tab(k: &mut FeatureKind, role: Role, i: usize) {
    let Some((body, op)) = body_op(k) else {
        crate::pattern_dialog::tab(k, role, i);
        return;
    };
    match role {
        Role::BodyTab => *body = [BodyType::Solid, BodyType::Surface, BodyType::Thin][i.min(2)],
        Role::OpTab => *op = BooleanOp::ALL[i.min(3)],
        _ => {}
    }
}

pub(crate) fn select(k: &mut FeatureKind, role: Role, i: usize) {
    match (k, role) {
        (FeatureKind::Plane(x), Role::PlaneType) => x.kind = PlaneType::ALL[i.min(PlaneType::ALL.len() - 1)],
        (FeatureKind::Sweep(x), Role::ProfileControl) => x.control = ProfileControl::ALL[i.min(2)],
        (FeatureKind::Loft(x), Role::StartCondition) => x.start = LoftCondition::ALL[i.min(LoftCondition::ALL.len() - 1)],
        (FeatureKind::Loft(x), Role::EndCondition) => x.end = LoftCondition::ALL[i.min(LoftCondition::ALL.len() - 1)],
        (k, r) => crate::pattern_dialog::select(k, r, i),
    }
}

/// A checkbox by its name. Returns its undo label, or `None` if it isn't one of these.
pub(crate) fn checkbox(k: &mut FeatureKind, name: &str, on: bool) -> Option<&'static str> {
    match (k, name) {
        (FeatureKind::Plane(x), "plane-flip-normal-checkbox") => {
            x.flip_normal = on;
            Some("Flip normal")
        }
        (FeatureKind::Plane(x), "plane-flip-alignment-checkbox") => {
            x.flip_alignment = on;
            Some("Flip alignment")
        }
        (FeatureKind::Sweep(x), "sweep-merge-all-checkbox") => {
            x.merge_all = on;
            Some("Merge with all")
        }
        (FeatureKind::Loft(x), "loft-merge-all-checkbox") => {
            x.merge_all = on;
            Some("Merge with all")
        }
        (k, "sweep-mid-plane-checkbox" | "loft-mid-plane-checkbox") => {
            thin_of(k)?.mid_plane = on;
            Some("Mid plane")
        }
        (FeatureKind::Split(x), "split-keep-tools-checkbox") => {
            x.keep_tools = on;
            Some("Keep tools")
        }
        (FeatureKind::Split(x), "split-trim-checkbox") => {
            x.trim = on;
            Some("Trim to face boundaries")
        }
        (FeatureKind::Split(x), "split-keep-both-checkbox") => {
            x.keep_both = on;
            Some("Keep both sides")
        }
        (k, n) => crate::pattern_dialog::checkbox(k, n, on),
    }
}

/// True for the checkboxes of these dialogs.
pub(crate) fn is_checkbox(name: &str) -> bool {
    matches!(
        name,
        "plane-flip-normal-checkbox"
            | "plane-flip-alignment-checkbox"
            | "sweep-merge-all-checkbox"
            | "loft-merge-all-checkbox"
            | "sweep-mid-plane-checkbox"
            | "loft-mid-plane-checkbox"
            | "split-keep-tools-checkbox"
            | "split-trim-checkbox"
            | "split-keep-both-checkbox"
    ) || crate::pattern_dialog::is_checkbox(name)
}

/// Plain numbers (the loft's magnitudes).
pub(crate) fn is_plain(role: Role) -> bool {
    matches!(role, Role::StartMagnitude | Role::EndMagnitude) || crate::transform_ui::is_plain(role)
}

/// A number's quantity: an angle (true) or a length.
pub(crate) fn is_angle(role: Role) -> bool {
    role == Role::PlaneAngle || crate::pattern_dialog::is_angle(role)
}

/// The undo label of a number field.
pub(crate) fn number_label(role: Role) -> Option<&'static str> {
    Some(match role {
        Role::PlaneOffset => "Offset distance",
        Role::PlaneAngle => "Angle",
        Role::StartMagnitude => "Start magnitude",
        Role::EndMagnitude => "End magnitude",
        Role::Thickness1 => "Thickness 1",
        Role::Thickness2 => "Thickness 2",
        r => return crate::pattern_dialog::number_label(r),
    })
}

/// A number field's value (mm, degrees or plain) and its text.
pub(crate) fn set_number(k: &mut FeatureKind, role: Role, v: f64, expr: String) {
    match (k, role) {
        (FeatureKind::Plane(x), Role::PlaneOffset) => {
            x.offset = v;
            x.offset_expr = expr;
        }
        (FeatureKind::Plane(x), Role::PlaneAngle) => {
            x.angle = v;
            x.angle_expr = expr;
        }
        (FeatureKind::Loft(x), Role::StartMagnitude) => {
            x.start_magnitude = v;
            x.start_magnitude_expr = expr;
        }
        (FeatureKind::Loft(x), Role::EndMagnitude) => {
            x.end_magnitude = v;
            x.end_magnitude_expr = expr;
        }
        (k, Role::Thickness1) => {
            if let Some(t) = thin_of(k) {
                t.thickness1 = v;
                t.thickness1_expr = expr;
            }
        }
        (k, Role::Thickness2) => {
            if let Some(t) = thin_of(k) {
                t.thickness2 = v;
                t.thickness2_expr = expr;
            }
        }
        (k, r) => crate::pattern_dialog::set_number(k, r, v, expr),
    }
}

/// A flip button.
pub(crate) fn flip(k: &mut FeatureKind, role: Role) {
    match (k, role) {
        (FeatureKind::Plane(x), Role::Flip) => x.flip = !x.flip,
        (FeatureKind::Split(x), Role::Flip) => x.flip = !x.flip,
        (k, Role::FlipWall) => {
            if let Some(t) = thin_of(k) {
                t.flip_wall = !t.flip_wall;
            }
        }
        (k, r) => crate::pattern_dialog::flip(k, r),
    }
}

/// An item's ✕.
pub(crate) fn remove(k: &mut FeatureKind, role: Role, i: usize) {
    fn take<T>(v: &mut Vec<T>, i: usize) -> bool {
        if i < v.len() {
            v.remove(i);
            true
        } else {
            false
        }
    }
    match (k, role) {
        (FeatureKind::Plane(x), Role::PlaneEntities) => {
            take(&mut x.entities, i);
        }
        (FeatureKind::Sweep(x), Role::SweepProfile) => {
            // The list: regions, whole sketches, faces.
            let (r, s) = (x.regions.len(), x.sketches.len());
            if i < r {
                take(&mut x.regions, i);
            } else if i < r + s {
                take(&mut x.sketches, i - r);
            } else {
                take(&mut x.faces, i - r - s);
            }
        }
        (FeatureKind::Sweep(x), Role::SweepPath) => {
            take(&mut x.path, i);
        }
        (FeatureKind::Sweep(x), Role::LockDirection) => x.lock_direction = None,
        (FeatureKind::Sweep(x), Role::MergeScope) => {
            take(&mut x.merge_scope, i);
        }
        (FeatureKind::Loft(x), Role::LoftProfiles) => {
            take(&mut x.profiles, i);
        }
        (FeatureKind::Loft(x), Role::LoftStartDirection) => x.start_direction = None,
        (FeatureKind::Loft(x), Role::LoftEndDirection) => x.end_direction = None,
        (FeatureKind::Loft(x), Role::MergeScope) => {
            take(&mut x.merge_scope, i);
        }
        (FeatureKind::Split(x), Role::SplitParts) => {
            match x.split_type {
                SplitType::Part => {
                    take(&mut x.parts, i);
                }
                SplitType::Face => {
                    take(&mut x.faces, i);
                }
            }
        }
        (FeatureKind::Split(x), Role::SplitTool) => x.tool = None,
        (k, r) => crate::pattern_dialog::remove(k, r, i),
    }
}

/// A loft profile dragged to another place (PS20.2).
pub(crate) fn move_item(k: &mut FeatureKind, role: Role, from: usize, to: usize) {
    if let (FeatureKind::Loft(x), Role::LoftProfiles) = (k, role)
        && from < x.profiles.len()
        && to < x.profiles.len()
    {
        let p = x.profiles.remove(from);
        x.profiles.insert(to, p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadrs_core::advanced::LoftFeature;

    #[test]
    fn a_dragged_profile_moves() {
        let ids = [FeatureId::new(), FeatureId::new(), FeatureId::new()];
        let s = |i: usize| LoftProfile::Sketch(ids[i]);
        let mut k = FeatureKind::Loft(LoftFeature { profiles: vec![s(0), s(1), s(2)], ..LoftFeature::default() });
        move_item(&mut k, Role::LoftProfiles, 0, 2);
        let FeatureKind::Loft(x) = &k else { unreachable!() };
        assert_eq!(x.profiles, vec![s(1), s(2), s(0)]);
    }
}
