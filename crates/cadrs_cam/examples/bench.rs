//! Times the 3D strategies on a dished plate (a 180 mm square, 45 high, its top a spherical
//! dish 16 mm deep, about 30k triangles), with their machining times, then verifies adaptive
//! clearing and a parallel finish together: `cargo run -r -p cadrs_cam --example bench`.

use std::time::Instant;

use cadrs_cam::geom::Aabb;
use cadrs_cam::model::Model;
use cadrs_cam::strategy::{Params, Pattern, Stock, generate};
use cadrs_cam::{Input, MachineKind, Tool, ToolKind};

fn dish() -> Model {
    let n = 120;
    let s = 180.0;
    let h = 45.0;
    let z = |x: f64, y: f64| {
        let r2 = x * x + y * y;
        if r2 < 140.0 * 140.0 {
            h - (16.5 - (400.0 - (400.0f64 * 400.0 - r2).sqrt())).max(0.0)
        } else {
            h
        }
    };
    let p = |i: usize, j: usize| {
        let x = -s / 2.0 + s * i as f64 / n as f64;
        let y = -s / 2.0 + s * j as f64 / n as f64;
        [x, y, z(x, y)]
    };
    let mut t = Vec::new();
    for j in 0..n {
        for i in 0..n {
            let (a, b, c, d) = (p(i, j), p(i + 1, j), p(i + 1, j + 1), p(i, j + 1));
            t.push([a, b, c]);
            t.push([a, c, d]);
        }
    }
    // The sides and bottom.
    let b = Aabb::new([-s / 2.0, -s / 2.0, 0.0], [s / 2.0, s / 2.0, 1.0]);
    t.extend(cadrs_cam::model::box_triangles(&b));
    Model::from_triangles("dish", t)
}

fn main() {
    let m = dish();
    println!("{} triangles", m.triangles.len());
    let stock = Stock::block(Aabb::new([-92.0, -92.0, 0.0], [92.0, 92.0, 46.0]));
    let mut run = Vec::new();
    for (pattern, kind, d) in [
        (Pattern::Rough3D, ToolKind::EndMill, 10.0),
        (Pattern::Adaptive, ToolKind::EndMill, 10.0),
        (Pattern::Parallel, ToolKind::BallMill, 6.0),
        (Pattern::ConstantZ, ToolKind::BallMill, 6.0),
        (Pattern::Scallop, ToolKind::BallMill, 6.0),
        (Pattern::Spiral, ToolKind::BallMill, 6.0),
        (Pattern::Pencil, ToolKind::BallMill, 6.0),
    ] {
        let tool = Tool::new(kind, d);
        let p = Params::new(pattern, &tool, MachineKind::Mill);
        let input = Input {
            model: &m,
            stock: &stock,
            tool: &tool,
            machine: MachineKind::Mill,
            params: &p,
            rest: None,
            fixtures: &[],
        };
        let t = Instant::now();
        let r = generate(&input);
        let secs = t.elapsed().as_secs_f64();
        match r {
            Ok(r) => {
                let stats = r.stats([0.0, 0.0, 100.0], 20_000.0);
                println!(
                    "{pattern:?}: {secs:.2} s, {} steps, {:.1} min of machining, {} plunges{}",
                    r.steps.len(),
                    stats.minutes,
                    stats.pierces,
                    r.warnings
                        .first()
                        .map(|w| format!(" ({w})"))
                        .unwrap_or_default()
                );
                if matches!(pattern, Pattern::Adaptive | Pattern::Parallel) {
                    run.push((tool, r));
                }
            }
            Err(e) => println!("{pattern:?}: {secs:.2} s, {e}"),
        }
    }
    // Verify the adaptive roughing and the parallel finish.
    let t = Instant::now();
    let segs: Vec<_> = run
        .iter()
        .map(|(_, tp)| tp.motion([0.0, 0.0, 100.0], 20_000.0))
        .collect();
    let ops: Vec<cadrs_cam::sim::SimOp> = run
        .iter()
        .zip(&segs)
        .map(|((tool, _), s)| cadrs_cam::sim::SimOp { tool, segments: s })
        .collect();
    let r = cadrs_cam::sim::simulate_mill(&stock, &m, &ops, 0.4);
    println!(
        "Verify: {:.2} s, {} gouge cells, rapid hits {:?}, holder hits {:?}",
        t.elapsed().as_secs_f64(),
        r.gouge_cells,
        r.rapid_hits,
        r.holder_hits
    );
}
