//! Fillets that consume a face whole (a full round: the fillets of its two opposite edges meet
//! across it). cable organizer Copy 2, Fillet 2: R1.5 on a 3 mm rib's top and end edges makes
//! two full rounds that meet at the rib's corner, where the rolling ball leaves a quarter
//! sphere joining the two half cylinders.

use std::f64::consts::PI;

use cadrs_kernel::native::kernel::NativeKernel;
use cadrs_kernel::native::tol;
use cadrs_kernel::*;
use nalgebra::{Point2, Point3};

const LENGTH: f64 = 10.0;
const WIDTH: f64 = 3.0;
const HEIGHT: f64 = 5.0;
const R: f64 = WIDTH / 2.0;

/// A box from `lo` to `hi` (x, y) extruded from height `z0` by `h`.
fn block(k: &mut NativeKernel, lo: (f64, f64), hi: (f64, f64), z0: f64, h: f64) -> BodyId {
    let c =
        [(lo.0, lo.1), (hi.0, lo.1), (hi.0, hi.1), (lo.0, hi.1)].map(|(x, y)| Point2::new(x, y));
    let outer = Loop {
        curves: (0..4)
            .map(|i| Curve2::Line {
                a: c[i],
                b: c[(i + 1) % 4],
                source: None,
            })
            .collect(),
    };
    let plane = Plane {
        origin: Point3::new(0.0, 0.0, z0),
        ..Plane::top()
    };
    k.extrude(
        &Profile::new(
            plane,
            vec![Region {
                outer,
                holes: vec![],
                source: None,
            }],
        ),
        Extent::Blind(h),
    )
    .unwrap()
    .bodies[0]
}

/// A rib LENGTH × WIDTH × HEIGHT and its edges.
fn rib() -> (NativeKernel, BodyId, Vec<EdgeInfo>) {
    let mut k = NativeKernel::new();
    let id = block(&mut k, (0.0, 0.0), (LENGTH, WIDTH), 0.0, HEIGHT);
    let edges = k.edges(id).unwrap();
    (k, id, edges)
}

fn spec() -> FilletSpec {
    FilletSpec {
        size: FilletSize::Radius(R),
        allow_overflow: true,
        profile: FilletProfile::Circular,
        propagate: true,
    }
}

fn at(v: f64, w: f64) -> bool {
    (v - w).abs() < tol::ROUNDOFF
}

/// The rib's top long edges, its top end edge at x = LENGTH and its two vertical edges there:
/// the full rounds of the top and of the end, meeting at the corner.
fn top_and_end(e: &EdgeInfo) -> bool {
    let top = at(e.start.z, HEIGHT) && at(e.end.z, HEIGHT);
    let end = at(e.start.x, LENGTH) && at(e.end.x, LENGTH);
    top && (end || !at(e.start.x, e.end.x)) || end && !at(e.start.z, e.end.z)
}

/// The volume the top and end full rounds leave of the rib: each round takes the 2r × r strip
/// less the half disc (r²(2 − π/2) per length) up to the ball centre (LENGTH − r along the top,
/// HEIGHT − r down the end), the corner the r × 2r × r block less the quarter ball.
fn rounded_rib_volume() -> f64 {
    LENGTH * WIDTH * HEIGHT
        - R * R * (2.0 - PI / 2.0) * (LENGTH - R + HEIGHT - R)
        - (2.0 * R * R * R - PI * R * R * R / 3.0)
}

/// The rib's top long edges: one full round across the top.
#[test]
fn one_full_round_builds() {
    let (mut k, id, edges) = rib();
    let top: Vec<EdgeId> = edges
        .iter()
        .filter(|e| at(e.start.z, HEIGHT) && at(e.end.z, HEIGHT) && !at(e.start.x, e.end.x))
        .map(|e| e.id)
        .collect();
    let r = k.fillet_with(id, &top, &spec()).unwrap();
    let v = k.mass_properties(r.bodies[0]).unwrap().volume;
    let want = LENGTH * WIDTH * HEIGHT - 2.0 * (1.0 - PI / 4.0) * R * R * LENGTH;
    assert!(
        (v - want).abs() < tol::POINT * want,
        "volume {v}, want {want}"
    );
}

/// The top's and the end's edges: two full rounds meeting at the corner, joined by the ball's
/// quarter sphere (not the cylinders' intersection), the top end edge's fillet gone.
#[test]
fn full_rounds_meeting_at_a_corner_build() {
    let (mut k, id, edges) = rib();
    let picked: Vec<EdgeId> = edges
        .iter()
        .filter(|e| top_and_end(e))
        .map(|e| e.id)
        .collect();
    assert_eq!(picked.len(), 5);
    let r = k.fillet_with(id, &picked, &spec()).unwrap();
    let out = k.get(r.bodies[0]).unwrap();
    assert!(out.check().is_empty(), "{:?}", out.check());
    assert_eq!(out.solids.len(), 1);
    let v = k.mass_properties(r.bodies[0]).unwrap().volume;
    let want = rounded_rib_volume();
    assert!((v - want).abs() < 1e-6 * want, "volume {v}, want {want}");
    // The corner face is a sphere about the ball centre, named from the top end edge.
    let sphere = (0..out.faces.len())
        .filter(|&f| {
            matches!(
                out.faces[f].surf,
                cadrs_kernel::native::surface::Surface::Sphere { .. }
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(sphere.len(), 1, "one quarter sphere");
    assert!(
        r.history
            .generated
            .iter()
            .any(|(f, o)| f.0 as usize == sphere[0] && matches!(o, Origin::FromEdge { .. }))
    );
}

/// The rib standing on a plate, its end round running down into the plate: the end face's
/// edge against the plate (concave) picked too becomes the half circle where the round meets
/// the plate, filleted there (half a torus, ending where the circle runs on into the rib's side
/// faces' edges on the plate).
#[test]
fn full_round_into_a_wall_with_its_root_filleted() {
    let mut k = NativeKernel::new();
    let plate = block(&mut k, (-5.0, -5.0), (LENGTH + 5.0, WIDTH + 5.0), -2.0, 2.0);
    let rib = block(&mut k, (0.0, 0.0), (LENGTH, WIDTH), 0.0, HEIGHT);
    let id = k.boolean(BoolOp::Union, plate, &[rib]).unwrap().bodies[0];
    let edges = k.edges(id).unwrap();
    let root = |e: &EdgeInfo| {
        at(e.start.x, LENGTH) && at(e.end.x, LENGTH) && at(e.start.z, 0.0) && at(e.end.z, 0.0)
    };
    let picked: Vec<EdgeId> = edges
        .iter()
        .filter(|e| top_and_end(e) || root(e))
        .map(|e| e.id)
        .collect();
    assert_eq!(picked.len(), 6);
    let v0 = k.mass_properties(id).unwrap().volume;
    let r = k.fillet_with(id, &picked, &spec()).unwrap();
    let out = k.get(r.bodies[0]).unwrap();
    assert!(out.check().is_empty(), "{:?}", out.check());
    assert_eq!(out.solids.len(), 1);
    let v = k.mass_properties(r.bodies[0]).unwrap().volume;
    // The rounded rib on the plate, plus the root fillet's half torus: its section (r²(1 − π/4))
    // turned half round at its centroid's radius r + r(10 − 3π)/(12 − 3π) (Pappus).
    let plate_v = v0 - LENGTH * WIDTH * HEIGHT;
    let section = R * R * (1.0 - PI / 4.0);
    let centroid = R + R * (10.0 - 3.0 * PI) / (12.0 - 3.0 * PI);
    let want = plate_v + rounded_rib_volume() + section * PI * centroid;
    assert!((v - want).abs() < 1e-6 * want, "volume {v}, want {want}");
}

/// The cable organizer's comb (Fillet 2, three of its eighteen prongs): prongs 3 thick standing
/// 11 high off a spine, each with a 3 × 3 tip over a step. Per prong: the full round down its
/// end face (its two 11-long edges), the full round along the tip's top (its two 3-long edges,
/// running out over the step), the edge between them (the corner's quarter sphere) and the end
/// face's bottom edge (convex: after the round, the half circle at the bottom, filleted there).
#[test]
fn comb_of_prongs_like_the_cable_organizer() {
    let (len, tip, step, high, gap) = (40.0, 3.0, 8.0, 11.0, 4.0);
    let mut k = NativeKernel::new();
    let mut comb = block(
        &mut k,
        (0.0, 0.0),
        (5.0, 3.0 * WIDTH + 2.0 * gap),
        0.0,
        step,
    );
    for i in 0..3 {
        // The prong's side profile in the xz plane, extruded across y.
        let y0 = i as f64 * (WIDTH + gap);
        let c = [
            (0.0, 0.0),
            (len, 0.0),
            (len, high),
            (len - tip, high),
            (len - tip, step),
            (0.0, step),
        ]
        .map(|(x, z)| Point2::new(x, z));
        let outer = Loop {
            curves: (0..6)
                .map(|i| Curve2::Line {
                    a: c[i],
                    b: c[(i + 1) % 6],
                    source: None,
                })
                .collect(),
        };
        let xz = Plane {
            origin: Point3::new(0.0, y0 + WIDTH, 0.0),
            x_dir: nalgebra::Vector3::x_axis(),
            normal: -nalgebra::Vector3::y_axis(),
        };
        let prong = k
            .extrude(
                &Profile::new(
                    xz,
                    vec![Region {
                        outer,
                        holes: vec![],
                        source: None,
                    }],
                ),
                Extent::Blind(WIDTH),
            )
            .unwrap()
            .bodies[0];
        comb = k.boolean(BoolOp::Union, comb, &[prong]).unwrap().bodies[0];
    }
    let edges = k.edges(comb).unwrap();
    let picked: Vec<EdgeId> = edges
        .iter()
        .filter(|e| {
            let end = at(e.start.x, len) && at(e.end.x, len);
            let top = at(e.start.z, high) && at(e.end.z, high);
            end || top && at(e.start.y, e.end.y)
        })
        .map(|e| e.id)
        .collect();
    assert_eq!(picked.len(), 3 * 6);
    let v0 = k.mass_properties(comb).unwrap().volume;
    let r = k.fillet_with(comb, &picked, &spec()).unwrap();
    let out = k.get(r.bodies[0]).unwrap();
    assert!(out.check().is_empty(), "{:?}", out.check());
    assert_eq!(out.solids.len(), 1);
    let v = k.mass_properties(r.bodies[0]).unwrap().volume;
    // Per prong: the end's round up to the ball centre (high − r), the tip's from the step's
    // edge to it (tip − r), the corner, and the bottom fillet's half torus (Pappus, its section
    // inside the round, the centroid at r − r(10 − 3π)/(12 − 3π) from the axis).
    let strip = R * R * (2.0 - PI / 2.0);
    let corner = 2.0 * R * R * R - PI * R * R * R / 3.0;
    let bottom = R * R * (1.0 - PI / 4.0) * PI * (R - R * (10.0 - 3.0 * PI) / (12.0 - 3.0 * PI));
    let want = v0 - 3.0 * (strip * (high - R + tip - R) + corner + bottom);
    assert!((v - want).abs() < 1e-6 * want, "volume {v}, want {want}");
}

/// A rib whose sides lean toward its top (a trapezoid 2b wide at its foot, 2t at its top, h
/// high, extruded along z by `LENGTH`): the full round is the arc of the circle touching the top
/// and both sides, centred (0, h − ρ) with ρ = h·t/(s − (b − t)), s the side's length; the sides
/// stay where they are. It takes, along the length, the four-sided piece between the top and the
/// sides' contacts less the circle's segment over the contacts' chord.
#[test]
fn a_full_round_between_leaning_sides_touches_all_three() {
    let (b, t, h) = (2.0, 1.2, 6.0);
    let mut k = NativeKernel::new();
    let c = [(-b, 0.0), (b, 0.0), (t, h), (-t, h)].map(|(x, y)| Point2::new(x, y));
    let outer = Loop {
        curves: (0..4)
            .map(|i| Curve2::Line {
                a: c[i],
                b: c[(i + 1) % 4],
                source: None,
            })
            .collect(),
    };
    let id = k
        .extrude(
            &Profile::new(
                Plane::top(),
                vec![Region {
                    outer,
                    holes: vec![],
                    source: None,
                }],
            ),
            Extent::Blind(LENGTH),
        )
        .unwrap()
        .bodies[0];
    let (lean, side) = (b - t, (b - t).hypot(h));
    let face = |k: &NativeKernel, n: nalgebra::Vector3<f64>| {
        k.faces(id)
            .unwrap()
            .into_iter()
            .find(|f| f.plane.is_some_and(|pl| pl.normal.dot(&n) > 1.0 - 1e-9))
            .unwrap()
            .id
    };
    let top = face(&k, nalgebra::Vector3::y());
    let right = face(&k, nalgebra::Vector3::new(h, lean, 0.0) / side);
    let left = face(&k, nalgebra::Vector3::new(-h, lean, 0.0) / side);
    let r = k.full_round(id, left, top, right).unwrap();
    let out = k.get(r.bodies[0]).unwrap();
    assert!(out.check().is_empty(), "{:?}", out.check());
    let rho = h * t / (side - lean);
    let n = (h / side, lean / side);
    let touch = (rho * n.0, h - rho + rho * n.1);
    // The piece above the chord between the contacts, less the segment of the circle there.
    let quad = (2.0 * t + 2.0 * touch.0) / 2.0 * (h - touch.1);
    let theta = PI - 2.0 * n.1.atan2(n.0);
    let segment = rho * rho / 2.0 * (theta - theta.sin());
    let want = LENGTH * ((b + t) * h - (quad - segment));
    let v = k.mass_properties(r.bodies[0]).unwrap().volume;
    assert!((v - want).abs() < 1e-9 * want, "volume {v}, want {want}");
    // The round is a cylinder of radius ρ; the top is gone.
    let faces = k.faces(r.bodies[0]).unwrap();
    assert!(faces.iter().any(
        |f| f.kind == SurfaceKind::Cylinder && f.radius.is_some_and(|x| (x - rho).abs() < 1e-9)
    ));
    assert!(!faces.iter().any(|f| {
        f.plane
            .is_some_and(|pl| pl.normal.dot(&nalgebra::Vector3::y()) > 1.0 - 1e-9)
    }));
}
