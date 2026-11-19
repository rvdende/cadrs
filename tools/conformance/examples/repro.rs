//! One case's act run on cadrs directly, its result written to STEP and read back, and every
//! body's full check printed: `cargo run -r --example repro -- <case id>`.

use brep_conformance::generate;
use brep_conformance::model::{Primitive, Shape};
use brep_protocol::Kernel;
use cadrs_conformance::{Cadrs, Solid};
use cadrs_kernel::native::CheckLevel;

fn main() {
    let id = std::env::args().nth(1).expect("a case id");
    let c = generate::cases(1)
        .into_iter()
        .find(|c| c.id == id)
        .expect("no case of that id");
    let k = Cadrs;
    let base: Primitive = match &c.a {
        Shape::Prim(p) => p.clone(),
        other => panic!("not a primitive: {other:?}"),
    };
    let a = k.make(&base).expect("making a");
    // A feature case's act, else the boolean the second argument names (union by default) of
    // `a` and `b`.
    let out = match c.feature.as_ref() {
        Some(f) => {
            println!("{:?} {:?} {}", f.act, f.points, f.size);
            match f.act {
                generate::Act::OffsetFaces => k.offset_faces(&a, &f.points, f.size),
                generate::Act::MoveFaces => {
                    k.move_faces(&a, &f.points, f.matrix.expect("a matrix"))
                }
                other => panic!("act {other:?} not in the repro yet"),
            }
        }
        None => {
            let Shape::Prim(pb) = &c.b else {
                panic!("b is not a primitive")
            };
            let b = k.make(pb).expect("making b");
            match std::env::args().nth(2).as_deref() {
                Some("intersection") => k.intersection(&a, &b),
                Some("difference") => k.difference(&a, &b),
                Some("rdifference") => k.difference(&b, &a),
                _ => k.union(&a, &b),
            }
        }
    };
    let out = match out {
        Ok(s) => s,
        Err(e) => {
            println!("refused: {e}");
            return;
        }
    };
    let show = |name: &str, s: &Solid| {
        let Solid::Body(b) = *s else {
            println!("{name}: empty");
            return;
        };
        k.with(|kk| {
            let body = kk.get(b).expect("a body");
            let issues = body.check_with(CheckLevel::Full);
            println!("{name}: {} faces, {:?}", body.faces().len(), issues);
            for (fi, f) in body.faces().iter().enumerate() {
                let loops: Vec<Vec<String>> = f
                    .loops
                    .iter()
                    .map(|l| {
                        l.iter()
                            .map(|c| {
                                let e = &body.edges()[c.edge];
                                let (a, b) = (body.verts()[e.v[0]].p, body.verts()[e.v[1]].p);
                                let (a, b) = if c.fwd { (a, b) } else { (b, a) };
                                format!(
                                    "e{}:({:.4},{:.4},{:.4})->({:.4},{:.4},{:.4})",
                                    c.edge, a.x, a.y, a.z, b.x, b.y, b.z
                                )
                            })
                            .collect()
                    })
                    .collect();
                println!("  face {fi} {:?} {:?}", f.surf.kind(), loops);
                if std::env::var_os("REPRO_PCURVES").is_some() {
                    for c in f.loops.iter().flatten() {
                        let e = &body.edges()[c.edge];
                        let (a, z) = (c.pc.point(e.t[0]), c.pc.point(e.t[1]));
                        println!(
                            "    e{} t {:?} pc ({:.4},{:.4}) -> ({:.4},{:.4}) fwd {}",
                            c.edge, e.t, a.x, a.y, z.x, z.y, c.fwd
                        );
                    }
                    if let cadrs_kernel::native::Surface::Extrusion { c, d } = &f.surf {
                        if let cadrs_kernel::native::Curve::Nurbs(n) = c {
                            println!("    extrusion: curve range {:?}, d {:?}", n.range(), d);
                        }
                    }
                }
            }
            for (i, e) in body.edges().iter().enumerate() {
                if e.tol > 1e-7 {
                    println!("  edge {i} tol {:.3e}", e.tol);
                }
            }
            for (i, v) in body.verts().iter().enumerate() {
                if v.tol > 1e-7 {
                    println!("  vertex {i} tol {:.3e} at {:?}", v.tol, v.p);
                }
            }
        });
    };
    show("result", &out);
    let text = k.write_step(&out).expect("writing STEP");
    let back = k.read_step(&text).expect("reading STEP");
    show("through STEP", &back);
    if std::env::var_os("REPRO_STEP").is_some() {
        std::fs::write("repro.step", &text).unwrap();
    }
}
