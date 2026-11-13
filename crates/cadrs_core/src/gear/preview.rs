//! Fast previews of generated gears: a member's display mesh straight from its generation's
//! equations, with no B-rep and no booleans, in well under a millisecond, so its shafts can be
//! dragged and the pair seen at once (the Gear feature's Fast preview).
//!
//! A hypoid member is sliced by spheres about its apex, from its toe to its heel. In each
//! slice, its two flanks are solved where they cross it (the face mill on the member's machine,
//! [`super::FaceMill`]: its crown about the crown's axis, the member about its own, both
//! blades' cones in closed form), one tooth space's, and turned round to every tooth. The
//! slice's outline is the band between the back cone and the face cone with the spaces notched
//! down to the root cone. The outlines are stitched slice to slice, each surface a strip of its
//! own (its edges sharp), and closed at the toe and the heel. Roundings, reliefs and the
//! fillets the blades' tips trace are left out.

use super::hypoid::{Design, Layout};
use super::{FaceMill, Vec3, add, cross, dot, norm, scale, sub, unit};
use std::f64::consts::{PI, TAU};

/// A triangle mesh: its points, their normals and its triangles.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub indices: Vec<u32>,
}

impl Mesh {
    /// A strip stitched between rows of points (each row the same length; `closed`: each row
    /// a loop), its own points.
    fn strip(&mut self, rows: &[Vec<Vec3>], closed: bool) -> std::ops::Range<usize> {
        let first = self.indices.len() / 3;
        let base = self.positions.len() as u32;
        let n = rows[0].len();
        for r in rows {
            self.positions.extend_from_slice(r);
        }
        let at = |j: usize, i: usize| base + (j * n + i % n) as u32;
        let across = if closed { n } else { n - 1 };
        for j in 0..rows.len() - 1 {
            for i in 0..across {
                let (a, b, c, d) = (at(j, i), at(j, i + 1), at(j + 1, i), at(j + 1, i + 1));
                self.indices.extend_from_slice(&[a, b, c, b, d, c]);
            }
        }
        first..self.indices.len() / 3
    }

    /// The normal (unnormalised) of triangle `t`.
    fn normal(&self, t: usize) -> Vec3 {
        let i = &self.indices[3 * t..3 * t + 3];
        let (a, b, c) = (
            self.positions[i[0] as usize],
            self.positions[i[1] as usize],
            self.positions[i[2] as usize],
        );
        cross(sub(b, a), sub(c, a))
    }

    /// The middle of triangle `t`.
    fn middle(&self, t: usize) -> Vec3 {
        let i = &self.indices[3 * t..3 * t + 3];
        let sum = add(
            add(self.positions[i[0] as usize], self.positions[i[1] as usize]),
            self.positions[i[2] as usize],
        );
        scale(sum, 1.0 / 3.0)
    }

    /// Triangles `range` turned over.
    fn flip(&mut self, range: std::ops::Range<usize>) {
        for t in range {
            self.indices.swap(3 * t + 1, 3 * t + 2);
        }
    }

    /// A flat-ish cap over the polygon `outer` (a loop) less the loop `hole`, triangulated in
    /// the plane coordinates `flat` of their points (the outer's, then the hole's).
    fn cap(
        &mut self,
        outer: &[Vec3],
        hole: &[Vec3],
        flat: &[[f64; 2]],
        out: impl Fn(Vec3) -> Vec3,
    ) -> bool {
        let base = self.positions.len() as u32;
        let coords: Vec<f64> = flat.iter().flat_map(|f| [f[0], f[1]]).collect();
        let holes: &[usize] = if hole.is_empty() { &[] } else { &[outer.len()] };
        let Ok(tris) = earcutr::earcut(&coords, holes, 2) else {
            return false;
        };
        self.positions.extend_from_slice(outer);
        self.positions.extend_from_slice(hole);
        let first = self.indices.len() / 3;
        self.indices
            .extend(tris.into_iter().map(|i| base + i as u32));
        // Each triangle facing `out` at its middle.
        for t in first..self.indices.len() / 3 {
            if dot(self.normal(t), out(self.middle(t))) < 0.0 {
                self.flip(t..t + 1);
            }
        }
        true
    }

    /// Its normals: each point's triangles' (each strip and cap has points of its own, so its
    /// edges stay sharp).
    fn finish(&mut self) {
        let mut normals = vec![[0.0; 3]; self.positions.len()];
        for t in self.indices.chunks_exact(3) {
            let (a, b, c) = (
                self.positions[t[0] as usize],
                self.positions[t[1] as usize],
                self.positions[t[2] as usize],
            );
            let n = cross(sub(b, a), sub(c, a));
            for &i in t {
                normals[i as usize] = add(normals[i as usize], n);
            }
        }
        self.normals = normals
            .into_iter()
            .map(|n| {
                if norm(n) > 0.0 {
                    unit(n)
                } else {
                    [0.0, 0.0, 1.0]
                }
            })
            .collect();
    }
}

/// `p` turned by `a` about the z axis.
fn about_z(p: Vec3, a: f64) -> Vec3 {
    let (s, c) = a.sin_cos();
    [c * p[0] - s * p[1], s * p[0] + c * p[1], p[2]]
}

/// A hypoid pair's member's preview, in the pair's frame (as [`super::hypoid::member`] has it),
/// or None where its flanks can't be followed across its face.
pub(super) fn hypoid_member(d: &Design, l: &Layout, gear: bool, grow: f64) -> Option<Mesh> {
    let debug = std::env::var_os("PREVIEW_DEBUG").is_some();
    let z = [0.0, 0.0, 1.0];
    let (apex, axis, pitch, cone, teeth) = if gear {
        (
            [0.0; 3],
            l.gear_axis,
            l.gear_pitch,
            l.mean_cone,
            d.gear_teeth,
        )
    } else {
        (
            l.pinion_apex,
            l.pinion_axis,
            l.pinion_pitch,
            l.pinion_cone,
            d.pinion_teeth,
        )
    };
    // The member's frame: x along its pitch line to M, z square to the pitch plane.
    let fx = unit(sub(l.m, apex));
    let fy = cross(z, fx);
    let local = |p: Vec3| {
        let q = sub(p, apex);
        [dot(q, fx), dot(q, fy), dot(q, z)]
    };
    let under = if gear { 1.0 } else { -1.0 };
    let g = [pitch.cos(), 0.0, -under * pitch.sin()];
    let x_g = [pitch.sin(), 0.0, under * pitch.cos()];
    let (ri, ro, ti, to) = (l.inner_radius, l.outer_radius, l.inner_tan, l.outer_tan);
    let (gi, go) = (grow * (1.0 + ti * ti).sqrt(), grow * (1.0 + to * to).sqrt());
    let ratio = l.crown_teeth / teeth;
    let room = PI * l.normal_module / 2.0;
    let mill = |centre: Vec3, radius: f64, tan_alpha: f64| {
        let centre = local(centre);
        FaceMill {
            hub: local([0.0; 3]),
            room,
            g,
            c: z,
            u: [1.0, 0.0, 0.0],
            v: [0.0, 1.0, 0.0],
            x_g,
            centre,
            radius,
            slot: 0.0,
            level: 0.0,
            tan_alpha,
            ratio,
            dedendum: 0.0,
            // (The contact is near the mean point: round the cutter toward it.)
            tau_mid: {
                let to_m = sub(local(l.m), centre);
                to_m[1].atan2(to_m[0])
            },
            delta: pitch,
            under,
        }
    };
    // The two blades that bound a space: the gear's, the outer blades of one cutter of its
    // crown's and the inner of the next (a crown pitch round); the pinion's, the crown tooth's.
    let blades = if gear {
        [
            mill(l.cutter, ro - go, -to),
            mill(about_z(l.cutter, -TAU / l.crown_teeth), ri + gi, ti),
        ]
    } else {
        [mill(l.cutter, ri - gi, ti), mill(l.cutter, ro + go, -to)]
    };
    // The blank in a sphere `s` about the apex: the angles from the axis of its back cone, its
    // root cone, its face cone (the pitch cone moved off along its normal).
    let off = |s: f64, by: f64| pitch + by.atan2((s * s - by * by).max(0.0).sqrt());
    let (add_, ded, back) = (l.addendum, l.dedendum, super::hypoid::back_of(gear, l));
    let width = 0.2 * l.mean_cone;
    // The blades' edges over the pitch plane (the crown's tool's): the gear's spaces from a
    // dedendum under it to the teeth's base over it, the pinion's teeth from their base under
    // it to a dedendum over it.
    let blade = if gear {
        (-l.dedendum, l.base)
    } else {
        (-l.base, l.dedendum)
    };
    const SLICES: usize = 8;
    const FLANK: usize = 8;
    const FLOOR: usize = 3;
    const LAND: usize = 4;
    let z_teeth = teeth.round() as usize;
    let tooth = TAU / teeth;
    // Each slice: its sphere, and its space's two flanks' azimuths from the root to the face.
    struct Slice {
        s: f64,
        angles: [f64; 3],
        flanks: [Vec<f64>; 2],
    }
    // From the middle slice (where the flanks are surely found) out to the heel, then to the
    // toe, each slice's solve started from its neighbour's.
    let mut found_slices: Vec<Option<(Slice, [Vec<[f64; 2]>; 2])>> =
        (0..=SLICES).map(|_| None).collect();
    let middle = SLICES / 2;
    // Whether the blades' flanks are the other way round about the space (set by the middle
    // slice: the blades stay which they are).
    let mut swapped = false;
    // Which way round each blade cuts into its tooth (found at the middle slice).
    let mut towards: [Option<f64>; 2] = [None, None];
    let order: Vec<(usize, Option<usize>)> = std::iter::once((middle, None))
        .chain((middle + 1..=SLICES).map(|j| (j, Some(j - 1))))
        .chain((0..middle).rev().map(|j| (j, Some(j + 1))))
        .collect();
    for (j, from) in order {
        let neighbour = from.and_then(|f| found_slices[f].as_ref());
        let was = neighbour.map(|n| n.1.clone());
        let s = cone - width / 2.0 + width * j as f64 / SLICES as f64;
        let angles = [off(s, -back), off(s, -ded), off(s, add_)];
        let thetas: Vec<f64> = (0..=FLANK)
            .map(|i| angles[1] + (angles[2] - angles[1]) * i as f64 / FLANK as f64)
            .collect();
        let mut sols: [Vec<[f64; 2]>; 2] = [Vec::new(), Vec::new()];
        let mut flanks: [Vec<f64>; 2] = [Vec::new(), Vec::new()];
        for (b, m) in blades.iter().enumerate() {
            // From the pitch cone (or the slice before), up and down the flank.
            let mid = FLANK / 2;
            let at = |th: f64| (s * th.cos(), s * th.sin());
            let start = {
                let (h, rho) = at(thetas[mid]);
                let guess = was.as_ref().map(|w| w[b][mid]);
                match m
                    .solve(1.0, h, rho, guess)
                    .or_else(|| m.solve(1.0, h, rho, None))
                {
                    Some(x) => x,
                    None => {
                        if debug {
                            eprintln!(
                                "preview: slice {j} blade {b}: no start at θ {:.4} (pitch {:.4})",
                                thetas[mid], pitch
                            );
                        }
                        return None;
                    }
                }
            };
            let mut found = vec![[0.0; 2]; FLANK + 1];
            found[mid] = start;
            // Down to the root and up to the face; where the flank can't be followed further
            // (it folds back: undercut, or the blade's tip takes over), on from its last
            // point (a preview's shortcut for the path of the blade's tip edge).
            let mut ok = [false; FLANK + 1];
            ok[mid] = true;
            for i in (0..mid).rev() {
                let (h, rho) = at(thetas[i]);
                if ok[i + 1]
                    && let Some(x) = m.solve(1.0, h, rho, Some(found[i + 1]))
                {
                    (found[i], ok[i]) = (x, true);
                }
            }
            for i in mid + 1..=FLANK {
                let (h, rho) = at(thetas[i]);
                if ok[i - 1]
                    && let Some(x) = m.solve(1.0, h, rho, Some(found[i - 1]))
                {
                    (found[i], ok[i]) = (x, true);
                }
            }
            let mut phis: Vec<f64> = found.iter().map(|x| m.point(1.0, x[0], x[1])[2]).collect();
            // Which way round the blade cuts into the tooth: the envelope's point is as far as
            // it reaches there (at the pitch cone of the middle slice, whole).
            let toward = *towards[b].get_or_insert_with(|| {
                let x = found[mid];
                let phi = m.turn_at(1.0, x[0], x[1]);
                let (h, rho) = at(thetas[mid]);
                let side = |d: f64| m.crossing(1.0, h, rho, phi + d, x).map(|c| c.1);
                // (The envelope's point a greatest angle: the blade reaches into the tooth the
                // greater way round.)
                match (side(-0.01), side(0.01)) {
                    (Some(a), Some(c)) if a + c > 2.0 * phis[mid] => -1.0,
                    _ => 1.0,
                }
            });
            // As far as the blade reaches round each circle as the cradle rolls (past the
            // envelope where it folds: undercut), from the envelope's point there, else the
            // reach of the circle before.
            let mut reached: Vec<Option<([f64; 2], f64)>> = vec![None; FLANK + 1];
            let order = std::iter::once(mid)
                .chain((0..mid).rev())
                .chain(mid + 1..=FLANK);
            for i in order {
                let (h, rho) = at(thetas[i]);
                let from = if ok[i] {
                    Some((found[i], m.turn_at(1.0, found[i][0], found[i][1])))
                } else if i < mid {
                    reached[i + 1]
                } else if i > mid {
                    reached[i - 1]
                } else {
                    None
                };
                if let Some(from) = from
                    && let Some((a, c)) = m.reach(1.0, h, rho, from, toward, blade)
                {
                    if debug {
                        let d = (a - phis[i] + PI).rem_euclid(TAU) - PI;
                        if !ok[i] || d.abs() > 1e-4 {
                            eprintln!(
                                "reach: gear {gear} slice {j} blade {b} i {i} ok {} env {:.5} reach {:.5} (d {d:.5}) turn {:.4} -> {:.4} w {:.3}",
                                ok[i], phis[i], a, from.1, c.1, c.0[1]
                            );
                        }
                    }
                    // (Unwrapped along the flank below.)
                    phis[i] = a;
                    ok[i] = true;
                    reached[i] = Some(c);
                }
            }
            for i in (0..mid).rev() {
                if !ok[i] {
                    phis[i] = phis[i + 1];
                }
            }
            for i in mid + 1..=FLANK {
                if !ok[i] {
                    phis[i] = phis[i - 1];
                }
            }
            flanks[b] = phis;
            sols[b] = found;
        }
        let wrap = |x: f64| (x + PI).rem_euclid(TAU) - PI;
        // Each flank along itself without a jump.
        for f in &mut flanks {
            for i in 1..=FLANK {
                f[i] = f[i - 1] + wrap(f[i] - f[i - 1]);
            }
        }
        match neighbour {
            // The middle slice: the space between its two flanks (each a tooth pitch round from
            // any other tooth's: all the teeth are alike), the narrower way round.
            None => {
                let shift = |d: f64| (d / tooth).round() * tooth;
                let d = flanks[1][FLANK / 2] - flanks[0][FLANK / 2];
                let s1 = shift(d);
                for a in flanks[1].iter_mut() {
                    *a -= s1;
                }
                if flanks[1][FLANK / 2] < flanks[0][FLANK / 2] {
                    swapped = true;
                    flanks.swap(0, 1);
                }
            }
            // Further out: each blade's flank as near its neighbour's as a tooth pitch allows
            // (the same flank, carried on).
            Some((n, _)) => {
                if swapped {
                    flanks.swap(0, 1);
                }
                for b in 0..2 {
                    let d = flanks[b][FLANK / 2] - n.flanks[b][FLANK / 2];
                    let k = (d / tooth).round() * tooth;
                    for a in flanks[b].iter_mut() {
                        *a -= k;
                    }
                }
            }
        }
        let gap = flanks[1][FLANK / 2] - flanks[0][FLANK / 2];
        if !(gap > 0.0 && gap < tooth) {
            if debug {
                eprintln!(
                    "preview: slice {j}: the space's width {gap:.4} of a tooth's {tooth:.4}; flanks {:?} neighbour {:?}",
                    [flanks[0][FLANK / 2], flanks[1][FLANK / 2]],
                    neighbour.map(|n| [n.0.flanks[0][FLANK / 2], n.0.flanks[1][FLANK / 2]])
                );
                for b in 0..2 {
                    eprintln!(
                        "   flank {b}: {:?}",
                        flanks[b]
                            .iter()
                            .map(|x| (x * 1e4).round() / 1e4)
                            .collect::<Vec<_>>()
                    );
                }
            }
            return None;
        }
        found_slices[j] = Some((Slice { s, angles, flanks }, sols));
    }
    let slices: Vec<Slice> = found_slices
        .into_iter()
        .map(|x| x.map(|(sl, _)| sl))
        .collect::<Option<_>>()?;
    // A point of the member at the sphere `s`, the angle `th` from its axis, the azimuth `phi`
    // (in the pair's frame).
    let world = |s: f64, th: f64, phi: f64| {
        let along = scale(g, s * th.cos());
        let round = add(scale(x_g, phi.cos()), scale([0.0, under, 0.0], phi.sin()));
        let p = add(along, scale(round, s * th.sin()));
        add(
            apex,
            add(add(scale(fx, p[0]), scale(fy, p[1])), scale(z, p[2])),
        )
    };
    let mut mesh = Mesh::default();
    // Each slice's outline, one piece at a time round the teeth: (θ, φ) along it.
    let outline = |sl: &Slice, k: usize| -> [Vec<(f64, f64)>; 4] {
        let t = |i: usize| sl.angles[1] + (sl.angles[2] - sl.angles[1]) * i as f64 / FLANK as f64;
        let turn = k as f64 * tooth;
        let (l0, r0) = (sl.flanks[0][0] + turn, sl.flanks[1][0] + turn);
        let (lf, rf) = (sl.flanks[0][FLANK] + turn, sl.flanks[1][FLANK] + turn);
        let left: Vec<(f64, f64)> = (0..=FLANK)
            .rev()
            .map(|i| (t(i), sl.flanks[0][i] + turn))
            .collect();
        let floor: Vec<(f64, f64)> = (1..FLOOR)
            .map(|i| (sl.angles[1], l0 + (r0 - l0) * i as f64 / FLOOR as f64))
            .collect();
        let right: Vec<(f64, f64)> = (0..=FLANK)
            .map(|i| (t(i), sl.flanks[1][i] + turn))
            .collect();
        let next = lf + tooth;
        let land: Vec<(f64, f64)> = (1..LAND)
            .map(|i| (sl.angles[2], rf + (next - rf) * i as f64 / LAND as f64))
            .collect();
        [left, floor, right, land]
    };
    // The strips: each piece of each tooth, slice to slice (a piece's ends shared with its
    // neighbours': the floor's and the land's take the flanks' ends).
    // (The outline's strips, and a land's to tell which way they face.)
    let first_outer = mesh.indices.len() / 3;
    let mut a_land: Option<std::ops::Range<usize>> = None;
    for k in 0..z_teeth {
        for piece in 0..4 {
            let rows: Vec<Vec<Vec3>> = slices
                .iter()
                .map(|sl| {
                    let o = outline(sl, k);
                    let mut pts: Vec<(f64, f64)> = o[piece].clone();
                    match piece {
                        1 => {
                            pts.insert(0, *o[0].last().unwrap());
                            pts.push(o[2][0]);
                        }
                        3 => {
                            pts.insert(0, *o[2].last().unwrap());
                            let next = outline(sl, (k + 1) % z_teeth);
                            let mut first = next[0][0];
                            if k + 1 == z_teeth {
                                first.1 += TAU;
                            }
                            pts.push(first);
                        }
                        _ => {}
                    }
                    pts.iter().map(|(th, ph)| world(sl.s, *th, *ph)).collect()
                })
                .collect();
            let r = mesh.strip(&rows, false);
            if piece == 3 && a_land.is_none() {
                a_land = Some(r);
            }
        }
    }
    let outer_strips = first_outer..mesh.indices.len() / 3;
    // Away from the axis: the land's; toward it, the back cone's.
    let from_axis = |p: Vec3| {
        let q = sub(p, apex);
        let along = dot(q, axis);
        sub(q, scale(axis, along))
    };
    let land = a_land?;
    let t = land.start + (land.end - land.start) / 2;
    if dot(mesh.normal(t), from_axis(mesh.middle(t))) < 0.0 {
        mesh.flip(outer_strips);
    }
    // The back cone, round.
    let round = 4 * z_teeth;
    let back_row = |sl: &Slice| -> Vec<Vec3> {
        (0..round)
            .map(|i| world(sl.s, sl.angles[0], TAU * i as f64 / round as f64))
            .collect()
    };
    let backs: Vec<Vec<Vec3>> = slices.iter().map(back_row).collect();
    let r = mesh.strip(&backs, true);
    let t = r.start + (r.end - r.start) / 2;
    if dot(mesh.normal(t), from_axis(mesh.middle(t))) > 0.0 {
        mesh.flip(r);
    }
    // The toe and the heel: the outline less the back cone's circle, flat in (θ cos φ, θ sin φ).
    for (sl, heel) in [(&slices[0], false), (&slices[SLICES], true)] {
        let mut outer: Vec<(f64, f64)> = Vec::new();
        for k in 0..z_teeth {
            for piece in outline(sl, k) {
                outer.extend(piece);
            }
        }
        let flat = |th: f64, ph: f64| [th * ph.cos(), th * ph.sin()];
        let outer_pts: Vec<Vec3> = outer.iter().map(|(t, p)| world(sl.s, *t, *p)).collect();
        let hole: Vec<Vec3> = back_row(sl);
        let mut planar: Vec<[f64; 2]> = outer.iter().map(|(t, p)| flat(*t, *p)).collect();
        planar.extend((0..round).map(|i| flat(sl.angles[0], TAU * i as f64 / round as f64)));
        // The toe faces the apex, the heel away from it.
        let out = |p: Vec3| {
            let q = sub(p, apex);
            if heel { q } else { scale(q, -1.0) }
        };
        if !mesh.cap(&outer_pts, &hole, &planar, out) {
            return None;
        }
    }
    mesh.finish();
    Some(mesh)
}

/// Points along a section's segment (its start and `n` more).
fn sample(seg: &super::Seg, n: usize) -> Vec<[f64; 2]> {
    use super::Seg;
    match seg {
        Seg::Line { a, b } => vec![*a, *b],
        Seg::Arc {
            center,
            radius,
            start,
            sweep,
        } => (0..=n)
            .map(|i| {
                let t = start + sweep * i as f64 / n as f64;
                [center[0] + radius * t.cos(), center[1] + radius * t.sin()]
            })
            .collect(),
        Seg::Circle { center, radius } => (0..=n)
            .map(|i| {
                let t = TAU * i as f64 / n as f64;
                [center[0] + radius * t.cos(), center[1] + radius * t.sin()]
            })
            .collect(),
        Seg::Spline { knots, poles } => {
            let (lo, hi) = (knots[3], knots[knots.len() - 4]);
            (0..=n)
                .map(|i| {
                    let u = lo + (hi - lo) * i as f64 / n as f64;
                    let (span, w) = super::basis(knots, poles.len(), u);
                    let mut p = [0.0; 2];
                    for (k, wk) in w.iter().enumerate() {
                        let q = poles[span - 3 + k];
                        p = [p[0] + wk * q[0], p[1] + wk * q[1]];
                    }
                    p
                })
                .collect()
        }
    }
}

/// A closed outline's points (each segment's start and its inner points: the next starts
/// where it ends).
fn outline_points(segs: &[super::Seg], n: usize) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    for s in segs {
        let mut pts = sample(s, n);
        pts.pop();
        out.extend(pts);
    }
    out
}

/// A prism-like body: the loop `ring(t)` (the same number of points each) for `t` from 0 to 1
/// stitched into a side, and capped at both ends less the loop `hole(t)` (none for an empty
/// one), each in the plane coordinates `flat` gives its points.
fn ringed(
    rings: &[Vec<Vec3>],
    holes: &[Vec<Vec3>],
    flat_ring: &[Vec<[f64; 2]>],
    flat_hole: &[Vec<[f64; 2]>],
    out_of: impl Fn(Vec3) -> Vec3,
    ends: [Vec3; 2],
) -> Option<Mesh> {
    let mut mesh = Mesh::default();
    let side = mesh.strip(rings, true);
    let t = side.start + (side.end - side.start) / 2;
    if dot(mesh.normal(t), out_of(mesh.middle(t))) < 0.0 {
        mesh.flip(side);
    }
    if !holes.is_empty() && !holes[0].is_empty() {
        let bore = mesh.strip(holes, true);
        let t = bore.start + (bore.end - bore.start) / 2;
        if dot(mesh.normal(t), out_of(mesh.middle(t))) > 0.0 {
            mesh.flip(bore);
        }
    }
    let last = rings.len() - 1;
    for (i, end) in [(0, ends[0]), (last, ends[1])] {
        let hole = holes.get(i).cloned().unwrap_or_default();
        let mut flat = flat_ring[i].clone();
        if let Some(h) = flat_hole.get(i) {
            flat.extend(h);
        }
        if !mesh.cap(&rings[i], &hole, &flat, |_| end) {
            return None;
        }
    }
    mesh.finish();
    Some(mesh)
}

/// A worm drive's member's preview, in the drive's frame (as [`super::worm`] has it): the worm
/// as it is (its thread's straight-sided axial section along its helix, sampled), its wheel as
/// a helical gear of the worm's lead angle, a space where the worm's thread is (near enough to
/// see the drive; its exact teeth are hobbed).
pub(super) fn worm_member(d: &super::worm::Design, wheel: bool, rotation: f64) -> Option<Mesh> {
    let m = d.module;
    let a = d.centre_distance();
    let r1 = d.quotient * m / 2.0;
    let pitch = PI * m;
    let lead = d.starts as f64 * pitch;
    let lead_angle = (d.starts as f64 / d.quotient).atan();
    let (root, tip) = (r1 - 1.25 * m, r1 + m);
    // The thread's middle at the angle φ round the worm (from −y, toward the wheel): where its
    // first start's middle is, as the exact worm's helix carries it.
    let length = d.length;
    let from = -length / 2.0 - 2.0 * lead;
    if wheel {
        let z2 = d.teeth as f64;
        let r2 = z2 * m / 2.0;
        let b = d.width;
        // A space under the worm's thread where it faces the wheel (φ 0, at the wheel's +y).
        // (The threads' middles where they face the wheel are at x ≡ `from` along the worm, a
        // pitch apart; the wheel's point at x on its pitch circle is at π/2 − x / r₂.)
        let at = from.rem_euclid(pitch);
        let space = PI / 2.0 - at / r2;
        let section = super::Section {
            flank: super::Flank::Involute {
                base: r2 * d.pressure.cos(),
                half: (PI * m / 2.0) / (2.0 * r2) + super::inv(d.pressure),
            },
            teeth: d.teeth,
            root: r2 - 1.25 * m,
            tip: r2 + m,
            fillet: 0.0,
            phase: space + PI / z2 + rotation,
        };
        let outline = outline_points(&section.outline(), 4);
        // The teeth's twist along the axis: the worm's lead angle, its hand.
        let twist = d.hand * lead_angle.tan() / r2;
        const STATIONS: usize = 12;
        let z_at = |k: usize| -b / 2.0 + b * k as f64 / STATIONS as f64;
        let turn = |p: [f64; 2], a: f64| {
            let (s, c) = a.sin_cos();
            [c * p[0] - s * p[1], s * p[0] + c * p[1]]
        };
        let flat: Vec<Vec<[f64; 2]>> = (0..=STATIONS)
            .map(|k| outline.iter().map(|p| turn(*p, twist * z_at(k))).collect())
            .collect();
        let rings: Vec<Vec<Vec3>> = flat
            .iter()
            .enumerate()
            .map(|(k, f)| f.iter().map(|p| [p[0], p[1], z_at(k)]).collect())
            .collect();
        let bore = d.wheel_bore / 2.0;
        let n_bore = outline.len().clamp(24, 128);
        let circle = |r: f64| -> Vec<[f64; 2]> {
            (0..n_bore)
                .map(|i| {
                    let t = TAU * i as f64 / n_bore as f64;
                    [r * t.cos(), r * t.sin()]
                })
                .collect()
        };
        let (holes, flat_holes): (Vec<Vec<Vec3>>, Vec<Vec<[f64; 2]>>) = if bore > 0.0 {
            (0..=STATIONS)
                .map(|k| {
                    let c = circle(bore);
                    (c.iter().map(|p| [p[0], p[1], z_at(k)]).collect(), c)
                })
                .unzip()
        } else {
            (Vec::new(), Vec::new())
        };
        return ringed(
            &rings,
            &holes,
            &flat,
            &flat_holes,
            |p| [p[0], p[1], 0.0],
            [[0.0, 0.0, -1.0], [0.0, 0.0, 1.0]],
        );
    }
    // The worm: its radius at x along its axis and φ round it.
    let px = pitch;
    let half_at =
        |r: f64| px / 4.0 - (r - r1) * d.pressure.tan() - d.backlash / (2.0 * lead_angle.cos());
    let radius = |x: f64, phi: f64| -> f64 {
        if x.abs() > length / 2.0 {
            return root;
        }
        let centre = from + d.hand * lead * phi / TAU;
        let u = (x - centre).rem_euclid(px);
        let off = u.min(px - u);
        if off <= half_at(tip) {
            tip
        } else if off >= half_at(root) {
            root
        } else {
            r1 + (px / 4.0 - d.backlash / (2.0 * lead_angle.cos()) - off) / d.pressure.tan()
        }
    };
    let core = length / 2.0 + m;
    let around = 96;
    let along = ((2.0 * core / px) * 24.0).ceil() as usize;
    let x_at = |k: usize| -core + 2.0 * core * k as f64 / along as f64;
    let phi_at = |j: usize| TAU * j as f64 / around as f64;
    let point = |x: f64, phi: f64, r: f64| [x, a - r * phi.cos(), -r * phi.sin()];
    let rings: Vec<Vec<Vec3>> = (0..=along)
        .map(|k| {
            let x = x_at(k);
            (0..around)
                .map(|j| point(x, phi_at(j), radius(x, phi_at(j))))
                .collect()
        })
        .collect();
    let flat: Vec<Vec<[f64; 2]>> = [0, along]
        .iter()
        .map(|&k| {
            let x = x_at(k);
            (0..around)
                .map(|j| {
                    let (r, p) = (radius(x, phi_at(j)), phi_at(j));
                    [r * p.cos(), r * p.sin()]
                })
                .collect()
        })
        .collect();
    let mut flat_all = vec![Vec::new(); along + 1];
    flat_all[0] = flat[0].clone();
    flat_all[along] = flat[1].clone();
    let bore = d.worm_bore / 2.0;
    let (holes, flat_holes): (Vec<Vec<Vec3>>, Vec<Vec<[f64; 2]>>) = if bore > 0.0 {
        (0..=along)
            .map(|k| {
                let ring: Vec<Vec3> = (0..around)
                    .map(|j| point(x_at(k), phi_at(j), bore))
                    .collect();
                let flat: Vec<[f64; 2]> = (0..around)
                    .map(|j| [bore * phi_at(j).cos(), bore * phi_at(j).sin()])
                    .collect();
                (ring, flat)
            })
            .unzip()
    } else {
        (Vec::new(), Vec::new())
    };
    ringed(
        &rings,
        &holes,
        &flat_all,
        &flat_holes,
        |p| [0.0, p[1] - a, p[2]],
        [[-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gear::hypoid::{Design, layout};

    fn volume(m: &Mesh) -> f64 {
        m.indices
            .chunks_exact(3)
            .map(|t| {
                let (a, b, c) = (
                    m.positions[t[0] as usize],
                    m.positions[t[1] as usize],
                    m.positions[t[2] as usize],
                );
                dot(a, cross(b, c))
            })
            .sum::<f64>()
            / 6.0
    }

    /// `hypoid.sv`'s pair previewed in well under a millisecond a member, closed, as much
    /// material as the exact members with their crown's sharp tips (22223 and 14634 mm³, with
    /// their end relief): within a percent or so, its spaces' fillets and the reliefs left out.
    #[test]
    #[ignore = "debugging: PREVIEW_DEBUG=1 prints where the flanks are followed"]
    fn a_bevel_pairs_preview() {
        let d = Design {
            pinion_teeth: 24.0,
            gear_teeth: 48.0,
            module: 2.0,
            shaft_angle: PI / 2.0,
            offset: 0.0,
            spiral: 25f64.to_radians(),
            cutter_radius: 75.6122,
            pressure: 20f64.to_radians(),
            pressure_shift: 0.0,
        };
        let l = layout(&d).unwrap();
        for gear in [true, false] {
            eprintln!(
                "{gear}: {:?}",
                hypoid_member(&d, &l, gear, 0.0).map(|m| volume(&m))
            );
        }
    }

    /// Whether `p` is inside the closed mesh `m` (a ray along +z, its crossings counted).
    fn inside(m: &Mesh, p: Vec3) -> bool {
        let mut n = 0;
        for t in m.indices.chunks_exact(3) {
            let [a, b, c] = [0, 1, 2].map(|k| m.positions[t[k] as usize]);
            let d =
                |u: Vec3, v: Vec3| (u[0] - p[0]) * (v[1] - p[1]) - (u[1] - p[1]) * (v[0] - p[0]);
            let (x, y, z) = (d(a, b), d(b, c), d(c, a));
            if !((x > 0.0 && y > 0.0 && z > 0.0) || (x < 0.0 && y < 0.0 && z < 0.0)) {
                continue;
            }
            let w = x + y + z;
            let h = (y * a[2] + z * b[2] + x * c[2]) / w;
            if h > p[2] {
                n += 1;
            }
        }
        n % 2 == 1
    }

    #[test]
    #[ignore = "debugging: where a pair's previews clash"]
    fn where_a_pairs_previews_clash() {
        let ratio: f64 = std::env::var("RATIO")
            .ok()
            .and_then(|r| r.parse().ok())
            .unwrap_or(2.0);
        let offset: f64 = std::env::var("OFFSET")
            .ok()
            .and_then(|r| r.parse().ok())
            .unwrap_or(20.0);
        let g = crate::gear::GearFeature {
            gear_type: crate::gear::GearType::Pair,
            module: 2.0,
            ratio,
            helix_angle: 25.0,
            style: crate::gear::ToothStyle::Helical,
            shaft_angle: 90.0,
            offset,
            backlash: 0.05,
            ..Default::default()
        };
        let shaft: f64 = std::env::var("SHAFT")
            .ok()
            .and_then(|r| r.parse().ok())
            .unwrap_or(90.0);
        let g = crate::gear::GearFeature {
            shaft_angle: shaft,
            ..g
        };
        let (_, mut members) = crate::gear::pair_members(&g, shaft, offset);
        if std::env::var_os("SV").is_some() {
            members = crate::samples::gears::hypoid_pair().map(|mut x| {
                (x.shaft_angle, x.offset) = (shaft, offset);
                x
            });
        }
        let made: Vec<(Mesh, Layout, bool)> = members
            .iter()
            .map(|x| {
                let (d, gear) = x.hypoid_design();
                let l = layout(&d).unwrap();
                (hypoid_member(&d, &l, gear, 0.05 / 4.0).unwrap(), l, gear)
            })
            .collect();
        for (a, b) in [(0, 1), (1, 0)] {
            let (m, l, gear) = &made[b];
            let (apex, axis) = if *gear {
                ([0.0; 3], l.gear_axis)
            } else {
                (l.pinion_apex, l.pinion_axis)
            };
            let mut found = 0;
            for (i, p) in m.positions.iter().enumerate() {
                if !inside(&made[a].0, [p[0] + 1.3e-7, p[1] + 2.1e-7, p[2]]) {
                    continue;
                }
                // (How far in: to the other's nearest point.)
                let deep = made[a]
                    .0
                    .positions
                    .iter()
                    .map(|q| norm(sub(*q, *p)))
                    .fold(f64::MAX, f64::min);
                if deep < 0.2 {
                    continue;
                }
                found += 1;
                if found > 12 {
                    continue;
                }
                let q = sub(*p, apex);
                let s = norm(q);
                let th = (dot(q, axis) / s).acos();
                eprintln!(
                    "{} vertex {i} in the {}: {deep:.3} deep; s {s:.3} (cone {:.3}, face {:.3}), θ {:.4} (pitch {:.4})",
                    if *gear { "gear's" } else { "pinion's" },
                    if *gear { "pinion" } else { "gear" },
                    if *gear { l.mean_cone } else { l.pinion_cone },
                    0.2 * l.mean_cone,
                    th,
                    if *gear { l.gear_pitch } else { l.pinion_pitch },
                );
            }
            eprintln!("{found} vertices over 0.2 mm in");
        }
    }

    #[test]
    fn the_hypoid_pairs_preview_is_fast_and_close() {
        let d = Design {
            pinion_teeth: 24.0,
            gear_teeth: 48.0,
            module: 2.0,
            shaft_angle: PI / 2.0,
            offset: 25.0,
            spiral: 25f64.to_radians(),
            cutter_radius: 75.6122,
            pressure: 20f64.to_radians(),
            pressure_shift: 8.854_877_139_886_24_f64.to_radians(),
        };
        let l = layout(&d).unwrap();
        for (gear, exact) in [(true, 22223.5), (false, 14634.0)] {
            let t = std::time::Instant::now();
            let m = hypoid_member(&d, &l, gear, 0.05 / 4.0).expect("a preview");
            let took = t.elapsed();
            let v = volume(&m);
            eprintln!(
                "{}: {} points, {} triangles, {took:?}, volume {v:.1} (exact {exact})",
                if gear { "gear" } else { "pinion" },
                m.positions.len(),
                m.indices.len() / 3
            );
            assert!((v - exact).abs() < 0.02 * exact, "volume {v} vs {exact}");
        }
    }
}
