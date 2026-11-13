//! Assembly mates, relations and tools, and the document types (kind Solid).
//!
//! Roles: context geometry is neutral (TOP/SOFT/MID/SHADE); ACCENT marks only what explains the tool.
//! Mates show two parts (the fixed one lit a step darker, SOFT top) and ACCENT for the motion the mate
//! allows or the faces it constrains. Relations accent the coupled motion. The document types share one
//! base, a square platform, with the type's emblem on it in ACCENT.

use crate::common::*;
use Kind::Solid;
use std::f64::consts::PI;

pub fn icons() -> Vec<Def> {
    vec![
        def("mate-fastened", Solid, "Fastened mate", mate_fastened),
        def("mate-revolute", Solid, "Revolute mate", mate_revolute),
        def("mate-slider", Solid, "Slider mate", mate_slider),
        def("mate-planar", Solid, "Planar mate", mate_planar),
        def(
            "mate-cylindrical",
            Solid,
            "Cylindrical mate",
            mate_cylindrical,
        ),
        def("mate-pin-slot", Solid, "Pin slot mate", mate_pin_slot),
        def("mate-ball", Solid, "Ball mate", mate_ball),
        def("mate-parallel", Solid, "Parallel mate", mate_parallel),
        def("mate-tangent", Solid, "Tangent mate", mate_tangent),
        def("mate-width", Solid, "Width mate", mate_width),
        def("relations", Solid, "Relations", relations),
        def("gear-relation", Solid, "Gear relation", gear_relation),
        def(
            "rack-pinion",
            Solid,
            "Rack and pinion relation",
            rack_pinion,
        ),
        def("screw-relation", Solid, "Screw relation", screw_relation),
        def("linear-relation", Solid, "Linear relation", linear_relation),
        def("group", Solid, "Group", group),
        def("replicate", Solid, "Replicate", replicate),
        def("explode", Solid, "Explode", explode),
        def("named-positions", Solid, "Named positions", named_positions),
        def("display-states", Solid, "Display states", display_states),
        def(
            "assembly-properties",
            Solid,
            "Assembly properties",
            assembly_properties,
        ),
        def(
            "bill-of-materials",
            Solid,
            "Bill of materials",
            bill_of_materials,
        ),
        def(
            "standard-content",
            Solid,
            "Standard content",
            standard_content,
        ),
        def("assembly", Solid, "Assembly", assembly),
        def("assembly-rigid", Solid, "Rigid subassembly", assembly_rigid),
        def("part-studio", Solid, "Part Studio", part_studio),
        def(
            "material-library",
            Solid,
            "Material library",
            material_library,
        ),
        def("feature-studio", Solid, "Feature Studio", feature_studio),
        def("variable-studio", Solid, "Variable Studio", variable_studio),
        def("cam-studio", Solid, "CAM Studio", cam_studio),
        def("pcb-studio", Solid, "PCB Studio", pcb_studio),
        def("render-studio", Solid, "Render Studio", render_studio),
    ]
}

fn r15() -> f64 {
    1.5f64.sqrt()
}

fn r05() -> f64 {
    0.5f64.sqrt()
}

const THIN: &str = r#"stroke-width="0.8""#;

fn acc(extra: &str) -> String {
    if extra.is_empty() {
        format!(r#"stroke="{ACCENT}""#)
    } else {
        format!(r#"stroke="{ACCENT}" {extra}"#)
    }
}

/// A box in the ground colours: SOFT top, MID and SHADE sides.
#[allow(clippy::too_many_arguments)]
fn ground_box(o: P, x0: f64, x1: f64, y0: f64, y1: f64, z0: f64, z1: f64) -> String {
    iso_box_with(o, x0, x1, y0, y1, z0, z1, SOFT, MID, SHADE, "")
}

/// Convex hull (monotone chain) of screen points, rounded to 3 decimals.
fn hull(ps: &[P]) -> Vec<P> {
    let mut ps: Vec<P> = ps.iter().map(|&(x, y)| (round3(x), round3(y))).collect();
    ps.sort_by(|a, b| a.partial_cmp(b).unwrap());
    ps.dedup();
    if ps.len() < 3 {
        return ps;
    }
    let half = |seq: &mut dyn Iterator<Item = P>| {
        let mut out: Vec<P> = Vec::new();
        for p in seq {
            while out.len() >= 2 && {
                let (a, b) = (out[out.len() - 2], out[out.len() - 1]);
                (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0) <= 0.0
            } {
                out.pop();
            }
            out.push(p);
        }
        out
    };
    let (lo, hi) = (
        half(&mut ps.iter().copied()),
        half(&mut ps.iter().rev().copied()),
    );
    cat(&[&lo[..lo.len() - 1], &hi[..hi.len() - 1]])
}

/// Screen points on a horizontal circle, by ellipse angle (0 = screen right, 90 = front).
#[allow(clippy::too_many_arguments)]
fn ring(o: P, cx: f64, cy: f64, z: f64, r: f64, d0: f64, d1: f64, step: f64) -> Vec<P> {
    let (x, y) = projector(o)(cx, cy, z);
    arc_points_step(x, y, r * r15(), r * r05(), d0, d1, step)
}

/// A curved arrow around a vertical axis, across the front of it.
#[allow(clippy::too_many_arguments)]
fn spin(o: P, cx: f64, cy: f64, z: f64, r: f64, d0: f64, d1: f64, head: f64, width: f64) -> String {
    curved_arrow(&ring(o, cx, cy, z, r, d0, d1, 5.0), head, width, ACCENT, "")
}

/// A double-headed accent arrow from a to b.
fn darrow(a: P, b: P, head: f64, width: f64) -> Vec<String> {
    let m = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    els![
        arrow(m.0, m.1, a.0, a.1, head, width, ACCENT, ""),
        arrow(m.0, m.1, b.0, b.1, head, width, ACCENT, "")
    ]
}

/// A point on the circle of radius r around (cx, cy), at 2*pi*i/n.
fn around(cx: f64, cy: f64, r: f64, i: usize, n: usize) -> P {
    let t = 2.0 * PI * i as f64 / n as f64;
    (cx + r * t.cos(), cy + r * t.sin())
}

/// A sphere lit from above-right: a shaded crescent at the lower left.
fn sphere(cx: f64, cy: f64, r: f64, fill: &str, shade: &str) -> Vec<String> {
    let n = 48;
    let body: Vec<P> = (0..n).map(|i| around(cx, cy, r, i, n)).collect();
    let k = r * 0.3;
    let lit: Vec<P> = (0..n)
        .map(|i| around(cx + k, cy - k, r * 0.92, i, n))
        .collect();
    els![
        circle(cx, cy, r, shade, r#"stroke="none""#),
        poly(&clip(&lit, &body), fill, r#"stroke="none""#),
        circle(cx, cy, r, "", "")
    ]
}

/// A cylinder lying along x; the +x end cap is visible.
fn xcyl(o: P, x0: f64, x1: f64, cy: f64, cz: f64, r: f64) -> Vec<String> {
    let p = projector(o);
    let n = 48;
    let end = |x: f64| {
        (0..n)
            .map(|i| around(cy, cz, r, i, n))
            .map(|(y, z)| p(x, y, z))
            .collect::<Vec<P>>()
    };
    els![
        poly(&hull(&cat(&[&end(x0), &end(x1)])), TOP, ""),
        poly(&end(x1), MID, "")
    ]
}

/// A gear outline in a plane, as (u, v) points.
fn gear_outline(cx: f64, cy: f64, r_out: f64, r_root: f64, teeth: usize, phase: f64) -> Vec<P> {
    let mut out = Vec::new();
    let w = 2.0 * PI / teeth as f64;
    for k in 0..teeth {
        let a = w * (k as f64 + phase);
        for (t, r) in [(0.0, r_root), (0.18, r_out), (0.5, r_out), (0.68, r_root)] {
            out.push((cx + r * (a + t * w).cos(), cy + r * (a + t * w).sin()));
        }
    }
    out
}

/// A gear lying flat, extruded from z0 to z1, with a hole.
#[allow(clippy::too_many_arguments)]
fn flat_gear(
    o: P,
    cx: f64,
    cy: f64,
    z0: f64,
    z1: f64,
    r_out: f64,
    r_root: f64,
    teeth: usize,
    phase: f64,
) -> Vec<String> {
    let p = projector(o);
    let g = gear_outline(cx, cy, r_out, r_root, teeth, phase);
    let at = |z: f64| g.iter().map(|&(u, v)| p(u, v, z)).collect::<Vec<P>>();
    let mut out = vec![poly(&at(z0), MID, "")];
    let steps = 4;
    for i in 1..steps {
        let z = z0 + (z1 - z0) * i as f64 / steps as f64;
        out.push(poly(&at(z), MID, r#"stroke="none""#));
    }
    out.push(poly(&at(z1), TOP, ""));
    out.push(poly(
        &ring(o, cx, cy, z1, r_root * 0.32, 0.0, 360.0, 10.0),
        SHADE,
        THIN,
    ));
    out
}

const TINT: &str = r#"stroke="none" fill-opacity="0.45""#;

/// A box marked as the input or emblem: neutral faces under an accent wash.
#[allow(clippy::too_many_arguments)]
fn tint_box(o: P, x0: f64, x1: f64, y0: f64, y1: f64, z0: f64, z1: f64) -> Vec<String> {
    els![
        iso_box(o, x0, x1, y0, y1, z0, z1),
        iso_box_with(o, x0, x1, y0, y1, z0, z1, ACCENT, ACCENT, ACCENT, TINT)
    ]
}

fn tint_cyl(o: P, cx: f64, cy: f64, z0: f64, z1: f64, r: f64) -> Vec<String> {
    els![
        cylinder(o, cx, cy, z0, z1, r),
        cylinder_with(o, cx, cy, z0, z1, r, ACCENT, ACCENT, TINT)
    ]
}

// ---------------------------------------------------------------------------------------------
// Mates: no bases. A thin floor rhombus (no thickness) is the only context where one is needed,
// the parts are big, and the accent motion arrows keep clear of every body.

const FLOOR: &str = r#"fill-opacity="0.55" stroke-width="0.8""#;

fn floor(o: P, x0: f64, x1: f64, y0: f64, y1: f64) -> String {
    let p = projector(o);
    poly(
        &[
            p(x0, y0, 0.0),
            p(x1, y0, 0.0),
            p(x1, y1, 0.0),
            p(x0, y1, 0.0),
        ],
        SOFT,
        FLOOR,
    )
}

fn mate_fastened() -> Vec<String> {
    let o = (12.0, 9.2);
    let p = projector(o);
    let (a, b) = (2.4, 8.6);
    els![
        floor(o, 0.0, 11.0, 0.0, 11.0),
        iso_box(o, a, b, a, b, 0.0, 6.6),
        polyline(
            &[p(a, b, 0.0), p(b, b, 0.0), p(b, a, 0.0)],
            &acc(r#"stroke-width="2.2""#)
        ),
    ]
}

fn mate_revolute() -> Vec<String> {
    let o = (12.0, 15.6);
    els![
        cylinder_with(o, 0.0, 0.0, 2.6, 11.5, 1.3, SOFT, MID, ""),
        cylinder(o, 0.0, 0.0, 5.0, 7.8, 4.4),
        cylinder_with(o, 0.0, 0.0, 7.8, 11.5, 1.3, SOFT, MID, ""),
        spin(o, 0.0, 0.0, 1.2, 5.2, 160.0, 22.0, 2.8, 2.6),
    ]
}

fn mate_slider() -> Vec<String> {
    let o = (15.2, 9.6);
    let p = projector(o);
    els![
        iso_box(o, 0.0, 6.0, 0.0, 6.0, 0.0, 6.0),
        darrow(p(-4.4, 9.4, 0.0), p(9.2, 9.4, 0.0), 3.0, 2.8)
    ]
}

fn mate_planar() -> Vec<String> {
    // The floor plane with a 4-way cross along its diagonals (screen horizontal and vertical).
    let o = (12.0, 6.2);
    let p = projector(o);
    let s = 11.5;
    let (cx, cy) = p(s / 2.0, s / 2.0, 0.0);
    let ends = [
        (1.6, 0.0, 8.8, 0.0),
        (-1.6, 0.0, -8.8, 0.0),
        (0.0, 1.3, 0.0, 5.3),
        (0.0, -1.3, 0.0, -5.3),
    ];
    let arrows: Vec<String> = ends
        .iter()
        .map(|&(a, b, c, d)| arrow(cx + a, cy + b, cx + c, cy + d, 2.8, 2.7, ACCENT, ""))
        .collect();
    els![floor(o, 0.0, s, 0.0, s), arrows]
}

fn mate_cylindrical() -> Vec<String> {
    let o = (10.6, 17.9);
    let p = projector(o);
    let ax = p(0.0, 0.0, 0.0).0 + 7.6;
    els![
        cylinder_with(o, 0.0, 0.0, 3.2, 14.5, 1.3, SOFT, MID, ""),
        cylinder(o, 0.0, 0.0, 6.2, 10.2, 3.0),
        cylinder_with(o, 0.0, 0.0, 10.2, 14.5, 1.3, SOFT, MID, ""),
        spin(o, 0.0, 0.0, 1.4, 4.2, 165.0, 25.0, 2.8, 2.6),
        darrow(
            (ax, p(0.0, 0.0, 3.4).1),
            (ax, p(0.0, 0.0, 14.4).1),
            2.8,
            2.6
        ),
    ]
}

fn mate_pin_slot() -> Vec<String> {
    let o = (9.9, 7.4);
    let p = projector(o);
    let (y, r, x0, x1) = (3.4, 1.7, 2.3, 11.7);
    let on = |x: f64, a: i32| (x + r * rad(a as f64).cos(), y + r * rad(a as f64).sin());
    let slot: Vec<P> = (90..271)
        .step_by(10)
        .map(|a| on(x0, a))
        .chain((-90..91).step_by(10).map(|a| on(x1, a)))
        .collect();
    let (a, b) = (p(x0 + 0.4, y + 4.0, 0.0), p(x1 + 1.8, y + 4.0, 0.0));
    els![
        floor(o, 0.0, 13.6, 0.0, 9.0),
        poly(
            &slot.iter().map(|&(u, v)| p(u, v, 0.0)).collect::<Vec<_>>(),
            SHADE,
            r#"stroke-width="1""#
        ),
        cylinder(o, x0, y, 0.0, 5.5, 1.35),
        arrow(a.0, a.1, b.0, b.1, 3.0, 2.8, ACCENT, ""),
    ]
}

fn mate_ball() -> Vec<String> {
    let (cx, cy, r) = (12.0, 12.0, 5.0);
    els![
        sphere(cx, cy, r, TOP, MID),
        curved_arrow(
            &arc_points(cx, cy, 8.8, 3.1, 25.0, 160.0),
            2.8,
            2.6,
            ACCENT,
            ""
        ),
        curved_arrow(
            &arc_points(cx, cy, 3.1, 8.8, -65.0, 65.0),
            2.8,
            2.6,
            ACCENT,
            ""
        ),
    ]
}

fn mate_parallel() -> Vec<String> {
    let o = (12.0, 12.4);
    let p = projector(o);
    let (s, t, gap) = (8.6, 1.0, 7.0);
    let w = acc(r#"stroke-width="1.4""#);

    let mark = |x: f64, y: f64| {
        let ((ax, ay), (bx, by)) = (p(x, y, t + 0.9), p(x, y, t + gap - 0.9));
        els![
            line(ax, ay, bx, by, &w),
            line(ax - 1.1, ay, ax + 1.1, ay, &w),
            line(bx - 1.1, by, bx + 1.1, by, &w)
        ]
    };

    els![
        ground_box(o, 0.0, s, 0.0, s, 0.0, t),
        mark(0.0, s + 1.2),
        mark(s + 1.2, 0.0),
        iso_box(o, 0.0, s, 0.0, s, t + gap, 2.0 * t + gap),
    ]
}

fn mate_tangent() -> Vec<String> {
    let o = (10.6, 9.0);
    let p = projector(o);
    let (r, y, x0, x1) = (3.0, 4.0, 2.2, 8.2);
    let end = p(x1, y, r);
    let roll: Vec<P> = (200..311)
        .step_by(5)
        .map(|a| {
            (
                end.0 + (r + 1.5) * 0.87 * rad(a as f64).cos(),
                end.1 + (r + 1.5) * rad(a as f64).sin(),
            )
        })
        .collect();
    els![
        floor(o, 0.0, 11.0, 0.0, 8.0),
        seg(
            p(0.3, y, 0.0),
            p(10.7, y, 0.0),
            &acc(r#"stroke-width="1.6""#)
        ),
        xcyl(o, x0, x1, y, r, r),
        curved_arrow(&roll, 2.6, 2.4, ACCENT, ""),
    ]
}

fn mate_width() -> Vec<String> {
    // Context: two thin parallel walls and the part between them. The accent arrows push in from
    // both walls toward the centre; they are drawn last so the near wall never hides them.
    let o = (8.4, 8.6);
    let p = projector(o);
    let (h, d, z, l) = (6.0, 5.6, 2.3, 15.0);
    let wall = |x: f64| {
        poly(
            &[p(x, 0.0, h), p(x, d, h), p(x, d, 0.0), p(x, 0.0, 0.0)],
            SOFT,
            r#"stroke-width="0.9""#,
        )
    };
    let to = |a: P, b: P| arrow(a.0, a.1, b.0, b.1, 2.8, 2.7, ACCENT, "");
    els![
        wall(0.0),
        iso_box(o, 5.2, 9.8, 0.6, d - 0.6, 0.0, 4.6),
        wall(l),
        to(p(0.6, d / 2.0, z), p(4.7, d / 2.0, z)),
        to(p(l - 0.6, d / 2.0, z), p(10.3, d / 2.0, z)),
    ]
}

// ---------------------------------------------------------------------------------------------
// Relations

fn relations() -> Vec<String> {
    let o = (8.3, 11.1);
    let p = projector(o);
    let (a, b) = ((0.0, 0.0, 3.9), (10.0, 0.0, 2.9));
    let (z0, z1) = (0.0, 2.6);
    let belt = hull(&cat(&[
        &ring(o, a.0, a.1, 1.3, a.2 + 0.1, 0.0, 360.0, 5.0),
        &ring(o, b.0, b.1, 1.3, b.2 + 0.1, 0.0, 360.0, 5.0),
    ]));
    let (ta, tb) = (p(a.0, a.1, z1), p(b.0, b.1, z1));
    els![
        cylinder(o, a.0, a.1, z0, z1, a.2),
        cylinder(o, b.0, b.1, z0, z1, b.2),
        poly(&belt, "", &acc(r#"stroke-width="1.5""#)),
        ellipse(ta.0, ta.1, a.2 * r15(), a.2 * r05(), TOP, ""),
        ellipse(tb.0, tb.1, b.2 * r15(), b.2 * r05(), TOP, ""),
    ]
}

fn gear_relation() -> Vec<String> {
    let o = (9.4, 11.4);
    let (ra, rb) = (5.6, 4.1);
    els![
        flat_gear(o, 0.0, 0.0, 0.0, 1.5, ra, ra - 1.7, 7, 0.0),
        flat_gear(o, ra + rb - 1.4, 0.0, 0.0, 1.5, rb, rb - 1.7, 5, 0.5),
        spin(o, 0.0, 0.0, 1.5, 2.6, 200.0, 20.0, 2.4, 2.4),
    ]
}

fn rack_pinion() -> Vec<String> {
    let o = (12.2, 6.4);
    let p = projector(o);
    let (r, tooth) = (4.3, 1.5);
    let (rack_y, h) = (7.6, 1.6);
    let mut edge = vec![(0.0, rack_y)];
    for x in [1.2, 4.6, 8.0, 11.4] {
        edge.extend([
            (x, rack_y),
            (x + 0.5, rack_y - tooth),
            (x + 1.6, rack_y - tooth),
            (x + 2.1, rack_y),
        ]);
    }
    edge.push((14.0, rack_y));
    let mut top: Vec<P> = edge.iter().map(|&(u, v)| p(u, v, h)).collect();
    top.extend([p(14.0, rack_y + 3.0, h), p(0.0, rack_y + 3.0, h)]);
    let (a, b) = (p(9.4, rack_y + 1.5, h), p(14.6, rack_y + 1.5, h));
    els![
        ground_box(o, 0.0, 14.0, rack_y, rack_y + 3.0, 0.0, h),
        poly(&top, SOFT, ""),
        flat_gear(o, 5.8, rack_y - r - 0.1, 0.0, h, r, r - tooth - 0.2, 6, 0.1),
        arrow(a.0, a.1, b.0, b.1, 2.6, 2.4, ACCENT, ""),
    ]
}

/// Splits `pts2` into runs of equal `flags`; each run but the last also takes the next point.
fn runs(flags: &[bool], pts2: &[P]) -> Vec<(bool, Vec<P>)> {
    let (mut runs, mut cur, mut cur_flag) = (Vec::new(), Vec::new(), None);
    for (&flag, &p) in flags.iter().zip(pts2) {
        if Some(flag) != cur_flag && !cur.is_empty() {
            let mut run = std::mem::take(&mut cur);
            run.push(p);
            runs.push((cur_flag.unwrap(), run));
        }
        cur_flag = Some(flag);
        cur.push(p);
    }
    if !cur.is_empty() {
        runs.push((cur_flag.unwrap(), cur));
    }
    runs
}

fn screw_relation() -> Vec<String> {
    let o = (11.0, 18.8);
    let p = projector(o);
    let (r_rod, r_h) = (1.3, 3.2);
    let (turns, z0, z1) = (1.6, 1.5, 13.0);
    let n = 90;
    let hel: Vec<(f64, f64, f64)> = (0..=n)
        .map(|i| {
            let a = 2.0 * PI * turns * i as f64 / n as f64 + 2.4;
            (
                r_h * a.cos(),
                r_h * a.sin(),
                z0 + (z1 - z0) * i as f64 / n as f64,
            )
        })
        .collect();
    let flags: Vec<bool> = hel.iter().map(|q| q.0 + q.1 > 0.0).collect();
    let screen: Vec<P> = hel.iter().map(|&(x, y, z)| p(x, y, z)).collect();
    let runs = runs(&flags, &screen);
    let mut out: Vec<String> = runs
        .iter()
        .filter(|r| !r.0)
        .map(|r| polyline(&r.1, &acc("")))
        .collect();
    out.push(cylinder(o, 0.0, 0.0, 0.0, 14.5, r_rod));
    let fronts: Vec<&Vec<P>> = runs.iter().filter(|r| r.0).map(|r| &r.1).collect();
    out.extend(
        fronts[..fronts.len() - 1]
            .iter()
            .map(|run| polyline(run, &acc(""))),
    );
    out.push(curved_arrow(fronts[fronts.len() - 1], 2.6, 2.4, ACCENT, ""));
    out
}

fn linear_relation() -> Vec<String> {
    let o = (10.8, 8.5);
    let p = projector(o);
    let mut out = Vec::new();
    for y0 in [0.0, 6.0] {
        let (a, b) = (p(5.3, y0 + 1.5, 1.5), p(11.7, y0 + 1.5, 1.5));
        out.extend([
            ground_box(o, 0.0, 12.0, y0, y0 + 3.0, 0.0, 1.5),
            iso_box(o, 1.5, 4.5, y0, y0 + 3.0, 1.5, 4.5),
            arrow(a.0, a.1, b.0, b.1, 2.6, 2.4, ACCENT, ""),
        ]);
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Assembly tools

fn group() -> Vec<String> {
    let o = (12.0, 10.2);
    let p = projector(o);
    let parts = [
        (0.0, 4.0, 0.0, 4.0, 0.0, 4.0),
        (5.0, 9.0, 0.0, 4.0, 0.0, 2.5),
        (0.0, 4.0, 5.0, 9.0, 0.0, 2.5),
    ];
    let mut corners = Vec::new();
    for &(x0, x1, y0, y1, z0, z1) in &parts {
        for x in [x0 - 1.0, x1 + 1.0] {
            for y in [y0 - 1.0, y1 + 1.0] {
                for z in [z0 - 0.8, z1 + 0.8] {
                    corners.push(p(x, y, z));
                }
            }
        }
    }
    let b = |(x0, x1, y0, y1, z0, z1): (f64, f64, f64, f64, f64, f64)| {
        iso_box(o, x0, x1, y0, y1, z0, z1)
    };
    els![
        poly(&hull(&corners), "", &acc(&dash(1.1))),
        b(parts[0]),
        b(parts[1]),
        b(parts[2])
    ]
}

fn replicate() -> Vec<String> {
    let o = (8.2, 9.6);
    let xs = [2.5, 7.0, 11.5];
    let y = 2.6;
    let copy = acc(r#"fill-opacity="0.3""#);
    let mut out = vec![ground_box(o, 0.0, 14.0, 0.0, 5.2, 0.0, 2.0)];
    for x in xs {
        out.push(poly(
            &ring(o, x, y, 2.0, 1.3, 0.0, 360.0, 10.0),
            SHADE,
            THIN,
        ));
    }
    out.push(cylinder(o, xs[0], y, 2.0, 6.5, 1.3));
    for x in &xs[1..] {
        out.push(cylinder_with(
            o, *x, y, 2.0, 6.5, 1.3, ACCENT, ACCENT, &copy,
        ));
    }
    out
}

fn explode() -> Vec<String> {
    let o = (12.0, 13.3);
    let p = projector(o);
    let guide = acc(&dash(0.9));
    let (a, b) = (p(7.0, 7.0, 3.6), p(7.0, 7.0, 7.2));
    els![
        iso_box(o, 0.0, 7.0, 0.0, 7.0, 0.0, 3.0),
        iso_box(o, 0.0, 7.0, 0.0, 7.0, 7.5, 10.0),
        seg(p(0.0, 7.0, 3.9), p(0.0, 7.0, 6.6), &guide),
        seg(p(7.0, 0.0, 3.9), p(7.0, 0.0, 6.6), &guide),
        arrow(a.0, a.1, b.0, b.1, 2.4, 2.3, ACCENT, ""),
    ]
}

fn mini_assembly(o: P) -> Vec<String> {
    els![
        ground_box(o, 0.0, 9.0, 0.0, 9.0, 0.0, 2.0),
        iso_box(o, 1.8, 6.2, 1.8, 6.2, 2.0, 6.8)
    ]
}

const BADGE_O: P = (10.4, 11.4);

fn named_positions() -> Vec<String> {
    els![
        mini_assembly(BADGE_O),
        path(
            "M16.9 2.5 H21.3 V9.8 L19.1 8.1 L16.9 9.8 Z",
            ACCENT,
            &acc("")
        )
    ]
}

fn display_states() -> Vec<String> {
    let eye = "M15.1 5.8 Q18.3 2 21.5 5.8 Q18.3 9.6 15.1 5.8 Z";
    els![
        mini_assembly(BADGE_O),
        path(eye, TOP, &acc("")),
        circle(18.3, 5.8, 1.2, ACCENT, &acc(""))
    ]
}

fn assembly_properties() -> Vec<String> {
    els![
        mini_assembly(BADGE_O),
        circle(18.6, 5.4, 3.1, ACCENT, &acc("")),
        line(
            18.6,
            5.2,
            18.6,
            7.0,
            &format!(r#"stroke="{TOP}" stroke-width="1.3""#)
        ),
        circle(18.6, 3.6, 0.75, TOP, r#"stroke="none""#),
    ]
}

fn bill_of_materials() -> Vec<String> {
    let mut out = els![
        rect(4.0, 2.5, 16.0, 19.0, 2.0, TOP, ""),
        path(
            "M6 2.5 H18 A2 2 0 0 1 20 4.5 V7.5 H4 V4.5 A2 2 0 0 1 6 2.5 Z",
            SOFT,
            THIN
        ),
    ];
    for (i, yy) in [11.0, 14.5, 18.0].into_iter().enumerate() {
        out.push(rect(6.5, yy - 1.1, 2.2, 2.2, 0.4, MID, THIN));
        out.push(line(
            10.5,
            yy,
            17.5,
            yy,
            &if i == 0 {
                acc(r#"stroke-width="1.4""#)
            } else {
                String::new()
            },
        ));
    }
    out
}

fn standard_content() -> Vec<String> {
    // A hex-head bolt, from the side: the head a hexagonal prism, the shank a cylinder with a few
    // thread lines across its front. Neutral, like part: it is a stock part, not an operation.
    let o = (12.0, 17.5);
    let p = projector(o);
    let (big_r, h0, h1) = (4.6, 11.0, 14.6);
    let r = 2.1;
    let hexv: Vec<P> = (0..6)
        .map(|i| {
            (
                big_r * rad((15 + 60 * i) as f64).cos(),
                big_r * rad((15 + 60 * i) as f64).sin(),
            )
        })
        .collect();
    let mut out = vec![cylinder_with(o, 0.0, 0.0, 0.0, h0, r, TOP, MID, "")];
    for i in 0..4 {
        // thread crests: the front half of a slightly tilted ring
        let z = 1.6 + 2.2 * i as f64;
        let front: Vec<P> = (-45..136)
            .step_by(10)
            .map(|a| {
                let a = a as f64;
                p(
                    r * rad(a).cos(),
                    r * rad(a).sin(),
                    z + 0.5 * rad(a - 45.0).sin(),
                )
            })
            .collect();
        out.push(polyline(&front, THIN));
    }
    for i in 0..6 {
        let ((x0, y0), (x1, y1)) = (hexv[i], hexv[(i + 1) % 6]);
        let (nx, ny) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        if nx + ny <= 0.01 {
            continue; // faces the back
        }
        let fill = if nx > ny + 0.01 {
            MID
        } else if ny > nx + 0.01 {
            SHADE
        } else {
            SOFT
        };
        out.push(poly(
            &[p(x0, y0, h1), p(x1, y1, h1), p(x1, y1, h0), p(x0, y0, h0)],
            fill,
            "",
        ));
    }
    out.push(poly(
        &hexv.iter().map(|&(x, y)| p(x, y, h1)).collect::<Vec<_>>(),
        TOP,
        "",
    ));
    out
}

// ---------------------------------------------------------------------------------------------
// Document types: an emblem on a square platform.

const PZ: f64 = 1.8;
const DOC_O: P = (12.0, 10.2);

fn platform(o: P) -> String {
    ground_box(o, 0.0, 10.0, 0.0, 10.0, 0.0, PZ)
}

fn assembly() -> Vec<String> {
    let o = DOC_O;
    els![
        platform(o),
        tint_box(o, 1.0, 5.6, 4.4, 9.0, PZ, PZ + 4.6),
        tint_cyl(o, 6.6, 3.4, PZ, PZ + 7.0, 2.3)
    ]
}

fn assembly_rigid() -> Vec<String> {
    // The assembly emblem, smaller, between square brackets: a subassembly that moves as one body.
    let k = 0.78;
    let o = (12.0, 10.2);
    let z = PZ * k;
    let brk = r#"stroke-width="1.4""#;
    els![
        ground_box(o, 0.0, 10.0 * k, 0.0, 10.0 * k, 0.0, z),
        tint_box(o, 1.0 * k, 5.6 * k, 4.4 * k, 9.0 * k, z, z + 4.6 * k),
        tint_cyl(o, 6.6 * k, 3.4 * k, z, z + 7.0 * k, 2.3 * k),
        polyline(&[(5.0, 3.5), (2.6, 3.5), (2.6, 20.5), (5.0, 20.5)], brk),
        polyline(&[(19.0, 3.5), (21.4, 3.5), (21.4, 20.5), (19.0, 20.5)], brk),
    ]
}

fn part_studio() -> Vec<String> {
    let o = DOC_O;
    els![platform(o), tint_box(o, 1.5, 8.5, 1.5, 8.5, PZ, PZ + 7.0)]
}

fn material_library() -> Vec<String> {
    let o = DOC_O;
    let (cx, cy) = projector(o)(5.0, 5.0, PZ + 4.0);
    els![
        platform(o),
        sphere(cx, cy, 4.3, ACCENT, ACCENT),
        ellipse_rot(
            cx + 1.5,
            cy - 1.6,
            1.3,
            0.8,
            TOP,
            r#"stroke="none" fill-opacity="0.8""#,
            -35.0
        ),
    ]
}

fn feature_studio() -> Vec<String> {
    let o = DOC_O;
    let code = acc(r#"stroke-width="1.8""#);
    els![
        platform(o),
        polyline(&[(8.8, 5.0), (5.4, 8.6), (8.8, 12.2)], &code),
        polyline(&[(15.2, 5.0), (18.6, 8.6), (15.2, 12.2)], &code),
        line(13.0, 4.2, 11.0, 13.0, &code),
    ]
}

fn variable_studio() -> Vec<String> {
    // The variables glyph (an italic f and an x) over the studio's platform.
    let o = DOC_O;
    let x = acc(r#"stroke-width="1.8""#);
    els![
        platform(o),
        path(
            "M13.4 3.6 C12.3 3 11 3.4 10.7 4.8 L9.2 12.6 C8.9 14 7.7 14.4 6.6 13.8",
            "",
            r#"stroke-width="1.8""#
        ),
        path("M8 7.6 H12.6", "", r#"stroke-width="1.8""#),
        path("M13.6 8 L17.6 12.6 M17.6 8 L13.6 12.6", "", &x),
    ]
}

fn cam_studio() -> Vec<String> {
    let o = DOC_O;
    let p = projector(o);
    let z = PZ + 2.2;
    let (a, b) = (p(6.4, 4.6, z), p(6.4, 9.4, z));
    els![
        platform(o),
        iso_box(o, 1.0, 9.0, 1.0, 9.0, PZ, z),
        polyline(
            &[p(6.4, 2.8, z), p(3.0, 2.8, z)],
            &acc(r#"stroke-width="1.4""#)
        ),
        arrow(a.0, a.1, b.0, b.1, 2.6, 2.4, ACCENT, ""),
        cylinder(o, 6.4, 2.8, z, z + 7.0, 1.7),
    ]
}

fn pcb_studio() -> Vec<String> {
    let o = DOC_O;
    let p = projector(o);
    let z = PZ;
    let trace = acc(r#"stroke-width="1.4""#);
    let mut out = vec![platform(o)];
    for y in [2.4, 5.2] {
        let c = p(8.6, y, z);
        out.extend([
            seg(p(4.6, y, z), c, &trace),
            circle(c.0, c.1, 0.95, ACCENT, r#"stroke="none""#),
        ]);
    }
    out.push(iso_box(o, 1.0, 4.6, 1.0, 6.6, z, z + 1.6));
    out
}

fn render_studio() -> Vec<String> {
    let o = DOC_O;
    let p = projector(o);
    let (cx, cy) = p(5.0, 5.0, PZ + 4.0);
    let (sx, sy) = (18.7, 5.4);
    let rays: Vec<String> = (0..360)
        .step_by(45)
        .map(|a| {
            let a = rad(a as f64);
            line(
                sx + 2.2 * a.cos(),
                sy + 2.2 * a.sin(),
                sx + 2.9 * a.cos(),
                sy + 2.9 * a.sin(),
                &acc(r#"stroke-width="1.3""#),
            )
        })
        .collect();
    els![
        platform(o),
        sphere(cx, cy, 4.3, TOP, MID),
        circle(sx, sy, 1.3, ACCENT, &acc("")),
        rays
    ]
}
