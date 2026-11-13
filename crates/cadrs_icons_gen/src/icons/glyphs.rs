//! Constraint glyphs and viewport markers (kind Glyph: INK only, stroke 2).
//!
//! The constraint glyphs are shown at 12-20 px next to sketch geometry and get tinted by the app, so
//! they share a small symbolic vocabulary:
//! - entities (lines, curves) are 2-unit strokes;
//! - points are filled dots of radius DOT (the constrained point a little bigger, BIG);
//! - the sketch plane is one flat parallelogram (plane()), used by Pierce and Use;
//! - outside or reference geometry is a row of round dots (DOTS).
//!
//! The viewport markers come in layered pairs that share their geometry:
//! - manipulator-arrow-halo (behind, dark translucent) + manipulator-arrow-line (on top, white or
//!   the hover colour). The arrow points up (the app rotates it) and spans the full height, so the
//!   node's centre is the arrow's midpoint.
//! - center-of-mass-disc (behind, white) + center-of-mass-quarters (on top, near black).

use crate::common::*;
use Kind::Glyph;

pub fn icons() -> Vec<Def> {
    vec![
        def(
            "constraint-coincident",
            Glyph,
            "Coincident",
            constraint_coincident,
        ),
        def(
            "constraint-concentric",
            Glyph,
            "Concentric",
            constraint_concentric,
        ),
        def(
            "constraint-parallel",
            Glyph,
            "Parallel",
            constraint_parallel,
        ),
        def("constraint-tangent", Glyph, "Tangent", constraint_tangent),
        def(
            "constraint-horizontal",
            Glyph,
            "Horizontal",
            constraint_horizontal,
        ),
        def(
            "constraint-vertical",
            Glyph,
            "Vertical",
            constraint_vertical,
        ),
        def(
            "constraint-perpendicular",
            Glyph,
            "Perpendicular",
            constraint_perpendicular,
        ),
        def("constraint-equal", Glyph, "Equal", constraint_equal),
        def(
            "constraint-midpoint",
            Glyph,
            "Midpoint",
            constraint_midpoint,
        ),
        def("constraint-normal", Glyph, "Normal", constraint_normal),
        def("constraint-pierce", Glyph, "Pierce", constraint_pierce),
        def(
            "constraint-symmetric",
            Glyph,
            "Symmetric",
            constraint_symmetric,
        ),
        def("constraint-fix", Glyph, "Fix", constraint_fix),
        def(
            "constraint-curvature",
            Glyph,
            "Curvature",
            constraint_curvature,
        ),
        def("constraint-offset", Glyph, "Offset", constraint_offset),
        def(
            "constraint-use",
            Glyph,
            "Use (projected geometry)",
            constraint_use,
        ),
        def(
            "manipulator-arrow-line",
            Glyph,
            "Manipulator arrow",
            manipulator_arrow_line,
        ),
        def(
            "manipulator-arrow-halo",
            Glyph,
            "Manipulator arrow halo",
            manipulator_arrow_halo,
        ),
        def(
            "center-of-mass-disc",
            Glyph,
            "Center of mass disc",
            center_of_mass_disc,
        ),
        def(
            "center-of-mass-quarters",
            Glyph,
            "Center of mass quarters",
            center_of_mass_quarters,
        ),
        def("origin", Glyph, "Origin", origin),
        def(
            "instance-dof",
            Glyph,
            "Under-constrained instance",
            instance_dof,
        ),
        def("mate-limits", Glyph, "Mate limits", mate_limits),
        def(
            "origin-fastened",
            Glyph,
            "Fastened to origin",
            origin_fastened,
        ),
    ]
}

const DOT: f64 = 2.5; // radius of an end point
const BIG: f64 = 3.3; // radius of the point a constraint is about
const DOTS: &str = r#"stroke-dasharray="0.01 4""#; // round dots along a stroke (round caps make the dots)

const AC: &str = r##"stroke="#2563eb""##; // an accent stroke

fn dot(x: f64, y: f64, r: f64, color: &str) -> String {
    circle(x, y, r, color, r#"stroke="none""#)
}

/// An arc path from angle a0 to a1 (degrees, clockwise on screen).
fn arc(cx: f64, cy: f64, r: f64, a0: f64, a1: f64, extra: &str) -> String {
    let (x0, y0) = (cx + r * rad(a0).cos(), cy + r * rad(a0).sin());
    let (x1, y1) = (cx + r * rad(a1).cos(), cy + r * rad(a1).sin());
    let large = if (a1 - a0).abs() > 180.0 { 1 } else { 0 };
    let sweep = if a1 > a0 { 1 } else { 0 };
    path(
        &format!(
            "M{} {} A{} {} 0 {large} {sweep} {} {}",
            f(x0),
            f(y0),
            f(r),
            f(r),
            f(x1),
            f(y1)
        ),
        "",
        extra,
    )
}

/// The sketch plane: a flat parallelogram seen from slightly above.
fn plane(y0: f64, y1: f64) -> String {
    let (x0, x1, skew) = (3.0, 21.0, 4.5);
    poly(
        &[(x0, y1), (x1 - skew, y1), (x1, y0), (x0 + skew, y0)],
        "",
        "",
    )
}

/// The hatched ground under constraint-fix and origin-fastened.
fn ground() -> Vec<String> {
    [8.5, 14.5, 20.5]
        .iter()
        .map(|&x| line(x, 16.5, x - 3.5, 21.0, ""))
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Constraints

fn constraint_coincident() -> Vec<String> {
    // Two entities through one shared point.
    els![
        line(4.0, 19.0, 20.0, 5.0, ""),
        line(4.0, 5.0, 20.0, 19.0, ""),
        dot(12.0, 12.0, BIG + 0.3, ACCENT)
    ]
}

fn constraint_concentric() -> Vec<String> {
    els![
        circle(12.0, 12.0, 9.0, "", ""),
        circle(12.0, 12.0, 4.3, "", AC)
    ]
}

fn constraint_parallel() -> Vec<String> {
    els![
        line(4.5, 19.5, 11.0, 4.5, ""),
        line(13.0, 19.5, 19.5, 4.5, AC)
    ]
}

fn constraint_tangent() -> Vec<String> {
    let r = 6.2;
    let top = 7.5;
    els![
        circle(12.0, top + r, r, "", ""),
        line(3.0, top, 21.0, top, ""),
        dot(12.0, top, 2.9, ACCENT)
    ]
}

fn constraint_horizontal() -> Vec<String> {
    els![
        line(5.0, 12.0, 19.0, 12.0, ""),
        dot(4.5, 12.0, DOT, ACCENT),
        dot(19.5, 12.0, DOT, ACCENT)
    ]
}

fn constraint_vertical() -> Vec<String> {
    els![
        line(12.0, 5.0, 12.0, 19.0, ""),
        dot(12.0, 4.5, DOT, ACCENT),
        dot(12.0, 19.5, DOT, ACCENT)
    ]
}

fn constraint_perpendicular() -> Vec<String> {
    // The right-angle mark goes underneath, so the ink lines stay whole.
    els![
        polyline(&[(9.0, 13.5), (15.0, 13.5), (15.0, 19.5)], AC),
        line(3.5, 19.5, 20.5, 19.5, ""),
        line(9.0, 4.0, 9.0, 19.5, ""),
    ]
}

fn constraint_equal() -> Vec<String> {
    let w = r#"stroke-width="2.4""#;
    els![
        line(5.0, 8.5, 19.0, 8.5, w),
        line(5.0, 15.5, 19.0, 15.5, &format!("{w} {AC}"))
    ]
}

fn constraint_midpoint() -> Vec<String> {
    // A segment between end ticks, its centre marked.
    els![
        line(3.0, 12.0, 21.0, 12.0, ""),
        line(3.0, 7.5, 3.0, 16.5, ""),
        line(21.0, 7.5, 21.0, 16.5, ""),
        dot(12.0, 12.0, BIG, ACCENT),
    ]
}

fn constraint_normal() -> Vec<String> {
    // A line leaving a curve at a right angle.
    els![
        line(12.0, 3.5, 12.0, 17.0, AC),
        arc(12.0, 31.0, 14.0, 222.0, 318.0, "")
    ]
}

fn constraint_pierce() -> Vec<String> {
    // A curve passing through the sketch plane; the pierce point marked.
    els![
        plane(10.0, 20.0),
        line(12.0, 2.5, 12.0, 10.0, ""),
        line(12.0, 20.0, 12.0, 22.0, ""),
        dot(12.0, 15.0, 2.8, ACCENT)
    ]
}

fn constraint_symmetric() -> Vec<String> {
    // Two mirrored shapes about a dotted axis.
    els![
        line(12.0, 3.0, 12.0, 21.0, &format!("{DOTS} {AC}")),
        poly(&[(8.5, 6.5), (8.5, 17.5), (3.0, 12.0)], INK, ""),
        poly(&[(15.5, 6.5), (15.5, 17.5), (21.0, 12.0)], INK, ""),
    ]
}

fn constraint_fix() -> Vec<String> {
    // A fixed support, as in a structural diagram: a wedge on hatched ground.
    els![
        poly(&[(12.0, 4.0), (17.5, 13.0), (6.5, 13.0)], ACCENT, AC),
        line(3.5, 16.5, 20.5, 16.5, ""),
        ground()
    ]
}

fn constraint_curvature() -> Vec<String> {
    // Two curves meeting at a point with matching curvature (a smooth S through the join).
    let a = sample_cubic((3.0, 19.0), (9.0, 19.0), (8.5, 12.0), (12.0, 12.0), 16);
    let b = sample_cubic((12.0, 12.0), (15.5, 12.0), (15.0, 5.0), (21.0, 5.0), 16);
    let s = cat(&[&a[..a.len() - 1], &b]);
    els![polyline(&s, ""), dot(12.0, 12.0, BIG, ACCENT)]
}

fn constraint_offset() -> Vec<String> {
    // Two curves a constant distance apart.
    els![
        arc(20.0, 20.0, 16.5, 180.0, 270.0, ""),
        arc(20.0, 20.0, 8.5, 180.0, 270.0, AC)
    ]
}

fn constraint_use() -> Vec<String> {
    // An outside edge brought down onto the sketch plane.
    els![
        line(7.0, 3.5, 17.0, 3.5, ""),
        arrow(12.0, 7.0, 12.0, 12.5, 4.5, 6.0, ACCENT, ""),
        plane(14.0, 20.5)
    ]
}

// ---------------------------------------------------------------------------------------------
// Viewport markers

// The manipulator arrow's geometry, shared by both layers: tip up at (12, TIP), a filled head
// HEAD long and W wide, a shaft down to BASE. The halo is the same geometry stroked HALO wider on
// each side, so it rings the line layer exactly.
const ARROW_TIP: f64 = 3.6;
const ARROW_HEAD: f64 = 8.0;
const ARROW_W: f64 = 9.0;
const ARROW_BASE: f64 = 19.8;
const ARROW_SHAFT: f64 = 2.4;
const HALO: f64 = 1.4;

fn manipulator_arrow(grow: f64) -> Vec<String> {
    let hb = ARROW_TIP + ARROW_HEAD;
    let head = poly(
        &[
            (12.0, ARROW_TIP),
            (12.0 + ARROW_W / 2.0, hb),
            (12.0 - ARROW_W / 2.0, hb),
        ],
        INK,
        &format!(r#"stroke-width="{}""#, f(0.8 + 2.0 * grow)),
    );
    let shaft = line(
        12.0,
        hb - 1.0,
        12.0,
        ARROW_BASE,
        &format!(r#"stroke-width="{}""#, f(ARROW_SHAFT + 2.0 * grow)),
    );
    els![shaft, head]
}

fn manipulator_arrow_line() -> Vec<String> {
    manipulator_arrow(0.0)
}

fn manipulator_arrow_halo() -> Vec<String> {
    manipulator_arrow(HALO)
}

const COM_R: f64 = 9.5; // the ring's centreline; its outer edge is at 10.5

fn center_of_mass_disc() -> Vec<String> {
    // Ends inside the ring's outer edge so no light fringe shows around the symbol.
    els![circle(12.0, 12.0, COM_R + 0.5, INK, r#"stroke="none""#)]
}

fn center_of_mass_quarters() -> Vec<String> {
    let r = COM_R;
    els![
        path(
            &format!(
                "M12 12 V{} A{} {} 0 0 1 {} 12 Z",
                f(12.0 - r),
                f(r),
                f(r),
                f(12.0 + r)
            ),
            INK,
            r#"stroke="none""#
        ),
        path(
            &format!(
                "M12 12 V{} A{} {} 0 0 1 {} 12 Z",
                f(12.0 + r),
                f(r),
                f(r),
                f(12.0 - r)
            ),
            INK,
            r#"stroke="none""#
        ),
        circle(12.0, 12.0, r, "", ""),
    ]
}

fn origin() -> Vec<String> {
    // A ring on crossed axis ticks, the origin point filled.
    els![
        circle(12.0, 12.0, 6.0, "", ""),
        dot(12.0, 12.0, 2.6, ACCENT),
        line(12.0, 2.0, 12.0, 5.0, ""),
        line(12.0, 19.0, 12.0, 22.0, ""),
        line(2.0, 12.0, 5.0, 12.0, ""),
        line(19.0, 12.0, 22.0, 12.0, ""),
    ]
}

fn instance_dof() -> Vec<String> {
    // A small three-axis triad: the degrees of freedom an instance still has. The origin is
    // filled; X runs right, Y up and Z out toward the viewer (down and to the left).
    let (ox, oy) = (10.0, 14.0);
    els![
        arrow(ox, oy, 21.0, 14.0, 4.2, 4.2, INK, ""),
        arrow(ox, oy, 10.0, 3.0, 4.2, 4.2, INK, ""),
        arrow(ox, oy, 3.6, 20.4, 4.2, 4.2, INK, ""),
        dot(ox, oy, BIG, ACCENT),
    ]
}

fn mate_limits() -> Vec<String> {
    // A range with stops: two short bars and a double arrow between them.
    els![
        line(6.5, 3.0, 17.5, 3.0, ""),
        line(6.5, 21.0, 17.5, 21.0, ""),
        arrow(12.0, 12.0, 12.0, 6.8, 3.6, 5.0, ACCENT, ""),
        arrow(12.0, 12.0, 12.0, 17.2, 3.6, 5.0, ACCENT, ""),
    ]
}

fn origin_fastened() -> Vec<String> {
    // The origin (a ring round a filled point) standing on hatched ground: an assembly fixed in
    // place at the origin. The ground matches constraint-fix; the ring sets it apart.
    els![
        circle(12.0, 7.5, 5.0, "", ""),
        dot(12.0, 7.5, 2.2, ACCENT),
        line(12.0, 12.5, 12.0, 16.5, ""),
        line(3.5, 16.5, 20.5, 16.5, ""),
        ground(),
    ]
}
