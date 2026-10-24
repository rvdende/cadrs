//! Fans ([`Fan`]): where a chamfer is wider than the turn it runs round, the corner's strip
//! gives way to a face from the point where its neighbours' rails meet.

use super::*;

/// Stations each way along a chamfer's strip [`folds`] looks at besides its middle.
const FOLD_STATIONS: usize = 8;

/// A chamfer round a turn tighter than its width on one of its faces (a rounded rectangle's
/// top edges chamfered wider than its corners' rounds; a rim running down a round narrower
/// than the chamfer): on that face the points `d` from the edges are the two neighbours'
/// rails, crossing at a point short of their ends (the corner's own rail would run back on
/// itself round the turn, past its centre of curvature). The corner's strip goes; the
/// neighbours' rails on the face stop at that point, the apex, their other rails at their
/// sections, each strip ending in the line between; and a ruled face from the apex to the
/// corner's rail on its other face fills between (an oblique cone round a circular corner).
pub(super) struct Fan {
    /// The corner's edge.
    pub(super) edge: usize,
    /// The face the rails meet on.
    pub(super) face: usize,
    pub(super) apex: P3,
    /// The rim: the corner's rail on its other faces (and those of the neighbours' pieces that
    /// go), from the first neighbour's end to the second's, and its parameter range.
    pub(super) rim: (Curve, f64, f64),
    /// Where the rim passes from one of those faces to the next: its parameter and point.
    pub(super) breaks: Vec<(f64, P3)>,
    /// Those faces in turn (one more than `breaks`).
    pub(super) walls: Vec<usize>,
}

/// The strips' pieces between rolls ([`pieces_of`]): a piece's ends and the piece across a roll,
/// as far as the rolls have set the ends (`ends`).
pub(super) struct StripPieces<'a> {
    b: &'a Body,
    efs: &'a [EdgeFillet],
    ends: &'a [[Option<End>; 2]],
}

impl StripPieces<'_> {
    /// Piece m's end j: its spine parameter (a roll's, or the edge's end) and its ball.
    fn end(&self, m: usize, j: usize) -> Option<(f64, Ball)> {
        let ef = &self.efs[m];
        match &self.ends[m][j] {
            Some(End::Section {
                ball,
                t,
                key: SectionKey::Roll(_),
            }) => Some((*t, *ball)),
            _ if ef.at_vertex[j] => Some((self.b.edges[ef.e].t[j], ef.end_ball(self.b, j).ok()?)),
            _ => None,
        }
    }

    /// The piece of the same edge beyond piece m's end j (across its roll there).
    fn next(&self, m: usize, j: usize) -> Option<usize> {
        if self.efs[m].at_vertex[j] {
            return None;
        }
        let (t, _) = self.end(m, j)?;
        (0..self.efs.len()).find(|&x| {
            x != m
                && self.efs[x].e == self.efs[m].e
                && self
                    .end(x, 1 - j)
                    .is_some_and(|(tx, _)| (tx - t).abs() <= tol::REL_FINE * (1.0 + t.abs()))
        })
    }
}

/// A fan's neighbour's end ([`End::Fan`]): the piece, which end, and its rails' spine parameters
/// and balls there.
pub(super) struct FanEnd {
    pub(super) strip: usize,
    pub(super) end: usize,
    pub(super) t: [f64; 2],
    pub(super) balls: [Ball; 2],
}

/// A fan with its neighbours' ends and the strips it takes the place of.
pub(super) struct FanPlan {
    pub(super) fan: Fan,
    pub(super) ends: [FanEnd; 2],
    pub(super) gone: Vec<usize>,
}

/// The side of strip `ef` whose contact lies past its edge's centre of curvature (its rail there
/// would run back on itself round the turn), at the strip's middle or at any of a few stations
/// along it (an edge whose turn tightens towards an end folds there).
fn folds(b: &Body, ef: &EdgeFillet) -> Option<usize> {
    let tol = tol::REL_POINT * (1.0 + ef.r());
    let past = |t: f64, ball: &Ball| -> Option<usize> {
        let (p, _, dp, dtn) = ef.spine.at_d(t);
        let kappa = dtn / dp.norm().max(tol::TINY);
        let curv = kappa.norm();
        if curv < tol::TINY {
            return None;
        }
        let inward = kappa / curv;
        (0..2).find(|&k| (ball.q[k].p - p).dot(&inward) > 1.0 / curv + tol)
    };
    let ed = &b.edges[ef.e];
    if let Some(k) = past(ef.tm, &ef.mid) {
        return Some(k);
    }
    // Only where the edge turns somewhere tighter than the contacts can lie from it are the
    // stations' balls worth solving. In the section a ball of offsets k₀, k₁ between faces at
    // the angle θ touches their tangent planes d₀ = (k₁ + k₀ cos θ) / sin θ and
    // d₁ = (k₀ + k₁ cos θ) / sin θ from the edge, its centre √(d₀² + k₀²) from it, and no
    // contact on the ball lies farther than that and its larger offset (exact between planes;
    // on curved faces the ball sits near where their tangent planes put it, and the offset is
    // the margin). Unknown angles count as tight.
    let n = FOLD_STATIONS;
    let tight = (0..=n).any(|i| {
        let s = i as f64 / n as f64;
        let t = ed.t[0] + (ed.t[1] - ed.t[0]) * s;
        let (_, _, dp, dtn) = ef.spine.at_d(t);
        let k0 = ef.radius_at(t).abs();
        let k1 = k0 * ef.ratio;
        let Some(th) = angle_at(b, ef.e, s.clamp(tol::REL_POINT, 1.0 - tol::REL_POINT)) else {
            return true;
        };
        let (sn, cs) = th.sin_cos();
        if sn < tol::ANGULAR {
            return true;
        }
        let d0 = (k1 + k0 * cs) / sn;
        let d1 = (k0 + k1 * cs) / sn;
        let centre = d0.hypot(k0).max(d1.hypot(k1));
        dtn.norm() * (centre + k0.max(k1)) > dp.norm()
    });
    if !tight {
        return None;
    }
    [ed.t[0], ed.t[1]].iter().find_map(|&te| {
        let mut cur = (ef.tm, ef.mid);
        (1..n).find_map(|i| {
            let t = ef.tm + (te - ef.tm) * i as f64 / n as f64;
            let x = ef.ball_from(b, &cur.1, cur.0, t)?;
            cur = (t, x);
            past(t, &x)
        })
    })
}

/// The fan at strip s ([`Fan`]), if it is one: a chamfer that [`folds`] on one face, between two
/// chamfers on that face. The corner is its edge's whole strip (its pieces between rolls); each
/// neighbour is the piece of its edge's strip the apex lies on ([`fan_apex`]), the pieces past
/// it to the vertex going with the corner's.
pub(super) fn fan_corner(
    b: &Body,
    efs: &[EdgeFillet],
    ends: &[[Option<End>; 2]],
    at: &HashMap<usize, Vec<(usize, usize)>>,
    s: usize,
) -> Option<FanPlan> {
    let ef = &efs[s];
    if !ef.flat || ef.closed {
        return None;
    }
    let face = ef.faces[folds(b, ef)?];
    let ps = StripPieces { b, efs, ends };
    let ed = &b.edges[ef.e];
    // The corner's pieces from its first vertex to its second, all on the face.
    let mut corner = vec![s];
    while let Some(x) = ps.next(corner[0], 0) {
        corner.insert(0, x);
    }
    while let Some(x) = ps.next(*corner.last()?, 1) {
        corner.push(x);
    }
    let k_of = |m: usize| efs[m].faces.iter().position(|&g| g == face);
    if corner.iter().any(|&m| k_of(m).is_none())
        || !efs[corner[0]].at_vertex[0]
        || !efs[*corner.last()?].at_vertex[1]
    {
        return None;
    }
    // Each side: the other strip at the vertex, and its edge's pieces from there back.
    let side = |v: usize| -> Option<Vec<(usize, usize)>> {
        let here = at.get(&v)?;
        if here.len() != 2 {
            return None;
        }
        let &(n, jn) = here.iter().find(|&&(i, _)| efs[i].e != ef.e)?;
        if !efs[n].flat {
            return None;
        }
        let mut c = vec![(n, jn)];
        while let Some(x) = ps.next(c.last()?.0, 1 - c.last()?.1) {
            c.push((x, jn));
        }
        Some(c)
    };
    let chains = [side(ed.v[0])?, side(ed.v[1])?];
    let tol = tol::REL_POINT * (1.0 + ef.r());
    let (pick, ts, balls) = fan_apex(&ps, &chains, face, tol)?;
    let ks = [k_of(chains[0][pick[0]].0)?, k_of(chains[1][pick[1]].0)?];
    let apex = P3::from((balls[0].q[ks[0]].p.coords + balls[1].q[ks[1]].p.coords) * 0.5);
    // Short of each picked piece's end towards the vertex, on the piece.
    let mut fan_ends = Vec::new();
    let mut gone = corner.clone();
    for i in 0..2 {
        let (m, jm) = chains[i][pick[i]];
        let en = &efs[m];
        let (te, end) = ps.end(m, jm)?;
        if (ts[i] - te) * (te - en.tm) >= 0.0 || (ts[i] - en.tm) * (te - en.tm) <= 0.0 {
            return None;
        }
        let mut t = [te; 2];
        let mut bs = [end; 2];
        t[ks[i]] = ts[i];
        bs[ks[i]] = balls[i];
        fan_ends.push(FanEnd {
            strip: m,
            end: jm,
            t,
            balls: bs,
        });
        gone.extend(chains[i][..pick[i]].iter().map(|x| x.0));
    }
    // The rim from the picked pieces' wall contacts at their ends: the other rails of the pieces
    // that go, the first side's from its picked piece out to the vertex, the corner's, and the
    // second side's from its vertex out.
    let mut pieces: Vec<(usize, usize)> = chains[0][..pick[0]]
        .iter()
        .rev()
        .map(|&(m, jm)| (m, 1 - jm))
        .collect();
    pieces.extend(corner.iter().map(|&m| (m, 0)));
    pieces.extend(chains[1][..pick[1]].iter().copied());
    let w = [0, 1].map(|i| fan_ends[i].balls[1 - ks[i]].q[1 - ks[i]].p);
    let Rim { rim, breaks, walls } = fan_rim(&ps, &pieces, face, w, tol)?;
    let [e0, e1]: [FanEnd; 2] = fan_ends.try_into().ok()?;
    Some(FanPlan {
        fan: Fan {
            face,
            apex,
            rim,
            breaks,
            walls,
            edge: ef.e,
        },
        ends: [e0, e1],
        gone,
    })
}

/// Where the rails on `face` of a fan's two sides cross (the apex), short of the vertex:
/// Gauss–Newton on the two spine parameters, each piece's ball run on from its end's; where a
/// parameter leaves a piece, the one before it (or back, the one after). Each side's piece
/// there (its index in `chains`), the parameters and the balls.
fn fan_apex(
    ps: &StripPieces,
    chains: &[Vec<(usize, usize)>; 2],
    face: usize,
    tol: f64,
) -> Option<([usize; 2], [f64; 2], [Ball; 2])> {
    let efs = ps.efs;
    let k_of = |m: usize| efs[m].faces.iter().position(|&g| g == face);
    let mut pick = [0usize; 2];
    let mut ts = [0.0; 2];
    let mut balls = [dummy_ball(); 2];
    for i in 0..2 {
        let (m, jm) = chains[i][0];
        (ts[i], balls[i]) = ps.end(m, jm)?;
    }
    let ball_at = |pick: &[usize; 2], i: usize, t: f64| -> Option<Ball> {
        let (m, jm) = chains[i][pick[i]];
        let (te, end) = ps.end(m, jm)?;
        efs[m].ball_from(ps.b, &end, te, t)
    };
    for _ in 0..60 {
        let ks = [k_of(chains[0][pick[0]].0)?, k_of(chains[1][pick[1]].0)?];
        let q = [balls[0].q[ks[0]].p, balls[1].q[ks[1]].p];
        let r = q[0] - q[1];
        if r.norm() <= tol {
            return Some((pick, ts, balls));
        }
        let cols: Vec<V3> = (0..2)
            .map(|i| {
                let (m, jm) = chains[i][pick[i]];
                let te = ps.end(m, jm)?.0;
                let h = 1e-6 * (1.0 + ts[i].abs()) * (te - efs[m].tm).signum();
                let x = ball_at(&pick, i, ts[i] + h)?;
                Some((x.q[ks[i]].p - q[i]) / h * if i == 0 { 1.0 } else { -1.0 })
            })
            .collect::<Option<_>>()?;
        let mm = nalgebra::Matrix3x2::from_columns(&[cols[0], cols[1]]);
        let dx = (mm.transpose() * mm).lu().solve(&(-(mm.transpose() * r)))?;
        for i in 0..2 {
            ts[i] += dx[i];
            loop {
                let (m, jm) = chains[i][pick[i]];
                let (te, _) = ps.end(m, jm)?;
                let start = ps.end(m, 1 - jm).map(|x| x.0);
                let before = start.is_some_and(|t0| (ts[i] - t0) * (te - t0) < 0.0);
                let after = pick[i] > 0 && start.is_some_and(|t0| (ts[i] - te) * (te - t0) > 0.0);
                if before && pick[i] + 1 < chains[i].len() {
                    pick[i] += 1;
                } else if after {
                    pick[i] -= 1;
                } else {
                    break;
                }
            }
            balls[i] = ball_at(&pick, i, ts[i])?;
        }
    }
    None
}

/// A fan's rim as [`Fan`] has it.
struct Rim {
    rim: (Curve, f64, f64),
    breaks: Vec<(f64, P3)>,
    walls: Vec<usize>,
}

/// A fan's rim: the rails not on `face` of `pieces` (each piece from its end given), their
/// contacts fitted as one curve from `w[0]` to `w[1]` (exact there); where it passes from one
/// face to the next, its parameter and point; and the faces in turn.
fn fan_rim(
    ps: &StripPieces,
    pieces: &[(usize, usize)],
    face: usize,
    w: [P3; 2],
    tol: f64,
) -> Option<Rim> {
    let mut pts: Vec<(P3, usize)> = Vec::new();
    for &(m, from) in pieces {
        let ef = &ps.efs[m];
        let wk = 1 - ef.faces.iter().position(|&g| g == face)?;
        let (t0, b0) = ps.end(m, from)?;
        let (t1, _) = ps.end(m, 1 - from)?;
        let mut cur = (t0, b0);
        for i in 0..=ACROSS {
            let t = t0 + (t1 - t0) * i as f64 / ACROSS as f64;
            cur = (t, ef.ball_from(ps.b, &cur.1, cur.0, t)?);
            let q = cur.1.q[wk].p;
            // (A roll's point once, on the face before.)
            if pts.last().is_none_or(|l| (l.0 - q).norm() > tol) {
                pts.push((q, ef.faces[wk]));
            }
        }
    }
    if pts.len() < 2 {
        return None;
    }
    let last = pts.len() - 1;
    (pts[0].0, pts[last].0) = (w[0], w[1]);
    let xs: Vec<P3> = pts.iter().map(|x| x.0).collect();
    let (rim, c0, c1) = fit_cut(&xs, None, [None, None]);
    let mut breaks = Vec::new();
    let mut walls = vec![pts[0].1];
    for p in pts.windows(2) {
        if p[1].1 != p[0].1 {
            let c = rim.closest(&p[0].0, c0.min(c1), c0.max(c1));
            breaks.push((c, rim.point(c)));
            walls.push(p[1].1);
        }
    }
    Some(Rim {
        rim: (rim, c0, c1),
        breaks,
        walls,
    })
}

/// The face of fan `c` ([`Fan`]): ruled from its apex to the corner's rail on the wall, between
/// the lines down its two neighbours' ends; the rail cuts the wall; at the apex a degenerate
/// edge (the ruled surface's side there, a point).
pub(super) fn fan_face(
    nb: &mut Body,
    fan: &Fan,
    c: usize,
    out: &mut Pieces,
    input: BodyId,
    vtol: f64,
) -> Result<()> {
    let line = |side: usize| {
        out.section_edges
            .get(&SectionKey::Fan(c, side))
            .copied()
            .ok_or_else(|| failed("corner", "a fan's neighbour has no end"))
    };
    let (l0, l1) = (line(0)?, line(1)?);
    // Each line runs from its wall contact to the apex.
    let (w0, av) = (nb.edges[l0].v[0], nb.edges[l0].v[1]);
    let w1 = nb.edges[l1].v[0];
    let (rim, a0, a1) = fan.rim.clone();
    // The rim in pieces by face, each cutting its face, between vertices on the edges between.
    let mut rims = Vec::new();
    let (mut from, mut tf) = (w0, a0);
    for (i, &(t, q)) in fan.breaks.iter().enumerate() {
        let vb = vertex_on_boundary(nb, fan.walls[i], &q, vtol)
            .ok_or_else(|| failed("corner", "a fan's rim misses the edge between its faces"))?;
        let e = add_edge(nb, rim.clone(), tf, t, from, vb);
        out.cuts.entry(fan.walls[i]).or_default().push(e);
        rims.push(e);
        (from, tf) = (vb, t);
    }
    let e = add_edge(nb, rim.clone(), tf, a1, from, w1);
    out.cuts
        .entry(
            *fan.walls
                .last()
                .ok_or_else(|| failed("corner", "a fan without a rim"))?,
        )
        .or_default()
        .push(e);
    rims.push(e);
    // Ruled from the apex to the rim where the strips beside end in lines down to it (flat
    // ones: an exact cone round a circle); else a Coons patch on those curves, the rim and the
    // apex.
    let straight = |e: usize| matches!(nb.edges[e].curve, Curve::Line { .. });
    let (surf, deg) = if straight(l0) && straight(l1) {
        let r = rim.to_nurbs(a0, a1);
        let cps: Vec<Vec<H>> = r
            .cps
            .iter()
            .map(|h| vec![hom(&fan.apex, h[3]), *h])
            .collect();
        let surf = Surface::Nurbs(std::sync::Arc::new(NurbsSurface {
            du: r.degree,
            dv: 1,
            ku: r.knots.clone(),
            kv: vec![0.0, 0.0, 1.0, 1.0],
            cps,
        }));
        (surf, nb.add_degenerate(fan.apex, a0, a1, av))
    } else {
        // Each side from the apex up to the rim (its edge runs the other way).
        let side = |e: usize| {
            let ee = nb.edges[e].clone();
            move |s: f64| ee.point(ee.t[1] + (ee.t[0] - ee.t[1]) * s)
        };
        let (left, right) = (side(l0), side(l1));
        let top = |s: f64| rim.point(a0 + (a1 - a0) * s);
        let apex = fan.apex;
        let surf = coons(&|_| apex, &top, &left, &right)
            .ok_or_else(|| failed("corner", "a fan's patch can't be fitted"))?;
        (surf, nb.add_degenerate(fan.apex, 0.0, 1.0, av))
    };
    out.new_faces.push(NewFace {
        surf,
        edges: [vec![l0, l1, deg], rims].concat(),
        src: FaceSrc::New(Origin::FromEdge {
            body: input,
            edge: EdgeId(fan.edge as u64),
        }),
        u_period: None,
        beside: fan.face,
        pcurves: vec![(
            deg,
            Curve::Line {
                o: P3::origin(),
                d: V3::x(),
            },
        )],
    });
    Ok(())
}
