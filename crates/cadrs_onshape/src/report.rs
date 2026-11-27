//! What an import did with each Onshape feature, so the gaps are visible and can be worked
//! through feature by feature.
//!
//! The same report is available as JSON ([`DocumentReport::to_json`], aggregated across the
//! documents of one run by [`aggregate`]): the desktop app's ticket list and the importer's
//! tests read it instead of parsing the text. Every way an import can fall short is a
//! [`Defect`] with a stable `key` (feature kind and normalized reason), so the same gap found
//! in several documents or features becomes one issue, not one per occurrence.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::Serialize;
use serde_json::{Value, json};

/// How a feature came across.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
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

impl Outcome {
    /// The report's word for it (`full`, `partial`, `skipped`, `suppressed`).
    pub fn tag(self) -> &'static str {
        match self {
            Outcome::Full => "full",
            Outcome::Partial => "partial",
            Outcome::Skipped => "skipped",
            Outcome::Suppressed => "suppressed",
        }
    }

    /// A defect's severity, for the ones worth a ticket (suppressed features are not).
    fn severity(self) -> Option<&'static str> {
        match self {
            Outcome::Skipped => Some("high"),
            Outcome::Partial => Some("medium"),
            Outcome::Full | Outcome::Suppressed => None,
        }
    }
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
    /// Onshape's face count and the matched cadrs part's, when both are known.
    pub faces: Option<(usize, usize)>,
}

impl PartCheck {
    /// The relative volume difference (0 = identical), if a part matched.
    pub fn error(&self) -> Option<f64> {
        self.cadrs_volume
            .map(|v| (v - self.onshape_volume).abs() / self.onshape_volume.abs().max(1e-9))
    }

    /// Matched: within 0.1 % of Onshape's volume, where Onshape has it (its box), and with as
    /// many faces.
    pub fn matches(&self) -> bool {
        self.error().is_some_and(|e| e < 1e-3)
            && self.bbox_note.is_none()
            && self.faces.is_none_or(|(o, c)| o == c)
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
    /// An assembly's instances placed and mates imported (each of how many).
    pub assembly: Option<AssemblyCounts>,
}

/// How much of an assembly came across.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AssemblyCounts {
    /// Instances placed, of all.
    pub instances: (usize, usize),
    /// Mates imported, of all (groups and mate connectors are listed as features).
    pub mates: (usize, usize),
}

#[derive(Debug, Clone, Default)]
pub struct DocumentReport {
    pub name: String,
    pub onshape_id: String,
    /// The cadrs document id, once written.
    pub cadrs_id: Option<String>,
    pub elements: Vec<ElementReport>,
}

/// One way an import fell short: the unit a ticket is opened for. `key` names the kind of gap
/// (not one occurrence), so the app can group the defects of a run that share it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Defect {
    pub key: String,
    /// `high` (a feature was not imported), `medium` (something was dropped).
    pub severity: &'static str,
    pub document: String,
    pub element: String,
    pub element_kind: String,
    pub feature: String,
    pub feature_kind: String,
    pub outcome: String,
    /// The mismatch or the note the importer wrote (its numbers vary between runs).
    pub reason: String,
}

impl ElementReport {
    pub fn count(&self, o: Outcome) -> usize {
        self.features.iter().filter(|f| f.outcome == o).count()
    }

    /// Every part matched, within 0.1 % of Onshape's volume and where Onshape has it.
    pub fn parts_match(&self) -> bool {
        !self.parts.is_empty() && self.parts.iter().all(PartCheck::matches)
    }

    fn counts_json(&self) -> Value {
        json!({
            "full": self.count(Outcome::Full),
            "partial": self.count(Outcome::Partial),
            "skipped": self.count(Outcome::Skipped),
            "suppressed": self.count(Outcome::Suppressed),
        })
    }

    fn to_json(&self) -> Value {
        let features: Vec<Value> = self
            .features
            .iter()
            .map(|f| {
                json!({
                    "name": f.name, "kind": f.kind, "outcome": f.outcome, "notes": f.notes,
                })
            })
            .collect();
        let parts: Vec<Value> = self
            .parts
            .iter()
            .map(|p| {
                json!({
                    "name": p.name,
                    "onshape_volume": p.onshape_volume,
                    "cadrs_volume": p.cadrs_volume,
                    "relative_error": p.error(),
                    "faces": p.faces.map(|(o, c)| [o, c]),
                    "bbox_note": p.bbox_note,
                    "match": p.matches(),
                })
            })
            .collect();
        json!({
            "name": self.name,
            "kind": self.kind.to_lowercase(),
            "imported": self.imported,
            "notes": self.notes,
            "counts": self.counts_json(),
            "parts_matched": self.parts.iter().filter(|p| p.matches()).count(),
            "features": features,
            "parts": parts,
            "assembly": self.assembly.map(|a| json!({
                "instances": { "placed": a.instances.0, "total": a.instances.1 },
                "mates": { "placed": a.mates.0, "total": a.mates.1 },
            })),
        })
    }

    /// The gaps this element has, one [`Defect`] per note (or per mismatch), whether or not
    /// the report prints it.
    pub fn defects(&self, document: &str) -> Vec<Defect> {
        let mut out = Vec::new();
        let mut push =
            |key: String, severity, feature: &str, kind: &str, outcome: &str, reason: String| {
                out.push(Defect {
                    key,
                    severity,
                    document: document.to_string(),
                    element: self.name.clone(),
                    element_kind: self.kind.to_lowercase(),
                    feature: feature.to_string(),
                    feature_kind: kind.to_string(),
                    outcome: outcome.to_string(),
                    reason,
                });
            };
        for f in &self.features {
            let Some(severity) = f.outcome.severity() else {
                continue;
            };
            // A feature without a note still needs a reason ("skipped, and it didn't say why").
            let notes = if f.notes.is_empty() {
                vec![match f.outcome {
                    Outcome::Partial => "imported with something dropped".to_string(),
                    _ => "not imported".to_string(),
                }]
            } else {
                f.notes.clone()
            };
            for note in notes {
                push(
                    ticket_key(&f.kind, &note),
                    severity,
                    &f.name,
                    &f.kind,
                    f.outcome.tag(),
                    note,
                );
            }
        }
        for p in &self.parts {
            match p.error() {
                None => push(
                    "onshape-import:part:missing-part".into(),
                    "high",
                    &p.name,
                    "part",
                    "missing",
                    format!("part is missing (Onshape has {:.1} mm³)", p.onshape_volume),
                ),
                Some(e) if e >= 1e-3 => push(
                    "onshape-import:part:volume-mismatch".into(),
                    "high",
                    &p.name,
                    "part",
                    "mismatch",
                    format!(
                        "volume is {:.2} % off ({:.1} vs {:.1} mm³)",
                        e * 100.0,
                        p.cadrs_volume.unwrap_or(0.0),
                        p.onshape_volume
                    ),
                ),
                Some(_) => {}
            }
            if let Some((onshape, cadrs)) = p.faces.filter(|(o, c)| o != c) {
                push(
                    "onshape-import:part:face-count-mismatch".into(),
                    "medium",
                    &p.name,
                    "part",
                    "mismatch",
                    format!("{cadrs} faces, Onshape has {onshape}"),
                );
            }
            if let Some(note) = &p.bbox_note {
                push(
                    "onshape-import:part:bounding-box-mismatch".into(),
                    "medium",
                    &p.name,
                    "part",
                    "mismatch",
                    note.clone(),
                );
            }
        }
        if let Some(a) = self.assembly {
            if a.instances.0 < a.instances.1 {
                push(
                    "onshape-import:assembly:instances-not-placed".into(),
                    "high",
                    "",
                    "assembly",
                    "partial",
                    format!("{} of {} instances placed", a.instances.0, a.instances.1),
                );
            }
            if a.mates.0 < a.mates.1 {
                push(
                    "onshape-import:assembly:mates-not-imported".into(),
                    "medium",
                    "",
                    "assembly",
                    "partial",
                    format!("{} of {} mates imported", a.mates.0, a.mates.1),
                );
            }
        }
        for note in &self.notes {
            push(
                ticket_key("element", note),
                "high",
                &self.name,
                &self.kind.to_lowercase(),
                "note",
                note.clone(),
            );
        }
        out
    }
}

impl DocumentReport {
    fn counts_json(&self) -> Value {
        let sum = |o: Outcome| self.elements.iter().map(|e| e.count(o)).sum::<usize>();
        let parts_matched: usize = self
            .elements
            .iter()
            .flat_map(|e| &e.parts)
            .filter(|p| p.matches())
            .count();
        let parts_total: usize = self.elements.iter().map(|e| e.parts.len()).sum();
        json!({
            "part_studios": self.elements.iter().filter(|e| e.kind == "PARTSTUDIO").count(),
            "elements": self.elements.len(),
            "features": {
                "full": sum(Outcome::Full),
                "partial": sum(Outcome::Partial),
                "skipped": sum(Outcome::Skipped),
                "suppressed": sum(Outcome::Suppressed),
            },
            "parts": { "matched": parts_matched, "total": parts_total },
        })
    }

    /// Every gap in the document, with `document` in each one.
    pub fn defects(&self) -> Vec<Defect> {
        self.elements
            .iter()
            .flat_map(|e| e.defects(&self.name))
            .collect()
    }

    /// The document's report as JSON (one entry in [`aggregate`]'s `documents`).
    pub fn to_json(&self, elapsed_s: f64) -> Value {
        let defects: Vec<Value> = self.defects().iter().map(Defect::to_json).collect();
        let elements: Vec<Value> = self.elements.iter().map(ElementReport::to_json).collect();
        json!({
            "name": self.name,
            "onshape_id": self.onshape_id,
            "cadrs_id": self.cadrs_id,
            "elapsed_s": (elapsed_s * 1000.0).round() / 1000.0,
            "counts": self.counts_json(),
            "elements": elements,
            "defects": defects,
        })
    }
}

impl Defect {
    fn to_json(&self) -> Value {
        json!({
            "key": self.key,
            "severity": self.severity,
            "document": self.document,
            "element": self.element,
            "element_kind": self.element_kind,
            "feature": self.feature,
            "feature_kind": self.feature_kind,
            "outcome": self.outcome,
            "reason": self.reason,
        })
    }
}

/// A reason with its run-specific numbers and names blanked, so the same gap groups together:
/// `"2 × DISTANCE (unsupported references) constraint dropped"` and the same with 6 are one.
pub fn normalize_reason(reason: &str) -> String {
    let mut out = String::new();
    let mut chars = reason.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            while chars
                .peek()
                .is_some_and(|c| c.is_ascii_digit() || *c == '.')
            {
                chars.next();
            }
            out.push('n');
        } else if c == '"' {
            for d in chars.by_ref() {
                if d == '"' {
                    break;
                }
            }
            out.push('"');
            out.push('…');
            out.push('"');
        } else {
            out.push(c.to_ascii_lowercase());
        }
    }
    let mut s = String::new();
    for w in out.split_whitespace() {
        if !s.is_empty() {
            s.push(' ');
        }
        s.push_str(w);
    }
    s.trim_end_matches(['.', ',', ';']).to_string()
}

/// A defect's stable id: `onshape-import:<feature kind>:<normalized reason, slugged>`.
pub fn ticket_key(kind: &str, reason: &str) -> String {
    let normalized = normalize_reason(reason);
    let mut slug = String::new();
    for c in normalized.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-').chars().take(72).collect::<String>();
    format!("onshape-import:{}:{slug}", kind.to_lowercase())
}

/// One group of defects that share a [`Defect::key`], with a few examples.
#[derive(Debug, Clone)]
struct Ticket {
    key: String,
    severity: String,
    reason: String,
    feature_kinds: BTreeSet<String>,
    outcomes: BTreeSet<String>,
    count: usize,
    examples: Vec<Value>,
}

/// The JSON report of one importer run: the documents, their defects, and the defects grouped
/// into tickets. `documents` are the per-document objects [`DocumentReport::to_json`] makes,
/// plus entries the supervisor makes for documents that failed outright (`{"failed": true}`).
pub fn aggregate(documents: Vec<Value>, tool: &str, user: &str, dry_run: bool) -> Value {
    let mut tickets: BTreeMap<String, Ticket> = BTreeMap::new();
    let mut features = [0usize; 4];
    let mut parts = (0usize, 0usize);
    let mut part_studios = 0usize;
    let mut clean_studios = 0usize;
    let mut failed = 0usize;
    let mut defects = 0usize;
    for doc in &documents {
        if doc.get("failed").and_then(Value::as_bool).unwrap_or(false) {
            failed += 1;
        }
        if let Some(c) = doc.get("counts").and_then(Value::as_object) {
            part_studios += c.get("part_studios").and_then(Value::as_u64).unwrap_or(0) as usize;
            features[0] += c["features"]["full"].as_u64().unwrap_or(0) as usize;
            features[1] += c["features"]["partial"].as_u64().unwrap_or(0) as usize;
            features[2] += c["features"]["skipped"].as_u64().unwrap_or(0) as usize;
            features[3] += c["features"]["suppressed"].as_u64().unwrap_or(0) as usize;
            parts.0 += c["parts"]["matched"].as_u64().unwrap_or(0) as usize;
            parts.1 += c["parts"]["total"].as_u64().unwrap_or(0) as usize;
            if c["parts"]["total"].as_u64().unwrap_or(0) > 0
                && c["parts"]["matched"] == c["parts"]["total"]
                && c["features"]["skipped"].as_u64().unwrap_or(0) == 0
                && c["features"]["partial"].as_u64().unwrap_or(0) == 0
            {
                clean_studios += 1;
            }
        }
        for d in doc
            .get("defects")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            defects += 1;
            let key = d["key"].as_str().unwrap_or("").to_string();
            let severity = d["severity"].as_str().unwrap_or("medium").to_string();
            let entry = tickets.entry(key.clone()).or_insert_with(|| Ticket {
                key,
                severity: severity.clone(),
                reason: d["reason"].as_str().unwrap_or("").to_string(),
                feature_kinds: BTreeSet::new(),
                outcomes: BTreeSet::new(),
                count: 0,
                examples: Vec::new(),
            });
            // "high" beats "medium".
            if rank(&severity) < rank(&entry.severity) {
                entry.severity = severity;
            }
            entry.count += 1;
            if let Some(k) = d["feature_kind"].as_str() {
                entry.feature_kinds.insert(k.to_string());
            }
            if let Some(o) = d["outcome"].as_str() {
                entry.outcomes.insert(o.to_string());
            }
            if entry.examples.len() < 5 {
                entry.examples.push(json!({
                    "document": d["document"],
                    "element": d["element"],
                    "feature": d["feature"],
                    "outcome": d["outcome"],
                    "reason": d["reason"],
                }));
            }
        }
    }
    let mut list: Vec<&Ticket> = tickets.values().collect();
    list.sort_by(|a, b| {
        rank(&a.severity)
            .cmp(&rank(&b.severity))
            .then(b.count.cmp(&a.count))
            .then(a.key.cmp(&b.key))
    });
    let tickets_json: Vec<Value> = list
        .iter()
        .map(|t| {
            json!({
                "key": t.key,
                "severity": t.severity,
                "count": t.count,
                "reason": t.reason,
                "feature_kinds": t.feature_kinds.iter().collect::<Vec<_>>(),
                "outcomes": t.outcomes.iter().collect::<Vec<_>>(),
                "examples": t.examples,
            })
        })
        .collect();
    let total = features.iter().sum::<usize>();
    json!({
        "kind": "cadrs-onshape-import",
        "schema": 1,
        "tool": tool,
        "user": user,
        "dry_run": dry_run,
        "summary": {
            "documents": documents.len(),
            "failed_documents": failed,
            "part_studios": part_studios,
            "clean_part_studios": clean_studios,
            "features": {
                "total": total,
                "full": features[0],
                "partial": features[1],
                "skipped": features[2],
                "suppressed": features[3],
            },
            "parts": { "matched": parts.0, "total": parts.1 },
            "defects": defects,
            "tickets": tickets_json.len(),
        },
        "tickets": tickets_json,
        "documents": documents,
    })
}

/// Severities sortable with "high" first.
fn rank(severity: &str) -> u8 {
    match severity {
        "high" => 0,
        "medium" => 1,
        _ => 2,
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
                    let matched = el.parts.iter().filter(|p| p.matches()).count();
                    write!(f, "; parts {matched}/{} match Onshape", el.parts.len())?;
                }
            }
            if let Some(a) = el.assembly {
                write!(
                    f,
                    ": instances {}/{} placed; mates {}/{}",
                    a.instances.0, a.instances.1, a.mates.0, a.mates.1
                )?;
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
                    Some(e) if e < 1e-3 => {
                        if let Some(n) = &p.bbox_note {
                            writeln!(f, "      placed     {}: {n}", p.name)?;
                        }
                    }
                    Some(e) => {
                        writeln!(
                            f,
                            "      volume     {}: {:.1} % off ({:.1} vs {:.1} mm³)",
                            p.name,
                            e * 100.0,
                            p.cadrs_volume.unwrap_or(0.0),
                            p.onshape_volume
                        )?;
                        if let Some(n) = &p.bbox_note {
                            writeln!(f, "                 {n}")?;
                        }
                    }
                    None => writeln!(
                        f,
                        "      missing    part {} ({:.1} mm³)",
                        p.name, p.onshape_volume
                    )?,
                }
                if let Some((o, c)) = p.faces.filter(|(o, c)| o != c) {
                    writeln!(f, "      faces      {}: {c} vs {o}", p.name)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element() -> ElementReport {
        ElementReport {
            name: "Part Studio 1".into(),
            kind: "PARTSTUDIO".into(),
            imported: true,
            features: vec![
                FeatureReport {
                    name: "Sketch 1".into(),
                    kind: "newSketch".into(),
                    outcome: Outcome::Partial,
                    notes: vec!["2 × DISTANCE (unsupported references) constraint dropped".into()],
                },
                FeatureReport {
                    name: "Shell 1".into(),
                    kind: "shell".into(),
                    outcome: Outcome::Skipped,
                    notes: vec!["no importer for this feature type yet".into()],
                },
            ],
            parts: vec![PartCheck {
                name: "Part 1".into(),
                onshape_volume: 100.0,
                cadrs_volume: Some(90.0),
                bbox_note: None,
                faces: Some((10, 11)),
            }],
            ..ElementReport::default()
        }
    }

    fn document() -> DocumentReport {
        DocumentReport {
            name: "Wrapper test".into(),
            onshape_id: "3fa34f71b410df24f38d45d1".into(),
            cadrs_id: Some("1".into()),
            elements: vec![element()],
        }
    }

    #[test]
    fn reasons_group_across_run_specific_numbers() {
        assert_eq!(
            normalize_reason("2 × DISTANCE (unsupported references) constraint dropped"),
            "n × distance (unsupported references) constraint dropped"
        );
        assert_eq!(
            normalize_reason("6 × DISTANCE (unsupported references) constraint dropped"),
            "n × distance (unsupported references) constraint dropped"
        );
        assert_eq!(
            normalize_reason("no such face \"1e5f\": maybe"),
            "no such face \"…\": maybe"
        );
        assert_eq!(
            ticket_key(
                "newSketch",
                "6 × DISTANCE (unsupported references) constraint dropped"
            ),
            "onshape-import:newsketch:n-distance-unsupported-references-constraint-dropped"
        );
    }

    #[test]
    fn a_documents_defects_are_its_features_and_mismatches() {
        let d = document();
        let defects = d.defects();
        let keys: Vec<&str> = defects.iter().map(|x| x.key.as_str()).collect();
        assert!(keys.contains(
            &"onshape-import:newsketch:n-distance-unsupported-references-constraint-dropped"
        ));
        assert!(keys.contains(&"onshape-import:shell:no-importer-for-this-feature-type-yet"));
        assert!(keys.contains(&"onshape-import:part:volume-mismatch"));
        assert!(keys.contains(&"onshape-import:part:face-count-mismatch"));
        assert!(!keys.iter().any(|k| k.contains("full")));
    }

    #[test]
    fn the_json_report_groups_defects_into_tickets() {
        let docs = vec![
            document().to_json(0.5),
            DocumentReport {
                name: "Other".into(),
                onshape_id: "0123456789abcdef01234567".into(),
                ..DocumentReport::default()
            }
            .to_json(0.1),
        ];
        let json = aggregate(docs, "cadrs-onshape test", "tester", false);
        assert_eq!(json["kind"], "cadrs-onshape-import");
        assert_eq!(json["summary"]["documents"], 2);
        assert_eq!(json["summary"]["features"]["total"], 2);
        assert_eq!(json["summary"]["parts"]["total"], 1);
        // The shell gap is the more severe, so its ticket comes first.
        assert_eq!(json["tickets"][0]["severity"], "high");
        assert_eq!(json["tickets"][0]["count"], 1);
        assert!(json["tickets"].as_array().unwrap().len() >= 4);
        let scale = json["tickets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["key"] == "onshape-import:part:volume-mismatch")
            .unwrap();
        assert_eq!(scale["examples"][0]["feature"], "Part 1");
        assert_eq!(json["documents"][0]["name"], "Wrapper test");
        assert_eq!(
            json["documents"][0]["elements"][0]["parts"][0]["faces"][1],
            11
        );
    }
}
