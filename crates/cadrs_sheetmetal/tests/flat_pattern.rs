//! Flat patterns checked against hand calculations (P3I.1).
//!
//! Flat lengths of walls meeting at virtual sharps: sum of the sharp lengths, minus two setbacks
//! per bend (outside setbacks `(R+T)·tan(θ/2)` when the walls' definition faces are the outside of
//! the bend, inside ones `R·tan(θ/2)` otherwise), plus the bend allowance `θ·(R + K·T)`.

use std::f64::consts::{FRAC_PI_2, PI};

use cadrs_sheetmetal::flat::{FlatError, PieceSource, flatten};
use cadrs_sheetmetal::model::{Bend, Joint, JointId, JointKind, Model, P3, RipStyle, SharpBuilder, Surface, V3, Wall, WallId};
use cadrs_sheetmetal::params::{BendCalc, BendReliefKind, CornerReliefKind, Params};
use cadrs_sheetmetal::poly::{P2, Polygon, Seg2};
use cadrs_sheetmetal::table::{BendDirection, JointType, table};
use cadrs_sheetmetal::{BendValue, bend};

const T: f64 = 2.0;
const R: f64 = 3.0;
const K: f64 = 0.45;
/// Flat outlines come out of polygon booleans that snap to a fine grid: compare sizes to 1e-6 mm.
const TOL: f64 = 1e-6;

fn params() -> Params {
    Params {
        thickness: T,
        bend_radius: R,
        k_factor: K,
        minimal_gap: 0.2,
        ..Default::default()
    }
}

fn rect(w: f64, h: f64) -> Polygon {
    Polygon::rect(P2::new(0.0, 0.0), P2::new(w, h))
}

fn ba90() -> f64 {
    bend::bend_allowance(R, T, FRAC_PI_2, K)
}

/// Width and height of a flat part.
fn size(m: &Model) -> (f64, f64) {
    let f = flatten(m);
    assert!(f.is_ok(), "{:?}", f.errors);
    assert_eq!(f.parts.len(), 1);
    let (lo, hi) = f.parts[0].bounds().unwrap();
    (hi.x - lo.x, hi.y - lo.y)
}

/// A 50 × 40 base in the XY plane (material above) with a 30 high flange on its x = 50 edge,
/// bent up (towards the material) or down.
fn l_bracket(p: Params, up: bool) -> Model {
    let mut b = SharpBuilder::new(p);
    let a = b.wall(P3::origin(), V3::x(), V3::y(), rect(50.0, 40.0));
    let f = if up {
        // Plane x = 50, local x up the flange: material towards −x.
        b.wall(P3::new(50.0, 0.0, 0.0), V3::z(), V3::y(), rect(30.0, 40.0))
    } else {
        b.wall(P3::new(50.0, 0.0, 0.0), -V3::z(), V3::y(), rect(30.0, 40.0))
    };
    b.bend(a, f, (P3::new(50.0, 0.0, 0.0), P3::new(50.0, 40.0, 0.0)));
    b.build().expect("valid L")
}

#[test]
fn l_bracket_bent_up_uses_outside_setbacks() {
    let (w, h) = size(&l_bracket(params(), true));
    let expect = 50.0 + 30.0 - 2.0 * (R + T) + ba90();
    assert!((w - expect).abs() < TOL, "{w} vs {expect}");
    assert!((h - 40.0).abs() < TOL);
}

#[test]
fn l_bracket_bent_down_uses_inside_setbacks() {
    let (w, _) = size(&l_bracket(params(), false));
    let expect = 50.0 + 30.0 - 2.0 * R + ba90();
    assert!((w - expect).abs() < TOL, "{w} vs {expect}");
}

#[test]
fn bend_allowance_and_deduction_modes() {
    let p = Params {
        bend_calc: BendCalc::BendAllowance,
        bend_allowance: 7.0,
        ..params()
    };
    let (w, _) = size(&l_bracket(p, true));
    assert!((w - (80.0 - 2.0 * (R + T) + 7.0)).abs() < TOL);
    // A deduction comes straight off the outside lengths.
    let p = Params {
        bend_calc: BendCalc::BendDeduction,
        bend_deduction: 4.0,
        ..params()
    };
    let (w, _) = size(&l_bracket(p, true));
    assert!((w - 76.0).abs() < TOL, "{w}");
}

#[test]
fn per_bend_value_overrides_the_model() {
    let mut m = l_bracket(params(), true);
    let JointKind::Bend(b) = &mut m.joints[0].kind else { panic!() };
    b.value = Some(BendValue::KFactor(0.3));
    let (w, _) = size(&m);
    let expect = 80.0 - 2.0 * (R + T) + bend::bend_allowance(R, T, FRAC_PI_2, 0.3);
    assert!((w - expect).abs() < TOL);
}

#[test]
fn obtuse_bend() {
    // A flange at 135° from the base (bent up 45°): θ = 45°.
    let theta = PI / 4.0;
    let mut b = SharpBuilder::new(params());
    let a = b.wall(P3::origin(), V3::x(), V3::y(), rect(50.0, 40.0));
    let d = V3::new(theta.cos(), 0.0, theta.sin());
    let f = b.wall(P3::new(50.0, 0.0, 0.0), d, V3::y(), rect(30.0, 40.0));
    b.bend(a, f, (P3::new(50.0, 0.0, 0.0), P3::new(50.0, 40.0, 0.0)));
    let m = b.build().unwrap();
    let bend = m.joints[0].bend().unwrap();
    assert!((bend.angle - theta).abs() < 1e-9);
    let (w, _) = size(&m);
    let expect = 80.0 - 2.0 * bend::outside_setback(R, T, theta).unwrap() + bend::bend_allowance(R, T, theta, K);
    assert!((w - expect).abs() < TOL);
}

#[test]
fn u_channel() {
    let mut b = SharpBuilder::new(params());
    let base = b.wall(P3::origin(), V3::x(), V3::y(), rect(60.0, 40.0));
    let right = b.wall(P3::new(60.0, 0.0, 0.0), V3::z(), V3::y(), rect(25.0, 40.0));
    // Plane x = 0: local x up, y along −y so the material (u × v) points to +x.
    let left = b.wall(P3::new(0.0, 40.0, 0.0), V3::z(), -V3::y(), rect(25.0, 40.0));
    b.bend(base, right, (P3::new(60.0, 0.0, 0.0), P3::new(60.0, 40.0, 0.0)));
    b.bend(base, left, (P3::new(0.0, 0.0, 0.0), P3::new(0.0, 40.0, 0.0)));
    let m = b.build().unwrap();
    let (w, h) = size(&m);
    let expect = 60.0 + 25.0 + 25.0 - 4.0 * (R + T) + 2.0 * ba90();
    assert!((w - expect).abs() < TOL, "{w} vs {expect}");
    assert!((h - 40.0).abs() < TOL);
    let t = table(&m);
    assert_eq!(t.bends.len(), 2);
    assert_eq!(t.bends[0].name, "Bend A");
    assert_eq!(t.bends[1].name, "Bend B");
    assert!(t.bends.iter().all(|r| r.direction == BendDirection::Up && (r.angle_deg - 90.0).abs() < 1e-9));
}

/// An open box: a 100 × 60 base with 20 high walls on all four sides, ripped at the corners.
fn open_box(p: Params, style: RipStyle) -> Model {
    let (x, y, h) = (100.0, 60.0, 20.0);
    let mut b = SharpBuilder::new(p);
    let base = b.wall(P3::origin(), V3::x(), V3::y(), rect(x, y));
    // Each side wall: local x up (z), local y along the base edge, material inwards.
    let east = b.wall(P3::new(x, 0.0, 0.0), V3::z(), V3::y(), rect(h, y));
    let north = b.wall(P3::new(x, y, 0.0), V3::z(), -V3::x(), rect(h, x));
    let west = b.wall(P3::new(0.0, y, 0.0), V3::z(), -V3::y(), rect(h, y));
    let south = b.wall(P3::new(0.0, 0.0, 0.0), V3::z(), V3::x(), rect(h, x));
    b.bend(base, east, (P3::new(x, 0.0, 0.0), P3::new(x, y, 0.0)));
    b.bend(base, north, (P3::new(x, y, 0.0), P3::new(0.0, y, 0.0)));
    b.bend(base, west, (P3::new(0.0, y, 0.0), P3::new(0.0, 0.0, 0.0)));
    b.bend(base, south, (P3::new(0.0, 0.0, 0.0), P3::new(x, 0.0, 0.0)));
    b.rip(east, north, (P3::new(x, y, 0.0), P3::new(x, y, h)), style);
    b.rip(north, west, (P3::new(0.0, y, 0.0), P3::new(0.0, y, h)), style);
    b.rip(west, south, (P3::new(0.0, 0.0, 0.0), P3::new(0.0, 0.0, h)), style);
    b.rip(south, east, (P3::new(x, 0.0, 0.0), P3::new(x, 0.0, h)), style);
    b.build().expect("valid box")
}

#[test]
fn open_box_lays_flat_as_a_cross_without_collisions() {
    let m = open_box(params(), RipStyle::EdgeJoint);
    let f = flatten(&m);
    assert!(f.is_ok(), "{:?}", f.errors);
    assert_eq!(f.parts.len(), 1, "rips don't split the part: the bends hold it together");
    let part = &f.parts[0];
    assert_eq!(part.outline.len(), 1);
    let (lo, hi) = part.bounds().unwrap();
    let ew = 100.0 + 2.0 * (20.0 - 2.0 * (R + T) + ba90());
    let ns = 60.0 + 2.0 * (20.0 - 2.0 * (R + T) + ba90());
    assert!((hi.x - lo.x - ew).abs() < TOL, "{} vs {ew}", hi.x - lo.x);
    assert!((hi.y - lo.y - ns).abs() < TOL, "{} vs {ns}", hi.y - lo.y);
    // Four corner zones were cut (plus the base's corners past the tangent lines).
    assert!(part.cuts.len() >= 4);
    let t = table(&m);
    assert_eq!(t.bends.len(), 4);
    assert_eq!(t.joints.len(), 4);
    assert!(t.joints.iter().all(|j| j.kind == JointType::Rip && j.style == Some(RipStyle::EdgeJoint)));
    assert_eq!(t.joints[0].name, "Rip 1");
}

#[test]
fn box_butt_joints_change_the_flanges_not_the_base() {
    let edge = flatten(&open_box(params(), RipStyle::EdgeJoint));
    let butt = flatten(&open_box(params(), RipStyle::ButtDirection1));
    assert!(butt.is_ok(), "{:?}", butt.errors);
    // Butt joints let one wall of each corner run on: more material than edge joints.
    assert!(butt.parts[0].area() > edge.parts[0].area());
}

#[test]
fn corner_relief_types_cut_more_than_simple() {
    let simple = flatten(&open_box(params(), RipStyle::EdgeJoint)).parts[0].area();
    for kind in [CornerReliefKind::RoundSized, CornerReliefKind::SquareSized, CornerReliefKind::RectangleScaled, CornerReliefKind::RoundScaled] {
        let mut p = params();
        p.corner_relief.kind = kind;
        p.corner_relief.size = 12.0;
        p.corner_relief.scale = 2.0;
        let f = flatten(&open_box(p, RipStyle::EdgeJoint));
        assert!(f.is_ok(), "{kind:?}: {:?}", f.errors);
        assert!(f.parts[0].area() < simple - 1.0, "{kind:?}: {} vs {simple}", f.parts[0].area());
    }
    // Closed mitres the corners instead: nothing extra removed, so at least Simple's area.
    let mut p = params();
    p.corner_relief.kind = CornerReliefKind::Closed;
    let f = flatten(&open_box(p, RipStyle::EdgeJoint));
    assert!(f.is_ok(), "{:?}", f.errors);
    assert!(f.parts[0].area() >= simple - 1e-6);
}

#[test]
fn corner_override_changes_one_corner() {
    let base = flatten(&open_box(params(), RipStyle::EdgeJoint)).parts[0].area();
    let mut m = open_box(params(), RipStyle::EdgeJoint);
    m.corner_overrides.push(cadrs_sheetmetal::model::CornerOverride {
        bends: (JointId(0), JointId(1)),
        relief: cadrs_sheetmetal::CornerRelief {
            kind: CornerReliefKind::RoundSized,
            size: 12.0,
            scale: 1.5,
        },
    });
    let one = flatten(&m).parts[0].area();
    let mut p = params();
    p.corner_relief.kind = CornerReliefKind::RoundSized;
    p.corner_relief.size = 12.0;
    let all = flatten(&open_box(p, RipStyle::EdgeJoint)).parts[0].area();
    assert!(one < base && one > all, "{all} < {one} < {base}");
}

#[test]
fn partial_flange_gets_bend_reliefs() {
    // The flange covers y 10..30 of the 40 long edge: the base carries on past both ends.
    let mut b = SharpBuilder::new(params());
    let a = b.wall(P3::origin(), V3::x(), V3::y(), rect(50.0, 40.0));
    let f = b.wall(P3::new(50.0, 10.0, 0.0), V3::z(), V3::y(), rect(30.0, 20.0));
    b.bend(a, f, (P3::new(50.0, 10.0, 0.0), P3::new(50.0, 30.0, 0.0)));
    let m = b.build().unwrap();
    let flat = flatten(&m);
    assert!(flat.is_ok(), "{:?}", flat.errors);
    let part = &flat.parts[0];
    assert_eq!(part.cuts.len(), 2, "one relief at each end");
    // Obround – Scaled, width scale 1, depth scale 2: T wide, R deeper than the tangent line.
    let (_, w_extra) = (T, R);
    let tangent_x = 50.0 - (R + T);
    let inside_notch = P2::new(tangent_x - w_extra / 2.0, 30.0 + T / 2.0);
    assert!(!part.outline.iter().any(|o| o.contains(inside_notch)), "notch cut at the end");
    let beside = P2::new(tangent_x - w_extra / 2.0, 30.0 + T + 1.0);
    assert!(part.outline.iter().any(|o| o.contains(beside)));
    // Tear removes almost nothing.
    let mut p = params();
    p.bend_relief.kind = BendReliefKind::Tear;
    let mut b2 = b.clone();
    b2.params = p;
    let tear = flatten(&b2.build().unwrap());
    assert!(tear.parts[0].area() > part.area());
}

fn planar(origin: P3) -> Surface {
    Surface::Planar {
        origin,
        u: V3::x(),
        v: V3::y(),
    }
}

fn tangent_bend(on_a: Seg2, on_b: Seg2, angle: f64) -> Bend {
    Bend {
        on_a,
        on_b,
        angle,
        toward_material: true,
        radius: R,
        model_radius: true,
        value: None,
        hem: false,
    }
}

#[test]
fn hem_lays_flat_with_its_allowance() {
    // A 50 long wall with a 10 long hem folded back 180° at its x = 50 edge.
    let mut bend = tangent_bend(
        Seg2::new(P2::new(50.0, 0.0), P2::new(50.0, 20.0)),
        Seg2::new(P2::new(0.0, 0.0), P2::new(0.0, 20.0)),
        PI,
    );
    bend.hem = true;
    let m = Model {
        params: Params {
            bend_calc: BendCalc::BendDeduction,
            ..params()
        },
        walls: vec![
            Wall {
                id: WallId(0),
                surface: planar(P3::origin()),
                outline: rect(50.0, 20.0),
            },
            Wall {
                id: WallId(1),
                surface: planar(P3::origin()),
                outline: rect(10.0, 20.0),
            },
        ],
        joints: vec![Joint {
            id: JointId(0),
            name: "Bend A".into(),
            a: WallId(0),
            b: WallId(1),
            kind: JointKind::Bend(bend),
        }],
        ..Default::default()
    };
    // With bend deduction the hem can't be laid flat: deduction is undefined at 180°.
    let f = flatten(&m);
    assert!(matches!(f.errors.as_slice(), [FlatError::BadJoint { .. }]), "{:?}", f.errors);
    let t = table(&m);
    assert_eq!(t.bends[0].value, None, "shown as –");
    assert!(!t.bends[0].editable, "hems aren't editable in the table");
    // With a K factor it lays flat: 50 + π(R + K·T) + 10.
    let mut m = m;
    m.params.bend_calc = BendCalc::KFactor;
    let (w, _) = size(&m);
    assert!((w - (60.0 + PI * (R + K * T))).abs() < TOL);
}

fn rolled(id: u32, radius: f64, arc: f64, height: f64) -> Wall {
    Wall {
        id: WallId(id),
        surface: Surface::Rolled {
            axis_origin: P3::origin(),
            axis: V3::z(),
            start: V3::x(),
            radius,
            material_outside: true,
        },
        outline: rect(arc, height),
    }
}

#[test]
fn rolled_tube_unrolls_at_its_neutral_radius() {
    // A full tube of inner radius 10, 1 thick, ripped along its seam: 2π(10 + 0.5·1) wide.
    let p = Params {
        thickness: 1.0,
        ..params()
    };
    let m = Model {
        params: p,
        walls: vec![rolled(0, 10.0, 2.0 * PI * 10.0, 30.0)],
        ..Default::default()
    };
    let (w, h) = size(&m);
    assert!((w - 2.0 * PI * 10.5).abs() < TOL, "{w}");
    assert!((h - 30.0).abs() < TOL);
}

#[test]
fn planar_wall_runs_into_a_rolled_one() {
    let p = Params {
        thickness: 1.0,
        ..params()
    };
    let m = Model {
        params: p,
        walls: vec![
            Wall {
                id: WallId(0),
                surface: planar(P3::origin()),
                outline: rect(20.0, 30.0),
            },
            rolled(1, 10.0, PI * 10.0, 30.0),
        ],
        joints: vec![Joint {
            id: JointId(0),
            name: "Tangent 1".into(),
            a: WallId(0),
            b: WallId(1),
            kind: JointKind::Tangent {
                on_a: Seg2::new(P2::new(20.0, 0.0), P2::new(20.0, 30.0)),
                on_b: Seg2::new(P2::new(0.0, 0.0), P2::new(0.0, 30.0)),
            },
        }],
        ..Default::default()
    };
    let (w, _) = size(&m);
    assert!((w - (20.0 + PI * 10.5)).abs() < TOL, "{w}");
    let t = table(&m);
    assert!(t.bends.is_empty(), "rolled walls aren't bends");
    assert_eq!(t.joints[0].kind, JointType::Tangent);
    assert_eq!(flatten(&m).parts[0].bends.len(), 0, "no bend lines for a rolled wall");
}

#[test]
fn flip_direction_up_flips_up_and_down() {
    let m = l_bracket(params(), true);
    let up = flatten(&m);
    assert!(up.parts[0].bends[0].up);
    assert_eq!(table(&m).bends[0].direction, BendDirection::Up);
    let mut flipped = m.clone();
    flipped.params.flip_direction_up = true;
    let f = flatten(&flipped);
    assert!(!f.parts[0].bends[0].up);
    assert_eq!(table(&flipped).bends[0].direction, BendDirection::Down);
    // Same size, mirrored.
    let (a, b) = (up.parts[0].bounds().unwrap(), f.parts[0].bounds().unwrap());
    assert!(((a.1.x - a.0.x) - (b.1.x - b.0.x)).abs() < TOL);
    assert!(b.0.x < 0.0);
}

#[test]
fn bend_lines_sit_between_the_tangent_lines() {
    let f = flatten(&l_bracket(params(), true));
    let b = &f.parts[0].bends[0];
    let x0 = 50.0 - (R + T);
    assert!((b.tangent_a.a.x - x0).abs() < 1e-9);
    assert!((b.tangent_b.a.x - (x0 + ba90())).abs() < 1e-9);
    assert!((b.center.a.x - (x0 + ba90() / 2.0)).abs() < 1e-9);
    assert!((b.center.len() - 40.0).abs() < 1e-9);
}

#[test]
fn overlapping_walls_report_a_collision() {
    // B laid flat to the right of A; C hinged on A's top edge hooks back down into B's place.
    let ba = bend::bend_allowance(R, T, FRAC_PI_2, K);
    let hook = Polygon::new(vec![
        P2::new(0.0, 0.0),
        P2::new(10.0, 0.0),
        P2::new(10.0, 2.0),
        P2::new(25.0, 2.0),
        P2::new(25.0, -8.0),
        P2::new(35.0, -8.0),
        P2::new(35.0, 20.0),
        P2::new(0.0, 20.0),
    ]);
    let m = Model {
        params: params(),
        walls: vec![
            Wall {
                id: WallId(0),
                surface: planar(P3::origin()),
                outline: rect(10.0, 10.0),
            },
            Wall {
                id: WallId(1),
                surface: planar(P3::origin()),
                outline: rect(30.0, 10.0),
            },
            Wall {
                id: WallId(2),
                surface: planar(P3::origin()),
                outline: hook,
            },
        ],
        joints: vec![
            Joint {
                id: JointId(0),
                name: "Bend A".into(),
                a: WallId(0),
                b: WallId(1),
                kind: JointKind::Bend(tangent_bend(
                    Seg2::new(P2::new(10.0, 0.0), P2::new(10.0, 10.0)),
                    Seg2::new(P2::new(0.0, 0.0), P2::new(0.0, 10.0)),
                    FRAC_PI_2,
                )),
            },
            Joint {
                id: JointId(1),
                name: "Bend B".into(),
                a: WallId(0),
                b: WallId(2),
                kind: JointKind::Bend(tangent_bend(
                    Seg2::new(P2::new(0.0, 10.0), P2::new(10.0, 10.0)),
                    Seg2::new(P2::new(0.0, 0.0), P2::new(10.0, 0.0)),
                    FRAC_PI_2,
                )),
            },
        ],
        ..Default::default()
    };
    assert!(ba < 8.0);
    let f = flatten(&m);
    let collision = f.errors.iter().find_map(|e| match e {
        FlatError::Collision { a, b, area } => Some((*a, *b, *area)),
        _ => None,
    });
    let (a, b, area) = collision.expect("collision reported");
    assert_eq!((a, b), (PieceSource::Wall(WallId(1)), PieceSource::Wall(WallId(2))));
    assert!(area > 1.0);
    assert_eq!(f.errors[0].message(), "Collision in sheet metal flat pattern");
}

#[test]
fn bends_closing_a_loop_are_reported() {
    // Base with two walls bent up at a corner, and the two walls bent to each other as well.
    let mut b = SharpBuilder::new(params());
    let base = b.wall(P3::origin(), V3::x(), V3::y(), rect(40.0, 40.0));
    let east = b.wall(P3::new(40.0, 0.0, 0.0), V3::z(), V3::y(), rect(30.0, 40.0));
    let north = b.wall(P3::new(40.0, 40.0, 0.0), V3::z(), -V3::x(), rect(30.0, 40.0));
    b.bend(base, east, (P3::new(40.0, 0.0, 0.0), P3::new(40.0, 40.0, 0.0)));
    b.bend(base, north, (P3::new(40.0, 40.0, 0.0), P3::new(0.0, 40.0, 0.0)));
    b.bend(east, north, (P3::new(40.0, 40.0, 0.0), P3::new(40.0, 40.0, 30.0)));
    let m = b.build().unwrap();
    let f = flatten(&m);
    assert!(f.errors.iter().any(|e| matches!(e, FlatError::BendLoop { .. })), "{:?}", f.errors);
}

#[test]
fn moving_a_bend_reorders_the_table_keeping_names() {
    let mut m = open_box(params(), RipStyle::EdgeJoint);
    assert!(m.move_joint(JointId(1), -1));
    let t = table(&m);
    assert_eq!(t.bends[0].name, "Bend B");
    assert_eq!(t.bends[0].number, 1);
    assert_eq!(t.bends[1].name, "Bend A");
    assert!(!m.move_joint(JointId(1), -1), "already first");
    // The flat pattern doesn't depend on the order here.
    assert!(flatten(&m).is_ok());
}

#[test]
fn table_value_column_follows_the_calculation() {
    let mut m = l_bracket(params(), true);
    assert_eq!(table(&m).value_column, "K Factor");
    assert!((table(&m).bends[0].value.unwrap().value() - K).abs() < 1e-12);
    m.params.bend_calc = BendCalc::BendDeduction;
    let t = table(&m);
    assert_eq!(t.value_column, "Bend deduction (mm)");
    assert!((t.bends[0].value.unwrap().value() - m.params.bend_deduction).abs() < 1e-12);
}

#[test]
fn inconsistent_material_sides_are_rejected() {
    let mut b = SharpBuilder::new(params());
    let a = b.wall(P3::origin(), V3::x(), V3::y(), rect(50.0, 40.0));
    // The flange's material side points the wrong way (outwards).
    let f = b.wall(P3::new(50.0, 40.0, 0.0), V3::z(), -V3::y(), rect(30.0, 40.0));
    b.bend(a, f, (P3::new(50.0, 0.0, 0.0), P3::new(50.0, 40.0, 0.0)));
    assert!(matches!(b.build(), Err(cadrs_sheetmetal::BuildError::InconsistentSide { .. })));
}

#[test]
fn model_round_trips_through_ron() {
    let m = open_box(params(), RipStyle::ButtDirection2);
    let text = ron::to_string(&m).unwrap();
    let back: Model = ron::from_str(&text).unwrap();
    assert_eq!(back, m);
}

