//! Section and broken-out views (P3C.8, D4.12): the parts cut before the hidden-line removal.
//!
//! No new kernel operation: the cut is an existing [`Kernel::extrude`] of the cut region (the
//! whole view for a section, the broken-out boundary otherwise) from in front of the parts back to
//! the cutting depth, and a [`Kernel::boolean`] subtract of that tool from each part (see
//! `crates/cadrs_kernel/README.md`, "Drawing-view projection"). The result is named from the part's names
//! (`naming::name_body` with the boolean's history), so edges the cut leaves alone keep their
//! persistent names and annotations attach as in any view.
//!
//! The **hatch region** is the faces the cut leaves on its plane: the planar faces of the result
//! whose normal is the direction of sight and whose depth is the cut's. Their triangles, seen
//! in the view, are turned counter-clockwise and their boundary (the triangle sides no other
//! triangle shares, by vertex position) is chained into loops: outer loops counter-clockwise and
//! holes clockwise, so the loops' signed areas add up to the region's area.

use std::collections::HashMap;

use cadrs_drawing::view_kinds::ViewCut;
use cadrs_kernel::naming::{self, BodyNames};
use cadrs_kernel::{BodyId, BoolOp, Curve2, Extent, Kernel, Loop, Plane, Profile, Region, Tessellation, ViewFrame};
use nalgebra::{Point2, Point3, Unit};

/// The operation id the cut's faces are named under.
pub const SECTION_OP: uuid::Uuid = uuid::Uuid::from_u128(0x5ec7_1000_0000_0000_0000_0000_0000_0001);

/// Bodies cut for a view.
pub struct Cut {
    /// The bodies to project (the cut results, or the originals where nothing was cut).
    pub bodies: Vec<BodyId>,
    pub names: Vec<BodyNames>,
    /// The hatch region's loops (view 2D).
    pub hatch: Vec<Vec<[f64; 2]>>,
    /// Temporary bodies to release after projecting.
    pub temps: Vec<BodyId>,
}

/// Cuts `bodies` (with their names) for a view with frame `frame`: removes the material nearer
/// the eye than `cut.depth` inside `cut.polygon` (the whole view when empty).
pub fn cut_bodies(k: &mut dyn Kernel, bodies: &[(BodyId, &BodyNames)], frame: &ViewFrame, cut: &ViewCut) -> Result<Cut, String> {
    let mut out = Cut { bodies: Vec::new(), names: Vec::new(), hatch: Vec::new(), temps: Vec::new() };
    // The parts' extent along the direction of sight and in the view.
    let (mut dmin, mut dmax) = (f64::MAX, f64::MIN);
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for (b, _) in bodies {
        let bb = k.bounding_box(*b).map_err(|e| e.to_string())?;
        for i in 0..8 {
            let p = Point3::new(
                if i & 1 == 0 { bb.min.x } else { bb.max.x },
                if i & 2 == 0 { bb.min.y } else { bb.max.y },
                if i & 4 == 0 { bb.min.z } else { bb.max.z },
            );
            let d = frame.depth(&p);
            let q = frame.to_2d(&p);
            dmin = dmin.min(d);
            dmax = dmax.max(d);
            lo = [lo[0].min(q.x), lo[1].min(q.y)];
            hi = [hi[0].max(q.x), hi[1].max(q.y)];
        }
    }
    let keep_all = |out: &mut Cut| {
        for (b, n) in bodies {
            out.bodies.push(*b);
            out.names.push((*n).clone());
        }
    };
    if bodies.is_empty() || cut.depth <= dmin + 1e-6 {
        keep_all(&mut out);
        return Ok(out);
    }
    let size = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(dmax - dmin).max(1.0);
    let margin = 0.1 * size + 1.0;
    let polygon: Vec<[f64; 2]> = if cut.polygon.len() >= 3 {
        cut.polygon.clone()
    } else {
        let (a, b) = ([lo[0] - margin, lo[1] - margin], [hi[0] + margin, hi[1] + margin]);
        vec![a, [b[0], a[1]], b, [a[0], b[1]]]
    };
    // The tool's sketch plane: in front of the parts, facing along the direction of sight.
    let start = dmin - margin;
    let plane = Plane {
        origin: frame.origin + frame.dir * start,
        x_dir: Unit::new_normalize(frame.x),
        normal: Unit::new_normalize(frame.dir),
    };
    // View 2D → plane 2D (the plane's y is normal × x = −(the view's up)).
    let ydir = plane.y_dir().into_inner();
    let up = frame.up();
    let to_plane = |p: [f64; 2]| {
        let m = frame.x * p[0] + up * p[1];
        Point2::new(m.dot(&frame.x), m.dot(&ydir))
    };
    let mut pts: Vec<Point2<f64>> = polygon.iter().map(|p| to_plane(*p)).collect();
    let area: f64 = (0..pts.len())
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            a.x * b.y - b.x * a.y
        })
        .sum();
    if area < 0.0 {
        pts.reverse();
    }
    let curves: Vec<Curve2> = (0..pts.len())
        .map(|i| Curve2::Line { a: pts[i], b: pts[(i + 1) % pts.len()], source: Some(i as u64 + 1) })
        .collect();
    let profile = Profile::new(plane, vec![Region { outer: Loop { curves }, holes: Vec::new(), source: Some(1) }]);
    let tool_r = k.extrude(&profile, Extent::Blind(cut.depth - start)).map_err(|e| format!("Section cut failed: {e}"))?;
    let Some(&tool) = tool_r.bodies.first() else {
        return Err("Section cut failed: no tool body".into());
    };
    out.temps.push(tool);
    for b in tool_r.bodies.iter().skip(1) {
        out.temps.push(*b);
    }
    let tool_names = naming::name_body(k, tool, SECTION_OP, &tool_r.history, &[]).map_err(|e| e.to_string())?;
    for (b, names) in bodies {
        let r = match k.boolean(BoolOp::Subtract, *b, &[tool]) {
            Ok(r) => r,
            Err(e) => {
                for t in &out.temps {
                    k.release(*t);
                }
                return Err(format!("Section cut failed: {e}"));
            }
        };
        for nb in r.bodies.iter().copied() {
            out.temps.push(nb);
            let inputs: Vec<(BodyId, &BodyNames)> = vec![(*b, *names), (tool, &tool_names)];
            let n = naming::name_body(k, nb, SECTION_OP, &r.history, &inputs).unwrap_or_default();
            out.hatch.extend(cap_loops(k, nb, frame, cut.depth)?);
            out.bodies.push(nb);
            out.names.push(n);
        }
    }
    Ok(out)
}

/// The faces of `body` on the cutting plane: planar, facing along the direction of sight, at the
/// cut's depth.
fn cap_faces(k: &dyn Kernel, body: BodyId, frame: &ViewFrame, depth: f64) -> Result<Vec<cadrs_kernel::FaceInfo>, String> {
    let faces = k.faces(body).map_err(|e| e.to_string())?;
    Ok(faces
        .into_iter()
        .filter(|f| {
            f.plane.as_ref().is_some_and(|p| {
                p.normal.into_inner().dot(&frame.dir).abs() > 1.0 - 1e-6 && (frame.depth(&p.origin) - depth).abs() < 1e-4
            })
        })
        .collect())
}

/// The loops of the faces of `body` on the cutting plane (see the module docs).
fn cap_loops(k: &dyn Kernel, body: BodyId, frame: &ViewFrame, depth: f64) -> Result<Vec<Vec<[f64; 2]>>, String> {
    let caps: Vec<cadrs_kernel::FaceId> = cap_faces(k, body, frame, depth)?.iter().map(|f| f.id).collect();
    if caps.is_empty() {
        return Ok(Vec::new());
    }
    let mesh = k
        .tessellate(body, Tessellation { deflection: 0.005, angle: 2.5f64.to_radians() })
        .map_err(|e| e.to_string())?;
    Ok(boundary_loops(
        mesh.indices
            .iter()
            .zip(&mesh.triangle_faces)
            .filter(|(_, f)| caps.contains(f))
            .map(|(t, _)| {
                t.map(|i| {
                    let q = frame.to_2d(&mesh.positions[i as usize]);
                    [q.x, q.y]
                })
            }),
    ))
}

// ---------------------------------------------------------------------------------------------
// Section view (P3E.3a, PS2.9, X14, A3.3, IR5.5): the 3D view cut by a plane, with caps.

/// A section view's cutting plane: the material on the side `normal` points to is removed (the
/// eye looks at the cut from that side).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SectionPlane {
    pub origin: [f64; 3],
    /// Unit length.
    pub normal: [f64; 3],
}

/// The faces a section leaves on its plane for one part: its triangles and boundary loops (3D,
/// in the part's placed coordinates) and its exact area (mm², the kernel's).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cap {
    pub triangles: Vec<[[f32; 3]; 3]>,
    pub loops: Vec<Vec<[f32; 3]>>,
    pub area: f64,
}

impl SectionPlane {
    /// The drawing frame that looks at the cut from the removed side: the material nearer the
    /// eye than depth 0 is what goes.
    fn frame(&self) -> ViewFrame {
        let n = nalgebra::Vector3::from(self.normal).normalize();
        let x = if n.x.abs() < 0.9 { nalgebra::Vector3::x() } else { nalgebra::Vector3::y() };
        let mut f = ViewFrame::new(-n, x);
        f.origin = Point3::from(self.origin);
        f
    }
}

/// The cap a section by `plane` leaves on `body`: a half-space boolean ([`cut_bodies`], the
/// drawings' section cut, with the whole of the body's extent as the cut region) and the cut
/// result's faces on the plane. A body the plane misses has no cap.
pub fn section_cap(k: &mut dyn Kernel, body: BodyId, plane: &SectionPlane) -> Result<Cap, String> {
    let frame = plane.frame();
    let names = BodyNames::default();
    let cut = ViewCut { depth: 0.0, polygon: Vec::new() };
    let r = match cut_bodies(k, &[(body, &names)], &frame, &cut) {
        Ok(r) => r,
        // All of it on the removed side: nothing left, so no cap.
        Err(_) => return Ok(Cap::default()),
    };
    let mut cap = Cap::default();
    let result = (|| -> Result<(), String> {
        for b in r.bodies.iter().copied().filter(|b| r.temps.contains(b)) {
            let faces = cap_faces(k, b, &frame, 0.0)?;
            if faces.is_empty() {
                continue;
            }
            cap.area += faces.iter().map(|f| f.area).sum::<f64>();
            let ids: Vec<cadrs_kernel::FaceId> = faces.iter().map(|f| f.id).collect();
            let mesh = k.tessellate(b, Tessellation { deflection: 0.01, angle: 5f64.to_radians() }).map_err(|e| e.to_string())?;
            let tris: Vec<[[f64; 3]; 3]> = mesh
                .indices
                .iter()
                .zip(&mesh.triangle_faces)
                .filter(|(_, f)| ids.contains(f))
                .map(|(t, _)| t.map(|i| {
                    let p = mesh.positions[i as usize];
                    [p.x, p.y, p.z]
                }))
                .collect();
            let up = frame.up();
            let to3 = |q: [f64; 2]| {
                let p = frame.origin + frame.x * q[0] + up * q[1];
                [p.x as f32, p.y as f32, p.z as f32]
            };
            cap.loops.extend(
                boundary_loops(tris.iter().map(|t| {
                    t.map(|p| {
                        let q = frame.to_2d(&Point3::from(p));
                        [q.x, q.y]
                    })
                }))
                .into_iter()
                .map(|l| l.into_iter().map(to3).collect::<Vec<_>>()),
            );
            cap.triangles.extend(tris.iter().map(|t| t.map(|p| [p[0] as f32, p[1] as f32, p[2] as f32])));
        }
        Ok(())
    })();
    for t in &r.temps {
        k.release(*t);
    }
    result.map(|_| cap)
}

/// One part to section: its Part Studio's features, the part, and where it is placed (an
/// assembly's occurrence; the identity in a Part Studio). `view_part` is its id on screen.
#[derive(Debug, Clone)]
pub struct SectionItem {
    pub view_part: crate::ids::PartId,
    pub features: Vec<crate::document::Feature>,
    pub part: crate::ids::PartId,
    pub pose: crate::assembly::Pose,
}

/// The caps of `items` cut by `plane`, worked out on the kernel thread (each part's exact body,
/// placed, cut by the half-space). Parts that fail or that the plane misses are left out.
pub fn caps(items: Vec<SectionItem>, plane: SectionPlane) -> crate::rebuild::PendingJob<Vec<(crate::ids::PartId, Cap)>> {
    crate::rebuild::run_on_worker(move |r| caps_on(r, &items, &plane))
}

#[cfg(feature = "occt")]
fn caps_on(r: &mut crate::rebuild::Rebuilder, items: &[SectionItem], plane: &SectionPlane) -> Vec<(crate::ids::PartId, Cap)> {
    let mut out = Vec::new();
    for it in items {
        let Ok(body) = r.placed_body(&it.features, it.part, it.pose.rotation, it.pose.translation) else { continue };
        let cap = section_cap(r.kernel_mut(), body, plane);
        r.release_body(body);
        if let Ok(cap) = cap
            && !cap.triangles.is_empty()
        {
            out.push((it.view_part, cap));
        }
    }
    out
}

#[cfg(not(feature = "occt"))]
fn caps_on(_r: &mut crate::rebuild::Rebuilder, _items: &[SectionItem], _plane: &SectionPlane) -> Vec<(crate::ids::PartId, Cap)> {
    Vec::new()
}

/// The boundary loops of a set of 2D triangles (see the module docs).
pub fn boundary_loops(tris: impl Iterator<Item = [[f64; 2]; 3]>) -> Vec<Vec<[f64; 2]>> {
    // Vertices by position (1e-6 mm).
    let key = |p: [f64; 2]| ((p[0] * 1e6).round() as i64, (p[1] * 1e6).round() as i64);
    let mut ids: HashMap<(i64, i64), usize> = HashMap::new();
    let mut pos: Vec<[f64; 2]> = Vec::new();
    let mut id = |p: [f64; 2], pos: &mut Vec<[f64; 2]>| {
        *ids.entry(key(p)).or_insert_with(|| {
            pos.push(p);
            pos.len() - 1
        })
    };
    // Directed edges of the counter-clockwise triangles, counted.
    let mut edges: HashMap<(usize, usize), usize> = HashMap::new();
    for t in tris {
        let a2 = (t[1][0] - t[0][0]) * (t[2][1] - t[0][1]) - (t[2][0] - t[0][0]) * (t[1][1] - t[0][1]);
        if a2.abs() < 1e-14 {
            continue;
        }
        let t = if a2 > 0.0 { t } else { [t[0], t[2], t[1]] };
        let v = t.map(|p| id(p, &mut pos));
        if v[0] == v[1] || v[1] == v[2] || v[0] == v[2] {
            continue;
        }
        for i in 0..3 {
            *edges.entry((v[i], v[(i + 1) % 3])).or_default() += 1;
        }
    }
    // Boundary: a directed edge whose reverse no triangle has.
    let mut next: HashMap<usize, Vec<usize>> = HashMap::new();
    for (&(a, b), &n) in &edges {
        let back = edges.get(&(b, a)).copied().unwrap_or(0);
        for _ in back..n {
            next.entry(a).or_default().push(b);
        }
    }
    let mut loops = Vec::new();
    let starts: Vec<usize> = {
        let mut s: Vec<usize> = next.keys().copied().collect();
        s.sort();
        s
    };
    for s in starts {
        while next.get(&s).is_some_and(|v| !v.is_empty()) {
            let mut l = vec![pos[s]];
            let mut cur = s;
            let mut guard = 0;
            while let Some(n) = next.get_mut(&cur).and_then(|v| v.pop()) {
                if n == s {
                    break;
                }
                l.push(pos[n]);
                cur = n;
                guard += 1;
                if guard > 1_000_000 {
                    break;
                }
            }
            if l.len() >= 3 {
                loops.push(l);
            }
        }
    }
    loops
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_square_with_a_hole_gives_two_loops_whose_areas_add_up() {
        // An 8-triangle square ring: outer 0..4, hole 1..3.
        let o = [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
        let i = [[1.0, 1.0], [3.0, 1.0], [3.0, 3.0], [1.0, 3.0]];
        let mut tris = Vec::new();
        for k in 0..4 {
            let (a, b) = (o[k], o[(k + 1) % 4]);
            let (c, d) = (i[k], i[(k + 1) % 4]);
            tris.push([a, b, d]);
            tris.push([a, d, c]);
        }
        let loops = boundary_loops(tris.into_iter());
        assert_eq!(loops.len(), 2);
        let area = cadrs_drawing::view_kinds::region_area(&loops);
        assert!((area - 12.0).abs() < 1e-9, "{area}");
    }

    /// P3E.3a (PS2.9): a section through a Ø40 cylinder, 30 tall, at mid height caps with its
    /// circle: area 400π = 1256.6371 mm², the cap's boundary a circle of radius 20 on the
    /// plane; flipped, the same cap; a plane above it leaves none.
    #[cfg(feature = "occt")]
    #[test]
    fn a_section_through_a_40_mm_cylinder_caps_with_its_circle() {
        use cadrs_kernel::backend::occt::OcctKernel;
        let mut k = OcctKernel::new();
        let circle = Curve2::Circle { center: Point2::new(5.0, -3.0), radius: 20.0, source: Some(1) };
        let profile = Profile::new(Plane::top(), vec![Region { outer: Loop { curves: vec![circle] }, holes: Vec::new(), source: Some(1) }]);
        let body = k.extrude(&profile, Extent::Blind(30.0)).unwrap().bodies[0];
        for normal in [[0.0, 0.0, 1.0], [0.0, 0.0, -1.0]] {
            let cap = section_cap(&mut k, body, &SectionPlane { origin: [0.0, 0.0, 12.0], normal }).unwrap();
            assert!((cap.area - 400.0 * std::f64::consts::PI).abs() < 1e-4, "{}", cap.area);
            assert!((cap.area - 1256.6371).abs() < 1e-4);
            let tri_area: f64 = cap
                .triangles
                .iter()
                .map(|t| {
                    let v = |p: [f32; 3]| nalgebra::Vector3::new(p[0] as f64, p[1] as f64, p[2] as f64);
                    (v(t[1]) - v(t[0])).cross(&(v(t[2]) - v(t[0]))).norm() / 2.0
                })
                .sum();
            assert!((tri_area - cap.area).abs() < 2.0, "{tri_area}");
            assert!(cap.triangles.iter().flatten().all(|p| (p[2] - 12.0).abs() < 1e-3));
            assert_eq!(cap.loops.len(), 1);
            assert!(cap.loops[0].iter().all(|p| ((p[0] - 5.0).hypot(p[1] + 3.0) - 20.0).abs() < 0.05));
        }
        // The body is still there, whole.
        assert!((k.mass_properties(body).unwrap().volume - 400.0 * std::f64::consts::PI * 30.0).abs() < 1e-3);
        let above = section_cap(&mut k, body, &SectionPlane { origin: [0.0, 0.0, 40.0], normal: [0.0, 0.0, 1.0] }).unwrap();
        assert!(above.triangles.is_empty() && above.area == 0.0);
    }
}
