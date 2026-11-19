//! The Frame feature (`cadrs_core::frame`, issue #266) through the document's commands: members
//! along sketch curves joined by mitres, butts and copes, checked against closed-form volumes
//! and lengths, and the cut list they make.
#![cfg(feature = "brep")]

use cadrs_core::advanced::PathRef;
use cadrs_core::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, SetExtrude};
use cadrs_core::document::{
    BooleanFeature, BooleanKind, Document, EdgeRef, ExtrudeFeature, FeatureKind,
};
use cadrs_core::frame::{
    self, Anchor, CornerOverride, CornerType, EndCut, FrameFeature, ProfileFamily, SketchPointRef,
};
use cadrs_core::{ElementId, Feature, FeatureId, History, Part, rebuild};
use cadrs_sketch::{PlaneRef, SketchOp, Vec2};
use std::f64::consts::PI;

struct Studio {
    d: Document,
    h: History,
    el: ElementId,
}

impl Studio {
    fn new() -> Self {
        let d = Document::new("Frames");
        let el = d.elements[0].id;
        Self {
            d,
            h: History::default(),
            el,
        }
    }

    fn features(&self) -> Vec<Feature> {
        self.d.element(self.el).unwrap().features().to_vec()
    }

    fn sketch(&mut self, plane: PlaneRef, ops: Vec<SketchOp>) -> FeatureId {
        let f = FeatureId::new();
        self.h
            .execute(
                &mut self.d,
                &AddSketch {
                    element: self.el,
                    feature: f,
                    plane: Some(plane),
                },
            )
            .unwrap();
        for op in ops {
            self.h
                .execute(
                    &mut self.d,
                    &EditSketch {
                        element: self.el,
                        feature: f,
                        op,
                    },
                )
                .unwrap();
        }
        f
    }

    fn frame(&mut self, x: FrameFeature) -> FeatureId {
        let feature = FeatureId::new();
        self.h
            .execute(
                &mut self.d,
                &AddFeature {
                    element: self.el,
                    feature,
                    base_name: "Frame".into(),
                    kind: FeatureKind::Frame(x),
                },
            )
            .unwrap();
        feature
    }

    fn add(&mut self, base: &str, kind: FeatureKind) -> FeatureId {
        let feature = FeatureId::new();
        self.h
            .execute(
                &mut self.d,
                &AddFeature {
                    element: self.el,
                    feature,
                    base_name: base.into(),
                    kind,
                },
            )
            .unwrap();
        feature
    }

    /// A box: the sketch's regions extruded `depth` up from Top.
    fn extrude(&mut self, sketch: FeatureId, depth: f64) -> FeatureId {
        let f = FeatureId::new();
        self.h
            .execute(
                &mut self.d,
                &AddExtrude {
                    element: self.el,
                    feature: f,
                    extrude: ExtrudeFeature::default(),
                },
            )
            .unwrap();
        self.h
            .execute(
                &mut self.d,
                &SetExtrude {
                    element: self.el,
                    feature: f,
                    extrude: ExtrudeFeature {
                        sketches: vec![sketch],
                        depth,
                        depth_expr: format!("{depth} mm"),
                        ..Default::default()
                    },
                    label: "Extrude".into(),
                },
            )
            .unwrap();
        f
    }

    /// How much any two of the parts overlap (mm³): each pair intersected by a Boolean, which
    /// fails when they only touch.
    fn overlap(&mut self) -> f64 {
        let parts: Vec<Part> = self.build().parts.clone();
        let mut total = 0.0;
        for i in 0..parts.len() {
            for j in i + 1..parts.len() {
                let f = self.add(
                    "Boolean",
                    FeatureKind::Boolean(BooleanFeature {
                        op: BooleanKind::Intersect,
                        tools: vec![parts[i].id, parts[j].id],
                        targets: Vec::new(),
                        keep_tools: true,
                        keep_remainder: false,
                        offset: None,
                    }),
                );
                let b = rebuild::build(&self.features());
                if b.error(f).is_none() {
                    total += b
                        .parts
                        .iter()
                        .filter(|p| p.features.contains(&f) && !parts.iter().any(|q| q.id == p.id))
                        .map(volume)
                        .sum::<f64>();
                }
                self.h.undo(&mut self.d);
            }
        }
        total
    }

    fn build(&self) -> std::sync::Arc<rebuild::Build> {
        let t = std::time::Instant::now();
        let b = rebuild::build(&self.features());
        eprintln!("built in {:?}", t.elapsed());
        assert!(b.errors.is_empty(), "rebuild errors: {:?}", b.errors);
        b
    }
}

fn polyline(points: &[(f64, f64)], closed: bool) -> SketchOp {
    SketchOp::AddPolyline {
        points: points.iter().map(|&(x, y)| Vec2::new(x, y)).collect(),
        closed,
        construction: false,
        label: "Add line",
    }
}

fn volume(p: &Part) -> f64 {
    p.mass.as_ref().expect("kernel mass").volume
}

fn total(b: &rebuild::Build) -> f64 {
    b.parts.iter().map(volume).sum()
}

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1e-9)
}

/// The area of an EN 10219 square tube: corners of 2t outside, t inside.
fn square_tube_area(b: f64, t: f64) -> f64 {
    let k = 4.0 - PI;
    (b * b - k * (2.0 * t).powi(2)) - ((b - 2.0 * t).powi(2) - k * t * t)
}

/// A square tube frame, mitred.
fn tube(size: &str) -> FrameFeature {
    let mut x = FrameFeature::default();
    x.set_family(ProfileFamily::SquareTube);
    x.size = size.into();
    x.corner = CornerType::Miter;
    x
}

/// A 400 × 300 rectangle on Top, its corners at (0, 0) and (400, 300).
fn rectangle(st: &mut Studio) -> FeatureId {
    st.sketch(
        PlaneRef::Top,
        vec![polyline(
            &[(0.0, 0.0), (400.0, 0.0), (400.0, 300.0), (0.0, 300.0)],
            true,
        )],
    )
}

#[test]
fn a_mitred_rectangle_is_its_section_along_its_centre_line() {
    let mut st = Studio::new();
    let sk = rectangle(&mut st);
    let mut x = tube("40x40x3");
    x.path = vec![PathRef::Sketch(sk)];
    st.frame(x);
    let b = st.build();
    assert_eq!(b.parts.len(), 4);
    let a = square_tube_area(40.0, 3.0);
    // A centred, symmetric section mitred at right angles: what one member's outer corner gains
    // its inner corner loses.
    let want = a * 1400.0;
    assert!(close(total(&b), want, 1e-6), "{} vs {want}", total(&b));
    // The cut list: two 440s and two 340s, mitred 45° at both ends.
    assert_eq!(b.frames.len(), 4);
    let rows = frame::cut_list(&b.frames);
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert!((rows[0].length - 440.0).abs() < 1e-6, "{rows:?}");
    assert!((rows[1].length - 340.0).abs() < 1e-6, "{rows:?}");
    for r in &rows {
        assert_eq!(r.quantity(), 2);
        assert_eq!(r.profile, "Square tube 40x40x3");
        for e in r.ends {
            assert!(
                matches!(e, EndCut::Angle(a) if (a - 45.0).abs() < 1e-6),
                "{e:?}"
            );
        }
    }
}

#[test]
fn butted_corners_keep_the_volume_and_square_ends() {
    let mut st = Studio::new();
    let sk = rectangle(&mut st);
    let mut x = tube("40x40x3");
    x.path = vec![PathRef::Sketch(sk)];
    x.corner = CornerType::Butt;
    st.frame(x);
    let b = st.build();
    assert_eq!(b.parts.len(), 4);
    let a = square_tube_area(40.0, 3.0);
    // Each corner: one member runs on 20 past the corner, the other stops 20 short.
    assert!(close(total(&b), a * 1400.0, 1e-6), "{}", total(&b));
    let lengths: f64 = b.frames.iter().map(|m| m.length).sum();
    assert!((lengths - 1400.0).abs() < 1e-6, "{lengths}");
    for m in &b.frames {
        assert_eq!(m.ends, [EndCut::Square, EndCut::Square], "{m:?}");
        // Every member is 400 ± 40 or 300 ± 40 or exactly as drawn.
        let ok = [360.0, 400.0, 440.0, 260.0, 300.0, 340.0]
            .iter()
            .any(|l| (m.length - l).abs() < 1e-6);
        assert!(ok, "{}", m.length);
    }
}

#[test]
fn a_corner_override_butts_one_corner_of_a_mitred_frame() {
    let mut st = Studio::new();
    let sk = rectangle(&mut st);
    let corner = {
        let el = st.d.element(st.el).unwrap();
        let g = &el.feature(sk).unwrap().sketch().unwrap().geometry;
        g.points
            .iter()
            .find(|(_, p)| p.pos.x.abs() < 1e-9 && p.pos.y.abs() < 1e-9)
            .map(|(id, _)| id)
            .expect("the origin corner")
    };
    let mut x = tube("40x40x3");
    x.path = vec![PathRef::Sketch(sk)];
    x.overrides = vec![CornerOverride {
        point: Some(SketchPointRef {
            sketch: sk,
            point: corner,
        }),
        at: [0.0, 0.0, 0.0],
        corner: CornerType::Butt,
        flip: false,
    }];
    st.frame(x);
    let b = st.build();
    assert!(b.warnings.is_empty(), "{:?}", b.warnings);
    assert_eq!(b.parts.len(), 4);
    let a = square_tube_area(40.0, 3.0);
    assert!(close(total(&b), a * 1400.0, 1e-6));
    let squares = b
        .frames
        .iter()
        .flat_map(|m| m.ends)
        .filter(|e| *e == EndCut::Square)
        .count();
    assert_eq!(squares, 2, "{:?}", b.frames);
}

#[test]
fn an_override_away_from_the_corners_warns() {
    let mut st = Studio::new();
    let sk = rectangle(&mut st);
    let mut x = tube("40x40x3");
    x.path = vec![PathRef::Sketch(sk)];
    x.overrides = vec![CornerOverride {
        point: None,
        at: [55.0, 0.0, 0.0],
        corner: CornerType::Butt,
        flip: false,
    }];
    let f = st.frame(x);
    let b = rebuild::build(&st.features());
    assert!(b.errors.is_empty());
    assert!(b.warning(f).is_some_and(|w| w.contains("corner override")));
}

#[test]
fn a_tee_butts_against_the_member_it_ends_on() {
    // A 400 bar along X, and a 300 one up from its middle.
    let mut st = Studio::new();
    let sk = st.sketch(
        PlaneRef::Top,
        vec![
            polyline(&[(0.0, 0.0), (400.0, 0.0)], false),
            polyline(&[(200.0, 0.0), (200.0, 300.0)], false),
        ],
    );
    let mut x = FrameFeature::default();
    x.set_family(ProfileFamily::RoundTube);
    x.size = "48.3x3.2".into();
    x.path = vec![PathRef::Sketch(sk)];
    x.corner = CornerType::Butt;
    st.frame(x);
    let b = st.build();
    assert_eq!(b.parts.len(), 2);
    let (ro, ri) = (24.15, 24.15 - 3.2);
    let a = PI * (ro * ro - ri * ri);
    let mut v: Vec<f64> = b.parts.iter().map(volume).collect();
    v.sort_by(f64::total_cmp);
    // The stem stops at the bar's side, its full radius from the bar's axis.
    assert!(
        close(v[0], a * (300.0 - ro), 1e-6),
        "{} vs {}",
        v[0],
        a * (300.0 - ro)
    );
    assert!(close(v[1], a * 400.0, 1e-6));
}

#[test]
fn a_coped_tee_saddles_round_the_bar() {
    let mut st = Studio::new();
    let sk = st.sketch(
        PlaneRef::Top,
        vec![
            polyline(&[(0.0, 0.0), (400.0, 0.0)], false),
            polyline(&[(200.0, 0.0), (200.0, 300.0)], false),
        ],
    );
    let mut x = FrameFeature::default();
    x.set_family(ProfileFamily::RoundTube);
    x.size = "48.3x3.2".into();
    x.path = vec![PathRef::Sketch(sk)];
    x.corner = CornerType::CopedButt;
    st.frame(x);
    let b = st.build();
    assert_eq!(b.parts.len(), 2);
    let (ro, ri) = (24.15f64, 24.15 - 3.2);
    let a = PI * (ro * ro - ri * ri);
    // The stem's annulus (x across the bar, z up) runs from the bar's surface
    // y = sqrt(ro² − x²) to 300.
    let n = 400;
    let mut saddle = 0.0;
    for i in 0..n {
        let r = ri + (ro - ri) * (i as f64 + 0.5) / n as f64;
        let dr = (ro - ri) / n as f64;
        for j in 0..n {
            let t = 2.0 * PI * (j as f64 + 0.5) / n as f64;
            let xx = r * t.cos();
            saddle += (ro * ro - xx * xx).max(0.0).sqrt() * r * dr * 2.0 * PI / n as f64;
        }
    }
    let want = a * 300.0 - saddle;
    let mut v: Vec<f64> = b.parts.iter().map(volume).collect();
    v.sort_by(f64::total_cmp);
    assert!(close(v[0], want, 2e-4), "{} vs {want}", v[0]);
    assert!(close(v[1], a * 400.0, 1e-6));
    let stem = b.frames.iter().find(|m| m.length < 350.0).unwrap();
    assert!(stem.ends.contains(&EndCut::Coped), "{stem:?}");
}

#[test]
fn three_members_at_a_corner_butt_against_the_first() {
    // Along X and Y on Top, and up Z on Front, all from the origin.
    let mut st = Studio::new();
    let a = st.sketch(
        PlaneRef::Top,
        vec![polyline(&[(0.0, 0.0), (300.0, 0.0)], false)],
    );
    let b2 = st.sketch(
        PlaneRef::Top,
        vec![polyline(&[(0.0, 0.0), (0.0, 250.0)], false)],
    );
    let c = st.sketch(
        PlaneRef::Front,
        vec![polyline(&[(0.0, 0.0), (0.0, 200.0)], false)],
    );
    let mut x = tube("40x40x3");
    x.path = vec![PathRef::Sketch(a), PathRef::Sketch(b2), PathRef::Sketch(c)];
    st.frame(x);
    let b = st.build();
    assert_eq!(b.parts.len(), 3);
    let area = square_tube_area(40.0, 3.0);
    let want = area * ((300.0 + 20.0) + (250.0 - 20.0) + (200.0 - 20.0));
    assert!(close(total(&b), want, 1e-6), "{} vs {want}", total(&b));
}

#[test]
fn tangent_curves_merge_into_one_bent_member() {
    // An L with its corner rounded: 200 along X, a quarter of radius 50, 150 up.
    let ops = vec![
        polyline(&[(0.0, 0.0), (200.0, 0.0)], false),
        SketchOp::AddArc {
            center: Vec2::new(200.0, 50.0),
            start: Vec2::new(200.0, 0.0),
            end: Vec2::new(250.0, 50.0),
            construction: false,
        },
        polyline(&[(250.0, 50.0), (250.0, 200.0)], false),
    ];
    let centre = 200.0 + PI * 50.0 / 2.0 + 150.0;
    let mut x = FrameFeature::default();
    x.set_family(ProfileFamily::RoundBar);
    x.size = "20".into();
    let a = PI * 100.0;
    for merge in [true, false] {
        let mut st = Studio::new();
        let sk = st.sketch(PlaneRef::Top, ops.clone());
        let mut y = x.clone();
        y.path = vec![PathRef::Sketch(sk)];
        y.merge_tangent = merge;
        st.frame(y);
        let b = st.build();
        assert_eq!(b.parts.len(), if merge { 1 } else { 3 });
        assert!(
            close(total(&b), a * centre, 1e-5),
            "{} vs {}",
            total(&b),
            a * centre
        );
        if merge {
            assert!(b.frames[0].curved);
            assert!((b.frames[0].length - centre).abs() < 1e-6);
        }
    }
}

#[test]
fn collinear_lines_are_one_straight_member() {
    let mut st = Studio::new();
    let sk = st.sketch(
        PlaneRef::Top,
        vec![
            polyline(&[(0.0, 0.0), (100.0, 0.0)], false),
            polyline(&[(100.0, 0.0), (250.0, 0.0)], false),
        ],
    );
    let mut x = tube("20x20x2");
    x.path = vec![PathRef::Sketch(sk)];
    st.frame(x);
    let b = st.build();
    assert_eq!(b.parts.len(), 1);
    assert!(!b.frames[0].curved);
    assert!((b.frames[0].length - 250.0).abs() < 1e-6);
}

#[test]
fn the_anchor_sets_the_section_on_the_path() {
    let mut st = Studio::new();
    let sk = st.sketch(
        PlaneRef::Top,
        vec![polyline(&[(0.0, 0.0), (100.0, 0.0)], false)],
    );
    let mut x = FrameFeature::default();
    x.set_family(ProfileFamily::FlatBar);
    x.size = "40x5".into();
    x.anchor = Anchor::Bottom;
    x.path = vec![PathRef::Sketch(sk)];
    st.frame(x);
    let b = st.build();
    let p = &b.parts[0];
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for q in &p.solid.positions {
        for i in 0..3 {
            lo[i] = lo[i].min(q[i]);
            hi[i] = hi[i].max(q[i]);
        }
    }
    // Up is the sketch's normal (Z), across is Y: a 40 × 5 bar standing on the sketch plane,
    // 40 across.
    assert!(
        lo[2].abs() < 1e-4 && (hi[2] - 5.0).abs() < 1e-4,
        "{lo:?} {hi:?}"
    );
    assert!((hi[1] - lo[1] - 40.0).abs() < 1e-4, "{lo:?} {hi:?}");
    assert!(close(volume(p), 200.0 * 100.0, 1e-9));
    // Turned a quarter: 40 high.
    let mut st = Studio::new();
    let sk = st.sketch(
        PlaneRef::Top,
        vec![polyline(&[(0.0, 0.0), (100.0, 0.0)], false)],
    );
    let mut x = FrameFeature::default();
    x.set_family(ProfileFamily::FlatBar);
    x.size = "40x5".into();
    x.angle = 90.0;
    x.path = vec![PathRef::Sketch(sk)];
    st.frame(x);
    let b = st.build();
    let p = &b.parts[0];
    let (mut lo, mut hi) = (f64::MAX, f64::MIN);
    for q in &p.solid.positions {
        lo = lo.min(q[2]);
        hi = hi.max(q[2]);
    }
    assert!((hi - lo - 40.0).abs() < 1e-4);
}

#[test]
fn a_custom_profile_is_its_sketch_regions() {
    let mut st = Studio::new();
    let profile = st.sketch(
        PlaneRef::Front,
        vec![polyline(
            &[(-5.0, -10.0), (5.0, -10.0), (5.0, 10.0), (-5.0, 10.0)],
            true,
        )],
    );
    let path = st.sketch(
        PlaneRef::Top,
        vec![polyline(&[(0.0, 0.0), (150.0, 0.0), (150.0, 100.0)], false)],
    );
    let mut x = FrameFeature::default();
    x.set_family(ProfileFamily::Custom);
    x.sketch = Some(profile);
    x.path = vec![PathRef::Sketch(path)];
    st.frame(x);
    let b = st.build();
    assert_eq!(b.parts.len(), 2);
    // 10 wide by 20 up, mitred at right angles about its middle: its section along 250.
    assert!(close(total(&b), 200.0 * 250.0, 1e-6), "{}", total(&b));
    assert!(b.frames[0].profile.starts_with("Custom ("));
}

#[test]
fn every_standard_size_builds_a_straight_member() {
    for family in ProfileFamily::ALL {
        if family == ProfileFamily::Custom {
            continue;
        }
        for size in frame::size_names(family) {
            let mut st = Studio::new();
            let sk = st.sketch(
                PlaneRef::Top,
                vec![polyline(&[(0.0, 0.0), (0.0, 120.0), (90.0, 120.0)], false)],
            );
            let mut x = FrameFeature::default();
            x.set_family(family);
            x.size = size.into();
            x.path = vec![PathRef::Sketch(sk)];
            st.frame(x);
            let b = rebuild::build(&st.features());
            assert!(b.errors.is_empty(), "{family:?} {size}: {:?}", b.errors);
            assert_eq!(b.parts.len(), 2, "{family:?} {size}");
        }
    }
}

#[test]
fn a_missing_path_is_reported() {
    let mut st = Studio::new();
    let x = tube("40x40x3");
    let f = st.frame(x);
    let b = rebuild::build(&st.features());
    assert!(b.error(f).is_some_and(|e| e.contains("Select")));
}

/// A part's box from its mesh: (min, max).
fn bounds(p: &Part) -> ([f64; 3], [f64; 3]) {
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for q in &p.solid.positions {
        for i in 0..3 {
            lo[i] = lo[i].min(q[i]);
            hi[i] = hi[i].max(q[i]);
        }
    }
    (lo, hi)
}

#[test]
fn end_caps_close_a_tubes_free_ends() {
    let mut st = Studio::new();
    let sk = st.sketch(
        PlaneRef::Top,
        vec![polyline(&[(0.0, 0.0), (200.0, 0.0)], false)],
    );
    let mut x = tube("40x40x3");
    x.path = vec![PathRef::Sketch(sk)];
    x.end_caps = true;
    x.cap_thickness = 4.0;
    st.frame(x);
    let b = st.build();
    assert_eq!(b.parts.len(), 3);
    let outer = 1600.0 - (4.0 - PI) * 36.0;
    let want = square_tube_area(40.0, 3.0) * 200.0 + 2.0 * outer * 4.0;
    assert!(close(total(&b), want, 1e-6), "{} vs {want}", total(&b));
    let caps: Vec<&Part> = b
        .parts
        .iter()
        .filter(|p| {
            b.frames
                .iter()
                .any(|m| m.part == p.id && m.plate && m.profile.starts_with("End cap 4 mm"))
        })
        .collect();
    assert_eq!(caps.len(), 2);
    let mut xs: Vec<(f64, f64)> = caps
        .iter()
        .map(|p| {
            let (lo, hi) = bounds(p);
            (lo[0], hi[0])
        })
        .collect();
    xs.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert!(
        (xs[0].0 + 4.0).abs() < 1e-4 && xs[0].1.abs() < 1e-4,
        "{xs:?}"
    );
    assert!(
        (xs[1].0 - 200.0).abs() < 1e-4 && (xs[1].1 - 204.0).abs() < 1e-4,
        "{xs:?}"
    );
    // Plates are listed apart from the stock.
    let rows = frame::cut_list(&b.frames);
    assert_eq!(frame::totals(&rows).len(), 1);
    assert!(rows.iter().any(|r| r.plate && r.quantity() == 2));
}

#[test]
fn a_gusset_fills_the_inside_of_a_corner() {
    let mut st = Studio::new();
    let sk = st.sketch(
        PlaneRef::Top,
        vec![polyline(&[(300.0, 0.0), (0.0, 0.0), (0.0, 200.0)], false)],
    );
    let mut x = tube("40x40x3");
    x.path = vec![PathRef::Sketch(sk)];
    x.gussets = true;
    x.gusset_size = 60.0;
    x.gusset_thickness = 5.0;
    st.frame(x);
    let b = st.build();
    assert_eq!(b.parts.len(), 3);
    let g = b
        .parts
        .iter()
        .find(|p| b.frames.iter().any(|m| m.part == p.id && m.plate))
        .expect("the gusset");
    assert!(
        close(volume(g), 0.5 * 60.0 * 60.0 * 5.0, 1e-9),
        "{}",
        volume(g)
    );
    let (lo, hi) = bounds(g);
    // From the tubes' inner sides at (20, 20), 60 along each, about the tubes' middle.
    for (v, w) in [(lo[0], 20.0), (lo[1], 20.0), (hi[0], 80.0), (hi[1], 80.0)] {
        assert!((v - w).abs() < 1e-4, "{lo:?} {hi:?}");
    }
    assert!((lo[2] + 2.5).abs() < 1e-4 && (hi[2] - 2.5).abs() < 1e-4);
    // At an angle: the legs still lie along the members' inner sides.
    let mut st = Studio::new();
    let a = 60f64.to_radians();
    let sk = st.sketch(
        PlaneRef::Top,
        vec![polyline(
            &[(300.0, 0.0), (0.0, 0.0), (200.0 * a.cos(), 200.0 * a.sin())],
            false,
        )],
    );
    let mut x = tube("40x40x3");
    x.path = vec![PathRef::Sketch(sk)];
    x.gussets = true;
    st.frame(x);
    let b = st.build();
    let g = b
        .parts
        .iter()
        .find(|p| b.frames.iter().any(|m| m.part == p.id && m.plate))
        .expect("the gusset");
    let s = x_gusset_size();
    assert!(close(volume(g), 0.5 * s * s * a.sin() * 5.0, 1e-9));
}

fn x_gusset_size() -> f64 {
    FrameFeature::default().gusset_size
}

#[test]
fn a_crossbar_in_two_halves_is_one_run_through_a_tee() {
    // The bar drawn as two lines meeting at the stem, not merged: the halves end square at
    // the stem's axis, the stem is cut against their side.
    let mut st = Studio::new();
    let sk = st.sketch(
        PlaneRef::Top,
        vec![
            polyline(&[(0.0, 0.0), (200.0, 0.0)], false),
            polyline(&[(200.0, 0.0), (400.0, 0.0)], false),
            polyline(&[(200.0, 0.0), (200.0, 300.0)], false),
        ],
    );
    let mut x = tube("40x40x3");
    x.corner = CornerType::Butt;
    x.merge_tangent = false;
    x.path = vec![PathRef::Sketch(sk)];
    st.frame(x);
    let b = st.build();
    assert_eq!(b.parts.len(), 3);
    let a = square_tube_area(40.0, 3.0);
    assert!(close(total(&b), a * (400.0 + 280.0), 1e-6), "{}", total(&b));
    assert!(st.overlap() < 1e-6);
}

#[test]
fn a_cross_runs_one_bar_through() {
    let mut st = Studio::new();
    let sk = st.sketch(
        PlaneRef::Top,
        vec![
            polyline(&[(0.0, 0.0), (200.0, 0.0)], false),
            polyline(&[(200.0, 0.0), (400.0, 0.0)], false),
            polyline(&[(200.0, -150.0), (200.0, 0.0)], false),
            polyline(&[(200.0, 0.0), (200.0, 150.0)], false),
        ],
    );
    let mut x = tube("40x40x3");
    x.corner = CornerType::Butt;
    x.merge_tangent = false;
    x.path = vec![PathRef::Sketch(sk)];
    st.frame(x);
    let b = st.build();
    assert_eq!(b.parts.len(), 4);
    let a = square_tube_area(40.0, 3.0);
    assert!(
        close(total(&b), a * (400.0 + 2.0 * 130.0), 1e-6),
        "{}",
        total(&b)
    );
}

#[test]
fn reversed_curves_keep_an_offset_section_on_one_side() {
    // Lines drawn every which way; the section to the left of the first one's way (inside the
    // rectangle once they run head to tail): its centroid runs 20 inside, round 1400 − 8·20.
    let mut st = Studio::new();
    let sk = st.sketch(
        PlaneRef::Top,
        vec![
            polyline(&[(0.0, 0.0), (400.0, 0.0)], false),
            polyline(&[(400.0, 300.0), (400.0, 0.0)], false),
            polyline(&[(400.0, 300.0), (0.0, 300.0)], false),
            polyline(&[(0.0, 0.0), (0.0, 300.0)], false),
        ],
    );
    let mut x = tube("40x40x3");
    x.anchor = Anchor::Left;
    x.path = vec![PathRef::Sketch(sk)];
    st.frame(x);
    let b = st.build();
    assert_eq!(b.parts.len(), 4);
    let a = square_tube_area(40.0, 3.0);
    assert!(
        close(total(&b), a * 1240.0, 1e-6),
        "{} vs {}",
        total(&b),
        a * 1240.0
    );
    assert!(st.overlap() < 1e-6);
    // Every member inside the rectangle.
    for p in &b.parts {
        let (lo, hi) = bounds(p);
        assert!(lo[0] > -1e-4 && lo[1] > -1e-4 && hi[0] < 400.0 + 1e-4 && hi[1] < 300.0 + 1e-4);
    }
}

#[test]
fn butts_at_other_angles_close_without_overlap() {
    for deg in [60.0f64, 120.0] {
        let mut st = Studio::new();
        let t = deg.to_radians();
        let sk = st.sketch(
            PlaneRef::Top,
            vec![polyline(
                &[(300.0, 0.0), (0.0, 0.0), (250.0 * t.cos(), 250.0 * t.sin())],
                false,
            )],
        );
        let mut x = tube("40x40x3");
        x.corner = CornerType::Butt;
        x.path = vec![PathRef::Sketch(sk)];
        st.frame(x);
        let b = st.build();
        assert_eq!(b.parts.len(), 2);
        let a = square_tube_area(40.0, 3.0);
        // The shorter is cut on the longer's side (20 off its axis), a plane cut keeping its
        // section's area along its axis from where that meets the plane; the longer runs on to
        // cover the cut end's furthest corner along it.
        let cut = 20.0 / t.sin();
        let mut v: Vec<f64> = b.parts.iter().map(volume).collect();
        v.sort_by(f64::total_cmp);
        assert!(
            close(v[0], a * (250.0 - cut), 1e-6),
            "{deg}: {} vs {}",
            v[0],
            a * (250.0 - cut)
        );
        // The longer runs on to the cut end's furthest corner along it: 20 (1 − cos θ) / sin θ.
        let w = 20.0 * (1.0 - t.cos()) / t.sin();
        let long = b.frames.iter().map(|m| m.length).fold(0.0, f64::max);
        assert!(
            (long - (300.0 + w)).abs() < 1e-4,
            "{deg}: {long} vs {}",
            300.0 + w
        );
        assert!(st.overlap() < 1e-6, "{deg}°");
    }
}

#[test]
fn a_custom_section_of_two_regions_keeps_both_at_its_cuts() {
    let mut st = Studio::new();
    let profile = st.sketch(
        PlaneRef::Front,
        vec![
            polyline(
                &[(-15.0, -5.0), (-5.0, -5.0), (-5.0, 5.0), (-15.0, 5.0)],
                true,
            ),
            polyline(&[(5.0, -5.0), (15.0, -5.0), (15.0, 5.0), (5.0, 5.0)], true),
        ],
    );
    let path = st.sketch(
        PlaneRef::Top,
        vec![polyline(&[(0.0, 0.0), (150.0, 0.0), (150.0, 100.0)], false)],
    );
    for corner in [CornerType::Miter, CornerType::Butt] {
        let mut x = FrameFeature::default();
        x.set_family(ProfileFamily::Custom);
        x.sketch = Some(profile);
        x.corner = corner;
        x.path = vec![PathRef::Sketch(path)];
        let f = st.frame(x);
        let b = st.build();
        assert_eq!(b.parts.len(), 2);
        // Two 10 × 10 bars either side of the path: what one gains at a corner the other loses.
        assert!(
            close(total(&b), 200.0 * 250.0, 1e-6),
            "{corner:?}: {}",
            total(&b)
        );
        let _ = f;
        st.h.undo(&mut st.d);
    }
}

#[test]
fn a_mitred_round_tube_is_listed_by_its_long_point() {
    let mut st = Studio::new();
    let sk = st.sketch(
        PlaneRef::Top,
        vec![polyline(&[(300.0, 0.0), (0.0, 0.0), (0.0, 200.0)], false)],
    );
    let mut x = FrameFeature::default();
    x.set_family(ProfileFamily::RoundTube);
    x.size = "48.3x3.2".into();
    x.corner = CornerType::Miter;
    x.path = vec![PathRef::Sketch(sk)];
    st.frame(x);
    let b = st.build();
    let mut l: Vec<f64> = b.frames.iter().map(|m| m.length).collect();
    l.sort_by(f64::total_cmp);
    assert!(
        (l[0] - 224.15).abs() < 2e-3 && (l[1] - 324.15).abs() < 2e-3,
        "{l:?}"
    );
}

#[test]
fn a_coped_t_slot_stem_stops_at_the_bars_outline() {
    // Coped against a T-slot extrusion, the stem is cut to its box (no tongues left in its
    // slot): the same as butted against it.
    let mut v = Vec::new();
    for corner in [CornerType::Butt, CornerType::CopedButt] {
        let mut st = Studio::new();
        let sk = st.sketch(
            PlaneRef::Top,
            vec![
                polyline(&[(0.0, 0.0), (400.0, 0.0)], false),
                polyline(&[(200.0, 0.0), (200.0, 300.0)], false),
            ],
        );
        st.frame(FrameFeature {
            corner,
            path: vec![PathRef::Sketch(sk)],
            ..Default::default()
        });
        let b = st.build();
        assert_eq!(b.parts.len(), 2, "{corner:?}");
        let mut vs: Vec<f64> = b.parts.iter().map(volume).collect();
        vs.sort_by(f64::total_cmp);
        v.push(vs[0]);
    }
    assert!(close(v[0], v[1], 1e-6), "{v:?}");
}

#[test]
fn a_box_edge_loop_frames_with_one_up() {
    // A 60 × 40 tube on a box's top edges: the members run head to tail with the loop's
    // normal as their up, so each is 40 high (not 60 on two of them).
    let mut st = Studio::new();
    let base = rectangle(&mut st);
    st.extrude(base, 50.0);
    let b = st.build();
    let part = &b.parts[0];
    let path: Vec<PathRef> = part
        .solid
        .edges
        .iter()
        .filter(|e| e.points.iter().all(|p| (p[2] - 50.0).abs() < 1e-6))
        .map(|e| {
            PathRef::Edge(EdgeRef {
                part: part.id,
                edge: e.name,
                seed: e.midpoint(),
            })
        })
        .collect();
    assert_eq!(path.len(), 4);
    let mut x = FrameFeature::default();
    x.set_family(ProfileFamily::RectTube);
    x.size = "60x40x3".into();
    x.corner = CornerType::Miter;
    x.path = path;
    let f = st.frame(x);
    let b = st.build();
    let members: Vec<&Part> = b.parts.iter().filter(|p| p.feature == f).collect();
    assert_eq!(members.len(), 4);
    for p in members {
        let (lo, hi) = bounds(p);
        assert!((hi[2] - lo[2] - 40.0).abs() < 1e-4, "{lo:?} {hi:?}");
    }
}

#[test]
fn flip_runs_the_next_longest_on_at_a_corner_of_three() {
    let mut st = Studio::new();
    let a = st.sketch(
        PlaneRef::Top,
        vec![polyline(&[(0.0, 0.0), (300.0, 0.0)], false)],
    );
    let b2 = st.sketch(
        PlaneRef::Top,
        vec![polyline(&[(0.0, 0.0), (0.0, 250.0)], false)],
    );
    let c = st.sketch(
        PlaneRef::Front,
        vec![polyline(&[(0.0, 0.0), (0.0, 200.0)], false)],
    );
    let mut x = tube("40x40x3");
    x.flip = true;
    x.path = vec![PathRef::Sketch(a), PathRef::Sketch(b2), PathRef::Sketch(c)];
    st.frame(x);
    let b = st.build();
    let area = square_tube_area(40.0, 3.0);
    let want = area * ((300.0 - 20.0) + (250.0 + 20.0) + (200.0 - 20.0));
    assert!(close(total(&b), want, 1e-6), "{} vs {want}", total(&b));
}

#[test]
fn a_circle_is_one_closed_member() {
    let mut st = Studio::new();
    let sk = st.sketch(
        PlaneRef::Top,
        vec![SketchOp::AddCircle {
            center: Vec2::new(0.0, 0.0),
            radius: 100.0,
            construction: false,
        }],
    );
    let mut x = FrameFeature::default();
    x.set_family(ProfileFamily::RoundBar);
    x.size = "20".into();
    x.path = vec![PathRef::Sketch(sk)];
    st.frame(x);
    let b = st.build();
    assert_eq!(b.parts.len(), 1);
    let want = PI * 100.0 * 2.0 * PI * 100.0;
    assert!(close(total(&b), want, 1e-5), "{} vs {want}", total(&b));
    assert!(b.frames[0].curved);
}

#[test]
fn a_member_is_trimmed_by_a_part() {
    // A bar from x = 0 to 400 through a 40-wide wall at x 300..340: what is left of x < 300
    // is kept.
    let mut st = Studio::new();
    let wall = st.sketch(
        PlaneRef::Top,
        vec![polyline(
            &[
                (300.0, -100.0),
                (340.0, -100.0),
                (340.0, 100.0),
                (300.0, 100.0),
            ],
            true,
        )],
    );
    let e = st.extrude(wall, 50.0);
    let sk = st.sketch(
        PlaneRef::Top,
        vec![polyline(&[(0.0, 0.0), (400.0, 0.0)], false)],
    );
    let wall_part = st.build().parts.iter().find(|p| p.feature == e).unwrap().id;
    let mut x = FrameFeature::default();
    x.set_family(ProfileFamily::SquareBar);
    x.size = "20".into();
    x.anchor = Anchor::Bottom;
    x.path = vec![PathRef::Sketch(sk)];
    x.trim = vec![wall_part];
    let f = st.frame(x);
    let b = st.build();
    let m = b.parts.iter().find(|p| p.feature == f).unwrap();
    assert!(close(volume(m), 400.0 * 300.0, 1e-6), "{}", volume(m));
}

#[test]
fn a_frame_survives_saving() {
    let mut x = tube("40x40x3");
    x.end_caps = true;
    x.overrides = vec![CornerOverride {
        point: None,
        at: [1.0, 2.0, 3.0],
        corner: CornerType::CopedButt,
        flip: true,
    }];
    let k = FeatureKind::Frame(x);
    let text = ron::to_string(&k).unwrap();
    let back: FeatureKind = ron::from_str(&text).unwrap();
    assert_eq!(back, k);
    // An older file without the later fields reads with their defaults.
    let old: frame::FrameFeature = ron::from_str("(size: \"20x20\")").unwrap();
    assert!(old.merge_tangent && !old.end_caps && old.trim.is_empty());
}
