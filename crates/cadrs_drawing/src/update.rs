//! Updating a drawing from the workspace (P3C.6, D13).
//!
//! Drawings don't regenerate by themselves: each view shows the state of its Part Studio kept in
//! the drawing ([`crate::ModelSource`]), and knows the dependency hash of the part it shows
//! ([`crate::View::source_hash`]). When the workspace's hash of that part differs, the view is
//! out of date ([`crate::Drawing::out_of_date`]) and the Update button turns gold. Updating
//! regenerates those views from the workspace and **re-measures** their annotations: every
//! reference is resolved in the new model and its stored shape (the last known place) refreshed.
//!
//! An annotation one of whose references no longer resolves **dangles** (D13.3): the dead
//! reference is never re-attached to a guess and stays exactly where it last was, so all the
//! red annotations of one deleted circle keep meeting at one point (`ex3-step9.png`), while the
//! live references follow the model; a dangling dimension shows its last value (the red
//! 75.00). It is drawn red until the user drags a grip onto new geometry (re-attaching it) or
//! deletes it. Text follows the live references, keeping its place relative to the geometry.

use crate::annotation::{
    Annotation, AnnotationKind, CenterlineKind, CircleCenterline, DimKind, EdgeRef, ModelData, Pick, PointRef, Shape,
    ViewModel, hole_of, is_dangling, measure, resolve, resolve_point,
};

type P2 = [f64; 2];
use crate::note::{Leader, LeaderEnd, leader_dangles};
use crate::view::View;

/// A reference slot of an annotation: an edge, or a point of an edge.
enum Slot<'a> {
    Edge(&'a mut EdgeRef),
    Point(&'a mut PointRef),
}

fn pick_slot(p: &mut Pick) -> Slot<'_> {
    match p {
        Pick::Point(pr) => Slot::Point(pr),
        Pick::Edge(e) => Slot::Edge(e),
    }
}

/// Every reference of an annotation.
fn slots(a: &mut AnnotationKind) -> Vec<Slot<'_>> {
    match a {
        AnnotationKind::Centermark(e) => vec![Slot::Edge(e)],
        AnnotationKind::Centerline(cl) => match &mut cl.kind {
            CenterlineKind::Points { a, b } => vec![Slot::Point(a), Slot::Point(b)],
            CenterlineKind::Lines { a, b } => vec![Slot::Edge(a), Slot::Edge(b)],
        },
        AnnotationKind::CircleCenterline(CircleCenterline::ThreePoints(p)) => p.iter_mut().map(Slot::Point).collect(),
        AnnotationKind::CircleCenterline(CircleCenterline::CenterPoint { center, on }) => vec![Slot::Point(center), Slot::Point(on)],
        AnnotationKind::VirtualSharp { a, b } => vec![Slot::Edge(a), Slot::Edge(b)],
        AnnotationKind::Dimension(d) => match &mut d.kind {
            DimKind::Diameter(e) | DimKind::Radius(e) => vec![Slot::Edge(e)],
            DimKind::Distance { a, b, .. } => vec![pick_slot(a), pick_slot(b)],
            DimKind::Angle { a, b } => vec![Slot::Edge(a), Slot::Edge(b)],
        },
        AnnotationKind::HoleCallout(hc) => vec![Slot::Edge(&mut hc.edge)],
        AnnotationKind::Baseline(b) => std::iter::once(pick_slot(&mut b.base)).chain(b.targets.iter_mut().map(pick_slot)).collect(),
        AnnotationKind::Ordinate(o) => std::iter::once(Slot::Point(&mut o.origin)).chain(o.points.iter_mut().map(Slot::Point)).collect(),
        AnnotationKind::ChamferDim(c) => vec![Slot::Edge(&mut c.edge)],
        AnnotationKind::ArcLength(a) => vec![Slot::Edge(&mut a.edge)],
        AnnotationKind::FeatureControl(f) => f.edge.iter_mut().map(Slot::Edge).collect(),
        AnnotationKind::Datum(d) => vec![Slot::Edge(&mut d.edge)],
        AnnotationKind::SurfaceFinish(f) => vec![Slot::Edge(&mut f.edge)],
        AnnotationKind::Weld(w) => vec![Slot::Edge(&mut w.edge)],
        AnnotationKind::Callout(_) => Vec::new(),
    }
}

/// A representative point of a shape: a circle's centre, a line's middle.
fn anchor_of(s: &Shape) -> P2 {
    match *s {
        Shape::Point(p) => p,
        Shape::Line { a, b } | Shape::Curve { a, b } => [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0],
        Shape::Circle { center, .. } => center,
    }
}

/// Where a slot is in model `m`, if its reference resolves there.
fn slot_anchor(view: &View, m: &dyn ViewModel, s: &Slot) -> Option<P2> {
    match s {
        Slot::Edge(e) => {
            let r = resolve(view, m, e);
            r.found.then(|| anchor_of(&r.shape))
        }
        Slot::Point(p) => {
            if !resolve(view, m, &p.edge).found {
                return None;
            }
            resolve_point(view, m, p).map(|(at, _)| at)
        }
    }
}

/// Moves a reference to its place in `m` (it resolves there).
fn take_from(view: &View, m: &dyn ViewModel, s: &mut Slot) {
    match s {
        Slot::Edge(e) => {
            let r = resolve(view, m, e);
            if r.found {
                e.shape = r.shape;
            }
        }
        Slot::Point(p) => {
            if let Some((at, _)) = resolve_point(view, m, p) {
                p.hint = at;
            }
            let r = resolve(view, m, &p.edge);
            if r.found {
                p.edge.shape = r.shape;
            }
        }
    }
}

/// Annotation `a` of `view` after an update from the model `old` (what the view showed, if it
/// is known) to `new`, re-measured: each reference that resolves in the new model takes its new
/// place; one that doesn't (it dangles) is never re-bound to other geometry and stays exactly
/// where it last was (the old model's place), so every dangling reference to the same deleted
/// geometry stays at one point (the red dot, the end of the red dimension and the red leader
/// of `ex3-step9.png` meet). A dangling dimension keeps the value it last measured. The text
/// moves with the annotation's live references (their mean displacement), so it keeps its
/// place relative to the geometry.
pub fn refreshed(view: &View, old: Option<&dyn ViewModel>, new: &dyn ViewModel, a: &Annotation) -> Annotation {
    let mut out = a.clone();
    // How far the references that resolve in both models moved.
    let mut moves: Vec<P2> = Vec::new();
    if let Some(old) = old {
        let mut probe = a.clone();
        for s in slots(&mut probe.kind) {
            if let (Some(o), Some(n)) = (slot_anchor(view, old, &s), slot_anchor(view, new, &s)) {
                moves.push([n[0] - o[0], n[1] - o[1]]);
            }
        }
    }
    let d = if moves.is_empty() {
        [0.0, 0.0]
    } else {
        let k = moves.len() as f64;
        [moves.iter().map(|m| m[0]).sum::<f64>() / k, moves.iter().map(|m| m[1]).sum::<f64>() / k]
    };
    let dangling = is_dangling(view, new, a);
    // The value it showed (for a dimension that dangles now).
    let last = match (&a.kind, dangling) {
        (AnnotationKind::Dimension(dim), true) => match old {
            Some(o) if !is_dangling(view, o, a) => measure(view, o, dim).map(|m| m.value),
            _ => dim.last.or_else(|| measure(view, &ModelData::default(), dim).map(|m| m.value)),
        },
        _ => None,
    };
    for mut s in slots(&mut out.kind) {
        if slot_anchor(view, new, &s).is_some() {
            take_from(view, new, &mut s);
        } else if let Some(old) = old
            && slot_anchor(view, old, &s).is_some()
        {
            // Dangling: its last place.
            take_from(view, old, &mut s);
        }
    }
    match &mut out.kind {
        AnnotationKind::Dimension(dim) => {
            dim.text = [dim.text[0] + d[0], dim.text[1] + d[1]];
            dim.last = last;
        }
        AnnotationKind::Ordinate(o) => {
            o.level += if o.vertical { d[0] } else { d[1] };
        }
        k @ (AnnotationKind::Baseline(_)
        | AnnotationKind::ChamferDim(_)
        | AnnotationKind::ArcLength(_)
        | AnnotationKind::FeatureControl(_)
        | AnnotationKind::Datum(_)
        | AnnotationKind::SurfaceFinish(_)
        | AnnotationKind::Weld(_)) => {
            if let Some(t) = k.text_mut() {
                *t = [t[0] + d[0], t[1] + d[1]];
            }
        }
        AnnotationKind::HoleCallout(hc) => {
            hc.text = [hc.text[0] + d[0], hc.text[1] + d[1]];
            if let Some(h) = hole_of(new, &hc.edge).or_else(|| old.and_then(|o| hole_of(o, &hc.edge))) {
                hc.last = Some(h.clone());
            }
        }
        _ => {}
    }
    out
}

fn refresh_point(view: &View, m: &dyn ViewModel, p: &mut PointRef) {
    take_from(view, m, &mut Slot::Point(p));
}

/// A note's leader after an update (like [`refreshed`]).
pub fn refreshed_leader(view: &View, old: Option<&dyn ViewModel>, new: &dyn ViewModel, l: &Leader) -> Leader {
    let mut out = *l;
    let m = if !leader_dangles(view, new, l) {
        Some(new)
    } else {
        old.filter(|o| !leader_dangles(view, *o, l))
    };
    if let Some(m) = m {
        match &mut out.end {
            LeaderEnd::Point(p) => refresh_point(view, m, p),
            LeaderEnd::Edge { edge, at } => {
                let res = resolve(view, m, edge);
                if res.found {
                    *at = res.shape.nearest(*at);
                    edge.shape = res.shape;
                }
            }
        }
    }
    out
}

/// The [`crate::DrawingOp::Update`] that brings `views` up to date: each with the model it
/// showed (`old`, if known) and the workspace's (`new`). `sources` are the studios' new states;
/// each view takes its part's hash in them. Notes with a leader into one of the views are
/// refreshed too.
pub fn update_op(
    d: &crate::Drawing,
    sources: Vec<crate::ModelSource>,
    views: &[(crate::ViewId, Option<&dyn ViewModel>, &dyn ViewModel)],
) -> crate::DrawingOp {
    let mut out = Vec::new();
    for (id, old, new) in views {
        let Some((_, v)) = d.view(*id) else { continue };
        let source_hash = sources
            .iter()
            .find(|s| s.element == v.reference.element)
            .and_then(|s| s.hash_of(v.reference.part))
            .or(v.source_hash);
        // Callouts (P3C.5) follow their occurrences in the assembly's new state.
        let old_asm = d.source(v.reference.element).and_then(|s| s.assembly.as_ref());
        let new_asm = sources.iter().find(|s| s.element == v.reference.element).and_then(|s| s.assembly.as_ref()).or(old_asm);
        let sheet = d.view(*id).and_then(|(i, _)| d.sheets.get(i));
        let boms: Vec<&crate::assembly::BomData> = sheet.map(|s| s.tables.iter().filter_map(|t| t.bom.as_ref()).collect()).unwrap_or_default();
        let annotations = v
            .annotations
            .iter()
            .map(|a| match &a.kind {
                AnnotationKind::Callout(c) => Annotation {
                    id: a.id,
                    kind: AnnotationKind::Callout(crate::assembly::refresh_callout(v, old_asm, new_asm, &boms, c)),
                },
                _ => refreshed(v, *old, *new, a),
            })
            .collect();
        out.push(crate::ViewUpdate { id: *id, source_hash, annotations });
    }
    let mut notes = Vec::new();
    for s in &d.sheets {
        for n in &s.notes {
            let mut changed = n.clone();
            for l in &mut changed.leaders {
                if let Some((_, old, new)) = views.iter().find(|(id, _, _)| *id == l.view)
                    && let Some((_, v)) = d.view(l.view)
                {
                    *l = refreshed_leader(v, *old, *new, l);
                }
            }
            if changed != *n {
                notes.push((s.id, changed));
            }
        }
    }
    crate::DrawingOp::Update { sources, views: out, notes }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotation::{
        AnnotationKind, DimFormat, Dimension, ModelData, ModelEdge, Orient, PointOf, Shape, annotation_graphics, is_dangling,
        measure,
    };
    use crate::standard::Scale;
    use crate::view::NamedView;
    use crate::{DrawingStyle, ObjectRef};
    use cadrs_kernel::naming::{EdgeName, FaceName, FaceOrigin, OpId};
    use cadrs_kernel::{ProjClass, ProjCurve, ProjEdge, ProjSource, ProjVisibility};
    use uuid::Uuid;

    fn name(k: u64) -> EdgeName {
        let f = |i: u64| FaceName { op: OpId::from_u128(1), origin: FaceOrigin::Unnamed { index: i as u32 }, split: 0 };
        EdgeName { faces: [f(k), f(k + 100)], index: 0 }
    }

    /// A circle of radius `r` at (x, 0) seen face-on in a front view, as model data.
    fn circle(m: &mut ModelData, k: u64, x: f64, r: f64) {
        let pts: Vec<[f64; 3]> = (0..=16)
            .map(|i| {
                let t = std::f64::consts::TAU * i as f64 / 16.0;
                [x + r * t.cos(), 0.0, r * t.sin()]
            })
            .collect();
        m.edges.insert(name(k), ModelEdge::Circle { center: [x, 0.0, 0.0], normal: [0.0, 1.0, 0.0], radius: r, points: pts });
        m.projection.edges.push(ProjEdge {
            visibility: ProjVisibility::Visible,
            class: ProjClass::Sharp,
            curve: ProjCurve::Arc {
                center: [x, 0.0].into(),
                radius: r,
                start: [x + r, 0.0].into(),
                mid: [x - r, 0.0].into(),
                end: [x + r, 0.0].into(),
                full: true,
            },
            points: vec![[x + r, 0.0].into(), [x - r, 0.0].into()],
            source: Some(ProjSource { edge_name: Some(name(k)), ..Default::default() }),
        });
    }

    fn centre(m: &ModelData, i: usize) -> Pick {
        let e = EdgeRef::of(&m.projection.edges[i]);
        Pick::Point(PointRef { edge: e, of: PointOf::Center, hint: [0.0, 0.0] })
    }

    fn view() -> View {
        View::base(ObjectRef { element: Uuid::nil(), part: None }, NamedView::Front, Scale::new(1, 1), [100.0, 100.0])
    }

    #[test]
    fn a_deleted_edge_dangles_and_is_never_rebound() {
        let v = view();
        // Old model: circles 1 (x 0), 2 (x 75), 3 (x 29).
        let mut old = ModelData::default();
        circle(&mut old, 1, 0.0, 4.125);
        circle(&mut old, 2, 75.0, 8.0);
        circle(&mut old, 3, 29.0, 12.7);
        let dim = Annotation::new(AnnotationKind::Dimension(Dimension {
            kind: DimKind::Distance { a: centre(&old, 0), b: centre(&old, 1), orient: Orient::Horizontal },
            text: [40.0, 30.0],
            format: DimFormat::default(),
            last: None,
        }));
        let AnnotationKind::Dimension(d) = &dim.kind else { unreachable!() };
        assert_eq!(measure(&v, &old, d).unwrap().value, 75.0);
        // New model: circle 1 is gone, 2 moved to x 100, and a new circle sits where 1 was.
        let mut new = ModelData::default();
        circle(&mut new, 2, 100.0, 8.0);
        circle(&mut new, 3, 54.0, 12.7);
        circle(&mut new, 9, 0.0, 4.125);
        assert!(is_dangling(&v, &new, &dim), "circle 1's name no longer resolves");
        let up = refreshed(&v, Some(&old), &new, &dim);
        // Never re-bound to the circle now at the same place: the name is kept.
        let AnnotationKind::Dimension(ud) = &up.kind else { unreachable!() };
        let DimKind::Distance { a: Pick::Point(pa), b: Pick::Point(pb), .. } = &ud.kind else { unreachable!() };
        assert_eq!(pa.edge.edge, Some(name(1)));
        // Only the dead end is frozen, exactly where it was (x 0, where every other reference to
        // circle 1 stays too); the live end follows its circle to x 100. It shows its last value.
        assert_eq!(pb.edge.shape.circle().unwrap().0, [100.0, 0.0]);
        assert_eq!(pa.edge.shape.circle().unwrap().0, [0.0, 0.0]);
        assert_eq!(ud.last, Some(75.0));
        let g = annotation_graphics(&DrawingStyle::default(), &v, &new, &up).unwrap();
        assert!(g.dangling);
        assert_eq!(g.texts[0].text, "75.00");
        // A centermark on the same deleted circle stays at the same point: they coincide.
        let cm = Annotation::new(AnnotationKind::Centermark(EdgeRef::of(&old.projection.edges[0])));
        let ucm = refreshed(&v, Some(&old), &new, &cm);
        let AnnotationKind::Centermark(e) = &ucm.kind else { unreachable!() };
        assert_eq!(e.shape.circle().unwrap().0, pa.edge.shape.circle().unwrap().0);
        let gcm = annotation_graphics(&DrawingStyle::default(), &v, &new, &ucm).unwrap();
        assert!(gcm.dangling && !gcm.fills.is_empty(), "a dangling centermark is a filled dot");
        // Its text moved with it (its live end moved 25).
        let AnnotationKind::Dimension(ud2) = &up.kind else { unreachable!() };
        assert_eq!(ud2.text, [65.0, 30.0]);
        // Re-attached (the grip dragged onto circle 3's centre): measured again, not dangling.
        let mut re = up.clone();
        if let AnnotationKind::Dimension(d) = &mut re.kind
            && let DimKind::Distance { a, .. } = &mut d.kind
        {
            *a = centre(&new, 1);
        }
        assert!(!is_dangling(&v, &new, &re));
        let AnnotationKind::Dimension(rd) = &re.kind else { unreachable!() };
        assert_eq!(measure(&v, &new, rd).unwrap().value, 46.0);
        let g = annotation_graphics(&DrawingStyle::default(), &v, &new, &re).unwrap();
        assert!(!g.dangling);
        assert_eq!(g.texts[0].text, "46.00");
    }

    #[test]
    fn resolving_annotations_follow_the_model() {
        let v = view();
        let mut old = ModelData::default();
        circle(&mut old, 2, 75.0, 8.0);
        let dia = Annotation::new(AnnotationKind::Dimension(Dimension {
            kind: DimKind::Diameter(EdgeRef::of(&old.projection.edges[0])),
            text: [90.0, 20.0],
            format: DimFormat::default(),
            last: None,
        }));
        let mut new = ModelData::default();
        circle(&mut new, 2, 100.0, 7.5);
        assert!(!is_dangling(&v, &new, &dia));
        let up = refreshed(&v, Some(&old), &new, &dia);
        let AnnotationKind::Dimension(d) = &up.kind else { unreachable!() };
        let DimKind::Diameter(e) = &d.kind else { unreachable!() };
        assert_eq!(e.shape, Shape::Circle { center: [100.0, 0.0], radius: 7.5, arc: None });
        let g = annotation_graphics(&DrawingStyle::default(), &v, &new, &up).unwrap();
        assert_eq!(g.texts[0].text, "Ø15.00");
    }
}
