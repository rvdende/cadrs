//! Onshape's Frame feature onto cadrs's (`cadrs_core::frame`). A child of `features`, so it
//! shares its parameter helpers.
//!
//! An Onshape frame's profile is a sketch of another document (a profile library's Part Studio,
//! at a version, in one configuration). The scraper stores that sketch solved, as
//! `<doc>/versions/<version>/<element>/sketches-<configuration>.json`; its closed loops become
//! the frame's own section ([`cadrs_core::frame::LibraryProfile`]), so the imported document
//! needs neither the library nor an extra sketch feature.

use cadrs_core::command::CommandError;
use cadrs_core::document::FeatureKind;
use cadrs_core::frame::{Anchor, CornerType, FrameFeature, LibraryProfile, ProfileFamily};
use cadrs_kernel::{Curve2, Loop, Region};
use cadrs_sketch::units::Quantity;
use nalgebra::Point2;
use serde_json::Value;

use crate::import::PartStudio;
use crate::raw::read_json;
use crate::refs;
use crate::report::FeatureReport;
use crate::sketch::param;

/// Metres (Onshape's REST unit) to mm.
const MM: f64 = 1000.0;

/// The file name's part for a configuration: its `parameter=value` pairs in order, joined by
/// `;`, with everything but letters, digits, `=`, `.`, `-` and `_` as `_`. The scraper
/// (`tools/onshape/scrape.js`) names the files so.
pub(crate) fn configuration_key(configuration: &Value) -> String {
    let pairs: Vec<String> = configuration
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            let id = c["parameterId"].as_str()?;
            let v = match &c["value"] {
                Value::String(s) => s.clone(),
                Value::Bool(b) => b.to_string(),
                Value::Null => c["expression"].as_str()?.to_string(),
                v => v.to_string(),
            };
            Some(format!("{id}={v}"))
        })
        .collect();
    pairs
        .join(";")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "=.-_;".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// `d<doc>::v<version>::e<element>::m<microversion>`: the document, version and element.
fn namespace_parts(ns: &str) -> Option<(&str, &str, &str)> {
    let mut parts = ns.split("::");
    let d = parts.next()?.strip_prefix('d')?;
    let v = parts.next()?.strip_prefix('v')?;
    let e = parts.next()?.strip_prefix('e')?;
    Some((d, v, e))
}

/// One curve of a profile with its ends, to chain into loops.
struct Piece {
    curve: Curve2,
    a: Point2<f64>,
    b: Point2<f64>,
}

fn reversed(c: &Curve2) -> Curve2 {
    match c {
        Curve2::Line { a, b, source } => Curve2::Line {
            a: *b,
            b: *a,
            source: *source,
        },
        Curve2::Arc {
            center,
            radius,
            start_angle,
            sweep,
            source,
        } => Curve2::Arc {
            center: *center,
            radius: *radius,
            start_angle: start_angle + sweep,
            sweep: -sweep,
            source: *source,
        },
        c => c.clone(),
    }
}

/// Points along a loop (its curves' ends and arcs' middles): enough for its area's sign and a
/// box.
fn outline(l: &Loop) -> Vec<Point2<f64>> {
    let mut out = Vec::new();
    for c in &l.curves {
        match c {
            Curve2::Line { a, .. } => out.push(*a),
            Curve2::Arc {
                center,
                radius,
                start_angle,
                sweep,
                ..
            } => {
                for k in 0..4 {
                    let t = start_angle + sweep * k as f64 / 4.0;
                    out.push(Point2::new(
                        center.x + radius * t.cos(),
                        center.y + radius * t.sin(),
                    ));
                }
            }
            Curve2::Circle { center, radius, .. } => {
                for k in 0..8 {
                    let t = std::f64::consts::TAU * k as f64 / 8.0;
                    out.push(Point2::new(
                        center.x + radius * t.cos(),
                        center.y + radius * t.sin(),
                    ));
                }
            }
            _ => {}
        }
    }
    out
}

fn signed_area(pts: &[Point2<f64>]) -> f64 {
    let n = pts.len();
    (0..n)
        .map(|i| {
            let (p, q) = (pts[i], pts[(i + 1) % n]);
            p.x * q.y - q.x * p.y
        })
        .sum::<f64>()
        / 2.0
}

fn inside(p: Point2<f64>, poly: &[Point2<f64>]) -> bool {
    let n = poly.len();
    let mut c = false;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            c = !c;
        }
    }
    c
}

/// The closed regions of a solved Onshape sketch (the first of `sketches`), in mm in the
/// sketch's own coordinates: its lines, arcs and circles that are not construction, chained
/// into loops, each loop inside another a hole of it (and one inside a hole a region again).
pub(crate) fn profile_regions(sketches: &Value) -> Option<(String, Vec<Region>)> {
    let sk = sketches["sketches"].as_array()?.first()?;
    let name = sk["name"].as_str().unwrap_or("Profile").to_string();
    let v2 = |v: &Value| Some(Point2::new(v["x"].as_f64()? * MM, v["y"].as_f64()? * MM));
    let mut loops: Vec<Loop> = Vec::new();
    let mut pieces: Vec<Piece> = Vec::new();
    for e in sk["entities"].as_array()? {
        if e["isConstruction"].as_bool() == Some(true) {
            continue;
        }
        let g = &e["geometry"];
        match e["sketchEntityType"].as_str()? {
            "skCircle" => loops.push(Loop {
                curves: vec![Curve2::Circle {
                    center: v2(&g["center2d"])?,
                    radius: g["radius"].as_f64()? * MM,
                    source: None,
                }],
            }),
            "skLineSegment" => {
                let (o, d) = (v2(&g["position2d"])?, &g["direction2d"]);
                let d = nalgebra::Vector2::new(d["x"].as_f64()?, d["y"].as_f64()?);
                let (t0, t1) = (
                    e["startParameter"].as_f64()? * MM,
                    e["endParameter"].as_f64()? * MM,
                );
                let (a, b) = (o + d * t0, o + d * t1);
                pieces.push(Piece {
                    curve: Curve2::Line { a, b, source: None },
                    a,
                    b,
                });
            }
            "skArc" => {
                let center = v2(&g["center2d"])?;
                let radius = g["radius"].as_f64()? * MM;
                let (t0, t1) = (e["startParameter"].as_f64()?, e["endParameter"].as_f64()?);
                // A clockwise arc's parameter runs the other way round.
                let (start_angle, sweep) = if g["clockWise"].as_bool() == Some(true) {
                    (-t0, -(t1 - t0))
                } else {
                    (t0, t1 - t0)
                };
                let at =
                    |t: f64| Point2::new(center.x + radius * t.cos(), center.y + radius * t.sin());
                pieces.push(Piece {
                    a: at(start_angle),
                    b: at(start_angle + sweep),
                    curve: Curve2::Arc {
                        center,
                        radius,
                        start_angle,
                        sweep,
                        source: None,
                    },
                });
            }
            _ => {}
        }
    }
    // Chain the pieces end to end.
    let tol = 1e-6;
    while let Some(first) = pieces.pop() {
        let (start, mut end) = (first.a, first.b);
        let mut curves = vec![first.curve];
        while (end - start).norm() > tol {
            let next = pieces
                .iter()
                .position(|p| (p.a - end).norm() < tol || (p.b - end).norm() < tol)?;
            let p = pieces.swap_remove(next);
            if (p.a - end).norm() < tol {
                end = p.b;
                curves.push(p.curve);
            } else {
                end = p.a;
                curves.push(reversed(&p.curve));
            }
        }
        loops.push(Loop { curves });
    }
    // Counter-clockwise, the largest first; each a hole of the smallest loop round it, or a
    // region of its own inside a hole.
    let mut shaped: Vec<(Loop, Vec<Point2<f64>>, f64)> = loops
        .into_iter()
        .map(|l| {
            let pts = outline(&l);
            let area = signed_area(&pts);
            if area < 0.0 {
                let l = Loop {
                    curves: l.curves.iter().rev().map(reversed).collect(),
                };
                let pts = outline(&l);
                (l, pts, -area)
            } else {
                (l, pts, area)
            }
        })
        .collect();
    shaped.sort_by(|a, b| b.2.total_cmp(&a.2));
    // (Depth: how many larger loops hold it.)
    let depth: Vec<usize> = (0..shaped.len())
        .map(|i| {
            (0..i)
                .filter(|&j| inside(shaped[i].1[0], &shaped[j].1))
                .count()
        })
        .collect();
    let mut regions: Vec<(usize, Region)> = Vec::new();
    for (i, (l, pts, _)) in shaped.iter().enumerate() {
        if depth[i] % 2 == 0 {
            regions.push((
                i,
                Region {
                    outer: l.clone(),
                    holes: Vec::new(),
                    source: None,
                },
            ));
        } else if let Some((_, r)) = regions
            .iter_mut()
            .rev()
            .find(|(j, _)| depth[*j] + 1 == depth[i] && inside(pts[0], &shaped[*j].1))
        {
            r.holes.push(l.clone());
        }
    }
    let regions: Vec<Region> = regions.into_iter().map(|(_, r)| r).collect();
    (!regions.is_empty()).then_some((name, regions))
}

impl PartStudio<'_> {
    /// The profile an Onshape frame names: the library sketch's regions, from the scraped file.
    fn frame_profile(&self, f: &Value) -> Result<LibraryProfile, CommandError> {
        let p =
            param(f, "profileSketch").ok_or_else(|| CommandError::Invalid("no profile".into()))?;
        let ns = p["namespace"].as_str().unwrap_or_default();
        let (d, v, e) = namespace_parts(ns).ok_or_else(|| {
            CommandError::Invalid("its profile is not a sketch of a versioned document".into())
        })?;
        let key = configuration_key(&p["configuration"]);
        let root = self
            .raw
            .dir
            .parent()
            .and_then(|d| d.parent())
            .ok_or_else(|| CommandError::Invalid("no raw folder".into()))?;
        let file = root
            .join(d)
            .join("versions")
            .join(v)
            .join(e)
            .join(if key.is_empty() {
                "sketches.json".to_string()
            } else {
                format!("sketches-{key}.json")
            });
        let json = read_json(&file).ok_or_else(|| {
            CommandError::Invalid(format!(
                "its profile sketch was not scraped ({})",
                file.display()
            ))
        })?;
        let (name, regions) = profile_regions(&json).ok_or_else(|| {
            CommandError::Invalid("its profile sketch has no closed regions".into())
        })?;
        Ok(LibraryProfile { name, regions })
    }

    /// Onshape's Frame: members along sketch curves and edges, with a library profile.
    pub(crate) fn frame(
        &mut self,
        f: &Value,
        fid: &str,
        fr: &mut FeatureReport,
    ) -> Result<(), CommandError> {
        let library = self.frame_profile(f)?;
        let mut lost = 0;
        let path = self.curve_refs(refs::picks(param(f, "selections")), &mut lost);
        Self::lost_note(fr, lost, "path selections");
        if path.is_empty() {
            return Err(CommandError::Invalid(
                "none of its path selections could be translated".into(),
            ));
        }
        let (angle, angle_expr) = self.quantity(f, "angle", Quantity::Angle)?;
        let mut x = FrameFeature {
            path,
            family: ProfileFamily::Custom,
            size: String::new(),
            library: Some(library),
            anchor: Anchor::Origin,
            angle,
            angle_expr,
            mirror: Self::flag(f, "mirrorProfile"),
            merge_tangent: Self::flag(f, "mergeTangentSegments"),
            corner: match Self::text(f, "defaultCornerType").as_str() {
                "MITER" => CornerType::Miter,
                "BUTT" => CornerType::Butt,
                "COPED_BUTT" => CornerType::CopedButt,
                _ => CornerType::None,
            },
            flip: Self::flag(f, "defaultButtFlip"),
            ..Default::default()
        };
        if param(f, "cornerOverrides")
            .and_then(|p| p["items"].as_array())
            .is_some_and(|a| !a.is_empty())
        {
            fr.notes.push("corner overrides not imported yet".into());
        }
        if Self::flag(f, "trim") {
            let mut lost = 0;
            x.trim = self.parts_param(f, "trimBodies", &mut lost);
            for p in refs::picks(param(f, "trimPlanes")) {
                match self.plane_ref(&p) {
                    Some(pl) => x.trim_planes.push(pl),
                    None => lost += 1,
                }
            }
            Self::lost_note(fr, lost, "trim selections");
        }
        self.add(f, fid, "Frame", FeatureKind::Frame(x), fr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_round_tube_is_a_ring() {
        let json: Value = serde_json::json!({"sketches": [{"name": "Round tube", "entities": [
            {"sketchEntityType": "skCircle", "isConstruction": false,
             "geometry": {"radius": 0.0174625, "center2d": {"x": 0.0, "y": 0.0}}},
            {"sketchEntityType": "skPoint", "isConstruction": true},
            {"sketchEntityType": "skCircle", "isConstruction": false,
             "geometry": {"radius": 0.0162179, "center2d": {"x": 0.0, "y": 0.0}}},
        ]}]});
        let (name, regions) = profile_regions(&json).unwrap();
        assert_eq!(name, "Round tube");
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].holes.len(), 1);
        let Curve2::Circle { radius, .. } = regions[0].outer.curves[0] else {
            panic!("a circle");
        };
        assert!((radius - 17.4625).abs() < 1e-12);
    }

    #[test]
    fn a_configuration_names_its_file() {
        let c = serde_json::json!([
            {"parameterId": "List_P3", "value": "_1_25__x_1_25_"},
            {"parameterId": "Rounded", "value": true},
        ]);
        assert_eq!(configuration_key(&c), "List_P3=_1_25__x_1_25_;Rounded=true");
    }
}
