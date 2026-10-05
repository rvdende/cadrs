//! Flat patterns checked against hand calculations (P3I.1).
//!
//! Flat lengths of walls meeting at virtual sharps: sum of the sharp lengths, minus two setbacks
//! per bend (outside setbacks `(R+T)·tan(θ/2)` when the walls' definition faces are the outside of
//! the bend, inside ones `R·tan(θ/2)` otherwise), plus the bend allowance `θ·(R + K·T)`.
//!
//! The samples (`cadrs_sheetmetal::samples`) use T = 2, R = 3, K = 0.45 here, so a 90° bend has
//! an outside setback of 5, an inside setback of 3 and an allowance of π/2 · 3.9 ≈ 6.126.

use std::f64::consts::{FRAC_PI_2, PI};

use cadrs_sheetmetal::flat::{FlatError, FlatPart, PieceSource, ReliefCut, ReliefSource, flatten};
use cadrs_sheetmetal::model::{BendEnd, CornerOverride, HemAlignment, JointId, JointKind, Model, P3, RipStyle, SharpBuilder, Surface, V3};
use cadrs_sheetmetal::params::{BendCalc, BendReliefKind, CornerReliefKind, Params};
use cadrs_sheetmetal::poly::{P2, Polygon, V2, overlap_area};
use cadrs_sheetmetal::table::{BendDirection, JointType, table};
use cadrs_sheetmetal::{BendValue, BuildError, CornerRelief, FlatPattern, WallId, bend, samples};

const T: f64 = 2.0;
const R: f64 = 3.0;
const K: f64 = 0.45;
const GAP: f64 = 0.2;
/// Flat outlines come out of polygon booleans on a 1 nm grid: compare sizes to 1e-6 mm.
const TOL: f64 = 1e-6;
const OSSB: f64 = R + T;

fn params() -> Params {
    Params {
        thickness: T,
        bend_radius: R,
        k_factor: K,
        minimal_gap: GAP,
        ..Default::default()
    }
}

fn ba90() -> f64 {
    bend::bend_allowance(R, T, FRAC_PI_2, K)
}

fn rect(w: f64, h: f64) -> Polygon {
    Polygon::rect(P2::origin(), P2::new(w, h))
}

/// A sample that must build and be consistent in 3D.
fn ok(m: Result<Model, BuildError>) -> Model {
    let m = m.expect("sample builds");
    assert!(m.validate().is_empty(), "consistent in 3D: {:?}", m.validate());
    m
}

fn one_part(m: &Model) -> FlatPart {
    let f = flatten(m);
    assert!(f.is_ok(), "{:?}", f.errors);
    assert_eq!(f.parts.len(), 1);
    f.parts.into_iter().next().unwrap()
}

/// Width and height of a flat part.
fn size(m: &Model) -> (f64, f64) {
    let (lo, hi) = one_part(m).bounds().unwrap();
    (hi.x - lo.x, hi.y - lo.y)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < TOL
}

fn close_pt(a: P2, b: P2) -> bool {
    (a - b).norm() < 1e-5
}

fn bounds(p: &Polygon) -> (P2, P2) {
    p.bounds().unwrap()
}

// ---------------------------------------------------------------------------------------------
// Flat lengths

#[test]
fn l_bracket_bent_up_uses_outside_setbacks() {
    let (w, h) = size(&ok(samples::l_bracket(params(), true)));
    let expect = 50.0 + 30.0 - 2.0 * OSSB + ba90();
    assert!(close(w, expect), "{w} vs {expect}");
    assert!(close(h, 40.0));
}

#[test]
fn l_bracket_bent_down_uses_inside_setbacks() {
    let (w, _) = size(&ok(samples::l_bracket(params(), false)));
    let expect = 50.0 + 30.0 - 2.0 * R + ba90();
    assert!(close(w, expect), "{w} vs {expect}");
}

#[test]
fn bend_allowance_mode() {
    let p = Params {
        bend_calc: BendCalc::BendAllowance,
        bend_allowance: 7.0,
        ..params()
    };
    assert!(close(size(&ok(samples::l_bracket(p, true))).0, 80.0 - 2.0 * OSSB + 7.0));
    assert!(close(size(&ok(samples::u_channel(p))).0, 110.0 - 4.0 * OSSB + 14.0));
    let (w, h) = size(&ok(samples::open_box(p, RipStyle::EdgeJoint)));
    assert!(close(w, 100.0 + 2.0 * (20.0 - 2.0 * OSSB + 7.0)), "{w}");
    assert!(close(h, 60.0 + 2.0 * (20.0 - 2.0 * OSSB + 7.0)), "{h}");
}

#[test]
fn bend_deduction_mode() {
    let p = Params {
        bend_calc: BendCalc::BendDeduction,
        bend_deduction: 4.0,
        ..params()
    };
    // Up: the sharp lengths are outside lengths, so the deduction comes straight off them.
    assert!(close(size(&ok(samples::l_bracket(p, true))).0, 76.0));
    assert!(close(size(&ok(samples::u_channel(p))).0, 110.0 - 8.0));
    // Down: the sharps are inside ones (50 + 30 inside = 52 + 32 outside), so 84 − 4 = 80.
    assert!(close(size(&ok(samples::l_bracket(p, false))).0, 80.0));
    let (w, _) = size(&ok(samples::open_box(p, RipStyle::EdgeJoint)));
    assert!(close(w, 100.0 + 2.0 * 20.0 - 2.0 * 4.0), "{w}");
}

#[test]
fn per_bend_value_overrides_the_model() {
    let mut m = ok(samples::l_bracket(params(), true));
    let JointKind::Bend(b) = &mut m.joints[0].kind else { panic!() };
    b.value = Some(BendValue::KFactor(0.3));
    let expect = 80.0 - 2.0 * OSSB + bend::bend_allowance(R, T, FRAC_PI_2, 0.3);
    assert!(close(size(&m).0, expect));
}

#[test]
fn obtuse_and_sharp_bends() {
    for deg in [10.0_f64, 45.0, 120.0, 150.0] {
        let theta = deg.to_radians();
        let mut b = SharpBuilder::new(params());
        let a = b.wall(P3::origin(), V3::x(), V3::y(), rect(50.0, 40.0));
        let d = V3::new(theta.cos(), 0.0, theta.sin());
        let f = b.wall(P3::new(50.0, 0.0, 0.0), d, V3::y(), rect(30.0, 40.0));
        b.bend(a, f, (P3::new(50.0, 0.0, 0.0), P3::new(50.0, 40.0, 0.0)));
        let m = ok(b.build());
        assert!((m.joints[0].bend().unwrap().angle - theta).abs() < 1e-9);
        let expect = 80.0 - 2.0 * bend::outside_setback(R, T, theta).unwrap() + bend::bend_allowance(R, T, theta, K);
        assert!(close(size(&m).0, expect), "{deg}°");
    }
}

#[test]
fn u_channel_and_its_table() {
    let m = ok(samples::u_channel(params()));
    let (w, h) = size(&m);
    assert!(close(w, 110.0 - 4.0 * OSSB + 2.0 * ba90()));
    assert!(close(h, 40.0));
    let t = table(&m);
    assert_eq!(t.bends.len(), 2);
    assert_eq!((t.bends[0].name.as_str(), t.bends[1].name.as_str()), ("Bend A", "Bend B"));
    assert!(t.bends.iter().all(|r| r.direction == BendDirection::Up && (r.angle_deg - 90.0).abs() < 1e-9));
    // The model's K factor, exactly as entered.
    assert_eq!(t.bends[0].value, Some(BendValue::KFactor(K)));
}

#[test]
fn hem_lays_flat_with_its_allowance() {
    let m = ok(samples::hem(params()));
    assert!(close(size(&m).0, 50.0 + PI * (R + K * T) + 10.0));
    assert!(!table(&m).bends[0].editable, "hems aren't editable in the table");
    // In bend deduction mode a hem has no deduction (undefined at 180°): the table shows "–" and
    // the flat uses the model's K factor.
    let mut d = m.clone();
    d.params.bend_calc = BendCalc::BendDeduction;
    assert!(flatten(&d).is_ok());
    assert_eq!(table(&d).bends[0].value, None, "shown as –");
    // A bend's own deduction there can't apply.
    let JointKind::Bend(b) = &mut d.joints[0].kind else { panic!() };
    b.value = Some(BendValue::Deduction(1.0));
    assert!(matches!(flatten(&d).errors.as_slice(), [FlatError::BadJoint { .. }]));
}

#[test]
fn hem_alignments_and_sides() {
    let edge = (P3::new(50.0, 0.0, 0.0), P3::new(50.0, 20.0, 0.0));
    for (toward, align, short) in [
        (true, HemAlignment::InPlace, 0.0),
        (true, HemAlignment::Outer, R + T),
        (false, HemAlignment::Outer, R + T),
    ] {
        let mut b = SharpBuilder::new(params());
        let a = b.wall(P3::origin(), V3::x(), V3::y(), rect(50.0, 20.0));
        b.hem(a, edge, 10.0, toward, align);
        let m = ok(b.build());
        assert!(close(size(&m).0, 50.0 - short + PI * (R + K * T) + 10.0), "{toward} {align:?}");
        assert_eq!(table(&m).bends[0].direction == BendDirection::Up, toward);
    }
}

#[test]
fn rolled_tube_unrolls_at_its_neutral_radius() {
    // A full tube of inner radius 10, 1 thick: 2π(10 + 0.5·1) wide.
    let p = Params {
        thickness: 1.0,
        ..params()
    };
    let (w, h) = size(&samples::tube(p));
    assert!(close(w, 2.0 * PI * 10.5), "{w}");
    assert!(close(h, 30.0));
}

#[test]
fn planar_wall_runs_into_a_rolled_one() {
    let p = Params {
        thickness: 1.0,
        ..params()
    };
    let m = samples::wall_into_half_tube(p);
    assert!(m.validate().is_empty(), "{:?}", m.validate());
    assert!(close(size(&m).0, 20.0 + PI * 10.5));
    let t = table(&m);
    assert!(t.bends.is_empty(), "rolled walls aren't bends");
    assert_eq!(t.joints[0].kind, JointType::Tangent);
    assert_eq!(one_part(&m).bends.len(), 0, "no bend lines for a rolled wall");
}

#[test]
fn flip_direction_up_flips_up_and_down() {
    let m = ok(samples::l_bracket(params(), true));
    let up = one_part(&m);
    assert!(up.bends[0].up);
    assert_eq!(table(&m).bends[0].direction, BendDirection::Up);
    let mut flipped = m.clone();
    flipped.params.flip_direction_up = true;
    let f = one_part(&flipped);
    assert!(!f.bends[0].up);
    assert_eq!(table(&flipped).bends[0].direction, BendDirection::Down);
    let (a, b) = (up.bounds().unwrap(), f.bounds().unwrap());
    assert!(close(a.1.x - a.0.x, b.1.x - b.0.x));
    assert!(b.0.x < 0.0, "mirrored");
}

#[test]
fn bend_lines_sit_between_the_tangent_lines() {
    let part = one_part(&ok(samples::l_bracket(params(), true)));
    let b = &part.bends[0];
    let x0 = 50.0 - OSSB;
    assert!(close(b.tangent_a.a.x, x0));
    assert!(close(b.tangent_b.a.x, x0 + ba90()));
    assert!(close(b.center.a.x, x0 + ba90() / 2.0));
    assert!(close(b.center.len(), 40.0));
    assert_eq!(b.center_visible.len(), 1);
    assert!((b.center_visible[0].len() - 40.0).abs() < 1e-6);
    assert_eq!(b.tangent_visible.len(), 2);
}

// ---------------------------------------------------------------------------------------------
// The open box: rips, bend extents, corner reliefs

#[test]
fn open_box_lays_flat_as_a_cross() {
    let m = ok(samples::open_box(params(), RipStyle::EdgeJoint));
    let part = one_part(&m);
    assert_eq!(part.outline.len(), 1);
    let (lo, hi) = part.bounds().unwrap();
    assert!(close(hi.x - lo.x, 100.0 + 2.0 * (20.0 - 2.0 * OSSB + ba90())));
    assert!(close(hi.y - lo.y, 60.0 + 2.0 * (20.0 - 2.0 * OSSB + ba90())));
    assert_eq!(part.corners.len(), 4);
    let t = table(&m);
    assert_eq!((t.bends.len(), t.joints.len()), (4, 4));
    assert!(t.joints.iter().all(|j| j.kind == JointType::Rip && j.style == Some(RipStyle::EdgeJoint)));
    assert_eq!(t.joints[0].name, "Rip 1");
    // Edge joints: every wall stops T + gap/2 short of each corner, so every bend is that much
    // shorter than its edge at both ends.
    let lens: Vec<f64> = m.joints.iter().filter_map(|j| j.bend()).map(|b| b.on_a.len()).collect();
    for (l, e) in lens.iter().zip([60.0, 100.0, 60.0, 100.0]) {
        assert!(close(*l, e - 2.0 * (T + GAP / 2.0)), "{l}");
    }
}

/// Bend lengths by wall (east, north, west, south: walls 1..4), whatever the joint order.
fn bend_lengths(m: &Model) -> Vec<f64> {
    (1..=4)
        .map(|w| m.joints.iter().find(|j| j.b == WallId(w)).and_then(|j| j.bend()).map(|b| b.on_a.len()).unwrap())
        .collect()
}

/// The open box with its bends added in the opposite order.
fn open_box_reversed(style: RipStyle) -> Result<Model, BuildError> {
    let (x, y, h) = (100.0, 60.0, 20.0);
    let mut b = SharpBuilder::new(params());
    let base = b.wall(P3::origin(), V3::x(), V3::y(), rect(x, y));
    let east = b.wall(P3::new(x, 0.0, 0.0), V3::z(), V3::y(), rect(h, y));
    let north = b.wall(P3::new(x, y, 0.0), V3::z(), -V3::x(), rect(h, x));
    let west = b.wall(P3::new(0.0, y, 0.0), V3::z(), -V3::y(), rect(h, y));
    let south = b.wall(P3::new(0.0, 0.0, 0.0), V3::z(), V3::x(), rect(h, x));
    b.bend(base, south, (P3::new(0.0, 0.0, 0.0), P3::new(x, 0.0, 0.0)));
    b.bend(base, west, (P3::new(0.0, y, 0.0), P3::new(0.0, 0.0, 0.0)));
    b.bend(base, north, (P3::new(x, y, 0.0), P3::new(0.0, y, 0.0)));
    b.bend(base, east, (P3::new(x, 0.0, 0.0), P3::new(x, y, 0.0)));
    b.rip(east, north, (P3::new(x, y, 0.0), P3::new(x, y, h)), style);
    b.rip(north, west, (P3::new(0.0, y, 0.0), P3::new(0.0, y, h)), style);
    b.rip(west, south, (P3::new(0.0, 0.0, 0.0), P3::new(0.0, 0.0, h)), style);
    b.rip(south, east, (P3::new(x, 0.0, 0.0), P3::new(x, 0.0, h)), style);
    b.build()
}

#[test]
fn butt_joint_bend_lengths_dont_depend_on_bend_order() {
    for style in [RipStyle::ButtDirection1, RipStyle::ButtDirection2] {
        let forward = ok(samples::open_box(params(), style));
        let reversed = ok(open_box_reversed(style));
        let (f, r) = (bend_lengths(&forward), bend_lengths(&reversed));
        for (a, b) in f.iter().zip(&r) {
            assert!(close(*a, *b), "{style:?}: {f:?} vs {r:?}");
        }
        // Each wall is the first wall of one rip (trimmed T + gap there) and the second of
        // another (not trimmed): one short end each.
        for (l, e) in f.iter().zip([60.0, 100.0, 60.0, 100.0]) {
            assert!(close(*l, e - (T + GAP)), "{style:?}: {f:?}");
        }
        assert!(flatten(&forward).is_ok(), "{:?}", flatten(&forward).errors);
    }
}

/// The NE corner zone of the box: where the east and north bend regions, extended, cross.
fn ne_zone() -> (P2, P2) {
    (P2::new(100.0 - OSSB, 60.0 - OSSB), P2::new(100.0 - OSSB + ba90(), 60.0 - OSSB + ba90()))
}

fn box_with_corner(kind: CornerReliefKind, size: f64, scale: f64) -> FlatPart {
    let mut p = params();
    p.corner_relief = CornerRelief { kind, size, scale };
    one_part(&ok(samples::open_box(p, RipStyle::EdgeJoint)))
}

/// The base ends at the bends' tangent lines everywhere (its corners past them came off).
fn m_base_has_no_corner(part: &FlatPart) -> bool {
    let base = part.piece(PieceSource::Wall(WallId(0))).unwrap();
    let (lo, hi) = bounds(&base.polygon);
    close_pt(lo, P2::new(OSSB, OSSB)) && close_pt(hi, P2::new(100.0 - OSSB, 60.0 - OSSB))
}

fn ne_cut(part: &FlatPart) -> &ReliefCut {
    part.cuts
        .iter()
        .find(|c| c.source == ReliefSource::Corner { bends: (JointId(0), JointId(1)) })
        .expect("the east/north corner was cut")
}

#[test]
fn corner_zone_is_where_the_bend_regions_cross() {
    let part = box_with_corner(CornerReliefKind::Simple, 0.0, 1.0);
    let c = part.corners.iter().find(|c| c.bends == (JointId(0), JointId(1))).unwrap();
    assert_eq!(c.ends, (BendEnd::End, BendEnd::Start));
    let (lo, hi) = bounds(&c.zone);
    assert!(close_pt(lo, ne_zone().0) && close_pt(hi, ne_zone().1), "{lo:?} {hi:?}");
    // Simple: just the zone. (The base's corner past both tangent lines came off with the bend
    // trims: nothing held it to the base.)
    let cut = ne_cut(&part);
    assert_eq!(cut.shapes.len(), 1);
    let (zl, zh) = bounds(&cut.shapes[0]);
    assert!(close_pt(zl, ne_zone().0) && close_pt(zh, ne_zone().1));
    assert!(m_base_has_no_corner(&part));
    assert!(cut.slit.is_none());
}

#[test]
fn every_corner_relief_type_has_its_exact_shape() {
    let (zlo, zhi) = ne_zone();
    let centre = P2::from((zlo.coords + zhi.coords) / 2.0);
    let ba = ba90();
    for (kind, half) in [
        (CornerReliefKind::SquareSized, 6.0),
        (CornerReliefKind::RoundSized, 6.0),
        (CornerReliefKind::RectangleScaled, 1.5 * ba / 2.0),
        (CornerReliefKind::RoundScaled, 1.5 * ba / 2.0),
    ] {
        let part = box_with_corner(kind, 12.0, 1.5);
        let cut = ne_cut(&part);
        // The zone, then the relief's own shape centred on it.
        assert_eq!(cut.shapes.len(), 2, "{kind:?}");
        let (lo, hi) = bounds(&cut.shapes[1]);
        assert!(close_pt(P2::from((lo.coords + hi.coords) / 2.0), centre), "{kind:?} centre");
        assert!((hi.x - lo.x - 2.0 * half).abs() < 1e-3, "{kind:?} size {}", hi.x - lo.x);
        let round = matches!(kind, CornerReliefKind::RoundSized | CornerReliefKind::RoundScaled);
        let expect_area = if round { PI * half * half } else { 4.0 * half * half };
        assert!((cut.shapes[1].area() - expect_area).abs() / expect_area < 0.005, "{kind:?} area");
        // The cut only touches the pieces around the corner.
        assert!(cut.targets.contains(&PieceSource::Wall(WallId(0))));
        assert!(!cut.targets.contains(&PieceSource::Wall(WallId(3))), "not the west wall");
    }
}

#[test]
fn closed_corner_mitres_the_bend_regions_with_the_minimal_gap() {
    let simple = box_with_corner(CornerReliefKind::Simple, 0.0, 1.0);
    let closed = box_with_corner(CornerReliefKind::Closed, 0.0, 1.0);
    // Closed keeps more material than Simple, and never lets the two bend regions overlap.
    assert!(closed.area() > simple.area());
    let east = &closed.piece(PieceSource::Bend(JointId(0))).unwrap().cut;
    let north = &closed.piece(PieceSource::Bend(JointId(1))).unwrap().cut;
    let overlap: f64 = east.iter().flat_map(|a| north.iter().map(move |b| overlap_area(a, b))).sum();
    assert!(overlap < 1e-9);
    // The diagonal itself stays open: the minimal gap runs along it.
    let (zlo, _) = ne_zone();
    let probe = zlo + V2::new(1.0, 1.0) * 1.0;
    assert!(!east.iter().chain(north.iter()).any(|p| p.contains(probe)));
}

#[test]
fn corner_override_changes_one_corner() {
    let base = box_with_corner(CornerReliefKind::Simple, 0.0, 1.0).area();
    let all = box_with_corner(CornerReliefKind::RoundSized, 12.0, 1.5).area();
    let mut m = ok(samples::open_box(params(), RipStyle::EdgeJoint));
    m.corner_overrides.push(CornerOverride {
        bends: (JointId(0), JointId(1)),
        relief: CornerRelief {
            kind: CornerReliefKind::RoundSized,
            size: 12.0,
            scale: 1.5,
        },
    });
    let one = one_part(&m).area();
    // One corner of four: a quarter of the extra material.
    assert!(((base - one) - (base - all) / 4.0).abs() < 1e-3, "{base} {one} {all}");
}

// ---------------------------------------------------------------------------------------------
// Bend reliefs on a partial flange (flange on y 10..30 of the base's 40 long x = 50 edge)

fn partial(kind: BendReliefKind, depth: f64, extend: bool) -> FlatPart {
    let mut p = params();
    p.bend_relief.kind = kind;
    p.bend_relief.depth = depth;
    p.bend_relief.extend = extend;
    one_part(&ok(samples::partial_flange(p)))
}

#[test]
fn every_bend_relief_type_has_its_exact_extent() {
    let x0 = 50.0 - OSSB; // the base's tangent line
    let x1 = x0 + ba90(); // the flange's
    // (type, extra depth into the base, width)
    for (kind, extra, width) in [
        (BendReliefKind::RectangleScaled, R, T), // depth scale 2: one radius past the tangent line
        (BendReliefKind::ObroundScaled, R, T),
        (BendReliefKind::SquareSized, 4.0, T),
        (BendReliefKind::ObroundSized, 4.0, T),
    ] {
        let part = partial(kind, 4.0, false);
        assert_eq!(part.cuts.len(), 2, "{kind:?}: one at each end");
        for (end, y0, y1) in [(BendEnd::End, 30.0, 30.0 + width), (BendEnd::Start, 10.0 - width, 10.0)] {
            let cut = part
                .cuts
                .iter()
                .find(|c| c.source == ReliefSource::BendEnd { bend: JointId(0), end })
                .unwrap_or_else(|| panic!("{kind:?} {end:?}"));
            assert_eq!(cut.shapes.len(), 1);
            let (lo, hi) = bounds(&cut.shapes[0]);
            assert!(close_pt(lo, P2::new(x0 - extra, y0)) && close_pt(hi, P2::new(x1, y1)), "{kind:?} {end:?}: {lo:?} {hi:?}");
            // It removes material from the base only, in the base's own coordinates: from the
            // relief's depth to the base's edge at x = 50 (the flange starts past x1).
            let removed: f64 = cut
                .removed
                .iter()
                .filter(|(s, _)| *s == PieceSource::Wall(WallId(0)))
                .flat_map(|(_, v)| v)
                .map(Polygon::area)
                .sum();
            let square = (50.0 - (x0 - extra)) * width;
            let rounded = matches!(kind, BendReliefKind::ObroundScaled | BendReliefKind::ObroundSized);
            let corners = if rounded { width * width / 2.0 * (1.0 - PI / 4.0) } else { 0.0 };
            assert!((removed - (square - corners)).abs() < 0.01, "{kind:?}: removed {removed}");
        }
    }
}

#[test]
fn tear_relief_removes_nothing_and_slits_the_end() {
    let part = partial(BendReliefKind::Tear, 4.0, false);
    let full = partial(BendReliefKind::RectangleScaled, 4.0, false);
    assert_eq!(part.cuts.len(), 2);
    for c in &part.cuts {
        assert!(c.shapes.is_empty());
        assert!(close(c.slit.expect("a slit").len(), ba90()));
    }
    assert!(part.area() > full.area());
    let pieces: f64 = part.pieces.iter().map(|p| p.polygon.area()).sum();
    assert!((part.area() - pieces).abs() < 1e-3, "no material removed");
}

#[test]
fn extended_relief_runs_to_the_edge_and_stays_on_its_own_pieces() {
    let part = partial(BendReliefKind::RectangleScaled, 4.0, true);
    for c in &part.cuts {
        assert!(c.targets.iter().all(|t| matches!(t, PieceSource::Wall(w) if w.0 <= 1) || matches!(t, PieceSource::Bend(_))));
    }
    // The base loses a slot from each bend end right to its edge (y = 40 and y = 0).
    let base = &part.piece(PieceSource::Wall(WallId(0))).unwrap().cut;
    assert!(!base.iter().any(|q| q.contains(P2::new(46.0, 39.5))));
    assert!(!base.iter().any(|q| q.contains(P2::new(46.0, 0.5))));
    assert!(base.iter().any(|q| q.contains(P2::new(20.0, 39.5))), "only along the bend");
}

#[test]
fn bend_lines_stop_at_corner_reliefs() {
    let part = box_with_corner(CornerReliefKind::RoundSized, 12.0, 1.5);
    let b = part.bend(JointId(0)).unwrap();
    let visible: f64 = b.center_visible.iter().map(|s| s.len()).sum();
    assert!(visible < b.center.len() - 1.0, "{visible} of {}", b.center.len());
    assert!(!b.tangent_visible.is_empty());
}

// ---------------------------------------------------------------------------------------------
// Errors

#[test]
fn overlapping_walls_report_a_collision_with_its_region() {
    let f = flatten(&ok(samples::hook_collision(params())));
    let (a, b, area, region) = f
        .errors
        .iter()
        .find_map(|e| match e {
            FlatError::Collision { a, b, area, region } => Some((*a, *b, *area, region.clone())),
            _ => None,
        })
        .expect("collision reported");
    assert_eq!((a, b), (PieceSource::Wall(WallId(1)), PieceSource::Wall(WallId(2))));
    assert!(area > 10.0, "{area}");
    assert!((region.iter().map(Polygon::area).sum::<f64>() - area).abs() < 1e-9);
    assert_eq!(f.errors[0].message(), "Collision in sheet metal flat pattern");
}

#[test]
fn walls_that_only_touch_dont_collide() {
    for style in RipStyle::ALL {
        let f = flatten(&ok(samples::open_box(params(), style)));
        assert!(f.is_ok(), "{style:?}: {:?}", f.errors);
    }
}

#[test]
fn bends_closing_a_loop_are_reported() {
    let f = flatten(&ok(samples::bend_loop(params())));
    assert!(f.errors.iter().any(|e| matches!(e, FlatError::BendLoop { .. })), "{:?}", f.errors);
}

#[test]
fn inconsistent_material_sides_are_rejected() {
    let mut b = SharpBuilder::new(params());
    let a = b.wall(P3::origin(), V3::x(), V3::y(), rect(50.0, 40.0));
    // The flange's material side points the wrong way (outwards).
    let f = b.wall(P3::new(50.0, 40.0, 0.0), V3::z(), -V3::y(), rect(30.0, 40.0));
    b.bend(a, f, (P3::new(50.0, 0.0, 0.0), P3::new(50.0, 40.0, 0.0)));
    let e = b.build().unwrap_err();
    assert!(matches!(e, BuildError::InconsistentSide { .. }));
    assert_eq!(e.to_string(), "The walls' material sides don't match across the joint");
}

// ---------------------------------------------------------------------------------------------
// 3D, table, persistence

#[test]
fn bend_geometry_in_3d() {
    let m = ok(samples::l_bracket(params(), true));
    let g = m.bend_geometry(JointId(0)).unwrap();
    // The bend's axis sits R + T above the base's underside, over the tangent line x = 45.
    assert!((g.ends.0 - P3::new(45.0, 0.0, 5.0)).norm() < 1e-9 && (g.ends.1 - P3::new(45.0, 40.0, 5.0)).norm() < 1e-9);
    assert_eq!((g.inner_radius, g.outer_radius, g.def_radius), (R, R + T, R + T));
    // Turning the base's tangent line by the sweep lands on the flange's (5 up the x = 50 face).
    assert!((g.rotate(P3::new(45.0, 0.0, 0.0), g.sweep) - P3::new(50.0, 0.0, 5.0)).norm() < 1e-9);
    let down = ok(samples::l_bracket(params(), false));
    let g = down.bend_geometry(JointId(0)).unwrap();
    assert!((g.ends.0 - P3::new(47.0, 0.0, -3.0)).norm() < 1e-9, "{:?}", g.ends);
    assert_eq!(g.def_radius, R);
}

#[test]
fn every_sample_is_consistent_in_3d() {
    let p = params();
    for m in [
        samples::l_bracket(p, true),
        samples::l_bracket(p, false),
        samples::u_channel(p),
        samples::open_box(p, RipStyle::EdgeJoint),
        samples::open_box(p, RipStyle::ButtDirection1),
        samples::open_box(p, RipStyle::ButtDirection2),
        samples::partial_flange(p),
        samples::hem(p),
        samples::hook_collision(p),
        samples::bend_loop(p),
    ] {
        ok(m);
    }
    assert!(samples::tube(p).validate().is_empty());
    assert!(samples::wall_into_half_tube(p).validate().is_empty());
}

#[test]
fn validate_catches_a_misplaced_wall() {
    let mut m = ok(samples::l_bracket(params(), true));
    if let Surface::Planar { origin, .. } = &mut m.walls[1].surface {
        origin.x += 1.0;
    }
    let errs = m.validate();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].joint, JointId(0));
}

#[test]
fn moving_a_bend_stays_within_its_table() {
    let mut m = ok(samples::open_box(params(), RipStyle::EdgeJoint));
    assert!(m.move_joint(JointId(1), -1));
    let t = table(&m);
    assert_eq!((t.bends[0].name.as_str(), t.bends[0].number), ("Bend B", 1));
    assert_eq!(t.bends[1].name, "Bend A");
    assert!(!m.move_joint(JointId(1), -1), "already first");
    // The last bend can't move down past the rips into another table.
    assert!(!m.move_joint(JointId(3), 1));
    // A rip moves among the rips.
    assert!(m.move_joint(JointId(5), -1));
    assert_eq!(table(&m).joints[0].name, "Rip 2");
    assert!(flatten(&m).is_ok());
}

#[test]
fn table_value_column_follows_the_calculation() {
    let mut m = ok(samples::l_bracket(params(), true));
    assert_eq!(table(&m).value_column, "K Factor");
    assert_eq!(table(&m).bends[0].value, Some(BendValue::KFactor(K)));
    m.params.bend_calc = BendCalc::BendDeduction;
    let t = table(&m);
    assert_eq!(t.value_column, "Bend deduction (mm)");
    assert_eq!(t.bends[0].value, Some(BendValue::Deduction(m.params.bend_deduction)));
}

#[test]
fn trimmed_walls_keep_exact_coordinates() {
    let m = ok(samples::open_box(params(), RipStyle::EdgeJoint));
    // No 57.89999997… left by the polygon booleans.
    for w in &m.walls {
        for p in &w.outline.outer {
            for v in [p.x, p.y] {
                let snapped = (v * 1000.0).round() / 1000.0;
                assert!((v - snapped).abs() < 1e-12 || (v - snapped).abs() > 1e-4, "{v}");
            }
        }
    }
}

#[test]
fn model_and_flat_round_trip_through_ron() {
    let m = ok(samples::open_box(params(), RipStyle::ButtDirection2));
    let back: Model = ron::from_str(&ron::to_string(&m).unwrap()).unwrap();
    assert_eq!(back, m);
    let f = flatten(&m);
    let back: FlatPattern = ron::from_str(&ron::to_string(&f).unwrap()).unwrap();
    assert_eq!(back.parts.len(), f.parts.len());
    // Settings saved by an older version (fields missing) load with defaults.
    let p: Params = ron::from_str("(thickness: 1.5)").unwrap();
    assert_eq!((p.thickness, p.k_factor), (1.5, 0.45));
}

// ---------------------------------------------------------------------------------------------
// Round 2: corners only where bends meet, hems in every mode, persistent ids, exact outputs

/// A 100 × 60 base with a full north flange and an east flange covering only y 0..`east`.
fn north_and_short_east(east: f64) -> Model {
    let mut b = SharpBuilder::new(params());
    let base = b.wall(P3::origin(), V3::x(), V3::y(), rect(100.0, 60.0));
    let e = b.wall(P3::new(100.0, 0.0, 0.0), V3::z(), V3::y(), rect(20.0, east));
    let n = b.wall(P3::new(100.0, 60.0, 0.0), V3::z(), -V3::x(), rect(20.0, 100.0));
    b.bend(base, e, (P3::new(100.0, 0.0, 0.0), P3::new(100.0, east, 0.0)));
    b.bend(base, n, (P3::new(100.0, 60.0, 0.0), P3::new(0.0, 60.0, 0.0)));
    ok(b.build())
}

#[test]
fn a_corner_needs_the_bends_to_meet() {
    for east in [10.0, 20.0, 30.0, 40.0, 50.0] {
        let part = one_part(&north_and_short_east(east));
        assert!(part.corners.is_empty(), "east {east}: the east bend stops well short of the north one");
        // The east bend's far end gets a bend relief instead; the north bend region is whole.
        assert!(part.cuts.iter().any(|c| c.source == ReliefSource::BendEnd { bend: JointId(0), end: BendEnd::End }), "east {east}");
        let north = part.piece(PieceSource::Bend(JointId(1))).unwrap();
        assert!((north.cut.iter().map(Polygon::area).sum::<f64>() - north.polygon.area()).abs() < 1e-6, "east {east}");
    }
    // Close to the zone (it starts at 55) but with the base carrying on past the bend's end: still
    // a bend relief, not a corner.
    for east in [52.9, 53.0, 54.9] {
        let part = one_part(&north_and_short_east(east));
        assert!(part.corners.is_empty(), "east {east}");
        assert!(part.cuts.iter().any(|c| c.source == ReliefSource::BendEnd { bend: JointId(0), end: BendEnd::End }), "east {east}");
    }
    // Running right up to the north wall (55 = where the north bend region begins) it is a corner.
    let part = one_part(&north_and_short_east(60.0));
    assert_eq!(part.corners.len(), 1);
}

#[test]
fn hems_lay_flat_in_every_calculation_mode() {
    let hem = PI * (R + K * T); // a hem's allowance from the model K factor
    for (calc, value, bend_allowance) in [
        (BendCalc::KFactor, K, hem),
        // The model's allowance is for its ordinary bends: hems use the model's K factor.
        (BendCalc::BendAllowance, 9.0, hem),
        // Deduction has no meaning at 180°: the hem uses the model's K factor.
        (BendCalc::BendDeduction, 4.0, hem),
    ] {
        let mut p = params();
        p.bend_calc = calc;
        p.k_factor = K;
        p.bend_allowance = value;
        p.bend_deduction = value;
        let m = ok(samples::hem(p));
        assert!(close(size(&m).0, 50.0 + bend_allowance + 10.0), "{calc:?}");
    }
}

#[test]
fn ids_and_names_follow_creation_order_and_can_be_given() {
    // A hem made before a bend comes first in the table.
    let mut b = SharpBuilder::new(params());
    let base = b.wall(P3::origin(), V3::x(), V3::y(), rect(50.0, 40.0));
    let h = b.hem(base, (P3::new(0.0, 40.0, 0.0), P3::new(0.0, 0.0, 0.0)), 8.0, true, HemAlignment::InPlace);
    let f = b.wall(P3::new(50.0, 0.0, 0.0), V3::z(), V3::y(), rect(30.0, 40.0));
    let j = b.bend(base, f, (P3::new(50.0, 0.0, 0.0), P3::new(50.0, 40.0, 0.0)));
    let m = ok(b.build());
    let t = table(&m);
    assert_eq!(t.bends[0].name, "Bend A");
    assert_eq!(t.bends[0].joint, m.joints[0].id);
    assert!(m.joints[0].bend().unwrap().hem);
    // Features give persistent ids and names; the rest are numbered around them.
    b.set_joint_id(j, JointId(70), Some("Flange 1 bend".into()));
    b.set_hem_id(h, JointId(71), WallId(90), None);
    b.set_wall_id(f, WallId(80));
    let m = ok(b.build());
    assert_eq!(m.joints.iter().map(|j| (j.id, j.name.as_str())).collect::<Vec<_>>(), [(JointId(71), "Bend A"), (JointId(70), "Flange 1 bend")]);
    let ids: Vec<WallId> = m.walls.iter().map(|w| w.id).collect();
    assert_eq!(ids, [WallId(0), WallId(80), WallId(90)]);
    // Duplicates are refused.
    b.set_wall_id(base, WallId(80));
    assert_eq!(b.build().unwrap_err(), BuildError::DuplicateWallId { id: WallId(80) });
}

#[test]
fn hems_stop_where_their_wall_does() {
    // An L whose base also gets a hem along its y = 0 edge: the bend trimmed that edge to x 0..45.
    let mut b = SharpBuilder::new(params());
    let base = b.wall(P3::origin(), V3::x(), V3::y(), rect(50.0, 40.0));
    let f = b.wall(P3::new(50.0, 0.0, 0.0), V3::z(), V3::y(), rect(30.0, 40.0));
    b.bend(base, f, (P3::new(50.0, 0.0, 0.0), P3::new(50.0, 40.0, 0.0)));
    b.hem(base, (P3::new(0.0, 0.0, 0.0), P3::new(50.0, 0.0, 0.0)), 8.0, true, HemAlignment::InPlace);
    let m = ok(b.build());
    let hem = m.joints.iter().find_map(|j| j.bend().filter(|b| b.hem)).unwrap();
    assert!(close(hem.on_a.len(), 50.0 - OSSB), "{}", hem.on_a.len());
}

#[test]
fn validate_checks_joints_along_their_whole_length() {
    use cadrs_sheetmetal::model::ModelErrorKind;
    let mut m = ok(samples::l_bracket(params(), true));
    // Stretch the bend's tangent line far past the base's edge (the flange keeps up).
    let JointKind::Bend(b) = &mut m.joints[0].kind else { panic!() };
    b.on_a.b.y += 30.0;
    b.on_b.b.y += 30.0;
    let errs = m.validate();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].kind, ModelErrorKind::NotOnEdge { second: false });
    assert_eq!(errs[0].to_string(), "The joint doesn't run along its wall's edge");
}

#[test]
fn flat_outputs_have_exact_coordinates() {
    let m = ok(samples::open_box(params(), RipStyle::EdgeJoint));
    let part = one_part(&m);
    // Every outline vertex is a piece corner or a cut corner, to the last bit.
    let exact: Vec<P2> = part.pieces.iter().flat_map(|p| p.polygon.outer.clone()).chain(part.cuts.iter().flat_map(|c| c.shapes.iter().flat_map(|s| s.outer.clone()))).collect();
    for o in &part.outline {
        for v in &o.outer {
            assert!(exact.iter().any(|e| e == v), "{v:?} isn't exact");
        }
    }
    let (lo, _) = part.bounds().unwrap();
    assert!((lo.x + (20.0 - 2.0 * OSSB + ba90())).abs() < 1e-12, "{}", lo.x);
}

#[test]
fn negative_allowances_are_reported_as_such() {
    let p = Params {
        bend_calc: BendCalc::BendDeduction,
        bend_deduction: 4.0,
        ..params()
    };
    let mut b = SharpBuilder::new(p);
    let a = b.wall(P3::origin(), V3::x(), V3::y(), rect(50.0, 40.0));
    let theta = 10f64.to_radians();
    let f = b.wall(P3::new(50.0, 0.0, 0.0), V3::new(theta.cos(), 0.0, theta.sin()), V3::y(), rect(30.0, 40.0));
    b.bend(a, f, (P3::new(50.0, 0.0, 0.0), P3::new(50.0, 40.0, 0.0)));
    let errs = flatten(&ok(b.build())).errors;
    assert!(
        matches!(errs.as_slice(), [FlatError::BadJoint { problem: cadrs_sheetmetal::flat::JointProblem::NegativeAllowance { .. }, .. }]),
        "{errs:?}"
    );
}

#[test]
fn validate_tolerance_isnt_widened_by_hems() {
    use cadrs_sheetmetal::model::ModelErrorKind;
    let mut b = SharpBuilder::new(params());
    let base = b.wall(P3::origin(), V3::x(), V3::y(), rect(50.0, 40.0));
    let f = b.wall(P3::new(50.0, 0.0, 0.0), V3::z(), V3::y(), rect(30.0, 40.0));
    b.bend(base, f, (P3::new(50.0, 0.0, 0.0), P3::new(50.0, 40.0, 0.0)));
    b.hem(base, (P3::new(0.0, 40.0, 0.0), P3::new(0.0, 0.0, 0.0)), 8.0, true, HemAlignment::InPlace);
    let mut m = ok(b.build());
    let JointKind::Bend(bend) = &mut m.joints[0].kind else { panic!() };
    bend.on_a.b.y += 30.0;
    bend.on_b.b.y += 30.0;
    assert_eq!(m.validate().first().map(|e| e.kind), Some(ModelErrorKind::NotOnEdge { second: false }));
}

#[test]
fn the_table_shows_what_the_flat_uses_for_hems() {
    let p = Params {
        bend_calc: BendCalc::BendAllowance,
        bend_allowance: 9.0,
        ..params()
    };
    let m = ok(samples::hem(p));
    assert_eq!(table(&m).bends[0].value, Some(BendValue::Allowance(PI * (R + K * T))));
}
