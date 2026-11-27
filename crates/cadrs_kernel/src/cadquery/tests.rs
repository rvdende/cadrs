//! The module's own checks: CadQuery programs and the volumes real CadQuery gives them.

use super::workplane::{Combine, Until};
use super::*;
use crate::native::math::V3;

fn xy() -> Workplane {
    Workplane::named("XY", V3::zeros()).unwrap()
}

fn vol(w: &Workplane) -> f64 {
    let s = w.shapes();
    let s = if s.len() == 1 {
        s[0].clone()
    } else {
        Shape::compound(s)
    };
    s.volume().unwrap()
}

#[test]
fn box_with_a_hole_and_fillets() {
    let w = xy()
        .box_(10.0, 10.0, 10.0, (true, true, true), Combine::Fuse, true)
        .unwrap();
    assert!((vol(&w) - 1000.0).abs() < 1e-6);
    let top = w
        .select(ShapeType::Face, Some(&Selector::parse(">Z").unwrap()), None)
        .unwrap();
    assert_eq!(top.objects.len(), 1);
    let h = top
        .workplane(0.0, false, "ProjectedOrigin", None)
        .unwrap()
        .hole(2.0, None, true)
        .unwrap();
    assert!(
        (vol(&h) - (1000.0 - std::f64::consts::PI * 10.0)).abs() < 1e-6,
        "{}",
        vol(&h)
    );
    let f = h
        .select(ShapeType::Edge, Some(&Selector::parse("|Z").unwrap()), None)
        .unwrap()
        .fillet(1.0)
        .unwrap();
    // Four vertical edges rounded: each takes (1 - π/4)·10.
    let want =
        1000.0 - std::f64::consts::PI * 10.0 - 4.0 * (1.0 - std::f64::consts::PI / 4.0) * 10.0;
    assert!((vol(&f) - want).abs() < 1e-6, "{} vs {want}", vol(&f));
}

#[test]
fn rect_and_circle_make_a_holed_plate() {
    let w = xy()
        .circle(3.0, false)
        .unwrap()
        .rect(10.0, 10.0, (true, true), false)
        .unwrap();
    let e = w
        .extrude(Until::Distance(1.0), Combine::Fuse, true, false, 0.0)
        .unwrap();
    assert!(
        (vol(&e) - 71.725_666_117_691_86).abs() < 1e-6,
        "{}",
        vol(&e)
    );
}

#[test]
fn polyline_close_extrude_and_taper() {
    let w = xy()
        .polyline(
            &[
                V3::new(0.0, 0.0, 0.0),
                V3::new(4.0, 0.0, 0.0),
                V3::new(4.0, 3.0, 0.0),
            ],
            false,
            false,
        )
        .unwrap()
        .close()
        .unwrap();
    let e = w
        .extrude(Until::Distance(2.0), Combine::Fuse, true, false, 0.0)
        .unwrap();
    assert!((vol(&e) - 12.0).abs() < 1e-9);
    let t = xy()
        .rect(10.0, 10.0, (true, true), false)
        .unwrap()
        .extrude(Until::Distance(2.0), Combine::Fuse, true, false, 10.0)
        .unwrap();
    assert!(
        (vol(&t) - 186.225_481_053_997_6).abs() < 1e-6,
        "{}",
        vol(&t)
    );
}

#[test]
fn selectors_index_as_cadquery() {
    let b = xy()
        .box_(10.0, 10.0, 10.0, (true, true, true), Combine::Fuse, true)
        .unwrap();
    let top = b
        .select(ShapeType::Face, Some(&Selector::parse(">Z").unwrap()), None)
        .unwrap();
    let w = top
        .workplane(0.0, false, "ProjectedOrigin", None)
        .unwrap()
        .rect(4.0, 4.0, (true, true), false)
        .unwrap();
    let b = w
        .extrude(Until::Distance(5.0), Combine::Fuse, true, false, 0.0)
        .unwrap();
    let z = |s: &str| -> Vec<f64> {
        b.select(ShapeType::Face, Some(&Selector::parse(s).unwrap()), None)
            .unwrap()
            .shapes()
            .iter()
            .map(|f| (f.center().z * 1000.0).round() / 1000.0)
            .collect()
    };
    assert_eq!(z("<Z[0]"), vec![10.0]);
    assert_eq!(z("<Z[1]"), vec![5.0]);
    assert_eq!(z(">Z[0]"), vec![-5.0]);
    assert_eq!(z(">Z[-2]"), vec![5.0]);
    assert_eq!(z("<Z"), vec![-5.0]);
}

/// A tube and a spoke across its wall whose flat ends touch the tube's cylinders along a line
/// (CadEvolve's hollow_cylinder_shell_with_double_spokes): OCCT unites them to 2764.6363.
#[test]
fn spoke_touching_both_cylinders_unites() {
    let (ri, ro, h, w) = (10.0, 12.0, 20.0, 1.0);
    let tube = xy()
        .circle(ro, false)
        .unwrap()
        .circle(ri, false)
        .unwrap()
        .extrude(Until::Distance(h), Combine::Fuse, true, false, 0.0)
        .unwrap();
    let pts = [
        V3::new(ri, -w / 2.0, 0.0),
        V3::new(ro, -w / 2.0, 0.0),
        V3::new(ro, w / 2.0, 0.0),
        V3::new(ri, w / 2.0, 0.0),
    ];
    let spoke = xy()
        .workplane(h / 4.0, false, "ProjectedOrigin", None)
        .unwrap()
        .polyline(&pts, false, false)
        .unwrap()
        .close()
        .unwrap()
        .extrude(Until::Distance(h / 2.0), Combine::Fuse, true, false, 0.0)
        .unwrap()
        .rotate(V3::zeros(), V3::z(), 22.5)
        .unwrap();
    let u = tube.union_wp(&spoke, true).unwrap();
    assert!((vol(&u) - 2764.636_266_429_097).abs() < 1e-6, "{}", vol(&u));
}

/// A filleted diagonal rib across a conical shell's wall (CadEvolve's
/// frustum_cone_with_internal_diagonal_supports): OCCT unites them to 1348.8225 (its fillets
/// approximated; ours exact, 3e-7 apart).
#[test]
fn filleted_rib_across_a_cone_wall_unites() {
    let (r1, r2, h, t, w, d, fr) = (12.0, 6.0, 25.0, 1.0, 1.0, 2.0, 0.2);
    let cone = |a: f64, b: f64| {
        let w = xy()
            .circle(a, false)
            .unwrap()
            .workplane(h, false, "ProjectedOrigin", None)
            .unwrap()
            .circle(b, false)
            .unwrap();
        w.loft(false, Combine::Fuse, true).unwrap()
    };
    let shell = cone(r1, r2)
        .cut_shapes(cone(r1 - t, r2 - t).shapes(), true)
        .unwrap();
    let (sx, ex) = (r1 - t, r2);
    let l = (ex - sx).hypot(h);
    let (nx, nz) = (-h / l, (ex - sx) / l);
    let hw = w / 2.0;
    let pts = [
        V3::new(sx + nx * hw, nz * hw, 0.0),
        V3::new(sx - nx * hw, -nz * hw, 0.0),
        V3::new(ex - nx * hw, h - nz * hw, 0.0),
        V3::new(ex + nx * hw, h + nz * hw, 0.0),
    ];
    let rib = Workplane::named("XZ", V3::zeros())
        .unwrap()
        .polyline(&pts, false, false)
        .unwrap()
        .close()
        .unwrap();
    let rib = rib
        .extrude(Until::Distance(d), Combine::Fuse, true, false, 0.0)
        .unwrap();
    let rib = rib
        .select(ShapeType::Edge, None, None)
        .unwrap()
        .fillet(fr)
        .unwrap();
    let u = shell.union_wp(&rib, true).unwrap();
    assert!((vol(&u) - 1348.822_524_061_117).abs() < 1e-3, "{}", vol(&u));
}

/// A square rib swept along a helix (`parametricCurve`) grazing a tube's bore: its outer face
/// crosses the bore in two helices close together (CadEvolve's oct_prism_with_internal_spoke_
/// support and its kind). OCCT unites them to 2626.5596.
#[test]
fn helical_rib_grazing_a_bore_unites() {
    let (r, h, turns, t) = (9.0, 20.0, 3.0, 0.8);
    let pts: Vec<V3> = (0..=400)
        .map(|i| {
            let u = i as f64 / 400.0;
            let a = std::f64::consts::TAU * turns * u;
            V3::new(r * a.cos(), r * a.sin(), h * u)
        })
        .collect();
    let path = xy().spline_approx(&pts, 1e-6, false, false, true).unwrap();
    let path = path.shapes().remove(0);
    let rib = xy().center(r, 0.0).rect(t, t, (true, true), false).unwrap();
    let rib = rib
        .sweep(&path, false, true, true, Combine::Fuse, true, "right", None)
        .unwrap();
    let tube = xy()
        .circle(r + t / 2.0 + 2.0, false)
        .unwrap()
        .circle(r + t / 2.0, false)
        .unwrap();
    let tube = tube
        .extrude(Until::Distance(h), Combine::Fuse, true, false, 0.0)
        .unwrap();
    let u = tube.union_wp(&rib, true).unwrap();
    assert!((vol(&u) - 2626.5596).abs() < 1e-3, "{}", vol(&u));
}

/// Replays a boolean the oracle logged (`CADRS_REPLAY=<dir>/<pid>-<n>` and `CADRS_REPLAY_OP=
/// Union|Subtract|Intersect`): its operands read back (`-0.body` the target, the rest tools).
#[test]
#[ignore = "a debugging aid: set CADRS_REPLAY"]
fn replay_logged_boolean() {
    use crate::Kernel;
    let Ok(stem) = std::env::var("CADRS_REPLAY") else {
        return;
    };
    let op = match std::env::var("CADRS_REPLAY_OP").as_deref() {
        Ok("Union") => crate::BoolOp::Union,
        Ok("Intersect") => crate::BoolOp::Intersect,
        _ => crate::BoolOp::Subtract,
    };
    let mut k = crate::native::NativeKernel::new();
    let mut ids = Vec::new();
    for j in 0.. {
        let Ok(bytes) = std::fs::read(format!("{stem}-{j}.body")) else {
            break;
        };
        ids.push(k.read_body(&bytes).unwrap());
    }
    if let Ok(spec) = std::env::var("CADRS_REPLAY_FACE") {
        // "<operand>:<face>": that face's coedges, their p-curves' ends.
        let (bi, fi) = spec.split_once(':').unwrap();
        let b = k.get(ids[bi.parse::<usize>().unwrap()]).unwrap().clone();
        let f = &b.faces[fi.parse::<usize>().unwrap()];
        for (li, l) in f.loops.iter().enumerate() {
            for c in l {
                let e = &b.edges[c.edge];
                let (s0, s1) = if c.fwd {
                    (e.t[0], e.t[1])
                } else {
                    (e.t[1], e.t[0])
                };
                let kind = match &c.pc {
                    crate::native::curve::Curve::Line { .. } => "line",
                    crate::native::curve::Curve::Nurbs(_) => "nurbs",
                    _ => "conic",
                };
                eprintln!(
                    "loop {li} edge {} fwd {} {kind} pc {:?} → {:?} (mid {:?})",
                    c.edge,
                    c.fwd,
                    c.pc.point(s0),
                    c.pc.point(s1),
                    c.pc.point(0.5 * (s0 + s1))
                );
            }
        }
    }
    let r = k.boolean(op, ids[0], &ids[1..]);
    match r {
        Ok(r) => eprintln!("volume {}", k.mass_properties(r.bodies[0]).unwrap().volume),
        Err(e) => panic!("{e}"),
    }
}

/// A three-turn helix's p-curve on its cylinder runs on through the turns, its end three turns
/// on from its start (not wrapped back into the first turn).
#[test]
fn helix_pcurve_unwraps_through_its_turns() {
    use crate::native::math::{Frame, P3};
    use crate::native::surface::Surface;
    let g = super::make::helix(5.0 / 3.0, 5.0, 20.0, V3::zeros(), V3::z(), 360.0, false)
        .unwrap()
        .remove(0);
    let f = Frame {
        o: P3::origin(),
        x: V3::x(),
        y: V3::y(),
        z: V3::z(),
    };
    let s = Surface::Cylinder { f, r: 20.0 };
    let pc = crate::native::pcurve::pcurve(&s, &g.curve, g.t0, g.t1, None);
    let (a, b) = (pc.point(g.t0), pc.point(g.t1));
    assert!(
        ((b.x - a.x) - 3.0 * std::f64::consts::TAU).abs() < 1e-6,
        "{a:?} → {b:?}"
    );
    assert!((b.y - a.y - 5.0).abs() < 1e-6, "{a:?} → {b:?}");
}

/// A conical spiral sampled through `parametricCurve`'s fit is recognized as the spiral it is.
#[test]
fn sampled_conical_spiral_is_a_helix() {
    let (r1, r2, h, turns) = (9.0, 6.0, 20.0, 3.0);
    let pts: Vec<V3> = (0..=400)
        .map(|i| {
            let t = i as f64 / 400.0;
            let (a, r) = (std::f64::consts::TAU * turns * t, r1 + (r2 - r1) * t);
            V3::new(r * a.cos(), r * a.sin(), h * t)
        })
        .collect();
    let g = super::make::spline_approx(&pts, 1e-6).unwrap();
    let hx = super::ops::helix_of(&[g]).expect("a conical spiral");
    assert!(
        (hx.r0 - 9.0).abs() < 1e-5 && (hx.r1 - 6.0).abs() < 1e-5,
        "{hx:?}"
    );
}
