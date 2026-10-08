//! Artwork from SVG files, for logos and the like on silkscreen or copper (KiCad's Import
//! Graphics): every filled or stroked path, curves flattened, as closed polygons in mm (SVG's
//! 96 px to the inch, so a drawing sized in mm comes in at that size), Y flipped up.

use crate::units::Pt;
use resvg::usvg;

/// Pieces a curve is cut into.
const CURVE_STEPS: usize = 12;

/// The SVG's shapes as polygons (mm, Y up), relative to the drawing's top-left corner.
pub fn svg_polygons(svg: &str) -> Result<Vec<Vec<Pt>>, String> {
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).map_err(|e| format!("not an SVG drawing: {e}"))?;
    let mut out = vec![];
    collect(tree.root(), &mut out);
    if out.is_empty() {
        return Err("the drawing has no shapes".into());
    }
    Ok(out)
}

fn collect(g: &usvg::Group, out: &mut Vec<Vec<Pt>>) {
    for n in g.children() {
        match n {
            usvg::Node::Group(g) => collect(g, out),
            usvg::Node::Path(p) if p.fill().is_some() || p.stroke().is_some() => {
                let t = p.abs_transform();
                let px_mm = 25.4 / 96.0;
                let pt = |x: f32, y: f32| {
                    let mut q = usvg::tiny_skia_path::Point::from_xy(x, y);
                    t.map_point(&mut q);
                    Pt::mm(q.x as f64 * px_mm, -(q.y as f64) * px_mm)
                };
                let mut cur: Vec<Pt> = vec![];
                let mut last = (0.0f32, 0.0f32);
                let mut flush = |cur: &mut Vec<Pt>| {
                    if cur.len() >= 3 {
                        out.push(std::mem::take(cur));
                    }
                    cur.clear();
                };
                for s in p.data().segments() {
                    use usvg::tiny_skia_path::PathSegment as S;
                    match s {
                        S::MoveTo(a) => {
                            flush(&mut cur);
                            cur.push(pt(a.x, a.y));
                            last = (a.x, a.y);
                        }
                        S::LineTo(a) => {
                            cur.push(pt(a.x, a.y));
                            last = (a.x, a.y);
                        }
                        S::QuadTo(c, a) => {
                            for k in 1..=CURVE_STEPS {
                                let u = k as f32 / CURVE_STEPS as f32;
                                let v = 1.0 - u;
                                cur.push(pt(v * v * last.0 + 2.0 * v * u * c.x + u * u * a.x, v * v * last.1 + 2.0 * v * u * c.y + u * u * a.y));
                            }
                            last = (a.x, a.y);
                        }
                        S::CubicTo(c1, c2, a) => {
                            for k in 1..=CURVE_STEPS {
                                let u = k as f32 / CURVE_STEPS as f32;
                                let v = 1.0 - u;
                                let (b0, b1, b2, b3) = (v * v * v, 3.0 * v * v * u, 3.0 * v * u * u, u * u * u);
                                cur.push(pt(b0 * last.0 + b1 * c1.x + b2 * c2.x + b3 * a.x, b0 * last.1 + b1 * c1.y + b2 * c2.y + b3 * a.y));
                            }
                            last = (a.x, a.y);
                        }
                        S::Close => flush(&mut cur),
                    }
                }
                flush(&mut cur);
            }
            _ => {}
        }
    }
}

/// Polygons moved so their box's top-left corner sits at `at`.
pub fn placed_at(polys: &[Vec<Pt>], at: Pt) -> Vec<Vec<Pt>> {
    let (min_x, max_y) = polys.iter().flatten().fold((i64::MAX, i64::MIN), |(x, y), p| (x.min(p.x), y.max(p.y)));
    let d = at - Pt::new(min_x, max_y);
    polys.iter().map(|ps| ps.iter().map(|p| *p + d).collect()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drawing_in_mm_comes_in_at_its_size() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="10mm" height="5mm" viewBox="0 0 10 5"><rect x="1" y="1" width="4" height="2" fill="black"/><circle cx="8" cy="2.5" r="1" fill="black"/></svg>"#;
        let polys = svg_polygons(svg).unwrap();
        assert_eq!(polys.len(), 2);
        let xs: Vec<f64> = polys[0].iter().map(|p| p.x as f64 / 1e6).collect();
        let ys: Vec<f64> = polys[0].iter().map(|p| p.y as f64 / 1e6).collect();
        let (x0, x1) = (xs.iter().cloned().fold(f64::MAX, f64::min), xs.iter().cloned().fold(f64::MIN, f64::max));
        assert!((x0 - 1.0).abs() < 1e-3 && (x1 - 5.0).abs() < 1e-3, "{x0} {x1}");
        // Y up: the rectangle's top (SVG y = 1) is at -1.
        assert!(ys.iter().any(|y| (y + 1.0).abs() < 1e-3));
        let placed = placed_at(&polys, Pt::mm(100.0, -50.0));
        let min_x = placed.iter().flatten().map(|p| p.x).min().unwrap();
        let max_y = placed.iter().flatten().map(|p| p.y).max().unwrap();
        assert_eq!((min_x, max_y), (Pt::mm(100.0, 0.0).x, Pt::mm(0.0, -50.0).y));
    }

    #[test]
    fn the_d4_logo_reads() {
        let polys = svg_polygons(include_str!("../../../fixtures/eda/d4_logo.svg")).unwrap();
        assert_eq!(polys.len(), 3);
        let w = polys.iter().flatten().map(|p| p.x).max().unwrap() - polys.iter().flatten().map(|p| p.x).min().unwrap();
        assert!((w as f64 / 1e6 - 5.609).abs() < 0.01, "{w}");
    }
}
