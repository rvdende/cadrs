//! DXF (and DWG) of a sketch or a planar face, for laser, plasma and waterjet cutting (P3F.2;
//! `intro-to-parametric-cad.md` P3.2, X7).
//!
//! The geometry is laid out flat as a [`Page`] of strokes in the sketch's (or the face's)
//! plane coordinates, in millimetres at full size, and written by the drawings' DXF writer
//! ([`cadrs_drawing::dxf`], P3C.7), so there is one DXF writer in cadrs:
//! - a sketch: its curves (construction geometry left out) on the `VISIBLE` layer: lines as
//!   LINE, circles as CIRCLE, arcs as ARC, ellipses as closed polylines;
//! - a planar face: its boundary edges, the same way (an edge on a circle is a CIRCLE when it
//!   closes, else an ARC; other curves are polylines).
//!
//! DWG goes through the drawings' external converter ([`cadrs_drawing::dwg`]): no DWG library
//! with a usable licence exists in Rust (ODA's are proprietary, LibreDWG is GPL), so cadrs
//! writes DXF and runs a converter the user installed (LibreDWG's `dxf2dwg` or the ODA File
//! Converter), never linking one. Without a converter the DWG option is disabled.

use cadrs_drawing::dxf::DxfVersion;
use cadrs_drawing::export::{Item, Layer, Page, Pen, Shape};
use cadrs_sketch::{CurveKind, PlaneFrame, Sketch};

use crate::solid::{FaceName, Solid};

/// Line width written for the geometry (mm).
const WIDTH: f64 = 0.25;

fn pen() -> Pen {
    Pen::new(WIDTH, Layer::Visible)
}

/// The geometry's true extents, lowest and highest x and y (P3F.2 judge: a sketch or face
/// reaching below or left of its origin), and the page sized to them. The DXF writer puts the
/// same extents in `$EXTMIN`/`$EXTMAX` and centres its view on them
/// ([`cadrs_drawing::dxf::extents`]); the coordinates themselves are left as they are.
pub fn fit(page: &mut Page) -> ([f64; 2], [f64; 2]) {
    let (lo, hi) = cadrs_drawing::dxf::extents(page);
    page.width = (hi[0] - lo[0]).max(1.0);
    page.height = (hi[1] - lo[1]).max(1.0);
    (lo, hi)
}

/// A sketch's curves as a page, in the sketch plane's coordinates (mm).
pub fn sketch_page(sketch: &Sketch, name: &str) -> Page {
    let mut page = Page { name: name.to_string(), ..Page::default() };
    for (id, c) in &sketch.curves {
        if c.construction {
            continue;
        }
        let p = |pid| {
            let v = sketch.pos(pid);
            [v.x, v.y]
        };
        let shape = match c.kind {
            CurveKind::Line { a, b } => Shape::Line { a: p(a), b: p(b) },
            CurveKind::Circle { center, radius } => Shape::Circle { center: p(center), radius },
            CurveKind::Arc { .. } => {
                let Some(g) = sketch.arc_geom(id) else { continue };
                let g = g.to_ccw();
                let start = g.start_angle.to_degrees();
                Shape::Arc { center: [g.center.x, g.center.y], radius: g.radius, start, end: start + g.sweep.to_degrees() }
            }
            CurveKind::Ellipse { .. } | CurveKind::EllipseOffset { .. } => {
                let Some(g) = sketch.ellipse_geom(id) else { continue };
                let n = 144;
                let points = (0..n)
                    .map(|k| {
                        let q = g.point_at(k as f64 / n as f64 * std::f64::consts::TAU);
                        [q.x, q.y]
                    })
                    .collect();
                Shape::Polyline { points, closed: true }
            }
            CurveKind::EllipseArc { .. } => {
                let Some(g) = sketch.ellipse_arc_geom(id) else { continue };
                let points = g.tessellate(std::f64::consts::TAU / 144.0, 8).into_iter().map(|q| [q.x, q.y]).collect();
                Shape::Polyline { points, closed: false }
            }
            CurveKind::Spline { start, end } => {
                let Some(spans) = sketch.spline_spans(id) else { continue };
                let points = cadrs_sketch::spline::tessellate(&spans, 16).into_iter().map(|q| [q.x, q.y]).collect();
                Shape::Polyline { points, closed: start == end }
            }
            CurveKind::Bezier { .. } => {
                let Some(g) = sketch.bezier_geom(id) else { continue };
                let n = 64;
                let points = (0..=n)
                    .map(|k| {
                        let q = g.point_at(k as f64 / n as f64);
                        [q.x, q.y]
                    })
                    .collect();
                Shape::Polyline { points, closed: false }
            }
        };
        page.items.push(Item::Stroke(shape, pen()));
    }
    fit(&mut page);
    page
}

/// A planar face's boundary as a page, in the face's plane coordinates (mm); `None` when the
/// part has no such face or it isn't flat.
pub fn face_page(solid: &Solid, face: &FaceName, name: &str) -> Option<Page> {
    let f = solid.faces.iter().find(|f| f.name == *face)?;
    let frame: PlaneFrame = f.plane?;
    let n = frame.normal();
    let to2 = |q: [f64; 3]| {
        let v = frame.to_sketch(q);
        [v.x, v.y]
    };
    let mut page = Page { name: name.to_string(), ..Page::default() };
    for e in solid.edges.iter().filter(|e| e.name.faces.contains(face)) {
        if e.points.len() < 2 {
            continue;
        }
        let pts: Vec<[f64; 2]> = e.points.iter().map(|q| to2(*q)).collect();
        let first = pts[0];
        let last = *pts.last().expect("two points");
        let closed = ((first[0] - last[0]).powi(2) + (first[1] - last[1]).powi(2)).sqrt() < 1e-6;
        let shape = if let Some(c) = e.circle {
            let center = to2(c.center);
            if closed {
                Shape::Circle { center, radius: c.radius }
            } else {
                let ang = |q: [f64; 2]| (q[1] - center[1]).atan2(q[0] - center[0]).to_degrees();
                // The polyline runs counter-clockwise in the plane when the circle's normal is
                // the plane's.
                let ccw = c.normal[0] * n[0] + c.normal[1] * n[1] + c.normal[2] * n[2] >= 0.0;
                let mid = pts[pts.len() / 2];
                let (a0, a1) = if ccw { (ang(first), ang(last)) } else { (ang(last), ang(first)) };
                let mut end = a1;
                while end <= a0 {
                    end += 360.0;
                }
                // Check the direction with a point in the middle of the edge.
                let m = ang(mid);
                let mut mm = m;
                while mm < a0 {
                    mm += 360.0;
                }
                if mm > end {
                    // The other way round.
                    let (s, mut e2) = (a1, a0);
                    while e2 <= s {
                        e2 += 360.0;
                    }
                    Shape::Arc { center, radius: c.radius, start: s, end: e2 }
                } else {
                    Shape::Arc { center, radius: c.radius, start: a0, end }
                }
            }
        } else if pts.len() == 2 || collinear(&pts) {
            Shape::Line { a: first, b: last }
        } else {
            Shape::Polyline { points: pts, closed: false }
        };
        page.items.push(Item::Stroke(shape, pen()));
    }
    fit(&mut page);
    Some(page)
}

fn collinear(pts: &[[f64; 2]]) -> bool {
    let (a, b) = (pts[0], pts[pts.len() - 1]);
    let d = [b[0] - a[0], b[1] - a[1]];
    let l = (d[0] * d[0] + d[1] * d[1]).sqrt();
    l > 0.0 && pts.iter().all(|p| ((p[0] - a[0]) * d[1] - (p[1] - a[1]) * d[0]).abs() / l < 1e-6)
}

/// The page as an ASCII DXF file.
pub fn write_dxf(page: &Page, version: DxfVersion) -> String {
    cadrs_drawing::dxf::write_dxf_version(page, version)
}

/// Writes `page` into `dir` as `<stem>.dxf`, or as `<stem>.dwg` through the DWG converter
/// (`dwg`); the path written.
pub fn write_file(page: &Page, version: DxfVersion, dwg: bool, dir: &std::path::Path, stem: &str) -> Result<std::path::PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let ext = if dwg { "dwg" } else { "dxf" };
    let path = crate::export::unique_path_ext(dir, stem, ext);
    let text = write_dxf(page, version);
    if dwg {
        let conv = cadrs_drawing::dwg::find_converter().filter(|c| c.can_write()).ok_or(cadrs_drawing::dwg::INSTALL_HINT)?;
        cadrs_drawing::dwg::dxf_to_dwg(&conv, &text, &path, version)?;
    } else {
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadrs_sketch::Vec2;

    /// P3F.2: a sketch rectangle with a circle in it is 4 LINEs and 1 CIRCLE in the DXF,
    /// read back with the drawings' DXF reader.
    #[test]
    fn a_rectangle_and_a_circle_are_four_lines_and_a_circle() {
        let mut s = Sketch::new();
        let c = [Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0), Vec2::new(100.0, 60.0), Vec2::new(0.0, 60.0)];
        for i in 0..4 {
            s.add_line(c[i], c[(i + 1) % 4]);
        }
        let center = s.add_point(Vec2::new(50.0, 30.0));
        s.curves.insert(cadrs_sketch::Curve { kind: CurveKind::Circle { center, radius: 10.0 }, construction: false });
        // A construction line is left out.
        let l = s.add_line(Vec2::new(0.0, 0.0), Vec2::new(100.0, 60.0));
        s.curves[l].construction = true;
        let page = sketch_page(&s, "Sketch 1");
        assert_eq!(page.counts(), (4, 0, 1, 0));
        let text = write_dxf(&page, DxfVersion::R2013);
        let back = cadrs_drawing::dxf::read_dxf(&text).unwrap();
        assert_eq!(cadrs_drawing::dxf::counts(&back), (4, 0, 1));
        // Millimetres ($INSUNITS 4), and the geometry where the sketch has it.
        assert!(text.contains("$INSUNITS\n 70\n4\n"), "{}", &text[..600]);
        assert_eq!(back.unit_mm, 1.0);
        use cadrs_drawing::sheet_sketch::Entity;
        let mut corners: Vec<[i64; 2]> = Vec::new();
        for e in &back.entities {
            match e {
                Entity::Line { a, b } => {
                    for p in [a, b] {
                        let q = [p[0].round() as i64, p[1].round() as i64];
                        assert!((p[0] - q[0] as f64).abs() < 1e-9 && (p[1] - q[1] as f64).abs() < 1e-9);
                        if !corners.contains(&q) {
                            corners.push(q);
                        }
                    }
                }
                Entity::Circle { center, radius } => {
                    assert_eq!((*center, *radius), ([50.0, 30.0], 10.0));
                }
                other => panic!("unexpected {other:?}"),
            }
        }
        corners.sort();
        assert_eq!(corners, vec![[0, 0], [0, 60], [100, 0], [100, 60]]);
    }

    /// P3F.2 judge: the extents are the geometry's lowest and highest points, below and left of
    /// the origin too (they were pinned at 0, 0).
    #[test]
    fn extents_reach_below_and_left_of_the_origin() {
        let mut s = Sketch::new();
        let c = [Vec2::new(-40.0, -25.0), Vec2::new(60.0, -25.0), Vec2::new(60.0, 35.0), Vec2::new(-40.0, 35.0)];
        for i in 0..4 {
            s.add_line(c[i], c[(i + 1) % 4]);
        }
        let center = s.add_point(Vec2::new(-50.0, 0.0));
        s.curves.insert(cadrs_sketch::Curve { kind: CurveKind::Circle { center, radius: 5.0 }, construction: false });
        let page = sketch_page(&s, "Sketch 1");
        let mut p = page.clone();
        assert_eq!(fit(&mut p), ([-55.0, -25.0], [60.0, 35.0]));
        assert_eq!((p.width, p.height), (115.0, 60.0));
        let text = write_dxf(&page, DxfVersion::R2013);
        let header = |var: &str| -> [f64; 2] {
            let at = text.find(var).unwrap();
            let v: Vec<f64> = text[at..].lines().skip(1).take(4).collect::<Vec<_>>().chunks(2).map(|c| c[1].trim().parse().unwrap()).collect();
            [v[0], v[1]]
        };
        assert_eq!(header("$EXTMIN"), [-55.0, -25.0]);
        assert_eq!(header("$EXTMAX"), [60.0, 35.0]);
    }
}
