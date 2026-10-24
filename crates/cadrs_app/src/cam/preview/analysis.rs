//! Verify's check of a run (on a worker thread): the stock cut by every operation in order (the
//! tilted ones on voxels in their own frames), what ran into the stock, the part or the
//! workholding, and the summary line.

use super::*;

/// What simulating the whole run found (worked out off the main thread).
pub(super) struct Analysis {
    pub(super) summary: String,
    pub(super) collisions: Vec<(f64, String)>,
    /// The part on the stock's grid, pulled in by a cell (mills).
    pub(super) part: Option<HeightMap>,
    /// The stock as the toolpaths before the previewed ones leave it (when there are any).
    pub(super) start: Option<HeightMap>,
    /// A cut sheet's pieces, each with whether it's a part cut free.
    pub(super) sheet: Option<Vec<(cadrs_core::cam::cadrs_cam::region::Region, bool)>>,
}

/// Simulates the whole run: the summary line, the collisions' times and (mills) the part on
/// the stock's grid.
#[allow(clippy::too_many_arguments)]
pub(super) fn analyse(
    stock: &Stock,
    model: &cadrs_core::cam::cadrs_cam::model::Model,
    fixtures: &[[P3; 3]],
    start: Option<HeightMap>,
    segments: &[Segment],
    spans: &[OpSpan],
    beam: bool,
    cell: f64,
) -> Analysis {
    // Simulated in the run's order; a tilted operation (3+2) cuts the stock in its own frame.
    let ops: Vec<SimOp> = spans
        .iter()
        .enumerate()
        .map(|(k, sp)| {
            let end = spans.get(k + 1).map_or(segments.len(), |n| n.first);
            SimOp {
                tool: &sp.tool,
                segments: &segments[sp.first..end],
            }
        })
        .collect();
    let frames: Vec<_> = spans.iter().map(|s| s.frame.filter(|_| s.tilted)).collect();
    let r = match program::simulate_ops(
        stock,
        model,
        fixtures,
        start.clone(),
        &ops,
        &frames,
        beam,
        cell,
    ) {
        program::SimResult::Sheet(r) => {
            let summary = format!(
                "Cut {} mm · {} part{} free{}",
                fmt0(r.cut_length),
                r.parts_free,
                if r.parts_free == 1 { "" } else { "s" },
                if r.damage > 0.01 {
                    format!(" · {} mm² cut from the part", fmt1(r.damage))
                } else {
                    String::new()
                }
            );
            let sheet = r
                .pieces
                .iter()
                .zip(&r.part_pieces)
                .map(|(f, part)| (f.rings(), *part))
                .collect();
            return Analysis {
                summary,
                collisions: Vec::new(),
                part: None,
                start: None,
                sheet: Some(sheet),
            };
        }
        program::SimResult::Mill(r) => r,
    };
    let tl = Timeline::new(segments.to_vec());
    let mut collisions = Vec::new();
    for (k, p) in r.rapid_hits.iter() {
        collisions.push((
            time_near(&tl, spans, *k, *p),
            format!("{}: a rapid runs into the stock", spans[*k].name),
        ));
    }
    for (k, p) in r.fixture_hits.iter() {
        collisions.push((
            time_near(&tl, spans, *k, *p),
            format!("{}: the tool runs into the workholding", spans[*k].name),
        ));
    }
    for (k, p) in r.holder_hits.iter() {
        collisions.push((
            time_near(&tl, spans, *k, *p),
            format!("{}: the shank or holder meets the stock", spans[*k].name),
        ));
    }
    // The tilted operations: in their own frames, against the part and the workholding.
    let mut gouge_depth = if r.gouge_cells > 0 {
        r.gouge_depth
    } else {
        0.0
    };
    for (k, sp) in spans.iter().enumerate() {
        let Some(f) = sp.frame else { continue };
        let end = spans.get(k + 1).map_or(segments.len(), |n| n.first);
        let local = |p: P3| f.to_local(p);
        let segs: Vec<Segment> = segments[sp.first..end]
            .iter()
            .map(|s| Segment {
                from: local(s.from),
                to: local(s.to),
                ..*s
            })
            .collect();
        let tris = |t: &[[P3; 3]]| -> Vec<[P3; 3]> {
            t.iter()
                .map(|t| [local(t[0]), local(t[1]), local(t[2])])
                .collect()
        };
        let c = sim::check_against_part(
            &tris(&model.triangles),
            &tris(fixtures),
            &sp.tool,
            &segs,
            0.01,
        );
        if !c.gouges.is_empty() {
            gouge_depth = gouge_depth.max(c.gouge_depth);
        }
        for (hits, what) in [
            (&c.rapid_hits, "a rapid runs through the part"),
            (&c.holder_hits, "the shank or holder meets the part"),
            (&c.fixture_hits, "the tool runs into the workholding"),
        ] {
            for p in hits {
                collisions.push((
                    time_near(&tl, spans, k, f.to_world(*p)),
                    format!("{}: {what}", sp.name),
                ));
            }
        }
    }
    collisions.sort_by(|a, b| a.0.total_cmp(&b.0));
    let summary = format!(
        "Removed {} mm³ · {} · {}{}",
        fmt0(r.removed),
        if gouge_depth <= 0.0 {
            "no gouges".to_string()
        } else {
            format!("gouges up to {} mm deep", fmt2(gouge_depth))
        },
        if collisions.is_empty() {
            "no collisions".to_string()
        } else {
            format!(
                "{} collision{}",
                collisions.len(),
                if collisions.len() == 1 { "" } else { "s" }
            )
        },
        if r.excess > 0.0 {
            format!(" · up to {} mm left", fmt2(r.excess))
        } else {
            String::new()
        }
    );
    let mut part = r.stock.blank();
    part.rasterize(&model.triangles, false);
    Analysis {
        summary,
        collisions,
        part: Some(part.eroded()),
        start,
        sheet: None,
    }
}

/// The time of the move of operation `k` nearest `p`.
fn time_near(tl: &Timeline, spans: &[OpSpan], k: usize, p: P3) -> f64 {
    let first = spans[k].first;
    let end = spans.get(k + 1).map_or(tl.segments.len(), |n| n.first);
    let mut best = (f64::INFINITY, first);
    for (i, s) in tl.segments.iter().enumerate().take(end).skip(first) {
        let d = cadrs_core::cam::cadrs_cam::geom::dist(s.to, p)
            .min(cadrs_core::cam::cadrs_cam::geom::dist(s.from, p));
        if d < best.0 {
            best = (d, i);
        }
    }
    tl.end_of(best.1.saturating_sub(1))
}
