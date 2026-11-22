//! Scratch: two dumped bodies (target, tool): each tool face's surface kind, its box, and for a
//! plane, its distance and angle to each target plane it nearly matches.
use cadrs_kernel::native::bodyio;
use cadrs_kernel::native::surface::Surface;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let t = bodyio::read(&std::fs::read(&a[1]).unwrap()).unwrap();
    let k = bodyio::read(&std::fs::read(&a[2]).unwrap()).unwrap();
    for (j, f) in k.faces.iter().enumerate() {
        let bb = k.face_bbox(j);
        println!(
            "tool face {j}: {:?} box {:.4?}..{:.4?}",
            f.surf.kind(),
            bb.min,
            bb.max
        );
        let Surface::Plane { f: pf } = &f.surf else {
            continue;
        };
        for (i, g) in t.faces.iter().enumerate() {
            let Surface::Plane { f: qf } = &g.surf else {
                continue;
            };
            let ang = pf.z.cross(&qf.z).norm();
            let d = (pf.o - qf.o).dot(&qf.z);
            if ang < 1e-3 && d.abs() < 1e-2 {
                println!("   ~ target face {i}: sin angle {ang:.3e}, offset {d:.3e}");
            }
        }
    }
    if std::env::var_os("RUN").is_some() {
        let flush = std::env::var_os("FLUSH").is_some();
        use cadrs_kernel::native::boolean::{boolean_bodies, boolean_bodies_flush};
        let op = cadrs_kernel::BoolOp::Subtract;
        let r = if flush {
            boolean_bodies_flush(&t, &k, op)
        } else {
            boolean_bodies(&t, &k, op)
        };
        match r {
            Ok(o) => println!(
                "result: {} faces, check {:?}",
                o.body.faces.len(),
                o.body.check()
            ),
            Err(e) => println!("refused: {e}"),
        }
    }
}
