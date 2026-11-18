//! Verify and Simulate for a run that leans its tool (a rotary or 5-axis pattern, a tilted
//! work plane) or cuts with an undercutting cutter ([`program::run_needs_solid`]): the height
//! map the other runs are cut on is seen down the setup's Z and cannot hold such cuts, so the
//! run is simulated on a solid stock instead ([`program::simulate_solid`], on the analysis
//! worker), which is cut whichever way the tool points.
//!
//! **What is shown.** The stock as each operation leaves it: the worker simulates the run up
//! to and including every operation in turn (up to [`MAX_STAGES`] of them; a longer run only
//! whole), and the preview shows the stock of the last operation that is finished at its
//! time, coloured by the operation that cut each surface, gouges red. Until the first
//! operation is through (and while the check is still running) the uncut stock is shown (the
//! stock's own shape: what earlier setups cut from it shows with the first stage). So
//! the stock is right at every operation's end and at the run's end, and does not change
//! while an operation is running: the tool moves over the stock that operation started on.
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

/// The most operations whose stock is simulated one by one (each is a run of its own from
/// the start, so the work grows with the square of this).
pub(super) const MAX_STAGES: usize = 8;

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

/// What the solid simulation gives the preview.
pub(super) struct Solid {
    /// The stock after each operation, in the setup's coordinates: one mesh an operation, or
    /// the final one alone for a long run.
    pub(super) stages: Vec<SimMesh>,
    pub(super) summary: String,
    pub(super) collisions: Vec<(f64, String)>,
}

/// Simulates the run on a solid stock: the stock after each operation and what the whole run
/// finds. `Err`: why it could not be run.
pub(super) fn analyse_solid(
    input: &SolidInput,
    segments: &[Segment],
    spans: &[OpSpan],
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
    let ids: Vec<CamId> = run.ops.iter().map(|o| o.id).collect();
    let simulate = |upto: Option<CamId>| {
        program::simulate_solid(
            cam,
            job,
            setup,
            &run,
            &input.parts,
            &input.paths,
            upto,
            QUALITY,
        )
    };
    let mut stages = Vec::new();
    if ids.len() <= MAX_STAGES {
        for id in &ids[..ids.len().saturating_sub(1)] {
            stages.push(simulate(Some(*id))?.stock.mesh(1));
        }
    }
    let r = simulate(None)?;
    stages.push(r.stock.mesh(1));
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
        stages,
        summary,
        collisions,
    })
}

/// Which of `stages` meshes the preview shows with `done` of `ops` operations finished: the
/// stock the last finished one left (`None`: the uncut stock). With the final mesh alone it
/// shows once every operation is through.
pub(super) fn stage_shown(stages: usize, ops: usize, done: usize) -> Option<usize> {
    if stages == 0 || done == 0 {
        None
    } else if stages >= ops {
        Some(done.min(stages) - 1)
    } else {
        (done >= ops).then_some(stages - 1)
    }
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
    fn the_stock_shown_is_the_last_finished_operations() {
        // A mesh an operation: none finished shows the uncut stock, then each in turn.
        assert_eq!(stage_shown(3, 3, 0), None);
        assert_eq!(stage_shown(3, 3, 1), Some(0));
        assert_eq!(stage_shown(3, 3, 3), Some(2));
        // The final mesh alone (a long run): only once the run is through.
        assert_eq!(stage_shown(1, 12, 11), None);
        assert_eq!(stage_shown(1, 12, 12), Some(0));
        assert_eq!(stage_shown(1, 1, 1), Some(0));
        // Nothing simulated yet.
        assert_eq!(stage_shown(0, 3, 3), None);
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
