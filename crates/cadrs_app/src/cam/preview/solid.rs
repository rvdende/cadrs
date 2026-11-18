//! Verify and Simulate for a run that leans its tool (a rotary or 5-axis pattern, a tilted
//! work plane) or cuts with an undercutting cutter ([`program::run_needs_solid`]): the height
//! map the other runs are cut on is seen down the setup's Z and cannot hold such cuts, so the
//! run is simulated on a solid stock instead ([`program::simulate_solid`], on the analysis
//! worker), which is cut whichever way the tool points.
//!
//! **What is shown.** The stock as the tool leaves it: the worker runs the simulation once and
//! hands out the stock along the way ([`program::simulate_solid_with`]), at [`SNAPSHOTS`]
//! evenly spaced times through the run and at every operation's end, as a mesh coloured by the
//! operation that cut each surface. The preview shows the last one at or before its time (the
//! uncut stock before the first: the stock's own shape, what earlier setups cut from it shows
//! with the first snapshot), and each arrives as soon as the worker is past it, so the cut
//! shows while the rest is still being simulated. The run's end comes again once the check is
//! done, with its gouges in red.
//!
//! **What is found.** The gouges, the rapids into material, the holder in the stock and the
//! tool in the workholding come from the same simulation, where the height map's are listed.
//!
//! Also here: the tool as a solid ([`tool_mesh`], [`cadrs_cam::cutter::cutter_mesh`]) that
//! the preview moves along the run.

use super::*;
use cadrs_core::cam::CamStudio;
use cadrs_core::cam::cadrs_cam::cutter::{ToolMesh, cutter_mesh};
use cadrs_core::cam::cadrs_cam::dexel::SimMesh;
use cadrs_core::cam::cadrs_cam::toolpath::Toolpath;
use cadrs_ui::i18n::{tr, tr_with};

/// How many times through the run the stock is shown as cut (besides each operation's end).
pub(super) const SNAPSHOTS: usize = 120;

/// How fine the solid stock's lattice is, 0 (fast) to 1 (fine).
const QUALITY: f64 = 0.3;

/// What the worker needs to simulate the run on a solid stock.
pub(super) struct SolidInput {
    pub(super) cam: CamStudio,
    pub(super) job: CamId,
    pub(super) setup: CamId,
    /// The operations previewed.
    pub(super) wanted: Vec<CamId>,
    pub(super) parts: Vec<cadrs_core::Part>,
    pub(super) paths: std::collections::HashMap<CamId, Toolpath>,
}

/// The stock after so many of the run's moves, in the setup's coordinates.
pub(super) type Stage = (usize, SimMesh);

/// What the solid simulation gives the preview.
pub(super) struct Solid {
    /// The stock at the run's end, its gouges marked.
    pub(super) end: Stage,
    pub(super) summary: String,
    pub(super) collisions: Vec<(f64, String)>,
}

/// After how many moves of `segments` the stock is shown: [`SNAPSHOTS`] evenly spaced times
/// and each operation's end, ascending, the run's end not among them.
pub(super) fn snapshot_marks(segments: &[Segment], spans: &[OpSpan]) -> Vec<usize> {
    let n = segments.len();
    let tl = Timeline::new(segments.to_vec());
    let total = tl.total();
    let mut marks: Vec<usize> = spans.iter().skip(1).map(|o| o.first).collect();
    let (mut k, mut done) = (0, 0);
    for i in 1..SNAPSHOTS {
        let t = total * i as f64 / SNAPSHOTS as f64;
        // (The moves finished by `t`.)
        while k < n && tl.end_of(k) <= t {
            k += 1;
        }
        if k > done {
            marks.push(k);
            done = k;
        }
    }
    marks.retain(|m| *m > 0 && *m < n);
    marks.sort_unstable();
    marks.dedup();
    marks
}

/// Simulates the run on a solid stock: what the whole run finds and the stock at its end,
/// handing `stage` the stock along the way ([`snapshot_marks`]). `Err`: why it could not be
/// run.
pub(super) fn analyse_solid(
    input: &SolidInput,
    segments: &[Segment],
    spans: &[OpSpan],
    stage: &mut dyn FnMut(Stage),
) -> Result<Solid, String> {
    let cam = &input.cam;
    let job = cam.job(input.job).ok_or("the job is gone")?;
    let setup = job
        .setups
        .iter()
        .find(|s| s.id == input.setup)
        .ok_or("the setup is gone")?;
    let run = program::setup_run(
        cam,
        job,
        setup,
        &input.parts,
        &input.paths,
        Some(&input.wanted),
    )?;
    let marks = snapshot_marks(segments, spans);
    let r = program::simulate_solid_with(
        cam,
        job,
        setup,
        &run,
        &input.parts,
        &input.paths,
        None,
        QUALITY,
        &marks,
        &mut |n, stock| stage((n, stock.mesh(1))),
    )?;
    let end = (segments.len(), r.stock.mesh(1));
    let tl = Timeline::new(segments.to_vec());
    // One of each kind an operation: where it first happens.
    let mut collisions: Vec<(f64, String)> = Vec::new();
    for (hits, what) in [
        (&r.rapid_hits, "cam-collision-rapid-into-stock"),
        (&r.fixture_hits, "cam-collision-tool-into-workholding"),
        (&r.holder_hits, "cam-collision-holder-meets-stock"),
    ] {
        let mut first: Vec<Option<f64>> = vec![None; spans.len()];
        for (k, p) in hits.iter().filter(|(k, _)| *k < spans.len()) {
            let t = super::analysis::time_near(&tl, spans, *k, *p);
            if first[*k].is_none_or(|was| t < was) {
                first[*k] = Some(t);
            }
        }
        for (k, t) in first.iter().enumerate() {
            if let Some(t) = t {
                collisions.push((*t, tr_with(what, &[("operation", &spans[k].name)])));
            }
        }
    }
    collisions.sort_by(|a, b| a.0.total_cmp(&b.0));
    // (Items joined by " · ".)
    let summary = format!(
        "{} · {} · {}{}",
        tr_with("cam-verify-removed", &[("volume", &fmt0(r.removed))]),
        if r.gouge_count == 0 {
            tr("cam-verify-no-gouges")
        } else {
            tr_with(
                "cam-verify-gouges-up-to",
                &[("depth", &fmt2(r.gouge_depth))],
            )
        },
        if collisions.is_empty() {
            tr("cam-verify-no-collisions")
        } else {
            tr_with(
                if collisions.len() == 1 {
                    "cam-verify-collision"
                } else {
                    "cam-verify-collisions"
                },
                &[("count", &collisions.len().to_string())],
            )
        },
        if r.excess > 0.0 {
            format!(
                " · {}",
                tr_with("cam-verify-left-up-to", &[("depth", &fmt2(r.excess))])
            )
        } else {
            String::new()
        }
    );
    Ok(Solid {
        end,
        summary,
        collisions,
    })
}

/// Which of `stages` (by their move counts, ascending) the preview shows with `done` of the
/// run's moves finished: the last at or before it (`None`: the uncut stock).
pub(super) fn stage_shown(stages: &[usize], done: usize) -> Option<usize> {
    stages.iter().rposition(|n| *n <= done)
}

/// A mesh the viewport's part material lights ([`crate::part_shading::PartShading`]): its
/// vertex colours are the surfaces' own, and nothing of it is selected.
fn lit_mesh(
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
) -> Mesh {
    let uv = vec![[0.0f32; 2]; positions.len()];
    Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
    .with_inserted_indices(bevy::mesh::Indices::U32(indices))
}

/// The solid stock (in the setup's coordinates, `frame`) as a mesh in the model's.
pub(super) fn stock_mesh(m: &SimMesh, frame: &Frame) -> Mesh {
    let positions = m
        .positions
        .iter()
        .map(|p| v3(frame.to_world([p[0] as f64, p[1] as f64, p[2] as f64])).to_array())
        .collect();
    let normals = m
        .normals
        .iter()
        .map(|n| v3(frame.dir_to_world([n[0] as f64, n[1] as f64, n[2] as f64])).to_array())
        .collect();
    lit_mesh(positions, normals, m.colors.clone(), m.indices.clone())
}

/// The uncut stock as a solid, from its surface's triangles in the model's coordinates: flat
/// faces in the stock's grey.
pub(super) fn blank_mesh(triangles: &[[P3; 3]]) -> Mesh {
    let mut positions = Vec::with_capacity(triangles.len() * 3);
    let mut normals = Vec::with_capacity(triangles.len() * 3);
    for t in triangles {
        let [a, b, c] = t.map(v3);
        let n = (b - a).cross(c - a).normalize_or_zero().to_array();
        for p in [a, b, c] {
            positions.push(p.to_array());
            normals.push(n);
        }
    }
    let colors = vec![[0.78, 0.80, 0.84, 1.0]; positions.len()];
    let indices = (0..positions.len() as u32).collect();
    lit_mesh(positions, normals, colors, indices)
}

/// A milling tool as a solid, its tip at the origin and its axis along +Z: flutes, shank,
/// holder and arbor each in its colour.
pub(super) fn tool_mesh(tool: &Tool) -> Mesh {
    let ToolMesh {
        positions,
        normals,
        indices,
        colors,
    } = cutter_mesh(tool, 48);
    lit_mesh(positions, normals, colors, indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stock_shown_is_the_last_one_cut_by_now() {
        // Nothing cut yet shows the uncut stock, then the last snapshot at or before now.
        assert_eq!(stage_shown(&[], 5), None);
        assert_eq!(stage_shown(&[3, 10, 20], 0), None);
        assert_eq!(stage_shown(&[3, 10, 20], 3), Some(0));
        assert_eq!(stage_shown(&[3, 10, 20], 19), Some(1));
        assert_eq!(stage_shown(&[3, 10, 20], 25), Some(2));
    }

    #[test]
    fn the_stock_is_shown_through_every_operation() {
        let seg = |x: f64| Segment {
            from: [x, 0.0, 0.0],
            to: [x + 1.0, 0.0, 0.0],
            feed: Some(60.0),
            kind: None,
            beam: false,
            minutes: 0.01,
            axis: None,
        };
        let segments: Vec<Segment> = (0..1000).map(|i| seg(i as f64)).collect();
        let span = |first: usize| OpSpan {
            first,
            name: String::new(),
            tool: Tool::new(cadrs_core::cam::cadrs_cam::tool::ToolKind::EndMill, 6.0),
            tilted: false,
            frame: None,
            tool_number: 1,
            gauge: 0.0,
        };
        let marks = snapshot_marks(&segments, &[span(0), span(7)]);
        // Each operation's end, then every so often through the run, never its end.
        assert!(marks.contains(&7));
        assert!(marks.len() >= SNAPSHOTS - 1, "{}", marks.len());
        assert!(marks.windows(2).all(|w| w[0] < w[1]));
        assert!(marks.iter().all(|m| *m > 0 && *m < 1000));
        assert!(
            marks
                .windows(2)
                .all(|w| w[1] - w[0] <= 1000 / SNAPSHOTS + 1)
        );
    }

    #[test]
    fn the_uncut_stock_is_a_flat_faced_solid() {
        let tris = cadrs_core::cam::cadrs_cam::model::box_triangles(
            &cadrs_core::cam::cadrs_cam::geom::Aabb::new([0.0; 3], [10.0, 20.0, 30.0]),
        );
        let m = blank_mesh(&tris);
        assert_eq!(m.count_vertices(), 36);
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(n)) =
            m.attribute(Mesh::ATTRIBUTE_NORMAL)
        else {
            panic!("normals");
        };
        // Every normal is along an axis, pointing out of the box.
        assert!(
            n.iter()
                .all(|v| v.iter().filter(|x| x.abs() > 0.999).count() == 1)
        );
    }

    #[test]
    fn a_tool_is_a_closed_lit_mesh() {
        let tool = Tool::new(cadrs_core::cam::cadrs_cam::tool::ToolKind::EndMill, 6.0);
        let m = tool_mesh(&tool);
        let n = m.count_vertices();
        assert!(n > 100);
        for a in [
            Mesh::ATTRIBUTE_NORMAL,
            Mesh::ATTRIBUTE_COLOR,
            Mesh::ATTRIBUTE_UV_0,
        ] {
            assert_eq!(m.attribute(a).map(|v| v.len()), Some(n));
        }
        assert_eq!(m.indices().map(|i| i.len() % 3), Some(0));
    }
}
