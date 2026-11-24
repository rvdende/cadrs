//! A solid patterned round an axis from one sector of it: a gear from one tooth's sector, a
//! dome from one pass's wedge. The sector is the solid between two half planes through the axis
//! a pitch apart (its cut faces lie on them); its copies, each turned a pitch on, are set side by
//! side and joined along the cut faces by index, with no boolean and no search: copy k's
//! leading cut face is copy k + 1's trailing one turned, so each edge and vertex of the one is
//! one of the other's (matched once, on the sector). A face on one side of a cut that goes on
//! on the other (a blank's cone, cut into a sector of it) is one face again: its pieces' loops
//! joined where the cut was, their p-curves taken to one piece's surface by the affine map of
//! (u, v) the turn makes (a shift round a cone, a turn in a plane; none on a copy's own
//! B-spline).

use super::curve::Curve;
use super::math::*;
use super::pcurve;
use super::surface::Surface;
use super::topo::{Body, Coedge, Face, Shell, Solid};

/// The turn by `angle` about the axis through `o` along the unit `z`.
pub fn turn(o: P3, z: V3, angle: f64) -> Affine {
    let r = nalgebra::Rotation3::from_axis_angle(&nalgebra::Unit::new_normalize(z), angle);
    let m = *r.matrix();
    Affine {
        m,
        t: o.coords - m * o.coords,
    }
}

fn apply(m: &Affine, p: &P3) -> P3 {
    P3::from(m.m * p.coords + m.t)
}

/// The solid of `n` copies of `sector`, the k-th turned `k · pitch` about the axis (`o`, `z`)
/// (`n · pitch` a whole turn), joined along their cut faces; None where the sector isn't one
/// (no cut faces on two half planes through the axis a pitch apart, or their edges don't match
/// within `tol`).
pub fn pattern_sector(sector: &Body, o: P3, z: V3, n: usize, pitch: f64, tol: f64) -> Option<Body> {
    pattern_faces(sector, o, z, n, pitch, tol).map(|p| p.body)
}

/// A patterned sector ([`pattern_faces`]).
pub struct Patterned {
    pub body: Body,
    /// Each face's sector face (the one its first piece is a copy of).
    pub from: Vec<Option<usize>>,
    /// Each face that is a copy of another, a whole piece of the sector turned on: that face
    /// (the first copy's) and the turn that takes it there.
    pub copies: super::copies::FaceCopies,
}

/// [`pattern_sector`], with what each face is of the sector's.
pub fn pattern_faces(
    sector: &Body,
    o: P3,
    z: V3,
    n: usize,
    pitch: f64,
    tol: f64,
) -> Option<Patterned> {
    match joined(sector, o, z, n, pitch, tol) {
        Ok(b) => Some(b),
        Err(why) => {
            if crate::native::env_flag!("CADRS_PATTERN_DEBUG") {
                eprintln!("pattern: not joined: {why}");
            }
            None
        }
    }
}

/// An affine map of (u, v): `m` and `c`.
#[derive(Clone, Copy)]
struct UvMap {
    m: nalgebra::Matrix2<f64>,
    c: V2,
}

impl UvMap {
    fn identity() -> Self {
        UvMap {
            m: nalgebra::Matrix2::identity(),
            c: V2::zeros(),
        }
    }

    fn curve(&self, pc: &Curve) -> Curve {
        let m = Affine {
            m: nalgebra::Matrix3::new(
                self.m[(0, 0)],
                self.m[(0, 1)],
                0.0,
                self.m[(1, 0)],
                self.m[(1, 1)],
                0.0,
                0.0,
                0.0,
                1.0,
            ),
            t: V3::new(self.c.x, self.c.y, 0.0),
        };
        pc.transformed(&m)
    }
}

/// The map taking (u, v) on `from` to (u, v) on `to` at the same point, where `to` is `from`
/// moved by a motion that keeps it (a cone turned about its axis): from three points near
/// `at`, checked at `check`. None where no affine map does.
fn uv_map(from: &Surface, to: &Surface, at: P2, check: &[P2], tol: f64) -> Option<UvMap> {
    let periods = pcurve::closed_periods(to);
    let h = 1e-3;
    let pts = [at, P2::new(at.x + h, at.y), P2::new(at.x, at.y + h)];
    let mut img = [P2::origin(); 3];
    for (i, q) in pts.iter().enumerate() {
        let p = from.eval(q.x, q.y);
        let (u, v) = to.project(&p, (i > 0).then(|| (img[0].x, img[0].y)));
        let mut w = P2::new(u, v);
        // (Near the first image across a seam of the periods.)
        if i > 0 {
            for d in 0..2 {
                if let Some(per) = periods[d] {
                    w[d] -= ((w[d] - img[0][d]) / per).round() * per;
                }
            }
        }
        img[i] = w;
    }
    let du = (img[1] - img[0]) / h;
    let dv = (img[2] - img[0]) / h;
    let m = nalgebra::Matrix2::new(du.x, dv.x, du.y, dv.y);
    // (The derivatives to rounding: an exact map is linear, so they are the map's own; round
    // its entries that are 0 or ±1 within the steps' error.)
    let m = m.map(|x| {
        for t in [-1.0, 0.0, 1.0] {
            if (x - t).abs() < 1e-6 {
                return t;
            }
        }
        x
    });
    let c = img[0].coords - m * at.coords;
    let map = UvMap { m, c };
    for q in check {
        let w = map.m * q.coords + map.c;
        if (to.eval(w.x, w.y) - from.eval(q.x, q.y)).norm() > tol {
            return None;
        }
    }
    Some(map)
}

/// A face's (u, v) at points along its loops.
fn boundary_uvs(b: &Body, f: &Face, per: usize) -> Vec<P2> {
    let mut out = Vec::new();
    for c in f.loops.iter().flatten() {
        let e = &b.edges[c.edge];
        for k in 0..per {
            let t = e.t[0] + (e.t[1] - e.t[0]) * (k as f64 + 0.5) / per as f64;
            let q = c.pc.point(t);
            out.push(P2::new(q.x, q.y));
        }
    }
    out
}

fn joined(
    sector: &Body,
    o: P3,
    z: V3,
    n: usize,
    pitch: f64,
    tol: f64,
) -> Result<Patterned, String> {
    let z = z.normalize();
    let r1 = turn(o, z, pitch);
    let normal = |f: &Face| -> Option<V3> {
        let Surface::Plane { f: fr } = &f.surf else {
            return None;
        };
        let nrm = fr.x.cross(&fr.y).normalize();
        (nrm.dot(&z).abs() < 1e-9 && (fr.o - o).dot(&nrm).abs() <= tol).then_some(nrm)
    };
    // The cut faces: trailing (1: the pitch's turn takes its plane onto another's) and leading
    // (2).
    let normals: Vec<Option<V3>> = sector.faces.iter().map(normal).collect();
    let mut side = vec![0u8; sector.faces.len()];
    for (i, ni) in normals.iter().enumerate() {
        let Some(ni) = ni else { continue };
        let turned = r1.m * ni;
        let leads = normals
            .iter()
            .flatten()
            .any(|nj| turned.cross(nj).norm() < 1e-9);
        side[i] = if leads { 1 } else { 2 };
    }
    if !side.contains(&1) || !side.contains(&2) {
        return Err("no cut faces on two half planes a pitch apart".into());
    }
    let ef = sector.edge_faces();
    // (An edge between the two cut faces, on the axis, is neither's: it goes with them.)
    let edge_side: Vec<u8> = (0..sector.edges.len())
        .map(|e| {
            let sides: Vec<u8> = ef[e].iter().map(|&f| side[f]).collect();
            if sides.contains(&1) && sides.contains(&2) {
                3
            } else {
                sides.into_iter().max().unwrap_or(0)
            }
        })
        .collect();
    // An edge's face that stays (not a cut face).
    let kept_face = |e: usize| ef[e].iter().copied().find(|&f| side[f] == 0);
    let a_edges: Vec<usize> = (0..sector.edges.len())
        .filter(|&e| edge_side[e] == 1)
        .collect();
    let b_edges: Vec<usize> = (0..sector.edges.len())
        .filter(|&e| edge_side[e] == 2)
        .collect();
    if a_edges.len() != b_edges.len() {
        return Err(format!(
            "{} trailing edges, {} leading",
            a_edges.len(),
            b_edges.len()
        ));
    }
    let ends = |e: usize| {
        let ed = &sector.edges[e];
        (
            sector.verts[ed.v[0]].p,
            sector.verts[ed.v[1]].p,
            ed.point(0.5 * (ed.t[0] + ed.t[1])),
        )
    };
    let near = |p: &P3, q: &P3| (p - q).norm() <= tol;
    // Each trailing edge's leading counterpart (the previous copy's: in the sector, the
    // trailing one turned a pitch on), whether it runs the other way, and whether it has the
    // trailing one's parameters (so a p-curve along the one is one along the other).
    let mut edge_to: Vec<Option<(usize, bool, bool)>> = vec![None; sector.edges.len()];
    let mut vert_to: Vec<Option<usize>> = vec![None; sector.verts.len()];
    for &ea in &a_edges {
        let (a0, a1, am) = ends(ea);
        let (a0, a1, am) = (apply(&r1, &a0), apply(&r1, &a1), apply(&r1, &am));
        let (eb, back) = b_edges
            .iter()
            .find_map(|&eb| {
                let (b0, b1, bm) = ends(eb);
                if near(&b0, &a0) && near(&b1, &a1) && near(&bm, &am) {
                    Some((eb, false))
                } else if near(&b0, &a1) && near(&b1, &a0) && near(&bm, &am) {
                    Some((eb, true))
                } else {
                    None
                }
            })
            .ok_or_else(|| format!("trailing edge {ea} matches no leading one"))?;
        let (a, b) = (&sector.edges[ea], &sector.edges[eb]);
        let same = !back
            && (0..2).all(|i| (a.t[i] - b.t[i]).abs() <= 1e-12 * (1.0 + a.t[i].abs()))
            && (0..=4).all(|k| {
                let t = a.t[0] + (a.t[1] - a.t[0]) * k as f64 / 4.0;
                near(&apply(&r1, &a.curve.point(t)), &b.curve.point(t))
            });
        edge_to[ea] = Some((eb, back, same));
        let (va, vb) = (a.v, b.v);
        let pairs = if back {
            [(va[0], vb[1]), (va[1], vb[0])]
        } else {
            [(va[0], vb[0]), (va[1], vb[1])]
        };
        for (x, y) in pairs {
            if vert_to[x].is_some_and(|w| w != y) {
                return Err("a vertex matched twice".into());
            }
            vert_to[x] = Some(y);
        }
    }
    let kept: Vec<usize> = (0..sector.faces.len()).filter(|&f| side[f] == 0).collect();
    let slot: Vec<Option<usize>> = {
        let mut s = vec![None; sector.faces.len()];
        for (i, &f) in kept.iter().enumerate() {
            s[f] = Some(i);
        }
        s
    };
    let nk = kept.len();
    // Faces going on across the cut: the trailing edge's face in copy k and the leading edge's
    // in copy k − 1 on one surface (in the sector, the leading one's surface holds the trailing
    // one's turned a pitch on), with the map of (u, v) from the one to the other.
    let mut across: Vec<Option<UvMap>> = vec![None; sector.edges.len()];
    for &ea in &a_edges {
        let (eb, _, _) = edge_to[ea].expect("matched");
        let (Some(fa), Some(fb)) = (kept_face(ea), kept_face(eb)) else {
            continue;
        };
        let (sa, sb) = (&sector.faces[fa], &sector.faces[fb]);
        if sa.sense != sb.sense {
            continue;
        }
        let moved = sa.surf.transformed(&r1);
        let uvs = boundary_uvs(sector, sa, 3);
        let Some(&at) = uvs.first() else { continue };
        // (`moved` and `sa.surf` share their parameters: the map from the trailing face's (u, v)
        // to the leading face's.)
        if let Some(m) = uv_map(&moved, &sb.surf, at, &uvs, tol) {
            across[ea] = Some(m);
        }
    }
    // Union-find over the copies' faces (copy k's kept face i is node k·nk + i).
    let mut parent: Vec<usize> = (0..n * nk).collect();
    fn root(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    // Each node's map of (u, v) to its group's first node's.
    let mut to_first: Vec<UvMap> = vec![UvMap::identity(); n * nk];
    // Joins by the pairs above, copy by copy: node (k, fa) onto (k − 1, fb) by the map, the
    // first copy's last (it closes the ring: its maps are the ones already made).
    let mut joins: Vec<(usize, usize, UvMap, usize, usize)> = Vec::new();
    for k in 0..n {
        let prev = (k + n - 1) % n;
        for &ea in &a_edges {
            let Some(m) = across[ea] else { continue };
            let (eb, _, _) = edge_to[ea].expect("matched");
            let (fa, fb) = (
                kept_face(ea).expect("a face"),
                kept_face(eb).expect("a face"),
            );
            let (x, y) = (
                k * nk + slot[fa].expect("kept"),
                prev * nk + slot[fb].expect("kept"),
            );
            joins.push((x, y, m, k, ea));
        }
    }
    // (Breadth first from each group's first node, so each node's map is composed along one
    // path; a ring's closing join adds nothing.)
    // (A join closing a ring, the face's two ends meeting round the axis, keeps its edge: the
    // face's seam, its two coedges a period apart, as a face round a cone has one.)
    let mut adj: Vec<Vec<(usize, UvMap, bool)>> = vec![Vec::new(); n * nk];
    let mut closing: rustc_hash::FxHashSet<(usize, usize)> = Default::default();
    for &(x, y, m, k, ea) in &joins {
        let (rx, ry) = (root(&mut parent, x), root(&mut parent, y));
        if rx == ry {
            closing.insert((k, ea));
            continue;
        }
        adj[x].push((y, m, true));
        adj[y].push((x, m, false));
        parent[rx.max(ry)] = rx.min(ry);
    }
    let mut seen = vec![false; n * nk];
    for s in 0..n * nk {
        if root(&mut parent, s) != s {
            continue;
        }
        seen[s] = true;
        let mut queue = std::collections::VecDeque::from([s]);
        while let Some(x) = queue.pop_front() {
            for &(y, m, fwd) in &adj[x] {
                if seen[y] {
                    continue;
                }
                seen[y] = true;
                // x's (u, v) → first's: to_first[x]. y's (u, v) → x's: fwd (x → y map m)
                // inverted, else m.
                let y_to_x = if fwd {
                    let inv = m.m.try_inverse().unwrap_or(nalgebra::Matrix2::identity());
                    UvMap {
                        m: inv,
                        c: -(inv * m.c),
                    }
                } else {
                    m
                };
                let f = to_first[x];
                to_first[y] = UvMap {
                    m: f.m * y_to_x.m,
                    c: f.m * y_to_x.c + f.c,
                };
                queue.push_back(y);
            }
        }
    }
    // The copies' vertices and edges, the trailing ones the previous copy's leading ones.
    let (nv, ne) = (sector.verts.len(), sector.edges.len());
    let mut out = Body::default();
    for k in 0..n {
        let m = turn(o, z, pitch * k as f64);
        let prev = (k + n - 1) % n;
        for v in &sector.verts {
            let mut v = v.clone();
            v.p = apply(&m, &v.p);
            out.verts.push(v);
        }
        for e in &sector.edges {
            let mut e = e.clone();
            e.curve = e.curve.transformed(&m);
            e.v = e.v.map(|v| match vert_to[v] {
                Some(w) => prev * nv + w,
                None => k * nv + v,
            });
            out.edges.push(e);
        }
    }
    // Whether copy k's trailing edge `ea` (by the sector's) is gone inside a face: its two faces
    // one, and not where the face's ring closes.
    // (Where the face's surface wraps round the axis, a cone's; a plane closing round its
    // centre needs no seam, its pieces' loops joining into whole ones.)
    let wraps = |ea: usize| {
        kept_face(ea).is_some_and(|f| {
            pcurve::closed_periods(&sector.faces[f].surf)
                .iter()
                .any(Option::is_some)
        })
    };
    let inside =
        |k: usize, ea: usize| across[ea].is_some() && !(closing.contains(&(k, ea)) && wraps(ea));
    // Each group's coedges, then its loops chained by vertex.
    let mut groups: rustc_hash::FxHashMap<usize, Vec<(usize, usize)>> = Default::default();
    for k in 0..n {
        for i in 0..nk {
            let x = k * nk + i;
            groups.entry(root(&mut parent, x)).or_default().push((k, i));
        }
    }
    let mut firsts: Vec<usize> = groups.keys().copied().collect();
    firsts.sort_unstable();
    let mut from: Vec<Option<usize>> = Vec::with_capacity(firsts.len());
    // Each face's pieces, as sector faces at copies counted from its first piece's copy, and
    // that copy: faces whose pieces are alike are one turned (a ring round the axis has all
    // the copies, and no other like it).
    let mut shape: Vec<(Vec<(usize, usize)>, usize)> = Vec::with_capacity(firsts.len());
    for first in firsts {
        let members = &groups[&first];
        let (k0, i0) = (first / nk, first % nk);
        let f0 = &sector.faces[kept[i0]];
        let surf = f0.surf.transformed(&turn(o, z, pitch * k0 as f64));
        // Each piece's loops (less the coedges gone inside the face).
        let mut per_loop: Vec<Vec<Option<Coedge>>> = Vec::new();
        let mut dropped = false;
        for &(k, i) in members {
            let prev = (k + n - 1) % n;
            let map = to_first[k * nk + i];
            let f = &sector.faces[kept[i]];
            for l in &f.loops {
                let mut coedges: Vec<Option<Coedge>> = Vec::new();
                for c in l {
                    if edge_side[c.edge] == 2 {
                        // A leading edge: gone where its face goes on into the next copy's (that
                        // copy's trailing edge is), else kept as it is.
                        let ea = a_edges
                            .iter()
                            .copied()
                            .find(|&ea| edge_to[ea].is_some_and(|t| t.0 == c.edge));
                        if ea.is_some_and(|ea| inside((k + 1) % n, ea)) {
                            dropped = true;
                            coedges.push(None);
                            continue;
                        }
                        coedges.push(Some(Coedge {
                            edge: k * ne + c.edge,
                            fwd: c.fwd,
                            pc: map.curve(&c.pc),
                        }));
                        continue;
                    }
                    match edge_to[c.edge] {
                        None => coedges.push(Some(Coedge {
                            edge: k * ne + c.edge,
                            fwd: c.fwd,
                            pc: map.curve(&c.pc),
                        })),
                        Some(_) if inside(k, c.edge) => {
                            dropped = true;
                            coedges.push(None);
                        }
                        Some((eb, back, same)) => {
                            let e = prev * ne + eb;
                            let pc = if same {
                                map.curve(&c.pc)
                            } else {
                                let old = &sector.edges[c.edge];
                                let mid = map.curve(&c.pc).point(0.5 * (old.t[0] + old.t[1]));
                                pcurve::pcurve(
                                    &surf,
                                    &out.edges[e].curve,
                                    out.edges[e].t[0],
                                    out.edges[e].t[1],
                                    Some(P2::new(mid.x, mid.y)),
                                )
                            };
                            coedges.push(Some(Coedge {
                                edge: e,
                                fwd: c.fwd != back,
                                pc,
                            }));
                        }
                    }
                }
                per_loop.push(coedges);
            }
        }
        // (A face that is one piece as it was keeps its loops; pieces joined are chained anew.)
        let loops = if members.len() == 1 && !dropped {
            per_loop
                .into_iter()
                .map(|l| l.into_iter().flatten().collect())
                .collect()
        } else {
            chain(&out, per_loop, &surf)?
        };
        out.faces.push(Face {
            surf,
            sense: f0.sense,
            loops,
        });
        from.push(Some(kept[i0]));
        let mut pieces: Vec<(usize, usize)> = members
            .iter()
            .map(|&(k, i)| (i, (k + n - k0) % n))
            .collect();
        pieces.sort_unstable();
        shape.push((pieces, k0));
    }
    out.shells.push(Shell {
        faces: (0..out.faces.len()).collect(),
        closed: true,
    });
    out.solids.push(Solid { shells: vec![0] });
    super::unify::compact(&mut out);
    // A face is a copy of the first face of its shape, turned on by the copies between them.
    let mut first_of: rustc_hash::FxHashMap<&Vec<(usize, usize)>, (usize, usize)> =
        Default::default();
    let mut copies: super::copies::FaceCopies = vec![None; shape.len()];
    for (f, (pieces, k)) in shape.iter().enumerate() {
        match first_of.get(pieces) {
            None => {
                first_of.insert(pieces, (f, *k));
            }
            Some(&(g, k0)) => {
                let m = turn(o, z, pitch * ((k + n - k0) % n) as f64);
                copies[f] = Some((g, super::copies::Rigid { m: m.m, t: m.t }));
            }
        }
    }
    Ok(Patterned {
        body: out,
        from,
        copies,
    })
}

/// Loops whose coedges inside the face are gone (None) joined again: each loop broken into runs
/// where its coedges went, and each run followed by the run that starts where it ends (another
/// piece's), shifted by whole periods to start where it ends in (u, v). A loop with nothing gone
/// stays as it is.
fn chain(
    b: &Body,
    loops: Vec<Vec<Option<Coedge>>>,
    surf: &Surface,
) -> Result<Vec<Vec<Coedge>>, String> {
    let start = |c: &Coedge| {
        let e = &b.edges[c.edge];
        if c.fwd { e.v[0] } else { e.v[1] }
    };
    let end = |c: &Coedge| {
        let e = &b.edges[c.edge];
        if c.fwd { e.v[1] } else { e.v[0] }
    };
    let at = |c: &Coedge, s: bool| {
        let e = &b.edges[c.edge];
        let t = if s == c.fwd { e.t[0] } else { e.t[1] };
        let q = c.pc.point(t);
        P2::new(q.x, q.y)
    };
    // (A B-spline's parameter range each way stands for a period too, where the surface meets
    // itself across it, if more loosely than `closed_periods` takes for closed.)
    let mut periods = pcurve::closed_periods(surf);
    if let Surface::Nurbs(ns) = surf {
        let ((u0, u1), (v0, v1)) = ns.range();
        if periods[0].is_none() {
            periods[0] = Some(u1 - u0);
        }
        if periods[1].is_none() {
            periods[1] = Some(v1 - v0);
        }
    }
    let mut out: Vec<Vec<Coedge>> = Vec::new();
    let mut runs: Vec<Vec<Coedge>> = Vec::new();
    for l in loops {
        if l.iter().all(Option::is_some) {
            out.push(l.into_iter().flatten().collect());
            continue;
        }
        // From just after a gap, round to it.
        let k = l.iter().position(Option::is_none).expect("a gap");
        let n = l.len();
        let mut run: Vec<Coedge> = Vec::new();
        let mut l: Vec<Option<Coedge>> = l;
        for i in 1..=n {
            match l[(k + i) % n].take() {
                Some(c) => run.push(c),
                None => {
                    if !run.is_empty() {
                        runs.push(std::mem::take(&mut run));
                    }
                }
            }
        }
    }
    let mut by_start: rustc_hash::FxHashMap<usize, Vec<usize>> = Default::default();
    for (i, r) in runs.iter().enumerate() {
        by_start.entry(start(&r[0])).or_default().push(i);
    }
    let mut used = vec![false; runs.len()];
    let mut runs: Vec<Option<Vec<Coedge>>> = runs.into_iter().map(Some).collect();
    for s in 0..runs.len() {
        if used[s] {
            continue;
        }
        used[s] = true;
        let first = runs[s].take().expect("unused");
        let v0 = start(&first[0]);
        let mut v = end(&first[first.len() - 1]);
        let mut last = at(&first[first.len() - 1], false);
        let mut l = first;
        while v != v0 {
            let Some(&i) = by_start
                .get(&v)
                .and_then(|rs| rs.iter().find(|&&i| !used[i]))
            else {
                return Err("a loop doesn't close".into());
            };
            used[i] = true;
            let mut r = runs[i].take().expect("unused");
            let s0 = at(&r[0], true);
            let mut shift = V2::zeros();
            for d in 0..2 {
                if let Some(per) = periods[d] {
                    shift[d] = ((last[d] - s0[d]) / per).round() * per;
                }
            }
            if shift.norm() > 0.0 {
                let m = Affine {
                    m: nalgebra::Matrix3::identity(),
                    t: V3::new(shift.x, shift.y, 0.0),
                };
                for c in &mut r {
                    c.pc = c.pc.transformed(&m);
                }
            }
            v = end(&r[r.len() - 1]);
            last = at(&r[r.len() - 1], false);
            l.extend(r);
        }
        out.push(l);
    }
    Ok(out)
}
