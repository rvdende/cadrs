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
    // (REPRO_MOVED=1: both operands moved by the case's motion, as its frame-invariance check.)
    let moved = std::env::var_os("REPRO_MOVED").is_some();
    let place = |p: &Primitive| if moved { p.moved(&c.motion) } else { p.clone() };
    let base: Primitive = match &c.a {
        Shape::Prim(p) => place(p),
        other => panic!("not a primitive: {other:?}"),
    };
    let a = k.make(&base).expect("making a");
    // A feature case's act, else the boolean the second argument names (union by default) of
    // `a` and `b`.
    let out = match c.feature.as_ref() {
        None if c.matrix.is_some() => {
            let t = std::time::Instant::now();
            let r = k.affine(&a, c.matrix.expect("a matrix"));
            eprintln!("affine: {:?}", t.elapsed());
            if let Ok(s) = &r {
                let t = std::time::Instant::now();
                let v = k.volume(s);
                eprintln!("volume {v:?}: {:?}", t.elapsed());
                let t = std::time::Instant::now();
                let _ = k.check(s);
                eprintln!("check: {:?}", t.elapsed());
            }
            r
        }
        Some(f) => {
            println!("{:?} {:?} {}", f.act, f.points, f.size);
            match f.act {
                generate::Act::OffsetFaces => k.offset_faces(&a, &f.points, f.size),
                generate::Act::Fillet => k.fillet(&a, &f.points, f.size),
                generate::Act::FilletG2 => k.fillet_g2(&a, &f.points, f.size, f.radii[0]),
                generate::Act::Offset => {
                    // (REPRO_TIMES=n: the act n times over, for a profiler.)
                    let n: usize = std::env::var("REPRO_TIMES")
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(1);
                    let t = std::time::Instant::now();
                    for _ in 1..n {
                        let _ = k.offset(&a, f.size);
                    }
                    let r = k.offset(&a, f.size);
                    eprintln!("offset x{n}: {:?} each", t.elapsed() / n as u32);
                    r
                }
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
            let b = k.make(&place(pb)).expect("making b");
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
                    if std::env::var_os("REPRO_SURF").is_some() {
                        println!(
                            "    surface {}",
                            format!("{:?}", f.surf)
                                .chars()
                                .take(600)
                                .collect::<String>()
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
    if std::env::var_os("REPRO_VOLUME").is_some() {
        println!(
            "volume {:?} through STEP {:?}",
            k.volume(&out),
            k.volume(&back)
        );
    }
    if std::env::var_os("REPRO_STEP").is_some() {
        std::fs::write("repro.step", &text).unwrap();
    }
}
