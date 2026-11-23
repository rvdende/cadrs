//! The set-back corner between three curvature-continuous fillets, as one smooth surface
//! ([`Kernel::fillet_setback`] with [`FilletProfile::Thumbweight`]).
//!
//! After springrep's `blend::shaped::corner` (MIT). In the corner's own axes, the depths
//! `u₀, u₁, u₂` in from its faces, a fillet's section is `(u_i, u_k) = d·(β(t), α(t))`, `α` and
//! `β` the weights of its legs ([`super::gn_blend`]). Let `φ(d·β(t)) = α/(α + β)`: 1 on a face,
//! falling to 0 at `d` from it, and 0 beyond. A face is where `φ(u_i) = 1` and the fillet between
//! two where `φ(u_i) + φ(u_k) = 1`, so the corner whose three fillets run on into each other is
//! the solid `φ(u₀) + φ(u₁) + φ(u₂) ≤ 1`. Set back, the corner is
//!
//! `φ(u₀) + φ(u₁) + φ(u₂) + B(u₀)·B(u₁)·B(u₂) ≤ 1`, `B(u) = (1 − u/s)⁴` within `s` and 0 beyond.
//!
//! Past the setback `s` along any edge that is the fillets and the faces exactly. Within it the
//! boundary is one surface of continuous curvature, since `φ` and `B` both reach 0 with three
//! vanishing derivatives, and it meets each fillet's end section with the fillet's curvature. The
//! added term takes material: over a face the boundary dips `d·B·B` beneath it at first order, so
//! the flat stops at the setback and not where the fillets' contact lines would cross.
//!
//! The surface is nine faces, each smooth in parameters of its own, cut where `φ` or `B` changes
//! form: a dip over each face, where both other depths are past `d`, a graph over them; a strip
//! along each fillet, where the distance along its edge is past `d`, each section the points on
//! rays from `(d, d)` toward the fillet's section, by the section's parameter; and the core,
//! where all three are within `d`, in thirds by which is least, on rays from `(d, d, d)`. Each is
//! a quintic B-spline through the surface at the Greville sites of even knots. Strips and thirds
//! are rational over the section's denominator, so a strip's outer side is the fillet's section
//! itself.
//!
//! [`Kernel::fillet_setback`]: crate::Kernel::fillet_setback

use std::sync::Arc;

use nalgebra::DMatrix;

use super::blend::{FaceSrc, Tracked};
use super::curve::Curve;
use super::math::*;
use super::nurbs::{H, NurbsCurve, NurbsSurface, basis_ders, find_span};
use super::surface::Surface;
use super::topo::{Body, Coedge, Face};
use crate::*;

const DEGREE: usize = 5;

/// Spans along a strip's section, and half as many along each side of a third: the fit then
/// holds the surface within a hundred-millionth of the radius.
const SECTION_SPANS: usize = 48;

/// The corner's solid in the depths from its faces: fillets of contact distance `d` and
/// thumbweight `w`, set back `s`.
struct Blend {
    d: f64,
    s: f64,
    w: f64,
}

/// The root of `f` between `lo`, where it is negative, and `hi`, where it is positive, by false
/// position with the retained end's value halved when it stalls (Illinois).
fn root(f: impl Fn(f64) -> f64, mut lo: f64, mut hi: f64) -> f64 {
    let (mut f_lo, mut f_hi) = (f(lo), f(hi));
    if f_lo >= 0.0 {
        return lo;
    }
    if f_hi <= 0.0 {
        return hi;
    }
    let mut last = 0;
    for _ in 0..200 {
        let x = (lo * f_hi - hi * f_lo) / (f_hi - f_lo);
        let fx = f(x);
        if fx == 0.0 {
            return x;
        }
        if fx < 0.0 {
            (lo, f_lo) = (x, fx);
            if last == -1 {
                f_hi *= 0.5;
            }
            last = -1;
        } else {
            (hi, f_hi) = (x, fx);
            if last == 1 {
                f_lo *= 0.5;
            }
            last = 1;
        }
        if hi - lo <= 4.0 * f64::EPSILON * hi.abs().max(lo.abs()) {
            break;
        }
    }
    0.5 * (lo + hi)
}

impl Blend {
    /// The section's leg weights `(α, β)` at `t`.
    fn legs(&self, t: f64) -> [f64; 2] {
        super::gn_blend::legs_at(self.w, t)
    }

    /// The section's denominator at `t`, the weight its rational form carries.
    fn den(&self, t: f64) -> f64 {
        let s = 1.0 - t;
        (s.powi(3) + t.powi(3)) * (1.0 - self.w + self.w * t) * (1.0 - self.w * t)
    }

    fn phi(&self, u: f64) -> f64 {
        if u >= self.d {
            return 0.0;
        }
        if u <= 0.0 {
            return 1.0;
        }
        // The section's parameter where its far leg's weight is u / d.
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            if self.d * self.legs(mid)[1] < u {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let [a, b] = self.legs(0.5 * (lo + hi));
        a / (a + b)
    }

    fn bump(&self, u: f64) -> f64 {
        (1.0 - u / self.s).max(0.0).powi(4)
    }

    /// Negative inside the solid, positive outside, falling as any depth grows.
    fn g(&self, u: [f64; 3]) -> f64 {
        u.iter().map(|&x| self.phi(x)).sum::<f64>()
            + u.iter().map(|&x| self.bump(x)).product::<f64>()
            - 1.0
    }

    /// Where the ray from `from`, inside the solid, against `dir` leaves it, within `reach`.
    fn on_ray(&self, from: [f64; 3], dir: [f64; 3], reach: f64) -> [f64; 3] {
        let at = |r: f64| [0, 1, 2].map(|i| from[i] - r * dir[i]);
        at(root(|r| self.g(at(r)), 0.0, reach))
    }

    /// The dip over a face at `(x, y)` from the other two, both past `d`: `(x, y, depth)`.
    fn dip(&self, x: f64, y: f64) -> [f64; 3] {
        if self.bump(x) * self.bump(y) == 0.0 {
            return [x, y, 0.0];
        }
        self.on_ray([x, y, self.d], [0.0, 0.0, 1.0], self.d)
    }

    /// The strip along an edge at the section's parameter `t` and `y` along the edge, past `d`:
    /// the depths from the edge's two faces, then `y`.
    fn strip(&self, t: f64, y: f64) -> [f64; 3] {
        let [a, b] = self.legs(t);
        if self.bump(y) == 0.0 {
            return [self.d * b, self.d * a, y];
        }
        let dir = [1.0 - b, 1.0 - a, 0.0];
        self.on_ray([self.d, self.d, y], dir, self.d / dir[0].max(dir[1]))
    }

    /// The third of the core nearest a face, at the section's parameters `p` and `q` in
    /// `[½, 1]`: the depths from the other two faces, then from its own, the least.
    fn third(&self, p: f64, q: f64) -> [f64; 3] {
        let cot = |t: f64| {
            let [a, b] = self.legs(t);
            (1.0 - b) / (1.0 - a)
        };
        self.on_ray([self.d; 3], [cot(p), cot(q), 1.0], self.d)
    }
}

/// A B-spline surface in a piece's own axes.
struct Fit {
    knots: [Vec<f64>; 2],
    count: [usize; 2],
    /// `i·count[1] + j` is the `i`-th along the first parameter and the `j`-th along the second.
    ctrl: Vec<[f64; 3]>,
    weights: Vec<f64>,
}

type Weight<'a> = Option<&'a dyn Fn(f64) -> f64>;

fn even_knots((lo, hi): (f64, f64), spans: usize) -> Vec<f64> {
    let mut k = vec![lo; DEGREE + 1];
    k.extend((1..spans).map(|i| lo + (hi - lo) * i as f64 / spans as f64));
    k.extend([hi; DEGREE + 1]);
    k
}

/// The Greville sites of clamped `knots`, their ends the domain's exactly.
fn sites(knots: &[f64]) -> Vec<f64> {
    let n = knots.len() - DEGREE - 1;
    let mut at: Vec<f64> = (0..n)
        .map(|i| knots[i + 1..=i + DEGREE].iter().sum::<f64>() / DEGREE as f64)
        .collect();
    (at[0], at[n - 1]) = (knots[0], knots[n]);
    at
}

/// The collocation matrix of the B-splines on `knots` at `at`.
fn collocation(knots: &[f64], at: &[f64]) -> DMatrix<f64> {
    let n = knots.len() - DEGREE - 1;
    let mut m = DMatrix::zeros(at.len(), n);
    for (r, &t) in at.iter().enumerate() {
        let span = find_span(n - 1, DEGREE, t, knots);
        let b = basis_ders(span, t, DEGREE, 0, knots);
        for (j, v) in b[0].iter().enumerate() {
            m[(r, span - DEGREE + j)] = *v;
        }
    }
    m
}

fn solve(m: &DMatrix<f64>, rhs: DMatrix<f64>) -> Result<DMatrix<f64>> {
    m.clone()
        .lu()
        .solve(&rhs)
        .ok_or_else(|| KernelError::InvalidGeometry {
            op: "set-back corner",
            why: "the corner's surface fit is singular".into(),
        })
}

/// The quintic B-spline through `f` at the Greville sites of even knots, `spans` each way over
/// `range`; with weights, the rational one whose weight is their product, each a polynomial of
/// degree five at most (so held exactly by the spline).
fn fit(
    f: &(dyn Fn(f64, f64) -> [f64; 3] + Sync),
    weight: [Weight; 2],
    range: [(f64, f64); 2],
    spans: [usize; 2],
) -> Result<Fit> {
    let knots = [0, 1].map(|k| even_knots(range[k], spans[k]));
    let at = [sites(&knots[0]), sites(&knots[1])];
    let (nu, nv) = (at[0].len(), at[1].len());
    let mats = [
        collocation(&knots[0], &at[0]),
        collocation(&knots[1], &at[1]),
    ];
    let mut seen = [vec![1.0; nu], vec![1.0; nv]];
    let mut held = seen.clone();
    for k in 0..2 {
        if let Some(g) = weight[k] {
            seen[k] = at[k].iter().map(|&t| g(t)).collect();
            let rhs = DMatrix::from_column_slice(at[k].len(), 1, &seen[k]);
            held[k] = solve(&mats[k], rhs)?.column(0).iter().copied().collect();
        }
    }
    // Values (weighted) at the sites, rows along the first parameter, one column per (j, c).
    // (Each site a root of the corner's equation, found on its own: side by side, the same
    // values whatever the threads.)
    let sites: Vec<usize> = (0..nu * nv).collect();
    let pts: Vec<[f64; 3]> = super::par::map(&sites, |&k| f(at[0][k / nv], at[1][k % nv]));
    let mut vals = DMatrix::zeros(nu, nv * 3);
    for i in 0..nu {
        for j in 0..nv {
            let p = pts[i * nv + j];
            for c in 0..3 {
                vals[(i, j * 3 + c)] = seen[0][i] * seen[1][j] * p[c];
            }
        }
    }
    // Through the first parameter, then the second.
    let along_u = solve(&mats[0], vals)?;
    let mut t = DMatrix::zeros(nv, nu * 3);
    for i in 0..nu {
        for j in 0..nv {
            for c in 0..3 {
                t[(j, i * 3 + c)] = along_u[(i, j * 3 + c)];
            }
        }
    }
    let both = solve(&mats[1], t)?;
    let mut ctrl = Vec::with_capacity(nu * nv);
    let mut weights = Vec::with_capacity(nu * nv);
    for i in 0..nu {
        for j in 0..nv {
            let w = held[0][i] * held[1][j];
            ctrl.push([0, 1, 2].map(|c| both[(j, i * 3 + c)] / w));
            weights.push(w);
        }
    }
    Ok(Fit {
        knots,
        count: [nu, nv],
        ctrl,
        weights,
    })
}

impl Fit {
    fn surface(&self, place: &dyn Fn([f64; 3]) -> P3) -> Surface {
        let cps = (0..self.count[0])
            .map(|i| {
                (0..self.count[1])
                    .map(|j| {
                        let k = i * self.count[1] + j;
                        let p = place(self.ctrl[k]);
                        let w = self.weights[k];
                        H::new(p.x * w, p.y * w, p.z * w, w)
                    })
                    .collect()
            })
            .collect();
        Surface::Nurbs(Arc::new(NurbsSurface {
            du: DEGREE,
            dv: DEGREE,
            ku: self.knots[0].clone(),
            kv: self.knots[1].clone(),
            cps,
        }))
    }

    /// The side where parameter `across` is at its start, or its end when `last`, running along
    /// the other parameter.
    fn side(&self, across: usize, last: bool, place: &dyn Fn([f64; 3]) -> P3) -> Curve {
        let along = 1 - across;
        let fixed = if last { self.count[across] - 1 } else { 0 };
        let index = |k: usize| {
            if across == 0 {
                fixed * self.count[1] + k
            } else {
                k * self.count[1] + fixed
            }
        };
        let cps = (0..self.count[along])
            .map(|k| {
                let p = place(self.ctrl[index(k)]);
                let w = self.weights[index(k)];
                H::new(p.x * w, p.y * w, p.z * w, w)
            })
            .collect();
        Curve::Nurbs(Arc::new(NurbsCurve {
            degree: DEGREE,
            knots: self.knots[along].clone(),
            cps,
        }))
    }
}

fn gone<T>() -> Result<T> {
    Err(KernelError::Unsupported(
        "a set-back corner whose fillets do not end clear of each other at its setback",
    ))
}

/// Closes one corner of `t`, the fillets of contact distance `d` and thumbweight `w` along the
/// right-handed directions `a` from the corner `at` already cut back by the cube of side `s`
/// whose faces in the body carry `mark`: the cube's faces are replaced by the nine faces of the
/// corner's surface (see the module docs).
pub(crate) fn close_corner(
    t: Tracked,
    mark: &FaceSrc,
    at: P3,
    a: [V3; 3],
    d: f64,
    s: f64,
    w: f64,
) -> Result<Tracked> {
    let Tracked { mut body, mut src } = t;
    let notch: Vec<usize> = (0..body.faces.len())
        .filter(|&f| src[f].contains(mark))
        .collect();
    if notch.len() != 3 {
        return gone();
    }
    let place = |m: usize| {
        move |l: [f64; 3]| at + a[(m + 1) % 3] * l[0] + a[(m + 2) % 3] * l[1] + a[m] * l[2]
    };
    let near = 1e-6 * (1.0 + s);
    let found = |b: &Body, p: P3| {
        (0..b.verts.len())
            .find(|&v| (b.verts[v].p - p).norm() <= near)
            .map_or_else(gone, Ok)
    };
    let between = |b: &Body, v: usize, u: usize| {
        (0..b.edges.len())
            .find(|&e| {
                let x = b.edges[e].v;
                (x[0] == v && x[1] == u) || (x[0] == u && x[1] == v)
            })
            .map_or_else(gone, Ok)
    };
    let mut touch = [[0; 2]; 3];
    let mut out = [0; 3];
    let mut section = [0; 3];
    for j in 0..3 {
        touch[j] = [
            found(&body, place(j)([0.0, d, s]))?,
            found(&body, place(j)([d, 0.0, s]))?,
        ];
        out[j] = found(&body, place(j)([s, s, 0.0]))?;
        section[j] = between(&body, touch[j][0], touch[j][1])?;
    }
    let mut lines = [[0; 2]; 3];
    for k in 0..3 {
        lines[k] = [
            between(&body, touch[(k + 1) % 3][1], out[k])?,
            between(&body, touch[(k + 2) % 3][0], out[k])?,
        ];
    }

    let blend = Blend { d, s, w };
    let along = ((6.0 * (s - d) / d).ceil() as usize).clamp(8, 64);
    let den = |t: f64| blend.den(t);
    let dip = fit(
        &|x, y| blend.dip(x, y),
        [None, None],
        [(d, s), (d, s)],
        [along, along],
    )?;
    let strip = fit(
        &|t, y| blend.strip(t, y),
        [Some(&den), None],
        [(0.0, 1.0), (d, s)],
        [SECTION_SPANS, along],
    )?;
    let third = fit(
        &|p, q| blend.third(p, q),
        [Some(&den), Some(&den)],
        [(0.5, 1.0), (0.5, 1.0)],
        [SECTION_SPANS / 2, SECTION_SPANS / 2],
    )?;

    // The core's middle, the middle of each strip's inner side, and each dip's inner corner.
    let centre = body.add_vertex(place(0)(blend.third(0.5, 0.5)));
    let mid = [0, 1, 2].map(|j| body.add_vertex(place(j)(blend.strip(0.5, d))));
    let low = [0, 1, 2].map(|k| body.add_vertex(place(k)(blend.dip(d, d))));
    // Each strip's sides beside the dips, the halves of its inner side, and the seam between two
    // thirds that ends at its middle.
    let (mut beside, mut inner, mut seam) = ([[0; 2]; 3], [[0; 2]; 3], [0; 3]);
    for j in 0..3 {
        let (next, prev) = ((j + 1) % 3, (j + 2) % 3);
        let pj = place(j);
        beside[j] = [
            body.add_edge(strip.side(0, false, &pj), d, s, low[next], touch[j][0]),
            body.add_edge(strip.side(0, true, &pj), d, s, low[prev], touch[j][1]),
        ];
        let c = strip.side(1, false, &pj);
        inner[j] = [
            body.add_edge(c.clone(), 0.0, 0.5, low[next], mid[j]),
            body.add_edge(c, 0.5, 1.0, mid[j], low[prev]),
        ];
        seam[j] = body.add_edge(third.side(1, false, &place(prev)), 0.5, 1.0, centre, mid[j]);
    }
    let outward = -(a[0] + a[1] + a[2]);
    let mut faces: Vec<Face> = Vec::new();
    let face = |body: &Body, surface: Surface, sides: &[(usize, usize)]| -> Result<Face> {
        // The loop as given, then turned to run counter-clockwise about the outward normal.
        let mut uses: Vec<(usize, bool)> = sides
            .iter()
            .map(|&(e, from)| (e, body.edges[e].v[0] == from))
            .collect();
        let mut area = V3::zeros();
        let mut pts = Vec::new();
        for &(e, fwd) in &uses {
            let ed = &body.edges[e];
            for i in 0..8 {
                let f = i as f64 / 8.0;
                let t = if fwd {
                    ed.t[0] + (ed.t[1] - ed.t[0]) * f
                } else {
                    ed.t[1] + (ed.t[0] - ed.t[1]) * f
                };
                pts.push(ed.point(t));
            }
        }
        for i in 0..pts.len() {
            area += pts[i].coords.cross(&pts[(i + 1) % pts.len()].coords);
        }
        if area.dot(&outward) < 0.0 {
            uses.reverse();
            for u in &mut uses {
                u.1 = !u.1;
            }
        }
        let mut lp = Vec::new();
        for (e, fwd) in uses {
            let ed = &body.edges[e];
            let pc = super::pcurve::pcurve(&surface, &ed.curve, ed.t[0], ed.t[1], None);
            lp.push(Coedge { edge: e, fwd, pc });
        }
        Ok(Face {
            surf: surface,
            sense: true,
            loops: vec![lp],
        })
    };
    for k in 0..3 {
        let (a_, b_) = ((k + 1) % 3, (k + 2) % 3);
        let pk = place(k);
        faces.push(face(
            &body,
            dip.surface(&pk),
            &[
                (beside[a_][1], low[k]),
                (lines[k][0], touch[a_][1]),
                (lines[k][1], out[k]),
                (beside[b_][0], touch[b_][0]),
            ],
        )?);
        faces.push(face(
            &body,
            strip.surface(&pk),
            &[
                (inner[k][0], low[a_]),
                (inner[k][1], mid[k]),
                (beside[k][1], low[b_]),
                (section[k], touch[k][1]),
                (beside[k][0], touch[k][0]),
            ],
        )?);
        faces.push(face(
            &body,
            third.surface(&pk),
            &[
                (seam[a_], centre),
                (inner[a_][1], mid[a_]),
                (inner[b_][0], low[k]),
                (seam[b_], mid[b_]),
            ],
        )?);
    }
    // The cube's faces go; the corner's come in their place.
    let keep: Vec<usize> = (0..body.faces.len())
        .filter(|f| !notch.contains(f))
        .collect();
    let mut all: Vec<Face> = keep.iter().map(|&f| body.faces[f].clone()).collect();
    src = keep.iter().map(|&f| src[f].clone()).collect();
    let first = all.len();
    for f in faces {
        all.push(f);
        src.push(vec![mark.clone()]);
    }
    body.faces = all;
    let n = body.faces.len();
    super::build::fix_senses(&mut body, first..n);
    // The fits hold the corner's surface to about a hundred-millionth of the radius: the edges
    // between its faces, and those it shares with the fillets, are that loose.
    let mut worst = vec![0.0f64; body.edges.len()];
    for f in &body.faces[first..n] {
        for c in f.loops.iter().flatten() {
            let ed = &body.edges[c.edge];
            for i in 0..=16 {
                let t = ed.t[0] + (ed.t[1] - ed.t[0]) * i as f64 / 16.0;
                let uv = c.pc.point(t);
                let off = (f.surf.eval(uv.x, uv.y) - ed.point(t)).norm();
                worst[c.edge] = worst[c.edge].max(off);
            }
        }
    }
    for (e, off) in worst.into_iter().enumerate() {
        if off > body.edges[e].tol {
            body.edges[e].tol = 2.0 * off;
            for v in body.edges[e].v {
                body.verts[v].tol = body.verts[v].tol.max(2.0 * off);
            }
        }
    }
    super::unify::compact(&mut body);
    body.shells.clear();
    body.solids.clear();
    super::boolean::assemble_shells(&mut body)?;
    if body.solids.len() != 1 {
        return Err(KernelError::InvalidResult {
            op: "set-back corner",
            why: "the corner's surface leaves the body open".into(),
        });
    }
    Ok(Tracked { body, src })
}
