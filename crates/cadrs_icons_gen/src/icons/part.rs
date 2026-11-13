//! Part Studio features, analysis tools and part/document objects (kind Solid).
//!
//! House style for this module: one iso block of side 8 centred at (12, 12) is the reference size.
//! Grey faces are context; ACCENT marks the input (tint or stroke) and the output (dashed, faint).

use crate::common::*;
use Kind::Solid;
use std::f64::consts::PI;

pub fn icons() -> Vec<Def> {
    vec![
        def("extrude", Solid, "Extrude", extrude),
        def("revolve", Solid, "Revolve", revolve),
        def("sweep", Solid, "Sweep", sweep),
        def("loft", Solid, "Loft", loft),
        def("thicken", Solid, "Thicken", thicken),
        def("enclose", Solid, "Enclose", enclose),
        def("fillet", Solid, "Fillet", fillet),
        def("chamfer", Solid, "Chamfer", chamfer),
        def("draft", Solid, "Draft", draft),
        def("rib", Solid, "Rib", rib),
        def("shell", Solid, "Shell", shell),
        def("hole", Solid, "Hole", hole),
        def("thread", Solid, "Thread", thread),
        def("gear", Solid, "Gear", gear),
        def("linear-pattern", Solid, "Linear pattern", linear_pattern),
        def(
            "circular-pattern",
            Solid,
            "Circular pattern",
            circular_pattern,
        ),
        def("boolean", Solid, "Boolean", boolean),
        def("split", Solid, "Split", split),
        def("transform", Solid, "Transform", transform),
        def("composite-part", Solid, "Composite part", composite_part),
        def("plane", Solid, "Plane", plane),
        def("mate-connector", Solid, "Mate connector", mate_connector),
        def(
            "custom-feature",
            Solid,
            "Add custom feature",
            custom_feature,
        ),
        def("measure", Solid, "Measure", measure),
        def("section-view", Solid, "Section view", section_view),
        def("mass-properties", Solid, "Mass properties", mass_properties),
        def("part", Solid, "Part", part),
        def("sketch", Solid, "Sketch", sketch),
        def("surface", Solid, "Surface", surface),
    ]
}

const THIN: &str = r#"stroke-width="0.8""#;
const HAIR: &str = r#"stroke-width="0.7""#;

/// Stroke attributes for an ACCENT line.
fn acc(w: f64) -> String {
    format!(r#"stroke="{ACCENT}" stroke-width="{}""#, f(w))
}

// The three roles. CONTEXT is neutral faces with INK outlines. INPUT (what the user selects) is
// an ACCENT tint over a neutral face, or an ACCENT stroke for an edge or path. OUTPUT (what the
// tool creates) is a dashed ACCENT outline with a faint ACCENT fill, or a solid ACCENT face when
// the new face is the point.
const SEL: &str = r#"stroke="none" fill-opacity="0.45""#;

fn out(w: f64) -> String {
    format!(r#"stroke="{ACCENT}" fill-opacity="0.15" {}"#, dash(w))
}

fn sel_edge(w: f64) -> String {
    format!(r#"stroke="{ACCENT}" stroke-width="{}""#, f(w))
}

/// Faces of a cube whose top-right edge (x=s, z=s) is replaced by `cut`, a list of (x, z)
/// profile points from the top face to the right face; also the cut's two end profiles.
fn block_edge(o: P, s: f64, cut: &[P]) -> (Vec<P>, Vec<P>, Vec<String>) {
    let p = projector(o);
    let a: Vec<P> = cut.iter().map(|&(x, z)| p(x, 0.0, z)).collect();
    let b: Vec<P> = cut.iter().map(|&(x, z)| p(x, s, z)).collect();
    let faces = els![
        poly(&[p(0.0, 0.0, s), a[0], b[0], p(0.0, s, s)], TOP, ""),
        poly(
            &[a[a.len() - 1], p(s, 0.0, 0.0), p(s, s, 0.0), b[b.len() - 1]],
            MID,
            ""
        ),
        poly(
            &cat(&[&[p(0.0, s, s)], &b, &[p(s, s, 0.0), p(0.0, s, 0.0)]]),
            SHADE,
            ""
        ),
    ];
    (a, b, faces)
}

/// An OUTPUT box: faint accent silhouette, dashed accent outline and the three edges that meet
/// at the near top corner (no per-face outlines, which clutter at small sizes).
#[allow(clippy::too_many_arguments)]
fn ghost_box(o: P, x0: f64, x1: f64, y0: f64, y1: f64, z0: f64, z1: f64, inner: bool) -> String {
    let p = projector(o);
    let sil = [
        p(x0, y0, z1),
        p(x1, y0, z1),
        p(x1, y0, z0),
        p(x1, y1, z0),
        p(x0, y1, z0),
        p(x0, y1, z1),
    ];
    let c = p(x1, y1, z1);
    let d = format!(r#"stroke="{ACCENT}" {}"#, dash(1.1));
    let mut out = els![
        poly(&sil, ACCENT, r#"stroke="none" fill-opacity="0.15""#),
        poly(&sil, "", &d)
    ];
    if inner {
        out.push(polyline(&[p(x0, y1, z1), c, p(x1, y0, z1)], &d));
        out.push(seg(c, p(x1, y1, z0), &d));
    } else {
        // small boxes: just a faint top face so they still read as boxes
        out.push(poly(
            &[p(x0, y0, z1), p(x1, y0, z1), c, p(x0, y1, z1)],
            ACCENT,
            r#"stroke="none" fill-opacity="0.25""#,
        ));
    }
    join(out)
}

fn cube(o: P, s: f64) -> String {
    iso_box(o, 0.0, s, 0.0, s, 0.0, s)
}

// ---------------------------------------------------------------------------------------------
// Features

fn extrude() -> Vec<String> {
    // Context: the slab. Input: its top face (the profile). Output: the dashed volume the extrude
    // adds, with an INK arrow rising through it.
    let o = (12.0, 13.9);
    let p = projector(o);
    let (s, z0, z1) = (8.0, 4.0, 10.0);
    let edge = format!(r#"stroke="{ACCENT}" {}"#, dash(1.1));
    let faint = r#"stroke="none" fill-opacity="0.15""#;
    let edges: Vec<String> = [(s, 0.0), (s, s), (0.0, s)]
        .iter()
        .map(|&(x, y)| seg(p(x, y, z0), p(x, y, z1), &edge))
        .collect();
    let (a, b) = (p(s / 2.0, s / 2.0, z0), p(s / 2.0, s / 2.0, z1 + 2.2));
    els![
        iso_box(o, 0.0, s, 0.0, s, 0.0, z0),
        poly(
            &[p(0.0, 0.0, z0), p(s, 0.0, z0), p(s, s, z0), p(0.0, s, z0)],
            ACCENT,
            SEL
        ),
        poly(
            &[p(s, 0.0, z1), p(s, s, z1), p(s, s, z0), p(s, 0.0, z0)],
            ACCENT,
            faint
        ),
        poly(
            &[p(0.0, s, z1), p(s, s, z1), p(s, s, z0), p(0.0, s, z0)],
            ACCENT,
            faint
        ),
        poly(
            &[p(0.0, 0.0, z1), p(s, 0.0, z1), p(s, s, z1), p(0.0, s, z1)],
            "",
            &edge
        ),
        edges,
        arrow(a.0, a.1, b.0, b.1, 3.2, 3.0, INK, ""),
    ]
}

fn revolve() -> Vec<String> {
    // Input: the flat profile (tinted). Context: the dash-dot axis. The INK arrow wraps the axis,
    // behind the profile at the back and in front of it at the front.
    let (cx, cy, rx, ry) = (9.0, 12.0, 6.5, 2.4);
    let arc = r#"stroke-width="1.4""#;
    let prof = "M11.5 5.5 h7 v10 l-3 3 h-4 z";
    els![
        polyline(&arc_points(cx, cy, rx, ry, 200.0, 340.0), arc),
        path(prof, TOP, ""),
        path(prof, ACCENT, SEL),
        line(9.0, 2.0, 9.0, 22.0, &dash_dot(0.9)),
        curved_arrow(
            &arc_points(cx, cy, rx, ry, 340.0, 520.0),
            3.2,
            2.8,
            INK,
            arc
        ),
    ]
}

fn sweep() -> Vec<String> {
    // The swept tube is the whole point, so it is a solid body: neutral shading under a light
    // accent wash (output), with the round profile it was swept from as the accent input cap.
    let tube = sample_cubic((5.4, 18.1), (14.6, 18.9), (9.4, 5.1), (18.6, 5.9), 48);
    let r = 2.7;
    let (left, right) = (offset_polyline(&tube, r), offset_polyline(&tube, -r));

    let cap = |i: usize, fill: &str, extra: &str| {
        let (x, y) = tube[i];
        let (a, b) = (tube[i.saturating_sub(1)], tube[(i + 1).min(tube.len() - 1)]);
        let ang = deg((b.1 - a.1).atan2(b.0 - a.0)) + 90.0;
        ellipse_rot(x, y, r, 1.3, fill, extra, ang)
    };

    let body = cat(&[&left, &rev(&right)]);
    els![
        poly(&body, MID, r#"stroke="none""#),
        polyline(
            &offset_polyline(&tube, -r * 0.45),
            &format!(r#"stroke="{SHADE}" stroke-width="1.4" stroke-opacity="0.45""#)
        ),
        polyline(
            &offset_polyline(&tube, r * 0.42),
            &format!(r#"stroke="{TOP}" stroke-width="1.2" stroke-opacity="0.75""#)
        ),
        poly(&body, ACCENT, r#"stroke="none" fill-opacity="0.2""#),
        polyline(&left, ""),
        polyline(&right, ""),
        cap(tube.len() - 1, MID, ""),
        cap(0, TOP, ""),
        cap(0, ACCENT, r#"stroke="none" fill-opacity="0.45""#),
        cap(0, "", &format!(r#"stroke="{ACCENT}" stroke-width="1.4""#)),
    ]
}

fn loft() -> Vec<String> {
    // X-ray view: the body blends a square base into a round top, and dashed hidden edges show
    // the whole base profile through it. Both profiles are accent: they are what the loft joins.
    let base_front = [(5.0, 18.0), (12.0, 21.5), (19.0, 18.0)];
    let base_back = [(5.0, 18.0), (12.0, 14.5), (19.0, 18.0)];
    els![
        path(
            "M5 18 L7.5 6.5 A4.5 1.8 0 0 0 16.5 6.5 L19 18 L12 21.5 Z",
            MID,
            ""
        ),
        path(
            "M12 21.5 C12 15, 12 11, 12 8.3",
            "",
            r#"stroke-width="0.8""#
        ),
        polyline(&base_back, &format!(r#"stroke="{ACCENT}" {}"#, dash(0.9))),
        polyline(
            &base_front,
            &format!(r#"stroke="{ACCENT}" stroke-width="1.4""#)
        ),
        ellipse(
            12.0,
            6.5,
            4.5,
            1.8,
            TOP,
            &format!(r#"stroke="{ACCENT}" stroke-width="1.4""#)
        ),
    ]
}

fn thicken() -> Vec<String> {
    // Input: the wavy surface (tinted). Output: the thickness below it (dashed, faint).
    let o = (9.4, 7.9);
    let p = projector(o);
    let (l, w, t) = (13.0, 7.0, 2.6);
    let z = |x: f64| 1.8 * (x / l * 2.0 * PI).sin() + t;
    let xs: Vec<f64> = (0..31).map(|i| (13 * i) as f64 / 30.0).collect();
    let front_top: Vec<P> = xs.iter().map(|&x| p(x, w, z(x))).collect();
    let front_bot: Vec<P> = xs.iter().map(|&x| p(x, w, z(x) - t)).collect();
    let back_top: Vec<P> = xs.iter().map(|&x| p(x, 0.0, z(x))).collect();
    let end = [
        p(l, 0.0, z(l)),
        p(l, w, z(l)),
        p(l, w, z(l) - t),
        p(l, 0.0, z(l) - t),
    ];
    let sheet = cat(&[&back_top, &rev(&front_top)]);
    els![
        poly(&cat(&[&front_top, &rev(&front_bot)]), ACCENT, &out(1.1)),
        poly(&end, ACCENT, &out(1.1)),
        poly(&sheet, TOP, ""),
        poly(&sheet, ACCENT, SEL),
    ]
}

fn enclose() -> Vec<String> {
    // Input: an open tube of surface (tinted). Output: the cap that closes it into a solid.
    let o = (12.0, 16.5);
    let p = projector(o);
    let (r, h) = (5.0, 9.0);
    let (rx, ry) = (r * 1.5f64.sqrt(), r * 0.5f64.sqrt());
    let (tx, ty) = p(0.0, 0.0, h);
    let d = format!(
        "M{} {} L{} {} A{} {} 0 0 0 {} {} L{} {} A{} {} 0 0 1 {} {} Z",
        f(tx - rx),
        f(ty),
        f(tx - rx),
        f(ty + h),
        f(rx),
        f(ry),
        f(tx + rx),
        f(ty + h),
        f(tx + rx),
        f(ty),
        f(rx),
        f(ry),
        f(tx - rx),
        f(ty)
    );
    els![
        cylinder_with(o, 0.0, 0.0, 0.0, h, r, SHADE, MID, ""),
        path(&d, ACCENT, SEL),
        ellipse(tx, ty, rx, ry, ACCENT, &out(1.2))
    ]
}

fn fillet() -> Vec<String> {
    let (o, s, r) = ((12.0, 12.0), 8.0, 4.2);
    let cut: Vec<P> = (0..=90)
        .step_by(10)
        .map(|t| {
            (
                s - r + r * rad(t as f64).sin(),
                s - r + r * rad(t as f64).cos(),
            )
        })
        .collect();
    let (a, b, faces) = block_edge(o, s, &cut);
    els![faces, poly(&cat(&[&a, &rev(&b)]), ACCENT, "")]
}

fn chamfer() -> Vec<String> {
    let (o, s, c) = ((12.0, 12.0), 8.0, 3.8);
    let (a, b, faces) = block_edge(o, s, &[(s - c, s), (s, s - c)]);
    els![faces, poly(&cat(&[&a, &rev(&b)]), ACCENT, "")]
}

fn draft() -> Vec<String> {
    // Context: the block. Output: the tapered face (solid accent). The dashed INK line is where
    // the face stood before, parallel to the pull direction.
    let (o, s, d) = ((12.0, 12.0), 8.0, 2.6);
    let p = projector(o);
    els![
        poly(
            &[
                p(0.0, 0.0, s),
                p(s - d, 0.0, s),
                p(s - d, s, s),
                p(0.0, s, s)
            ],
            TOP,
            ""
        ),
        poly(
            &[p(0.0, s, s), p(s - d, s, s), p(s, s, 0.0), p(0.0, s, 0.0)],
            SHADE,
            ""
        ),
        poly(
            &[
                p(s - d, 0.0, s),
                p(s - d, s, s),
                p(s, s, 0.0),
                p(s, 0.0, 0.0)
            ],
            ACCENT,
            ""
        ),
        seg(p(s, s, 0.0), p(s, s, s + 1.0), &dash(1.0)),
    ]
}

fn rib() -> Vec<String> {
    // Context: the L-bracket. Output: the thin web between its faces.
    let o = (10.9, 12.2);
    let p = projector(o);
    let (l, w, t, h) = (10.0, 7.0, 2.0, 9.0);
    let (y0, y1) = (w / 2.0 - 0.8, w / 2.0 + 0.8);
    els![
        iso_box(o, 0.0, l, 0.0, w, 0.0, t),
        iso_box(o, 0.0, t, 0.0, w, t, h),
        poly(
            &[
                p(t, y0, h - 0.5),
                p(t, y1, h - 0.5),
                p(l - 0.5, y1, t),
                p(l - 0.5, y0, t)
            ],
            ACCENT,
            r#"fill-opacity="0.7""#
        ),
        poly(
            &[p(t, y1, h - 0.5), p(l - 0.5, y1, t), p(t, y1, t)],
            ACCENT,
            ""
        ),
    ]
}

fn shell() -> Vec<String> {
    // Context: the hollowed box. Input: the removed top face, tinted over the opening.
    let o = (12.0, 12.2);
    let p = projector(o);
    let (s, h, t) = (8.5, 7.0, 1.5);
    let rim = [
        p(t, t, h),
        p(s - t, t, h),
        p(s - t, s - t, h),
        p(t, s - t, h),
    ];
    els![
        iso_box(o, 0.0, s, 0.0, s, 0.0, h),
        poly(
            &clip(
                &[
                    p(t, t, t),
                    p(s - t, t, t),
                    p(s - t, s - t, t),
                    p(t, s - t, t)
                ],
                &rim
            ),
            TOP,
            HAIR
        ),
        poly(
            &clip(
                &[p(t, t, h), p(s - t, t, h), p(s - t, t, t), p(t, t, t)],
                &rim
            ),
            SHADE,
            HAIR
        ),
        poly(
            &clip(
                &[p(t, t, h), p(t, s - t, h), p(t, s - t, t), p(t, t, t)],
                &rim
            ),
            MID,
            HAIR
        ),
        poly(&rim, ACCENT, r#"stroke="none" fill-opacity="0.4""#),
        poly(&rim, "", &sel_edge(1.3)),
    ]
}

fn hole() -> Vec<String> {
    let o = (12.0, 9.0);
    let (s, h) = (10.0, 4.0);
    let (c, r1, r2, d1) = (s / 2.0, 3.4, 1.9, 1.6);
    let e1 = iso_circle(o, c, c, h, r1, 48);
    let floor = iso_circle(o, c, c, h - d1, r1, 48);
    let e2 = iso_circle(o, c, c, h - d1, r2, 48);
    let deep = iso_circle(o, c, c, h - d1 - 4.0, r2, 48);
    els![
        iso_box(o, 0.0, s, 0.0, s, 0.0, h),
        poly(&e1, ACCENT, ""),
        poly(&clip(&floor, &e1), TOP, HAIR),
        poly(&clip(&e2, &e1), ACCENT, HAIR),
        poly(&clip(&clip(&deep, &e2), &e1), SHADE, r#"stroke="none""#),
        poly(&e1, "", ""),
    ]
}

fn thread() -> Vec<String> {
    // Context: the plain rod. Output: the threaded band, a solid accent face with a toothed
    // silhouette.
    let o = (12.0, 17.4);
    let (ox, oy) = o;
    let (r, k, h, zt, pitch) = (4.0, 0.9, 11.0, 7.2, 1.8);
    let (rx, ry) = (r * 1.5f64.sqrt(), r * 0.5f64.sqrt());
    let (mut left, mut right) = (Vec::new(), Vec::new());
    let mut z = 0.5;
    while z + pitch <= zt + 0.01 {
        right.extend([
            (ox + rx + k, oy - z - pitch / 4.0),
            (ox + rx, oy - z - pitch / 2.0),
        ]);
        left.extend([
            (ox - rx - k, oy - z - pitch * 3.0 / 4.0),
            (ox - rx, oy - z - pitch),
        ]);
        z += pitch;
    }
    let band = cat(&[
        &[(ox - rx, oy - zt)],
        &rev(&left),
        &[(ox - rx, oy)],
        &arc_points_step(ox, oy, rx, ry, 180.0, 0.0, 6.0)[1..],
        &[(ox + rx, oy)],
        &right,
        &[(ox + rx, oy - zt)],
        &arc_points_step(ox, oy - zt, rx, ry, 0.0, 180.0, 6.0)[1..],
    ]);
    els![cylinder(o, 0.0, 0.0, 0.0, h, r), poly(&band, ACCENT, "")]
}

fn gear() -> Vec<String> {
    // Context: the blank's hub and bore. Output: the teeth, a solid accent face with a toothed
    // silhouette.
    let o = (12.0, 12.6);
    let p = projector(o);
    let (r_out, r_root, h, teeth) = (7.0, 5.0, 2.6, 8);
    let w = 2.0 * PI / teeth as f64;
    let mut g = Vec::new();
    for k in 0..teeth {
        let a = w * (k as f64 + 0.2);
        for (t, r) in [(0.0, r_root), (0.17, r_out), (0.5, r_out), (0.67, r_root)] {
            g.push((r * (a + t * w).cos(), r * (a + t * w).sin()));
        }
    }
    let at = |z: f64| g.iter().map(|&(u, v)| p(u, v, z)).collect::<Vec<P>>();
    let side: Vec<String> = (1..4)
        .map(|i| poly(&at(h * i as f64 / 4.0), MID, r#"stroke="none""#))
        .collect();
    els![
        poly(&at(0.0), MID, ""),
        side,
        poly(&at(h), ACCENT, ""),
        poly(&iso_circle(o, 0.0, 0.0, h, 0.66 * r_root, 40), TOP, THIN),
        poly(&iso_circle(o, 0.0, 0.0, h, 0.28 * r_root, 24), SHADE, THIN),
    ]
}

fn linear_pattern() -> Vec<String> {
    // Context: the seed cube. Output: two dashed copies along x.
    let o = (6.6, 8.9);
    let (c, p) = (4.6, 6.2);
    let ghosts: Vec<String> = [2.0, 1.0]
        .iter()
        .map(|&i| ghost_box(o, i * p, i * p + c, 0.0, c, 0.0, c, false))
        .collect();
    els![ghosts, iso_box(o, 0.0, c, 0.0, c, 0.0, c)]
}

fn circular_pattern() -> Vec<String> {
    // Context: the seed cube (front) and the axis. Output: three dashed copies at 90 degrees.
    let o = (12.0, 13.4);
    let p = projector(o);
    let (r, c) = (5.4, 3.4);
    let mut items: Vec<(f64, usize, f64, f64)> = (0..4)
        .map(|i| {
            let t = rad((45 + 90 * i) as f64);
            (t.cos() + t.sin(), i, r * t.cos(), r * t.sin())
        })
        .collect();
    items.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut out = vec![seg(p(0.0, 0.0, -2.0), p(0.0, 0.0, 8.0), &dash_dot(0.9))];
    for (_, i, cx, cy) in items {
        let (x0, x1, y0, y1) = (cx - c / 2.0, cx + c / 2.0, cy - c / 2.0, cy + c / 2.0);
        out.push(if i == 0 {
            iso_box(o, x0, x1, y0, y1, 0.0, c)
        } else {
            ghost_box(o, x0, x1, y0, y1, 0.0, c, false)
        });
    }
    out
}

fn boolean() -> Vec<String> {
    let (r, d, cy) = (6.2, 7.4, 12.0);
    let (c1, c2) = (12.0 - d / 2.0, 12.0 + d / 2.0);
    let h = (r * r - (d / 2.0f64).powf(2.0)).sqrt(); // half height of the lens

    let crescent = |cx: f64| {
        // Inside the sphere, outside the same circle nudged up-right: the shadow side.
        let (ox, oy) = (cx + 1.7, cy - 1.7);
        let inside = |(x, y): P, a: f64, b: f64| hypot(x - a, y - b) < r;
        let on =
            |x: f64, y: f64, t: i32| (x + r * rad(t as f64).cos(), y + r * rad(t as f64).sin());
        let rim = (-45..316).step_by(3).map(|t| on(cx, cy, t));
        let inner = (-45..=315).rev().step_by(3).map(|t| on(ox, oy, t));
        rim.filter(|&p| !inside(p, ox, oy))
            .chain(inner.filter(|&p| inside(p, cx, cy)))
            .collect::<Vec<P>>()
    };

    let lens = format!(
        "M12 {} A{} {} 0 0 1 12 {} A{} {} 0 0 1 12 {} Z",
        f(cy - h),
        f(r),
        f(r),
        f(cy + h),
        f(r),
        f(r),
        f(cy - h)
    );
    let none = r#"stroke="none""#;
    els![
        circle(c1, cy, r, SOFT, none),
        poly(&crescent(c1), MID, none),
        circle(c1 + 0.6, cy - 2.6, 1.8, TOP, none),
        circle(c2, cy, r, MID, none),
        poly(&crescent(c2), SHADE, none),
        circle(c2 + 1.9, cy - 2.2, 2.0, TOP, none),
        path(&lens, ACCENT, r#"stroke="none" fill-opacity="0.7""#),
        circle(c1, cy, r, "", ""),
        circle(c2, cy, r, "", ""),
    ]
}

fn split() -> Vec<String> {
    // Context: the two halves. Input: the splitting plane (tinted, accent edge).
    let o = (11.0, 12.0);
    let p = projector(o);
    let (s, g, cut) = (8.0, 2.2, 3.6);
    let q = 2.2;
    let x = cut + g / 2.0;
    let plane = [
        p(x, -q, s + q),
        p(x, s + q, s + q),
        p(x, s + q, -q),
        p(x, -q, -q),
    ];
    els![
        iso_box(o, 0.0, cut, 0.0, s, 0.0, s),
        poly(
            &plane,
            ACCENT,
            &format!(r#"{} fill-opacity="0.3""#, sel_edge(1.2))
        ),
        iso_box(o, cut + g, s + g, 0.0, s, 0.0, s),
    ]
}

fn transform() -> Vec<String> {
    // Context: the part where it is. Output: where it goes (dashed). INK arrow for the move.
    let (o1, o2) = ((7.2, 16.6), (16.8, 7.4));
    let c = 4.6;
    let (p1, p2) = (projector(o1), projector(o2));
    let (a, b) = (p1(c, c * 0.35, c * 0.9), p2(c * 0.35, c, c * 0.15));
    els![
        ghost_box(o2, 0.0, c, 0.0, c, 0.0, c, false),
        iso_box(o1, 0.0, c, 0.0, c, 0.0, c),
        arrow(a.0, a.1, b.0, b.1, 3.0, 2.8, INK, r#"stroke-width="1.4""#),
    ]
}

fn composite_part() -> Vec<String> {
    // Context: a plate with two blocks on it, kept as they are. Output: the one part they make
    // (a dashed accent outline round all three, the silhouette of their bounding box).
    let o = (11.6, 5.2);
    let p = projector(o);
    let (x0, x1, y0, y1, z0, z1) = (-1.2, 11.2, -1.2, 9.2, -1.0, 6.4);
    let sil = [
        p(x0, y0, z1),
        p(x1, y0, z1),
        p(x1, y0, z0),
        p(x1, y1, z0),
        p(x0, y1, z0),
        p(x0, y1, z1),
    ];
    els![
        iso_box(o, 0.0, 10.0, 0.0, 8.0, 0.0, 1.4),
        iso_box(o, 1.2, 4.4, 1.0, 4.0, 1.4, 5.2),
        iso_box(o, 5.4, 8.4, 4.0, 6.8, 1.4, 3.6),
        poly(&sil, "", &format!(r#"stroke="{ACCENT}" {}"#, dash(1.1))),
    ]
}

fn plane() -> Vec<String> {
    // Context: a reference face. Output: the new plane offset above it (dashed, faint).
    let o = (12.0, 5.6);
    let p = projector(o);
    let s = 10.0;
    els![
        poly(
            &[
                p(0.0, 0.0, -6.0),
                p(s, 0.0, -6.0),
                p(s, s, -6.0),
                p(0.0, s, -6.0)
            ],
            TOP,
            ""
        ),
        poly(
            &[
                p(0.0, 0.0, 0.0),
                p(s, 0.0, 0.0),
                p(s, s, 0.0),
                p(0.0, s, 0.0)
            ],
            ACCENT,
            &out(1.2)
        ),
    ]
}

fn mate_connector() -> Vec<String> {
    // Context: the connector symbol, a disc on the floor plane with one quarter filled. Output:
    // the accent triad rising from its centre (z up, x and y along the floor).
    let o = (12.0, 14.6);
    let p = projector(o);
    let r = 6.6;
    let disc = iso_circle(o, 0.0, 0.0, 0.0, r, 48);
    let wedge: Vec<P> = std::iter::once(p(0.0, 0.0, 0.0))
        .chain(
            (0..=90)
                .step_by(6)
                .map(|t| p(r * rad(t as f64).cos(), r * rad(t as f64).sin(), 0.0)),
        )
        .collect();
    let w = r#"stroke-width="1.4""#;
    let c = p(0.0, 0.0, 0.0);
    let to = |q: P| arrow(c.0, c.1, q.0, q.1, 3.0, 2.8, ACCENT, w);
    els![
        poly(&disc, TOP, ""),
        poly(&wedge, MID, r#"stroke-width="0.9""#),
        to(p(0.0, 0.0, 12.0)),
        to(p(9.8, 0.0, 0.0)),
        to(p(0.0, 9.8, 0.0)),
        circle(c.0, c.1, 1.3, ACCENT, r#"stroke="none""#),
    ]
}

fn custom_feature() -> Vec<String> {
    let o = (10.0, 10.4);
    let p = projector(o);
    let s = 7.5;
    let (h, m) = (s * 0.45, s * 0.5);
    let (bx, by, a) = (18.4, 18.4, 2.9);
    els![
        poly(
            &[p(0.0, 0.0, s), p(m, 0.0, s), p(m, s, s), p(0.0, s, s)],
            TOP,
            ""
        ),
        poly(
            &[p(m, 0.0, s), p(m, s, s), p(m, s, h), p(m, 0.0, h)],
            MID,
            ""
        ),
        poly(
            &[p(m, 0.0, h), p(s, 0.0, h), p(s, s, h), p(m, s, h)],
            TOP,
            ""
        ),
        poly(
            &[p(s, 0.0, h), p(s, s, h), p(s, s, 0.0), p(s, 0.0, 0.0)],
            MID,
            ""
        ),
        poly(
            &[
                p(0.0, s, s),
                p(m, s, s),
                p(m, s, h),
                p(s, s, h),
                p(s, s, 0.0),
                p(0.0, s, 0.0)
            ],
            SHADE,
            ""
        ),
        path(
            &format!(
                "M{} {} H{} M{} {} V{}",
                f(bx - a),
                f(by),
                f(bx + a),
                f(bx),
                f(by - a),
                f(by + a)
            ),
            "",
            &acc(2.3)
        ),
    ]
}

fn measure() -> Vec<String> {
    // Context: the block. Output: the accent dimension between its end faces.
    let o = (9.6, 12.4);
    let p = projector(o);
    let (l, w, h, up) = (12.0, 6.0, 4.5, 3.6);
    let m = p(l / 2.0, 0.0, h + up);
    let (e0, e1) = (p(0.0, 0.0, h + up), p(l, 0.0, h + up));
    els![
        iso_box(o, 0.0, l, 0.0, w, 0.0, h),
        seg(p(0.0, 0.0, h + 1.0), p(0.0, 0.0, h + up + 1.2), THIN),
        seg(p(l, 0.0, h + 1.0), p(l, 0.0, h + up + 1.2), THIN),
        arrow(
            m.0,
            m.1,
            e0.0,
            e0.1,
            2.8,
            2.6,
            ACCENT,
            r#"stroke-width="1.4""#
        ),
        arrow(
            m.0,
            m.1,
            e1.0,
            e1.1,
            2.8,
            2.6,
            ACCENT,
            r#"stroke-width="1.4""#
        ),
    ]
}

fn section_view() -> Vec<String> {
    let o = (10.3, 12.0);
    let p = projector(o);
    let (l, d, h) = (10.0, 6.0, 8.0);
    let w = 2.0; // wall
    let cut = [p(0.0, d, h), p(l, d, h), p(l, d, 0.0), p(0.0, d, 0.0)];
    let cav = [
        p(w, d, h - w),
        p(l - w, d, h - w),
        p(l - w, d, w),
        p(w, d, w),
    ];
    let dd = |ps: &[P]| {
        format!(
            "M{} Z",
            ps.iter()
                .map(|&(x, y)| format!("{} {}", f(x), f(y)))
                .collect::<Vec<_>>()
                .join(" L")
        )
    };
    els![
        poly(
            &[p(0.0, 0.0, h), p(l, 0.0, h), p(l, d, h), p(0.0, d, h)],
            TOP,
            ""
        ),
        poly(
            &[p(l, 0.0, h), p(l, d, h), p(l, d, 0.0), p(l, 0.0, 0.0)],
            MID,
            ""
        ),
        poly(&cav, SHADE, r#"stroke="none""#),
        poly(
            &clip(
                &[
                    p(w, w, h - w),
                    p(w, d + 1.0, h - w),
                    p(w, d + 1.0, w),
                    p(w, w, w)
                ],
                &cav
            ),
            MID,
            HAIR
        ),
        poly(
            &clip(
                &[
                    p(w, w, w),
                    p(l - w, w, w),
                    p(l - w, d + 1.0, w),
                    p(w, d + 1.0, w)
                ],
                &cav
            ),
            TOP,
            HAIR
        ),
        path(
            &format!("{} {}", dd(&cut), dd(&cav)),
            ACCENT,
            r#"fill-rule="evenodd""#
        ),
    ]
}

fn mass_properties() -> Vec<String> {
    let o = (12.0, 12.0);
    let (cx, cy, r) = (12.0, 12.0, 3.6);
    let q = |a0: f64| {
        format!(
            "M{} {} L{} {} A{} {} 0 0 1 {} {} Z",
            f(cx),
            f(cy),
            f(cx + r * rad(a0).cos()),
            f(cy + r * rad(a0).sin()),
            f(r),
            f(r),
            f(cx + r * rad(a0 + 90.0).cos()),
            f(cy + r * rad(a0 + 90.0).sin())
        )
    };
    els![
        cube(o, 8.0),
        circle(cx, cy, r, TOP, &acc(1.3)),
        path(
            &format!("{} {}", q(-90.0), q(90.0)),
            ACCENT,
            r#"stroke="none""#
        )
    ]
}

fn part() -> Vec<String> {
    els![cube((12.0, 12.0), 8.4)]
}

fn sketch() -> Vec<String> {
    // A sketch plane (context) with the profile drawn on it (output, accent) and the pencil
    // drawing it, its tip on the profile's far corner.
    let o = (12.0, 8.8);
    let p = projector(o);
    let (s, a, b, r) = (11.0, 2.4, 8.6, 2.8);
    let plane = [
        p(0.0, 0.0, 0.0),
        p(s, 0.0, 0.0),
        p(s, s, 0.0),
        p(0.0, s, 0.0),
    ];
    let arc: Vec<P> = (0..=90)
        .step_by(10)
        .map(|t| {
            p(
                b - r + r * rad(t as f64).cos(),
                b - r + r * rad(t as f64).sin(),
                0.0,
            )
        })
        .collect();
    let prof = cat(&[&[p(a, a, 0.0), p(b, a, 0.0)], &arc, &[p(a, b, 0.0)]]);
    let tip = p(a, a, 0.0);
    let ang = rad(-68.0);
    let (ux, uy) = (ang.cos(), ang.sin());
    let (nx, ny) = (-uy, ux);
    let (l, w, cone) = (6.4, 1.6, 2.4);
    let base = (tip.0 + ux * cone, tip.1 + uy * cone);
    let end = (base.0 + ux * l, base.1 + uy * l);
    let body = [
        (base.0 + nx * w, base.1 + ny * w),
        (end.0 + nx * w, end.1 + ny * w),
        (end.0 - nx * w, end.1 - ny * w),
        (base.0 - nx * w, base.1 - ny * w),
    ];
    els![
        poly(&plane, TOP, ""),
        poly(
            &prof,
            ACCENT,
            &format!(r#"stroke="{ACCENT}" stroke-width="1.5" fill-opacity="0.15""#)
        ),
        poly(&body, SOFT, ""),
        poly(&[tip, body[0], body[3]], TOP, ""),
    ]
}

fn surface() -> Vec<String> {
    // A thin curved sheet with no real thickness: a surface body. Neutral, like part.
    let top = "M2.5 9.5 C7 3.5, 11 13.5, 15.5 7.5 L21.5 11.5 C17 17.5, 13 7.5, 8.5 13.5 Z";
    let edge = "M8.5 13.5 C13 7.5, 17 17.5, 21.5 11.5 L21.5 13.4 C17 19.4, 13 9.4, 8.5 15.4 Z";
    let left = "M2.5 9.5 L8.5 13.5 L8.5 15.4 L2.5 11.4 Z";
    els![
        path(edge, SHADE, ""),
        path(left, MID, ""),
        path(top, TOP, "")
    ]
}
