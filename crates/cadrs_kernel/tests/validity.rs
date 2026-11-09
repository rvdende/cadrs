//! `Body::check_with(CheckLevel::Full)`: valid models pass it, and each kind of defect it adds
//! to the fast check (loops crossing in (u, v), loop orientation and nesting, shell
//! connectivity, p-curves off their edges between the fast check's samples, voids) is found.

use std::f64::consts::{PI, TAU};

use cadrs_kernel::native::curve::Curve;
use cadrs_kernel::native::kernel::NativeKernel;
use cadrs_kernel::native::math::Affine;
use cadrs_kernel::native::nurbs::NurbsCurve;
use cadrs_kernel::native::topo::{Body, CheckLevel, Solid};
use cadrs_kernel::*;
use nalgebra::{Point2, Point3, Vector3, Vector4};

fn p(x: f64, y: f64) -> Point2<f64> {
    Point2::new(x, y)
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Loop {
    let c = [p(x0, y0), p(x1, y0), p(x1, y1), p(x0, y1)];
    Loop {
        curves: (0..4)
            .map(|i| Curve2::Line {
                a: c[i],
                b: c[(i + 1) % 4],
                source: None,
            })
            .collect(),
    }
}

fn circle(x: f64, y: f64, r: f64) -> Loop {
    Loop {
        curves: vec![Curve2::Circle {
            center: p(x, y),
            radius: r,
            source: None,
        }],
    }
}

fn region(outer: Loop, holes: Vec<Loop>) -> Region {
    Region {
        outer,
        holes,
        source: None,
    }
}

fn extrude(k: &mut NativeKernel, r: Region, depth: f64) -> BodyId {
    k.extrude(&Profile::new(Plane::top(), vec![r]), Extent::Blind(depth))
        .unwrap()
        .bodies[0]
}

/// The XZ plane (normal −Y), for revolve profiles.
fn front() -> Plane {
    Plane {
        origin: Point3::origin(),
        x_dir: Vector3::x_axis(),
        normal: -Vector3::y_axis(),
    }
}

fn body(k: &NativeKernel, id: BodyId) -> Body {
    Body::clone(k.get(id).unwrap())
}

#[track_caller]
fn valid(b: &Body) {
    let issues = b.check_with(CheckLevel::Full);
    assert!(issues.is_empty(), "{issues:?}");
}

#[track_caller]
fn finds(b: &Body, what: &str) {
    let issues = b.check_with(CheckLevel::Full);
    assert!(
        issues.iter().any(|i| i.contains(what)),
        "expected {what:?} in {issues:?}"
    );
}

/// A plate with a round hole: 10 × 10 × 2, hole of radius 2 at (5, 5).
fn plate(k: &mut NativeKernel) -> BodyId {
    extrude(
        k,
        region(rect(0.0, 0.0, 10.0, 10.0), vec![circle(5.0, 5.0, 2.0)]),
        2.0,
    )
}

/// A move by `du` along u (of p-curves).
fn moved(du: f64) -> Affine {
    Affine {
        m: nalgebra::Matrix3::identity(),
        t: Vector3::new(du, 0.0, 0.0),
    }
}

/// The planar face of `b` with the most loops (the plate's top or bottom).
fn holed_face(b: &Body) -> usize {
    (0..b.faces.len())
        .max_by_key(|&f| b.faces[f].loops.len())
        .unwrap()
}

#[test]
fn valid_models_pass_the_full_check() {
    let mut k = NativeKernel::new();
    let bx = extrude(&mut k, region(rect(0.0, 0.0, 10.0, 6.0), vec![]), 4.0);
    let cyl = extrude(&mut k, region(circle(0.0, 0.0, 3.0), vec![]), 5.0);
    let pl = plate(&mut k);
    // A sphere and a torus, revolved about z.
    let half = Region {
        outer: Loop {
            curves: vec![
                Curve2::Arc {
                    center: p(0.0, 0.0),
                    radius: 2.0,
                    start_angle: -PI / 2.0,
                    sweep: PI,
                    source: None,
                },
                Curve2::Line {
                    a: p(0.0, 2.0),
                    b: p(0.0, -2.0),
                    source: None,
                },
            ],
        },
        holes: vec![],
        source: None,
    };
    let z = Axis {
        origin: Point3::origin(),
        dir: Vector3::z_axis(),
    };
    let sphere = k
        .revolve(&Profile::new(front(), vec![half]), z, TAU)
        .unwrap()
        .bodies[0];
    let torus = k
        .revolve(
            &Profile::new(front(), vec![region(circle(5.0, 0.0, 1.0), vec![])]),
            z,
            TAU,
        )
        .unwrap()
        .bodies[0];
    // A boolean with a curved cut, a fillet and a hollow shell (a solid with a void).
    let cut = k.boolean(BoolOp::Subtract, bx, &[cyl]).unwrap().bodies[0];
    let edge = k.edges(bx).unwrap()[0].id;
    let filleted = k.fillet(bx, &[edge], 1.0).unwrap().bodies[0];
    let hollow = k
        .shell_with(
            bx,
            &ShellSpec {
                remove: vec![],
                thickness: 1.0,
                outward: false,
                hollow: true,
                sharp: true,
            },
        )
        .unwrap()
        .bodies[0];
    for id in [bx, cyl, pl, sphere, torus, cut, filleted, hollow] {
        valid(&body(&k, id));
    }
    assert_eq!(
        k.get(hollow).unwrap().solids[0].shells.len(),
        2,
        "the hollow box has a void"
    );
}

#[test]
fn finds_loops_that_cross() {
    let mut k = NativeKernel::new();
    let id = plate(&mut k);
    let mut b = body(&k, id);
    let fi = holed_face(&b);
    // Move the hole's p-curve so it straddles the outer loop's side x = 10.
    let shift = moved(4.0);
    for c in b.faces[fi].loops[1].iter_mut() {
        c.pc = c.pc.transformed(&shift);
    }
    finds(&b, "cross in (u, v)");
}

#[test]
fn finds_holes_outside_and_wrong_way() {
    let mut k = NativeKernel::new();
    let id = plate(&mut k);
    let good = body(&k, id);
    let fi = holed_face(&good);
    // The hole moved wholly outside the outer loop.
    let mut b = good.clone();
    let far = moved(30.0);
    for c in b.faces[fi].loops[1].iter_mut() {
        c.pc = c.pc.transformed(&far);
    }
    finds(&b, "a hole outside the outer loop");
    // The hole and the outer loop swapped: the first loop is no longer the outer one.
    let mut b = good.clone();
    b.faces[fi].loops.swap(0, 1);
    finds(&b, "outer loop");
    // The whole face turned over without flipping its sense: both loops run the wrong way.
    let mut b = good.clone();
    for l in &mut b.faces[fi].loops {
        l.reverse();
        for c in l.iter_mut() {
            c.fwd = !c.fwd;
        }
    }
    finds(&b, "runs clockwise");
    finds(&b, "a hole running the same way");
}

#[test]
fn finds_disconnected_and_shared_shells() {
    let mut k = NativeKernel::new();
    let a = extrude(&mut k, region(rect(0.0, 0.0, 1.0, 1.0), vec![]), 1.0);
    let c = extrude(&mut k, region(rect(5.0, 0.0, 6.0, 1.0), vec![]), 1.0);
    let mut b = body(&k, a);
    b.append(&body(&k, c));
    // Both boxes' faces in one shell of one solid.
    let mut one = b.clone();
    let faces: Vec<usize> = (0..one.faces.len()).collect();
    one.shells = vec![cadrs_kernel::native::topo::Shell {
        faces,
        closed: true,
    }];
    one.solids = vec![Solid { shells: vec![0] }];
    assert!(
        one.check().is_empty(),
        "the fast check doesn't look at connectivity: {:?}",
        one.check()
    );
    finds(&one, "isn't connected");
    // A face in both shells.
    let mut two = b.clone();
    two.shells[1].faces.push(0);
    finds(&two, "face 0 is in shells 0 and 1");
}

#[test]
fn finds_voids_outside_or_inside_out() {
    let mut k = NativeKernel::new();
    let big = extrude(&mut k, region(rect(0.0, 0.0, 10.0, 10.0), vec![]), 10.0);
    let inner = extrude(&mut k, region(rect(4.0, 4.0, 6.0, 6.0), vec![]), 6.0);
    let away = extrude(&mut k, region(rect(40.0, 4.0, 42.0, 6.0), vec![]), 6.0);
    let with_void = |small: BodyId, turn: bool| {
        let mut b = body(&k, big);
        let mut s = body(&k, small);
        if turn {
            for f in &mut s.faces {
                Body::reverse_face(f);
            }
        }
        b.append(&s);
        b.solids = vec![Solid { shells: vec![0, 1] }];
        b
    };
    valid(&with_void(inner, true));
    finds(&with_void(away, true), "lies outside the outer shell");
    finds(&with_void(inner, false), "must point into the void");
}

/// A 10 × 10 × 2 box with one straight edge's p-curve on a planar face replaced by a polyline
/// with a 0.01 bump between the fast check's first two samples (10 % and 25 % along it).
fn box_with_a_bumped_pcurve() -> Body {
    let mut k = NativeKernel::new();
    let id = extrude(&mut k, region(rect(0.0, 0.0, 10.0, 10.0), vec![]), 2.0);
    let mut b = body(&k, id);
    let (fi, li, ci) = b
        .faces
        .iter()
        .enumerate()
        .flat_map(|(fi, f)| {
            f.loops
                .iter()
                .enumerate()
                .flat_map(move |(li, l)| (0..l.len()).map(move |ci| (fi, li, ci)))
        })
        .find(|&(fi, li, ci)| {
            matches!(b.faces[fi].loops[li][ci].pc, Curve::Line { .. })
                && !b.edges[b.faces[fi].loops[li][ci].edge].degenerate
        })
        .unwrap();
    let c = &b.faces[fi].loops[li][ci];
    let e = &b.edges[c.edge];
    let (t0, t1) = (e.t[0], e.t[1]);
    let at = |s: f64| c.pc.point(t0 + (t1 - t0) * s);
    let (a, z) = (at(0.0), at(1.0));
    let d = (z - a).normalize();
    let bump = at(0.17) + Vector3::new(-d.y, d.x, 0.0) * 0.01;
    let h = |q: Point3<f64>| Vector4::new(q.x, q.y, q.z, 1.0);
    let t = |s: f64| t0 + (t1 - t0) * s;
    let knots = vec![t0, t0, t(0.12), t(0.17), t(0.22), t1, t1];
    let cps = vec![h(a), h(at(0.12)), h(bump), h(at(0.22)), h(z)];
    b.faces[fi].loops[li][ci].pc = Curve::Nurbs(std::sync::Arc::new(NurbsCurve {
        degree: 1,
        knots,
        cps,
    }));
    b
}

#[test]
fn finds_a_pcurve_off_its_edge_between_samples() {
    let b = box_with_a_bumped_pcurve();
    assert!(
        b.check().is_empty(),
        "the fast check samples past the bump: {:?}",
        b.check()
    );
    finds(&b, "p-curve off the edge");
}

#[test]
fn sewing_refits_pcurves_off_their_edges() {
    // Sewing ends with OCCT's SameParameter: a p-curve off its edge (here between the fast
    // check's samples) is projected again rather than kept, so the sewn body passes the full
    // check without its edges' tolerances raised to the bump.
    let b = box_with_a_bumped_pcurve();
    let sewn = cadrs_kernel::native::surfacing::sew_faces(b, 1e-6).unwrap();
    valid(&sewn);
    let worst = sewn.edges.iter().map(|e| e.tol).fold(0.0, f64::max);
    assert!(worst < 1e-4, "an edge's tolerance raised to {worst:e}");
}

/// Faces `faces` of body `id` alone, as a sheet body in `k`.
fn sheet_of(k: &mut NativeKernel, id: BodyId, faces: &[usize]) -> BodyId {
    let sheet = cadrs_kernel::native::kernel::subset(&body(k, id), faces, false);
    k.insert(sheet)
}

#[test]
fn a_hole_turned_a_whole_turn_is_a_void() {
    // The plate's top face (a 10 × 10 square with a hole of radius 2) turned about the line
    // x = −5 along y: a ring whose hole sweeps a closed tunnel, a void shell of its own.
    let mut k = NativeKernel::new();
    let pl = plate(&mut k);
    let fi = holed_face(&body(&k, pl));
    let sheet = sheet_of(&mut k, pl, &[fi]);
    let axis = Axis {
        origin: Point3::new(-5.0, 0.0, 0.0),
        dir: Vector3::y_axis(),
    };
    let ring = k
        .sweep_body(sheet, &BodySweep::Rotate { axis, angle: TAU })
        .unwrap()
        .bodies[0];
    let b = body(&k, ring);
    valid(&b);
    assert_eq!(b.solids.len(), 1);
    assert_eq!(b.solids[0].shells.len(), 2, "the tunnel is a void");
    let want = TAU * 10.0 * (100.0 - 4.0 * PI);
    let v = k.mass_properties(ring).unwrap().volume;
    assert!((v - want).abs() < 1e-6 * want, "{v} vs {want}");
}

#[test]
fn disjoint_wires_sweep_into_separate_shells() {
    let mut k = NativeKernel::new();
    let seg = |k: &mut NativeKernel, y: f64| {
        let c = PathCurve::Sketch {
            plane: Plane::top(),
            curve: Curve2::Line {
                a: p(0.0, y),
                b: p(5.0, y),
                source: None,
            },
        };
        k.create_wire(&[c]).unwrap().bodies[0]
    };
    let (a, b) = (seg(&mut k, 0.0), seg(&mut k, 3.0));
    let both = k.compound(&[a, b]).unwrap().bodies[0];
    let walls = k
        .sweep_body(both, &BodySweep::Translate(Vector3::new(0.0, 0.0, 2.0)))
        .unwrap()
        .bodies[0];
    let w = body(&k, walls);
    valid(&w);
    assert_eq!(w.shells.len(), 2);
}

#[test]
fn sewing_rebuilds_the_shells() {
    // Two faces of a box that share an edge, read with each face listed twice in a shell of
    // its own (as the SMLib reader gives lamina faces): sewn, they are one open shell.
    let mut k = NativeKernel::new();
    let bx = extrude(&mut k, region(rect(0.0, 0.0, 1.0, 1.0), vec![]), 1.0);
    let b = body(&k, bx);
    let e = b.faces[0].loops[0][0].edge;
    let other = (1..b.faces.len())
        .find(|&f| b.faces[f].loops.iter().flatten().any(|c| c.edge == e))
        .unwrap();
    let mut two = cadrs_kernel::native::kernel::subset(&b, &[0, other], false);
    two.shells = vec![
        cadrs_kernel::native::topo::Shell {
            faces: vec![0, 0],
            closed: false,
        },
        cadrs_kernel::native::topo::Shell {
            faces: vec![1, 1],
            closed: false,
        },
    ];
    finds(&two, "listed twice");
    let sewn = cadrs_kernel::native::surfacing::sew_faces(two, 1e-6).unwrap();
    valid(&sewn);
    assert_eq!(sewn.shells.len(), 1);
    assert_eq!(sewn.shells[0].faces.len(), 2);
}

#[test]
fn a_sweep_round_a_sharp_corner_is_mitred() {
    // A 2 × 2 square square to an L-shaped path (10 along x, then 10 along y): the mitred bar,
    // volume A·L = 80 and area 168, valid at the full level.
    let mut k = NativeKernel::new();
    let plane = Plane {
        origin: Point3::origin(),
        x_dir: Vector3::y_axis(),
        normal: Vector3::x_axis(),
    };
    let profile = Profile::new(plane, vec![region(rect(-1.0, -1.0, 1.0, 1.0), vec![])]);
    let path = vec![
        PathCurve::Sketch {
            plane: Plane::top(),
            curve: Curve2::Line {
                a: p(0.0, 0.0),
                b: p(10.0, 0.0),
                source: None,
            },
        },
        PathCurve::Sketch {
            plane: Plane::top(),
            curve: Curve2::Line {
                a: p(10.0, 0.0),
                b: p(10.0, 10.0),
                source: None,
            },
        },
    ];
    let r = k
        .sweep_with(
            &profile,
            &SweepSpec {
                body: BodyKind::Solid,
                path,
                control: SweepControl::None,
                corners: Default::default(),
                faces: vec![],
            },
        )
        .unwrap();
    let id = r.bodies[0];
    valid(&body(&k, id));
    let m = k.mass_properties(id).unwrap();
    assert!(
        (m.volume - 80.0).abs() < 1e-6 && (m.surface_area - 168.0).abs() < 1e-6,
        "{m:?}"
    );
    // Every face keeps a name: a side of a profile curve, or a cap.
    let named: std::collections::HashSet<u64> =
        r.history.generated.iter().map(|(f, _)| f.0).collect();
    assert_eq!(named.len(), k.faces(id).unwrap().len());
}

#[test]
fn a_whole_turn_of_a_holed_region_keeps_its_hole_as_a_void() {
    // A 4 × 4 square with a round hole at its middle, 5 to 9 off the axis, turned a whole turn:
    // no caps join the two loops' sweeps, so the hole's torus is a void shell of its own (one
    // shell holding both wasn't edge-connected). Volume by Pappus: (16 − π)·2π·7.
    let mut k = NativeKernel::new();
    let z = Axis {
        origin: Point3::origin(),
        dir: Vector3::z_axis(),
    };
    let r = region(rect(5.0, -2.0, 9.0, 2.0), vec![circle(7.0, 0.0, 1.0)]);
    let id = k
        .revolve(&Profile::new(front(), vec![r]), z, TAU)
        .unwrap()
        .bodies[0];
    let b = body(&k, id);
    valid(&b);
    assert_eq!(b.solids.len(), 1);
    assert_eq!(
        b.solids[0].shells.len(),
        2,
        "the outer shell and the hole's void"
    );
    let v = k.mass_properties(id).unwrap().volume;
    let want = (16.0 - PI) * TAU * 7.0;
    assert!((v - want).abs() < 1e-6 * want, "{v} vs {want}");
}

#[test]
fn a_sheet_extrude_of_a_holed_region_has_a_shell_per_loop() {
    // The walls the outer loop and the hole sweep share no edge: two shells.
    let mut k = NativeKernel::new();
    let p = Profile::new(
        Plane::top(),
        vec![region(
            rect(0.0, 0.0, 10.0, 10.0),
            vec![circle(5.0, 5.0, 2.0)],
        )],
    );
    let spec = ExtrudeSpec {
        body: BodyKind::Surface,
        ..ExtrudeSpec::blind(Vector3::z_axis(), 3.0)
    };
    let id = k.extrude_with(&p, &spec).unwrap().bodies[0];
    let b = body(&k, id);
    valid(&b);
    assert_eq!(b.shells.len(), 2);
}
