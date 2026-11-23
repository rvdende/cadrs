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
    self, Anchor, CornerOverride, CornerType, EndCut, FrameFeature, GussetPosition, ProfileFamily,
    SketchPointRef,
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
    }
    // The long sides run on at both ends, the short ones stop at them.
    let mut l: Vec<f64> = b.frames.iter().map(|m| m.length).collect();
    l.sort_by(f64::total_cmp);
    for (got, want) in l.iter().zip([260.0, 260.0, 440.0, 440.0]) {
        assert!((got - want).abs() < 1e-6, "{l:?}");
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
        turn: 0,
        gusset: None,
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
        turn: 0,
        gusset: None,
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
fn three_members_at_a_corner_join_the_flattest_pair() {
    // The pair lying flat (X and Y) is joined, the longer running on; the post up Z is cut
    // against both.
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
    x.corner = CornerType::Butt;
    x.path = vec![PathRef::Sketch(a), PathRef::Sketch(b2), PathRef::Sketch(c)];
    st.frame(x);
    let b = st.build();
    assert_eq!(b.parts.len(), 3);
    let area = square_tube_area(40.0, 3.0);
    let want = area * ((300.0 + 20.0) + (250.0 - 20.0) + (200.0 - 20.0));
    assert!(close(total(&b), want, 1e-6), "{} vs {want}", total(&b));
    let mut l: Vec<f64> = b.frames.iter().map(|m| m.length).collect();
    l.sort_by(f64::total_cmp);
    for (got, want) in l.iter().zip([180.0, 230.0, 320.0]) {
        assert!((got - want).abs() < 1e-4, "{l:?}");
    }
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
fn flip_swaps_the_runner_within_the_main_pair() {
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
    x.corner = CornerType::Butt;
    x.flip = true;
    x.path = vec![PathRef::Sketch(a), PathRef::Sketch(b2), PathRef::Sketch(c)];
    st.frame(x);
    let b = st.build();
    let area = square_tube_area(40.0, 3.0);
    let want = area * ((300.0 - 20.0) + (250.0 + 20.0) + (200.0 - 20.0));
    assert!(close(total(&b), want, 1e-6), "{} vs {want}", total(&b));
    // Y runs on now, X stops at it; the post as before.
    let mut l: Vec<f64> = b.frames.iter().map(|m| m.length).collect();
    l.sort_by(f64::total_cmp);
    for (got, want) in l.iter().zip([180.0, 270.0, 280.0]) {
        assert!((got - want).abs() < 1e-4, "{l:?}");
    }
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
    // A ring's stock is its centre line.
    assert!(
        (b.frames[0].length - 2.0 * PI * 100.0).abs() < 1e-6,
        "{}",
        b.frames[0].length
    );
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
        turn: 0,
        gusset: None,
    }];
    let k = FeatureKind::Frame(x);
    let text = ron::to_string(&k).unwrap();
    let back: FeatureKind = ron::from_str(&text).unwrap();
    assert_eq!(back, k);
    // An older file without the later fields reads with their defaults.
    let old: frame::FrameFeature = ron::from_str("(size: \"20x20\")").unwrap();
    assert!(old.merge_tangent && !old.end_caps && old.trim.is_empty());
}

/// The sketch point of `sketch` at (x, y).
fn point_at(st: &Studio, sketch: FeatureId, x: f64, y: f64) -> SketchPointRef {
    let el = st.d.element(st.el).unwrap();
    let g = &el.feature(sketch).unwrap().sketch().unwrap().geometry;
    let point = g
        .points
        .iter()
        .find(|(_, p)| (p.pos.x - x).abs() < 1e-9 && (p.pos.y - y).abs() < 1e-9)
        .map(|(id, _)| id)
        .expect("a sketch point there");
    SketchPointRef { sketch, point }
}

#[test]
fn a_square_butted_frame_pairs_its_opposite_sides() {
    // All four as long: the two along the first run on at both ends, the other two stop short
    // at both, so the cut list has two pairs (not three lengths).
    let mut st = Studio::new();
    let sk = st.sketch(
        PlaneRef::Top,
        vec![polyline(
            &[(0.0, 0.0), (300.0, 0.0), (300.0, 300.0), (0.0, 300.0)],
            true,
        )],
    );
    let mut x = tube("40x40x3");
    x.corner = CornerType::Butt;
    x.path = vec![PathRef::Sketch(sk)];
    st.frame(x);
    let b = st.build();
    let rows = frame::cut_list(&b.frames);
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert!((rows[0].length - 340.0).abs() < 1e-6 && rows[0].quantity() == 2);
    assert!((rows[1].length - 260.0).abs() < 1e-6 && rows[1].quantity() == 2);
}

#[test]
fn t_slot_inside_a_rectangle_butts_without_gaps_or_overlaps() {
    // 20x20 standing inside the sketch's rectangle (its left side on the lines): the long sides
    // run to the corners, the short ones stop at their inner faces: 2·400 + 2·(300 − 2·20) of
    // section in all.
    let area = {
        let mut st = Studio::new();
        let sk = st.sketch(
            PlaneRef::Top,
            vec![polyline(&[(0.0, 0.0), (100.0, 0.0)], false)],
        );
        st.frame(FrameFeature {
            path: vec![PathRef::Sketch(sk)],
            ..Default::default()
        });
        total(&st.build()) / 100.0
    };
    let mut st = Studio::new();
    let sk = rectangle(&mut st);
    st.frame(FrameFeature {
        path: vec![PathRef::Sketch(sk)],
        anchor: Anchor::Left,
        ..Default::default()
    });
    let b = st.build();
    assert_eq!(b.parts.len(), 4);
    assert!(
        close(total(&b), area * 1320.0, 1e-6),
        "{} vs {}",
        total(&b),
        area * 1320.0
    );
    assert!(st.overlap() < 1e-6);
}

#[test]
fn a_diagonal_brace_butts_and_copes_into_the_frame() {
    for corner in [CornerType::Butt, CornerType::CopedButt] {
        let mut st = Studio::new();
        let sk = st.sketch(
            PlaneRef::Top,
            vec![
                polyline(
                    &[(0.0, 0.0), (400.0, 0.0), (400.0, 300.0), (0.0, 300.0)],
                    true,
                ),
                polyline(&[(0.0, 0.0), (400.0, 150.0)], false),
            ],
        );
        let mut x = FrameFeature::default();
        x.set_family(ProfileFamily::RoundTube);
        x.size = "33.7x3.2".into();
        x.corner = corner;
        x.path = vec![PathRef::Sketch(sk)];
        st.frame(x);
        let b = st.build();
        assert_eq!(b.parts.len(), 5, "{corner:?}");
        assert!(st.overlap() < 1e-6, "{corner:?}");
    }
}

#[test]
fn a_coped_corner_of_two_tubes_has_no_overlap() {
    let mut st = Studio::new();
    let sk = st.sketch(
        PlaneRef::Top,
        vec![polyline(&[(300.0, 0.0), (0.0, 0.0), (0.0, 200.0)], false)],
    );
    let mut x = FrameFeature::default();
    x.set_family(ProfileFamily::RoundTube);
    x.size = "48.3x3.2".into();
    x.corner = CornerType::CopedButt;
    x.path = vec![PathRef::Sketch(sk)];
    st.frame(x);
    let b = st.build();
    assert_eq!(b.parts.len(), 2);
    assert!(st.overlap() < 1e-6);
    // The shorter is coped onto the longer, which runs on to cover it.
    let short = b.frames.iter().find(|m| m.length < 250.0).unwrap();
    assert!(short.ends.contains(&EndCut::Coped));
}

#[test]
fn a_cube_of_box_edges_meets_three_at_each_corner() {
    // Each corner: one runs on half the section past it, two stop half a section short.
    let mut st = Studio::new();
    let base = st.sketch(
        PlaneRef::Top,
        vec![polyline(
            &[(0.0, 0.0), (200.0, 0.0), (200.0, 200.0), (0.0, 200.0)],
            true,
        )],
    );
    st.extrude(base, 200.0);
    let b = st.build();
    let part = &b.parts[0];
    let path: Vec<PathRef> = part
        .solid
        .edges
        .iter()
        .map(|e| {
            PathRef::Edge(EdgeRef {
                part: part.id,
                edge: e.name,
                seed: e.midpoint(),
            })
        })
        .collect();
    assert_eq!(path.len(), 12);
    let mut x = tube("20x20x2");
    x.corner = CornerType::Butt;
    x.path = path;
    let f = st.frame(x);
    let b = st.build();
    let members: Vec<&Part> = b.parts.iter().filter(|p| p.feature == f).collect();
    assert_eq!(members.len(), 12);
    let want = square_tube_area(20.0, 2.0) * (12.0 * 200.0 - 8.0 * 10.0);
    let got: f64 = members.iter().map(|p| volume(p)).sum();
    assert!(close(got, want, 1e-6), "{got} vs {want}");
    let lengths: f64 = b.frames.iter().map(|m| m.length).sum();
    assert!(
        (lengths - (12.0 * 200.0 - 8.0 * 10.0)).abs() < 1e-3,
        "{lengths}"
    );
}

#[test]
fn a_bent_members_length_reaches_its_long_point() {
    // A rounded L (200, a quarter of 50, 150) then back along 250: mitred at the L's end, the
    // bent member's stock is its centre line and the 20 its outer corner reaches past.
    let mut st = Studio::new();
    let sk = st.sketch(
        PlaneRef::Top,
        vec![
            polyline(&[(0.0, 0.0), (200.0, 0.0)], false),
            SketchOp::AddArc {
                center: Vec2::new(200.0, 50.0),
                start: Vec2::new(200.0, 0.0),
                end: Vec2::new(250.0, 50.0),
                construction: false,
            },
            polyline(&[(250.0, 50.0), (250.0, 200.0), (0.0, 200.0)], false),
        ],
    );
    let mut x = tube("40x40x3");
    x.path = vec![PathRef::Sketch(sk)];
    st.frame(x);
    let b = st.build();
    assert_eq!(b.parts.len(), 2);
    let bent = b.frames.iter().find(|m| m.curved).unwrap();
    let want = 200.0 + PI * 50.0 / 2.0 + 150.0 + 20.0;
    assert!(
        (bent.length - want).abs() < 1e-3,
        "{} vs {want}",
        bent.length
    );
}

#[test]
fn a_trimmed_end_gets_no_cap_and_says_so() {
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
    let mut x = tube("20x20x2");
    x.anchor = Anchor::Bottom;
    x.end_caps = true;
    x.path = vec![PathRef::Sketch(sk)];
    x.trim = vec![wall_part];
    let f = st.frame(x.clone());
    let b = st.build();
    let ours: Vec<&Part> = b.parts.iter().filter(|p| p.feature == f).collect();
    assert_eq!(ours.len(), 2, "the member and one cap");
    let m = b.frames.iter().find(|m| !m.plate).unwrap();
    assert!(m.ends.contains(&EndCut::Trimmed), "{:?}", m.ends);
    assert!((m.length - 300.0).abs() < 1e-3, "{}", m.length);
    // The other side: the 60 past the wall.
    st.h.undo(&mut st.d);
    x.trim_flip = true;
    x.end_caps = false;
    let f = st.frame(x);
    let b = st.build();
    let m = b.parts.iter().find(|p| p.feature == f).unwrap();
    assert!(
        close(volume(m), square_tube_area(20.0, 2.0) * 60.0, 1e-6),
        "{}",
        volume(m)
    );
}

#[test]
fn a_mirrored_turned_angle_frame_meets_without_overlap() {
    let mut st = Studio::new();
    let sk = rectangle(&mut st);
    let mut x = FrameFeature::default();
    x.set_family(ProfileFamily::Angle);
    x.size = "40x40x4".into();
    x.corner = CornerType::Miter;
    x.anchor = Anchor::Origin;
    x.mirror = true;
    x.angle = 90.0;
    x.path = vec![PathRef::Sketch(sk)];
    st.frame(x);
    let b = st.build();
    assert_eq!(b.parts.len(), 4);
    assert!(st.overlap() < 1e-6);
}

#[test]
fn adding_a_curve_keeps_the_other_members_ids() {
    let mut st = Studio::new();
    let a = rectangle(&mut st);
    let extra = st.sketch(
        PlaneRef::Top,
        vec![polyline(&[(0.0, 500.0), (300.0, 500.0)], false)],
    );
    let mut x = tube("40x40x3");
    x.path = vec![PathRef::Sketch(a)];
    st.frame(x.clone());
    let before: Vec<cadrs_core::PartId> = st.build().parts.iter().map(|p| p.id).collect();
    st.h.undo(&mut st.d);
    x.path = vec![PathRef::Sketch(extra), PathRef::Sketch(a)];
    st.frame(x);
    let after: Vec<cadrs_core::PartId> = st.build().parts.iter().map(|p| p.id).collect();
    assert_eq!(after.len(), 5);
    for p in &before {
        assert!(after.iter().any(|q| q.index == p.index), "{p:?} kept");
    }
}

#[test]
fn an_override_copes_a_tee_and_turns_a_three_member_corner() {
    // A tee overridden to Coped butt.
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
    x.overrides = vec![CornerOverride {
        point: Some(point_at(&st, sk, 200.0, 0.0)),
        at: [200.0, 0.0, 0.0],
        corner: CornerType::CopedButt,
        flip: false,
        turn: 0,
        gusset: None,
    }];
    st.frame(x);
    let b = st.build();
    assert!(b.warnings.is_empty(), "{:?}", b.warnings);
    assert!(b.frames.iter().any(|m| m.ends.contains(&EndCut::Coped)));
    // Three at a corner, the shortest made to run on by the override's turn.
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
    x.corner = CornerType::Butt;
    x.path = vec![PathRef::Sketch(a), PathRef::Sketch(b2), PathRef::Sketch(c)];
    x.overrides = vec![CornerOverride {
        point: None,
        at: [0.0, 0.0, 0.0],
        corner: CornerType::Butt,
        flip: false,
        turn: 2,
        gusset: None,
    }];
    st.frame(x);
    let b = st.build();
    let area = square_tube_area(40.0, 3.0);
    let want = area * ((300.0 - 20.0) + (250.0 - 20.0) + (200.0 + 20.0));
    assert!(close(total(&b), want, 1e-6), "{} vs {want}", total(&b));
}

#[test]
fn a_t_slot_member_weighs_its_catalogue_mass() {
    let mut st = Studio::new();
    let sk = st.sketch(
        PlaneRef::Top,
        vec![polyline(&[(0.0, 0.0), (300.0, 0.0)], false)],
    );
    st.frame(FrameFeature {
        path: vec![PathRef::Sketch(sk)],
        ..Default::default()
    });
    let b = st.build();
    let rows = frame::cut_list(&b.frames);
    let m = frame::row_material(&rows[0], &b.frames, &b.parts, &[]);
    assert!(m.nominal);
    assert_eq!(m.material.as_deref(), Some("Aluminium 6063-T5"));
    assert!((m.mass.unwrap() - 0.15).abs() < 1e-9, "{:?}", m.mass);
}

#[test]
fn a_chamfered_gusset_sits_flush_with_the_top() {
    let mut st = Studio::new();
    let sk = st.sketch(
        PlaneRef::Top,
        vec![polyline(&[(300.0, 0.0), (0.0, 0.0), (0.0, 200.0)], false)],
    );
    let mut x = tube("40x40x3");
    x.path = vec![PathRef::Sketch(sk)];
    x.gussets = true;
    x.gusset_chamfer = 10.0;
    x.gusset_position = GussetPosition::Top;
    st.frame(x);
    let b = st.build();
    let g = b
        .parts
        .iter()
        .find(|p| b.frames.iter().any(|m| m.part == p.id && m.plate))
        .expect("the gusset");
    assert!(
        close(volume(g), (60.0 * 60.0 - 10.0 * 10.0) / 2.0 * 5.0, 1e-9),
        "{}",
        volume(g)
    );
    let (lo, hi) = bounds(g);
    assert!(
        (hi[2] - 20.0).abs() < 1e-4 && (lo[2] - 15.0).abs() < 1e-4,
        "{lo:?} {hi:?}"
    );
}

#[test]
fn an_override_puts_a_gusset_in_one_corner() {
    let mut st = Studio::new();
    let sk = rectangle(&mut st);
    let mut x = tube("40x40x3");
    x.path = vec![PathRef::Sketch(sk)];
    x.overrides = vec![CornerOverride {
        point: Some(point_at(&st, sk, 400.0, 300.0)),
        at: [400.0, 300.0, 0.0],
        corner: CornerType::Miter,
        flip: false,
        turn: 0,
        gusset: Some(true),
    }];
    st.frame(x);
    let b = st.build();
    assert_eq!(b.frames.iter().filter(|m| m.plate).count(), 1);
}

/// The sketch line of `sketch` from (x0, y0) to (x1, y1) (either way).
fn line_at(st: &Studio, sketch: FeatureId, a: (f64, f64), b: (f64, f64)) -> cadrs_sketch::CurveId {
    let el = st.d.element(st.el).unwrap();
    let g = &el.feature(sketch).unwrap().sketch().unwrap().geometry;
    let at = |p: cadrs_sketch::PointId, q: (f64, f64)| {
        let v = g.points[p].pos;
        (v.x - q.0).abs() < 1e-9 && (v.y - q.1).abs() < 1e-9
    };
    g.curves
        .iter()
        .find(|(_, c)| match c.kind {
            cadrs_sketch::CurveKind::Line { a: p, b: q } => {
                (at(p, a) && at(q, b)) || (at(p, b) && at(q, a))
            }
            _ => false,
        })
        .map(|(id, _)| id)
        .expect("the line")
}

#[test]
fn editing_the_sketch_keeps_the_other_members_ids() {
    // A U of three lines framed as a whole sketch; one line deleted, another drawn: the
    // members of the lines left keep their parts.
    let mut st = Studio::new();
    let sk = st.sketch(
        PlaneRef::Top,
        vec![polyline(
            &[(0.0, 300.0), (0.0, 0.0), (400.0, 0.0), (400.0, 300.0)],
            false,
        )],
    );
    let mut x = tube("40x40x3");
    x.path = vec![PathRef::Sketch(sk)];
    let f = st.frame(x);
    let b = st.build();
    let id_of = |b: &rebuild::Build, at: [f64; 3]| {
        b.parts
            .iter()
            .filter(|p| p.feature == f)
            .find(|p| {
                let c = p.mass.unwrap().center_of_mass;
                (c.x - at[0]).abs() < 30.0 && (c.y - at[1]).abs() < 30.0
            })
            .map(|p| p.id)
    };
    let bottom = id_of(&b, [200.0, 0.0, 0.0]).unwrap();
    let right = id_of(&b, [400.0, 150.0, 0.0]).unwrap();
    let left_line = line_at(&st, sk, (0.0, 300.0), (0.0, 0.0));
    st.h.execute(
        &mut st.d,
        &EditSketch {
            element: st.el,
            feature: sk,
            op: SketchOp::Delete {
                curves: vec![left_line],
                points: Vec::new(),
                dimensions: Vec::new(),
                constraints: Vec::new(),
            },
        },
    )
    .unwrap();
    st.h.execute(
        &mut st.d,
        &EditSketch {
            element: st.el,
            feature: sk,
            op: polyline(&[(-200.0, 500.0), (-100.0, 500.0)], false),
        },
    )
    .unwrap();
    let b = st.build();
    assert_eq!(b.parts.iter().filter(|p| p.feature == f).count(), 3);
    assert_eq!(id_of(&b, [200.0, 0.0, 0.0]), Some(bottom));
    assert_eq!(id_of(&b, [400.0, 150.0, 0.0]), Some(right));
}

#[test]
fn coped_corners_at_sharp_and_wide_angles_close_without_overlap() {
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
        let mut x = FrameFeature::default();
        x.set_family(ProfileFamily::RoundTube);
        x.size = "48.3x3.2".into();
        x.corner = CornerType::CopedButt;
        x.path = vec![PathRef::Sketch(sk)];
        st.frame(x);
        let b = st.build();
        assert_eq!(b.parts.len(), 2, "{deg}°");
        assert!(st.overlap() < 1e-6, "{deg}°");
        // The runner covers the coped end: nothing of it past the runner's end.
        let (lo_runner, lo_coped) = {
            let mut los: Vec<(f64, f64)> = b
                .parts
                .iter()
                .map(|p| (bounds(p).0[0], volume(p)))
                .collect();
            los.sort_by(|a, b| b.1.total_cmp(&a.1));
            (los[0].0, los[1].0)
        };
        // (At 120° the coped member itself runs off that way.)
        if deg < 90.0 {
            assert!(
                lo_coped >= lo_runner - 1e-3,
                "{deg}°: {lo_coped} < {lo_runner}"
            );
        }
    }
}

#[test]
fn a_brace_into_a_mitred_corner_keeps_the_mitre() {
    // Rails mitred at (0, 0) with a diagonal ending there too: the rails stay mitred (the
    // widest pair), the brace is cut against both.
    let mut st = Studio::new();
    let sk = st.sketch(
        PlaneRef::Top,
        vec![
            polyline(
                &[(0.0, 0.0), (400.0, 0.0), (400.0, 300.0), (0.0, 300.0)],
                true,
            ),
            polyline(&[(0.0, 0.0), (400.0, 300.0)], false),
        ],
    );
    let mut x = tube("40x40x3");
    x.path = vec![PathRef::Sketch(sk)];
    st.frame(x);
    let b = st.build();
    assert_eq!(b.parts.len(), 5);
    let mitred = b
        .frames
        .iter()
        .filter(|m| {
            m.ends
                .iter()
                .all(|e| matches!(e, EndCut::Angle(a) if (a - 45.0).abs() < 1e-6))
        })
        .count();
    assert_eq!(mitred, 4, "{:?}", b.frames);
    assert!(st.overlap() < 1e-6);
}

#[test]
fn a_post_and_a_diagonal_on_one_chord_split_where_they_meet() {
    let mut st = Studio::new();
    let sk = st.sketch(
        PlaneRef::Top,
        vec![
            polyline(&[(0.0, 0.0), (600.0, 0.0)], false),
            polyline(&[(300.0, 0.0), (300.0, 300.0)], false),
            polyline(&[(300.0, 0.0), (600.0, 300.0)], false),
        ],
    );
    let mut x = tube("40x40x3");
    x.corner = CornerType::Butt;
    x.path = vec![PathRef::Sketch(sk)];
    st.frame(x);
    let b = st.build();
    assert_eq!(b.parts.len(), 3);
    assert!(st.overlap() < 1e-6);
}

#[test]
fn a_cube_cut_list_has_two_items() {
    // Its horizontal pairs run through at each corner, the verticals stop between them: the
    // 4 of one way are 220, the other 8 are 180, all square.
    let mut st = Studio::new();
    let base = st.sketch(
        PlaneRef::Top,
        vec![polyline(
            &[(0.0, 0.0), (200.0, 0.0), (200.0, 200.0), (0.0, 200.0)],
            true,
        )],
    );
    st.extrude(base, 200.0);
    let b = st.build();
    let part = &b.parts[0];
    let path: Vec<PathRef> = part
        .solid
        .edges
        .iter()
        .map(|e| {
            PathRef::Edge(EdgeRef {
                part: part.id,
                edge: e.name,
                seed: e.midpoint(),
            })
        })
        .collect();
    let mut x = tube("20x20x2");
    x.corner = CornerType::Butt;
    x.path = path;
    st.frame(x);
    let b = st.build();
    let rows = frame::cut_list(&b.frames);
    let mut got: Vec<(i64, usize)> = rows
        .iter()
        .map(|r| ((r.length * 10.0).round() as i64, r.quantity()))
        .collect();
    got.sort();
    assert_eq!(got, [(1800, 8), (2200, 4)], "{rows:?}");
}

#[test]
fn flipping_a_corner_override_swaps_the_runner() {
    let mut st = Studio::new();
    let sk = st.sketch(
        PlaneRef::Top,
        vec![polyline(&[(300.0, 0.0), (0.0, 0.0), (0.0, 200.0)], false)],
    );
    let mut x = tube("40x40x3");
    x.path = vec![PathRef::Sketch(sk)];
    x.overrides = vec![CornerOverride {
        point: Some(point_at(&st, sk, 0.0, 0.0)),
        at: [0.0, 0.0, 0.0],
        corner: CornerType::Butt,
        flip: true,
        turn: 1,
        gusset: None,
    }];
    st.frame(x);
    let b = st.build();
    let mut l: Vec<f64> = b.frames.iter().map(|m| m.length).collect();
    l.sort_by(f64::total_cmp);
    // The shorter runs on: 200 + 20, the longer stops at it: 300 − 20.
    assert!(
        (l[0] - 220.0).abs() < 1e-6 && (l[1] - 280.0).abs() < 1e-6,
        "{l:?}"
    );
}

#[test]
fn a_rounded_rectangle_is_one_closed_member() {
    // 400 × 300 with corners of 50: one bent ring, its section along its whole length.
    let mut st = Studio::new();
    let arc = |c: (f64, f64), s: (f64, f64), e: (f64, f64)| SketchOp::AddArc {
        center: Vec2::new(c.0, c.1),
        start: Vec2::new(s.0, s.1),
        end: Vec2::new(e.0, e.1),
        construction: false,
    };
    let sk = st.sketch(
        PlaneRef::Top,
        vec![
            polyline(&[(50.0, 0.0), (350.0, 0.0)], false),
            arc((350.0, 50.0), (350.0, 0.0), (400.0, 50.0)),
            polyline(&[(400.0, 50.0), (400.0, 250.0)], false),
            arc((350.0, 250.0), (400.0, 250.0), (350.0, 300.0)),
            polyline(&[(350.0, 300.0), (50.0, 300.0)], false),
            arc((50.0, 250.0), (50.0, 300.0), (0.0, 250.0)),
            polyline(&[(0.0, 250.0), (0.0, 50.0)], false),
            arc((50.0, 50.0), (0.0, 50.0), (50.0, 0.0)),
        ],
    );
    let mut x = FrameFeature::default();
    x.set_family(ProfileFamily::RoundBar);
    x.size = "20".into();
    x.path = vec![PathRef::Sketch(sk)];
    st.frame(x);
    let b = st.build();
    assert_eq!(b.parts.len(), 1);
    let centre = 2.0 * 300.0 + 2.0 * 200.0 + 2.0 * PI * 50.0;
    assert!(close(total(&b), PI * 100.0 * centre, 1e-5), "{}", total(&b));
    assert!(
        (b.frames[0].length - centre).abs() < 1e-6,
        "{}",
        b.frames[0].length
    );
}

#[test]
fn a_tee_on_an_arc_is_cut_against_it() {
    // A quarter arc of radius 300 and a spoke from its middle towards the centre.
    let mut st = Studio::new();
    let a = 45f64.to_radians();
    let sk = st.sketch(
        PlaneRef::Top,
        vec![
            SketchOp::AddArc {
                center: Vec2::new(0.0, 0.0),
                start: Vec2::new(300.0, 0.0),
                end: Vec2::new(0.0, 300.0),
                construction: false,
            },
            polyline(&[(300.0 * a.cos(), 300.0 * a.sin()), (0.0, 0.0)], false),
        ],
    );
    let mut x = FrameFeature::default();
    x.set_family(ProfileFamily::RoundBar);
    x.size = "20".into();
    x.corner = CornerType::CopedButt;
    x.path = vec![PathRef::Sketch(sk)];
    st.frame(x);
    let b = st.build();
    assert_eq!(b.parts.len(), 2);
    assert!(st.overlap() < 1e-6);
    let spoke = b.frames.iter().find(|m| !m.curved).unwrap();
    assert!(spoke.ends.contains(&EndCut::Coped), "{:?}", spoke.ends);
}

#[test]
fn a_trim_through_two_walls_keeps_the_longest_piece() {
    let mut st = Studio::new();
    let walls = st.sketch(
        PlaneRef::Top,
        vec![
            polyline(
                &[
                    (100.0, -100.0),
                    (120.0, -100.0),
                    (120.0, 100.0),
                    (100.0, 100.0),
                ],
                true,
            ),
            polyline(
                &[
                    (300.0, -100.0),
                    (320.0, -100.0),
                    (320.0, 100.0),
                    (300.0, 100.0),
                ],
                true,
            ),
        ],
    );
    let e = st.extrude(walls, 50.0);
    let sk = st.sketch(
        PlaneRef::Top,
        vec![polyline(&[(0.0, 0.0), (400.0, 0.0)], false)],
    );
    let wall_parts: Vec<cadrs_core::PartId> = st
        .build()
        .parts
        .iter()
        .filter(|p| p.feature == e)
        .map(|p| p.id)
        .collect();
    assert_eq!(wall_parts.len(), 2);
    let mut x = FrameFeature::default();
    x.set_family(ProfileFamily::SquareBar);
    x.size = "10".into();
    x.anchor = Anchor::Bottom;
    x.path = vec![PathRef::Sketch(sk)];
    x.trim = wall_parts;
    let f = st.frame(x);
    let b = st.build();
    let m = b.parts.iter().find(|p| p.feature == f).unwrap();
    // 0..100, 120..300 and 320..400: the middle 180 kept.
    assert!(close(volume(m), 100.0 * 180.0, 1e-6), "{}", volume(m));
}

#[test]
fn the_cut_list_tells_mitres_facing_other_ways_apart() {
    // A Z (its mitres facing opposite ways) and a picture frame's piece (both the same way),
    // all 400 long between 45° cuts: two items.
    let mut st = Studio::new();
    let z = st.sketch(
        PlaneRef::Top,
        vec![polyline(
            &[(0.0, 300.0), (0.0, 0.0), (400.0, 0.0), (400.0, -300.0)],
            false,
        )],
    );
    let u = st.sketch(
        PlaneRef::Top,
        vec![polyline(
            &[
                (1000.0, 300.0),
                (1000.0, 0.0),
                (1400.0, 0.0),
                (1400.0, 300.0),
            ],
            false,
        )],
    );
    let mut x = FrameFeature::default();
    x.set_family(ProfileFamily::RectTube);
    x.size = "60x40x3".into();
    x.corner = CornerType::Miter;
    x.path = vec![PathRef::Sketch(z), PathRef::Sketch(u)];
    st.frame(x);
    let b = st.build();
    let rows = frame::cut_list(&b.frames);
    let both_mitred: Vec<&frame::CutListRow> = rows
        .iter()
        .filter(|r| r.ends.iter().all(|e| matches!(e, EndCut::Angle(_))))
        .collect();
    assert_eq!(both_mitred.len(), 2, "{rows:?}");
}

#[test]
fn a_t_slot_rail_trimmed_by_a_t_slot_post_keeps_nothing_in_its_slot() {
    // A 40x40 post along X, and a 20x20 rail up to its axis in a frame of its own, trimmed by
    // it: the rail stops at the post's face, 300 − 20 long, nothing left in the post's slot.
    let mut st = Studio::new();
    let post = st.sketch(
        PlaneRef::Top,
        vec![polyline(&[(-200.0, 0.0), (200.0, 0.0)], false)],
    );
    let f = st.frame(FrameFeature {
        size: "40x40".into(),
        path: vec![PathRef::Sketch(post)],
        ..Default::default()
    });
    let post_part = st.build().parts.iter().find(|p| p.feature == f).unwrap().id;
    let area = {
        let mut st = Studio::new();
        let sk = st.sketch(
            PlaneRef::Top,
            vec![polyline(&[(0.0, 0.0), (100.0, 0.0)], false)],
        );
        st.frame(FrameFeature {
            path: vec![PathRef::Sketch(sk)],
            ..Default::default()
        });
        total(&st.build()) / 100.0
    };
    let rail = st.sketch(
        PlaneRef::Top,
        vec![polyline(&[(0.0, -300.0), (0.0, 0.0)], false)],
    );
    let g = st.frame(FrameFeature {
        path: vec![PathRef::Sketch(rail)],
        trim: vec![post_part],
        ..Default::default()
    });
    let b = st.build();
    let r = b.parts.iter().find(|p| p.feature == g).unwrap();
    assert!(
        close(volume(r), area * 280.0, 1e-6),
        "{} vs {}",
        volume(r),
        area * 280.0
    );
    let m = b.frames.iter().find(|m| m.feature == g).unwrap();
    assert!((m.length - 280.0).abs() < 1e-4, "{}", m.length);
}

#[test]
fn a_curve_picked_with_its_sketch_is_one_member() {
    let mut st = Studio::new();
    let sk = rectangle(&mut st);
    let line = line_at(&st, sk, (0.0, 0.0), (400.0, 0.0));
    let mut x = tube("40x40x3");
    x.path = vec![
        PathRef::Sketch(sk),
        PathRef::SketchCurve {
            sketch: sk,
            curve: line,
        },
    ];
    st.frame(x);
    assert_eq!(st.build().parts.len(), 4);
}

#[test]
fn a_loop_drawn_clockwise_keeps_a_left_section_inside() {
    // The rectangle's lines drawn the other way round (and its first one backwards): the
    // section to the members' left is still inside it.
    let mut st = Studio::new();
    let sk = st.sketch(
        PlaneRef::Top,
        vec![polyline(
            &[(0.0, 0.0), (0.0, 300.0), (400.0, 300.0), (400.0, 0.0)],
            true,
        )],
    );
    let mut x = tube("40x40x3");
    x.anchor = Anchor::Left;
    x.path = vec![PathRef::Sketch(sk)];
    st.frame(x);
    let b = st.build();
    for p in &b.parts {
        let (lo, hi) = bounds(p);
        assert!(lo[0] > -1e-4 && lo[1] > -1e-4 && hi[0] < 400.0 + 1e-4 && hi[1] < 300.0 + 1e-4);
    }
}

#[test]
fn mirrored_corners_cut_alike_pieces() {
    // Two L corners, one the other's mirror image: round tube, then square tube; either way,
    // two items of two.
    for family in [ProfileFamily::RoundTube, ProfileFamily::SquareTube] {
        let mut st = Studio::new();
        let sk = st.sketch(
            PlaneRef::Top,
            vec![
                polyline(&[(300.0, 0.0), (0.0, 0.0), (0.0, 200.0)], false),
                polyline(&[(700.0, 0.0), (1000.0, 0.0), (1000.0, 200.0)], false),
            ],
        );
        let mut x = FrameFeature::default();
        x.set_family(family);
        x.corner = CornerType::Miter;
        x.path = vec![PathRef::Sketch(sk)];
        st.frame(x);
        let b = st.build();
        let rows = frame::cut_list(&b.frames);
        let q: Vec<usize> = rows.iter().map(|r| r.quantity()).collect();
        assert_eq!(q, [2, 2], "{family:?}: {rows:?}");
    }
}

#[test]
fn a_rail_against_a_slanted_t_slot_post_is_trimmed_by_its_shape() {
    // The post turned 30° about Z: no square face to cut the rail at, so the rail is trimmed
    // by the post as it is, reaching past the corner of the post's box.
    let mut st = Studio::new();
    let t = 30f64.to_radians();
    let post = st.sketch(
        PlaneRef::Top,
        vec![polyline(
            &[
                (-200.0 * t.cos(), -200.0 * t.sin()),
                (200.0 * t.cos(), 200.0 * t.sin()),
            ],
            false,
        )],
    );
    let f = st.frame(FrameFeature {
        size: "40x40".into(),
        path: vec![PathRef::Sketch(post)],
        ..Default::default()
    });
    let b = st.build();
    let post_part = b.parts.iter().find(|p| p.feature == f).unwrap();
    let post_low = bounds(post_part).0[1];
    let rail = st.sketch(
        PlaneRef::Top,
        vec![polyline(&[(0.0, -300.0), (0.0, 0.0)], false)],
    );
    let g = st.frame(FrameFeature {
        path: vec![PathRef::Sketch(rail)],
        trim: vec![post_part.id],
        ..Default::default()
    });
    let b = st.build();
    let r = b.parts.iter().find(|p| p.feature == g).unwrap();
    assert!(
        bounds(r).1[1] > post_low + 1.0,
        "{} vs {post_low}",
        bounds(r).1[1]
    );
    assert!(st.overlap() < 1e-6);
}

#[test]
fn a_mitre_whose_members_both_start_there_is_butted_for_an_unsymmetric_section() {
    let mut st = Studio::new();
    let sk = st.sketch(
        PlaneRef::Top,
        vec![
            polyline(&[(0.0, 0.0), (300.0, 0.0)], false),
            polyline(&[(0.0, 0.0), (0.0, 200.0)], false),
            polyline(&[(0.0, 0.0), (-150.0, -150.0)], false),
        ],
    );
    let mut x = FrameFeature::default();
    x.set_family(ProfileFamily::Angle);
    x.size = "40x40x4".into();
    x.corner = CornerType::Miter;
    x.merge_tangent = false;
    x.path = vec![PathRef::Sketch(sk)];
    let f = st.frame(x);
    let b = st.build();
    assert!(
        b.warning(f).is_some_and(|w| w.contains("butted")),
        "{:?}",
        b.warning(f)
    );
    assert!(st.overlap() < 1e-6);
}

#[test]
fn a_loop_with_a_post_at_a_corner_keeps_a_left_section_inside() {
    // Drawn clockwise, with a third member at one corner: still a loop, run counter-clockwise.
    let mut st = Studio::new();
    let sk = st.sketch(
        PlaneRef::Top,
        vec![
            polyline(
                &[(0.0, 0.0), (0.0, 300.0), (400.0, 300.0), (400.0, 0.0)],
                true,
            ),
            polyline(&[(400.0, 0.0), (600.0, 0.0)], false),
        ],
    );
    let mut x = tube("40x40x3");
    x.anchor = Anchor::Left;
    x.path = vec![PathRef::Sketch(sk)];
    let f = st.frame(x);
    let b = st.build();
    let loop_parts: Vec<&Part> = b
        .parts
        .iter()
        .filter(|p| p.feature == f && bounds(p).1[0] < 400.0 + 1e-3)
        .collect();
    assert_eq!(loop_parts.len(), 4);
    for p in loop_parts {
        let (lo, hi) = bounds(p);
        assert!(
            lo[0] > -1e-4 && lo[1] > -1e-4 && hi[1] < 300.0 + 1e-4,
            "{lo:?} {hi:?}"
        );
    }
}

#[test]
fn a_d_shaped_loop_keeps_a_left_section_inside_either_way() {
    for reversed in [false, true] {
        let mut st = Studio::new();
        let line = if reversed {
            polyline(&[(100.0, 0.0), (-100.0, 0.0)], false)
        } else {
            polyline(&[(-100.0, 0.0), (100.0, 0.0)], false)
        };
        let sk = st.sketch(
            PlaneRef::Top,
            vec![
                line,
                SketchOp::AddArc {
                    center: Vec2::new(0.0, 0.0),
                    start: Vec2::new(100.0, 0.0),
                    end: Vec2::new(-100.0, 0.0),
                    construction: false,
                },
            ],
        );
        let mut x = tube("20x20x2");
        x.anchor = Anchor::Left;
        x.path = vec![PathRef::Sketch(sk)];
        st.frame(x);
        let b = st.build();
        for p in &b.parts {
            let (lo, hi) = bounds(p);
            assert!(
                lo[1] > -1e-3 && hi[1] < 100.0 + 1e-3,
                "{reversed}: {lo:?} {hi:?}"
            );
        }
    }
}

#[test]
fn a_brace_into_a_corner_is_listed_with_its_compound_cut() {
    let mut st = Studio::new();
    let sk = st.sketch(
        PlaneRef::Top,
        vec![
            polyline(
                &[(0.0, 0.0), (400.0, 0.0), (400.0, 300.0), (0.0, 300.0)],
                true,
            ),
            polyline(&[(0.0, 0.0), (400.0, 300.0)], false),
        ],
    );
    let mut x = tube("40x40x3");
    x.path = vec![PathRef::Sketch(sk)];
    st.frame(x);
    let b = st.build();
    // (The rails are 440 and 340 long, mitred; the brace is the fifth.)
    let brace = b
        .frames
        .iter()
        .find(|m| (m.length - 440.0).abs() > 1.0 && (m.length - 340.0).abs() > 1.0)
        .expect("the brace");
    assert!(
        brace.ends.iter().all(|e| matches!(e, EndCut::Compound(..))),
        "{:?}",
        brace.ends
    );
}

#[test]
fn the_end_offset_runs_free_ends_on_or_back() {
    for (off, want) in [(15.0, 330.0), (-10.0, 280.0)] {
        let mut st = Studio::new();
        let sk = st.sketch(
            PlaneRef::Top,
            vec![polyline(&[(0.0, 0.0), (300.0, 0.0)], false)],
        );
        let mut x = tube("20x20x2");
        x.end_offset = off;
        x.end_caps = true;
        x.path = vec![PathRef::Sketch(sk)];
        st.frame(x);
        let b = st.build();
        let m = b.frames.iter().find(|m| !m.plate).unwrap();
        assert!((m.length - want).abs() < 1e-4, "{off}: {}", m.length);
        // The caps sit on the moved ends.
        let xs: Vec<f64> = b
            .parts
            .iter()
            .filter(|p| b.frames.iter().any(|m| m.part == p.id && m.plate))
            .map(|p| bounds(p).0[0])
            .collect();
        assert!(
            xs.iter().any(|x| (x - (-off - 3.0)).abs() < 1e-4),
            "{off}: {xs:?}"
        );
        assert!(st.overlap() < 1e-6);
    }
}

#[test]
fn a_trim_that_splits_a_member_says_so() {
    let mut st = Studio::new();
    let wall = st.sketch(
        PlaneRef::Top,
        vec![polyline(
            &[
                (100.0, -100.0),
                (120.0, -100.0),
                (120.0, 100.0),
                (100.0, 100.0),
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
    x.size = "10".into();
    x.anchor = Anchor::Bottom;
    x.path = vec![PathRef::Sketch(sk)];
    x.trim = vec![wall_part];
    let f = st.frame(x);
    let b = st.build();
    assert!(
        b.warning(f).is_some_and(|w| w.contains("split")),
        "{:?}",
        b.warning(f)
    );
}

#[test]
fn member_overrides_turn_some_members_on_edge() {
    // A 20x40 frame, its 300 sides turned a quarter: those stand 40 high, the 400 ones lie
    // 20 high; butted, nothing overlaps.
    let mut st = Studio::new();
    let sk = rectangle(&mut st);
    let side = |st: &Studio, a, b| PathRef::SketchCurve {
        sketch: sk,
        curve: line_at(st, sk, a, b),
    };
    let overrides = vec![
        frame::MemberOverride {
            curve: side(&st, (400.0, 0.0), (400.0, 300.0)),
            angle: 90.0,
            angle_expr: "90 deg".into(),
        },
        frame::MemberOverride {
            curve: side(&st, (0.0, 300.0), (0.0, 0.0)),
            angle: 90.0,
            angle_expr: "90 deg".into(),
        },
    ];
    let f = st.frame(FrameFeature {
        size: "20x40".into(),
        path: vec![PathRef::Sketch(sk)],
        member_overrides: overrides,
        ..Default::default()
    });
    let b = st.build();
    assert!(b.warning(f).is_none(), "{:?}", b.warning(f));
    for p in &b.parts {
        let (lo, hi) = bounds(p);
        let (dx, dy, dz) = (hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]);
        if dx > dy {
            assert!((dz - 20.0).abs() < 1e-4, "a 400 side: {dz}");
        } else {
            assert!((dz - 40.0).abs() < 1e-4, "a 300 side: {dz}");
        }
    }
    assert!(st.overlap() < 1e-6);
}

#[test]
fn a_tee_into_a_turned_member_stops_at_its_turned_side() {
    // A 400 bar of 20x40 along X, a 300 stem up from its middle: the stem stops at the bar's
    // side, 20 out lying flat and 10 out turned on edge.
    let mut stops = Vec::new();
    for turned in [false, true] {
        let mut st = Studio::new();
        let sk = st.sketch(
            PlaneRef::Top,
            vec![
                polyline(&[(0.0, 0.0), (400.0, 0.0)], false),
                polyline(&[(200.0, 0.0), (200.0, 300.0)], false),
            ],
        );
        let bar = PathRef::SketchCurve {
            sketch: sk,
            curve: line_at(&st, sk, (0.0, 0.0), (400.0, 0.0)),
        };
        let member_overrides = if turned {
            vec![frame::MemberOverride {
                curve: bar,
                angle: 90.0,
                angle_expr: "90 deg".into(),
            }]
        } else {
            Vec::new()
        };
        let f = st.frame(FrameFeature {
            size: "20x40".into(),
            path: vec![PathRef::Sketch(sk)],
            member_overrides,
            ..Default::default()
        });
        let b = st.build();
        assert!(b.warning(f).is_none(), "{:?}", b.warning(f));
        let (bar, stem): (Vec<&Part>, Vec<&Part>) = b.parts.iter().partition(|p| {
            let (lo, hi) = bounds(p);
            hi[0] - lo[0] > hi[1] - lo[1]
        });
        let (bar_hi, stem_lo) = (bounds(bar[0]).1[1], bounds(stem[0]).0[1]);
        assert!(
            (bar_hi - stem_lo).abs() < 1e-4,
            "{turned}: {bar_hi} {stem_lo}"
        );
        assert!(st.overlap() < 1e-6);
        stops.push(stem_lo);
    }
    assert!(
        (stops[0] - 20.0).abs() < 1e-4 && (stops[1] - 10.0).abs() < 1e-4,
        "{stops:?}"
    );
}

#[test]
fn a_mitre_is_kept_only_where_its_halves_meet() {
    // A 20x40 rectangle mitred, one side turned: a quarter turn makes its corners' halves
    // differ (butted, and said so); a half turn doesn't (the section is the same upside down).
    for (angle, butted) in [(0.0, false), (90.0, true), (180.0, false)] {
        let mut st = Studio::new();
        let sk = rectangle(&mut st);
        let side = PathRef::SketchCurve {
            sketch: sk,
            curve: line_at(&st, sk, (400.0, 0.0), (400.0, 300.0)),
        };
        let f = st.frame(FrameFeature {
            size: "20x40".into(),
            path: vec![PathRef::Sketch(sk)],
            corner: CornerType::Miter,
            member_overrides: vec![frame::MemberOverride {
                curve: side,
                angle,
                angle_expr: format!("{angle} deg"),
            }],
            ..Default::default()
        });
        let b = st.build();
        assert_eq!(
            b.warning(f).is_some_and(|w| w.contains("butted")),
            butted,
            "{angle}: {:?}",
            b.warning(f)
        );
        let mitres = b
            .frames
            .iter()
            .flat_map(|m| m.ends)
            .filter(|e| matches!(e, EndCut::Angle(a) if (a - 45.0).abs() < 1e-6))
            .count();
        assert_eq!(mitres, if butted { 4 } else { 8 }, "{angle}");
        assert!(st.overlap() < 1e-6, "{angle}");
    }
    // A channel isn't symmetric, but round a loop its members run head to tail: mitred.
    let mut st = Studio::new();
    let sk = rectangle(&mut st);
    let mut x = FrameFeature::default();
    x.set_family(ProfileFamily::Channel);
    x.size = frame::size_names(ProfileFamily::Channel)[0].into();
    x.corner = CornerType::Miter;
    x.path = vec![PathRef::Sketch(sk)];
    let f = st.frame(x);
    let b = st.build();
    assert!(b.warning(f).is_none(), "{:?}", b.warning(f));
    assert!(st.overlap() < 1e-6);
}

#[test]
fn a_negative_end_offset_on_a_bent_end_follows_its_curve() {
    // 100 along X, then a quarter of radius 50: a round bar's free end on the arc, both ends
    // stopped 20 short along the path.
    let ops = vec![
        polyline(&[(0.0, 0.0), (100.0, 0.0)], false),
        SketchOp::AddArc {
            center: Vec2::new(100.0, 50.0),
            start: Vec2::new(100.0, 0.0),
            end: Vec2::new(150.0, 50.0),
            construction: false,
        },
    ];
    let centre = 100.0 + PI * 25.0;
    let mut st = Studio::new();
    let sk = st.sketch(PlaneRef::Top, ops);
    let mut x = FrameFeature::default();
    x.set_family(ProfileFamily::RoundBar);
    x.size = "20".into();
    x.end_offset = -20.0;
    x.end_offset_expr = "-20 mm".into();
    x.path = vec![PathRef::Sketch(sk)];
    let f = st.frame(x.clone());
    let b = st.build();
    assert!(b.warning(f).is_none(), "{:?}", b.warning(f));
    let want = PI * 100.0 * (centre - 40.0);
    assert!(close(total(&b), want, 1e-6), "{} vs {want}", total(&b));
    assert!(
        (b.frames[0].length - (centre - 40.0)).abs() < 1e-6,
        "{}",
        b.frames[0].length
    );
    // Its ends lie square to the path where it stops: at x 20, and 20 back round the arc from
    // (150, 50), the bar's outside there furthest along X.
    let (lo, hi) = bounds(&b.parts[0]);
    let a = 20.0 / 50.0;
    let far = 100.0 + 60.0 * f64::cos(a);
    // (From the display mesh: within its deflection.)
    assert!(
        (hi[0] - far).abs() < 0.2 && (lo[0] - 20.0).abs() < 1e-3,
        "{lo:?} {hi:?}"
    );
    // Stopped short more than it is long: an error.
    let mut st = Studio::new();
    let sk = st.sketch(
        PlaneRef::Top,
        vec![polyline(&[(0.0, 0.0), (100.0, 0.0)], false)],
    );
    x.end_offset = -60.0;
    x.path = vec![PathRef::Sketch(sk)];
    let f = st.frame(x);
    let b = rebuild::build(&st.features());
    assert!(
        b.error(f).is_some_and(|e| e.contains("nothing")),
        "{:?}",
        b.error(f)
    );
}

#[test]
fn gussets_go_either_side_of_a_tee_and_in_the_pair_a_corner_of_three_joins() {
    // A tee of 40x40x3: a gusset on each side of the stem.
    let mut st = Studio::new();
    let sk = st.sketch(
        PlaneRef::Top,
        vec![
            polyline(&[(0.0, 0.0), (400.0, 0.0)], false),
            polyline(&[(200.0, 0.0), (200.0, 300.0)], false),
        ],
    );
    let mut x = tube("40x40x3");
    x.corner = CornerType::Butt;
    x.gussets = true;
    x.path = vec![PathRef::Sketch(sk)];
    st.frame(x.clone());
    let b = st.build();
    let plates: Vec<&Part> = b
        .parts
        .iter()
        .filter(|p| b.frames.iter().any(|m| m.part == p.id && m.plate))
        .collect();
    assert_eq!(plates.len(), 2);
    for p in &plates {
        let (lo, hi) = bounds(p);
        // From the stem's sides (180, 220) and the bar's (y 20), out 60 (the default size).
        let s = x_gusset_size();
        assert!((lo[1] - 20.0).abs() < 1e-4 && (hi[1] - 20.0 - s).abs() < 1e-4);
        assert!(
            (hi[0] - 180.0).abs() < 1e-4 || (lo[0] - 220.0).abs() < 1e-4,
            "{lo:?} {hi:?}"
        );
    }
    assert!(st.overlap() < 1e-6);
    // Three at a corner (X and Y on Top, up Z): one gusset, in the flat pair joined; an
    // override there sees three members meet.
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
    x.path = vec![PathRef::Sketch(a), PathRef::Sketch(b2), PathRef::Sketch(c)];
    x.overrides = vec![
        CornerOverride {
            point: None,
            at: [0.0, 0.0, 0.0],
            corner: CornerType::Butt,
            flip: false,
            turn: 0,
            gusset: None,
        },
        CornerOverride {
            point: None,
            at: [300.0, 0.0, 0.0],
            corner: CornerType::Butt,
            flip: false,
            turn: 0,
            gusset: None,
        },
    ];
    let f = st.frame(x);
    let b = rebuild::build(&st.features());
    let plates: Vec<&Part> = b
        .parts
        .iter()
        .filter(|p| b.frames.iter().any(|m| m.part == p.id && m.plate))
        .collect();
    assert_eq!(plates.len(), 1);
    let (lo, hi) = bounds(plates[0]);
    assert!(hi[2] - lo[2] < 10.0, "flat: {lo:?} {hi:?}");
    let corners = b.frame_corners.iter().find(|c| c.feature == f).unwrap();
    assert_eq!(corners.ends, [3, 1]);
}

#[test]
fn a_trim_at_a_plane_keeps_the_longer_side() {
    // A 400 tube from x -100 to 300 crossing Right (x = 0), and a 200 one clear of it: the first
    // cut at the plane, its 300 side kept (flipped: its 100 side); the second left whole. The
    // cut end gets no cap.
    for (flip, want) in [(false, 300.0), (true, 100.0)] {
        let mut st = Studio::new();
        let sk = st.sketch(
            PlaneRef::Top,
            vec![
                polyline(&[(-100.0, 0.0), (300.0, 0.0)], false),
                polyline(&[(50.0, 200.0), (250.0, 200.0)], false),
            ],
        );
        let mut x = tube("40x40x3");
        x.path = vec![PathRef::Sketch(sk)];
        x.trim_planes = vec![PlaneRef::Right];
        x.trim_flip = flip;
        x.end_caps = true;
        let f = st.frame(x);
        let b = st.build();
        assert!(b.warning(f).is_none(), "{:?}", b.warning(f));
        let mut lengths: Vec<f64> = b
            .frames
            .iter()
            .filter(|m| !m.plate)
            .map(|m| m.length)
            .collect();
        lengths.sort_by(f64::total_cmp);
        let mut expect = [200.0, want];
        expect.sort_by(f64::total_cmp);
        for (got, w) in lengths.iter().zip(expect) {
            assert!((got - w).abs() < 1e-4, "{flip}: {lengths:?}");
        }
        let area = square_tube_area(40.0, 3.0);
        let members: f64 = b
            .parts
            .iter()
            .filter(|p| b.frames.iter().any(|m| m.part == p.id && !m.plate))
            .map(volume)
            .sum();
        assert!(
            close(members, area * (200.0 + want), 1e-6),
            "{flip}: {members}"
        );
        // Caps: both of the whole one's ends, one of the cut one's.
        assert_eq!(b.frames.iter().filter(|m| m.plate).count(), 3, "{flip}");
        let cut = b
            .parts
            .iter()
            .find(|p| bounds(p).1[1] < 30.0 && b.frames.iter().any(|m| m.part == p.id && !m.plate))
            .unwrap();
        let (lo, hi) = bounds(cut);
        if flip {
            assert!(
                (hi[0] - 0.0).abs() < 1e-4 && (lo[0] + 100.0).abs() < 1e-3,
                "{lo:?} {hi:?}"
            );
        } else {
            assert!(
                (lo[0] - 0.0).abs() < 1e-4 && (hi[0] - 300.0).abs() < 1e-3,
                "{lo:?} {hi:?}"
            );
        }
    }
}

#[test]
fn caps_set_into_the_ends_shorten_the_member() {
    // A 300 tube capped 5 thick inside its ends: the tube 290 from 5 to 295, the caps filling
    // the last 5 at each end; nothing past 0 or 300, nothing overlapping.
    let mut st = Studio::new();
    let sk = st.sketch(
        PlaneRef::Top,
        vec![polyline(&[(0.0, 0.0), (300.0, 0.0)], false)],
    );
    let mut x = tube("40x40x3");
    x.path = vec![PathRef::Sketch(sk)];
    x.end_caps = true;
    x.cap_thickness = 5.0;
    x.cap_thickness_expr = "5 mm".into();
    x.cap_inward = true;
    let f = st.frame(x);
    let b = st.build();
    assert!(b.warning(f).is_none(), "{:?}", b.warning(f));
    let tube = b.frames.iter().find(|m| !m.plate).unwrap();
    assert!((tube.length - 290.0).abs() < 1e-4, "{}", tube.length);
    let mut xs: Vec<(f64, f64)> = b
        .parts
        .iter()
        .map(|p| {
            let (lo, hi) = bounds(p);
            (lo[0], hi[0])
        })
        .collect();
    xs.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let want = [(0.0, 5.0), (5.0, 295.0), (295.0, 300.0)];
    for (got, w) in xs.iter().zip(want) {
        assert!(
            (got.0 - w.0).abs() < 1e-4 && (got.1 - w.1).abs() < 1e-4,
            "{xs:?}"
        );
    }
    assert!(st.overlap() < 1e-6);
}

#[test]
fn a_cap_on_a_bent_end_stopped_short_sits_square_to_the_arc() {
    // A 33.7x3.2 tube 100 along X then a quarter of radius 100 round to (200, 100), stopped 20
    // short at each end and capped: the arc end's cap lies across the tube where it stops,
    // 0.2 rad back round the arc, and touches nothing it shouldn't.
    let mut st = Studio::new();
    let sk = st.sketch(
        PlaneRef::Top,
        vec![
            polyline(&[(0.0, 0.0), (100.0, 0.0)], false),
            SketchOp::AddArc {
                center: Vec2::new(100.0, 100.0),
                start: Vec2::new(100.0, 0.0),
                end: Vec2::new(200.0, 100.0),
                construction: false,
            },
        ],
    );
    let mut x = FrameFeature::default();
    x.set_family(ProfileFamily::RoundTube);
    x.size = "33.7x3.2".into();
    x.end_caps = true;
    x.end_offset = -20.0;
    x.end_offset_expr = "-20 mm".into();
    x.path = vec![PathRef::Sketch(sk)];
    let f = st.frame(x);
    let b = st.build();
    assert!(b.warning(f).is_none(), "{:?}", b.warning(f));
    let caps: Vec<&Part> = b
        .parts
        .iter()
        .filter(|p| b.frames.iter().any(|m| m.part == p.id && m.plate))
        .collect();
    assert_eq!(caps.len(), 2);
    // The arc end's cap: centred on the arc at 0.2 rad short of its end.
    let a = 0.2f64;
    let at = [100.0 + 100.0 * a.cos(), 100.0 - 100.0 * a.sin()];
    let near = caps.iter().any(|p| {
        let (lo, hi) = bounds(p);
        let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
        (c[0] - at[0]).hypot(c[1] - at[1]) < 4.0
    });
    assert!(
        near,
        "{:?}",
        caps.iter().map(|p| bounds(p)).collect::<Vec<_>>()
    );
    assert!(st.overlap() < 1e-6);
}

#[test]
fn the_frames_fixture_is_current() {
    // `fixtures/frames.cadrs` is the sample document as `samples::frames::document()` builds
    // it (open it with `cadrs fixtures/frames.cadrs`). Regenerate it with
    // `CADRS_WRITE_FIXTURES=1 cargo test -r -p cadrs_core --test frame the_frames_fixture`.
    use cadrs_core::samples::frames;
    let doc = frames::document().unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/frames.cadrs");
    let file = frames::file(doc.clone());
    let text = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default()).unwrap();
    if std::env::var("CADRS_WRITE_FIXTURES").is_ok() {
        std::fs::write(&path, &text).unwrap();
    }
    let stored = cadrs_core::Store::load_path(&path).expect("the fixture loads");
    assert_eq!(stored.document, doc, "fixtures/frames.cadrs is out of date");
    // Both Part Studios build clean.
    for el in &stored.document.elements {
        let b = rebuild::build(el.features());
        assert!(
            b.errors.is_empty() && b.warnings.is_empty(),
            "{}: {:?} {:?}",
            el.name,
            b.errors,
            b.warnings
        );
    }
}

/// A 400 round tube along X and a 300 one up from its middle, each a Frame of its own (so
/// neither is joined to the other): their features and the sketch they follow.
fn two_tubes(st: &mut Studio) -> (FeatureId, FeatureId) {
    let bar = st.sketch(
        PlaneRef::Top,
        vec![polyline(&[(0.0, 0.0), (400.0, 0.0)], false)],
    );
    let stem = st.sketch(
        PlaneRef::Top,
        vec![polyline(&[(200.0, 0.0), (200.0, 300.0)], false)],
    );
    let made = [bar, stem].map(|sk| {
        let mut x = FrameFeature::default();
        x.set_family(ProfileFamily::RoundTube);
        x.size = "48.3x3.2".into();
        x.path = vec![PathRef::Sketch(sk)];
        st.frame(x)
    });
    (made[0], made[1])
}

/// ∫∫ √(R² − z²) over a disc of radius r (z across it): what a cylinder of radius R, its axis
/// through the disc's centre and in its plane, takes off a bar of that section ending on the
/// axis (Simpson's rule in z = r sin t, smooth there).
fn saddle(r: f64, big: f64) -> f64 {
    let n = 4000;
    let f = |t: f64| 2.0 * r * r * t.cos().powi(2) * (big * big - (r * t.sin()).powi(2)).sqrt();
    let h = PI / n as f64;
    let mut s = f(-PI / 2.0) + f(PI / 2.0);
    for i in 1..n {
        s += f(-PI / 2.0 + i as f64 * h) * if i % 2 == 1 { 4.0 } else { 2.0 };
    }
    s * h / 3.0
}

#[test]
fn a_frame_trim_saddles_a_tube_ending_on_anothers_axis() {
    // The stem ends on the bar's axis: trimmed to the bar, its end is the bar's outside.
    let mut st = Studio::new();
    let (bar, stem) = two_tubes(&mut st);
    let before = st.build();
    let part =
        |b: &rebuild::Build, f: FeatureId| b.parts.iter().find(|p| p.feature == f).unwrap().id;
    let (bar_part, stem_part) = (part(&before, bar), part(&before, stem));
    let trim = st.add(
        "Frame trim",
        FeatureKind::FrameTrim(frame::FrameTrimFeature {
            groups: vec![vec![bar_part], vec![stem_part]],
        }),
    );
    let b = st.build();
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    assert!(b.warnings.is_empty(), "{:?}", b.warnings);
    assert_eq!(b.parts.len(), 2);
    let (ro, ri) = (24.15, 24.15 - 3.2);
    let a = PI * (ro * ro - ri * ri);
    // The bar is whole; the stem keeps its part and loses the saddle.
    let bar_now = b.parts.iter().find(|p| p.id == bar_part).unwrap();
    assert!(
        close(volume(bar_now), a * 400.0, 1e-9),
        "{}",
        volume(bar_now)
    );
    let stem_now = b.parts.iter().find(|p| p.id == stem_part).unwrap();
    assert!(stem_now.features.contains(&trim));
    let want = a * 300.0 - (saddle(ro, ro) - saddle(ri, ro));
    assert!(
        close(volume(stem_now), want, 1e-9),
        "{} vs {want}",
        volume(stem_now)
    );
    // The cut list says so, and gives the stem its length to the bar's side at its longest.
    let m = b.frames.iter().find(|m| m.part == stem_part).unwrap();
    assert_eq!(m.ends[0], EndCut::Trimmed);
    assert_eq!(m.ends[1], EndCut::Square);
    assert!(close(m.volume, want, 1e-9));
    let long = 300.0 - (ro * ro - ri * ri).sqrt();
    assert!(m.length <= 300.0 && m.length >= long - 1e-6, "{}", m.length);
}

#[test]
fn a_frame_trims_later_groups_give_way_to_all_before_them() {
    // Three groups: the bar, the stem, and a brace across the stem. The brace is cut by the
    // stem (and would be by the bar); the stem only by the bar.
    let mut st = Studio::new();
    let (bar, stem) = two_tubes(&mut st);
    let across = st.sketch(
        PlaneRef::Top,
        vec![polyline(&[(100.0, 150.0), (200.0, 150.0)], false)],
    );
    let mut x = FrameFeature::default();
    x.set_family(ProfileFamily::RoundTube);
    x.size = "33.7x3.2".into();
    x.path = vec![PathRef::Sketch(across)];
    let brace = st.frame(x);
    let before = st.build();
    let part = |f: FeatureId| before.parts.iter().find(|p| p.feature == f).unwrap().id;
    let (bar_part, stem_part, brace_part) = (part(bar), part(stem), part(brace));
    let stem_before = volume(before.parts.iter().find(|p| p.id == stem_part).unwrap());
    st.add(
        "Frame trim",
        FeatureKind::FrameTrim(frame::FrameTrimFeature {
            groups: vec![vec![bar_part], vec![stem_part], vec![brace_part]],
        }),
    );
    let b = st.build();
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    let (ro, ri) = (24.15, 24.15 - 3.2);
    let a = PI * (ro * ro - ri * ri);
    let stem_now = volume(b.parts.iter().find(|p| p.id == stem_part).unwrap());
    let want = a * 300.0 - (saddle(ro, ro) - saddle(ri, ro));
    assert!(close(stem_before, a * 300.0, 1e-9));
    assert!(close(stem_now, want, 1e-9), "{stem_now} vs {want}");
    // The brace ends on the stem's axis: saddled to the stem's outside.
    let (bo, bi) = (16.85, 16.85 - 3.2);
    let brace_now = volume(b.parts.iter().find(|p| p.id == brace_part).unwrap());
    let want = PI * (bo * bo - bi * bi) * 100.0 - (saddle(bo, ro) - saddle(bi, ro));
    assert!(close(brace_now, want, 1e-9), "{brace_now} vs {want}");
}

#[test]
fn a_cut_list_gathers_the_members_in_an_open_composite() {
    let mut st = Studio::new();
    let (bar, stem) = two_tubes(&mut st);
    let f = st.add(
        "Cut list",
        FeatureKind::CutList(frame::CutListFeature::default()),
    );
    let b = st.build();
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    assert_eq!(b.composites.len(), 1);
    let c = &b.composites[0];
    assert_eq!(c.part.feature, f);
    assert!(!c.closed);
    let made: Vec<FeatureId> = c.members.iter().map(|p| p.feature).collect();
    assert_eq!(made, vec![bar, stem]);
    // Its members are still parts of their own, and still the cut list's rows.
    assert_eq!(b.parts.len(), 3);
    let live = frame::live_members(&b.frames, &b.parts);
    assert_eq!(live.len(), 2);

    // Only the parts picked, when not every member.
    let mut st = Studio::new();
    let (bar, _) = two_tubes(&mut st);
    let bar_part = st
        .build()
        .parts
        .iter()
        .find(|p| p.feature == bar)
        .unwrap()
        .id;
    st.add(
        "Cut list",
        FeatureKind::CutList(frame::CutListFeature {
            all: false,
            parts: vec![bar_part],
            overrides: Vec::new(),
        }),
    );
    let b = st.build();
    assert_eq!(b.composites[0].members, vec![bar_part]);

    // A part a column's value is given for is listed too, frame member or not.
    let mut st = Studio::new();
    let (bar, stem) = two_tubes(&mut st);
    let plate = st.sketch(
        PlaneRef::Top,
        vec![polyline(
            &[(0.0, 100.0), (50.0, 100.0), (50.0, 150.0), (0.0, 150.0)],
            true,
        )],
    );
    let plate = st.extrude(plate, 5.0);
    let before = st.build();
    let plate_part = before.parts.iter().find(|p| p.feature == plate).unwrap().id;
    st.add(
        "Cut list",
        FeatureKind::CutList(frame::CutListFeature {
            all: true,
            parts: Vec::new(),
            overrides: vec![frame::CutListOverride {
                parts: vec![plate_part],
                column: "Description".into(),
                value: "Foot".into(),
            }],
        }),
    );
    let b = st.build();
    let made: Vec<FeatureId> = b.composites[0].members.iter().map(|p| p.feature).collect();
    assert_eq!(made, vec![bar, stem, plate]);
}
