//! Merging faces that lie on the same surface and share edges (OCCT's UnifySameDomain), and
//! edges that continue each other through a vertex only they use.

use super::curve::Curve;
use super::math::*;
use super::pcurve;
use super::surface::Surface;
use super::tol;
use super::topo::{Body, Coedge, Edge, Face, co_range};

/// p-curve start point (uv) of a coedge.
fn co_uv(b: &Body, c: &Coedge, end: bool) -> P2 {
    let (t0, t1) = co_range(&b.edges[c.edge], c.fwd);
    let p = c.pc.point(if end { t1 } else { t0 });
    P2::new(p.x, p.y)
}

/// The p-curve of an edge on `s`, shifted by periods so its start (in the coedge's direction)
/// is at `at`.
fn pc_from(s: &Surface, e: &Edge, fwd: bool, at: Option<P2>) -> Curve {
    // (Hinted by `at`: on a closed B-spline surface, an edge along the seam lies at the end
    // of the range the chain is at.)
    let pc = pcurve::pcurve(s, &e.curve, e.t[0], e.t[1], at);
    let Some(at) = at else { return pc };
    let (t0, _) = co_range(e, fwd);
    let p = pc.point(t0);
    let du = if s.u_periodic() {
        ((at.x - p.x) / TAU).round() * TAU
    } else {
        0.0
    };
    let dv = if s.v_periodic() {
        ((at.y - p.y) / TAU).round() * TAU
    } else {
        0.0
    };
    if du == 0.0 && dv == 0.0 {
        pc
    } else {
        pc.transformed(&Affine::translation(V3::new(du, dv, 0.0)))
    }
}

/// The p-curve of coedge `c` on `s` continuing a chain at `at` (None: as it lies). A
/// degenerate edge (a pole) keeps its own p-curve, moved to start at `at`: its 3D curve is a
/// point, which says nothing about the parameter line it runs along.
fn chain_pc(s: &Surface, b: &Body, c: &Coedge, at: Option<P2>) -> Curve {
    let e = &b.edges[c.edge];
    if !e.degenerate {
        return pc_from(s, e, c.fwd, at);
    }
    let Some(at) = at else { return c.pc.clone() };
    let start = co_uv(b, c, false);
    let d = at - start;
    if d.norm() == 0.0 {
        c.pc.clone()
    } else {
        c.pc.transformed(&Affine::translation(V3::new(d.x, d.y, 0.0)))
    }
}

struct Dsu(Vec<usize>);
impl Dsu {
    fn find(&mut self, x: usize) -> usize {
        let mut r = x;
        while self.0[r] != r {
            r = self.0[r];
        }
        let mut y = x;
        while self.0[y] != r {
            let n = self.0[y];
            self.0[y] = r;
            y = n;
        }
        r
    }
    fn join(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            let (lo, hi) = if a < b { (a, b) } else { (b, a) };
            self.0[hi] = lo;
        }
    }
}

/// Splits edge `e` at parameter t (a new vertex), updating every face that uses it. Returns the
/// new vertex.
pub fn split_edge(b: &mut Body, e: usize, t: f64) -> usize {
    let p = b.edges[e].point(t);
    let v = b.add_vertex(p);
    let old = b.edges[e].clone();
    b.edges[e] = Edge {
        t: [old.t[0], t],
        v: [old.v[0], v],
        ..old.clone()
    };
    b.edges.push(Edge {
        t: [t, old.t[1]],
        v: [v, old.v[1]],
        ..old
    });
    let e2 = b.edges.len() - 1;
    for f in &mut b.faces {
        for l in &mut f.loops {
            let mut nl = Vec::with_capacity(l.len() + 1);
            for c in l.drain(..) {
                if c.edge == e {
                    let second = Coedge {
                        edge: e2,
                        fwd: c.fwd,
                        pc: c.pc.clone(),
                    };
                    if c.fwd {
                        nl.push(c);
                        nl.push(second);
                    } else {
                        nl.push(second);
                        nl.push(c);
                    }
                } else {
                    nl.push(c);
                }
            }
            *l = nl;
        }
    }
    v
}

/// Whether the middle of face `f`'s first edge other than `e` lies on surface `s` as
/// [`boundary_on`] measures (true when `f` has no other edge): one of the points that test takes.
fn edge_mid_on(b: &Body, f: usize, e: usize, s: &Surface) -> bool {
    let Some(c) = b.faces[f]
        .loops
        .iter()
        .flatten()
        .find(|c| c.edge != e && !b.edges[c.edge].degenerate)
    else {
        return true;
    };
    let ed = &b.edges[c.edge];
    let p = ed.point(ed.t[0] + (ed.t[1] - ed.t[0]) * 2.0 / 4.0);
    let (u, v) = s.project(&p, None);
    (s.eval(u, v) - p).norm() <= tol::REL_POINT * (1.0 + p.coords.norm())
}

/// Whether face `f`'s edges lie on surface `s` (a few points of each, within 1e-6 of their
/// size), where `s` is: inside a B-spline's range, not on its extension.
/// Whether face `f`'s edges' ends lie within the merge distance of the plane `s`.
fn flat_on(b: &Body, f: usize, s: &Surface) -> bool {
    b.faces[f].loops.iter().flatten().all(|c| {
        let e = &b.edges[c.edge];
        [e.t[0], e.t[1]].iter().all(|&t| {
            s.signed_distance(&e.point(t))
                .is_none_or(|d| d.abs() <= tol::POINT)
        })
    })
}

fn boundary_on(b: &Body, f: usize, s: &Surface) -> bool {
    b.faces[f].loops.iter().flatten().all(|c| {
        let e = &b.edges[c.edge];
        (0..=4).all(|i| {
            let p = e.point(e.t[0] + (e.t[1] - e.t[0]) * i as f64 / 4.0);
            let (u, v) = s.project(&p, None);
            (s.eval(u, v) - p).norm() <= tol::REL_POINT * (1.0 + p.coords.norm())
        })
    })
}

/// Line edges between the same two vertices, the same way: a boolean leaves two where a
/// surface touches another along the other's seam (holes tangent to each other, one's seam
/// where they touch), one each side of the seam. As one edge, the pieces of the surface it
/// splits there have an edge in common to merge across. Per edge, the edge it is the same as
/// (`usize::MAX`: none); `None` when there are none.
fn coincident_lines(b: &Body) -> Option<Vec<usize>> {
    let mut first: std::collections::HashMap<[usize; 2], usize> = Default::default();
    let mut to = vec![usize::MAX; b.edges.len()];
    for (i, e) in b.edges.iter().enumerate() {
        if e.degenerate || e.v[0] == e.v[1] || !matches!(e.curve, Curve::Line { .. }) {
            continue;
        }
        match first.get(&e.v) {
            None => {
                first.insert(e.v, i);
            }
            Some(&j) => {
                let f = &b.edges[j];
                let same = (0..=2).all(|k| {
                    let t = e.t[0] + (e.t[1] - e.t[0]) * k as f64 / 2.0;
                    (f.point(t) - e.point(t)).norm() <= tol::POINT
                }) && (f.t[0] - e.t[0]).abs() <= tol::POINT
                    && (f.t[1] - e.t[1]).abs() <= tol::POINT;
                if same {
                    to[i] = j;
                }
            }
        }
    }
    to.iter().any(|&j| j != usize::MAX).then_some(to)
}

/// `b` with the edges `to` maps ([`coincident_lines`]) replaced.
fn join_edges(b: &mut Body, to: &[usize]) {
    for f in &mut b.faces {
        for c in f.loops.iter_mut().flatten() {
            if to[c.edge] != usize::MAX {
                c.edge = to[c.edge];
            }
        }
    }
    compact(b);
}

/// Merges groups of faces; `can_merge(f, g)` says whether two faces may be merged (faces of
/// different inputs). `from` (per face) is concatenated for merged faces, the group's first face
/// first. Returns the new `from`.
pub fn unify_faces(
    b: &mut Body,
    from: Vec<Vec<(usize, usize)>>,
    can_merge: &dyn Fn(usize, usize) -> bool,
) -> Vec<Vec<(usize, usize)>> {
    // Coincident line edges joined, if the faces then merge so that no edge has more than two
    // uses; else as they were.
    if let Some(to) = coincident_lines(b) {
        let mut joined = b.clone();
        join_edges(&mut joined, &to);
        let out = merge_faces(&mut joined, from.clone(), can_merge);
        if joined.edge_faces().iter().all(|fs| fs.len() <= 2) {
            *b = joined;
            return out;
        }
    }
    merge_faces(b, from, can_merge)
}

/// [`unify_faces`] once coincident edges are joined.
fn merge_faces(
    b: &mut Body,
    from: Vec<Vec<(usize, usize)>>,
    can_merge: &dyn Fn(usize, usize) -> bool,
) -> Vec<Vec<(usize, usize)>> {
    let nf = b.faces.len();
    let ef = b.edge_faces();
    let mut dsu = Dsu((0..nf).collect());
    // Whether faces `f` and `g`, either side of edge `e`, lie on one surface and may merge.
    let mergeable = |f: usize, g: usize, e: usize| -> bool {
        if f == g || !can_merge(f, g) {
            return false;
        }
        let (a, c) = (&b.faces[f], &b.faces[g]);
        // B-splines and extrusions: the merged face takes the first's surface and
        // parametrization, so two different ones (coinciding by a sampled test, perhaps over
        // part of their extents) merge only when each one's boundary lies on the other.
        let sampled = |s: &Surface| matches!(s, Surface::Nurbs(_) | Surface::Extrusion { .. });
        // (Which takes, first, the middle of one of each face's other edges lying on the other's
        // surface: one projection each, where the sampled comparison of the two surfaces takes
        // eighty-odd; adjacent pieces of different surfaces mostly fail it.)
        if a.surf != c.surf
            && (sampled(&a.surf) || sampled(&c.surf))
            && !(edge_mid_on(b, f, e, &c.surf) && edge_mid_on(b, g, e, &a.surf))
        {
            return false;
        }
        let same = if a.surf == c.surf {
            Some(true)
        } else {
            a.surf.same_domain(&c.surf, tol::POINT)
        };
        // Two planes a hair from parallel pass `same_domain` (its test is on the cosine of their
        // angle): they merge only when each face lies on the other's plane too (a cube's top and
        // the face of a cut a millionth steep across it don't).
        let planes =
            matches!(a.surf, Surface::Plane { .. }) && matches!(c.surf, Surface::Plane { .. });
        if planes && a.surf != c.surf && !(flat_on(b, f, &c.surf) && flat_on(b, g, &a.surf)) {
            return false;
        }
        same.is_some_and(|same| same == (a.sense == c.sense))
            && (!(sampled(&a.surf) || sampled(&c.surf))
                || a.surf == c.surf
                || (boundary_on(b, f, &c.surf) && boundary_on(b, g, &a.surf)))
    };
    for (e, fs) in ef.iter().enumerate() {
        match fs.len() {
            2 => {
                if mergeable(fs[0], fs[1], e) {
                    dsu.join(fs[0], fs[1]);
                }
            }
            // Two surfaces touching along the edge (holes tangent to each other, the wall
            // between them worn to nothing), each split there: each surface's two pieces
            // merge, and the surfaces just touch, as OCCT leaves them (issue #104).
            // (Or one surface's seam, a face on both sides, that another touches along.)
            4 => {
                let fits = |f: usize, g: usize| f == g || mergeable(f, g, e);
                let pairs = (1..4).find_map(|k| {
                    let o: Vec<usize> = (1..4).filter(|&i| i != k).collect();
                    let pairs = [(fs[0], fs[k]), (fs[o[0]], fs[o[1]])];
                    pairs.iter().all(|&(f, g)| fits(f, g)).then_some(pairs)
                });
                for (f, g) in pairs.into_iter().flatten() {
                    dsu.join(f, g);
                }
            }
            _ => {}
        }
    }
    let mut groups: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    for f in 0..nf {
        let r = dsu.find(f);
        groups.entry(r).or_default().push(f);
    }
    // Merged faces go into place at once (a later group's re-seaming splits edges in every face
    // of `b`, these included).
    let mut replace = vec![false; nf];
    let mut drop = vec![false; nf];
    // Each face's group's first face (where a merged face goes).
    let mut first = vec![usize::MAX; nf];
    for g in groups.values() {
        for &f in g {
            first[f] = g[0];
        }
    }
    for (_, g) in groups {
        if g.len() < 2 {
            continue;
        }
        if let Some(face) = merge_group(b, &g) {
            // A merged face whose loops cross in (u, v) is no face: the pieces stay apart. (A
            // region wrapping a cylinder more than once, as the faces round a helical thread
            // joined to it: every iso-u seam from its bottom to its top crosses the thread.)
            let piece = std::mem::replace(&mut b.faces[g[0]], face);
            if b.check_face_domain(g[0])
                .iter()
                .any(|e| e.contains("cross"))
            {
                b.faces[g[0]] = piece;
                continue;
            }
            replace[g[0]] = true;
            for &f in &g[1..] {
                drop[f] = true;
            }
        }
    }
    // Rebuild the face list.
    let mut new_from: Vec<Vec<(usize, usize)>> = Vec::new();
    let mut faces = Vec::new();
    let mut gfrom: std::collections::HashMap<usize, Vec<(usize, usize)>> = Default::default();
    // The sources of a merged face: all its group's (keyed by the group's first face, where the
    // merged face is: the union-find root needn't be that face).
    for f in 0..nf {
        let g0 = first[f];
        if replace[g0] {
            gfrom.entry(g0).or_default().extend(from[f].iter().copied());
        }
    }
    for f in 0..nf {
        if drop[f] {
            continue;
        }
        faces.push(b.faces[f].clone());
        new_from.push(if replace[f] {
            gfrom.remove(&f).unwrap_or_else(|| from[f].clone())
        } else {
            from[f].clone()
        });
    }
    b.faces = faces;
    compact(b);
    // Pieces of one input face merged again name it once.
    for srcs in &mut new_from {
        let mut seen = std::collections::HashSet::new();
        srcs.retain(|s| seen.insert(*s));
    }
    new_from
}

/// Merges the faces of a boolean's result that lie on one surface across an edge (OCCT's
/// UnifySameDomain after a boolean): pieces of the two inputs (a union's coplanar faces, a
/// cut's wall continuing a face of the target), and pieces of one input face that a
/// coincident face of the other body imprinted and the operation kept on both sides. Two
/// faces of one input stay apart (the input had them so); with `flush` (a blend), any merge.
/// `from` is each face's sources (owner, input face) as [`unify_faces`] takes it.
pub fn unify_boolean(
    b: &mut Body,
    from: Vec<Vec<(usize, usize)>>,
    flush: bool,
) -> Vec<Vec<(usize, usize)>> {
    let src: Vec<(usize, usize)> = from.iter().map(|f| f[0]).collect();
    unify_faces(b, from, &|f, g| {
        flush || src[f].0 != src[g].0 || src[f] == src[g]
    })
}

/// Gives each shell its own copy of a vertex several shells share (solids touching at a
/// corner: the boolean pools their vertices), so every vertex lies on one shell and the
/// shells' Euler characteristics stand alone. The first shell keeps the vertex.
pub fn split_shared_vertices(b: &mut Body) {
    let mut shell_of_edge = vec![usize::MAX; b.edges.len()];
    for (si, s) in b.shells.iter().enumerate() {
        for &f in &s.faces {
            for c in b.faces[f].loops.iter().flatten() {
                shell_of_edge[c.edge] = si;
            }
        }
    }
    // For each vertex, its copy per shell after the first that uses it.
    let mut copies: std::collections::HashMap<(usize, usize), usize> = Default::default();
    let mut owner: Vec<Option<usize>> = vec![None; b.verts.len()];
    for e in 0..b.edges.len() {
        let s = shell_of_edge[e];
        if s == usize::MAX {
            continue;
        }
        for k in 0..2 {
            let v = b.edges[e].v[k];
            match owner[v] {
                None => owner[v] = Some(s),
                Some(o) if o == s => {}
                Some(_) => {
                    let nv = *copies.entry((v, s)).or_insert_with(|| {
                        b.verts.push(b.verts[v].clone());
                        b.verts.len() - 1
                    });
                    b.edges[e].v[k] = nv;
                }
            }
        }
    }
}

/// Removes slits from faces on surfaces that aren't periodic (an edge a planar face uses twice,
/// as a full revolve leaves on its discs and annuli): their loops are chained again without
/// them (the largest first, as the outer loop), and unused edges are dropped.
pub fn drop_slits(b: &mut Body) {
    for fi in 0..b.faces.len() {
        let f = &b.faces[fi];
        if f.surf.u_periodic() || f.surf.v_periodic() {
            continue;
        }
        let mut uses: std::collections::HashMap<usize, usize> = Default::default();
        for c in f.loops.iter().flatten() {
            *uses.entry(c.edge).or_default() += 1;
        }
        if !uses.iter().any(|(e, n)| *n == 2 && !b.edges[*e].degenerate) {
            continue;
        }
        let rest: Vec<Coedge> = f
            .loops
            .iter()
            .flatten()
            .filter(|c| uses[&c.edge] != 2 && !b.edges[c.edge].degenerate)
            .cloned()
            .collect();
        if rest.is_empty() {
            continue;
        }
        // Chain by vertices.
        let mut used = vec![false; rest.len()];
        let mut loops: Vec<Vec<Coedge>> = Vec::new();
        let mut ok = true;
        for s0 in 0..rest.len() {
            if used[s0] {
                continue;
            }
            used[s0] = true;
            let mut ch = vec![rest[s0].clone()];
            let (start, mut end) = (b.co_start(&rest[s0]), b.co_end(&rest[s0]));
            while end != start {
                match (0..rest.len()).find(|&k| !used[k] && b.co_start(&rest[k]) == end) {
                    Some(nx) => {
                        used[nx] = true;
                        end = b.co_end(&rest[nx]);
                        ch.push(rest[nx].clone());
                    }
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok {
                break;
            }
            loops.push(ch);
        }
        if !ok {
            continue;
        }
        // The outer loop first: the largest in extent.
        let extent = |l: &Vec<Coedge>| {
            let mut bb = Bbox::empty();
            for c in l {
                let e = &b.edges[c.edge];
                for i in 0..=8 {
                    bb.add(&e.point(e.t[0] + (e.t[1] - e.t[0]) * i as f64 / 8.0));
                }
            }
            bb.diagonal()
        };
        loops.sort_by(|x, y| extent(y).total_cmp(&extent(x)));
        b.faces[fi].loops = loops;
    }
    compact(b);
}

/// Drops unused edges and vertices.
pub fn compact(b: &mut Body) {
    let mut used_e = vec![false; b.edges.len()];
    for f in &b.faces {
        for l in &f.loops {
            for c in l {
                used_e[c.edge] = true;
            }
        }
    }
    let mut emap = vec![usize::MAX; b.edges.len()];
    let mut edges = Vec::new();
    for (i, e) in b.edges.iter().enumerate() {
        if used_e[i] {
            emap[i] = edges.len();
            edges.push(e.clone());
        }
    }
    let mut used_v = vec![false; b.verts.len()];
    for e in &edges {
        used_v[e.v[0]] = true;
        used_v[e.v[1]] = true;
    }
    let mut vmap = vec![usize::MAX; b.verts.len()];
    let mut verts = Vec::new();
    for (i, v) in b.verts.iter().enumerate() {
        if used_v[i] {
            vmap[i] = verts.len();
            verts.push(v.clone());
        }
    }
    for e in &mut edges {
        e.v = [vmap[e.v[0]], vmap[e.v[1]]];
    }
    for f in &mut b.faces {
        for l in &mut f.loops {
            for c in l {
                c.edge = emap[c.edge];
            }
        }
    }
    b.edges = edges;
    b.verts = verts;
}

/// Pieces of a band merged into one (#114: a partial torus revolved on, or mirrored; an
/// ellipse's or a closed spline's surface of revolution): the group's outer boundary is two
/// closed curves, each once round the surface the way its pieces' seams cross, and the pieces'
/// seams join end to end from one to the other. The merged face runs round the first curve,
/// along the seams to the second, round it, and back: as each piece did. `None` when the group
/// isn't such a band (the loop must close in the base face's parameters).
fn merge_band(b: &Body, g: &[usize]) -> Option<Face> {
    let base = g[0];
    let surf = b.faces[base].surf.clone();
    let mut uses: std::collections::HashMap<usize, Vec<usize>> = Default::default();
    for &f in g {
        for c in b.faces[f].loops.iter().flatten() {
            uses.entry(c.edge).or_default().push(f);
        }
    }
    let internal = |e: usize| uses[&e].iter().any(|x| *x != uses[&e][0]);
    let seam = |e: usize| uses[&e].len() == 2 && uses[&e][0] == uses[&e][1];
    // The boundary's coedges, and the seams (each once).
    let mut rest: Vec<Coedge> = Vec::new();
    let mut seams: Vec<usize> = Vec::new();
    for &f in g {
        for c in b.faces[f].loops.iter().flatten() {
            if b.edges[c.edge].degenerate || internal(c.edge) {
                continue;
            }
            if seam(c.edge) {
                if !seams.contains(&c.edge) {
                    seams.push(c.edge);
                }
                continue;
            }
            rest.push(c.clone());
        }
    }
    // Two closed chains of the boundary.
    let mut used = vec![false; rest.len()];
    let mut chains: Vec<Vec<Coedge>> = Vec::new();
    for s in 0..rest.len() {
        if used[s] {
            continue;
        }
        used[s] = true;
        let mut ch = vec![rest[s].clone()];
        let (start, mut end) = (b.co_start(&rest[s]), b.co_end(&rest[s]));
        while end != start {
            let k = (0..rest.len()).find(|&k| !used[k] && b.co_start(&rest[k]) == end)?;
            used[k] = true;
            end = b.co_end(&rest[k]);
            ch.push(rest[k].clone());
        }
        chains.push(ch);
    }
    if chains.len() != 2 || seams.is_empty() {
        return None;
    }
    let on = |ch: &[Coedge], v: usize| ch.iter().any(|c| b.co_start(c) == v);
    // The seams as one path from a vertex of the first chain to one of the second.
    let ends = |e: usize| b.edges[e].v;
    let start_v = seams
        .iter()
        .flat_map(|&e| ends(e))
        .find(|&v| on(&chains[0], v))?;
    let mut path: Vec<Coedge> = Vec::new();
    let mut at = start_v;
    let mut left = seams.clone();
    while !on(&chains[1], at) {
        let k = left.iter().position(|&e| ends(e).contains(&at))?;
        let e = left.remove(k);
        let fwd = ends(e)[0] == at;
        path.push(Coedge {
            edge: e,
            fwd,
            pc: b.faces[base].loops[0][0].pc.clone(),
        });
        at = if fwd { ends(e)[1] } else { ends(e)[0] };
    }
    if !left.is_empty() || path.is_empty() {
        return None;
    }
    let end_v = at;
    let rotate_to = |ch: &mut Vec<Coedge>, v: usize| -> Option<()> {
        let k = ch.iter().position(|c| b.co_start(c) == v)?;
        ch.rotate_left(k);
        Some(())
    };
    let (mut c1, mut c2) = (chains[0].clone(), chains[1].clone());
    rotate_to(&mut c1, start_v)?;
    rotate_to(&mut c2, end_v)?;
    let back: Vec<Coedge> = path
        .iter()
        .rev()
        .map(|c| Coedge {
            edge: c.edge,
            fwd: !c.fwd,
            pc: c.pc.clone(),
        })
        .collect();
    let mut lp: Vec<Coedge> = c1;
    lp.extend(path);
    lp.extend(c2);
    lp.extend(back);
    // p-curves continuous along the loop, from the base face's chart; the loop must close
    // there (the seam's two runs a period apart in v).
    let center = {
        let mut bb = Bbox::empty();
        for c in b.faces[base].loops.iter().flatten() {
            let q = co_uv(b, c, false);
            bb.add(&P3::new(q.x, q.y, 0.0));
        }
        let c = bb.center();
        P2::new(c.x, c.y)
    };
    let close = |p: P2, q: P2| (p - q).norm() <= tol::ROUNDOFF * (1.0 + p.coords.norm());
    let mut at: Option<P2> = None;
    for (k, c) in lp.iter_mut().enumerate() {
        c.pc = if k == 0 {
            let e = &b.edges[c.edge];
            pcurve::pcurve(&surf, &e.curve, e.t[0], e.t[1], Some(center))
        } else {
            chain_pc(&surf, b, c, at)
        };
        // Across a seam the surface doesn't call periodic (a closed meridian's surface of
        // revolution): the p-curve moved on to where the loop is, if the surface repeats there
        // all along it.
        if let Some(a) = at
            && !close(co_uv(b, c, false), a)
        {
            c.pc = repeat_onto(&surf, &c.pc, &b.edges[c.edge], c.fwd, a)?;
        }
        at = Some(co_uv(b, c, true));
    }
    let (first, last) = (co_uv(b, &lp[0], false), at?);
    if !close(first, last) {
        return None;
    }
    Some(Face {
        surf,
        sense: b.faces[base].sense,
        loops: vec![lp],
    })
}

/// A coedge's p-curve `pc` (of edge `e`, run `fwd`) moved to start at `at`, where the surface
/// repeats it: the same point at each of its samples moved so (across a seam of a surface that
/// doesn't call itself periodic, as a closed meridian's surface of revolution). `None` when
/// the surface doesn't repeat there.
fn repeat_onto(surf: &Surface, pc: &Curve, e: &Edge, fwd: bool, at: P2) -> Option<Curve> {
    let (t0, _) = co_range(e, fwd);
    let s = pc.point(t0);
    let d = at - P2::new(s.x, s.y);
    let size = 1.0 + e.point(e.t[0]).coords.norm();
    let same = (0..=8).all(|i| {
        let q = pc.point(e.t[0] + (e.t[1] - e.t[0]) * i as f64 / 8.0);
        (surf.eval(q.x, q.y) - surf.eval(q.x + d.x, q.y + d.y)).norm() <= tol::REL_POINT * size
    });
    same.then(|| pc.transformed(&Affine::translation(V3::new(d.x, d.y, 0.0))))
}

/// One pole edge a whole turn at vertex `w`, in place of the pieces' `poles` there (its range
/// the sum of theirs), added to `b`.
fn pole_edge(b: &mut Body, poles: &[usize], w: usize) -> Option<usize> {
    let here: Vec<usize> = poles
        .iter()
        .copied()
        .filter(|&e| b.edges[e].v[0] == w)
        .collect();
    let first = *here.first()?;
    let span: f64 = here
        .iter()
        .map(|&e| b.edges[e].t[1] - b.edges[e].t[0])
        .sum();
    let mut e = b.edges[first].clone();
    e.t = [0.0, span];
    b.edges.push(e);
    Some(b.edges.len() - 1)
}

/// Pieces that close a whole surface merged into one (#114: a torus of two halves, or of a
/// circular pattern's quarters; a sphere of two pieces): the group has no boundary left, only
/// edges between its pieces, the pieces' own seams and poles. As one revolve makes it, the face
/// runs along one of the edges between the pieces (its new seam `c`), then
/// - a torus: along the pieces' seams round the other way back to where it started, back along
///   `c` and back round the seams;
/// - a sphere: round the pole at its end (one pole edge there, a whole turn), back along `c`
///   and round the pole at its start;
/// - a cone or a dome (a pole at one end, the group's boundary round the other): round the
///   boundary, along `c` to the pole, round it and back.
///
/// Whichever way round gives the face a positive area. `None` when the group isn't such a
/// closed surface.
fn merge_closed(b: &mut Body, g: &[usize]) -> Option<Face> {
    let base = g[0];
    let surf = b.faces[base].surf.clone();
    let sense = b.faces[base].sense;
    let mut uses: std::collections::HashMap<usize, Vec<usize>> = Default::default();
    for &f in g {
        for c in b.faces[f].loops.iter().flatten() {
            uses.entry(c.edge).or_default().push(f);
        }
    }
    let internal = |e: usize| uses[&e].iter().any(|x| *x != uses[&e][0]);
    let seam = |e: usize| uses[&e].len() == 2 && uses[&e][0] == uses[&e][1];
    // The new seam: the base face's first edge shared with another piece; nothing else may
    // bound the group.
    let mut c: Option<Coedge> = None;
    let (mut seams, mut poles): (Vec<usize>, Vec<usize>) = (Vec::new(), Vec::new());
    let mut rest: Vec<Coedge> = Vec::new();
    for &f in g {
        for co in b.faces[f].loops.iter().flatten() {
            let e = co.edge;
            if b.edges[e].degenerate {
                if !poles.contains(&e) {
                    poles.push(e);
                }
            } else if internal(e) {
                if c.is_none() && f == base {
                    c = Some(co.clone());
                }
            } else if seam(e) {
                if !seams.contains(&e) {
                    seams.push(e);
                }
            } else {
                rest.push(co.clone());
            }
        }
    }
    let c = c?;
    let [cs, ce] = b.edges[c.edge].v;
    let rev = |x: &Coedge| Coedge {
        edge: x.edge,
        fwd: !x.fwd,
        pc: x.pc.clone(),
    };
    // The loops to try, the second the other way round.
    let tries: Vec<Vec<Coedge>> =
        if cs == ce && poles.is_empty() && !seams.is_empty() && rest.is_empty() {
            // Round the pieces' seams from c's vertex back to it.
            let mut path: Vec<Coedge> = Vec::new();
            let mut at = cs;
            let mut left = seams.clone();
            loop {
                let k = left.iter().position(|&e| b.edges[e].v.contains(&at))?;
                let e = left.remove(k);
                let fwd = b.edges[e].v[0] == at;
                path.push(Coedge {
                    edge: e,
                    fwd,
                    pc: c.pc.clone(),
                });
                at = if fwd {
                    b.edges[e].v[1]
                } else {
                    b.edges[e].v[0]
                };
                if at == cs {
                    break;
                }
            }
            if !left.is_empty() {
                return None;
            }
            let back: Vec<Coedge> = path.iter().rev().map(rev).collect();
            let c_fwd = Coedge {
                fwd: true,
                ..c.clone()
            };
            let one = [
                vec![c_fwd.clone()],
                path.clone(),
                vec![rev(&c_fwd)],
                back.clone(),
            ]
            .concat();
            let two = [vec![c_fwd.clone()], back, vec![rev(&c_fwd)], path].concat();
            vec![one, two]
        } else if cs != ce && seams.is_empty() && !poles.is_empty() {
            // One pole edge a whole turn at each pole of c, in place of the pieces' (each the sum
            // of their ranges).
            let at_cs = poles.iter().any(|&e| b.edges[e].v[0] == cs);
            let at_ce = poles.iter().any(|&e| b.edges[e].v[0] == ce);
            if !rest.is_empty() {
                // A cone or dome: the boundary one closed chain through c's other end.
                let (pole_v, base_v) = match (at_cs, at_ce) {
                    (true, false) => (cs, ce),
                    (false, true) => (ce, cs),
                    _ => return None,
                };
                let mut chain: Vec<Coedge> = Vec::new();
                let mut used = vec![false; rest.len()];
                let mut at = base_v;
                loop {
                    let k = (0..rest.len()).find(|&k| !used[k] && b.co_start(&rest[k]) == at)?;
                    used[k] = true;
                    at = b.co_end(&rest[k]);
                    chain.push(rest[k].clone());
                    if at == base_v {
                        break;
                    }
                }
                if used.iter().any(|u| !u) {
                    return None;
                }
                let d = pole_edge(b, &poles, pole_v)?;
                let up = Coedge {
                    fwd: b.edges[c.edge].v[0] == base_v,
                    ..c.clone()
                };
                let co = |fwd: bool| Coedge {
                    edge: d,
                    fwd,
                    pc: c.pc.clone(),
                };
                let one = [chain.clone(), vec![up.clone(), co(true), rev(&up)]].concat();
                let two = [chain, vec![up.clone(), co(false), rev(&up)]].concat();
                vec![one, two]
            } else {
                if !(at_cs && at_ce) {
                    return None;
                }
                let (ds, de) = (pole_edge(b, &poles, cs)?, pole_edge(b, &poles, ce)?);
                let co = |e: usize, fwd: bool| Coedge {
                    edge: e,
                    fwd,
                    pc: c.pc.clone(),
                };
                let c_fwd = Coedge {
                    fwd: true,
                    ..c.clone()
                };
                let one = vec![co(ds, true), c_fwd.clone(), co(de, false), rev(&c_fwd)];
                let two = vec![co(ds, false), c_fwd.clone(), co(de, true), rev(&c_fwd)];
                vec![one, two]
            }
        } else {
            return None;
        };
    // The base face's chart: the loop starts there.
    let center = {
        let mut bb = Bbox::empty();
        for co in b.faces[base].loops.iter().flatten() {
            let q = co_uv(b, co, false);
            bb.add(&P3::new(q.x, q.y, 0.0));
        }
        let m = bb.center();
        P2::new(m.x, m.y)
    };
    let flip = if sense { 1.0 } else { -1.0 };
    let (mu, mv) = surf.metric();
    let close = |p: P2, q: P2| (p - q).norm() <= tol::ROUNDOFF * (1.0 + p.coords.norm());
    'tries: for mut lp in tries {
        // The p-curves: c in the base face's chart, the rest on from it; a pole's along the
        // pole from where the edge before ends to where the one after starts (after it is
        // placed, a period along).
        let k0 = lp.iter().position(|x| !b.edges[x.edge].degenerate)?;
        lp.rotate_left(k0);
        let e0 = &b.edges[lp[0].edge];
        lp[0].pc = pcurve::pcurve(&surf, &e0.curve, e0.t[0], e0.t[1], Some(center));
        let n = lp.len();
        let mut at = co_uv(b, &lp[0], true);
        for k in 1..n {
            let e = b.edges[lp[k].edge].clone();
            if e.degenerate {
                // Round the pole a whole turn in u, the way that gives a positive area (tried
                // both ways by the two loops).
                let (t0, t1) = co_range(&e, lp[k].fwd);
                let turn = if lp[k].fwd { TAU } else { -TAU };
                let d = V3::new(turn / (t1 - t0), 0.0, 0.0);
                let o = P3::new(at.x - d.x * t0, at.y, 0.0);
                lp[k].pc = Curve::Line { o, d };
            } else {
                lp[k].pc = chain_pc(&surf, b, &lp[k], Some(at));
                if !close(co_uv(b, &lp[k], false), at) {
                    match repeat_onto(&surf, &lp[k].pc, &e, lp[k].fwd, at) {
                        Some(pc) => lp[k].pc = pc,
                        None => continue 'tries,
                    }
                }
            }
            at = co_uv(b, &lp[k], true);
        }
        if !close(at, co_uv(b, &lp[0], false)) {
            continue;
        }
        let mut pts = Vec::new();
        for x in &lp {
            let e = &b.edges[x.edge];
            let (t0, t1) = co_range(e, x.fwd);
            for i in 0..32 {
                let q = x.pc.point(t0 + (t1 - t0) * i as f64 / 32.0);
                pts.push(P2::new(q.x * mu, q.y * mv * flip));
            }
        }
        if polygon_area(&pts) > 0.0 {
            return Some(Face {
                surf,
                sense,
                loops: vec![lp],
            });
        }
    }
    None
}

/// One merged face for the group, or None when the merge can't be made cleanly.
fn merge_group(b: &mut Body, g: &[usize]) -> Option<Face> {
    if let Some(f) = merge_band(b, g) {
        return Some(f);
    }
    if let Some(f) = merge_closed(b, g) {
        return Some(f);
    }
    let base = g[0];
    let surf = b.faces[base].surf.clone();
    let sense = b.faces[base].sense;
    // Edge uses within the group.
    let mut uses: std::collections::HashMap<usize, Vec<usize>> = Default::default();
    for &f in g {
        for l in &b.faces[f].loops {
            for c in l {
                uses.entry(c.edge).or_default().push(f);
            }
        }
    }
    let internal = |e: usize| -> bool {
        let u = &uses[&e];
        u.len() >= 2 && u.iter().any(|x| *x != u[0])
    };
    let seam = |e: usize| -> bool {
        let u = &uses[&e];
        u.len() == 2 && u[0] == u[1]
    };
    let mut rest: Vec<Coedge> = Vec::new();
    for &f in g {
        for l in &b.faces[f].loops {
            for c in l {
                if internal(c.edge) {
                    continue;
                }
                // A seam: periodic surfaces are re-seamed below; on others it is a slit (a full
                // revolve's planar annulus), which bounds nothing.
                if seam(c.edge) {
                    continue;
                }
                // A pole edge of a face on another parametrization of the surface (a sphere
                // turned another way) where the base surface is regular there: a point.
                let e = &b.edges[c.edge];
                if e.degenerate {
                    let p = e.point(e.t[0]);
                    let (u, v) = surf.project(&p, None);
                    let (_, su, sv) = surf.d1(u, v);
                    if su.cross(&sv).norm() > tol::REL * (1.0 + su.norm() * sv.norm()) {
                        continue;
                    }
                }
                rest.push(c.clone());
            }
        }
    }
    if rest.is_empty() {
        return None;
    }
    // Two pole edges at one vertex (each face had its own) would need joining into one over
    // both their parameter ranges: left apart.
    let mut poles = std::collections::HashSet::new();
    for c in rest.iter().filter(|c| b.edges[c.edge].degenerate) {
        if !poles.insert(b.edges[c.edge].v[0]) {
            return None;
        }
    }
    // Chain into closed loops by vertices.
    let mut used = vec![false; rest.len()];
    let mut chains: Vec<Vec<Coedge>> = Vec::new();
    // Chains start on real edges (a pole's degenerate edge starts and ends at one vertex: a
    // chain begun there would close at once).
    let deg = |k: usize| b.edges[rest[k].edge].degenerate;
    let order: Vec<usize> = (0..rest.len())
        .filter(|&k| !deg(k))
        .chain((0..rest.len()).filter(|&k| deg(k)))
        .collect();
    for s in order {
        if used[s] {
            continue;
        }
        used[s] = true;
        let mut ch = vec![rest[s].clone()];
        let (start, mut end) = (b.co_start(&rest[s]), b.co_end(&rest[s]));
        let mut guard = 0;
        loop {
            // A pole's degenerate edge first: the loop runs along it as soon as it reaches
            // the pole (else it would be left as a loop of its own).
            let at_end = |k: &usize| !used[*k] && b.co_start(&rest[*k]) == end;
            let pole = (0..rest.len()).filter(at_end).find(|&k| deg(k));
            let nx = match pole {
                Some(k) => k,
                None if end == start => break,
                None => (0..rest.len()).find(at_end)?,
            };
            used[nx] = true;
            end = b.co_end(&rest[nx]);
            ch.push(rest[nx].clone());
            guard += 1;
            if guard > rest.len() {
                return None;
            }
        }
        chains.push(ch);
    }
    // A chain that runs along an edge and straight back (a seam of one face that is another's
    // boundary, cancelled where the faces join): a slit bounding nothing, dropped.
    for ch in &mut chains {
        while ch.len() > 2 {
            let n = ch.len();
            let Some(k) = (0..n)
                .find(|&k| ch[k].edge == ch[(k + 1) % n].edge && ch[k].fwd != ch[(k + 1) % n].fwd)
            else {
                break;
            };
            let k2 = (k + 1) % n;
            ch.remove(k.max(k2));
            ch.remove(k.min(k2));
        }
    }
    // p-curves on the base surface, continuous along each chain.
    let center = {
        let mut bb = Bbox::empty();
        for l in &b.faces[base].loops {
            for c in l {
                let q = co_uv(b, c, false);
                bb.add(&P3::new(q.x, q.y, 0.0));
            }
        }
        let c = bb.center();
        P2::new(c.x, c.y)
    };
    let mut windings = Vec::new();
    for ch in &mut chains {
        let mut at: Option<P2> = None;
        // Start the chain in the base face's chart, at an edge that isn't a pole.
        if let Some(k) = ch.iter().position(|c| !b.edges[c.edge].degenerate) {
            ch.rotate_left(k);
        }
        let first = &ch[0];
        let pc0 = pcurve::pcurve(
            &surf,
            &b.edges[first.edge].curve,
            b.edges[first.edge].t[0],
            b.edges[first.edge].t[1],
            Some(center),
        );
        let (t0, _) = co_range(&b.edges[first.edge], first.fwd);
        let p0 = pc0.point(t0);
        let start = P2::new(p0.x, p0.y);
        for c in ch.iter_mut() {
            let pc = if at.is_none() && !b.edges[c.edge].degenerate {
                pc0.clone()
            } else {
                chain_pc(&surf, b, c, at)
            };
            c.pc = pc;
            at = Some(co_uv(b, c, true));
        }
        // A pole's p-curve runs along the pole from where the edge before it ends to where the
        // one after it starts (a pole the merged face only touches: dropped).
        let n = ch.len();
        if n > 2 {
            for k in 0..n {
                if !b.edges[ch[k].edge].degenerate {
                    continue;
                }
                let (from, to) = (
                    co_uv(b, &ch[(k + n - 1) % n], true),
                    co_uv(b, &ch[(k + 1) % n], false),
                );
                let (t0, t1) = co_range(&b.edges[ch[k].edge], ch[k].fwd);
                let d = (to - from) / (t1 - t0);
                let o = from - d * t0;
                ch[k].pc = Curve::Line {
                    o: P3::new(o.x, o.y, 0.0),
                    d: V3::new(d.x, d.y, 0.0),
                };
            }
            ch.retain(|c| {
                !b.edges[c.edge].degenerate
                    || (co_uv(b, c, true) - co_uv(b, c, false)).norm() > tol::DEGENERATE
            });
            at = ch.last().map(|c| co_uv(b, c, true));
        }
        // (Every edge a pole the face only touches: nothing to merge.)
        let end = at?;
        windings.push(((end.x - start.x) / TAU).round() as i64);
        if surf.v_periodic() && ((end.y - start.y) / TAU).round() as i64 != 0 {
            return None;
        }
    }
    let flip = if sense { 1.0 } else { -1.0 };
    let (mu, mv) = surf.metric();
    let area = |b: &Body, ch: &[Coedge]| -> f64 {
        let mut pts = Vec::new();
        for c in ch {
            let e = &b.edges[c.edge];
            let (t0, t1) = co_range(e, c.fwd);
            let n = match c.pc {
                Curve::Line { .. } => 1,
                _ => 32,
            };
            for i in 0..n {
                let p = c.pc.point(t0 + (t1 - t0) * i as f64 / n as f64);
                pts.push(P2::new(p.x * mu, p.y * mv * flip));
            }
        }
        polygon_area(&pts)
    };
    let wrapping: Vec<usize> = (0..chains.len()).filter(|&i| windings[i] != 0).collect();
    if std::env::var("CADRS_BOOL_DEBUG").is_ok() {
        eprintln!(
            "merge group {g:?}: {} chains, windings {windings:?}",
            chains.len()
        );
    }
    let mut loops: Vec<Vec<Coedge>> = Vec::new();
    let mut holes: Vec<Vec<Coedge>> = Vec::new();
    if wrapping.is_empty() {
        for ch in chains {
            if area(b, &ch) > 0.0 {
                loops.push(ch);
            } else {
                holes.push(ch);
            }
        }
        if loops.len() != 1 {
            return None;
        }
    } else {
        if wrapping.len() != 2 || !surf.u_periodic() {
            return None;
        }
        let (mut i1, mut i2) = (wrapping[0], wrapping[1]);
        if windings[i1] + windings[i2] != 0 || windings[i1].abs() != 1 {
            return None;
        }
        // Seam at a vertex of chain 1 where neither of its edges there runs along the iso-u
        // line (a seam there would lie on that edge: the face's own boundary).
        let along_iso = |c: &Coedge| {
            let e = &b.edges[c.edge];
            let (a, z) = (co_uv(b, c, false).x, co_uv(b, c, true).x);
            let m = c.pc.point(0.5 * (e.t[0] + e.t[1])).x;
            !e.degenerate && (a - z).abs() * mu < tol::POINT && (a - m).abs() * mu < tol::POINT
        };
        // The holes' spans in u (their p-curves run on continuously): a seam at a u inside one
        // (or a whole turn from it) would run through that hole (Dr Browns cap: a window
        // through the dome's seam, the dome's pieces merged again after a union).
        let spans: Vec<(f64, f64)> = (0..chains.len())
            .filter(|&i| i != i1 && i != i2)
            .map(|i| {
                chains[i]
                    .iter()
                    .flat_map(|c| {
                        [co_uv(b, c, false).x, co_uv(b, c, true).x, {
                            let e = &b.edges[c.edge];
                            c.pc.point(0.5 * (e.t[0] + e.t[1])).x
                        }]
                    })
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), u| {
                        (lo.min(u), hi.max(u))
                    })
            })
            .collect();
        let clear = |u: f64| {
            spans.iter().all(|&(lo, hi)| {
                let w = unwrap_near(u, 0.5 * (lo + hi), TAU);
                !(w > lo + tol::ROUNDOFF && w < hi - tol::ROUNDOFF)
            })
        };
        let ok_at = |ch: &[Coedge], k: usize| {
            !along_iso(&ch[k]) && !along_iso(&ch[(k + ch.len() - 1) % ch.len()])
        };
        let good = |ch: &[Coedge]| {
            (0..ch.len())
                .find(|&k| ok_at(ch, k) && clear(co_uv(b, &ch[k], false).x))
                .or_else(|| (0..ch.len()).find(|&k| ok_at(ch, k)))
        };
        // (From the other chain when every vertex of the first has such an edge.)
        if good(&chains[i1]).is_none() && good(&chains[i2]).is_some() {
            std::mem::swap(&mut i1, &mut i2);
        }
        if let Some(k) = good(&chains[i1]) {
            chains[i1].rotate_left(k);
        }
        // No vertex of chain 1 clear of the holes: one made on it where it is.
        if !clear(co_uv(b, &chains[i1][0], false).x) {
            let at = chains[i1]
                .iter()
                .enumerate()
                .filter(|(_, c)| !along_iso(c) && !b.edges[c.edge].degenerate)
                .find_map(|(k, c)| {
                    let e = &b.edges[c.edge];
                    (1..16)
                        .map(|i| e.t[0] + (e.t[1] - e.t[0]) * i as f64 / 16.0)
                        .find(|&t| clear(c.pc.point(t).x))
                        .map(|t| (k, t))
                });
            if let Some((k, t)) = at {
                let c = chains[i1][k].clone();
                let e_id = c.edge;
                split_edge(b, e_id, t);
                let e2 = b.edges.len() - 1;
                let (first, second) = (
                    Coedge {
                        edge: e_id,
                        fwd: c.fwd,
                        pc: c.pc.clone(),
                    },
                    Coedge {
                        edge: e2,
                        fwd: c.fwd,
                        pc: c.pc,
                    },
                );
                let (pa, pb) = if c.fwd {
                    (first, second)
                } else {
                    (second, first)
                };
                chains[i1].splice(k..=k, [pa, pb]);
                chains[i1].rotate_left(k + 1);
            }
        }
        let s1 = b.co_start(&chains[i1][0]);
        let us = co_uv(b, &chains[i1][0], false).x;
        // A vertex of chain 2 at the same u (mod 2π), the nearest along it (a farther one would
        // run the seam over the nearer and the edges between), else split an edge there.
        let v1 = co_uv(b, &chains[i1][0], false).y;
        let mut s2: Option<(usize, usize)> = None; // (coedge index in chain 2 starting there, vertex)
        let mut best = f64::INFINITY;
        for (k, c) in chains[i2].iter().enumerate() {
            let q = co_uv(b, c, false);
            if ((q.x - us) / TAU - ((q.x - us) / TAU).round()).abs() * TAU * mu < tol::CONFUSION
                && (q.y - v1).abs() < best
            {
                best = (q.y - v1).abs();
                s2 = Some((k, b.co_start(c)));
            }
        }
        if s2.is_none() {
            // Find the coedge whose u range contains us (mod 2π) and split its edge there.
            let mut hit = None;
            for (k, c) in chains[i2].iter().enumerate() {
                let (a0, a1) = (co_uv(b, c, false).x, co_uv(b, c, true).x);
                let (lo, hi) = if a0 < a1 { (a0, a1) } else { (a1, a0) };
                let target = unwrap_near(us, (lo + hi) * 0.5, TAU);
                if target > lo + tol::ROUNDOFF_FINE && target < hi - tol::ROUNDOFF_FINE {
                    hit = Some((k, target));
                    break;
                }
            }
            let (k, target) = hit?;
            let c = chains[i2][k].clone();
            let e = &b.edges[c.edge];
            // Solve u(t) = target on the p-curve by bisection.
            let (mut lo, mut hi) = (e.t[0], e.t[1]);
            let f = |t: f64| c.pc.point(t).x - target;
            let flo = f(lo);
            if flo.signum() == f(hi).signum() {
                return None;
            }
            for _ in 0..100 {
                let m = 0.5 * (lo + hi);
                if f(m).signum() == flo.signum() {
                    lo = m;
                } else {
                    hi = m;
                }
            }
            let t = 0.5 * (lo + hi);
            let e_id = c.edge;
            let v = split_edge(b, e_id, t);
            // Rebuild chain 2 with the split coedges (the split updated b.faces, not our chain).
            let e2 = b.edges.len() - 1;
            let pc = c.pc.clone();
            let (first, second) = (
                Coedge {
                    edge: e_id,
                    fwd: c.fwd,
                    pc: pc.clone(),
                },
                Coedge {
                    edge: e2,
                    fwd: c.fwd,
                    pc,
                },
            );
            let (pa, pb) = if c.fwd {
                (first, second)
            } else {
                (second, first)
            };
            chains[i2].splice(k..=k, [pa, pb]);
            s2 = Some((k + 1, v));
        }
        let (k2, s2v) = s2?;
        // Rotate chain 2 to start at s2.
        chains[i2].rotate_left(k2);
        // Re-align p-curves: chain 1 from u_s, chain 2 from where chain 1 ends.
        let realign = |b: &Body, ch: &mut Vec<Coedge>, start: P2| {
            let mut at = Some(start);
            for c in ch.iter_mut() {
                c.pc = chain_pc(&surf, b, c, at);
                at = Some(co_uv(b, c, true));
            }
        };
        let start1 = co_uv(b, &chains[i1][0], false);
        let mut ch1 = chains[i1].clone();
        realign(b, &mut ch1, start1);
        let end1 = co_uv(b, ch1.last()?, true);
        // Seam edge along the iso-u line from s1 to s2.
        let iso = surf.iso_u(us)?;
        let p2 = b.verts[s2v].p;
        let v1 = start1.y;
        let v_near = {
            let (_, v) = surf.project(&p2, Some((us, start1.y)));
            v
        };
        // Where v closes too (a torus), the seam can run either way round from s1 to s2: the
        // nearer may cross what is cut away (an arc's band: through the arc's gap), so the other
        // way is tried when the loop comes out clockwise.
        let candidates: Vec<f64> = if surf.v_periodic() {
            vec![v_near, v_near + if v_near > v1 { -TAU } else { TAU }]
        } else {
            vec![v_near]
        };
        let line_at = |u: f64| Curve::Line {
            o: P3::new(u, 0.0, 0.0),
            d: V3::new(0.0, 1.0, 0.0),
        };
        let mut found: Option<Vec<Coedge>> = None;
        for v2 in candidates {
            let mut ch2 = chains[i2].clone();
            realign(b, &mut ch2, P2::new(end1.x, v2));
            let (lo, hi, vlo, vhi) = if v1 < v2 {
                (s1, s2v, v1, v2)
            } else {
                (s2v, s1, v2, v1)
            };
            b.edges.push(Edge {
                curve: iso.clone(),
                t: [vlo, vhi],
                v: [lo, hi],
                degenerate: false,
                tol: super::topo::TOL_MIN,
            });
            let se = b.edges.len() - 1;
            // From s1 (at u_s + 2πw) to s2, then chain 2, then from s2 (at u_s) back to s1.
            let up_first = v1 < v2; // the seam edge runs s1 → s2
            let mut lp = ch1.clone();
            lp.push(Coedge {
                edge: se,
                fwd: up_first,
                pc: line_at(end1.x),
            });
            lp.extend(ch2);
            lp.push(Coedge {
                edge: se,
                fwd: !up_first,
                pc: line_at(start1.x),
            });
            // The loop must be counter-clockwise in oriented uv.
            if area(b, &lp) > 0.0 {
                found = Some(lp);
                break;
            }
        }
        loops.push(found?);
        for (i, ch) in chains.into_iter().enumerate() {
            if i != i1 && i != i2 {
                holes.push(ch);
            }
        }
    }
    loops.extend(holes);
    Some(Face { surf, sense, loops })
}

/// Merges pairs of edges meeting at a vertex used by no other edge when they lie on one
/// line or circle and bound the same faces.
pub fn merge_edges(b: &mut Body) {
    loop {
        let mut vedges: Vec<Vec<usize>> = vec![Vec::new(); b.verts.len()];
        for (i, e) in b.edges.iter().enumerate() {
            if e.degenerate {
                vedges[e.v[0]].push(usize::MAX);
                continue;
            }
            vedges[e.v[0]].push(i);
            if e.v[1] != e.v[0] {
                vedges[e.v[1]].push(i);
            }
        }
        let ef = b.edge_faces();
        let mut done = false;
        for (v, es) in vedges.iter().enumerate() {
            if es.len() != 2 || es[0] == es[1] || es.contains(&usize::MAX) {
                continue;
            }
            let (e1, e2) = (es[0], es[1]);
            let (a, c) = (b.edges[e1].clone(), b.edges[e2].clone());
            if a.v[0] == a.v[1] || c.v[0] == c.v[1] {
                continue;
            }
            let mut fa = ef[e1].clone();
            let mut fc = ef[e2].clone();
            fa.sort();
            fc.sort();
            if fa != fc {
                continue;
            }
            // Same carrier.
            let same = match (&a.curve, &c.curve) {
                (Curve::Line { o: o1, d: d1 }, Curve::Line { o: o2, d: d2 }) => {
                    d1.normalize().cross(&d2.normalize()).norm() < 1e-10 && {
                        let w = o2 - o1;
                        w.cross(&d1.normalize()).norm() < tol::ANGULAR
                    }
                }
                (
                    Curve::Circle {
                        c: c1,
                        x: x1,
                        y: y1,
                        r: r1,
                    },
                    Curve::Circle {
                        c: c2,
                        x: x2,
                        y: y2,
                        r: r2,
                    },
                ) => {
                    (c1 - c2).norm() < tol::ROUNDOFF
                        && (r1 - r2).abs() < tol::ROUNDOFF
                        && x1.cross(y1).cross(&x2.cross(y2)).norm() < 1e-10
                }
                _ => false,
            };
            if !same {
                continue;
            }
            // The merged edge on a's curve: from a's far end to c's far end through v.
            let far_a = if a.v[0] == v { a.v[1] } else { a.v[0] };
            let far_c = if c.v[0] == v { c.v[1] } else { c.v[0] };
            let curve = a.curve.clone();
            let pc_ = b.verts[far_c].p;
            // Parameters on a's curve, continuous through v.
            let tv = if a.v[0] == v { a.t[0] } else { a.t[1] };
            let ta = if a.v[0] == v { a.t[1] } else { a.t[0] };
            let tc = match &curve {
                Curve::Line { o, d } => (pc_ - o).dot(d) / d.norm_squared(),
                _ => {
                    // Angle on the circle unwrapped away from ta through tv.
                    let raw = curve.closest(&pc_, tv - PI, tv + PI);
                    let dir = (tv - ta).signum();
                    let mut t = unwrap_near(raw, tv, TAU);
                    if (t - tv) * dir <= 0.0 {
                        t += TAU * dir;
                    }
                    t
                }
            };
            let closed = far_a == far_c;
            let (t0, t1, v0, v1) = if ta < tc {
                (ta, tc, far_a, far_c)
            } else {
                (tc, ta, far_c, far_a)
            };
            // (A circle can't run past a whole turn; a line's parameter is a length.)
            if matches!(curve, Curve::Circle { .. }) && (t1 - t0) > TAU + tol::ANGULAR {
                continue;
            }
            let ne = Edge {
                curve: curve.clone(),
                t: [t0, t1],
                v: [v0, v1],
                degenerate: false,
                tol: super::topo::TOL_MIN,
            };
            let _ = closed;
            b.edges[e1] = ne.clone();
            // Replace coedges: in each face, the coedges of e1 and e2 become one of e1.
            for fi in 0..b.faces.len() {
                let surf = b.faces[fi].surf.clone();
                for li in 0..b.faces[fi].loops.len() {
                    for _rep in 0..2 {
                        let l = b.faces[fi].loops[li].clone();
                        let n = l.len();
                        // Find e1 and e2 adjacent in this loop.
                        let mut pos = None;
                        for k in 0..n {
                            let (x, y) = (&l[k], &l[(k + 1) % n]);
                            let x_end = {
                                let ed = if x.edge == e1 { &a } else { &c };
                                if x.fwd { ed.v[1] } else { ed.v[0] }
                            };
                            if ((x.edge == e1 && y.edge == e2) || (x.edge == e2 && y.edge == e1))
                                && x_end == v
                            {
                                pos = Some(k);
                                break;
                            }
                        }
                        let Some(k) = pos else { continue };
                        let first = &l[k];
                        // Direction: the merged coedge starts where `first` starts.
                        let start_v = {
                            let ed = if first.edge == e1 {
                                &a.clone()
                            } else {
                                &c.clone()
                            };
                            if first.fwd { ed.v[0] } else { ed.v[1] }
                        };
                        // p-curve continuous with the old first coedge's start.
                        let old_e = if first.edge == e1 {
                            a.clone()
                        } else {
                            c.clone()
                        };
                        let (ot0, _) = co_range(&old_e, first.fwd);
                        let fwd = if ne.v[0] != ne.v[1] {
                            ne.v[0] == start_v
                        } else {
                            let travel =
                                old_e.curve.tangent(ot0) * if first.fwd { 1.0 } else { -1.0 };
                            ne.curve.tangent(ne.t[0]).dot(&travel) > 0.0
                        };
                        let sp = first.pc.point(ot0);
                        let sp = P2::new(sp.x, sp.y);
                        let mut pc = pc_from(&surf, &ne, fwd, Some(sp));
                        // (Across a seam the surface doesn't call periodic: moved on to `sp`.)
                        let s0 = pc.point(co_range(&ne, fwd).0);
                        if (P2::new(s0.x, s0.y) - sp).norm()
                            > tol::ROUNDOFF * (1.0 + sp.coords.norm())
                            && let Some(moved) = repeat_onto(&surf, &pc, &ne, fwd, sp)
                        {
                            pc = moved;
                        }
                        let mut nl: Vec<Coedge> = Vec::with_capacity(n - 1);
                        for (j, co) in l.iter().enumerate() {
                            if j == k {
                                nl.push(Coedge {
                                    edge: e1,
                                    fwd,
                                    pc: pc.clone(),
                                });
                            } else if j == (k + 1) % n {
                                continue;
                            } else {
                                nl.push(co.clone());
                            }
                        }
                        if k + 1 == n {
                            // The pair wrapped: the first element was removed above; fix order.
                        }
                        b.faces[fi].loops[li] = nl;
                    }
                }
            }
            done = true;
            break;
        }
        if !done {
            break;
        }
        compact(b);
    }
}

/// B-spline edges that are straight lines run at a constant speed (a loft's rails to a point, a
/// skin's between corners) become lines on the same parameters, so their faces' p-curves hold
/// as they are and what meets them sees a line (a chamfer's or a tube's planes and cylinders, not
/// B-splines fitted along them). True when any changed.
pub fn straighten(b: &mut Body) -> bool {
    let mut changed = false;
    for e in &mut b.edges {
        let Curve::Nurbs(n) = &e.curve else { continue };
        if e.degenerate {
            continue;
        }
        let (t0, t1) = (e.t[0], e.t[1]);
        let (p0, p1) = (n.point(t0), n.point(t1));
        let len = (p1 - p0).norm();
        if !(t1 > t0 && len > tol::POINT) {
            continue;
        }
        let d = (p1 - p0) / (t1 - t0);
        let o = p0 - d * t0;
        let close = 1e-12 * (1.0 + len + p0.coords.norm());
        if (1..16).all(|i| {
            let t = t0 + (t1 - t0) * i as f64 / 16.0;
            (n.point(t) - (o + d * t)).norm() <= close
        }) {
            e.curve = Curve::Line { o, d };
            changed = true;
        }
    }
    changed
}

/// B-spline faces that are flat (all control points in one plane: a loft or skin through
/// coplanar curves) become planes, with their p-curves recomputed, so booleans see them
/// coincide with other planar faces. True when any changed.
pub fn planarize(b: &mut Body) -> bool {
    let mut changed = false;
    for fi in 0..b.faces.len() {
        let Surface::Nurbs(ns) = &b.faces[fi].surf else {
            continue;
        };
        let pts: Vec<P3> = ns.cps.iter().flatten().map(super::nurbs::dehom).collect();
        let c = P3::from(pts.iter().fold(V3::zeros(), |a, p| a + p.coords) / pts.len() as f64);
        let size = pts.iter().map(|p| (p - c).norm()).fold(0.0, f64::max);
        let ((u0, u1), (v0, v1)) = ns.range();
        let (um, vm) = (0.5 * (u0 + u1), 0.5 * (v0 + v1));
        let s = &b.faces[fi].surf;
        let (o, su, sv) = s.d1(um, vm);
        let n = su.cross(&sv);
        if n.norm() < tol::REL_FINE * (1.0 + size * size) {
            continue;
        }
        let n = n.normalize();
        // Sampled on the surface (exact for a flat skin), and the control net as well.
        let mut dev = 0.0f64;
        for i in 0..=8 {
            for j in 0..=8 {
                let q = s.eval(
                    u0 + (u1 - u0) * i as f64 / 8.0,
                    v0 + (v1 - v0) * j as f64 / 8.0,
                );
                dev = dev.max((q - o).dot(&n).abs());
            }
        }
        if std::env::var("CADRS_BOOL_DEBUG").is_ok() {
            let net = pts
                .iter()
                .map(|p| (p - o).dot(&n).abs())
                .fold(0.0, f64::max);
            eprintln!("planarize face {fi}: deviation {dev:e} (net {net:e}, size {size})");
        }
        if dev > 1e-10 * (1.0 + size) {
            continue;
        }
        let x = su.normalize();
        let y = n.cross(&x);
        let plane = Surface::Plane {
            f: Frame { o, x, y, z: n },
        };
        let edges = b.edges.clone();
        for l in &mut b.faces[fi].loops {
            for co in l.iter_mut() {
                let e = &edges[co.edge];
                co.pc = pcurve::pcurve(&plane, &e.curve, e.t[0], e.t[1], None);
            }
        }
        b.faces[fi].surf = plane;
        changed = true;
    }
    changed
}

/// B-spline faces that lie on a cylinder (a profile line along a helix's axis swept round it,
/// a skin of circles) become cylinders, with their p-curves and sense recomputed, so booleans
/// see them coincide with the cylinders they lie on. Faces turning a whole way round are left.
/// True when any changed.
pub fn cylindrize(b: &mut Body) -> bool {
    let mut changed = false;
    for fi in 0..b.faces.len() {
        let Surface::Nurbs(ns) = &b.faces[fi].surf else {
            continue;
        };
        let ((u0, u1), (v0, v1)) = ns.range();
        let s = b.faces[fi].surf.clone();
        let n = 12;
        let mut pts: Vec<P3> = Vec::new();
        let mut nrm: Vec<V3> = Vec::new();
        for i in 0..=n {
            for j in 0..=n {
                let (u, v) = (
                    u0 + (u1 - u0) * i as f64 / n as f64,
                    v0 + (v1 - v0) * j as f64 / n as f64,
                );
                let (p, su, sv) = s.d1(u, v);
                let nn = su.cross(&sv);
                if nn.norm() < 1e-14 {
                    continue;
                }
                pts.push(p);
                nrm.push(nn.normalize());
            }
        }
        if pts.len() < 20 {
            continue;
        }
        // The axis: square to every normal.
        let mut m = nalgebra::Matrix3::zeros();
        for q in &nrm {
            m += q * q.transpose();
        }
        let eig = m.symmetric_eigen();
        let k = (0..3)
            .min_by(|&a, &c| eig.eigenvalues[a].total_cmp(&eig.eigenvalues[c]))
            .unwrap();
        let z = eig.eigenvectors.column(k).into_owned().normalize();
        if nrm.iter().any(|q| q.dot(&z).abs() > 1e-7) {
            continue;
        }
        // Not a plane: the normals must turn.
        let spread = nrm.iter().map(|q| q.dot(&nrm[0])).fold(1.0, f64::min);
        if spread > 1.0 - tol::ANGULAR_POINT {
            continue;
        }
        // The circle in the plane square to z (algebraic fit), then the check.
        let x0 = super::math::Frame::from_z(P3::origin(), z);
        let loc: Vec<(f64, f64)> = pts
            .iter()
            .map(|p| (p.coords.dot(&x0.x), p.coords.dot(&x0.y)))
            .collect();
        let mut a = nalgebra::Matrix3::zeros();
        let mut rhs = V3::zeros();
        for &(x, y) in &loc {
            let row = V3::new(x, y, 1.0);
            a += row * row.transpose();
            rhs += row * (x * x + y * y);
        }
        let Some(sol) = a.lu().solve(&rhs) else {
            continue;
        };
        let (cx, cy) = (sol.x / 2.0, sol.y / 2.0);
        let r = (sol.z + cx * cx + cy * cy).sqrt();
        if !(r.is_finite() && r > tol::ROUNDOFF) {
            continue;
        }
        let axis_o = P3::from(x0.x * cx + x0.y * cy);
        let dist = |p: &P3| {
            let w = p - axis_o;
            (w - z * w.dot(&z)).norm()
        };
        let mut ok = true;
        for i in 0..=40 {
            for j in 0..=40 {
                let p = s.eval(
                    u0 + (u1 - u0) * i as f64 / 40.0,
                    v0 + (v1 - v0) * j as f64 / 40.0,
                );
                if (dist(&p) - r).abs() > tol::REL * (1.0 + r) {
                    ok = false;
                }
            }
        }
        if !ok {
            continue;
        }
        // The frame: the face's middle at u = π (its seam on the far side).
        let mid = s.eval(0.5 * (u0 + u1), 0.5 * (v0 + v1));
        let w = mid - axis_o;
        let radial = (w - z * w.dot(&z)).normalize();
        let fx = -radial;
        let fy = z.cross(&fx);
        let cyl = Surface::Cylinder {
            f: Frame {
                o: axis_o,
                x: fx,
                y: fy,
                z,
            },
            r,
        };
        // Angular extent of the face (from its edges): less than a turn.
        let edges = b.edges.clone();
        let mut ang = (f64::INFINITY, f64::NEG_INFINITY);
        for co in b.faces[fi].loops.iter().flatten() {
            let e = &edges[co.edge];
            for i in 0..=8 {
                let (u, _) = cyl.project(
                    &e.point(e.t[0] + (e.t[1] - e.t[0]) * i as f64 / 8.0),
                    Some((PI, 0.0)),
                );
                ang = (ang.0.min(u), ang.1.max(u));
            }
        }
        if !(ang.1 - ang.0 < TAU - tol::ANGULAR_POINT) {
            continue;
        }
        let mid_uv = cyl.project(&mid, Some((PI, 0.0)));
        for l in &mut b.faces[fi].loops {
            for co in l.iter_mut() {
                let e = &edges[co.edge];
                co.pc = pcurve::pcurve(
                    &cyl,
                    &e.curve,
                    e.t[0],
                    e.t[1],
                    Some(P2::new(mid_uv.0, mid_uv.1)),
                );
            }
        }
        b.faces[fi].surf = cyl;
        super::build::fix_senses(b, [fi]);
        changed = true;
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::super::kernel::NativeKernel;
    use crate::*;
    use nalgebra::Point2;

    /// Two prisms of a profile with a Bézier side stacked one on the other and united: one
    /// prism's faces (regression: the Bézier side stayed two faces, extrusions of one curve
    /// shifted along their direction not being merged).
    #[test]
    fn stacked_spline_prisms_merge() {
        let mut k = NativeKernel::new();
        let p = |x: f64, y: f64| Point2::new(x, y);
        let bez = Curve2::Bezier {
            poles: [p(0.0, 0.0), p(0.0, 10.0), p(20.0, 10.0), p(20.0, 0.0)],
            source: None,
        };
        let lp = Loop {
            curves: vec![
                Curve2::Line {
                    a: p(20.0, 0.0),
                    b: p(0.0, 0.0),
                    source: None,
                },
                bez,
            ],
        };
        let region = Region {
            outer: lp,
            holes: vec![],
            source: None,
        };
        let mut upper = Plane::top();
        upper.origin.z = 5.0;
        let a = k
            .extrude(
                &Profile::new(Plane::top(), vec![region.clone()]),
                Extent::Blind(5.0),
            )
            .unwrap()
            .bodies[0];
        let b = k
            .extrude(&Profile::new(upper, vec![region]), Extent::Blind(5.0))
            .unwrap()
            .bodies[0];
        let u = k.boolean(BoolOp::Union, a, &[b]).unwrap().bodies[0];
        let body = k.get(u).unwrap();
        assert_eq!(body.check(), Vec::<String>::new());
        assert_eq!(
            body.faces.len(),
            4,
            "{:?}",
            body.faces.iter().map(|f| f.surf.kind()).collect::<Vec<_>>()
        );
        assert!((k.mass_properties(u).unwrap().volume - 1200.0).abs() < 1e-6);
    }
}
