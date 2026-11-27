//! Operations on shapes: booleans, `clean`, prisms, revolves, sweeps, lofts, fillets,
//! chamfers, shells and point classification (`Shape.fuse`, `Solid.extrudeLinear`, …).

use super::geom::{BoundBox, Plane};
use super::make;
use super::shape::{EdgeGeom, Part, Repr, Shape, ShapeType};
use super::{BodyRef, CqError, CqResult, brep, op1, session, value_err};
use crate::native::curve::Curve;
use crate::native::math::{P3, V3};
use crate::native::surface::Surface;
use crate::{Axis, BoolOp, ExtrudeSpec, FaceId, FaceInput, Kernel, PathCurve, PointClass, Profile};
use nalgebra::Unit;
use std::sync::Arc;

/// The solids (and sheets) a shape is made of, compounds opened.
pub fn members(s: &Shape) -> Vec<Shape> {
    match &s.repr {
        Repr::Compound(v) => v.iter().flat_map(members).collect(),
        Repr::Body {
            kind: ShapeType::Compound,
            ..
        } => s.children(),
        _ => vec![s.clone()],
    }
}

fn is_3d(s: &Shape) -> bool {
    !matches!(
        s.shape_type(),
        ShapeType::Vertex | ShapeType::Edge | ShapeType::Wire
    )
}

fn boolean_bodies(op: BoolOp, target: &Shape, tools: &[Shape]) -> CqResult<Shape> {
    let t = target.body()?;
    let tb: Vec<BodyRef> = tools.iter().map(|s| s.body()).collect::<CqResult<_>>()?;
    let ids: Vec<crate::BodyId> = tb.iter().map(|b| b.0).collect();
    // (An empty result is an empty compound, as OCCT's: a cut taking everything, an
    // intersection of things apart.)
    let r = match op1(|k| k.boolean(op, t.0, &ids)) {
        Ok(b) => Ok(Shape::of_body(b)),
        Err(CqError::Empty(_)) => Ok(Shape::compound(vec![])),
        Err(e) => Err(e),
    };
    if let Ok(dir) = std::env::var("CADRS_CQ_ORACLE") {
        oracle_log(&dir, op, &t, &tb, &r);
    }
    r
}

/// The oracle's record of a boolean (`CADRS_CQ_ORACLE=<dir>`): its operands meshed (OFF files,
/// within a known deflection) and what it gave, for `tools/cadevolve/oracle.py` to recompute
/// with an independent mesh boolean (Manifold; CGAL's exact one where every face is flat).
fn oracle_log(dir: &str, op: BoolOp, target: &BodyRef, tools: &[BodyRef], r: &CqResult<Shape>) {
    use std::io::Write;
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let pid = std::process::id();
    let mut files = Vec::new();
    let mut planar = true;
    let mut kinds = Vec::new();
    let mut area = 0.0;
    let mut defl_used = 0.0_f64;
    for (j, b) in std::iter::once(target).chain(tools).enumerate() {
        let body = brep(b);
        planar &= body
            .faces
            .iter()
            .all(|f| matches!(f.surf, Surface::Plane { .. }));
        let mut c: std::collections::BTreeMap<&str, usize> = Default::default();
        for f in &body.faces {
            *c.entry(super::shape::surface_type(&f.surf).name())
                .or_default() += 1;
        }
        kinds.push(
            c.iter()
                .map(|(k, n)| format!("{k}:{n}"))
                .collect::<Vec<_>>()
                .join(" "),
        );
        let diag = session(|k| k.bounding_box(b.0))
            .map(|bb| bb.diagonal())
            .unwrap_or(1.0);
        let defl = (diag * 2e-4).max(1e-5);
        defl_used = defl_used.max(defl);
        area += session(|k| k.mass_properties(b.0))
            .map(|m| m.surface_area)
            .unwrap_or(0.0);
        let Ok(m) = session(|k| {
            k.tessellate(
                b.0,
                crate::Tessellation {
                    deflection: defl,
                    angle: 0.05,
                },
            )
        }) else {
            return;
        };
        if let Ok(bytes) = session(|k| k.write_body(b.0)) {
            let _ = std::fs::write(format!("{dir}/{pid}-{n}-{j}.body"), bytes);
        }
        let path = format!("{dir}/{pid}-{n}-{j}.off");
        let Ok(mut f) = std::fs::File::create(&path) else {
            return;
        };
        let _ = writeln!(f, "OFF\n{} {} 0", m.positions.len(), m.indices.len());
        for p in &m.positions {
            let _ = writeln!(f, "{:.17e} {:.17e} {:.17e}", p.x, p.y, p.z);
        }
        for t in &m.indices {
            let _ = writeln!(f, "3 {} {} {}", t[0], t[1], t[2]);
        }
        files.push(path);
    }
    let (status, vol, out_area) = match r {
        Ok(s) => (
            "ok",
            s.volume().unwrap_or(f64::NAN),
            s.area().unwrap_or(f64::NAN),
        ),
        Err(_) => ("error", f64::NAN, f64::NAN),
    };
    let err = match r {
        Err(e) => e.to_string().replace('"', "'"),
        Ok(_) => String::new(),
    };
    let rec = format!(
        "{{\"op\": \"{op:?}\", \"files\": {files:?}, \"kinds\": {kinds:?}, \"planar\": {planar}, \"deflection\": {defl_used:e}, \"area\": {area:e}, \"status\": \"{status}\", \"vol\": {}, \"out_area\": {}, \"err\": \"{err}\"}}",
        if vol.is_finite() {
            format!("{vol:.17e}")
        } else {
            "null".into()
        },
        if out_area.is_finite() {
            format!("{out_area:.17e}")
        } else {
            "null".into()
        },
    );
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(format!("{dir}/booleans.jsonl"))
    {
        let _ = writeln!(f, "{rec}");
    }
}

/// `fuse(*others)`: the union of the shape's members and the others' (a compound's members
/// fused with one another too, as OCCT's general fuse does).
pub fn fuse(a: &Shape, others: &[Shape]) -> CqResult<Shape> {
    let mut all: Vec<Shape> = members(a);
    for o in others {
        all.extend(members(o));
    }
    let (solid, rest): (Vec<Shape>, Vec<Shape>) = all.into_iter().partition(is_3d);
    if solid.is_empty() {
        return Ok(Shape::compound(rest));
    }
    let r = if solid.len() == 1 {
        solid[0].clone()
    } else {
        boolean_bodies(BoolOp::Union, &solid[0], &solid[1..])?
    };
    if rest.is_empty() {
        Ok(r)
    } else {
        let mut v = vec![r];
        v.extend(rest);
        Ok(Shape::compound(v))
    }
}

/// `cut(*tools)`.
pub fn cut(a: &Shape, tools: &[Shape]) -> CqResult<Shape> {
    let tools: Vec<Shape> = tools.iter().flat_map(members).filter(is_3d).collect();
    if tools.is_empty() || !is_3d(a) {
        return Ok(a.clone());
    }
    boolean_bodies(BoolOp::Subtract, a, &tools)
}

/// `intersect(*tools)`: what the shape and the tools' union share.
pub fn intersect(a: &Shape, tools: &[Shape]) -> CqResult<Shape> {
    let tools: Vec<Shape> = tools.iter().flat_map(members).filter(is_3d).collect();
    if tools.is_empty() {
        return value_err!("nothing to intersect with");
    }
    let tool = if tools.len() == 1 {
        tools[0].clone()
    } else {
        boolean_bodies(BoolOp::Union, &tools[0], &tools[1..])?
    };
    let am = members(a);
    let target = if am.len() > 1 {
        boolean_bodies(BoolOp::Union, &am[0], &am[1..])?
    } else {
        a.clone()
    };
    boolean_bodies(BoolOp::Intersect, &target, &[tool])
}

/// `clean()`: faces that continue one another merged (OCCT's `UnifySameDomain`).
pub fn clean(s: &Shape) -> CqResult<Shape> {
    match &s.repr {
        Repr::Body { body, kind }
            if matches!(
                kind,
                ShapeType::Solid | ShapeType::Compound | ShapeType::Shell
            ) =>
        {
            let b = body.clone();
            match op1(|k| k.simplify(b.0)) {
                Ok(r) => {
                    let mut out = Shape::of_body(r);
                    out.label = s.label.clone();
                    Ok(out)
                }
                Err(_) => Ok(s.clone()),
            }
        }
        Repr::Compound(v) => Ok(Shape::compound(
            v.iter().map(clean).collect::<CqResult<_>>()?,
        )),
        _ => Ok(s.clone()),
    }
}

// ------------------------------------------------------------------ faces as inputs

/// A planar face of a body as an operation's input: the face and the sketch plane it lies on.
fn face_input(f: &Shape) -> CqResult<(BodyRef, FaceInput, crate::Plane)> {
    let body = match &f.repr {
        Repr::Part {
            body,
            part: Part::Face(_),
            ..
        }
        | Repr::Body {
            body,
            kind: ShapeType::Face,
        } => body.clone(),
        _ => return value_err!("a face is needed, not a {}", f.shape_type().name()),
    };
    let (b, fi) = f.face_ref().expect("a face");
    let face = &b.faces[fi];
    let Surface::Plane { f: fr } = &face.surf else {
        return value_err!("the face is not planar");
    };
    let n = if face.sense { fr.z } else { -fr.z };
    let plane = crate::Plane {
        origin: fr.o,
        x_dir: Unit::new_normalize(fr.x),
        normal: Unit::new_normalize(n),
    };
    Ok((
        body.clone(),
        FaceInput {
            body: body.0,
            face: FaceId(fi as u64),
            source: 0,
        },
        plane,
    ))
}

/// Faces grouped by the plane they lie on (one operation each).
fn by_plane(faces: &[Shape]) -> CqResult<Vec<(crate::Plane, Vec<FaceInput>, Vec<BodyRef>)>> {
    let mut groups: Vec<(crate::Plane, Vec<FaceInput>, Vec<BodyRef>)> = Vec::new();
    for f in faces {
        let (b, fi, pl) = face_input(f)?;
        let same = groups.iter_mut().find(|(g, _, _)| {
            g.normal.dot(&pl.normal) > 1.0 - 1e-12
                && (pl.origin - g.origin).dot(&g.normal).abs()
                    < 1e-9 * (1.0 + pl.origin.coords.norm())
        });
        match same {
            Some((_, v, bs)) => {
                v.push(fi);
                bs.push(b);
            }
            None => groups.push((pl, vec![fi], vec![b])),
        }
    }
    Ok(groups)
}

/// Shapes made separately, as one: a compound when several.
fn together(mut v: Vec<Shape>) -> Shape {
    if v.len() == 1 {
        v.remove(0)
    } else {
        Shape::compound(v)
    }
}

/// `Solid.extrudeLinear(face, vector, taper)`: each face swept along `v`.
pub fn extrude(faces: &[Shape], v: V3, taper_deg: f64) -> CqResult<Shape> {
    let len = v.norm();
    if len == 0.0 {
        return value_err!("BRepSweep_Translation: null extrusion vector");
    }
    if taper_deg != 0.0 {
        return faces
            .iter()
            .map(|f| tapered(f, v, taper_deg))
            .collect::<CqResult<Vec<_>>>()
            .map(together);
    }
    let mut out = Vec::new();
    for (plane, inputs, _keep) in by_plane(faces)? {
        let mut dir = v / len;
        if dir.dot(&plane.normal).abs() < 1e-12 {
            return value_err!("the extrusion direction lies in the face's plane");
        }
        // (The faces stay referenced until the operation is done.)
        let profile = Profile::new(plane, vec![]);
        let mut spec = ExtrudeSpec::blind(Unit::new_normalize(dir), len);
        spec.faces = inputs;
        let r = op1(|k| k.extrude_with(&profile, &spec));
        let r = match r {
            Ok(r) => r,
            Err(e) => {
                // The other way round, then moved back (a direction against the face's normal).
                dir = -dir;
                let _ = dir;
                return Err(e);
            }
        };
        out.push(Shape::of_body(r));
    }
    Ok(together(out))
}

/// A prism whose sides lean in by `taper` degrees (OCCT's `LocOpe_DPrism`): a loft from the face
/// to its outline offset in by `h·tan(taper)` at height `h`.
fn tapered(f: &Shape, v: V3, taper_deg: f64) -> CqResult<Shape> {
    let (_, _, pl) = face_input(f)?;
    let n = pl.normal.into_inner();
    let d = if v.dot(&n) >= 0.0 { 1.0 } else { -1.0 };
    let h = v.norm();
    let inset = h * taper_deg.to_radians().tan();
    let outer = f.outer_wire()?.edge_geoms();
    // A circle's tapered prism is a cone, made exactly; past its apex (a taper steep enough
    // to close the circle before the depth) the cone goes on as its second nappe, the two
    // meeting at the apex point (as OCCT's draft prism makes it).
    if let [g] = outer.as_slice()
        && let Curve::Circle { c, r, .. } = &g.curve
        && g.t1 - g.t0 >= std::f64::consts::TAU - 1e-9
        && f.inner_wires()?.is_empty()
    {
        let dir = n * d;
        // (A tip within rounding of the axis is the apex itself.)
        let r2 = if (r - inset).abs() <= 1e-9 * r.max(1.0) {
            0.0
        } else {
            r - inset
        };
        if r2 >= 0.0 {
            return make::cone(*r, r2, h, c.coords, dir, 360.0);
        }
        let h1 = h * r / inset;
        let first = make::cone(*r, 0.0, h1, c.coords, dir, 360.0)?;
        let apex = c.coords + dir * h1;
        let second = make::cone(0.0, -r2, h - h1, apex, dir, 360.0)?;
        return Ok(Shape::compound(vec![first, second]));
    }
    let plane = Plane::of_axes(pl.origin.coords, pl.x_dir.into_inner(), n);
    let top = offset_wire(&outer, &plane, -inset, true)?;
    let top: Vec<EdgeGeom> = top
        .into_iter()
        .map(|g| g.transformed(&crate::native::math::Affine::translation(n * d * h)))
        .collect();
    let s = loft(&[LoftItem::Wire(outer), LoftItem::Wire(top)], true)?;
    let holes = f.inner_wires()?;
    if holes.is_empty() {
        return Ok(s);
    }
    value_err!("Inner wires not allowed with tapered extrusion")
}

/// A closed planar wire offset by `d` (outwards when positive): `sharp` corners extended (OCCT's
/// intersection join; straight-sided wires) or rounded (arc join). Several wires when the
/// offset splits it.
pub fn offset_wires(
    w: &[EdgeGeom],
    pl: &Plane,
    d: f64,
    sharp: bool,
) -> CqResult<Vec<Vec<EdgeGeom>>> {
    // A whole circle's offset is the circle of radius r + d (outwards: away from its centre).
    if let [g] = w
        && let Curve::Circle { c, x, y, r } = &g.curve
        && g.t1 - g.t0 >= std::f64::consts::TAU - 1e-9
    {
        let r2 = r + d;
        if r2 <= 0.0 {
            return Ok(vec![]);
        }
        return Ok(vec![vec![EdgeGeom {
            curve: Curve::Circle {
                c: *c,
                x: *x,
                y: *y,
                r: r2,
            },
            t0: g.t0,
            t1: g.t1,
        }]]);
    }
    let to_local = pl.to_local_map();
    let to_world = pl.to_world_map();
    let local: Vec<EdgeGeom> = w.iter().map(|g| g.transformed(&to_local)).collect();
    if sharp && local.iter().all(|g| matches!(g.curve, Curve::Line { .. })) && local.len() >= 3 {
        // Each side moved out by d, neighbours meeting where their lines cross.
        let ccw = make::signed_area(&local, &Plane::named("XY", V3::zeros())?) >= 0.0;
        let out_n = |g: &EdgeGeom| {
            let t = (g.end() - g.start()).normalize();
            let n = V3::new(t.y, -t.x, 0.0);
            if ccw { n } else { -n }
        };
        let lines: Vec<(V3, V3)> = local
            .iter()
            .map(|g| (g.start() + out_n(g) * d, (g.end() - g.start()).normalize()))
            .collect();
        let k = lines.len();
        let mut pts = Vec::with_capacity(k + 1);
        for i in 0..k {
            let (p0, d0) = lines[(i + k - 1) % k];
            let (p1, d1) = lines[i];
            let den = d0.x * d1.y - d0.y * d1.x;
            if den.abs() < 1e-12 {
                pts.push(p1);
            } else {
                let q = p1 - p0;
                let s = (q.x * d1.y - q.y * d1.x) / den;
                pts.push(p0 + d0 * s);
            }
        }
        pts.push(pts[0]);
        let edges = make::polygon(&pts, false)?;
        return Ok(vec![
            edges
                .into_iter()
                .map(|g| g.transformed(&to_world))
                .collect(),
        ]);
    }
    let curves: Vec<(Curve, f64, f64)> = local
        .iter()
        .map(|g| (g.curve.clone(), g.t0, g.t1))
        .collect();
    let loops = crate::native::offset::wire::offset_wire(&curves, d)?;
    Ok(loops
        .into_iter()
        .map(|l| {
            l.into_iter()
                .map(|p| {
                    EdgeGeom {
                        curve: p.c,
                        t0: p.t0,
                        t1: p.t1,
                    }
                    .transformed(&to_world)
                })
                .collect()
        })
        .collect())
}

/// [`offset_wires`]' first wire.
pub fn offset_wire(w: &[EdgeGeom], pl: &Plane, d: f64, sharp: bool) -> CqResult<Vec<EdgeGeom>> {
    offset_wires(w, pl, d, sharp)?
        .into_iter()
        .next()
        .ok_or_else(|| CqError::Value("the offset vanished".into()))
}

/// `Solid.revolve(face, angleDegrees, axisStart, axisEnd)`.
pub fn revolve(faces: &[Shape], deg: f64, a: V3, b: V3) -> CqResult<Shape> {
    if (b - a).norm() == 0.0 {
        return value_err!("gp_Dir() - input vector has zero norm");
    }
    let axis = Axis {
        origin: P3::from(a),
        dir: Unit::new_normalize(b - a),
    };
    let full = deg >= 360.0 - 1e-9;
    let mut out = Vec::new();
    for (plane, inputs, _keep) in by_plane(faces)? {
        // About the faces' own normal a face only turns within its plane: it sweeps no volume
        // (OCCT makes a flat, empty solid of it).
        if axis.dir.cross(&plane.normal).norm() < 1e-9 {
            out.push(Shape::compound(vec![]));
            continue;
        }
        let profile = Profile::new(plane, vec![]);
        let mut spec =
            crate::RevolveSpec::solid(axis, if full { None } else { Some(deg.to_radians()) });
        spec.full = full;
        spec.faces = inputs;
        out.push(Shape::of_body(op1(|k| k.revolve_with(&profile, &spec))?));
    }
    Ok(together(out))
}

/// An edge as a sweep's path curve.
pub fn path_curve(g: &EdgeGeom) -> PathCurve {
    use crate::Curve2;
    let p2 = nalgebra::Point2::new;
    match &g.curve {
        Curve::Line { o, d } => {
            let a = o + d * g.t0;
            let b = o + d * g.t1;
            let dir = (b - a).normalize();
            let n = super::geom::default_x(&dir);
            let plane = crate::Plane {
                origin: a,
                x_dir: Unit::new_normalize(dir),
                normal: Unit::new_normalize(n),
            };
            PathCurve::Sketch {
                plane,
                curve: Curve2::Line {
                    a: p2(0.0, 0.0),
                    b: p2((b - a).norm(), 0.0),
                    source: None,
                },
            }
        }
        Curve::Circle { c, x, y, r } => {
            let plane = crate::Plane {
                origin: *c,
                x_dir: Unit::new_normalize(*x),
                normal: Unit::new_normalize(x.cross(y)),
            };
            PathCurve::Sketch {
                plane,
                curve: Curve2::Arc {
                    center: p2(0.0, 0.0),
                    radius: *r,
                    start_angle: g.t0,
                    sweep: g.t1 - g.t0,
                    source: None,
                },
            }
        }
        Curve::Ellipse { c, x, y, a, b } => {
            let plane = crate::Plane {
                origin: *c,
                x_dir: Unit::new_normalize(*x),
                normal: Unit::new_normalize(x.cross(y)),
            };
            PathCurve::Sketch {
                plane,
                curve: Curve2::EllipseArc {
                    center: p2(0.0, 0.0),
                    major_radius: *a,
                    minor_radius: *b,
                    rotation: 0.0,
                    start: g.t0,
                    sweep: g.t1 - g.t0,
                    source: None,
                },
            }
        }
        Curve::Nurbs(n) => {
            let (a, b) = n.range();
            let piece = if (g.t0 - a).abs() > 1e-12 || (g.t1 - b).abs() > 1e-12 {
                n.piece(g.t0, g.t1)
            } else {
                n.as_ref().clone()
            };
            PathCurve::Bspline(piece.to_bspline())
        }
    }
}

/// The conical spiral points lie on within `tol` (radius and height both linear in the angle
/// about an axis): its axis first guessed from the tangents (square to their differences), then
/// every parameter (the axis's direction and place, the radius at the start and its growth a
/// radian, the rise a radian) refined by Gauss–Newton over the points, and the result checked
/// at every point.
fn spiral_through(pts: &[P3], tans: &[V3], tol: f64) -> Option<crate::Helix> {
    use nalgebra::{DMatrix, DVector, SymmetricEigen};
    let n = pts.len();
    if n < 16 {
        return None;
    }
    let mean = tans.iter().sum::<V3>() / n as f64;
    let mut m = nalgebra::Matrix3::zeros();
    for t in tans {
        let d = t - mean;
        m += d * d.transpose();
    }
    let e = SymmetricEigen::new(m);
    let (imin, _) = e
        .eigenvalues
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.total_cmp(b.1))?;
    let mut a0 = e.eigenvectors.column(imin).into_owned().normalize();
    if mean.dot(&a0) < 0.0 {
        a0 = -a0;
    }
    let ex = super::geom::default_x(&a0);
    let ey = a0.cross(&ex);
    // The centre: the points' mean across the axis (whole turns balance about it).
    let c0 = pts.iter().map(|p| p.coords).sum::<V3>() / n as f64;
    let c0 = c0 - a0 * c0.dot(&a0);
    // Parameters: tilt of the axis (two), its point across it (two), r0, k, rise c, z0.
    let model = |q: &[f64]| -> Option<(V3, V3, V3, V3)> {
        let a = (a0 + ex * q[0] + ey * q[1]).normalize();
        let x = (ex - a * ex.dot(&a)).normalize();
        let y = a.cross(&x);
        let c = c0 + ex * q[2] + ey * q[3];
        Some((a, x, y, c))
    };
    let angles = |a: V3, x: V3, y: V3, c: V3| -> Vec<f64> {
        let mut out = Vec::with_capacity(n);
        let mut prev: Option<f64> = None;
        for p in pts {
            let w = p.coords - c;
            let mut t = w.dot(&y).atan2(w.dot(&x));
            if let Some(pv) = prev {
                t += ((pv - t) / std::f64::consts::TAU).round() * std::f64::consts::TAU;
            }
            out.push(t);
            prev = Some(t);
        }
        let _ = a;
        out
    };
    // Residuals at q: each point against the spiral at its own angle.
    let resid = |q: &[f64], th: &[f64]| -> Option<DVector<f64>> {
        let (a, x, y, c) = model(q)?;
        let mut r = DVector::zeros(3 * n);
        for (i, p) in pts.iter().enumerate() {
            let t = th[i] - th[0];
            let rho = q[4] + q[5] * t;
            let want = c + a * (q[7] + q[6] * t) + (x * th[i].cos() + y * th[i].sin()) * rho;
            let d = p.coords - want;
            r[3 * i] = d.x;
            r[3 * i + 1] = d.y;
            r[3 * i + 2] = d.z;
        }
        Some(r)
    };
    // A start: radius and height linear in the angle about the guessed axis.
    let (a, x, y, c) = model(&[0.0; 8])?;
    let th = angles(a, x, y, c);
    let span = th[n - 1] - th[0];
    if span.abs() < 1.0 {
        return None;
    }
    let rho0 = {
        let w = pts[0].coords - c;
        (w - a * w.dot(&a)).norm()
    };
    let rho1 = {
        let w = pts[n - 1].coords - c;
        (w - a * w.dot(&a)).norm()
    };
    let z0 = (pts[0].coords - c).dot(&a);
    let z1 = (pts[n - 1].coords - c).dot(&a);
    let mut q = vec![
        0.0,
        0.0,
        0.0,
        0.0,
        rho0,
        (rho1 - rho0) / span,
        (z1 - z0) / span,
        z0,
    ];
    for _ in 0..30 {
        let (a, x, y, c) = model(&q)?;
        let th = angles(a, x, y, c);
        let r = resid(&q, &th)?;
        let mut jac = DMatrix::zeros(3 * n, 8);
        for j in 0..8 {
            let h = 1e-7 * (1.0 + q[j].abs());
            let mut qp = q.clone();
            qp[j] += h;
            let rp = resid(&qp, &th)?;
            jac.set_column(j, &((rp - &r) / h));
        }
        let step = (jac.transpose() * &jac)
            .lu()
            .solve(&(-(jac.transpose() * &r)))?;
        for j in 0..8 {
            q[j] += step[j];
        }
        if step.norm() < 1e-14 * (1.0 + q.iter().map(|v| v.abs()).fold(0.0, f64::max)) {
            break;
        }
    }
    let (a, x, y, c) = model(&q)?;
    let th = angles(a, x, y, c);
    let r = resid(&q, &th)?;
    if (0..n).any(|i| V3::new(r[3 * i], r[3 * i + 1], r[3 * i + 2]).norm() > tol) {
        return None;
    }
    let span = th[n - 1] - th[0];
    let rise = q[6] * span;
    if rise.abs() <= tol {
        return None;
    }
    let up = rise > 0.0;
    let dir = if up { a } else { -a };
    let start_out = x * th[0].cos() + y * th[0].sin();
    Some(crate::Helix {
        axis: crate::Axis {
            origin: P3::from(c + a * q[7]),
            dir: Unit::new_normalize(dir),
        },
        x: Unit::new_normalize(start_out),
        r0: q[4],
        r1: q[4] + q[5] * span,
        height: rise.abs(),
        turns: span / std::f64::consts::TAU * if up { 1.0 } else { -1.0 },
        start_angle: 0.0,
    })
}

/// How a sweep holds its section as it goes (`Solid.sweep`'s `isFrenet` and `mode`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SweepMode {
    /// OCCT's corrected Frenet frame: the least twist.
    Corrected,
    /// The Frenet frame.
    Frenet,
    /// The section kept parallel to itself (a fixed binormal `mode` vector).
    Fixed(V3),
}

/// `Solid.sweep(face, path, makeSolid, isFrenet, mode, transition)`.
pub fn sweep(
    faces: &[Shape],
    path: &[EdgeGeom],
    solid: bool,
    mode: SweepMode,
    round: bool,
) -> CqResult<Shape> {
    if path.is_empty() {
        return value_err!("a sweep needs a path");
    }
    // A helical path (sampled or fitted) swept along its exact helix: screw surfaces, whose
    // intersections are closed forms.
    let helix = helix_of(path);
    let curves: Vec<PathCurve> = match &helix {
        Some(h) => h.path(),
        None => path.iter().map(path_curve).collect(),
    };
    let control = match mode {
        SweepMode::Fixed(_) => crate::SweepControl::KeepOrientation,
        SweepMode::Frenet => match helix {
            Some(h) => crate::SweepControl::Helix(h),
            None => crate::SweepControl::None,
        },
        SweepMode::Corrected => crate::SweepControl::None,
    };
    let mut out = Vec::new();
    for (plane, inputs, _keep) in by_plane(faces)? {
        let profile = Profile::new(plane, vec![]);
        let spec = crate::SweepSpec {
            body: if solid {
                crate::BodyKind::Solid
            } else {
                crate::BodyKind::Surface
            },
            path: curves.clone(),
            control: control.clone(),
            faces: inputs,
            corners: if round {
                crate::SweepCorners::Round
            } else {
                crate::SweepCorners::Mitre
            },
        };
        out.push(Shape::of_body(op1(|k| k.sweep_with(&profile, &spec))?));
    }
    Ok(together(out))
}

/// A sweep whose profile twists by `twist` radians (right-handed about the direction of
/// travel) along a straight or smooth path (`Solid.extrudeLinearWithRotation`).
pub fn sweep_twisted(faces: &[Shape], path: &[EdgeGeom], twist: f64) -> CqResult<Shape> {
    let curves: Vec<PathCurve> = path.iter().map(path_curve).collect();
    let law = crate::SweepLaw {
        twist,
        scale: 1.0,
        spiral: false,
    };
    let mut out = Vec::new();
    for (plane, inputs, _keep) in by_plane(faces)? {
        let profile = Profile::new(plane, vec![]);
        let spec = crate::SweepSpec {
            body: crate::BodyKind::Solid,
            path: curves.clone(),
            control: crate::SweepControl::None,
            faces: inputs,
            corners: crate::SweepCorners::Mitre,
        };
        out.push(Shape::of_body(op1(|k| {
            k.sweep_with_law(&profile, &spec, &law)
        })?));
    }
    Ok(together(out))
}

/// The helix (or conical spiral) a path runs along, when it is one: a `parametricCurve` or
/// spline through a helix's points, an approximation of it. Checked, not assumed: a dense
/// sampling of the path (at least 512 points) must lie within 1e-6·(1 + size) of the fitted
/// helix, under what a sampled helix's fit strays by and far over what a curve that merely
/// resembles one keeps to.
pub fn helix_of(path: &[EdgeGeom]) -> Option<crate::Helix> {
    let len: f64 = path.iter().map(|g| g.length()).sum();
    if len <= 0.0 {
        return None;
    }
    let n = 512.0;
    let mut pts = Vec::new();
    let mut tans = Vec::new();
    for g in path {
        let k = (n * g.length() / len).ceil().max(8.0) as usize;
        for i in 0..=k {
            let t = g.t0 + (g.t1 - g.t0) * i as f64 / k as f64;
            pts.push(g.curve.point(t));
            tans.push(g.tangent(t));
        }
    }
    let size = pts.iter().map(|p| (p - pts[0]).norm()).fold(0.0, f64::max);
    let tol = 1e-6 * (1.0 + size);
    let h = crate::native::sweep::frames::helix_through(&pts, &tans, tol)
        .or_else(|| spiral_through(&pts, &tans, tol))?;
    // The helix runs where the path does: from its start to its end.
    let (a, _) = h.at(0.0);
    let (b, _) = h.at(1.0);
    let tol = 1e-5 * (1.0 + size);
    if (a.coords - path[0].start()).norm() > tol
        || (b.coords - path[path.len() - 1].end()).norm() > tol
    {
        return None;
    }
    Some(h)
}

/// A loft's section.
#[derive(Debug, Clone)]
pub enum LoftItem {
    /// A closed wire.
    Wire(Vec<EdgeGeom>),
    /// A point (an end of the loft).
    Point(V3),
}

/// `Solid.makeLoft(wires, ruled)` / `loft(...)`.
pub fn loft(items: &[LoftItem], ruled: bool) -> CqResult<Shape> {
    if items.len() < 2 {
        return value_err!("More than one wire is required");
    }
    if ruled && items.len() > 2 {
        // Ruled: straight between each pair of sections, the pieces united.
        let pieces: Vec<Shape> = items
            .windows(2)
            .map(|w| loft(w, true))
            .collect::<CqResult<_>>()?;
        return fuse(&pieces[0], &pieces[1..]);
    }
    let mut keep = Vec::new();
    let mut sections = Vec::new();
    for it in items {
        match it {
            LoftItem::Point(p) => sections.push(crate::LoftSection::Point(P3::from(*p))),
            LoftItem::Wire(w) => {
                let faces = make::faces_from_wires(&[w.clone()], None)?;
                let f = &faces[0];
                let (b, fi, _) = face_input(f)?;
                keep.push(b);
                sections.push(crate::LoftSection::Face(fi));
            }
        }
    }
    let spec = crate::LoftSpec {
        body: crate::BodyKind::Solid,
        sections,
        start: Default::default(),
        end: Default::default(),
        source: 0,
    };
    Ok(Shape::of_body(op1(|k| k.loft_with(&spec))?))
}

// ------------------------------------------------------------------ edges and faces of a solid

/// The index in `body` of each edge (taken from it, or lying where one of its edges lies).
fn edge_ids(body: &BodyRef, edges: &[Shape]) -> CqResult<Vec<crate::EdgeId>> {
    let b = brep(body);
    let mut out = Vec::new();
    for e in edges {
        if let Some((from, i)) = e.edge_from()
            && from.0 == body.0
        {
            out.push(crate::EdgeId(i as u64));
            continue;
        }
        let gs = e.edge_geoms();
        let Some(g) = gs.first() else { continue };
        let (s, t, m) = (g.start(), g.end(), g.point(0.5 * (g.t0 + g.t1)));
        let tol = 1e-6 * (1.0 + m.norm());
        let found = b.edges.iter().enumerate().find(|(_, be)| {
            if be.degenerate {
                return false;
            }
            let (bs, bt) = (
                be.curve.point(be.t[0]).coords,
                be.curve.point(be.t[1]).coords,
            );
            let ends = ((bs - s).norm() < tol && (bt - t).norm() < tol)
                || ((bs - t).norm() < tol && (bt - s).norm() < tol);
            ends && {
                let q = be.curve.closest(&P3::from(m), be.t[0], be.t[1]);
                (be.curve.point(q).coords - m).norm() < tol
            }
        });
        match found {
            Some((i, _)) => out.push(crate::EdgeId(i as u64)),
            None => return value_err!("an edge to fillet or chamfer is not an edge of the solid"),
        }
    }
    // Seams (an edge with its face on both sides) aren't blended: OCCT passes over them.
    let ef = b.edge_faces();
    out.retain(|e| {
        ef.get(e.0 as usize)
            .is_none_or(|f| !(f.len() == 2 && f[0] == f[1]))
    });
    out.sort_by_key(|e| e.0);
    out.dedup();
    Ok(out)
}

/// The index in `body` of each face (taken from it, or lying where one of its faces lies).
fn face_ids(body: &BodyRef, faces: &[Shape]) -> CqResult<Vec<FaceId>> {
    let b = brep(body);
    let mut out = Vec::new();
    for f in faces {
        if let Repr::Part {
            body: fb,
            part: Part::Face(i),
            ..
        } = &f.repr
            && fb.0 == body.0
        {
            out.push(FaceId(*i as u64));
            continue;
        }
        let c = f.center();
        let a = f.area()?;
        let tol = 1e-6 * (1.0 + c.norm() + a.sqrt());
        let found = (0..b.faces.len()).find(|&i| {
            let (fa, fc) = crate::native::mass::face_area(&b, i);
            (fa - a).abs() < tol * (1.0 + a.sqrt()) && (fc.coords - c).norm() < tol
        });
        match found {
            Some(i) => out.push(FaceId(i as u64)),
            None => return value_err!("a face is not a face of the solid"),
        }
    }
    Ok(out)
}

/// `fillet(radius, edges)`.
pub fn fillet(s: &Shape, r: f64, edges: &[Shape]) -> CqResult<Shape> {
    let body = s.body()?;
    let ids = edge_ids(&body, edges)?;
    if ids.is_empty() {
        return value_err!("Fillets requires that edges be selected");
    }
    Ok(Shape::of_body(op1(|k| k.fillet(body.0, &ids, r))?))
}

/// `chamfer(length, length2, edges)`.
pub fn chamfer(s: &Shape, d1: f64, d2: Option<f64>, edges: &[Shape]) -> CqResult<Shape> {
    let body = s.body()?;
    let ids = edge_ids(&body, edges)?;
    if ids.is_empty() {
        return value_err!("Chamfer requires that edges be selected");
    }
    let spec = match d2 {
        Some(d2) if d2 != d1 => crate::ChamferSpec::TwoDistances(d1, d2),
        _ => crate::ChamferSpec::EqualDistance(d1),
    };
    // OCCT's chamfer: the first distance on the edge's first face in the solid's order, and a
    // corner where three chamfers meet cut flat.
    let mut opts = crate::ChamferOpts::new(spec);
    opts.corners = crate::ChamferCorners::Flat;
    if d2.is_some_and(|d2| d2 != d1) {
        let b = brep(&body);
        let ef = b.edge_faces();
        let mut first: Vec<FaceId> = ids
            .iter()
            .filter_map(|e| ef.get(e.0 as usize).and_then(|f| f.iter().min().copied()))
            .map(|f| FaceId(f as u64))
            .collect();
        first.sort_by_key(|f| f.0);
        first.dedup();
        opts.first_on = first;
    }
    Ok(Shape::of_body(op1(|k| {
        k.chamfer_with(body.0, &ids, &opts)
    })?))
}

/// `hollow(faces, thickness, kind)`: a shell of the solid, open at `faces` (a closed void when
/// there are none); outward when `t` is positive.
pub fn hollow(s: &Shape, faces: &[Shape], t: f64, sharp: bool) -> CqResult<Shape> {
    let body = s.body()?;
    let ids = face_ids(&body, faces)?;
    let spec = crate::ShellSpec {
        remove: ids.clone(),
        thickness: t.abs(),
        outward: t > 0.0,
        hollow: ids.is_empty(),
        sharp,
        nothing_hollowed: Default::default(),
    };
    Ok(Shape::of_body(op1(|k| k.shell_with(body.0, &spec))?))
}

/// `isInside(point, tolerance)`: inside the solid or on its boundary.
pub fn is_inside(s: &Shape, p: V3, tol: f64) -> CqResult<bool> {
    let body = s.body()?;
    let c = session(|k| k.classify(body.0, P3::from(p), tol))?;
    Ok(!matches!(c, PointClass::Outside))
}

/// The prism of faces through all of a solid both ways along `n` (`cutThruAll`'s tool).
pub fn through_prism(solid: &Shape, faces: &[Shape], n: V3) -> CqResult<Shape> {
    let bb = solid.bounding_box()?;
    let mut out = Vec::new();
    for f in faces {
        let fb = f.bounding_box()?;
        let reach = bb.union(&fb).diagonal() * 2.0 + 1.0;
        let back = f.translate(-n.normalize() * reach)?;
        out.push(extrude(&[back], n.normalize() * 2.0 * reach, 0.0)?);
    }
    Ok(together(out))
}

/// The box enclosing shapes.
pub fn bounds(v: &[Shape]) -> CqResult<BoundBox> {
    let mut out: Option<BoundBox> = None;
    for s in v {
        let b = s.bounding_box()?;
        out = Some(match out {
            Some(o) => o.union(&b),
            None => b,
        });
    }
    out.ok_or_else(|| CqError::Value("nothing to bound".into()))
}

/// Keeps `Arc` in use (the faces' bodies stay alive through an operation).
#[allow(dead_code)]
fn hold<T>(_: &[Arc<T>]) {}

// ------------------------------------------------------------------ sewing and export

/// `Shell.makeShell(faces)`: faces sewn at their edges.
pub fn sew(faces: &[Shape]) -> CqResult<Shape> {
    let bodies: Vec<BodyRef> = faces.iter().map(|f| f.body()).collect::<CqResult<_>>()?;
    let ids: Vec<crate::BodyId> = bodies.iter().map(|b| b.0).collect();
    Ok(Shape::of_body(op1(|k| k.sew_sheets(&ids, 1e-6))?))
}

/// `Solid.makeSolid(shell)`: the solid a closed shell bounds.
pub fn solid_of_shell(s: &Shape) -> CqResult<Shape> {
    let b = s.body()?;
    Ok(Shape::of_body(op1(|k| k.sew_solid(&[b.0], 1e-6))?))
}

/// The shape as a STEP file.
pub fn export_step(s: &Shape) -> CqResult<Vec<u8>> {
    let b = s.body()?;
    Ok(session(|k| k.export_step(&[b.0]))?)
}

/// The shape as a binary STL file (meshed within 0.01 mm).
pub fn export_stl(s: &Shape) -> CqResult<Vec<u8>> {
    let b = s.body()?;
    let m = session(|k| k.tessellate(b.0, crate::Tessellation::default()))?;
    let mut out = vec![0u8; 80];
    out.extend_from_slice(&(m.indices.len() as u32).to_le_bytes());
    for t in &m.indices {
        let p: Vec<P3> = t.iter().map(|&i| m.positions[i as usize]).collect();
        let n = (p[1] - p[0]).cross(&(p[2] - p[0]));
        let n = if n.norm() > 0.0 { n.normalize() } else { n };
        for v in std::iter::once(n.cast::<f32>()).chain(p.iter().map(|q| q.coords.cast::<f32>())) {
            for c in v.iter() {
                out.extend_from_slice(&c.to_le_bytes());
            }
        }
        out.extend_from_slice(&[0, 0]);
    }
    Ok(out)
}
