//! From a CAM Studio's jobs to toolpaths, a simulated stock and G-code.
//!
//! - **Frames**: a setup's work coordinate system and each work plane ([`work_frame`]): the
//!   origin (the model's, a point of the bodies' or the stock's box, or typed) and the axes.
//! - **Stock** ([`StockShape`]): the job's box or cylinder round its bodies (in the stock's own
//!   directions) or a stock component's body, expressed in any frame ([`stock_in`]).
//! - **Toolpaths** ([`op_input`], [`generate`]): an operation's tool, work plane, the parts and
//!   stock in that plane's coordinates and the parameters, handed to `cadrs_cam`'s strategies;
//!   [`op_key`] identifies an input so the app computes each toolpath once (the job cache).
//! - **Verify** ([`simulate`]): a setup's operations run over its stock.
//! - **Post** ([`nc_program`], [`post_setup`]): a setup's toolpaths in its datum (or in work
//!   planes' own datums, with Advanced work planes), with tool numbers, coolant and stops, through
//!   the job machine's post.

use std::collections::HashMap;

use cadrs_cam::geom::{Aabb, Frame, P3, add, dot, normalize, sub};
use cadrs_cam::model::Model;
use cadrs_cam::post::WorkOffset;
use cadrs_cam::sim::{self, MillResult, SheetResult, SimOp};
use cadrs_cam::strategy::Stock;
use cadrs_cam::toolpath::{Step, Toolpath};
use cadrs_cam::{Machine, NcOperation, NcProgram};

use super::{
    CamId, CamStudio, Job, OpKind, Origin, Role, Setup, StockKind, WorkPlane, WorkPlaneType,
    box_point,
};
use crate::parts::Part;

/// The job's bodies among the studio's built parts: the picked ones, else every part
/// component's.
pub fn job_bodies<'a>(cam: &CamStudio, job: &Job, parts: &'a [Part]) -> Vec<&'a Part> {
    parts
        .iter()
        .filter(|p| {
            if job.bodies.is_empty() {
                cam.component_of(p.id).is_some_and(|c| c.role == Role::Part)
            } else {
                job.bodies.contains(&p.id)
            }
        })
        .collect()
}

/// The job's workholding bodies.
pub fn job_workholding<'a>(job: &Job, parts: &'a [Part]) -> Vec<&'a Part> {
    parts
        .iter()
        .filter(|p| job.workholdings.contains(&p.id))
        .collect()
}

/// The workholding's triangles in `frame`'s coordinates (from the display meshes: obstacles,
/// so their tessellation's few hundredths don't matter beside the clearances).
pub fn workholding_triangles(job: &Job, parts: &[Part], frame: &Frame) -> Vec<[P3; 3]> {
    let mut out = Vec::new();
    for p in job_workholding(job, parts) {
        let s = &p.solid;
        out.extend(s.indices.chunks(3).map(|t| {
            [
                frame.to_local(s.positions[t[0] as usize]),
                frame.to_local(s.positions[t[1] as usize]),
                frame.to_local(s.positions[t[2] as usize]),
            ]
        }));
    }
    out
}

/// The box of parts' display meshes in `frame`'s coordinates.
pub fn parts_box(parts: &[&Part], frame: &Frame) -> Aabb {
    Aabb::of(
        parts
            .iter()
            .flat_map(|p| p.solid.positions.iter().map(|q| frame.to_local(*q))),
    )
}

/// The job's stock in model coordinates.
#[derive(Debug, Clone, PartialEq)]
pub enum StockShape {
    /// A box in a frame's coordinates.
    Box { frame: Frame, b: Aabb },
    /// A cylinder about a frame's Z: its centre (frame XY), radius and Z range.
    Cylinder {
        frame: Frame,
        center: [f64; 2],
        radius: f64,
        z: [f64; 2],
    },
    /// A body's triangles (model coordinates).
    Mesh(Vec<[P3; 3]>),
}

impl StockShape {
    /// Its triangles in model coordinates.
    pub fn triangles(&self) -> Vec<[P3; 3]> {
        match self {
            StockShape::Box { frame, b } => cadrs_cam::model::box_triangles(b)
                .into_iter()
                .map(|t| t.map(|p| frame.to_world(p)))
                .collect(),
            StockShape::Cylinder {
                frame,
                center,
                radius,
                z,
            } => cadrs_cam::model::cylinder_triangles(*center, *radius, z[0], z[1], 96)
                .into_iter()
                .map(|t| t.map(|p| frame.to_world(p)))
                .collect(),
            StockShape::Mesh(t) => t.clone(),
        }
    }

    /// Its box in `frame`'s coordinates.
    pub fn bounds_in(&self, frame: &Frame) -> Aabb {
        Aabb::of(
            self.triangles()
                .iter()
                .flatten()
                .map(|p| frame.to_local(*p)),
        )
    }
}

/// The job's stock (see [`StockShape`]); `None` without bodies.
pub fn stock_shape(cam: &CamStudio, job: &Job, parts: &[Part]) -> Option<StockShape> {
    let bodies = job_bodies(cam, job, parts);
    if let StockKind::Component(id) = job.stock.kind {
        let c = cam.component(id)?;
        let tris: Vec<[P3; 3]> = parts
            .iter()
            .filter(|p| p.id.feature == c.feature_id())
            .flat_map(|p| super::geometry::part_triangles(p, &Frame::WORLD))
            .collect();
        return (!tris.is_empty()).then_some(StockShape::Mesh(tris));
    }
    if bodies.is_empty() {
        return None;
    }
    let frame = job.stock.frame();
    let b = parts_box(&bodies, &frame);
    let o = job.stock.offsets;
    match job.stock.kind {
        StockKind::Box => Some(StockShape::Box {
            frame,
            b: Aabb::new(
                [b.min[0] - o[0], b.min[1] - o[2], b.min[2] - o[4]],
                [b.max[0] + o[1], b.max[1] + o[3], b.max[2] + o[5]],
            ),
        }),
        StockKind::Cylinder => {
            let c = [(b.min[0] + b.max[0]) / 2.0, (b.min[1] + b.max[1]) / 2.0];
            let r = bodies
                .iter()
                .flat_map(|p| p.solid.positions.iter().map(|q| frame.to_local(*q)))
                .map(|p| (p[0] - c[0]).hypot(p[1] - c[1]))
                .fold(0.0, f64::max);
            Some(StockShape::Cylinder {
                frame,
                center: c,
                radius: r + job.stock.radial,
                z: [b.min[2] - o[4], b.max[2] + o[5]],
            })
        }
        StockKind::Component(_) => unreachable!(),
    }
}

/// The stock as a strategy sees it in `frame`: a box when it's one aligned with the frame,
/// else its triangles.
pub fn stock_in(shape: &StockShape, frame: &Frame) -> Stock {
    let bounds = shape.bounds_in(frame);
    match shape {
        StockShape::Box { frame: sf, .. } if aligned(sf, frame) => Stock::block(bounds),
        _ => Stock {
            bounds,
            triangles: Some(
                shape
                    .triangles()
                    .into_iter()
                    .map(|t| t.map(|p| frame.to_local(p)))
                    .collect(),
            ),
        },
    }
}

/// Whether two frames' axes are parallel (each axis to one of the other's).
fn aligned(a: &Frame, b: &Frame) -> bool {
    [a.x, a.y, a.z].iter().all(|u| {
        [b.x, b.y, b.z]
            .iter()
            .any(|v| dot(*u, *v).abs() > 1.0 - 1e-9)
    })
}

/// A work plane's frame in model coordinates (see the module docs).
pub fn work_frame(cam: &CamStudio, job: &Job, wp: &WorkPlane, parts: &[Part]) -> Frame {
    let axes = Frame::from_zx([0.0; 3], normalize(wp.z), wp.x);
    let origin = match wp.origin {
        Origin::World => [0.0; 3],
        Origin::Manual(p) => p,
        Origin::ComponentBox(bp) => {
            let b = parts_box(&job_bodies(cam, job, parts), &axes);
            if b.is_empty() {
                [0.0; 3]
            } else {
                axes.to_world(box_point(&b, bp))
            }
        }
        Origin::StockBox(bp) => match stock_shape(cam, job, parts) {
            Some(s) => axes.to_world(box_point(&s.bounds_in(&axes), bp)),
            None => [0.0; 3],
        },
    };
    Frame { origin, ..axes }
}

/// The frame a toolpath is worked out in: its work plane's, turned to its own tool axis when
/// the Axis tab sets one (the work plane's origin, its X kept as near as it can be).
pub fn toolpath_frame(
    cam: &CamStudio,
    job: &Job,
    wp: &WorkPlane,
    params: &cadrs_cam::Params,
    parts: &[Part],
) -> Frame {
    let wf = work_frame(cam, job, wp, parts);
    match params.axis {
        cadrs_cam::strategy::ToolAxis::WorkPlane => wf,
        cadrs_cam::strategy::ToolAxis::Direction(z) => Frame::from_zx(wf.origin, z, wf.x),
    }
}

/// The setup's work coordinate system.
pub fn setup_frame(cam: &CamStudio, job: &Job, setup: &Setup, parts: &[Part]) -> Frame {
    work_frame(cam, job, &setup.wcs, parts)
}

/// Everything a toolpath needs, in its work plane's coordinates.
pub struct OpInput {
    pub op: CamId,
    pub frame: Frame,
    pub tool: cadrs_cam::Tool,
    pub tool_number: u32,
    pub params: cadrs_cam::Params,
    pub machine: Machine,
    pub stock: Stock,
    /// The parts to send to the kernel: (features, part ids).
    pub parts: Vec<crate::ids::PartId>,
    pub tolerance: f64,
    /// The workholding in the frame: kept clear of.
    pub fixtures: Vec<[P3; 3]>,
}

/// An operation's input, or why it can't be computed.
pub fn op_input(cam: &CamStudio, op: CamId, parts: &[Part]) -> Result<OpInput, String> {
    let (job, _, o) = cam.operation(op).ok_or("The toolpath is gone")?;
    let OpKind::Toolpath(tp) = &o.kind else {
        return Err("Not a toolpath".into());
    };
    let machine = job
        .machine
        .clone()
        .ok_or("Create the job's machine first")?;
    let tool = cam
        .tool_for(op)
        .ok_or("Create a tool above the toolpath first")?;
    let (_, wp) = cam.work_plane_for(op).ok_or("The toolpath is gone")?;
    let frame = toolpath_frame(cam, job, wp, &tp.params, parts);
    let shape = stock_shape(cam, job, parts).ok_or("The job has no bodies")?;
    let bodies = job_bodies(cam, job, parts);
    if bodies.is_empty() {
        return Err("The job has no bodies".into());
    }
    Ok(OpInput {
        op,
        frame,
        tool: tool.tool.clone(),
        tool_number: tool.number,
        params: tp.params.clone(),
        stock: stock_in(&shape, &frame),
        machine,
        parts: bodies.iter().map(|p| p.id).collect(),
        tolerance: tp.params.tolerance,
        fixtures: workholding_triangles(job, parts, &frame),
    })
}

/// A key for the studio's parts (its Derived features): worked out once for many
/// [`op_key`]s.
pub fn features_key(features: &[crate::document::Feature]) -> u64 {
    let b = blake3::hash(format!("{features:?}").as_bytes());
    u64::from_le_bytes(b.as_bytes()[..8].try_into().unwrap())
}

/// A key for an operation's input and the parts it machines (`features`: [`features_key`]):
/// the same key, the same toolpath (the job cache: going back to an earlier state recomputes
/// nothing).
pub fn op_key(input: &OpInput, features: u64) -> u64 {
    let mut h = blake3::Hasher::new();
    h.update(format!("{:?}", input.frame).as_bytes());
    h.update(format!("{:?}", input.tool).as_bytes());
    h.update(format!("{:?}", input.params).as_bytes());
    // (And what of the machine `generate` reads: its spindle limit scales speeds and feeds.)
    h.update(format!("{:?}", input.machine.kind).as_bytes());
    h.update(&input.machine.max_spindle.to_le_bytes());
    h.update(format!("{:?}", input.stock).as_bytes());
    h.update(format!("{:?}", input.parts).as_bytes());
    for t in &input.fixtures {
        h.update(format!("{t:?}").as_bytes());
    }
    h.update(&features.to_le_bytes());
    let b = h.finalize();
    u64::from_le_bytes(b.as_bytes()[..8].try_into().unwrap())
}

/// Runs the strategy (the model already in the input's frame).
pub fn generate(
    input: &OpInput,
    model: &Model,
    rest: Option<&cadrs_cam::heightmap::HeightMap>,
) -> Result<Toolpath, String> {
    let strategy_input = cadrs_cam::Input {
        model,
        stock: &input.stock,
        tool: &input.tool,
        machine: input.machine.kind,
        params: &input.params,
        rest,
        fixtures: &input.fixtures,
    };
    let mut tp = cadrs_cam::generate(&strategy_input)?;
    // Not faster than the machine's spindle goes (the feeds follow, keeping the chip load).
    if input.machine.kind.is_milling()
        && let Some(k) = tp.limit_spindle(input.machine.max_spindle)
    {
        tp.warnings.push(format!(
            "The spindle speed ({} rpm) is over the machine's {} rpm: it runs at {} rpm with \
             the feeds slowed to {} % to keep the chip load",
            input.params.speeds.spindle_rpm.round(),
            input.machine.max_spindle.round(),
            input.machine.max_spindle.round(),
            (k * 100.0).round()
        ));
    }
    Ok(tp)
}

/// The heights a toolpath wants exact sections at (ask the kernel for them with the model).
pub fn section_levels(input: &OpInput, model: &Model) -> Vec<f64> {
    cadrs_cam::strategy::section_levels(&cadrs_cam::Input {
        model,
        stock: &input.stock,
        tool: &input.tool,
        machine: input.machine.kind,
        params: &input.params,
        rest: None,
        fixtures: &[],
    })
}

/// How finely the kernel tessellates the parts for a toolpath: at half its tolerance where the
/// tool follows the surfaces (finishing, profiles, cuts), coarser for roughing (a quarter of the
/// stock it leaves: the roughing never comes closer than that).
pub fn model_tolerance(p: &cadrs_cam::Params) -> f64 {
    use cadrs_cam::Pattern::*;
    let fine = (p.tolerance * 0.5).clamp(0.002, 0.05);
    match p.pattern {
        Rough2D | Rough3D | Adaptive => ((p.wall_stock.min(p.floor_stock)) / 4.0).clamp(fine, 0.1),
        Face | Drill | Bore | ThreadMill => 0.05_f64.max(fine),
        _ => fine,
    }
}

/// The model request for an operation (the kernel works it out, [`super::geometry::model`]).
pub fn model_request(
    cam: &CamStudio,
    input: &OpInput,
    levels: Vec<f64>,
) -> super::geometry::ModelRequest {
    super::geometry::ModelRequest {
        features: cam.part_features().to_vec(),
        parts: input.parts.clone(),
        frame: input.frame,
        tolerance: model_tolerance(&input.params),
        levels,
    }
}

/// A toolpath computed in frame `from` moved into frame `to` (their Z axes must agree: a
/// 3-axis machine can't tilt).
pub fn move_toolpath(tp: &Toolpath, from: &Frame, to: &Frame) -> Result<Toolpath, String> {
    if !from.same_z(to) {
        return Err(
            "The work plane tilts the tool: only a 3+2 or 5-axis machine can cut it".into(),
        );
    }
    let rel = to.relative(from);
    let map = |p: P3| rel.to_world(p);
    let map2 = |p: [f64; 2]| {
        let q = rel.to_world([p[0], p[1], 0.0]);
        [q[0], q[1]]
    };
    let steps = tp
        .steps
        .iter()
        .map(|s| match s {
            Step::Rapid(p) => Step::Rapid(map(*p)),
            Step::Feed { to, feed, kind } => Step::Feed {
                to: map(*to),
                feed: *feed,
                kind: *kind,
            },
            Step::Arc {
                to,
                center,
                cw,
                feed,
                kind,
            } => Step::Arc {
                to: map(*to),
                center: map2(*center),
                cw: *cw,
                feed: *feed,
                kind: *kind,
            },
            Step::Drill(c) => {
                let mut c = *c;
                let dz = rel.origin[2];
                c.at = map2(c.at);
                c.r_plane += dz;
                c.bottom += dz;
                c.retract += dz;
                Step::Drill(c)
            }
            other => other.clone(),
        })
        .collect();
    Ok(Toolpath {
        steps,
        warnings: tp.warnings.clone(),
    })
}

/// The toolpaths of a setup, in order, with their tools and frames: what a simulation or post
/// runs. `paths` holds the computed toolpaths by operation; missing ones are an error naming
/// the first.
pub struct SetupRun<'a> {
    pub ops: Vec<RunOp<'a>>,
    pub frame: Frame,
}

pub struct RunOp<'a> {
    pub id: CamId,
    pub name: &'a str,
    pub tool: &'a cadrs_cam::Tool,
    pub tool_number: u32,
    pub posting: &'a cadrs_cam::strategy::Posting,
    /// In the setup's coordinates.
    pub toolpath: Toolpath,
    /// The work plane it was computed in (`None`: the setup's), its type and datum.
    pub plane: Option<(Frame, WorkPlaneType, Option<u32>)>,
    /// In its own work plane's coordinates.
    pub local: &'a Toolpath,
    /// Its work plane's name ("" for the setup's).
    pub plane_name: &'a str,
    /// Its work plane tilts the tool off the setup's Z (3+2): `toolpath` is its moves as
    /// straight lines in the setup's coordinates (to show and time; a height-map simulation
    /// along the setup's Z can't cut it).
    pub tilted: bool,
}

/// Collects a setup's operations (see [`SetupRun`]); `only`: just these operations.
pub fn setup_run<'a>(
    cam: &'a CamStudio,
    job: &'a Job,
    setup: &'a Setup,
    parts: &[Part],
    paths: &'a HashMap<CamId, Toolpath>,
    only: Option<&[CamId]>,
) -> Result<SetupRun<'a>, String> {
    let frame = setup_frame(cam, job, setup, parts);
    let mut ops = Vec::new();
    for o in &setup.operations {
        let OpKind::Toolpath(tp) = &o.kind else {
            continue;
        };
        if o.suppressed || only.is_some_and(|ids| !ids.contains(&o.id)) {
            continue;
        }
        let local = paths
            .get(&o.id)
            .ok_or_else(|| format!("{} isn't calculated: regenerate it first", o.name))?;
        let tool = cam
            .tool_for(o.id)
            .ok_or_else(|| format!("{} has no tool above it", o.name))?;
        let (wid, wp) = cam.work_plane_for(o.id).ok_or("gone")?;
        let plane_name = wid
            .and_then(|w| cam.operation(w))
            .map_or("", |(_, _, op)| op.name.as_str());
        let own_axis = tp.params.axis != cadrs_cam::strategy::ToolAxis::WorkPlane;
        let (toolpath, plane, tilted) = if wid.is_none() && !own_axis {
            (local.clone(), None, false)
        } else {
            let wf = toolpath_frame(cam, job, wp, &tp.params, parts);
            // Its own axis is cut in a tilted working plane (in the work plane's datum
            // when that's the setup's).
            let kind = if own_axis && !wf.same_z(&frame) {
                WorkPlaneType::Dynamic
            } else if wid.is_none() {
                WorkPlaneType::None
            } else {
                wp.kind
            };
            if wf.same_z(&frame) {
                (
                    move_toolpath(local, &wf, &frame).map_err(|e| format!("{}: {e}", o.name))?,
                    Some((wf, kind, wp.datum)),
                    false,
                )
            } else {
                let rapid = job.machine.as_ref().map_or(5_000.0, |m| m.rapid_rate);
                (
                    unroll_toolpath(local, &wf, &frame, rapid),
                    Some((wf, kind, wp.datum)),
                    true,
                )
            }
        };
        ops.push(RunOp {
            id: o.id,
            name: &o.name,
            tool: &tool.tool,
            tool_number: tool.number,
            posting: &tp.params.posting,
            toolpath,
            plane,
            local,
            plane_name,
            tilted,
        });
    }
    Ok(SetupRun { ops, frame })
}

/// A toolpath computed in `from` as straight moves in `to`'s coordinates (any tilt between
/// them): arcs and cycles become the lines they move along.
pub fn unroll_toolpath(tp: &Toolpath, from: &Frame, to: &Frame, rapid_rate: f64) -> Toolpath {
    let rel = to.relative(from);
    let Some(start) = tp.start() else {
        return Toolpath::default();
    };
    let mut steps = vec![Step::Rapid(rel.to_world(start))];
    for seg in tp.motion(start, rapid_rate) {
        let p = rel.to_world(seg.to);
        steps.push(match seg.feed {
            None => Step::Rapid(p),
            Some(feed) => Step::Feed {
                to: p,
                feed,
                kind: seg.kind.unwrap_or(cadrs_cam::toolpath::FeedKind::Cut),
            },
        });
    }
    Toolpath {
        steps,
        warnings: tp.warnings.clone(),
    }
}

/// The stock an operation starts from: as the setups before its setup leave it (each one's
/// toolpaths cut into a height map, carried into the next setup's coordinates: moved, turned
/// about Z or flipped), then its own setup's toolpaths before `upto` (`None`: the setup as it
/// starts), in `frame`'s coordinates on a grid of `cell`. `None` when it can't be carried: a setup tilted against the one before, or a
/// toolpath not calculated yet.
#[allow(clippy::too_many_arguments)]
pub fn stock_before(
    cam: &CamStudio,
    job: &Job,
    setup: CamId,
    upto: Option<CamId>,
    parts: &[Part],
    paths: &HashMap<CamId, Toolpath>,
    frame: &Frame,
    cell: f64,
) -> Option<cadrs_cam::heightmap::HeightMap> {
    let shape = stock_shape(cam, job, parts)?;
    let target = job.setups.iter().position(|s| s.id == setup)?;
    let rapid = job.machine.as_ref().map_or(5_000.0, |m| m.rapid_rate);
    // Setups turned against one another (not just flipped), or tilted toolpaths: the stock is
    // carried as voxels instead.
    let frames: Vec<Frame> = job
        .setups
        .iter()
        .take(target + 1)
        .filter(|s| !s.suppressed)
        .map(|s| setup_frame(cam, job, s, parts))
        .collect();
    let turned = frames
        .windows(2)
        .any(|w| (dot(w[0].z, w[1].z).abs() - 1.0).abs() > 1e-6)
        || frames
            .last()
            .is_some_and(|f| (dot(f.z, frame.z).abs() - 1.0).abs() > 1e-6);
    let tilted = job.setups.iter().take(target + 1).any(|st| {
        st.operations.iter().any(|o| {
            o.toolpath()
                .is_some_and(|t| t.params.axis != cadrs_cam::strategy::ToolAxis::WorkPlane)
                || matches!(&o.kind, OpKind::WorkPlane(w) if w.kind == WorkPlaneType::Dynamic)
        })
    });
    if turned || tilted {
        return stock_before_voxels(
            cam, job, &shape, target, upto, parts, paths, frame, cell, rapid,
        );
    }
    let mut state: Option<(cadrs_cam::heightmap::HeightMap, Frame, f64)> = None;
    for (si, st) in job.setups.iter().enumerate().take(target + 1) {
        if st.suppressed && si < target {
            continue;
        }
        let f = setup_frame(cam, job, st, parts);
        let stock_f = stock_in(&shape, &f);
        let mut map = match state.take() {
            None => stock_f.height_map(cell),
            Some((m, pf, pb)) => m.reframed(&pf, pb, &f, &stock_f.bounds, cell)?,
        };
        let ids: Vec<CamId> = st
            .operations
            .iter()
            .take_while(|o| si < target || Some(o.id) != upto)
            .filter(|o| !o.suppressed && matches!(o.kind, OpKind::Toolpath(_)))
            .map(|o| o.id)
            .collect();
        let cut_here = si < target || upto.is_some();
        if cut_here && !ids.is_empty() {
            let run = setup_run(cam, job, st, parts, paths, Some(&ids)).ok()?;
            for o in run.ops.iter().filter(|o| !o.tilted) {
                let start = o.toolpath.start().unwrap_or([0.0; 3]);
                for s in o.toolpath.motion(start, rapid) {
                    if !s.is_rapid() {
                        map.cut(o.tool, s.from, s.to);
                    }
                }
            }
        }
        state = Some((map, f, stock_f.bounds.min[2]));
    }
    let (map, f, bottom) = state?;
    if f == *frame && (map.cell - cell).abs() < 1e-12 {
        return Some(map);
    }
    map.reframed(&f, bottom, frame, &stock_in(&shape, frame).bounds, cell)
}

/// [`stock_before`] through voxels: each toolpath (in its own frame, tilted or not) cut on a
/// height map of the voxels and carved back into them.
#[allow(clippy::too_many_arguments)]
fn stock_before_voxels(
    cam: &CamStudio,
    job: &Job,
    shape: &StockShape,
    target: usize,
    upto: Option<CamId>,
    parts: &[Part],
    paths: &HashMap<CamId, Toolpath>,
    frame: &Frame,
    cell: f64,
    rapid: f64,
) -> Option<cadrs_cam::heightmap::HeightMap> {
    let tris = shape.triangles();
    let b = Aabb::of(tris.iter().flatten().copied());
    let s = b.size();
    // A voxel a 200th of the stock's longest side (at most a millimetre), no finer than the
    // maps. The voxels err toward material, so a coarse one only costs air cut.
    let vcell = (s[0].max(s[1]).max(s[2]) / 200.0).min(1.0).max(cell);
    let mut vox = cadrs_cam::voxels::Voxels::of_solid(&tris, vcell);
    for (si, st) in job.setups.iter().enumerate().take(target + 1) {
        if st.suppressed && si < target {
            continue;
        }
        let ids: Vec<CamId> = st
            .operations
            .iter()
            .take_while(|o| si < target || Some(o.id) != upto)
            .filter(|o| !o.suppressed && matches!(o.kind, OpKind::Toolpath(_)))
            .map(|o| o.id)
            .collect();
        if ids.is_empty() || (si == target && upto.is_none()) {
            continue;
        }
        let run = setup_run(cam, job, st, parts, paths, Some(&ids)).ok()?;
        for o in &run.ops {
            // In its own frame: the setup's, or its tilted plane's.
            let (of, tp) = match (&o.plane, o.tilted) {
                (Some((wf, _, _)), true) => (*wf, o.local),
                _ => (run.frame, &o.toolpath),
            };
            // Room round the stock for the voxels' turned corners.
            let b = stock_in(shape, &of).bounds;
            let pad = [2.0 * vcell, 2.0 * vcell, 0.0];
            let area = Aabb::new(sub(b.min, pad), add(b.max, pad));
            let mut map = vox.height_map(&of, &area, vcell);
            let start = tp.start().unwrap_or([0.0; 3]);
            for seg in tp.motion(start, rapid) {
                if !seg.is_rapid() {
                    map.cut(o.tool, seg.from, seg.to);
                }
            }
            vox.carve(&of, &map);
        }
    }
    Some(vox.height_map(frame, &stock_in(shape, frame).bounds, cell))
}

/// The motion of a setup's operations (in its coordinates) as the posted program makes it,
/// from `start`: each operation from where the one before ended; where the program changes tool
/// or work offset, up clear over where the tool was, down to the operation's first height and
/// across at it (the post's Z-first entry); else up to the higher of the two heights, across
/// and down.
pub fn run_motions(
    setup: &Setup,
    job: &Job,
    run: &SetupRun,
    start: P3,
) -> Vec<Vec<cadrs_cam::toolpath::Segment>> {
    use cadrs_cam::toolpath::Segment;
    let rapid_rate = job.machine.as_ref().map_or(5_000.0, |m| m.rapid_rate);
    let milling = job.machine.as_ref().is_some_and(|m| m.kind.is_milling());
    // The work offsets the program gives the operations (as posted).
    let datums: Option<Vec<u32>> = nc_program(setup, job, run)
        .ok()
        .map(|p| p.operations.iter().map(|o| o.datum).collect());
    let rapid = |from: P3, to: P3| Segment {
        from,
        to,
        feed: None,
        kind: None,
        beam: false,
        minutes: cadrs_cam::geom::dist(from, to) / rapid_rate.max(1.0),
    };
    let mut at = start;
    let mut tool: Option<u32> = None;
    let mut datum: Option<u32> = None;
    let mut out = Vec::with_capacity(run.ops.len());
    for (k, o) in run.ops.iter().enumerate() {
        let d = datums.as_ref().and_then(|v| v.get(k).copied());
        let new_plane = k == 0 || o.plane != run.ops[k - 1].plane;
        let entering = tool != Some(o.tool_number)
            || o.posting.force_tool_change
            || d != datum
            || (new_plane && (d.is_none() || o.tilted));
        let first = o.toolpath.steps.iter().find_map(|st| match st {
            Step::Rapid(p) => Some(*p),
            _ => None,
        });
        let mut segs = Vec::new();
        if entering
            && milling
            && let Some(f) = first
        {
            let clear = at[2].max(f[2]).max(start[2]);
            let over = [at[0], at[1], clear];
            // Down its tool axis: the setup's Z, or a tilted plane's (3+2, entered from up
            // clear along it, then across in it).
            let down = match o.plane.as_ref().filter(|_| o.tilted) {
                Some((wf, _, _)) => {
                    let pf = run.frame.relative(wf);
                    let (ol, fl) = (pf.to_local(over), pf.to_local(f));
                    pf.to_world([ol[0], ol[1], fl[2]])
                }
                None => [at[0], at[1], f[2]],
            };
            for (a, b) in [(at, over), (over, down), (down, f)] {
                if cadrs_cam::geom::dist(a, b) > 1e-9 {
                    segs.push(rapid(a, b));
                }
            }
            at = f;
        } else if milling
            && let Some(f) = first
            && ((f[0] - at[0]).abs() > 1e-9 || (f[1] - at[1]).abs() > 1e-9)
        {
            // On with the same tool and offset: up to the higher height, across, down (as the
            // post writes it).
            let h = at[2].max(f[2]);
            let (up, over) = ([at[0], at[1], h], [f[0], f[1], h]);
            for (a, b) in [(at, up), (up, over), (over, f)] {
                if cadrs_cam::geom::dist(a, b) > 1e-9 {
                    segs.push(rapid(a, b));
                }
            }
            at = f;
        }
        segs.extend(o.toolpath.motion(at, rapid_rate));
        if let Some(l) = segs.last() {
            at = l.to;
        }
        tool = Some(o.tool_number);
        datum = d;
        out.push(segs);
    }
    out
}

/// What Verify found.
pub enum SimResult {
    Mill(Box<MillResult>),
    Sheet(Box<SheetResult>),
}

/// Simulates operations (their motion in the setup's coordinates) over `stock`, from `start`
/// (the stock the setups and toolpaths before left; `None`: the whole stock): a sheet for
/// beam machines, else a height map, with each tilted operation (3+2, `frames[k]` its own
/// frame in the setup's coordinates) cut on voxels in its frame. Verify runs this.
#[allow(clippy::too_many_arguments)]
pub fn simulate_ops(
    stock: &Stock,
    model: &Model,
    fixtures: &[[P3; 3]],
    start: Option<cadrs_cam::heightmap::HeightMap>,
    ops: &[SimOp],
    frames: &[Option<Frame>],
    beam: bool,
    cell: f64,
) -> SimResult {
    if beam {
        return SimResult::Sheet(Box::new(sim::simulate_sheet(stock, model, ops)));
    }
    let first = start.unwrap_or_else(|| stock.height_map(cell));
    SimResult::Mill(Box::new(if frames.iter().any(Option::is_some) {
        let bottom = stock.bounds.min[2] as f32;
        sim::simulate_mill_tilted(first, bottom, model, fixtures, ops, frames)
    } else {
        sim::simulate_mill_on(first, model, fixtures, ops)
    }))
}

/// Simulates a setup's operations (all of them, or up to and including `upto`) as Verify does:
/// from the stock the setups and toolpaths before them left.
#[allow(clippy::too_many_arguments)]
pub fn simulate(
    cam: &CamStudio,
    job: &Job,
    setup: &Setup,
    run: &SetupRun,
    parts: &[Part],
    paths: &HashMap<CamId, Toolpath>,
    upto: Option<CamId>,
    quality: f64,
) -> Result<SimResult, String> {
    let machine = job.machine.as_ref().ok_or("The job has no machine")?;
    let shape = stock_shape(cam, job, parts).ok_or("The job has no bodies")?;
    let stock = stock_in(&shape, &run.frame);
    let model = super::geometry::model_from_parts(&job_bodies(cam, job, parts), &run.frame);
    let start = [0.0, 0.0, stock.bounds.max[2] + 30.0];
    let mut motions = Vec::new();
    let mut frames = Vec::new();
    for (o, m) in run.ops.iter().zip(run_motions(setup, job, run, start)) {
        motions.push((o.tool, m));
        frames.push(
            o.plane
                .as_ref()
                .filter(|_| o.tilted)
                .map(|(wf, _, _)| run.frame.relative(wf)),
        );
        if upto == Some(o.id) {
            break;
        }
    }
    let ops: Vec<SimOp> = motions
        .iter()
        .map(|(t, m)| SimOp {
            tool: t,
            segments: m,
        })
        .collect();
    let beam = machine.kind.is_cutting();
    let cell = sim::sim_cell(&stock, quality);
    let first = run.ops.first().map(|o| o.id);
    let before = (!beam)
        .then(|| stock_before(cam, job, setup.id, first, parts, paths, &run.frame, cell))
        .flatten();
    let fixtures = workholding_triangles(job, parts, &run.frame);
    Ok(simulate_ops(
        &stock, &model, &fixtures, before, &ops, &frames, beam, cell,
    ))
}

/// The program for a setup's operations (see the module docs).
pub fn nc_program(setup: &Setup, job: &Job, run: &SetupRun) -> Result<NcProgram, String> {
    let machine = job.machine.as_ref().ok_or("The job has no machine")?;
    let s = &machine.post;
    let base = setup.wcs.datum.unwrap_or(1).max(1);
    // A work offset is an origin (it doesn't turn: the moves are posted in the setup's
    // axes), to a micron in the setup's coordinates.
    let key = |f: &Frame| {
        let o = run
            .frame
            .to_local(f.origin)
            .map(|v| (v * 1000.0).round() as i64);
        let b = blake3::hash(format!("{o:?}").as_bytes());
        u64::from_le_bytes(b.as_bytes()[..8].try_into().unwrap())
    };
    let base_key = key(&run.frame);
    // Each work offset is one origin: the setup's own, then the planes' overrides, reserved
    // before the automatic planes take the free numbers after them.
    let mut datums: HashMap<u64, u32> = HashMap::new();
    let mut owner: HashMap<u32, (u64, String)> = HashMap::new();
    owner.insert(
        base,
        (key(&run.frame), format!("{}'s work offset", setup.name)),
    );
    fn claim(
        owner: &mut HashMap<u32, (u64, String)>,
        n: u32,
        k: u64,
        who: &str,
    ) -> Result<(), String> {
        match owner.get(&n) {
            Some((o, _)) if *o == k => Ok(()),
            Some((_, other)) => Err(format!(
                "{who}: work offset {n} (G54 is 1) is already {other}; give it another"
            )),
            None => {
                owner.insert(n, (k, format!("{who}'s work plane")));
                Ok(())
            }
        }
    }
    if s.advanced_work_planes {
        for o in &run.ops {
            if let Some((wf, WorkPlaneType::Absolute, Some(d))) = &o.plane {
                claim(&mut owner, *d, key(wf), o.name)?;
                datums.entry(key(wf)).or_insert(*d);
            }
        }
    }
    let mut operations = Vec::new();
    // The work offsets the program uses: the setup's first, the planes' after.
    let mut offsets = vec![WorkOffset {
        number: base,
        origin: format!("{}'s work coordinate system", setup.name),
        at: [0.0; 3],
        operations: Vec::new(),
    }];
    for o in &run.ops {
        let mut tilt = None;
        let (toolpath, datum) = match &o.plane {
            None => (o.toolpath.clone(), base),
            Some((wf, WorkPlaneType::Dynamic, _)) => {
                if machine.indexing.is_none() {
                    return Err(format!(
                        "{}: a Dynamic work plane needs a machine with indexing (3+2)",
                        o.name
                    ));
                }
                // The control's tilted working plane, in the setup's datum.
                tilt = Some(cadrs_cam::post::Tilt::of(&run.frame.relative(wf)));
                (o.local.clone(), base)
            }
            Some((_, _, _)) if o.tilted => {
                return Err(format!(
                    "{}: its work plane tilts the tool; make it Dynamic to cut it with a 3+2 \
                     machine's tilted working plane",
                    o.name
                ));
            }
            Some((_, WorkPlaneType::None, _)) => (o.toolpath.clone(), base),
            Some((wf, WorkPlaneType::Absolute, d)) => {
                if s.advanced_work_planes {
                    let k = key(wf);
                    let n = match (d, datums.get(&k)) {
                        (Some(d), _) => *d,
                        (None, Some(n)) => *n,
                        // At the setup's own origin: its offset.
                        (None, None) if k == base_key => base,
                        (None, None) => {
                            let mut n = base + 1;
                            while owner.contains_key(&n) {
                                n += 1;
                            }
                            claim(&mut owner, n, k, o.name)?;
                            datums.insert(k, n);
                            n
                        }
                    };
                    // A work offset only moves the origin (G54…G59 don't turn): the plane's
                    // moves in the setup's axes, from the plane's origin.
                    let shifted = Frame {
                        origin: wf.origin,
                        ..run.frame
                    };
                    let tp = move_toolpath(o.local, wf, &shifted)
                        .map_err(|e| format!("{}: {e}", o.name))?;
                    let at = run.frame.to_local(wf.origin);
                    match offsets.iter_mut().find(|w: &&mut WorkOffset| w.number == n) {
                        Some(w) => w.operations.push(o.name.to_string()),
                        None => offsets.push(WorkOffset {
                            number: n,
                            origin: o.plane_name.to_string(),
                            at,
                            operations: vec![o.name.to_string()],
                        }),
                    }
                    (tp, n)
                } else {
                    (o.toolpath.clone(), base)
                }
            }
        };
        operations.push(NcOperation {
            name: o.name.to_string(),
            tool: o.tool.clone(),
            tool_number: o.tool_number,
            toolpath,
            coolant: o.posting.coolant,
            datum,
            stop_before: o.posting.stop_before,
            force_tool_change: o.posting.force_tool_change,
            tilt,
        });
    }
    for op in &operations {
        if op.datum == base {
            offsets[0].operations.push(op.name.clone());
        }
    }
    Ok(NcProgram {
        name: format!("{} {}", job.name, setup.name),
        number: setup.program_number,
        offsets,
        operations,
    })
}

/// The work offsets beyond the setup's own that a setup's program uses, for the operator: "G55:
/// Work plane 1 at X 10 Y 0 Z 0 from G54 (Profile 2)". Empty when it uses just one.
pub fn offset_lines(setup: &Setup, job: &Job, run: &SetupRun) -> Vec<String> {
    let Some(m) = job.machine.as_ref() else {
        return Vec::new();
    };
    let Ok(p) = nc_program(setup, job, run) else {
        return Vec::new();
    };
    if p.offsets.len() < 2 {
        return Vec::new();
    }
    let code = |n: u32| m.dialect.datum(n).unwrap_or_else(|| format!("offset {n}"));
    let first = code(p.offsets[0].number);
    p.offsets
        .iter()
        .skip(1)
        .map(|o| {
            format!(
                "{}: {} at X {} Y {} Z {} from {first} ({})",
                code(o.number),
                o.origin,
                cadrs_cam::tool::fmt_mm(o.at[0]),
                cadrs_cam::tool::fmt_mm(o.at[1]),
                cadrs_cam::tool::fmt_mm(o.at[2]),
                o.operations.join(", ")
            )
        })
        .collect()
}

/// The G-code for a setup's operations.
pub fn post_setup(setup: &Setup, job: &Job, run: &SetupRun) -> Result<String, String> {
    let machine = job.machine.as_ref().ok_or("The job has no machine")?;
    let program = nc_program(setup, job, run)?;
    cadrs_cam::post(&program, machine, &machine.post).map_err(|e| e.to_string())
}

/// Moves outside the machine's travel: the setup's toolpaths placed on the table by its machine
/// mating (the stock's bottom centre on the table's centre, or offset). The Post dialog posts
/// such a program only when the operator says so. Z is the tool tip's span against the Z
/// travel: the tool's length decides where in the travel that span sits (against the room
/// between the table and the spindle, which the machine doesn't record), not how much of it.
pub fn travel_warnings(job: &Job, setup: &Setup, run: &SetupRun, stock: &Stock) -> Vec<String> {
    let Some(m) = &job.machine else {
        return Vec::new();
    };
    let centre = [
        (stock.bounds.min[0] + stock.bounds.max[0]) / 2.0,
        (stock.bounds.min[1] + stock.bounds.max[1]) / 2.0,
        stock.bounds.min[2],
    ];
    let shift = match setup.mating {
        super::Mating::Automatic => [0.0; 3],
        super::Mating::Manual(o) => o,
    };
    let mut out = Vec::new();
    for o in &run.ops {
        let b = o.toolpath.bounds();
        if b.is_empty() {
            continue;
        }
        for i in 0..2 {
            let lo = b.min[i] - centre[i] + shift[i];
            let hi = b.max[i] - centre[i] + shift[i];
            if lo < -m.travel[i] / 2.0 - 1e-6 || hi > m.travel[i] / 2.0 + 1e-6 {
                out.push(format!(
                    "{} runs beyond the machine's {} travel",
                    o.name,
                    ["X", "Y"][i]
                ));
                break;
            }
        }
        if b.max[2] - b.min[2] > m.travel[2] {
            out.push(format!(
                "{} needs more Z travel than the machine has",
                o.name
            ));
        }
    }
    out
}
