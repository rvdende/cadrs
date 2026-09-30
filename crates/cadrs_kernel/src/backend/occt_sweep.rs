//! Sweeps, lofts and splits of the OCCT backend (P3.7, [`Kernel::sweep_with`],
//! [`Kernel::loft_with`], [`Kernel::split`]).
//!
//! **Sweep** (`BRepOffsetAPI_MakePipe`; `BRepOffsetAPI_MakePipeShell` to lock the profile's
//! direction). The path's curves are chained end to end. Where the profile's plane crosses the
//! path (nearest the profile) the sweep starts: at an end of an open path it runs along the
//! whole path; part way along an open path it runs both ways (PS19.5), two sweeps fused; on a
//! closed path the path is cut there so it starts and ends at the profile. Faces are tagged
//! through OCCT's history as an extrude's: the side of each profile curve, the start cap on the
//! profile and the end cap (a two-way sweep's far ends are its end and start caps).
//!
//! **Loft**. Without end conditions, `BRepOffsetAPI_ThruSections` through the sections (wires,
//! and vertices for point sections). With a start or end condition, the loft is built here:
//! every section is sampled ([`crate::loft::sample_sections`]), each section interpolated by a
//! cubic B-spline with a shared knot vector, and each column of poles by a cubic B-spline
//! through the sections with the end derivatives (`Shape::try_loft_solid`): *Normal to
//! profile* is `m·L·n` (n the section's normal along the loft, L the length of the polyline
//! through the section centres, m the magnitude, with the sections placed along [0, 1] by that
//! length), *Tangent to profile* the same length along the section's plane, away from its
//! centre (towards it when the next section is smaller).
//!
//! **Split** (`BRepAlgoAPI_Splitter`): by an unbounded plane (a large square face), a face, or
//! a sheet body. The body's faces are modified, the cut faces generated.

use glam::DVec3;
use nalgebra::{Point3, Vector3};
use opencascade::primitives::{Edge, Shape, Wire};
use opencascade::safe::{LoftDerivative, SubKind, SweepMode};

use super::extrude::{Tagged, chain_wire, edge_origins};
use super::{
    Edges, OcctKernel, curve_edge, face_count, faces_of, generated, loop_wire, occt, oriented, profile_faces,
    select_edges, sweep_origins, to_glam, to_na, union_tagged,
};
use crate::loft::{SectionCurves, sample_sections, section_loop};
use crate::{
    BodyId, BodyKind, EdgeId, FaceId, FaceInput, History, InputFace, Kernel, KernelError, LoftCondition, LoftEnd,
    LoftSection, LoftSpec, OpResult, Origin, PathCurve, Plane, Profile, Result, SplitTool, SurfaceKind,
    SweepControl, SweepSpec,
};

/// A face edge's samples in loop order with its edge id (a loft section of a face, P3.10).
type Polyline = (Vec<Point3<f64>>, Option<u64>);

/// How close path curve ends must be to join (mm).
const PATH_JOIN: f64 = 1e-5;

/// Path edges shorter than this (mm) are slivers, left out of the chain.
const MIN_PATH_EDGE: f64 = 1e-4;

fn invalid(why: impl Into<String>) -> KernelError {
    KernelError::InvalidParameter(why.into())
}

/// A path edge in the direction the path runs along it.
pub(super) struct PathEdge {
    edge: Edge,
    /// Runs against its curve's parameter.
    reversed: bool,
}

impl PathEdge {
    fn start(&self) -> DVec3 {
        if self.reversed { self.edge.end_point() } else { self.edge.start_point() }
    }

    fn end(&self) -> DVec3 {
        if self.reversed { self.edge.start_point() } else { self.edge.end_point() }
    }

    /// The edge oriented the way the path runs (a new handle on the same edge).
    pub(super) fn oriented(&self) -> Result<Edge> {
        copy_edge(&self.edge, self.reversed)
    }

    /// Points along it, in the direction the path runs.
    pub(super) fn points(&self) -> Vec<DVec3> {
        let shape = Shape::from(&self.edge);
        let mut pts = self.edge.polyline(&shape, 0.05, 0.01).unwrap_or_default();
        if pts.len() < 2 {
            pts = vec![self.edge.start_point(), self.edge.end_point()];
        }
        if self.reversed {
            pts.reverse();
        }
        pts
    }
}

/// A new handle on an edge, reversed or not.
fn copy_edge(e: &Edge, reversed: bool) -> Result<Edge> {
    let r = e.try_reversed().map_err(occt)?;
    if reversed { Ok(r) } else { r.try_reversed().map_err(occt) }
}

/// The path's edges chained end to end (each run the way the path goes), and whether it closes.
pub(super) fn chain(edges: Vec<Edge>) -> Result<(Vec<PathEdge>, bool)> {
    if edges.is_empty() {
        return Err(invalid("Select a sweep path"));
    }
    // A sliver edge (a fillet can leave one about 1e-6 mm long where two faces of a rim meet)
    // would sweep a face too thin to mesh: it is left out, its neighbours' ends meet within
    // `PATH_JOIN` anyway.
    let edges: Vec<Edge> = if edges.len() > 1 {
        let length = |e: &Edge| {
            let shape = Shape::from(e);
            let pts = e.polyline(&shape, 0.05, 0.01).unwrap_or_default();
            pts.windows(2).map(|w| w[0].distance(w[1])).sum::<f64>().max(e.start_point().distance(e.end_point()))
        };
        edges.into_iter().filter(|e| length(e) >= MIN_PATH_EDGE).collect()
    } else {
        edges
    };
    if edges.is_empty() {
        return Err(invalid("Select a sweep path"));
    }
    let ends: Vec<(DVec3, DVec3)> = edges.iter().map(|e| (e.start_point(), e.end_point())).collect();
    let n = edges.len();
    let touches = |p: DVec3, skip: usize| -> usize {
        (0..n)
            .filter(|&j| j != skip)
            .map(|j| (ends[j].0.distance(p) < PATH_JOIN) as usize + (ends[j].1.distance(p) < PATH_JOIN) as usize)
            .sum()
    };
    // Start at a free end if there is one (an open path).
    let mut first = (0usize, false);
    for (i, (s, e)) in ends.iter().enumerate() {
        let closed_edge = s.distance(*e) < PATH_JOIN;
        if closed_edge {
            continue;
        }
        if touches(*s, i) == 0 {
            first = (i, false);
            break;
        }
        if touches(*e, i) == 0 {
            first = (i, true);
            break;
        }
    }
    let mut used = vec![false; n];
    let mut order = vec![first];
    used[first.0] = true;
    let mut at = if first.1 { ends[first.0].0 } else { ends[first.0].1 };
    loop {
        let next = (0..n).filter(|&j| !used[j]).find_map(|j| {
            if ends[j].0.distance(at) < PATH_JOIN {
                Some((j, false))
            } else if ends[j].1.distance(at) < PATH_JOIN {
                Some((j, true))
            } else {
                None
            }
        });
        let Some((j, rev)) = next else { break };
        used[j] = true;
        order.push((j, rev));
        at = if rev { ends[j].0 } else { ends[j].1 };
    }
    if used.iter().any(|u| !u) {
        return Err(invalid("The sweep path must be one chain of connected curves"));
    }
    let start = if first.1 { ends[first.0].1 } else { ends[first.0].0 };
    let closed = at.distance(start) < PATH_JOIN;
    let mut slots: Vec<Option<Edge>> = edges.into_iter().map(Some).collect();
    let out = order
        .into_iter()
        .map(|(i, reversed)| PathEdge {
            edge: slots[i].take().expect("each edge once"),
            reversed,
        })
        .collect();
    Ok((out, closed))
}

/// Where along the path the profile's plane crosses it, nearest `center`: the edge and the
/// point. `None` when the plane doesn't cross the path.
fn pierce(path: &[PathEdge], plane: &Plane, center: Point3<f64>) -> Option<(usize, DVec3)> {
    let o = to_glam(plane.origin.coords);
    let n = to_glam(plane.normal.into_inner());
    let c = to_glam(center.coords);
    let mut best: Option<(f64, usize, DVec3)> = None;
    for (i, e) in path.iter().enumerate() {
        let pts = e.points();
        for w in pts.windows(2) {
            let (fa, fb) = ((w[0] - o).dot(n), (w[1] - o).dot(n));
            let p = if fa.abs() < 1e-9 {
                w[0]
            } else if fb.abs() < 1e-9 {
                w[1]
            } else if fa.signum() != fb.signum() {
                w[0] + (w[1] - w[0]) * (fa / (fa - fb))
            } else {
                continue;
            };
            let d = p.distance(c);
            if best.is_none_or(|(bd, ..)| d < bd) {
                best = Some((d, i, p));
            }
        }
    }
    best.map(|(_, i, p)| (i, p))
}

/// The spines of a sweep: one (the path, started at the profile) or two (from the profile
/// forwards and backwards), and whether each runs backwards (a two-way sweep's second half).
fn spines(path: Vec<PathEdge>, closed: bool, plane: &Plane, center: Point3<f64>) -> Result<Vec<(Wire, bool)>> {
    let wire = |edges: &[Edge]| Wire::try_from_edges(edges).map_err(occt);
    let oriented = |p: &[PathEdge]| p.iter().map(PathEdge::oriented).collect::<Result<Vec<_>>>();
    let tol = 1e-6 * (1.0 + path.iter().map(|e| e.start().length()).fold(0.0, f64::max));
    let Some((i, at)) = pierce(&path, plane, center) else {
        return Ok(vec![(wire(&oriented(&path)?)?, false)]);
    };
    let start = path[0].start();
    let end = path[path.len() - 1].end();
    if at.distance(start) < tol || (!closed && at.distance(end) < tol) {
        if at.distance(start) < tol {
            return Ok(vec![(wire(&oriented(&path)?)?, false)]);
        }
        // At the far end of an open path: run it backwards.
        let back: Vec<Edge> = path.iter().rev().map(|e| copy_edge(&e.edge, !e.reversed)).collect::<Result<_>>()?;
        return Ok(vec![(wire(&back)?, false)]);
    }
    // At a joint between two edges: split there without cutting an edge.
    let joint = (0..path.len()).find(|&j| path[j].start().distance(at) < tol);
    let (before, after): (Vec<Edge>, Vec<Edge>) = match joint {
        Some(j) => (oriented(&path[..j])?, oriented(&path[j..])?),
        None => {
            let e = path[i].oriented()?;
            let pieces = e.try_split_at(at).map_err(occt)?;
            if pieces.len() != 2 {
                return Err(KernelError::OperationFailed("the path could not be cut at the profile".into()));
            }
            // The pieces follow the curve's parameter: put them in the path's direction.
            let (a, b) = if path[i].reversed {
                (copy_edge(&pieces[1], true)?, copy_edge(&pieces[0], true)?)
            } else {
                (copy_edge(&pieces[0], false)?, copy_edge(&pieces[1], false)?)
            };
            let mut before = oriented(&path[..i])?;
            before.push(a);
            let mut after = vec![b];
            after.extend(oriented(&path[i + 1..])?);
            (before, after)
        }
    };
    if closed {
        // Start and end at the profile.
        let mut all = after;
        all.extend(before);
        return Ok(vec![(wire(&all)?, false)]);
    }
    let back: Vec<Edge> = before.iter().rev().map(|e| e.try_reversed().map_err(occt)).collect::<Result<_>>()?;
    Ok(vec![(wire(&after)?, false), (wire(&back)?, true)])
}

/// A two-way sweep's backward half: its start and end caps swap (its far end is the sweep's
/// start).
fn swap_caps(tags: &mut [Option<Origin>]) {
    for t in tags.iter_mut().flatten() {
        *t = match *t {
            Origin::StartCap { region } => Origin::EndCap { region },
            Origin::EndCap { region } => Origin::StartCap { region },
            o => o,
        };
    }
}

/// The middle of a profile's curves (where the path should meet it).
fn profile_center(profile: &Profile) -> Point3<f64> {
    let mut acc = Vector3::zeros();
    let mut n = 0.0;
    for c in profile
        .regions
        .iter()
        .flat_map(|r| std::iter::once(&r.outer).chain(&r.holes))
        .flat_map(|l| l.curves.iter())
        .chain(profile.chains.iter().flat_map(|c| c.curves.iter()))
    {
        for k in 0..8 {
            acc += profile.plane.to_model(c.point_at(k as f64 / 8.0)).coords;
            n += 1.0;
        }
    }
    if n == 0.0 { profile.plane.origin } else { Point3::from(acc / n) }
}

impl OcctKernel {
    /// The path's curves as edges.
    fn path_edges(&self, path: &[PathCurve]) -> Result<Vec<Edge>> {
        let mut out = Vec::new();
        for pc in path {
            match pc {
                PathCurve::Edge { body, edge } => {
                    let shape = self.body(*body)?;
                    out.extend(select_edges(shape, &[*edge])?);
                }
                PathCurve::Sketch { plane, curve } => {
                    out.push(curve_edge(plane, curve, curve.start(), curve.end(), curve.is_closed())?);
                }
                PathCurve::Bezier3 { poles, weights } => {
                    let pts: Vec<glam::DVec3> = poles.iter().map(|p| to_glam(p.coords)).collect();
                    let w = if weights.len() == pts.len() { weights.clone() } else { vec![1.0; pts.len()] };
                    out.push(Edge::try_bezier(&pts, &w).map_err(occt)?);
                }
            }
        }
        Ok(out)
    }

    pub(super) fn sweep_full(&mut self, profile: &Profile, spec: &SweepSpec) -> Result<OpResult> {
        let edges = self.path_edges(&spec.path)?;
        let (path, closed) = chain(edges)?;
        let center = if profile.regions.is_empty() && profile.chains.is_empty() {
            // Faces only: the middle of the first face.
            spec.faces
                .first()
                .and_then(|f| self.faces(f.body).ok()?.into_iter().find(|i| i.id == f.face).map(|i| i.center))
                .unwrap_or(profile.plane.origin)
        } else {
            profile_center(profile)
        };
        let plane = match spec.faces.first() {
            Some(f) if profile.regions.is_empty() && profile.chains.is_empty() => self
                .faces(f.body)?
                .into_iter()
                .find(|i| i.id == f.face)
                .and_then(|i| i.plane)
                .unwrap_or(profile.plane),
            _ => profile.plane,
        };
        let spines = spines(path, closed, &plane, center)?;
        let mode = match spec.control {
            SweepControl::None => SweepMode::CorrectedFrenet,
            SweepControl::KeepOrientation => SweepMode::Fixed,
            SweepControl::LockDirection(d) => SweepMode::Binormal(to_glam(d.into_inner())),
        };
        let mut pieces: Vec<Tagged> = Vec::new();
        for (spine, backward) in &spines {
            let mut these = match spec.body {
                BodyKind::Solid => self.sweep_regions(profile, &spec.faces, spine, mode, closed)?,
                BodyKind::Thin { left, right } => {
                    let thin = crate::thin::thin_profile(profile, left, right)?;
                    self.sweep_regions(&thin, &[], spine, mode, closed)?
                }
                BodyKind::Surface => self.sweep_curve_sheets(profile, spine, mode)?,
            };
            if *backward {
                for (_, tags) in &mut these {
                    swap_caps(tags);
                }
            }
            pieces.extend(these);
        }
        let (shape, tags) = if spec.body == BodyKind::Surface {
            let shapes: Vec<Shape> = pieces.iter().map(|(s, _)| super::extrude::clone(s)).collect();
            let compound = Shape::try_compound(shapes.iter()).map_err(occt)?;
            // A compound keeps its members' faces in order.
            let tags: Vec<Option<Origin>> = pieces.into_iter().flat_map(|(_, t)| t).collect();
            (compound, tags)
        } else {
            union_tagged(pieces)?
        };
        let n = face_count(&shape)?;
        let mut tags = tags;
        tags.resize(n, None);
        self.insert(shape, generated(tags))
    }

    /// A region swept by `BRepOffsetAPI_MakePipeShell`: its outer loop's solid less its holes'
    /// (a closed path makes a closed ring without caps, which `MakePipe` can't: it leaves two
    /// coincident caps inside the solid), each face tagged with the profile curve that swept it.
    fn shell_sweep_region(&self, plane: &Plane, region: &crate::Region, index: usize, spine: &Wire, mode: SweepMode) -> Result<Tagged> {
        let region_id = region.source.unwrap_or(index as u64);
        let pipe = |curves: Vec<crate::Curve2>, lp: usize| -> Result<Tagged> {
            let wire = loop_wire(plane, &curves)?;
            let (shape, h) = spine.try_pipe_shell_h(&wire, mode, true).map_err(occt)?;
            let edges = wire.edges_geometry().map_err(occt)?;
            let ids: Vec<(u64, Vec<Point3<f64>>)> = curves
                .iter()
                .enumerate()
                .map(|(ci, c)| {
                    let id = c.source().unwrap_or_else(|| crate::unsourced_curve(lp, ci));
                    (id, (0..=32).map(|k| plane.to_model(c.point_at(k as f64 / 32.0))).collect())
                })
                .collect();
            let mut tags = vec![None; face_count(&shape)?];
            edge_origins(&edges, &h.edges, &ids, region_id, &mut tags);
            for (list, origin) in [
                (&h.first, Origin::StartCap { region: region_id }),
                (&h.last, Origin::EndCap { region: region_id }),
            ] {
                for &f in list {
                    if let Some(t) = tags.get_mut(f) {
                        t.get_or_insert(origin);
                    }
                }
            }
            Ok((shape, tags))
        };
        let (mut acc, mut tags) = pipe(oriented(&region.outer, true)?, 0)?;
        for (k, hole) in region.holes.iter().enumerate() {
            let (cut, cut_tags) = pipe(oriented(hole, true)?, k + 1)?;
            let (next, h) = acc.try_subtract_h(&cut).map_err(occt)?;
            tags = super::carry(&h, &[&tags, &cut_tags], face_count(&next)?);
            acc = next;
        }
        Ok((acc, tags))
    }

    /// Solid sweeps of the profile's regions and the input faces.
    fn sweep_regions(&self, profile: &Profile, faces: &[FaceInput], spine: &Wire, mode: SweepMode, closed: bool) -> Result<Vec<Tagged>> {
        let mut out = Vec::new();
        if !profile.regions.is_empty() {
            for (i, (face, region)) in profile_faces(&profile.plane, &profile.regions)?
                .iter()
                .zip(&profile.regions)
                .enumerate()
            {
                let shell_mode = match mode {
                    SweepMode::Binormal(_) => Some(mode),
                    SweepMode::CorrectedFrenet | SweepMode::Frenet if closed => Some(mode),
                    _ => None,
                };
                if let Some(m) = shell_mode {
                    out.push(self.shell_sweep_region(&profile.plane, region, i, spine, m)?);
                    continue;
                }
                let (solid, h) = spine.try_pipe_h(&Shape::from(face), mode).map_err(occt)?;
                let origins = sweep_origins(face, &profile.plane, i, region, &solid, &h)?;
                out.push((solid, origins));
            }
        }
        for input in faces {
            let body = self.body(input.body)?;
            let face = faces_of(body)
                .into_iter()
                .nth(input.face.0 as usize)
                .ok_or_else(|| KernelError::OperationFailed(format!("unknown {:?}", input.face)))?;
            let (solid, h) = spine.try_pipe_h(&Shape::from(&face), mode).map_err(occt)?;
            let mut origins = vec![None; face_count(&solid)?];
            let body_edges = Edges::of(body);
            let face_edges = Edges::of_face(&face);
            for (i, generated) in h.edges.iter().enumerate() {
                let Some(edge) = face_edges.list.get(i).and_then(|e| body_edges.index(e)) else {
                    continue;
                };
                for &f in generated {
                    if let Some(o) = origins.get_mut(f) {
                        o.get_or_insert(Origin::FromEdge { body: input.body, edge: EdgeId(edge as u64) });
                    }
                }
            }
            for (list, origin) in [
                (&h.first, Origin::StartCap { region: input.source }),
                (&h.last, Origin::EndCap { region: input.source }),
            ] {
                for &f in list {
                    if let Some(o) = origins.get_mut(f) {
                        o.get_or_insert(origin);
                    }
                }
            }
            out.push((solid, origins));
        }
        if out.is_empty() {
            return Err(KernelError::InvalidProfile("nothing to sweep".into()));
        }
        Ok(out)
    }

    /// Sheets swept by the profile's curves (region loops and open chains).
    fn sweep_curve_sheets(&self, profile: &Profile, spine: &Wire, mode: SweepMode) -> Result<Vec<Tagged>> {
        let plane = profile.plane;
        let mut out = Vec::new();
        let mut add = |curves: Vec<crate::Curve2>, closed: bool, region: u64, lp: usize| -> Result<()> {
            let wire = if closed { loop_wire(&plane, &curves)? } else { chain_wire(&plane, &curves)? };
            let (shape, h) = match mode {
                SweepMode::Binormal(_) => spine.try_pipe_shell_h(&wire, mode, false).map_err(occt)?,
                _ => spine.try_pipe_h(&wire.to_shape(), mode).map_err(occt)?,
            };
            let edges = wire.edges_geometry().map_err(occt)?;
            let ids: Vec<(u64, Vec<Point3<f64>>)> = curves
                .iter()
                .enumerate()
                .map(|(ci, c)| {
                    let id = c.source().unwrap_or_else(|| crate::unsourced_curve(lp, ci));
                    (id, (0..=32).map(|k| plane.to_model(c.point_at(k as f64 / 32.0))).collect())
                })
                .collect();
            let mut tags = vec![None; face_count(&shape)?];
            edge_origins(&edges, &h.edges, &ids, region, &mut tags);
            out.push((shape, tags));
            Ok(())
        };
        for (i, region) in profile.regions.iter().enumerate() {
            let id = region.source.unwrap_or(i as u64);
            add(oriented(&region.outer, true)?, true, id, 0)?;
            for (k, hole) in region.holes.iter().enumerate() {
                add(oriented(hole, false)?, true, id, k + 1)?;
            }
        }
        for (i, c) in profile.chains.iter().enumerate() {
            let id = c.source.unwrap_or((profile.regions.len() + i) as u64);
            add(c.curves.clone(), false, id, 0)?;
        }
        if out.is_empty() {
            return Err(KernelError::InvalidProfile("nothing to sweep".into()));
        }
        Ok(out)
    }

    // -----------------------------------------------------------------------------------------
    // Loft

    pub(super) fn loft_full(&mut self, spec: &LoftSpec) -> Result<OpResult> {
        if spec.sections.len() < 2 {
            return Err(invalid("Select at least two profiles to loft"));
        }
        for (i, s) in spec.sections.iter().enumerate() {
            if matches!(s, LoftSection::Point(_)) && i != 0 && i + 1 != spec.sections.len() {
                return Err(invalid("A point can only be the first or last loft profile"));
            }
        }
        for e in [&spec.start, &spec.end] {
            if e.condition != LoftCondition::Default && !(e.magnitude.is_finite() && e.magnitude > 0.0) {
                return Err(invalid("A loft end's magnitude must be greater than zero"));
            }
        }
        let conditioned = spec.start.condition != LoftCondition::Default || spec.end.condition != LoftCondition::Default;
        if conditioned {
            self.loft_interpolated(spec)
        } else {
            self.loft_thru(spec)
        }
    }

    /// A section as a wire (or vertex) for ThruSections, with the curve ids of its edges.
    fn section_shape(&self, s: &LoftSection) -> Result<(Shape, Vec<u64>)> {
        match s {
            LoftSection::Profile(p) => {
                let curves = section_loop(p)?;
                let ids = curves
                    .iter()
                    .enumerate()
                    .map(|(i, c)| c.source().unwrap_or_else(|| crate::unsourced_curve(0, i)))
                    .collect();
                Ok((loop_wire(&p.plane, &curves)?.to_shape(), ids))
            }
            LoftSection::Face(f) => {
                let body = self.body(f.body)?;
                let face = faces_of(body)
                    .into_iter()
                    .nth(f.face.0 as usize)
                    .ok_or_else(|| KernelError::OperationFailed(format!("unknown {:?}", f.face)))?;
                // Its outer wire: the one with the largest box.
                let wires = Shape::from(&face).sub_shapes(SubKind::Wire).map_err(occt)?;
                let size = |w: &Shape| w.bbox().map(|(a, b)| (b - a).length()).unwrap_or(0.0);
                let outer = wires
                    .into_iter()
                    .max_by(|a, b| size(a).total_cmp(&size(b)))
                    .ok_or_else(|| KernelError::OperationFailed("the face has no boundary".into()))?;
                let n = outer.sub_count(SubKind::Edge).map_err(occt)?;
                Ok((outer, (0..n as u64).map(|i| crate::unsourced_curve(0, i as usize)).collect()))
            }
            LoftSection::Point(p) => Ok((Shape::try_vertex(to_glam(p.coords)).map_err(occt)?, Vec::new())),
        }
    }

    /// A loft through the sections without end conditions (`BRepOffsetAPI_ThruSections`).
    fn loft_thru(&mut self, spec: &LoftSpec) -> Result<OpResult> {
        let mut shapes = Vec::new();
        let mut ids = Vec::new();
        for s in &spec.sections {
            let (shape, i) = self.section_shape(s)?;
            shapes.push(shape);
            ids.push(i);
        }
        let solid = spec.body == BodyKind::Solid;
        // P3.10 (PS20.1): a non-planar face can't be capped flat; the loft's sides are sewn to
        // the face itself.
        let curved_ends = self.curved_end_faces(spec)?;
        let (shape, h) = Shape::try_thru_sections_h(&shapes, solid && curved_ends.is_empty(), false, false, 0).map_err(occt)?;
        let sewn = solid && !curved_ends.is_empty();
        let shape = if sewn { self.sew_with_ends(&shape, spec, 1e-6)? } else { shape };
        let mut tags: Vec<Option<Origin>> = vec![None; face_count(&shape)?];
        // The first section with edges names the side faces.
        let mut offset = 0;
        for (si, s) in shapes.iter().enumerate() {
            let n = s.sub_count(SubKind::Edge).map_err(occt)?;
            if n > 0 {
                for (k, made) in h.edges.iter().skip(offset).take(n).enumerate() {
                    let curve = ids[si].get(k).copied().unwrap_or_else(|| crate::unsourced_curve(0, k));
                    for &f in made {
                        if let Some(t) = tags.get_mut(f) {
                            t.get_or_insert(Origin::ProfileCurve { region: spec.source, curve });
                        }
                    }
                }
                break;
            }
            offset += n;
        }
        for (list, origin) in [
            (&h.first, Origin::StartCap { region: spec.source }),
            (&h.last, Origin::EndCap { region: spec.source }),
        ] {
            for &f in list {
                if let Some(t) = tags.get_mut(f) {
                    t.get_or_insert(origin);
                }
            }
        }
        let shape = match spec.body {
            BodyKind::Thin { left, right } => self.thicken(&shape, left, right)?,
            _ => shape,
        };
        let n = face_count(&shape)?;
        tags.resize(n, None);
        if matches!(spec.body, BodyKind::Thin { .. }) || sewn {
            tags = self.loft_tags_by_geometry(&shape, spec, &[])?;
        }
        self.insert(shape, generated(tags))
    }

    /// A loft with end conditions, interpolated through sampled sections.
    fn loft_interpolated(&mut self, spec: &LoftSpec) -> Result<OpResult> {
        let mut secs = Vec::new();
        for s in &spec.sections {
            match s {
                LoftSection::Profile(p) => secs.push(SectionCurves::new(p.plane, section_loop(p)?)),
                LoftSection::Face(f) => secs.push(self.face_section(f)?),
                LoftSection::Point(_) => return Err(invalid("End conditions need profiles at both ends, not points")),
            }
        }
        let matching = |c: LoftCondition| matches!(c, LoftCondition::MatchTangent | LoftCondition::MatchCurvature);
        for (end, section) in [(&spec.start, spec.sections.first()), (&spec.end, spec.sections.last())] {
            if matching(end.condition) && !matches!(section, Some(LoftSection::Face(_))) {
                return Err(invalid("Match tangent and Match curvature need a face as the profile (its neighbouring faces)"));
            }
        }
        let samples = sample_sections(&secs)?;
        let k = samples.centers.len();
        // The sections along [0, 1] by the length through their centres.
        let mut v = vec![0.0];
        for i in 1..k {
            let d = (samples.centers[i] - samples.centers[i - 1]).norm();
            v.push(v[i - 1] + d);
        }
        let total = v[k - 1];
        if total < 1e-9 {
            return Err(invalid("The loft's profiles lie on top of each other"));
        }
        let vparams: Vec<f64> = v.iter().map(|x| x / total).collect();
        let radius = |i: usize| -> f64 {
            let c = samples.centers[i];
            let pts: Vec<&Point3<f64>> = samples.patches.iter().flat_map(|p| p[i].iter()).collect();
            pts.iter().map(|p| (*p - c).norm()).sum::<f64>() / pts.len().max(1) as f64
        };
        let derivative = |this: &Self, end: &LoftEnd, i: usize, toward: usize| -> Result<LoftDerivative> {
            let n = samples.normals[i];
            let len = end.magnitude * total;
            Ok(match end.condition {
                LoftCondition::Default => LoftDerivative::Free,
                LoftCondition::NormalToProfile => LoftDerivative::Vector(to_glam(n * len)),
                LoftCondition::TangentToProfile => {
                    let sign = if radius(toward) < radius(i) { -1.0 } else { 1.0 };
                    LoftDerivative::Radial {
                        normal: to_glam(n),
                        center: to_glam(samples.centers[i].coords),
                        length: sign * len,
                    }
                }
                LoftCondition::NormalDirection(d) | LoftCondition::TangentDirection(d) => {
                    let d = Vector3::new(d[0], d[1], d[2]);
                    if d.norm() < 1e-12 || !d.iter().all(|x| x.is_finite()) {
                        return Err(invalid("A loft direction needs a non-zero vector"));
                    }
                    // Along the loft, as the section's normal is: the normal where the direction
                    // is square to it, else the way from the section towards its neighbour.
                    let along = if n.dot(&d).abs() > 1e-9 {
                        n
                    } else {
                        let c = samples.centers[toward] - samples.centers[i];
                        if i == 0 { c } else { -c }
                    };
                    let d = if along.dot(&d) < 0.0 { -d.normalize() } else { d.normalize() };
                    if matches!(end.condition, LoftCondition::NormalDirection(_)) {
                        LoftDerivative::Vector(to_glam(d * len))
                    } else {
                        let sign = if radius(toward) < radius(i) { -1.0 } else { 1.0 };
                        LoftDerivative::Radial { normal: to_glam(d), center: to_glam(samples.centers[i].coords), length: sign * len }
                    }
                }
                LoftCondition::MatchTangent | LoftCondition::MatchCurvature => {
                    let Some(LoftSection::Face(f)) = spec.sections.get(i) else {
                        return Err(invalid("Match tangent needs a face as the profile"));
                    };
                    let curvature = end.condition == LoftCondition::MatchCurvature;
                    this.match_derivatives(f, &samples.patches, i, i == 0, len, samples.periodic, curvature)?
                }
            })
        };
        let start = derivative(self, &spec.start, 0, 1)?;
        let mut end = derivative(self, &spec.end, k - 1, k - 2)?;
        // At the end the derivative still points along the loft (out of the last section); a
        // tangent condition flares the other way round.
        if let LoftDerivative::Radial { normal, center, length } = end {
            end = LoftDerivative::Radial { normal, center, length: -length };
        }
        let points: Vec<Vec<Vec<DVec3>>> = samples
            .patches
            .iter()
            .map(|p| p.iter().map(|s| s.iter().map(|q| to_glam(q.coords)).collect()).collect())
            .collect();
        let solid = spec.body == BodyKind::Solid;
        // A face end is capped by the face itself (sewn to the loft's side), planar or not: a
        // flat cap made from the interpolated section would lie a hair off the face, and an Add
        // onto the face's part would leave slivers.
        let face_ends = [spec.sections.first(), spec.sections.last()].into_iter().flatten().any(|s| matches!(s, LoftSection::Face(_)));
        let shape = Shape::try_loft_solid(&points, samples.periodic, &vparams, start, end, solid && !face_ends).map_err(occt)?;
        let shape = if solid && face_ends { self.sew_with_ends(&shape, spec, 1e-4)? } else { shape };
        let shape = match spec.body {
            BodyKind::Thin { left, right } => self.thicken(&shape, left, right)?,
            _ => shape,
        };
        let centers: Vec<(Point3<f64>, Option<u64>)> = samples
            .patches
            .iter()
            .zip(&samples.sources)
            .map(|(p, s)| {
                let pts: Vec<&Point3<f64>> = p.iter().flatten().collect();
                let c = pts.iter().fold(Vector3::zeros(), |a, q| a + q.coords) / pts.len().max(1) as f64;
                (Point3::from(c), *s)
            })
            .collect();
        let tags = self.loft_tags_by_geometry(&shape, spec, &centers)?;
        self.insert(shape, generated(tags))
    }

    /// A face of a body as a loft section (P3.10): its outer loop of edges, each sampled
    /// densely, in loop order, with the edge's id as the source; the plane is the face's (a
    /// planar face) or the loop's best fit (Newell's normal through its centre).
    fn face_section(&self, f: &FaceInput) -> Result<SectionCurves> {
        let shape = self.body(f.body)?;
        let infos = self.edges(f.body)?;
        let mut pieces: Vec<Polyline> = Vec::new();
        for e in infos.iter().filter(|e| e.faces.contains(&Some(f.face)) && e.faces[0] != e.faces[1]) {
            // Dense enough that the chords stay within a few µm of a curve (a circle r 10 in 4096
            // pieces: sagitta 3·10⁻⁶ mm); the loft then samples along them by length.
            let n = if e.curve == crate::CurveKind::Line { 1 } else { 4096 };
            let pts = shape.edge_samples(e.id.0 as usize, n).map_err(occt)?;
            pieces.push((pts.iter().map(|p| Point3::from(to_na(*p))).collect(), Some(e.id.0)));
        }
        if pieces.is_empty() {
            return Err(invalid("The face has no boundary"));
        }
        // Chain them into loops, end to start.
        let tol = 1e-5;
        let mut loops: Vec<Vec<Polyline>> = Vec::new();
        while let Some(first) = pieces.pop() {
            let mut lp = vec![first];
            loop {
                let tail = *lp[lp.len() - 1].0.last().expect("samples");
                let head = lp[0].0[0];
                if (tail - head).norm() < tol && lp.len() > 1 || (lp.len() == 1 && (tail - head).norm() < tol) {
                    break;
                }
                let Some(i) = pieces
                    .iter()
                    .position(|(p, _)| (p[0] - tail).norm() < tol || (p[p.len() - 1] - tail).norm() < tol)
                else {
                    break;
                };
                let (mut p, src) = pieces.remove(i);
                if (p[0] - tail).norm() >= tol {
                    p.reverse();
                }
                lp.push((p, src));
            }
            loops.push(lp);
        }
        let size = |lp: &Vec<Polyline>| {
            let pts: Vec<&Point3<f64>> = lp.iter().flat_map(|(p, _)| p.iter()).collect();
            let mut lo = *pts[0];
            let mut hi = *pts[0];
            for p in &pts {
                lo = lo.inf(p);
                hi = hi.sup(p);
            }
            (hi - lo).norm()
        };
        let outer = loops
            .into_iter()
            .max_by(|a, b| size(a).total_cmp(&size(b)))
            .ok_or_else(|| invalid("The face has no boundary"))?;
        let info = self
            .faces(f.body)?
            .into_iter()
            .find(|i| i.id == f.face)
            .ok_or_else(|| KernelError::OperationFailed(format!("unknown {:?}", f.face)))?;
        let plane = match info.plane {
            Some(p) => p,
            None => {
                let pts: Vec<Point3<f64>> = outer.iter().flat_map(|(p, _)| p.iter().copied()).collect();
                let c = Point3::from(pts.iter().fold(Vector3::zeros(), |a, p| a + p.coords) / pts.len() as f64);
                let mut n = Vector3::zeros();
                for i in 0..pts.len() {
                    let (a, b) = (pts[i] - c, pts[(i + 1) % pts.len()] - c);
                    n += a.cross(&b);
                }
                // The side the face's material normal points to.
                let probe = shape.face_derivatives(f.face.0 as usize, &[to_glam(info.center.coords)]).map_err(occt)?;
                let outward = probe.first().map(|d| to_na(d.normal)).unwrap_or(n);
                if n.dot(&outward) < 0.0 {
                    n = -n;
                }
                let n = nalgebra::Unit::try_new(n, 1e-12).ok_or_else(|| invalid("The face's boundary is degenerate"))?;
                let t = if n.x.abs() < 0.9 { Vector3::x() } else { Vector3::y() };
                let x = nalgebra::Unit::new_normalize(t - n.into_inner() * t.dot(&n));
                Plane { origin: c, x_dir: x, normal: n }
            }
        };
        Ok(SectionCurves::sampled(plane, outer))
    }

    /// Match tangent / Match curvature (P3.10, PS20.4) at a face section: at every sample of
    /// the section, the direction in the neighbouring face's tangent plane square to the
    /// section's boundary (continuing that face past the edge, along the loft), `len` long; with
    /// `curvature` also the second derivative giving the loft the neighbouring face's normal
    /// curvature in that direction (`κ len² n`).
    #[allow(clippy::too_many_arguments)]
    fn match_derivatives(
        &self,
        f: &FaceInput,
        patches: &[Vec<Vec<Point3<f64>>>],
        section: usize,
        start: bool,
        len: f64,
        periodic: bool,
        curvature: bool,
    ) -> Result<LoftDerivative> {
        let shape = self.body(f.body)?;
        let infos = self.edges(f.body)?;
        let boundary: Vec<(&crate::EdgeInfo, Vec<Point3<f64>>)> = infos
            .iter()
            .filter(|e| e.faces.contains(&Some(f.face)) && e.faces[0] != e.faces[1])
            .map(|e| {
                let pts = shape.edge_samples(e.id.0 as usize, 32).unwrap_or_default();
                (e, pts.iter().map(|p| Point3::from(to_na(*p))).collect())
            })
            .collect();
        let neighbour = |p: &Point3<f64>| -> Option<FaceId> {
            let dist = |pts: &Vec<Point3<f64>>| pts.windows(2).map(|w| super::segment_distance(*p, w[0], w[1])).fold(f64::MAX, f64::min);
            let (e, _) = boundary.iter().min_by(|a, b| dist(&a.1).total_cmp(&dist(&b.1)))?;
            e.faces.iter().flatten().copied().find(|x| *x != f.face)
        };
        let mut first: Vec<Vec<DVec3>> = Vec::new();
        let mut second: Vec<Vec<DVec3>> = Vec::new();
        for patch in patches {
            let pts = &patch[section];
            let n = pts.len();
            // The profile face's own normal and the neighbouring faces' derivatives.
            let own = shape
                .face_derivatives(f.face.0 as usize, &pts.iter().map(|p| to_glam(p.coords)).collect::<Vec<_>>())
                .map_err(occt)?;
            // The neighbouring face of each sample, and its derivatives there (one query per face).
            let mut near: Vec<FaceId> = Vec::with_capacity(n);
            for p in pts {
                near.push(neighbour(p).ok_or_else(|| invalid("A profile face's edge has no neighbouring face to match"))?);
            }
            let mut derivs: Vec<Option<opencascade::safe::FaceDerivatives>> = vec![None; n];
            let mut faces: Vec<FaceId> = near.clone();
            faces.sort_by_key(|x| x.0);
            faces.dedup();
            for face in faces {
                let idx: Vec<usize> = (0..n).filter(|&j| near[j] == face).collect();
                let q: Vec<DVec3> = idx.iter().map(|&j| to_glam(pts[j].coords)).collect();
                let got = shape.face_derivatives(face.0 as usize, &q).map_err(occt)?;
                for (j, d) in idx.into_iter().zip(got) {
                    derivs[j] = Some(d);
                }
            }
            let mut d1 = Vec::with_capacity(n);
            let mut d2 = Vec::with_capacity(n);
            for j in 0..n {
                let (a, b) = if periodic {
                    (pts[(j + n - 1) % n], pts[(j + 1) % n])
                } else {
                    (pts[j.saturating_sub(1)], pts[(j + 1).min(n - 1)])
                };
                let t = (b - a).try_normalize(1e-12).unwrap_or_else(Vector3::x);
                let nd = derivs[j].ok_or_else(|| invalid("The neighbouring face could not be evaluated"))?;
                let na = to_na(nd.normal);
                let mut d = t.cross(&na);
                if d.norm() < 1e-9 {
                    return Err(invalid("A neighbouring face is tangent to the profile face; nothing to match"));
                }
                d.normalize_mut();
                let nf = own.get(j).map(|x| to_na(x.normal)).unwrap_or_else(Vector3::z);
                // Leaving the first face (away from its body), arriving at the last (into it).
                let away = d.dot(&nf);
                if (start && away < 0.0) || (!start && away > 0.0) {
                    d = -d;
                }
                d1.push(to_glam(d * len));
                if curvature {
                    let kappa = nd.normal_curvature(to_glam(d));
                    d2.push(to_glam(na * (kappa * len * len)));
                }
            }
            first.push(d1);
            second.push(d2);
        }
        Ok(LoftDerivative::Samples { first, second: curvature.then_some(second) })
    }

    /// The loft's end sections that are non-planar faces (their caps are the faces).
    fn curved_end_faces(&self, spec: &LoftSpec) -> Result<Vec<FaceInput>> {
        let mut out = Vec::new();
        for s in [spec.sections.first(), spec.sections.last()].into_iter().flatten() {
            if let LoftSection::Face(f) = s {
                let info = self.faces(f.body)?.into_iter().find(|i| i.id == f.face);
                if info.is_some_and(|i| i.plane.is_none()) {
                    out.push(*f);
                }
            }
        }
        Ok(out)
    }

    /// The loft's side shell closed with its end sections: planar ends capped by their faces or
    /// profiles, non-planar face ends by copies of the faces, sewn within `tol`.
    fn sew_with_ends(&self, sides: &Shape, spec: &LoftSpec, tol: f64) -> Result<Shape> {
        let mut caps: Vec<Shape> = Vec::new();
        for s in [spec.sections.first(), spec.sections.last()].into_iter().flatten() {
            match s {
                LoftSection::Face(f) => {
                    let body = self.body(f.body)?;
                    let face = faces_of(body)
                        .into_iter()
                        .nth(f.face.0 as usize)
                        .ok_or_else(|| KernelError::OperationFailed(format!("unknown {:?}", f.face)))?;
                    caps.push(Shape::from(&face));
                }
                LoftSection::Profile(p) => {
                    for face in profile_faces(&p.plane, &p.regions)? {
                        caps.push(Shape::from(&face));
                    }
                }
                LoftSection::Point(_) => {}
            }
        }
        let mut all: Vec<&Shape> = vec![sides];
        all.extend(caps.iter());
        let solid = Shape::try_sew_solid(&all, tol).map_err(occt)?;
        let v = solid.mass_properties().volume;
        if v.is_nan() || v <= 0.0 {
            return Err(invalid("The loft's faces don't close up with its end faces"));
        }
        Ok(solid)
    }

    /// Tags a loft's faces by where they lie: planar faces on the first or last section's
    /// plane are its caps; the others are the sides of the first section's curves (the patch
    /// whose samples lie nearest).
    fn loft_tags_by_geometry(&self, shape: &Shape, spec: &LoftSpec, patches: &[(Point3<f64>, Option<u64>)]) -> Result<Vec<Option<Origin>>> {
        let plane_of = |s: &LoftSection| -> Option<Plane> {
            match s {
                LoftSection::Profile(p) => Some(p.plane),
                LoftSection::Face(f) => self.faces(f.body).ok()?.into_iter().find(|i| i.id == f.face)?.plane,
                LoftSection::Point(_) => None,
            }
        };
        let first = spec.sections.first().and_then(plane_of);
        let last = spec.sections.last().and_then(plane_of);
        let on = |pl: &Option<Plane>, info: &crate::FaceInfo| -> bool {
            let (Some(pl), Some(fp)) = (pl, info.plane) else { return false };
            fp.normal.dot(&pl.normal).abs() > 1.0 - 1e-9 && (info.center - pl.origin).dot(&pl.normal).abs() < 1e-6
        };
        let infos = super::face_infos(shape);
        // Non-planar face ends (P3.10) are their own caps: the same area and centre.
        let face_of = |s: Option<&LoftSection>| -> Option<crate::FaceInfo> {
            let Some(LoftSection::Face(f)) = s else { return None };
            self.faces(f.body).ok()?.into_iter().find(|i| i.id == f.face && i.plane.is_none())
        };
        let (curved_first, curved_last) = (face_of(spec.sections.first()), face_of(spec.sections.last()));
        let same = |a: &Option<crate::FaceInfo>, b: &crate::FaceInfo| {
            a.as_ref().is_some_and(|a| {
                (a.area - b.area).abs() < 1e-6 * a.area.max(1.0) && (a.center - b.center).norm() < 1e-4 * a.area.sqrt().max(1.0)
            })
        };
        let mut tags = Vec::with_capacity(infos.len());
        for (i, info) in infos.iter().enumerate() {
            let tag = if same(&curved_first, info) {
                Origin::StartCap { region: spec.source }
            } else if same(&curved_last, info) {
                Origin::EndCap { region: spec.source }
            } else if info.kind == SurfaceKind::Plane && on(&first, info) {
                Origin::StartCap { region: spec.source }
            } else if info.kind == SurfaceKind::Plane && on(&last, info) {
                Origin::EndCap { region: spec.source }
            } else {
                let curve = patches
                    .iter()
                    .min_by(|a, b| (a.0 - info.center).norm().total_cmp(&(b.0 - info.center).norm()))
                    .and_then(|(_, s)| *s)
                    .unwrap_or_else(|| crate::unsourced_curve(0, i));
                Origin::ProfileCurve { region: spec.source, curve }
            };
            tags.push(Some(tag));
        }
        Ok(tags)
    }

    /// A sheet (a loft's surface) thickened into a wall `left` inside and `right` outside.
    fn thicken(&self, sheet: &Shape, left: f64, right: f64) -> Result<Shape> {
        if !(left >= 0.0 && right >= 0.0 && left + right > 1e-6) {
            return Err(invalid("the wall thickness must be greater than zero"));
        }
        let size = |s: &Shape| s.bbox().map(|(a, b)| (b - a).length()).unwrap_or(0.0);
        let base = size(sheet);
        // Which way the sheet's normals point: a small outward offset makes a larger box.
        let probe = sheet.try_thicken_h(1e-3 * (1.0 + base)).map_err(occt)?.0;
        let plus_is_out = size(&probe) > base;
        let side = |d: f64, outward: bool| -> Result<Shape> {
            let signed = if outward == plus_is_out { d } else { -d };
            Ok(sheet.try_thicken_h(signed).map_err(occt)?.0)
        };
        match (left > 1e-9, right > 1e-9) {
            (true, false) => side(left, false),
            (false, true) => side(right, true),
            _ => {
                let a = side(left, false)?;
                let b = side(right, true)?;
                a.try_union(&b).map_err(occt)
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Split

    /// The tool as a shape: a plane as a square larger than `body`, a face as itself, a sheet as
    /// a copy.
    fn split_tool_shape(&self, body: BodyId, tool: &SplitTool) -> Result<Shape> {
        let shape = self.body(body)?;
        let (bmin, bmax) = shape.bbox().map_err(occt)?;
        let size = (bmax - bmin).length().max(1.0);
        let tool_shape = match tool {
            SplitTool::Plane(p) => {
                // A square on the plane, larger than the body, centred on the body's box.
                let n = p.normal.into_inner();
                let m = Point3::from(to_na((bmin + bmax) / 2.0));
                let centre = m - n * (m - p.origin).dot(&n);
                let (u, v) = (p.x_dir.into_inner() * 2.0 * size, p.y_dir().into_inner() * 2.0 * size);
                let corners = [centre - u - v, centre + u - v, centre + u + v, centre - u + v];
                let g = |q: Point3<f64>| to_glam(q.coords);
                let edges = (0..4)
                    .map(|i| Edge::try_segment(g(corners[i]), g(corners[(i + 1) % 4])).map_err(occt))
                    .collect::<Result<Vec<_>>>()?;
                let w = Wire::try_from_edges(&edges).map_err(occt)?;
                Shape::from(&opencascade::primitives::Face::try_from_wires(&w, &[]).map_err(occt)?)
            }
            SplitTool::Face { body: b, face } => {
                let s = self.body(*b)?;
                let f = faces_of(s)
                    .into_iter()
                    .nth(face.0 as usize)
                    .ok_or_else(|| KernelError::OperationFailed(format!("unknown {face:?}")))?;
                Shape::from(&f)
            }
            SplitTool::Body(b) => super::extrude::clone(self.body(*b)?),
        };
        Ok(tool_shape)
    }

    pub(super) fn split_faces_of(&mut self, body: BodyId, faces: &[FaceId], tool: &SplitTool) -> Result<OpResult> {
        let tool_shape = self.split_tool_shape(body, tool)?;
        let shape = self.body(body)?;
        let idx: Vec<usize> = faces.iter().map(|f| f.0 as usize).collect();
        let (result, h) = shape
            .try_split_faces_h(&idx, &tool_shape)
            .map_err(|e| invalid(format!("The tool doesn't split the faces ({e:?})")))?;
        let before = face_count(shape)?;
        let after = face_count(&result)?;
        if after <= before {
            return Err(invalid("The tool doesn't split the faces"));
        }
        let mut history = History::default();
        for (i, outs) in h.faces.iter().enumerate().take(before) {
            let input = InputFace { body, face: FaceId(i as u64) };
            if outs.is_empty() {
                history.deleted.push(input);
            }
            for &f in outs {
                history.modified.push((FaceId(f as u64), input));
            }
        }
        self.insert_raw(result, history)
    }

    pub(super) fn split_full(&mut self, body: BodyId, tool: &SplitTool, source: u64) -> Result<OpResult> {
        let tool_shape = self.split_tool_shape(body, tool)?;
        let shape = self.body(body)?;
        let (result, h) = shape.try_split_h(&[tool_shape]).map_err(occt)?;
        let pieces = result.sub_count(SubKind::Solid).map_err(occt)?;
        if pieces < 2 {
            return Err(invalid("The split doesn't cut the part in two"));
        }
        let body_faces = face_count(shape)?;
        let mut history = History::default();
        let mut seen = vec![false; face_count(&result)?];
        for (i, outs) in h.faces.iter().enumerate() {
            if i < body_faces {
                let input = InputFace { body, face: FaceId(i as u64) };
                if outs.is_empty() {
                    history.deleted.push(input);
                }
                for &f in outs {
                    if let Some(s) = seen.get_mut(f) {
                        *s = true;
                    }
                    history.modified.push((FaceId(f as u64), input));
                }
            }
        }
        for (f, s) in seen.iter().enumerate() {
            if !s {
                history.generated.push((FaceId(f as u64), Origin::StartCap { region: source }));
            }
        }
        self.insert_raw(result, history)
    }
}
