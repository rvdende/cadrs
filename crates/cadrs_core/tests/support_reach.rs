//! How far two parts' display meshes reach into each other (gear pairs in mesh), shared by
//! the gear tests.

use cadrs_core::Part;

type V3 = [f64; 3];

fn sub3(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot3(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross3(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// The triangles of a part's display mesh.
fn triangles(p: &Part) -> Vec<[V3; 3]> {
    p.solid
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| t.map(|i| p.solid.positions[i as usize]))
        .collect()
}

/// How far `p` is from the triangle a, b, c (Ericson's closest point).
fn to_triangle(p: V3, a: V3, b: V3, c: V3) -> f64 {
    let (ab, ac, ap) = (sub3(b, a), sub3(c, a), sub3(p, a));
    let at = |v: V3| dot3(sub3(p, v), sub3(p, v)).sqrt();
    let mix = |u: V3, v: V3, t: f64| [0, 1, 2].map(|k| u[k] + t * (v[k] - u[k]));
    let (d1, d2) = (dot3(ab, ap), dot3(ac, ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return at(a);
    }
    let bp = sub3(p, b);
    let (d3, d4) = (dot3(ab, bp), dot3(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return at(b);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return at(mix(a, b, d1 / (d1 - d3)));
    }
    let cp = sub3(p, c);
    let (d5, d6) = (dot3(ab, cp), dot3(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return at(c);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return at(mix(a, c, d2 / (d2 - d6)));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 {
        return at(mix(b, c, (d4 - d3) / ((d4 - d3) + (d5 - d6))));
    }
    let n = cross3(ab, ac);
    dot3(ap, n).abs() / dot3(n, n).sqrt()
}

/// A mesh's triangles by the 1 mm cell of the xy plane they are over.
struct Grid {
    lo: [f64; 2],
    n: [usize; 2],
    cells: Vec<Vec<[V3; 3]>>,
}

impl Grid {
    fn new(mesh: &[[V3; 3]], lo: V3, hi: V3) -> Self {
        let n = [0, 1].map(|k| ((hi[k] - lo[k]).ceil() as usize).max(1));
        let mut cells = vec![Vec::new(); n[0] * n[1]];
        for t in mesh {
            let range = |k: usize| {
                let (a, b) = t
                    .iter()
                    .fold((f64::MAX, f64::MIN), |(a, b), q| (a.min(q[k]), b.max(q[k])));
                let cell = |v: f64| ((v - lo[k]).floor().max(0.0) as usize).min(n[k] - 1);
                (b >= lo[k] && a <= hi[k]).then(|| (cell(a), cell(b)))
            };
            let (Some((x0, x1)), Some((y0, y1))) = (range(0), range(1)) else {
                continue;
            };
            for x in x0..=x1 {
                for y in y0..=y1 {
                    cells[x * n[1] + y].push(*t);
                }
            }
        }
        Self {
            lo: [lo[0], lo[1]],
            n,
            cells,
        }
    }

    fn cell(&self, p: V3) -> Option<[usize; 2]> {
        let c = [0, 1].map(|k| (p[k] - self.lo[k]).floor());
        (0..2)
            .all(|k| c[k] >= 0.0 && (c[k] as usize) < self.n[k])
            .then(|| [c[0] as usize, c[1] as usize])
    }

    /// Whether `p` is inside the closed mesh: a ray up from it crosses the mesh an odd number
    /// of times.
    fn inside(&self, p: V3) -> bool {
        let Some([x, y]) = self.cell(p) else {
            return false;
        };
        let mut crossings = 0;
        for [a, b, c] in &self.cells[x * self.n[1] + y] {
            // The triangle over (px, py), by its edges' signs seen from above.
            let side =
                |u: &V3, v: &V3| (v[0] - u[0]) * (p[1] - u[1]) - (v[1] - u[1]) * (p[0] - u[0]);
            let (s0, s1, s2) = (side(a, b), side(b, c), side(c, a));
            let area = s0 + s1 + s2;
            if area.abs() < 1e-14 || !((s0 >= 0.0) == (s1 >= 0.0) && (s1 >= 0.0) == (s2 >= 0.0)) {
                continue;
            }
            let z = (s1 * a[2] + s2 * b[2] + s0 * c[2]) / area;
            if z > p[2] {
                crossings += 1;
            }
        }
        crossings % 2 == 1
    }

    /// How far `p` is from the nearest of the mesh's triangles near it (those over the cells
    /// round its own).
    fn depth(&self, p: V3) -> f64 {
        let Some([x, y]) = self.cell(p) else {
            return 0.0;
        };
        let mut best = f64::MAX;
        for cx in x.saturating_sub(2)..=(x + 2).min(self.n[0] - 1) {
            for cy in y.saturating_sub(2)..=(y + 2).min(self.n[1] - 1) {
                for [a, b, c] in &self.cells[cx * self.n[1] + cy] {
                    best = best.min(to_triangle(p, *a, *b, *c));
                }
            }
        }
        if best == f64::MAX { 0.0 } else { best }
    }
}

/// How far each of two parts' display meshes reaches into the other (the deepest, and how many
/// vertices).
pub fn reach_of(parts: &[Part]) -> (f64, usize) {
    assert_eq!(parts.len(), 2);
    // Only where the two gears' boxes overlap.
    let bounds = |p: &Part| {
        p.solid
            .positions
            .iter()
            .fold(([f64::MAX; 3], [f64::MIN; 3]), |(lo, hi), q| {
                (
                    [0, 1, 2].map(|k| lo[k].min(q[k])),
                    [0, 1, 2].map(|k| hi[k].max(q[k])),
                )
            })
    };
    let (ba, bb) = (bounds(&parts[0]), bounds(&parts[1]));
    let lo = [0, 1, 2].map(|k| ba.0[k].max(bb.0[k]) - 1.0);
    let hi = [0, 1, 2].map(|k| ba.1[k].min(bb.1[k]) + 1.0);
    let mut worst = (0.0f64, 0);
    let (mut over, mut deepest) = ([0usize; 5], ([0.0; 3], 0));
    for (a, b) in [(0, 1), (1, 0)] {
        let grid = Grid::new(&triangles(&parts[a]), lo, hi);
        for p in &parts[b].solid.positions {
            // (A hair off the vertex, so the ray up meets no edge of the other mesh by chance.)
            let p = [p[0] + 1.3e-7, p[1] + 2.1e-7, p[2]];
            if (0..3).all(|k| p[k] >= lo[k] && p[k] <= hi[k]) && grid.inside(p) {
                let d = grid.depth(p);
                if d > worst.0 {
                    deepest = (p, b);
                }
                for (k, limit) in [0.05, 0.2, 0.5, 1.0, 3.0].into_iter().enumerate() {
                    if d > limit {
                        over[k] += 1;
                    }
                }
                worst = (worst.0.max(d), worst.1 + 1);
            }
        }
    }
    eprintln!("deeper than 0.05/0.2/0.5/1/3 mm: {over:?}; the deepest at {deepest:?}");
    worst
}
