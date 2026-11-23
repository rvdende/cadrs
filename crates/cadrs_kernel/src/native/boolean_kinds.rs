//! Booleans of bodies other than solids: sheets, wires and acorn points (Onshape's body kinds,
//! [`BodyClass`]), with each other and with solids.
//!
//! Solid with solid is [`super::boolean`]'s. The others are built from it and from
//! [`super::edge_bop`]:
//! - **Sheets** are split by the other body with [`boolean::imprint`] (the faces of one body
//!   cut along where the other meets them, every piece kept), and the pieces chosen by a point
//!   inside each: against a solid by [`classify::classify_point`] (sheet ∩ solid keeps the
//!   pieces inside or on it, sheet − solid those outside); against a sheet by whether the
//!   point lies on one of its faces (sheet ∩ sheet keeps the overlap, sheet − sheet the rest,
//!   a split along the other sheet where they only cross). Sheet ∪ sheet keeps every piece of
//!   both but the second's pieces lying on the first, sewn along their shared edges and merged
//!   where they lie on one surface; where three or more faces would meet at an edge (crossing
//!   sheets) the sheets aren't joined, so every shell stays manifold (OCCT's result is a
//!   compound of the split sheets: the same faces).
//! - **Solid − sheet** is a split: the solid's faces imprinted by the sheet, plus the sheet's
//!   pieces inside the solid twice, once facing each way; [`boolean::assemble_shells`] pairs
//!   the four faces at each crossing edge by angle, so each side closes into its own solid.
//!   Pieces of the sheet that don't reach across (a free edge inside the solid) are left out.
//! - **Wires** go through [`super::edge_bop`]: their edges split where they cross or touch the
//!   other body and the pieces classified (common: in or on it; cut: outside). A wire ∩ sheet
//!   or wire ∩ wire also gives the points where they cross (acorns). Wire ∪ wire keeps every
//!   piece of both, shared stretches once. The pieces are chained again by
//!   [`Body::chain_wires`].
//! - **Points**: common keeps the points in or on the other body, cut those off it; point ∪
//!   point keeps both sets (coincident points once).
//! - A cut by a lower-dimensional body (solid − wire, sheet − point, …) leaves the target
//!   as it was; a common with one is symmetric (solid ∩ wire = wire ∩ solid).
//!
//! The result's kind ([`BodyClass`]) per operation, `a op b`:
//!
//! | a \ b  | Solid                  | Sheet                  | Wire          | Point |
//! |--------|------------------------|------------------------|---------------|-------|
//! | Solid  | ∪ ∩ − Solid            | ∩ Sheet, − Solid       | ∩ Wire, − a   | ∩ Point, − a |
//! | Sheet  | ∩ − Sheet              | ∪ ∩ − Sheet            | ∩ Wire/Point/Mixed, − a | ∩ Point, − a |
//! | Wire   | ∩ − Wire               | ∩ Wire/Point/Mixed, − Wire | ∪ ∩ − Wire (∩ also Point/Mixed) | ∩ Point, − a |
//! | Point  | ∩ − Point              | ∩ − Point              | ∩ − Point     | ∪ ∩ − Point |
//!
//! A union of bodies of different kinds is refused (`Unsupported`: a non-manifold cell
//! complex), as is any boolean of a `Mixed` body; a result with nothing left is
//! [`KernelError::EmptyResult`], as for solids.

use super::boolean::{self, BoolOut};
use super::classify::{self, InFace};
use super::edge_bop::{Other, Piece, edge_boolean};
use super::kernel::NativeKernel;
use super::math::*;
use super::pcurve;
use super::topo::{Body, Coedge, co_range};
use crate::*;

/// The distance under which two points of the kinds' booleans are one: the boolean's default
/// tolerance ([`boolean::VTOL`]) with the margin its merged entities carry.
fn tol() -> f64 {
    boolean::VTOL * 10.0
}

/// The kernel operation when an input isn't a solid: the target combined with each tool in
/// turn (as [`boolean::boolean`]), None when every input is a solid.
pub fn boolean(
    k: &mut NativeKernel,
    op: BoolOp,
    target: BodyId,
    tools: &[BodyId],
    opts: boolean::BoolOptions,
) -> Option<Result<OpResult>> {
    let mut inputs = vec![target];
    inputs.extend_from_slice(tools);
    let all_solid = inputs.iter().all(|&b| {
        k.get(b)
            .map(|x| kind(x) == BodyClass::Solid)
            .unwrap_or(true)
    });
    if all_solid {
        return None;
    }
    Some(run(k, op, target, tools, opts))
}

fn run(
    k: &mut NativeKernel,
    op: BoolOp,
    target: BodyId,
    tools: &[BodyId],
    opts: boolean::BoolOptions,
) -> Result<OpResult> {
    let mut cur = (**k.get(target)?).clone();
    let mut origin: Vec<Vec<(BodyId, usize)>> =
        (0..cur.faces.len()).map(|i| vec![(target, i)]).collect();
    for &t in tools {
        let tb = std::sync::Arc::clone(k.get(t)?);
        let r = combine(&cur, &tb, op, opts)?;
        origin = r
            .from
            .iter()
            .map(|srcs| {
                srcs.iter()
                    .flat_map(|&(o, f)| {
                        if o == 0 {
                            origin[f].clone()
                        } else {
                            vec![(t, f)]
                        }
                    })
                    .collect()
            })
            .collect();
        cur = r.body;
    }
    let mut history = History::default();
    for (i, srcs) in origin.iter().enumerate() {
        for (bd, f) in srcs {
            history.modified.push((
                FaceId(i as u64),
                InputFace {
                    body: *bd,
                    face: FaceId(*f as u64),
                },
            ));
        }
    }
    let mut inputs = vec![target];
    inputs.extend_from_slice(tools);
    for &ib in &inputs {
        let n = k.get(ib)?.faces.len();
        for f in 0..n {
            if !origin.iter().flatten().any(|(b, g)| *b == ib && *g == f) {
                history.deleted.push(InputFace {
                    body: ib,
                    face: FaceId(f as u64),
                });
            }
        }
    }
    let id = k.insert(cur);
    Ok(OpResult {
        bodies: vec![id],
        history,
    })
}

/// `a op b` for bodies of any kinds; `from` as [`boolean::boolean_bodies`] gives it (owner 0
/// is `a`, 1 is `b`).
pub fn combine(a: &Body, b: &Body, op: BoolOp, opts: boolean::BoolOptions) -> Result<BoolOut> {
    use BodyClass as C;
    let (ca, cb) = (kind(a), kind(b));
    let same = |body: &Body| BoolOut {
        body: body.clone(),
        from: (0..body.faces.len()).map(|i| vec![(0, i)]).collect(),
    };
    let empty = || Err(KernelError::EmptyResult("the result is empty".into()));
    if ca == C::Mixed || cb == C::Mixed {
        return Err(KernelError::Unsupported(
            "booleans of bodies holding several kinds of pieces (split them first)",
        ));
    }
    match (ca, cb) {
        (C::Solid, C::Solid) => boolean::boolean_bodies_with(a, b, op, opts),
        (_, C::Empty) => match op {
            BoolOp::Intersect => empty(),
            _ => Ok(same(a)),
        },
        (C::Empty, _) => match op {
            BoolOp::Union => Ok(BoolOut {
                body: b.clone(),
                from: (0..b.faces.len()).map(|i| vec![(1, i)]).collect(),
            }),
            _ => empty(),
        },
        (x, y) if op == BoolOp::Union && x != y => Err(KernelError::Unsupported(
            "a union of bodies of different kinds (a solid with a sheet, a sheet with a wire, …)",
        )),
        // A cut by a body of a lower kind leaves the target alone.
        (x, y)
            if op == BoolOp::Subtract && rank(y) < rank(x) && !(x == C::Solid && y == C::Sheet) =>
        {
            Ok(same(a))
        }
        // A common with a body of a lower kind is that body's common with it.
        (x, y) if op == BoolOp::Intersect && rank(y) < rank(x) => {
            let r = combine(b, a, op, opts)?;
            Ok(BoolOut {
                body: r.body,
                from: r
                    .from
                    .into_iter()
                    .map(|s| s.into_iter().map(|(o, f)| (1 - o, f)).collect())
                    .collect(),
            })
        }
        (C::Solid, C::Sheet) => solid_minus_sheet(a, b),
        (C::Sheet, C::Solid) => sheet_with(a, b, op, true),
        (C::Sheet, C::Sheet) if op == BoolOp::Union => sheet_union(a, b),
        (C::Sheet, C::Sheet) => sheet_with(a, b, op, false),
        (C::Wire, _) => wire_with(a, b, op),
        (C::Point, _) => points_with(a, b, op),
        _ => Err(KernelError::Unsupported(
            "this boolean of bodies of different kinds",
        )),
    }
}

/// What a body is for a boolean: its [`Body::class`], but faces closing up (every edge used
/// once each way) count as a solid whether or not they are listed as one, and faces with
/// solids as solids (the solid boolean has always taken them so).
fn kind(b: &Body) -> BodyClass {
    let c = b.class();
    if !b.wires.is_empty() || !b.points.is_empty() || b.faces.is_empty() {
        return c;
    }
    if !b.solids.is_empty() {
        return BodyClass::Solid;
    }
    let mut uses = vec![0i32; b.edges.len()];
    let mut count = vec![0u32; b.edges.len()];
    for c in b.faces.iter().flat_map(|f| f.loops.iter().flatten()) {
        uses[c.edge] += if c.fwd { 1 } else { -1 };
        count[c.edge] += 1;
    }
    let closed = (0..b.edges.len())
        .all(|e| b.edges[e].degenerate || count[e] == 0 || (count[e] == 2 && uses[e] == 0));
    if closed {
        BodyClass::Solid
    } else {
        BodyClass::Sheet
    }
}

/// The dimension of a kind's pieces.
fn rank(c: BodyClass) -> u8 {
    match c {
        BodyClass::Solid => 3,
        BodyClass::Sheet => 2,
        BodyClass::Wire => 1,
        _ => 0,
    }
}

/// Every face's sampled loops.
fn polys_of(b: &Body) -> Vec<Vec<Vec<P2>>> {
    b.faces
        .iter()
        .map(|f| classify::face_polygons(b, f))
        .collect()
}

/// A few points inside face `f`, farthest from its boundary first.
fn inside_points(b: &Body, f: usize) -> Vec<P3> {
    let face = &b.faces[f];
    let polys = classify::face_polygons(b, face);
    boolean::interior_points_of_polys(&polys)
        .into_iter()
        .take(6)
        .map(|q| face.surf.eval(q.x, q.y))
        .collect()
}

/// Whether `p` lies on a face of `b` (within `tol`).
fn on_faces(b: &Body, polys: &[Vec<Vec<P2>>], p: &P3, tol: f64) -> bool {
    b.faces.iter().enumerate().any(|(fi, f)| {
        let (u, v) = f.surf.project(p, None);
        (f.surf.eval(u, v) - p).norm() <= tol
            && classify::periodic_candidates(&f.surf, u, v, &polys[fi])
                .into_iter()
                .any(|uv| classify::in_face(&polys[fi], &f.surf, uv, tol) != InFace::Outside)
    })
}

/// Where a piece of a sheet lies against the other body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    In,
    On,
    Out,
}

/// The side of face `f` of `pieces` against `other`: a solid (`solid`) by point classification
/// (the first of its inside points not on the solid's boundary decides; all on it: On), a
/// sheet by whether its inside point lies on one of the sheet's faces.
fn side_of(pieces: &Body, f: usize, other: &Body, polys: &[Vec<Vec<P2>>], solid: bool) -> Side {
    let pts = inside_points(pieces, f);
    if !solid {
        return match pts.first() {
            Some(p) if on_faces(other, polys, p, tol()) => Side::On,
            _ => Side::Out,
        };
    }
    for p in &pts {
        match classify::classify_point(other, polys, p, tol()) {
            PointClass::Inside => return Side::In,
            PointClass::Outside => return Side::Out,
            PointClass::OnBoundary => {}
        }
    }
    Side::On
}

/// The faces `keep` of `b` (an imprint's pieces, `from` per face) as a body of their own. The
/// pieces stay apart (a sheet cut by a sheet it crosses is split there).
fn keep_faces(b: &Body, from: &[Vec<(usize, usize)>], keep: &[usize]) -> Result<BoolOut> {
    if keep.is_empty() {
        return Err(KernelError::EmptyResult("the result is empty".into()));
    }
    let mut out = super::kernel::subset(b, keep, false);
    let from: Vec<Vec<(usize, usize)>> = keep.iter().map(|&f| from[f].clone()).collect();
    finish_sheet(&mut out, from, false)
}

/// A sheet's last steps: with `unify`, faces on one surface merged where the rule allows
/// ([`super::unify::unify_boolean`]); edges on one curve merged; shells assembled anew.
fn finish_sheet(out: &mut Body, from: Vec<Vec<(usize, usize)>>, unify: bool) -> Result<BoolOut> {
    let from = if unify {
        super::unify::unify_boolean(out, from, false)
    } else {
        from
    };
    super::unify::merge_edges(out);
    out.shells.clear();
    out.solids.clear();
    boolean::assemble_shells(out)?;
    Ok(BoolOut {
        body: std::mem::take(out),
        from,
    })
}

/// Sheet `a` ∩ / − `b` (a solid when `solid`, else a sheet): `a`'s pieces split along `b`,
/// those inside or on `b` for a common, the others for a cut.
fn sheet_with(a: &Body, b: &Body, op: BoolOp, solid: bool) -> Result<BoolOut> {
    let im = boolean::imprint(a, b, None)?;
    let polys = polys_of(b);
    let keep: Vec<usize> = (0..im.body.faces.len())
        .filter(|&f| {
            let side = side_of(&im.body, f, b, &polys, solid);
            match op {
                BoolOp::Intersect => side != Side::Out,
                _ => side == Side::Out,
            }
        })
        .collect();
    keep_faces(&im.body, &im.from, &keep)
}

/// Sheet `a` ∪ sheet `b`: all of `a`'s pieces, and `b`'s not lying on `a`, sewn together
/// where their boundaries meet. Where three or more faces would meet at an edge (the sheets
/// crossing, or one ending on the other) they aren't joined: each sheet keeps its own copy of
/// the edge (and its pieces stay split there), so every shell of the result is a manifold
/// sheet; a sheet that already had such an edge is refused.
fn sheet_union(a: &Body, b: &Body) -> Result<BoolOut> {
    let ia = boolean::imprint(a, b, None)?;
    let ib = boolean::imprint(b, a, None)?;
    let polys_a = polys_of(a);
    let mut out = ia.body.clone();
    let mut from = ia.from.clone();
    let keep_b: Vec<usize> = (0..ib.body.faces.len())
        .filter(|&f| side_of(&ib.body, f, a, &polys_a, false) == Side::Out)
        .collect();
    let part = super::kernel::subset(&ib.body, &keep_b, false);
    out.append(&part);
    from.extend(
        keep_b
            .iter()
            .map(|&f| ib.from[f].iter().map(|&(_, g)| (1, g)).collect::<Vec<_>>()),
    );
    out.shells.clear();
    fuse(&mut out, tol());
    let owner: Vec<usize> = from.iter().map(|f| f[0].0).collect();
    let mut crossing = vec![false; out.edges.len()];
    let ef = out.edge_faces();
    for e in 0..ef.len() {
        if out.edges[e].degenerate || ef[e].len() <= 2 {
            continue;
        }
        crossing[e] = true;
        out.edges.push(out.edges[e].clone());
        crossing.push(true);
        let copy = out.edges.len() - 1;
        for (fi, f) in out.faces.iter_mut().enumerate() {
            if owner[fi] == 1 {
                for c in f.loops.iter_mut().flatten().filter(|c| c.edge == e) {
                    c.edge = copy;
                }
            }
        }
    }
    let ef = out.edge_faces();
    if (0..out.edges.len()).any(|e| !out.edges[e].degenerate && ef[e].len() > 2) {
        return Err(KernelError::BooleanFailed { stage: "sheet union", why: "a sheet of the union has three or more faces meeting at an edge (not a manifold sheet)".into() });
    }
    // Faces split along a crossing stay split.
    let split: std::collections::HashSet<(usize, usize)> = (0..out.edges.len())
        .filter(|&e| crossing[e] && ef[e].len() == 2)
        .map(|e| (ef[e][0].min(ef[e][1]), ef[e][0].max(ef[e][1])))
        .collect();
    super::surfacing::orient(&mut out)?;
    let src: Vec<(usize, usize)> = from.iter().map(|f| f[0]).collect();
    let from = super::unify::unify_faces(&mut out, from, &|f, g| {
        (src[f].0 != src[g].0 || src[f] == src[g]) && !split.contains(&(f.min(g), f.max(g)))
    });
    finish_sheet(&mut out, from, false)
}

/// Solid `a` − sheet `b`: `a` split along the sheet's pieces that cross it.
fn solid_minus_sheet(a: &Body, b: &Body) -> Result<BoolOut> {
    let ia = boolean::imprint(a, b, None)?;
    let is = boolean::imprint(b, a, None)?;
    let polys_a = polys_of(a);
    let inside: Vec<usize> = (0..is.body.faces.len())
        .filter(|&f| side_of(&is.body, f, a, &polys_a, true) == Side::In)
        .collect();
    let unchanged = || BoolOut {
        body: a.clone(),
        from: (0..a.faces.len()).map(|i| vec![(0, i)]).collect(),
    };
    if inside.is_empty() {
        return Ok(unchanged());
    }
    let na = ia.body.faces.len();
    let mut out = ia.body.clone();
    out.shells.clear();
    out.solids.clear();
    let mut from = ia.from.clone();
    let sheet = super::kernel::subset(&is.body, &inside, false);
    let mut flipped = sheet.clone();
    for f in &mut flipped.faces {
        super::topo::Body::reverse_face(f);
    }
    // Piece k of the sheet is face na + k and, facing the other way, na + n + k.
    let n = inside.len();
    for part in [&sheet, &flipped] {
        out.append(part);
        from.extend(
            inside
                .iter()
                .map(|&f| is.from[f].iter().map(|&(_, g)| (1, g)).collect::<Vec<_>>()),
        );
    }
    out.shells.clear();
    fuse(&mut out, tol());
    // Pieces with an edge that no face of the solid and no other piece uses stop inside the
    // solid: they cut nothing through (dropped, both copies, until none is left).
    let mut gone = vec![false; out.faces.len()];
    loop {
        let mut users: Vec<Vec<usize>> = vec![Vec::new(); out.edges.len()];
        for (fi, f) in out.faces.iter().enumerate() {
            if gone[fi] {
                continue;
            }
            for c in f.loops.iter().flatten() {
                if !users[c.edge].contains(&fi) {
                    users[c.edge].push(fi);
                }
            }
        }
        let piece = |f: usize| (f >= na).then(|| (f - na) % n);
        let mut changed = false;
        for (e, us) in users.iter().enumerate() {
            if out.edges[e].degenerate || us.is_empty() {
                continue;
            }
            let pieces: Vec<Option<usize>> = us.iter().map(|&f| piece(f)).collect();
            if let Some(Some(k)) = pieces.first()
                && pieces.iter().all(|p| *p == Some(*k))
            {
                gone[na + k] = true;
                gone[na + n + k] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    if (0..n).all(|k| gone[na + k]) {
        return Ok(unchanged());
    }
    let keep: Vec<usize> = (0..out.faces.len()).filter(|&f| !gone[f]).collect();
    out.faces = keep.iter().map(|&f| out.faces[f].clone()).collect();
    let from: Vec<Vec<(usize, usize)>> = keep.iter().map(|&f| from[f].clone()).collect();
    super::unify::compact(&mut out);
    pair_at_cuts(&mut out, &from);
    boolean::assemble_shells(&mut out)?;
    super::unify::split_shared_vertices(&mut out);
    if crate::native::env_flag!("CADRS_BOOL_DEBUG") {
        let ef = out.edge_faces();
        eprintln!(
            "split by a sheet: {} faces, shells {:?}, edge uses {:?}",
            out.faces.len(),
            out.shells
                .iter()
                .map(|s| (s.faces.len(), s.closed))
                .collect::<Vec<_>>(),
            ef.iter().map(|f| f.len()).collect::<Vec<_>>()
        );
    }
    if out.shells.iter().any(|s| !s.closed) || out.solids.is_empty() {
        return Err(KernelError::OpenResult(
            "the split by the sheet left an open shell".into(),
        ));
    }
    Ok(BoolOut { body: out, from })
}

/// At each edge where the sheet's two copies meet two pieces of the solid (owner 0 in
/// `from`), the edge is split in two: each copy keeps it with the solid piece on its material
/// side (behind its outward normal), so each side's faces close up on their own. (Pairing by
/// angle can't order the two copies: they lie on one surface.)
fn pair_at_cuts(b: &mut Body, from: &[Vec<(usize, usize)>]) {
    let ef = b.edge_faces();
    for (e, users) in ef.iter().enumerate() {
        if users.len() != 4 || b.edges[e].degenerate {
            continue;
        }
        let (solid, sheet): (Vec<usize>, Vec<usize>) =
            users.iter().partition(|&&f| from[f][0].0 == 0);
        if solid.len() != 2 || sheet.len() != 2 {
            continue;
        }
        let m = b.edges[e].point(b.edges[e].mid_t());
        let copy = sheet[0];
        let f = &b.faces[copy];
        let (u, v) = f.surf.project(&m, None);
        let n = f.surf.normal(u, v) * if f.sense { 1.0 } else { -1.0 };
        let side = |g: usize| inside_points(b, g).first().map_or(0.0, |p| (p - m).dot(&n));
        let (s0, s1) = (side(solid[0]), side(solid[1]));
        if s0 * s1 >= 0.0 {
            continue;
        }
        // The copy's material is behind it: the solid piece there goes with it.
        let mate = if s0 < 0.0 { solid[0] } else { solid[1] };
        b.edges.push(b.edges[e].clone());
        let ne = b.edges.len() - 1;
        for g in [copy, mate] {
            for c in b.faces[g]
                .loops
                .iter_mut()
                .flatten()
                .filter(|c| c.edge == e)
            {
                c.edge = ne;
            }
        }
    }
}

/// Merges vertices of `b` closer than `tol` and edges between the same vertices that run
/// along each other (any number of them: four faces meet at a split's crossing edge), the
/// coedges moved onto the edge kept with their p-curves recomputed on it.
pub fn fuse(b: &mut Body, tol: f64) {
    // Vertices.
    let nv = b.verts.len();
    let mut vmap: Vec<usize> = (0..nv).collect();
    let point_box = |p: &P3| {
        let mut bb = Bbox::empty();
        bb.add(p);
        bb
    };
    let tree = super::bvh::Bvh::new(b.verts.iter().map(|v| point_box(&v.p)).collect());
    for i in 0..nv {
        if vmap[i] != i {
            continue;
        }
        let mut near = tree.query(&point_box(&b.verts[i].p).grow(tol));
        near.sort_unstable();
        for j in near.into_iter().filter(|&j| j > i) {
            if vmap[j] == j && (b.verts[i].p - b.verts[j].p).norm() <= tol {
                vmap[j] = i;
            }
        }
    }
    for e in &mut b.edges {
        e.v = [vmap[e.v[0]], vmap[e.v[1]]];
    }
    for p in &mut b.points {
        *p = vmap[*p];
    }
    // Edges: grouped by their ends, each merged into the first it runs along.
    let ne = b.edges.len();
    let mut groups: std::collections::HashMap<(usize, usize), Vec<usize>> = Default::default();
    for (i, e) in b.edges.iter().enumerate() {
        if !e.degenerate {
            groups
                .entry((e.v[0].min(e.v[1]), e.v[0].max(e.v[1])))
                .or_default()
                .push(i);
        }
    }
    let mut emap: Vec<(usize, bool)> = (0..ne).map(|i| (i, true)).collect();
    let mut keys: Vec<_> = groups.keys().copied().collect();
    keys.sort_unstable();
    for key in keys {
        let g = &groups[&key];
        for (x, &i) in g.iter().enumerate() {
            if emap[i].0 != i {
                continue;
            }
            for &j in &g[x + 1..] {
                if emap[j].0 != j {
                    continue;
                }
                let (a, c) = (&b.edges[i], &b.edges[j]);
                let along = c.curve.runs_along(
                    (c.t[0], c.t[1]),
                    &a.curve,
                    (a.t[0].min(a.t[1]), a.t[0].max(a.t[1])),
                    [0.25, 0.5, 0.75],
                    tol,
                    false,
                );
                if !along {
                    continue;
                }
                let fwd = if a.v[0] != a.v[1] {
                    a.v == c.v
                } else {
                    let m = c.point(c.mid_t());
                    let t = a.curve.closest(&m, a.t[0].min(a.t[1]), a.t[0].max(a.t[1]));
                    a.curve.tangent(t).dot(&c.curve.tangent(c.mid_t())) > 0.0
                };
                emap[j] = (i, fwd);
            }
        }
    }
    let edges = b.edges.clone();
    for f in &mut b.faces {
        let surf = f.surf.clone();
        for c in f.loops.iter_mut().flatten() {
            let (x, same) = emap[c.edge];
            if x == c.edge {
                continue;
            }
            let (old, ed) = (&edges[c.edge], &edges[x]);
            let fwd = if same { c.fwd } else { !c.fwd };
            let (ot0, _) = co_range(old, c.fwd);
            let sp = c.pc.point(ot0);
            let mut pc = pcurve::pcurve(
                &surf,
                &ed.curve,
                ed.t[0],
                ed.t[1],
                Some(P2::new(sp.x, sp.y)),
            );
            let (nt0, _) = co_range(ed, fwd);
            let q = pc.point(nt0);
            let du = if surf.u_periodic() {
                ((sp.x - q.x) / TAU).round() * TAU
            } else {
                0.0
            };
            let dv = if surf.v_periodic() {
                ((sp.y - q.y) / TAU).round() * TAU
            } else {
                0.0
            };
            if du != 0.0 || dv != 0.0 {
                pc = pc.transformed(&Affine::translation(V3::new(du, dv, 0.0)));
            }
            *c = Coedge { edge: x, fwd, pc };
        }
    }
    for e in &mut b.edges {
        e.tol = e.tol.max(tol);
    }
    super::unify::compact(b);
}

/// The edges of a wire body as pieces.
fn pieces_of(b: &Body) -> Vec<Piece> {
    b.free_edges()
        .into_iter()
        .map(|e| Piece {
            curve: b.edges[e].curve.clone(),
            t0: b.edges[e].t[0],
            t1: b.edges[e].t[1],
        })
        .collect()
}

/// A wire-and-point body: `pieces` (ends within `tol` one vertex) chained into wires, and the
/// `points` that lie on none of them as acorns.
fn wire_body(pieces: &[Piece], points: &[P3], tol: f64) -> Result<BoolOut> {
    let mut b = Body::default();
    let vertex = |b: &mut Body, p: P3| -> usize {
        match b.verts.iter().position(|v| (v.p - p).norm() <= tol) {
            Some(v) => v,
            None => b.add_vertex(p),
        }
    };
    for pc in pieces {
        let (p0, p1) = (pc.curve.point(pc.t0), pc.curve.point(pc.t1));
        let len = pc.curve.length(pc.t0.min(pc.t1), pc.t0.max(pc.t1));
        if len <= tol {
            continue;
        }
        let (v0, v1) = (vertex(&mut b, p0), vertex(&mut b, p1));
        if pc.t0 <= pc.t1 {
            b.add_edge(pc.curve.clone(), pc.t0, pc.t1, v0, v1);
        } else {
            b.add_edge(pc.curve.clone(), pc.t1, pc.t0, v1, v0);
        }
    }
    let on_edge = |b: &Body, p: &P3| {
        b.edges.iter().any(|e| {
            let (lo, hi) = (e.t[0].min(e.t[1]), e.t[0].max(e.t[1]));
            (e.curve.point(e.curve.closest(p, lo, hi)) - p).norm() <= tol
        })
    };
    for p in points {
        if !on_edge(&b, p) && !b.points.iter().any(|&v| (b.verts[v].p - p).norm() <= tol) {
            b.add_point(*p);
        }
    }
    if b.edges.is_empty() && b.points.is_empty() {
        return Err(KernelError::EmptyResult("the result is empty".into()));
    }
    b.chain_wires();
    b.fit_tolerances(tol);
    Ok(BoolOut {
        body: b,
        from: Vec::new(),
    })
}

/// Where the pieces cross the faces of `other` (the points inside the faces).
fn crossings_with_faces(pieces: &[Piece], other: &Body, tol: f64) -> Vec<P3> {
    let polys = polys_of(other);
    let mut out = Vec::new();
    for pc in pieces {
        let (lo, hi) = (pc.t0.min(pc.t1), pc.t0.max(pc.t1));
        for (fi, f) in other.faces.iter().enumerate() {
            for t in super::ssi::curve_surface(&pc.curve, lo, hi, &f.surf) {
                let p = pc.curve.point(t);
                let (u, v) = f.surf.project(&p, None);
                if (f.surf.eval(u, v) - p).norm() > tol {
                    continue;
                }
                if classify::periodic_candidates(&f.surf, u, v, &polys[fi])
                    .into_iter()
                    .any(|uv| classify::in_face(&polys[fi], &f.surf, uv, tol) != InFace::Outside)
                {
                    out.push(p);
                }
            }
        }
    }
    out
}

/// Where two sets of pieces cross.
fn crossings_with_pieces(a: &[Piece], b: &[Piece], tol: f64) -> Vec<P3> {
    let mut out = Vec::new();
    for x in a {
        for y in b {
            let (xl, xh) = (x.t0.min(x.t1), x.t0.max(x.t1));
            let (yl, yh) = (y.t0.min(y.t1), y.t0.max(y.t1));
            for (t, _) in super::ssi::curve_curve(&x.curve, xl, xh, &y.curve, yl, yh, tol) {
                out.push(x.curve.point(t));
            }
        }
    }
    out
}

/// Wire `a` op `b` (any kind but a mixed one).
fn wire_with(a: &Body, b: &Body, op: BoolOp) -> Result<BoolOut> {
    let mine = pieces_of(a);
    let size = a.bbox().diagonal().max(b.bbox().diagonal()).max(tol::POINT);
    let t = (size * 1e-10).max(tol());
    let theirs;
    let other = match kind(b) {
        BodyClass::Wire => {
            theirs = pieces_of(b);
            Other::Edges(&theirs)
        }
        BodyClass::Point => {
            // A cut by points leaves the wire; a common is the points' (handled by the caller).
            return wire_body(&mine, &[], t);
        }
        _ => Other::Body(b),
    };
    let kept = edge_boolean(op, &mine, &other, false).ok_or(KernelError::Unsupported(
        "a union of a wire with a solid or sheet",
    ))?;
    let points = match (op, kind(b)) {
        (BoolOp::Intersect, BodyClass::Sheet) => crossings_with_faces(&mine, b, t),
        (BoolOp::Intersect, BodyClass::Wire) => crossings_with_pieces(&mine, &pieces_of(b), t),
        _ => Vec::new(),
    };
    wire_body(&kept, &points, t)
}

/// Points `a` op `b`.
fn points_with(a: &Body, b: &Body, op: BoolOp) -> Result<BoolOut> {
    let pts: Vec<P3> = a.points.iter().map(|&v| a.verts[v].p).collect();
    let t = tol();
    let theirs: Vec<P3> = b.points.iter().map(|&v| b.verts[v].p).collect();
    let on = |p: &P3| -> bool {
        match kind(b) {
            BodyClass::Point => theirs.iter().any(|q| (q - p).norm() <= t),
            BodyClass::Wire => b.free_edges().iter().any(|&e| {
                let ed = &b.edges[e];
                let (lo, hi) = (ed.t[0].min(ed.t[1]), ed.t[0].max(ed.t[1]));
                (ed.curve.point(ed.curve.closest(p, lo, hi)) - p).norm() <= t
            }),
            BodyClass::Sheet => on_faces(b, &polys_of(b), p, t),
            _ => classify::classify_point(b, &polys_of(b), p, t) != PointClass::Outside,
        }
    };
    let keep: Vec<P3> = match op {
        BoolOp::Intersect => pts.into_iter().filter(|p| on(p)).collect(),
        BoolOp::Subtract => pts.into_iter().filter(|p| !on(p)).collect(),
        BoolOp::Union => pts.into_iter().chain(theirs.iter().copied()).collect(),
    };
    wire_body(&[], &keep, t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Kernel;

    fn boxed(k: &mut NativeKernel, at: [f64; 3], d: [f64; 3]) -> BodyId {
        match k.box_at(at[0], at[1], at[2], d[0], d[1], d[2]).unwrap() {
            super::super::draw::Shape::Body(b) => b,
            _ => unreachable!(),
        }
    }

    /// A square sheet `size` across through `o` square to `n`.
    fn sheet(k: &mut NativeKernel, o: [f64; 3], n: [f64; 3], size: f64) -> BodyId {
        k.insert(super::super::ops::plane_sheet(
            P3::from(o),
            V3::from(n),
            size,
        ))
    }

    fn wire(k: &mut NativeKernel, pts: &[[f64; 3]]) -> BodyId {
        let curves: Vec<(super::super::curve::Curve, f64, f64)> = pts
            .windows(2)
            .map(|w| {
                let (c, len) = super::super::curve::Curve::line(P3::from(w[0]), P3::from(w[1]));
                (c, 0.0, len)
            })
            .collect();
        k.insert(super::super::kernel::wire_of_curves(&curves).unwrap())
    }

    fn area(k: &NativeKernel, b: BodyId) -> f64 {
        super::super::mass::body_mass(k.get(b).unwrap()).surface_area
    }

    fn length(k: &NativeKernel, b: BodyId) -> f64 {
        let body = k.get(b).unwrap();
        body.free_edges()
            .iter()
            .map(|&e| {
                body.edges[e]
                    .curve
                    .length(body.edges[e].t[0], body.edges[e].t[1])
            })
            .sum()
    }

    fn valid(k: &NativeKernel, b: BodyId) -> &Body {
        let body = k.get(b).unwrap();
        assert_eq!(body.check(), Vec::<String>::new());
        body
    }

    /// A sheet crossing a box: its common is the part inside (a 10 × 10 square), its cut the
    /// part outside (the 20 × 20 square less it), each a sheet.
    #[test]
    fn sheet_with_solid() {
        let mut k = NativeKernel::new();
        let bx = boxed(&mut k, [0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let sh = sheet(&mut k, [5.0, 5.0, 5.0], [0.0, 0.0, 1.0], 10.0);
        let i = k.boolean(BoolOp::Intersect, sh, &[bx]).unwrap().bodies[0];
        assert_eq!(valid(&k, i).class(), BodyClass::Sheet);
        assert!((area(&k, i) - 100.0).abs() < 1e-6, "{}", area(&k, i));
        // The common is symmetric.
        let i2 = k.boolean(BoolOp::Intersect, bx, &[sh]).unwrap().bodies[0];
        assert!((area(&k, i2) - 100.0).abs() < 1e-6);
        let d = k.boolean(BoolOp::Subtract, sh, &[bx]).unwrap().bodies[0];
        assert_eq!(valid(&k, d).class(), BodyClass::Sheet);
        assert!((area(&k, d) - 300.0).abs() < 1e-6, "{}", area(&k, d));
        assert!(matches!(
            k.boolean(BoolOp::Union, sh, &[bx]),
            Err(KernelError::Unsupported(_))
        ));
    }

    /// A solid less a sheet crossing it all the way: two solids, the volume kept.
    #[test]
    fn solid_minus_sheet_splits() {
        let mut k = NativeKernel::new();
        let bx = boxed(&mut k, [0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let sh = sheet(&mut k, [5.0, 5.0, 4.0], [0.0, 0.0, 1.0], 10.0);
        let r = k.boolean(BoolOp::Subtract, bx, &[sh]).unwrap();
        let body = valid(&k, r.bodies[0]);
        assert_eq!(body.solids.len(), 2);
        assert_eq!(body.faces.len(), 12);
        let m = super::super::mass::body_mass(body);
        assert!(
            (m.volume - 1000.0).abs() < 1e-6 && (m.surface_area - 800.0).abs() < 1e-6,
            "{m:?}"
        );
        // A sheet reaching only partly in cuts nothing through: the box comes back.
        let part = sheet(&mut k, [0.0, 0.0, 4.0], [0.0, 0.0, 1.0], 5.0);
        let r = k.boolean(BoolOp::Subtract, bx, &[part]).unwrap();
        assert_eq!(valid(&k, r.bodies[0]).solids.len(), 1);
    }

    /// Two overlapping coplanar sheets: their union is one face (the overlap merged), their
    /// difference and common the parts; two crossing sheets unite into two manifold sheets.
    #[test]
    fn sheet_with_sheet() {
        let mut k = NativeKernel::new();
        let a = sheet(&mut k, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 5.0);
        let b = sheet(&mut k, [5.0, 5.0, 0.0], [0.0, 0.0, 1.0], 5.0);
        let u = k.boolean(BoolOp::Union, a, &[b]).unwrap();
        let body = valid(&k, u.bodies[0]);
        assert_eq!(body.faces.len(), 1);
        assert_eq!(body.edges.len(), 8);
        assert!((area(&k, u.bodies[0]) - 175.0).abs() < 1e-6);
        // The merged face continues both inputs' faces (history).
        assert_eq!(u.history.modified.len(), 2);
        let d = k.boolean(BoolOp::Subtract, a, &[b]).unwrap().bodies[0];
        assert!((area(&k, d) - 75.0).abs() < 1e-6, "{}", area(&k, d));
        let i = k.boolean(BoolOp::Intersect, a, &[b]).unwrap().bodies[0];
        assert!((area(&k, i) - 25.0).abs() < 1e-6, "{}", area(&k, i));
        let c = sheet(&mut k, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], 5.0);
        let x = k.boolean(BoolOp::Union, a, &[c]).unwrap().bodies[0];
        let body = valid(&k, x);
        assert_eq!(body.class(), BodyClass::Sheet);
        assert_eq!(body.faces.len(), 4);
        assert!((area(&k, x) - 200.0).abs() < 1e-6);
    }

    /// Wires: common with a solid keeps the inside piece, cut the outside ones; a wire crossing
    /// a sheet meets it in a point; two crossing wires unite into four edges at a vertex.
    #[test]
    fn wires() {
        let mut k = NativeKernel::new();
        let bx = boxed(&mut k, [0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let w = wire(&mut k, &[[-5.0, 5.0, 5.0], [15.0, 5.0, 5.0]]);
        let i = k.boolean(BoolOp::Intersect, w, &[bx]).unwrap().bodies[0];
        assert_eq!(valid(&k, i).class(), BodyClass::Wire);
        assert!((length(&k, i) - 10.0).abs() < 1e-6);
        let d = k.boolean(BoolOp::Subtract, w, &[bx]).unwrap().bodies[0];
        assert_eq!(valid(&k, d).wires.len(), 2);
        assert!((length(&k, d) - 10.0).abs() < 1e-6);
        // The box less the wire is the box.
        let same = k.boolean(BoolOp::Subtract, bx, &[w]).unwrap().bodies[0];
        assert_eq!(valid(&k, same).faces.len(), 6);
        let sh = sheet(&mut k, [5.0, 5.0, 5.0], [1.0, 0.0, 0.0], 10.0);
        let p = k.boolean(BoolOp::Intersect, w, &[sh]).unwrap().bodies[0];
        let body = valid(&k, p);
        assert_eq!(body.class(), BodyClass::Point);
        assert!((body.verts[body.points[0]].p - P3::new(5.0, 5.0, 5.0)).norm() < 1e-9);
        let w2 = wire(&mut k, &[[5.0, -5.0, 5.0], [5.0, 15.0, 5.0]]);
        let u = k.boolean(BoolOp::Union, w, &[w2]).unwrap().bodies[0];
        let body = valid(&k, u);
        assert_eq!(body.class(), BodyClass::Wire);
        assert_eq!(body.edges.len(), 4);
        assert!((length(&k, u) - 40.0).abs() < 1e-6);
        let x = k.boolean(BoolOp::Intersect, w, &[w2]).unwrap().bodies[0];
        assert_eq!(valid(&k, x).class(), BodyClass::Point);
    }

    /// Points: common with a solid keeps those in it, cut those out of it.
    #[test]
    fn points() {
        let mut k = NativeKernel::new();
        let bx = boxed(&mut k, [0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let pts = k
            .create_points(&[
                P3::new(1.0, 1.0, 1.0),
                P3::new(20.0, 0.0, 0.0),
                P3::new(10.0, 5.0, 5.0),
            ])
            .unwrap()
            .bodies[0];
        let i = k.boolean(BoolOp::Intersect, pts, &[bx]).unwrap().bodies[0];
        assert_eq!(valid(&k, i).points.len(), 2);
        let d = k.boolean(BoolOp::Subtract, pts, &[bx]).unwrap().bodies[0];
        assert_eq!(valid(&k, d).points.len(), 1);
    }

    /// Two boxes touching at a corner: each shell has its own corner vertex (regression: the
    /// union shared it, Euler characteristic 3 over two shells).
    #[test]
    fn corner_touch_union() {
        let mut k = NativeKernel::new();
        let a = boxed(&mut k, [0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let b = boxed(&mut k, [10.0, 10.0, 10.0], [10.0, 10.0, 10.0]);
        let u = k.boolean(BoolOp::Union, a, &[b]).unwrap().bodies[0];
        let body = valid(&k, u);
        assert_eq!(body.solids.len(), 2);
        assert_eq!(body.verts.len(), 16);
    }

    /// A box united with a box inside it touching its bottom: the box's six faces (regression:
    /// the bottom stayed split in two, the pieces of one face a coincident face cut apart being
    /// merged only when they came from different inputs).
    #[test]
    fn union_merges_pieces_of_one_face() {
        let mut k = NativeKernel::new();
        let a = boxed(&mut k, [0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let b = boxed(&mut k, [2.0, 2.0, 0.0], [3.0, 3.0, 5.0]);
        let u = k.boolean(BoolOp::Union, a, &[b]).unwrap();
        let body = valid(&k, u.bodies[0]);
        assert_eq!((body.faces.len(), body.edges.len()), (6, 12));
        let d = k.boolean(BoolOp::Subtract, a, &[b]).unwrap().bodies[0];
        assert_eq!(valid(&k, d).faces.len(), 11);
    }
}
