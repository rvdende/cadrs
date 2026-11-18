//! Tests of the solid simulation ([`simulate_mill_solid`]) against the height-map one and
//! against moves with known outcomes.

use super::*;
use crate::geom::{Aabb, Frame, normalize};
use crate::heightmap::EMPTY;
use crate::machine::MachineKind;
use crate::model::box_triangles;
use crate::sim::{SimOp, simulate_mill};
use crate::strategy::{Input, Params, Pattern, generate};
use crate::tool::ToolKind;

fn seg(from: P3, to: P3, feed: bool, axis: Option<[P3; 2]>) -> Segment {
    Segment {
        from,
        to,
        feed: feed.then_some(500.0),
        kind: feed.then_some(crate::toolpath::FeedKind::Cut),
        beam: false,
        minutes: 0.0,
        axis,
    }
}

/// A 3-axis roughing toolpath through both simulators: the solid one leaves the same top
/// surface as the height map, within a cell (sideways too: a wall may stand a cell over), and
/// removes the same volume.
#[test]
fn a_three_axis_pocket_matches_the_height_map() {
    let m = crate::strategy::testing::pocket_block();
    let stock = crate::strategy::testing::stock_for(&m, 3.0, 1.0);
    let tool = Tool::new(ToolKind::EndMill, 6.0);
    let mut p = Params::new(Pattern::Rough3D, &tool, MachineKind::Mill);
    p.step_down = 3.0;
    p.wall_stock = 0.3;
    p.floor_stock = 0.3;
    let input = Input {
        model: &m,
        stock: &stock,
        tool: &tool,
        machine: MachineKind::Mill,
        params: &p,
        rest: None,
        fixtures: &[],
    };
    let tp = generate(&input).unwrap();
    let segs = tp.motion([0.0, 0.0, 40.0], 5000.0);
    let cell = 0.25;
    let flat = simulate_mill(
        &stock,
        &m,
        &[SimOp {
            tool: &tool,
            segments: &segs,
        }],
        cell,
    );
    let block = TriDexel::block(Frame::WORLD, &stock.bounds, cell);
    let part = TriDexel::of_solid(Frame::WORLD, &m.triangles, &stock.bounds, cell);
    let solid = simulate_mill_solid(
        block,
        Some(&part),
        None,
        &[SolidOp {
            tool: &tool,
            segments: &segs,
            frame: Frame::WORLD,
            tag: 1,
        }],
    );
    assert_eq!(
        solid.gouge_count,
        0,
        "{:?} {}",
        solid.gouges.first(),
        solid.gouge_depth
    );
    assert!(solid.rapid_hits.is_empty(), "{:?}", solid.rapid_hits);
    assert!(solid.holder_hits.is_empty(), "{:?}", solid.holder_hits);
    // (The height map counts a whole cell for each of its edge columns, half of it outside
    // the stock: 3 % too much here, where the margin round the part is cut to the bottom.)
    let (before, after) = (
        stock.bounds.size().iter().product::<f64>(),
        solid.stock.volume(),
    );
    assert!(
        (before - after - solid.removed).abs() < 1e-2,
        "{}",
        solid.removed
    );
    assert!(
        (solid.removed - flat.removed).abs() < flat.removed * 0.04,
        "{} {}",
        solid.removed,
        flat.removed
    );
    // The pocket is roughed to the floor stock, as the height map has it.
    let s = &solid.stock;
    let top = |x: f64, y: f64| -> f64 {
        let (i, j) = (
            ((x - s.min[0]) / s.step[0]).round() as usize,
            ((y - s.min[1]) / s.step[1]).round() as usize,
        );
        s.spans(2, i, j)
            .last()
            .map_or(f64::NEG_INFINITY, |sp| sp.hi as f64 + s.min[2])
    };
    assert!((top(30.0, 20.0) - 12.3).abs() < 0.05, "{}", top(30.0, 20.0));
    assert!(
        (solid.excess - flat.excess).abs() < 0.5,
        "{} {}",
        solid.excess,
        flat.excess
    );
    // Everywhere: the solid's top is between the lowest and the highest of the height map
    // within a cell of the place.
    let (mut worst, mut checked) = (0.0f64, 0);
    for j in 0..s.n[1] {
        for i in 0..s.n[0] {
            let (x, y) = (
                s.min[0] + i as f64 * s.step[0],
                s.min[1] + j as f64 * s.step[1],
            );
            let z = top(x, y);
            let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
            for dj in -1..=1 {
                for di in -1..=1 {
                    let v = flat
                        .stock
                        .sample([x + di as f64 * cell, y + dj as f64 * cell]);
                    if v != EMPTY {
                        lo = lo.min(v as f64);
                        hi = hi.max(v as f64);
                    }
                }
            }
            if lo.is_finite() && z.is_finite() {
                worst = worst.max(lo - z).max(z - hi);
                checked += 1;
            }
        }
    }
    assert!(checked > 35_000, "{checked}");
    assert!(worst < 0.02, "{worst}");
}

/// What the height-map simulation reports, the solid one does: a plunge through the part is a
/// gouge of its depth, a rapid into the stock a hit; flutes too short and a holder on the
/// stock are found; a pass down a wall whose stock the flutes clear is not.
#[test]
fn the_solid_simulation_reports_the_same_things() {
    let cell = 0.2;
    let b = Aabb::new([0.0, 0.0, 0.0], [20.0, 20.0, 12.0]);
    let part_tris = box_triangles(&Aabb::new([0.0, 0.0, 0.0], [20.0, 20.0, 10.0]));
    let part = TriDexel::of_solid(Frame::WORLD, &part_tris, &b, cell);
    let tool = Tool::new(ToolKind::EndMill, 4.0);
    let segs = vec![
        seg([10.0, 10.0, 20.0], [10.0, 10.0, 15.0], false, None),
        seg([10.0, 10.0, 15.0], [10.0, 10.0, 5.0], true, None),
        seg([10.0, 10.0, 5.0], [15.0, 15.0, 3.0], false, None),
    ];
    let r = simulate_mill_solid(
        TriDexel::block(Frame::WORLD, &b, cell),
        Some(&part),
        None,
        &[SolidOp {
            tool: &tool,
            segments: &segs,
            frame: Frame::WORLD,
            tag: 1,
        }],
    );
    assert!(r.gouge_count > 0);
    assert!((r.gouge_depth - 5.0).abs() < 0.05, "{}", r.gouge_depth);
    assert_eq!(r.rapid_hits.len(), 1, "{:?}", r.rapid_hits);
    let hole = std::f64::consts::PI * 4.0 * 7.0;
    assert!(
        (r.removed - hole).abs() < hole * 0.03,
        "{} {hole}",
        r.removed
    );
    assert!(r.stock.mesh(1).tags.contains(&crate::dexel::GOUGE));

    // Flutes 10 long: down a 40 mm wall along 0.3 mm of wall stock is fine; slotting 20 deep
    // in one go buries the shank.
    let mut tool = Tool::new(ToolKind::EndMill, 6.0);
    tool.cutting_length = 10.0;
    let wall = TriDexel::block(
        Frame::WORLD,
        &Aabb::new([0.0, 0.0, 0.0], [20.3, 20.0, 40.0]),
        0.25,
    );
    let down = vec![seg([23.0, 10.0, 40.0], [23.0, 10.0, 0.0], true, None)];
    let op = |segments| SolidOp {
        tool: &tool,
        segments,
        frame: Frame::WORLD,
        tag: 1,
    };
    let r = simulate_mill_solid(wall, None, None, &[op(&down)]);
    assert!(r.holder_hits.is_empty(), "{:?}", r.holder_hits);
    // (The sliver of the wall's stock inside the cutter's circle: 0.53 mm² of it, 40 high.)
    assert!((r.removed - 21.1).abs() < 4.0, "{}", r.removed);
    let slab = TriDexel::block(
        Frame::WORLD,
        &Aabb::new([0.0, 0.0, 0.0], [60.0, 20.0, 40.0]),
        0.5,
    );
    let slot = vec![
        seg([40.0, 10.0, 45.0], [40.0, 10.0, 20.0], true, None),
        seg([40.0, 10.0, 20.0], [55.0, 10.0, 20.0], true, None),
    ];
    // (The plunge isn't a hit, as on the height map: the shank follows the flutes down the
    // hole they made. The move sideways drags it through uncut stock.)
    let r = simulate_mill_solid(slab.clone(), None, None, &[op(&slot)]);
    assert_eq!(r.holder_hits.len(), 1, "{:?}", r.holder_hits);
    assert!(r.holder_hits[0].1[2] >= 30.0, "{:?}", r.holder_hits);

    // A Ø30 holder 20 above the tip, down a hole 25 deep: it sits on the stock.
    let mut tool = Tool::new(ToolKind::EndMill, 6.0);
    tool.cutting_length = 30.0;
    tool.overall_length = 20.0;
    tool.holder = Some(crate::tool::Profile::cylinder("h", 30.0, 40.0));
    let down = vec![seg([40.0, 10.0, 45.0], [40.0, 10.0, 15.0], true, None)];
    let run = |tool: &Tool, fixtures: Option<&TriDexel>| {
        simulate_mill_solid(
            slab.clone(),
            None,
            fixtures,
            &[SolidOp {
                tool,
                segments: &down,
                frame: Frame::WORLD,
                tag: 1,
            }],
        )
    };
    let r = run(&tool, None);
    assert_eq!(r.holder_hits.len(), 1);
    assert!(r.holder_hits[0].1[2] >= 35.0, "{:?}", r.holder_hits);
    // A clamp beside the hole, under the holder: hit by the holder, on a lattice of its own.
    let clamp = box_triangles(&Aabb::new([48.0, 5.0, 40.0], [58.0, 15.0, 50.0]));
    let fixtures = TriDexel::of_solid(
        Frame::WORLD,
        &clamp,
        &Aabb::new([47.0, 4.0, 39.0], [59.0, 16.0, 51.0]),
        0.4,
    );
    let r = run(&tool, Some(&fixtures));
    assert_eq!(r.fixture_hits.len(), 1, "{:?}", r.fixture_hits);
    tool.holder = None;
    let r = run(&tool, Some(&fixtures));
    assert!(
        r.holder_hits.is_empty() && r.fixture_hits.is_empty(),
        "{r:?}"
    );
}

/// A leaning operation in a frame of its own: a T-slot cutter run along the block's side
/// under its top (which no height map from above can show), and a 5-axis pass whose tool
/// turns as it goes.
#[test]
fn leaning_and_undercutting_cuts() {
    // (A cell that puts no lattice plane on a face of the slot: see the module docs on
    // volumes.)
    let cell = 0.23;
    let b = Aabb::new([0.0, 0.0, 0.0], [40.0, 30.0, 20.0]);
    // The slot: the cutter's disc (Ø16, 4 thick) 6 into the −Y side, its top 5 under the
    // block's.
    let mut slotter = Tool::new(ToolKind::Form, 1.0);
    slotter.set_form(crate::cutter::FormProfile::t_slot(16.0, 4.0, 6.0, 30.0));
    slotter.overall_length = 40.0;
    let slot = vec![seg([-10.0, -2.0, 11.0], [50.0, -2.0, 11.0], true, None)];
    // The same pass in a frame turned a quarter about Z at the block's far corner: its x is
    // the model's −y from 30, its y the model's x.
    let turned = Frame::from_zx([0.0, 30.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]);
    let slot_turned = vec![seg(
        turned.to_local([-10.0, -2.0, 11.0]),
        turned.to_local([50.0, -2.0, 11.0]),
        true,
        None,
    )];
    let mut stocks = Vec::new();
    for (segments, frame) in [(&slot, Frame::WORLD), (&slot_turned, turned)] {
        let r = simulate_mill_solid(
            TriDexel::block(Frame::WORLD, &b, cell),
            None,
            None,
            &[SolidOp {
                tool: &slotter,
                segments,
                frame,
                tag: 3,
            }],
        );
        // The neck (Ø6, its edge at y = 1) cuts a groove up through the top too.
        let want = 40.0 * (6.0 * 4.0 + 1.0 * 5.0);
        assert!(
            (r.removed - want).abs() < want * 0.05,
            "{} {want}",
            r.removed
        );
        assert!(r.holder_hits.is_empty(), "{:?}", r.holder_hits);
        // Down a ray 3 in from the side: material, the slot, material again.
        let s = &r.stock;
        let spans = s.spans(
            2,
            (20.0 / s.step[0]).round() as usize,
            (3.0 / s.step[1]).round() as usize,
        );
        let got: Vec<(f32, f32)> = spans.iter().map(|s| (s.lo, s.hi)).collect();
        assert_eq!(got, [(0.0, 11.0), (15.0, 20.0)]);
        assert_eq!((spans[0].hi_tag, spans[1].lo_tag), (3, 3));
        stocks.push(r.stock);
    }
    assert!((stocks[0].volume() - stocks[1].volume()).abs() < 1.0);

    // Five axes: a ball mill along the top's middle, leaning from 30° back to 30° forward as
    // it goes, its tip 2 under the top: the groove is what the ball leaves, its centre 4 up
    // the leaning axis from the tip.
    let ball = Tool::new(ToolKind::BallMill, 8.0);
    let lean = |deg: f64| normalize([deg.to_radians().sin(), 0.0, deg.to_radians().cos()]);
    let mut pass = Vec::new();
    for k in 0..10 {
        let (x0, x1) = (5.0 + 3.0 * k as f64, 8.0 + 3.0 * k as f64);
        let (a0, a1) = (lean(-30.0 + 6.0 * k as f64), lean(-24.0 + 6.0 * k as f64));
        pass.push(seg(
            [x0, 15.0, 18.0],
            [x1, 15.0, 18.0],
            true,
            Some([a0, a1]),
        ));
    }
    let r = simulate_mill_solid(
        TriDexel::block(Frame::WORLD, &b, cell),
        None,
        None,
        &[SolidOp {
            tool: &ball,
            segments: &pass,
            frame: Frame::WORLD,
            tag: 1,
        }],
    );
    assert!(r.holder_hits.is_empty() && r.rapid_hits.is_empty());
    let s = &r.stock;
    let eps = crate::dexel::sim_eps(cell);
    for x in [10.0, 20.0, 30.0] {
        for dy in [0.0f64, 1.0, 2.0, 3.0] {
            let (i, j) = (
                (x / s.step[0]).round() as usize,
                ((15.0 + dy) / s.step[1]).round() as usize,
            );
            let (x, dy) = (i as f64 * s.step[0], j as f64 * s.step[1] - 15.0);
            // The lowest any of the ball's places along the pass reaches down this ray.
            let mut want = f64::INFINITY;
            for seg in &pass {
                for k in 0..=2000 {
                    let f = k as f64 / 2000.0;
                    let tip = crate::geom::lerp(seg.from, seg.to, f);
                    let c = crate::geom::add(tip, crate::geom::scale(seg.axis_at(f), 4.0));
                    let h2 = 16.0 - dy * dy - (x - c[0]) * (x - c[0]);
                    if h2 >= 0.0 {
                        want = want.min(c[2] - h2.sqrt());
                    }
                }
            }
            let got = s.spans(2, i, j).last().unwrap().hi as f64;
            assert!((got - want).abs() < 3.0 * eps, "{x} {dy}: {got} {want}");
            // (Leaning, the ball reaches under its tip's height: 0.24 under at 20°.)
            if dy.abs() < 0.2 && (x - 10.0).abs() < 0.2 {
                assert!(want < 17.85 && want > 17.6, "{want}");
            }
        }
    }
}

/// The stock cut as a run plays: part way along a move cuts that much of it, the whole run
/// leaves what the simulation does, and going back starts again from the uncut stock.
#[test]
fn the_live_stock_is_cut_to_where_the_tool_is() {
    let tool = Tool::new(ToolKind::EndMill, 6.0);
    let slab = TriDexel::block(
        Frame::WORLD,
        &Aabb::new([0.0, 0.0, 0.0], [60.0, 20.0, 20.0]),
        0.5,
    );
    // Three slots across the slab (y from -10 to 30, so 20 of the 40 in it), a rapid between each.
    let mut moves = Vec::new();
    for x in [10.0, 30.0, 50.0] {
        moves.push(seg([x, -10.0, 30.0], [x, -10.0, 15.0], false, None));
        moves.push(seg([x, -10.0, 15.0], [x, 30.0, 15.0], true, None));
        moves.push(seg([x, 30.0, 15.0], [x, 30.0, 30.0], false, None));
    }
    let full = slab.volume();
    let whole = simulate_mill_solid(
        slab.clone(),
        None,
        None,
        &[SolidOp {
            tool: &tool,
            segments: &moves,
            frame: Frame::WORLD,
            tag: 1,
        }],
    );
    let slot = (full - whole.stock.volume()) / 3.0;
    assert!(slot > 100.0, "{slot}");
    let mut live = LiveStock::new(slab, vec![(tool.clone(), moves.clone())]);
    let cut = |l: &LiveStock| (full - l.stock.volume()) / slot;
    // Half way along the first slot's cut, its tip at y = 10: the slot from y = 0 to 10 and
    // the cutter's half disc beyond, of the whole slot's 20 × 6.
    let half = (10.0 * 6.0 + std::f64::consts::PI * 9.0 / 2.0) / 120.0;
    live.cut_to(1, 0.5);
    assert!((cut(&live) - half).abs() < 0.03, "{}", cut(&live));
    // On to the second slot's rapid down: the first whole.
    live.cut_to(3, 1.0);
    assert!((cut(&live) - 1.0).abs() < 0.05, "{}", cut(&live));
    assert_eq!(live.at(), (3, 1.0));
    // The whole run: the simulation's stock.
    live.cut_to(8, 1.0);
    assert!((live.stock.volume() - whole.stock.volume()).abs() < 1e-6);
    // Back to the first slot's half: the same as before.
    live.cut_to(1, 0.5);
    assert!((cut(&live) - half).abs() < 0.03, "{}", cut(&live));
}
