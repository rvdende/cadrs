//! The regular tetrahedron with corners (3, 3, 3), (3, −3, −3), (−3, 3, −3) and (−3, −3, 3),
//! grown by 1: the solid of every point within 1 of it. Each face moves 1 out, each edge becomes
//! a piece of a cylinder of radius 1 and each corner a piece of a ball.
//!
//! The tetrahedron is a loft from a sketched triangle, its base through three of the corners,
//! to the fourth corner as a point (cadrs has no tetrahedron, nor any polyhedron, primitive; a
//! loft to a point is how its Part Studio makes a pyramid). Then cadrs's offset with rounded
//! edges.
use crate::Refusal;
use crate::bench::{P3, p2, p3, plane, polygon, sketch};
use crate::shared::offset::grown;
use cadrs_kernel::{BodyId, BodyKind, DefaultKernel, Kernel, LoftEnd, LoftSection, LoftSpec};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let apex = p3(3.0, 3.0, 3.0);
    let base: [P3; 3] = [
        p3(3.0, -3.0, -3.0),
        p3(-3.0, 3.0, -3.0),
        p3(-3.0, -3.0, 3.0),
    ];
    // The sketch plane through the base's corners, its normal toward the apex.
    let normal = (base[1] - base[0]).cross(&(base[2] - base[0]));
    let normal = if normal.dot(&(apex - base[0])) < 0.0 {
        -normal
    } else {
        normal
    };
    let on = plane(base[0], base[1] - base[0], normal);
    let y_dir = on.normal.cross(&on.x_dir);
    let at = |p: P3| {
        let d = p - on.origin;
        p2(d.dot(&on.x_dir), d.dot(&y_dir))
    };
    let corners: Vec<(f64, f64)> = base.iter().map(|&p| at(p)).map(|q| (q.x, q.y)).collect();
    let triangle = sketch(on, vec![polygon(&corners)]);
    let spec = LoftSpec {
        body: BodyKind::Solid,
        sections: vec![LoftSection::Profile(triangle), LoftSection::Point(apex)],
        start: LoftEnd::default(),
        end: LoftEnd::default(),
        source: 1,
    };
    let tetrahedron = k.loft_with(&spec)?.bodies[0];
    grown(k, tetrahedron, 1.0)
}
