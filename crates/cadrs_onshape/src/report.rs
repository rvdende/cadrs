//! What an import did with each Onshape feature, so the gaps are visible and can be worked
//! through feature by feature.

use std::fmt;

/// How a feature came across.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Outcome {
    /// Imported with everything that matters.
    Full,
    /// Imported, but something was dropped or approximated (see the notes).
    Partial,
    /// Not imported (no cadrs equivalent yet, or it failed).
    Skipped,
    /// Suppressed in Onshape: not imported.
    Suppressed,
}

#[derive(Debug, Clone)]
pub struct FeatureReport {
    pub name: String,
    /// Onshape's feature type (`extrude`, `newSketch`, …).
    pub kind: String,
    pub outcome: Outcome,
    pub notes: Vec<String>,
}

/// A Part Studio's part compared with Onshape's mass properties.
#[derive(Debug, Clone)]
pub struct PartCheck {
    pub name: String,
    /// Onshape's volume (mm³) and the matched cadrs part's, if one matched.
    pub onshape_volume: f64,
    pub cadrs_volume: Option<f64>,
    /// How the matched part's bounding box differs from Onshape's, if it does.
    pub bbox_note: Option<String>,
}

impl PartCheck {
    /// The relative volume difference (0 = identical), if a part matched.
    pub fn error(&self) -> Option<f64> {
        self.cadrs_volume.map(|v| (v - self.onshape_volume).abs() / self.onshape_volume.abs().max(1e-9))
    }
}

#[derive(Debug, Clone, Default)]
pub struct ElementReport {
    pub name: String,
    /// `PARTSTUDIO`, `ASSEMBLY`, …
    pub kind: String,
    pub features: Vec<FeatureReport>,
    pub parts: Vec<PartCheck>,
    /// Problems that concern the whole element.
    pub notes: Vec<String>,
    pub imported: bool,
}

#[derive(Debug, Clone, Default)]
pub struct DocumentReport {
    pub name: String,
    pub onshape_id: String,
    /// The cadrs document id, once written.
    pub cadrs_id: Option<String>,
    pub elements: Vec<ElementReport>,
}

impl ElementReport {
    pub fn count(&self, o: Outcome) -> usize {
        self.features.iter().filter(|f| f.outcome == o).count()
    }

    /// Every part matched, within 0.1 % of Onshape's volume.
    pub fn parts_match(&self) -> bool {
        !self.parts.is_empty() && self.parts.iter().all(|p| p.error().is_some_and(|e| e < 1e-3))
    }
}

impl fmt::Display for DocumentReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "# {} ({})", self.name, self.onshape_id)?;
        for el in &self.elements {
            write!(f, "  [{}] {}", el.kind.to_lowercase(), el.name)?;
            if el.kind == "PARTSTUDIO" {
                write!(
                    f,
                    ": {} full, {} partial, {} skipped",
                    el.count(Outcome::Full),
                    el.count(Outcome::Partial),
                    el.count(Outcome::Skipped)
                )?;
                if !el.parts.is_empty() {
                    let matched = el.parts.iter().filter(|p| p.error().is_some_and(|e| e < 1e-3)).count();
                    write!(f, "; parts {matched}/{} match Onshape", el.parts.len())?;
                }
            }
            if !el.imported {
                write!(f, " (not imported)")?;
            }
            writeln!(f)?;
            for n in &el.notes {
                writeln!(f, "      ! {n}")?;
            }
            for fe in &el.features {
                if fe.outcome == Outcome::Full && fe.notes.is_empty() {
                    continue;
                }
                let tag = match fe.outcome {
                    Outcome::Full => "ok",
                    Outcome::Partial => "partial",
                    Outcome::Skipped => "SKIPPED",
                    Outcome::Suppressed => "suppressed",
                };
                writeln!(f, "      {tag:10} {} ({})", fe.name, fe.kind)?;
                for n in &fe.notes {
                    writeln!(f, "                 - {n}")?;
                }
            }
            for p in &el.parts {
                match p.error() {
                    Some(e) if e < 1e-3 => {}
                    Some(e) => {
                        writeln!(f, "      volume     {}: {:.1} % off ({:.1} vs {:.1} mm³)", p.name, e * 100.0, p.cadrs_volume.unwrap_or(0.0), p.onshape_volume)?;
                        if let Some(n) = &p.bbox_note {
                            writeln!(f, "                 {n}")?;
                        }
                    }
                    None => writeln!(f, "      missing    part {} ({:.1} mm³)", p.name, p.onshape_volume)?,
                }
            }
        }
        Ok(())
    }
}
