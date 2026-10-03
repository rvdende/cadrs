//! Backend-generic conformance suite. Every case takes `&mut dyn Kernel`; each enabled backend
//! instantiates all of them in its own module at the bottom of this file.
#![allow(dead_code, unused_macros)]

use std::f64::consts::{FRAC_PI_4, PI, TAU};

use cadrs_kernel::*;
use nalgebra::{Point2, Point3, Translation3, UnitQuaternion, Vector3};

// ---------------------------------------------------------------------------------------------
// Helpers

fn p(x: f64, y: f64) -> Point2<f64> {
    Point2::new(x, y)
}

fn line(a: Point2<f64>, b: Point2<f64>) -> Curve2 {
    Curve2::Line { a, b, source: None }
}

fn arc(center: Point2<f64>, radius: f64, start_angle: f64, sweep: f64) -> Curve2 {
    Curve2::Arc {
        center,
        radius,
        start_angle,
        sweep,
        source: None,
    }
}

fn circle(center: Point2<f64>, radius: f64) -> Loop {
    Loop {
        curves: vec![Curve2::Circle {
            center,
            radius,
            source: None,
        }],
    }
}

fn polygon(points: &[Point2<f64>]) -> Loop {
    let n = points.len();
    Loop {
        curves: (0..n)
            .map(|i| line(points[i], points[(i + 1) % n]))
            .collect(),
    }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Loop {
    polygon(&[p(x0, y0), p(x1, y0), p(x1, y1), p(x0, y1)])
}

fn region(outer: Loop) -> Region {
    Region {
        outer,
        holes: vec![],
        source: None,
    }
}

fn top(regions: Vec<Region>) -> Profile {
    Profile::new(Plane::top(), regions)
}

/// The XZ plane with in-plane y along +Z (normal −Y), for revolve profiles.
fn front() -> Plane {
    Plane {
        origin: Point3::origin(),
        x_dir: Vector3::x_axis(),
        normal: -Vector3::y_axis(),
    }
}

fn one(result: OpResult) -> BodyId {
    assert_eq!(result.bodies.len(), 1, "expected one body");
    result.bodies[0]
}

#[track_caller]
fn close(actual: f64, expected: f64, tol: f64) {
    assert!(
        (actual - expected).abs() <= tol,
        "got {actual}, expected {expected} ± {tol}"
    );
}

/// A 10 × 20 × 30 box on Top with its corner at the origin.
fn make_box(k: &mut dyn Kernel) -> BodyId {
    one(k
        .extrude(
            &top(vec![region(rect(0.0, 0.0, 10.0, 20.0))]),
            Extent::Blind(30.0),
        )
        .unwrap())
}

fn vertical_edge(k: &dyn Kernel, body: BodyId, length: f64) -> EdgeId {
    k.edges(body)
        .unwrap()
        .into_iter()
        .find(|e| (e.length - length).abs() < 1e-9)
        .expect("an edge of that length")
        .id
}

// ---------------------------------------------------------------------------------------------
// Cases

pub fn box_extrude(k: &mut dyn Kernel) {
    let b = make_box(k);
    let m = k.mass_properties(b).unwrap();
    close(m.volume, 6000.0, 1e-6);
    close(m.surface_area, 2.0 * (200.0 + 300.0 + 600.0), 1e-6);
    close(m.center_of_mass.x, 5.0, 1e-9);
    close(m.center_of_mass.y, 10.0, 1e-9);
    close(m.center_of_mass.z, 15.0, 1e-9);

    let faces = k.faces(b).unwrap();
    assert_eq!(faces.len(), 6);
    assert!(
        faces
            .iter()
            .all(|f| f.kind == SurfaceKind::Plane && f.plane.is_some())
    );
    close(faces.iter().map(|f| f.area).sum(), 2200.0, 1e-6);

    let edges = k.edges(b).unwrap();
    assert_eq!(edges.len(), 12);
    close(
        edges.iter().map(|e| e.length).sum(),
        4.0 * (10.0 + 20.0 + 30.0),
        1e-9,
    );
    assert!(
        edges
            .iter()
            .all(|e| e.faces[0].is_some() && e.faces[1].is_some())
    );
}

pub fn extrude_symmetric(k: &mut dyn Kernel) {
    let profile = top(vec![region(rect(0.0, 0.0, 10.0, 20.0))]);
    let b = one(k.extrude(&profile, Extent::Symmetric(30.0)).unwrap());
    let m = k.mass_properties(b).unwrap();
    close(m.volume, 6000.0, 1e-6);
    close(m.center_of_mass.z, 0.0, 1e-9);
}

pub fn extrude_two_sided(k: &mut dyn Kernel) {
    let profile = top(vec![region(rect(0.0, 0.0, 10.0, 20.0))]);
    let b = one(k
        .extrude(
            &profile,
            Extent::TwoSided {
                forward: 30.0,
                backward: 10.0,
            },
        )
        .unwrap());
    let m = k.mass_properties(b).unwrap();
    close(m.volume, 8000.0, 1e-6);
    close(m.center_of_mass.z, 10.0, 1e-9);
}

pub fn extrude_negative_blind(k: &mut dyn Kernel) {
    let profile = top(vec![region(rect(0.0, 0.0, 10.0, 20.0))]);
    let b = one(k.extrude(&profile, Extent::Blind(-30.0)).unwrap());
    let m = k.mass_properties(b).unwrap();
    close(m.volume, 6000.0, 1e-6);
    close(m.center_of_mass.z, -15.0, 1e-9);
}

/// A clockwise outer loop and a counter-clockwise hole must be reoriented by the backend.
pub fn extrude_region_with_hole(k: &mut dyn Kernel) {
    let mut outer = rect(0.0, 0.0, 40.0, 40.0);
    outer.curves.reverse();
    for c in &mut outer.curves {
        if let Curve2::Line { a, b, .. } = c {
            std::mem::swap(a, b);
        }
    }
    let profile = top(vec![Region {
        outer,
        holes: vec![circle(p(20.0, 20.0), 5.0)],
        source: None,
    }]);
    let b = one(k.extrude(&profile, Extent::Blind(10.0)).unwrap());
    let m = k.mass_properties(b).unwrap();
    close(m.volume, (1600.0 - PI * 25.0) * 10.0, 1e-6);
    close(
        m.surface_area,
        2.0 * (1600.0 - PI * 25.0) + 160.0 * 10.0 + TAU * 5.0 * 10.0,
        1e-6,
    );
}

pub fn boolean_subtract_intersect(k: &mut dyn Kernel) {
    let a = make_box(k);
    let tool = one(k
        .extrude(
            &top(vec![region(rect(5.0, 5.0, 15.0, 15.0))]),
            Extent::Blind(30.0),
        )
        .unwrap());
    let cut = one(k.boolean(BoolOp::Subtract, a, &[tool]).unwrap());
    close(
        k.mass_properties(cut).unwrap().volume,
        6000.0 - 1500.0,
        1e-6,
    );
    let common = one(k.boolean(BoolOp::Intersect, a, &[tool]).unwrap());
    close(k.mass_properties(common).unwrap().volume, 1500.0, 1e-6);
    let fused = one(k.boolean(BoolOp::Union, a, &[tool]).unwrap());
    close(
        k.mass_properties(fused).unwrap().volume,
        6000.0 + 1500.0,
        1e-6,
    );
    // Inputs are untouched.
    close(k.mass_properties(a).unwrap().volume, 6000.0, 1e-6);
}

pub fn boolean_multiple_tools(k: &mut dyn Kernel) {
    let a = make_box(k);
    let t1 = one(k
        .extrude(
            &top(vec![region(circle(p(5.0, 5.0), 2.0))]),
            Extent::Blind(30.0),
        )
        .unwrap());
    let t2 = one(k
        .extrude(
            &top(vec![region(circle(p(5.0, 15.0), 2.0))]),
            Extent::Blind(30.0),
        )
        .unwrap());
    let cut = one(k.boolean(BoolOp::Subtract, a, &[t1, t2]).unwrap());
    close(
        k.mass_properties(cut).unwrap().volume,
        6000.0 - 2.0 * PI * 4.0 * 30.0,
        1e-6,
    );
}

pub fn transform_moves_body(k: &mut dyn Kernel) {
    let b = make_box(k);
    let t = Transform::from_parts(
        Translation3::new(100.0, 0.0, 0.0),
        UnitQuaternion::from_axis_angle(&Vector3::z_axis(), PI / 2.0),
    );
    let moved = one(k.transform(b, &t).unwrap());
    let m = k.mass_properties(moved).unwrap();
    close(m.volume, 6000.0, 1e-6);
    // (5, 10, 15) rotated 90° about Z is (-10, 5, 15), then moved by +100 in x.
    close(m.center_of_mass.x, 90.0, 1e-9);
    close(m.center_of_mass.y, 5.0, 1e-9);
    close(m.center_of_mass.z, 15.0, 1e-9);
}

/// P3B.9 (Check interference): the volume two moved copies of a body share, by Intersect.
pub fn intersect_moved_copies(k: &mut dyn Kernel) {
    let b = make_box(k);
    // A copy turned 90° about Z (x −20…0, y 0…10) and moved by (15, 5, 10): x −5…15, y 5…15,
    // z 10…40. Shared with the box (x 0…10, y 0…20, z 0…30): 10 × 10 × 20 = 2000.
    let t = Transform::from_parts(
        Translation3::new(15.0, 5.0, 10.0),
        UnitQuaternion::from_axis_angle(&Vector3::z_axis(), PI / 2.0),
    );
    let moved = one(k.transform(b, &t).unwrap());
    let common = k.boolean(BoolOp::Intersect, b, &[moved]).unwrap();
    let v: f64 = common.bodies.iter().map(|c| k.mass_properties(*c).unwrap().volume).sum();
    close(v, 2000.0, 1e-6);
    // Moved apart: nothing shared, which the kernel reports as an error (an empty result).
    let apart = one(k.transform(b, &Transform::translation(0.0, 50.0, 0.0)).unwrap());
    assert!(k.boolean(BoolOp::Intersect, b, &[apart]).is_err());
}

pub fn revolve_tube(k: &mut dyn Kernel) {
    let (r, big_r, h) = (5.0, 10.0, 20.0);
    let profile = Profile::new(front(), vec![region(rect(r, 0.0, big_r, h))]);
    let axis = Axis {
        origin: Point3::origin(),
        dir: Vector3::z_axis(),
    };
    let b = one(k.revolve(&profile, axis, TAU).unwrap());
    let m = k.mass_properties(b).unwrap();
    close(m.volume, PI * (big_r * big_r - r * r) * h, 1e-6);
    close(
        m.surface_area,
        2.0 * PI * (big_r * big_r - r * r) + TAU * (big_r + r) * h,
        1e-6,
    );

    let half = one(k.revolve(&profile, axis, PI).unwrap());
    close(
        k.mass_properties(half).unwrap().volume,
        PI * (big_r * big_r - r * r) * h / 2.0,
        1e-6,
    );
}

pub fn fillet_box_edge(k: &mut dyn Kernel) {
    let b = make_box(k);
    let edge = vertical_edge(k, b, 30.0);
    let r = 2.0;
    let f = one(k.fillet(b, &[edge], r).unwrap());
    close(
        k.mass_properties(f).unwrap().volume,
        6000.0 - (1.0 - FRAC_PI_4) * r * r * 30.0,
        1e-6,
    );
    assert_eq!(k.faces(f).unwrap().len(), 7);
}

pub fn chamfer_box_edge(k: &mut dyn Kernel) {
    let b = make_box(k);
    let edge = vertical_edge(k, b, 30.0);
    let d = 2.0;
    let c = one(k
        .chamfer(b, &[edge], ChamferSpec::EqualDistance(d))
        .unwrap());
    close(
        k.mass_properties(c).unwrap().volume,
        6000.0 - d * d / 2.0 * 30.0,
        1e-6,
    );
}

pub fn shell_box(k: &mut dyn Kernel) {
    let b = make_box(k);
    let top_face = k
        .faces(b)
        .unwrap()
        .into_iter()
        .find(|f| f.plane.is_some_and(|p| (p.origin.z - 30.0).abs() < 1e-9))
        .expect("top face")
        .id;
    match k.shell(b, &[top_face], 1.0) {
        Ok(result) => {
            let s = one(result);
            close(
                k.mass_properties(s).unwrap().volume,
                6000.0 - 8.0 * 18.0 * 29.0,
                1e-6,
            );
        }
        Err(KernelError::Unsupported(_)) => {}
        Err(e) => panic!("shell failed: {e}"),
    }
}

pub fn tessellate_and_export(k: &mut dyn Kernel) {
    let profile = top(vec![region(circle(p(0.0, 0.0), 10.0))]);
    let b = one(k.extrude(&profile, Extent::Blind(5.0)).unwrap());
    let mesh = k.tessellate(b, Tessellation::default()).unwrap();
    assert!(!mesh.indices.is_empty());
    assert_eq!(mesh.positions.len(), mesh.normals.len());
    assert_eq!(mesh.indices.len(), mesh.triangle_faces.len());
    let n = mesh.positions.len() as u32;
    assert!(mesh.indices.iter().flatten().all(|&i| i < n));
    let faces = k.faces(b).unwrap().len() as u64;
    assert!(mesh.triangle_faces.iter().all(|f| f.0 < faces));
    assert!(!mesh.edges.is_empty());

    // Signed volume of the closed mesh is close to the exact one: checks the winding.
    let vol: f64 = mesh
        .indices
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| mesh.positions[i as usize].coords);
            a.dot(&b.cross(&c)) / 6.0
        })
        .sum();
    close(vol, PI * 100.0 * 5.0, PI * 100.0 * 5.0 * 0.01);

    let step = k.export_step(&[b]).unwrap();
    assert!(String::from_utf8_lossy(&step).starts_with("ISO-10303-21"));
}

pub fn circle_edges_have_exact_length(k: &mut dyn Kernel) {
    let profile = top(vec![region(circle(p(0.0, 0.0), 10.0))]);
    let b = one(k.extrude(&profile, Extent::Blind(5.0)).unwrap());
    let edges = k.edges(b).unwrap();
    let circles: Vec<_> = edges
        .iter()
        .filter(|e| (e.length - TAU * 10.0).abs() < 1e-9)
        .collect();
    assert_eq!(circles.len(), 2, "edges: {edges:?}");
}

/// PS6 Control Arm from `reference/onshape/training/intro-to-part-studios.md`, built as the
/// course does it: Extrude 1 (40 mm) of the hub ring, right web and right eye ring, then
/// Extrude 2 (25 mm, Add) of the left web and left eye ring.
pub fn control_arm(k: &mut dyn Kernel) {
    const HUB_R: f64 = 35.0;
    const BORE_R: f64 = 17.5;
    const EYE_R: f64 = 17.5;
    const EYE_HOLE_R: f64 = 10.0;
    const X: f64 = 107.5;
    const KEY_HW: f64 = 22.5;
    const KEY_HH: f64 = 5.0;

    // External tangents: the tangent points lie along (cos t, ±sin t) from each center.
    let t = ((HUB_R - EYE_R) / X).acos();
    let on = |cx: f64, r: f64, a: f64| p(cx + r * a.cos(), r * a.sin());

    // Hub ring: Ø70 with the bore (Ø35 ∪ 45 × 10 keyway) as the hole.
    let a = (KEY_HH / BORE_R).asin();
    let bx = BORE_R * a.cos();
    let bore = Loop {
        curves: vec![
            arc(p(0.0, 0.0), BORE_R, a, PI - 2.0 * a),
            line(p(-bx, KEY_HH), p(-KEY_HW, KEY_HH)),
            line(p(-KEY_HW, KEY_HH), p(-KEY_HW, -KEY_HH)),
            line(p(-KEY_HW, -KEY_HH), p(-bx, -KEY_HH)),
            arc(p(0.0, 0.0), BORE_R, PI + a, PI - 2.0 * a),
            line(p(bx, -KEY_HH), p(KEY_HW, -KEY_HH)),
            line(p(KEY_HW, -KEY_HH), p(KEY_HW, KEY_HH)),
            line(p(KEY_HW, KEY_HH), p(bx, KEY_HH)),
        ],
    };
    let hub_ring = Region {
        outer: circle(p(0.0, 0.0), HUB_R),
        holes: vec![bore],
        source: None,
    };

    let eye_ring = |cx: f64| Region {
        outer: circle(p(cx, 0.0), EYE_R),
        holes: vec![circle(p(cx, 0.0), EYE_HOLE_R)],
        source: None,
    };

    // Right web, counter-clockwise: lower tangent, around the eye (clockwise, outside it),
    // upper tangent back, then the small hub arc (clockwise).
    let right_web = region(Loop {
        curves: vec![
            line(on(0.0, HUB_R, -t), on(X, EYE_R, -t)),
            arc(p(X, 0.0), EYE_R, -t, -(TAU - 2.0 * t)),
            line(on(X, EYE_R, t), on(0.0, HUB_R, t)),
            arc(p(0.0, 0.0), HUB_R, t, -2.0 * t),
        ],
    });
    let left_web = region(Loop {
        curves: vec![
            line(on(0.0, HUB_R, PI - t), on(-X, EYE_R, PI - t)),
            arc(p(-X, 0.0), EYE_R, PI - t, -(TAU - 2.0 * t)),
            line(on(-X, EYE_R, PI + t), on(0.0, HUB_R, PI + t)),
            arc(p(0.0, 0.0), HUB_R, PI + t, -2.0 * t),
        ],
    });

    let e1 = one(k
        .extrude(
            &top(vec![hub_ring, right_web, eye_ring(X)]),
            Extent::Blind(40.0),
        )
        .unwrap());
    let e2 = one(k
        .extrude(&top(vec![left_web, eye_ring(-X)]), Extent::Blind(25.0))
        .unwrap());
    let part = one(k.boolean(BoolOp::Union, e1, &[e2]).unwrap());

    let m = k.mass_properties(part).unwrap();
    eprintln!(
        "{}: Control Arm volume {:.4} mm³, area {:.4} mm²",
        k.name(),
        m.volume,
        m.surface_area
    );
    close(m.volume, 368_749.705, 0.01);
    close(m.surface_area, 50_179.71, 0.05);
}

/// P3.1 acceptance: a 100 × 60 × 25 box. V = 100·60·25 = 150 000 mm³ and
/// A = 2(100·60 + 100·25 + 60·25) = 2(6000 + 2500 + 1500) = 20 000 mm².
pub fn rectangle_100x60x25(k: &mut dyn Kernel) {
    let profile = top(vec![region(rect(0.0, 0.0, 100.0, 60.0))]);
    let b = one(k.extrude(&profile, Extent::Blind(25.0)).unwrap());
    let m = k.mass_properties(b).unwrap();
    close(m.volume, 150_000.0, 1e-6);
    close(m.surface_area, 20_000.0, 1e-6);
}

/// Ellipses, whole and as arcs, including the sketch conventions (a minor radius larger than
/// the major one, or negative).
pub fn ellipse_profiles(k: &mut dyn Kernel) {
    let ellipse = |a: f64, b: f64, rotation: f64| Loop {
        curves: vec![Curve2::Ellipse {
            center: p(5.0, -3.0),
            major_radius: a,
            minor_radius: b,
            rotation,
            source: None,
        }],
    };
    // V = π·a·b·h for every way of writing the same ellipse.
    for (a, b, rot) in [(20.0, 10.0, 0.3), (10.0, 20.0, 0.3), (20.0, -10.0, -1.0)] {
        let body = one(k
            .extrude(&top(vec![region(ellipse(a, b, rot))]), Extent::Blind(5.0))
            .unwrap());
        close(k.mass_properties(body).unwrap().volume, PI * 200.0 * 5.0, 1e-6);
        let kinds: Vec<SurfaceKind> = k.faces(body).unwrap().iter().map(|f| f.kind).collect();
        assert_eq!(kinds.iter().filter(|&&s| s == SurfaceKind::Plane).count(), 2);
    }
    // Half an ellipse (a = 20 along x, b = 10) closed by its major axis: V = π·a·b/2·h.
    let half = |b: f64, start: f64, sweep: f64| Loop {
        curves: vec![
            Curve2::EllipseArc {
                center: p(0.0, 0.0),
                major_radius: 20.0,
                minor_radius: b,
                rotation: 0.0,
                start,
                sweep,
                source: None,
            },
            line(p(-20.0, 0.0), p(20.0, 0.0)),
        ],
    };
    for (b, start, sweep) in [(10.0, 0.0, PI), (-10.0, 0.0, -PI), (10.0, PI, -PI)] {
        let mut lp = half(b, start, sweep);
        // The closing line runs from the arc's end back to its start.
        let (s, e) = (lp.curves[0].start(), lp.curves[0].end());
        lp.curves[1] = line(e, s);
        let body = one(k.extrude(&top(vec![region(lp)]), Extent::Blind(4.0)).unwrap());
        close(k.mass_properties(body).unwrap().volume, PI * 200.0 / 2.0 * 4.0, 1e-6);
    }
}

/// A cubic Bézier side (Final, S12.14: a sketch's Bézier curve extruded). The region under the
/// Bézier with poles (0,0), (0,10), (20,10), (20,0), closed by the x axis, has area 120
/// (y = 30t(1 − t), x = 60t² − 40t³: ∫ y dx = 3600 ∫ t²(1 − t)² dt = 120), so 5 mm of it is
/// 600 mm³; its lateral face is one B-spline (Bézier) face, run either way round.
pub fn bezier_profiles(k: &mut dyn Kernel) {
    let bez = Curve2::Bezier { poles: [p(0.0, 0.0), p(0.0, 10.0), p(20.0, 10.0), p(20.0, 0.0)], source: None };
    for lp in [
        Loop { curves: vec![line(p(20.0, 0.0), p(0.0, 0.0)), bez.clone()] },
        Loop { curves: vec![bez.reversed(), line(p(0.0, 0.0), p(20.0, 0.0))] },
    ] {
        let body = one(k.extrude(&top(vec![region(lp)]), Extent::Blind(5.0)).unwrap());
        close(k.mass_properties(body).unwrap().volume, 600.0, 1e-6);
        let kinds: Vec<SurfaceKind> = k.faces(body).unwrap().iter().map(|f| f.kind).collect();
        assert_eq!(kinds.len(), 4, "{kinds:?}");
        assert_eq!(kinds.iter().filter(|&&s| s == SurfaceKind::Plane).count(), 3, "{kinds:?}");
    }
    // A hole with a Bézier side in a square: 40 × 40 × 5 − 600.
    let hole = Loop {
        curves: vec![
            Curve2::Bezier { poles: [p(-10.0, 0.0), p(-10.0, 10.0), p(10.0, 10.0), p(10.0, 0.0)], source: None },
            line(p(10.0, 0.0), p(-10.0, 0.0)),
        ],
    };
    let r = Region { outer: rect(-20.0, -20.0, 20.0, 20.0), holes: vec![hole], source: None };
    let body = one(k.extrude(&top(vec![r]), Extent::Blind(5.0)).unwrap());
    close(k.mass_properties(body).unwrap().volume, 8000.0 - 600.0, 1e-6);
}

/// Smooth fillet corners (Final, PS14.6; `Kernel::fillet_smooth`): the corner of a 30 mm
/// cube where three R3 fillets meet, set back 4.5 mm (1.5 R) along the contact lines, so the
/// ball cut away about the vertex V has ρ = √(4.5² + 3²).
///
/// Bounds on the volume: outside the ball the smooth solid is the default fillet F, so
/// V_smooth = V_F − |F ∩ B| + |patch region|, where the region under the patch lies in the
/// ball and in the sharp cube C: 0 ≤ |patch region| ≤ |C ∩ B| = πρ³/6 (an octant of the ball).
/// So V_F − |F ∩ B| ≤ V_smooth ≤ V_F − |F ∩ B| + πρ³/6, and never more than the sharp cube.
/// The patch meets every neighbouring face tangentially: at the mesh points they share, the
/// normals agree to under 1°.
pub fn fillet_smooth_corner(k: &mut dyn Kernel) {
    let r: f64 = 3.0;
    let setback = 1.5 * r;
    let rho = (setback * setback + r * r).sqrt();
    let cube = one(k.extrude(&top(vec![region(rect(0.0, 0.0, 30.0, 30.0))]), Extent::Blind(30.0)).unwrap());
    let v = Point3::new(30.0, 30.0, 30.0);
    let corner = k.vertices(cube).unwrap().into_iter().find(|x| (x.point - v).norm() < 1e-9).unwrap();
    assert_eq!(corner.edges.len(), 3);
    let spec = FilletSpec::radius(r);
    let sharp = volume(k, cube);
    close(sharp, 27000.0, 1e-9);
    let default = one(k.fillet_with(cube, &corner.edges, &spec).unwrap());
    let vf = volume(k, default);
    let result = k.fillet_smooth(cube, &corner.edges, &spec, setback).unwrap();
    let smooth = one(result.clone());
    let vs = volume(k, smooth);
    // |F ∩ B|: the default fillet's material within ρ of the corner.
    let plane = Plane { origin: v, x_dir: Vector3::x_axis(), normal: -Vector3::y_axis() };
    let half = Region {
        outer: Loop { curves: vec![arc(p(0.0, 0.0), rho, -PI / 2.0, PI), line(p(0.0, rho), p(0.0, -rho))] },
        holes: vec![],
        source: None,
    };
    let ball = one(k.revolve(&Profile::new(plane, vec![half]), Axis { origin: v, dir: Vector3::z_axis() }, TAU).unwrap());
    close(volume(k, ball), 4.0 / 3.0 * PI * rho.powi(3), 1e-6);
    let fb = one(k.boolean(BoolOp::Intersect, default, &[ball]).unwrap());
    let in_ball = volume(k, fb);
    let octant = PI * rho.powi(3) / 6.0;
    eprintln!("smooth corner: sharp {sharp:.4}, default {vf:.4}, smooth {vs:.4}, |F∩B| {in_ball:.4}, πρ³/6 {octant:.4}");
    assert!(vs >= vf - in_ball - 1e-6, "{vs} < {vf} − {in_ball}");
    assert!(vs <= vf - in_ball + octant + 1e-6, "{vs} > {vf} − {in_ball} + {octant}");
    assert!(vs < sharp);
    // It stays in the cube: the patch is an approximation (MakeFilling's surface meets its
    // constraints within its tolerances), so it may stand a micrometre proud of a face.
    let bb = k.bounding_box(smooth).unwrap();
    assert!((bb.max - Point3::new(30.0, 30.0, 30.0)).norm() < 2e-3 && bb.min.coords.norm() < 1e-6, "{bb:?}");
    // One patch, generated from the corner's vertex.
    let patches: Vec<FaceId> = result.history.generated.iter().filter(|(_, o)| matches!(o, Origin::FromVertex { .. })).map(|(f, _)| *f).collect();
    assert_eq!(patches.len(), 1, "{:?}", result.history.generated);
    let patch = patches[0];
    // The fillet faces (from the three edges) are still there.
    assert_eq!(result.history.generated.iter().filter(|(_, o)| matches!(o, Origin::FromEdge { .. })).count(), 3);
    // Tangent to its neighbours: along every edge between the patch and another face, at 32
    // points, the two faces' exact normals agree to under 1°.
    let mesh = k.tessellate(smooth, Tessellation { deflection: 0.01, angle: 0.1 }).unwrap();
    let mut worst: f64 = 0.0;
    let mut sides = 0;
    for e in k.edges(smooth).unwrap() {
        let other = match e.faces {
            [Some(a), Some(b)] if a == patch && b != patch => b,
            [Some(a), Some(b)] if b == patch && a != patch => a,
            _ => continue,
        };
        sides += 1;
        let poly = &mesh.edges.iter().find(|(id, _)| *id == e.id).unwrap().1;
        let pts: Vec<Point3<f64>> = (0..32).map(|i| poly[i * (poly.len() - 1) / 31]).collect();
        let (n0, n1) = (k.face_normals_at(smooth, patch, &pts).unwrap(), k.face_normals_at(smooth, other, &pts).unwrap());
        for (a, b) in n0.iter().zip(&n1) {
            worst = worst.max(a.dot(b).clamp(-1.0, 1.0).acos());
        }
    }
    eprintln!("smooth corner: {sides} sides, worst angle {:.4}°", worst.to_degrees());
    // Three flat faces and three fillets round the patch.
    assert_eq!(sides, 6);
    assert!(worst < 1f64.to_radians(), "{}°", worst.to_degrees());
    // A cylinder's rim has no corner: the plain fillet.
    let cyl = one(k.extrude(&top(vec![region(circle(p(0.0, 0.0), 10.0))]), Extent::Blind(10.0)).unwrap());
    let rim = k.edges(cyl).unwrap().into_iter().find(|e| e.curve == CurveKind::Circle && e.start.z > 5.0).unwrap().id;
    let a = one(k.fillet_smooth(cyl, &[rim], &spec, setback).unwrap());
    let b = one(k.fillet_with(cyl, &[rim], &spec).unwrap());
    close(volume(k, a), volume(k, b), 1e-9);
}

/// OCCT exceptions and failed builders come back as errors instead of aborting the process.
pub fn failures_are_errors(k: &mut dyn Kernel) {
    let b = make_box(k);
    let edge = vertical_edge(k, b, 30.0);
    // A 50 mm fillet on a 10 × 20 box's edge can't be built.
    assert!(k.fillet(b, &[edge], 50.0).is_err());
    // A collinear "arc" (three points on a line) can't be built.
    let bad = top(vec![region(Loop {
        curves: vec![
            arc(p(0.0, 0.0), 10.0, 0.0, 1e-13),
            line(p(10.0, 0.0), p(0.0, 0.0)),
        ],
    })]);
    assert!(k.extrude(&bad, Extent::Blind(5.0)).is_err());
    // A depth below the modelling tolerance (1e-6 mm) is refused, with why.
    let square = top(vec![region(rect(0.0, 0.0, 10.0, 10.0))]);
    match k.extrude(&square, Extent::Blind(1e-7)) {
        Err(KernelError::InvalidParameter(why)) => assert!(why.contains("tolerance"), "{why}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(k.extrude(&square, Extent::Blind(1e-5)).is_ok());
    // The session still works.
    close(k.mass_properties(b).unwrap().volume, 6000.0, 1e-6);
}

/// Chamfers measured with two distances or a distance and an angle.
pub fn chamfer_two_distances_and_angle(k: &mut dyn Kernel) {
    let b = make_box(k);
    let edge = vertical_edge(k, b, 30.0);
    // A right triangle of legs 2 and 3 along a 30 mm edge: 2·3/2·30 = 90 mm³ removed.
    let c = one(k.chamfer(b, &[edge], ChamferSpec::TwoDistances(2.0, 3.0)).unwrap());
    close(k.mass_properties(c).unwrap().volume, 6000.0 - 90.0, 1e-6);
    // 2 mm at 45° is the equal-distance chamfer: 2·2/2·30 = 60 mm³.
    let d = one(k
        .chamfer(
            b,
            &[edge],
            ChamferSpec::DistanceAngle {
                distance: 2.0,
                angle: FRAC_PI_4,
            },
        )
        .unwrap());
    close(k.mass_properties(d).unwrap().volume, 6000.0 - 60.0, 1e-6);
}

/// The tessellation's normals point out of the material and its edges lie on the mesh.
pub fn tessellation_normals_and_edges(k: &mut dyn Kernel) {
    let profile = top(vec![Region {
        outer: rect(-20.0, -20.0, 20.0, 20.0),
        holes: vec![circle(p(0.0, 0.0), 8.0)],
        source: None,
    }]);
    let b = one(k.extrude(&profile, Extent::Blind(10.0)).unwrap());
    let mesh = k.tessellate(b, Tessellation::default()).unwrap();
    for (t, tri) in mesh.indices.iter().enumerate() {
        let [a, b2, c] = tri.map(|i| mesh.positions[i as usize]);
        let wind = (b2 - a).cross(&(c - a));
        if wind.norm() < 1e-9 {
            continue;
        }
        let n = mesh.normals[tri[0] as usize];
        assert!(wind.dot(&n) > 0.0, "triangle {t} winds against its normal");
    }
    // The bore's normals point toward its axis.
    let bore: Vec<usize> = (0..mesh.positions.len())
        .filter(|&i| {
            let q = mesh.positions[i];
            ((q.x * q.x + q.y * q.y).sqrt() - 8.0).abs() < 1e-6 && mesh.normals[i].z.abs() < 1e-6
        })
        .collect();
    assert!(!bore.is_empty());
    for i in bore {
        let q = mesh.positions[i];
        assert!(mesh.normals[i].dot(&Vector3::new(-q.x, -q.y, 0.0)) > 0.0);
    }
    // 12 box edges, 2 bore circles and the bore's seam.
    assert_eq!(mesh.edges.len(), 15);
    // Every edge point is a mesh vertex.
    for (_, pts) in &mesh.edges {
        for q in pts {
            assert!(mesh.positions.iter().any(|v| (v - q).norm() < 1e-9));
        }
    }
    // Released bodies are gone.
    k.release(b);
    assert!(k.mass_properties(b).is_err());
}

// ---------------------------------------------------------------------------------------------
// P3.2: history, persistent naming and topology queries

/// A `w × h` rectangle whose sides carry sketch ids 1 (bottom), 2 (right), 3 (top), 4 (left).
fn sourced_rect(x0: f64, y0: f64, w: f64, h: f64) -> Loop {
    let c = [p(x0, y0), p(x0 + w, y0), p(x0 + w, y0 + h), p(x0, y0 + h)];
    Loop {
        curves: (0..4)
            .map(|i| Curve2::Line {
                a: c[i],
                b: c[(i + 1) % 4],
                source: Some(i as u64 + 1),
            })
            .collect(),
    }
}

fn op(n: u128) -> OpId {
    uuid::Uuid::from_u128(n)
}

/// An extrude reports each face's origin: the side of each profile curve and the two caps.
pub fn extrude_history(k: &mut dyn Kernel) {
    let profile = top(vec![region(sourced_rect(0.0, 0.0, 10.0, 20.0))]);
    let r = k.extrude(&profile, Extent::Blind(30.0)).unwrap();
    let b = one(r.clone());
    let faces = k.faces(b).unwrap();
    assert_eq!(r.history.generated.len(), 6);
    let origin_at = |c: [f64; 3]| {
        let f = faces
            .iter()
            .find(|f| (f.center - Point3::from(c)).norm() < 1e-9)
            .expect("a face there");
        r.history
            .generated
            .iter()
            .find(|(id, _)| *id == f.id)
            .map(|(_, o)| *o)
    };
    assert_eq!(origin_at([5.0, 10.0, 0.0]), Some(Origin::StartCap { region: 0 }));
    assert_eq!(origin_at([5.0, 10.0, 30.0]), Some(Origin::EndCap { region: 0 }));
    // Curve 1 runs along y = 0, curve 2 along x = 10, …
    for (c, curve) in [([5.0, 0.0, 15.0], 1), ([10.0, 10.0, 15.0], 2), ([5.0, 20.0, 15.0], 3), ([0.0, 10.0, 15.0], 4)] {
        assert_eq!(origin_at(c), Some(Origin::ProfileCurve { region: 0, curve }));
    }
    // Two touching regions: each has its own caps; their shared side is inside the body.
    let mut right = sourced_rect(10.0, 0.0, 10.0, 20.0);
    for c in &mut right.curves {
        if let Curve2::Line { source, .. } = c {
            *source = source.map(|s| s + 10);
        }
    }
    let r2 = k
        .extrude(&top(vec![region(sourced_rect(0.0, 0.0, 10.0, 20.0)), region(right)]), Extent::Blind(5.0))
        .unwrap();
    let caps: Vec<Origin> = r2
        .history
        .generated
        .iter()
        .map(|(_, o)| *o)
        .filter(|o| matches!(o, Origin::StartCap { .. } | Origin::EndCap { .. }))
        .collect();
    assert_eq!(caps.len(), 4);
    assert!(caps.contains(&Origin::EndCap { region: 1 }));
    // Curve 2 (x = 10) of region 0 and curve 14 (x = 10) of region 1 are shared: no faces.
    let sides: Vec<u64> = r2
        .history
        .generated
        .iter()
        .filter_map(|(_, o)| match o {
            Origin::ProfileCurve { curve, .. } => Some(*curve),
            _ => None,
        })
        .collect();
    assert_eq!(sides.len(), 6, "{sides:?}");
    assert!(!sides.contains(&2) && !sides.contains(&14));
}

/// A boolean reports kept, trimmed, split and deleted faces; naming numbers the pieces of a
/// split face and keeps the names of trimmed ones.
pub fn boolean_history_and_split_names(k: &mut dyn Kernel) {
    let a_op = op(1);
    let r = k
        .extrude(&top(vec![region(sourced_rect(0.0, 0.0, 10.0, 20.0))]), Extent::Blind(30.0))
        .unwrap();
    let a = one(r.clone());
    let a_names = naming::name_body(k, a, a_op, &r.history, &[]).unwrap();
    // A slot across the top: x from −1 to 11, y from 8 to 12, z from 20 up.
    let slot_r = k
        .extrude(
            &Profile {
                plane: Plane {
                    origin: Point3::new(0.0, 0.0, 20.0),
                    ..Plane::top()
                },
                regions: vec![region(rect(-1.0, 8.0, 11.0, 12.0))],
                chains: vec![],
            },
            Extent::Blind(20.0),
        )
        .unwrap();
    let slot = one(slot_r.clone());
    let slot_names = naming::name_body(k, slot, op(3), &slot_r.history, &[]).unwrap();
    let cut = k.boolean(BoolOp::Subtract, a, &[slot]).unwrap();
    let c = one(cut.clone());
    // The top face is split in two; the sides x = 0 and x = 10 are notched (trimmed).
    let top_face = a_names
        .faces
        .iter()
        .position(|n| n.origin == FaceOrigin::Cap { region: 0, end: true })
        .unwrap();
    let pieces = cut
        .history
        .modified
        .iter()
        .filter(|(_, from)| from.body == a && from.face.0 == top_face as u64)
        .count();
    assert_eq!(pieces, 2);
    assert!(cut.history.deleted.iter().any(|f| f.body == slot));
    let names =
        naming::name_body(k, c, op(2), &cut.history, &[(a, &a_names), (slot, &slot_names)]).unwrap();
    let top_base = FaceName::new(a_op, FaceOrigin::Cap { region: 0, end: true });
    let splits: Vec<u32> = names.faces.iter().filter(|n| n.base() == top_base).map(|n| n.split).collect();
    assert_eq!(splits.len(), 2);
    assert!(splits.contains(&1) && splits.contains(&2));
    // Trimmed sides keep their names exactly.
    let side = FaceName::new(a_op, FaceOrigin::Side { region: 0, curve: 2 });
    assert_eq!(names.faces_named(&side).len(), 1);
    // The slot's floor and walls come from the tool, and keep the tool's names.
    assert!(names.faces.iter().all(|n| n.is_stable()));
    let floor = FaceName::new(op(3), FaceOrigin::Cap { region: 0, end: false });
    assert_eq!(names.faces_named(&floor).len(), 1);
}

/// Face, edge and vertex names survive edits to the profile's dimensions and the depth, and
/// still name the geometrically matching entities.
pub fn names_survive_edits(k: &mut dyn Kernel) {
    let build = |k: &mut dyn Kernel, w: f64, depth: f64| {
        let r = k
            .extrude(&top(vec![region(sourced_rect(0.0, 0.0, w, 20.0))]), Extent::Blind(depth))
            .unwrap();
        let b = one(r.clone());
        (b, naming::name_body(k, b, op(1), &r.history, &[]).unwrap())
    };
    let (b1, n1) = build(k, 10.0, 30.0);
    let (b2, n2) = build(k, 16.0, 45.0);
    let top_right = |names: &BodyNames| {
        EdgeName::new(
            FaceName::new(op(1), FaceOrigin::Cap { region: 0, end: true }),
            FaceName::new(op(1), FaceOrigin::Side { region: 0, curve: 2 }),
            0,
        )
        .into_ids(names)
    };
    for (b, names, w, depth) in [(b1, &n1, 10.0, 30.0), (b2, &n2, 16.0, 45.0)] {
        // The end cap is at the depth.
        let faces = k.faces(b).unwrap();
        let end = names.faces_named(&FaceName::new(op(1), FaceOrigin::Cap { region: 0, end: true }));
        assert_eq!(end.len(), 1);
        close(faces[end[0].0 as usize].center.z, depth, 1e-9);
        // The top edge of the right side runs along x = w, z = depth.
        let e = top_right(names);
        assert_eq!(e.len(), 1);
        let info = &k.edges(b).unwrap()[e[0].0 as usize];
        close(info.mid.x, w, 1e-9);
        close(info.mid.z, depth, 1e-9);
        // 6 faces, 12 named edges, 8 named vertices, all different.
        assert_eq!(names.faces.len(), 6);
        let mut en: Vec<EdgeName> = names.edges.iter().flatten().copied().collect();
        en.sort();
        en.dedup();
        assert_eq!(en.len(), 12);
        let mut vn: Vec<VertexName> = names.vertices.iter().flatten().copied().collect();
        vn.sort();
        vn.dedup();
        assert_eq!(vn.len(), 8);
    }
    // The same names in both builds.
    let set = |n: &BodyNames| {
        let mut v: Vec<FaceName> = n.faces.clone();
        v.sort();
        v
    };
    assert_eq!(set(&n1), set(&n2));
}

trait IntoIds {
    fn into_ids(self, names: &BodyNames) -> Vec<EdgeId>;
}

impl IntoIds for EdgeName {
    fn into_ids(self, names: &BodyNames) -> Vec<EdgeId> {
        names.edges_named(&self)
    }
}

/// Face adjacency: a box's top touches its four sides; a cylinder's side touches both caps.
pub fn face_adjacency(k: &mut dyn Kernel) {
    let b = make_box(k);
    let faces = k.faces(b).unwrap();
    let top_face = faces
        .iter()
        .find(|f| (f.center.z - 30.0).abs() < 1e-9)
        .unwrap()
        .id;
    let adjacent = k.adjacent_faces(b, top_face).unwrap();
    assert_eq!(adjacent.len(), 4);
    for f in adjacent {
        close(faces[f.0 as usize].center.z, 15.0, 1e-9);
    }
    let cyl = one(k.extrude(&top(vec![region(circle(p(0.0, 0.0), 5.0))]), Extent::Blind(10.0)).unwrap());
    let faces = k.faces(cyl).unwrap();
    let side = faces.iter().find(|f| f.kind == SurfaceKind::Cylinder).unwrap().id;
    assert_eq!(k.adjacent_faces(cyl, side).unwrap().len(), 2);
}

/// Tangent chains: the top outline of a slot (two lines and two tangent half circles) is one
/// chain of four edges; a box edge is a chain of one.
pub fn tangent_chains(k: &mut dyn Kernel) {
    let slot = Loop {
        curves: vec![
            line(p(0.0, 0.0), p(30.0, 0.0)),
            arc(p(30.0, 10.0), 10.0, -PI / 2.0, PI),
            line(p(30.0, 20.0), p(0.0, 20.0)),
            arc(p(0.0, 10.0), 10.0, PI / 2.0, PI),
        ],
    };
    let b = one(k.extrude(&top(vec![region(slot)]), Extent::Blind(5.0)).unwrap());
    let edges = k.edges(b).unwrap();
    let start = edges
        .iter()
        .find(|e| e.curve == CurveKind::Line && (e.mid - Point3::new(15.0, 0.0, 5.0)).norm() < 1e-9)
        .unwrap()
        .id;
    let chain = k.tangent_chain(b, start).unwrap();
    assert_eq!(chain.len(), 4, "{chain:?}");
    for id in &chain {
        close(edges[id.0 as usize].mid.z, 5.0, 1e-9);
    }
    // The four top edges add up to the slot's perimeter: 2·30 + 2π·10.
    close(chain.iter().map(|e| edges[e.0 as usize].length).sum(), 60.0 + TAU * 10.0, 1e-9);
    let bx = make_box(k);
    let e = vertical_edge(k, bx, 30.0);
    assert_eq!(k.tangent_chain(bx, e).unwrap(), vec![e]);
}

/// Exact edge geometry and vertices: a box has 8 vertices of 3 edges each; a line's tangents
/// point along it.
pub fn edges_and_vertices(k: &mut dyn Kernel) {
    let b = make_box(k);
    let vertices = k.vertices(b).unwrap();
    assert_eq!(vertices.len(), 8);
    assert!(vertices.iter().all(|v| v.edges.len() == 3));
    for e in k.edges(b).unwrap() {
        assert_eq!(e.curve, CurveKind::Line);
        close((e.end - e.start).norm(), e.length, 1e-9);
        let d = (e.end - e.start).normalize();
        close(d.dot(&e.start_tangent), 1.0, 1e-9);
        close(d.dot(&e.end_tangent), 1.0, 1e-9);
        close((e.mid - nalgebra::center(&e.start, &e.end)).norm(), 0.0, 1e-9);
    }
    // A cylinder's cap circles are closed edges of length 2πr.
    let cyl = one(k.extrude(&top(vec![region(circle(p(0.0, 0.0), 5.0))]), Extent::Blind(10.0)).unwrap());
    let circles: Vec<EdgeInfo> = k
        .edges(cyl)
        .unwrap()
        .into_iter()
        .filter(|e| e.curve == CurveKind::Circle)
        .collect();
    assert_eq!(circles.len(), 2);
    for c in circles {
        assert!(c.is_closed());
        close(c.length, TAU * 5.0, 1e-9);
    }
}

// ---------------------------------------------------------------------------------------------
// P3.3: the full extrude, splitting, rays and bounding boxes

fn up() -> nalgebra::Unit<Vector3<f64>> {
    Vector3::z_axis()
}

fn down() -> nalgebra::Unit<Vector3<f64>> {
    -Vector3::z_axis()
}

/// A plate on Top: `x0..x1 × y0..y1`, from `z0` to `z1`.
fn plate(k: &mut dyn Kernel, x0: f64, y0: f64, x1: f64, y1: f64, z0: f64, z1: f64) -> BodyId {
    let profile = Profile::new(
        Plane {
            origin: Point3::new(0.0, 0.0, z0),
            ..Plane::top()
        },
        vec![region(rect(x0, y0, x1, y1))],
    );
    one(k.extrude(&profile, Extent::Blind(z1 - z0)).unwrap())
}

/// The planar face of `body` facing `normal` whose centre is at height `z`.
fn face_at(k: &dyn Kernel, body: BodyId, normal: Vector3<f64>, z: f64) -> FaceId {
    k.faces(body)
        .unwrap()
        .into_iter()
        .find(|f| {
            f.plane.is_some_and(|pl| pl.normal.dot(&normal) > 0.999999) && (f.center.z - z).abs() < 1e-6
        })
        .expect("a face there")
        .id
}

fn square(half: f64) -> Profile {
    top(vec![region(rect(-half, -half, half, half))])
}

fn volume(k: &dyn Kernel, b: BodyId) -> f64 {
    k.mass_properties(b).unwrap().volume
}

fn at_height(z: f64, regions: Vec<Region>) -> Profile {
    Profile::new(
        Plane {
            origin: Point3::new(0.0, 0.0, z),
            ..Plane::top()
        },
        regions,
    )
}

/// Up to face, parallel: a 10 × 10 square on Top up to the underside of a plate at z = 30 gives
/// 10·10·30 = 3000; with an offset of 5 (stopping short) 10·10·25 = 2500.
pub fn extrude_up_to_face_parallel(k: &mut dyn Kernel) {
    let target = plate(k, -50.0, -50.0, 50.0, 50.0, 30.0, 40.0);
    let face = face_at(k, target, -Vector3::z(), 30.0);
    let mut spec = ExtrudeSpec::blind(up(), 1.0);
    spec.end = ExtrudeEnd::UpToFace { body: target, face, offset: 0.0 };
    let b = one(k.extrude_with(&square(5.0), &spec).unwrap());
    close(volume(k, b), 3000.0, 1e-6);
    spec.end = ExtrudeEnd::UpToFace { body: target, face, offset: 5.0 };
    let b = one(k.extrude_with(&square(5.0), &spec).unwrap());
    close(volume(k, b), 2500.0, 1e-6);
    // Against the face's direction there is nothing to reach.
    spec.direction = down();
    assert!(matches!(k.extrude_with(&square(5.0), &spec), Err(KernelError::InvalidParameter(_))));
}

/// Up to face, oblique: the slanted underside z = 30 + 0.2·x of a wedge. Over the square
/// −5..5 the x term cancels: V = ∫∫ (30 + 0.2x) = 30·100 = 3000.
pub fn extrude_up_to_face_oblique(k: &mut dyn Kernel) {
    // The wedge's side profile on Front (x along X, y along Z). Front's normal is −Y, so the
    // sweep runs from y = 50 to y = −50.
    let plane = Plane {
        origin: Point3::new(0.0, 50.0, 0.0),
        ..front()
    };
    let wedge = Profile::new(
        plane,
        vec![region(polygon(&[p(-50.0, 20.0), p(50.0, 40.0), p(50.0, 60.0), p(-50.0, 60.0)]))],
    );
    let w = one(k.extrude(&wedge, Extent::Blind(100.0)).unwrap());
    let slanted = k
        .faces(w)
        .unwrap()
        .into_iter()
        .find(|f| f.plane.is_some_and(|pl| pl.normal.z < -0.5 && pl.normal.x.abs() > 0.05))
        .expect("the slanted face")
        .id;
    let mut spec = ExtrudeSpec::blind(up(), 1.0);
    spec.end = ExtrudeEnd::UpToFace { body: w, face: slanted, offset: 0.0 };
    let r = k.extrude_with(&square(5.0), &spec).unwrap();
    close(volume(k, r.bodies[0]), 3000.0, 1e-6);
    // The trimmed end is the end cap: no face is left unnamed.
    assert_eq!(r.history.generated.len(), k.faces(r.bodies[0]).unwrap().len());
    assert!(r.history.generated.iter().any(|(_, o)| matches!(o, Origin::EndCap { .. })));
}

/// Up to face, oblique, with the face's plane through the sketch plane's origin (an arc's
/// radial end face): the profile, 10 × 10 over x = 15..25, is wholly on one side of the plane
/// z = 2x, so the sweep stops at it: V = ∫∫ 2x = 10·10·40 = 4000.
pub fn extrude_up_to_face_through_origin(k: &mut dyn Kernel) {
    let plane = Plane {
        origin: Point3::new(0.0, 50.0, 0.0),
        ..front()
    };
    let wedge = Profile::new(
        plane,
        vec![region(polygon(&[p(10.0, 20.0), p(30.0, 60.0), p(30.0, 70.0), p(10.0, 70.0)]))],
    );
    let w = one(k.extrude(&wedge, Extent::Blind(100.0)).unwrap());
    let slanted = k
        .faces(w)
        .unwrap()
        .into_iter()
        .find(|f| f.plane.is_some_and(|pl| pl.normal.x > 0.5 && pl.normal.z < -0.3))
        .expect("the slanted face")
        .id;
    let mut spec = ExtrudeSpec::blind(up(), 1.0);
    spec.end = ExtrudeEnd::UpToFace { body: w, face: slanted, offset: 0.0 };
    let r = k.extrude_with(&top(vec![region(rect(15.0, -5.0, 25.0, 5.0))]), &spec).unwrap();
    close(volume(k, r.bodies[0]), 4000.0, 1e-6);
    // A profile the plane crosses is refused.
    let across = top(vec![region(rect(-5.0, -5.0, 5.0, 5.0))]);
    assert!(matches!(k.extrude_with(&across, &spec), Err(KernelError::InvalidParameter(_))));
}

/// Up to part, conforming: a square under a cylinder of radius 10 along X at z = 30. The end
/// follows the cylinder's underside, z = 30 − √(100 − y²):
/// V = 10·(10·30 − ∫₋₅⁵ √(100 − y²) dy) = 10·(300 − (5√75 + 100·asin ½)) = 2043.3885.
pub fn extrude_up_to_part_conforms(k: &mut dyn Kernel) {
    let right = Plane {
        origin: Point3::new(-50.0, 0.0, 0.0),
        x_dir: Vector3::y_axis(),
        normal: Vector3::x_axis(),
    };
    let cyl = one(k
        .extrude(&Profile::new(right, vec![region(circle(p(0.0, 30.0), 10.0))]), Extent::Blind(100.0))
        .unwrap());
    let expected = 10.0 * (300.0 - (5.0 * 75f64.sqrt() + 100.0 * 0.5f64.asin()));
    let mut spec = ExtrudeSpec::blind(up(), 1.0);
    spec.end = ExtrudeEnd::UpToPart { body: cyl, offset: 0.0 };
    let b = one(k.extrude_with(&square(5.0), &spec).unwrap());
    close(volume(k, b), expected, 1e-6);
    // Up to next finds the same cylinder.
    spec.end = ExtrudeEnd::UpToNext { offset: 0.0 };
    spec.scene = vec![cyl];
    let b = one(k.extrude_with(&square(5.0), &spec).unwrap());
    close(volume(k, b), expected, 1e-6);
    // A profile wider than the cylinder only partly meets it: an error, not a guess.
    let wide = top(vec![region(rect(-5.0, -20.0, 5.0, 20.0))]);
    assert!(matches!(k.extrude_with(&wide, &spec), Err(KernelError::InvalidParameter(_))));
}

/// Up to next from a sketch on a plate's top face, cutting down: the next face is the plate's
/// bottom, so the tool is the plate's thickness deep: V = π·5²·20. Upward nothing lies ahead (an
/// error). From z = 50 down past two plates, the first one met (its top at z = 20) ends it:
/// π·5²·30.
pub fn extrude_up_to_next_from_a_face(k: &mut dyn Kernel) {
    let plate_b = plate(k, -50.0, -50.0, 50.0, 50.0, 0.0, 20.0);
    let hole = || vec![region(circle(p(0.0, 0.0), 5.0))];
    let mut spec = ExtrudeSpec::blind(down(), 1.0);
    spec.end = ExtrudeEnd::UpToNext { offset: 0.0 };
    spec.scene = vec![plate_b];
    let b = one(k.extrude_with(&at_height(20.0, hole()), &spec).unwrap());
    close(volume(k, b), PI * 25.0 * 20.0, 1e-6);
    spec.direction = up();
    assert!(matches!(
        k.extrude_with(&at_height(20.0, hole()), &spec),
        Err(KernelError::InvalidParameter(_))
    ));
    let lower = plate(k, -50.0, -50.0, 50.0, 50.0, -30.0, -10.0);
    spec.direction = down();
    spec.scene = vec![lower, plate_b];
    let b = one(k.extrude_with(&at_height(50.0, hole()), &spec).unwrap());
    close(volume(k, b), PI * 25.0 * 30.0, 1e-6);
}

/// Through all: from the plate's mid-plane both ways (Symmetric) covers its thickness,
/// π·5²·20; from z = 30 down it reaches the plate's bottom, π·5²·30. Without parts it is an
/// error.
pub fn extrude_through_all(k: &mut dyn Kernel) {
    let plate_b = plate(k, -50.0, -50.0, 50.0, 50.0, 0.0, 20.0);
    let hole = || vec![region(circle(p(0.0, 0.0), 5.0))];
    let mut spec = ExtrudeSpec::blind(up(), 1.0);
    spec.end = ExtrudeEnd::ThroughAll;
    spec.symmetric = true;
    spec.scene = vec![plate_b];
    let b = one(k.extrude_with(&at_height(10.0, hole()), &spec).unwrap());
    close(volume(k, b), PI * 25.0 * 20.0, 1e-6);
    spec.symmetric = false;
    spec.direction = down();
    let b = one(k.extrude_with(&at_height(30.0, hole()), &spec).unwrap());
    close(volume(k, b), PI * 25.0 * 30.0, 1e-6);
    spec.scene.clear();
    assert!(matches!(
        k.extrude_with(&at_height(30.0, hole()), &spec),
        Err(KernelError::InvalidParameter(_))
    ));
}

/// Starting offset, Up to vertex with an offset, a second end, and a direction out of the
/// normal. Square 10 × 10 (area 100):
/// - start 5 up, up to a vertex at z = 40 with offset 10: z 5..30, V = 2500;
/// - Blind 10 and a second end Blind 4: V = 1400;
/// - along (0, 1, 1)/√2 for 10√2: an oblique prism 10 high, V = 1000.
pub fn extrude_options(k: &mut dyn Kernel) {
    let mut spec = ExtrudeSpec::blind(up(), 1.0);
    spec.start_offset = 5.0;
    spec.end = ExtrudeEnd::UpToVertex { point: Point3::new(3.0, 7.0, 40.0), offset: 10.0 };
    let b = one(k.extrude_with(&square(5.0), &spec).unwrap());
    close(volume(k, b), 2500.0, 1e-6);
    let bb = k.bounding_box(b).unwrap();
    close(bb.min.z, 5.0, 1e-6);
    close(bb.max.z, 30.0, 1e-6);

    let mut spec = ExtrudeSpec::blind(up(), 10.0);
    spec.second = Some(ExtrudeEnd::Blind(4.0));
    let b = one(k.extrude_with(&square(5.0), &spec).unwrap());
    close(volume(k, b), 1400.0, 1e-6);
    close(k.bounding_box(b).unwrap().min.z, -4.0, 1e-6);

    let slant = nalgebra::Unit::new_normalize(Vector3::new(0.0, 1.0, 1.0));
    let spec = ExtrudeSpec::blind(slant, 10.0 * 2f64.sqrt());
    let b = one(k.extrude_with(&square(5.0), &spec).unwrap());
    close(volume(k, b), 1000.0, 1e-6);
    close(k.bounding_box(b).unwrap().max.y, 15.0, 1e-6);
    // A direction in the sketch plane can't be extruded along.
    let flat = ExtrudeSpec::blind(Vector3::x_axis(), 10.0);
    assert!(matches!(k.extrude_with(&square(5.0), &flat), Err(KernelError::InvalidParameter(_))));
}

/// Surface extrude: a 20 × 10 rectangle's boundary swept 5 gives four sheets of area
/// 2·(20 + 10)·5 = 300 and no volume; an open chain (a line 30 long) gives a sheet 30·5.
pub fn surface_extrude(k: &mut dyn Kernel) {
    let mut spec = ExtrudeSpec::blind(up(), 5.0);
    spec.body = BodyKind::Surface;
    let b = one(k.extrude_with(&top(vec![region(rect(0.0, 0.0, 20.0, 10.0))]), &spec).unwrap());
    let m = k.mass_properties(b).unwrap();
    close(m.surface_area, 300.0, 1e-6);
    close(m.volume, 0.0, 1e-12);
    assert_eq!(k.solid_count(b).unwrap(), 0);
    let open = Profile {
        plane: Plane::top(),
        regions: vec![],
        chains: vec![Chain { curves: vec![line(p(0.0, 0.0), p(30.0, 0.0))], source: Some(7) }],
    };
    let r = k.extrude_with(&open, &spec).unwrap();
    close(k.mass_properties(r.bodies[0]).unwrap().surface_area, 150.0, 1e-6);
    assert!(r.history.generated.iter().any(|(_, o)| matches!(o, Origin::ProfileCurve { region: 7, .. })));
    // A sheet's boundary edges have one face; a cylinder sheet's seam has its face on both
    // sides.
    let rect_edges = k.edges(b).unwrap();
    let free = rect_edges.iter().filter(|e| e.faces[1].is_none()).count();
    assert_eq!(free, 8, "the rectangle's top and bottom outlines");
    let tube = one(k.extrude_with(&top(vec![region(circle(p(0.0, 0.0), 5.0))]), &spec).unwrap());
    let edges = k.edges(tube).unwrap();
    assert_eq!(edges.iter().filter(|e| e.faces[1].is_none()).count(), 2, "the two circles");
    assert_eq!(edges.iter().filter(|e| e.faces[0].is_some() && e.faces[0] == e.faces[1]).count(), 1, "the seam");
}

/// Thin extrude of a 20 × 20 square, 10 deep: 2 mm inside gives (20² − 16²)·10 = 1440; 2 inside
/// and 1 outside (22² − 16²)·10 = 2280. An open line 30 long, 2 to its left: 30·2·10 = 600.
pub fn thin_extrude(k: &mut dyn Kernel) {
    let sq = top(vec![region(rect(0.0, 0.0, 20.0, 20.0))]);
    let mut spec = ExtrudeSpec::blind(up(), 10.0);
    spec.body = BodyKind::Thin { left: 2.0, right: 0.0 };
    let b = one(k.extrude_with(&sq, &spec).unwrap());
    close(volume(k, b), 1440.0, 1e-6);
    // Inside: the wall stays within the square.
    let bb = k.bounding_box(b).unwrap();
    close(bb.min.x, 0.0, 1e-6);
    close(bb.max.x, 20.0, 1e-6);
    spec.body = BodyKind::Thin { left: 2.0, right: 1.0 };
    let b = one(k.extrude_with(&sq, &spec).unwrap());
    close(volume(k, b), 2280.0, 1e-6);
    let open = Profile {
        plane: Plane::top(),
        regions: vec![],
        chains: vec![Chain { curves: vec![line(p(0.0, 0.0), p(30.0, 0.0))], source: None }],
    };
    spec.body = BodyKind::Thin { left: 2.0, right: 0.0 };
    let b = one(k.extrude_with(&open, &spec).unwrap());
    close(volume(k, b), 600.0, 1e-6);
    // Left of a line along +X (seen from +Z) is +Y.
    close(k.bounding_box(b).unwrap().max.y, 2.0, 1e-6);
}

/// Splitting, rays and boxes: a 10 × 20 × 30 box cut by a slab across its middle leaves two
/// solids of 10·20·10 each, split into two bodies. A vertical ray meets the box at z = 0 and 30.
pub fn split_solids_rays_and_boxes(k: &mut dyn Kernel) {
    let b = make_box(k);
    let bb = k.bounding_box(b).unwrap();
    close(bb.min.x, 0.0, 1e-9);
    close(bb.max.y, 20.0, 1e-9);
    close(bb.max.z, 30.0, 1e-9);
    let hits = k.ray_hits(b, Point3::new(5.0, 5.0, -10.0), Vector3::z()).unwrap();
    assert_eq!(hits.len(), 2);
    close(hits[0].t, 10.0, 1e-9);
    close(hits[1].t, 40.0, 1e-9);
    close(hits[1].point.z, 30.0, 1e-9);
    let slab = plate(k, -5.0, -5.0, 15.0, 25.0, 10.0, 20.0);
    let cut = one(k.boolean(BoolOp::Subtract, b, &[slab]).unwrap());
    assert_eq!(k.solid_count(cut).unwrap(), 2);
    let pieces = k.split_solids(cut).unwrap();
    assert_eq!(pieces.len(), 2);
    for piece in &pieces {
        let body = piece.bodies[0];
        close(volume(k, body), 2000.0, 1e-6);
        // Every face continues a face of the cut body.
        assert_eq!(piece.history.modified.len(), k.faces(body).unwrap().len());
        assert!(piece.history.modified.iter().all(|(_, from)| from.body == cut));
    }
    // One solid: one copy.
    assert_eq!(k.split_solids(b).unwrap().len(), 1);
}

/// P3H.6 (Composite part): two boxes gathered without merging, even where they touch: a
/// 10 × 20 × 30 box (6000) and a 10 × 10 × 5 plate on its top face (500) make one body of two
/// solids, volume 6500 and 6 + 6 faces (a union would merge the touching faces), each face
/// continuing its input's face.
pub fn compound_gathers_bodies(k: &mut dyn Kernel) {
    let a = make_box(k);
    let b = plate(k, 0.0, 0.0, 10.0, 10.0, 30.0, 35.0);
    let r = k.compound(&[a, b]).unwrap();
    let c = one(r.clone());
    close(volume(k, c), 6500.0, 1e-6);
    assert_eq!(k.solid_count(c).unwrap(), 2);
    let faces = k.faces(c).unwrap();
    assert_eq!(faces.len(), 12);
    assert_eq!(r.history.modified.len(), 12);
    let from_a = r.history.modified.iter().filter(|(_, f)| f.body == a).count();
    assert_eq!(from_a, 6);
    // The first body's faces come first, in its order.
    for (j, (out, from)) in r.history.modified.iter().enumerate().take(6) {
        assert_eq!(out.0 as usize, j);
        assert_eq!(from.body, a);
        assert_eq!(from.face.0 as usize, j);
    }
    // The inputs are untouched.
    close(volume(k, a), 6000.0, 1e-6);
    assert!(k.compound(&[]).is_err());
}

/// A planar face as input: the top face of the 10 × 20 × 30 box extruded 5 up gives
/// 10·20·5 = 1000; its sides are named after the box's edges, its caps after the input.
pub fn extrude_a_face(k: &mut dyn Kernel) {
    let b = make_box(k);
    let top_face = face_at(k, b, Vector3::z(), 30.0);
    let mut spec = ExtrudeSpec::blind(up(), 5.0);
    spec.faces = vec![FaceInput { body: b, face: top_face, source: 42 }];
    let r = k.extrude_with(&at_height(30.0, vec![]), &spec).unwrap();
    close(volume(k, r.bodies[0]), 1000.0, 1e-6);
    let origins: Vec<Origin> = r.history.generated.iter().map(|(_, o)| *o).collect();
    let from_edges = origins
        .iter()
        .filter(|o| matches!(o, Origin::FromEdge { body, .. } if *body == b))
        .count();
    assert_eq!(from_edges, 4);
    assert!(origins.contains(&Origin::StartCap { region: 42 }));
    assert!(origins.contains(&Origin::EndCap { region: 42 }));
}

/// Union merges faces across the seam: a 10 × 10 × 10 block with a 10 × 10 × 5 block on top of
/// it is one 10 × 10 × 15 box with 6 faces (its four sides are single faces, each continuing the
/// target's side); two regions extruded together are merged the same way.
pub fn union_merges_coplanar_faces(k: &mut dyn Kernel) {
    let a = plate(k, 0.0, 0.0, 10.0, 10.0, 0.0, 10.0);
    let b = plate(k, 0.0, 0.0, 10.0, 10.0, 10.0, 15.0);
    let r = k.boolean(BoolOp::Union, a, &[b]).unwrap();
    let fused = r.bodies[0];
    close(volume(k, fused), 1500.0, 1e-6);
    let faces = k.faces(fused).unwrap();
    assert_eq!(faces.len(), 6, "{faces:?}");
    // Each side continues a face of the target (the first input wins a merge).
    let sides: Vec<FaceId> = faces
        .iter()
        .filter(|f| f.plane.is_some_and(|p| p.normal.z.abs() < 1e-9))
        .map(|f| f.id)
        .collect();
    assert_eq!(sides.len(), 4);
    for side in sides {
        let from = r.history.modified.iter().find(|(f, _)| *f == side).map(|(_, i)| i.body);
        assert_eq!(from, Some(a));
    }
    // Two regions extruded together: one cap at each end (as Parasolid merges them), 6 faces in
    // all, and each merged cap lists both regions' caps as its origins.
    let two = top(vec![region(rect(0.0, 0.0, 10.0, 10.0)), region(rect(10.0, 0.0, 20.0, 10.0))]);
    let r = k.extrude(&two, Extent::Blind(5.0)).unwrap();
    let body = r.bodies[0];
    let faces = k.faces(body).unwrap();
    assert_eq!(faces.len(), 6, "{faces:?}");
    let caps: Vec<FaceId> = faces
        .iter()
        .filter(|f| f.plane.is_some_and(|p| p.normal.z.abs() > 0.999))
        .map(|f| f.id)
        .collect();
    assert_eq!(caps.len(), 2);
    for cap in caps {
        let regions: Vec<Origin> = r.history.generated.iter().filter(|(f, _)| *f == cap).map(|(_, o)| *o).collect();
        assert_eq!(regions.len(), 2, "{regions:?}");
    }
    close(volume(k, body), 1000.0, 1e-6);
}

// ---------------------------------------------------------------------------------------------
// P3.4: the full revolve, face axes and edge circles

fn z_axis() -> Axis {
    Axis {
        origin: Point3::origin(),
        dir: Vector3::z_axis(),
    }
}

/// The ring r 5..10, z 0..20 on Front (the tube's profile).
fn ring_profile() -> Profile {
    Profile::new(front(), vec![region(rect(5.0, 0.0, 10.0, 20.0))])
}

/// π(10² − 5²)·20: the whole tube.
const TUBE: f64 = PI * 75.0 * 20.0;

fn spec(axis: Axis, end: RevolveEnd) -> RevolveSpec {
    RevolveSpec {
        full: false,
        end,
        ..RevolveSpec::solid(axis, None)
    }
}

/// A torus from a circle: V = 2π²Rr², A = 4π²Rr (Pappus); one toroidal face with the axis.
pub fn revolve_torus(k: &mut dyn Kernel) {
    let (big_r, r) = (20.0, 5.0);
    let profile = Profile::new(front(), vec![region(circle(p(big_r, 10.0), r))]);
    let b = one(k.revolve_with(&profile, &RevolveSpec::solid(z_axis(), None)).unwrap());
    let m = k.mass_properties(b).unwrap();
    close(m.volume, 2.0 * PI * PI * big_r * r * r, 1e-6);
    close(m.surface_area, 4.0 * PI * PI * big_r * r, 1e-6);
    close(m.center_of_mass.z, 10.0, 1e-9);
    let faces = k.faces(b).unwrap();
    assert_eq!(faces.len(), 1);
    assert_eq!(faces[0].kind, SurfaceKind::Torus);
    let axis = faces[0].axis.expect("a torus has an axis");
    assert!(axis.dir.z.abs() > 1.0 - 1e-12);
    close(axis.origin.x, 0.0, 1e-9);
    close(axis.origin.y, 0.0, 1e-9);
}

/// Full, Blind (an angle), Symmetric and a second end (PS7.3), with their caps.
pub fn revolve_types(k: &mut dyn Kernel) {
    let profile = ring_profile();
    // Full: no caps.
    let full = k.revolve_with(&profile, &RevolveSpec::solid(z_axis(), None)).unwrap();
    close(volume(k, one(full.clone())), TUBE, 1e-6);
    assert!(
        !full
            .history
            .generated
            .iter()
            .any(|(_, o)| matches!(o, Origin::StartCap { .. } | Origin::EndCap { .. }))
    );
    // Blind 90°: a quarter, turning counter-clockwise about +Z from +X (towards +Y).
    let r = k.revolve_with(&profile, &spec(z_axis(), RevolveEnd::Angle(PI / 2.0))).unwrap();
    let q = one(r.clone());
    close(volume(k, q), TUBE / 4.0, 1e-6);
    let c = k.mass_properties(q).unwrap().center_of_mass;
    assert!(c.x > 0.0 && c.y > 0.0, "{c:?}");
    close(c.x, c.y, 1e-9);
    let origins: Vec<Origin> = r.history.generated.iter().map(|(_, o)| *o).collect();
    assert!(origins.iter().any(|o| matches!(o, Origin::StartCap { .. })));
    assert!(origins.iter().any(|o| matches!(o, Origin::EndCap { .. })));
    // The axis the other way: towards −Y.
    let flipped = Axis {
        dir: -Vector3::z_axis(),
        ..z_axis()
    };
    let f = one(k.revolve_with(&profile, &spec(flipped, RevolveEnd::Angle(PI / 2.0))).unwrap());
    assert!(k.mass_properties(f).unwrap().center_of_mass.y < 0.0);
    // Symmetric 90°: split evenly across the profile's plane.
    let sym = RevolveSpec {
        symmetric: true,
        ..spec(z_axis(), RevolveEnd::Angle(PI / 2.0))
    };
    let s = one(k.revolve_with(&profile, &sym).unwrap());
    close(volume(k, s), TUBE / 4.0, 1e-6);
    close(k.mass_properties(s).unwrap().center_of_mass.y, 0.0, 1e-9);
    // Second end: 90° one way and 45° the other.
    let two = RevolveSpec {
        second: Some(RevolveEnd::Angle(PI / 4.0)),
        ..spec(z_axis(), RevolveEnd::Angle(PI / 2.0))
    };
    let t = one(k.revolve_with(&profile, &two).unwrap());
    close(volume(k, t), TUBE * 3.0 / 8.0, 1e-6);
    // Refused: more than a turn, an axis across the profile, an axis normal to the plane.
    let over = RevolveSpec {
        second: Some(RevolveEnd::Angle(PI)),
        ..spec(z_axis(), RevolveEnd::Angle(1.5 * PI))
    };
    assert!(matches!(k.revolve_with(&profile, &over), Err(KernelError::InvalidParameter(_))));
    let across = Axis {
        origin: Point3::new(7.0, 0.0, 0.0),
        ..z_axis()
    };
    assert!(matches!(
        k.revolve_with(&profile, &RevolveSpec::solid(across, None)),
        Err(KernelError::InvalidParameter(_))
    ));
    let normal = Axis {
        origin: Point3::origin(),
        dir: Vector3::y_axis(),
    };
    assert!(matches!(
        k.revolve_with(&profile, &RevolveSpec::solid(normal, None)),
        Err(KernelError::InvalidParameter(_))
    ));
}

/// A plate far from the tube (nothing the revolve meets).
fn plate_far(k: &mut dyn Kernel) -> BodyId {
    plate(k, 100.0, 100.0, 110.0, 110.0, 0.0, 10.0)
}

/// Up to vertex, face, part and next (PS7.3), with an offset angle. The target is a plate at
/// x ≤ 0, y 5..40, z −5..30: its face x = 0 lies in a plane through the axis, a quarter turn
/// from the profile.
pub fn revolve_up_to(k: &mut dyn Kernel) {
    let profile = ring_profile();
    let plate = plate(k, -30.0, 5.0, 0.0, 40.0, -5.0, 30.0);
    let face = k
        .faces(plate)
        .unwrap()
        .into_iter()
        .find(|f| f.plane.is_some_and(|pl| pl.normal.x > 0.999))
        .unwrap()
        .id;
    let end = |e: RevolveEnd| spec(z_axis(), e);
    // Up to vertex at 60°.
    let v = Point3::new(50.0 * (PI / 3.0).cos(), 50.0 * (PI / 3.0).sin(), 3.0);
    let b = one(k.revolve_with(&profile, &end(RevolveEnd::UpToVertex { point: v, offset: 0.0 })).unwrap());
    close(volume(k, b), TUBE / 6.0, 1e-6);
    // Up to face (a plane through the axis): a quarter; with a 30° offset, a sixth.
    let b = one(k.revolve_with(&profile, &end(RevolveEnd::UpToFace { body: plate, face, offset: 0.0 })).unwrap());
    close(volume(k, b), TUBE / 4.0, 1e-6);
    let b = one(k
        .revolve_with(&profile, &end(RevolveEnd::UpToFace { body: plate, face, offset: PI / 6.0 }))
        .unwrap());
    close(volume(k, b), TUBE / 6.0, 1e-6);
    // Up to part conforms to the plate: the same quarter (the trimmed face is an end cap); with
    // the offset, a sixth.
    let r = k
        .revolve_with(&profile, &end(RevolveEnd::UpToPart { body: plate, offset: 0.0 }))
        .unwrap();
    assert!(r.history.generated.iter().any(|(_, o)| matches!(o, Origin::EndCap { .. })));
    close(volume(k, one(r)), TUBE / 4.0, 1e-5);
    let b = one(k
        .revolve_with(&profile, &end(RevolveEnd::UpToPart { body: plate, offset: PI / 6.0 }))
        .unwrap());
    close(volume(k, b), TUBE / 6.0, 1e-5);
    // Up to next finds the plate among the scene's bodies.
    let far = plate_far(k);
    let next = RevolveSpec {
        scene: vec![far, plate],
        ..end(RevolveEnd::UpToNext { offset: 0.0 })
    };
    let b = one(k.revolve_with(&profile, &next).unwrap());
    close(volume(k, b), TUBE / 4.0, 1e-5);
    // Nothing in the way is an error; so is a target the profile never meets.
    let none = RevolveSpec {
        scene: vec![far],
        ..end(RevolveEnd::UpToNext { offset: 0.0 })
    };
    assert!(k.revolve_with(&profile, &none).is_err());
    assert!(k
        .revolve_with(&profile, &end(RevolveEnd::UpToPart { body: far, offset: 0.0 }))
        .is_err());
}

/// Surface and thin revolves (PS7.4): an open line turned into a cylinder sheet and into a tube
/// wall on its left.
pub fn revolve_surface_and_thin(k: &mut dyn Kernel) {
    let chain = Profile {
        chains: vec![Chain {
            curves: vec![line(p(5.0, 0.0), p(5.0, 20.0))],
            source: Some(7),
        }],
        ..Profile::new(front(), vec![])
    };
    let surface = |s: RevolveSpec| RevolveSpec {
        body: BodyKind::Surface,
        ..s
    };
    let sheet = k.revolve_with(&chain, &surface(RevolveSpec::solid(z_axis(), None))).unwrap();
    assert!(!sheet.history.generated.is_empty());
    let s = one(sheet);
    let m = k.mass_properties(s).unwrap();
    close(m.volume, 0.0, 1e-12);
    close(m.surface_area, TAU * 5.0 * 20.0, 1e-6);
    // A quarter sheet has free edges.
    let q = one(k
        .revolve_with(&chain, &surface(spec(z_axis(), RevolveEnd::Angle(PI / 2.0))))
        .unwrap());
    close(k.mass_properties(q).unwrap().surface_area, TAU * 5.0 * 20.0 / 4.0, 1e-6);
    assert!(k.edges(q).unwrap().iter().any(|e| e.faces[1].is_none()));
    // Thin: 1 mm to the left of the line going up (towards the axis): r 4..5.
    let thin = RevolveSpec {
        body: BodyKind::Thin { left: 1.0, right: 0.0 },
        ..RevolveSpec::solid(z_axis(), None)
    };
    let t = one(k.revolve_with(&chain, &thin).unwrap());
    close(volume(k, t), PI * (25.0 - 16.0) * 20.0, 1e-6);
}

/// A planar face of a body as a revolve's input (PS7.1): the end face y = 0 of a plate x 5..10,
/// y −20..0, z 0..5 (a 5 × 5 square in a plane through the Z axis), turned a quarter about Z:
/// V = π(10² − 5²)·5/4; its sides come from the plate's edges, its caps from the input.
pub fn revolve_a_face(k: &mut dyn Kernel) {
    let b = plate(k, 5.0, -20.0, 10.0, 0.0, 0.0, 5.0);
    let info = k
        .faces(b)
        .unwrap()
        .into_iter()
        .find(|f| f.plane.is_some_and(|pl| pl.normal.y > 0.999))
        .expect("the end face y = 0");
    let (face, plane) = (info.id, info.plane.unwrap());
    let spec = RevolveSpec {
        faces: vec![FaceInput { body: b, face, source: 9 }],
        ..spec(z_axis(), RevolveEnd::Angle(PI / 2.0))
    };
    let r = k.revolve_with(&Profile::new(plane, vec![]), &spec).unwrap();
    assert!(r.history.generated.iter().any(|(_, o)| matches!(o, Origin::FromEdge { body, .. } if *body == b)));
    assert!(r.history.generated.iter().any(|(_, o)| *o == Origin::EndCap { region: 9 }));
    assert!(r.history.generated.iter().any(|(_, o)| *o == Origin::StartCap { region: 9 }));
    close(volume(k, one(r)), PI * 75.0 * 5.0 / 4.0, 1e-6);
    // Surfaces don't take faces.
    let surf = RevolveSpec { body: BodyKind::Surface, ..spec.clone() };
    assert!(k.revolve_with(&Profile::new(plane, vec![]), &surf).is_err());
}

/// The axes of curved faces and the circles of circular edges (a revolve axis can be either).
pub fn face_axes_and_edge_circles(k: &mut dyn Kernel) {
    let profile = at_height(0.0, vec![region(circle(p(3.0, 4.0), 2.5))]);
    let b = one(k.extrude(&profile, Extent::Blind(10.0)).unwrap());
    let faces = k.faces(b).unwrap();
    let side = faces.iter().find(|f| f.kind == SurfaceKind::Cylinder).expect("a cylinder");
    let axis = side.axis.expect("its axis");
    assert!(axis.dir.z.abs() > 1.0 - 1e-12);
    close(axis.origin.x, 3.0, 1e-9);
    close(axis.origin.y, 4.0, 1e-9);
    assert!(faces.iter().filter(|f| f.kind == SurfaceKind::Plane).all(|f| f.axis.is_none()));
    let circles: Vec<Circle3> = k.edges(b).unwrap().iter().filter_map(|e| e.circle).collect();
    assert_eq!(circles.len(), 2);
    for c in circles {
        close(c.radius, 2.5, 1e-12);
        close(c.center.x, 3.0, 1e-9);
        close(c.center.y, 4.0, 1e-9);
        assert!(c.normal.z.abs() > 1.0 - 1e-12);
    }
    // Straight edges have no circle.
    let b = make_box(k);
    assert!(k.edges(b).unwrap().iter().all(|e| e.circle.is_none()));
}

/// The inertia tensor (P3.5), at unit density, about the centre of mass with axes parallel to
/// the model's (Onshape's Mass properties panel convention).
pub fn inertia_tensor(k: &mut dyn Kernel) {
    // The 10 × 20 × 30 box (a, b, c along x, y, z): V = 6000, about its centre
    // Ixx = V(b² + c²)/12 = 6000·1300/12 = 650 000, Iyy = V(a² + c²)/12 = 500 000,
    // Izz = V(a² + b²)/12 = 250 000, no products (symmetric).
    let b = make_box(k);
    let m = k.mass_properties(b).unwrap();
    close(m.inertia[(0, 0)], 650_000.0, 1e-4);
    close(m.inertia[(1, 1)], 500_000.0, 1e-4);
    close(m.inertia[(2, 2)], 250_000.0, 1e-4);
    for (r, c) in [(0, 1), (0, 2), (1, 2), (1, 0), (2, 0), (2, 1)] {
        close(m.inertia[(r, c)], 0.0, 1e-4);
    }
    // About the origin (parallel axes, d = (5, 10, 15)): Ixx = 650 000 + V(10² + 15²) =
    // 2 600 000, Ixy = −V·5·10 = −300 000.
    let o = m.inertia_about(Point3::origin());
    close(o[(0, 0)], 2_600_000.0, 1e-3);
    close(o[(0, 1)], -300_000.0, 1e-3);
    close(o[(1, 0)], -300_000.0, 1e-3);
    // An L (a 20 × 10 bar A plus a 10 × 10 block B on its left end), 10 thick: A has V 2000 at
    // (10, 5), B 1000 at (5, 15), so the centre is (25/3, 25/3). Each rectangle's own product
    // is zero, so ∫(x − cx)(y − cy) dV = 2000·(5/3)(−10/3) + 1000·(−10/3)(20/3) = −300 000/9,
    // and the tensor's Ixy = +100 000/3 (the products carry the minus sign).
    let l = polygon(&[p(0.0, 0.0), p(20.0, 0.0), p(20.0, 10.0), p(10.0, 10.0), p(10.0, 20.0), p(0.0, 20.0)]);
    let b = one(k.extrude(&top(vec![region(l)]), Extent::Blind(10.0)).unwrap());
    let m = k.mass_properties(b).unwrap();
    close(m.volume, 3000.0, 1e-6);
    close(m.center_of_mass.x, 25.0 / 3.0, 1e-9);
    close(m.inertia[(0, 1)], 100_000.0 / 3.0, 1e-4);
    close(m.inertia[(1, 0)], 100_000.0 / 3.0, 1e-4);
    close(m.inertia[(0, 2)], 0.0, 1e-4);
    // A cylinder r 5, h 10 about its axis: Izz = V r²/2 = (250π)·25/2.
    let b = one(k.extrude(&top(vec![region(circle(p(0.0, 0.0), 5.0))]), Extent::Blind(10.0)).unwrap());
    let m = k.mass_properties(b).unwrap();
    close(m.inertia[(2, 2)], 250.0 * PI * 25.0 / 2.0, 1e-6);
    // Ixx = V(3r² + h²)/12 = 250π·(75 + 100)/12.
    close(m.inertia[(0, 0)], 250.0 * PI * 175.0 / 12.0, 1e-6);
}

// ---------------------------------------------------------------------------------------------
// P3.6: fillet (radius, width, overflow), chamfer options, shell options

/// The edges of `body` whose two ends lie at height `z`.
fn edges_at_height(k: &dyn Kernel, body: BodyId, z: f64) -> Vec<EdgeInfo> {
    k.edges(body)
        .unwrap()
        .into_iter()
        .filter(|e| (e.start.z - z).abs() < 1e-9 && (e.end.z - z).abs() < 1e-9)
        .collect()
}

pub fn fillet_cube_all_edges(k: &mut dyn Kernel) {
    // A 100³ cube with R10 on all 12 edges. Each edge's straight part loses the corner area
    // (1 − π/4)r² along its length; counting every edge over its full length L = 100 counts the
    // 8 corner cubes (side r) three times, and a corner cube keeps a sphere octant (πr³/6), so it
    // loses r³ − πr³/6 once. The corner correction adds back the difference:
    //   V = L³ − (4 − π)·r²·12·L/4 + 8 [3 (1 − π/4) r³ − (1 − π/6) r³]
    //     = 1e6 − (4 − π)·100·12·100/4 + 8·(3(1 − π/4) − (1 − π/6))·1000 = 975 587.01…
    let b = plate(k, 0.0, 0.0, 100.0, 100.0, 0.0, 100.0);
    let all: Vec<EdgeId> = k.edges(b).unwrap().iter().map(|e| e.id).collect();
    assert_eq!(all.len(), 12);
    let f = one(k.fillet_with(b, &all, &FilletSpec::radius(10.0)).unwrap());
    let (l, r) = (100.0, 10.0);
    let corner = 3.0 * (1.0 - FRAC_PI_4) * r * r * r - (1.0 - PI / 6.0) * r * r * r;
    let expected = l * l * l - (4.0 - PI) * r * r * 12.0 * l / 4.0 + 8.0 * corner;
    close(volume(k, f), expected, 1e-4);
    // 6 flat faces, 12 cylinders, 8 sphere octants.
    assert_eq!(k.faces(f).unwrap().len(), 26);
}

pub fn fillet_width(k: &mut dyn Kernel) {
    // Width 3 on a box edge (faces at 90°, normals φ = 90° apart): r = w / (2 sin 45°) = 3/√2,
    // removing (1 − π/4) r² per unit length: 30·(1 − π/4)·4.5.
    let b = make_box(k);
    let edge = vertical_edge(k, b, 30.0);
    let spec = FilletSpec { size: FilletSize::Width(3.0), ..FilletSpec::radius(1.0) };
    let f = one(k.fillet_with(b, &[edge], &spec).unwrap());
    close(volume(k, f), 6000.0 - (1.0 - FRAC_PI_4) * 4.5 * 30.0, 1e-6);
    // A 45° slope (a trapezoid 20 wide at the base, 10 at the top, 10 tall, 30 deep): the edge
    // between the top and the slope has normals φ = 45° apart, r = w / (2 sin 22.5°); a fillet
    // between two faces φ apart removes the area r² (tan(φ/2) − φ/2).
    let trap = one(k
        .extrude(
            &Profile::new(front(), vec![region(polygon(&[p(0.0, 0.0), p(20.0, 0.0), p(10.0, 10.0), p(0.0, 10.0)]))]),
            Extent::Blind(-30.0),
        )
        .unwrap());
    let v0 = volume(k, trap);
    close(v0, 30.0 * 150.0, 1e-6);
    let edge = k
        .edges(trap)
        .unwrap()
        .into_iter()
        .find(|e| (e.start.x - 10.0).abs() < 1e-9 && (e.end.x - 10.0).abs() < 1e-9 && (e.start.z - 10.0).abs() < 1e-9)
        .expect("the top/slope edge")
        .id;
    let phi = PI / 4.0;
    let r = 3.0 / (2.0 * (phi / 2.0).sin());
    let f = one(k.fillet_with(trap, &[edge], &spec).unwrap());
    close(v0 - volume(k, f), 30.0 * r * r * ((phi / 2.0).tan() - phi / 2.0), 1e-6);
    // The same as a radius fillet of that radius.
    let g = one(k.fillet_with(trap, &[edge], &FilletSpec::radius(r)).unwrap());
    close(volume(k, g), volume(k, f), 1e-6);
    // A width fillet along a tangent chain whose angle varies (a rounded plate cut by a slanted
    // plane z = 10 + 0.25x, whose outline is lines and ellipse arcs): its radii vary between the
    // ones of the steepest and flattest angles, so it removes less than the largest radius and
    // more than the smallest.
    let rounded = Loop {
        curves: vec![
            line(p(10.0, 0.0), p(50.0, 0.0)),
            arc(p(50.0, 10.0), 10.0, -PI / 2.0, PI / 2.0),
            line(p(60.0, 10.0), p(60.0, 30.0)),
            arc(p(50.0, 30.0), 10.0, 0.0, PI / 2.0),
            line(p(50.0, 40.0), p(10.0, 40.0)),
            arc(p(10.0, 30.0), 10.0, PI / 2.0, PI / 2.0),
            line(p(0.0, 30.0), p(0.0, 10.0)),
            arc(p(10.0, 10.0), 10.0, PI, PI / 2.0),
        ],
    };
    let plate_b = one(k.extrude(&top(vec![region(rounded)]), Extent::Blind(20.0)).unwrap());
    let wedge = one(k
        .extrude(
            &Profile::new(front(), vec![region(polygon(&[p(-10.0, 7.5), p(70.0, 27.5), p(70.0, 40.0), p(-10.0, 40.0)]))]),
            Extent::Symmetric(200.0),
        )
        .unwrap());
    let cut = one(k.boolean(BoolOp::Subtract, plate_b, &[wedge]).unwrap());
    let v_cut = volume(k, cut);
    let slope = k
        .faces(cut)
        .unwrap()
        .into_iter()
        .find(|f| f.plane.is_some_and(|pl| pl.normal.x < -0.1 && pl.normal.z > 0.5))
        .expect("the sloped face")
        .id;
    let chain_edge = k
        .edges(cut)
        .unwrap()
        .into_iter()
        .find(|e| e.faces.contains(&Some(slope)))
        .expect("an edge of the slope")
        .id;
    let chain = k.tangent_chain(cut, chain_edge).unwrap();
    assert_eq!(chain.len(), 5, "the slope's outline below the top is one tangent chain");
    let w = one(k.fillet_with(cut, &[chain_edge], &spec).unwrap());
    let removed = v_cut - volume(k, w);
    // The slope's normal (−0.25, 0, 1)/|…| is φ = acos(0.25/1.0308) = 75.96° from the x = 0
    // wall's and 90° from the walls along x. A width fillet's section area is
    // A(φ) = r²(tan(φ/2) − φ/2) with r = w/(2 sin(φ/2)), which grows with φ: the fillet removes
    // between A(75.96°) and A(90°) times the chain's length (ends excepted).
    let area = |phi: f64| {
        let r = 3.0 / (2.0 * (phi / 2.0).sin());
        r * r * ((phi / 2.0).tan() - phi / 2.0)
    };
    let infos = k.edges(cut).unwrap();
    let length: f64 = chain.iter().map(|e| infos.iter().find(|i| i.id == *e).unwrap().length).sum();
    let phi_min = (0.25f64 / (1.0f64 + 0.0625).sqrt()).acos();
    assert!(removed > 0.97 * area(phi_min) * length, "{removed} vs {}", area(phi_min) * length);
    assert!(removed < 1.03 * area(PI / 2.0) * length, "{removed} vs {}", area(PI / 2.0) * length);
}

pub fn fillet_overflow(k: &mut dyn Kernel) {
    // A 40 × 40 × 20 block with a Ø6 hole at (7, 20): an R5 fillet on the top edge at x = 0 has
    // its contact line at x = 5, across the hole (x 4..10), so it runs onto the hole's wall.
    let b = plate(k, 0.0, 0.0, 40.0, 40.0, 0.0, 20.0);
    let hole = one(k.extrude(&top(vec![region(circle(p(7.0, 20.0), 3.0))]), Extent::Blind(20.0)).unwrap());
    let d = one(k.boolean(BoolOp::Subtract, b, &[hole]).unwrap());
    let edge = edges_at_height(k, d, 20.0)
        .into_iter()
        .find(|e| e.start.x.abs() < 1e-9 && e.end.x.abs() < 1e-9)
        .unwrap()
        .id;
    let on = FilletSpec::radius(5.0);
    let off = FilletSpec { allow_overflow: false, ..on };
    let f = one(k.fillet_with(d, &[edge], &on).unwrap());
    assert!(volume(k, f) < volume(k, d));
    let err = k.fillet_with(d, &[edge], &off).unwrap_err();
    assert!(err.to_string().contains("Allow edge overflow"), "{err}");
    // Away from the hole (at x = 12, spanning 9..15) the same fillet doesn't overflow and removes
    // (1 − π/4)·25 along the 40 mm edge.
    let b2 = plate(k, 0.0, 0.0, 40.0, 40.0, 0.0, 20.0);
    let hole2 = one(k.extrude(&top(vec![region(circle(p(12.0, 20.0), 3.0))]), Extent::Blind(20.0)).unwrap());
    let d2 = one(k.boolean(BoolOp::Subtract, b2, &[hole2]).unwrap());
    let edge2 = edges_at_height(k, d2, 20.0)
        .into_iter()
        .find(|e| e.start.x.abs() < 1e-9 && e.end.x.abs() < 1e-9)
        .unwrap()
        .id;
    let g = one(k.fillet_with(d2, &[edge2], &off).unwrap());
    close(volume(k, g), volume(k, d2) - (1.0 - FRAC_PI_4) * 25.0 * 40.0, 1e-6);
}

pub fn chamfer_options(k: &mut dyn Kernel) {
    // 2 mm × 45° on a box edge removes 2·2/2 per unit length (the course's PS17.7).
    let b = make_box(k);
    let edge = vertical_edge(k, b, 30.0);
    let opts = ChamferOpts::new(ChamferSpec::DistanceAngle { distance: 2.0, angle: FRAC_PI_4 });
    let c = one(k.chamfer_with(b, &[edge], &opts).unwrap());
    close(volume(k, c), 6000.0 - 2.0 * 2.0 / 2.0 * 30.0, 1e-6);
    // Two distances 2 and 4: the same volume either way round, but the flip moves the 2 mm side
    // to the other face, so the faces x = const and y = const trade 2 mm of width.
    let widths = |k: &mut dyn Kernel, opts: ChamferOpts| -> (f64, f64) {
        let c = one(k.chamfer_with(b, &[edge], &opts).unwrap());
        close(volume(k, c), 6000.0 - 2.0 * 4.0 / 2.0 * 30.0, 1e-6);
        let faces = k.faces(c).unwrap();
        let area = |n: Vector3<f64>| {
            faces
                .iter()
                .filter(|f| f.plane.is_some_and(|pl| pl.normal.dot(&n).abs() > 0.999))
                .map(|f| f.area)
                .fold(f64::INFINITY, f64::min)
        };
        (area(Vector3::x()), area(Vector3::y()))
    };
    let two = ChamferOpts::new(ChamferSpec::TwoDistances(2.0, 4.0));
    let a = widths(k, two.clone());
    let f = widths(k, ChamferOpts { flip: true, ..two.clone() });
    close((a.0 - f.0).abs(), 2.0 * 30.0, 1e-6);
    close((a.1 - f.1).abs(), 2.0 * 30.0, 1e-6);
    // A direction override flips that edge on its own: the same as flipping all here.
    let o = widths(k, ChamferOpts { flipped: vec![edge], ..two });
    close(o.0, f.0, 1e-9);
    // Tangent measurement: the same as Offset between planar faces...
    let t = ChamferOpts { measurement: ChamferMeasure::Tangent, ..ChamferOpts::new(ChamferSpec::EqualDistance(2.0)) };
    let ct = one(k.chamfer_with(b, &[edge], &t).unwrap());
    close(volume(k, ct), 6000.0 - 2.0 * 30.0, 1e-6);
    // ...and on a cylinder's rim (r 10): the side face is straight across the rim, so the
    // tangent distance is the offset distance.
    let equal = ChamferOpts::new(ChamferSpec::EqualDistance(2.0));
    let tangent = ChamferOpts { measurement: ChamferMeasure::Tangent, ..equal.clone() };
    let cyl = one(k.extrude(&top(vec![region(circle(p(0.0, 0.0), 10.0))]), Extent::Blind(10.0)).unwrap());
    let rim = edges_at_height(k, cyl, 10.0)[0].id;
    let offset_rim = one(k.chamfer_with(cyl, &[rim], &equal).unwrap());
    let tangent_rim = one(k.chamfer_with(cyl, &[rim], &tangent).unwrap());
    close(volume(k, tangent_rim), volume(k, offset_rim), 1e-6);
    // A D-shaped prism (half disc r 10, 10 high): its two straight vertical edges join the flat
    // side to the curved side, which curves across them. There the tangent distance 2 becomes
    // the chord 2·r·sin(atan(2/r)/2) on the curved side only, so it is a two-distance chord.
    let d_loop = Loop { curves: vec![arc(p(0.0, 0.0), 10.0, 0.0, PI), line(p(-10.0, 0.0), p(10.0, 0.0))] };
    let d = one(k.extrude(&top(vec![region(d_loop)]), Extent::Blind(10.0)).unwrap());
    let straight = k
        .edges(d)
        .unwrap()
        .into_iter()
        .find(|e| e.curve == CurveKind::Line && (e.length - 10.0).abs() < 1e-9 && e.start.x > 0.0)
        .expect("a vertical edge at x = 10")
        .id;
    let chord = 2.0 * 10.0 * ((2.0f64 / 10.0).atan() / 2.0).sin();
    let cut = |k: &mut dyn Kernel, opts: &ChamferOpts| {
        let c = one(k.chamfer_with(d, &[straight], opts).unwrap());
        volume(k, c)
    };
    let offset_d = cut(k, &equal);
    let tangent_d = cut(k, &tangent);
    let two = |a, b| ChamferOpts::new(ChamferSpec::TwoDistances(a, b));
    let a = cut(k, &two(chord, 2.0));
    let b = cut(k, &two(2.0, chord));
    assert!((tangent_d - offset_d).abs() > 1e-3, "tangent {tangent_d} = offset {offset_d}");
    assert!((tangent_d - a).abs() < 1e-6 || (tangent_d - b).abs() < 1e-6, "{tangent_d} vs {a} / {b}");
}

/// The area between the corner `v` and a Bezier section from `p0` to its last pole (Green's
/// theorem over the curve and the two straight sides, sampled finely).
fn section_area(poles: &[Point2<f64>], weights: &[f64], v: Point2<f64>) -> f64 {
    let eval = |t: f64| -> Point2<f64> {
        // De Casteljau in homogeneous coordinates.
        let mut pts: Vec<(f64, f64, f64)> = poles.iter().zip(weights).map(|(p, w)| (p.x * w, p.y * w, *w)).collect();
        while pts.len() > 1 {
            pts = pts
                .windows(2)
                .map(|q| {
                    let l = |a: f64, b: f64| a + (b - a) * t;
                    (l(q[0].0, q[1].0), l(q[0].1, q[1].1), l(q[0].2, q[1].2))
                })
                .collect();
        }
        Point2::new(pts[0].0 / pts[0].2, pts[0].1 / pts[0].2)
    };
    let n = 200_000;
    let mut ring: Vec<Point2<f64>> = (0..=n).map(|i| eval(i as f64 / n as f64)).collect();
    ring.push(v);
    let mut twice = 0.0;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        twice += a.x * b.y - b.x * a.y;
    }
    twice.abs() / 2.0
}

pub fn fillet_sections(k: &mut dyn Kernel) {
    // The 10 × 20 × 30 box's vertical edge at (10, 20), r 3: the contact points are 3 from the
    // corner on both faces (90°). A conic at the circle's rho sin45°/(1 + sin45°) is the
    // circle: (1 − π/4)·9 per mm over 30.
    let b = make_box(k);
    let edge = vertical_edge(k, b, 30.0);
    let with = |profile| FilletSpec { profile, ..FilletSpec::radius(3.0) };
    let s = FRAC_PI_4.sin();
    let round = one(k.fillet_with(b, &[edge], &with(FilletProfile::Conic { rho: s / (1.0 + s) })).unwrap());
    close(volume(k, round), 6000.0 - (1.0 - PI / 4.0) * 9.0 * 30.0, 1e-6);
    // Rho 0.5 is a parabola, which takes 2/3 of the contact triangle (area 9/2): 1/3 of it goes.
    let parabola = one(k.fillet_with(b, &[edge], &with(FilletProfile::Conic { rho: 0.5 })).unwrap());
    close(volume(k, parabola), 6000.0 - 1.5 * 30.0, 1e-6);
    // Curvature (G2), magnitude 0.5: the quintic's section area, integrated here.
    let (p1, v, p2) = (p(0.0, 3.0), p(0.0, 0.0), p(3.0, 0.0));
    let a = |q: Point2<f64>, f: f64| q + (v - q) * f;
    let poles = [p1, a(p1, 0.25), a(p1, 0.5), a(p2, 0.5), a(p2, 0.25), p2];
    let g2 = one(k.fillet_with(b, &[edge], &with(FilletProfile::Curvature { magnitude: 0.5 })).unwrap());
    close(volume(k, g2), 6000.0 - section_area(&poles, &[1.0; 6], v) * 30.0, 1e-5);
    // A concave edge (the inside corner of an L, 10 high) gains the section: the parabola's
    // r 2 adds 4/6 per mm.
    let l = polygon(&[p(0.0, 0.0), p(20.0, 0.0), p(20.0, 5.0), p(5.0, 5.0), p(5.0, 20.0), p(0.0, 20.0)]);
    let lb = one(k.extrude(&top(vec![region(l)]), Extent::Blind(10.0)).unwrap());
    let inner = k
        .edges(lb)
        .unwrap()
        .into_iter()
        .find(|e| (e.mid - Point3::new(5.0, 5.0, 5.0)).norm() < 1e-9)
        .expect("the inside corner")
        .id;
    let before = volume(k, lb);
    let spec = FilletSpec { profile: FilletProfile::Conic { rho: 0.5 }, ..FilletSpec::radius(2.0) };
    let filled = one(k.fillet_with(lb, &[inner], &spec).unwrap());
    close(volume(k, filled), before + 4.0 / 6.0 * 10.0, 1e-6);
    // Not a straight edge between flat faces: refused with the reason.
    let cyl = one(k.extrude(&top(vec![region(circle(p(0.0, 0.0), 10.0))]), Extent::Blind(10.0)).unwrap());
    let rim = edges_at_height(k, cyl, 10.0)[0].id;
    let err = k.fillet_with(cyl, &[rim], &with(FilletProfile::Conic { rho: 0.5 })).unwrap_err();
    assert!(err.to_string().contains("straight edges"), "{err}");
}

pub fn full_round(k: &mut dyn Kernel) {
    // The 10 × 20 × 30 box's top rounded between its x = 0 and x = 10 sides: r 5, taking the
    // two corner slivers (1 − π/4)·25 each along 20: V = 6000 − (2 − π/2)·25·20.
    let b = make_box(k);
    let top_face = face_at(k, b, Vector3::z(), 30.0);
    let side = |k: &dyn Kernel, n: Vector3<f64>| {
        k.faces(b).unwrap().into_iter().find(|f| f.plane.is_some_and(|pl| pl.normal.dot(&n) > 0.999999)).unwrap().id
    };
    let (left, right) = (side(k, -Vector3::x()), side(k, Vector3::x()));
    let r = one(k.full_round(b, left, top_face, right).unwrap());
    close(volume(k, r), 6000.0 - (2.0 - PI / 2.0) * 25.0 * 20.0, 1e-6);
    // The round is one cylinder face of radius 5 (or two halves), with no flat top left.
    let faces = k.faces(r).unwrap();
    assert!(faces.iter().any(|f| f.kind == SurfaceKind::Cylinder && f.radius.is_some_and(|x| (x - 5.0).abs() < 1e-9)));
    assert!(!faces.iter().any(|f| f.plane.is_some_and(|pl| pl.normal.dot(&Vector3::z()) > 0.999)));
    // Sides that aren't parallel: an error saying what it needs.
    let front = side(k, -Vector3::y());
    let err = k.full_round(b, left, top_face, front).unwrap_err();
    assert!(err.to_string().contains("parallel"), "{err}");
}

pub fn shell_options(k: &mut dyn Kernel) {
    // 100 × 60 × 40, bottom open, 4 mm inward: V = 100·60·40 − 92·52·36 = 67 776.
    let b = plate(k, 0.0, 0.0, 100.0, 60.0, 0.0, 40.0);
    let bottom = face_at(k, b, -Vector3::z(), 0.0);
    let spec = ShellSpec { remove: vec![bottom], thickness: 4.0, outward: false, hollow: false };
    let s = one(k.shell_with(b, &spec).unwrap());
    close(volume(k, s), 100.0 * 60.0 * 40.0 - 92.0 * 52.0 * 36.0, 1e-6);
    // Outward: the walls lie outside at exactly 4 mm; OCCT rounds the outer edges and corners
    // (an edge's offset is a quarter cylinder, a corner's a sphere octant): the top and 4 sides
    // · t + the 8 edges above the opening · πt²/4 + the 4 top corners · πt³/6.
    let out = one(k.shell_with(b, &ShellSpec { outward: true, ..spec.clone() }).unwrap());
    let t: f64 = 4.0;
    let faces = 100.0 * 60.0 + 2.0 * 100.0 * 40.0 + 2.0 * 60.0 * 40.0;
    let edges = 2.0 * (100.0 + 60.0) + 4.0 * 40.0;
    close(volume(k, out), faces * t + edges * PI * t * t / 4.0 + 4.0 * PI * t * t * t / 6.0, 1e-3);
    // Hollow inward: a closed box with a 92 × 52 × 32 void.
    let hollow = one(k.shell_with(b, &ShellSpec { remove: vec![], hollow: true, ..spec.clone() }).unwrap());
    close(volume(k, hollow), 240_000.0 - 92.0 * 52.0 * 32.0, 1e-6);
    assert_eq!(k.solid_count(hollow).unwrap(), 1);
    // Walls thicker than half the 60 mm width would cross: an error, not a wrong body.
    let err = k.shell_with(b, &ShellSpec { thickness: 35.0, ..spec.clone() }).unwrap_err();
    assert!(err.to_string().contains("reduce the thickness"), "{err}");
    // Nothing to remove and not hollow: an error.
    assert!(k.shell_with(b, &ShellSpec { remove: vec![], ..spec }).is_err());
}

pub fn shell_around_a_counterbore(k: &mut dyn Kernel) {
    // A 100 × 60 × 40 box with a through hole r_h 2.65 and a counterbore r_c 4.875 × 5 at its
    // middle, shelled 4 mm with the bottom open. OCCT's thick solid can't drop the counterbore
    // floor's offset (the floor, 2.225 wide, is narrower than the wall), so the kernel builds the
    // walls as the body's points within t of its faces. The cavity is the offset box (92·52·36)
    // except round the hole, where it is a solid of revolution: nothing inside r_h + t, and
    // from r_h + t to r_c + t its top is 35 − √(t² − (r − r_c)²) (rounded about the concave
    // corner of the counterbore) instead of 36. With g(s) = ∫₀ˢ√(t² − u²)du,
    // ∫ 2πr√(t² − (r − r_c)²)dr = 2π[−(t² − s²)^{3/2}/3 + r_c·g(s)] over s = r − r_c.
    let b = plate(k, 0.0, 0.0, 100.0, 60.0, 0.0, 40.0);
    let section = [p(0.0, -0.5), p(4.875, -0.5), p(4.875, 5.0), p(2.65, 5.0), p(2.65, 41.0), p(0.0, 41.0)];
    let dir = -Vector3::z_axis();
    let at = Point3::new(50.0, 30.0, 40.0);
    let plane = Plane { origin: at, x_dir: Vector3::x_axis(), normal: nalgebra::Unit::new_normalize(Vector3::x().cross(&dir)) };
    let tool = one(k.revolve_with(&Profile::new(plane, vec![region(polygon(&section))]), &RevolveSpec::solid(Axis { origin: at, dir }, None)).unwrap());
    let d = one(k.boolean(BoolOp::Subtract, b, &[tool]).unwrap());
    let bottom = face_at(k, d, -Vector3::z(), 0.0);
    let s = one(k.shell_with(d, &ShellSpec { remove: vec![bottom], thickness: 4.0, outward: false, hollow: false }).unwrap());
    let (t, rh, rc): (f64, f64, f64) = (4.0, 2.65, 4.875);
    let g = |s: f64| (s * (t * t - s * s).max(0.0).sqrt() + t * t * (s / t).asin()) / 2.0;
    let f = |s: f64| -(t * t - s * s).max(0.0).powf(1.5) / 3.0 + rc * g(s);
    let (r0, r1) = (rh + t, rc + t);
    let ring = 2.0 * PI * (f(t) - f(r0 - rc));
    let cavity = 92.0 * 52.0 * 36.0 + 35.0 * PI * (r1 * r1 - r0 * r0) - ring - 36.0 * PI * r1 * r1;
    let body = 240_000.0 - PI * rc * rc * 5.0 - PI * rh * rh * 35.0;
    close(volume(k, s), body - cavity, 1e-6);
}

pub fn edge_face_radius(k: &mut dyn Kernel) {
    // P3.6: a cylinder's face knows its radius.
    let cyl = one(k.extrude(&top(vec![region(circle(p(0.0, 0.0), 7.5))]), Extent::Blind(10.0)).unwrap());
    let side = k.faces(cyl).unwrap().into_iter().find(|f| f.kind == SurfaceKind::Cylinder).unwrap();
    close(side.radius.unwrap(), 7.5, 1e-9);
}

// ---------------------------------------------------------------------------------------------
// P3.7: offset ellipses, sweeps, lofts, splits

/// The perimeter of the ellipse with semi-axes a, b (Simpson's rule on its speed).
fn ellipse_perimeter(a: f64, b: f64) -> f64 {
    let n = 20_000;
    let h = TAU / n as f64;
    let speed = |t: f64| ((a * t.sin()).powi(2) + (b * t.cos()).powi(2)).sqrt();
    (0..=n)
        .map(|i| {
            let w = if i == 0 || i == n { 1.0 } else if i % 2 == 1 { 4.0 } else { 2.0 };
            w * speed(i as f64 * h)
        })
        .sum::<f64>()
        * h
        / 3.0
}

fn offset_ellipse(a: f64, b: f64, d: f64) -> Curve2 {
    Curve2::OffsetEllipseArc {
        center: p(0.0, 0.0),
        major_radius: a,
        minor_radius: b,
        rotation: 0.0,
        start: 0.0,
        sweep: TAU,
        offset: d,
        source: None,
    }
}

/// P3.7 (X13, PS21.2): the curve 1.25 inside a 60 × 40 ellipse (a = 30, b = 20). For a convex
/// curve of area A and perimeter L, the curve d inside it encloses A − L·d + π·d² (Steiner),
/// so the band between them is L·d − π·d²; extruded 2 mm. An outward offset adds L·d + π·d².
/// The offset curve's length is L − 2πd (inward) exactly (to 1e-7: the B-spline is within 1e-8
/// of OCCT's offset curve). Volumes are to 5e-5 relative: OCCT's fixed-order integration over a
/// face bounded by a B-spline is not exact (the geometry is: the rim's length matches).
pub fn offset_ellipse_profiles(k: &mut dyn Kernel) {
    let (a, b, d) = (30.0, 20.0, 1.25);
    let (area, l) = (PI * a * b, ellipse_perimeter(a, b));
    let outer = Loop {
        curves: vec![Curve2::Ellipse { center: p(0.0, 0.0), major_radius: a, minor_radius: b, rotation: 0.0, source: None }],
    };
    let inner = Loop { curves: vec![offset_ellipse(a, b, -d)] };
    let band = one(k.extrude(&top(vec![Region { outer, holes: vec![inner.clone()], source: None }]), Extent::Blind(2.0)).unwrap());
    let band_v = 2.0 * (l * d - PI * d * d);
    close(volume(k, band), band_v, 5e-5 * 2.0 * area);
    let disc = one(k.extrude(&top(vec![region(inner)]), Extent::Blind(2.0)).unwrap());
    let disc_v = 2.0 * (area - l * d + PI * d * d);
    close(volume(k, disc), disc_v, 5e-5 * disc_v);
    // The bottom rim (the offset curve is two half edges, see `loop_wire`).
    let rim: f64 = k.edges(disc).unwrap().into_iter().filter(|e| e.start.z.abs() < 1e-9 && e.end.z.abs() < 1e-9).map(|e| e.length).sum();
    close(rim, l - TAU * d, 1e-7);
    let out = one(k.extrude(&top(vec![region(Loop { curves: vec![offset_ellipse(a, b, 0.5)] })]), Extent::Blind(1.0)).unwrap());
    let out_v = area + l * 0.5 + PI * 0.25;
    close(volume(k, out), out_v, 5e-5 * out_v);
    // An inward offset past the smallest radius of curvature (b²/a = 13.3) is refused.
    let bad = Loop { curves: vec![offset_ellipse(a, b, -14.0)] };
    assert!(k.extrude(&top(vec![region(bad)]), Extent::Blind(1.0)).is_err());
}

/// The L path of the P3.7 acceptance on Top: 100 along X, a quarter turn of radius 50, 80 along
/// Y.
fn l_path() -> Vec<PathCurve> {
    let plane = Plane::top();
    vec![
        PathCurve::Sketch { plane, curve: line(p(0.0, 0.0), p(100.0, 0.0)) },
        PathCurve::Sketch { plane, curve: arc(p(100.0, 50.0), 50.0, -PI / 2.0, PI / 2.0) },
        PathCurve::Sketch { plane, curve: line(p(150.0, 50.0), p(150.0, 130.0)) },
    ]
}

/// A Ø10 circle on the plane through `at` square to X.
fn circle_across_x(at: Point3<f64>, r: f64) -> Profile {
    Profile::new(
        Plane { origin: at, x_dir: Vector3::y_axis(), normal: Vector3::x_axis() },
        vec![Region { outer: circle(p(0.0, 0.0), r), holes: vec![], source: Some(7) }],
    )
}

/// P3.7 (PS19): a Ø10 circle swept along the L path (straight 100, a bend of radius 50,
/// straight 80): by Pappus V = A·(length of the centre line) = π·25·(100 + 80 + π/2·50). From
/// the middle of the first straight it sweeps both ways (PS19.5): the same body. Thin 1 mm
/// inside: π(25 − 16)·length. Along a slot's closed top outline (model edges, two straights
/// of 60 and two R10 ends) from the middle of a straight: π·r²·(120 + 20π).
pub fn sweep_along_paths(k: &mut dyn Kernel) {
    let length = 100.0 + 80.0 + PI / 2.0 * 50.0;
    let spec = SweepSpec { body: BodyKind::Solid, path: l_path(), control: SweepControl::None, faces: vec![] };
    let s = one(k.sweep_with(&circle_across_x(Point3::origin(), 5.0), &spec).unwrap());
    close(volume(k, s), PI * 25.0 * length, 1e-6);
    let faces = k.faces(s).unwrap();
    assert_eq!(faces.len(), 5, "three sides and two caps");
    let r = k.sweep_with(&circle_across_x(Point3::origin(), 5.0), &spec).unwrap();
    let starts = r.history.generated.iter().filter(|(_, o)| matches!(o, Origin::StartCap { region: 7 })).count();
    let sides = r.history.generated.iter().filter(|(_, o)| matches!(o, Origin::ProfileCurve { region: 7, .. })).count();
    assert_eq!((starts, sides), (1, 3));
    // Both ways from the middle of the first straight.
    let mid = one(k.sweep_with(&circle_across_x(Point3::new(50.0, 0.0, 0.0), 5.0), &spec).unwrap());
    close(volume(k, mid), PI * 25.0 * length, 1e-6);
    assert_eq!(k.solid_count(mid).unwrap(), 1);
    // Thin, 1 mm inside the circle.
    let thin = SweepSpec { body: BodyKind::Thin { left: 1.0, right: 0.0 }, ..spec.clone() };
    let t = one(k.sweep_with(&circle_across_x(Point3::origin(), 5.0), &thin).unwrap());
    close(volume(k, t), PI * (25.0 - 16.0) * length, 1e-5);
    // Keep profile orientation: along a straight path the same prism.
    let straight = SweepSpec {
        path: vec![PathCurve::Sketch { plane: Plane::top(), curve: line(p(0.0, 0.0), p(40.0, 0.0)) }],
        control: SweepControl::KeepOrientation,
        ..spec.clone()
    };
    let st = one(k.sweep_with(&circle_across_x(Point3::origin(), 5.0), &straight).unwrap());
    close(volume(k, st), PI * 25.0 * 40.0, 1e-6);
    // A slot's closed top outline as model edges, the profile at the middle of a straight.
    let slot = Loop {
        curves: vec![
            line(p(0.0, 0.0), p(60.0, 0.0)),
            arc(p(60.0, 10.0), 10.0, -PI / 2.0, PI),
            line(p(60.0, 20.0), p(0.0, 20.0)),
            arc(p(0.0, 10.0), 10.0, PI / 2.0, PI),
        ],
    };
    let body = one(k.extrude(&top(vec![region(slot)]), Extent::Blind(5.0)).unwrap());
    let top_edges: Vec<PathCurve> = k
        .edges(body)
        .unwrap()
        .into_iter()
        .filter(|e| (e.start.z - 5.0).abs() < 1e-9 && (e.end.z - 5.0).abs() < 1e-9)
        .map(|e| PathCurve::Edge { body, edge: e.id })
        .collect();
    assert_eq!(top_edges.len(), 4);
    let ring = SweepSpec { path: top_edges, ..spec };
    let bead = one(k.sweep_with(&circle_across_x(Point3::new(30.0, 0.0, 5.0), 2.0), &ring).unwrap());
    close(volume(k, bead), PI * 4.0 * (120.0 + 20.0 * PI), 1e-5);
    // The bead joined to the slot: its lower half inside the body adds the outer part of the
    // disc's area around the rim (half the disc's area off the walls' plane minus the quarter
    // inside the body... measured, then only checked to add volume).
    let joined = one(k.boolean(BoolOp::Union, body, &[bead]).unwrap());
    assert_eq!(k.solid_count(joined).unwrap(), 1);
    assert!(volume(k, joined) > volume(k, body));
}

/// A Ø6 circle swept around a closed R40 circle is a torus (the trimmer document's Sweep 1):
/// V = 2π²·R·r² = 720π². Its side faces all lie on the one toroidal surface, and merging them
/// leaves a face with no edges, which OCCT can't mesh (and in OCCT 7.8.1 used to crash in
/// ShapeUpgrade_UnifySameDomain): the body keeps its seams and tessellates. The path as one
/// circle (the profile at its start) or as two half arcs gives the same body.
pub fn sweep_around_a_circle(k: &mut dyn Kernel) {
    let plane = Plane::top();
    let across_y = Profile::new(
        Plane { origin: Point3::origin(), x_dir: Vector3::x_axis(), normal: Vector3::y_axis() },
        vec![Region { outer: circle(p(0.0, 0.0), 3.0), holes: vec![], source: Some(7) }],
    );
    let circle_path = vec![PathCurve::Sketch { plane, curve: Curve2::Circle { center: p(-40.0, 0.0), radius: 40.0, source: None } }];
    let arcs_path = vec![
        PathCurve::Sketch { plane, curve: arc(p(0.0, 40.0), 40.0, -PI / 2.0, PI) },
        PathCurve::Sketch { plane, curve: arc(p(0.0, 40.0), 40.0, PI / 2.0, PI) },
    ];
    for (profile, path) in [(across_y, circle_path), (circle_across_x(Point3::origin(), 3.0), arcs_path)] {
        let spec = SweepSpec { body: BodyKind::Solid, path, control: SweepControl::None, faces: vec![] };
        let torus = one(k.sweep_with(&profile, &spec).unwrap());
        close(volume(k, torus), 2.0 * PI * PI * 40.0 * 9.0, 1e-5);
        let mesh = k.tessellate(torus, Tessellation::default()).unwrap();
        assert!(!mesh.indices.is_empty());
    }
}

/// P3.11 (PS20.4): Normal direction and Tangent direction, a picked vector in place of the
/// profile's normal. Along +Z (the profiles' normal) they are Normal to profile and Tangent to
/// profile: the 10 × 10 squares 20 apart give the prism, 2000; the r 10 → r 5 circles the same
/// body as Tangent to profile. The vector's sign doesn't matter (it is turned along the loft).
/// Oblique, (1, 0, 1) at the start between the equal squares: every point of the section leaves
/// with the same derivative, so each horizontal slice is the square moved sideways and the
/// volume stays 2000 (Cavalieri), but the body leans out past x = 5. A zero vector is an error.
pub fn loft_direction_conditions(k: &mut dyn Kernel) {
    let squares = || vec![LoftSection::Profile(square_at(0.0, 5.0)), LoftSection::Profile(square_at(20.0, 5.0))];
    let circles = || vec![LoftSection::Profile(circle_at(0.0, 0.0, 10.0)), LoftSection::Profile(circle_at(20.0, 0.0, 5.0))];
    let end = |condition| LoftEnd { condition, magnitude: 1.0 };
    let up = end(LoftCondition::NormalDirection([0.0, 0.0, 1.0]));
    let down = end(LoftCondition::NormalDirection([0.0, 0.0, -1.0]));
    let prism = loft(k, squares(), up, down);
    close(volume(k, prism), 2000.0, 1e-6);
    let flare = loft(k, circles(), end(LoftCondition::TangentDirection([0.0, 0.0, -3.0])), end(LoftCondition::TangentDirection([0.0, 0.0, 1.0])));
    let tangent = loft(k, circles(), end(LoftCondition::TangentToProfile), end(LoftCondition::TangentToProfile));
    close(volume(k, flare), volume(k, tangent), 1e-6);
    let oblique = loft(k, squares(), end(LoftCondition::NormalDirection([1.0, 0.0, 1.0])), end(LoftCondition::NormalToProfile));
    close(volume(k, oblique), 2000.0, 1e-4);
    let b = k.bounding_box(oblique).unwrap();
    assert!(b.max.x > 5.5 && b.min.x > -5.0 - 1e-6, "{b:?}");
    let err = k
        .loft_with(&LoftSpec { body: BodyKind::Solid, sections: squares(), start: end(LoftCondition::NormalDirection([0.0; 3])), end: LoftEnd::default(), source: 9 })
        .unwrap_err();
    assert!(err.to_string().contains("non-zero"), "{err}");
}

fn square_at(z: f64, half: f64) -> Profile {
    at_height(z, vec![Region { outer: rect(-half, -half, half, half), holes: vec![], source: Some(3) }])
}

fn circle_at(z: f64, x: f64, r: f64) -> Profile {
    at_height(z, vec![Region { outer: circle(p(x, 0.0), r), holes: vec![], source: Some(4) }])
}

fn loft(k: &mut dyn Kernel, sections: Vec<LoftSection>, start: LoftEnd, end: LoftEnd) -> BodyId {
    one(k.loft_with(&LoftSpec { body: BodyKind::Solid, sections, start, end, source: 9 }).unwrap())
}

/// P3.7 (PS20): a loft between two equal 10 × 10 squares 20 apart is the prism, 2000 mm³;
/// between circles r 10 and r 5 20 apart the frustum, πh(R² + Rr + r²)/3 = 3500π/3; from a
/// circle r 10 to a point 15 above its centre the cone, π·100·15/3 = 500π. Normal to profile
/// at both ends with magnitude 1 between the squares is still the prism (the derivative is the
/// straight line's). Two squares sharing a side are one profile (their outer contour); two
/// apart are not (PS20.5).
pub fn loft_sections_and_conditions(k: &mut dyn Kernel) {
    let none = LoftEnd::default();
    let prism = loft(k, vec![LoftSection::Profile(square_at(0.0, 5.0)), LoftSection::Profile(square_at(20.0, 5.0))], none, none);
    close(volume(k, prism), 2000.0, 1e-6);
    let frustum = loft(k, vec![LoftSection::Profile(circle_at(0.0, 0.0, 10.0)), LoftSection::Profile(circle_at(20.0, 0.0, 5.0))], none, none);
    close(volume(k, frustum), 3500.0 * PI / 3.0, 1e-6);
    let r = k
        .loft_with(&LoftSpec {
            body: BodyKind::Solid,
            sections: vec![LoftSection::Profile(circle_at(0.0, 0.0, 10.0)), LoftSection::Profile(circle_at(20.0, 0.0, 5.0))],
            start: none,
            end: none,
            source: 9,
        })
        .unwrap();
    let names: Vec<Origin> = r.history.generated.iter().map(|(_, o)| *o).collect();
    assert!(names.contains(&Origin::StartCap { region: 9 }) && names.contains(&Origin::EndCap { region: 9 }), "{names:?}");
    let cone = loft(k, vec![LoftSection::Profile(circle_at(0.0, 0.0, 10.0)), LoftSection::Point(Point3::new(0.0, 0.0, 15.0))], none, none);
    close(volume(k, cone), 500.0 * PI, 1e-6);
    let normal = LoftEnd { condition: LoftCondition::NormalToProfile, magnitude: 1.0 };
    let conditioned = loft(k, vec![LoftSection::Profile(square_at(0.0, 5.0)), LoftSection::Profile(square_at(20.0, 5.0))], normal, normal);
    close(volume(k, conditioned), 2000.0, 1e-6);
    assert_eq!(k.solid_count(conditioned).unwrap(), 1);
    // Normal to profile between two circles r 10 and r 5: a smooth frustum-like body that
    // leaves both ends square to them, so it bulges beyond the frustum near the big end and
    // pinches near the small one; its volume lies between the r 5 and r 10 cylinders'.
    let bulge = loft(k, vec![LoftSection::Profile(circle_at(0.0, 0.0, 10.0)), LoftSection::Profile(circle_at(20.0, 0.0, 5.0))], normal, normal);
    let v = volume(k, bulge);
    assert!(v > PI * 25.0 * 20.0 && v < PI * 100.0 * 20.0, "{v}");
    // Three sections with conditions, as the Funnel's.
    let three = loft(
        k,
        vec![
            LoftSection::Profile(circle_at(0.0, 0.0, 10.0)),
            LoftSection::Profile(circle_at(-10.0, 2.0, 5.0)),
            LoftSection::Profile(circle_at(-20.0, 3.0, 1.0)),
        ],
        LoftEnd { condition: LoftCondition::NormalToProfile, magnitude: 0.5 },
        normal,
    );
    assert!(k.mass_properties(three).unwrap().volume > 0.0);
    // Two squares sharing a side loft as their 20 × 10 outline.
    let pair = at_height(0.0, vec![
        Region { outer: rect(-10.0, -5.0, 0.0, 5.0), holes: vec![], source: Some(1) },
        Region { outer: rect(0.0, -5.0, 10.0, 5.0), holes: vec![], source: Some(2) },
    ]);
    let joined = loft(k, vec![LoftSection::Profile(pair), LoftSection::Profile(at_height(10.0, vec![region(rect(-10.0, -5.0, 10.0, 5.0))]))], none, none);
    close(volume(k, joined), 2000.0, 1e-6);
    let apart = at_height(0.0, vec![region(rect(-10.0, -5.0, -6.0, 5.0)), region(rect(6.0, -5.0, 10.0, 5.0))]);
    let err = k
        .loft_with(&LoftSpec { body: BodyKind::Solid, sections: vec![LoftSection::Profile(apart), LoftSection::Profile(square_at(10.0, 5.0))], start: none, end: none, source: 9 })
        .unwrap_err();
    assert!(err.to_string().contains("one closed contour"), "{err}");
    // PS20.3: a square (4 edges) to a circle (1) with Normal to profile, magnitude 1 (the
    // derivative is the straight chord, so each point runs straight from its square point to its
    // circle point): the circle's quarters meet the square's sides, every section lies between
    // the inscribed r 10 circle and the 20 × 20 square, so 100π·20 < V < 400·20, and the body is
    // centred on the axis.
    let sq_circle = loft(k, vec![LoftSection::Profile(square_at(0.0, 10.0)), LoftSection::Profile(circle_at(20.0, 0.0, 10.0))], normal, normal);
    assert_eq!(k.solid_count(sq_circle).unwrap(), 1);
    let m = k.mass_properties(sq_circle).unwrap();
    assert!(m.volume > 2000.0 * PI && m.volume < 8000.0, "{}", m.volume);
    assert!(m.center_of_mass.x.abs() < 1e-3 && m.center_of_mass.y.abs() < 1e-3, "{:?}", m.center_of_mass);
    // PS20.6: a Thin loft between two r 10 circles 20 apart is a tube 1 thick: its side sheet
    // thickened by 1 on one side, π(10² − 9²)·20 = 380π inside or π(11² − 10²)·20 = 420π outside.
    let tube = one(
        k.loft_with(&LoftSpec {
            body: BodyKind::Thin { left: 1.0, right: 0.0 },
            sections: vec![LoftSection::Profile(circle_at(0.0, 0.0, 10.0)), LoftSection::Profile(circle_at(20.0, 0.0, 10.0))],
            start: none,
            end: none,
            source: 9,
        })
        .unwrap(),
    );
    let v = volume(k, tube);
    assert!((v - 380.0 * PI).abs() < 1e-3 * v || (v - 420.0 * PI).abs() < 1e-3 * v, "{v}");
}

/// P3.7 (PS18.5): the 10 × 20 × 30 box split by the plane z = 10 is two solids of 2000 and
/// 4000 mm³ sharing the cut; the cut faces are generated, the box's faces modified. Split by a
/// face of another body (a plate's top at z = 25) the same way; a plane that misses the box is
/// an error.
pub fn split_by_plane_and_face(k: &mut dyn Kernel) {
    let b = make_box(k);
    let plane = Plane { origin: Point3::new(0.0, 0.0, 10.0), ..Plane::top() };
    let r = k.split(b, &SplitTool::Plane(plane), 42).unwrap();
    let s = one(r.clone());
    assert_eq!(k.solid_count(s).unwrap(), 2);
    assert!(r.history.generated.iter().any(|(_, o)| *o == Origin::StartCap { region: 42 }));
    let pieces = k.split_solids(s).unwrap();
    let mut vols: Vec<f64> = pieces.iter().map(|p| volume(k, p.bodies[0])).collect();
    vols.sort_by(f64::total_cmp);
    close(vols[0], 2000.0, 1e-6);
    close(vols[1], 4000.0, 1e-6);
    let slab = plate(k, -50.0, -50.0, 50.0, 50.0, 20.0, 25.0);
    let face = face_at(k, slab, Vector3::z(), 25.0);
    let by_face = one(k.split(b, &SplitTool::Face { body: slab, face }, 43).unwrap());
    let pieces = k.split_solids(by_face).unwrap();
    let mut vols: Vec<f64> = pieces.iter().map(|p| volume(k, p.bodies[0])).collect();
    vols.sort_by(f64::total_cmp);
    close(vols[0], 1000.0, 1e-6);
    close(vols[1], 5000.0, 1e-6);
    let miss = Plane { origin: Point3::new(0.0, 0.0, 100.0), ..Plane::top() };
    assert!(k.split(b, &SplitTool::Plane(miss), 44).is_err());
}


// ---------------------------------------------------------------------------------------------
// P3.8: mirror and pattern transforms, face tools, classification

/// P3.8 (PS26): the 10 × 20 × 30 box reflected in the plane x = 0 is a valid box of 6000 mm³
/// at x −10..0 (CoM x −5) whose faces point out of it; joined to the original it is one
/// 20 × 20 × 30 box (12 000). Turned a quarter about Z it lies at x −20..0 (CoM (−10, 5, 15)).
/// A motion that scales is refused.
pub fn mirror_and_motions(k: &mut dyn Kernel) {
    let b = make_box(k);
    let m = Motion::reflection(Point3::origin(), Vector3::x_axis());
    assert!(m.is_reflection());
    let r = k.transform_motion(b, &m).unwrap();
    assert_eq!(r.history.modified.len(), 6, "every face continues its original");
    let mirrored = one(r);
    let mp = k.mass_properties(mirrored).unwrap();
    close(mp.volume, 6000.0, 1e-6);
    close(mp.center_of_mass.x, -5.0, 1e-9);
    close(mp.center_of_mass.y, 10.0, 1e-9);
    // The face at x = −10 points to −x (out of the copy).
    let far = k
        .faces(mirrored)
        .unwrap()
        .into_iter()
        .find(|f| (f.center.x + 10.0).abs() < 1e-9)
        .expect("the far face");
    assert!(far.plane.unwrap().normal.x < -0.999999);
    assert_eq!(k.classify(mirrored, Point3::new(-5.0, 10.0, 15.0), 1e-7).unwrap(), PointClass::Inside);
    let joined = one(k.boolean(BoolOp::Union, b, &[mirrored]).unwrap());
    assert_eq!(k.solid_count(joined).unwrap(), 1);
    close(volume(k, joined), 12000.0, 1e-6);
    // The faces the union merged across the mirror plane mesh.
    assert_eq!(k.faces(joined).unwrap().len(), 6);
    k.tessellate(joined, Tessellation { deflection: 0.05, angle: 0.1 }).unwrap();
    // A half cylinder (r 10, y ≥ 0, 5 high) and its mirror image across y = 0 join into the
    // cylinder π·100·5, which meshes (OCCT's merge of the two curved faces turns the result
    // inside out, so that union keeps its seams), both ways round.
    let half = region(Loop { curves: vec![arc(p(0.0, 0.0), 10.0, 0.0, PI), line(p(-10.0, 0.0), p(10.0, 0.0))] });
    let hb = one(k.extrude(&top(vec![half]), Extent::Blind(5.0)).unwrap());
    let hm = one(k.transform_motion(hb, &Motion::reflection(Point3::origin(), Vector3::y_axis())).unwrap());
    let cyl = one(k.boolean(BoolOp::Union, hb, &[hm]).unwrap());
    close(volume(k, cyl), PI * 100.0 * 5.0, 1e-6);
    k.tessellate(cyl, Tessellation { deflection: 0.05, angle: 0.1 }).unwrap();
    let other_way = one(k.boolean(BoolOp::Union, hm, &[hb]).unwrap());
    close(volume(k, other_way), PI * 100.0 * 5.0, 1e-6);
    let quarter = Motion::rotation(&Axis { origin: Point3::origin(), dir: Vector3::z_axis() }, PI / 2.0);
    let turned = one(k.transform_motion(b, &quarter).unwrap());
    let tp = k.mass_properties(turned).unwrap();
    close(tp.center_of_mass.x, -10.0, 1e-9);
    close(tp.center_of_mass.y, 5.0, 1e-9);
    close(tp.center_of_mass.z, 15.0, 1e-9);
    // Motions compose and invert.
    let both = quarter.then(&m);
    let back = both.then(&both.inverse());
    assert!((back.linear - nalgebra::Matrix3::identity()).norm() < 1e-12 && back.translation.norm() < 1e-12);
    let scale = Motion { linear: nalgebra::Matrix3::identity() * 2.0, translation: Vector3::zeros() };
    assert!(k.transform_motion(b, &scale).is_err());
}

/// P3G.4 (DV3.3): a Derived feature's placement is the rigid motion taking the base frame (a
/// source mate connector at the box's top-face centre (5, 10, 30), Part Studio axes) onto a
/// location frame at (0, 0, 100) turned a quarter about Z. The copy's every face continues
/// its original (the copy's faces are renamed after them, as a pattern instance's), the volume
/// is kept, and the centroid (5, 10, 15) goes to (0, 0, 85) (the base point onto the location,
/// the offset (0, 0, −15) along Z unchanged by the turn).
pub fn derived_frame_motion(k: &mut dyn Kernel) {
    let b = make_box(k);
    let base = Motion::translation(Vector3::new(5.0, 10.0, 30.0));
    let turn = Motion::rotation(&Axis { origin: Point3::origin(), dir: Vector3::z_axis() }, PI / 2.0);
    let location = turn.then(&Motion::translation(Vector3::new(0.0, 0.0, 100.0)));
    let m = base.inverse().then(&location);
    let r = k.transform_motion(b, &m).unwrap();
    assert_eq!(r.history.modified.len(), 6, "every face continues its original");
    assert!(r.history.generated.is_empty());
    let copy = one(r);
    let mp = k.mass_properties(copy).unwrap();
    close(mp.volume, 6000.0, 1e-6);
    close(mp.center_of_mass.x, 0.0, 1e-9);
    close(mp.center_of_mass.y, 0.0, 1e-9);
    close(mp.center_of_mass.z, 85.0, 1e-9);
    // The original is untouched.
    close(k.mass_properties(b).unwrap().center_of_mass.z, 15.0, 1e-9);
}

/// P3.8 (PS22.2, PS26.3): the face tool of a pocket's five faces (a 20 × 10 × 5 pocket in a
/// 100 × 100 × 20 plate) is the pocket's volume, 1000, with one cap (generated from `source`),
/// outside the plate (a pocket). Mirrored across x = 50 and cut, it removes another 1000. A
/// boss's faces give the boss (inside the part it was added to). A drilled hole's cylinder and
/// 118° cone give π r² h + π r² (r / tan 59°) / 3. One flat face alone encloses nothing.
pub fn face_tools(k: &mut dyn Kernel) {
    let plate_body = plate(k, 0.0, 0.0, 100.0, 100.0, 0.0, 20.0);
    let cutter = plate(k, 10.0, 10.0, 30.0, 20.0, 15.0, 21.0);
    let pocketed = one(k.boolean(BoolOp::Subtract, plate_body, &[cutter]).unwrap());
    let inside = |c: &Point3<f64>| (10.0..=30.0).contains(&c.x) && (10.0..=20.0).contains(&c.y) && c.z < 19.999;
    let faces: Vec<FaceId> = k.faces(pocketed).unwrap().into_iter().filter(|f| inside(&f.center)).map(|f| f.id).collect();
    assert_eq!(faces.len(), 5);
    let r = k.face_tool(pocketed, &faces, 77).unwrap();
    assert_eq!(r.history.generated.iter().filter(|(_, o)| *o == Origin::StartCap { region: 77 }).count(), 1);
    assert_eq!(r.history.modified.len(), 5);
    let tool = one(r);
    let tp = k.mass_properties(tool).unwrap();
    close(tp.volume, 1000.0, 1e-6);
    close(tp.center_of_mass.z, 17.5, 1e-9);
    assert_eq!(k.classify(pocketed, tp.center_of_mass, 1e-7).unwrap(), PointClass::Outside);
    let mirrored = one(k.transform_motion(tool, &Motion::reflection(Point3::new(50.0, 0.0, 0.0), Vector3::x_axis())).unwrap());
    close(k.mass_properties(mirrored).unwrap().center_of_mass.x, 80.0, 1e-9);
    let two = one(k.boolean(BoolOp::Subtract, pocketed, &[mirrored]).unwrap());
    close(volume(k, two), 200_000.0 - 2000.0, 1e-6);
    // A boss.
    let base = plate(k, 0.0, 0.0, 100.0, 100.0, 0.0, 20.0);
    let lump = plate(k, 40.0, 40.0, 60.0, 50.0, 20.0, 26.0);
    let bossed = one(k.boolean(BoolOp::Union, base, &[lump]).unwrap());
    let boss_faces: Vec<FaceId> =
        k.faces(bossed).unwrap().into_iter().filter(|f| f.center.z > 20.0 + 1e-9).map(|f| f.id).collect();
    assert_eq!(boss_faces.len(), 5);
    let boss = one(k.face_tool(bossed, &boss_faces, 78).unwrap());
    let bp = k.mass_properties(boss).unwrap();
    close(bp.volume, 20.0 * 10.0 * 6.0, 1e-6);
    assert_eq!(k.classify(bossed, bp.center_of_mass, 1e-7).unwrap(), PointClass::Inside);
    // A drilled hole: Ø8 × 8 with a 118° point, from the top of a plate.
    let (r0, h) = (4.0, 8.0);
    let tip = r0 / (59f64).to_radians().tan();
    let hole_plane = Plane { origin: Point3::new(50.0, 50.0, 20.0), x_dir: Vector3::x_axis(), normal: -Vector3::y_axis() };
    let section = Profile::new(hole_plane, vec![region(polygon(&[p(0.0, 1.0), p(r0, 1.0), p(r0, -h), p(0.0, -h - tip)]))]);
    let axis = Axis { origin: Point3::new(50.0, 50.0, 20.0), dir: Vector3::z_axis() };
    let drill = one(k.revolve(&section, axis, TAU).unwrap());
    let block = plate(k, 0.0, 0.0, 100.0, 100.0, 0.0, 20.0);
    let drilled = one(k.boolean(BoolOp::Subtract, block, &[drill]).unwrap());
    let hole_faces: Vec<FaceId> = k
        .faces(drilled)
        .unwrap()
        .into_iter()
        .filter(|f| matches!(f.kind, SurfaceKind::Cylinder | SurfaceKind::Cone))
        .map(|f| f.id)
        .collect();
    assert_eq!(hole_faces.len(), 2);
    let hole = one(k.face_tool(drilled, &hole_faces, 79).unwrap());
    close(volume(k, hole), PI * r0 * r0 * h + PI * r0 * r0 * tip / 3.0, 1e-6);
    // One flat face alone.
    let top_face = face_at(k, drilled, Vector3::z(), 20.0);
    assert!(k.face_tool(drilled, &[top_face], 80).is_err());
}

/// P3.8: points inside, outside and on the 10 × 20 × 30 box.
pub fn classify_points(k: &mut dyn Kernel) {
    let b = make_box(k);
    assert_eq!(k.classify(b, Point3::new(5.0, 10.0, 15.0), 1e-7).unwrap(), PointClass::Inside);
    assert_eq!(k.classify(b, Point3::new(15.0, 10.0, 15.0), 1e-7).unwrap(), PointClass::Outside);
    assert_eq!(k.classify(b, Point3::new(10.0, 10.0, 15.0), 1e-7).unwrap(), PointClass::OnBoundary);
}
/// P3.8 (a Face split): the 10 × 20 × 30 box's top face split by the plane x = 5 gives two
/// 5 × 20 faces, both continuing the top face; the box keeps its volume (6000) and stays one
/// solid of 7 faces. The two ends' edges are split where the cut meets them, the faces themselves
/// not. A plane that misses the face is an error.
pub fn split_faces_by_plane(k: &mut dyn Kernel) {
    let b = make_box(k);
    let top_face = face_at(k, b, Vector3::z(), 30.0);
    let plane = Plane { origin: Point3::new(5.0, 0.0, 0.0), x_dir: Vector3::y_axis(), normal: Vector3::x_axis() };
    let r = k.split_faces(b, &[top_face], &SplitTool::Plane(plane)).unwrap();
    assert!(r.history.generated.is_empty());
    let from_top: Vec<FaceId> =
        r.history.modified.iter().filter(|(_, from)| from.body == b && from.face == top_face).map(|(f, _)| *f).collect();
    assert_eq!(from_top.len(), 2);
    let s = one(r);
    assert_eq!(k.solid_count(s).unwrap(), 1);
    close(volume(k, s), 6000.0, 1e-6);
    let faces = k.faces(s).unwrap();
    assert_eq!(faces.len(), 7);
    let mut halves: Vec<f64> = faces.iter().filter(|f| from_top.contains(&f.id)).map(|f| f.center.x).collect();
    halves.sort_by(f64::total_cmp);
    close(halves[0], 2.5, 1e-9);
    close(halves[1], 7.5, 1e-9);
    for f in faces.iter().filter(|f| from_top.contains(&f.id)) {
        close(f.area, 100.0, 1e-9);
    }
    let miss = Plane { origin: Point3::new(50.0, 0.0, 0.0), ..plane };
    assert!(k.split_faces(b, &[top_face], &SplitTool::Plane(miss)).is_err());
}


// ---------------------------------------------------------------------------------------------
// Drawing-view projection (P3C.2)

/// A 20 mm cube with a Ø10 hole through it along Z, seen from the front and from above:
/// the front view's hidden lines are the hole's two sides; every projected edge knows the body
/// edge (or, for the hole's sides, the cylinder face) it came from; a fillet shows smooth
/// (tangent) edges; two bodies hide each other.
fn project_view(k: &mut dyn Kernel) {
    let profile = top(vec![Region {
        outer: rect(0.0, 0.0, 20.0, 20.0),
        holes: vec![circle(p(10.0, 10.0), 5.0)],
        source: None,
    }]);
    let b = one(k.extrude(&profile, Extent::Blind(20.0)).unwrap());
    let edges = k.edges(b).unwrap();
    let faces = k.faces(b).unwrap();
    let opts = ProjectOptions::default();
    // Front: looking along +Y, 2D (x, y) = model (x, z).
    let front = k.project(&[b], &ViewFrame::new(Vector3::y(), Vector3::x()), &opts).unwrap();
    let hidden: Vec<&ProjEdge> = front.edges.iter().filter(|e| e.visibility == ProjVisibility::Hidden).collect();
    assert_eq!(hidden.len(), 2, "the hole's two sides: {hidden:?}");
    let mut xs: Vec<f64> = hidden.iter().map(|e| e.points[0].x).collect();
    xs.sort_by(f64::total_cmp);
    close(xs[0], 5.0, 1e-6);
    close(xs[1], 15.0, 1e-6);
    for e in &hidden {
        close(e.length(), 20.0, 1e-6);
        assert_eq!(e.class, ProjClass::Outline);
        let f = e.source.and_then(|s| s.face).expect("the hole's face");
        assert_eq!(faces[f.0 as usize].kind, SurfaceKind::Cylinder);
    }
    let visible: Vec<&ProjEdge> = front.edges.iter().filter(|e| e.visibility == ProjVisibility::Visible).collect();
    let outline: f64 = visible.iter().map(|e| e.length()).sum();
    close(outline, 80.0, 1e-6);
    let (lo, hi) = front.bounds().unwrap();
    close(hi.x - lo.x, 20.0, 1e-9);
    close(hi.y - lo.y, 20.0, 1e-9);
    // Horizontal visible edges come from the face nearest the eye (y = 0).
    for e in &visible {
        let id = e.source.and_then(|s| s.edge).expect("a source edge");
        let info = edges.iter().find(|i| i.id == id).unwrap();
        if (info.start.z - info.end.z).abs() < 1e-9 {
            assert!(info.mid.y.abs() < 1e-6, "visible edge from y = {}", info.mid.y);
        }
    }
    // Top: the hole is a visible circle (the bottom circle right behind it is dropped).
    let above = k.project(&[b], &ViewFrame::new(-Vector3::z(), Vector3::x()), &opts).unwrap();
    assert!(above.edges.iter().all(|e| e.visibility == ProjVisibility::Visible));
    let circles: Vec<&ProjEdge> = above
        .edges
        .iter()
        .filter(|e| matches!(e.curve, ProjCurve::Arc { .. }))
        .collect();
    let arc_len: f64 = circles.iter().map(|e| e.length()).sum();
    close(arc_len, TAU * 5.0, 0.05);
    for c in &circles {
        let ProjCurve::Arc { center, radius, .. } = c.curve else { unreachable!() };
        close(radius, 5.0, 1e-6);
        close(center.x, 10.0, 1e-6);
        close(center.y, 10.0, 1e-6);
        let id = c.source.and_then(|s| s.edge).expect("the circle's edge");
        let info = edges.iter().find(|i| i.id == id).unwrap();
        close(info.mid.z, 20.0, 1e-6);
    }
    // Hidden lines off.
    let no_hidden = k
        .project(&[b], &ViewFrame::new(Vector3::y(), Vector3::x()), &ProjectOptions { hidden: false, ..opts })
        .unwrap();
    assert!(no_hidden.edges.iter().all(|e| e.visibility == ProjVisibility::Visible));
    // A filleted edge: its boundaries are smooth edges in the front view.
    let bx = plate(k, 0.0, 0.0, 20.0, 20.0, 0.0, 20.0);
    let top_front = k
        .edges(bx)
        .unwrap()
        .into_iter()
        .find(|e| e.mid.y.abs() < 1e-9 && (e.mid.z - 20.0).abs() < 1e-9)
        .unwrap()
        .id;
    let filleted = one(k.fillet(bx, &[top_front], 4.0).unwrap());
    let f = k.project(&[filleted], &ViewFrame::new(Vector3::y(), Vector3::x()), &opts).unwrap();
    assert!(f.edges.iter().any(|e| e.class == ProjClass::Smooth && e.visibility == ProjVisibility::Visible));
    // Two bodies hide each other: a small block behind the cube is hidden in the front view.
    let behind = plate(k, 5.0, 30.0, 15.0, 40.0, 5.0, 15.0);
    let both = k.project(&[bx, behind], &ViewFrame::new(Vector3::y(), Vector3::x()), &opts).unwrap();
    assert!(both
        .edges
        .iter()
        .any(|e| e.visibility == ProjVisibility::Hidden && e.source.is_some_and(|s| s.body == 1)));
    assert!(!both
        .edges
        .iter()
        .any(|e| e.visibility == ProjVisibility::Visible && e.source.is_some_and(|s| s.body == 1)));
}

/// Section views (P3C.8) cut with existing operations: a box on the viewer's side subtracted
/// from the part leaves the cut faces on the cutting plane. A tube Ø40/Ø20 × 30 cut through its
/// axis has two 10 × 30 cut faces: 600 mm².
fn section_cut(k: &mut dyn Kernel) {
    let tube = top(vec![Region { outer: circle(p(0.0, 0.0), 20.0), holes: vec![circle(p(0.0, 0.0), 10.0)], source: None }]);
    let t = one(k.extrude(&tube, Extent::Blind(30.0)).unwrap());
    // Everything at x > 0 (the eye's side of the plane x = 0, seen from +X).
    let half_space = plate(k, 0.0, -50.0, 50.0, 50.0, -10.0, 40.0);
    let cut = one(k.boolean(BoolOp::Subtract, t, &[half_space]).unwrap());
    let caps: Vec<FaceInfo> = k
        .faces(cut)
        .unwrap()
        .into_iter()
        .filter(|f| f.plane.as_ref().is_some_and(|pl| pl.normal.x.abs() > 1.0 - 1e-9 && pl.origin.x.abs() < 1e-6))
        .collect();
    assert_eq!(caps.len(), 2, "two cut faces");
    close(caps.iter().map(|f| f.area).sum::<f64>(), 600.0, 1e-6);
    // The input is untouched (the drawing's part keeps its body).
    close(volume(k, t), PI * (400.0 - 100.0) * 30.0, 1e-3);
    close(volume(k, cut), PI * (400.0 - 100.0) * 30.0 / 2.0, 1e-3);
}

// ---------------------------------------------------------------------------------------------
// P3.10: draft, offset, variable and asymmetric fillets, loft Match conditions, non-planar faces

/// The frustum between squares of sides `a` and `b`, `h` apart: h(A + B + √(AB))/3.
fn square_frustum(a: f64, b: f64, h: f64) -> f64 {
    h * (a * a + b * b + a * b) / 3.0
}

/// The four side faces of a box standing on Top (normals horizontal).
fn side_faces(k: &dyn Kernel, body: BodyId) -> Vec<FaceId> {
    k.faces(body)
        .unwrap()
        .into_iter()
        .filter(|f| f.plane.is_some_and(|pl| pl.normal.z.abs() < 1e-9))
        .map(|f| f.id)
        .collect()
}

/// P3.10 (PS4.9): a 100³ cube's four sides drafted 5° about its bottom face (neutral plane
/// z = 0, pull +Z) lean in: the top square's side is 100 − 2·100·tan 5°, and the body is the
/// frustum h(A + B + √(AB))/3. A negative angle leans them out (100 + 2·100·tan 5°). About the
/// mid plane z = 50 the bottom grows and the top shrinks by 100·tan 5° each side... by
/// 2·50·tan 5° in all. A Ø20 × 20 cylinder's side drafted 5° is the cone frustum
/// πh(R² + Rr + r²)/3 with r = 10 − 20 tan 5°. The top face, parallel to the neutral plane,
/// can't be drafted.
pub fn draft_cube_sides(k: &mut dyn Kernel) {
    let t = 5f64.to_radians().tan();
    let cube = plate(k, 0.0, 0.0, 100.0, 100.0, 0.0, 100.0);
    let sides = side_faces(k, cube);
    assert_eq!(sides.len(), 4);
    let spec = |angle: f64, z: f64| DraftSpec {
        faces: sides.clone(),
        angle: angle.to_radians(),
        pull: up(),
        neutral: Point3::new(0.0, 0.0, z),
        tangent_propagation: true,
    };
    let inward = one(k.draft(cube, &spec(5.0, 0.0)).unwrap());
    close(volume(k, inward), square_frustum(100.0, 100.0 - 200.0 * t, 100.0), 1e-6);
    let lid = face_at(k, inward, Vector3::z(), 100.0);
    let area = k.faces(inward).unwrap().into_iter().find(|f| f.id == lid).unwrap().area;
    close(area, (100.0 - 200.0 * t).powi(2), 1e-6);
    let outward = one(k.draft(cube, &spec(-5.0, 0.0)).unwrap());
    close(volume(k, outward), square_frustum(100.0, 100.0 + 200.0 * t, 100.0), 1e-6);
    let mid = one(k.draft(cube, &spec(5.0, 50.0)).unwrap());
    close(volume(k, mid), square_frustum(100.0 + 100.0 * t, 100.0 - 100.0 * t, 100.0), 1e-6);
    // A cylinder becomes a cone.
    let cyl = one(k.extrude(&top(vec![region(circle(p(0.0, 0.0), 10.0))]), Extent::Blind(20.0)).unwrap());
    let side: Vec<FaceId> = k.faces(cyl).unwrap().into_iter().filter(|f| f.kind == SurfaceKind::Cylinder).map(|f| f.id).collect();
    assert_eq!(side.len(), 1);
    let cone = one(
        k.draft(cyl, &DraftSpec { faces: side, angle: 5f64.to_radians(), pull: up(), neutral: Point3::origin(), tangent_propagation: true })
            .unwrap(),
    );
    let r = 10.0 - 20.0 * t;
    close(volume(k, cone), PI * 20.0 * (100.0 + 10.0 * r + r * r) / 3.0, 1e-5);
    // A face parallel to the neutral plane can't be drafted.
    let top_face = face_at(k, cube, Vector3::z(), 100.0);
    assert!(k.draft(cube, &DraftSpec { faces: vec![top_face], ..spec(5.0, 0.0) }).is_err());
}

/// P3.10 (PS5.5, the Boolean's Subtract offset): the 10 × 20 × 30 box offset 1 mm with sharp
/// edges is 12 × 22 × 32; with rounded edges it is the box grown by a ball (Steiner's formula)
/// abc + 2(ab + bc + ca)d + π(a + b + c)d² + 4πd³/3; only the top face offset 5 mm is
/// 10 × 20 × 35.
pub fn offset_solids(k: &mut dyn Kernel) {
    let b = make_box(k);
    let sharp = one(k.offset(b, &OffsetSpec { distance: 1.0, faces: vec![], sharp: true }).unwrap());
    close(volume(k, sharp), 12.0 * 22.0 * 32.0, 1e-6);
    let round = one(k.offset(b, &OffsetSpec { distance: 1.0, faces: vec![], sharp: false }).unwrap());
    let (a, bb, c, d) = (10.0, 20.0, 30.0, 1.0);
    let steiner = a * bb * c + 2.0 * (a * bb + bb * c + c * a) * d + PI * (a + bb + c) * d * d + 4.0 * PI * d * d * d / 3.0;
    close(volume(k, round), steiner, 1e-3);
    let top_face = face_at(k, b, Vector3::z(), 30.0);
    let faces: Vec<(FaceId, f64)> = k
        .faces(b)
        .unwrap()
        .into_iter()
        .map(|f| (f.id, if f.id == top_face { 5.0 } else { 0.0 }))
        .collect();
    let taller = one(k.offset(b, &OffsetSpec { distance: 0.0, faces, sharp: true }).unwrap());
    close(volume(k, taller), 10.0 * 20.0 * 35.0, 1e-6);
}

/// P3.10 (PS14.6 Variable fillet): on the box's 30 mm vertical edge between two faces at 90°,
/// the fillet's section at radius r removes (1 − π/4)r² per mm. A law 3 → 3 → 3 is the
/// constant R3: (1 − π/4)·9·30 exactly. A law from 2 at the edge's start to 4 at its end removes
/// between the constant R2's and R4's amounts, and within 1 % of the closed form for circular
/// sections of the linearly varying radius, (1 − π/4)∫(2 + z/15)² dz = (1 − π/4)·280 = 60.089
/// (OCCT gives 60.619: its evolving-radius surface isn't exactly those sections, so this is a
/// bound, not an exact value).
pub fn fillet_variable_radius(k: &mut dyn Kernel) {
    let b = make_box(k);
    let edge = vertical_edge(k, b, 30.0);
    let v0 = volume(k, b);
    let flat = one(k.fillet_variable(b, &[FilletLaw { edge, radii: vec![(0.0, 3.0), (0.5, 3.0), (1.0, 3.0)] }], true).unwrap());
    close(v0 - volume(k, flat), (1.0 - PI / 4.0) * 9.0 * 30.0, 1e-6);
    let info = k.edges(b).unwrap().into_iter().find(|e| e.id == edge).unwrap();
    // Radius 2 where the edge starts, 4 where it ends.
    let (r0, r1) = (2.0, 4.0);
    assert!((info.start - info.end).norm() > 29.0);
    let grown = one(k.fillet_variable(b, &[FilletLaw { edge, radii: vec![(0.0, r0), (1.0, r1)] }], true).unwrap());
    let removed = v0 - volume(k, grown);
    let c = 1.0 - PI / 4.0;
    assert!(removed > c * 4.0 * 30.0 && removed < c * 16.0 * 30.0, "{removed}");
    close(removed, c * 280.0, 0.01 * c * 280.0);
    assert!(k.fillet_variable(b, &[FilletLaw { edge, radii: vec![(0.0, -1.0)] }], true).is_err());
}

/// P3.10 (PS14.6 Asymmetric): on the box's 90° vertical edge, radii 2 and 4 give the quarter
/// ellipse with those semi-axes, removing (1 − π/4)·2·4 per mm; flipped, the same amount with
/// the contact distances swapped between the faces.
pub fn fillet_asymmetric(k: &mut dyn Kernel) {
    let b = make_box(k);
    let edge = vertical_edge(k, b, 30.0);
    let v0 = volume(k, b);
    let spec = |flip: bool| FilletSpec {
        size: FilletSize::Radius(2.0),
        allow_overflow: true,
        profile: FilletProfile::Asymmetric { second: 4.0, flip },
    };
    let a = one(k.fillet_with(b, &[edge], &spec(false)).unwrap());
    close(v0 - volume(k, a), (1.0 - PI / 4.0) * 8.0 * 30.0, 1e-4);
    let f = one(k.fillet_with(b, &[edge], &spec(true)).unwrap());
    close(v0 - volume(k, f), (1.0 - PI / 4.0) * 8.0 * 30.0, 1e-4);
    // The flip moves the centroid: the two results differ.
    let (ca, cf) = (k.mass_properties(a).unwrap().center_of_mass, k.mass_properties(f).unwrap().center_of_mass);
    assert!((ca - cf).norm() > 1e-4);
}

/// P3.11 (PS14.6 Partial fillet): R2 on the box's 30 mm vertical edge between 0.25 and 0.75 of
/// it removes the quarter-round section (1 − π/4)·R² along exactly those 15 mm, with a flat end
/// face square to the edge at each bound; the whole range equals the whole fillet; a concave
/// edge works the same way (material added); bad ranges are refused.
pub fn fillet_partial(k: &mut dyn Kernel) {
    let b = make_box(k);
    let edge = vertical_edge(k, b, 30.0);
    let v0 = volume(k, b);
    let c = 1.0 - PI / 4.0;
    let spec = FilletSpec::radius(2.0);
    let r = k.fillet_partial(b, edge, &spec, 0.25, 0.75).unwrap();
    let part = one(r.clone());
    close(v0 - volume(k, part), c * 4.0 * 15.0, 1e-6);
    // The fillet face and the two end faces are new; everything else continues the box.
    let origins: Vec<_> = r.history.generated.iter().map(|(_, o)| *o).collect();
    assert_eq!(origins.iter().filter(|o| matches!(o, Origin::FromEdge { .. })).count(), 1, "{origins:?}");
    assert!(origins.contains(&Origin::StartCap { region: 0 }) && origins.contains(&Origin::EndCap { region: 0 }), "{origins:?}");
    let caps: Vec<_> = r.history.generated.iter().filter(|(_, o)| !matches!(o, Origin::FromEdge { .. })).map(|(f, _)| *f).collect();
    let faces = k.faces(part).unwrap();
    let info = k.edges(b).unwrap().into_iter().find(|e| e.id == edge).unwrap();
    for f in caps {
        let face = faces.iter().find(|x| x.id == f).unwrap();
        close(face.area, c * 4.0, 1e-6);
        let along = (face.center - info.start).dot(&(info.end - info.start).normalize());
        assert!((along - 7.5).abs() < 1e-6 || (along - 22.5).abs() < 1e-6, "{along}");
    }
    // From the start: the end at the vertex is the whole fillet's.
    let s = one(k.fillet_partial(b, edge, &spec, 0.0, 0.5).unwrap());
    close(v0 - volume(k, s), c * 4.0 * 15.0, 1e-6);
    let whole = one(k.fillet_partial(b, edge, &spec, 0.0, 1.0).unwrap());
    let full = one(k.fillet_with(b, &[edge], &spec).unwrap());
    close(volume(k, whole), volume(k, full), 1e-6);
    // A circular edge (a Ø20 cylinder's top rim) from 0.1 to 0.6: half of the round's volume.
    let cyl = one(k.extrude(&top(vec![region(circle(p(0.0, 0.0), 10.0))]), Extent::Blind(10.0)).unwrap());
    let rim = k.edges(cyl).unwrap().into_iter().find(|e| e.curve == CurveKind::Circle && e.start.z > 5.0).unwrap().id;
    let vc = volume(k, cyl);
    let rf = one(k.fillet_with(cyl, &[rim], &spec).unwrap());
    let round = vc - volume(k, rf);
    let hf = one(k.fillet_partial(cyl, rim, &spec, 0.1, 0.6).unwrap());
    let half = vc - volume(k, hf);
    close(half, round / 2.0, 1e-4 * round);
    // The L's inner (concave) edge: material added, the same section.
    let l = one(k.extrude(&top(vec![region(polygon(&[p(0.0, 0.0), p(20.0, 0.0), p(20.0, 10.0), p(10.0, 10.0), p(10.0, 20.0), p(0.0, 20.0)]))]), Extent::Blind(30.0)).unwrap());
    let inner = k.edges(l).unwrap().into_iter().find(|e| (e.start.x - 10.0).abs() < 1e-9 && (e.start.y - 10.0).abs() < 1e-9 && (e.end.x - 10.0).abs() < 1e-9 && (e.end.y - 10.0).abs() < 1e-9).unwrap().id;
    let vl = volume(k, l);
    let lf = one(k.fillet_partial(l, inner, &spec, 0.25, 0.75).unwrap());
    close(volume(k, lf) - vl, c * 4.0 * 15.0, 1e-6);
    assert!(k.fillet_partial(b, edge, &spec, 0.6, 0.4).is_err());
    assert!(k.fillet_partial(b, edge, &spec, -0.1, 0.4).is_err());
}

/// The z component of the unit normals of `face`'s mesh points at height `z`.
fn normal_z_at(k: &dyn Kernel, body: BodyId, face: FaceId, z: f64) -> Vec<f64> {
    let mesh = k.tessellate(body, Tessellation::default()).unwrap();
    let mut out = Vec::new();
    for (t, tri) in mesh.indices.iter().enumerate() {
        if mesh.triangle_faces[t] != face {
            continue;
        }
        for &i in tri {
            let q = mesh.positions[i as usize];
            if (q.z - z).abs() < 1e-4 {
                out.push(mesh.normals[i as usize].z);
            }
        }
    }
    out
}

/// P3.10 (PS20.4 Match tangent, Match curvature): Loft 1 from a circle r 10 at z = 0 to one of
/// r 5 at z = 20 is a cone frustum; its side's normal leans up by n_z = 0.25/√(1 + 0.25²).
/// Loft 2 from Loft 1's top face to a circle r 10 at z = 40 with Match tangent continues the
/// cone: its side's normal where it starts has the same n_z (G1). Without it (default) the new
/// side flares out, n_z = −0.25/√1.0625; Normal to profile starts vertical (n_z = 0). Match
/// curvature is also G1 there. A sketch profile has no faces to match (an error).
pub fn loft_match_tangent(k: &mut dyn Kernel) {
    let none = LoftEnd::default();
    let first = loft(k, vec![LoftSection::Profile(circle_at(0.0, 0.0, 10.0)), LoftSection::Profile(circle_at(20.0, 0.0, 5.0))], none, none);
    close(volume(k, first), PI * 20.0 * (100.0 + 50.0 + 25.0) / 3.0, 1e-6);
    let cap = face_at(k, first, Vector3::z(), 20.0);
    let cone_nz = 0.25 / (1.0f64 + 0.0625).sqrt();
    let side = |k: &dyn Kernel, b: BodyId| {
        k.faces(b).unwrap().into_iter().find(|f| f.plane.is_none()).expect("a side").id
    };
    let first_side = side(k, first);
    for nz in normal_z_at(k, first, first_side, 20.0) {
        close(nz, cone_nz, 1e-3);
    }
    let from_cap = |k: &mut dyn Kernel, c: LoftCondition| {
        loft(
            k,
            vec![LoftSection::Face(FaceInput { body: first, face: cap, source: 7 }), LoftSection::Profile(circle_at(40.0, 0.0, 10.0))],
            LoftEnd { condition: c, magnitude: 1.0 },
            none,
        )
    };
    let matched = from_cap(k, LoftCondition::MatchTangent);
    let s = side(k, matched);
    let nzs = normal_z_at(k, matched, s, 20.0);
    assert!(!nzs.is_empty());
    for nz in nzs {
        close(nz, cone_nz, 2e-3);
    }
    let plain = from_cap(k, LoftCondition::Default);
    for nz in normal_z_at(k, plain, side(k, plain), 20.0) {
        close(nz, -cone_nz, 2e-3);
    }
    let normal = from_cap(k, LoftCondition::NormalToProfile);
    for nz in normal_z_at(k, normal, side(k, normal), 20.0) {
        close(nz, 0.0, 2e-3);
    }
    let g2 = from_cap(k, LoftCondition::MatchCurvature);
    for nz in normal_z_at(k, g2, side(k, g2), 20.0) {
        close(nz, cone_nz, 2e-3);
    }
    assert!((volume(k, g2) - volume(k, matched)).abs() > 1e-3);
    // Both lofts join Loft 1 into one solid.
    let joined = one(k.boolean(BoolOp::Union, first, &[matched]).unwrap());
    assert_eq!(k.solid_count(joined).unwrap(), 1);
    // A sketch profile has nothing to match.
    let r = k.loft_with(&LoftSpec {
        body: BodyKind::Solid,
        sections: vec![LoftSection::Profile(circle_at(0.0, 0.0, 10.0)), LoftSection::Profile(circle_at(20.0, 0.0, 5.0))],
        start: LoftEnd { condition: LoftCondition::MatchTangent, magnitude: 1.0 },
        end: none,
        source: 9,
    });
    assert!(r.is_err());
}

/// P3.10 (PS20.1): a non-planar face as a profile. A half cylinder r 10 along −Y (0..20),
/// lying on Top; its curved face lofted to a 20 × 20 rectangle at z = 30 over it. The loft's
/// sides are the planes x = ±10 and y = 0, −20 (the semicircle and the rectangle's edge lie in
/// one plane), so the solid is the box 20 × 20 × 30 less the half cylinder:
/// 12 000 − (π·100/2)·20 = 12 000 − 1000π. Its bottom is the curved face itself.
pub fn loft_from_a_curved_face(k: &mut dyn Kernel) {
    let half = Profile::new(
        front(),
        vec![region(Loop { curves: vec![arc(p(0.0, 0.0), 10.0, 0.0, PI), line(p(-10.0, 0.0), p(10.0, 0.0))] })],
    );
    let body = one(k.extrude_with(&half, &ExtrudeSpec::blind(-Vector3::y_axis(), 20.0)).unwrap());
    close(volume(k, body), 50.0 * PI * 20.0, 1e-6);
    let curved = k.faces(body).unwrap().into_iter().find(|f| f.kind == SurfaceKind::Cylinder).expect("the curved face").id;
    let rect_top = Profile::new(
        Plane { origin: Point3::new(0.0, 0.0, 30.0), ..Plane::top() },
        vec![Region { outer: rect(-10.0, -20.0, 10.0, 0.0), holes: vec![], source: Some(5) }],
    );
    let none = LoftEnd::default();
    let l = loft(k, vec![LoftSection::Face(FaceInput { body, face: curved, source: 7 }), LoftSection::Profile(rect_top)], none, none);
    close(volume(k, l), 12000.0 - 1000.0 * PI, 1e-4);
    assert!(k.faces(l).unwrap().iter().any(|f| f.kind == SurfaceKind::Cylinder));
    // It sits on the half cylinder: together one solid, the box.
    let joined = one(k.boolean(BoolOp::Union, l, &[body]).unwrap());
    close(volume(k, joined), 12000.0, 1e-3);
}



/// The Import feature's kernel side: a STEP file read back as one body per solid with the
/// volumes written, and a closed triangle mesh (a cube's 12 triangles) sewn into a solid.
fn import_step_and_mesh(k: &mut dyn Kernel) {
    let a = make_box(k);
    let b = one(k.extrude(&top(vec![region(rect(50.0, 0.0, 55.0, 5.0))]), Extent::Blind(5.0)).unwrap());
    let step = match k.export_step(&[a, b]) {
        Ok(s) => s,
        Err(KernelError::Unsupported(_)) => return,
        Err(e) => panic!("{e}"),
    };
    let bodies = k.import_step(&step).unwrap();
    assert_eq!(bodies.len(), 2);
    let mut v: Vec<f64> = bodies.iter().map(|b| volume(k, *b)).collect();
    v.sort_by(f64::total_cmp);
    close(v[0], 125.0, 1e-6);
    close(v[1], 6000.0, 1e-6);
    assert!(k.import_step(b"not a step file").is_err());

    // A 10 mm cube as 12 outward triangles.
    let c = |x: f64, y: f64, z: f64| Point3::new(x * 10.0, y * 10.0, z * 10.0);
    let quads = [
        [c(0., 0., 0.), c(0., 1., 0.), c(1., 1., 0.), c(1., 0., 0.)],
        [c(0., 0., 1.), c(1., 0., 1.), c(1., 1., 1.), c(0., 1., 1.)],
        [c(0., 0., 0.), c(1., 0., 0.), c(1., 0., 1.), c(0., 0., 1.)],
        [c(0., 1., 0.), c(0., 1., 1.), c(1., 1., 1.), c(1., 1., 0.)],
        [c(0., 0., 0.), c(0., 0., 1.), c(0., 1., 1.), c(0., 1., 0.)],
        [c(1., 0., 0.), c(1., 1., 0.), c(1., 1., 1.), c(1., 0., 1.)],
    ];
    let tris: Vec<[Point3<f64>; 3]> = quads.iter().flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]]).collect();
    let cube = k.mesh_solid(&tris, 1e-6).unwrap();
    close(volume(k, cube), 1000.0, 1e-6);
    // Coplanar triangles merged: the six faces of the cube.
    assert_eq!(k.faces(cube).unwrap().len(), 6);
    // An open mesh (a face missing) is not a solid.
    assert!(k.mesh_solid(&tris[..10], 1e-6).is_err());
}

/// `joins`: bodies that share a face or overlap make one solid; bodies apart don't.
fn joins_touch_overlap_apart(k: &mut dyn Kernel) {
    let a = plate(k, 0.0, 0.0, 10.0, 10.0, 0.0, 10.0);
    let touching = plate(k, 10.0, 0.0, 20.0, 10.0, 0.0, 10.0);
    let overlapping = plate(k, 5.0, 5.0, 15.0, 15.0, 5.0, 15.0);
    let apart = plate(k, 30.0, 0.0, 40.0, 10.0, 0.0, 10.0);
    assert!(k.joins(a, touching).unwrap());
    assert!(k.joins(a, overlapping).unwrap());
    assert!(!k.joins(a, apart).unwrap());
}

// ---------------------------------------------------------------------------------------------
// Backends

/// P3F.2: STEP and IGES export and import, with assembly structure, and the mesh exports.
/// - A 100 × 60 × 25 box through STEP comes back with V = 150 000 mm³ and A = 20 000 mm².
/// - An assembly of two parts (the box twice, turned 90° about Z and moved, and a Ø20 × 10
///   cylinder once) comes back as 2 parts and 3 occurrences at the same placements, with the
///   product and instance names.
/// - IGES keeps the box's area to 1e−3 (and its volume: solids are written as MSBO solids).
/// - The box's tessellation is 12 triangles; as binary STL, ASCII STL and OBJ its mesh volume
///   (divergence theorem) is 150 000 mm³.
pub fn exchange_round_trips(k: &mut dyn Kernel) {
    use cadrs_kernel::exchange::*;
    let bx = one(k.extrude(&top(vec![region(rect(0.0, 0.0, 100.0, 60.0))]), Extent::Blind(25.0)).unwrap());
    let cyl = one(k.extrude(&top(vec![region(circle(p(0.0, 0.0), 10.0))]), Extent::Blind(10.0)).unwrap());
    // STEP, one part.
    let bytes = k.export_model(ExchangeFormat::Step, "Box", &[(bx, "Box".into())], &[]).unwrap();
    let m = k.import_model(ExchangeFormat::Step, &bytes).unwrap();
    assert_eq!(m.parts.len(), 1);
    assert_eq!(m.parts[0].name, "Box");
    assert!(!m.is_assembly());
    let mp = k.mass_properties(m.parts[0].body).unwrap();
    close(mp.volume, 150_000.0, 1e-6);
    close(mp.surface_area, 20_000.0, 1e-6);
    // STEP, an assembly.
    let turn = Motion {
        linear: nalgebra::Matrix3::new(0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0),
        translation: Vector3::new(200.0, 10.0, 5.0),
    };
    let lift = Motion { linear: nalgebra::Matrix3::identity(), translation: Vector3::new(0.0, 0.0, 25.0) };
    let instances = vec![
        ExportInstance { part: 0, placement: Motion::identity(), name: "Box <1>".into() },
        ExportInstance { part: 0, placement: turn, name: "Box <2>".into() },
        ExportInstance { part: 1, placement: lift, name: "Pin <1>".into() },
    ];
    let bytes = k
        .export_model(ExchangeFormat::Step, "Two parts", &[(bx, "Box".into()), (cyl, "Pin".into())], &instances)
        .unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("NEXT_ASSEMBLY_USAGE_OCCURRENCE"), "an assembly in the file");
    let m = k.import_model(ExchangeFormat::Step, &bytes).unwrap();
    assert!(m.is_assembly());
    assert_eq!(m.name, "Two parts");
    assert_eq!(m.parts.len(), 2, "each part once");
    assert_eq!(m.occurrences.len(), 3);
    let names: Vec<&str> = m.parts.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Box", "Pin"]);
    for (want, got) in instances.iter().zip(&m.occurrences) {
        assert_eq!(got.part, want.part);
        assert_eq!(got.name, want.name);
        assert!((got.placement.linear - want.placement.linear).norm() < 1e-9, "{:?}", got.placement);
        assert!((got.placement.translation - want.placement.translation).norm() < 1e-9, "{:?}", got.placement);
    }
    close(k.mass_properties(m.parts[1].body).unwrap().volume, PI * 100.0 * 10.0, 1e-6);
    // IGES.
    let bytes = k.export_model(ExchangeFormat::Iges, "Box", &[(bx, "Box".into())], &[]).unwrap();
    let m = k.import_model(ExchangeFormat::Iges, &bytes).unwrap();
    let area: f64 = m.parts.iter().map(|p| k.mass_properties(p.body).unwrap().surface_area).sum();
    close(area, 20_000.0, 1e-3);
    let volume: f64 = m.parts.iter().map(|p| k.mass_properties(p.body).unwrap().volume).sum();
    close(volume, 150_000.0, 1e-3);
    // Meshes.
    let mesh = k.tessellate(bx, Tessellation { deflection: 0.1, angle: 0.5 }).unwrap();
    assert_eq!(mesh.indices.len(), 12);
    close(mesh_volume(triangles(&mesh)), 150_000.0, 1e-6);
    let stl = read_stl_binary(&write_stl_binary(&mesh, "box")).unwrap();
    assert_eq!(stl.len(), 12);
    close(mesh_volume(stl), 150_000.0, 1e-6);
    close(mesh_volume(read_stl_ascii(&write_stl_ascii(&mesh, "box"))), 150_000.0, 1e-6);
    let obj = read_obj(&write_obj(&[("box", &mesh)]));
    assert_eq!(obj.len(), 12);
    close(mesh_volume(obj), 150_000.0, 1e-6);
}

macro_rules! conformance {
    ($backend:ident, $make:expr) => {
        mod $backend {
            conformance!(@cases $make;
                box_extrude, extrude_symmetric, extrude_two_sided, extrude_negative_blind,
                extrude_region_with_hole, boolean_subtract_intersect, boolean_multiple_tools,
                transform_moves_body, intersect_moved_copies, revolve_tube, fillet_box_edge, chamfer_box_edge, shell_box,
                tessellate_and_export, circle_edges_have_exact_length, control_arm,
                rectangle_100x60x25, ellipse_profiles, bezier_profiles, failures_are_errors,
                chamfer_two_distances_and_angle, tessellation_normals_and_edges,
                extrude_history, boolean_history_and_split_names, names_survive_edits,
                face_adjacency, tangent_chains, edges_and_vertices,
                extrude_up_to_face_parallel, extrude_up_to_face_oblique, extrude_up_to_face_through_origin,
                extrude_up_to_part_conforms, extrude_up_to_next_from_a_face, extrude_through_all,
                extrude_options, surface_extrude, thin_extrude, split_solids_rays_and_boxes, compound_gathers_bodies,
                extrude_a_face, union_merges_coplanar_faces, revolve_torus, revolve_types,
                revolve_up_to, revolve_surface_and_thin, face_axes_and_edge_circles, revolve_a_face,
                inertia_tensor, fillet_cube_all_edges, fillet_width, fillet_overflow,
                chamfer_options, shell_options, shell_around_a_counterbore, edge_face_radius,
                fillet_sections, full_round, offset_ellipse_profiles, sweep_along_paths, sweep_around_a_circle,
                loft_sections_and_conditions, split_by_plane_and_face, mirror_and_motions,
                face_tools, classify_points, split_faces_by_plane, project_view, section_cut,
                draft_cube_sides, offset_solids, fillet_variable_radius, fillet_asymmetric,
                fillet_partial, fillet_smooth_corner, loft_match_tangent, loft_from_a_curved_face,
                loft_direction_conditions, exchange_round_trips, derived_frame_motion, import_step_and_mesh, joins_touch_overlap_apart);
        }
    };
    (@cases $make:expr; $($case:ident),* $(,)?) => {
        $(
            #[test]
            fn $case() {
                let mut kernel = $make;
                super::$case(&mut kernel);
            }
        )*
    };
}

#[cfg(feature = "occt")]
conformance!(occt, cadrs_kernel::backend::occt::OcctKernel::new());
