//! Flange, Hem and Make joint on a model's definition (P3I.4, `cadrs_sheetmetal::sharp_edit`), checked
//! against hand calculations.
//!
//! T = 2, R = 3, K = 0.45: a 90° bend has an outside setback `OSSB = R + T = 5`, an inside one
//! `ISSB = R = 3` and an allowance `BA = π/2 · (R + K·T) ≈ 6.126`. A flange's Distance runs from
//! the outer virtual sharp to its tip, so its wall is `D − OSSB` long; the wall it stands on ends
//! `OSSB` short of the flange's outer sharp, which the alignment puts `T·tan(θ/2)` (Inner),
//! `T·tan(θ/2)/2` (Middle), 0 (Outer) or `OSSB` (Hold line) past the edge.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

use cadrs_sheetmetal::sharp_edit::{
    self as edit, EdgeFrame, EdgePick, FlangeAlignment, FlangeEdge, FlangeOpts, HemEdge, HemKind, HemOpts, JointSpec, PickSide, SharpDef,
};
use cadrs_sheetmetal::model::{HemAlignment, JointKind, Model, P3, RipStyle, SharpBuilder, Surface, V3};
use cadrs_sheetmetal::params::Params;
use cadrs_sheetmetal::poly::{P2, Polygon};
use cadrs_sheetmetal::{bend, flatten};

const T: f64 = 2.0;
const R: f64 = 3.0;
const K: f64 = 0.45;
const GAP: f64 = 0.2;
const OSSB: f64 = R + T;
const TOL: f64 = 1e-6;

fn params() -> Params {
    Params { thickness: T, bend_radius: R, k_factor: K, minimal_gap: GAP, ..Default::default() }
}

fn ba(theta: f64, r: f64) -> f64 {
    bend::bend_allowance(r, T, theta, K)
}

/// A 50 × 40 base in the XY plane, material above (z 0..T).
fn base() -> SharpDef {
    let mut b = SharpBuilder::new(params());
    let w = b.wall(P3::origin(), V3::x(), V3::y(), Polygon::rect(P2::origin(), P2::new(50.0, 40.0)));
    b.set_wall_id(w, cadrs_sheetmetal::WallId(1));
    let m = b.build().unwrap();
    SharpDef::new(b, &m, m.walls.len(), m.joints.len())
}

fn model(def: &SharpDef) -> Model {
    let m = def.build().expect("builds");
    assert!(m.validate().is_empty(), "consistent in 3D: {:?}", m.validate());
    m
}

/// Picks the top (material face) edge between two points.
fn pick(m: &Model, a: P3, b: P3) -> EdgePick {
    let p = edit::locate(m, &[a, b]).expect("on a wall edge");
    assert!(!p.joined);
    p
}

fn size(m: &Model) -> (f64, f64) {
    let f = flatten(m);
    assert!(f.is_ok(), "{:?}", f.errors);
    assert_eq!(f.parts.len(), 1);
    let (lo, hi) = f.parts[0].bounds().unwrap();
    (hi.x - lo.x, hi.y - lo.y)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < TOL
}

fn east_edge(m: &Model) -> EdgePick {
    pick(m, P3::new(50.0, 0.0, T), P3::new(50.0, 40.0, T))
}

fn flange_on(def: &mut SharpDef, edges: Vec<(EdgePick, f64, bool, f64, Option<(f64, f64)>)>, alignment: FlangeAlignment, miter: Option<f64>, hold: bool) {
    let fe: Vec<FlangeEdge> = edges
        .into_iter()
        .enumerate()
        .map(|(i, (pick, angle, toward, distance, partial))| FlangeEdge { pick, key: 100 + i as u64, angle, toward, distance, partial })
        .collect();
    edit::flange(def, &fe, &FlangeOpts { alignment, radius: None, miter, hold_adjacent: hold }).expect("flange");
}

#[test]
fn locate_finds_the_face_a_picked_edge_is_on() {
    let def = base();
    let m = model(&def);
    assert_eq!(east_edge(&m).side, PickSide::Material);
    let bottom = pick(&m, P3::new(50.0, 0.0, 0.0), P3::new(50.0, 40.0, 0.0));
    assert_eq!(bottom.side, PickSide::Definition);
    // A side face: its corners.
    let side = edit::locate(&m, &[P3::new(50.0, 0.0, 0.0), P3::new(50.0, 40.0, 0.0), P3::new(50.0, 40.0, T), P3::new(50.0, 0.0, T)]).unwrap();
    assert_eq!(side.side, PickSide::Side);
    assert!(edit::locate(&m, &[P3::new(20.0, 10.0, 0.0), P3::new(30.0, 10.0, 0.0)]).is_none());
}

#[test]
fn flange_alignments_move_the_bend() {
    for (align, base_len) in [
        (FlangeAlignment::Inner, 50.0 + T - OSSB),
        (FlangeAlignment::Outer, 50.0 - OSSB),
        (FlangeAlignment::Middle, 50.0 + T / 2.0 - OSSB),
        (FlangeAlignment::HoldLine, 50.0),
    ] {
        let mut def = base();
        let m = model(&def);
        flange_on(&mut def, vec![(east_edge(&m), FRAC_PI_2, true, 30.0, None)], align, None, true);
        let m = model(&def);
        let (w, h) = size(&m);
        let want = base_len + ba(FRAC_PI_2, R) + (30.0 - OSSB);
        assert!(close(w, want), "{align:?}: {w} vs {want}");
        assert!(close(h, 40.0));
        // Its outer face reaches the distance above the base's outer (bottom) face.
        let top = m.walls.iter().flat_map(|wl| wl.outline.outer.iter().map(|q| wl.surface.point(*q).z)).fold(f64::MIN, f64::max);
        assert!(close(top, 30.0), "{align:?}: {top}");
        assert_eq!(m.joints[0].name, "Bend A");
    }
}

#[test]
fn flange_away_from_the_material_measures_from_the_outer_sharp_too() {
    let mut def = base();
    let m = model(&def);
    // Down from the bottom-face edge (away from the material): the outside is now the top.
    let p = pick(&m, P3::new(50.0, 0.0, 0.0), P3::new(50.0, 40.0, 0.0));
    flange_on(&mut def, vec![(p, FRAC_PI_2, false, 30.0, None)], FlangeAlignment::Inner, None, true);
    let m = model(&def);
    let (w, _) = size(&m);
    assert!(close(w, 50.0 - R + ba(FRAC_PI_2, R) + (30.0 - T - R)), "{w}");
    let low = m.walls.iter().flat_map(|wl| wl.outline.outer.iter().map(|q| wl.surface.point(*q).z)).fold(f64::MAX, f64::min);
    // The tip is 30 below the top (outer) face.
    assert!(close(low, T - 30.0), "{low}");
}

#[test]
fn flange_at_45_degrees() {
    let mut def = base();
    let m = model(&def);
    flange_on(&mut def, vec![(east_edge(&m), FRAC_PI_4, true, 30.0, None)], FlangeAlignment::Outer, None, true);
    let m = model(&def);
    let sb = OSSB * (FRAC_PI_4 / 2.0).tan();
    let (w, _) = size(&m);
    assert!(close(w, 50.0 - sb + ba(FRAC_PI_4, R) + 30.0 - sb), "{w}");
}

#[test]
fn edge_frame_gives_angles_and_the_outer_sharp() {
    let def = base();
    let m = model(&def);
    let f = EdgeFrame::of(&m, &east_edge(&m)).unwrap();
    assert!((f.out - V3::x()).norm() < 1e-12);
    let (a, toward) = f.angle_of(V3::new(1.0, 0.0, 1.0)).unwrap();
    assert!((a - FRAC_PI_4).abs() < 1e-12 && toward);
    assert!(f.angle_of(V3::y()).is_none());
    let s = f.outer_sharp(FlangeAlignment::Inner, FRAC_PI_2, true, R);
    assert!((s - P3::new(50.0 + T, 0.0, 0.0)).norm() < 1e-12);
}

#[test]
fn partial_flange_keeps_the_rest_of_the_edge() {
    for hold in [true, false] {
        let mut def = base();
        let m = model(&def);
        flange_on(&mut def, vec![(east_edge(&m), FRAC_PI_2, true, 30.0, Some((10.0, 15.0)))], FlangeAlignment::Outer, None, hold);
        let m = model(&def);
        let flange = m.walls.iter().find(|w| w.id != cadrs_sheetmetal::WallId(1)).unwrap();
        let (lo, hi) = flange.outline.bounds().unwrap();
        let along = if (hi.x - lo.x - 15.0).abs() < 1e-9 { hi.x - lo.x } else { hi.y - lo.y };
        assert!(close(along, 15.0), "{lo:?} {hi:?}");
        let f = flatten(&m);
        assert!(f.is_ok(), "{:?}", f.errors);
        // The base keeps its corners (it reaches x = 50 outside the flange's stretch).
        let b = m.walls.iter().find(|w| w.id == cadrs_sheetmetal::WallId(1)).unwrap();
        let reach = b.outline.outer.iter().map(|q| q.x).fold(f64::MIN, f64::max);
        assert!(close(reach, 50.0), "{reach}");
        // Bend reliefs at both ends of the bend.
        assert!(!f.parts[0].cuts.is_empty());
    }
}

#[test]
fn two_flanges_meeting_at_a_corner_are_mitred_with_a_rip() {
    let mut def = base();
    let m = model(&def);
    let north = pick(&m, P3::new(0.0, 40.0, T), P3::new(50.0, 40.0, T));
    flange_on(&mut def, vec![(east_edge(&m), FRAC_PI_2, true, 20.0, None), (north, FRAC_PI_2, true, 20.0, None)], FlangeAlignment::Inner, None, true);
    let m = model(&def);
    let names: Vec<&str> = m.joints.iter().map(|j| j.name.as_str()).collect();
    assert_eq!(names, ["Bend A", "Bend B", "Joint C"]);
    assert!(matches!(m.joints[2].kind, JointKind::Rip { style: RipStyle::EdgeJoint, .. }));
    // Edge joint: each flange stops at the other's inside face, half the minimal gap short.
    let east = m.walls.iter().find(|w| w.id == cadrs_sheetmetal::WallId(cadrs_sheetmetal::construct::stable_id(100))).unwrap();
    let ymax = east.outline.outer.iter().map(|q| east.surface.point(*q).y).fold(f64::MIN, f64::max);
    assert!(close(ymax, 40.0 - GAP / 2.0), "{ymax}");
    let f = flatten(&m);
    assert!(f.is_ok(), "{:?}", f.errors);
    assert_eq!(f.parts.len(), 1);
}

#[test]
fn flanges_without_automatic_miter_are_cut_at_the_miter_angle() {
    let mut def = base();
    let m = model(&def);
    let north = pick(&m, P3::new(0.0, 40.0, T), P3::new(50.0, 40.0, T));
    flange_on(&mut def, vec![(east_edge(&m), FRAC_PI_2, true, 20.0, None), (north, FRAC_PI_2, true, 20.0, None)], FlangeAlignment::Outer, Some(FRAC_PI_4), true);
    let m = model(&def);
    assert_eq!(m.joints.len(), 2, "no rip");
    let f = flatten(&m);
    assert!(f.is_ok(), "{:?}", f.errors);
}

fn hem_on(def: &mut SharpDef, picks: Vec<EdgePick>, o: HemOpts) {
    let he: Vec<HemEdge> = picks.into_iter().enumerate().map(|(i, pick)| HemEdge { pick, key: 200 + i as u64, toward: true }).collect();
    edit::hem(def, &he, &o).expect("hem");
}

fn hem_opts(kind: HemKind) -> HemOpts {
    HemOpts { kind, radius: R, angle: 1.5 * PI, gap: 0.5, total: 12.5, alignment: HemAlignment::Outer, closed: false }
}

#[test]
fn straight_hem_total_length() {
    for (flattened, r) in [(false, R), (true, GAP / 2.0)] {
        let mut def = base();
        let m = model(&def);
        let o = HemOpts { radius: r, ..hem_opts(HemKind::Straight) };
        hem_on(&mut def, vec![east_edge(&m)], o);
        let m = model(&def);
        let (w, _) = size(&m);
        let want = (50.0 - (r + T)) + ba(PI, r) + (12.5 - (r + T));
        assert!(close(w, want), "flattened {flattened}: {w} vs {want}");
        // Flattened: the hem lies the minimal gap above the base.
        let hem = &m.walls[1];
        let z = hem.surface.point(hem.outline.outer[0]).z;
        assert!(close(z, T + 2.0 * r + T), "{z}");
        assert!(m.joints[0].bend().unwrap().hem);
    }
}

#[test]
fn rolled_hem_is_its_bend() {
    let mut def = base();
    let m = model(&def);
    hem_on(&mut def, vec![east_edge(&m)], hem_opts(HemKind::Rolled));
    let m = model(&def);
    let (w, _) = size(&m);
    let want = (50.0 - OSSB) + ba(1.5 * PI, R) + edit::ROLLED_TAIL;
    assert!(close(w, want), "{w} vs {want}");
    assert!((m.joints[0].bend().unwrap().angle - 1.5 * PI).abs() < 1e-12);
    // In place: the bend starts at the edge.
    let mut def = base();
    let m = model(&def);
    hem_on(&mut def, vec![east_edge(&m)], HemOpts { alignment: HemAlignment::InPlace, ..hem_opts(HemKind::Rolled) });
    let (w, _) = size(&model(&def));
    assert!(close(w, 50.0 + ba(1.5 * PI, R) + edit::ROLLED_TAIL), "{w}");
}

#[test]
fn tear_drop_hem_ends_the_gap_off_the_wall() {
    let (gap, total) = (0.5, 12.5);
    let (beta, leg) = edit::tear_drop(R, T, gap, total).unwrap();
    assert!(beta > 0.0 && beta < FRAC_PI_2 && leg > 0.0);
    assert!((R * (1.0 + beta.cos()) - leg * beta.sin() - gap).abs() < 1e-9);
    assert!(((R + T) * (1.0 + beta.sin()) + leg * beta.cos() - total).abs() < 1e-9);
    let mut def = base();
    let m = model(&def);
    hem_on(&mut def, vec![east_edge(&m)], HemOpts { gap, total, ..hem_opts(HemKind::TearDrop) });
    let m = model(&def);
    let (w, _) = size(&m);
    assert!(close(w, (50.0 - OSSB) + ba(PI + beta, R) + leg), "{w}");
    // In 3D: the leg's end comes `gap` above the base's top face, and `total` in from x = 50.
    let hem = &m.walls[1];
    let Surface::Planar { .. } = hem.surface else { panic!() };
    let nb = hem.surface.normal().unwrap();
    let pts: Vec<P3> = hem.outline.outer.iter().flat_map(|q| {
        let p = hem.surface.point(*q);
        [p, p + nb * T]
    }).collect();
    let zmin = pts.iter().map(|p| p.z).fold(f64::MAX, f64::min);
    let xmin = pts.iter().map(|p| p.x).fold(f64::MAX, f64::min);
    assert!((zmin - T - gap).abs() < 1e-9, "{zmin}");
    assert!((xmin - (50.0 - total)).abs() < 1e-9, "{xmin}");
}

#[test]
fn hems_meeting_at_a_corner() {
    for closed in [false, true] {
        let mut def = base();
        let m = model(&def);
        let north = pick(&m, P3::new(0.0, 40.0, T), P3::new(50.0, 40.0, T));
        hem_on(&mut def, vec![east_edge(&m), north], HemOpts { closed, ..hem_opts(HemKind::Straight) });
        let m = model(&def);
        let f = flatten(&m);
        assert!(f.is_ok(), "closed {closed}: {:?}", f.errors);
    }
}

#[test]
fn make_joint_bend_and_rip() {
    // A base and a wall standing 5 above its east edge, apart.
    let make = || {
        let mut b = SharpBuilder::new(params());
        let a = b.wall(P3::origin(), V3::x(), V3::y(), Polygon::rect(P2::origin(), P2::new(50.0, 40.0)));
        b.set_wall_id(a, cadrs_sheetmetal::WallId(1));
        let w = b.wall(P3::new(50.0, 0.0, 5.0), V3::z(), V3::y(), Polygon::rect(P2::origin(), P2::new(25.0, 40.0)));
        b.set_wall_id(w, cadrs_sheetmetal::WallId(2));
        let m = b.build().unwrap();
        SharpDef::new(b, &m, m.walls.len(), m.joints.len())
    };
    let mut def = make();
    let m = model(&def);
    let e1 = east_edge(&m);
    let e2 = edit::locate(&m, &[P3::new(50.0, 0.0, 5.0), P3::new(50.0, 40.0, 5.0)]).unwrap();
    edit::make_joint(&mut def, &e1, &e2, 7, JointSpec::Bend(None)).unwrap();
    let m = model(&def);
    let (w, _) = size(&m);
    assert!(close(w, (50.0 - OSSB) + ba(FRAC_PI_2, R) + (30.0 - OSSB)), "{w}");
    for style in RipStyle::ALL {
        let mut def = make();
        let m = model(&def);
        let e1 = east_edge(&m);
        let e2 = edit::locate(&m, &[P3::new(50.0, 0.0, 5.0), P3::new(50.0, 40.0, 5.0)]).unwrap();
        edit::make_joint(&mut def, &e1, &e2, 7, JointSpec::Rip(style)).unwrap();
        let m = model(&def);
        assert!(matches!(m.joints[0].kind, JointKind::Rip { .. }));
        assert_eq!(flatten(&m).parts.len(), 2, "a rip keeps them apart");
    }
}
