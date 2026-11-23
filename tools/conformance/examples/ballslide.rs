//! Scratch: brep-bench's swept/ball-turned-while-slid: a ball of radius 1 at (5, 0, 0) turned half a
//! turn about z while slid 25 along x; its faces and volume against the exact 61.3253759732.
use cadrs_kernel::{Axis, DefaultKernel, Kernel, Primitive, RigidStep, SolidMotion};
use nalgebra::{Point3, Vector3};

fn main() {
    let mut k = DefaultKernel::new();
    let b = k
        .primitive(&Primitive::ball(Point3::new(5.0, 0.0, 0.0), 1.0))
        .unwrap()
        .bodies[0];
    let steps = vec![
        RigidStep::Turn {
            axis: Axis::new(Point3::origin(), Vector3::z()),
            from: 0.0,
            to: std::f64::consts::PI,
        },
        RigidStep::Slide {
            from: Vector3::zeros(),
            to: Vector3::new(25.0, 0.0, 0.0),
        },
    ];
    let t = std::time::Instant::now();
    let id = k
        .sweep_solid(b, &SolidMotion::Rigid { steps })
        .unwrap()
        .bodies[0];
    println!("swept in {:?}", t.elapsed());
    let body = k.get(id).unwrap();
    for (fi, f) in body.faces().iter().enumerate() {
        let s = format!("{:?}", f.surf);
        println!("face {fi}: {}", &s[..s.len().min(140)]);
    }
    println!(
        "volume {:?} exact 61.3253759732",
        k.mass_properties(id).map(|m| m.volume)
    );
}
