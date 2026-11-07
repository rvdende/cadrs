//! Hard surface–surface intersections: tangent and branching cylinders, loops far smaller than
//! the surfaces, near-tangent and tangent tori, and SMLib's `ssi_*.smp` fixtures (NVIDIA
//! usd-brep, Apache-2.0; `tests/data/smlib/ssi_advanced`) against the lengths its own intersector gives
//! (`tools/smlib_oracle`: `smlib_oracle ssi <file>`, frozen below).

use std::sync::Arc;

use cadrs_kernel::native::math::*;
use cadrs_kernel::native::smb;
use cadrs_kernel::native::ssi::{self, IntCurve};
use cadrs_kernel::native::ssi_numeric::{self, UvBox};
use cadrs_kernel::native::surface::Surface;
use cadrs_kernel::native::topo::Body;

type Dom = ((f64, f64), (f64, f64));

/// Whether (u, v) lies in the box (angles modulo a turn), within a relative 1e-6.
fn in_dom(s: &Surface, d: Dom, (u, v): (f64, f64)) -> bool {
    let ok = |x: f64, (lo, hi): (f64, f64), periodic: bool| {
        let e = (hi - lo) * 1e-6 + 1e-9;
        if periodic {
            if hi - lo >= TAU - 1e-9 {
                return true;
            }
            wrap(x, lo - e, TAU) <= hi + e
        } else {
            x >= lo - e && x <= hi + e
        }
    };
    ok(u, d.0, s.u_periodic()) && ok(v, d.1, s.v_periodic())
}

/// The foot of `p` on `s`, hinted; from a global search too when the hinted one is far.
fn near(s: &Surface, p: &P3, hint: Option<(f64, f64)>) -> (f64, f64) {
    let uv = match hint {
        Some(_) => s.project(p, hint),
        // A fine grid start: a wavy B-spline has many local nearest points.
        None => {
            let g = s.project_coarse(p, 160);
            s.project(p, Some(g))
        }
    };
    if (s.eval(uv.0, uv.1) - p).norm() < 1e-5 {
        return uv;
    }
    // A closed B-spline's Newton stops on its seam: try from both sides of it too.
    let ((u0, u1), (v0, v1)) = s.param_box();
    let mut best = uv;
    let g = s.project(p, None);
    for c in [
        g,
        s.project(p, Some((u0, uv.1))),
        s.project(p, Some((u1, uv.1))),
        s.project(p, Some((uv.0, v0))),
        s.project(p, Some((uv.0, v1))),
    ] {
        if (s.eval(c.0, c.1) - p).norm() < (s.eval(best.0, best.1) - p).norm() {
            best = c;
        }
    }
    best
}

/// The length of the curves' parts inside both parameter boxes (as SMLib bounds its curves),
/// and the largest distance of a sampled point from either surface.
fn clipped(cs: &[IntCurve], a: &Surface, da: Dom, b: &Surface, db: Dom) -> (f64, f64) {
    let (mut len, mut dev) = (0.0, 0.0f64);
    for ic in cs {
        let n = 4000;
        let (mut ha, mut hb) = (None, None);
        let mut prev: Option<(P3, bool)> = None;
        for i in 0..=n {
            let t = ic.t0 + (ic.t1 - ic.t0) * i as f64 / n as f64;
            let p = ic.c.point(t);
            let ua = near(a, &p, ha);
            let ub = near(b, &p, hb);
            ha = Some(ua);
            hb = Some(ub);
            let inside = in_dom(a, da, ua) && in_dom(b, db, ub);
            if inside {
                let d = (a.eval(ua.0, ua.1) - p)
                    .norm()
                    .max((b.eval(ub.0, ub.1) - p).norm());
                if d > 1e-6 && std::env::var_os("SSI_TEST_DEBUG").is_some() {
                    eprintln!(
                        "  off by {d:e} at sample {i}/{n} {p:?}: a {:e} b {:e}",
                        (a.eval(ua.0, ua.1) - p).norm(),
                        (b.eval(ub.0, ub.1) - p).norm()
                    );
                }
                dev = dev.max(d);
            }
            if let Some((q, true)) = prev.filter(|_| inside) {
                len += (p - q).norm();
            }
            prev = Some((p, inside));
        }
    }
    (len, dev)
}

/// The parameter box of a face: its p-curves' extent.
fn face_dom(body: &Body, f: usize) -> Dom {
    let (mut u, mut v) = (
        (f64::INFINITY, f64::NEG_INFINITY),
        (f64::INFINITY, f64::NEG_INFINITY),
    );
    for co in body.faces[f].loops.iter().flatten() {
        let e = &body.edges[co.edge];
        for i in 0..=64 {
            let t = e.t[0] + (e.t[1] - e.t[0]) * i as f64 / 64.0;
            let q = co.pc.point(t);
            u = (u.0.min(q.x), u.1.max(q.x));
            v = (v.0.min(q.y), v.1.max(q.y));
        }
    }
    (u, v)
}

/// The domain SMLib intersects a face over (`SmFace::GetUVDomain`), as far as its results
/// show: a B-spline's or a sphere's whole range, the face's extent on the other surfaces.
fn smlib_domain(s: &Surface, face: Dom) -> Dom {
    let ((u0, u1), (v0, v1)) = s.param_box();
    let open = |a: f64, b: f64| a <= -1e4 || b >= 1e4;
    let whole = matches!(s, Surface::Nurbs(_) | Surface::Sphere { .. });
    let u = if whole && !open(u0, u1) {
        (u0, u1)
    } else {
        face.0
    };
    let v = if whole && !open(v0, v1) {
        (v0, v1)
    } else {
        face.1
    };
    (u, v)
}

/// The two surfaces of a fixture with their parameter boxes: brep 0's and brep 1's face, or
/// the part's two surfaces over their whole ranges.
fn fixture(file: &str) -> [(Surface, Dom); 2] {
    let path = format!(
        "{}/tests/data/smlib/ssi_advanced/{file}",
        env!("CARGO_MANIFEST_DIR")
    );
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let part = smb::read_part(&bytes).unwrap_or_else(|e| panic!("{file}: {e}"));
    if part.breps.len() >= 2 {
        let side = |k: usize| {
            let body = &part.breps[k].body;
            assert_eq!(
                body.faces.len(),
                1,
                "{file}: brep {k} has {} faces",
                body.faces.len()
            );
            let s = body.faces[0].surf.clone();
            let d = face_dom(body, 0);
            // (A face whose p-curves span nothing, a closed surface's seam: the whole range.)
            let d = if d.0.1 - d.0.0 > 1e-9 && d.1.1 - d.1.0 > 1e-9 {
                d
            } else {
                s.param_box()
            };
            (s.clone(), smlib_domain(&s, d))
        };
        [side(0), side(1)]
    } else {
        assert!(part.surfaces.len() >= 2, "{file}: no two surfaces");
        let side = |k: usize| (part.surfaces[k].clone(), part.surfaces[k].param_box());
        [side(0), side(1)]
    }
}

/// The intersection as the boolean asks for it: both boxes and the overlap of their bounds.
fn intersect(a: &Surface, da: Dom, b: &Surface, db: Dom) -> Vec<IntCurve> {
    let (ba, bb) = (a.patch_bbox(da.0, da.1), b.patch_bbox(db.0, db.1));
    let mut ob = ba;
    ob.min = ob.min.sup(&bb.min);
    ob.max = ob.max.inf(&bb.max);
    let ob = ob.grow(1e-6 * ba.diagonal().max(bb.diagonal()));
    ssi::surface_surface(a, da, b, db, &ob)
}

/// An SMLib fixture: the length of its crossing and tangent curves (SMLib's own result), with a
/// relative tolerance.
fn smlib_case(file: &str, length: f64, rel: f64) {
    let [(a, da), (b, db)] = fixture(file);
    if std::env::var_os("SSI_TEST_DEBUG").is_some() {
        eprintln!(
            "{file}: a {:?} over {da:?} (box {:?}), b {:?} over {db:?} (box {:?})",
            a.kind(),
            a.param_box(),
            b.kind(),
            b.param_box()
        );
    }
    let t = std::time::Instant::now();
    let cs = intersect(&a, da, &b, db);
    let el = t.elapsed();
    let (len, dev) = clipped(&cs, &a, da, &b, db);
    let raw: Vec<f64> = cs.iter().map(|ic| ic.c.length(ic.t0, ic.t1)).collect();
    eprintln!(
        "{file}: {} curves {raw:.4?}, length {len:.6} (SMLib {length:.6}), deviation {dev:.1e}, {el:?}",
        cs.len()
    );
    let scale = a.patch_bbox(da.0, da.1).diagonal().max(1.0);
    assert!(
        dev < 1e-6 * scale,
        "{file}: a curve strays {dev:e} from the surfaces"
    );
    assert!(
        (len - length).abs() <= rel * length.max(1.0),
        "{file}: length {len}, SMLib {length}"
    );
}

macro_rules! smlib {
    ($($(#[$m:meta])* $name:ident: $file:literal, $len:expr;)*) => {$(
        #[test]
        $(#[$m])*
        fn $name() {
            smlib_case($file, $len, 2e-3);
        }
    )*};
}

// The crossing (type 2) and tangent (type 3) curves' total length from `smlib_oracle ssi`.
// Point contacts (type 1) have none; coincident regions (types 4 and 6) are left to the
// booleans' coincident-face handling and aren't compared.
smlib! {
    smlib_2pl: "ssi_2pl.smp", 3.0;
    smlib_bumppy_plane2: "ssi_bumppy_plane2.smp", 119.339493;
    // SMLib traces the first of the two branches crossing at (0, 0, 1.5) out to the branch
    // point and back, twice over (76.11 in all, its samples 3e-4 off the surfaces); its second
    // branch, 21.5595, is ours to 2e-4. Ours: the two branches, each checked on both surfaces.
    smlib_clasic_ruled: "ssi_clasic_ruled.smp", 43.537537;
    smlib_cone_2pln: "ssi_cone_2pln.smp", 20.402487;
    smlib_cone_cyl1: "ssi_cone_cyl1.smp", 14.482707;
    smlib_cone_cyl2: "ssi_cone_cyl2.smp", 11.478492;
    smlib_cone_plane1: "ssi_cone_plane1.smp", 14.142136;
    smlib_cyl_bicub: "ssi_cyl_bicub.smp", 28.494004;
    smlib_cylcyl1: "ssi_cylcyl1.smp", 18.046928;
    smlib_cylcyl_90_tan: "ssi_cylcyl_90_tan.smp", 1.910099;
    smlib_cyl_cyl_sing: "ssi_cyl_cyl_sing.smp", 30.561582;
    smlib_cyl_cyl_sing2: "ssi_cyl_cyl_sing2.smp", 15.885526;
    smlib_ext_cyl_2int: "ssi_ext_cyl_2int.smp", 18.0;
    smlib_ext_cyl_none: "ssi_ext_cyl_none.smp", 0.0;
    smlib_ext_cyl_touch: "ssi_ext_cyl_touch.smp", 0.0;
    smlib_ext_ext_inside: "ssi_ext_ext_inside.smp", 20.0;
    smlib_ext_ext_none: "ssi_ext_ext_none.smp", 0.0;
    smlib_ext_ext_opposite: "ssi_ext_ext_opposite.smp", 12.0;
    smlib_ext_ext_overlap: "ssi_ext_ext_overlap.smp", 12.0;
    smlib_ext_ext_same: "ssi_ext_ext_same.smp", 20.0;
    smlib_ext_ext_touch: "ssi_ext_ext_touch.smp", 0.0;
    smlib_plncyl1: "ssi_plncyl1.smp", 4.712389;
    smlib_plncyl2: "ssi_plncyl2.smp", 19.080155;
    smlib_pln_rev1: "ssi_pln_rev1.smp", 38.503168;
    // The plane holds both side meridians of the surface of revolution: SMLib reports the one
    // at its parameter's end (8.0012 + 16.0154, ours to 1e-7), not the one at its start
    // (24.99 inside both domains, checked on both surfaces).
    smlib_pln_rev2: "ssi_pln_rev2.smp", 49.006050;
    smlib_pln_rev3: "ssi_pln_rev3.smp", 23.659952;
    smlib_rev_cone_full: "ssi_rev_cone_full.smp", 18.542542;
    smlib_rev_cyl_full: "ssi_rev_cyl_full.smp", 18.849556;
    smlib_rev_rev_180_pts: "ssi_rev_rev_180_pts.smp", 0.0;
    smlib_rev_rev1: "ssi_rev_rev1.smp", 42.021793;
    smlib_rev_rev_360: "ssi_rev_rev_360.smp", 80.834085;
    smlib_rev_rev_90: "ssi_rev_rev_90.smp", 20.208521;
    smlib_rev_rev_opposite: "ssi_rev_rev_opposite.smp", 80.834085;
    smlib_rev_rev_rot180: "ssi_rev_rev_rot180.smp", 80.834085;
    smlib_rev_sph_full: "ssi_rev_sph_full.smp", 14.823624;
    smlib_rhino_bug1: "ssi_rhino_bug1.smp", 3.0;
    smlib_sph_plane1: "ssi_sph_plane1.smp", 31.415926;
    smlib_sph_plane2: "ssi_sph_plane2.smp", 31.415926;
    smlib_spiral: "ssi_spiral.smp", 67.080376;
    smlib_tor_in_tor_tan_ellip: "ssi_tor_in_tor_tan_ellip.smp", 87.964594;
    smlib_tor_in_tor_tang_hyper: "ssi_tor_in_tor_tang_hyper.smp", 25.132741;
    smlib_tor_overlap: "ssi_tor_overlap.smp", 42.004784;
    smlib_tor_purpen_tor_sing: "ssi_tor_purpen_tor_sing.smp", 125.77828;
    smlib_tor_sph_tan: "ssi_tor_sph_tan.smp", 31.415643;
    smlib_tor_sph_tan2: "ssi_tor_sph_tan2.smp", 31.415886;
    smlib_tor_sph_touch: "ssi_tor_sph_touch.smp", 0.0;
}

fn frame(o: P3, z: V3) -> Frame {
    Frame::from_z(o, z)
}

fn region(r: f64) -> Bbox {
    Bbox {
        min: P3::new(-r, -r, -r),
        max: P3::new(r, r, r),
    }
}

/// The surface as a B-spline (so no closed form applies).
fn nurbs(s: &Surface, u: (f64, f64), v: (f64, f64)) -> Surface {
    Surface::Nurbs(Arc::new(s.to_nurbs(u, v)))
}

/// Every curve point on both surfaces.
fn assert_on_both(cs: &[IntCurve], a: &Surface, b: &Surface, tol: f64) {
    for ic in cs {
        for i in 0..=200 {
            let p = ic.c.point(ic.t0 + (ic.t1 - ic.t0) * i as f64 / 200.0);
            let (da, db) = (a.distance(&p), b.distance(&p));
            assert!(
                da < tol && db < tol,
                "point {p:?} off the surfaces: {da:e}, {db:e}"
            );
        }
    }
}

fn total_length(cs: &[IntCurve]) -> f64 {
    cs.iter().map(|ic| ic.c.length(ic.t0, ic.t1)).sum()
}

/// Two cylinders of one radius with crossing axes (Steinmetz): two ellipses through two branch
/// points where the cylinders are tangent.
#[test]
fn branching_cylinders() {
    let a = Surface::Cylinder {
        f: frame(P3::origin(), V3::z()),
        r: 1.0,
    };
    let b = Surface::Cylinder {
        f: frame(P3::origin(), V3::x()),
        r: 1.0,
    };
    let cs = ssi::surface_surface(
        &a,
        ((0.0, TAU), (-2.0, 2.0)),
        &b,
        ((0.0, TAU), (-2.0, 2.0)),
        &region(2.0),
    );
    assert_on_both(&cs, &a, &b, 1e-6);
    // Each ellipse has semi-axes √2 and 1: perimeter 7.640395578.
    let len = total_length(&cs);
    assert!((len - 2.0 * 7.640395578).abs() < 1e-4, "length {len}");
}

/// Two perpendicular cylinders touching from outside: a single point of contact, no curve.
#[test]
fn tangent_cylinders_touch_at_a_point() {
    let a = Surface::Cylinder {
        f: frame(P3::origin(), V3::z()),
        r: 1.0,
    };
    let b = Surface::Cylinder {
        f: frame(P3::new(0.0, 2.0, 0.0), V3::x()),
        r: 1.0,
    };
    let cs = ssi::surface_surface(
        &a,
        ((0.0, TAU), (-2.0, 2.0)),
        &b,
        ((0.0, TAU), (-2.0, 2.0)),
        &region(3.0),
    );
    assert!(total_length(&cs) < 1e-6, "{} curves", cs.len());
}

/// A plane just under the top of a B-spline sphere cuts a circle of radius 0.0045 out of a
/// surface 2 across: a loop far smaller than any fixed sampling grid sees.
#[test]
fn small_loop_on_a_spline_sphere() {
    let sph = Surface::Sphere {
        f: frame(P3::origin(), V3::z()),
        r: 1.0,
    };
    let a = nurbs(&sph, (0.0, TAU), (-PI / 2.0, PI / 2.0));
    let h = 0.99999;
    let b = Surface::Plane {
        f: Frame::from_z(P3::new(0.0, 0.0, h), V3::new(0.3, 0.2, 1.0).normalize()),
    };
    let n = V3::new(0.3, 0.2, 1.0).normalize();
    let d = h * n.z; // the plane's distance from the centre
    let r = (1.0 - d * d).sqrt();
    let cs = ssi::surface_surface(
        &a,
        ((0.0, TAU), (-PI / 2.0, PI / 2.0)),
        &b,
        ((-2.0, 2.0), (-2.0, 2.0)),
        &region(1.5),
    );
    assert_on_both(&cs, &sph, &b, 1e-7);
    let len = total_length(&cs);
    assert!(
        (len - TAU * r).abs() < 1e-5 * TAU * r + 1e-9,
        "length {len}, circle {}",
        TAU * r
    );
    assert!(cs.iter().all(|c| c.closed), "the loop isn't closed");
}

/// The numeric engine finds a loop of radius 1e-3 on a surface 2 across, and reports how it
/// searched.
#[test]
fn numeric_engine_finds_tiny_loop() {
    let sph = Surface::Sphere {
        f: frame(P3::origin(), V3::z()),
        r: 1.0,
    };
    let r: f64 = 1e-3;
    let b = Surface::Plane {
        f: Frame::from_z(P3::new(0.0, 0.0, (1.0 - r * r).sqrt()), V3::z()),
    };
    let (res, st) = ssi_numeric::intersect_stats(
        &sph,
        UvBox::new((0.0, TAU), (-PI / 2.0, PI / 2.0)),
        &b,
        UvBox::new((-2.0, 2.0), (-2.0, 2.0)),
        &region(1.5),
    );
    eprintln!("tiny loop: {st:?}, {} curves", res.curves.len());
    assert_eq!(res.curves.len(), 1);
    let c = &res.curves[0];
    assert!(c.closed);
    let len: f64 = (0..c.pts.len())
        .map(|k| (c.pts[(k + 1) % c.pts.len()].p - c.pts[k].p).norm())
        .sum();
    assert!((len - TAU * r).abs() < 0.01 * TAU * r, "length {len}");
}

/// A sphere centred on a torus's core circle, a hair bigger than the tube: two long thin loops
/// pinched almost to a point where the sphere is nearly tangent to the tube.
#[test]
fn near_tangent_torus_and_sphere() {
    let tor = Surface::Torus {
        f: frame(P3::origin(), V3::z()),
        rmaj: 3.0,
        rmin: 1.0,
    };
    let sph = Surface::Sphere {
        f: frame(P3::new(3.0, 0.0, 0.0), V3::z()),
        r: 1.001,
    };
    let cs = ssi::surface_surface(
        &tor,
        ((0.0, TAU), (-PI, PI)),
        &sph,
        ((0.0, TAU), (-PI / 2.0, PI / 2.0)),
        &region(5.0),
    );
    assert_on_both(&cs, &tor, &sph, 1e-6);
    assert_eq!(cs.len(), 2, "two loops");
    assert!(cs.iter().all(|c| c.closed));
}

/// The same sphere exactly as big as the tube touches it along a whole circle (a tangent curve
/// of length 2π) and crosses it nowhere.
#[test]
fn tangent_torus_and_sphere() {
    let tor = Surface::Torus {
        f: frame(P3::origin(), V3::z()),
        rmaj: 3.0,
        rmin: 1.0,
    };
    let sph = Surface::Sphere {
        f: frame(P3::new(3.0, 0.0, 0.0), V3::z()),
        r: 1.0,
    };
    let cs = ssi::surface_surface(
        &tor,
        ((0.0, TAU), (-PI, PI)),
        &sph,
        ((0.0, TAU), (-PI / 2.0, PI / 2.0)),
        &region(5.0),
    );
    assert_on_both(&cs, &tor, &sph, 1e-5);
    let len = total_length(&cs);
    assert!((len - TAU).abs() < 1e-2, "length {len}");
}

/// Two B-spline tori crossing at right angles, the second's core through the first's tube:
/// every curve point on both surfaces, and the same length either way round.
#[test]
fn crossing_spline_tori_symmetric() {
    let t1 = Surface::Torus {
        f: frame(P3::origin(), V3::z()),
        rmaj: 3.0,
        rmin: 1.0,
    };
    let t2 = Surface::Torus {
        f: frame(P3::new(3.0, 0.0, 0.0), V3::y()),
        rmaj: 1.5,
        rmin: 1.0,
    };
    let (da, db) = (((0.0, TAU), (-PI, PI)), ((0.0, TAU), (-PI, PI)));
    let (n1, n2) = (nurbs(&t1, da.0, da.1), nurbs(&t2, db.0, db.1));
    let ab = ssi::surface_surface(&n1, da, &n2, db, &region(8.0));
    let ba = ssi::surface_surface(&n2, db, &n1, da, &region(8.0));
    assert_on_both(&ab, &t1, &t2, 1e-6);
    let (l1, l2) = (total_length(&ab), total_length(&ba));
    assert!(l1 > 1.0 && (l1 - l2).abs() < 1e-5 * l1, "{l1} vs {l2}");
}

/// SMLib's ssi_cone_cyl1 with the cone analytic: a cone (apex (5, 0, 0), base radius 5 at x = 0,
/// its seam on +z) against a cylinder of radius 5 along x, touching it along the generator to
/// the apex. The curve through the apex crosses the cone's seam; its two marches met there
/// overlapping (14.517 against SMLib's 14.483). Regression: they meet at the earlier one's end.
#[test]
fn cone_seam_curves_meet_without_overlap() {
    let cone = Surface::Cone {
        f: Frame {
            o: P3::origin(),
            x: V3::z(),
            y: V3::x().cross(&V3::z()),
            z: V3::x(),
        },
        r: 5.0,
        a: -std::f64::consts::FRAC_PI_4,
    };
    let cyl = Surface::Cylinder {
        f: frame(P3::new(0.0, -5.0, 0.0), V3::x()),
        r: 5.0,
    };
    let cs = ssi::surface_surface(
        &cyl,
        ((0.0, TAU), (0.0, 5.0)),
        &cone,
        ((0.0, TAU), (0.0, 5.0 * 2f64.sqrt())),
        &region(10.0),
    );
    assert_on_both(&cs, &cyl, &cone, 1e-6);
    let len = total_length(&cs);
    assert!((len - 14.48270673448).abs() < 5e-3, "length {len}");
}

/// A B-spline face whose side v = 0 lies on a cylinder (within 1e-8: shape_type_a_a1's offset,
/// the wall of a hole round a cylinder's Steinmetz curve), crossing it at an angle, and crossing
/// it again at a shallow one in four narrow bands. Marched, the intersection along the side
/// left the box at every rounding: 55 overlapping pieces, which a boolean cut at every touch.
/// Now the side itself (one curve) and the eight crossings, each from the side to the far side.
#[test]
fn box_side_on_the_other_surface() {
    let body = cadrs_kernel::native::bodyio::read(include_bytes!("data/ssi_side_on_cylinder.body"))
        .unwrap();
    let (cyl, wall) = (&body.faces[0].surf, &body.faces[1].surf);
    let region = Bbox {
        min: P3::new(
            -0.07701975090180072,
            -0.11003187651919082,
            0.298583911423951,
        ),
        max: P3::new(0.11003456216815335, 0.11003869235335319, 0.50071534336464),
    };
    let r = ssi::surface_surface_full(
        cyl,
        ((0.0, TAU), (0.0, 1.0)),
        wall,
        ((0.0, 1.0), (0.0, 1.0)),
        &region,
    );
    let len = |c: &IntCurve| c.c.length(c.t0.min(c.t1), c.t0.max(c.t1));
    let (long, short): (Vec<&IntCurve>, Vec<&IntCurve>) =
        r.curves.iter().partition(|c| len(c) > 0.5);
    assert_eq!(
        long.len(),
        1,
        "the side, once: {:?}",
        r.curves.iter().map(len).collect::<Vec<_>>()
    );
    assert!(
        (len(long[0]) - 1.1675).abs() < 1e-3,
        "the side's length {}",
        len(long[0])
    );
    assert_eq!(
        short.len(),
        8,
        "the crossings: {:?}",
        short.iter().map(|c| len(c)).collect::<Vec<_>>()
    );
    // Each crossing runs from the side (on the cylinder of radius 0.1 round z) to the face's
    // far side (radius 0.11).
    let rho = |p: P3| (p.x * p.x + p.y * p.y).sqrt();
    for c in short {
        let (a, b) = (rho(c.c.point(c.t0)), rho(c.c.point(c.t1)));
        let (lo, hi) = (a.min(b), a.max(b));
        assert!(
            (lo - 0.1).abs() < 1e-6 && (hi - 0.11).abs() < 1e-6,
            "a crossing from radius {lo} to {hi}"
        );
    }
}

/// Two B-splines one over part: a bends up from z = 0 at x = 1 (a quadratic knot there), b is
/// flat. Their intersection is the shared region's boundary: the sides of a on b (x = 0, and
/// y = 0 and 1 up to where a leaves b) and where a leaves b inside the boxes (the line x ≈ 1).
#[test]
fn partly_coincident_region_boundary() {
    use cadrs_kernel::native::nurbs::{NurbsSurface, hom};
    let row = |x: f64, z: f64| vec![hom(&P3::new(x, 0.0, z), 1.0), hom(&P3::new(x, 1.0, z), 1.0)];
    let a = NurbsSurface::new(
        2,
        1,
        vec![0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 2.0],
        vec![0.0, 0.0, 1.0, 1.0],
        vec![row(0.0, 0.0), row(0.5, 0.0), row(1.5, 0.0), row(2.0, 1.0)],
    )
    .unwrap();
    let b = NurbsSurface::new(
        1,
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![0.0, 0.0, 1.0, 1.0],
        vec![
            vec![
                hom(&P3::new(-0.5, -0.5, 0.0), 1.0),
                hom(&P3::new(-0.5, 1.5, 0.0), 1.0),
            ],
            vec![
                hom(&P3::new(2.5, -0.5, 0.0), 1.0),
                hom(&P3::new(2.5, 1.5, 0.0), 1.0),
            ],
        ],
    )
    .unwrap();
    let (a, b) = (Surface::Nurbs(Arc::new(a)), Surface::Nurbs(Arc::new(b)));
    let r = ssi::surface_surface_full(
        &a,
        ((0.0, 2.0), (0.0, 1.0)),
        &b,
        ((0.0, 1.0), (0.0, 1.0)),
        &region(3.0),
    );
    let len = |c: &IntCurve| c.c.length(c.t0.min(c.t1), c.t0.max(c.t1));
    let total: f64 = r.coincident.iter().map(len).sum();
    assert!(
        r.curves.is_empty() && r.tangent.is_empty(),
        "nothing but the region's boundary: {} crossing, {} tangent",
        r.curves.len(),
        r.tangent.len()
    );
    // x = 0 (1), the two sides to x ≈ 1 (each a little over 1), and the line where a parts.
    assert!((total - 4.0).abs() < 0.05, "the boundary's length {total}");
    let parting = r.coincident.iter().find(|c| {
        [0.0, 0.5, 1.0]
            .iter()
            .all(|f| (c.c.point(c.t0 + (c.t1 - c.t0) * f).x - 1.0).abs() < 0.02)
    });
    let parting = parting.expect("a boundary curve along x ≈ 1");
    assert!(
        (len(parting) - 1.0).abs() < 1e-3,
        "the parting line's length {}",
        len(parting)
    );
    for f in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let p = parting.c.point(parting.t0 + (parting.t1 - parting.t0) * f);
        assert!(p.z.abs() < 1e-5, "the parting line off b: {p:?}");
    }
}
