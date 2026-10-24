//! Delete face's heal where the deleted faces aren't closed off by whole inner loops (#22: a
//! 3D-printing teardrop over a bore, a fillet, a chamfer, a notch running into a pocket): the
//! faces round them are extended over the gap until they meet, as OCCT's defeaturing does.
//!
//! Each connected group of deleted faces is healed with booleans. For a cut (the teardrop's
//! notch), the empty space in a box round the group (the box less the body) is split into cells
//! by the extended surfaces of the faces next to the group (its neighbours); the cells just off
//! the deleted faces are the gap, and they are added to the body. For a protrusion (a rib, a
//! fillet in an inside corner) the material in the box is split the same way and the cells just
//! inside the deleted faces are taken away. Booleans that merge faces on one surface then make
//! the extended neighbours whole. Splitting into cells keeps each neighbour local: a pocket's
//! wall that only touches the end of a teardrop bounds the gap there and nowhere else.
//!
//! The heal is right when the result has no face of its own: each face continues a face of
//! the body (only extended surfaces bound the gap), none is a deleted face's, and it is one
//! solid. A box too small leaves its own faces in the gap; then the next, larger box is tried,
//! and a cut that doesn't heal is tried as a protrusion. Neighbours other than planes and
//! cylinders are refused for now.

use super::kernel::NativeKernel;
use super::math::*;
use super::surface::Surface;
use super::tol;
use super::topo::Body;
use crate::*;

/// Box margins tried, as fractions of the group's size: just past its own bounding box first
/// (enough for a teardrop or a right-angled fillet, and well clear of the booleans' tolerance),
/// then larger for gaps that reach past it.
const MARGINS: [f64; 4] = [1e-3, 0.1, 0.5, 1.5];

/// A neighbour's surface, as the heal splits space by it.
#[derive(Debug, Clone, Copy)]
enum Side {
    /// The plane through `o` with the face's outward normal `n`.
    Plane { o: P3, n: V3, x: V3 },
    /// A cylinder about `o` + t·`z`, radius `r`.
    Cylinder { o: P3, z: V3, x: V3, r: f64 },
}

impl Side {
    fn of(b: &Body, f: usize) -> Result<Side> {
        let face = &b.faces[f];
        let sign = if face.sense { 1.0 } else { -1.0 };
        match &face.surf {
            Surface::Plane { f: fr } => Ok(Side::Plane {
                o: fr.o,
                n: fr.x.cross(&fr.y).normalize() * sign,
                x: fr.x,
            }),
            Surface::Cylinder { f: fr, r } => Ok(Side::Cylinder {
                o: fr.o,
                z: fr.z.normalize(),
                x: fr.x,
                r: *r,
            }),
            _ => Err(KernelError::Unsupported(
                "healing deleted faces next to faces other than planes and cylinders",
            )),
        }
    }

    /// Whether `other` splits space the same way (a STEP file's face cut in two: one surface).
    fn same(&self, other: &Side, tol: f64) -> bool {
        let parallel = |a: &V3, b: &V3| a.cross(b).norm() <= tol::ANGULAR_POINT;
        match (self, other) {
            (Side::Plane { o, n, .. }, Side::Plane { o: p, n: m, .. }) => {
                parallel(n, m) && (p - o).dot(n).abs() <= tol
            }
            (
                Side::Cylinder { o, z, r, .. },
                Side::Cylinder {
                    o: p, z: w, r: q, ..
                },
            ) => {
                let d = p - o;
                parallel(z, w) && (d - z * d.dot(z)).norm() <= tol && (r - q).abs() <= tol
            }
            _ => false,
        }
    }

    /// Whether `p` is on the material side of this neighbour (as the face is oriented).
    fn material(&self, b: &Body, f: usize, p: &P3) -> bool {
        match *self {
            Side::Plane { o, n, .. } => (p - o).dot(&n) < 0.0,
            // The natural normal points away from the axis: a face facing that way is a boss's
            // side (material inside).
            Side::Cylinder { o, z, r, .. } => {
                let d = p - o;
                let inside = (d - z * d.dot(&z)).norm() < r;
                inside == b.faces[f].sense
            }
        }
    }
}

/// Delete face's heal by extending the neighbours (see the module docs). `gone` are the faces
/// to delete (indices into the body's faces).
pub fn heal(k: &mut NativeKernel, body: BodyId, gone: &[usize]) -> Result<OpResult> {
    let original = (**k.get(body)?).clone();
    // Which original faces each working face continues.
    let mut sources: Vec<Vec<usize>> = (0..original.faces.len()).map(|f| vec![f]).collect();
    let mut cur = original.clone();
    for group in groups(&original, gone) {
        // The group's faces now: every face continuing one of them (an earlier group's heal can
        // split one). One merged with a kept face can't be deleted alone.
        let now: Vec<usize> = (0..cur.faces.len())
            .filter(|&g| sources[g].iter().any(|s| group.contains(s)))
            .collect();
        if now
            .iter()
            .any(|&g| sources[g].iter().any(|s| !gone.contains(s)))
        {
            return Err(refused("a face to delete was merged with one to keep"));
        }
        if now.is_empty() {
            continue;
        }
        let (healed, from) = heal_group(k, &cur, &now)?;
        let mut next_sources = vec![Vec::new(); healed.faces.len()];
        for (g, srcs) in from.iter().enumerate() {
            for &(_, f) in srcs.iter().filter(|(o, _)| *o == 0) {
                for &s in &sources[f] {
                    if !next_sources[g].contains(&s) {
                        next_sources[g].push(s);
                    }
                }
            }
        }
        sources = next_sources;
        cur = healed;
    }
    // No piece of a deleted face is left.
    if sources.iter().flatten().any(|s| gone.contains(s)) {
        return Err(refused("part of a face to delete is left"));
    }
    let mut history = History::default();
    for (g, srcs) in sources.iter().enumerate() {
        for &s in srcs {
            history.modified.push((
                FaceId(g as u64),
                InputFace {
                    body,
                    face: FaceId(s as u64),
                },
            ));
        }
    }
    for &f in gone {
        history.deleted.push(InputFace {
            body,
            face: FaceId(f as u64),
        });
    }
    let id = k.insert(cur);
    Ok(OpResult {
        bodies: vec![id],
        history,
    })
}

/// A refusal, saying why.
fn refused(why: &str) -> KernelError {
    KernelError::InvalidResult {
        op: "delete face",
        why: format!("the faces can't be deleted: {why}"),
    }
}

/// The deleted faces in groups joined by their edges.
fn groups(b: &Body, gone: &[usize]) -> Vec<Vec<usize>> {
    let ef = b.edge_faces();
    let mut out: Vec<Vec<usize>> = Vec::new();
    let mut seen: Vec<usize> = Vec::new();
    for &start in gone {
        if seen.contains(&start) {
            continue;
        }
        let mut group = vec![start];
        seen.push(start);
        let mut i = 0;
        while i < group.len() {
            let f = group[i];
            for fs in ef.iter().filter(|fs| fs.contains(&f)) {
                for &g in fs {
                    if gone.contains(&g) && !seen.contains(&g) {
                        seen.push(g);
                        group.push(g);
                    }
                }
            }
            i += 1;
        }
        out.push(group);
    }
    out
}

/// A healed body and, for each of its faces, its sources (owner 0 = the body before).
type Healed = (Body, Vec<Vec<(usize, usize)>>);

/// One group healed: the body and, for each of its faces, its sources (owner 0 = `b`).
fn heal_group(k: &mut NativeKernel, b: &Body, group: &[usize]) -> Result<Healed> {
    let ef = b.edge_faces();
    let mut neighbours: Vec<usize> = Vec::new();
    for fs in ef.iter().filter(|fs| fs.iter().any(|f| group.contains(f))) {
        for &f in fs {
            if !group.contains(&f) && !neighbours.contains(&f) {
                neighbours.push(f);
            }
        }
    }
    // The group's faces' mesh: its extent, and a point just off its largest triangle.
    let id = k.insert(super::kernel::subset(b, group, false));
    let mesh = k.tessellate(id, Tessellation::default());
    k.release(id);
    let mesh = mesh?;
    let (mut lo, mut hi) = (
        P3::new(f64::MAX, f64::MAX, f64::MAX),
        P3::new(f64::MIN, f64::MIN, f64::MIN),
    );
    for p in &mesh.positions {
        lo = lo.inf(p);
        hi = hi.sup(p);
    }
    let probe = mesh
        .indices
        .iter()
        .map(|t| t.map(|i| mesh.positions[i as usize]))
        .map(|p| ((p[1] - p[0]).cross(&(p[2] - p[0])), p))
        .max_by(|a, b| a.0.norm().total_cmp(&b.0.norm()));
    let Some((n, tri)) = probe.filter(|(n, _)| n.norm() > 0.0) else {
        return Err(KernelError::InvalidParameter(
            "the faces to delete have no area".into(),
        ));
    };
    let size = (hi - lo).norm().max(tol::POINT);
    let mut sides: Vec<(usize, Side)> = Vec::new();
    for f in neighbours {
        let s = Side::of(b, f)?;
        if !sides.iter().any(|(_, t)| t.same(&s, 1e-6 * size.max(1.0))) {
            sides.push((f, s));
        }
    }
    // A cut if just off the faces (in the gap) is on the material side of most neighbours,
    // else a protrusion; the other is tried after.
    let off = P3::from((tri[0].coords + tri[1].coords + tri[2].coords) / 3.0)
        + n.normalize() * (size * 1e-3);
    let material = sides
        .iter()
        .filter(|(f, s)| s.material(b, *f, &off))
        .count();
    let first_cut = 2 * material >= sides.len();
    let sides: Vec<Side> = sides.into_iter().map(|(_, s)| s).collect();
    let mut first = None;
    for cut in [first_cut, !first_cut] {
        for m in MARGINS {
            let pad = V3::repeat(m * size + 10.0 * tol::POINT);
            match try_heal(k, b, group, &sides, cut, lo - pad, hi + pad, size) {
                Ok(r) => return Ok(r),
                Err(e) => {
                    first.get_or_insert(e);
                }
            }
        }
    }
    Err(first.unwrap_or_else(|| refused("the healed body is not a valid solid")))
}

/// A cell of the space round the gap: a solid, and which of its faces lie on a deleted face.
struct Cell {
    body: Body,
    on_gone: Vec<bool>,
}

/// The heal with the box `lo`–`hi`, checked (see the module docs).
#[allow(clippy::too_many_arguments)]
fn try_heal(
    k: &mut NativeKernel,
    b: &Body,
    group: &[usize],
    sides: &[Side],
    cut: bool,
    lo: P3,
    hi: P3,
    size: f64,
) -> Result<Healed> {
    let far = 4.0 * ((hi - lo).norm() + size);
    let centre = P3::from((lo.coords + hi.coords) / 2.0);
    let bx = Cell {
        on_gone: Vec::new(),
        body: solid(k, &box_profile(lo, hi), Extent::Blind(hi.z - lo.z))?,
    };
    // The empty space in the box (a cut) or the material (a protrusion), in cells split by the
    // neighbours' surfaces; the deleted faces are on the gap's cells.
    let op = if cut {
        BoolOp::Subtract
    } else {
        BoolOp::Intersect
    };
    let mut cells = split(&bx, b, op, |o, f| o == 1 && group.contains(&f))?;
    for s in sides {
        let piece = match *s {
            Side::Plane { o, n, x } => {
                let at = centre - n * (centre - o).dot(&n);
                solid(k, &square(at, x, -n, far), Extent::Blind(far))?
            }
            Side::Cylinder { o, z, x, r } => {
                let at = o + z * (centre - o).dot(&z);
                solid(k, &disc(at, x, z, r), Extent::Symmetric(2.0 * far))?
            }
        };
        let mut next = Vec::new();
        for c in &cells {
            for op in [BoolOp::Intersect, BoolOp::Subtract] {
                next.extend(split(c, &piece, op, |o, f| o == 0 && c.on_gone[f])?);
            }
        }
        cells = next;
    }
    let mut gap: Option<Body> = None;
    for c in cells.into_iter().filter(|c| c.on_gone.contains(&true)) {
        gap = Some(match gap {
            None => c.body,
            Some(g) => super::boolean::boolean_bodies(&g, &c.body, BoolOp::Union)?.body,
        });
    }
    let gap = gap.ok_or_else(|| refused("no gap next to them"))?;
    let op = if cut { BoolOp::Union } else { BoolOp::Subtract };
    let r = super::boolean::boolean_bodies_flush(b, &gap, op)?;
    let healed = r.from.iter().all(|srcs| {
        srcs.iter().any(|(o, _)| *o == 0) && !srcs.iter().any(|(o, f)| *o == 0 && group.contains(f))
    });
    if !healed || r.body.solids.len() != 1 {
        return Err(refused("the faces round them don't close the gap"));
    }
    Ok((r.body, r.from))
}

/// The solids of `cell op tool`, each a cell of its own (none for an empty result), a face on a
/// deleted face where `gone(owner, face)` holds for one of its sources.
fn split(
    cell: &Cell,
    tool: &Body,
    op: BoolOp,
    gone: impl Fn(usize, usize) -> bool,
) -> Result<Vec<Cell>> {
    let r = match super::boolean::boolean_bodies(&cell.body, tool, op) {
        Ok(r) => r,
        Err(KernelError::EmptyResult(_)) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    if r.body.faces.is_empty() {
        return Ok(Vec::new());
    }
    let on_gone: Vec<bool> = r
        .from
        .iter()
        .map(|srcs| srcs.iter().any(|&(o, f)| gone(o, f)))
        .collect();
    if r.body.solids.len() <= 1 {
        return Ok(vec![Cell {
            body: r.body,
            on_gone,
        }]);
    }
    (0..r.body.solids.len())
        .map(|s| {
            let faces = r.body.solid_faces(s);
            let mut body = super::kernel::subset(&r.body, &faces, true);
            body.shells.clear();
            body.solids.clear();
            super::boolean::assemble_shells(&mut body)?;
            Ok(Cell {
                body,
                on_gone: faces.iter().map(|&f| on_gone[f]).collect(),
            })
        })
        .collect()
}

/// The body of an extrude of `profile`.
fn solid(k: &mut NativeKernel, profile: &Profile, extent: Extent) -> Result<Body> {
    let r = k.extrude(profile, extent)?;
    let id = *r.bodies.first().ok_or_else(|| KernelError::InvalidResult {
        op: "delete face",
        why: "no solid for the heal's tool".into(),
    })?;
    let b = (**k.get(id)?).clone();
    k.release(id);
    Ok(b)
}

fn plane(origin: P3, x: V3, normal: V3) -> Plane {
    Plane {
        origin,
        x_dir: nalgebra::Unit::new_normalize(x),
        normal: nalgebra::Unit::new_normalize(normal),
    }
}

fn line(a: (f64, f64), b: (f64, f64)) -> Curve2 {
    Curve2::Line {
        a: nalgebra::Point2::new(a.0, a.1),
        b: nalgebra::Point2::new(b.0, b.1),
        source: None,
    }
}

fn rect_region(x0: f64, y0: f64, x1: f64, y1: f64) -> Region {
    Region {
        outer: Loop {
            curves: vec![
                line((x0, y0), (x1, y0)),
                line((x1, y0), (x1, y1)),
                line((x1, y1), (x0, y1)),
                line((x0, y1), (x0, y0)),
            ],
        },
        holes: Vec::new(),
        source: None,
    }
}

/// The box `lo`–`hi` as a rectangle on its floor (extruded up by its height).
fn box_profile(lo: P3, hi: P3) -> Profile {
    Profile::new(
        plane(lo, V3::x(), V3::z()),
        vec![rect_region(0.0, 0.0, hi.x - lo.x, hi.y - lo.y)],
    )
}

/// A square `2·half` wide centred on `at` in the plane through it normal to `normal`.
fn square(at: P3, x: V3, normal: V3, half: f64) -> Profile {
    let x = (x - normal * x.dot(&normal)).normalize();
    Profile::new(
        plane(at, x, normal),
        vec![rect_region(-half, -half, half, half)],
    )
}

/// A disc of radius `r` centred on `at` normal to `z`.
fn disc(at: P3, x: V3, z: V3, r: f64) -> Profile {
    Profile::new(
        plane(at, x, z),
        vec![Region {
            outer: Loop {
                curves: vec![Curve2::Circle {
                    center: nalgebra::Point2::origin(),
                    radius: r,
                    source: None,
                }],
            },
            holes: Vec::new(),
            source: None,
        }],
    )
}
