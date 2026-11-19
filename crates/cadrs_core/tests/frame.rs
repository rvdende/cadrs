//! The Frame feature (`cadrs_core::frame`, issue #266) through the document's commands: members
//! along sketch curves joined by mitres, butts and copes, checked against closed-form volumes
//! and lengths, and the cut list they make.
#![cfg(feature = "brep")]

use cadrs_core::advanced::PathRef;
use cadrs_core::commands::{AddFeature, AddSketch, EditSketch};
use cadrs_core::document::{Document, FeatureKind};
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

fn tube(size: &str) -> FrameFeature {
    let mut x = FrameFeature::default();
    x.set_family(ProfileFamily::SquareTube);
    x.size = size.into();
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
            lo[i] = lo[i].min(q[i] as f64);
            hi[i] = hi[i].max(q[i] as f64);
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
        lo = lo.min(q[2] as f64);
        hi = hi.max(q[2] as f64);
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
