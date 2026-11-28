//! Reading OCCT's BRep text format (`.brep` files as `BRepTools::Write` and DRAW's `save` write
//! them; versions 1 to 3, with or without DRAW's `DBRep_DrawableShape` header).
//!
//! **The file.** After the version line come tables, each a keyword and a count: `Locations`
//! (elementary 3×4 matrices, `1 …`, and products of earlier ones raised to powers, `2 l p … 0`),
//! `Curve2ds`, `Curves`, `Polygon3D`, `PolygonOnTriangulations`, `Surfaces`, `Triangulations`,
//! then `TShapes`: each a type (`Ve Ed Wi Fa Sh So CS Co`), its geometry, a flags word and its
//! sub-shapes (`+n l`, `-n l`, `in l`, `en l`: an orientation, a shape counted back from the end
//! of the table, a location; `*` ends the list). The last reference is the shape itself.
//!
//! **Geometry.** Curves: line, circle, ellipse, parabola (an exact quadratic on the edge's range),
//! hyperbola (interpolated), Bézier, B-spline (periodic ones unrolled), trimmed (its basis) and
//! offset (exact for lines and circles in their plane, else interpolated). Surfaces: plane,
//! cylinder, cone, sphere, torus, linear extrusion, revolution, Bézier, B-spline, rectangular
//! trimmed (its basis) and offset (exact for the elementary surfaces, else interpolated).
//! A spline edge curve is cut to the edge's range.
//!
//! **Topology.** A sub-shape's place is its parents' locations composed with its own, and its
//! orientation theirs composed with its own (as `TopAbs::Compose`). Two uses of one TShape with
//! the same composed location are one vertex or edge. The body is rebuilt by
//! [`super::xbuild::XBuilder`] from the 3D geometry (the file's p-curves aren't needed): our own
//! p-curves, pole edges and seams; the file's degenerate edges are left out (they are put back),
//! as are internal and external edges. An edge the file gives only p-curves gets a 3D curve
//! lifted from its first (OCCT's `BRepLib::BuildCurves3d`). Solids keep their shells; shells
//! and faces outside a solid become sheets: a shell whose edges don't close it is open whatever
//! its flag says, and faces of no shell make a shell per edge-connected piece.

use std::collections::HashMap;
use std::sync::Arc;

use super::curve::Curve;
use super::math::*;
use super::nurbs::{H, NurbsCurve, NurbsSurface, hom};
use super::surface::Surface;
use super::tol;
use super::topo::{Body, Coedge, Face, Shell, Solid, TOL_MIN, co_range};
use super::xbuild::{XBuilder, XFace};

type R<T> = Result<T, String>;

// ---------------------------------------------------------------------------------------------
// Tokens

struct Tok<'a> {
    it: std::iter::Peekable<std::str::SplitAsciiWhitespace<'a>>,
}

impl<'a> Tok<'a> {
    fn next(&mut self) -> R<&'a str> {
        self.it
            .next()
            .ok_or_else(|| "unexpected end of file".to_string())
    }
    fn peek(&mut self) -> Option<&'a str> {
        self.it.peek().copied()
    }
    fn f(&mut self) -> R<f64> {
        let t = self.next()?;
        t.parse::<f64>().map_err(|_| format!("not a number: {t}"))
    }
    fn i(&mut self) -> R<i64> {
        let t = self.next()?;
        t.parse::<i64>().map_err(|_| format!("not an integer: {t}"))
    }
    fn u(&mut self) -> R<usize> {
        let v = self.i()?;
        usize::try_from(v).map_err(|_| format!("not a count: {v}"))
    }
    fn p3(&mut self) -> R<P3> {
        Ok(P3::new(self.f()?, self.f()?, self.f()?))
    }
    fn v3(&mut self) -> R<V3> {
        Ok(V3::new(self.f()?, self.f()?, self.f()?))
    }
    fn expect(&mut self, kw: &str) -> R<()> {
        let t = self.next()?;
        if t == kw {
            Ok(())
        } else {
            Err(format!("expected {kw}, found {t}"))
        }
    }
    /// A table's count when the next keyword is `kw` (tables a writer may leave out).
    fn table(&mut self, kw: &str) -> R<usize> {
        if self.peek() == Some(kw) {
            self.next()?;
            self.u()
        } else {
            Ok(0)
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Geometry as the file has it

/// A coordinate system: origin, main direction, x and y (y may make it left-handed).
#[derive(Debug, Clone, Copy)]
struct Ax {
    o: P3,
    z: V3,
    x: V3,
    y: V3,
}

impl Ax {
    fn read(t: &mut Tok) -> R<Self> {
        Ok(Self {
            o: t.p3()?,
            z: t.v3()?,
            x: t.v3()?,
            y: t.v3()?,
        })
    }
    fn frame(&self) -> Frame {
        Frame::from_xz(self.o, self.x, self.z)
    }
    /// The file's own axes (left-handed ones too), so (u, v) are OCCT's.
    fn frame_exact(&self) -> Frame {
        Frame {
            o: self.o,
            x: self.x.normalize(),
            y: self.y.normalize(),
            z: self.z.normalize(),
        }
    }
    /// +1 for a right-handed system, −1 for a left-handed one (whose surfaces' natural normals
    /// point the other way from ours).
    fn hand(&self) -> f64 {
        if self.x.cross(&self.y).dot(&self.z) >= 0.0 {
            1.0
        } else {
            -1.0
        }
    }
}

/// A B-spline as OCCT holds it: poles, weights, distinct knots with multiplicities.
#[derive(Debug, Clone)]
pub(crate) struct Spline {
    pub degree: usize,
    pub poles: Vec<H>,
    pub knots: Vec<f64>,
    pub mults: Vec<usize>,
    pub periodic: bool,
}

#[derive(Debug, Clone)]
enum GCurve {
    Line(P3, V3),
    Circle(Ax, f64),
    Ellipse(Ax, f64, f64),
    Parabola(Ax, f64),
    Hyperbola(Ax, f64, f64),
    Spline(Spline),
    Trimmed(Box<GCurve>),
    Offset(f64, V3, Box<GCurve>),
}

#[derive(Debug, Clone)]
enum GSurf {
    Plane(Ax),
    Cylinder(Ax, f64),
    Cone(Ax, f64, f64),
    Sphere(Ax, f64),
    Torus(Ax, f64, f64),
    Extrusion(V3, GCurve),
    Revolution(P3, V3, GCurve),
    /// Poles along u (outer) and v (inner), and the two directions' knots.
    Spline {
        du: usize,
        dv: usize,
        poles: Vec<Vec<H>>,
        u: (Vec<f64>, Vec<usize>, bool),
        v: (Vec<f64>, Vec<usize>, bool),
    },
    Trimmed(Box<GSurf>),
    Offset(f64, Box<GSurf>),
}

/// The largest B-spline degree a file may give (OCCT's `Geom_BSplineCurve::MaxDegree`).
const MAX_DEGREE: usize = 25;

/// A degree read from the file, checked.
fn degree(t: &mut Tok) -> R<usize> {
    let d = t.u()?;
    if d > MAX_DEGREE {
        return Err(format!("a B-spline of degree {d}"));
    }
    Ok(d)
}

fn read_poles(t: &mut Tok, n: usize, rational: bool) -> R<Vec<H>> {
    (0..n)
        .map(|_| {
            let p = t.p3()?;
            let w = if rational { t.f()? } else { 1.0 };
            Ok(hom(&p, w))
        })
        .collect()
}

fn read_knots(t: &mut Tok, n: usize) -> R<(Vec<f64>, Vec<usize>)> {
    let (mut k, mut m) = (Vec::new(), Vec::new());
    for _ in 0..n {
        k.push(t.f()?);
        m.push(t.u()?);
    }
    Ok((k, m))
}

fn read_curve(t: &mut Tok) -> R<GCurve> {
    Ok(match t.i()? {
        1 => GCurve::Line(t.p3()?, t.v3()?),
        2 => GCurve::Circle(Ax::read(t)?, t.f()?),
        3 => GCurve::Ellipse(Ax::read(t)?, t.f()?, t.f()?),
        4 => GCurve::Parabola(Ax::read(t)?, t.f()?),
        5 => GCurve::Hyperbola(Ax::read(t)?, t.f()?, t.f()?),
        6 => {
            let rational = t.i()? != 0;
            let degree = degree(t)?;
            let poles = read_poles(t, degree + 1, rational)?;
            GCurve::Spline(Spline {
                degree,
                poles,
                knots: vec![0.0, 1.0],
                mults: vec![degree + 1; 2],
                periodic: false,
            })
        }
        7 => {
            let rational = t.i()? != 0;
            let periodic = t.i()? != 0;
            let (degree, np, nk) = (degree(t)?, t.u()?, t.u()?);
            let poles = read_poles(t, np, rational)?;
            let (knots, mults) = read_knots(t, nk)?;
            GCurve::Spline(Spline {
                degree,
                poles,
                knots,
                mults,
                periodic,
            })
        }
        8 => {
            let (_a, _b) = (t.f()?, t.f()?);
            GCurve::Trimmed(Box::new(read_curve(t)?))
        }
        9 => {
            let d = t.f()?;
            let v = t.v3()?;
            GCurve::Offset(d, v, Box::new(read_curve(t)?))
        }
        k => return Err(format!("unknown curve type {k}")),
    })
}

fn p2(t: &mut Tok) -> R<P3> {
    Ok(P3::new(t.f()?, t.f()?, 0.0))
}

fn d2(t: &mut Tok) -> R<V3> {
    Ok(V3::new(t.f()?, t.f()?, 0.0))
}

fn ax2(t: &mut Tok) -> R<Ax> {
    let (o, x, y) = (p2(t)?, d2(t)?, d2(t)?);
    Ok(Ax {
        o,
        z: x.cross(&y),
        x,
        y,
    })
}

fn poles2(t: &mut Tok, n: usize, rational: bool) -> R<Vec<H>> {
    (0..n)
        .map(|_| {
            let p = p2(t)?;
            let w = if rational { t.f()? } else { 1.0 };
            Ok(hom(&p, w))
        })
        .collect()
}

/// A 2D curve (a p-curve), as a curve in the z = 0 plane.
fn read_curve2d(t: &mut Tok) -> R<GCurve> {
    Ok(match t.i()? {
        1 => GCurve::Line(p2(t)?, d2(t)?),
        2 => GCurve::Circle(ax2(t)?, t.f()?),
        3 => GCurve::Ellipse(ax2(t)?, t.f()?, t.f()?),
        4 => GCurve::Parabola(ax2(t)?, t.f()?),
        5 => GCurve::Hyperbola(ax2(t)?, t.f()?, t.f()?),
        6 => {
            let rational = t.i()? != 0;
            let degree = degree(t)?;
            let poles = poles2(t, degree + 1, rational)?;
            GCurve::Spline(Spline {
                degree,
                poles,
                knots: vec![0.0, 1.0],
                mults: vec![degree + 1; 2],
                periodic: false,
            })
        }
        7 => {
            let rational = t.i()? != 0;
            let periodic = t.i()? != 0;
            let (degree, np, nk) = (degree(t)?, t.u()?, t.u()?);
            let poles = poles2(t, np, rational)?;
            let (knots, mults) = read_knots(t, nk)?;
            GCurve::Spline(Spline {
                degree,
                poles,
                knots,
                mults,
                periodic,
            })
        }
        8 => {
            skip(t, 2)?;
            GCurve::Trimmed(Box::new(read_curve2d(t)?))
        }
        9 => {
            // Offset to the right of the direction of travel: T × z.
            let d = t.f()?;
            GCurve::Offset(d, V3::z(), Box::new(read_curve2d(t)?))
        }
        k => return Err(format!("unknown 2D curve type {k}")),
    })
}

fn skip(t: &mut Tok, n: usize) -> R<()> {
    for _ in 0..n {
        t.next()?;
    }
    Ok(())
}

fn read_surface(t: &mut Tok) -> R<GSurf> {
    Ok(match t.i()? {
        1 => GSurf::Plane(Ax::read(t)?),
        2 => GSurf::Cylinder(Ax::read(t)?, t.f()?),
        3 => GSurf::Cone(Ax::read(t)?, t.f()?, t.f()?),
        4 => GSurf::Sphere(Ax::read(t)?, t.f()?),
        5 => GSurf::Torus(Ax::read(t)?, t.f()?, t.f()?),
        6 => {
            let d = t.v3()?;
            GSurf::Extrusion(d, read_curve(t)?)
        }
        7 => {
            let (o, d) = (t.p3()?, t.v3()?);
            GSurf::Revolution(o, d, read_curve(t)?)
        }
        8 => {
            let (ur, vr) = (t.i()? != 0, t.i()? != 0);
            let (du, dv) = (degree(t)?, degree(t)?);
            let poles = (0..=du)
                .map(|_| read_poles(t, dv + 1, ur || vr))
                .collect::<R<Vec<_>>>()?;
            GSurf::Spline {
                du,
                dv,
                poles,
                u: (vec![0.0, 1.0], vec![du + 1; 2], false),
                v: (vec![0.0, 1.0], vec![dv + 1; 2], false),
            }
        }
        9 => {
            let (ur, vr) = (t.i()? != 0, t.i()? != 0);
            let (up, vp) = (t.i()? != 0, t.i()? != 0);
            let (du, dv) = (degree(t)?, degree(t)?);
            let (nu, nv) = (t.u()?, t.u()?);
            let (nku, nkv) = (t.u()?, t.u()?);
            let poles = (0..nu)
                .map(|_| read_poles(t, nv, ur || vr))
                .collect::<R<Vec<_>>>()?;
            let (ku, mu) = read_knots(t, nku)?;
            let (kv, mv) = read_knots(t, nkv)?;
            GSurf::Spline {
                du,
                dv,
                poles,
                u: (ku, mu, up),
                v: (kv, mv, vp),
            }
        }
        10 => {
            skip(t, 4)?;
            GSurf::Trimmed(Box::new(read_surface(t)?))
        }
        11 => {
            let d = t.f()?;
            GSurf::Offset(d, Box::new(read_surface(t)?))
        }
        k => return Err(format!("unknown surface type {k}")),
    })
}

// ---------------------------------------------------------------------------------------------
// Splines

/// The knot vector and poles of a spline whose domain covers [t0, t1] (a periodic one unrolled
/// over enough periods), cut to [t0, t1] when that lies inside.
pub(crate) fn spline_curve(s: &Spline, t0: f64, t1: f64) -> R<NurbsCurve> {
    let p = s.degree;
    if p == 0 || p > MAX_DEGREE || s.knots.len() < 2 || s.knots.len() != s.mults.len() {
        return Err("a bad B-spline".into());
    }
    // The flat knots' count before making them (a count from the file could be anything).
    let flat = s
        .mults
        .iter()
        .try_fold(0usize, |a, &m| a.checked_add(m))
        .filter(|&n| n <= s.poles.len() + 2 * p + 2);
    if flat.is_none() || s.knots.iter().any(|k| !k.is_finite()) {
        return Err("a B-spline's knot multiplicities don't fit its poles".into());
    }
    let (a, b) = (s.knots[0], s.knots[s.knots.len() - 1]);
    if !s.periodic {
        let knots: Vec<f64> = s
            .knots
            .iter()
            .zip(&s.mults)
            .flat_map(|(k, m)| std::iter::repeat_n(*k, *m))
            .collect();
        if knots.len() != s.poles.len() + p + 1 {
            return Err(format!(
                "{} knots for {} poles of degree {p}",
                knots.len(),
                s.poles.len()
            ));
        }
        let c = NurbsCurve::new(p, knots, s.poles.clone()).map_err(|e| e.message())?;
        let (lo, hi) = (t0.max(a), t1.min(b));
        return Ok(if hi > lo && (lo > a || hi < b) {
            clamp(c, lo, hi)
        } else {
            c
        });
    }
    // One period of knots (the last knot is the first one a period on).
    let per: Vec<f64> = s.knots[..s.knots.len() - 1]
        .iter()
        .zip(&s.mults)
        .flat_map(|(k, m)| std::iter::repeat_n(*k, *m))
        .collect();
    let n = per.len();
    if n != s.poles.len() || n <= p {
        return Err(format!(
            "a periodic B-spline with {} poles for {n} knots",
            s.poles.len()
        ));
    }
    let period = b - a;
    if period <= 0.0
        || !period.is_finite()
        || !t0.is_finite()
        || !t1.is_finite()
        || (t1 - t0).abs() / period > 1e4
    {
        return Err("a periodic B-spline's range doesn't fit its period".into());
    }
    let knot = |j: i64| -> f64 {
        per[j.rem_euclid(n as i64) as usize] + period * j.div_euclid(n as i64) as f64
    };
    // OCCT's flat knots start p + 1 − m₁ knots before the first one, with the first basis
    // function on the first pole.
    let m1 = (p + 1).saturating_sub(s.mults[0]) as i64;
    let (lo, hi) = if t1 > t0 { (t0, t1) } else { (a, b) };
    if ((lo - a) / period).abs() > 1e4 || ((hi - a) / period).abs() > 1e4 {
        return Err("a periodic B-spline's range lies too many periods away".into());
    }
    let plo = ((lo - a) / period).floor() as i64 - 1;
    let phi = ((hi - a) / period).ceil() as i64 + 1;
    let (i0, i1) = (plo * n as i64, phi * n as i64);
    let knots: Vec<f64> = (i0..=i1 + p as i64 + 1).map(|i| knot(i - m1)).collect();
    let cps: Vec<H> = (i0..=i1)
        .map(|i| s.poles[i.rem_euclid(n as i64) as usize])
        .collect();
    Ok(clamp(
        NurbsCurve::new(p, knots, cps).map_err(|e| e.message())?,
        lo,
        hi,
    ))
}

/// The curve on [a, b] (inside its domain) with clamped ends.
fn clamp(c: NurbsCurve, a: f64, b: f64) -> NurbsCurve {
    let eps = 1e-13 * (1.0 + a.abs().max(b.abs()));
    if !(b > a + eps) {
        // An empty range (a degenerate edge's): nothing to cut.
        return c;
    }
    clamp_on(c.clone(), a, b, eps).unwrap_or(c)
}

fn clamp_on(mut c: NurbsCurve, a: f64, b: f64, eps: f64) -> Option<NurbsCurve> {
    let p = c.degree;
    for u in [a, b] {
        let m = c.knots.iter().filter(|k| (**k - u).abs() <= eps).count();
        for _ in m..p {
            c = c.insert_knot(u);
        }
    }
    let ia = c.knots.iter().position(|k| (*k - a).abs() <= eps)?;
    let ma = c.knots[ia..]
        .iter()
        .take_while(|k| (**k - a).abs() <= eps)
        .count();
    let ib = c.knots.iter().position(|k| (*k - b).abs() <= eps)?;
    if ib < ia + ma || ia + ma < p + 1 {
        return None;
    }
    let mut knots = vec![a; p + 1];
    knots.extend_from_slice(&c.knots[ia + ma..ib]);
    knots.extend(vec![b; p + 1]);
    let first = ia + ma - 1 - p;
    let ncp = knots.len() - p - 1;
    Some(NurbsCurve {
        degree: p,
        knots,
        cps: c.cps.get(first..first + ncp)?.to_vec(),
    })
}

pub(crate) fn spline_surface(
    du: usize,
    dv: usize,
    poles: &[Vec<H>],
    u: &(Vec<f64>, Vec<usize>, bool),
    v: &(Vec<f64>, Vec<usize>, bool),
) -> R<NurbsSurface> {
    if poles.is_empty()
        || poles.iter().any(|r| r.len() != poles[0].len())
        || u.0.len() < 2
        || v.0.len() < 2
    {
        return Err("a bad B-spline surface".into());
    }
    let full = |k: &(Vec<f64>, Vec<usize>, bool)| (k.0[0], k.0[k.0.len() - 1]);
    // Along u, column by column.
    let nv = poles[0].len();
    let mut cols: Vec<NurbsCurve> = Vec::with_capacity(nv);
    for j in 0..nv {
        let s = Spline {
            degree: du,
            poles: poles.iter().map(|r| r[j]).collect(),
            knots: u.0.clone(),
            mults: u.1.clone(),
            periodic: u.2,
        };
        let (a, b) = full(u);
        cols.push(spline_curve(&s, a, b)?);
    }
    let nu = cols[0].cps.len();
    let ku = cols[0].knots.clone();
    // Then along v, row by row.
    let mut rows: Vec<NurbsCurve> = Vec::with_capacity(nu);
    for i in 0..nu {
        let s = Spline {
            degree: dv,
            poles: cols.iter().map(|c| c.cps[i]).collect(),
            knots: v.0.clone(),
            mults: v.1.clone(),
            periodic: v.2,
        };
        let (a, b) = full(v);
        rows.push(spline_curve(&s, a, b)?);
    }
    let kv = rows[0].knots.clone();
    Ok(NurbsSurface {
        screw: None,
        du,
        dv,
        ku,
        kv,
        cps: rows.into_iter().map(|r| r.cps).collect(),
    })
}

/// A curve through points of `f` on [t0, t1] (for the curves we have no exact form of).
fn sampled(f: impl Fn(f64) -> P3, t0: f64, t1: f64) -> Curve {
    let n = 64;
    let ts: Vec<f64> = (0..=n)
        .map(|i| t0 + (t1 - t0) * i as f64 / n as f64)
        .collect();
    let pts: Vec<P3> = ts.iter().map(|&t| f(t)).collect();
    Curve::Nurbs(Arc::new(NurbsCurve::interpolate(&pts, &ts, 3, None, None)))
}

// ---------------------------------------------------------------------------------------------
// Geometry to ours

/// The curve on [t0, t1] (the edge's range: unbounded curves and spline pieces need one).
fn to_curve(g: &GCurve, t0: f64, t1: f64) -> R<Curve> {
    Ok(match g {
        GCurve::Line(o, d) => Curve::Line {
            o: *o,
            d: d.normalize(),
        },
        GCurve::Circle(ax, r) => Curve::Circle {
            c: ax.o,
            x: ax.x.normalize(),
            y: ax.y.normalize(),
            r: *r,
        },
        GCurve::Ellipse(ax, a, b) => Curve::Ellipse {
            c: ax.o,
            x: ax.x.normalize(),
            y: ax.y.normalize(),
            a: *a,
            b: *b,
        },
        GCurve::Parabola(ax, f) => {
            // o + t²/(4f)·x + t·y: a quadratic, exactly a Bézier curve on [t0, t1].
            let pt = |t: f64| ax.o + ax.x * (t * t / (4.0 * f)) + ax.y * t;
            let d0 = ax.x * (t0 / (2.0 * f)) + ax.y;
            let mid = pt(t0) + d0 * ((t1 - t0) / 2.0);
            let mut c = NurbsCurve::bezier(&[pt(t0), mid, pt(t1)], &[1.0; 3]);
            c = c.reparam(t0, t1);
            Curve::Nurbs(Arc::new(c))
        }
        GCurve::Hyperbola(ax, a, b) => {
            let (ax, a, b) = (*ax, *a, *b);
            sampled(
                move |t| ax.o + ax.x * (a * t.cosh()) + ax.y * (b * t.sinh()),
                t0,
                t1,
            )
        }
        GCurve::Spline(s) => Curve::Nurbs(Arc::new(spline_curve(s, t0, t1)?)),
        GCurve::Trimmed(c) => to_curve(c, t0, t1)?,
        GCurve::Offset(d, v, basis) => {
            let c = to_curve(basis, t0, t1)?;
            let v = v.normalize();
            match &c {
                // The offset direction is T × V.
                Curve::Line { o, d: dir } if dir.cross(&v).norm() > tol::ANGULAR_FINE => {
                    Curve::Line {
                        o: o + dir.cross(&v).normalize() * *d,
                        d: *dir,
                    }
                }
                Curve::Circle { c: o, x, y, r }
                    if x.cross(y).normalize().cross(&v).norm() < tol::ANGULAR_FINE =>
                {
                    let r2 = r + d * x.cross(y).normalize().dot(&v);
                    if r2 <= 0.0 {
                        return Err("an offset circle of no radius".into());
                    }
                    Curve::Circle {
                        c: *o,
                        x: *x,
                        y: *y,
                        r: r2,
                    }
                }
                _ => {
                    let d = *d;
                    sampled(
                        move |t| c.point(t) + c.tangent(t).cross(&v).normalize() * d,
                        t0,
                        t1,
                    )
                }
            }
        }
    })
}

/// The basis curve's own range (a spline's knots; lines and conics have none).
fn curve_range(g: &GCurve) -> Option<(f64, f64)> {
    match g {
        GCurve::Spline(s) => Some((s.knots[0], *s.knots.last()?)),
        GCurve::Trimmed(c) | GCurve::Offset(_, _, c) => curve_range(c),
        GCurve::Circle(..) | GCurve::Ellipse(..) => Some((0.0, TAU)),
        _ => None,
    }
}

/// A line (through trims), as our curve; None for other curves.
fn base_line(g: &GCurve) -> Option<Curve> {
    match g {
        GCurve::Line(o, d) => Some(Curve::Line {
            o: *o,
            d: d.normalize(),
        }),
        GCurve::Trimmed(c) => base_line(c),
        _ => None,
    }
}

/// Our surface and whether its natural normal is OCCT's.
fn to_surface(g: &GSurf, exact: bool) -> R<(Surface, bool)> {
    let fr = |ax: &Ax| if exact { ax.frame_exact() } else { ax.frame() };
    let hand = |ax: &Ax| exact || ax.hand() > 0.0;
    Ok(match g {
        GSurf::Plane(ax) => (Surface::Plane { f: fr(ax) }, hand(ax)),
        GSurf::Cylinder(ax, r) => (Surface::Cylinder { f: fr(ax), r: *r }, hand(ax)),
        GSurf::Cone(ax, r, a) => (
            Surface::Cone {
                f: fr(ax),
                r: *r,
                a: *a,
            },
            hand(ax),
        ),
        GSurf::Sphere(ax, r) => (Surface::Sphere { f: fr(ax), r: *r }, hand(ax)),
        GSurf::Torus(ax, rmaj, rmin) => (
            Surface::Torus {
                f: fr(ax),
                rmaj: *rmaj,
                rmin: *rmin,
            },
            hand(ax),
        ),
        GSurf::Extrusion(d, c) => {
            // A line (unbounded, its parameter the length along it) needs no range: the
            // extrusion of one is a plane, parametrized as OCCT's.
            let c = match (curve_range(c), base_line(c)) {
                (Some((a, b)), _) => to_curve(c, a, b)?,
                (None, Some(line)) => line,
                (None, None) => return Err("an extrusion of an unbounded curve".into()),
            };
            // OCCT's normal is C' × d, ours Su × Sv: the same.
            (
                Surface::Extrusion {
                    c,
                    d: d.normalize(),
                },
                true,
            )
        }
        GSurf::Revolution(o, z, c) => {
            let (a, b) = curve_range(c).unwrap_or((-1.0, 1.0));
            let m = to_curve(c, a, b)?;
            let z = z.normalize();
            let mid = m.point((a + b) / 2.0);
            let w = mid - o;
            let xh = w - z * w.dot(&z);
            // OCCT's revolution runs u about the axis and v along the meridian, as ours.
            (
                Surface::Revolution {
                    f: Frame::from_xz(*o, xh, z),
                    m,
                },
                true,
            )
        }
        GSurf::Spline {
            du,
            dv,
            poles,
            u,
            v,
        } => (
            Surface::Nurbs(Arc::new(spline_surface(*du, *dv, poles, u, v)?)),
            true,
        ),
        GSurf::Trimmed(s) => to_surface(s, exact)?,
        GSurf::Offset(d, s) => offset_surface(*d, s, exact)?,
    })
}

fn offset_surface(d: f64, g: &GSurf, exact: bool) -> R<(Surface, bool)> {
    let (base, same) = to_surface(g, exact)?;
    // Along OCCT's normal: ours, or ours turned round.
    let d = if same { d } else { -d };
    let pos = |r: f64| {
        if r > tol::ROUNDOFF_FINE {
            Ok(r)
        } else {
            Err("an offset surface of no radius".to_string())
        }
    };
    Ok((
        match &base {
            Surface::Plane { f } => Surface::Plane {
                f: Frame {
                    o: f.o + f.z * d,
                    ..*f
                },
            },
            Surface::Cylinder { f, r } => Surface::Cylinder {
                f: *f,
                r: pos(r + d)?,
            },
            Surface::Sphere { f, r } => Surface::Sphere {
                f: *f,
                r: pos(r + d)?,
            },
            Surface::Torus { f, rmaj, rmin } => Surface::Torus {
                f: *f,
                rmaj: *rmaj,
                rmin: pos(rmin + d)?,
            },
            Surface::Cone { f, r, a } => Surface::Cone {
                f: *f,
                r: r + d / a.cos(),
                a: *a,
            },
            s => {
                // Interpolated through offset points on a grid.
                let ((u0, u1), (v0, v1)) = s.param_box();
                let n = 32;
                let us: Vec<f64> = (0..=n)
                    .map(|i| u0 + (u1 - u0) * i as f64 / n as f64)
                    .collect();
                let vs: Vec<f64> = (0..=n)
                    .map(|i| v0 + (v1 - v0) * i as f64 / n as f64)
                    .collect();
                let rows: Vec<NurbsCurve> = vs
                    .iter()
                    .map(|&v| {
                        let pts: Vec<P3> = us
                            .iter()
                            .map(|&u| s.eval(u, v) + s.normal(u, v) * d)
                            .collect();
                        NurbsCurve::interpolate(&pts, &us, 3, None, None)
                    })
                    .collect();
                Surface::Nurbs(Arc::new(NurbsSurface::through_curves(
                    &rows, &vs, 3, None, None,
                )))
            }
        },
        same,
    ))
}

// ---------------------------------------------------------------------------------------------
// Locations

/// A location: a product of elementary ones raised to powers (as TopLoc compares them) and its
/// map.
#[derive(Debug, Clone)]
struct Loc {
    chain: Vec<(usize, i64)>,
    m: Affine,
}

fn compose(a: &Affine, b: &Affine) -> Affine {
    Affine {
        m: a.m * b.m,
        t: a.m * b.t + a.t,
    }
}

fn inverse(a: &Affine) -> Affine {
    let mi =
        a.m.try_inverse()
            .unwrap_or_else(nalgebra::Matrix3::identity);
    Affine {
        m: mi,
        t: -(mi * a.t),
    }
}

fn normalize(chain: Vec<(usize, i64)>) -> Vec<(usize, i64)> {
    let mut out: Vec<(usize, i64)> = Vec::with_capacity(chain.len());
    for (e, p) in chain {
        match out.last_mut() {
            Some(l) if l.0 == e => {
                l.1 += p;
                if l.1 == 0 {
                    out.pop();
                }
            }
            _ if p != 0 => out.push((e, p)),
            _ => {}
        }
    }
    out
}

impl Loc {
    fn identity() -> Self {
        Self {
            chain: Vec::new(),
            m: Affine::identity(),
        }
    }
    fn then(&self, o: &Loc) -> Loc {
        let mut chain = self.chain.clone();
        chain.extend_from_slice(&o.chain);
        Loc {
            chain: normalize(chain),
            m: compose(&self.m, &o.m),
        }
    }
    fn inverse(&self) -> Loc {
        Loc {
            chain: self.chain.iter().rev().map(|&(e, p)| (e, -p)).collect(),
            m: inverse(&self.m),
        }
    }
    fn powered(&self, p: i64) -> Loc {
        let base = if p < 0 { self.inverse() } else { self.clone() };
        let mut out = Loc::identity();
        for _ in 0..p.unsigned_abs() {
            out = out.then(&base);
        }
        out
    }
}

// ---------------------------------------------------------------------------------------------
// Shapes

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Ori {
    Fwd,
    Rev,
    Int,
    Ext,
}

impl Ori {
    fn compose(self, child: Ori) -> Ori {
        match (self, child) {
            (Ori::Fwd, c) => c,
            (Ori::Rev, Ori::Fwd) => Ori::Rev,
            (Ori::Rev, Ori::Rev) => Ori::Fwd,
            (Ori::Rev, c) => c,
            (o, _) => o,
        }
    }
}

#[derive(Debug, Clone)]
enum Kind {
    Vertex(P3),
    /// The 3D curve (index, location, range), when there is one, whether it is degenerate, and
    /// its p-curves.
    Edge(Option<(usize, usize, f64, f64)>, bool, Vec<PcRep>),
    Wire,
    /// The surface (index, location).
    Face(Option<(usize, usize)>),
    Shell,
    Solid,
    Compound,
}

/// An edge's p-curve on a surface (two on a closed surface: the edge's forward use first).
#[derive(Debug, Clone)]
struct PcRep {
    pc: usize,
    pc2: Option<usize>,
    surf: usize,
    loc: usize,
    range: (f64, f64),
}

#[derive(Debug, Clone)]
struct TShape {
    kind: Kind,
    /// A vertex's or edge's tolerance from the file (0 for other shapes).
    tol: f64,
    closed: bool,
    subs: Vec<(Ori, usize, usize)>,
}

/// A parsed file.
struct File {
    locs: Vec<Loc>,
    curves2d: Vec<GCurve>,
    curves: Vec<GCurve>,
    surfaces: Vec<GSurf>,
    shapes: Vec<TShape>,
    root: (Ori, usize, usize),
}

fn shape_ref(t: &mut Tok, n: usize) -> R<Option<(Ori, usize, usize)>> {
    let w = t.next()?;
    if w == "*" {
        return Ok(None);
    }
    let ori = match w.as_bytes()[0] {
        b'+' => Ori::Fwd,
        b'-' => Ori::Rev,
        b'i' => Ori::Int,
        b'e' => Ori::Ext,
        _ => return Err(format!("bad shape reference {w}")),
    };
    let k: usize = w[1..]
        .parse()
        .map_err(|_| format!("bad shape reference {w}"))?;
    if k == 0 || k > n {
        return Err(format!("shape reference {w} out of range"));
    }
    let l = t.u()?;
    Ok(Some((ori, n - k, l)))
}

fn parse(text: &str) -> R<File> {
    // The version line (after DRAW's header, if any).
    let start = text
        .find("CASCADE Topology V")
        .ok_or("not an OCCT BRep file")?;
    let rest = &text[start..];
    let version: u32 = rest["CASCADE Topology V".len()..]
        .chars()
        .next()
        .and_then(|c| c.to_digit(10))
        .ok_or("no format version")?;
    let body = rest.find('\n').map_or("", |i| &rest[i..]);
    let mut t = Tok {
        it: body.split_ascii_whitespace().peekable(),
    };

    // Locations (index 0: none).
    t.expect("Locations")?;
    let nl = t.u()?;
    let mut locs = vec![Loc::identity()];
    for _ in 0..nl {
        match t.i()? {
            1 => {
                let mut v = [0.0; 12];
                for x in v.iter_mut() {
                    *x = t.f()?;
                }
                let m =
                    nalgebra::Matrix3::new(v[0], v[1], v[2], v[4], v[5], v[6], v[8], v[9], v[10]);
                let id = locs.len();
                locs.push(Loc {
                    chain: vec![(id, 1)],
                    m: Affine {
                        m,
                        t: V3::new(v[3], v[7], v[11]),
                    },
                });
            }
            2 => {
                let mut l = Loc::identity();
                loop {
                    let l1 = t.u()?;
                    if l1 == 0 {
                        break;
                    }
                    let p = t.i()?;
                    let base = locs.get(l1).ok_or("location out of range")?.clone();
                    l = base.powered(p).then(&l);
                }
                locs.push(l);
            }
            k => return Err(format!("unknown location type {k}")),
        }
    }

    let n2 = t.table("Curve2ds")?;
    let curves2d = (0..n2)
        .map(|_| read_curve2d(&mut t))
        .collect::<R<Vec<_>>>()?;
    let nc = t.table("Curves")?;
    let curves = (0..nc).map(|_| read_curve(&mut t)).collect::<R<Vec<_>>>()?;
    let np = t.table("Polygon3D")?;
    for _ in 0..np {
        let nn = t.u()?;
        let has = t.i()? != 0;
        skip(
            &mut t,
            nn.saturating_mul(if has { 4 } else { 3 }).saturating_add(1),
        )?;
    }
    let npt = t.table("PolygonOnTriangulations")?;
    for _ in 0..npt {
        let nn = t.u()?;
        skip(&mut t, nn)?;
        // "p", the deflection, whether there are parameters.
        t.next()?;
        t.next()?;
        let has = t.i()? != 0;
        if has {
            skip(&mut t, nn)?;
        }
    }
    let ns = t.table("Surfaces")?;
    let surfaces = (0..ns)
        .map(|_| read_surface(&mut t))
        .collect::<R<Vec<_>>>()?;
    let ntr = t.table("Triangulations")?;
    for _ in 0..ntr {
        let (nn, ntri) = (t.u()?, t.u()?);
        let uv = t.i()? != 0;
        let normals = if version >= 3 { t.i()? != 0 } else { false };
        let per_node = 3 + if uv { 2 } else { 0 } + if normals { 3 } else { 0 };
        skip(
            &mut t,
            nn.saturating_mul(per_node)
                .saturating_add(ntri.saturating_mul(3))
                .saturating_add(1),
        )?;
    }

    t.expect("TShapes")?;
    let n = t.u()?;
    // (Capacity from the count only as far as the file could hold that many.)
    let mut shapes: Vec<TShape> = Vec::with_capacity(n.min(text.len()));
    for si in 0..n {
        let ty = t.next()?;
        let mut tol = 0.0;
        let kind = (|| -> R<Kind> {
            Ok(match ty {
                "Ve" => {
                    tol = t.f()?;
                    let p = t.p3()?;
                    // Point representations, to a "0 0".
                    loop {
                        let _p1 = t.f()?;
                        match t.i()? {
                            0 => break,
                            1 => skip(&mut t, 1)?,
                            2 => skip(&mut t, 2)?,
                            3 => skip(&mut t, 2)?,
                            k => return Err(format!("unknown point representation {k}")),
                        }
                        let _l = t.u()?;
                    }
                    Kind::Vertex(p)
                }
                "Ed" => {
                    tol = t.f()?;
                    let (_sp, _sr) = (t.i()?, t.i()?);
                    let degenerate = t.i()? == 1;
                    let mut c3 = None;
                    let mut pcs: Vec<PcRep> = Vec::new();
                    loop {
                        match t.i()? {
                            0 => break,
                            1 => {
                                let (c, l) = (t.u()?, t.u()?);
                                let (a, b) = (t.f()?, t.f()?);
                                if c >= 1 && c <= curves.len() && l < locs.len() {
                                    c3 = Some((c - 1, l, a, b));
                                }
                            }
                            k @ (2 | 3) => {
                                let pc = t.u()?;
                                let pc2 = if k == 3 {
                                    // pc2 and the regularity, written "24C0" or "24 C0".
                                    let w = t.next()?;
                                    let digits: String =
                                        w.chars().take_while(|c| c.is_ascii_digit()).collect();
                                    if digits.len() == w.len() {
                                        skip(&mut t, 1)?;
                                    }
                                    Some(
                                        digits
                                            .parse::<usize>()
                                            .map_err(|_| format!("bad p-curve index {w}"))?,
                                    )
                                } else {
                                    None
                                };
                                let (sf, l) = (t.u()?, t.u()?);
                                let (a, b) = (t.f()?, t.f()?);
                                if version == 2 {
                                    skip(&mut t, 4)?;
                                }
                                let ok = |i: usize| i >= 1 && i <= curves2d.len();
                                if ok(pc)
                                    && pc2.is_none_or(ok)
                                    && sf >= 1
                                    && sf <= surfaces.len()
                                    && l < locs.len()
                                {
                                    pcs.push(PcRep {
                                        pc: pc - 1,
                                        pc2: pc2.map(|x| x - 1),
                                        surf: sf - 1,
                                        loc: l,
                                        range: (a, b),
                                    });
                                }
                            }
                            4 => skip(&mut t, 5)?,
                            5 => skip(&mut t, 2)?,
                            6 => skip(&mut t, 3)?,
                            7 => skip(&mut t, 4)?,
                            k => return Err(format!("unknown edge representation {k}")),
                        }
                    }
                    Kind::Edge(c3, degenerate, pcs)
                }
                "Wi" => Kind::Wire,
                "Fa" => {
                    let val = t.i()?;
                    let mut surf = None;
                    if val == 0 || val == 1 {
                        let _tol = t.f()?;
                        let (s, l) = (t.u()?, t.u()?);
                        if s >= 1 && s <= surfaces.len() && l < locs.len() {
                            surf = Some((s - 1, l));
                        }
                        if t.peek() == Some("2") {
                            skip(&mut t, 2)?;
                        }
                    } else {
                        skip(&mut t, 1)?;
                    }
                    Kind::Face(surf)
                }
                "Sh" => Kind::Shell,
                "So" => Kind::Solid,
                "CS" | "Co" => Kind::Compound,
                k => return Err(format!("unknown shape type {k}")),
            })
        })()
        .map_err(|e| format!("shape {} ({ty}): {e}", n - si))?;
        let flags = t.next()?.as_bytes().to_vec();
        let closed = flags.get(4) == Some(&b'1');
        let mut subs = Vec::new();
        // References count back from the end of the whole table; only earlier shapes exist.
        while let Some(r) = shape_ref(&mut t, n)? {
            if r.1 >= shapes.len() || r.2 >= locs.len() {
                return Err(format!(
                    "shape {} ({ty}): a sub-shape reference out of range",
                    n - si
                ));
            }
            subs.push(r);
        }
        shapes.push(TShape {
            kind,
            tol,
            closed,
            subs,
        });
    }
    let root = match shape_ref(&mut t, n) {
        Ok(Some(r)) if r.1 < shapes.len() && r.2 < locs.len() => r,
        _ => (Ori::Fwd, shapes.len().checked_sub(1).ok_or("no shapes")?, 0),
    };
    Ok(File {
        locs,
        curves2d,
        curves,
        surfaces,
        shapes,
        root,
    })
}

// ---------------------------------------------------------------------------------------------
// Unbounded planes

/// Half the side of the square that stands for a plane face the file leaves unbounded (no
/// wire: OCCT's infinite plane), and the depth of the box that stands for a half-space (a solid
/// of one such face). Large against any model the kernel builds, small enough that coordinates
/// on it keep about 1e-10 of absolute precision.
pub const UNBOUNDED: f64 = 1e6;

/// Gives every plane face of the file without a wire a square of side 2·[`UNBOUNDED`] about
/// its origin; a shell of one such face only (a half-space, `halfspace` in DRAW) becomes the
/// box of that depth behind the face as the shape uses it (its orientation composed from the
/// root down). OCCT reads both as infinite; the kernel's bodies are bounded, and against any
/// finite body the result is the same.
fn bound_unbounded_planes(f: &mut File) {
    let has_wire = |f: &File, s: usize| {
        f.shapes[s]
            .subs
            .iter()
            .any(|&(_, w, _)| matches!(f.shapes[w].kind, Kind::Wire))
    };
    let unbounded = |f: &File, s: usize| {
        matches!(f.shapes[s].kind, Kind::Face(Some((si, _))) if matches!(f.surfaces[si], GSurf::Plane(_)))
            && !has_wire(f, s)
    };
    let faces: Vec<usize> = (0..f.shapes.len()).filter(|&s| unbounded(f, s)).collect();
    if faces.is_empty() {
        return;
    }
    // Shells of one unbounded face, and how they use it.
    let halfspaces: Vec<(usize, Ori, usize)> = (0..f.shapes.len())
        .filter(|&s| matches!(f.shapes[s].kind, Kind::Shell))
        .filter_map(|s| match f.shapes[s].subs[..] {
            [(o, fs, l)] if unbounded(f, fs) => Some((s, o, l)),
            _ => None,
        })
        .collect();
    // The orientation each shell is used with, composed from the root.
    let mut used: HashMap<usize, Ori> = HashMap::new();
    let mut stack = vec![(f.root.1, f.root.0)];
    let mut seen = std::collections::HashSet::new();
    while let Some((s, o)) = stack.pop() {
        if !seen.insert((s, o)) {
            continue;
        }
        if matches!(f.shapes[s].kind, Kind::Shell) {
            used.entry(s).or_insert(o);
        } else if matches!(f.shapes[s].kind, Kind::Compound | Kind::Solid) {
            stack.extend(
                f.shapes[s]
                    .subs
                    .iter()
                    .map(|&(c, sub, _)| (sub, o.compose(c))),
            );
        }
    }
    for fs in faces {
        let Kind::Face(Some((si, sl))) = f.shapes[fs].kind else {
            continue;
        };
        let GSurf::Plane(ax) = f.surfaces[si] else {
            continue;
        };
        let (x, y) = (ax.x.normalize(), ax.y.normalize());
        // (u, v) on the plane as the file's axes give them: a loop counter-clockwise in (u, v)
        // runs round the face's natural normal x × y.
        let m = x.cross(&y);
        let at = |u: f64, v: f64, w: f64| ax.o + x * u + y * v + m * w;
        let l = UNBOUNDED;
        let square = [
            at(-l, -l, 0.0),
            at(l, -l, 0.0),
            at(l, l, 0.0),
            at(-l, l, 0.0),
        ];
        let users: Vec<(usize, Ori, usize)> = halfspaces
            .iter()
            .copied()
            .filter(|h| f.shapes[h.0].subs[0].1 == fs)
            .collect();
        let mut b = PlaneBuilder {
            f: &mut *f,
            verts: Vec::new(),
            edges: Vec::new(),
        };
        let wire = b.wire(&square);
        b.f.shapes[fs].subs.push((Ori::Fwd, wire, sl));
        for (sh, o, fl) in users {
            // The material lies behind the face as the shape uses it: away from m when it
            // takes the face forward.
            let fwd = used.get(&sh).copied().unwrap_or(Ori::Fwd).compose(o) != Ori::Rev;
            let d = if fwd { -2.0 * l } else { 2.0 * l };
            let far = [at(-l, -l, d), at(l, -l, d), at(l, l, d), at(-l, l, d)];
            let mut quads: Vec<[P3; 4]> = (0..4)
                .map(|k| [square[k], far[k], far[(k + 1) % 4], square[(k + 1) % 4]])
                .collect();
            quads.push([far[0], far[3], far[2], far[1]]);
            for q in quads {
                // Each quad runs round its face's natural normal n, and the shell takes the new
                // faces as it takes the plane's: forward, n is the outward normal (the quads
                // run round the box's outside); reversed, the box lies on the other side of the
                // plane and the same order runs round the inward normal.
                let n = (q[1] - q[0]).cross(&(q[2] - q[1])).normalize();
                let qx = (q[1] - q[0]).normalize();
                b.f.surfaces.push(GSurf::Plane(Ax {
                    o: q[0],
                    z: n,
                    x: qx,
                    y: n.cross(&qx),
                }));
                let w = b.wire(&q);
                b.f.shapes.push(TShape {
                    kind: Kind::Face(Some((b.f.surfaces.len() - 1, sl))),
                    tol: 0.0,
                    closed: false,
                    subs: vec![(Ori::Fwd, w, sl)],
                });
                let nf = b.f.shapes.len() - 1;
                b.f.shapes[sh].subs.push((o, nf, fl));
            }
            b.f.shapes[sh].closed = true;
        }
    }
}

/// Adds vertices, line edges and wires to a file (for [`bound_unbounded_planes`]), sharing a
/// vertex or edge between the loops that meet there.
struct PlaneBuilder<'a> {
    f: &'a mut File,
    verts: Vec<(P3, usize)>,
    /// (start vertex, end vertex, edge shape).
    edges: Vec<(usize, usize, usize)>,
}

impl PlaneBuilder<'_> {
    fn vertex(&mut self, p: P3) -> usize {
        if let Some(&(_, v)) = self
            .verts
            .iter()
            .find(|(q, _)| (q - p).norm() <= super::tol::CONFUSION)
        {
            return v;
        }
        self.f.shapes.push(TShape {
            kind: Kind::Vertex(p),
            tol: super::tol::CONFUSION,
            closed: false,
            subs: Vec::new(),
        });
        let v = self.f.shapes.len() - 1;
        self.verts.push((p, v));
        v
    }

    /// A wire through `pts` (closed), as the file's shapes.
    fn wire(&mut self, pts: &[P3]) -> usize {
        let mut subs = Vec::new();
        for k in 0..pts.len() {
            let (p, q) = (pts[k], pts[(k + 1) % pts.len()]);
            let (a, b) = (self.vertex(p), self.vertex(q));
            let found = self.edges.iter().find_map(|&(s, e, ed)| {
                if (s, e) == (a, b) {
                    Some((ed, Ori::Fwd))
                } else if (s, e) == (b, a) {
                    Some((ed, Ori::Rev))
                } else {
                    None
                }
            });
            let (ed, o) = match found {
                Some(x) => x,
                None => {
                    let len = (q - p).norm();
                    self.f.curves.push(GCurve::Line(p, (q - p) / len));
                    let c = self.f.curves.len() - 1;
                    self.f.shapes.push(TShape {
                        kind: Kind::Edge(Some((c, 0, 0.0, len)), false, Vec::new()),
                        tol: super::tol::CONFUSION,
                        closed: false,
                        subs: vec![(Ori::Fwd, a, 0), (Ori::Rev, b, 0)],
                    });
                    let ed = self.f.shapes.len() - 1;
                    self.edges.push((a, b, ed));
                    (ed, Ori::Fwd)
                }
            };
            subs.push((o, ed, 0));
        }
        self.f.shapes.push(TShape {
            kind: Kind::Wire,
            tol: 0.0,
            closed: true,
            subs,
        });
        self.f.shapes.len() - 1
    }
}

// ---------------------------------------------------------------------------------------------
// Building

/// The 3D curve of an edge the file gives only p-curves (OCCT's `BRepLib::BuildCurves3d`): its
/// first p-curve carried onto its surface (placed as the edge at `loc` places it), on the
/// p-curve's range, which becomes the edge's.
fn lifted(f: &File, pcs: &[PcRep], loc: &Loc) -> R<(Curve, f64, f64)> {
    let r = pcs.first().ok_or("no 3D curve and no p-curve")?;
    let (a, b) = r.range;
    if !(a.is_finite() && b.is_finite() && a < b) {
        return Err(format!("a p-curve range [{a}, {b}]"));
    }
    let (surf, _) = to_surface(&f.surfaces[r.surf], true)?;
    let surf = surf.transformed(&loc.then(&f.locs[r.loc]).m);
    let pc = to_curve(&f.curves2d[r.pc], a, b)?;
    let c = sampled(
        |t| {
            let q = pc.point(t);
            surf.eval(q.x, q.y)
        },
        a,
        b,
    );
    Ok((c, a, b))
}

/// A location's map rounded (to 1e-9), so two uses of a shape placed alike are one shape
/// whichever products of locations placed them.
fn loc_key(l: &Loc) -> [i64; 12] {
    let m = &l.m;
    let v = [
        m.m[(0, 0)],
        m.m[(0, 1)],
        m.m[(0, 2)],
        m.m[(1, 0)],
        m.m[(1, 1)],
        m.m[(1, 2)],
        m.m[(2, 0)],
        m.m[(2, 1)],
        m.m[(2, 2)],
        m.t.x,
        m.t.y,
        m.t.z,
    ];
    v.map(|x| (x * 1e9).round() as i64)
}

struct Builder<'a> {
    f: &'a File,
    xb: XBuilder,
    verts: HashMap<(usize, [i64; 12]), usize>,
    edges: HashMap<(usize, [i64; 12]), Option<usize>>,
    free_faces: Vec<usize>,
    /// Edges outside any face (wires and edges of the shape), in order.
    free_edges: Vec<usize>,
    /// Whether any free edge is in a wire.
    wires: bool,
    /// Vertices outside any edge (a compound's), placed.
    free_points: Vec<P3>,
    warnings: Vec<String>,
    /// The file's degenerate edges (left out): how many edges were made before each, and its
    /// point, for OCCT's exploration order.
    degenerate: Vec<(usize, P3)>,
}

impl Builder<'_> {
    fn vertex(&mut self, s: usize, loc: &Loc) -> Option<usize> {
        let key = (s, loc_key(loc));
        if let Some(&v) = self.verts.get(&key) {
            return Some(v);
        }
        let Kind::Vertex(p) = self.f.shapes[s].kind else {
            return None;
        };
        let v = self.xb.vertex(loc.m.point(&p));
        self.xb.body.verts[v].tol = self.f.shapes[s].tol.max(TOL_MIN);
        self.verts.insert(key, v);
        Some(v)
    }

    fn edge(&mut self, s: usize, loc: &Loc) -> Option<usize> {
        let key = (s, loc_key(loc));
        if let Some(e) = self.edges.get(&key) {
            return *e;
        }
        let e = self.make_edge(s, loc);
        self.edges.insert(key, e);
        e
    }

    fn make_edge(&mut self, s: usize, loc: &Loc) -> Option<usize> {
        let sh = &self.f.shapes[s];
        let Kind::Edge(c3, degenerate, ref pcs) = sh.kind else {
            return None;
        };
        if degenerate {
            let at = self.xb.body.edges.len();
            if let Some(&(_, vs, l)) = sh.subs.first()
                && let Some(v) = self.vertex(vs, &loc.then(&self.f.locs[l]))
            {
                self.degenerate.push((at, self.xb.body.verts[v].p));
            }
            return None;
        }
        let (mut v0, mut v1) = (None, None);
        for &(o, vs, l) in &sh.subs.clone() {
            let vl = loc.then(&self.f.locs[l]);
            match o {
                Ori::Fwd => v0 = self.vertex(vs, &vl),
                Ori::Rev => v1 = self.vertex(vs, &vl),
                _ => {}
            }
        }
        let (Some(v0), Some(v1)) = (v0, v1) else {
            self.warnings.push(format!("edge {s}: no vertices"));
            return None;
        };
        let curve = match c3 {
            Some((ci, cl, a, b)) => to_curve(&self.f.curves[ci], a, b)
                .map(|c| c.transformed(&compose(&loc.m, &self.f.locs[cl].m))),
            None => lifted(self.f, pcs, loc).map(|(c, _, _)| c),
        };
        let curve = match curve {
            Ok(c) => c,
            Err(e) => {
                self.warnings.push(format!("edge {s}: {e}"));
                return None;
            }
        };
        let e = self.xb.edge(curve, v0, v1);
        if let Some(e) = e {
            self.xb.body.edges[e].tol = self.f.shapes[s].tol.max(TOL_MIN);
        }
        if e.is_none() {
            self.warnings.push(format!("edge {s}: empty"));
        }
        e
    }

    fn face(&mut self, s: usize, ori: Ori, loc: &Loc) -> Option<usize> {
        let sh = self.f.shapes[s].clone();
        let Kind::Face(Some((si, sl))) = sh.kind else {
            self.warnings.push(format!("face {s}: no surface"));
            return None;
        };
        let (surf, same) = match to_surface(&self.f.surfaces[si], false) {
            Ok(x) => x,
            Err(e) => {
                self.warnings.push(format!("face {s}: {e}"));
                return None;
            }
        };
        let at = compose(&loc.m, &self.f.locs[sl].m);
        let surf = surf.transformed(&at);
        let mut loops = Vec::new();
        for &(wo, ws, wl) in &sh.subs {
            let wloc = loc.then(&self.f.locs[wl]);
            let wori = ori.compose(wo);
            let mut lp = Vec::new();
            for &(eo, es, el) in &self.f.shapes[ws].subs.clone() {
                let o = wori.compose(eo);
                if !matches!(o, Ori::Fwd | Ori::Rev) {
                    continue;
                }
                if let Some(e) = self.edge(es, &wloc.then(&self.f.locs[el])) {
                    lp.push((e, o == Ori::Fwd));
                }
            }
            if !lp.is_empty() {
                loops.push(lp);
            }
        }
        // The outward normal is OCCT's natural normal for a forward face.
        let sense = (ori == Ori::Fwd) == same;
        Some(self.xb.face(XFace { surf, sense, loops }))
    }

    fn shell(&mut self, s: usize, ori: Ori, loc: &Loc) -> Vec<usize> {
        let mut faces = Vec::new();
        for &(o, fs, l) in &self.f.shapes[s].subs.clone() {
            let o = ori.compose(o);
            if let Some(fi) = self.face(fs, o, &loc.then(&self.f.locs[l])) {
                faces.push(fi);
            }
        }
        faces
    }

    fn visit(&mut self, s: usize, ori: Ori, loc: &Loc) {
        let sh = self.f.shapes[s].clone();
        match sh.kind {
            Kind::Compound => {
                for &(o, c, l) in &sh.subs {
                    self.visit(c, ori.compose(o), &loc.then(&self.f.locs[l]));
                }
            }
            Kind::Solid => {
                let mut shells = Vec::new();
                for &(o, c, l) in &sh.subs {
                    if !matches!(self.f.shapes[c].kind, Kind::Shell) {
                        continue;
                    }
                    let faces = self.shell(c, ori.compose(o), &loc.then(&self.f.locs[l]));
                    if !faces.is_empty() {
                        shells.push(self.xb.shell(faces, true));
                    }
                }
                if !shells.is_empty() {
                    self.xb.solid(shells);
                }
            }
            Kind::Shell => {
                let faces = self.shell(s, ori, loc);
                if !faces.is_empty() {
                    self.xb.shell(faces, sh.closed);
                }
            }
            Kind::Face(_) => {
                if let Some(fi) = self.face(s, ori, loc) {
                    self.free_faces.push(fi);
                }
            }
            Kind::Wire => {
                self.wires = true;
                for &(o, c, l) in &sh.subs {
                    self.visit(c, ori.compose(o), &loc.then(&self.f.locs[l]));
                }
            }
            Kind::Edge(..) => {
                if matches!(ori, Ori::Fwd | Ori::Rev)
                    && let Some(e) = self.edge(s, loc)
                    && !self.free_edges.contains(&e)
                {
                    self.free_edges.push(e);
                }
            }
            Kind::Vertex(p) => {
                let q = loc.m.point(&p);
                if !self
                    .free_points
                    .iter()
                    .any(|x| (x - q).norm() <= super::tol::CONFUSION)
                {
                    self.free_points.push(q);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Building straight from the file

/// Builds the body from the file as it is: its vertices, its edges' 3D curves and ranges, its
/// faces on surfaces with OCCT's (u, v) (the file's own axes) and their loops with the file's
/// p-curves (a seam's two). Shared shapes stay shared. An edge with p-curves and no 3D curve
/// gets one lifted from its first p-curve ([`lifted`]); a wire whose edges don't chain as listed
/// is put in order. Anything else the file leaves out (an edge without a p-curve on a face, an
/// edge with no curve at all) makes it give up (the caller rebuilds through [`XBuilder`]
/// instead).
struct Direct<'a> {
    f: &'a File,
    body: Body,
    verts: HashMap<(usize, [i64; 12]), usize>,
    /// Vertices by position (1e-6 cells), to merge coincident vertex records.
    grid: HashMap<[i64; 3], Vec<usize>>,
    edges: HashMap<(usize, [i64; 12]), usize>,
    shells: Vec<(Vec<usize>, bool)>,
    solids: Vec<Vec<usize>>,
    free_faces: Vec<usize>,
}

impl Direct<'_> {
    fn vertex(&mut self, s: usize, loc: &Loc) -> R<usize> {
        let key = (s, loc_key(loc));
        if let Some(&v) = self.verts.get(&key) {
            return Ok(v);
        }
        let Kind::Vertex(p) = self.f.shapes[s].kind else {
            return Err("not a vertex".into());
        };
        let p = loc.m.point(&p);
        // Two vertex records at one point (files from other systems): one vertex.
        let cell = |q: &P3| {
            [
                (q.x * 1e6).round() as i64,
                (q.y * 1e6).round() as i64,
                (q.z * 1e6).round() as i64,
            ]
        };
        let c = cell(&p);
        let mut found = None;
        'look: for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(vs) = self.grid.get(&[
                        c[0].saturating_add(dx),
                        c[1].saturating_add(dy),
                        c[2].saturating_add(dz),
                    ]) && let Some(&v) = vs
                        .iter()
                        .find(|&&v| (self.body.verts[v].p - p).norm() < 1e-7)
                    {
                        found = Some(v);
                        break 'look;
                    }
                }
            }
        }
        let v = match found {
            Some(v) => v,
            None => {
                let v = self.body.add_vertex(p);
                self.grid.entry(c).or_default().push(v);
                v
            }
        };
        let tol = self.f.shapes[s].tol;
        if tol > self.body.verts[v].tol {
            self.body.verts[v].tol = tol;
        }
        self.verts.insert(key, v);
        Ok(v)
    }

    fn edge(&mut self, s: usize, loc: &Loc) -> R<usize> {
        let key = (s, loc_key(loc));
        if let Some(&e) = self.edges.get(&key) {
            return Ok(e);
        }
        let sh = &self.f.shapes[s];
        let Kind::Edge(c3, degenerate, ref pcs) = sh.kind else {
            return Err("not an edge".into());
        };
        let (mut v0, mut v1) = (None, None);
        for &(o, vs, l) in &sh.subs.clone() {
            let vl = loc.then(&self.f.locs[l]);
            match o {
                Ori::Fwd => v0 = Some(self.vertex(vs, &vl)?),
                Ori::Rev => v1 = Some(self.vertex(vs, &vl)?),
                _ => {}
            }
        }
        let (v0, v1) = match (v0, v1) {
            (Some(a), Some(b)) => (a, b),
            (Some(a), None) | (None, Some(a)) => (a, a),
            _ => return Err(format!("edge {s} has no vertices")),
        };
        let e = if degenerate {
            let (a, b) = pcs.first().map_or((0.0, 1.0), |r| r.range);
            self.body.add_degenerate(self.body.verts[v0].p, a, b, v0)
        } else {
            let (c, a, b) = match c3 {
                Some((ci, cl, a, b)) => (
                    to_curve(&self.f.curves[ci], a, b)?
                        .transformed(&compose(&loc.m, &self.f.locs[cl].m)),
                    a,
                    b,
                ),
                None => lifted(self.f, pcs, loc).map_err(|e| format!("edge {s}: {e}"))?,
            };
            self.body.add_edge(c, a, b, v0, v1)
        };
        self.body.edges[e].tol = self.f.shapes[s].tol.max(TOL_MIN);
        self.edges.insert(key, e);
        Ok(e)
    }

    fn face(&mut self, s: usize, ori: Ori, loc: &Loc) -> R<usize> {
        let sh = self.f.shapes[s].clone();
        let Kind::Face(Some((si, sl))) = sh.kind else {
            return Err(format!("face {s} has no surface"));
        };
        let (surf, _) = to_surface(&self.f.surfaces[si], true)?;
        let sloc = loc.then(&self.f.locs[sl]);
        let surf = surf.transformed(&sloc.m);
        let skey = loc_key(&sloc);
        let mut loops: Vec<Vec<Coedge>> = Vec::new();
        for &(wo, ws, wl) in &sh.subs {
            let wloc = loc.then(&self.f.locs[wl]);
            let mut lp = Vec::new();
            for &(eo, es, el) in &self.f.shapes[ws].subs.clone() {
                // Relative to the face (its own orientation aside), then as the face uses it.
                let rel = wo.compose(eo);
                if !matches!(rel, Ori::Fwd | Ori::Rev) {
                    continue;
                }
                let eloc = wloc.then(&self.f.locs[el]);
                let e = self.edge(es, &eloc)?;
                let Kind::Edge(_, _, ref pcs) = self.f.shapes[es].kind else {
                    unreachable!()
                };
                let rep = pcs
                    .iter()
                    .find(|r| r.surf == si && loc_key(&eloc.then(&self.f.locs[r.loc])) == skey);
                let ed = &self.body.edges[e];
                let pc = match rep {
                    Some(rep) => {
                        let idx = match rep.pc2 {
                            Some(p2) if rel == Ori::Rev => p2,
                            _ => rep.pc,
                        };
                        to_curve(&self.f.curves2d[idx], ed.t[0], ed.t[1])?
                    }
                    // OCCT keeps no p-curves on planes (it projects the edge when asked).
                    None if matches!(surf, Surface::Plane { .. }) && !ed.degenerate => {
                        super::pcurve::pcurve(&surf, &ed.curve, ed.t[0], ed.t[1], None)
                    }
                    None => return Err(format!("edge {es} has no p-curve on face {s}")),
                };
                lp.push(Coedge {
                    edge: e,
                    fwd: ori.compose(rel) == Ori::Fwd,
                    pc,
                });
            }
            if !lp.is_empty() {
                loops.push(lp);
            }
        }
        let sense = ori == Ori::Fwd;
        let chained: Option<Vec<Vec<Coedge>>> =
            loops.iter().map(|l| chain(l.clone(), &self.body)).collect();
        let mut loops = match chained {
            Some(ls) => ls,
            None => {
                // A wire whose edges don't chain as listed (out of order, run the other way, or
                // two loops in one wire): put in order as OCCT's `ShapeFix_Wire::FixReorder`
                // does ([`super::heal::reorder_face_loops`]), when that closes every loop.
                self.body.faces.push(Face {
                    surf: surf.clone(),
                    sense,
                    loops,
                });
                let fi = self.body.faces.len() - 1;
                super::heal::reorder_face_loops(&mut self.body, fi, 0.0);
                let ls = self.body.faces.pop().map(|f| f.loops).unwrap_or_default();
                let b = &self.body;
                let closes = |l: &Vec<Coedge>| {
                    (0..l.len()).all(|k| b.co_end(&l[k]) == b.co_start(&l[(k + 1) % l.len()]))
                };
                if ls.is_empty() || !ls.iter().all(closes) {
                    return Err(format!("a loop of face {s} doesn't chain"));
                }
                ls
            }
        };
        for l in &mut loops {
            align_periods(&surf, l, &self.body);
        }
        // The outer loop first: the one enclosing the most (counter-clockwise about the outward
        // normal: positive in (u, v) for a forward face).
        let area = |l: &Vec<Coedge>| uv_area(l, &self.body) * if sense { 1.0 } else { -1.0 };
        if let Some(k) =
            (0..loops.len()).max_by(|&a, &b| area(&loops[a]).total_cmp(&area(&loops[b])))
        {
            loops.swap(0, k);
        }
        self.body.faces.push(Face { surf, sense, loops });
        Ok(self.body.faces.len() - 1)
    }

    fn shell(&mut self, s: usize, ori: Ori, loc: &Loc) -> R<Vec<usize>> {
        let mut faces = Vec::new();
        for &(o, fs, l) in &self.f.shapes[s].subs.clone() {
            if matches!(self.f.shapes[fs].kind, Kind::Face(_)) {
                faces.push(self.face(fs, ori.compose(o), &loc.then(&self.f.locs[l]))?);
            }
        }
        Ok(faces)
    }

    fn visit(&mut self, s: usize, ori: Ori, loc: &Loc) -> R<()> {
        let sh = self.f.shapes[s].clone();
        match sh.kind {
            Kind::Compound => {
                for &(o, c, l) in &sh.subs {
                    self.visit(c, ori.compose(o), &loc.then(&self.f.locs[l]))?;
                }
            }
            Kind::Solid => {
                let mut shells = Vec::new();
                for &(o, c, l) in &sh.subs {
                    if matches!(self.f.shapes[c].kind, Kind::Shell) {
                        let faces = self.shell(c, ori.compose(o), &loc.then(&self.f.locs[l]))?;
                        self.shells.push((faces, true));
                        shells.push(self.shells.len() - 1);
                    }
                }
                self.solids.push(shells);
            }
            Kind::Shell => {
                let faces = self.shell(s, ori, loc)?;
                self.shells.push((faces, sh.closed));
            }
            Kind::Face(_) => {
                let f = self.face(s, ori, loc)?;
                self.free_faces.push(f);
            }
            _ => {}
        }
        Ok(())
    }
}

/// A loop's coedges in order, each starting where the one before ends (files list a wire's
/// edges in any order); at a fork, an edge that closes on itself first (a circle round a
/// cylinder: taken later, the chain would leave it behind), and another edge before going
/// straight back along the same one (a seam's two uses). None when they don't close up.
fn chain(mut rest: Vec<Coedge>, b: &Body) -> Option<Vec<Coedge>> {
    if rest.is_empty() {
        return None;
    }
    let mut out = vec![rest.remove(0)];
    while !rest.is_empty() {
        let last = out.last()?;
        let end = b.co_end(last);
        let i = rest
            .iter()
            .enumerate()
            .filter(|(_, c)| b.co_start(c) == end)
            .min_by_key(|(i, c)| {
                (
                    (c.edge == last.edge) as usize,
                    (b.co_end(c) != end) as usize,
                    *i,
                )
            })
            .map(|(i, _)| i)?;
        out.push(rest.remove(i));
    }
    (b.co_end(out.last()?) == b.co_start(&out[0])).then_some(out)
}

/// Shifts each p-curve of a loop by whole periods to start where the one before ended (loops
/// are continuous in (u, v); the file's p-curves may sit in any period).
fn align_periods(s: &Surface, l: &mut [Coedge], b: &Body) {
    let (pu, pv) = (
        if s.u_periodic() { Some(TAU) } else { None },
        if s.v_periodic() { Some(TAU) } else { None },
    );
    if pu.is_none() && pv.is_none() {
        return;
    }
    let ends = |c: &Coedge| {
        let (a, z) = co_range(&b.edges[c.edge], c.fwd);
        (c.pc.point(a), c.pc.point(z))
    };
    for k in 1..l.len() {
        let prev_end = ends(&l[k - 1]).1;
        let start = ends(&l[k]).0;
        let shift = |d: f64, per: Option<f64>| per.map_or(0.0, |p| (d / p).round() * p);
        let (du, dv) = (
            shift(prev_end.x - start.x, pu),
            shift(prev_end.y - start.y, pv),
        );
        if du != 0.0 || dv != 0.0 {
            l[k].pc = l[k]
                .pc
                .transformed(&Affine::translation(V3::new(du, dv, 0.0)));
        }
    }
}

/// The signed area a loop encloses in (u, v) (sampled).
fn uv_area(l: &[Coedge], b: &Body) -> f64 {
    let mut pts: Vec<P3> = Vec::new();
    for c in l {
        let (a, z) = co_range(&b.edges[c.edge], c.fwd);
        for i in 0..16 {
            pts.push(c.pc.point(a + (z - a) * i as f64 / 16.0));
        }
    }
    let n = pts.len();
    (0..n)
        .map(|i| {
            let (p, q) = (pts[i], pts[(i + 1) % n]);
            p.x * q.y - q.x * p.y
        })
        .sum::<f64>()
        / 2.0
}

fn build_direct(f: &File) -> R<Body> {
    let mut d = Direct {
        f,
        body: Body::default(),
        verts: HashMap::new(),
        grid: HashMap::new(),
        edges: HashMap::new(),
        shells: Vec::new(),
        solids: Vec::new(),
        free_faces: Vec::new(),
    };
    let (o, s, l) = f.root;
    d.visit(s, o, &f.locs[l])?;
    if d.body.faces.is_empty() {
        return Err("no faces".into());
    }
    let mut body = d.body;
    for (faces, closed) in d.shells {
        body.shells.push(Shell { faces, closed });
    }
    for shells in d.solids {
        body.solids.push(Solid { shells });
    }
    let free = d.free_faces;
    body.shells
        .extend(body.face_components(&free).into_iter().map(|faces| Shell {
            faces,
            closed: false,
        }));
    open_unclosed_shells(&mut body);
    Ok(body)
}

/// Marks open the shells outside solids that the file flags closed but that have an edge only
/// one of their faces uses (a sheet's boundary): OCCT's closed flag is the writer's word, the
/// edges are the shell's.
fn open_unclosed_shells(b: &mut Body) {
    let in_solid: std::collections::HashSet<usize> = b
        .solids
        .iter()
        .flat_map(|s| s.shells.iter().copied())
        .collect();
    for si in 0..b.shells.len() {
        if !b.shells[si].closed || in_solid.contains(&si) {
            continue;
        }
        let mut uses: HashMap<usize, usize> = HashMap::new();
        for c in b.shells[si]
            .faces
            .iter()
            .flat_map(|&f| b.faces[f].loops.iter().flatten())
        {
            *uses.entry(c.edge).or_default() += 1;
        }
        if uses.iter().any(|(&e, &n)| n == 1 && !b.edges[e].degenerate) {
            b.shells[si].closed = false;
        }
    }
}

/// Splits each shell of `faces` only (the faces outside any shell of the file, gathered in one)
/// into a shell per edge-connected piece: OCCT checks a shell's faces connected, and the file
/// put those faces in no shell.
fn split_free_shell(b: &mut Body, faces: &[usize]) {
    let free: std::collections::HashSet<usize> = faces.iter().copied().collect();
    let shells = std::mem::take(&mut b.shells);
    for sh in shells {
        if !sh.closed && !sh.faces.is_empty() && sh.faces.iter().all(|f| free.contains(f)) {
            let pieces = b.face_components(&sh.faces);
            b.shells.extend(pieces.into_iter().map(|faces| Shell {
                faces,
                closed: false,
            }));
        } else {
            b.shells.push(sh);
        }
    }
}

/// What a BRep file holds.
pub struct BrepRead {
    /// The faces' body (solids, then sheets); None when the file has no faces.
    pub body: Option<Body>,
    /// The middle points of the body's edges in OCCT's exploration order (the order of first
    /// use, depth first, as `TopExp_Explorer` meets them; a degenerate edge at its vertex). The faces
    /// keep the file's order.
    pub edge_order: Vec<P3>,
    /// The degenerate edges' places in `edge_order` (their vertices' points there).
    pub degenerate_order: Vec<usize>,
    /// Edges outside faces (a wire's or a compound's): curve and parameter range.
    pub free_edges: Vec<(Curve, f64, f64)>,
    /// Whether the free edges are in wires (else loose edges).
    pub wires: bool,
    /// The vertices of a file with no faces or edges.
    pub points: Vec<P3>,
    /// Vertices outside any edge in a file with faces or edges (a compound's loose vertices).
    pub loose_points: Vec<P3>,
    pub warnings: Vec<String>,
}

/// [`read`], with free edges and OCCT's edge order.
pub fn read_ordered(bytes: &[u8]) -> R<BrepRead> {
    let text = String::from_utf8_lossy(bytes);
    let mut f = parse(&text)?;
    // The shape itself used internal or external (DRAW saves a face so): as it is. Below the
    // root those uses stay left out, as OCCT's explorers leave them.
    if !matches!(f.root.0, Ori::Fwd | Ori::Rev) {
        f.root.0 = Ori::Fwd;
    }
    bound_unbounded_planes(&mut f);
    let mut xb = XBuilder::new();
    xb.trust_orientation = true;
    let mut b = Builder {
        f: &f,
        xb,
        verts: HashMap::new(),
        edges: HashMap::new(),
        free_faces: Vec::new(),
        free_edges: Vec::new(),
        wires: false,
        free_points: Vec::new(),
        warnings: Vec::new(),
        degenerate: Vec::new(),
    };
    let (o, s, l) = f.root;
    b.visit(s, o, &f.locs[l]);
    let free = std::mem::take(&mut b.free_faces);
    if !free.is_empty() {
        b.xb.shell(free.clone(), false);
    }
    let mut warnings = std::mem::take(&mut b.warnings);
    let free_edges: Vec<(Curve, f64, f64)> = b
        .free_edges
        .iter()
        .map(|&e| {
            let ed = &b.xb.body.edges[e];
            (ed.curve.clone(), ed.t[0], ed.t[1])
        })
        .collect();
    // The edges' middles in order, the degenerate ones' points where they came.
    let mut mids: Vec<P3> = Vec::new();
    let mut degenerate_order: Vec<usize> = Vec::new();
    let mut pending = b.degenerate.iter().peekable();
    for (i, e) in b.xb.body.edges.iter().enumerate() {
        while let Some(&(_, p)) = pending.next_if(|x| x.0 <= i) {
            degenerate_order.push(mids.len());
            mids.push(p);
        }
        mids.push(e.point(e.mid_t()));
    }
    for &(_, p) in pending {
        degenerate_order.push(mids.len());
        mids.push(p);
    }
    let has_faces = !b.xb.faces.is_empty();
    // Straight from the file when that gives a valid body; else rebuilt.
    let direct = if has_faces && std::env::var_os("CADRS_OCCT_REBUILD").is_none() {
        match build_direct(&f) {
            Ok(x) => {
                let issues = x.check();
                // A face of no area (bounds left out: a whole sphere) or of negative area (a loop
                // on a periodic surface read the wrong way round) means the file's loops don't
                // say what the rebuild works out.
                let bad_face = if issues.is_empty() {
                    (0..x.faces.len()).find(|&f| !(super::mass::face_area(&x, f).0 > 0.0))
                } else {
                    None
                };
                if let Some(f) = bad_face {
                    warnings.push(format!(
                        "rebuilt (face {f} of the file's body has no area of its own)"
                    ));
                    None
                } else if issues.is_empty() {
                    Some(x)
                } else {
                    warnings.push(format!("rebuilt (the file's body: {})", issues[0]));
                    None
                }
            }
            Err(e) => {
                warnings.push(format!("rebuilt ({e})"));
                None
            }
        }
    } else {
        None
    };
    let body = if let Some(x) = direct {
        Some(x)
    } else if has_faces {
        let (mut body, w, map) = b.xb.finish_mapped();
        warnings.extend(w);
        // Solids keep their shells and a shell keeps its faces (in a shell of the file or not);
        // the faces of no shell make a shell per piece, and an open shell isn't closed.
        let free: Vec<usize> = free
            .iter()
            .filter_map(|&f| map.get(f).copied().flatten())
            .collect();
        split_free_shell(&mut body, &free);
        open_unclosed_shells(&mut body);
        Some(body).filter(|x| !x.faces.is_empty())
    } else {
        None
    };
    // A body with numbers no geometry can have (NaN, infinite) or dangling references: an
    // error, before anything integrates over it.
    if let Some(why) = body.as_ref().and_then(|x| {
        x.check_refs()
            .into_iter()
            .next()
            .or_else(|| x.size_issue(super::tol::SIZE))
    }) {
        return Err(why);
    }
    // A solid enclosing a negative volume (a whole torus, whose one loop of seams runs either
    // way): turned round.
    let body = if let Some(mut x) = body {
        for si in 0..x.solids.len() {
            let faces = x.solid_faces(si);
            let sub = super::kernel::subset(&x, &faces, true);
            if super::topo::points_in(&sub) {
                for f in faces {
                    Body::reverse_face(&mut x.faces[f]);
                }
            }
        }
        Some(x)
    } else {
        None
    };
    // A file of only vertices (or of edges without curves, a wire collapsed to a point).
    let mut points: Vec<P3> = Vec::new();
    let mut loose_points = std::mem::take(&mut b.free_points);
    if body.is_none() && free_edges.is_empty() {
        loose_points.clear();
        fn walk(f: &File, s: usize, loc: &Loc, out: &mut Vec<P3>) {
            if let Kind::Vertex(p) = f.shapes[s].kind {
                let q = loc.m.point(&p);
                if !out.iter().any(|x| (x - q).norm() < 1e-7) {
                    out.push(q);
                }
            }
            for &(_, c, l) in &f.shapes[s].subs {
                walk(f, c, &loc.then(&f.locs[l]), out);
            }
        }
        walk(&f, s, &f.locs[l], &mut points);
        if points.is_empty() {
            return Err(format!("no faces or edges read ({})", warnings.join("; ")));
        }
    }
    Ok(BrepRead {
        body,
        edge_order: mids,
        degenerate_order,
        free_edges,
        wires: b.wires,
        points,
        loose_points,
        warnings,
    })
}

/// Reads an OCCT BRep text file into a body and the problems met: its solids, then sheets,
/// then the edges outside faces chained into wires ([`Body::chain_wires`]: a wire's, or loose
/// edges of a compound) and the vertices outside edges as acorn points. A file of only edges
/// gives a wire body, of only vertices a point body.
pub fn read(bytes: &[u8]) -> R<(Body, Vec<String>)> {
    let r = read_ordered(bytes)?;
    let mut body = r.body.unwrap_or_default();
    with_loose(
        &mut body,
        &r.free_edges,
        if r.points.is_empty() {
            &r.loose_points
        } else {
            &r.points
        },
    );
    if body.faces.is_empty() && body.edges.is_empty() && body.points.is_empty() {
        return Err("no faces, edges or vertices read".to_string());
    }
    if let Some(why) = body.check().into_iter().next() {
        return Err(format!("the file's edges and vertices: {why}"));
    }
    Ok((body, r.warnings))
}

/// Adds edges outside faces (chained into wires) and loose vertices (acorn points) to `b`. An
/// edge end within [`super::tol::POINT`] of another such edge's end shares its vertex.
fn with_loose(b: &mut Body, edges: &[(Curve, f64, f64)], points: &[P3]) {
    let first_new = b.verts.len();
    let vertex = |b: &mut Body, p: P3| match (first_new..b.verts.len())
        .find(|&v| (b.verts[v].p - p).norm() <= super::tol::POINT)
    {
        Some(v) => v,
        None => b.add_vertex(p),
    };
    for (c, t0, t1) in edges {
        let (v0, v1) = (vertex(b, c.point(*t0)), vertex(b, c.point(*t1)));
        b.add_edge(c.clone(), *t0, *t1, v0, v1);
    }
    if !edges.is_empty() {
        b.chain_wires();
    }
    for &p in points {
        let v = b.add_vertex(p);
        b.points.push(v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BodyClass;

    /// A unit box as OCCT 7.8 writes it (`box b 1 2 3; save b`), trimmed of its p-curves.
    const BOX: &str = include_str!("../../tests/data/occt_box.brep");

    #[test]
    fn reads_a_box() {
        let (b, w) = read(BOX.as_bytes()).unwrap();
        assert_eq!(
            (b.verts.len(), b.edges.len(), b.faces.len(), b.solids.len()),
            (8, 12, 6, 1),
            "{w:?}"
        );
        let v = super::super::mass::body_volume(&b);
        assert!((v - 6.0).abs() < 1e-9, "{v}");
    }

    /// A surface of linear extrusion of a line (unbounded: no parameter range of its own) is
    /// read, parametrized as OCCT's (CFI_5_b12fgg.rle's two such faces were dropped, leaving 7
    /// of its 12 edges).
    #[test]
    fn reads_extrusions_of_lines() {
        let g = GSurf::Extrusion(
            V3::z(),
            GCurve::Line(P3::new(1.0, 2.0, 0.0), V3::new(2.0, 0.0, 0.0)),
        );
        let (s, same) = to_surface(&g, false).unwrap();
        assert!(same);
        assert!((s.eval(3.0, 4.0) - P3::new(4.0, 2.0, 4.0)).norm() < 1e-12);
    }

    #[test]
    fn periodic_splines_unroll() {
        // A periodic quadratic through 4 poles (a closed curve): unrolled, it closes and runs
        // through the same points as its rotated copy.
        let poles: Vec<H> = [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)]
            .iter()
            .map(|&(x, y)| hom(&P3::new(x, y, 0.0), 1.0))
            .collect();
        let s = Spline {
            degree: 2,
            poles,
            knots: vec![0.0, 1.0, 2.0, 3.0, 4.0],
            mults: vec![1; 5],
            periodic: true,
        };
        let c = spline_curve(&s, 0.0, 4.0).unwrap();
        assert!((c.point(0.0) - c.point(4.0)).norm() < 1e-12);
        let shifted = spline_curve(&s, 1.5, 5.5).unwrap();
        for k in 0..=8 {
            let t = 1.5 + 0.25 * k as f64;
            let q = if t > 4.0 { t - 4.0 } else { t };
            assert!((shifted.point(t) - c.point(q)).norm() < 1e-12, "{t}");
        }
        // Quadratic B-spline with knots i: at knot 1 the point is the mean of poles 0 and 1.
        let m = (c.point(1.0) - P3::new(0.5, 0.5, 0.0)).norm();
        let m2 = (c.point(1.0) - P3::new(-0.5, 0.5, 0.0)).norm();
        assert!(m < 1e-12 || m2 < 1e-12, "{:?}", c.point(1.0));
    }

    /// A 2 × 2 square face on the plane z = 0 whose four edges have p-curves and no 3D curves
    /// (as files from older writers have them).
    const PCURVES_ONLY: &str = "CASCADE Topology V1, (c) Matra-Datavision
Locations 0
Curve2ds 4
1 0 0 1 0
1 2 0 0 1
1 2 2 -1 0
1 0 2 0 -1
Curves 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 1
1 0 0 0 0 0 1 1 0 0 0 1 0
Triangulations 0

TShapes 10
Ve
1e-07
0 0 0
0 0

0101100
*
Ve
1e-07
2 0 0
0 0

0101100
*
Ve
1e-07
2 2 0
0 0

0101100
*
Ve
1e-07
0 2 0
0 0

0101100
*
Ed
 1e-07 1 1 0
2  1 1 0 0 2
0

0101000
+10 0 -9 0 *
Ed
 1e-07 1 1 0
2  2 1 0 0 2
0

0101000
+9 0 -8 0 *
Ed
 1e-07 1 1 0
2  3 1 0 0 2
0

0101000
+8 0 -7 0 *
Ed
 1e-07 1 1 0
2  4 1 0 0 2
0

0101000
+7 0 -10 0 *
Wi

0101000
+6 0 +5 0 +4 0 +3 0 *
Fa
0  1e-07 1 0

0111000
+2 0 *

+1 0
";

    #[test]
    fn edges_with_only_pcurves_get_3d_curves() {
        // The edges' curves are their p-curves carried onto the plane: the face reads whole.
        let (b, w) = read(PCURVES_ONLY.as_bytes()).unwrap();
        assert_eq!((b.faces.len(), b.edges.len()), (1, 4), "{w:?}");
        let issues = b.check_with(super::super::topo::CheckLevel::Full);
        assert!(issues.is_empty(), "{issues:?}");
        let a = super::super::mass::face_area(&b, 0).0;
        assert!((a - 4.0).abs() < 1e-9, "{a}");
        let e = &b.edges[0];
        assert!((e.point(e.mid_t()) - P3::new(1.0, 0.0, 0.0)).norm() < 1e-9);
    }

    #[test]
    fn open_shells_and_loose_faces() {
        // A box's faces less its top, in a shell the file flags closed: open. Its top and bottom
        // alone (faces of no shell, sharing no edge): a shell each.
        let (b, _) = read(BOX.as_bytes()).unwrap();
        let z = |f: usize| super::super::mass::face_area(&b, f).1.z;
        let top = (0..6).max_by(|&x, &y| z(x).total_cmp(&z(y))).unwrap();
        let bottom = (0..6).min_by(|&x, &y| z(x).total_cmp(&z(y))).unwrap();
        let mut open = b.clone();
        open.solids.clear();
        open.shells = vec![Shell {
            faces: (0..6).filter(|&f| f != top).collect(),
            closed: true,
        }];
        open_unclosed_shells(&mut open);
        assert!(!open.shells[0].closed);
        let mut loose = b.clone();
        loose.solids.clear();
        loose.shells = vec![Shell {
            faces: vec![top, bottom],
            closed: false,
        }];
        split_free_shell(&mut loose, &[top, bottom]);
        assert_eq!(loose.shells.len(), 2);
        assert!(
            loose
                .check_with(super::super::topo::CheckLevel::Full)
                .is_empty()
        );
    }

    #[test]
    fn loops_chain_round_closed_edges() {
        // newFS.rle's wire: a closed edge at a, a line from b to a, the same way back on another
        // line, a closed edge at b, listed in that order: from a the second line, then the closed
        // edge at b before the first line (taken first, the chain was left at a without it).
        let mut b = Body::default();
        let (va, vb) = (
            b.add_vertex(P3::new(0.0, 0.0, 1.0)),
            b.add_vertex(P3::origin()),
        );
        let line = Curve::Line {
            o: P3::origin(),
            d: V3::z(),
        };
        let ca = b.add_edge(line.clone(), 0.0, 1.0, va, va);
        let l1 = b.add_edge(line.clone(), 0.0, 1.0, vb, va);
        let l2 = b.add_edge(line.clone(), 0.0, 1.0, vb, va);
        let cb = b.add_edge(line.clone(), 0.0, 1.0, vb, vb);
        let co = |edge: usize, fwd: bool| Coedge {
            edge,
            fwd,
            pc: line.clone(),
        };
        let l = chain(
            vec![co(ca, false), co(l1, true), co(l2, false), co(cb, true)],
            &b,
        )
        .expect("the loop chains");
        assert_eq!(
            l.iter().map(|c| c.edge).collect::<Vec<_>>(),
            vec![ca, l2, cb, l1]
        );
    }

    /// A file's head with one plane, z = 0 with normal +z (DRAW's `plane p` as a face).
    fn plane_file(shapes: &str) -> String {
        format!(
            "CASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 1\n1 0 0 0 0 0 1 1 0 0 0 1 0\nTriangulations 0\n\n{shapes}"
        )
    }

    #[test]
    fn unbounded_plane_is_a_large_square() {
        // `mkface f p` with no bounds: OCCT's infinite plane, read as a square of side 2·UNBOUNDED.
        let text = plane_file("TShapes 1\nFa\n0  1e-07 1 0\n\n0101000\n*\n\n+1 0\n");
        let (b, w) = read(text.as_bytes()).unwrap();
        assert_eq!(
            (b.faces.len(), b.edges.len(), b.solids.len()),
            (1, 4, 0),
            "{w:?}"
        );
        assert!(
            b.check_with(super::super::topo::CheckLevel::Full)
                .is_empty()
        );
        let (a, c) = super::super::mass::face_area(&b, 0);
        assert!((a - 4.0 * UNBOUNDED * UNBOUNDED).abs() < 1e-6 * a, "{a}");
        assert!(c.coords.norm() < 1e-6 * UNBOUNDED, "{c:?}");
    }

    #[test]
    fn halfspace_is_a_box_behind_its_plane() {
        // A solid of one shell of the unbounded plane (DRAW's `halfspace`): the shell takes the
        // face forward, so the material lies below z = 0; the solid reversed, above it.
        for (solid_ori, below) in [("+", true), ("-", false)] {
            let text = plane_file(&format!(
                "TShapes 3\nFa\n0  1e-07 1 0\n\n0101000\n*\nSh\n\n0101100\n+3 0 *\nSo\n\n0100000\n{solid_ori}2 0 *\n\n+1 0\n"
            ));
            let (b, w) = read(text.as_bytes()).unwrap();
            assert_eq!(
                (b.faces.len(), b.edges.len(), b.solids.len()),
                (6, 12, 1),
                "{w:?}"
            );
            assert!(b.check().is_empty(), "{:?}", b.check());
            let m = super::super::mass::body_mass(&b);
            let l = UNBOUNDED;
            assert!(
                (m.volume - 8.0 * l * l * l).abs() < 1e-6 * m.volume,
                "{}",
                m.volume
            );
            assert!(
                (m.center_of_mass.z + if below { l } else { -l }).abs() < 1e-6 * l,
                "{:?}",
                m.center_of_mass
            );
        }
    }

    #[test]
    fn wires_and_loose_vertices_are_read() {
        // A compound of a face (the unbounded plane), an edge on the line y = 5 from x = 0 to 2,
        // in a wire with a second from x = 2 to 3, and a loose vertex: the body holds all three
        // kinds. Without the face, the wire alone.
        let edges = "Curves 1\n1 0 5 0 1 0 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 1\n1 0 0 0 0 0 1 1 0 0 0 1 0\nTriangulations 0\n\n";
        let shapes = "Ve\n1e-07\n0 5 0\n0 0\n\n0101101\n*\nVe\n1e-07\n2 5 0\n0 0\n\n0101101\n*\nVe\n1e-07\n3 5 0\n0 0\n\n0101101\n*\n\
            Ed\n 1e-07 1 1 0\n1  1 0 0 2\n0\n\n0101000\n+9 0 -8 0 *\nEd\n 1e-07 1 1 0\n1  1 0 2 3\n0\n\n0101000\n+8 0 -7 0 *\n\
            Wi\n\n0101000\n+6 0 +5 0 *\nVe\n1e-07\n7 7 7\n0 0\n\n0101101\n*\nFa\n0  1e-07 1 0\n\n0101000\n*\n";
        let head = "CASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 0\n";
        let with_face =
            format!("{head}{edges}TShapes 9\n{shapes}Co\n\n1100000\n+4 0 +3 0 +2 0 *\n\n+1 0\n");
        let (b, w) = read(with_face.as_bytes()).unwrap();
        assert_eq!(
            (b.faces.len(), b.wires.len(), b.points.len()),
            (1, 1, 1),
            "{w:?}"
        );
        assert_eq!(b.wires[0].edges.len(), 2);
        assert!((b.verts[b.points[0]].p - P3::new(7.0, 7.0, 7.0)).norm() < 1e-12);
        assert_eq!(b.class(), BodyClass::Mixed);
        assert!(b.check().is_empty(), "{:?}", b.check());
        // (References count back from the table's end: the same shapes renumbered.)
        let wire_only = format!(
            "{head}{edges}TShapes 6\n{}{}{}\n+1 0\n",
            &shapes[..shapes.find("Ed").unwrap()],
            "Ed\n 1e-07 1 1 0\n1  1 0 0 2\n0\n\n0101000\n+6 0 -5 0 *\nEd\n 1e-07 1 1 0\n1  1 0 2 3\n0\n\n0101000\n+5 0 -4 0 *\n",
            "Wi\n\n0101000\n+3 0 +2 0 *\n"
        );
        let (b, w) = read(wire_only.as_bytes()).unwrap();
        assert_eq!(
            (b.faces.len(), b.wires.len(), b.edges.len(), b.class()),
            (0, 1, 2, BodyClass::Wire),
            "{w:?}"
        );
        let len: f64 = b.edges.iter().map(|e| e.curve.length(e.t[0], e.t[1])).sum();
        assert!((len - 3.0).abs() < 1e-12, "{len}");
    }
}
