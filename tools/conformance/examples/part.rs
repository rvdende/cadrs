//! Scratch: a part case's shape (its `b`) built step by step, each boolean's outcome printed,
//! with each primitive's faces' surface kinds.
use brep_conformance::generate;
use brep_conformance::model::Shape;
use brep_protocol::Kernel;
use cadrs_conformance::{Cadrs, Solid};

fn kinds(k: &Cadrs, s: &Solid) -> String {
    let Solid::Body(id) = s else {
        return "empty".into();
    };
    k.with(|kk| {
        let b = kk.get(*id).unwrap();
        let mut n: std::collections::BTreeMap<String, usize> = Default::default();
        for f in b.faces() {
            *n.entry(format!("{:?}", f.surf.kind())).or_default() += 1;
        }
        format!("{n:?}")
    })
}

fn build(k: &Cadrs, s: &Shape, depth: usize) -> Solid {
    match s {
        Shape::Prim(p) => {
            let r = k.make(p).expect("a primitive");
            println!("{:depth$}prim: {}", "", kinds(k, &r));
            r
        }
        Shape::Bool(op, a, b) => {
            let (x, y) = (build(k, a, depth + 1), build(k, b, depth + 1));
            match k.boolean(*op, &x, &y) {
                Ok(r) => {
                    println!("{:depth$}{op:?}: ok", "");
                    r
                }
                Err(e) => {
                    println!("{:depth$}{op:?}: {e:?}", "");
                    std::process::exit(1)
                }
            }
        }
    }
}

fn main() {
    let id = std::env::args().nth(1).expect("id");
    let c = generate::cases(1)
        .into_iter()
        .find(|c| c.id.starts_with(&id))
        .expect("case");
    build(&Cadrs, &c.b, 0);
}
