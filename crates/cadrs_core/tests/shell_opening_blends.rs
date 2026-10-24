//! Rounds and a chamfer round the opening of an outward shell (a user's document,
//! `fixtures/shell_opening_chamfer.cadrs`): a 30 mm cube with every edge rounded 5, shelled 6.5
//! outward with its back face, two top rounds and their corner taken away; Fillet 2 rounds the
//! opening's seven rims, Chamfer 1 (tangent propagation) chamfers the rim round the back wall's
//! cut end. The document rebuilds at a range of fillet radii and chamfer widths: every feature
//! builds, and the chamfer takes material off, more the wider it is, about as the square of
//! its width.
#![cfg(feature = "brep")]

use cadrs_core::document::Document;
use cadrs_core::{
    ElementId, FeatureId, FeatureKind, History, Store, commands::SetFeature, rebuild,
};

/// The fillet radii the sweep tries (the document's own 2.8 among them).
const RADII: [f64; 6] = [1.0, 2.0, 2.8, 3.25, 3.5, 4.0];
/// The chamfer widths it tries at each (the document's own 2).
const WIDTHS: [f64; 4] = [0.5, 1.0, 1.5, 2.0];

struct Studio {
    d: Document,
    h: History,
    el: ElementId,
    fillet: FeatureId,
    chamfer: FeatureId,
}

impl Studio {
    fn new() -> Self {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/shell_opening_chamfer.cadrs");
        let d = Store::load_path(&path).expect("the fixture loads").document;
        let el = d.elements[0].id;
        let id = |name: &str| {
            d.element(el)
                .unwrap()
                .features()
                .iter()
                .find(|f| f.name == name)
                .unwrap_or_else(|| panic!("no feature {name}"))
                .id
        };
        let (fillet, chamfer) = (id("Fillet 2"), id("Chamfer 1"));
        Self {
            d,
            h: History::default(),
            el,
            fillet,
            chamfer,
        }
    }

    /// Fillet 2's radius `r` and Chamfer 1's width `w`.
    fn set(&mut self, r: f64, w: f64) {
        let el = self.d.element(self.el).unwrap();
        let mut fillet = el.feature(self.fillet).unwrap().kind.clone();
        let FeatureKind::Fillet(x) = &mut fillet else {
            panic!("Fillet 2 isn't a fillet")
        };
        x.size = r;
        x.size_expr = format!("{r} mm");
        let mut chamfer = el.feature(self.chamfer).unwrap().kind.clone();
        let FeatureKind::Chamfer(x) = &mut chamfer else {
            panic!("Chamfer 1 isn't a chamfer")
        };
        x.distance = w;
        x.distance_expr = format!("{w} mm");
        for (feature, kind) in [(self.fillet, fillet), (self.chamfer, chamfer)] {
            self.h
                .execute(
                    &mut self.d,
                    &SetFeature {
                        element: self.el,
                        feature,
                        kind,
                        label: "Edit".into(),
                    },
                )
                .expect("the edit applies");
        }
    }

    /// The part's volume built up to and with the chamfer (`with_chamfer`) or without it; the
    /// features that failed, by name and why, otherwise.
    fn volume(&self, with_chamfer: bool) -> Result<f64, String> {
        let el = self.d.element(self.el).unwrap();
        let mut features = el.active_features();
        if !with_chamfer {
            features.retain(|f| f.id != self.chamfer);
        }
        let b = rebuild::build(&features);
        if !b.errors.is_empty() {
            let name = |id: &FeatureId| {
                el.feature(*id)
                    .map_or_else(|| format!("{id:?}"), |f| f.name.clone())
            };
            return Err(b
                .errors
                .iter()
                .map(|(id, e)| format!("{}: {e}", name(id)))
                .collect::<Vec<_>>()
                .join("; "));
        }
        match b.parts.as_slice() {
            [p] => Ok(p.mass.as_ref().expect("the part's mass").volume),
            ps => Err(format!("{} parts", ps.len())),
        }
    }
}

/// Every radius and width: what failed (the fillet alone, or the chamfer after it) and where
/// the chamfer didn't take more off the wider it was.
fn sweep(radii: &[f64], widths: &[f64]) -> Vec<String> {
    let mut s = Studio::new();
    let mut bad = Vec::new();
    for &r in radii {
        s.set(r, widths[0]);
        let filleted = match s.volume(false) {
            Ok(v) => v,
            Err(e) => {
                bad.push(format!("fillet {r}: {e}"));
                continue;
            }
        };
        let mut last = 0.0;
        let mut first: Option<(f64, f64)> = None;
        for &w in widths {
            s.set(r, w);
            match s.volume(true) {
                Ok(v) => {
                    let removed = filleted - v;
                    if removed <= last {
                        bad.push(format!(
                            "fillet {r}, chamfer {w}: takes off {removed:.4} mm³, no more than {last:.4} narrower"
                        ));
                    }
                    // A section of w²/2 or so along the rims: the narrowest's times (w/w₀)²,
                    // a little more where corners and fans grow faster than that.
                    if let Some((w0, v0)) = first {
                        let ratio = removed / (v0 * (w / w0).powi(2));
                        if !(0.9..1.15).contains(&ratio) {
                            bad.push(format!(
                                "fillet {r}, chamfer {w}: takes off {removed:.4} mm³, {ratio:.3} of (w/w₀)² times {v0:.4}"
                            ));
                        }
                    } else {
                        first = Some((w, removed));
                    }
                    last = removed;
                }
                Err(e) => bad.push(format!("fillet {r}, chamfer {w}: {e}")),
            }
        }
    }
    bad
}

#[test]
fn the_documents_own_fillet_and_chamfer() {
    let bad = sweep(&[2.8], &[2.0]);
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// Fillet 2 at 1 mm, the chamfer 0.5 and 2: wider than the rounds it runs round and down, it
/// fans at those corners (the neighbours' rails meeting in a point short of them, a cone from
/// there down to the corner's other rail).
#[test]
fn a_chamfer_wider_than_the_rounds_fans() {
    let bad = sweep(&[1.0], &[0.5, 2.0]);
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// Slow (a rebuild of the shell, fillets and chamfer each time): run with `--ignored`.
#[test]
#[ignore]
fn fillet_radii_and_chamfer_widths() {
    let bad = sweep(&RADII, &WIDTHS);
    assert!(bad.is_empty(), "{} failed:\n{}", bad.len(), bad.join("\n"));
}

/// Tangent propagation from Chamfer 1's seed on the opening's rim (the body after Fillet 2 at
/// the document's 2.8 mm): the rim's sharp edges round the opening, not the smooth edges where
/// the rounded body's faces meet tangentially (a chamfer's chain ran on along every one).
#[test]
fn the_chamfers_chain_skips_smooth_edges() {
    let s = Studio::new();
    let el = s.d.element(s.el).unwrap();
    let mut features = el.active_features();
    features.retain(|f| f.id != s.chamfer);
    let b = rebuild::build(&features);
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    let solid = &b.parts[0].solid;
    let seed = [15.0, 1.4, 25.6];
    let dist = |e: &cadrs_core::solid::SolidEdge| {
        e.points
            .iter()
            .map(|p| (0..3).map(|i| (p[i] - seed[i]).powi(2)).sum::<f64>())
            .fold(f64::MAX, f64::min)
    };
    let start = solid
        .edges
        .iter()
        .min_by(|a, c| dist(a).total_cmp(&dist(c)))
        .unwrap();
    let group = start.tangent_group;
    // The rim's 16 edges (running on along the smooth ones too, 62).
    let chain = solid
        .edges
        .iter()
        .filter(|e| e.tangent_group == group)
        .count();
    assert_eq!(chain, 16);
}
