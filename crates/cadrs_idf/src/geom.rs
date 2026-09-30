//! Outline loops and their geometry.
//!
//! An IDF loop is a list of `(x, y, angle)` points. The first point's angle is ignored. For
//! each later point, angle 0 means a straight segment from the previous point, any other value
//! an arc from the previous point with that included angle in degrees (positive =
//! counter-clockwise), and ±360 a full circle centred on the previous point and passing through
//! this one (IDF 3.0; a circle is a whole loop).

use serde::{Deserialize, Serialize};

/// Closure tolerance for [`Loop::is_closed`] (in the loop's units).
pub const CLOSE_TOL: f64 = 1e-9;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LoopPoint {
    pub x: f64,
    pub y: f64,
    /// Included angle in degrees of the segment ending at this point.
    pub angle: f64,
}

impl LoopPoint {
    pub fn new(x: f64, y: f64, angle: f64) -> LoopPoint {
        LoopPoint { x, y, angle }
    }
}

/// One loop of an outline, with its IDF label (0 = outline, counter-clockwise; 1.. = cut-outs,
/// clockwise, in board outlines; 0/1 = CCW/CW elsewhere).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Loop {
    pub label: u32,
    pub points: Vec<LoopPoint>,
}

/// A 2D point `[x, y]`.
pub type P2 = [f64; 2];

/// One edge of a loop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Segment {
    Line { start: P2, end: P2 },
    /// `sweep` in degrees, positive counter-clockwise.
    Arc { start: P2, end: P2, center: P2, radius: f64, sweep: f64 },
    Circle { center: P2, radius: f64 },
}

/// Axis-aligned bounding box.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BBox {
    pub min: P2,
    pub max: P2,
}

impl BBox {
    pub fn empty() -> BBox {
        BBox { min: [f64::INFINITY; 2], max: [f64::NEG_INFINITY; 2] }
    }

    pub fn add(&mut self, p: P2) {
        for ((lo, hi), v) in self.min.iter_mut().zip(&mut self.max).zip(p) {
            *lo = lo.min(v);
            *hi = hi.max(v);
        }
    }

    pub fn union(&self, o: &BBox) -> BBox {
        if o.is_empty() {
            return *self;
        }
        let mut b = *self;
        b.add(o.min);
        b.add(o.max);
        b
    }

    pub fn width(&self) -> f64 {
        self.max[0] - self.min[0]
    }

    pub fn height(&self) -> f64 {
        self.max[1] - self.min[1]
    }

    pub fn is_empty(&self) -> bool {
        self.min[0] > self.max[0]
    }

    /// True if the boxes overlap with positive area.
    pub fn overlaps(&self, o: &BBox) -> bool {
        self.min[0] < o.max[0] && o.min[0] < self.max[0] && self.min[1] < o.max[1] && o.min[1] < self.max[1]
    }
}

fn is_circle_angle(a: f64) -> bool {
    a.abs() == 360.0
}

/// Sine and cosine of an angle in degrees, exact at multiples of 90°.
pub fn sin_cos_deg(deg: f64) -> (f64, f64) {
    let r = deg.rem_euclid(360.0);
    if r == 0.0 {
        (0.0, 1.0)
    } else if r == 90.0 {
        (1.0, 0.0)
    } else if r == 180.0 {
        (0.0, -1.0)
    } else if r == 270.0 {
        (-1.0, 0.0)
    } else {
        deg.to_radians().sin_cos()
    }
}

/// Centre and radius of the arc from `a` to `b` with included angle `sweep` degrees.
pub fn arc_center(a: P2, b: P2, sweep: f64) -> (P2, f64) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let chord = dx.hypot(dy);
    let half = sweep.to_radians() / 2.0;
    let m = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    if chord == 0.0 {
        return (m, 0.0);
    }
    let radius = chord / (2.0 * half.sin().abs());
    // Signed distance from the chord midpoint to the centre, along the chord's left normal.
    let d = if sweep.abs() == 180.0 { 0.0 } else { (chord / 2.0) / half.tan() };
    let n = [-dy / chord, dx / chord];
    ([m[0] + n[0] * d, m[1] + n[1] * d], radius)
}

impl Loop {
    pub fn new(label: u32, points: Vec<LoopPoint>) -> Loop {
        Loop { label, points }
    }

    /// Closed polygon (or arc chain) through `(x, y, angle)` triples; the first point is
    /// repeated at the end if it isn't already.
    pub fn from_triples(label: u32, pts: &[(f64, f64, f64)]) -> Loop {
        let mut points: Vec<LoopPoint> = pts.iter().map(|&(x, y, a)| LoopPoint::new(x, y, a)).collect();
        if let (Some(f), Some(l)) = (points.first().copied(), points.last().copied())
            && points.len() > 1
            && (f.x != l.x || f.y != l.y)
        {
            points.push(LoopPoint::new(f.x, f.y, 0.0));
        }
        Loop { label, points }
    }

    /// Axis-aligned rectangle, counter-clockwise, closed.
    pub fn rect(label: u32, x0: f64, y0: f64, x1: f64, y1: f64) -> Loop {
        Loop::from_triples(label, &[(x0, y0, 0.0), (x1, y0, 0.0), (x1, y1, 0.0), (x0, y1, 0.0), (x0, y0, 0.0)])
    }

    /// Full circle (IDF 3.0 form: centre, then a point on the circle with angle 360).
    pub fn circle(label: u32, cx: f64, cy: f64, r: f64) -> Loop {
        Loop { label, points: vec![LoopPoint::new(cx, cy, 0.0), LoopPoint::new(cx + r, cy, 360.0)] }
    }

    /// True if this loop is a single 360° circle.
    pub fn is_circle(&self) -> bool {
        self.points.len() == 2 && is_circle_angle(self.points[1].angle)
    }

    /// The loop's edges. Arc centres are computed from the chord and included angle.
    pub fn segments(&self) -> impl Iterator<Item = Segment> + '_ {
        self.points.windows(2).map(|w| {
            let a = [w[0].x, w[0].y];
            let b = [w[1].x, w[1].y];
            let ang = w[1].angle;
            if ang == 0.0 {
                Segment::Line { start: a, end: b }
            } else if is_circle_angle(ang) {
                Segment::Circle { center: a, radius: (b[0] - a[0]).hypot(b[1] - a[1]) }
            } else {
                let (center, radius) = arc_center(a, b, ang);
                Segment::Arc { start: a, end: b, center, radius, sweep: ang }
            }
        })
    }

    /// A loop is closed if it is a circle, or its last point equals its first (within
    /// [`CLOSE_TOL`]) and it has at least two points.
    pub fn is_closed(&self) -> bool {
        if self.is_circle() {
            return true;
        }
        match (self.points.first(), self.points.last()) {
            (Some(f), Some(l)) if self.points.len() >= 2 => {
                (f.x - l.x).abs() <= CLOSE_TOL && (f.y - l.y).abs() <= CLOSE_TOL
            }
            _ => false,
        }
    }

    /// Signed enclosed area (positive when counter-clockwise), including the circular segments
    /// that arcs add or remove. Only meaningful for closed loops.
    pub fn signed_area(&self) -> f64 {
        let mut a = 0.0;
        for s in self.segments() {
            match s {
                Segment::Line { start, end } => a += cross(start, end) / 2.0,
                Segment::Arc { start, end, radius, sweep, .. } => {
                    let t = sweep.to_radians();
                    a += cross(start, end) / 2.0 + radius * radius / 2.0 * (t - t.sin());
                }
                Segment::Circle { radius, .. } => a += std::f64::consts::PI * radius * radius,
            }
        }
        a
    }

    /// Enclosed area (absolute).
    pub fn area(&self) -> f64 {
        self.signed_area().abs()
    }

    /// Exact bounding box, including arc extremes.
    pub fn bbox(&self) -> BBox {
        let mut bb = BBox::empty();
        if let Some(p) = self.points.first()
            && !self.is_circle()
        {
            bb.add([p.x, p.y]);
        }
        for s in self.segments() {
            match s {
                Segment::Line { start, end } => {
                    bb.add(start);
                    bb.add(end);
                }
                Segment::Circle { center, radius } => {
                    bb.add([center[0] - radius, center[1] - radius]);
                    bb.add([center[0] + radius, center[1] + radius]);
                }
                Segment::Arc { start, end, center, radius, sweep } => {
                    bb.add(start);
                    bb.add(end);
                    let a0 = (start[1] - center[1]).atan2(start[0] - center[0]).to_degrees();
                    // Each axis direction k*90° lies on the arc if it is within the sweep.
                    for k in 0..4 {
                        let dir = k as f64 * 90.0;
                        let rel = if sweep > 0.0 { (dir - a0).rem_euclid(360.0) } else { (a0 - dir).rem_euclid(360.0) };
                        if rel > 0.0 && rel < sweep.abs() {
                            let (s, c) = sin_cos_deg(dir);
                            bb.add([center[0] + radius * c, center[1] + radius * s]);
                        }
                    }
                }
            }
        }
        bb
    }

    /// Multiply every coordinate by `k` (angles unchanged).
    pub fn scale(&mut self, k: f64) {
        self.map_coords(&|v| v * k);
    }

    /// Apply `f` to every coordinate (angles unchanged), e.g. a unit conversion.
    pub fn map_coords(&mut self, f: &impl Fn(f64) -> f64) {
        for p in &mut self.points {
            p.x = f(p.x);
            p.y = f(p.y);
        }
    }

    /// Map every point through `f`. If `mirrored` (the map reverses orientation), arc angles
    /// are negated so each arc stays the same curve; circle markers stay 360.
    pub fn map_points(&self, mirrored: bool, f: impl Fn(f64, f64) -> (f64, f64)) -> Loop {
        let points = self
            .points
            .iter()
            .map(|p| {
                let (x, y) = f(p.x, p.y);
                let angle = if mirrored && !is_circle_angle(p.angle) { -p.angle } else { p.angle };
                LoopPoint { x, y, angle }
            })
            .collect();
        Loop { label: self.label, points }
    }

    /// The loop with each 360° circle replaced by two 180° arcs (for IDF 2.0).
    pub fn without_circles(&self) -> Loop {
        if !self.points.iter().any(|p| is_circle_angle(p.angle)) {
            return self.clone();
        }
        let mut out: Vec<LoopPoint> = Vec::new();
        let mut i = 0;
        while i < self.points.len() {
            let p = self.points[i];
            if i + 1 < self.points.len() && is_circle_angle(self.points[i + 1].angle) {
                let q = self.points[i + 1];
                let far = LoopPoint::new(2.0 * p.x - q.x, 2.0 * p.y - q.y, 180.0);
                out.push(LoopPoint::new(q.x, q.y, 0.0));
                out.push(far);
                out.push(LoopPoint::new(q.x, q.y, 180.0));
                i += 2;
            } else {
                out.push(p);
                i += 1;
            }
        }
        Loop { label: self.label, points: out }
    }
}

fn cross(a: P2, b: P2) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

/// Bounding box of several loops.
pub fn loops_bbox(loops: &[Loop]) -> BBox {
    loops.iter().fold(BBox::empty(), |b, l| b.union(&l.bbox()))
}
