//! P3.7 / PS21: the Funnel exercise (intro-to-part-studios.md PS21, `ex4-*`) through the
//! document's commands, in inches and pounds, with the course's self-check: Onshape shows
//! V = 2.974 in³, A = 84.098 in², CoM (0.556, −7.545e−5, −0.547) in and 0.098 lb (2.974 in³ ×
//! 0.033 lb/in³, Polypropylene).
//!
//! The loft's surface is cadrs's (a cubic B-spline through the sections with the end
//! derivatives, `cadrs_kernel::loft`), not Parasolid's, so the volume is compared within 1 % and
//! the area within 2 % (the brief's tolerances; the kept variant is within 0.1 % of both).
//!
//! **The PS21.7 direction question** (gap doc, "What each exercise needs"): the drawing
//! (`ex4-step7.png`) shows the Ø2.5 and Ø0.5 circles on the side away from the handle (−X), but
//! the screenshot's CoM X is +0.556, which could fit either reading because the heavy handle and
//! bead are on +X. Both variants are built and the one closer to (0.556, 2.974) is kept: the
//! circles on −X (CoM X 0.556), as drawn; on +X the CoM X is 0.778.
//!
//! **Mass.** Polypropylene is 0.033 lb/in³ exactly (913.437 kg/m³, `material`), so 2.975 in³
//! weighs 0.0982 lb: 0.098 lb as the course's, and the inertia reads as `ex4-step18.png`'s
//! (Lxx 0.205, Lyy 0.518, Lxz −0.059 in² lb).
#![cfg(feature = "occt")]

use cadrs_core::advanced::{LoftCondition, LoftFeature, LoftProfile, PathRef, SweepFeature};
use cadrs_core::applied::{EdgeOrFace, FilletFeature, ShellFeature};
use cadrs_core::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, SetExtrude, SetPartMaterial};
use cadrs_core::document::{BooleanFeature, BooleanKind, BooleanOp, Document, EdgeRef, ExtrudeFeature, FaceRef, FeatureKind};
use cadrs_core::parts::mass_report;
use cadrs_core::plane::{PlaneEntity, PlaneFeature, PlaneType};
use cadrs_core::{ElementId, Feature, FeatureId, History, Part, material, rebuild, samples};
use cadrs_sketch::{CurveKind, PlaneRef, Sketch, SketchOp, Vec2};

/// Millimetres per inch.
const IN: f64 = 25.4;
/// Kilograms per pound.
const LB: f64 = 0.453_592_37;

struct Studio {
    d: Document,
    h: History,
    el: ElementId,
}

impl Studio {
    fn new() -> Self {
        let d = Document::new("Funnel");
        let el = d.elements[0].id;
        Self { d, h: History::default(), el }
    }

    fn features(&self) -> Vec<Feature> {
        self.d.element(self.el).unwrap().features().to_vec()
    }

    fn g(&self, f: FeatureId) -> Sketch {
        self.d.element(self.el).unwrap().feature(f).unwrap().sketch().unwrap().geometry.clone()
    }

    fn sketch(&mut self, plane: PlaneRef, ops: Vec<SketchOp>) -> FeatureId {
        let f = FeatureId::new();
        self.h.execute(&mut self.d, &AddSketch { element: self.el, feature: f, plane: Some(plane) }).unwrap();
        for op in ops {
            self.edit(f, op);
        }
        f
    }

    fn edit(&mut self, f: FeatureId, op: SketchOp) {
        self.h.execute(&mut self.d, &EditSketch { element: self.el, feature: f, op }).unwrap();
    }

    fn add(&mut self, base: &str, kind: FeatureKind) -> FeatureId {
        let feature = FeatureId::new();
        self.h
            .execute(&mut self.d, &AddFeature { element: self.el, feature, base_name: base.into(), kind })
            .unwrap();
        feature
    }

    fn extrude(&mut self, e: ExtrudeFeature) -> FeatureId {
        let f = FeatureId::new();
        self.h
            .execute(&mut self.d, &AddExtrude { element: self.el, feature: f, extrude: ExtrudeFeature::default() })
            .unwrap();
        self.h
            .execute(&mut self.d, &SetExtrude { element: self.el, feature: f, extrude: e, label: "Extrude".into() })
            .unwrap();
        f
    }

    fn parts(&self) -> Vec<Part> {
        let b = rebuild::build(&self.features());
        assert!(b.errors.is_empty(), "rebuild errors: {:?}", b.errors);
        b.parts.clone()
    }
}

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x * IN, y * IN)
}

/// The edge of the part passing nearest `p` (inches).
fn edge_ref(part: &Part, p: [f64; 3]) -> EdgeRef {
    let p = p.map(|c| c * IN);
    let e = part.solid.edges.iter().min_by(|a, b| a.distance(p).total_cmp(&b.distance(p))).unwrap();
    assert!(e.distance(p) < 1e-3, "no edge at {p:?}");
    EdgeRef { part: part.id, edge: e.name, seed: p }
}

/// The planar face of the part facing `n` that contains `p` (inches).
fn face_ref(part: &Part, n: [f64; 3], p: [f64; 3]) -> FaceRef {
    let p = p.map(|c| c * IN);
    let s = &part.solid;
    let i = (0..s.faces.len())
        .find(|&i| {
            s.faces[i].plane.is_some_and(|pl| {
                let m = pl.normal();
                let len = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt();
                (m[0] * n[0] + m[1] * n[1] + m[2] * n[2]) / len > 0.999
            }) && s.face_contains(i, p)
        })
        .expect("a face there");
    FaceRef { part: part.id, face: s.faces[i].name, seed: p }
}

/// Where the slanted handle line from `(4.5, 0.75)` (at 105° to the end line, 15° above the
/// −X direction) meets the 6 × 4 ellipse: the root of (4.5 − c·s)²/9 + (0.75 + d·s)²/4 = 1 for
/// s in (0, 3), by bisection (c = cos 15°, d = sin 15°).
fn handle_meets_ellipse() -> (f64, f64) {
    let (c, d) = (15f64.to_radians().cos(), 15f64.to_radians().sin());
    let f = |s: f64| (4.5 - c * s).powi(2) / 9.0 + (0.75 + d * s).powi(2) / 4.0 - 1.0;
    let (mut lo, mut hi) = (0.0, 3.0);
    for _ in 0..200 {
        let m = (lo + hi) / 2.0;
        if f(m) > 0.0 { lo = m } else { hi = m }
    }
    let s = (lo + hi) / 2.0;
    (4.5 - c * s, 0.75 + d * s)
}

/// The Funnel's numbers.
#[derive(Debug, Clone, Copy)]
struct Report {
    /// in³
    volume: f64,
    /// in²
    area: f64,
    /// in
    com: [f64; 3],
    /// lb
    mass: f64,
    /// The edges Create selection → Tangent connected picks from the end line's top edge.
    tangent_connected: usize,
    /// in² lb: Lxx, Lyy, Lzz, Lxz.
    inertia: [f64; 4],
}

/// Builds the Funnel with the loft's circles on the `side` of the origin (+1: +X, the handle's
/// side; −1: −X).
fn funnel(side: f64) -> Report {
    let mut st = Studio::new();
    // PS21.2: Sketch 1 on Top: the 6 × 4 ellipse, its offset 0.125 inside (the rim), and the
    // handle (the end line 1.5 long at x = 4.5, the slanted lines at 105° to it, ending on the
    // ellipse).
    let ellipse = SketchOp::AddEllipse { center: v(0.0, 0.0), major: v(3.0, 0.0), minor: 2.0 * IN, construction: false };
    let s1 = st.sketch(PlaneRef::Top, vec![ellipse.clone()]);
    let e = st.g(s1).curves.keys().next().unwrap();
    st.edit(s1, SketchOp::Offset { chain: vec![(e, false)], distance: 0.125 * IN, left: true, label: (0.0, 0.0) });
    let (px, py) = handle_meets_ellipse();
    st.edit(
        s1,
        SketchOp::AddPolyline {
            points: vec![v(px, py), v(4.5, 0.75), v(4.5, -0.75), v(px, -py)],
            closed: false,
            construction: false,
            label: "Add line",
        },
    );
    // PS21.3: Extrude the whole sketch 0.125 up, New: the band and the handle.
    st.extrude(ExtrudeFeature { sketches: vec![s1], depth: 0.125 * IN, depth_expr: "0.125 in".into(), ..ExtrudeFeature::default() });
    // PS21.4: Sketch 2 on the rim's bottom face (z = 0, the Top plane's place): the rim's inner
    // curve (0.125 inside the ellipse) and its offset 0.05 outwards. Here the ellipse is drawn
    // and offset twice; Profile 1 is the disc and the band between the two offsets (the app's
    // scenario uses Use on the rim's edge instead).
    let s2 = st.sketch(PlaneRef::Top, vec![ellipse]);
    let e2 = st.g(s2).curves.keys().next().unwrap();
    st.edit(s2, SketchOp::Offset { chain: vec![(e2, false)], distance: 0.125 * IN, left: true, label: (0.0, 0.0) });
    let inner = st.g(s2).curves.iter().find(|(_, c)| matches!(c.kind, CurveKind::EllipseOffset { .. })).unwrap().0;
    st.edit(s2, SketchOp::Offset { chain: vec![(inner, false)], distance: 0.05 * IN, left: false, label: (0.0, 0.0) });
    let profile1 = samples::region_refs(s2, &st.g(s2), &[v(0.0, 0.0), v(2.9, 0.0)]);
    assert_eq!(profile1.len(), 2, "the disc and the 0.05 band");
    // PS21.5: the Lower Plane, Top offset 3 in down.
    let lower = st.add(
        "Plane",
        FeatureKind::Plane(PlaneFeature {
            kind: PlaneType::Offset,
            entities: vec![PlaneEntity::Plane(PlaneRef::Top)],
            offset: 3.0 * IN,
            offset_expr: "3 in".into(),
            flip: true,
            ..PlaneFeature::default()
        }),
    );
    // PS21.6: the Middle Plane, between Top and the Lower Plane (z = −1.5 in).
    let lower_ref = cadrs_core::parts::plane_feature_ref(&st.features(), lower).expect("the lower plane builds");
    let middle = st.add(
        "Plane",
        FeatureKind::Plane(PlaneFeature {
            kind: PlaneType::MidPlane,
            entities: vec![PlaneEntity::Plane(PlaneRef::Top), PlaneEntity::Plane(lower_ref)],
            ..PlaneFeature::default()
        }),
    );
    let middle_ref = cadrs_core::parts::plane_feature_ref(&st.features(), middle).expect("the middle plane builds");
    assert!((middle_ref.frame().origin[2] + 1.5 * IN).abs() < 1e-9);
    // PS21.7 and PS21.8: the Ø2.5 circle on the Middle Plane 0.5 from the origin, the Ø0.5 on
    // the Lower Plane 0.75 from it, on `side`.
    let s3 = st.sketch(middle_ref, vec![SketchOp::AddCircle { center: v(side * 0.5, 0.0), radius: 1.25 * IN, construction: false }]);
    let s4 = st.sketch(lower_ref, vec![SketchOp::AddCircle { center: v(side * 0.75, 0.0), radius: 0.25 * IN, construction: false }]);
    // PS21.9 and PS21.11: the Loft, New (as after the edit that lets the shell work), Normal to
    // profile at both ends, magnitudes 0.5 and 1.
    let one = |s: FeatureId, g: &Sketch, at: Vec2| LoftProfile::Regions { sketch: s, regions: samples::region_refs(s, g, &[at]) };
    let (g3, g4) = (st.g(s3), st.g(s4));
    let loft = st.add(
        "Loft",
        FeatureKind::Loft(LoftFeature {
            profiles: vec![
                LoftProfile::Regions { sketch: s2, regions: profile1 },
                one(s3, &g3, v(side * 0.5, 0.0)),
                one(s4, &g4, v(side * 0.75, 0.0)),
            ],
            start: LoftCondition::NormalToProfile,
            start_magnitude: 0.5,
            start_magnitude_expr: "0.5".into(),
            end: LoftCondition::NormalToProfile,
            end_magnitude: 1.0,
            end_magnitude_expr: "1".into(),
            ..LoftFeature::default()
        }),
    );
    let parts = st.parts();
    assert_eq!(parts.len(), 2, "the rim and the loft");
    let loft_part = parts.iter().find(|p| p.feature == loft).unwrap().clone();
    let rim = parts.iter().find(|p| p.feature != loft).unwrap().clone();
    // PS21.10: Shell the loft 0.05 in, its top and bottom faces removed.
    let top = face_ref(&loft_part, [0.0, 0.0, 1.0], [0.0, 0.0, 0.0]);
    let bottom = face_ref(&loft_part, [0.0, 0.0, -1.0], [side * 0.75, 0.0, -3.0]);
    st.add(
        "Shell",
        FeatureKind::Shell(ShellFeature { faces: vec![top, bottom], thickness: 0.05 * IN, thickness_expr: "0.05 in".into(), ..ShellFeature::default() }),
    );
    // PS21.12: Boolean Union, the loft first (it gives the result its identity).
    st.add("Boolean", FeatureKind::Boolean(BooleanFeature { op: BooleanKind::Union, tools: vec![loft_part.id, rim.id], ..BooleanFeature::default() }));
    let parts = st.parts();
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].id, loft_part.id, "the first tool supplies the part");
    let funnel = parts[0].clone();
    // PS21.13: Fillet R0.5 on the handle's four vertical corner edges (0.125 high).
    let corners = [[px, py], [px, -py], [4.5, 0.75], [4.5, -0.75]];
    let entities = corners.iter().map(|c| EdgeOrFace::Edge(edge_ref(&funnel, [c[0], c[1], 0.0625]))).collect();
    st.add(
        "Fillet",
        FeatureKind::Fillet(FilletFeature { entities, size: 0.5 * IN, size_expr: "0.5 in".into(), ..FilletFeature::default() }),
    );
    // PS21.14: Sketch 5 on Front: the D-shaped bead (an R0.125 half circle centred on the
    // handle's top outer edge, closed by two vertical lines and a line on the handle's bottom
    // face). Front's sketch axes are X and Z.
    let s5 = st.sketch(
        PlaneRef::Front,
        vec![
            SketchOp::AddArc { center: v(4.5, 0.125), start: v(4.625, 0.125), end: v(4.375, 0.125), construction: false },
            SketchOp::AddPolyline {
                points: vec![v(4.375, 0.125), v(4.375, 0.0), v(4.625, 0.0), v(4.625, 0.125)],
                closed: false,
                construction: false,
                label: "Add line",
            },
        ],
    );
    let bead = samples::region_refs(s5, &st.g(s5), &[v(4.5, 0.06)]);
    assert_eq!(bead.len(), 1);
    // PS21.15: Sweep Add along the outer top edges: the tangent-connected chain of the end
    // line's top edge (Create selection → Tangent connected).
    let funnel = st.parts()[0].clone();
    let start = edge_ref(&funnel, [4.5, 0.0, 0.125]);
    let group = funnel.solid.edge(&start.edge).unwrap().tangent_group;
    let path: Vec<PathRef> = funnel
        .solid
        .edges
        .iter()
        .filter(|e| e.tangent_group == group)
        .map(|e| PathRef::Edge(EdgeRef { part: funnel.id, edge: e.name, seed: e.midpoint() }))
        .collect();
    let tangent_connected = path.len();
    st.add("Sweep", FeatureKind::Sweep(SweepFeature { regions: bead, path, op: BooleanOp::Add, ..SweepFeature::default() }));
    // PS21.16: Extrude the loft's bottom annulus 1 in down, Add (the spout).
    let funnel = st.parts()[0].clone();
    let annulus = face_ref(&funnel, [0.0, 0.0, -1.0], [side * 0.75 + 0.225, 0.0, -3.0]);
    st.extrude(ExtrudeFeature { faces: vec![annulus], depth: IN, depth_expr: "1 in".into(), op: BooleanOp::Add, ..ExtrudeFeature::default() });
    // PS21.17 and PS21.18: Polypropylene, and the mass properties.
    let parts = st.parts();
    assert_eq!(parts.len(), 1, "one part: the Funnel");
    let pp = material::library("Polypropylene");
    st.h.execute(&mut st.d, &SetPartMaterial { element: st.el, parts: vec![parts[0].id], material: pp }).unwrap();
    let props = st.d.element(st.el).unwrap().part_props().to_vec();
    let r = mass_report(&[&parts[0]], &props).unwrap();
    let m = r.mass.unwrap();
    Report {
        volume: r.volume / IN.powi(3),
        area: r.surface_area / IN.powi(2),
        com: [m.center_of_mass.x / IN, m.center_of_mass.y / IN, m.center_of_mass.z / IN],
        mass: m.mass / LB,
        inertia: [(0, 0), (1, 1), (2, 2), (0, 2)].map(|(a, b)| m.inertia[(a, b)] / LB / IN.powi(2)),
        tangent_connected,
    }
}

/// PS21: both readings of PS21.7, the kept one checked against the course's self-check.
#[test]
fn funnel_course_self_check() {
    let away = funnel(-1.0);
    let toward = funnel(1.0);
    eprintln!("circles on −X (away from the handle): {away:?}");
    eprintln!("circles on +X (the handle's side):    {toward:?}");
    // The screenshot: CoM X 0.556 in, V 2.974 in³. How far each variant is from it (relative).
    let miss = |r: &Report| ((r.com[0] - 0.556) / 0.556).abs() + ((r.volume - 2.974) / 2.974).abs();
    assert!(miss(&away) < miss(&toward), "the circles lie away from the handle, as drawn");
    let kept = away;
    assert!((kept.volume - 2.974).abs() <= 0.01 * 2.974, "volume {}", kept.volume);
    assert!((kept.area - 84.098).abs() <= 0.02 * 84.098, "area {}", kept.area);
    assert!((kept.com[0] - 0.556).abs() < 0.005 && kept.com[1].abs() < 1e-3 && (kept.com[2] + 0.547).abs() < 0.005, "CoM {:?}", kept.com);
    // 0.033 lb/in³.
    let density = 0.033;
    assert!((kept.mass - kept.volume * density).abs() < 1e-9);
    // The panel's 3 decimals, as ex4-step18.png: 0.098 lb; Lxx 0.205, Lyy 0.518, Lxz −0.059.
    assert_eq!(format!("{:.3}", kept.mass), "0.098");
    let [lxx, lyy, _, lxz] = kept.inertia;
    assert_eq!([format!("{lxx:.3}"), format!("{lyy:.3}"), format!("{lxz:.3}")], ["0.205", "0.518", "-0.059"]);
    // Tangent connected finds Onshape's 8 edges: cadrs's rim top is two faces (the band's and
    // the handle's region), where OCCT's fillet leaves a sliver edge; the sliver is left out of
    // the solid and the chain joins across it (`brep::SLIVER`).
    assert_eq!(kept.tangent_connected, 8);
}
