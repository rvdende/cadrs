//! Scratch: a boolean case's a ∩ b meshed at 0.01 mm, its vertices welded where they are the
//! same point, and the edges used other than once each way printed with the faces beside them.
use brep_conformance::generate;
use brep_conformance::model::Shape;
use brep_protocol::Kernel;
use cadrs_conformance::{Cadrs, Solid};
use std::collections::HashMap;

fn build(k: &Cadrs, s: &Shape) -> Solid {
    match s {
        Shape::Prim(p) => k.make(p).expect("a primitive"),
        Shape::Bool(op, a, b) => k.boolean(*op, &build(k, a), &build(k, b)).expect("a boolean"),
    }
}

fn main() {
    let id = std::env::args().nth(1).expect("id");
    let c = generate::cases(1).into_iter().find(|c| c.id == id).expect("case");
    let k = Cadrs;
    let (a, b) = (build(&k, &c.a), build(&k, &c.b));
    let i = k.intersection(&a, &b).unwrap();
    let Solid::Body(bid) = i else { panic!("empty") };
    let m = k.with(|kk| cadrs_kernel::Kernel::tessellate(kk, bid, cadrs_kernel::Tessellation { deflection: 0.01, angle: std::f64::consts::PI }).unwrap());
    let key = |p: &nalgebra::Point3<f64>| (p.x.to_bits(), p.y.to_bits(), p.z.to_bits());
    let mut id_of: HashMap<(u64, u64, u64), usize> = HashMap::new();
    let ids: Vec<usize> = m.positions.iter().map(|p| { let n = id_of.len(); *id_of.entry(key(p)).or_insert(n) }).collect();
    let mut uses: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (t, tri) in m.indices.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (ids[tri[k] as usize], ids[tri[(k + 1) % 3] as usize]);
            uses.entry((a, b)).or_default().push(t);
        }
    }
    let mut bad = 0;
    for (&(a, b), ts) in &uses {
        let back = uses.get(&(b, a)).map_or(0, |v| v.len());
        if ts.len() != 1 || back != 1 {
            bad += 1;
            if bad <= 12 {
                let pa = m.positions[ids.iter().position(|&x| x == a).unwrap()];
                let faces: Vec<_> = ts.iter().map(|&t| m.triangle_faces[t]).collect();
                println!("edge {pa:?} uses {} back {back} faces {faces:?}", ts.len());
            }
        }
    }
    println!("{} triangles, {bad} bad edge uses", m.indices.len());
    k.with(|kk| {
        let body = kk.get(bid).unwrap();
        for f in [24usize, 3, 9] {
            let face = &body.faces()[f];
            println!("face {f}: {:?} sense {} loops {:?}", face.surf.kind(), face.sense, face.loops.iter().map(|l| l.len()).collect::<Vec<_>>());
        }
    });
}
