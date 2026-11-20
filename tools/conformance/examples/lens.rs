//! Scratch: a case's a ∩ b classified at the suite's sample points of the intersection's box,
//! against the truth (in both operands), the misplaced points printed with their distances.
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
    let i = k.intersection(&a, &b).unwrap();
    let both = Shape::bool(Op::Intersection, c.a.clone(), c.b.clone());
    let bx = oracle::bounds(&both).unwrap();
    println!("box {:?}", bx);
    println!("volume {:?}", k.volume(&i));
    let pts = oracle::samples(bx, 4096);
    let got = k.holds(&i, &pts).unwrap();
    for (p, g) in pts.iter().zip(&got) {
        let truth = oracle::contains(&both, *p);
        if truth != *g {
            let (da, db) = (
                oracle::boundary_distance(pa, *p),
                oracle::boundary_distance(pb, *p),
            );
            println!("{p:?}: kernel {g}, truth {truth}, to a {da:?}, to b {db:?}");
        }
    }
}
