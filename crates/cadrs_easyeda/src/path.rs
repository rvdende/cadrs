//! SVG path data (EasyEDA's paths, arcs and regions) as lines, circular arcs and point loops,
//! in the path's own coordinates.

/// A piece of a path.
#[derive(Clone, Debug, PartialEq)]
pub enum Seg {
    Line([f64; 2], [f64; 2]),
    /// A circular arc: start, a point halfway along, end.
    Arc([f64; 2], [f64; 2], [f64; 2]),
}

/// A sub-path: its pieces, and whether it closes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sub {
    pub segs: Vec<Seg>,
    pub closed: bool,
}

impl Sub {
    /// Its points, arcs flattened to `per_arc` pieces.
    pub fn points(&self, per_arc: usize) -> Vec<[f64; 2]> {
        let mut out: Vec<[f64; 2]> = vec![];
        for s in &self.segs {
            match *s {
                Seg::Line(a, b) => {
                    if out.is_empty() {
                        out.push(a);
                    }
                    out.push(b);
                }
                Seg::Arc(a, m, b) => {
                    if out.is_empty() {
                        out.push(a);
                    }
                    out.extend(flatten_arc(a, m, b, per_arc).into_iter().skip(1));
                }
            }
        }
        out
    }
}

/// The circle through three points (centre, radius), if they aren't in a line.
pub fn circle_through(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> Option<([f64; 2], f64)> {
    let d = 2.0 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
    if d.abs() < 1e-12 {
        return None;
    }
    let sq = |p: [f64; 2]| p[0] * p[0] + p[1] * p[1];
    let ux = (sq(a) * (b[1] - c[1]) + sq(b) * (c[1] - a[1]) + sq(c) * (a[1] - b[1])) / d;
    let uy = (sq(a) * (c[0] - b[0]) + sq(b) * (a[0] - c[0]) + sq(c) * (b[0] - a[0])) / d;
    Some(([ux, uy], (a[0] - ux).hypot(a[1] - uy)))
}

/// An arc (start, mid, end) as `n` + 1 points.
fn flatten_arc(a: [f64; 2], m: [f64; 2], b: [f64; 2], n: usize) -> Vec<[f64; 2]> {
    let Some((c, r)) = circle_through(a, m, b) else { return vec![a, b] };
    let ang = |p: [f64; 2]| (p[1] - c[1]).atan2(p[0] - c[0]);
    let (t0, tm, t1) = (ang(a), ang(m), ang(b));
    let tau = std::f64::consts::TAU;
    // Sweep from t0 to t1 through tm.
    let ccw = (tm - t0).rem_euclid(tau) < (t1 - t0).rem_euclid(tau);
    let sweep = if ccw { (t1 - t0).rem_euclid(tau) } else { -(t0 - t1).rem_euclid(tau) };
    (0..=n).map(|i| t0 + sweep * i as f64 / n as f64).map(|t| [c[0] + r * t.cos(), c[1] + r * t.sin()]).collect()
}

/// An SVG elliptical arc (endpoint form) as a circular arc's three points, or line pieces when
/// it's elliptical.
fn svg_arc(p0: [f64; 2], rx: f64, ry: f64, phi_deg: f64, large: bool, sweep: bool, p1: [f64; 2]) -> Vec<Seg> {
    if (rx.abs() < 1e-9 || ry.abs() < 1e-9) || (p0[0] - p1[0]).hypot(p0[1] - p1[1]) < 1e-9 {
        return vec![Seg::Line(p0, p1)];
    }
    // SVG 1.1 F.6.5: the centre parameterisation.
    let phi = phi_deg.to_radians();
    let (cp, sp) = (phi.cos(), phi.sin());
    let (dx, dy) = ((p0[0] - p1[0]) / 2.0, (p0[1] - p1[1]) / 2.0);
    let (x1, y1) = (cp * dx + sp * dy, -sp * dx + cp * dy);
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    let l = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if l > 1.0 {
        rx *= l.sqrt();
        ry *= l.sqrt();
    }
    let num = (rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1).max(0.0);
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut k = (num / den).sqrt();
    if large == sweep {
        k = -k;
    }
    let (cx1, cy1) = (k * rx * y1 / ry, -k * ry * x1 / rx);
    let (cx, cy) = (cp * cx1 - sp * cy1 + (p0[0] + p1[0]) / 2.0, sp * cx1 + cp * cy1 + (p0[1] + p1[1]) / 2.0);
    let angle = |ux: f64, uy: f64, vx: f64, vy: f64| (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
    let t1 = angle(1.0, 0.0, (x1 - cx1) / rx, (y1 - cy1) / ry);
    let mut dt = angle((x1 - cx1) / rx, (y1 - cy1) / ry, (-x1 - cx1) / rx, (-y1 - cy1) / ry);
    let tau = std::f64::consts::TAU;
    if !sweep && dt > 0.0 {
        dt -= tau;
    } else if sweep && dt < 0.0 {
        dt += tau;
    }
    let at = |t: f64| {
        let (x, y) = (rx * t.cos(), ry * t.sin());
        [cp * x - sp * y + cx, sp * x + cp * y + cy]
    };
    if (rx - ry).abs() < 1e-6 * rx.max(1.0) {
        // Circular; a full turn needs two halves to stay well defined.
        if dt.abs() > std::f64::consts::PI * 1.5 {
            let m = at(t1 + dt / 2.0);
            return vec![Seg::Arc(p0, at(t1 + dt / 4.0), m), Seg::Arc(m, at(t1 + dt * 0.75), p1)];
        }
        return vec![Seg::Arc(p0, at(t1 + dt / 2.0), p1)];
    }
    let n = 16;
    let mut prev = p0;
    (1..=n)
        .map(|i| {
            let q = if i == n { p1 } else { at(t1 + dt * i as f64 / n as f64) };
            let s = Seg::Line(prev, q);
            prev = q;
            s
        })
        .collect()
}

fn tokens(d: &str) -> Vec<Tok> {
    let mut out = vec![];
    let b = d.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_ascii_alphabetic() && c != 'e' && c != 'E' {
            out.push(Tok::Cmd(c));
            i += 1;
        } else if c.is_ascii_digit() || c == '-' || c == '+' || c == '.' {
            let start = i;
            i += 1;
            let mut dot = c == '.';
            while i < b.len() {
                let ch = b[i] as char;
                if ch.is_ascii_digit() {
                    i += 1;
                } else if ch == '.' && !dot {
                    dot = true;
                    i += 1;
                } else if (ch == 'e' || ch == 'E') && i + 1 < b.len() {
                    i += 1;
                    if b[i] == b'-' || b[i] == b'+' {
                        i += 1;
                    }
                } else {
                    break;
                }
            }
            if let Ok(v) = d[start..i].parse() {
                out.push(Tok::Num(v));
            }
        } else {
            i += 1;
        }
    }
    out
}

#[derive(Clone, Copy, Debug)]
enum Tok {
    Cmd(char),
    Num(f64),
}

/// Parses path data: M/L/H/V/A/Z (and C/Q/S/T as straight lines to their end points), upper
/// case absolute, lower case relative.
pub fn parse(d: &str) -> Vec<Sub> {
    let toks = tokens(d);
    let mut subs: Vec<Sub> = vec![];
    let mut cur = Sub::default();
    let (mut pos, mut start) = ([0.0, 0.0], [0.0, 0.0]);
    let mut i = 0;
    let mut cmd = 'M';
    let nums = |i: &mut usize, n: usize| -> Option<Vec<f64>> {
        let mut v = vec![];
        while v.len() < n {
            match toks.get(*i) {
                Some(Tok::Num(x)) => v.push(*x),
                _ => return None,
            }
            *i += 1;
        }
        Some(v)
    };
    let finish = |cur: &mut Sub, subs: &mut Vec<Sub>| {
        if !cur.segs.is_empty() {
            subs.push(std::mem::take(cur));
        } else {
            *cur = Sub::default();
        }
    };
    while i < toks.len() {
        if let Tok::Cmd(c) = toks[i] {
            cmd = c;
            i += 1;
            if c == 'Z' || c == 'z' {
                if pos != start {
                    cur.segs.push(Seg::Line(pos, start));
                }
                cur.closed = true;
                pos = start;
                finish(&mut cur, &mut subs);
                continue;
            }
        }
        let rel = cmd.is_ascii_lowercase();
        let off = |p: [f64; 2], x: f64, y: f64| if rel { [p[0] + x, p[1] + y] } else { [x, y] };
        let n = match cmd.to_ascii_uppercase() {
            'M' | 'L' | 'T' => 2,
            'H' | 'V' => 1,
            'A' => 7,
            'C' => 6,
            'Q' | 'S' => 4,
            _ => {
                i += 1;
                continue;
            }
        };
        let Some(v) = nums(&mut i, n) else {
            i += 1;
            continue;
        };
        match cmd.to_ascii_uppercase() {
            'M' => {
                finish(&mut cur, &mut subs);
                pos = off(pos, v[0], v[1]);
                start = pos;
                // Further pairs are line-tos.
                cmd = if rel { 'l' } else { 'L' };
            }
            'L' | 'T' => {
                let q = off(pos, v[n - 2], v[n - 1]);
                cur.segs.push(Seg::Line(pos, q));
                pos = q;
            }
            'C' | 'Q' | 'S' => {
                let q = off(pos, v[n - 2], v[n - 1]);
                cur.segs.push(Seg::Line(pos, q));
                pos = q;
            }
            'H' => {
                let q = [if rel { pos[0] + v[0] } else { v[0] }, pos[1]];
                cur.segs.push(Seg::Line(pos, q));
                pos = q;
            }
            'V' => {
                let q = [pos[0], if rel { pos[1] + v[0] } else { v[0] }];
                cur.segs.push(Seg::Line(pos, q));
                pos = q;
            }
            'A' => {
                let q = off(pos, v[5], v[6]);
                cur.segs.extend(svg_arc(pos, v[0], v[1], v[2], v[3] != 0.0, v[4] != 0.0, q));
                pos = q;
            }
            _ => {}
        }
    }
    finish(&mut cur, &mut subs);
    subs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f64; 2], b: [f64; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6
    }

    #[test]
    fn lines_and_relative_moves() {
        let s = parse("M355,265h10");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].segs, vec![Seg::Line([355.0, 265.0], [365.0, 265.0])]);
        let s = parse("M 4000 3061.0236 L 4000 2994.0945 L 4062.9921 2994.0945 Z");
        assert!(s[0].closed);
        assert_eq!(s[0].points(4).len(), 4);
    }

    #[test]
    fn half_circle_arc() {
        // From (0,0) to (10,0), radius 5, sweep 1: in y-down SVG, clockwise on screen, i.e.
        // through (5,-5).
        let s = parse("M 0 0 A 5 5 0 0 1 10 0");
        let Seg::Arc(a, m, b) = s[0].segs[0] else { panic!("{s:?}") };
        assert!(close(a, [0.0, 0.0]) && close(b, [10.0, 0.0]), "{a:?} {b:?}");
        assert!(close(m, [5.0, -5.0]), "{m:?}");
        let (c, r) = circle_through(a, m, b).unwrap();
        assert!(close(c, [5.0, 0.0]) && (r - 5.0).abs() < 1e-9);
    }
}
