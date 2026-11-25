//! IGES 5.3 reading and writing of B-rep solids: Manifold Solid B-Rep Objects (186) of shells
//! (514), faces (510), loops (508), edge lists (504) and vertex lists (502).
//!
//! **Format.** 80-column records: Start (S), Global (G), Directory Entry (D, two lines per
//! entity, 8-column fields), Parameter Data (P, columns 1–64 with the entity's directory line in
//! 66–72) and Terminate (T). Parameters are separated by the global delimiters (`,` and `;` by
//! default); strings are Hollerith (`4HName`); reals may use a `D` exponent.
//!
//! **Geometry.** Curves: lines (110), circular arcs (100, counter-clockwise in their definition
//! plane, placed by a transformation matrix 124), conic arcs (104, ellipses), rational B-splines
//! (126). Surfaces: planes (190, and 108), right circular cylinders (192), cones (194, semi-angle
//! in degrees), spheres (196), tori (198), surfaces of revolution (120), tabulated cylinders
//! (122), rational B-spline surfaces (128); points 116 and directions 123. Every entity's
//! directory transformation (a chain of 124s) is applied. Lengths are converted to mm from the
//! global unit flag.
//!
//! **Topology.** A 504 edge runs from its start vertex to its end vertex along its curve; a 508
//! loop lists edges with an orientation flag (1: along the edge) and optional p-curves (ignored,
//! ours are computed); a 510 face has its surface and loops (the first outer when its flag is
//! 1); a 514 shell lists faces with a flag saying whether the face's outward side agrees with its
//! surface's normal. As OCCT does, loops are written counter-clockwise about the surface's
//! normal, so the reader reverses the loops of a face whose flag is 0. The body is rebuilt by
//! [`super::xbuild::XBuilder`]. A part's name goes in a Name property (406 form 15) attached to
//! its 186, and in the 186's directory label.
//!
//! **Trimmed surfaces.** Most IGES in the wild has no topology: faces are trimmed (144) or
//! bounded (143) surfaces whose boundaries (142 / 141) are model space curves, often composites
//! (102). A file without B-rep solids or shells is read that way: the faces are sewn by their
//! boundary curves (vertices welded within ten times the file's resolution, coincident segments
//! made one edge), each connected group of faces is a shell, a closed one a solid, and the
//! shells are oriented consistently and outward.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Arc;

use super::curve::Curve;
use super::math::*;
use super::nurbs::{H, NurbsCurve, NurbsSurface, dehom, hom};
use super::step::{ReadModel, ReadOccurrence, ReadPart, canonical, same_sense};
use super::surface::Surface;
use super::tol;
use super::topo::*;
use super::xbuild::{XBuilder, XFace};

// ---------------------------------------------------------------------------------------------
// Writing

/// A real for IGES: shortest round-trip digits with a decimal point.
fn real(x: f64) -> String {
    super::step::real(x).to_string()
}

fn hollerith(s: &str) -> String {
    let s: String = s
        .chars()
        .map(|c| {
            if c.is_ascii() && !c.is_ascii_control() {
                c
            } else {
                '?'
            }
        })
        .take(200)
        .collect();
    format!("{}H{}", s.len(), s)
}

struct Ent {
    kind: u32,
    form: u32,
    params: Vec<String>,
    transform: usize,
    label: String,
    /// Directory status: 00 visible, subordinate switch, use, hierarchy.
    status: &'static str,
}

#[derive(Default)]
struct Writer {
    ents: Vec<Ent>,
}

impl Writer {
    /// Adds an entity; returns its directory pointer (the line number of its first D record).
    fn add(&mut self, kind: u32, form: u32, params: Vec<String>) -> usize {
        self.ents.push(Ent {
            kind,
            form,
            params,
            transform: 0,
            label: String::new(),
            status: "00010000",
        });
        2 * self.ents.len() - 1
    }
    fn ent(&mut self, de: usize) -> &mut Ent {
        &mut self.ents[(de - 1) / 2]
    }
    fn point(&mut self, p: &P3) -> usize {
        self.add(116, 0, vec![real(p.x), real(p.y), real(p.z), "0".into()])
    }
    fn dir(&mut self, d: &V3) -> usize {
        let d = d.normalize();
        self.add(123, 0, vec![real(d.x), real(d.y), real(d.z)])
    }
    /// A 124 matrix placing local coordinates in the frame (o, x, y, z).
    fn matrix(&mut self, o: &P3, x: &V3, y: &V3, z: &V3) -> usize {
        let p = vec![x.x, y.x, z.x, o.x, x.y, y.y, z.y, o.y, x.z, y.z, z.z, o.z];
        self.add(124, 0, p.into_iter().map(real).collect())
    }

    fn nurbs_curve(&mut self, n: &NurbsCurve) -> usize {
        let k = n.cps.len() - 1;
        let m = n.degree;
        let (a, b) = n.range();
        let pts: Vec<P3> = n.cps.iter().map(dehom).collect();
        let planar = {
            // A plane through the control points (for PROP1 and the normal).
            let c = pts.iter().fold(V3::zeros(), |s, p| s + p.coords) / pts.len() as f64;
            let mut nrm = V3::zeros();
            for i in 0..pts.len() {
                let (p, q) = (pts[i].coords - c, pts[(i + 1) % pts.len()].coords - c);
                nrm += p.cross(&q);
            }
            if nrm.norm() > tol::DEGENERATE
                && pts
                    .iter()
                    .all(|p| (p.coords - c).dot(&nrm.normalize()).abs() < tol::ROUNDOFF)
            {
                Some(nrm.normalize())
            } else {
                None
            }
        };
        let closed = (pts[0] - pts[k]).norm() < tol::ROUNDOFF;
        let mut p = vec![
            k.to_string(),
            m.to_string(),
            (planar.is_some() as u8).to_string(),
            (closed as u8).to_string(),
            (!n.is_rational() as u8).to_string(),
            "0".into(),
        ];
        p.extend(n.knots.iter().map(|x| real(*x)));
        p.extend(n.cps.iter().map(|h| real(h.w)));
        for q in &pts {
            p.extend([real(q.x), real(q.y), real(q.z)]);
        }
        p.extend([real(a), real(b)]);
        let nn = planar.unwrap_or(V3::z());
        p.extend([real(nn.x), real(nn.y), real(nn.z)]);
        self.add(126, 0, p)
    }

    /// The curve of an edge on [t0, t1].
    fn curve(&mut self, c: &Curve, t0: f64, t1: f64) -> usize {
        match c {
            Curve::Line { .. } => {
                let (a, b) = (c.point(t0), c.point(t1));
                self.add(
                    110,
                    0,
                    vec![
                        real(a.x),
                        real(a.y),
                        real(a.z),
                        real(b.x),
                        real(b.y),
                        real(b.z),
                    ],
                )
            }
            Curve::Circle { c: cc, x, y, r } => {
                let z = x.cross(y);
                let tm = self.matrix(cc, x, y, &z);
                let (s, e) = ((t0.cos() * r, t0.sin() * r), (t1.cos() * r, t1.sin() * r));
                let full = (t1 - t0 - TAU).abs() < tol::ANGULAR_FINE;
                let e = if full { s } else { e };
                let de = self.add(
                    100,
                    0,
                    vec![
                        "0.".into(),
                        "0.".into(),
                        "0.".into(),
                        real(s.0),
                        real(s.1),
                        real(e.0),
                        real(e.1),
                    ],
                );
                self.ent(de).transform = tm;
                de
            }
            Curve::Ellipse { .. } | Curve::Nurbs(_) => {
                let n = c.to_nurbs(t0, t1);
                self.nurbs_curve(&n)
            }
        }
    }

    fn nurbs_surface(&mut self, n: &NurbsSurface) -> usize {
        let (k1, k2) = (n.cps.len() - 1, n.cps[0].len() - 1);
        let ((u0, u1), (v0, v1)) = n.range();
        let rational = {
            let w0 = n.cps[0][0].w;
            n.cps.iter().flatten().any(|h| (h.w - w0).abs() > 1e-14)
        };
        let mut p = vec![
            k1.to_string(),
            k2.to_string(),
            n.du.to_string(),
            n.dv.to_string(),
            "0".into(),
            "0".into(),
            (!rational as u8).to_string(),
            "0".into(),
            "0".into(),
        ];
        p.extend(n.ku.iter().map(|x| real(*x)));
        p.extend(n.kv.iter().map(|x| real(*x)));
        // The first index varies fastest: column by column.
        let column = |j: usize| n.cps.iter().map(move |row| row[j]);
        for j in 0..=k2 {
            p.extend(column(j).map(|h| real(h.w)));
        }
        for j in 0..=k2 {
            for h in column(j) {
                let q = dehom(&h);
                p.extend([real(q.x), real(q.y), real(q.z)]);
            }
        }
        p.extend([real(u0), real(u1), real(v0), real(v1)]);
        self.add(128, 0, p)
    }

    /// Writes a surface in its canonical form; returns the entity and that form.
    fn surface(&mut self, s: &Surface) -> (usize, Surface) {
        let c = canonical(s);
        let de = match &c {
            Surface::Plane { f } => {
                let (o, n, x) = (self.point(&f.o), self.dir(&f.z), self.dir(&f.x));
                self.add(190, 1, vec![o.to_string(), n.to_string(), x.to_string()])
            }
            Surface::Cylinder { f, r } => {
                let (o, a, x) = (self.point(&f.o), self.dir(&f.z), self.dir(&f.x));
                self.add(
                    192,
                    1,
                    vec![o.to_string(), a.to_string(), real(*r), x.to_string()],
                )
            }
            Surface::Cone { f, r, a } => {
                let (o, ax, x) = (self.point(&f.o), self.dir(&f.z), self.dir(&f.x));
                self.add(
                    194,
                    1,
                    vec![
                        o.to_string(),
                        ax.to_string(),
                        real(*r),
                        real(a.to_degrees()),
                        x.to_string(),
                    ],
                )
            }
            Surface::Sphere { f, r } => {
                let (o, ax, x) = (self.point(&f.o), self.dir(&f.z), self.dir(&f.x));
                self.add(
                    196,
                    1,
                    vec![o.to_string(), real(*r), ax.to_string(), x.to_string()],
                )
            }
            Surface::Torus { f, rmaj, rmin } => {
                let (o, ax, x) = (self.point(&f.o), self.dir(&f.z), self.dir(&f.x));
                self.add(
                    198,
                    1,
                    vec![
                        o.to_string(),
                        ax.to_string(),
                        real(*rmaj),
                        real(*rmin),
                        x.to_string(),
                    ],
                )
            }
            Surface::Revolution { f, m } => {
                let axis = self.curve(&Curve::Line { o: f.o, d: f.z }, 0.0, 1.0);
                let (a, b) = m.nurbs_range().unwrap_or((0.0, TAU));
                let g = self.curve(m, a, b);
                self.add(
                    120,
                    0,
                    vec![axis.to_string(), g.to_string(), "0.".into(), real(TAU)],
                )
            }
            Surface::Extrusion { c: cv, d } => {
                let (a, b) = cv.nurbs_range().unwrap_or((0.0, TAU));
                let g = self.curve(cv, a, b);
                let e = cv.point(a) + d;
                self.add(122, 0, vec![g.to_string(), real(e.x), real(e.y), real(e.z)])
            }
            Surface::Nurbs(n) => self.nurbs_surface(n),
        };
        (de, c)
    }

    /// A body's wires and points: independent curves (one per edge) and points (116), grouped
    /// in a 402 form 7 named after the part.
    fn wireframe(&mut self, b: &Body, name: &str) -> Option<usize> {
        let mut members = Vec::new();
        for &(ei, _) in b.wires.iter().flat_map(|w| w.edges.iter()) {
            let e = &b.edges[ei];
            let de = self.curve(&e.curve, e.t[0], e.t[1]);
            self.ent(de).status = "00000000";
            members.push(de);
        }
        for &v in &b.points {
            let de = self.point(&b.verts[v].p);
            self.ent(de).status = "00000000";
            members.push(de);
        }
        if members.is_empty() {
            return None;
        }
        let mut p = vec![members.len().to_string()];
        p.extend(members.iter().map(|m| m.to_string()));
        let de = self.add(402, 7, p);
        let e = self.ent(de);
        e.status = "00000000";
        e.label = name
            .chars()
            .filter(|c| c.is_ascii_graphic())
            .take(8)
            .collect();
        Some(de)
    }

    fn body(&mut self, b: &Body, name: &str) -> Vec<usize> {
        let wire = self.wireframe(b, name);
        if b.faces.is_empty() {
            return wire.into_iter().collect();
        }
        let in_wire: std::collections::HashSet<usize> = b
            .wires
            .iter()
            .flat_map(|w| w.edges.iter().map(|e| e.0))
            .collect();
        let used: Vec<usize> = {
            let mut u = vec![false; b.verts.len()];
            for (_, e) in b
                .edges
                .iter()
                .enumerate()
                .filter(|(i, e)| !e.degenerate && !in_wire.contains(i))
            {
                u[e.v[0]] = true;
                u[e.v[1]] = true;
            }
            (0..b.verts.len()).filter(|&i| u[i]).collect()
        };
        let mut vidx = vec![0usize; b.verts.len()];
        let mut vp = vec![used.len().to_string()];
        for (k, &i) in used.iter().enumerate() {
            vidx[i] = k + 1;
            let p = b.verts[i].p;
            vp.extend([real(p.x), real(p.y), real(p.z)]);
        }
        let vl = self.add(502, 1, vp);
        let edges: Vec<usize> = (0..b.edges.len())
            .filter(|&e| !b.edges[e].degenerate && !in_wire.contains(&e))
            .collect();
        let mut eidx = vec![0usize; b.edges.len()];
        let mut ep = vec![edges.len().to_string()];
        for (k, &ei) in edges.iter().enumerate() {
            eidx[ei] = k + 1;
            let e = &b.edges[ei];
            let c = self.curve(&e.curve, e.t[0], e.t[1]);
            ep.extend([
                c.to_string(),
                vl.to_string(),
                vidx[e.v[0]].to_string(),
                vl.to_string(),
                vidx[e.v[1]].to_string(),
            ]);
        }
        let el = self.add(504, 1, ep);
        let mut face_de = vec![(0usize, true); b.faces.len()];
        for (fi, f) in b.faces.iter().enumerate() {
            let (s, written) = self.surface(&f.surf);
            let same = same_sense(b, f, &written);
            let mut loops = Vec::new();
            for l in &f.loops {
                // Counter-clockwise about the surface's normal.
                let cs: Vec<&Coedge> = l.iter().filter(|c| !b.edges[c.edge].degenerate).collect();
                if cs.is_empty() {
                    continue;
                }
                let order: Vec<(usize, bool)> = if same {
                    cs.iter().map(|c| (c.edge, c.fwd)).collect()
                } else {
                    cs.iter().rev().map(|c| (c.edge, !c.fwd)).collect()
                };
                let mut lp = vec![order.len().to_string()];
                for (e, fwd) in order {
                    lp.extend([
                        "0".into(),
                        el.to_string(),
                        eidx[e].to_string(),
                        (fwd as u8).to_string(),
                        "0".into(),
                    ]);
                }
                loops.push(self.add(508, 1, lp));
            }
            let mut fp = vec![s.to_string(), loops.len().to_string(), "1".into()];
            fp.extend(loops.iter().map(|l| l.to_string()));
            face_de[fi] = (self.add(510, 1, fp), same);
        }
        let mut shells = Vec::new();
        for sh in &b.shells {
            let mut p = vec![sh.faces.len().to_string()];
            for &f in &sh.faces {
                p.extend([face_de[f].0.to_string(), (face_de[f].1 as u8).to_string()]);
            }
            shells.push(self.add(514, if sh.closed { 1 } else { 2 }, p));
        }
        let mut tops = Vec::new();
        let label: String = name
            .chars()
            .filter(|c| c.is_ascii_graphic())
            .take(8)
            .collect();
        let in_solid: std::collections::HashSet<usize> = b
            .solids
            .iter()
            .flat_map(|s| s.shells.iter().copied())
            .collect();
        for s in &b.solids {
            let mut p = vec![
                shells[s.shells[0]].to_string(),
                "1".into(),
                (s.shells.len() - 1).to_string(),
            ];
            for &v in &s.shells[1..] {
                p.extend([shells[v].to_string(), "1".into()]);
            }
            let nm = self.add(406, 15, vec!["1".into(), hollerith(name)]);
            p.extend(["0".into(), "1".into(), nm.to_string()]);
            let de = self.add(186, 0, p);
            let e = self.ent(de);
            e.status = "00000000";
            e.label = label.clone();
            tops.push(de);
        }
        for (si, &sh) in shells.iter().enumerate() {
            if !in_solid.contains(&si) {
                let e = self.ent(sh);
                e.status = "00000000";
                e.label = label.clone();
                tops.push(sh);
            }
        }
        tops.extend(wire);
        tops
    }

    fn finish(self, bbox_max: f64) -> Vec<u8> {
        let mut out = String::new();
        let line = |out: &mut String, data: &str, sec: char, seq: usize| {
            let _ = writeln!(out, "{data:<72}{sec}{seq:>7}");
        };
        line(&mut out, "cadrs native kernel IGES export", 'S', 1);
        // Global section.
        let now = {
            // YYYYMMDD.HHNNSS from the STEP writer's timestamp.
            let t = super::step::timestamp();
            format!(
                "{}{}{}.{}{}{}",
                &t[0..4],
                &t[5..7],
                &t[8..10],
                &t[11..13],
                &t[14..16],
                &t[17..19]
            )
        };
        let g = [
            "1H,".to_string(),
            "1H;".into(),
            hollerith("cadrs"),
            hollerith("cadrs.igs"),
            hollerith("cadrs native kernel"),
            hollerith("1.0"),
            "32".into(),
            "38".into(),
            "6".into(),
            "308".into(),
            "15".into(),
            hollerith("cadrs"),
            "1.".into(),
            "2".into(),
            "2HMM".into(),
            "1".into(),
            "1.".into(),
            hollerith(&now),
            "1.E-07".into(),
            real(bbox_max.max(1.0)),
            hollerith(""),
            hollerith(""),
            "11".into(),
            "0".into(),
            hollerith(&now),
        ];
        let mut gl = Vec::new();
        let mut cur = String::new();
        for (i, t) in g.iter().enumerate() {
            let tok = format!("{t}{}", if i + 1 == g.len() { ";" } else { "," });
            if cur.len() + tok.len() > 72 {
                gl.push(std::mem::take(&mut cur));
            }
            cur.push_str(&tok);
        }
        gl.push(cur);
        for (i, l) in gl.iter().enumerate() {
            line(&mut out, l, 'G', i + 1);
        }
        // Parameter lines (to know each entity's start and count).
        let mut plines: Vec<(String, usize)> = Vec::new();
        let mut pinfo = Vec::new();
        for (k, e) in self.ents.iter().enumerate() {
            let de = 2 * k + 1;
            let start = plines.len() + 1;
            let mut toks = vec![e.kind.to_string()];
            toks.extend(e.params.iter().cloned());
            let mut cur = String::new();
            for (i, t) in toks.iter().enumerate() {
                let tok = format!("{t}{}", if i + 1 == toks.len() { ";" } else { "," });
                if cur.len() + tok.len() > 64 && !cur.is_empty() {
                    plines.push((std::mem::take(&mut cur), de));
                }
                if tok.len() > 64 {
                    // Only long strings: split across lines.
                    let mut rest = tok.as_str();
                    while rest.len() > 64 {
                        plines.push((rest[..64].to_string(), de));
                        rest = &rest[64..];
                    }
                    cur.push_str(rest);
                } else {
                    cur.push_str(&tok);
                }
            }
            plines.push((cur, de));
            pinfo.push((start, plines.len() + 1 - start));
        }
        for (k, e) in self.ents.iter().enumerate() {
            let seq = 2 * k + 1;
            let _ = writeln!(
                out,
                "{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}D{:>7}",
                e.kind, pinfo[k].0, 0, 0, 0, 0, e.transform, 0, e.status, seq
            );
            let _ = writeln!(
                out,
                "{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}D{:>7}",
                e.kind,
                0,
                0,
                pinfo[k].1,
                e.form,
                "",
                "",
                e.label,
                0,
                seq + 1
            );
        }
        for (i, (l, de)) in plines.iter().enumerate() {
            let _ = writeln!(out, "{l:<64} {de:>7}P{:>7}", i + 1);
        }
        let t = format!(
            "S{:>7}G{:>7}D{:>7}P{:>7}",
            1,
            gl.len(),
            self.ents.len() * 2,
            plines.len()
        );
        line(&mut out, &t, 'T', 1);
        out.into_bytes()
    }
}

/// Writes bodies (with names) as an IGES file of MSBO solids (sheets as open shells).
pub fn write(parts: &[(&Body, &str)]) -> Vec<u8> {
    let mut w = Writer::default();
    let mut m: f64 = 1.0;
    for (b, name) in parts {
        let bb = b.bbox();
        if !bb.is_empty() {
            m = m.max(bb.min.coords.amax()).max(bb.max.coords.amax());
        }
        w.body(b, name);
    }
    w.finish(m)
}

// ---------------------------------------------------------------------------------------------
// Reading

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Empty,
}

impl Tok {
    fn f(&self) -> Option<f64> {
        match self {
            Tok::Num(x) => Some(*x),
            Tok::Empty => Some(0.0),
            Tok::Str(_) => None,
        }
    }
}

/// Splits parameter data into tokens (Hollerith strings may hold the delimiters).
fn tokens(s: &str, pd: char, rd: char) -> Vec<Tok> {
    let b: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut cur = String::new();
    while i < b.len() {
        let c = b[i];
        if c == pd || c == rd {
            let t = cur.trim();
            out.push(if t.is_empty() {
                Tok::Empty
            } else {
                t.replace(['D', 'd'], "E")
                    .parse()
                    .map(Tok::Num)
                    .unwrap_or(Tok::Empty)
            });
            cur.clear();
            if c == rd {
                return out;
            }
            i += 1;
            continue;
        }
        if c == 'H' && !cur.trim().is_empty() && cur.trim().chars().all(|d| d.is_ascii_digit()) {
            let n: usize = cur.trim().parse().unwrap_or(0);
            let st = i + 1;
            let en = (st + n).min(b.len());
            out.push(Tok::Str(b[st..en].iter().collect()));
            cur.clear();
            i = en;
            // Skip to the next delimiter.
            while i < b.len() && b[i] != pd && b[i] != rd {
                i += 1;
            }
            if i < b.len() && b[i] == rd {
                return out;
            }
            i += 1;
            continue;
        }
        cur.push(c);
        i += 1;
    }
    if !cur.trim().is_empty() {
        out.push(
            cur.trim()
                .replace(['D', 'd'], "E")
                .parse()
                .map(Tok::Num)
                .unwrap_or(Tok::Empty),
        );
    }
    out
}

struct DirEnt {
    kind: u32,
    form: u32,
    transform: usize,
    label: String,
    params: Vec<Tok>,
    /// The status's subordinate switch is 00: the entity stands on its own (not only a part of
    /// another).
    independent: bool,
}

struct File {
    ents: HashMap<usize, DirEnt>,
    /// Length unit → mm.
    scale: f64,
    /// The global section's minimum resolution (mm).
    resolution: f64,
}

type R<T> = Result<T, String>;

fn field(l: &str, k: usize) -> &str {
    l.get(k * 8..(k + 1) * 8).unwrap_or("").trim()
}

fn parse(bytes: &[u8]) -> R<File> {
    let text = String::from_utf8_lossy(bytes);
    let mut sec: HashMap<char, Vec<String>> = HashMap::new();
    for raw in text.lines() {
        let l = raw.trim_end_matches('\r');
        if l.trim().is_empty() {
            continue;
        }
        let l: String = format!("{l:<80}");
        let c = l.chars().nth(72).unwrap_or(' ');
        if !"SGDPTC".contains(c) {
            return Err("not an IGES file (no section letter in column 73)".into());
        }
        if c == 'C' {
            return Err("compressed IGES is not supported".into());
        }
        sec.entry(c).or_default().push(l);
    }
    if !sec.contains_key(&'D') || !sec.contains_key(&'P') {
        return Err("not an IGES file (no directory or parameter section)".into());
    }
    let g: String = sec
        .get(&'G')
        .map(|v| {
            v.iter()
                .map(|l| l.get(..72).unwrap_or("").to_string())
                .collect()
        })
        .unwrap_or_default();
    // The delimiters: "1H," style, or defaults.
    let (mut pd, mut rd) = (',', ';');
    let gc: Vec<char> = g.chars().collect();
    let mut gi = 0;
    if gc.len() >= 3 && gc[0] == '1' && gc[1] == 'H' {
        pd = gc[2];
        gi = 4;
        if gc.len() >= gi + 3 && gc[gi] == '1' && gc[gi + 1] == 'H' {
            rd = gc[gi + 2];
        }
    } else if gc.first() == Some(&',') {
        gi = 1;
        if gc.len() >= gi + 3 && gc[gi] == '1' && gc[gi + 1] == 'H' {
            rd = gc[gi + 2];
        }
    }
    let gt = tokens(&g, pd, rd);
    let unit = gt.get(13).and_then(|t| t.f()).unwrap_or(2.0) as i64;
    let scale = match unit {
        1 => 25.4,
        2 => 1.0,
        4 => 304.8,
        5 => 1_609_344.0,
        6 => 1000.0,
        7 => 1e6,
        8 => 0.0254,
        9 => 1e-3,
        10 => 10.0,
        11 => 2.54e-5,
        3 => match gt.get(14) {
            Some(Tok::Str(s)) if s.eq_ignore_ascii_case("IN") || s.eq_ignore_ascii_case("INCH") => {
                25.4
            }
            Some(Tok::Str(s)) if s.eq_ignore_ascii_case("M") => 1000.0,
            Some(Tok::Str(s)) if s.eq_ignore_ascii_case("CM") => 10.0,
            _ => 1.0,
        },
        _ => 1.0,
    };
    let _ = gi;
    // Parameter data by directory pointer.
    let mut pdata: HashMap<usize, String> = HashMap::new();
    for l in &sec[&'P'] {
        let de: usize = l.get(64..72).unwrap_or("").trim().parse().unwrap_or(0);
        pdata
            .entry(de)
            .or_default()
            .push_str(l.get(..64).unwrap_or(""));
    }
    let d = &sec[&'D'];
    let mut ents = HashMap::new();
    for k in 0..d.len() / 2 {
        let (a, b) = (&d[2 * k], &d[2 * k + 1]);
        let de = 2 * k + 1;
        let kind: u32 = field(a, 0).parse().unwrap_or(0);
        let transform: usize = field(a, 6).parse().unwrap_or(0);
        let form: u32 = field(b, 4).parse().unwrap_or(0);
        let label = field(b, 7).to_string();
        let independent = field(a, 8).get(2..4).is_none_or(|sw| sw == "00");
        let params = pdata
            .get(&de)
            .map(|s| tokens(s, pd, rd))
            .unwrap_or_default();
        // The first parameter repeats the entity type.
        let params = if params.is_empty() {
            params
        } else {
            params[1..].to_vec()
        };
        ents.insert(
            de,
            DirEnt {
                kind,
                form,
                transform,
                label,
                params,
                independent,
            },
        );
    }
    let resolution = gt
        .get(18)
        .and_then(|t| t.f())
        .filter(|r| *r > 0.0)
        .unwrap_or(tol::POINT)
        * scale;
    Ok(File {
        ents,
        scale,
        resolution,
    })
}

struct Reader<'a> {
    f: &'a File,
    xb: XBuilder,
    verts: HashMap<(usize, usize), usize>,
    edges: HashMap<(usize, usize), Option<usize>>,
}

impl<'a> Reader<'a> {
    fn ent(&self, de: usize) -> R<&'a DirEnt> {
        self.f
            .ents
            .get(&de)
            .ok_or_else(|| format!("directory entry {de} is missing"))
    }
    fn num(e: &DirEnt, i: usize) -> R<f64> {
        e.params
            .get(i)
            .and_then(|t| t.f())
            .ok_or_else(|| format!("entity {} has too few parameters", e.kind))
    }
    /// A count or degree: a whole number no larger than the entity's parameter list (a larger
    /// one can't describe data the entity holds, and would overflow the index arithmetic).
    fn count(e: &DirEnt, i: usize) -> R<usize> {
        let x = Self::num(e, i)?;
        if x >= 0.0 && x.fract() == 0.0 && x <= e.params.len() as f64 {
            Ok(x as usize)
        } else {
            Err(format!("entity {} has a bad count {x}", e.kind))
        }
    }
    fn ptr(e: &DirEnt, i: usize) -> R<usize> {
        Ok(Self::num(e, i)?.abs() as usize)
    }

    /// The model placement of an entity's coordinates (its chain of 124 matrices), unscaled.
    fn transform(&self, de: usize, depth: usize) -> R<Affine> {
        if de == 0 || depth > 16 {
            return Ok(Affine::identity());
        }
        let e = self.ent(de)?;
        if e.kind != 124 {
            return Ok(Affine::identity());
        }
        let v: Vec<f64> = (0..12).map(|i| Self::num(e, i)).collect::<R<_>>()?;
        let m = nalgebra::Matrix3::new(v[0], v[1], v[2], v[4], v[5], v[6], v[8], v[9], v[10]);
        let a = Affine {
            m,
            t: V3::new(v[3], v[7], v[11]),
        };
        let parent = self.transform(e.transform, depth + 1)?;
        Ok(Affine {
            m: parent.m * a.m,
            t: parent.m * a.t + parent.t,
        })
    }

    /// An entity's placement into mm.
    fn place(&self, e: &DirEnt) -> R<Affine> {
        let t = self.transform(e.transform, 0)?;
        let s = self.f.scale;
        Ok(Affine {
            m: t.m * s,
            t: t.t * s,
        })
    }

    fn point(&self, de: usize) -> R<P3> {
        let e = self.ent(de)?;
        if e.kind != 116 {
            return Err(format!("entity {de} is not a point"));
        }
        Ok(self.place(e)?.point(&P3::new(
            Self::num(e, 0)?,
            Self::num(e, 1)?,
            Self::num(e, 2)?,
        )))
    }

    fn dir(&self, de: usize) -> R<V3> {
        let e = self.ent(de)?;
        if e.kind != 123 {
            return Err(format!("entity {de} is not a direction"));
        }
        let t = self.transform(e.transform, 0)?;
        let v = t.m * V3::new(Self::num(e, 0)?, Self::num(e, 1)?, Self::num(e, 2)?);
        if v.norm() < tol::TINY {
            return Err("a zero direction".into());
        }
        Ok(v.normalize())
    }

    fn nurbs_curve(e: &DirEnt) -> R<NurbsCurve> {
        let k = Self::count(e, 0)?;
        let m = Self::count(e, 1)?;
        let nk = k + m + 2;
        let base = 6;
        let knots: Vec<f64> = (0..nk).map(|i| Self::num(e, base + i)).collect::<R<_>>()?;
        let wb = base + nk;
        let cb = wb + k + 1;
        let mut cps = Vec::with_capacity(k + 1);
        for i in 0..=k {
            let w = Self::num(e, wb + i)?;
            let p = P3::new(
                Self::num(e, cb + 3 * i)?,
                Self::num(e, cb + 3 * i + 1)?,
                Self::num(e, cb + 3 * i + 2)?,
            );
            cps.push(hom(&p, if w > 0.0 { w } else { 1.0 }));
        }
        if m == 0 || k + 1 < m + 1 {
            return Err("a B-spline curve of degree 0".into());
        }
        NurbsCurve::new(m, knots, cps).map_err(|e| e.message())
    }

    fn curve(&self, de: usize) -> R<Curve> {
        let e = self.ent(de)?;
        let pl = self.place(e)?;
        let n = |i| Self::num(e, i);
        Ok(match e.kind {
            110 => {
                let (a, b) = (
                    pl.point(&P3::new(n(0)?, n(1)?, n(2)?)),
                    pl.point(&P3::new(n(3)?, n(4)?, n(5)?)),
                );
                let (c, _) = Curve::line(a, b);
                c
            }
            100 => {
                let (zt, cx, cy, sx, sy) = (n(0)?, n(1)?, n(2)?, n(3)?, n(4)?);
                let r = (sx - cx).hypot(sy - cy);
                let c = pl.point(&P3::new(cx, cy, zt));
                let s = pl.m.determinant().abs().cbrt();
                let x = pl.m * V3::x() / s;
                let y = pl.m * V3::y() / s;
                // The circle's parameter 0 along x (the edge's vertices set its range).
                Curve::Circle { c, x, y, r: r * s }
            }
            104 => {
                // A conic A x² + B xy + C y² + D x + E y + F = 0 at z = ZT: ellipses centred at the
                // origin along the axes (form 1).
                let (a, c, f, zt) = (n(0)?, n(2)?, n(5)?, n(6)?);
                if e.form != 1 || a * f >= 0.0 || c * f >= 0.0 {
                    return Err("only ellipses are supported among conic arcs".into());
                }
                let s = pl.m.determinant().abs().cbrt();
                let (ra, rb) = ((-f / a).sqrt() * s, (-f / c).sqrt() * s);
                Curve::Ellipse {
                    c: pl.point(&P3::new(0.0, 0.0, zt)),
                    x: pl.m * V3::x() / s,
                    y: pl.m * V3::y() / s,
                    a: ra,
                    b: rb,
                }
            }
            126 => {
                let c = Self::nurbs_curve(e)?;
                Curve::Nurbs(Arc::new(NurbsCurve {
                    cps: c
                        .cps
                        .iter()
                        .map(|h| hom(&pl.point(&dehom(h)), h.w))
                        .collect(),
                    ..c
                }))
            }
            k => return Err(format!("unsupported IGES curve type {k}")),
        })
    }

    /// The map of a parent's definition space applied to its children's results (already in
    /// mm): the parent's matrix, its translation scaled to mm.
    fn child_map(&self, e: &DirEnt) -> R<Affine> {
        let t = self.transform(e.transform, 0)?;
        Ok(Affine {
            m: t.m,
            t: t.t * self.f.scale,
        })
    }

    fn surface(&self, de: usize) -> R<Surface> {
        let e = self.ent(de)?;
        // Raw coordinates (pl, with the unit scale) and child entities (ch).
        let pl = self.place(e)?;
        let ch = self.child_map(e)?;
        let n = |i| Self::num(e, i);
        let p = |i| -> R<usize> { Self::ptr(e, i) };
        let dir = |d: usize| -> R<V3> { Ok((ch.m * self.dir(d)?).normalize()) };
        let frame = |o: usize, z: usize, x: Option<usize>| -> R<Frame> {
            let o = ch.point(&self.point(o)?);
            let z = dir(z)?;
            let x = match x {
                Some(x) if x != 0 => dir(x)?,
                _ => any_perpendicular(&z),
            };
            Ok(Frame::from_xz(o, x, z))
        };
        let sc = pl.m.determinant().abs().cbrt();
        Ok(match e.kind {
            190 => Surface::Plane {
                f: frame(p(0)?, p(1)?, if e.form == 1 { Some(p(2)?) } else { None })?,
            },
            108 => {
                // A x + B y + C z = D.
                let (a, b, c, d) = (n(0)?, n(1)?, n(2)?, n(3)?);
                let nn = V3::new(a, b, c);
                let l = nn.norm();
                if l < tol::TINY {
                    return Err("a plane with a zero normal".into());
                }
                let o = pl.point(&P3::from(nn * (d / (l * l))));
                Surface::Plane {
                    f: Frame::from_z(o, (pl.m * nn).normalize()),
                }
            }
            192 => Surface::Cylinder {
                f: frame(p(0)?, p(1)?, if e.form == 1 { Some(p(3)?) } else { None })?,
                r: n(2)? * sc,
            },
            194 => Surface::Cone {
                f: frame(p(0)?, p(1)?, if e.form == 1 { Some(p(4)?) } else { None })?,
                r: n(2)? * sc,
                a: n(3)?.to_radians(),
            },
            196 => {
                let f = if e.form == 1 {
                    frame(p(0)?, p(2)?, Some(p(3)?))?
                } else {
                    let o = ch.point(&self.point(p(0)?)?);
                    Frame::from_xz(o, ch.m * V3::x(), ch.m * V3::z())
                };
                Surface::Sphere { f, r: n(1)? * sc }
            }
            198 => Surface::Torus {
                f: frame(p(0)?, p(1)?, if e.form == 1 { Some(p(4)?) } else { None })?,
                rmaj: n(2)? * sc,
                rmin: n(3)? * sc,
            },
            120 => {
                let Curve::Line { o, d } = self.curve(p(0)?)?.transformed(&ch) else {
                    return Err("a surface of revolution's axis is not a line".into());
                };
                let d = d.normalize();
                let m = self.curve(p(1)?)?.transformed(&ch);
                let mid = match m.nurbs_range() {
                    Some((a, b)) => m.point((a + b) / 2.0),
                    None => m.point(0.0),
                };
                let w = mid - o;
                Surface::Revolution {
                    f: Frame::from_xz(o, w - d * w.dot(&d), d),
                    m,
                }
            }
            122 => {
                let c = self.curve(p(0)?)?.transformed(&ch);
                let a = c.nurbs_range().map(|r| r.0).unwrap_or(0.0);
                let d = pl.point(&P3::new(n(1)?, n(2)?, n(3)?)) - c.point(a);
                Surface::Extrusion { c, d }
            }
            128 => {
                let c = |i| Self::count(e, i);
                let (k1, k2, m1, m2) = (c(0)?, c(1)?, c(2)?, c(3)?);
                if m1 == 0 || m2 == 0 {
                    return Err("a B-spline surface of degree 0".into());
                }
                let base = 9;
                let (nk1, nk2) = (k1 + m1 + 2, k2 + m2 + 2);
                let ku: Vec<f64> = (0..nk1).map(|i| n(base + i)).collect::<R<_>>()?;
                let kv: Vec<f64> = (0..nk2).map(|i| n(base + nk1 + i)).collect::<R<_>>()?;
                let wb = base + nk1 + nk2;
                let cb = wb + (k1 + 1) * (k2 + 1);
                let mut cps = vec![vec![H::zeros(); k2 + 1]; k1 + 1];
                for j in 0..=k2 {
                    for (i, row) in cps.iter_mut().enumerate() {
                        // The first index varies fastest.
                        let idx = j * (k1 + 1) + i;
                        let w = n(wb + idx)?;
                        let q = pl.point(&P3::new(
                            n(cb + 3 * idx)?,
                            n(cb + 3 * idx + 1)?,
                            n(cb + 3 * idx + 2)?,
                        ));
                        row[j] = hom(&q, if w > 0.0 { w } else { 1.0 });
                    }
                }
                Surface::Nurbs(Arc::new(
                    NurbsSurface::new(m1, m2, ku, kv, cps).map_err(|e| e.message())?,
                ))
            }
            k => return Err(format!("unsupported IGES surface type {k}")),
        })
    }

    fn vertex(&mut self, list: usize, idx: usize) -> R<usize> {
        if let Some(&v) = self.verts.get(&(list, idx)) {
            return Ok(v);
        }
        let e = self.ent(list)?;
        if e.kind != 502 {
            return Err(format!("entity {list} is not a vertex list"));
        }
        let pl = self.place(e)?;
        // (1-based; an index no list holds is an error, not an overflow.)
        let k = idx
            .checked_sub(1)
            .and_then(|i| i.checked_mul(3))
            .map(|i| i + 1)
            .filter(|&k| k < e.params.len())
            .ok_or_else(|| format!("vertex {idx} of list {list} doesn't exist"))?;
        let p = pl.point(&P3::new(
            Self::num(e, k)?,
            Self::num(e, k + 1)?,
            Self::num(e, k + 2)?,
        ));
        let v = self.xb.vertex(p);
        self.verts.insert((list, idx), v);
        Ok(v)
    }

    fn edge(&mut self, list: usize, idx: usize) -> R<Option<usize>> {
        if let Some(&e) = self.edges.get(&(list, idx)) {
            return Ok(e);
        }
        let el = self.ent(list)?;
        if el.kind != 504 || idx == 0 {
            return Err(format!("entity {list} is not an edge list"));
        }
        let k = 1 + 5 * (idx - 1);
        let c = self.curve(Self::ptr(el, k)?)?;
        let a = self.vertex(Self::ptr(el, k + 1)?, Self::ptr(el, k + 2)?)?;
        let b = self.vertex(Self::ptr(el, k + 3)?, Self::ptr(el, k + 4)?)?;
        let e = self.xb.edge(c, a, b);
        self.edges.insert((list, idx), e);
        Ok(e)
    }

    fn face(&mut self, de: usize, agrees: bool) -> R<usize> {
        let e = self.ent(de)?;
        if e.kind != 510 {
            return Err(format!("entity {de} is not a face"));
        }
        let surf = self.surface(Self::ptr(e, 0)?)?;
        let n = Self::ptr(e, 1)?;
        let mut loops = Vec::new();
        for li in 0..n {
            let l = self.ent(Self::ptr(e, 3 + li)?)?;
            if l.kind != 508 {
                return Err("a face bound is not a loop".into());
            }
            let ne = Self::ptr(l, 0)?;
            let mut i = 1;
            let mut cs = Vec::new();
            for _ in 0..ne {
                let (ty, list, idx, of, k) = (
                    Self::ptr(l, i)?,
                    Self::ptr(l, i + 1)?,
                    Self::ptr(l, i + 2)?,
                    Self::num(l, i + 3)?,
                    Self::ptr(l, i + 4)?,
                );
                i += 5 + 2 * k;
                if ty != 0 {
                    continue; // a vertex: a singular point
                }
                if let Some(ed) = self.edge(list, idx)? {
                    cs.push((ed, of != 0.0));
                }
            }
            // Loops run about the surface's normal; a reversed face's run the other way round
            // about its outward normal.
            if !agrees {
                cs.reverse();
                for c in cs.iter_mut() {
                    c.1 = !c.1;
                }
            }
            loops.push(cs);
        }
        Ok(self.xb.face(XFace {
            surf,
            sense: agrees,
            loops,
        }))
    }

    fn shell(&mut self, de: usize, flip: bool) -> R<usize> {
        let e = self.ent(de)?;
        if e.kind != 514 {
            return Err(format!("entity {de} is not a shell"));
        }
        let n = Self::ptr(e, 0)?;
        let mut faces = Vec::new();
        for k in 0..n {
            let fde = Self::ptr(e, 1 + 2 * k)?;
            let of = Self::num(e, 2 + 2 * k)? != 0.0;
            match self.face(fde, of ^ flip) {
                Ok(f) => faces.push(f),
                Err(err) => self.xb.warnings.push(err),
            }
        }
        Ok(self.xb.shell(faces, e.form == 1))
    }
}

/// The Name property (406 form 15) among an entity's trailing pointers, else its label.
fn name_of(f: &File, e: &DirEnt, nparams: usize) -> String {
    let p = &e.params;
    // (Counts beyond the list can't be right: they stop the search, never overflow it.)
    let get = |i: usize| {
        p.get(i)
            .and_then(|t| t.f())
            .map(|x| x as usize)
            .filter(|&x| x <= p.len())
    };
    if let Some(na) = get(nparams)
        && let Some(np) = get(nparams + 1 + na)
    {
        for k in 0..np {
            if let Some(pe) = get(nparams + 2 + na + k).and_then(|d| f.ents.get(&d))
                && pe.kind == 406
                && pe.form == 15
                && let Some(Tok::Str(s)) = pe.params.get(1)
            {
                return s.clone();
            }
        }
    }
    e.label.clone()
}

/// A piece of a trimmed surface's boundary: a 3D curve from `a` to `b`.
type Seg = super::xbuild::LooseSeg;

impl<'a> Reader<'a> {
    /// The 3D segments of a model space boundary curve, composites (102) flattened, in order.
    fn segments(&self, de: usize, out: &mut Vec<Seg>, depth: usize) -> R<()> {
        let e = self.ent(de)?;
        let pl = self.place(e)?;
        let n = |i| Self::num(e, i);
        match e.kind {
            102 if depth < 8 => {
                for k in 0..Self::ptr(e, 0)? {
                    self.segments(Self::ptr(e, 1 + k)?, out, depth + 1)?;
                }
            }
            110 => {
                let (a, b) = (
                    pl.point(&P3::new(n(0)?, n(1)?, n(2)?)),
                    pl.point(&P3::new(n(3)?, n(4)?, n(5)?)),
                );
                if (b - a).norm() > tol::DEGENERATE {
                    out.push(Seg {
                        c: Curve::line(a, b).0,
                        a,
                        b,
                    });
                }
            }
            100 => {
                let zt = n(0)?;
                let (a, b) = (
                    pl.point(&P3::new(n(3)?, n(4)?, zt)),
                    pl.point(&P3::new(n(5)?, n(6)?, zt)),
                );
                out.push(Seg {
                    c: self.curve(de)?,
                    a,
                    b,
                });
            }
            104 => {
                let zt = n(6)?;
                let (a, b) = (
                    pl.point(&P3::new(n(7)?, n(8)?, zt)),
                    pl.point(&P3::new(n(9)?, n(10)?, zt)),
                );
                out.push(Seg {
                    c: self.curve(de)?,
                    a,
                    b,
                });
            }
            126 => {
                let k = Self::count(e, 0)?;
                let m = Self::count(e, 1)?;
                let vb = 6 + (k + m + 2) + (k + 1) + 3 * (k + 1);
                let c = self.curve(de)?;
                let (r0, r1) = c.nurbs_range().unwrap_or((0.0, 1.0));
                let (v0, v1) = (
                    n(vb).unwrap_or(r0).clamp(r0, r1),
                    n(vb + 1).unwrap_or(r1).clamp(r0, r1),
                );
                let c = match &c {
                    // A sub-range of the spline: trim it, so its ends are the segment's.
                    Curve::Nurbs(nc)
                        if v0 > r0 + tol::ROUNDOFF_FINE || v1 < r1 - tol::ROUNDOFF_FINE =>
                    {
                        Curve::Nurbs(Arc::new(
                            Curve::Nurbs(nc.clone()).to_nurbs(v0.min(v1), v0.max(v1)),
                        ))
                    }
                    _ => c,
                };
                let (a, b) = (c.point(v0), c.point(v1));
                out.push(Seg { c, a, b });
            }
            k => return Err(format!("unsupported IGES boundary curve type {k}")),
        }
        Ok(())
    }

    /// A curve on a surface (142): its model space curve, or its parameter space curve mapped
    /// through a spline surface.
    fn boundary(&self, de: usize, surf: &Surface) -> R<Vec<Seg>> {
        let e = self.ent(de)?;
        let mut out = Vec::new();
        match e.kind {
            142 => {
                let (bptr, cptr) = (Self::ptr(e, 2)?, Self::ptr(e, 3)?);
                if cptr != 0 {
                    self.segments(cptr, &mut out, 0)?;
                } else if bptr != 0 {
                    let Surface::Nurbs(_) = surf else {
                        return Err("a parameter space boundary on a non-spline surface".into());
                    };
                    let mut uv = Vec::new();
                    // Parameter curves are in the surface's own space: no unit scale.
                    self.segments(bptr, &mut uv, 0)?;
                    for sg in uv {
                        let s = self.f.scale;
                        let unscale = |p: &P3| P3::new(p.x / s, p.y / s, 0.0);
                        let (pa, pb) = (unscale(&sg.a), unscale(&sg.b));
                        let c2 = sg.c.transformed(&Affine {
                            m: nalgebra::Matrix3::identity() / s,
                            t: V3::zeros(),
                        });
                        let (t0, t1) = super::xbuild::edge_range(
                            &c2,
                            &pa,
                            &pb,
                            (pa - pb).norm() < tol::DEGENERATE,
                        )
                        .unwrap_or((0.0, 1.0));
                        let ts: Vec<f64> =
                            (0..=32).map(|i| t0 + (t1 - t0) * i as f64 / 32.0).collect();
                        let pts: Vec<P3> = ts
                            .iter()
                            .map(|&t| {
                                let q = c2.point(t);
                                surf.eval(q.x, q.y)
                            })
                            .collect();
                        let fit = NurbsCurve::interpolate(&pts, &ts, 3, None, None);
                        out.push(Seg {
                            a: pts[0],
                            b: pts[32],
                            c: Curve::Nurbs(Arc::new(fit)),
                        });
                    }
                } else {
                    return Err("a boundary with no curve".into());
                }
            }
            141 => {
                // TYPE, PREF, SPTR, N, then per curve: CRVPT, SENSE (1 along, 2 reversed), K, K pcurves.
                let nc = Self::ptr(e, 3)?;
                let mut i = 4;
                for _ in 0..nc {
                    let (cv, sense, k) =
                        (Self::ptr(e, i)?, Self::ptr(e, i + 1)?, Self::ptr(e, i + 2)?);
                    i += 3 + k;
                    let mut segs = Vec::new();
                    self.segments(cv, &mut segs, 0)?;
                    if sense == 2 {
                        segs.reverse();
                        for sg in segs.iter_mut() {
                            std::mem::swap(&mut sg.a, &mut sg.b);
                        }
                    }
                    out.extend(segs);
                }
            }
            k => return Err(format!("unsupported IGES boundary type {k}")),
        }
        Ok(out)
    }
}

/// Sews trimmed (144) and bounded (143) surfaces — faces with no topology — by their boundary
/// curves: vertices welded within the file's resolution, segments matched to shared edges,
/// each connected group of faces a shell (a solid when closed). The faces' orientation is then
/// made consistent and outward ([`super::xbuild::orient_shells`]).
fn read_trimmed(f: &File, out: &mut ReadModel) {
    let mut des: Vec<usize> = f
        .ents
        .iter()
        .filter(|(_, e)| e.kind == 144 || e.kind == 143)
        .map(|(d, _)| *d)
        .collect();
    if des.is_empty() {
        return;
    }
    des.sort_unstable();
    // The weld distance: ten times the file's resolution, within 1e-9 and 1e-4 of the model's
    // size.
    let size = des
        .iter()
        .filter_map(|d| {
            let e = &f.ents[d];
            let r = Reader {
                f,
                xb: XBuilder::new(),
                verts: HashMap::new(),
                edges: HashMap::new(),
            };
            let mut segs = Vec::new();
            r.boundary(
                Reader::ptr(e, 3).ok()?,
                &Surface::Plane { f: Frame::world() },
            )
            .ok()
            .map(|s| segs = s)?;
            Some(
                segs.iter()
                    .map(|s| s.a.coords.amax().max(-s.a.coords.min()))
                    .fold(0.0, f64::max),
            )
        })
        .fold(0.0, f64::max)
        .max(1e-3);
    let tol = (f.resolution * 10.0)
        .clamp(tol::REL * size, 1e-4 * size)
        .max(tol::ROUNDOFF);
    let r = Reader {
        f,
        xb: XBuilder::new(),
        verts: HashMap::new(),
        edges: HashMap::new(),
    };
    let mut loose = Vec::new();
    for de in des {
        let e = &f.ents[&de];
        let res = (|| -> R<(Surface, Vec<Vec<Seg>>)> {
            let surf = r.surface(Reader::ptr(e, 0)?)?;
            let mut loops = Vec::new();
            if e.kind == 144 {
                let (n1, n2, pto) = (Reader::ptr(e, 1)?, Reader::ptr(e, 2)?, Reader::ptr(e, 3)?);
                loops.push(if n1 == 0 || pto == 0 {
                    super::xbuild::natural_boundary(&surf)?
                } else {
                    r.boundary(pto, &surf)?
                });
                for k in 0..n2 {
                    loops.push(r.boundary(Reader::ptr(e, 4 + k)?, &surf)?);
                }
            } else {
                // 143: TYPE, SPTR, N, boundaries (the first outer).
                let n = Reader::ptr(e, 2)?;
                for k in 0..n {
                    loops.push(r.boundary(Reader::ptr(e, 3 + k)?, &surf)?);
                }
            }
            Ok((surf, loops))
        })();
        match res {
            Ok(x) => loose.push(x),
            Err(err) => out.warnings.push(format!("IGES face {de}: {err}")),
        }
    }
    let (body, w) = super::xbuild::sew_loose(loose, tol, true);
    out.warnings.extend(w);

    // Each solid a part; the loose sheets together.
    for piece in super::exchange::split_into_solids(&body) {
        if !piece.faces.is_empty() {
            out.occurrences.push(ReadOccurrence {
                part: out.parts.len(),
                placement: Affine::identity(),
                name: String::new(),
                color: None,
            });
            out.parts.push(ReadPart {
                body: piece,
                name: String::new(),
                colors: Vec::new(),
            });
        }
    }
}

/// Reads an IGES file's MSBO solids and free shells, or else its trimmed surfaces sewn, as
/// parts at the identity.
/// A file of curves and points (no B-rep, no trimmed surfaces): each 402 group of them a wire
/// body named after the group, the independent ones in no group one more. Curves are welded
/// end to end within ten times the file's resolution and chained into wires.
fn read_wires(f: &File, out: &mut ReadModel) {
    let is_curve = |k: u32| matches!(k, 100 | 102 | 104 | 110 | 126);
    let mut des: Vec<usize> = f.ents.keys().copied().collect();
    des.sort_unstable();
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    let mut grouped = std::collections::HashSet::new();
    for &de in &des {
        let e = &f.ents[&de];
        if e.kind != 402 {
            continue;
        }
        let n = Reader::count(e, 0).unwrap_or(0);
        let members: Vec<usize> = (0..n)
            .filter_map(|k| Reader::ptr(e, 1 + k).ok())
            .filter(|m| {
                f.ents
                    .get(m)
                    .is_some_and(|x| is_curve(x.kind) || x.kind == 116)
            })
            .collect();
        if !members.is_empty() {
            grouped.extend(members.iter().copied());
            groups.push((name_of(f, e, 1 + n), members));
        }
    }
    let loose: Vec<usize> = des
        .iter()
        .copied()
        .filter(|d| {
            !grouped.contains(d)
                && f.ents[d].independent
                && (is_curve(f.ents[d].kind) || f.ents[d].kind == 116)
        })
        .collect();
    if !loose.is_empty() {
        groups.push((String::new(), loose));
    }
    let tol = (f.resolution * 10.0).max(tol::ROUNDOFF);
    for (name, members) in groups {
        let r = Reader {
            f,
            xb: XBuilder::new(),
            verts: HashMap::new(),
            edges: HashMap::new(),
        };
        let (mut segs, mut pts) = (Vec::new(), Vec::new());
        for de in members {
            if f.ents[&de].kind == 116 {
                match r.point(de) {
                    Ok(p) => pts.push(p),
                    Err(e) => out.warnings.push(e),
                }
                continue;
            }
            let mut ss = Vec::new();
            if let Err(e) = r.segments(de, &mut ss, 0) {
                out.warnings.push(e);
            }
            for s in ss {
                let closed = (s.a - s.b).norm() <= tol && s.c.is_periodic();
                segs.push(super::step::WireSeg {
                    c: s.c,
                    a: s.a,
                    b: s.b,
                    closed,
                });
            }
        }
        let body = super::step::wire_body(&segs, &pts, tol);
        if !body.wires.is_empty() || !body.points.is_empty() {
            out.occurrences.push(ReadOccurrence {
                part: out.parts.len(),
                placement: Affine::identity(),
                name: name.clone(),
                color: None,
            });
            out.parts.push(ReadPart {
                body,
                name,
                colors: Vec::new(),
            });
        }
    }
}

pub fn read(bytes: &[u8]) -> Result<ReadModel, String> {
    let out = read_at(bytes, 1.0)?;
    // A model smaller than a boolean's small one ([`super::boolean::SMALL_MODEL`]) is read
    // again larger, by a power of two, and scaled back: the reader's join distances are
    // lengths in mm, the size of a part a few microns across (as STEP's `Model::boost_of`).
    let mut bb = Bbox::empty();
    for p in &out.parts {
        bb.union(&p.body.bbox());
    }
    let size = if bb.is_empty() { 0.0 } else { bb.diagonal() };
    if !(size > 0.0 && size < super::boolean::SMALL_MODEL) {
        return Ok(out);
    }
    let boost = (super::boolean::WORKING_SIZE / size).log2().floor().exp2();
    let Ok(mut big) = read_at(bytes, boost) else {
        return Ok(out);
    };
    let shrink = Affine {
        m: nalgebra::Matrix3::identity() / boost,
        t: V3::zeros(),
    };
    for p in &mut big.parts {
        p.body = p.body.transformed(&shrink);
    }
    for o in &mut big.occurrences {
        o.placement.t /= boost;
    }
    Ok(big)
}

/// [`read`] with every length `boost` times what the file says.
fn read_at(bytes: &[u8], boost: f64) -> Result<ReadModel, String> {
    let mut f = parse(bytes)?;
    f.scale *= boost;
    f.resolution *= boost;
    let mut des: Vec<usize> = f.ents.keys().copied().collect();
    des.sort_unstable();
    let mut used_shells = std::collections::HashSet::new();
    let mut out = ReadModel::default();
    for &de in &des {
        let e = &f.ents[&de];
        if e.kind != 186 {
            continue;
        }
        let mut r = Reader {
            f: &f,
            xb: XBuilder::new(),
            verts: HashMap::new(),
            edges: HashMap::new(),
        };
        let res = (|| -> R<String> {
            let sh = Reader::ptr(e, 0)?;
            let sof = Reader::num(e, 1)? != 0.0;
            let nv = Reader::ptr(e, 2)?;
            used_shells.insert(sh);
            let mut shells = vec![r.shell(sh, !sof)?];
            for k in 0..nv {
                let v = Reader::ptr(e, 3 + 2 * k)?;
                let vof = Reader::num(e, 4 + 2 * k)? != 0.0;
                used_shells.insert(v);
                shells.push(r.shell(v, !vof)?);
            }
            r.xb.solid(shells);
            Ok(name_of(&f, e, 3 + 2 * nv))
        })();
        match res {
            Ok(name) => {
                let (body, w) = r.xb.finish();
                out.warnings.extend(w);
                if !body.faces.is_empty() {
                    out.occurrences.push(ReadOccurrence {
                        part: out.parts.len(),
                        placement: Affine::identity(),
                        name: name.clone(),
                        color: None,
                    });
                    out.parts.push(ReadPart {
                        body,
                        name,
                        colors: Vec::new(),
                    });
                }
            }
            Err(err) => out.warnings.push(err),
        }
    }
    // Shells of no solid: closed ones as solids, open ones as sheets.
    for &de in &des {
        let e = &f.ents[&de];
        if e.kind != 514 || used_shells.contains(&de) {
            continue;
        }
        let mut r = Reader {
            f: &f,
            xb: XBuilder::new(),
            verts: HashMap::new(),
            edges: HashMap::new(),
        };
        match r.shell(de, false) {
            Ok(s) => {
                if e.form == 1 {
                    r.xb.solid(vec![s]);
                }
                let name = name_of(&f, e, 1 + 2 * Reader::ptr(e, 0).unwrap_or(0));
                let (body, w) = r.xb.finish();
                out.warnings.extend(w);
                if !body.faces.is_empty() {
                    out.occurrences.push(ReadOccurrence {
                        part: out.parts.len(),
                        placement: Affine::identity(),
                        name: name.clone(),
                        color: None,
                    });
                    out.parts.push(ReadPart {
                        body,
                        name,
                        colors: Vec::new(),
                    });
                }
            }
            Err(err) => out.warnings.push(err),
        }
    }
    if out.parts.is_empty() {
        read_trimmed(&f, &mut out);
    }
    if out.parts.is_empty() {
        read_wires(&f, &mut out);
    }
    if out.parts.is_empty() {
        return Err(if let Some(w) = out.warnings.first() {
            format!("the file holds no B-rep we can read ({w})")
        } else {
            "the file holds no B-rep solids, shells or trimmed surfaces".into()
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 10 mm cube as six trimmed planes (108 + 144, boundaries 142 of 102 composites of lines),
    /// half of them bounded the wrong way round: sewn into one outward solid.
    #[test]
    fn sews_trimmed_surfaces_into_a_solid() {
        trimmed_cube(10.0);
    }

    /// The same four nanometres across, under the distance points are joined at (it sewed
    /// before small models were read larger too: the cube's corners are welded by edge): read at a working size and scaled back, where the sewing's
    /// distances (lengths in mm) are a good share of it.
    #[test]
    fn sews_a_micron_cube_of_trimmed_surfaces() {
        trimmed_cube(4e-6);
    }

    fn trimmed_cube(side: f64) {
        let c = |x: f64, y: f64, z: f64| P3::new(x * side, y * side, z * side);
        let quads = [
            [c(0., 0., 0.), c(0., 1., 0.), c(1., 1., 0.), c(1., 0., 0.)],
            [c(0., 0., 1.), c(1., 0., 1.), c(1., 1., 1.), c(0., 1., 1.)],
            [c(0., 0., 0.), c(1., 0., 0.), c(1., 0., 1.), c(0., 0., 1.)],
            [c(0., 1., 0.), c(0., 1., 1.), c(1., 1., 1.), c(1., 1., 0.)],
            [c(0., 0., 0.), c(0., 0., 1.), c(0., 1., 1.), c(0., 1., 0.)],
            [c(1., 0., 0.), c(1., 1., 0.), c(1., 1., 1.), c(1., 0., 1.)],
        ];
        let mut w = Writer::default();
        for (k, q) in quads.iter().enumerate() {
            let q: Vec<P3> = if k % 2 == 0 {
                q.to_vec()
            } else {
                q.iter().rev().copied().collect()
            };
            let n = (q[1] - q[0]).cross(&(q[2] - q[0])).normalize();
            let d = n.dot(&q[0].coords);
            let plane = w.add(
                108,
                0,
                [n.x, n.y, n.z, d]
                    .iter()
                    .map(|x| real(*x))
                    .chain([
                        "0".into(),
                        "0.".into(),
                        "0.".into(),
                        "0.".into(),
                        "0.".into(),
                    ])
                    .collect(),
            );
            let mut comp = vec!["4".to_string()];
            for i in 0..4 {
                let (a, b) = (q[i], q[(i + 1) % 4]);
                comp.push(
                    w.add(
                        110,
                        0,
                        [a.x, a.y, a.z, b.x, b.y, b.z]
                            .iter()
                            .map(|x| real(*x))
                            .collect(),
                    )
                    .to_string(),
                );
            }
            let comp = w.add(102, 0, comp);
            let bnd = w.add(
                142,
                0,
                vec![
                    "0".into(),
                    plane.to_string(),
                    "0".into(),
                    comp.to_string(),
                    "2".into(),
                ],
            );
            w.add(
                144,
                0,
                vec![plane.to_string(), "1".into(), "0".into(), bnd.to_string()],
            );
        }
        let m = read(&w.finish(10.0)).unwrap();
        assert_eq!(m.parts.len(), 1);
        let b = &m.parts[0].body;
        assert!(b.check().is_empty(), "{:?}", b.check());
        assert_eq!((b.solids.len(), b.faces.len(), b.edges.len()), (1, 6, 12));
        let mp = super::super::mass::body_mass(b);
        assert!(
            (mp.volume / side.powi(3) - 1.0).abs() < 1e-12
                && (mp.surface_area / (6.0 * side * side) - 1.0).abs() < 1e-12,
            "{mp:?}"
        );
    }

    #[test]
    fn tokens_with_hollerith_and_d_exponents() {
        let t = tokens("110,1.5D0,,3HA,B,2.E-3;ignored", ',', ';');
        assert_eq!(
            t,
            vec![
                Tok::Num(110.0),
                Tok::Num(1.5),
                Tok::Empty,
                Tok::Str("A,B".into()),
                Tok::Num(0.002)
            ]
        );
    }
}
