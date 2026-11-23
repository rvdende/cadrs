//! Tests of the tri-dexel stock against shapes with known answers.

use super::*;
use crate::geom::dist;
use crate::model::box_triangles;
use crate::tool::ToolKind;

const UP: P3 = [0.0, 0.0, 1.0];

fn pose(tip: P3, axis: P3) -> Pose {
    Pose {
        tip,
        axis: normalize(axis),
    }
}

fn block(size: P3, cell: f64) -> TriDexel {
    TriDexel::block(Frame::WORLD, &Aabb::new([0.0; 3], size), cell)
}

/// A small deterministic generator.
struct Rng(u64);

impl Rng {
    fn unit(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
    }

    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }

    fn dir(&mut self) -> P3 {
        loop {
            let v = [
                self.range(-1.0, 1.0),
                self.range(-1.0, 1.0),
                self.range(-1.0, 1.0),
            ];
            let l = len(v);
            if l > 0.1 && l <= 1.0 {
                return scale(v, 1.0 / l);
            }
        }
    }
}

/// Whether `x` is in the piece `k` held with its tip at `b` and its axis along `a`.
fn member(k: Convex, b: P3, a: P3, x: P3) -> bool {
    let w = sub(x, b);
    let h = dot(w, a);
    match k {
        Convex::Frustum { r0, h0, r1, h1 } => {
            let r = r0 + (r1 - r0) * (h - h0) / (h1 - h0);
            h >= h0 && h <= h1 && dot(w, w) - h * h <= r * r
        }
        Convex::Sphere { hc, rho, h0, h1 } => {
            h >= h0 && h <= h1 && dist(x, add(b, scale(a, hc))) <= rho
        }
    }
}

/// The interval of a ray inside a sliding piece is what sampling the slide and the ray finds,
/// for pieces, axes, slides and rays of every sort (the special ones too: no slide, a slide
/// along the ray or along the piece's axis, the ray along or across the axis).
#[test]
fn a_rays_stretch_in_a_sweep_is_what_sampling_finds() {
    let mut rng = Rng(11);
    let (mut found, mut empty) = (0, 0);
    for case in 0..400 {
        let k = if case % 3 == 0 {
            let rho = rng.range(0.5, 5.0);
            let hc = rng.range(0.0, 6.0);
            let h0 = hc - rho * rng.range(-0.3, 1.0);
            Convex::Sphere {
                hc,
                rho,
                h0,
                h1: (h0 + rho * rng.range(0.2, 2.0)).min(hc + rho),
            }
        } else {
            let h0 = rng.range(0.0, 8.0);
            let r0 = if case % 7 == 1 {
                0.0
            } else {
                rng.range(0.2, 5.0)
            };
            Convex::Frustum {
                r0,
                h0,
                r1: if case % 5 == 2 {
                    r0.max(0.5)
                } else {
                    rng.range(0.2, 5.0)
                },
                h1: h0 + rng.range(0.05, 8.0),
            }
        };
        let axis = case % 3;
        let mut unit = [0.0; 3];
        unit[axis] = 1.0;
        let a = match case % 11 {
            0 => unit,
            1 => normalize(cross(unit, rng.dir())),
            _ => rng.dir(),
        };
        let d = match case % 13 {
            0 => [0.0; 3],
            1 => scale(unit, rng.range(-12.0, 12.0)),
            2 => scale(a, rng.range(-12.0, 12.0)),
            _ => scale(rng.dir(), rng.range(0.0, 15.0)),
        };
        let b = [
            rng.range(-3.0, 3.0),
            rng.range(-3.0, 3.0),
            rng.range(-3.0, 3.0),
        ];
        let sweep = Sweep { k, b, a, d };
        let form = sweep.form(axis);
        let bb = sweep.bounds();
        let (bu, cv) = others(axis);
        for _ in 0..8 {
            let pu = rng.range(bb.min[bu] - 0.5, bb.max[bu] + 0.5);
            let pv = rng.range(bb.min[cv] - 0.5, bb.max[cv] + 0.5);
            let exact = form.span(pu, pv);
            // Whether the ray's point at `t` is in the piece somewhere along the slide, by
            // trying `ns` places along it.
            let in_sweep = |t: f64, ns: usize| -> bool {
                let mut x = [0.0; 3];
                x[axis] = t;
                x[bu] = pu;
                x[cv] = pv;
                (0..=ns).any(|j| member(k, add(b, scale(d, j as f64 / ns as f64)), a, x))
            };
            // Sampled along the ray across the sweep's box: everything found must be in the
            // exact stretch.
            let (t0, t1) = (bb.min[axis] - 1.0, bb.max[axis] + 1.0);
            let nt = 600;
            let mut sampled: Option<(f64, f64)> = None;
            for i in 0..=nt {
                let t = t0 + (t1 - t0) * i as f64 / nt as f64;
                if in_sweep(t, 96) {
                    sampled = Some(sampled.map_or((t, t), |(lo, _)| (lo, t)));
                }
            }
            match (exact, sampled) {
                (Some((lo, hi)), _) => {
                    if let Some((slo, shi)) = sampled {
                        assert!(lo <= slo + 1e-6 && hi >= shi - 1e-6, "{case}: {sweep:?}");
                    }
                    // And the exact stretch is no wider than the sweep: just inside each of
                    // its ends the ray is in the piece at some place along the slide.
                    if hi - lo > 4e-3 {
                        let inset = ((hi - lo) / 4.0).min(0.02);
                        assert!(
                            in_sweep(lo + inset, 20_000) && in_sweep(hi - inset, 20_000),
                            "{case}: {sweep:?} at {pu} {pv}: {lo} {hi} {sampled:?}"
                        );
                        found += 1;
                    }
                }
                (None, Some(_)) => panic!("{case}: {sweep:?} at {pu} {pv} missed"),
                (None, None) => empty += 1,
            }
        }
    }
    assert!(found > 800 && empty > 200, "{found} {empty}");
}

/// A block is its box: its volume exact, whatever the cell.
#[test]
fn a_block_is_exact() {
    for cell in [0.5, 0.37, 1.3] {
        let s = block([40.0, 30.0, 10.0], cell);
        assert!(
            (s.volume() - 12_000.0).abs() < 1e-6,
            "{cell}: {}",
            s.volume()
        );
        assert_eq!(s.size(), [40.0, 30.0, 10.0]);
    }
    assert!((solid_cell([100.0, 40.0, 20.0], 0.0) - 100.0 / 192.0).abs() < 1e-12);
    assert!((solid_cell([100.0, 40.0, 20.0], 1.0) - 100.0 / 640.0).abs() < 1e-12);
}

/// A block faced by a flat end mill: the volume removed is the area times the depth, exactly
/// (the cut is flat and midway between lattice planes: see the module docs).
#[test]
fn facing_removes_exactly_the_layer() {
    let mut s = block([40.0, 30.0, 10.0], 0.5);
    let tool = Tool::new(ToolKind::EndMill, 10.0);
    let c = SimCutter::new(&tool, sim_eps(s.cell));
    let mut removed = 0.0;
    for y in [0.0, 8.0, 16.0, 24.0, 30.0] {
        removed += s.cut(&c, pose([-6.0, y, 8.25], UP), pose([46.0, y, 8.25], UP), 1);
    }
    assert!((removed - 40.0 * 30.0 * 1.75).abs() < 1e-3, "{removed}");
    assert!(
        (s.volume() - 40.0 * 30.0 * 8.25).abs() < 1e-3,
        "{}",
        s.volume()
    );
    // Every ray along Z ends at the cut, tagged with the operation.
    for j in 0..s.n[1] {
        for i in 0..s.n[0] {
            let spans = s.spans(2, i, j);
            assert_eq!(spans.len(), 1);
            assert_eq!((spans[0].lo, spans[0].hi), (0.0, 8.25));
            assert_eq!((spans[0].lo_tag, spans[0].hi_tag), (UNCUT, 1));
        }
    }
    // At a depth that isn't midway the volume is within a layer half a cell thick.
    let mut s = block([40.0, 30.0, 10.0], 0.5);
    let mut removed = 0.0;
    for y in [0.0, 8.0, 16.0, 24.0, 30.0] {
        removed += s.cut(&c, pose([-6.0, y, 8.6], UP), pose([46.0, y, 8.6], UP), 1);
    }
    assert!(
        (removed - 1200.0 * 1.4).abs() < 1200.0 * 0.25 * 2.0 / 3.0 + 1e-3,
        "{removed}"
    );
}

/// A ball mill's groove: a length of the ball's segment and a cap's two halves at its ends.
#[test]
fn a_ball_groove_has_the_capsules_volume() {
    let (r, depth, length) = (3.0f64, 2.0f64, 20.0);
    let segment =
        r * r * ((r - depth) / r).acos() - (r - depth) * (2.0 * r * depth - depth * depth).sqrt();
    let cap = std::f64::consts::PI * depth * depth * (3.0 * r - depth) / 3.0;
    let want = segment * length + cap;
    let tool = Tool::new(ToolKind::BallMill, 2.0 * r);
    for (cell, within) in [(0.5, 0.04), (0.25, 0.02)] {
        let mut s = block([40.0, 20.0, 10.0], cell);
        let c = SimCutter::new(&tool, sim_eps(cell));
        let before = s.volume();
        let removed = s.cut(
            &c,
            pose([10.0, 10.0, 8.0], UP),
            pose([30.0, 10.0, 8.0], UP),
            1,
        );
        assert!(
            (removed - want).abs() < want * within,
            "{cell}: {removed} {want}"
        );
        assert!((before - s.volume() - removed).abs() < 1e-3);
        // The ray down the groove's middle ends exactly at its floor; one a millimetre off the
        // middle where the ball is.
        let (i, j) = (
            (20.0 / s.step[0]).round() as usize,
            (10.0 / s.step[1]).round() as usize,
        );
        assert!((s.spans(2, i, j)[0].hi - 8.0).abs() < 1e-5);
        let off = (1.0 / s.step[1]).round() as usize;
        let y = off as f64 * s.step[1];
        let floor = 8.0 + r - (r * r - y * y).sqrt();
        assert!((s.spans(2, i, j + off)[0].hi as f64 - floor).abs() < 1e-5);
    }
}

/// An end mill swung through a quarter turn about its tip sweeps its own cylinder and a
/// quarter of what its length sweeps round the pivot: `π r² L + (π/2)(r L² + ⅔ r³)`.
#[test]
fn a_swing_about_the_tip_removes_a_sector() {
    let (r, l) = (3.0f64, 20.0f64);
    let mut tool = Tool::new(ToolKind::EndMill, 2.0 * r);
    tool.cutting_length = l;
    let quarter = std::f64::consts::FRAC_PI_2;
    let want = std::f64::consts::PI * r * r * l + quarter * (r * l * l + 2.0 / 3.0 * r * r * r);
    // (Each sub-move holds the tool at its middle angle, so its corners reach up to the
    // tolerance past the true sweep at the sub-move's ends: a little more is removed.)
    for (cell, within) in [(0.5, 0.05), (0.25, 0.025)] {
        let mut s = TriDexel::block(
            Frame::WORLD,
            &Aabb::new([-8.0, -8.0, -8.0], [28.0, 8.0, 28.0]),
            cell,
        );
        let c = SimCutter::new(&tool, sim_eps(cell));
        let removed = s.cut(&c, pose([0.0; 3], UP), pose([0.0; 3], [1.0, 0.0, 0.0]), 1);
        assert!(
            (removed - want).abs() < want * within,
            "{cell}: {removed} {want}"
        );
        // Out along the diagonal the cut ends where the tool's far corner swung: √(L² + r²)
        // from the pivot, within the cutter's tolerance and the lattice's rounding.
        let reach = l.hypot(r);
        let k = (8.0 / s.step[1]).round() as usize;
        let mut worst = 0.0f64;
        for i in 0..s.n[0] {
            let x = s.min[0] + i as f64 * s.step[0];
            for span in s.spans(2, i, k) {
                for z in [span.lo, span.hi] {
                    let z = s.min[2] + z as f64;
                    let rho = x.hypot(z);
                    if x > 6.0 && z > 6.0 && rho < reach + 2.0 && rho > reach - 2.0 {
                        worst = worst.max((rho - reach).abs());
                    }
                }
            }
        }
        assert!(worst > 0.0 && worst < 3.0 * c.eps + 1e-3, "{cell}: {worst}");
    }
}

/// Each edge of a mesh is walked once each way: closed and wound one way. Its volume.
fn check_closed(m: &SimMesh) -> f64 {
    let mut edges = std::collections::HashMap::new();
    let mut volume = 0.0;
    for t in m.indices.chunks(3) {
        for e in 0..3 {
            *edges.entry((t[e], t[(e + 1) % 3])).or_insert(0u32) += 1;
        }
        let p = [0, 1, 2].map(|k| m.positions[t[k] as usize].map(|x| x as f64));
        volume += dot(p[0], cross(p[1], p[2])) / 6.0;
    }
    for ((a, b), n) in &edges {
        assert_eq!(*n, 1, "edge {a}-{b}");
        assert_eq!(edges.get(&(*b, *a)), Some(&1), "edge {a}-{b}");
    }
    assert_eq!(m.positions.len(), m.normals.len());
    assert_eq!(m.positions.len(), m.colors.len());
    assert_eq!(m.positions.len(), m.tags.len());
    volume
}

/// A stock cut from above and from the side: the mesh is closed, holds the stock's volume and
/// is coloured by operation.
#[test]
fn the_mesh_is_closed_and_holds_the_volume() {
    let mut s = block([40.0, 30.0, 20.0], 0.5);
    let v = check_closed(&s.mesh(1));
    assert!((v - 24_000.0).abs() < 1e-2, "{v}");
    assert!((check_closed(&s.mesh(3)) - 24_000.0).abs() < 1e-2);
    let ball = SimCutter::new(&Tool::new(ToolKind::BallMill, 6.0), sim_eps(0.5));
    let mut flat = Tool::new(ToolKind::EndMill, 8.0);
    flat.cutting_length = 30.0;
    let flat = SimCutter::new(&flat, sim_eps(0.5));
    s.cut(
        &ball,
        pose([5.0, 8.0, 16.0], UP),
        pose([35.0, 22.0, 17.0], UP),
        1,
    );
    // In from the +X side, leaning, and a hole right through from above.
    let lean = normalize([1.0, 0.2, 0.5]);
    s.cut(
        &flat,
        pose([30.0, 15.0, 8.0], lean),
        pose([30.0, 5.0, 6.0], lean),
        2,
    );
    s.cut(
        &flat,
        pose([12.0, 20.0, -2.0], UP),
        pose([12.0, 20.0, -1.0], UP),
        3,
    );
    for step in [1, 2] {
        let m = s.mesh(step);
        let v = check_closed(&m);
        let within = if step == 1 { 0.02 } else { 0.06 };
        assert!(
            (v - s.volume()).abs() < s.volume() * within,
            "{step}: {v} {}",
            s.volume()
        );
        for tag in [UNCUT, 1, 2, 3] {
            assert!(m.tags.contains(&tag), "{step}: no vertex tagged {tag}");
        }
        // Normals point out: on the uncut top face, up.
        let top = (0..m.positions.len())
            .find(|&i| m.positions[i][2] == 20.0 && m.tags[i] == UNCUT && m.normals[i][2] > 0.99);
        assert!(top.is_some());
    }
    assert_eq!(tag_color(GOUGE), [0.92, 0.12, 0.12, 1.0]);
    assert_ne!(tag_color(1), tag_color(2));
}

/// One thread cuts the same stock, to the bit, and measures the same volumes as many.
#[test]
fn one_thread_gives_the_same_stock() {
    let run = || {
        let mut s = block([60.0, 60.0, 30.0], 0.2);
        let face = SimCutter::new(&Tool::new(ToolKind::FaceMill, 50.0), sim_eps(0.2));
        let ball = SimCutter::new(&Tool::new(ToolKind::BallMill, 8.0), sim_eps(0.2));
        let mut removed = Vec::new();
        removed.push(s.cut(
            &face,
            pose([-30.0, 20.0, 27.0], UP),
            pose([90.0, 40.0, 27.0], UP),
            1,
        ));
        let mut rng = Rng(5);
        for _ in 0..40 {
            let from = pose(
                [
                    rng.range(5.0, 55.0),
                    rng.range(5.0, 55.0),
                    rng.range(15.0, 26.0),
                ],
                add(UP, scale(rng.dir(), 0.5)),
            );
            let to = pose(
                [
                    rng.range(5.0, 55.0),
                    rng.range(5.0, 55.0),
                    rng.range(15.0, 26.0),
                ],
                add(UP, scale(rng.dir(), 0.5)),
            );
            removed.push(s.cut(&ball, from, to, 2));
        }
        (s, removed)
    };
    let many = run();
    let one = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap()
        .install(run);
    assert!(many.1 == one.1, "the volumes differ");
    assert!(many.0 == one.0, "the stocks differ");
    assert!(many.1[0] > 1000.0);
}

/// A solid from its triangles: a box off the lattice's planes, and boxes that overlap and
/// touch (the pocket block is five).
#[test]
fn a_solid_from_triangles() {
    let b = Aabb::new([0.0; 3], [30.0, 20.0, 10.0]);
    let tris = box_triangles(&Aabb::new([3.3, 2.2, 1.1], [25.7, 17.4, 8.6]));
    let s = TriDexel::of_solid(Frame::WORLD, &tris, &b, 0.25);
    let want = 22.4 * 15.2 * 7.5;
    assert!(
        (s.volume() - want).abs() < want * 0.01,
        "{} {want}",
        s.volume()
    );
    let (i, j) = (60, 40);
    let spans = s.spans(2, i, j);
    assert_eq!(spans.len(), 1);
    assert!((spans[0].lo - 1.1).abs() < 1e-5 && (spans[0].hi - 8.6).abs() < 1e-5);
    // The same box wound inside out fills the same.
    let flipped: Vec<[P3; 3]> = tris.iter().map(|t| [t[0], t[2], t[1]]).collect();
    assert!(TriDexel::of_solid(Frame::WORLD, &flipped, &b, 0.25) == s);
    // In a turned frame the solid is the same solid.
    let f = Frame::from_zx([1.0, 2.0, 3.0], [0.3, -0.2, 1.0], [1.0, 0.4, 0.0]);
    let corners = tris.iter().flatten().map(|p| f.to_local(*p));
    let t = TriDexel::of_solid(f, &tris, &Aabb::of(corners).grown(1.0), 0.25);
    assert!(
        (t.volume() - want).abs() < want * 0.01,
        "{} {want}",
        t.volume()
    );

    // Every face of these boxes lies on a lattice plane, and every ray along a face or an
    // edge: each crossing still counts once.
    let m = crate::strategy::testing::pocket_block();
    let b = Aabb::new([-2.0, -2.0, 0.0], [62.0, 42.0, 21.0]);
    let s = TriDexel::of_solid(Frame::WORLD, &m.triangles, &b, 0.5);
    let want = 60.0 * 40.0 * 20.0 - 30.0 * 20.0 * 8.0;
    assert!(
        (s.volume() - want).abs() < want * 0.02,
        "{} {want}",
        s.volume()
    );
    // Through the pocket's floor, the wall beside it, and outside the block.
    let at = |x: f64, y: f64| {
        s.spans(
            2,
            (x / 0.5).round() as usize + 4,
            (y / 0.5).round() as usize + 4,
        )
    };
    assert_eq!(
        at(30.0, 20.0)
            .iter()
            .map(|s| (s.lo, s.hi))
            .collect::<Vec<_>>(),
        [(0.0, 12.0)]
    );
    assert_eq!(
        at(7.0, 20.0)
            .iter()
            .map(|s| (s.lo, s.hi))
            .collect::<Vec<_>>(),
        [(0.0, 20.0)]
    );
    assert!(at(-1.0, 20.0).is_empty());
    let v = check_closed(&s.mesh(1));
    assert!((v - want).abs() < want * 0.02, "{v}");
}

/// A stock height map as a solid, and back: seen down its own Z it is the same map; seen from
/// the side, the same block.
#[test]
fn a_height_map_becomes_a_solid_and_back() {
    let mut map = HeightMap::block(&Aabb::new([0.0; 3], [20.0, 10.0, 8.0]), 0.5);
    let tool = Tool::new(ToolKind::EndMill, 4.0);
    map.cut(&tool, [10.0, -5.0, 5.0], [10.0, 15.0, 5.0]);
    let s = TriDexel::of_height_map(&map, &Frame::WORLD, 0.0, 0.5);
    let want = 20.0 * 10.0 * 8.0 - 4.0 * 10.0 * 3.0;
    assert!(
        (s.volume() - want).abs() < want * 0.03,
        "{} {want}",
        s.volume()
    );
    let back = s.height_map(&Frame::WORLD, &Aabb::new([0.0; 3], [20.0, 10.0, 8.0]), 0.5);
    for j in 0..map.ny {
        for i in 0..map.nx {
            let p = map.point(i, j);
            assert!((back.sample(p) - map.at(i, j)).abs() < 1e-5, "{p:?}");
        }
    }
    // From the +Y side (its Z the model's +Y, its X the model's +X, its Y the model's −Z):
    // through the slot nothing but the floor under it; beside it the block's whole depth.
    let side = Frame::from_zx([0.0, 10.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]);
    let seen = s.height_map(
        &side,
        &Aabb::new([-1.0, -9.0, -11.0], [21.0, 1.0, 1.0]),
        0.5,
    );
    let at = |x: f64, z: f64| seen.sample([x, -z]);
    assert_eq!(at(10.0, 6.5), EMPTY);
    assert!(at(10.0, 3.0).abs() < 0.8, "{}", at(10.0, 3.0));
    assert!(at(4.0, 6.5).abs() < 0.8, "{}", at(4.0, 6.5));
    // A tilted view never shows less material than there is, and not much more: a block's
    // top from 30° over (it rises tan 30° along the view's −Y), on a map finer than the
    // lattice.
    let s = block([20.0, 10.0, 8.0], 0.5);
    let tilt = Frame::from_zx(
        [10.0, 5.0, 8.0],
        [0.0, 0.5, 0.75f64.sqrt()],
        [1.0, 0.0, 0.0],
    );
    let seen = s.height_map(&tilt, &Aabb::new([-4.0, -2.0, -5.0], [4.0, 2.0, 5.0]), 0.2);
    for j in 0..seen.ny {
        for i in 0..seen.nx {
            let (p, z) = (seen.point(i, j), seen.at(i, j) as f64);
            let truth = p[1] * 30f64.to_radians().tan();
            assert!(z >= truth - 1e-5 && z < truth + 0.7, "{p:?}: {z} {truth}");
        }
    }
}

/// A part with a groove cut 1 mm into it: found, its depth right within a cell, the place on
/// the groove; before the groove nothing, and the stock left over the part measured.
#[test]
fn compare_finds_a_gouge_and_its_depth() {
    let b = Aabb::new([0.0; 3], [30.0, 20.0, 12.0]);
    let part_tris = box_triangles(&Aabb::new([0.0; 3], [30.0, 20.0, 10.0]));
    for cell in [0.5, 0.3] {
        let part = TriDexel::of_solid(Frame::WORLD, &part_tris, &b, cell);
        let mut s = TriDexel::block(Frame::WORLD, &b, cell);
        let d = s.compare(&part, 0.05);
        assert_eq!(d.gouge_count, 0);
        assert!((d.excess - 2.0).abs() < cell, "{}", d.excess);
        let face = SimCutter::new(&Tool::new(ToolKind::EndMill, 12.0), sim_eps(cell));
        for y in [0.0, 10.0, 20.0] {
            s.cut(
                &face,
                pose([-8.0, y, 10.0], UP),
                pose([38.0, y, 10.0], UP),
                1,
            );
        }
        let d = s.compare(&part, 0.05);
        assert_eq!(
            (d.gouge_count, d.gouge_depth),
            (0, 0.0),
            "{cell}: {:?}",
            d.gouges.first()
        );
        assert!(d.excess < cell, "{}", d.excess);
        // A ball groove, 1 deep at its middle, across the part.
        let ball = SimCutter::new(&Tool::new(ToolKind::BallMill, 6.0), sim_eps(cell));
        s.cut(
            &ball,
            pose([15.0, -5.0, 9.0], UP),
            pose([15.0, 25.0, 9.0], UP),
            2,
        );
        let before = s.clone();
        let d = s.mark_gouges(&part, 0.05);
        assert!(d.gouge_count > 20);
        assert!(
            (d.gouge_depth - 1.0).abs() < cell,
            "{cell}: {}",
            d.gouge_depth
        );
        assert!(d.gouge_depth <= 1.0 + 1e-3, "{cell}: {}", d.gouge_depth);
        for p in &d.gouges {
            // On the groove: 3 from the ball's centre line, under the part's top.
            let rho = (p[0] - 15.0).hypot(p[2] - 12.0);
            assert!((rho - 3.0).abs() < 1e-3 && p[2] < 10.0 - 0.04, "{p:?}");
        }
        assert_eq!(before.compare(&part, 0.05), d);
        // Marked for the mesh: red in the groove, nowhere else.
        let m = s.mesh(1);
        check_closed(&m);
        let red: Vec<usize> = (0..m.tags.len()).filter(|&i| m.tags[i] == GOUGE).collect();
        assert!(red.len() > 20);
        for i in red {
            assert!((m.positions[i][0] - 15.0).abs() < 3.0 && m.positions[i][2] < 10.0);
        }
    }
    // A gouge into a slanted face is measured square to the face: a wedge (its top rising
    // 1 in 2 along X) cut 0.5 into along its normal.
    let wedge: Vec<[P3; 3]> = {
        let (a, b, c, d) = (
            [0.0, 0.0, 0.0],
            [20.0, 0.0, 0.0],
            [20.0, 0.0, 10.0],
            [0.0, 0.0, 0.0],
        );
        let _ = d;
        let y = |p: P3| [p[0], 20.0, p[2]];
        vec![
            [a, c, b],
            [y(a), y(b), y(c)],
            [a, b, y(b)],
            [a, y(b), y(a)],
            [b, c, y(c)],
            [b, y(c), y(b)],
            [c, a, y(a)],
            [c, y(a), y(c)],
        ]
    };
    let b = Aabb::new([0.0; 3], [20.0, 20.0, 12.0]);
    let part = TriDexel::of_solid(Frame::WORLD, &wedge, &b, 0.25);
    assert!((part.volume() - 2000.0).abs() < 30.0, "{}", part.volume());
    let mut s = part.clone();
    // An end mill square to the slope, its flat end 0.5 under it, run along Y.
    let n = normalize([-1.0, 0.0, 2.0]);
    let on = [10.0, 0.0, 5.0];
    let tip = |y: f64| add([on[0], y, on[2]], scale(n, -0.5));
    let flat = SimCutter::new(&Tool::new(ToolKind::EndMill, 4.0), sim_eps(0.25));
    s.cut(&flat, pose(tip(-5.0), n), pose(tip(25.0), n), 1);
    let d = s.compare(&part, 0.05);
    // (0.5 over the middle of the slope; beside the wedge's flat sides, where the three axes
    // leave it by different faces, the least of the three: 0.5 / cos of the slope.)
    assert!(
        d.gouge_depth > 0.47 && d.gouge_depth < 0.57,
        "{}",
        d.gouge_depth
    );
    for p in &d.gouges {
        let under = (p[0] / 2.0 - p[2]) * (2.0 / 5f64.sqrt());
        assert!(under > 0.04 && under < 0.5 + 1e-3, "{p:?}");
    }
}

/// A holder dragged through the stock is found, where it is; held clear it isn't. The cutter
/// running along the floor it has just cut is not in material.
#[test]
fn hits_finds_a_holder_in_the_stock() {
    let mut s = block([60.0, 20.0, 40.0], 0.5);
    let mut tool = Tool::new(ToolKind::EndMill, 6.0);
    tool.cutting_length = 30.0;
    tool.overall_length = 20.0;
    tool.holder = Some(crate::tool::Profile::cylinder("h", 30.0, 40.0));
    let c = SimCutter::new(&tool, sim_eps(0.5));
    assert!(c.shank.is_empty() && !c.holder.is_empty());
    let (from, to) = (pose([20.0, 10.0, 25.0], UP), pose([40.0, 10.0, 25.0], UP));
    // 15 deep with 20 of stick-out: clear. 25 deep: the holder is 5 into the top.
    assert_eq!(s.hits(&c, Which::Holder, from, to, 0.05), None);
    let (from, to) = (pose([20.0, 10.0, 15.0], UP), pose([40.0, 10.0, 15.0], UP));
    let p = s
        .hits(&c, Which::Holder, from, to, 0.05)
        .expect("the holder in the stock");
    assert!(
        p[2] >= 35.0 && p[2] <= 40.0 && p[0] >= 5.0 && p[0] <= 55.0,
        "{p:?}"
    );
    assert_eq!(s.hits(&c, Which::Shank, from, to, 0.05), None);
    assert!(s.hits(&c, Which::All, from, to, 0.05).is_some());
    // Leaning the other way the holder swings clear of the block's end.
    let lean = normalize([-1.0, 0.0, 0.3]);
    let (from, to) = (pose([2.0, 10.0, 30.0], lean), pose([2.0, 10.0, 20.0], lean));
    assert_eq!(s.hits(&c, Which::Holder, from, to, 0.05), None);
    assert!(s.hits(&c, Which::Cutter, from, to, 0.05).is_some());
    // After the cut the cutter (a hair narrower) goes back along its own slot freely.
    s.cut(&c, from, to, 1);
    assert_eq!(s.hits(&c, Which::Cutter, from, to, 0.05), None);
    // A shank: flutes 10 long, 25 deep in a slot only the flutes cut.
    let mut short = Tool::new(ToolKind::EndMill, 6.0);
    short.cutting_length = 10.0;
    short.overall_length = 40.0;
    let c = SimCutter::new(&short, sim_eps(0.5));
    let (from, to) = (pose([20.0, 10.0, 15.0], UP), pose([40.0, 10.0, 15.0], UP));
    assert!(s.hits(&c, Which::Shank, from, to, 0.05).is_some());
    assert!(s.hits(&c, Which::Assembly, from, to, 0.05).is_some());
}

/// A ball mill held square to a sphere, its tip on it, run down the meridians: the stock is
/// left on the sphere (never under it by more than the moves' own chords, over it by no more
/// than the scallops), and the comparison with the sphere finds no gouge.
#[test]
fn a_ball_square_to_a_sphere_leaves_the_sphere() {
    let (big, r, cell) = (20.0f64, 3.0f64, 0.5);
    let b = Aabb::new([-22.0, -22.0, 0.0], [22.0, 22.0, 22.0]);
    let mut s = TriDexel::block(Frame::WORLD, &b, cell);
    let tool = Tool::new(ToolKind::BallMill, 2.0 * r);
    let c = SimCutter::new(&tool, sim_eps(cell));
    let on = |polar: f64, turn: f64| -> P3 {
        let (p, t) = (polar.to_radians(), turn.to_radians());
        [p.sin() * t.cos(), p.sin() * t.sin(), p.cos()]
    };
    let at = |polar: f64, turn: f64| {
        let n = on(polar, turn);
        pose(scale(n, big), n)
    };
    let mut moves = 0;
    for m in 0..90 {
        let turn = m as f64 * 4.0;
        for k in 0..32 {
            let (p0, p1) = (k as f64 * 2.0, (k + 1) as f64 * 2.0);
            s.cut(&c, at(p0, turn), at(p1, turn), 1);
            moves += 1;
        }
    }
    assert_eq!(moves, 2880);
    // Along the rays down Z over the cap (to 55° from the pole).
    let reach = big * 55f64.to_radians().sin();
    let (mut low, mut high, mut seen) = (0.0f64, 0.0f64, 0);
    for j in 0..s.n[1] {
        for i in 0..s.n[0] {
            let (x, y) = (
                b.min[0] + i as f64 * s.step[0],
                b.min[1] + j as f64 * s.step[1],
            );
            if x.hypot(y) > reach {
                continue;
            }
            let top = s.spans(2, i, j).last().expect("stock under the cap").hi as f64;
            let off = (x * x + y * y + top * top).sqrt() - big;
            low = low.min(off);
            high = high.max(off);
            seen += 1;
        }
    }
    assert!(seen > 3000);
    // A 2° chord of the sphere sags 0.003 under it; the cutter's tolerance on top of that.
    assert!(low > -(0.004 + 3.0 * c.eps), "{low}");
    // 4° apart at 55°: scallops of (20·sin 55°·4°)² / (8·3) at the most.
    assert!(high < 0.06 + 3.0 * c.eps, "{high}");
    // Against the sphere as a part (a mesh of it, inside the true one): no gouge.
    let mut tris = Vec::new();
    let (rings, around) = (48, 96);
    let pt = |i: usize, j: usize| {
        scale(
            on(
                180.0 * i as f64 / rings as f64,
                360.0 * j as f64 / around as f64,
            ),
            big,
        )
    };
    for i in 0..rings {
        for j in 0..around {
            let (a, bq, cq, d) = (pt(i, j), pt(i + 1, j), pt(i + 1, j + 1), pt(i, j + 1));
            if i > 0 {
                tris.push([a, bq, d]);
            }
            if i + 1 < rings {
                tris.push([bq, cq, d]);
            }
        }
    }
    let part = TriDexel::of_solid(Frame::WORLD, &tris, &b, cell);
    let half = 2.0 / 3.0 * std::f64::consts::PI * big.powi(3);
    assert!(
        (part.volume() - half).abs() < half * 0.01,
        "{}",
        part.volume()
    );
    let d = s.compare(&part, 0.05);
    assert_eq!(d.gouge_count, 0, "{:?} {}", d.gouges.first(), d.gouge_depth);
}
