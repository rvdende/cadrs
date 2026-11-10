//! Regressions for offsets, shells and local operations (`native::local` and `native::offset`) through the `Kernel` trait, checked against closed forms.

use std::f64::consts::PI;

use cadrs_kernel::native::NativeKernel;
use cadrs_kernel::native::math::Frame;
use cadrs_kernel::native::surface::Surface;
use cadrs_kernel::*;
use nalgebra::{Point2, Point3, Vector3};

fn one(r: OpResult) -> BodyId {
    assert_eq!(r.bodies.len(), 1, "one body");
    r.bodies[0]
}

fn polygon(pts: &[(f64, f64)]) -> Loop {
    let p = |q: (f64, f64)| Point2::new(q.0, q.1);
    Loop {
        curves: (0..pts.len())
            .map(|i| Curve2::Line {
                a: p(pts[i]),
                b: p(pts[(i + 1) % pts.len()]),
                source: None,
            })
            .collect(),
    }
}

fn circle(c: (f64, f64), r: f64) -> Loop {
    Loop {
        curves: vec![Curve2::Circle {
            center: Point2::new(c.0, c.1),
            radius: r,
            source: None,
        }],
    }
}

/// The region `outer` less `holes` on the top plane, extruded up by `h`.
fn prism(k: &mut NativeKernel, outer: Loop, holes: Vec<Loop>, h: f64) -> BodyId {
    one(k
        .extrude(
            &Profile::new(
                Plane::top(),
                vec![Region {
                    outer,
                    holes,
                    source: None,
                }],
            ),
            Extent::Blind(h),
        )
        .unwrap())
}

fn volume(k: &NativeKernel, b: BodyId) -> f64 {
    k.mass_properties(b).unwrap().volume
}

fn close(got: f64, want: f64, rel: f64) {
    assert!(
        (got - want).abs() <= rel * want.abs(),
        "{got} vs {want} ({:.1e} off)",
        (got - want).abs() / want.abs()
    );
}

/// The planar face with outward normal `n` at `at` along it.
fn face_at(k: &NativeKernel, b: BodyId, n: Vector3<f64>, at: f64) -> FaceId {
    k.faces(b)
        .unwrap()
        .into_iter()
        .find(|f| {
            f.plane.is_some_and(|p| p.normal.dot(&n) > 1.0 - 1e-9)
                && (f.center.coords.dot(&n) - at).abs() < 1e-9
        })
        .expect("no such face")
        .id
}

/// A box a × b × c grown by a ball of radius d (Steiner's formula).
fn grown_box(a: f64, b: f64, c: f64, d: f64) -> f64 {
    a * b * c
        + 2.0 * d * (a * b + b * c + c * a)
        + PI * d * d * (a + b + c)
        + 4.0 / 3.0 * PI * d * d * d
}

/// A 10 mm cube with a through hole of radius 1 grown by 2 (more than the hole's radius): the
/// hole's wall (a concave cylinder whose offset would have no radius) fills to its axis, its
/// rims' tubes (wider than the rims) are spindle tori. Inside the hole every point is within 1
/// of the wall; above its mouths a point at radius ρ < 1 and height z is nearest the rim, so
/// it is in only when (1 − ρ)² + z² ≤ 4: the grown cube less a dimple of volume
/// 2π∫₀¹ ρ (2 − √(4 − (1 − ρ)²)) dρ at each mouth.
#[test]
fn rounded_offset_past_a_hole_radius() {
    let mut k = NativeKernel::new();
    let (r, d) = (1.0, 2.0);
    let b = prism(
        &mut k,
        polygon(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]),
        vec![circle((5.0, 5.0), r)],
        10.0,
    );
    let out = one(k
        .offset(
            b,
            &OffsetSpec {
                distance: d,
                faces: vec![],
                sharp: false,
            },
        )
        .unwrap());
    // Simpson's rule on the dimple's (smooth) integrand.
    let n = 2000;
    let f = |rho: f64| rho * (d - (d * d - (r - rho) * (r - rho)).sqrt());
    let h = r / n as f64;
    let dimple = 2.0 * PI * h / 3.0
        * (0..=n)
            .map(|i| {
                f(i as f64 * h)
                    * if i == 0 || i == n {
                        1.0
                    } else if i % 2 == 1 {
                        4.0
                    } else {
                        2.0
                    }
            })
            .sum::<f64>();
    close(
        volume(&k, out),
        grown_box(10.0, 10.0, 10.0, d) - 2.0 * dimple,
        1e-6,
    );
}

/// A cylinder (radius 3, height 5) grown by a ball of radius 1: the side moved out, the caps
/// up, the rims' tubes quarter tori. The vertex on each rim (where the seam meets it) is no
/// corner: the rim runs on through it, so it gets no ball (research/algorithms/
/// offsets_sweeps_import.md card 5); the volume is still the exact one,
/// π(r + d)²h + 2πr²d + π²rd² + 4πd³/3.
#[test]
fn rounded_offset_of_a_cylinder() {
    let mut k = NativeKernel::new();
    let (r, h, d) = (3.0, 5.0, 1.0);
    let b = prism(&mut k, circle((0.0, 0.0), r), vec![], h);
    let out = one(k
        .offset(
            b,
            &OffsetSpec {
                distance: d,
                faces: vec![],
                sharp: false,
            },
        )
        .unwrap());
    let want = PI * (r + d) * (r + d) * h
        + 2.0 * PI * r * r * d
        + PI * PI * r * d * d
        + 4.0 / 3.0 * PI * d * d * d;
    close(volume(&k, out), want, 1e-9);
}

/// An L-shaped block (a 10 × 10 square less its 5 × 5 corner, 8 tall) inset by 1, rounded
/// (SMLib's inset, `SetExtendConvexEdges(FALSE)`):
/// the faces move in by 1 and the concave edge, which every point within 1 of keeps away,
/// becomes a quarter round of radius 1 about it: the sharp inset plus the square corner less
/// the quarter disc, (1 − π/4), over the inset height 6.
#[test]
fn rounded_inset_rounds_concave_edges() {
    let mut k = NativeKernel::new();
    let b = prism(
        &mut k,
        polygon(&[
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 5.0),
            (5.0, 5.0),
            (5.0, 10.0),
            (0.0, 10.0),
        ]),
        vec![],
        8.0,
    );
    let out = one(k
        .offset(
            b,
            &OffsetSpec {
                distance: -1.0,
                faces: vec![],
                sharp: false,
            },
        )
        .unwrap());
    // The sharp inset: the 8 × 8 square less its 5 × 5 corner beyond (4, 4) …
    let sharp = (64.0 - 25.0) * 6.0;
    close(volume(&k, out), sharp + (1.0 - PI / 4.0) * 6.0, 1e-9);
    // … which the sharp offset gives.
    let s = one(k
        .offset(
            b,
            &OffsetSpec {
                distance: -1.0,
                faces: vec![],
                sharp: true,
            },
        )
        .unwrap());
    close(volume(&k, s), sharp, 1e-9);
}

/// The same L inset by 1 with its bottom kept: the faces round it move in, the bottom stays.
#[test]
fn rounded_inset_keeps_a_face() {
    let mut k = NativeKernel::new();
    let b = prism(
        &mut k,
        polygon(&[
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 5.0),
            (5.0, 5.0),
            (5.0, 10.0),
            (0.0, 10.0),
        ]),
        vec![],
        8.0,
    );
    let bottom = face_at(&k, b, -Vector3::z(), 0.0);
    let out = one(k
        .offset(
            b,
            &OffsetSpec {
                distance: -1.0,
                faces: vec![(bottom, 0.0)],
                sharp: false,
            },
        )
        .unwrap());
    close(
        volume(&k, out),
        (64.0 - 25.0) * 7.0 + (1.0 - PI / 4.0) * 7.0,
        1e-9,
    );
}

/// An outward shell with sharp corners (SMLib's `bExtendCorners`): the open bottom stays where
/// it was, the other faces move out by 4 and meet where they cross: (108 · 68 · 44) − the body.
#[test]
fn outward_shell_with_sharp_corners() {
    let mut k = NativeKernel::new();
    let b = prism(
        &mut k,
        polygon(&[(0.0, 0.0), (100.0, 0.0), (100.0, 60.0), (0.0, 60.0)]),
        vec![],
        40.0,
    );
    let bottom = face_at(&k, b, -Vector3::z(), 0.0);
    let spec = ShellSpec {
        remove: vec![bottom],
        thickness: 4.0,
        outward: true,
        hollow: false,
        sharp: true,
    };
    let out = one(k.shell_with(b, &spec).unwrap());
    close(
        volume(&k, out),
        108.0 * 68.0 * 44.0 - 100.0 * 60.0 * 40.0,
        1e-9,
    );
}

/// A rounded inward shell of a 20 × 10 block with a fin 2 thick (x 9–11, out to y = 15), all
/// 10 high, bottom open, 1.5 thick: the fin's walls cross (it is thinner than two walls), so it
/// stays solid (OCCT's thick solids keep such ribs; this was refused as walls that intersect
/// themselves). The cavity is the block's 17 × 7 × 8.5 and, under the fin's root, the part of
/// its 2 × 1.5 strip farther than 1.5 from both concave corners (less two quarter discs, which
/// overlap by half a lens), 8.5 high.
#[test]
fn rounded_shell_keeps_a_thin_fin_solid() {
    let mut k = NativeKernel::new();
    let b = prism(
        &mut k,
        polygon(&[
            (0.0, 0.0),
            (20.0, 0.0),
            (20.0, 10.0),
            (11.0, 10.0),
            (11.0, 15.0),
            (9.0, 15.0),
            (9.0, 10.0),
            (0.0, 10.0),
        ]),
        vec![],
        10.0,
    );
    let bottom = face_at(&k, b, -Vector3::z(), 0.0);
    let spec = ShellSpec {
        remove: vec![bottom],
        thickness: 1.5,
        outward: false,
        hollow: false,
        sharp: false,
    };
    let out = one(k.shell_with(b, &spec).unwrap());
    let r: f64 = 1.5;
    let half_lens = r * r * (1.0 / r).acos() - (r * r - 1.0).sqrt();
    let strip = 2.0 * r - (2.0 * PI * r * r / 4.0 - half_lens);
    close(
        volume(&k, out),
        20.0 * 10.0 * 10.0 + 2.0 * 5.0 * 10.0 - (17.0 * 7.0 + strip) * 8.5,
        1e-9,
    );
    // Sharp, the walls can't be made: an error.
    let err = k
        .shell_with(
            b,
            &ShellSpec {
                sharp: true,
                ..spec
            },
        )
        .unwrap_err();
    assert!(err.to_string().contains("reduce the thickness"), "{err}");
}

/// OCCT's compshape A1 (its reference volume is the input's own): a 100-cube with a 10 × 10 lip
/// along one edge, a face square to the lip open, shelled inward 10 thick with rounded joins. The
/// lip lies within 10 of its own faces and the tube round its concave edge only touches the
/// cavity, so the cavity is 90 × 80 × 80: 1010000 − 576000 = 434000.
#[test]
fn rounded_shell_of_a_cube_with_a_lip() {
    let mut k = NativeKernel::new();
    let b = prism(
        &mut k,
        polygon(&[
            (0.0, 0.0),
            (90.0, 0.0),
            (90.0, -10.0),
            (100.0, -10.0),
            (100.0, 100.0),
            (0.0, 100.0),
        ]),
        vec![],
        100.0,
    );
    let open = face_at(&k, b, -Vector3::z(), 0.0);
    let spec = ShellSpec {
        remove: vec![open],
        thickness: 10.0,
        outward: false,
        hollow: false,
        sharp: false,
    };
    let out = one(k.shell_with(b, &spec).unwrap());
    close(volume(&k, out), 1_010_000.0 - 90.0 * 80.0 * 80.0, 1e-9);
}

/// A cube's top replaced by a cylinder across it whose axis lies above the top: the side
/// planes cut the cylinder twice, and the domain (the upper half) picks the arcs. The result
/// is the 10 × 10 × 12 block under a half round of radius 5 (SMLib's local operation 3).
#[test]
fn replace_face_takes_the_arc_in_the_domain() {
    let mut k = NativeKernel::new();
    let b = prism(
        &mut k,
        polygon(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]),
        vec![],
        10.0,
    );
    let top = face_at(&k, b, Vector3::z(), 10.0);
    // u from −x round to +z: the upper half for u in [0, π].
    let f = Frame::from_xz(Point3::new(5.0, 0.0, 12.0), -Vector3::x(), Vector3::y());
    let upper = FaceReplacement {
        face: top,
        surface: Surface::Cylinder { f, r: 5.0 },
        domain: Some(((0.0, PI), (0.0, 10.0))),
    };
    let out = one(k.replace_faces(b, std::slice::from_ref(&upper)).unwrap());
    close(volume(&k, out), 1200.0 + 125.0 * PI, 1e-9);
    // Without a domain, the crossings nearest the old edges: the lower half.
    let lower = one(k
        .replace_faces(
            b,
            &[FaceReplacement {
                domain: None,
                ..upper
            }],
        )
        .unwrap());
    close(volume(&k, lower), 1200.0 - 125.0 * PI, 1e-9);
}

/// A cylinder whose side is given its exact B-spline form (closed round, not periodic: its seam
/// on the spline's boundary), then offset sharp: the seam moves as the spline's own iso line
/// (an exact iso curve of the surface, not an iso line of an analytic one).
#[test]
fn seam_on_a_closed_spline_moves() {
    let mut k = NativeKernel::new();
    let b = prism(&mut k, circle((0.0, 0.0), 5.0), vec![], 10.0);
    let side = k
        .faces(b)
        .unwrap()
        .into_iter()
        .find(|f| f.kind == SurfaceKind::Cylinder)
        .expect("no side")
        .id;
    let surf = {
        let body = k.get(b).unwrap();
        let f = &body.faces[side.0 as usize];
        Surface::Nurbs(std::sync::Arc::new(
            f.surf.to_nurbs((0.0, 2.0 * PI), (-1.0, 11.0)),
        ))
    };
    let spline = one(k
        .replace_faces(
            b,
            &[FaceReplacement {
                face: side,
                surface: surf,
                domain: None,
            }],
        )
        .unwrap());
    close(volume(&k, spline), PI * 25.0 * 10.0, 1e-6);
    let out = one(k
        .offset(
            spline,
            &OffsetSpec {
                distance: 1.0,
                faces: vec![],
                sharp: true,
            },
        )
        .unwrap());
    close(volume(&k, out), PI * 36.0 * 12.0, 1e-5);
}

/// OCCT's offset of an L-shaped prism (a 2 × 2 square less its 1 × 1 corner, 1 deep) by 1
/// with the inner riser moved 3: the riser passes the outer side, so the step between them
/// vanishes and so does the outer side (the riser moved past it): the 5 × 4 rectangle, 3 deep.
#[test]
fn sharp_offset_where_faces_vanish() {
    let mut k = NativeKernel::new();
    let b = prism(
        &mut k,
        polygon(&[
            (0.0, 0.0),
            (2.0, 0.0),
            (2.0, 1.0),
            (1.0, 1.0),
            (1.0, 2.0),
            (0.0, 2.0),
        ]),
        vec![],
        1.0,
    );
    let riser = k
        .faces(b)
        .unwrap()
        .into_iter()
        .find(|f| f.plane.is_some_and(|p| p.normal.x > 0.5) && (f.center.x - 1.0).abs() < 1e-9)
        .expect("no riser")
        .id;
    let out = one(k
        .offset(
            b,
            &OffsetSpec {
                distance: 1.0,
                faces: vec![(riser, 3.0)],
                sharp: true,
            },
        )
        .unwrap());
    close(volume(&k, out), 5.0 * 4.0 * 3.0, 1e-9);
}

/// Every hole of every face lies inside its face's outer loop in (u, v).
fn holes_inside(b: &cadrs_kernel::native::topo::Body) -> std::result::Result<(), String> {
    use cadrs_kernel::native::topo::co_range;
    for (fi, f) in b.faces.iter().enumerate() {
        let polys: Vec<Vec<(f64, f64)>> = f
            .loops
            .iter()
            .map(|l| {
                l.iter()
                    .flat_map(|c| {
                        let e = &b.edges[c.edge];
                        let (t0, t1) = co_range(e, c.fwd);
                        (0..16).map(move |i| {
                            let q = c.pc.point(t0 + (t1 - t0) * i as f64 / 16.0);
                            (q.x, q.y)
                        })
                    })
                    .collect()
            })
            .collect();
        let inside = |p: (f64, f64), poly: &[(f64, f64)]| {
            let mut w = false;
            for i in 0..poly.len() {
                let (a, c) = (poly[i], poly[(i + 1) % poly.len()]);
                if (a.1 > p.1) != (c.1 > p.1) && p.0 < a.0 + (p.1 - a.1) * (c.0 - a.0) / (c.1 - a.1)
                {
                    w = !w;
                }
            }
            w
        };
        for (li, h) in polys.iter().enumerate().skip(1) {
            if h.iter().any(|p| !inside(*p, &polys[0])) {
                return Err(format!("face {fi}: hole {li} isn't inside the outer loop"));
            }
        }
    }
    Ok(())
}

/// OCCT's draft angle J4: a box fused onto a cylinder's side, its face drafted; the hole the
/// box leaves in the cylinder grows across the cylinder's seam, which has to move round out
/// of it (the hole may not straddle the seam).
#[test]
fn draft_moves_a_seam_out_of_a_hole() {
    use cadrs_kernel::native::draw;
    let mut k = NativeKernel::new();
    let p = k.plane(0.0, 0.0, 0.0, 1.0, 0.0, 0.0);
    let p = k.pcylinder_on(&p, 10.0, 20.0).unwrap();
    let bx = k.box_at(5.0, 0.5, 0.0, 10.0, 5.0, 20.0).unwrap();
    let f = k.bfuse(&p, &bx).unwrap();
    let faces = k.nexplode_faces(&f).unwrap();
    let draw::Shape::Body(r) = k
        .depouille(
            &f,
            [0.0, 0.0, 1.0],
            &[(&faces[3], 5.0, [0.0, 0.0, 20.0], [0.0, 0.0, 1.0])],
        )
        .unwrap()
    else {
        panic!("not a body")
    };
    holes_inside(k.get(r).unwrap()).unwrap();
}

/// The loop of face `fi` of SMLib's off_extrusion (its end faces: two B-splines, an arc and a
/// line) as a wire offset takes it: each edge in the loop's direction, a reversed one as a
/// decreasing parameter range, in the face's plane (or as it is in space).
fn extrusion_loop(
    fi: usize,
    in_plane: bool,
) -> Vec<(cadrs_kernel::native::curve::Curve, f64, f64)> {
    extrusion_loop_n(fi, 0, in_plane)
}

/// [`extrusion_loop`] of the face's loop `li`.
fn extrusion_loop_n(
    fi: usize,
    li: usize,
    in_plane: bool,
) -> Vec<(cadrs_kernel::native::curve::Curve, f64, f64)> {
    use cadrs_kernel::native::math::Affine;
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/smlib/offset/off_extrusion.smb"
    ))
    .unwrap();
    let b = cadrs_kernel::native::smb::read_brep(&bytes).unwrap().body;
    let f = &b.faces[fi];
    let Surface::Plane { f: fr } = &f.surf else {
        panic!("face {fi} isn't planar")
    };
    let fr = Frame::from_z(fr.o, fr.x.cross(&fr.y));
    let m = nalgebra::Matrix3::from_rows(&[fr.x.transpose(), fr.y.transpose(), fr.z.transpose()]);
    let to_plane = Affine {
        m,
        t: -(m * fr.o.coords),
    };
    f.loops[li]
        .iter()
        .map(|c| {
            let e = &b.edges[c.edge];
            let curve = if in_plane {
                e.curve.transformed(&to_plane)
            } else {
                e.curve.clone()
            };
            if c.fwd {
                (curve, e.t[0], e.t[1])
            } else {
                (curve, e.t[1], e.t[0])
            }
        })
        .collect()
}

/// The planar wire offset of off_extrusion's end loop with its reversed edges given as
/// decreasing parameter ranges: offset outward and inward (it panicked in a closest-point
/// search over a backwards range). A side face's loop, square to the plane z = 0 the offset
/// works in, flattens to edges of no length: an error, not a panic.
#[test]
fn wire_offset_takes_reversed_edges() {
    use cadrs_kernel::native::curve::Curve;
    use cadrs_kernel::native::offset::wire::offset_wire;
    let wire = extrusion_loop(0, true);
    assert!(
        wire.iter().any(|e| matches!(e.0, Curve::Nurbs(_))) && wire.iter().any(|e| e.1 > e.2),
        "a spline and a reversed edge"
    );
    for d in [0.3, -0.3, 1.0] {
        let loops = offset_wire(&wire, d).unwrap_or_else(|e| panic!("offset by {d}: {e:?}"));
        assert_eq!(loops.len(), 1, "offset by {d}");
    }
    // Faces 3 and up are the extrusion's flat sides.
    assert!(offset_wire(&extrusion_loop(3, false), 0.3).is_err());
}

/// Offsets of off_extrusion's end loop past the radius of its concave B-spline (about 1.1):
/// that edge's offset folds into a swallowtail, whose two cuts meet at one point. They stayed
/// two cuts (they were merged as one, and the loop came out empty from 1.8 to 2.3 outward): one
/// loop at every distance, its area growing with the distance.
#[test]
fn wire_offset_trims_a_swallowtail() {
    use cadrs_kernel::native::offset::wire::offset_wire;
    let wire = extrusion_loop(0, true);
    let area = |d: f64| -> f64 {
        let loops = offset_wire(&wire, d).unwrap_or_else(|e| panic!("offset by {d}: {e:?}"));
        assert_eq!(loops.len(), 1, "offset by {d}");
        let mut a = 0.0;
        for p in &loops[0] {
            for k in 0..200 {
                let (x, y) = (
                    p.c.point(p.t0 + (p.t1 - p.t0) * k as f64 / 200.0),
                    p.c.point(p.t0 + (p.t1 - p.t0) * (k + 1) as f64 / 200.0),
                );
                a += 0.5 * (x.x * y.y - y.x * x.y);
            }
        }
        a.abs()
    };
    let ds = [-3.0, -2.5, -2.0, -1.5, 1.5, 1.8, 2.0, 2.2, 2.3, 2.5];
    let areas: Vec<f64> = ds.iter().map(|&d| area(d)).collect();
    assert!(areas.windows(2).all(|w| w[0] < w[1]), "{areas:?}");
}

/// OCCT bug 26233: a whole sphere's face thickened by 10 outward: the solid between radii 100
/// and 110, its outer sphere one shell and the inner one a void (the face has no boundary to
/// join them across).
#[test]
fn thickened_sphere_face_has_a_void() {
    use cadrs_kernel::native::draw;
    use cadrs_kernel::native::topo::CheckLevel;
    let mut k = NativeKernel::new();
    let s = k.psphere(100.0).unwrap();
    let faces = k.explode_faces(&s).unwrap();
    let draw::Shape::Body(r) = k.thickshell(&faces[0], 10.0).unwrap() else {
        panic!("not a body")
    };
    let b = k.get(r).unwrap();
    assert_eq!((b.solids.len(), b.shells.len(), b.faces.len()), (1, 2, 2));
    assert_eq!(b.check_with(CheckLevel::Full), Vec::<String>::new());
    close(
        volume(&k, r),
        4.0 / 3.0 * PI * (110f64.powi(3) - 100f64.powi(3)),
        1e-6,
    );
}

/// The slab of a B-spline face closing to a cone-like apex (SMLib's boolean_trees_result, face
/// 11: its normals at the apex turn round a cone about −y): the apex offsets to a closed curve,
/// the offset face's edge there, with the fan of normals between it and the apex a side. The
/// slab left the point out, and the offset face's loop didn't close.
#[test]
fn slab_of_a_face_with_a_cone_like_apex() {
    use cadrs_kernel::native::topo::CheckLevel;
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/smlib/offset/boolean_trees_result.smb"
    ))
    .unwrap();
    let b = cadrs_kernel::native::smb::read_brep(&bytes).unwrap().body;
    let fi = 11;
    let apex = cadrs_kernel::native::math::P3::new(12.0, -8.0, 0.0);
    assert!(
        b.faces[fi]
            .loops
            .iter()
            .flatten()
            .any(|c| b.edges[c.edge].degenerate
                && (b.verts[b.edges[c.edge].v[0]].p - apex).norm() < 1e-9),
        "face {fi} has no pole at the apex"
    );
    let slab = cadrs_kernel::native::offset::slab::face_slab(&b, fi, 0.3).unwrap();
    assert_eq!(slab.check_with(CheckLevel::Full), Vec::<String>::new());
    // The fan: a closed edge 0.3 from the apex all round.
    let fan = slab.edges.iter().any(|e| {
        !e.degenerate
            && e.v[0] == e.v[1]
            && (0..8).all(|i| {
                ((e.point(e.t[0] + (e.t[1] - e.t[0]) * i as f64 / 8.0) - apex).norm() - 0.3).abs()
                    < 1e-6
            })
    });
    assert!(fan, "no offset curve round the apex");
}

/// The inward slab of OffsetBoxSphOnCorner's corner spline (a sphere of radius about 37.3 as a
/// B-spline closed in v, poles at u = 0 and 1, its loop running round the pole at u = 1 along
/// its seam): the seam's offset is the offset surface's own seam, so the offset face's loop goes
/// round that pole too, and its area is the face's scaled by ((R − t)/R)². A fitted seam strayed
/// off the offset surface's seam near the pole, and the offset face left out the ring round it.
#[test]
fn slab_of_a_face_round_a_pole_along_its_seam() {
    use cadrs_kernel::native::mass;
    use cadrs_kernel::native::topo::CheckLevel;
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/smlib/offset/OffsetBoxSphOnCorner.smb"
    ))
    .unwrap();
    let b = cadrs_kernel::native::smb::read_brep(&bytes).unwrap().body;
    let fi = 10;
    // The sphere (read as one, or as the B-spline: its poles at u = 0 and 1 a diameter apart).
    let r = match &b.faces[fi].surf {
        Surface::Sphere { r, .. } => *r,
        s => {
            let (p0, p1) = (s.eval(0.0, 0.3), s.eval(1.0, 0.3));
            let (c, r) = (p0 + (p1 - p0) * 0.5, (p1 - p0).norm() * 0.5);
            assert!(
                (0..=8).all(
                    |i| ((s.eval(0.1 * i as f64 + 0.05, 0.37) - c).norm() - r).abs() < 1e-6 * r
                ),
                "not a sphere"
            );
            r
        }
    };
    let t = 2.54;
    let slab = cadrs_kernel::native::offset::slab::face_slab(&b, fi, -t).unwrap();
    assert_eq!(slab.check_with(CheckLevel::Full), Vec::<String>::new());
    let want = mass::face_area(&b, fi).0 * ((r - t) / r).powi(2);
    let got = (0..slab.faces.len())
        .map(|f| mass::face_area(&slab, f).0)
        .fold(f64::NAN, |best, a| {
            if (a - want).abs() < (best - want).abs() || best.is_nan() {
                a
            } else {
                best
            }
        });
    assert!(
        (got - want).abs() < 1e-4 * want,
        "offset face {got}, want {want}"
    );
}

/// A sourced rectangle (each side a curve with its own id), for naming.
fn named_rect(w: f64, h: f64) -> Loop {
    let c = [
        Point2::new(0.0, 0.0),
        Point2::new(w, 0.0),
        Point2::new(w, h),
        Point2::new(0.0, h),
    ];
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

/// Persistent names through offsets and shells: a 10 × 20 × 30 box offset by 2 rounded keeps
/// each face's name once (its offset), and names its 12 edge tubes and 8 corner balls from the
/// edges and vertices; offset sharp, the same six names and nothing new. Shelled 1 thick with
/// its top open, the top's name is on the walls' rim (what is left of the top) and each other
/// face's name on two faces (its outer side and the inner wall offset from it). (The offset's history used to say face i came from
/// face i, whatever the union of its pieces had numbered them.)
#[test]
fn names_survive_offsets_and_shells() {
    use cadrs_kernel::naming::{self, FaceName, FaceOrigin};
    let op = |n: u128| uuid::Uuid::from_u128(n);
    let mut k = NativeKernel::new();
    let r = k
        .extrude(
            &Profile::new(
                Plane::top(),
                vec![Region {
                    outer: named_rect(10.0, 20.0),
                    holes: vec![],
                    source: None,
                }],
            ),
            Extent::Blind(30.0),
        )
        .unwrap();
    let b = one(r.clone());
    let names = naming::name_body(&k, b, op(1), &r.history, &[]).unwrap();
    assert!(names.faces.iter().all(|n| n.is_stable()));
    let check = |k: &NativeKernel,
                 r: &OpResult,
                 edges: usize,
                 verts: usize,
                 twice: bool,
                 rim: Option<FaceName>| {
        let out = r.bodies[0];
        let got = naming::name_body(k, out, op(2), &r.history, &[(b, &names)]).unwrap();
        assert!(got.faces.iter().all(|n| n.is_stable()), "{:?}", got.faces);
        for n in &names.faces {
            // (The open face's name stays on the walls' rim, what is left of it.)
            let want = if Some(*n) == rim {
                1
            } else if twice {
                2
            } else {
                1
            };
            // (Two faces from one are its pieces: split 1 and 2.)
            assert_eq!(
                got.faces.iter().filter(|g| g.base() == n.base()).count(),
                want,
                "{n:?} in {:?}",
                got.faces
            );
        }
        let count =
            |f: &dyn Fn(&FaceOrigin) -> bool| got.faces.iter().filter(|n| f(&n.origin)).count();
        assert_eq!(count(&|o| matches!(o, FaceOrigin::FromEdge { .. })), edges);
        assert_eq!(
            count(&|o| matches!(o, FaceOrigin::FromVertex { .. })),
            verts
        );
    };
    let rounded = k
        .offset(
            b,
            &OffsetSpec {
                distance: 2.0,
                faces: vec![],
                sharp: false,
            },
        )
        .unwrap();
    check(&k, &rounded, 12, 8, false, None);
    let sharp = k
        .offset(
            b,
            &OffsetSpec {
                distance: 2.0,
                faces: vec![],
                sharp: true,
            },
        )
        .unwrap();
    check(&k, &sharp, 0, 0, false, None);
    let top = face_at(&k, b, Vector3::z(), 30.0);
    let shelled = k
        .shell_with(
            b,
            &ShellSpec {
                remove: vec![top],
                thickness: 1.0,
                outward: false,
                hollow: false,
                sharp: false,
            },
        )
        .unwrap();
    let top_name = names.faces[top.0 as usize];
    check(&k, &shelled, 0, 0, true, Some(top_name));
}

/// SMLib's offset demo 6, checked against the exact erosion (SMLib's own volume is its
/// approximation): off_extrusion (a profile 10 deep: an outer loop of B-splines, an arc and a
/// line, a square hole and a round one) inset 0.3 with its end face 0 kept is the profile eroded
/// by 0.3, 9.7 deep. The eroded section from the 2D offsets of its loops (the outer one in, the
/// holes out), × 9.7, is the volume to the inset spline's fit.
#[test]
fn rounded_inset_of_an_extrusion_is_its_eroded_profile() {
    use cadrs_kernel::native::offset::wire::offset_wire;
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/smlib/offset/off_extrusion.smb"
    ))
    .unwrap();
    let b = cadrs_kernel::native::smb::read_brep(&bytes).unwrap().body;
    let area = |loops: &[Vec<cadrs_kernel::native::offset::wire::Piece2>]| -> f64 {
        let mut a = 0.0;
        for p in loops.iter().flatten() {
            for k in 0..400 {
                let (x, y) = (
                    p.c.point(p.t0 + (p.t1 - p.t0) * k as f64 / 400.0),
                    p.c.point(p.t0 + (p.t1 - p.t0) * (k + 1) as f64 / 400.0),
                );
                a += 0.5 * (x.x * y.y - y.x * x.y);
            }
        }
        a.abs()
    };
    let n = b.faces[0].loops.len();
    assert_eq!(n, 3);
    let section = area(&offset_wire(&extrusion_loop_n(0, 0, true), -0.3).unwrap())
        - (1..n)
            .map(|li| area(&offset_wire(&extrusion_loop_n(0, li, true), 0.3).unwrap()))
            .sum::<f64>();
    let mut k = NativeKernel::new();
    let id = k.insert(b);
    let out = one(k
        .offset(
            id,
            &OffsetSpec {
                distance: -0.3,
                faces: vec![(FaceId(0), 0.0)],
                sharp: false,
            },
        )
        .unwrap());
    close(volume(&k, out), section * 9.7, 1e-5);
}

/// OCCT's offset multi C5: a 10 × 1 plate with two fins (x 2–3 and 7–8, up to z = 7, their
/// sides split at z = 4), 10 deep; the upper halves of the fins' inner faces offset by 3, the
/// rest kept. They cross over (the slot is 4 wide), so the slot's upper half fills: a bridge
/// over a 4 × 3 hole, section 10 + 12 + 12 = 34, volume 340. The moved faces are deleted, and the
/// bridge's underside is generated from the two edges where the moved halves met the kept ones.
/// (Neighbours along one plane moved apart, and walls that cross, were refused: "an offset that
/// splits the profile".)
#[test]
fn sharp_offset_that_closes_a_slot() {
    use cadrs_kernel::native::draw::Shape;
    let mut k = NativeKernel::new();
    let pts: Vec<[f64; 3]> = [
        (0, 0),
        (10, 0),
        (10, 1),
        (8, 1),
        (8, 4),
        (8, 7),
        (7, 7),
        (7, 4),
        (7, 1),
        (3, 1),
        (3, 4),
        (3, 7),
        (2, 7),
        (2, 4),
        (2, 1),
        (0, 1),
        (0, 0),
    ]
    .iter()
    .map(|&(x, z)| [x as f64, 0.0, z as f64])
    .collect();
    let p = k.polyline(&pts).unwrap();
    let f = k.mkplane(&p).unwrap();
    let s = k.prism(&f, 0.0, 10.0, 0.0).unwrap();
    let faces = k.explode_faces(&s).unwrap();
    let Shape::Body(input) = s else {
        panic!("not a body")
    };
    let inner = |k: &NativeKernel, x: f64| {
        faces.iter().position(|f| matches!(f, Shape::BodyFace(_, id) if { let c = k.faces(input).unwrap()[id.0 as usize].center; (c.x - x).abs() < 1e-9 && c.z > 5.0 })).unwrap()
    };
    let (right, left) = (inner(&k, 7.0), inner(&k, 3.0));
    let r = k
        .offsetperform_on(&s, 0.0, &[(&faces[right], 3.0), (&faces[left], 3.0)], false)
        .unwrap();
    let Shape::Body(out) = r else {
        panic!("not a body")
    };
    close(volume(&k, out), 340.0, 1e-9);
    let h = k.savehistory().unwrap();
    k.isdeleted(&h, &faces[right], true).unwrap();
    k.isdeleted(&h, &faces[left], true).unwrap();
    // The step edges: the moved faces' edges at z = 4 along y.
    let step = |k: &NativeKernel, fi: usize| {
        k.explode_edges(&faces[fi]).unwrap().into_iter().find(|e| matches!(e, Shape::BodyEdge(b, id) if { let ed = &k.get(*b).unwrap().edges[id.0 as usize]; (ed.point(ed.t[0]).z - 4.0).abs() < 1e-9 && (ed.point(ed.t[1]).z - 4.0).abs() < 1e-9 })).unwrap()
    };
    for fi in [right, left] {
        let Shape::Compound(g) = k.generated(&h, &step(&k, fi)).unwrap() else {
            panic!("not a compound")
        };
        assert_eq!(g.len(), 1);
    }
}

/// A drafted body: valid under the full check, with every vertex and edge at a tight tolerance
/// (a vertex the edges miss gets a tolerance that covers the miss, which the check accepts),
/// and its area OCCT's (or ours, where OCCT's is wrong) to 1e-3.
fn drafted(k: &mut NativeKernel, r: &cadrs_kernel::native::draw::Shape, area: f64) {
    drafted_at(k, r, area, cadrs_kernel::native::topo::CheckLevel::Full);
}

/// [`drafted`] checked at `level`.
fn drafted_at(
    k: &mut NativeKernel,
    r: &cadrs_kernel::native::draw::Shape,
    area: f64,
    level: cadrs_kernel::native::topo::CheckLevel,
) {
    let cadrs_kernel::native::draw::Shape::Body(id) = r else {
        panic!("not a body")
    };
    let b = k.get(*id).unwrap();
    assert_eq!(b.check_with(level), Vec::<String>::new());
    let worst = b
        .verts
        .iter()
        .map(|v| v.tol)
        .chain(b.edges.iter().map(|e| e.tol))
        .fold(0.0, f64::max);
    assert!(worst < 1e-5, "a tolerance of {worst}");
    close(k.sprops(r).unwrap(), area, 1e-3);
}

/// OCCT's draft angle C4: the end plane of a half tube drafted by 1° meets the inner cylinder
/// in a long thin ellipse; the edge is its short arc (sampled evenly in its parameter, the
/// ellipse's nearest point to the old edge's lay on the far side).
#[test]
fn draft_meets_a_cylinder_in_a_thin_ellipse() {
    let mut k = NativeKernel::new();
    let p = k
        .polyline(&[
            [2.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [10.0, 0.0, 20.0],
            [2.0, 0.0, 20.0],
            [2.0, 0.0, 0.0],
        ])
        .unwrap();
    let p = k.mkplane(&p).unwrap();
    let p = k.revol(&p, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 180.0).unwrap();
    let e = k.nexplode_edges(&p).unwrap();
    let p = k.blend_radii(&p, &[(2.0, &e[11]), (1.0, &e[3])]).unwrap();
    let f = k.nexplode_faces(&p).unwrap();
    let r = k
        .depouille(
            &p,
            [0.0, 0.0, 1.0],
            &[
                (&f[7], 2.0, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
                (&f[0], 1.0, [0.0, 0.0, 10.0], [0.0, 0.0, 1.0]),
            ],
        )
        .unwrap();
    drafted(&mut k, &r, 1273.54);
}

/// OCCT's draft angle F6: a revolved tube's bore is a cone whose half-angle is past a right
/// angle (its natural normal points to the axis); drafted by −10°, it widens downwards.
#[test]
fn draft_of_a_bore_cone_past_a_right_angle() {
    let mut k = NativeKernel::new();
    let p = k.plane_x(0.0, 0.0, 0.0, 0.0, 0.0, 1.0, -1.0, 0.0, 0.0);
    let p = k.psphere_zone(Some(&p), 20.0, 0.0, 90.0, 360.0).unwrap();
    let pr = k
        .profile(
            None,
            "O 2.0 0.0 5.0 P 0.0 -1.0 0.0 1.0 0.0 0.0 L 10.0 T 0.0 30.0 T -7.0 0.0",
        )
        .unwrap();
    let pr = k.revol(&pr, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 360.0).unwrap();
    let f = k.bfuse(&p, &pr).unwrap();
    let ff = k.nexplode_faces(&f).unwrap();
    let r = k
        .depouille(
            &f,
            [0.0, 0.0, 1.0],
            &[(&ff[4], -10.0, [0.0, 0.0, 35.0], [0.0, 0.0, 1.0])],
        )
        .unwrap();
    drafted(&mut k, &r, 5966.54);
}

/// OCCT's draft angle F7: the drafted wall of a box on a torus moves its corner past the end
/// of a kept intersection edge (a bounded B-spline), which is recomputed to reach it.
#[test]
fn draft_extends_a_kept_spline_edge() {
    let mut k = NativeKernel::new();
    let p = k.ptorus_a(None, 25.0, 20.0, 90.0).unwrap();
    let bx = k.box_at(10.0, 10.0, 10.0, 18.0, 18.0, 30.0).unwrap();
    let f = k.bfuse(&bx, &p).unwrap();
    let ff = k.nexplode_faces(&f).unwrap();
    let r = k
        .depouille(
            &f,
            [0.0, 0.0, 1.0],
            &[(&ff[2], 5.0, [0.0, 0.0, 40.0], [0.0, 0.0, 1.0])],
        )
        .unwrap();
    drafted(&mut k, &r, 9010.79);
}

/// OCCT's draft angle K8: a drafted wall's foot slides along the torus's equator past the
/// corner where its seams cross; the corner moves along the equator, the seam with it. OCCT's
/// own result is faulty; ours is the drafted prism alone fused with the torus.
#[test]
fn draft_slides_a_torus_seam_corner() {
    let mut k = NativeKernel::new();
    let pt = k.plane(0.0, 0.0, 0.0, 1.0, 0.0, 0.0);
    let pt = k.ptorus_on(&pt, 15.0, 12.0).unwrap();
    let pf = k
        .profile(
            None,
            "O -5.0 0.5 15.0 L 10.0 C 2.0 90.0 L 5.0 D -1.0 0.0 L 14.0 D 0.0 -1.0 L 5.0 C 2.0 90.0",
        )
        .unwrap();
    let pr = k.prism(&pf, 0.0, 0.0, 25.0).unwrap();
    let f = k.bfuse(&pt, &pr).unwrap();
    let ff = k.nexplode_faces(&f).unwrap();
    let r = k
        .depouille(
            &f,
            [0.0, 0.0, 1.0],
            &[(&ff[3], 5.0, [0.0, 0.0, 40.0], [0.0, 0.0, 1.0])],
        )
        .unwrap();
    drafted(&mut k, &r, 7721.62);
}

/// OCCT's draft angle K5: a pipe on a sphere drafted; its cylinder's foot leaves the sphere's
/// seam (the seam's meridian no longer meets it), so the seam moves to the iso line through
/// the corner, the pole's point edge turning with it. OCCT's own result is faulty; ours is the
/// drafted pipe alone fused with the sphere.
#[test]
fn draft_carries_a_sphere_seam_off() {
    let mut k = NativeKernel::new();
    let ps = k.plane_x(10.0, -3.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.2, 1.0);
    let ps = k.psphere_on(&ps, 20.0).unwrap();
    let pf = k
        .profile(
            None,
            "O 5.0 -0.5 10.0 L 10.0 C 5.0 90.0 L 5.0 D -1.0 0.0 L 14.0 D 0.0 -1.0",
        )
        .unwrap();
    let pl = k.polyline(&[[0.0, 0.0, 10.0], [0.0, 0.0, 35.0]]).unwrap();
    let p2 = k.pipe(&pl, &pf).unwrap();
    let f = k.bfuse(&ps, &p2).unwrap();
    let ff = k.nexplode_faces(&f).unwrap();
    let r = k
        .depouille(
            &f,
            [0.0, 0.0, 1.0],
            &[(&ff[1], -5.0, [0.0, 0.0, 35.0], [0.0, 0.0, 1.0])],
        )
        .unwrap();
    drafted(&mut k, &r, 5837.90);
}

/// OCCT's draft angle J9: a drafted pipe on a cone carries the corner on the cone's seam off
/// it; the seam's other end slides round the cone's rim to the iso line through it. (Checked
/// at the fast level: the corner is where the pipe's wall and its round meet tangentially,
/// at the most u their joined boundary reaches, so the seam through it touches the boundary
/// there, which the full check's (u, v) crossing test reports.)
#[test]
fn draft_slides_a_cone_seam_round_its_rim() {
    let mut k = NativeKernel::new();
    let p = k.plane(0.0, 0.0, 0.0, 1.0, 0.0, 0.0);
    let p = k.pcone_on(&p, 15.0, 14.0, 20.0).unwrap();
    let pf = k
        .profile(
            None,
            "O 5.0 0.0 5.0 L 10.0 C 2.0 90.0 L 5.0 D -1.0 0.0 L 14.0 D 0.0 -1.0 L 5.0 C 2.0 90.0",
        )
        .unwrap();
    let pl = k.polyline(&[[0.0, 0.0, 5.0], [0.0, 0.0, 30.0]]).unwrap();
    let p2 = k.pipe(&pl, &pf).unwrap();
    let f = k.bfuse(&p, &p2).unwrap();
    let ff = k.nexplode_faces(&f).unwrap();
    let r = k
        .depouille(
            &f,
            [0.0, 0.0, 1.0],
            &[(&ff[4], 5.0, [0.0, 0.0, 29.0], [0.0, 0.0, 1.0])],
        )
        .unwrap();
    drafted_at(
        &mut k,
        &r,
        3814.99,
        cadrs_kernel::native::topo::CheckLevel::Fast,
    );
}

/// OCCT's draft angle L5: a cylinder of radius 1200 drafted by 1° meets a plane along its axis
/// in a sliver of a hyperbola, which the plane–cone intersection's sampling round the turn
/// steps over; the edge is traced between its moved ends.
#[test]
fn draft_traces_an_edge_on_a_huge_cone() {
    use cadrs_kernel::native::draw::Shape;
    let mut k = NativeKernel::new();
    // A block whose −y side is a cylinder of radius 1200 along z (CFE903_pro16ggt has the like).
    let bx = k.box_at(-75.0, -10.0, 0.0, 100.0, 50.0, 100.0).unwrap();
    let p = k.plane(-21.0, 1204.0, 0.0, 0.0, 0.0, 1.0);
    let c = k.pcylinder_on(&p, 1200.0, 100.0).unwrap();
    let b = k.bcommon(&bx, &c).unwrap();
    let Shape::Body(id) = b else {
        panic!("not a body")
    };
    let faces = k.nexplode_faces(&b).unwrap();
    let cyl = faces.iter().find(|f| matches!(f, Shape::BodyFace(_, i) if matches!(k.get(id).unwrap().faces[i.0 as usize].surf, Surface::Cylinder { .. }))).unwrap().clone();
    let r = k
        .depouille(
            &b,
            [0.0, 0.0, -1.0],
            &[(&cyl, 1.0, [0.0, 0.0, 100.0], [0.0, 0.0, -1.0])],
        )
        .unwrap();
    // The side leans out by tan 1° per unit below z = 100: the block's volume grows by the
    // wedge under the drafted side, ½·100²·tan 1°·(its width along x, 100).
    let Shape::Body(out) = r else {
        panic!("not a body")
    };
    let grown = volume(&k, out) - volume(&k, id);
    let want = 0.5 * 100.0 * 100.0 * (1.0f64).to_radians().tan() * 100.0;
    close(grown.abs(), want, 2e-3);
    let area = k.sprops(&r).unwrap();
    drafted_at(
        &mut k,
        &r,
        area,
        cadrs_kernel::native::topo::CheckLevel::Full,
    );
}

/// OCCT's draft angle K9: on a horn torus, a drafted pipe wall's corner slides along the
/// torus's u-seam past the corner where its v-seam crosses; that corner moves along the u-seam
/// and the v-seam (an iso-v circle) with it. Ours equals the drafted pipe alone fused with the
/// torus.
#[test]
fn draft_slides_a_torus_v_seam() {
    let mut k = NativeKernel::new();
    let pt = k.plane(0.0, 0.0, 0.0, 1.0, 0.0, 0.0);
    let pt = k.ptorus_on(&pt, 15.0, 15.0).unwrap();
    let pf = k
        .profile(
            None,
            "O -8.0 -12.0 15.0 L 5.0 C 2.0 90.0 L 20.0 C 2.0 90.0 L 5.0",
        )
        .unwrap();
    let pl = k.polyline(&[[0.0, 0.0, 15.0], [0.0, 0.0, 45.0]]).unwrap();
    let p2 = k.pipe(&pl, &pf).unwrap();
    let f = k.bfuse(&pt, &p2).unwrap();
    let ff = k.nexplode_faces(&f).unwrap();
    let r = k
        .depouille(
            &f,
            [0.0, 0.0, 1.0],
            &[(&ff[5], 5.0, [0.0, 0.0, 44.0], [0.0, 0.0, 1.0])],
        )
        .unwrap();
    drafted(&mut k, &r, 9932.99);
}

/// OCCT's draft angle K3: a cone on a sphere drafted by 12° widens its foot past the sphere's
/// pole; the pole leaves the sphere's face, whose hole joins its outer loop where the seam
/// crosses it. Ours equals the drafted cone alone fused with the sphere.
#[test]
fn draft_widens_a_hole_over_a_pole() {
    let mut k = NativeKernel::new();
    let p = k.plane_x(0.0, 0.0, 0.0, 0.0, 0.5, -1.0, 1.0, 0.0, 0.0);
    let p = k.psphere_on(&p, 20.0).unwrap();
    let p2 = k.plane_x(0.0, 0.0, 15.0, 0.0, 0.0, 1.0, 0.0, -1.0, 0.0);
    let p2 = k.pcone_on(&p2, 9.0, 7.0, 15.0).unwrap();
    let f = k.bfuse(&p, &p2).unwrap();
    let ff = k.nexplode_faces(&f).unwrap();
    let r = k
        .depouille(
            &f,
            [0.0, 0.0, 1.0],
            &[(&ff[0], 12.0, [0.0, 0.0, 30.0], [0.0, 0.0, 1.0])],
        )
        .unwrap();
    drafted(&mut k, &r, 5536.07);
}

/// OCCT's offset shape_type_i E1: a torus moved in by 1 is the torus of minor radius 4. Its
/// seam's corner sits on both seam circles; placing it on a seam (a circle's parameter taken
/// within a turn, not wrapped to the far end of a huge range, where its angle has lost its
/// digits) keeps it there.
#[test]
fn inset_of_a_torus() {
    use cadrs_kernel::native::draw::Shape;
    let mut k = NativeKernel::new();
    let s = k.ptorus(20.0, 5.0).unwrap();
    let r = k.offsetperform(&s, -1.0, &[], false).unwrap();
    let Shape::Body(out) = r else {
        panic!("not a body")
    };
    close(volume(&k, out), 2.0 * PI * PI * 20.0 * 16.0, 1e-9);
    assert_eq!(
        k.get(out)
            .unwrap()
            .check_with(cadrs_kernel::native::topo::CheckLevel::Full),
        Vec::<String>::new()
    );
}

#[test]
fn sharp_offset_drops_a_slope_its_neighbours_cross_over() {
    // OCCT offset/shape_type_i_c/V7: a 10 × 6 × 10 box less a notch through x 2..8 whose floor
    // (z 5, y 0..2) runs down a short slope (to z 3 at y 3) into its back wall (y 3), offset
    // sharply by 2.5. The moved floor (z 7.5) and wall (y 0.5) meet before the moved slope
    // reaches them: the slope vanishes (it used to stay, inside out, piercing the moved top),
    // leaving a 1 × 3 × 5 slot in the 15 × 11 × 15 block: 2460.
    let mut k = NativeKernel::new();
    let bx = prism(
        &mut k,
        polygon(&[(0.0, 0.0), (10.0, 0.0), (10.0, 6.0), (0.0, 6.0)]),
        vec![],
        10.0,
    );
    let side = Plane {
        origin: Point3::new(2.0, 0.0, 0.0),
        x_dir: Vector3::y_axis(),
        normal: Vector3::x_axis(),
    };
    let notch = polygon(&[(0.0, 5.0), (2.0, 5.0), (3.0, 3.0), (3.0, 10.0), (0.0, 10.0)]);
    let tool = one(k
        .extrude(
            &Profile::new(
                side,
                vec![Region {
                    outer: notch,
                    holes: vec![],
                    source: None,
                }],
            ),
            Extent::Blind(6.0),
        )
        .unwrap());
    let cut = one(k.boolean(BoolOp::Subtract, bx, &[tool]).unwrap());
    let off = one(k
        .offset(
            cut,
            &OffsetSpec {
                distance: 2.5,
                faces: vec![],
                sharp: true,
            },
        )
        .unwrap());
    let issues = k
        .get(off)
        .unwrap()
        .check_with(cadrs_kernel::native::topo::CheckLevel::Full);
    assert!(issues.is_empty(), "{issues:?}");
    close(volume(&k, off), 2460.0, 1e-9);
}

/// Move face refuses moves that would wreck the body (main's `check_face_moves` guarded OCCT's
/// offset, which crashed on them): a hole's wall moved onto or past its axis, a hole grown out
/// through the block's sides (leaving it in pieces). A side moved into the hole opens it, as the
/// body less the slab the side sweeps. Moves that keep the topology still work, to closed forms.
#[test]
fn move_face_refuses_topology_changes() {
    let mut k = NativeKernel::new();
    let b = prism(
        &mut k,
        polygon(&[(-10.0, -10.0), (10.0, -10.0), (10.0, 10.0), (-10.0, 10.0)]),
        vec![circle((0.0, 0.0), 3.0)],
        10.0,
    );
    let wall = k
        .faces(b)
        .unwrap()
        .into_iter()
        .find(|f| f.kind == SurfaceKind::Cylinder)
        .unwrap()
        .id;
    let side = face_at(&k, b, Vector3::x(), 10.0);
    let moved = |k: &mut NativeKernel, f: FaceId, d: f64| {
        k.offset(
            b,
            &OffsetSpec {
                distance: 0.0,
                faces: vec![(f, d)],
                sharp: true,
            },
        )
    };
    // The hole's radius 3 → 1 and → 8; the side in by 5.
    for (f, d, want) in [
        (wall, 2.0, 4000.0 - PI * 10.0),
        (wall, -5.0, 4000.0 - PI * 640.0),
        (side, -5.0, 3000.0 - PI * 90.0),
    ] {
        let out = one(moved(&mut k, f, d).unwrap());
        close(volume(&k, out), want, 1e-9);
    }
    // The side in by 8 runs into the hole and opens it (the side square to its neighbours, the
    // body less the slab it sweeps): the half plate x ≤ 2 less the hole's part there.
    let opened = one(moved(&mut k, side, -8.0).unwrap());
    let seg = 9.0 * (2.0f64 / 3.0).acos() - 2.0 * 5f64.sqrt();
    close(volume(&k, opened), 2400.0 - 10.0 * (9.0 * PI - seg), 1e-9);
    // The hole's radius to 0 or past it, or out past the plate's sides (the plate in four).
    for (f, d) in [(wall, 3.0), (wall, 4.0), (wall, -8.0)] {
        let r = moved(&mut k, f, d);
        assert!(
            matches!(r, Err(KernelError::InvalidParameter(_))),
            "{f:?} by {d}: {r:?}"
        );
    }
}

/// Move face, Translate: a 10 × 10 × 10 cube's top lifted by 3 (the sides follow up) and a side
/// pushed in by 2 along the x axis; volumes and the moved face's place exact.
#[test]
fn move_face_translates_and_neighbours_follow() {
    let mut k = NativeKernel::new();
    let cube = prism(
        &mut k,
        polygon(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]),
        vec![],
        10.0,
    );
    let top = face_at(&k, cube, Vector3::z(), 10.0);
    let up = one(k
        .move_faces(
            cube,
            &[top],
            &Motion::translation(Vector3::new(0.0, 0.0, 3.0)),
        )
        .unwrap());
    close(volume(&k, up), 1300.0, 1e-9);
    assert!(face_at(&k, up, Vector3::z(), 13.0).0 == top.0);
    // Moved within its own plane, the face (and the body) stays as it was.
    let slid = one(k
        .move_faces(
            cube,
            &[top],
            &Motion::translation(Vector3::new(4.0, 0.0, 0.0)),
        )
        .unwrap());
    close(volume(&k, slid), 1000.0, 1e-9);
    let side = face_at(&k, cube, Vector3::x(), 10.0);
    let thin = one(k
        .move_faces(
            cube,
            &[side],
            &Motion::translation(Vector3::new(-2.0, 0.0, 0.0)),
        )
        .unwrap());
    close(volume(&k, thin), 800.0, 1e-9);
    assert!(
        k.get(thin)
            .unwrap()
            .check_with(cadrs_kernel::native::CheckLevel::Full)
            .is_empty()
    );
}

/// Move face, Rotate: the cube's top turned 10° about its edge over the y axis: the body is the
/// cube with a wedge of 10 × 10 × 10·tan 10° (half its box) more.
#[test]
fn move_face_rotates_about_an_edge() {
    let mut k = NativeKernel::new();
    let cube = prism(
        &mut k,
        polygon(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]),
        vec![],
        10.0,
    );
    let top = face_at(&k, cube, Vector3::z(), 10.0);
    // About the top's edge x = 0, z = 10 (counter-clockwise seen from +y lowers +x: turn by −10°
    // to raise it).
    let axis = Axis {
        origin: Point3::new(0.0, 0.0, 10.0),
        dir: nalgebra::Unit::new_normalize(Vector3::y()),
    };
    let tilted = one(k
        .move_faces(cube, &[top], &Motion::rotation(&axis, -10f64.to_radians()))
        .unwrap());
    close(
        volume(&k, tilted),
        1000.0 + 0.5 * 10.0 * 10.0 * 10.0 * 10f64.to_radians().tan(),
        1e-9,
    );
    assert!(
        k.get(tilted)
            .unwrap()
            .check_with(cadrs_kernel::native::CheckLevel::Full)
            .is_empty()
    );
}

/// Move face of a boss: its wall and top translated across its base, the base's hole loop
/// following; moved off the base's edge (the wall would run into the base's sides) it is
/// refused with `InvalidParameter`, and a reflection is refused outright.
#[test]
fn move_face_moves_a_boss_and_refuses_topology_changes() {
    let mut k = NativeKernel::new();
    let base = prism(
        &mut k,
        polygon(&[(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)]),
        vec![],
        5.0,
    );
    let boss = one(k
        .extrude(
            &Profile::new(
                Plane {
                    origin: Point3::new(0.0, 0.0, 5.0),
                    ..Plane::top()
                },
                vec![Region {
                    outer: circle((10.0, 10.0), 3.0),
                    holes: vec![],
                    source: None,
                }],
            ),
            Extent::Blind(4.0),
        )
        .unwrap());
    let part = one(k.boolean(BoolOp::Union, base, &[boss]).unwrap());
    let v0 = volume(&k, part);
    let faces: Vec<FaceId> = k
        .faces(part)
        .unwrap()
        .into_iter()
        .filter(|f| f.center.z > 5.0 + 1e-6)
        .map(|f| f.id)
        .collect();
    assert_eq!(faces.len(), 2, "the boss's wall and top");
    let moved = one(k
        .move_faces(
            part,
            &faces,
            &Motion::translation(Vector3::new(4.0, -2.0, 0.0)),
        )
        .unwrap());
    close(volume(&k, moved), v0, 1e-7);
    let top = k
        .faces(moved)
        .unwrap()
        .into_iter()
        .find(|f| (f.center.z - 9.0).abs() < 1e-9)
        .expect("the boss's top");
    assert!(
        (top.center - Point3::new(14.0, 8.0, 9.0)).norm() < 1e-6,
        "{:?}",
        top.center
    );
    assert!(
        k.get(moved)
            .unwrap()
            .check_with(cadrs_kernel::native::CheckLevel::Full)
            .is_empty()
    );
    match k.move_faces(
        part,
        &faces,
        &Motion::translation(Vector3::new(9.0, 0.0, 0.0)),
    ) {
        Err(KernelError::InvalidParameter(_)) => {}
        r => panic!("a boss moved off its base: {r:?}"),
    }
    let mirror = Motion::reflection(Point3::new(10.0, 10.0, 0.0), Vector3::x_axis());
    assert!(matches!(
        k.move_faces(part, &faces, &mirror),
        Err(KernelError::InvalidParameter(_))
    ));
}

/// Sharp offsets of a whole sphere and of a sphere cut by a plane (OCCT's offset
/// shape_type_i_e3/e4/d4): the sphere's seam keeps its two sides when the sphere grows or shrinks
/// (its p-curves were projected onto one side, leaving a loop that encloses nothing).
#[test]
fn sharp_offset_of_a_sphere_keeps_its_seam() {
    use cadrs_kernel::native::draw::Shape;
    let mut k = NativeKernel::new();
    let Shape::Body(ball) = k.psphere(15.0).unwrap() else {
        panic!("a sphere body")
    };
    for d in [1.0, -1.0] {
        let r = one(k
            .offset(
                ball,
                &OffsetSpec {
                    distance: d,
                    faces: vec![],
                    sharp: true,
                },
            )
            .unwrap());
        close(volume(&k, r), 4.0 / 3.0 * PI * (15.0f64 + d).powi(3), 1e-6);
        assert!(
            k.get(r)
                .unwrap()
                .check_with(cadrs_kernel::native::CheckLevel::Full)
                .is_empty()
        );
    }
    // Below latitude 60°: a sphere and a plane; grown by 1 the cap's plane rises by 1.
    let Shape::Body(zone) = k.psphere_zone(None, 15.0, -90.0, 60.0, 360.0).unwrap() else {
        panic!("a sphere body")
    };
    let r = one(k
        .offset(
            zone,
            &OffsetSpec {
                distance: 1.0,
                faces: vec![],
                sharp: true,
            },
        )
        .unwrap());
    let (rr, h) = (16.0f64, 15.0 * 60f64.to_radians().sin() + 1.0);
    // The ball of radius 16 less its cap above z = h.
    let cap = PI * (rr - h).powi(2) * (3.0 * rr - (rr - h)) / 3.0;
    close(volume(&k, r), 4.0 / 3.0 * PI * rr.powi(3) - cap, 1e-6);
}

/// The "extrude test" document: a 30 mm cube with every edge filleted 5, shelled 5 outward with
/// its top's flat removed. The cube is a 20 mm cube grown by a ball of 5, so its outward walls
/// are the 20 mm cube grown by 10 less it, less the opening: the top's 20 × 20 flat carried
/// straight up through the 5 mm wall (the top's rounds are tangent to it, so the walls round
/// over them as everywhere else, as Onshape's do; they were cut off flat at the top's plane).
#[test]
fn outward_shell_of_a_rounded_cube_keeps_its_rounds_round_the_opening() {
    let mut k = NativeKernel::new();
    let cube = prism(
        &mut k,
        polygon(&[(-15.0, -15.0), (15.0, -15.0), (15.0, 15.0), (-15.0, 15.0)]),
        vec![],
        30.0,
    );
    let edges: Vec<EdgeId> = k.edges(cube).unwrap().iter().map(|e| e.id).collect();
    let b = one(k.fillet(cube, &edges, 5.0).unwrap());
    let top = face_at(&k, b, Vector3::z(), 30.0);
    let spec = ShellSpec {
        remove: vec![top],
        thickness: 5.0,
        outward: true,
        hollow: false,
        sharp: false,
    };
    let out = one(k.shell_with(b, &spec).unwrap());
    // A cube of side a grown by a ball of r: a³ + 6a²r + 3πar² + 4πr³/3.
    let grown = |a: f64, r: f64| {
        a.powi(3) + 6.0 * a * a * r + 3.0 * PI * a * r * r + 4.0 / 3.0 * PI * r.powi(3)
    };
    close(
        volume(&k, out),
        grown(20.0, 10.0) - grown(20.0, 5.0) - 20.0 * 20.0 * 5.0,
        1e-6,
    );
    let bb = k.bounding_box(out).unwrap();
    assert!((bb.max.z - 35.0).abs() < 1e-6, "top at {}", bb.max.z);
    // No flat at the top's plane facing up (the rounds cut off there).
    assert!(!k.faces(out).unwrap().iter().any(|f| {
        f.plane.is_some_and(|p| p.normal.z > 1.0 - 1e-9) && (f.center.z - 30.0).abs() < 1e-6
    }));
}

/// The rounded cube of [`outward_shell_of_a_rounded_cube_keeps_its_rounds_round_the_opening`]
/// with one of its top's rounds removed too, then alone (a curved face, its edges all smooth: "removing a
/// curved face from an outward shell is not supported"): the round's opening is its slab, the
/// quarter of the annulus from 5 to 10 round its axis, 20 long, carried straight out.
#[test]
fn outward_shell_removes_a_round_with_its_flat() {
    let mut k = NativeKernel::new();
    let cube = prism(
        &mut k,
        polygon(&[(-15.0, -15.0), (15.0, -15.0), (15.0, 15.0), (-15.0, 15.0)]),
        vec![],
        30.0,
    );
    let edges: Vec<EdgeId> = k.edges(cube).unwrap().iter().map(|e| e.id).collect();
    let b = one(k.fillet(cube, &edges, 5.0).unwrap());
    let top = face_at(&k, b, Vector3::z(), 30.0);
    // The round along x between the top and the side at y = 15.
    let round = k
        .faces(b)
        .unwrap()
        .into_iter()
        .find(|f| {
            f.kind == SurfaceKind::Cylinder
                && f.center.x.abs() < 1e-6
                && f.center.y > 10.0
                && f.center.z > 25.0
        })
        .expect("no such round")
        .id;
    let spec = ShellSpec {
        remove: vec![top, round],
        thickness: 5.0,
        outward: true,
        hollow: false,
        sharp: false,
    };
    let out = one(k.shell_with(b, &spec).unwrap());
    let grown = |a: f64, r: f64| {
        a.powi(3) + 6.0 * a * a * r + 3.0 * PI * a * r * r + 4.0 / 3.0 * PI * r.powi(3)
    };
    close(
        volume(&k, out),
        grown(20.0, 10.0) - grown(20.0, 5.0) - 20.0 * 20.0 * 5.0 - PI / 4.0 * 75.0 * 20.0,
        1e-6,
    );
    // The round alone: its slab is the only opening.
    let spec = ShellSpec {
        remove: vec![round],
        ..spec
    };
    let out = one(k.shell_with(b, &spec).unwrap());
    close(
        volume(&k, out),
        grown(20.0, 10.0) - grown(20.0, 5.0) - PI / 4.0 * 75.0 * 20.0,
        1e-6,
    );
}

/// The elliptic cylinder x²/25 + y²/9 ≤ 1, 0 ≤ z ≤ 10, shrunk by d: its outline's inward offset
/// extruded between z = d and 10 − d. By 1 (less than the least radius of curvature b²/a = 1.8)
/// the offset is smooth, area and length by Steiner's formulas; by 2.5 and 2.9 it folds in
/// swallowtails about the ends of the long axis and is trimmed to meet itself there in an edge,
/// its area and length integrated over the kept part (t between asin √(((ad/b)² − b²)/(a² − b²))
/// and π less that, where it crosses the long axis). Rounded and sharp alike: no reflex corner.
#[test]
fn elliptic_cylinder_shrunk_past_its_curvature() {
    let (a, b, h) = (5.0, 3.0, 10.0);
    // p = 4aE(1 − b²/a²), the ellipse's perimeter.
    let p = 25.526_998_863_398_127;
    for (d, volume, area) in [
        (
            1.0,
            (PI * a * b - p + PI) * (h - 2.0),
            2.0 * (PI * a * b - p + PI) + (p - 2.0 * PI) * (h - 2.0),
        ),
        (2.5, 15.313_384_732_037_418, 51.932_013_180_920_92),
        (2.9, 1.155_321_205_251_106_2, 17.866_244_267_901_9),
    ] {
        for sharp in [false, true] {
            let mut k = NativeKernel::new();
            let c = prism(
                &mut k,
                Loop {
                    curves: vec![Curve2::Ellipse {
                        center: Point2::origin(),
                        major_radius: a,
                        minor_radius: b,
                        rotation: 0.0,
                        source: None,
                    }],
                },
                vec![],
                h,
            );
            let r = one(k
                .offset(
                    c,
                    &OffsetSpec {
                        distance: -d,
                        faces: vec![],
                        sharp,
                    },
                )
                .unwrap_or_else(|e| panic!("shrunk by {d}: {e}")));
            let m = k.mass_properties(r).unwrap();
            close(m.volume, volume, 1e-10);
            close(m.surface_area, area, 1e-10);
            let body = k.get(r).unwrap();
            assert!(
                body.check_with(cadrs_kernel::native::topo::CheckLevel::Full)
                    .is_empty()
            );
        }
    }
}
