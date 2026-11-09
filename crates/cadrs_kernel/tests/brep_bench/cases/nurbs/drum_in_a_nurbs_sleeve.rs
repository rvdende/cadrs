//! The drum of radius 5 about the z axis from z = 0 to 10 united with a sleeve round its middle
//! (between radii 5 and 7, 2 ≤ z ≤ 8), whose bore is the drum's cylinder written as a rational
//! B-spline surface (the nine-point quadratic circle round the axis, a line along it, its seam
//! on the line through (0, 5)).
//!
//! The drum is a circle extruded. The sleeve is the ring between the circles of radii 7 and 5
//! extruded 6 from z = 2 (sketched with its x axis along +y, so the bore's seam is the B-spline's
//! own, on the line through (0, 5)), its bore then given the B-spline with Replace face (its
//! parameters the net's own, 0 to 1 each way); the two united.
use crate::Refusal;
use crate::bench::{cylinder, extrude, face_at, p3, plane, round, sketch_with_holes, union, v3};
use crate::shared::nurbs::spline_surface;
use crate::shared::sweep::first;
use cadrs_kernel::{BodyId, DefaultKernel, FaceReplacement, Kernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let drum = cylinder(k, p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 5.0, 10.0)?;
    let on = plane(p3(0.0, 0.0, 2.0), v3(0.0, 1.0, 0.0), v3(0.0, 0.0, 1.0));
    let ring = sketch_with_holes(on, round((0.0, 0.0), 7.0), vec![round((0.0, 0.0), 5.0)]);
    let sleeve = extrude(k, &ring, 6.0)?;
    let h = 2f64.sqrt() / 2.0;
    let around = [
        (0.0, 5.0),
        (-5.0, 5.0),
        (-5.0, 0.0),
        (-5.0, -5.0),
        (0.0, -5.0),
        (5.0, -5.0),
        (5.0, 0.0),
        (5.0, 5.0),
        (0.0, 5.0),
    ];
    let w = [1.0, h, 1.0, h, 1.0, h, 1.0, h, 1.0];
    let points: Vec<Vec<[f64; 3]>> = around
        .iter()
        .map(|&(x, y)| vec![[x, y, 2.0], [x, y, 8.0]])
        .collect();
    let weights: Vec<Vec<f64>> = w.iter().map(|&wi| vec![wi, wi]).collect();
    let bore = spline_surface(
        2,
        1,
        vec![
            0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0,
        ],
        vec![0.0, 0.0, 1.0, 1.0],
        &points,
        Some(&weights),
    )?;
    let face = face_at(k, sleeve, p3(5.0, 0.0, 5.0))?;
    let sleeve = first(k.replace_faces(
        sleeve,
        &[FaceReplacement {
            face,
            surface: bore,
            domain: Some(((0.0, 1.0), (0.0, 1.0))),
        }],
    )?)?;
    union(k, drum, sleeve)
}
