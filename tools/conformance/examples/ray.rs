//! Scratch: a boolean case's B26 ray cast at a ∪ b and at each operand.
use brep_conformance::generate;
use brep_conformance::model::{Op, Shape};
use brep_conformance::oracle;
use brep_protocol::Kernel;
use cadrs_conformance::Cadrs;

fn main() {
    let id = std::env::args().nth(1).expect("id");
    let c = generate::cases(1)
        .into_iter()
        .find(|c| c.id == id)
        .expect("case");
    let (Shape::Prim(pa), Shape::Prim(pb)) = (&c.a, &c.b) else {
        panic!()
    };
    let k = Cadrs;
    let (a, b) = (k.make(pa).unwrap(), k.make(pb).unwrap());
    let u = Shape::bool(Op::Union, c.a.clone(), c.b.clone());
    let (lo, hi) = oracle::bounds(&u).unwrap();
    let mid = [0, 1, 2].map(|i| 0.5 * (lo[i] + hi[i]));
    let half = (0..3).map(|i| (hi[i] - lo[i]).powi(2)).sum::<f64>().sqrt();
    let d = c.motion.r[1];
    let aside = c.motion.r[2].map(|x| x * 0.1 * half);
    let o = [0, 1, 2].map(|i| mid[i] + aside[i] - d[i] * 1.5 * half);
    let (want, _) = oracle::crossings(&u, o, d, 3.0 * half, 8192);
    println!("want {want:?}");
    println!("a {:?}", k.raycast(&a, o, d));
    if let cadrs_conformance::Solid::Body(id) = a {
        k.with(|kk| {
            use cadrs_kernel::native::{
                classify,
                math::{P3, V3},
            };
            let body = kk.get(id).unwrap();
            let (po, pd) = (P3::new(o[0], o[1], o[2]), V3::new(d[0], d[1], d[2]));
            for (fi, f) in body.faces().iter().enumerate() {
                println!(
                    "  face {fi}: {}",
                    format!("{:?}", f.surf)
                        .chars()
                        .take(260)
                        .collect::<String>()
                );
                let hits = classify::ray_surface(&f.surf, &po, &pd, -1e3, 1e3);
                let polys = classify::face_polygons(body, f);
                for (t, u, v) in hits {
                    let inf = classify::in_face(&polys, &f.surf, nalgebra::Point2::new(u, v), 1e-9);
                    println!(
                        "  face {fi} {:?}: t {t} uv ({u}, {v}) {inf:?}",
                        f.surf.kind()
                    );
                }
            }
        });
    }
    println!("b {:?}", k.raycast(&b, o, d));
    let s = k.union(&a, &b).unwrap();
    println!("a ∪ b {:?}", k.raycast(&s, o, d));
}

#[allow(dead_code)]
fn faces_debug() {}
