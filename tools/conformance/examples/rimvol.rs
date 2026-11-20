//! Scratch: the elliptic cylinder of semi-axes 10 and 6, 8 high, its top rim rounded to 2
//! (brep-bench's fillet/elliptic-cylinder-rim): each face's volume integral against the exact
//! total.
use cadrs_kernel::{DefaultKernel, ExtrudeSpec, Kernel, Loop, Pick, Plane, Profile};

fn main() {
    let mut k = DefaultKernel::new();
    let profile = Profile::of(Plane::xy(0.0), vec![Loop::ellipse([0.0, 0.0], 10.0, 6.0)]);
    let cyl = k
        .extrude_with(&profile, &ExtrudeSpec::normal_to(&profile, 8.0))
        .unwrap()
        .bodies[0];
    let near = |p: &nalgebra::Point3<f64>| (p.z - 8.0).abs() < 1e-7;
    let rim = k
        .edges_where(cyl, &|e| near(&e.start) && near(&e.mid) && near(&e.end))
        .unwrap();
    let id = k.fillet(cyl, &rim, 2.0).unwrap().bodies[0];
    let b = k.get(id).unwrap();
    let z = nalgebra::Vector3::zeros();
    let mut tot = 0.0;
    for (fi, f) in b.faces().iter().enumerate() {
        let q = cadrs_kernel::native::mass::face_integrals(b, f, &z);
        tot += q[4];
        println!(
            "face {fi} {:?} sense {} loops {:?}: {:.10}",
            f.surf.kind(),
            f.sense,
            f.loops.iter().map(|l| l.len()).collect::<Vec<_>>(),
            q[4]
        );
    }
    println!(
        "sum {tot:.10}, exact 1466.5488314543, kernel volume {:?}",
        k.mass_properties(id).map(|m| m.volume)
    );
}
