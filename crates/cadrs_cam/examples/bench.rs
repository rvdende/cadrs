//! Times the 3D strategies on a dished plate (a 180 mm square, 45 high, its top a spherical
//! dish 16 mm deep, about 30k triangles), with their machining times, then verifies adaptive
//! clearing and a parallel finish together, and last the solid (tri-dexel) simulation on a
//! 5-axis finish of the dish, the ball mill held square to it, at the coarsest and the finest
//! resolution: `cargo run -r -p cadrs_cam --example bench`.

use std::time::Instant;

use cadrs_cam::geom::Aabb;
use cadrs_cam::model::Model;
use cadrs_cam::strategy::{Params, Pattern, Stock, generate};
use cadrs_cam::{Input, MachineKind, Tool, ToolKind};

/// The dish's height at a point.
fn dish_z(x: f64, y: f64) -> f64 {
    let h = 45.0;
    let r2 = x * x + y * y;
    if r2 < 140.0 * 140.0 {
        h - (16.5 - (400.0 - (400.0f64 * 400.0 - r2).sqrt())).max(0.0)
    } else {
        h
    }
}

fn dish() -> Model {
    let n = 120;
    let s = 180.0;
    let z = dish_z;
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
    solid(&stock);
}

/// The dish as a closed solid (the model above is its top over a thin base, which is all the
/// 3-axis patterns look at): the top, walls down from its four edges, and a bottom.
fn dish_solid() -> Vec<[[f64; 3]; 3]> {
    let (n, s) = (120usize, 180.0);
    let at = |i: usize| -s / 2.0 + s * i as f64 / n as f64;
    let top = |i: usize, j: usize| [at(i), at(j), dish_z(at(i), at(j))];
    let foot = |i: usize, j: usize| [at(i), at(j), 0.0];
    let mut t = Vec::new();
    for j in 0..n {
        for i in 0..n {
            let (a, b, c, d) = (top(i, j), top(i + 1, j), top(i + 1, j + 1), top(i, j + 1));
            t.push([a, b, c]);
            t.push([a, c, d]);
        }
    }
    t.push([foot(0, 0), foot(n, n), foot(n, 0)]);
    t.push([foot(0, 0), foot(0, n), foot(n, n)]);
    // A wall under the top's edge from grid point `a` to `b`, the solid on its left.
    let mut wall = |a: (usize, usize), b: (usize, usize)| {
        let (b0, b1, t0, t1) = (foot(a.0, a.1), foot(b.0, b.1), top(a.0, a.1), top(b.0, b.1));
        t.push([b0, b1, t1]);
        t.push([b0, t1, t0]);
    };
    for k in 0..n {
        wall((k, 0), (k + 1, 0));
        wall((n, k), (n, k + 1));
        wall((k + 1, n), (k, n));
        wall((0, k + 1), (0, k));
    }
    t
}

/// The solid simulation: a ball mill following the dish square to it (so every move turns the
/// tool), 48 passes of 80 moves, after a face mill has taken the top off; then the comparison
/// with the part and the mesh.
fn solid(stock: &Stock) {
    let solid = dish_solid();
    use cadrs_cam::dexel::{TriDexel, solid_cell};
    use cadrs_cam::geom::{Frame, normalize};
    use cadrs_cam::sim::{SolidOp, simulate_mill_solid};
    use cadrs_cam::toolpath::{FeedKind, Segment};
    let feed = |from, to, axis| Segment {
        about: None,
        from,
        to,
        feed: Some(1000.0),
        kind: Some(FeedKind::Cut),
        beam: false,
        minutes: 0.0,
        axis,
    };
    let face = Tool::new(ToolKind::FaceMill, 50.0);
    let facing: Vec<Segment> = (0..5)
        .map(|k| {
            let y = -80.0 + 40.0 * k as f64;
            feed([-120.0, y, 45.0], [120.0, y, 45.0], None)
        })
        .collect();
    let ball = Tool::new(ToolKind::BallMill, 6.0);
    let at = |x: f64, y: f64| -> ([f64; 3], [f64; 3]) {
        let e = 1e-4;
        let n = normalize([
            -(dish_z(x + e, y) - dish_z(x - e, y)) / (2.0 * e),
            -(dish_z(x, y + e) - dish_z(x, y - e)) / (2.0 * e),
            1.0,
        ]);
        ([x, y, dish_z(x, y)], n)
    };
    let mut finish = Vec::new();
    for j in 0..48 {
        let y = -84.6 + 3.6 * j as f64;
        for i in 0..80 {
            let (x0, x1) = (-88.0 + 2.2 * i as f64, -88.0 + 2.2 * (i + 1) as f64);
            let ((p0, a0), (p1, a1)) = (at(x0, y), at(x1, y));
            finish.push(feed(p0, p1, Some([a0, a1])));
        }
    }
    let turning = finish
        .iter()
        .filter(|s| s.axis.is_some_and(|[a, b]| a != b))
        .count();
    println!(
        "Solid simulation: {} facing moves, {} finishing moves ({turning} turning the tool)",
        facing.len(),
        finish.len()
    );
    for quality in [0.0, 1.0] {
        let cell = solid_cell(stock.bounds.size(), quality);
        let t = Instant::now();
        let block = TriDexel::block(Frame::WORLD, &stock.bounds, cell);
        let part = TriDexel::of_solid(Frame::WORLD, &solid, &stock.bounds, cell);
        let built = t.elapsed().as_secs_f64();
        let ops = [
            SolidOp {
                tool: &face,
                segments: &facing,
                frame: Frame::WORLD,
                tag: 1,
            },
            SolidOp {
                tool: &ball,
                segments: &finish,
                frame: Frame::WORLD,
                tag: 2,
            },
        ];
        let t = Instant::now();
        let r = simulate_mill_solid(block, Some(&part), None, &ops);
        let run = t.elapsed().as_secs_f64();
        let t = Instant::now();
        let mesh = r.stock.mesh(1);
        let meshed = t.elapsed().as_secs_f64();
        println!(
            "  cell {cell:.3} ({:?} points, {} spans): stock and part {built:.2} s, \
             simulation {run:.2} s ({:.0} µs a move), mesh {meshed:.2} s ({} triangles); \
             removed {:.0} mm³, {} gouge points (deepest {:.3}), excess {:.2}, holder hits {}",
            r.stock.n,
            r.stock.span_count(),
            run * 1e6 / (facing.len() + finish.len()) as f64,
            mesh.indices.len() / 3,
            r.removed,
            r.gouge_count,
            r.gouge_depth,
            r.excess,
            r.holder_hits.len(),
        );
    }
}
