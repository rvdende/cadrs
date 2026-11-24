//! What a request for a quote says about a part (`cadrs_org::capability::PartSummary`),
//! measured on its solid: is it a flat profile of one thickness (cuttable from sheet by a
//! waterjet, laser, plasma or router), how thick, how long its cuts are, how many separate cuts
//! (pierces), its smallest hole and arc, its area and its size.
//!
//! A part is a flat profile when two of its planar faces face opposite ways (its caps) and every
//! other face is a side: its surface normals all lie across the caps' normal (planes and
//! cylinders standing on the caps). The caps chosen are the largest such pair.

use cadrs_org::capability::PartSummary;
use cadrs_sketch::Vec3;

use crate::solid::Solid;

fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn dist(a: Vec3, b: Vec3) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// A face's area: the kernel's, or its triangles'.
fn face_area(solid: &Solid, i: usize) -> f64 {
    let f = &solid.faces[i];
    if let Some(a) = f.area {
        return a;
    }
    (f.first_triangle..f.first_triangle + f.triangle_count)
        .map(|t| {
            let [a, b, c] = [0, 1, 2].map(|k| solid.positions[solid.indices[3 * t + k] as usize]);
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let w = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            0.5 * dot(w, w).sqrt()
        })
        .sum()
}

/// Whether every normal of face `i` lies across `n` (a side face of a profile along `n`).
fn is_side(solid: &Solid, i: usize, n: Vec3) -> bool {
    let f = &solid.faces[i];
    if let Some(plane) = f.plane {
        return dot(plane.normal(), n).abs() < 1e-6;
    }
    (f.first_triangle..f.first_triangle + f.triangle_count).all(|t| {
        (0..3).all(|k| {
            let m = solid.normals[solid.indices[3 * t + k] as usize];
            dot(m, n).abs() < 1e-3
        })
    })
}

/// Measures `solid` for a request for a quote.
pub fn part_summary(solid: &Solid, name: &str, material: Option<String>) -> PartSummary {
    let mut summary = PartSummary {
        name: name.to_owned(),
        material,
        ..Default::default()
    };
    // Its size, largest first (overwritten in the caps' frame for a flat part).
    if let Some(first) = solid.positions.first() {
        let (mut lo, mut hi) = (*first, *first);
        for p in &solid.positions {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let mut size = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
        size.sort_by(|a, b| b.total_cmp(a));
        summary.size = size;
    }

    // The caps: the largest pair of opposite planar faces with only sides besides.
    let planar: Vec<usize> = (0..solid.faces.len())
        .filter(|&i| solid.faces[i].plane.is_some())
        .collect();
    // (top area, a top face, the faces in the top's plane, thickness).
    let mut best: Option<(f64, usize, Vec<usize>, f64)> = None;
    for &top in &planar {
        let frame = solid.faces[top].plane.expect("planar");
        let n = frame.normal();
        let at = dot(frame.origin, n);
        // Faces in the top's plane, and in one plane facing the other way.
        let mut tops = Vec::new();
        let mut bottom_at = None;
        let mut flat = true;
        for i in 0..solid.faces.len() {
            let f = &solid.faces[i];
            match f.plane {
                Some(p)
                    if dot(p.normal(), n) > 1.0 - 1e-6 && (dot(p.origin, n) - at).abs() < 1e-6 =>
                {
                    tops.push(i)
                }
                Some(p) if dot(p.normal(), n) < -1.0 + 1e-6 => {
                    let level = dot(p.origin, n);
                    match bottom_at {
                        None => bottom_at = Some(level),
                        Some(b) if (b - level).abs() < 1e-6 => {}
                        Some(_) => flat = false,
                    }
                }
                _ if is_side(solid, i, n) => {}
                _ => flat = false,
            }
            if !flat {
                break;
            }
        }
        let (true, Some(bottom_at)) = (flat, bottom_at) else {
            continue;
        };
        let area: f64 = tops.iter().map(|&i| face_area(solid, i)).sum();
        let thickness = (at - bottom_at).abs();
        if best.as_ref().is_none_or(|b| area > b.0 + 1e-9) {
            best = Some((area, top, tops, thickness));
        }
    }
    let Some((area, top, tops, thickness)) = best else {
        return summary;
    };
    summary.flat_thickness = Some(thickness);
    summary.area = area;

    // The profile: the top's loops are the cuts.
    let mut loops = 0;
    let mut length = 0.0;
    for &i in &tops {
        for l in &solid.faces[i].loops {
            if l.len() < 2 {
                continue;
            }
            loops += 1;
            length += l.windows(2).map(|w| dist(w[0], w[1])).sum::<f64>()
                + dist(*l.last().expect("points"), l[0]);
        }
    }
    summary.pierces = loops;
    summary.cut_length = length;

    // Holes and arcs: circular edges on the top.
    let top_names: Vec<_> = tops.iter().map(|&i| solid.faces[i].name).collect();
    for e in solid
        .edges
        .iter()
        .filter(|e| e.name.faces.iter().any(|f| top_names.contains(f)))
    {
        let Some(c) = e.circle else { continue };
        let closed =
            e.points.len() > 2 && dist(e.points[0], *e.points.last().expect("points")) < 1e-6;
        if closed {
            let d = 2.0 * c.radius;
            summary.min_hole = Some(summary.min_hole.map_or(d, |m: f64| m.min(d)));
        } else {
            // An arc of the outline: the tightest of them bounds the inside corners.
            summary.min_inside_radius = Some(
                summary
                    .min_inside_radius
                    .map_or(c.radius, |m: f64| m.min(c.radius)),
            );
        }
    }

    // Its size in the caps' frame.
    let frame = solid.faces[top].plane.expect("planar");
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for p in &solid.positions {
        let q = frame.to_sketch(*p);
        lo = [lo[0].min(q.x), lo[1].min(q.y)];
        hi = [hi[0].max(q.x), hi[1].max(q.y)];
    }
    let (a, b) = (hi[0] - lo[0], hi[1] - lo[1]);
    summary.size = [a.max(b), a.min(b), thickness];
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brep::{ProfileGroup, extrude};
    use cadrs_kernel::DefaultKernel;
    use cadrs_sketch::region::regions;
    use cadrs_sketch::{PlaneRef, Sketch, SketchOp, Vec2};

    fn plate(holes: &[(f64, f64, f64)], depth: f64) -> Solid {
        let mut s = Sketch::new();
        let v = Vec2::new;
        SketchOp::AddPolyline {
            points: vec![v(0.0, 0.0), v(200.0, 0.0), v(200.0, 80.0), v(0.0, 80.0)],
            closed: true,
            construction: false,
            label: "Add line",
        }
        .apply(&mut s)
        .unwrap();
        for &(x, y, r) in holes {
            SketchOp::AddCircle {
                center: Vec2::new(x, y),
                radius: r,
                construction: false,
            }
            .apply(&mut s)
            .unwrap();
        }
        // The plate's region: the one bounded by the rectangle (the holes are its own regions).
        let rs: Vec<(u64, _)> = regions(&s)
            .into_iter()
            .filter(|r| r.curves.len() > 1 || holes.is_empty())
            .enumerate()
            .map(|(i, r)| (i as u64, r))
            .collect();
        let frame = PlaneRef::Top.frame();
        let mut k = DefaultKernel::new();
        extrude(
            &mut k,
            uuid::Uuid::from_u128(1),
            &[ProfileGroup::new(frame, rs)],
            depth,
            false,
        )
        .unwrap()
        .solid
    }

    #[test]
    fn a_plate_with_holes_is_a_flat_profile() {
        let solid = plate(&[(50.0, 40.0, 4.25), (150.0, 40.0, 6.0)], 10.0);
        let s = part_summary(&solid, "bracket", Some("mild steel".into()));
        assert_eq!(
            s.flat_thickness.map(|t| (t * 1e6).round() / 1e6),
            Some(10.0)
        );
        assert_eq!(s.pierces, 3, "{s:?}");
        let expected = 2.0 * (200.0 + 80.0) + 2.0 * std::f64::consts::PI * (4.25 + 6.0);
        assert!(
            (s.cut_length - expected).abs() < 1.0,
            "{} vs {expected}",
            s.cut_length
        );
        assert!((s.min_hole.unwrap() - 8.5).abs() < 1e-6);
        let area = 200.0 * 80.0 - std::f64::consts::PI * (4.25f64.powi(2) + 36.0);
        assert!((s.area - area).abs() < 2.0, "{} vs {area}", s.area);
        assert!((s.size[0] - 200.0).abs() < 1e-6 && (s.size[1] - 80.0).abs() < 1e-6);
    }

    #[test]
    fn a_plain_plate_is_flat_too() {
        let solid = plate(&[], 3.0);
        let s = part_summary(&solid, "plate", None);
        assert_eq!(s.pierces, 1);
        assert_eq!(s.min_hole, None);
        assert!((s.flat_thickness.unwrap() - 3.0).abs() < 1e-6);
    }
}
