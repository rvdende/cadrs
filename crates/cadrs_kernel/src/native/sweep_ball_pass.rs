//! What a ball-end mill fills as it is carried upright along straight moves in one upright
//! plane (a raster pass of a three-axis program), below a height: exactly, face by face.
//!
//! The mill is a half ball under a cylinder of its radius, convex, and only slides; so over a
//! move it fills the hull of where it starts and where it stops, and over a pass (every move
//! forward the same way in plan) the union of those is, between two upright walls its radius
//! either side of the pass's plane, everything above the ball's lowest surface:
//!
//! - under each move, the lower half of the cylinder of the ball's radius about the line its
//!   centre runs along;
//! - where the centre's line turns up (a valley), the ball's own sphere between the two half
//!   cylinders, which end square to their lines there: a lune from one of the ball's side
//!   points to the other;
//! - where it turns down (a hill), nothing: the two cylinders are each other's mirror in the
//!   plane halving the turn and meet in an ellipse there, where both end;
//! - at the pass's two ends, the ball's sphere from the half cylinder's end round to its
//!   equator, and above the equator half the shank's cylinder;
//! - the two walls, each one polygon standing on the half cylinders' side lines;
//! - a flat top at the height asked for, which is below the lowest the mill's own top comes
//!   (what the mill fills above that isn't made: a cut's target ends below it).
//!
//! The faces share their edges as they are made, so nothing is intersected and nothing sewn.
//! (cad-kernel's `swept::raster` makes the same solid, each face a primitive trimmed by
//! booleans.)

use super::curve::Curve;
use super::math::*;
use super::surface::Surface;
use super::tol;
use super::topo::Body;
use super::xbuild::{XBuilder, XFace};
use std::f64::consts::PI;

/// A ball-end mill a body is, standing along `up` on the line through `on`: its ball's centre's
/// height above `on`, its radius, and its top's height above the centre. None for another body.
pub(crate) fn ball_mill(b: &Body, on: &P3, up: &V3) -> Option<(f64, f64, f64)> {
    if b.faces.len() != 3 || b.solids.len() != 1 {
        return None;
    }
    let size = b.bbox().diagonal().max(tol::point());
    let near = tol::REL_POINT * size;
    let off = |p: &P3| {
        let d = p - on;
        (d - up * d.dot(up)).norm()
    };
    let along = |v: &V3| v.cross(up).norm() < tol::ANGULAR;
    let (mut ball, mut shank, mut top) = (None, None, None);
    for f in &b.faces {
        match &f.surf {
            Surface::Sphere { f, r } if off(&f.o) <= near => ball = Some(((f.o - on).dot(up), *r)),
            Surface::Cylinder { f, r } if along(&f.z) && off(&f.o) <= near => shank = Some(*r),
            Surface::Plane { f } if along(&f.z) => top = Some((f.o - on).dot(up)),
            _ => return None,
        }
    }
    let ((centre, r), shank, top) = (ball?, shank?, top?);
    let length = top - centre;
    if (shank - r).abs() > near || length <= near {
        return None;
    }
    // The half ball under the cylinder and nothing else: by its volume.
    let want = 2.0 / 3.0 * PI * r.powi(3) + PI * r * r * length;
    let got = super::mass::body_volume(b);
    ((got - want).abs() <= 1e-9 * want).then_some((centre, r, length))
}

/// What a ball of radius `r` under an upright shank of its radius fills as its centre runs
/// straight from each of `centres` to the next, below the height `top` along `up` (see the
/// module). None where the moves aren't a pass (one upright plane, each forward the same way
/// in plan, none upright), where a hill is too sharp for the moves either side of it (their
/// cylinders' ends would cross), where `top` isn't above every centre, or where the solid
/// made isn't sound.
pub(crate) fn ball_pass(centres: &[P3], r: f64, up: &V3, top: f64) -> Option<Body> {
    let up = up.try_normalize(tol::TINY)?;
    let (first, last) = (*centres.first()?, *centres.last()?);
    let span = last - first;
    let d = (span - up * span.dot(&up)).try_normalize(tol::point())?;
    // To the side: (d, w, up) right-handed.
    let w = up.cross(&d);
    let size = span.norm().max(r);
    let near = tol::REL_POINT * size;
    // The centres in the pass's plane (along, up), straight runs made one move.
    let mut at: Vec<(f64, f64)> = Vec::with_capacity(centres.len());
    for c in centres {
        let v = c - first;
        if v.dot(&w).abs() > near {
            return None;
        }
        let p = (v.dot(&d), c.coords.dot(&up));
        if let [.., a, b] = at[..]
            && ((b.0 - a.0) * (p.1 - b.1) - (b.1 - a.1) * (p.0 - b.0)).abs()
                <= tol::ANGULAR * (b.0 - a.0).hypot(b.1 - a.1) * (p.0 - b.0).hypot(p.1 - b.1)
            && (p.0 - b.0) > 0.0
        {
            at.pop();
        }
        at.push(p);
    }
    let n = at.len() - 1;
    if n == 0 {
        return None;
    }
    let base = first - up * first.coords.dot(&up);
    let point = |p: (f64, f64)| base + d * p.0 + up * p.1;
    // Each move's direction and, square to it in the plane, the way down.
    let mut dirs = Vec::with_capacity(n);
    for s in at.windows(2) {
        let (dx, dz) = (s[1].0 - s[0].0, s[1].1 - s[0].1);
        // Forward, and no steeper than a mill can be told from a plunge.
        if dx <= near || dz.abs() > 50.0 * dx {
            return None;
        }
        let l = dx.hypot(dz);
        let a = d * (dx / l) + up * (dz / l);
        let down = d * (dz / l) - up * (dx / l);
        dirs.push((a, down, l));
    }
    if at.iter().any(|p| top <= p.1 + near) {
        return None;
    }
    // A joint's turn (up for more than nought), and whether the ball shows there.
    let turn = |j: usize| {
        let ((a, _, _), (b, _, _)) = (dirs[j - 1], dirs[j]);
        -a.cross(&b).dot(&w).atan2(a.dot(&b))
    };
    // (With (d, w, up) right-handed a turn up is a turn about −w.)
    let valley = |j: usize| turn(j) > tol::ANGULAR.max((8.0 * tol::CONFUSION / r).sqrt());
    // A hill's ellipse reaches r·tan(turn/2) back along each move: two on one move mustn't meet.
    let reach = |j: usize| {
        if j == 0 || j == n || valley(j) {
            0.0
        } else {
            r * (turn(j).abs() / 2.0).tan()
        }
    };
    for (i, step) in dirs.iter().enumerate() {
        if reach(i) + reach(i + 1) >= step.2 * (1.0 - 1e-6) {
            return None;
        }
    }

    let mut xb = XBuilder::new();
    let centre: Vec<P3> = at.iter().map(|&p| point(p)).collect();
    let vp: Vec<usize> = centre.iter().map(|c| xb.vertex(c + w * r)).collect();
    let vm: Vec<usize> = centre.iter().map(|c| xb.vertex(c - w * r)).collect();
    let lifted = |j: usize| centre[j] + up * (top - at[j].1);
    let (tp0, tm0) = (xb.vertex(lifted(0) + w * r), xb.vertex(lifted(0) - w * r));
    let (tpn, tmn) = (xb.vertex(lifted(n) + w * r), xb.vertex(lifted(n) - w * r));
    let line = |xb: &mut XBuilder, a: usize, b: usize| {
        let (c, _) = Curve::line(xb.body.verts[a].p, xb.body.verts[b].p);
        xb.edge(c, a, b)
    };
    // Half a circle about `c` from its side point at +w to the one at −w, by way of `via`.
    let half = |xb: &mut XBuilder, c: P3, via: V3, a: usize, b: usize| {
        xb.edge(Curve::Circle { c, x: w, y: via, r }, a, b)
    };
    // The side lines, each move's.
    let lp: Vec<usize> = (0..n)
        .map(|i| line(&mut xb, vp[i], vp[i + 1]))
        .collect::<Option<_>>()?;
    let lm: Vec<usize> = (0..n)
        .map(|i| line(&mut xb, vm[i], vm[i + 1]))
        .collect::<Option<_>>()?;
    // Where the half cylinders end at each joint: `ends[j]` the one before it's end, `starts[j]`
    // the one after it's start (one edge for both at a hill).
    let (mut ends, mut starts) = (vec![usize::MAX; n + 1], vec![usize::MAX; n + 1]);
    starts[0] = half(&mut xb, centre[0], dirs[0].1, vp[0], vm[0])?;
    ends[n] = half(&mut xb, centre[n], dirs[n - 1].1, vp[n], vm[n])?;
    for j in 1..n {
        if valley(j) {
            ends[j] = half(&mut xb, centre[j], dirs[j - 1].1, vp[j], vm[j])?;
            starts[j] = half(&mut xb, centre[j], dirs[j].1, vp[j], vm[j])?;
        } else {
            // The ellipse in the plane halving the turn: its long half axis down that plane.
            let down = (dirs[j - 1].1 + dirs[j].1).try_normalize(tol::TINY)?;
            let e = xb.edge(
                Curve::Ellipse {
                    c: centre[j],
                    x: w,
                    y: down,
                    a: r,
                    b: r / (turn(j) / 2.0).cos(),
                },
                vp[j],
                vm[j],
            )?;
            (ends[j], starts[j]) = (e, e);
        }
    }
    // The ends: the ball's equator's outer half, the shank's uprights, the top's half circles.
    let (q0, qn) = (
        half(&mut xb, centre[0], -d, vp[0], vm[0])?,
        half(&mut xb, centre[n], d, vp[n], vm[n])?,
    );
    let (up0, um0) = (line(&mut xb, vp[0], tp0)?, line(&mut xb, vm[0], tm0)?);
    let (upn, umn) = (line(&mut xb, vp[n], tpn)?, line(&mut xb, vm[n], tmn)?);
    let (hp, hm) = (line(&mut xb, tp0, tpn)?, line(&mut xb, tm0, tmn)?);
    let (r0, rn) = (
        half(&mut xb, lifted(0), -d, tp0, tm0)?,
        half(&mut xb, lifted(n), d, tpn, tmn)?,
    );

    // The faces, each surface's normal the solid's outward one and its loop anticlockwise
    // about it. A cylinder's seam and a sphere's poles are put clear of its face.
    let mut faces = Vec::new();
    let mut face = |xb: &mut XBuilder, surf: Surface, l: Vec<(usize, bool)>| {
        faces.push(xb.face(XFace {
            surf,
            sense: true,
            loops: vec![l],
        }));
    };
    let frame = |o: P3, x: V3, y: V3, z: V3| Frame { o, x, y, z };
    // A sphere about `c` whose face lies round the direction `mid` (in the pass's plane): its
    // poles square to that in the plane, its seam behind.
    let ball = |c: P3, mid: V3| {
        let z = w.cross(&mid);
        Surface::Sphere {
            f: frame(c, -mid, z.cross(&-mid), z),
            r,
        }
    };
    for i in 0..n {
        let (a, down, _) = dirs[i];
        face(
            &mut xb,
            Surface::Cylinder {
                f: frame(centre[i], -down, -w, a),
                r,
            },
            vec![
                (lp[i], true),
                (ends[i + 1], true),
                (lm[i], false),
                (starts[i], false),
            ],
        );
    }
    for j in 1..n {
        if valley(j) {
            let mid = (dirs[j - 1].1 + dirs[j].1).try_normalize(tol::TINY)?;
            face(
                &mut xb,
                ball(centre[j], mid),
                vec![(ends[j], false), (starts[j], true)],
            );
        }
    }
    // The start: the ball behind the first half cylinder, the shank's back half above it.
    let mid0 = (dirs[0].1 - d).try_normalize(tol::TINY)?;
    face(
        &mut xb,
        ball(centre[0], mid0),
        vec![(starts[0], true), (q0, false)],
    );
    face(
        &mut xb,
        Surface::Cylinder {
            f: frame(centre[0], d, w, up),
            r,
        },
        vec![(q0, true), (um0, true), (r0, false), (up0, false)],
    );
    // The end: the ball ahead of the last half cylinder, the shank's front half.
    let midn = (dirs[n - 1].1 + d).try_normalize(tol::TINY)?;
    face(
        &mut xb,
        ball(centre[n], midn),
        vec![(ends[n], false), (qn, true)],
    );
    face(
        &mut xb,
        Surface::Cylinder {
            f: frame(centre[n], -d, -w, up),
            r,
        },
        vec![(qn, false), (upn, true), (rn, true), (umn, false)],
    );
    // The walls, standing on the side lines.
    let mut wall: Vec<(usize, bool)> = (0..n).rev().map(|i| (lp[i], false)).collect();
    wall.extend([(up0, true), (hp, true), (upn, false)]);
    face(
        &mut xb,
        Surface::Plane {
            f: frame(centre[0] + w * r, d, -up, w),
        },
        wall,
    );
    let mut wall: Vec<(usize, bool)> = (0..n).map(|i| (lm[i], true)).collect();
    wall.extend([(umn, true), (hm, false), (um0, false)]);
    face(
        &mut xb,
        Surface::Plane {
            f: frame(centre[0] - w * r, d, up, -w),
        },
        wall,
    );
    // The top.
    face(
        &mut xb,
        Surface::Plane {
            f: frame(lifted(0), d, w, up),
        },
        vec![(hp, false), (r0, true), (hm, true), (rn, false)],
    );
    let shell = xb.shell(faces, true);
    xb.solid(vec![shell]);
    let (mut body, warnings) = xb.finish();
    if crate::native::env_flag!("CADRS_SWEEP_DEBUG") && !warnings.is_empty() {
        eprintln!("ball pass: {warnings:?}");
    }
    body.fit_tolerances_searched(1e-5 * (1.0 + size));
    let sound = body.solids.len() == 1 && body.check().is_empty();
    if crate::native::env_flag!("CADRS_SWEEP_DEBUG") && !sound {
        eprintln!("ball pass: not sound: {:?}", body.check());
    }
    sound.then_some(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn volume(b: &Body) -> f64 {
        crate::native::mass::body_volume(b)
    }

    /// One level move of 10: half a cylinder under a box, a quarter ball and half the shank's
    /// cylinder at each end.
    #[test]
    fn a_level_move() {
        let (r, top) = (3.0, 7.0);
        let b = ball_pass(
            &[P3::new(1.0, 2.0, 0.5), P3::new(11.0, 2.0, 0.5)],
            r,
            &V3::z(),
            top,
        )
        .expect("a pass");
        assert_eq!(b.faces.len(), 8);
        let h = top - 0.5;
        let want = PI * r * r / 2.0 * 10.0
            + 2.0 * r * 10.0 * h
            + 2.0 * (PI * r.powi(3) / 3.0 + PI * r * r / 2.0 * h);
        let got = volume(&b);
        assert!((got - want).abs() < 1e-9 * want, "{got} for {want}");
    }

    /// Down, up and down again (a valley, then a hill), askew in plan: sound, a lune at the
    /// valley and none at the hill, and its volume the level pass's of the same length in plan
    /// less what lies between the centres' line and the level through its lowest point... told
    /// here by slices: the solid's volume is its section's area along the pass, which at each
    /// place along is the half disc and the walls' rectangle up to the top.
    #[test]
    fn a_valley_and_a_hill() {
        let (r, top) = (3.0, 9.0);
        let d = V3::new(0.6, 0.8, 0.0);
        let at = |s: f64, z: f64| P3::origin() + d * s + V3::z() * z;
        let centres = [at(0.0, 2.0), at(5.0, 0.5), at(10.0, 2.5), at(15.0, 1.0)];
        let b = ball_pass(&centres, r, &V3::z(), top).expect("a pass");
        assert!(b.check().is_empty(), "{:?}", b.check());
        // Three half cylinders, a lune, two end balls, two half shanks, two walls, a top.
        assert_eq!(b.faces.len(), 11);
        assert_eq!(
            b.faces
                .iter()
                .filter(|f| matches!(f.surf, Surface::Sphere { .. }))
                .count(),
            3
        );
        // By points: inside where within the radius of a move's line or above it between the
        // walls, outside elsewhere (told a little way off the faces).
        let inside = |p: &P3| {
            let (s, z) = ((p - P3::origin()).dot(&d), p.z);
            let y = (p - P3::origin()).dot(&V3::z().cross(&d));
            if z > top || y.abs() > r {
                return false;
            }
            let pts = [(0.0, 2.0), (5.0, 0.5), (10.0, 2.5), (15.0, 1.0)];
            pts.windows(2).any(|m| {
                let (a, c) = (m[0], m[1]);
                let (ux, uz) = (c.0 - a.0, c.1 - a.1);
                let t = (((s - a.0) * ux + (z - a.1) * uz) / (ux * ux + uz * uz)).clamp(0.0, 1.0);
                let (qx, qz) = (a.0 + ux * t, a.1 + uz * t);
                // Within the ball about the nearest point of the move, or in the shank above
                // some place on it.
                ((s - qx).powi(2) + (z - qz).powi(2) + y * y).sqrt() <= r
                    || (0..=200).any(|k| {
                        let t = k as f64 / 200.0;
                        let (cx, cz) = (a.0 + ux * t, a.1 + uz * t);
                        z >= cz && ((s - cx).powi(2) + y * y).sqrt() <= r
                    })
            })
        };
        let polys: Vec<Vec<Vec<P2>>> = b
            .faces
            .iter()
            .map(|f| crate::native::classify::face_polygons(&b, f))
            .collect();
        let mut seen = [0usize; 2];
        for i in 0..40 {
            for j in 0..14 {
                for k in 0..24 {
                    let p = at(
                        -4.0 + 23.0 * (i as f64 + 0.37) / 40.0,
                        -3.5 + 13.0 * (k as f64 + 0.41) / 24.0,
                    ) + V3::z().cross(&d) * (-3.4 + 6.8 * (j as f64 + 0.53) / 14.0);
                    let got = crate::native::classify::parity_inside(&b, &polys, &p);
                    let want = inside(&p);
                    // (The shank's test is by 200 places a move: leave what lies within a
                    // hair of the boundary.)
                    let edge = [0.03, -0.03].iter().any(|e| {
                        [V3::x(), V3::y(), V3::z()]
                            .iter()
                            .any(|ax| inside(&(p + ax * *e)) != want)
                    });
                    if !edge {
                        assert_eq!(got, want, "at {p:?}");
                        seen[usize::from(want)] += 1;
                    }
                }
            }
        }
        assert!(seen[0] > 1000 && seen[1] > 1000, "{seen:?}");
    }

    /// A hill too sharp for the short move after it, and a move straight up: no pass.
    #[test]
    fn what_is_no_pass() {
        let z = V3::z();
        let p = |x: f64, h: f64| P3::new(x, 0.0, h);
        assert!(
            ball_pass(
                &[p(0.0, 0.0), p(5.0, 4.0), p(5.5, 0.0), p(9.0, 3.0)],
                3.0,
                &z,
                9.0
            )
            .is_none()
        );
        assert!(ball_pass(&[p(0.0, 0.0), p(0.0, 4.0)], 3.0, &z, 9.0).is_none());
        assert!(ball_pass(&[p(0.0, 0.0), p(5.0, 4.0)], 3.0, &z, 3.0).is_none());
        // Moves in a line are one move.
        let b = ball_pass(&[p(0.0, 0.0), p(2.0, 1.0), p(6.0, 3.0)], 3.0, &z, 9.0).unwrap();
        assert_eq!(b.faces.len(), 8);
    }
}
