//! Sheet metal features (kind Solid) and the sheet metal panels (kind Line).
//!
//! House style for this module: sheets are drawn as a thin profile in the xz plane extruded along y
//! (`sheet`): flat walls, round bends, a visible end cap on the near (y max) side. Grey faces are
//! context; ACCENT marks what the tool makes or acts on (the bend, the new wall, the hem, the relief
//! cut), one idea per icon, as in part.rs.

use crate::common::*;
use Kind::{Line, Solid};

pub fn icons() -> Vec<Def> {
    vec![
        def(
            "sheet-metal-model",
            Solid,
            "Sheet metal model",
            sheet_metal_model,
        ),
        def(
            "sheet-metal-finish",
            Solid,
            "Finish sheet metal model",
            sheet_metal_finish,
        ),
        def("sheet-metal-flange", Solid, "Flange", sheet_metal_flange),
        def("sheet-metal-hem", Solid, "Hem", sheet_metal_hem),
        def("sheet-metal-tab", Solid, "Tab", sheet_metal_tab),
        def("sheet-metal-bend", Solid, "Bend", sheet_metal_bend),
        def("sheet-metal-jog", Solid, "Jog", sheet_metal_jog),
        def("sheet-metal-form", Solid, "Form", sheet_metal_form),
        def(
            "sheet-metal-loft",
            Solid,
            "Sheet metal loft",
            sheet_metal_loft,
        ),
        def(
            "sheet-metal-make-joint",
            Solid,
            "Make joint",
            sheet_metal_make_joint,
        ),
        def(
            "sheet-metal-modify-joint",
            Solid,
            "Modify joint",
            sheet_metal_modify_joint,
        ),
        def("sheet-metal-corner", Solid, "Corner", sheet_metal_corner),
        def(
            "sheet-metal-bend-relief",
            Solid,
            "Bend relief",
            sheet_metal_bend_relief,
        ),
        def(
            "sheet-metal-corner-break",
            Solid,
            "Corner break",
            sheet_metal_corner_break,
        ),
        def(
            "sheet-metal-table",
            Line,
            "Sheet metal table and flat view",
            sheet_metal_table,
        ),
        def("flat-pattern", Line, "Flat pattern", flat_pattern),
    ]
}

const T: f64 = 1.3; // sheet thickness
const RB: f64 = 1.6; // inner bend radius
const W: f64 = 7.5; // sheet depth along y
const HAIR: &str = r#"stroke-width="0.7""#;

/// Points on a circle in the xz plane from angle a0 to a1 (degrees, counter-clockwise), in 8 steps.
fn arc(cx: f64, cz: f64, r: f64, a0: f64, a1: f64) -> Vec<P> {
    let n = 8;
    (0..=n)
        .map(|i| {
            let a = rad(a0 + (a1 - a0) * i as f64 / n as f64);
            (cx + r * a.cos(), cz + r * a.sin())
        })
        .collect()
}

/// Palette grey for a face whose outward normal in the xz plane is (nx, nz).
fn face_fill(nx: f64, nz: f64) -> &'static str {
    if nz > 0.75 {
        return TOP;
    }
    if nx > 0.75 {
        return MID;
    }
    SOFT
}

/// A sheet: the closed counter-clockwise profile `prof` [(x, z)] extruded from y = 0 to W.
/// Edges listed in `accent` (indices into the profile) are filled with ACCENT. Returns the
/// elements back to front: the faces you can see from the camera, then the near end cap.
fn sheet(o: P, prof: &[P], accent: &[usize]) -> Vec<String> {
    let p = projector(o);
    let (y0, y1) = (0.0, W);
    let n = prof.len();
    let mut quads = Vec::new();
    for i in 0..n {
        let ((x0, z0), (x1, z1)) = (prof[i], prof[(i + 1) % n]);
        let (dx, dz) = (x1 - x0, z1 - z0);
        let l = hypot(dx, dz);
        if l < 1e-9 {
            continue;
        }
        let (nx, nz) = (dz / l, -dx / l); // outward for a counter-clockwise profile (x right, z up)
        if nx + nz <= 1e-6 {
            continue; // faces away from the camera
        }
        let fill = if accent.contains(&i) {
            ACCENT
        } else {
            face_fill(nx, nz)
        };
        let depth = (x0 + x1) / 2.0 + (z0 + z1) / 2.0;
        quads.push((
            depth,
            poly(
                &[p(x0, y0, z0), p(x1, y0, z1), p(x1, y1, z1), p(x0, y1, z0)],
                fill,
                HAIR,
            ),
        ));
    }
    quads.sort_by(|a, b| a.0.total_cmp(&b.0));
    let cap: Vec<P> = prof.iter().map(|&(x, z)| p(x, y1, z)).collect();
    els![
        quads.into_iter().map(|q| q.1).collect::<Vec<_>>(),
        poly(&cap, SHADE, "")
    ]
}

/// A base along x with a wall bent up at x = l: the closed outline, counter-clockwise, and the
/// indices of the bend's edges (inner and outer arcs).
fn l_profile(l: f64, h: f64) -> (Vec<P>, Vec<usize>) {
    let (t, r) = (T, RB);
    let prof = cat(&[
        &[(0.0, 0.0)],
        &arc(l - t - r, t + r, r + t, 270.0, 360.0),
        &[(l, h), (l - t, h)],
        &arc(l - t - r, t + r, r, 360.0, 270.0),
        &[(0.0, t)],
    ]);
    // Edges: 0 base bottom, 1-8 outer arc, 9 outer wall, 10 top, 11 inner wall, 12-19 inner arc.
    let bend = (1..9).chain(12..20).collect();
    (prof, bend)
}

fn sheet_metal_model() -> Vec<String> {
    // A sheet bent up into an L; the bend is what makes it sheet metal (its end section in accent:
    // the bend's own faces turn away from the camera).
    let o = (9.2, 9.8);
    let (prof, bend) = l_profile(10.0, 9.0);
    let p = projector(o);
    let section = cat(&[&prof[1..10], &prof[12..21]]); // outer arc, then inner arc back
    let sec: Vec<P> = section.iter().map(|&(x, z)| p(x, W, z)).collect();
    els![sheet(o, &prof, &bend), poly(&sec, ACCENT, HAIR)]
}

fn sheet_metal_finish() -> Vec<String> {
    // The finished L, with a check mark: no more sheet metal features.
    let (prof, _) = l_profile(9.0, 8.0);
    els![
        sheet((9.2, 11.5), &prof, &[]),
        polyline(
            &[(4.2, 6.2), (6.4, 8.4), (10.6, 3.8)],
            &format!(r#"stroke="{ACCENT}" stroke-width="1.8""#)
        ),
    ]
}

fn sheet_metal_flange() -> Vec<String> {
    // Context: the base. Output: the new wall and its bend (accent).
    let (prof, mut wall) = l_profile(10.0, 9.0);
    wall.extend([9, 10, 11]);
    sheet((9.2, 9.8), &prof, &wall)
}

fn sheet_metal_hem() -> Vec<String> {
    // The base's edge folded back over itself (accent).
    let (l, t, r, h) = (11.0, T, 0.9, 5.0);
    let (cx, cz) = (l - r - t, t + r);
    let prof = cat(&[
        &[(0.0, 0.0), (cx, 0.0)],
        &arc(cx, cz, r + t, 270.0, 450.0)[1..],
        &[
            (l - r - t - h, t + 2.0 * r + t),
            (l - r - t - h, 2.0 * r + t),
        ],
        &arc(cx, cz, r, 90.0, -90.0),
        &[(0.0, t)],
    ]);
    let hem: Vec<usize> = (1..prof.len() - 2).collect(); // all but the base's bottom, top and end
    sheet((8.6, 11.2), &prof, &hem)
}

fn sheet_metal_tab() -> Vec<String> {
    // A wall with a tongue added on its top edge (accent).
    let (prof, _) = l_profile(10.0, 7.5);
    let o = (9.2, 10.6);
    els![
        sheet(o, &prof, &[]),
        iso_box_with(
            o,
            10.0 - T,
            10.0,
            W * 0.3,
            W * 0.7,
            7.5,
            10.5,
            ACCENT,
            ACCENT,
            ACCENT,
            HAIR
        )
    ]
}

fn sheet_metal_bend() -> Vec<String> {
    // A flat sheet folded along a bend line (accent dashes) on its top face.
    let a = rad(50.0);
    let (l1, l2, t, r) = (7.0, 7.0, T, RB);
    let c = (l1, t + r); // bend centre
    let p_in = arc(c.0, c.1, r, 270.0, 270.0 + 50.0);
    let p_out = arc(c.0, c.1, r + t, 270.0, 270.0 + 50.0);
    let (ex, ez) = (a.cos(), a.sin());
    let (li, lo) = (p_in[p_in.len() - 1], p_out[p_out.len() - 1]);
    let tip_in = (li.0 + l2 * ex, li.1 + l2 * ez);
    let tip_out = (lo.0 + l2 * ex, lo.1 + l2 * ez);
    let prof = cat(&[
        &[(0.0, 0.0)],
        &p_out,
        &[tip_out, tip_in],
        &rev(&p_in),
        &[(0.0, t)],
    ]);
    let o = (6.5, 13.5);
    let p = projector(o);
    els![
        sheet(o, &prof, &[]),
        seg(
            p(l1 - 2.2, 0.0, t),
            p(l1 - 2.2, W, t),
            &format!(r#"stroke="{ACCENT}" {}"#, dash(1.2))
        )
    ]
}

fn sheet_metal_jog() -> Vec<String> {
    // A Z-shaped step: two opposite bends offset the sheet (accent).
    let (t, r, l, d) = (T, 1.0, 5.0, 3.5);
    let c1 = (l, t + r);
    let c2 = (l + r + t + r, t + d - r);
    let prof = cat(&[
        &[(0.0, 0.0)],
        &arc(c1.0, c1.1, r + t, 270.0, 360.0),
        &arc(c2.0, c2.1, r, 180.0, 90.0),
        &[(c2.0 + l, t + d), (c2.0 + l, 2.0 * t + d)],
        &arc(c2.0, c2.1, r + t, 90.0, 180.0),
        &arc(c1.0, c1.1, r, 360.0, 270.0),
        &[(0.0, t)],
    ]);
    let k = 9; // points per arc
    // Edges 1..2k-1: lower bend, riser, upper bend (outside); 2k+3..4k+1: the same inside.
    let step: Vec<usize> = (1..2 * k).chain(2 * k + 3..4 * k + 2).collect();
    sheet((7.5, 13.0), &prof, &step)
}

fn sheet_metal_form() -> Vec<String> {
    // A flat sheet with a louver pressed into it (accent).
    let o = (12.3, 9.6);
    let p = projector(o);
    let plate = iso_box(o, 0.0, 11.0, 0.0, 9.0, 0.0, T);
    let (x0, x1, y0, y1, h) = (3.0, 8.0, 3.2, 5.8, 1.9);
    let hood = [
        p(x0, y1, T),
        p(x0, y0, T + h),
        p(x1, y0, T + h),
        p(x1, y1, T),
    ];
    let side = [p(x1, y1, T), p(x1, y0, T + h), p(x1, y0, T)];
    els![
        plate,
        poly(
            &[
                p(x0, y0, T),
                p(x1, y0, T),
                p(x1, y0, T + h),
                p(x0, y0, T + h)
            ],
            SHADE,
            HAIR
        ),
        poly(&hood, ACCENT, HAIR),
        poly(&side, ACCENT, HAIR),
    ]
}

fn sheet_metal_loft() -> Vec<String> {
    // A duct from a square (bottom) to a circle (top): the lofted sheet, its top rim in accent.
    let o = (12.0, 17.5);
    let p = projector(o);
    let (s, r, h) = (4.6, 3.0, 9.0);
    let sq = [p(-s, -s, 0.0), p(s, -s, 0.0), p(s, s, 0.0), p(-s, s, 0.0)];
    let top = iso_circle(o, 0.0, 0.0, h, r, 48);
    let (rx, ry) = (r * 1.5f64.sqrt(), r * 0.5f64.sqrt());
    let (tx, ty) = p(0.0, 0.0, h);
    let (left, right, front) = ((tx - rx, ty), (tx + rx, ty), (tx, ty + ry));
    els![
        poly(&[sq[2], sq[1], right, front], MID, HAIR),
        poly(&[sq[3], sq[2], front, left], SHADE, HAIR),
        poly(&top, TOP, HAIR),
        poly(
            &top,
            "",
            &format!(r#"stroke="{ACCENT}" stroke-width="1.4""#)
        ),
    ]
}

fn sheet_metal_make_joint() -> Vec<String> {
    // Two walls meeting at a corner; the seam between them is the joint (accent).
    let o = (12.0, 13.2);
    let h = 8.5;
    let p = projector(o);
    els![
        iso_box(o, 0.0, T, -6.0, 0.0, 0.0, h),
        iso_box(o, 0.8, 7.2, 0.0, T, 0.0, h),
        seg(
            p(T + 0.4, T, 0.0),
            p(T + 0.4, T, h),
            &format!(r#"stroke="{ACCENT}" stroke-width="1.6""#)
        ),
    ]
}

fn sheet_metal_modify_joint() -> Vec<String> {
    // The L with its bend dashed (accent): the joint being changed.
    let (prof, _) = l_profile(10.0, 9.0);
    let o = (9.2, 9.8);
    let p = projector(o);
    let outer: Vec<P> = prof[1..10].iter().map(|&(x, z)| p(x, W, z)).collect();
    els![
        sheet(o, &prof, &[]),
        polyline(&outer, &format!(r#"stroke="{ACCENT}" {}"#, dash(1.4)))
    ]
}

/// A base plate with walls up along its x max and y max edges, meeting at a corner (x max, y
/// max): the corner the corner tools act on.
fn tray(o: P, base: f64) -> Vec<String> {
    let (flange, t) = (7.0, T);
    els![
        iso_box(o, 0.0, base, 0.0, base, 0.0, t),
        iso_box(o, base - t, base, 0.0, base - t - 0.6, t, flange),
        iso_box(o, 0.0, base - t - 0.6, base - t, base, t, flange),
    ]
}

fn sheet_metal_corner() -> Vec<String> {
    // Two walls meeting at a corner of the base; the round relief cut there (accent).
    let o = (12.0, 9.2);
    let base = 10.0;
    let c = iso_circle(o, base - T - 0.3, base - T - 0.3, T, 1.8, 48);
    els![tray(o, base), poly(&c, ACCENT, HAIR)]
}

fn sheet_metal_bend_relief() -> Vec<String> {
    // A wall covering part of the base's edge; the slot cut where its bend ends (accent).
    let o = (11.0, 9.5);
    let p = projector(o);
    let (l, d) = (10.0, 10.0);
    let (y0, y1) = (3.2, d);
    let slot = [
        p(l - T - 2.6, y0 - 1.0, T),
        p(l, y0 - 1.0, T),
        p(l, y0, T),
        p(l - T - 2.6, y0, T),
    ];
    els![
        iso_box(o, 0.0, l, 0.0, d, 0.0, T),
        iso_box(o, l - T, l, y0, y1, T, 8.0),
        poly(&slot, ACCENT, HAIR)
    ]
}

fn sheet_metal_corner_break() -> Vec<String> {
    // A plate whose near corner is rounded off (accent: the rounded edge).
    let o = (12.5, 8.8);
    let p = projector(o);
    let (s, r, t) = (10.0, 3.6, T);
    let arc: Vec<P> = (0..=90)
        .step_by(10)
        .map(|a| {
            (
                s - r + r * rad(a as f64).cos(),
                s - r + r * rad(a as f64).sin(),
            )
        })
        .collect();
    let on = |z: f64, ps: &[P]| ps.iter().map(|&(x, y)| p(x, y, z)).collect::<Vec<_>>();
    let top = cat(&[
        &[p(0.0, 0.0, t), p(s, 0.0, t)],
        &on(t, &arc),
        &[p(0.0, s, t)],
    ]);
    let right = [
        p(s, 0.0, t),
        p(s, 0.0, 0.0),
        p(s, s - r, 0.0),
        p(s, s - r, t),
    ];
    let left = [
        p(0.0, s, t),
        p(s - r, s, t),
        p(s - r, s, 0.0),
        p(0.0, s, 0.0),
    ];
    let band = cat(&[&on(t, &arc), &on(0.0, &rev(&arc))]);
    els![
        poly(&right, MID, HAIR),
        poly(&left, SHADE, HAIR),
        poly(&band, ACCENT, HAIR),
        poly(&top, TOP, HAIR)
    ]
}

// ---------------------------------------------------------------------------------------------
// Panels (kind Line)

fn lacc(el: String) -> String {
    el.replacen("/>", &format!(r#" stroke="{ACCENT}"/>"#), 1)
}

fn sheet_metal_table() -> Vec<String> {
    // A table (top) over a flat pattern with its bend line (accent dashes).
    els![
        rect(3.0, 3.0, 18.0, 7.5, 1.5, "", ""),
        path("M3 6.75 H21 M9 3 V10.5", "", ""),
        rect(3.0, 13.5, 18.0, 7.5, 1.5, "", ""),
        lacc(line(12.0, 13.5, 12.0, 21.0, &dash(1.75))),
    ]
}

fn flat_pattern() -> Vec<String> {
    // An unfolded box: a cross-shaped outline with its bend lines dashed (accent).
    let cross = path("M8 3 H16 V8 H21 V16 H16 V21 H8 V16 H3 V8 H8 Z", "", "");
    let bends = path(
        "M8 8 H16 M8 16 H16 M8 8 V16 M16 8 V16",
        "",
        &format!(r#"stroke="{ACCENT}" {}"#, dash(1.5)),
    );
    els![cross, bends]
}
