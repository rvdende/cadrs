//! A four-armed star handle: a flat star outline sketched on the Front plane (sketch (x, y) the
//! model's (x, z)), extruded 12 along +y, its sides then drafted 2° in about the plane y = 0.
//!
//! The outline: four tip circles of radius 4 centered 33.1 out on the axes, four fillet circles
//! of radius 3 centered 15.9 out on the diagonals, and each arm's sides the lines tangent to its
//! tip circle and the fillet circle beside it, passing between them. Going around: a tip's outer
//! arc, a side, a fillet's inner arc (clockwise, facing the origin), a side, the next tip's arc.
//! The tangent points are where a sketch's tangency constraints put them, worked out here as a
//! sketch solver would.
use crate::Refusal;
use crate::bench::{V3, arc, chain, extrude, faces_where, line, p2, p3, sketch, unit, xz};
use cadrs_kernel::{BodyId, DefaultKernel, DraftSpec, Kernel};
use nalgebra::Vector2;
use std::f64::consts::FRAC_1_SQRT_2;

const TIP_DISTANCE: f64 = 33.1;
const TIP_RADIUS: f64 = 4.0;
const FILLET_DISTANCE: f64 = 15.9;
const FILLET_RADIUS: f64 = 3.0;

type V2 = Vector2<f64>;

/// `v` turned `k` quarter turns counter-clockwise.
fn quarter(v: V2, k: usize) -> V2 {
    match k % 4 {
        0 => v,
        1 => V2::new(-v.y, v.x),
        2 => -v,
        _ => V2::new(v.y, -v.x),
    }
}

/// `v` mirrored in the diagonal u = v.
fn swap(v: V2) -> V2 {
    V2::new(v.y, v.x)
}

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let tip = V2::new(TIP_DISTANCE, 0.0);
    let d = FILLET_DISTANCE * FRAC_1_SQRT_2;
    let fillet = V2::new(d, d);
    // The first arm's side toward the diagonal is the line n·p = c with the tip circle a radius
    // on its positive side and the fillet circle a radius on its negative, so that
    // n·(tip − fillet) is the sum of the radii; of the two such lines, the one touching the tip
    // circle on the diagonal's side of the axis.
    let between = tip - fillet;
    let turn = between.y.atan2(between.x) - ((TIP_RADIUS + FILLET_RADIUS) / between.norm()).acos();
    let n = V2::new(turn.cos(), turn.sin());
    let on_tip = tip - n * TIP_RADIUS;
    let on_fillet = fillet + n * FILLET_RADIUS;
    // the tip's arc runs counter-clockwise from −a to a across the axis; the fillet's clockwise
    // from f to its mirror image in the diagonal, 90° − f
    let a = (on_tip - tip).y.atan2((on_tip - tip).x).to_degrees();
    let f = (on_fillet - fillet)
        .y
        .atan2((on_fillet - fillet).x)
        .to_degrees();
    let fillet_sweep = (90.0 - 2.0 * f).rem_euclid(360.0) - 360.0;
    let pt = |v: V2| p2(v.x, v.y);
    let mut curves = Vec::new();
    for q in 0..4 {
        let turn = 90.0 * q as f64;
        curves.push(arc(pt(quarter(tip, q)), TIP_RADIUS, turn - a, 2.0 * a));
        curves.push(line(pt(quarter(on_tip, q)), pt(quarter(on_fillet, q))));
        curves.push(arc(
            pt(quarter(fillet, q)),
            FILLET_RADIUS,
            turn + f,
            fillet_sweep,
        ));
        curves.push(line(
            pt(quarter(swap(on_fillet), q)),
            pt(quarter(swap(on_tip), q)),
        ));
    }
    // the Front plane's normal is −y: extrude against it, along +y
    let star = extrude(k, &sketch(xz(0.0), vec![chain(curves)]), -12.0)?;
    // the sides: every face but the two ends square to y
    let sides = faces_where(k, star, |f| f.plane.is_none_or(|p| p.normal.y.abs() < 0.5))?;
    let spec = DraftSpec {
        faces: sides,
        angle: 2f64.to_radians(),
        pull: unit(V3::y()),
        neutral: p3(0.0, 0.0, 0.0),
        neutral_normal: None,
        tangent_propagation: true,
    };
    k.draft(star, &spec)?
        .bodies
        .first()
        .copied()
        .ok_or_else(|| Refusal::from("the draft made no body"))
}
