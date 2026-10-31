//! How far an imported part lies from Onshape's own export of it: the distance from points of
//! the part's mesh to the exported STEP body's exact faces, and from points of the STEP body's
//! mesh to the part's mesh. Prints the largest and mean of each, and the worst places.
//!
//! `cargo run -r -p cadrs_onshape --example step_compare -- <file.ron> <part.step> <part name>`

use cadrs_core::document::Feature;
use cadrs_kernel::{DefaultKernel, DistanceTarget, Kernel, Tessellation};
use nalgebra::Point3;

fn main() {
    let mut args = std::env::args().skip(1);
    let dump = args.next().expect("a dumped feature list");
    let step = args.next().expect("a STEP file");
    let name = args.next().expect("a part name");
    let features: Vec<Feature> =
        ron::from_str(&std::fs::read_to_string(&dump).expect("readable")).expect("a feature list");
    let b = cadrs_core::rebuild::build(&features);
    let part = b
        .parts
        .iter()
        .find(|p| p.name == name)
        .expect("a part of that name");
    let s = &part.solid;
    let ours: Vec<Point3<f64>> = s
        .positions
        .iter()
        .map(|p| Point3::new(p[0], p[1], p[2]))
        .collect();
    let tris: Vec<[Point3<f64>; 3]> = s
        .indices
        .chunks_exact(3)
        .map(|t| {
            [
                ours[t[0] as usize],
                ours[t[1] as usize],
                ours[t[2] as usize],
            ]
        })
        .collect();
    let mut k = DefaultKernel::new();
    let bodies = k
        .import_step(&std::fs::read(&step).expect("a readable STEP file"))
        .expect("a STEP body");
    // Our mesh's points to Onshape's faces.
    let mut to_theirs: Vec<(f64, Point3<f64>)> = ours
        .iter()
        .step_by((ours.len() / 4000).max(1))
        .map(|p| {
            let d = bodies
                .iter()
                .filter_map(|b| {
                    k.min_distance(&DistanceTarget::Point(*p), &DistanceTarget::Body(*b))
                        .ok()
                })
                .map(|r| r.distance)
                .fold(f64::INFINITY, f64::min);
            (d, *p)
        })
        .collect();
    // Onshape's mesh points to ours.
    let mut theirs: Vec<Point3<f64>> = Vec::new();
    for b in &bodies {
        if let Ok(m) = k.tessellate(*b, Tessellation::default()) {
            theirs.extend(m.positions);
        }
    }
    let mut to_ours: Vec<(f64, Point3<f64>)> = theirs
        .iter()
        .step_by((theirs.len() / 4000).max(1))
        .map(|p| {
            let d = tris
                .iter()
                .map(|t| point_triangle(p, t))
                .fold(f64::INFINITY, f64::min);
            (d, *p)
        })
        .collect();
    for (what, v) in [
        ("cadrs -> Onshape", &mut to_theirs),
        ("Onshape -> cadrs", &mut to_ours),
    ] {
        v.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mean = v.iter().map(|x| x.0).sum::<f64>() / v.len().max(1) as f64;
        let under = |t: f64| v.iter().filter(|x| x.0 <= t).count() as f64 / v.len().max(1) as f64;
        println!(
            "{what}: {} points, max {:.3} mm, mean {:.4} mm, within 0.1 mm {:.1}%, within 0.5 mm {:.1}%",
            v.len(),
            v.first().map_or(0.0, |x| x.0),
            mean,
            100.0 * under(0.1),
            100.0 * under(0.5)
        );
        for (d, p) in v.iter().take(5) {
            println!("   {d:.3} mm at ({:.2}, {:.2}, {:.2})", p.x, p.y, p.z);
        }
    }
}

/// The distance from `p` to triangle `t`.
fn point_triangle(p: &Point3<f64>, t: &[Point3<f64>; 3]) -> f64 {
    let (a, b, c) = (t[0], t[1], t[2]);
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(&ap), ac.dot(&ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return (p - a).norm();
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(&bp), ac.dot(&bp));
    if d3 >= 0.0 && d4 <= d3 {
        return (p - b).norm();
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return (p - (a + ab * v)).norm();
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(&cp), ac.dot(&cp));
    if d6 >= 0.0 && d5 <= d6 {
        return (p - c).norm();
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return (p - (a + ac * w)).norm();
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return (p - (b + (c - b) * w)).norm();
    }
    let denom = 1.0 / (va + vb + vc);
    let (v, w) = (vb * denom, vc * denom);
    (p - (a + ab * v + ac * w)).norm()
}
