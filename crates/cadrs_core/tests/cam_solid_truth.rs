//! The solid stock simulation against the kernel (issue #278): a few tool moves cut from a
//! block or a cylinder by `cadrs_cam::dexel::TriDexel`, and the same moves cut exactly by the
//! kernel (the cutter a solid of revolution, what it sweeps cut from the stock:
//! `Kernel::cut_swept`), in the style of brep-bench's `cnc/` cases.
//!
//! Each case compares
//! - the volume: the simulation's `volume()` with the kernel's `mass_properties`, within half a
//!   percent of what was removed plus 0.6 of a cell over the cut surface (the kernel's area of
//!   the faces the cutter left);
//! - the surface: points on the kernel result's cut faces, each moved two cells out of the solid
//!   and two cells into it along the face's normal: the simulation must have no material at the
//!   first and material at the second. Points within 2.5 cells of the stock's own surface are
//!   left out (a probe there would leave the stock), and a small stated share may miss: the ones
//!   beside a sharp edge of the cut, where two cells along the normal crosses to another face.
//!
//! Every test prints its numbers (`-- --nocapture`).

use std::f64::consts::{FRAC_1_SQRT_2, PI, TAU};
use std::time::Instant;

use cadrs_core::cam::cadrs_cam::dexel::{Pose, SimCutter, TriDexel, sim_eps};
use cadrs_core::cam::cadrs_cam::geom::{Aabb, Frame, P3};
use cadrs_core::cam::cadrs_cam::tool::{Tool, ToolKind};
use cadrs_kernel::{
    Axis, BodyId, Curve2, DefaultKernel, ExtrudeSpec, FaceId, Kernel, Loop, Motion, Plane, Profile,
    RevolveSpec, SolidMotion, Tessellation,
};
use nalgebra::{Point3, Vector3};

type V3 = Vector3<f64>;

/// How long every cutter is, from its tip to its top (mm): long enough that the top is
/// outside the stock in every case, so only the cutter's sides and tip cut.
const LENGTH: f64 = 40.0;

fn arr(v: V3) -> P3 {
    [v.x, v.y, v.z]
}

// The kernel's side.

/// The solid of revolution about the z axis of the half section through these `[radius,
/// height]` corners (closed back to the first), with the corner at `round` (if any) rounded by
/// a quarter circle that takes the place of the two sides meeting there.
fn revolved(k: &mut DefaultKernel, corners: &[[f64; 2]], round: Option<usize>) -> BodyId {
    let n = corners.len();
    let mut curves = Vec::new();
    for i in 0..n {
        let (a, b) = (corners[i], corners[(i + 1) % n]);
        if round == Some(i + 1) {
            // The quarter circle from this corner to the one after the rounded one.
            let c = corners[(i + 2) % n];
            curves.push(Curve2::conic(a, b, FRAC_1_SQRT_2, c));
        } else if round != Some(i) {
            curves.push(Curve2::line(a, b));
        }
    }
    // (The Front plane: sketch x the model's x, sketch y the model's z.)
    let half = Profile::of(Plane::xz(0.0), vec![Loop::new(curves)]);
    let spec = RevolveSpec::solid(Axis::new(Point3::origin(), V3::z()), None);
    k.revolve_with(&half, &spec).unwrap().first().unwrap()
}

/// A ball-end mill of `radius`, upright, its tip (the lowest point of its ball) at the origin.
fn ball_mill(k: &mut DefaultKernel, radius: f64) -> BodyId {
    revolved(
        k,
        &[
            [0.0, 0.0],
            [radius, 0.0],
            [radius, radius],
            [radius, LENGTH],
            [0.0, LENGTH],
        ],
        Some(1),
    )
}

/// A bull-nose mill of `radius` with corners of radius `corner`, upright, its tip at the origin.
fn bull_mill(k: &mut DefaultKernel, radius: f64, corner: f64) -> BodyId {
    revolved(
        k,
        &[
            [0.0, 0.0],
            [radius - corner, 0.0],
            [radius, 0.0],
            [radius, corner],
            [radius, LENGTH],
            [0.0, LENGTH],
        ],
        Some(2),
    )
}

/// A flat end mill of `radius`, upright, its tip at the origin: a circle extruded.
fn flat_mill(k: &mut DefaultKernel, radius: f64) -> BodyId {
    cylinder(k, Point3::origin(), V3::z(), radius, LENGTH)
}

/// A cutter with a conical point, upright, its tip at the origin: a flat of radius `tip`
/// (0 for a drill's point), widening to `radius` at the height `shoulder`, then straight up to
/// `length` (no straight part when `length` is the shoulder's height).
fn pointed_mill(
    k: &mut DefaultKernel,
    tip: f64,
    radius: f64,
    shoulder: f64,
    length: f64,
) -> BodyId {
    let mut corners = vec![[0.0, 0.0]];
    if tip > 0.0 {
        corners.push([tip, 0.0]);
    }
    corners.push([radius, shoulder]);
    if length > shoulder + 1e-9 {
        corners.push([radius, length]);
    }
    corners.push([0.0, length]);
    revolved(k, &corners, None)
}

fn cylinder(k: &mut DefaultKernel, base: Point3<f64>, dir: V3, radius: f64, length: f64) -> BodyId {
    let circle = Profile::of(
        Plane::square_to(base, dir),
        vec![Loop::circle([0.0, 0.0], radius)],
    );
    k.extrude_with(&circle, &ExtrudeSpec::normal_to(&circle, length))
        .unwrap()
        .first()
        .unwrap()
}

/// An upright cutter (its tip at the origin) with its tip at `tip` and its axis along `axis`:
/// turned about the level line square to both, then moved.
fn placed(k: &mut DefaultKernel, cutter: BodyId, tip: V3, axis: V3) -> BodyId {
    let axis = axis.normalize();
    let about = V3::z().cross(&axis);
    let mut body = cutter;
    if about.norm() > 1e-12 {
        let turn = Motion::rotation(&Axis::new(Point3::origin(), about), axis.z.acos());
        body = k.transform_motion(body, &turn).unwrap().first().unwrap();
    }
    k.transform_motion(body, &Motion::translation(tip))
        .unwrap()
        .first()
        .unwrap()
}

/// `stock` less everything `tool` passes through under `motion` (the kernel's cut by a swept
/// solid), which must be the exact cut: the kernel's fallback, a mesh of flat facets, is no
/// ground truth.
fn cut_swept(k: &mut DefaultKernel, stock: BodyId, tool: BodyId, motion: &SolidMotion) -> BodyId {
    let made = k
        .cut_swept(stock, tool, motion)
        .expect("the kernel cuts the stock by the swept cutter")
        .first()
        .unwrap();
    let faces = k.faces(made).unwrap();
    assert!(
        faces.len() < 500,
        "the kernel made the cut as a mesh ({} faces), not exactly",
        faces.len()
    );
    made
}

// The stock, on both sides.

#[derive(Clone, Copy)]
enum Stock {
    /// The box from `min` to `max`.
    Block { min: P3, max: P3 },
    /// The cylinder of `radius` about the model's axis `axis` (0: x, 2: z) from `from` to `to`
    /// along it.
    Cylinder {
        axis: usize,
        radius: f64,
        from: f64,
        to: f64,
    },
}

impl Stock {
    fn body(&self, k: &mut DefaultKernel) -> BodyId {
        match *self {
            Stock::Block { min, max } => {
                let base = Profile::of(
                    Plane::xy(min[2]),
                    vec![Loop::rectangle(min[0], min[1], max[0], max[1])],
                );
                k.extrude_with(&base, &ExtrudeSpec::normal_to(&base, max[2] - min[2]))
                    .unwrap()
                    .first()
                    .unwrap()
            }
            Stock::Cylinder {
                axis,
                radius,
                from,
                to,
            } => {
                let mut dir = V3::zeros();
                dir[axis] = 1.0;
                cylinder(k, Point3::origin() + dir * from, dir, radius, to - from)
            }
        }
    }

    fn bounds(&self) -> Aabb {
        match *self {
            Stock::Block { min, max } => Aabb::new(min, max),
            Stock::Cylinder {
                axis,
                radius,
                from,
                to,
            } => {
                let (mut min, mut max) = ([-radius; 3], [radius; 3]);
                (min[axis], max[axis]) = (from, to);
                Aabb::new(min, max)
            }
        }
    }

    /// How far inside the stock `p` is (mm from its nearest face; negative outside).
    fn depth(&self, p: P3) -> f64 {
        match *self {
            Stock::Block { min, max } => (0..3)
                .map(|i| (p[i] - min[i]).min(max[i] - p[i]))
                .fold(f64::INFINITY, f64::min),
            Stock::Cylinder {
                axis,
                radius,
                from,
                to,
            } => {
                let rho = p[(axis + 1) % 3].hypot(p[(axis + 2) % 3]);
                (radius - rho).min(p[axis] - from).min(to - p[axis])
            }
        }
    }

    /// The stock for the simulation on a lattice of `cell`: a block as it is, a cylinder from
    /// the kernel's own (`body`), meshed finely (0.0005 mm: a few mm³ short of the cylinder).
    fn dexel(&self, k: &DefaultKernel, body: BodyId, cell: f64) -> TriDexel {
        match self {
            Stock::Block { .. } => TriDexel::block(Frame::WORLD, &self.bounds(), cell),
            Stock::Cylinder { .. } => {
                let fine = Tessellation {
                    deflection: 0.0005,
                    angle: PI / 360.0,
                };
                let mesh = k.tessellate(body, fine).unwrap();
                let tris: Vec<[P3; 3]> = mesh
                    .indices
                    .iter()
                    .map(|t| t.map(|i| arr(mesh.positions[i as usize].coords)))
                    .collect();
                // (A lattice a cell wider all round than the cylinder: `of_solid` leaves the rays
                // in three of a lattice's outer faces empty, so a solid that reaches them comes
                // out short; see `a_block_from_its_own_mesh_is_the_block`.)
                TriDexel::of_solid(Frame::WORLD, &tris, &self.bounds().grown(cell), cell)
            }
        }
    }
}

// The simulation's side.

/// A tool of `kind` whose flutes are the whole cutter of `length`.
fn tool(kind: ToolKind, diameter: f64, length: f64) -> Tool {
    let mut t = Tool::new(kind, diameter);
    t.cutting_length = length;
    t.overall_length = length;
    t
}

/// Cuts the moves from each pose to the next out of `stock`; the volume removed, as the cuts
/// report it.
fn run(stock: &mut TriDexel, tool: &Tool, poses: &[Pose]) -> f64 {
    let cutter = SimCutter::new(tool, sim_eps(stock.cell));
    poses
        .windows(2)
        .map(|w| stock.cut(&cutter, w[0], w[1], 1))
        .sum()
}

fn pose(tip: V3, axis: V3) -> Pose {
    Pose {
        tip: arr(tip),
        axis: arr(axis.normalize()),
    }
}

/// Whether the simulated stock has material at `p`: by the ray nearest `p` of each of the three
/// grids, two of the three agreeing. (The nearest ray is up to half a cell off to each side.)
fn has_material(stock: &TriDexel, p: P3) -> bool {
    let q = stock.to_lattice(p);
    let size = stock.size();
    if (0..3).any(|i| q[i] < 0.0 || q[i] > size[i]) {
        return false;
    }
    let index = |i: usize| ((q[i] / stock.step[i]).round() as usize).min(stock.n[i] - 1);
    let votes = (0..3)
        .filter(|&axis| {
            let (u, v) = (index((axis + 1) % 3), index((axis + 2) % 3));
            let x = q[axis] as f32;
            stock
                .spans(axis, u, v)
                .iter()
                .any(|s| s.lo <= x && x <= s.hi)
        })
        .count();
    votes >= 2
}

// The comparison.

/// What a case measured.
struct Measured {
    /// The kernel's volume of the stock and of the cut stock (mm³).
    stock_volume: f64,
    kernel_volume: f64,
    /// The simulation's volume of the cut stock, and what its cuts say they removed.
    sim_volume: f64,
    sim_removed: f64,
    /// The area of the faces the cutter left, by the kernel (mm²).
    cut_area: f64,
    /// The volume tolerance (mm³).
    allowed: f64,
    /// Surface points probed, and those where the simulation still has material two cells
    /// outside the kernel's solid, or has none two cells inside it.
    tested: usize,
    left_over: usize,
    cut_away: usize,
}

impl Measured {
    fn removed(&self) -> f64 {
        self.stock_volume - self.kernel_volume
    }

    fn missed(&self) -> f64 {
        (self.left_over + self.cut_away) as f64 / self.tested.max(1) as f64
    }
}

/// Compares the simulated `sim` with the kernel's `result`, both cut from `stock` (`blank` in
/// the kernel), and prints the numbers. The volumes must agree within the tolerance and at most
/// `misses` of the surface probes may disagree.
#[allow(clippy::too_many_arguments)]
fn compare(
    name: &str,
    k: &DefaultKernel,
    stock: &Stock,
    blank: BodyId,
    result: BodyId,
    sim: &TriDexel,
    sim_removed: f64,
    misses: f64,
    started: Instant,
) -> Measured {
    let cell = sim.cell;
    let stock_volume = k.mass_properties(blank).unwrap().volume;
    let kernel_volume = k.mass_properties(result).unwrap().volume;
    let mesh = k.tessellate(result, Tessellation::default()).unwrap();
    let triangles: Vec<([P3; 3], FaceId)> = mesh
        .indices
        .iter()
        .zip(&mesh.triangle_faces)
        .map(|(t, f)| (t.map(|i| arr(mesh.positions[i as usize].coords)), *f))
        .collect();
    let middle = |t: &[P3; 3]| [0, 1, 2].map(|i| (t[0][i] + t[1][i] + t[2][i]) / 3.0);
    // The faces the cutter left: those with a point well inside the stock (the stock's own
    // faces, trimmed, are on its surface, within the mesh's 0.01 mm).
    let infos = k.faces(result).unwrap();
    let mut is_cut = vec![false; infos.len()];
    for (t, f) in &triangles {
        if stock.depth(middle(t)) > 0.05 {
            is_cut[f.0 as usize] = true;
        }
    }
    let cut_area: f64 = infos
        .iter()
        .filter(|f| is_cut[f.id.0 as usize])
        .map(|f| f.area)
        .sum();
    // Points spread over the cut faces by area, about one to every two cells each way (a
    // fixed sequence of pseudo-random places in each triangle).
    let (mut tested, mut left_over, mut cut_away) = (0, 0, 0);
    let mut owed = 0.0;
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut random = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    let mut examples = Vec::new();
    for (t, f) in &triangles {
        if !is_cut[f.0 as usize] {
            continue;
        }
        let [a, b, c] = t.map(V3::from);
        // (Counter-clockwise seen from outside: the normal points out of the solid.)
        let normal = (b - a).cross(&(c - a));
        let area = normal.norm() / 2.0;
        if area < 1e-12 {
            continue;
        }
        let normal = normal.normalize();
        owed += area / (4.0 * cell * cell);
        while owed >= 1.0 {
            owed -= 1.0;
            let (mut u, mut v) = (random(), random());
            if u + v > 1.0 {
                (u, v) = (1.0 - u, 1.0 - v);
            }
            let p = a + (b - a) * u + (c - a) * v;
            if stock.depth(arr(p)) < 2.5 * cell {
                continue;
            }
            tested += 1;
            let outside = has_material(sim, arr(p + normal * 2.0 * cell));
            let inside = has_material(sim, arr(p - normal * 2.0 * cell));
            left_over += outside as usize;
            cut_away += (!inside && !outside) as usize;
            if (outside || !inside) && examples.len() < 5 {
                examples.push((arr(p), arr(normal), outside, inside));
            }
        }
    }
    let m = Measured {
        stock_volume,
        kernel_volume,
        sim_volume: sim.volume(),
        sim_removed,
        cut_area,
        allowed: 0.0,
        tested,
        left_over,
        cut_away,
    };
    let m = Measured {
        allowed: 0.005 * m.removed() + cut_area * cell * 0.6,
        ..m
    };
    let error = m.sim_volume - m.kernel_volume;
    println!(
        "{name}: cell {cell}, kernel {:.2} mm³ (stock {:.2}, removed {:.2}), simulated {:.2} \
         (its cuts removed {:.2}), error {:+.2} mm³ = {:+.3} % of the removed (allowed ±{:.2}), \
         cut surface {:.1} mm², {} points probed, {} missed ({:.2} %: {} left over, {} cut away), \
         {:.2} s",
        m.kernel_volume,
        m.stock_volume,
        m.removed(),
        m.sim_volume,
        m.sim_removed,
        error,
        100.0 * error / m.removed(),
        m.allowed,
        m.cut_area,
        m.tested,
        m.left_over + m.cut_away,
        100.0 * m.missed(),
        m.left_over,
        m.cut_away,
        started.elapsed().as_secs_f64(),
    );
    assert!(
        m.removed() > 1.0,
        "{name}: the kernel's cut removed nothing"
    );
    assert!(
        error.abs() <= m.allowed,
        "{name}: the simulation's volume {:.3} is not the kernel's {:.3} within {:.3}",
        m.sim_volume,
        m.kernel_volume,
        m.allowed
    );
    assert!(m.tested > 200, "{name}: only {} points probed", m.tested);
    assert!(
        m.missed() <= misses,
        "{name}: {} of {} surface points disagree ({} with material left outside the kernel's \
         solid, {} with none inside it), e.g. (point, normal, material outside, material \
         inside) {examples:?}",
        m.left_over + m.cut_away,
        m.tested,
        m.left_over,
        m.cut_away
    );
    m
}

/// The block every block case cuts: 60 by 40 by 30, a corner at the origin.
const BLOCK: Stock = Stock::Block {
    min: [0.0, 0.0, 0.0],
    max: [60.0, 40.0, 30.0],
};

/// The blank of brep-bench's `cnc/five-axis-dome`: the cylinder of radius 50 about the z axis
/// from z = 0 to 45.
const DOME_BLANK: Stock = Stock::Cylinder {
    axis: 2,
    radius: 50.0,
    from: 0.0,
    to: 45.0,
};

/// The dome's outward normal at a latitude and a longitude (degrees).
fn dome_normal(lat: f64, lon: f64) -> V3 {
    let (l, p) = (lat.to_radians(), lon.to_radians());
    V3::new(l.cos() * p.cos(), l.cos() * p.sin(), l.sin())
}

#[test]
fn a_ball_mill_slid_on_a_compound_angle_leaves_a_capsule_groove() {
    // The mill upright, its tip going from outside one end of the block to outside the other,
    // across it and downhill at once: a groove with a half-round floor that deepens.
    let started = Instant::now();
    let (from, to) = (V3::new(-8.0, 10.0, 26.0), V3::new(68.0, 30.0, 14.0));
    let mut k = DefaultKernel::new();
    let blank = BLOCK.body(&mut k);
    let mill = ball_mill(&mut k, 4.0);
    let at = placed(&mut k, mill, from, V3::z());
    let result = cut_swept(&mut k, blank, at, &SolidMotion::slide(to - from));

    let mut sim = BLOCK.dexel(&k, blank, 0.5);
    let removed = run(
        &mut sim,
        &tool(ToolKind::BallMill, 8.0, LENGTH),
        &[pose(from, V3::z()), pose(to, V3::z())],
    );
    compare(
        "ball groove",
        &k,
        &BLOCK,
        blank,
        result,
        &sim,
        removed,
        0.01,
        started,
    );
}

#[test]
fn a_flat_mill_slid_level_leaves_a_slot_of_known_volume() {
    // The mill upright, 8 deep, from clear of the block's x = 0 face to a stop inside it, on a
    // line 17° off the x axis. In plan the slot is a strip 10 wide cut off obliquely by the
    // face, which holds as much as one cut off square at its centre line's crossing, and a half
    // disc at the stop; so the volume removed is 8 · (10 · the centre line's length inside +
    // π 5² / 2). (The floor, at z = 22, is on a plane of the lattice: the rays along x and y
    // in that plane count as cut, so the simulation removes a quarter cell too much over the
    // floor by two grids of three, 2 % here. Midway between planes it would be exact.)
    let started = Instant::now();
    let (from, to) = (V3::new(-10.0, 12.0, 22.0), V3::new(35.0, 26.0, 22.0));
    let mut k = DefaultKernel::new();
    let blank = BLOCK.body(&mut k);
    let mill = flat_mill(&mut k, 5.0);
    let at = placed(&mut k, mill, from, V3::z());
    let result = cut_swept(&mut k, blank, at, &SolidMotion::slide(to - from));

    let mut sim = BLOCK.dexel(&k, blank, 0.5);
    let removed = run(
        &mut sim,
        &tool(ToolKind::EndMill, 10.0, LENGTH),
        &[pose(from, V3::z()), pose(to, V3::z())],
    );
    let m = compare(
        "flat slot",
        &k,
        &BLOCK,
        blank,
        result,
        &sim,
        removed,
        0.01,
        started,
    );
    let inside = (to - from).norm() * 35.0 / 45.0;
    let exact = 8.0 * (10.0 * inside + PI * 25.0 / 2.0);
    println!(
        "flat slot: removed in closed form {exact:.3}, by the kernel {:.3}, by the simulation {:.3}",
        m.removed(),
        m.stock_volume - m.sim_volume
    );
    assert!(
        (m.removed() - exact).abs() < 1e-6,
        "the kernel removed {}, not {exact}",
        m.removed()
    );
    assert!(
        (m.stock_volume - m.sim_volume - exact).abs() <= m.allowed,
        "the simulation removed {}, not {exact}",
        m.stock_volume - m.sim_volume
    );
}

#[test]
fn a_ball_mill_square_to_a_dome_turned_up_a_meridian() {
    // One meridian pass of brep-bench's `cnc/five-axis-dome` (its eleventh, at longitude 20°):
    // the ball-end mill of radius 4 with its tip on the dome of radius 40 and its axis along the
    // dome's normal, turned from latitude 3° to 75° about the level line through the dome's
    // centre square to the meridian. The simulation gets the pass a degree at a move: between
    // two such poses its tip goes straight, 0.0015 mm inside the arc.
    let started = Instant::now();
    let lon = 20f64;
    let mut k = DefaultKernel::new();
    let blank = DOME_BLANK.body(&mut k);
    let mill = ball_mill(&mut k, 4.0);
    let low = dome_normal(3.0, lon);
    let at = placed(&mut k, mill, low * 40.0, low);
    let level = V3::new(lon.to_radians().sin(), -lon.to_radians().cos(), 0.0);
    let about = Axis::new(Point3::origin(), level);
    let turn = SolidMotion::swing(about, 0.0, 72f64.to_radians());
    let result = cut_swept(&mut k, blank, at, &turn);

    let mut sim = DOME_BLANK.dexel(&k, blank, 0.5);
    let poses: Vec<Pose> = (0..=72)
        .map(|i| {
            let n = dome_normal(3.0 + i as f64, lon);
            pose(n * 40.0, n)
        })
        .collect();
    let removed = run(&mut sim, &tool(ToolKind::BallMill, 8.0, LENGTH), &poses);
    compare(
        "dome meridian",
        &k,
        &DOME_BLANK,
        blank,
        result,
        &sim,
        removed,
        0.01,
        started,
    );
}

#[test]
fn a_bull_mill_turned_once_round_the_axis_leaves_a_ring() {
    // One roughing circle of the same case: the bull-nose mill (radius 8, corners of 2) upright
    // with its tip at height 21, once round the z axis at the radius the case gives that
    // height, 9 outside the dome's circle there. The mill reaches past the blank's side, so the
    // ring is a step round its rim with a rounded inner corner. The simulation gets the circle
    // 2° at a move (the chord 0.007 mm inside the arc).
    let started = Instant::now();
    let z = 21f64;
    let radius = (40f64 * 40.0 - z * z).sqrt() + 9.0;
    let mut k = DefaultKernel::new();
    let blank = DOME_BLANK.body(&mut k);
    let mill = bull_mill(&mut k, 8.0, 2.0);
    let at = placed(&mut k, mill, V3::new(radius, 0.0, z), V3::z());
    let round = SolidMotion::swing(Axis::new(Point3::origin(), V3::z()), 0.0, TAU);
    let result = cut_swept(&mut k, blank, at, &round);

    let mut sim = DOME_BLANK.dexel(&k, blank, 0.5);
    let mut bull = tool(ToolKind::BullMill, 16.0, LENGTH);
    bull.corner_radius = 2.0;
    let poses: Vec<Pose> = (0..=180)
        .map(|i| {
            let a = (2.0 * i as f64).to_radians();
            pose(V3::new(radius * a.cos(), radius * a.sin(), z), V3::z())
        })
        .collect();
    let removed = run(&mut sim, &bull, &poses);
    compare(
        "bull ring",
        &k,
        &DOME_BLANK,
        blank,
        result,
        &sim,
        removed,
        0.01,
        started,
    );
}

#[test]
fn a_leaning_flat_mill_slid_level_leaves_a_slot_of_elliptic_section() {
    // The flat end mill of radius 5 leaning 30° to the side of its move (a level line 20° off
    // the x axis), from outside the block to a stop inside it: seen along the move its end is
    // an ellipse, so the slot's floor is one, and one wall overhangs.
    let started = Instant::now();
    let along = V3::new(20f64.to_radians().cos(), 20f64.to_radians().sin(), 0.0);
    let side = V3::z().cross(&along);
    let axis = V3::z() * 30f64.to_radians().cos() + side * 30f64.to_radians().sin();
    let from = V3::new(-12.0, 8.0, 22.0);
    let to = from + along * 50.0;
    let mut k = DefaultKernel::new();
    let blank = BLOCK.body(&mut k);
    let mill = flat_mill(&mut k, 5.0);
    let at = placed(&mut k, mill, from, axis);
    let result = cut_swept(&mut k, blank, at, &SolidMotion::slide(to - from));

    let mut sim = BLOCK.dexel(&k, blank, 0.5);
    let removed = run(
        &mut sim,
        &tool(ToolKind::EndMill, 10.0, LENGTH),
        &[pose(from, axis), pose(to, axis)],
    );
    compare(
        "leaning slot",
        &k,
        &BLOCK,
        blank,
        result,
        &sim,
        removed,
        0.01,
        started,
    );
}

#[test]
fn a_taper_mill_screwed_about_the_x_axis_leaves_a_thread_space() {
    // The 4-axis rotary move: one turn of the thread space of brep-bench's worm (lead 2π, its
    // sides 20° from square to the axis, from radius 7.5 out) on a bar of radius 12.5 about the
    // x axis. The cutter is the space's own section turned about a radial line: a taper mill
    // with a flat tip, 6 long (it ends outside the bar, and narrow enough there that its turns
    // don't overlap), its tip 7.5 from the x axis and its axis radial, turned once round the x
    // axis while sliding 2π along it. The simulation gets the turn 2° at a move.
    let started = Instant::now();
    let bar = Stock::Cylinder {
        axis: 0,
        radius: 12.5,
        from: 0.0,
        to: 30.0,
    };
    let side = 20f64.to_radians().tan();
    let (tip, length) = (PI / 2.0 - 2.5 * side, 6.0);
    let top = tip + length * side;
    let mut k = DefaultKernel::new();
    let blank = bar.body(&mut k);
    let mill = pointed_mill(&mut k, tip, top, length, length);
    let at = placed(&mut k, mill, V3::new(10.0, 0.0, 7.5), V3::z());
    let screw = SolidMotion::helical(Axis::new(Point3::origin(), V3::x()), TAU, 0.0, TAU);
    let result = cut_swept(&mut k, blank, at, &screw);

    let mut sim = bar.dexel(&k, blank, 0.25);
    let mut taper = tool(ToolKind::TaperMill, 2.0 * top, length);
    taper.angle = 40.0;
    taper.tip_diameter = 2.0 * tip;
    let poses: Vec<Pose> = (0..=180)
        .map(|i| {
            let a = (2.0 * i as f64).to_radians();
            let radial = V3::new(0.0, -a.sin(), a.cos());
            pose(V3::new(10.0 + a, 0.0, 0.0) + radial * 7.5, radial)
        })
        .collect();
    let removed = run(&mut sim, &taper, &poses);
    compare(
        "thread space",
        &k,
        &bar,
        blank,
        result,
        &sim,
        removed,
        0.02,
        started,
    );
}

#[test]
fn a_drill_plunged_along_a_leaning_axis_leaves_a_hole_with_a_conical_floor() {
    // A drill of radius 2.5 with a 90° point, leaning 25° from upright, plunged along its own
    // axis from 4 above the block's top to 18 below it.
    let started = Instant::now();
    let (lean, round) = (25f64.to_radians(), 35f64.to_radians());
    let axis = V3::new(
        lean.sin() * round.cos(),
        lean.sin() * round.sin(),
        lean.cos(),
    );
    let entry = V3::new(25.0, 18.0, 30.0);
    let (from, to) = (entry + axis * 4.0, entry - axis * 18.0);
    let mut k = DefaultKernel::new();
    let blank = BLOCK.body(&mut k);
    let drill = pointed_mill(&mut k, 0.0, 2.5, 2.5, LENGTH);
    let at = placed(&mut k, drill, from, axis);
    let result = cut_swept(&mut k, blank, at, &SolidMotion::slide(to - from));

    let mut sim = BLOCK.dexel(&k, blank, 0.25);
    let mut bit = tool(ToolKind::Drill, 5.0, LENGTH);
    bit.angle = 90.0;
    let removed = run(&mut sim, &bit, &[pose(from, axis), pose(to, axis)]);
    compare(
        "leaning hole",
        &k,
        &BLOCK,
        blank,
        result,
        &sim,
        removed,
        0.02,
        started,
    );
}

#[test]
fn a_block_from_its_own_mesh_is_the_block() {
    // The block as the kernel meshes it (twelve triangles, exact), made a solid stock on the
    // lattice over its own box: it should be `TriDexel::block`'s, all 72000 mm³. `of_solid`
    // fills each ray by a rasteriser's rule, which counts a ray lying in a face of the mesh on
    // one side only, so the rays in the lattice's faces y = 0, z = 30 (along x), x = 60, z = 0
    // (along y) and x = 0, y = 0 (along z) are empty. Each stands for half a cell of material.
    let mut k = DefaultKernel::new();
    let body = BLOCK.body(&mut k);
    let mesh = k.tessellate(body, Tessellation::default()).unwrap();
    let tris: Vec<[P3; 3]> = mesh
        .indices
        .iter()
        .map(|t| t.map(|i| arr(mesh.positions[i as usize].coords)))
        .collect();
    let solid = TriDexel::of_solid(Frame::WORLD, &tris, &BLOCK.bounds(), 0.5);
    let block = TriDexel::block(Frame::WORLD, &BLOCK.bounds(), 0.5);
    assert!((block.volume() - 72000.0).abs() < 1e-6);
    assert!(
        (solid.volume() - 72000.0).abs() < 1.0,
        "the block's mesh as a solid stock holds {} mm³",
        solid.volume()
    );
    // A point just inside the middle of each face is in material.
    for axis in 0..3 {
        for end in [0.25, -0.25] {
            let mut p = [30.0, 20.0, 15.0];
            p[axis] = if end > 0.0 { end } else { 2.0 * p[axis] + end };
            assert!(has_material(&solid, p), "no material at {p:?}");
        }
    }
}
