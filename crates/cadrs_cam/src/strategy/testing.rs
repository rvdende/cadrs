//! Fixtures and checks the strategies' tests share: models, stocks and the gouge check.

use super::*;
use crate::geom::Aabb;
use crate::model::{Face, Surface, box_triangles};

/// A 60 × 40 × 20 block with a 30 × 20 pocket 8 deep in its top (the pocket's floor an exact
/// face), at the origin.
pub fn pocket_block() -> Model {
    let mut tris = Vec::new();
    // The block as four walls around the pocket plus the floor slab: boxes that overlap
    // make a valid shadow for the strategies (they union the upward triangles).
    tris.extend(box_triangles(&Aabb::new(
        [0.0, 0.0, 0.0],
        [60.0, 40.0, 12.0],
    )));
    tris.extend(box_triangles(&Aabb::new(
        [0.0, 0.0, 12.0],
        [15.0, 40.0, 20.0],
    )));
    tris.extend(box_triangles(&Aabb::new(
        [45.0, 0.0, 12.0],
        [60.0, 40.0, 20.0],
    )));
    tris.extend(box_triangles(&Aabb::new(
        [15.0, 0.0, 12.0],
        [45.0, 10.0, 20.0],
    )));
    tris.extend(box_triangles(&Aabb::new(
        [15.0, 30.0, 12.0],
        [45.0, 40.0, 20.0],
    )));
    let mut m = Model::from_triangles("block", tris);
    let sq = |x0: f64, y0: f64, x1: f64, y1: f64, z: f64| {
        vec![[x0, y0, z], [x1, y0, z], [x1, y1, z], [x0, y1, z]]
    };
    m.faces.push(Face {
        name: "floor".into(),
        body: 0,
        surface: Surface::Plane {
            normal: [0.0, 0.0, 1.0],
        },
        loops: vec![sq(15.0, 10.0, 45.0, 30.0, 12.0)],
        triangles: vec![],
    });
    m.faces.push(Face {
        name: "top".into(),
        body: 0,
        surface: Surface::Plane {
            normal: [0.0, 0.0, 1.0],
        },
        loops: vec![
            sq(0.0, 0.0, 60.0, 40.0, 20.0),
            sq(15.0, 10.0, 45.0, 30.0, 20.0),
        ],
        triangles: vec![],
    });
    m
}

/// How far the toolpath's worst move (rapids, links, leads and cuts alike, sampled every
/// 0.05 mm) takes the tool into the model (exact drop-cutter), and where: 0 if never.
pub fn max_gouge(m: &Model, tool: &Tool, tp: &Toolpath) -> (f64, crate::geom::P3) {
    max_gouge_within(m, tool, tp, 0.001)
}

/// [`max_gouge`] allowing `tol` into the surface along its normal (a finishing
/// toolpath's own tolerance): the cutter is shrunk by it all round, so a wall (where a
/// hair sideways is a long way down) counts by how far into it the cutter goes.
pub fn max_gouge_within(m: &Model, tool: &Tool, tp: &Toolpath, tol: f64) -> (f64, crate::geom::P3) {
    let idx = crate::dropcutter::TriIndex::new(&m.triangles);
    let mut scratch = Vec::new();
    let mut tool = tool.clone();
    tool.diameter -= 2.0 * tol;
    if tool.kind == ToolKind::BallMill {
        tool.corner_radius = tool.diameter / 2.0;
    }
    let tool = &tool;
    let start = tp
        .steps
        .iter()
        .find_map(|s| match s {
            crate::toolpath::Step::Rapid(p) => Some(*p),
            _ => None,
        })
        .expect("a rapid");
    let mut worst = (0.0, start);
    for seg in tp.motion(start, 5000.0) {
        let n = (crate::geom::dist(seg.from, seg.to) / 0.05).ceil().max(1.0) as usize;
        for i in 0..=n {
            let q = crate::geom::lerp(seg.from, seg.to, i as f64 / n as f64);
            let low = crate::dropcutter::drop_point(
                tool,
                &idx,
                [q[0], q[1]],
                f64::NEG_INFINITY,
                &mut scratch,
            );
            if low - tol - q[2] > worst.0 {
                worst = (low - tol - q[2], q);
            }
        }
    }
    worst
}

pub fn stock_for(m: &Model, margin: f64, top: f64) -> Stock {
    let b = m.bounds();
    Stock::block(Aabb::new(
        [b.min[0] - margin, b.min[1] - margin, b.min[2]],
        [b.max[0] + margin, b.max[1] + margin, b.max[2] + top],
    ))
}
