//! Feature-list queries (P3.9): the filter field's language (PS3.4–PS3.6), feature type names,
//! and dependencies (PS11.2).
//!
//! **Filter.** The text in the "Filter by name or type" field is one query:
//! - plain text matches a feature whose name *or* type contains it, ignoring case ("Extrude"
//!   lists every extrude, whatever its name; "Extrude 1" also matches "Extrude 10");
//! - a term in double quotes matches the exact name (`"Extrude 1"` does not match `Extrude 10`);
//! - a leading prefix narrows what the text is compared with: `:name <name>`, `:type <type>`,
//!   `:part <part>` (the features that made or changed a part with that name), `:folder <name>`
//!   (the features in that folder), `:errors [name]` (the features with errors, optionally with
//!   that name) and `:variable <name>` (P3F.4: the Variable defining `#name` and the features
//!   whose expressions use it; `:variable` alone lists every variable and every use). The
//!   prefix itself is case-insensitive; the `:variable` name is case-sensitive, as in Onshape,
//!   with or without its `#`.
//!
//! **Dependencies.** A feature's parents are the features it refers to ([`Feature::parents`])
//! and, for a sketch, the feature its plane or face comes from; its children are the features
//! that have it as a parent.

use crate::document::{Feature, FeatureKind};
use crate::ids::FeatureId;
use std::collections::HashMap;

use crate::parts::Part;

/// The name of a feature's type as the filter's `:type` matches it ("Extrude", "Fillet", …).
pub fn type_label(kind: &FeatureKind) -> &'static str {
    match kind {
        FeatureKind::Sketch(_) => "Sketch",
        FeatureKind::Extrude(_) => "Extrude",
        FeatureKind::Boolean(_) => "Boolean",
        FeatureKind::Revolve(_) => "Revolve",
        FeatureKind::DeletePart(_) => "Delete part",
        FeatureKind::Fillet(_) => "Fillet",
        FeatureKind::Chamfer(_) => "Chamfer",
        FeatureKind::Shell(_) => "Shell",
        FeatureKind::Hole(_) => "Hole",
        FeatureKind::Plane(_) => "Plane",
        FeatureKind::Sweep(_) => "Sweep",
        FeatureKind::Loft(_) => "Loft",
        FeatureKind::Split(_) => "Split",
        FeatureKind::MateConnector(_) => "Mate connector",
        FeatureKind::Pattern(p) => match p.kind {
            crate::pattern::PatternKind::Linear => "Linear pattern",
            crate::pattern::PatternKind::Circular => "Circular pattern",
            crate::pattern::PatternKind::Curve => "Curve pattern",
        },
        FeatureKind::Mirror(_) => "Mirror",
        FeatureKind::Draft(_) => "Draft",
        FeatureKind::Transform(_) => "Transform",
        FeatureKind::Composite(_) => "Composite part",
        FeatureKind::Import(_) => "Import",
        FeatureKind::Derived(_) => "Derived",
        FeatureKind::Thicken(_) => "Thicken",
        FeatureKind::Helix(_) => "Helix",
        FeatureKind::Fill(_) => "Fill",
        FeatureKind::Variable(_) => "Variable",
        FeatureKind::SheetMetalModel(_) => "Sheet metal model",
        FeatureKind::SheetMetalLoft(_) => "Sheet metal loft",
        FeatureKind::Form(_) => "Form",
        FeatureKind::TagForm(_) => "Tag",
    }
}

/// What a filter prefix compares its text with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterField {
    /// No prefix: the name or the type.
    NameOrType,
    Name,
    Type,
    Part,
    Folder,
    Errors,
    Variable,
}

/// A parsed filter query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    pub field: FilterField,
    /// The text to look for (lower case, except for `:variable`); empty matches everything the
    /// field allows (`:errors` alone: every feature with errors).
    pub text: String,
    /// The text was in quotes: the whole name must match.
    pub exact: bool,
}

/// What the filter knows about a feature.
#[derive(Debug, Clone, Default)]
pub struct FeatureFacts<'a> {
    pub name: &'a str,
    pub type_label: &'a str,
    /// The folder it is in.
    pub folder: Option<&'a str>,
    /// It failed to rebuild (or its sketch has errors).
    pub error: bool,
    /// The names of the parts it made or changed.
    pub parts: Vec<&'a str>,
    /// P3F.4: the variables it defines or whose names its expressions use (without `#`); see
    /// [`variable_facts`].
    pub variables: Vec<String>,
}

/// The variables a feature defines or uses, for [`FeatureFacts::variables`].
pub fn variable_facts(f: &Feature) -> Vec<String> {
    let mut out = crate::variables::uses(f);
    if let FeatureKind::Variable(v) = &f.kind
        && !out.contains(&v.name)
    {
        out.insert(0, v.name.clone());
    }
    out
}

/// The prefixes and what they do, as the filter field's hover help lists them (PS3.6).
pub const FILTER_HELP: &[(&str, &str)] = &[
    ("text", "Name or type contains it"),
    ("\"name\"", "Exact name"),
    (":part <name>", "Features that affect the part"),
    (":type <type>", "Features of that type"),
    (":name <name>", "Features with that name"),
    (":errors [name]", "Features with errors"),
    (":folder <name>", "Features in that folder"),
    (":variable <name>", "Variables and the features using them"),
    ("", "(case-sensitive)"),
];

impl Filter {
    /// Parses the filter field's text; `None` for an empty query (everything shows).
    pub fn parse(query: &str) -> Option<Filter> {
        let q = query.trim();
        if q.is_empty() {
            return None;
        }
        let (field, rest) = match q.strip_prefix(':') {
            Some(after) => {
                let (word, rest) = after.split_once(char::is_whitespace).unwrap_or((after, ""));
                let field = match word.to_lowercase().as_str() {
                    "name" => FilterField::Name,
                    "type" => FilterField::Type,
                    "part" => FilterField::Part,
                    "folder" => FilterField::Folder,
                    "errors" | "error" => FilterField::Errors,
                    "variable" => FilterField::Variable,
                    // An unknown prefix is plain text.
                    _ => return Some(Filter::text(FilterField::NameOrType, q)),
                };
                (field, rest.trim())
            }
            None => (FilterField::NameOrType, q),
        };
        Some(Filter::text(field, rest))
    }

    fn text(field: FilterField, text: &str) -> Filter {
        let (exact, inner) = match text.strip_prefix('"') {
            Some(t) => (true, t.strip_suffix('"').unwrap_or(t)),
            None => (false, text),
        };
        let text = if field == FilterField::Variable { inner.to_string() } else { inner.to_lowercase() };
        Filter { field, text, exact }
    }

    /// True if `s` matches the text (lower-cased, contains; or equal when quoted).
    fn hit(&self, s: &str) -> bool {
        let s = s.to_lowercase();
        if self.exact { s == self.text } else { s.contains(&self.text) }
    }

    /// True if the feature shows in the filtered list.
    pub fn matches(&self, f: &FeatureFacts) -> bool {
        match self.field {
            FilterField::NameOrType => self.hit(f.name) || (!self.exact && self.hit(f.type_label)),
            FilterField::Name => self.hit(f.name),
            FilterField::Type => self.hit(f.type_label),
            FilterField::Part => f.parts.iter().any(|p| self.hit(p)),
            FilterField::Folder => f.folder.is_some_and(|n| self.hit(n)),
            FilterField::Errors => f.error && (self.text.is_empty() || self.hit(f.name)),
            // P3F.4: the variable and the features using it.
            FilterField::Variable => {
                let want = self.text.trim_start_matches('#');
                f.variables.iter().any(|v| if self.exact || want.is_empty() { want.is_empty() || v == want } else { v.contains(want) })
            }
        }
    }
}

/// A feature's parents in `features` (PS11.2): what it refers to, and for a sketch the feature
/// its plane or face comes from. In list order.
pub fn parents(features: &[Feature], id: FeatureId) -> Vec<FeatureId> {
    let Some(f) = features.iter().find(|f| f.id == id) else { return Vec::new() };
    let mut out = f.parents();
    if let Some(s) = f.sketch()
        && let Some(plane) = s.plane
        && let Some(p) = plane_parent(&plane)
        && p != id
        && !out.contains(&p)
    {
        out.push(p);
    }
    out.retain(|p| features.iter().any(|g| g.id == *p));
    out.sort_by_key(|p| features.iter().position(|g| g.id == *p));
    out
}

/// The feature a sketch plane comes from (a face's feature, or a Plane feature).
fn plane_parent(plane: &cadrs_sketch::PlaneRef) -> Option<FeatureId> {
    match plane {
        cadrs_sketch::PlaneRef::Face(f) => Some(FeatureId(f.face.op)),
        cadrs_sketch::PlaneRef::Feature(f) => Some(FeatureId(f.feature)),
        _ => None,
    }
}

/// A feature's children in `features` (PS11.2): the features that have it as a parent. In list
/// order.
pub fn children(features: &[Feature], id: FeatureId) -> Vec<FeatureId> {
    features.iter().filter(|f| f.id != id && parents(features, f.id).contains(&id)).map(|f| f.id).collect()
}

/// [`parents`] with what the rebuild knows (P3.11, PS11.2): a feature that changed a part it
/// doesn't name (an Add or Remove whose merge scope it found by itself, a hole's default scope)
/// also has the feature that made that part as a parent. `parts` are the built parts, each with
/// every feature that made or changed it ([`Part::features`]); `uses` is
/// [`crate::rebuild::Build::uses`] (a fillet's tangent chain over another fillet's faces).
pub fn parents_with(features: &[Feature], parts: &[Part], uses: &HashMap<FeatureId, Vec<FeatureId>>, id: FeatureId) -> Vec<FeatureId> {
    let mut out = parents(features, id);
    for u in uses.get(&id).into_iter().flatten() {
        if *u != id && !out.contains(u) && features.iter().any(|g| g.id == *u) {
            out.push(*u);
        }
    }
    for p in parts {
        if p.feature != id && p.features.contains(&id) && !out.contains(&p.feature) && features.iter().any(|g| g.id == p.feature) {
            out.push(p.feature);
        }
    }
    out.sort_by_key(|p| features.iter().position(|g| g.id == *p));
    out
}

/// [`children`] with what the rebuild knows (see [`parents_with`]).
pub fn children_with(features: &[Feature], parts: &[Part], uses: &HashMap<FeatureId, Vec<FeatureId>>, id: FeatureId) -> Vec<FeatureId> {
    features.iter().filter(|f| f.id != id && parents_with(features, parts, uses, f.id).contains(&id)).map(|f| f.id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts<'a>(name: &'a str, ty: &'a str) -> FeatureFacts<'a> {
        FeatureFacts { name, type_label: ty, ..Default::default() }
    }

    #[test]
    fn plain_text_matches_name_or_type() {
        let f = Filter::parse("Extrude").unwrap();
        assert_eq!(f, Filter { field: FilterField::NameOrType, text: "extrude".into(), exact: false });
        assert!(f.matches(&facts("Extrude 1", "Extrude")));
        // Renamed, but still an extrude.
        assert!(f.matches(&facts("Base plate", "Extrude")));
        assert!(!f.matches(&facts("Fillet 1", "Fillet")));
        // Case doesn't matter, and the whole text is one term.
        let f = Filter::parse("  extrude 1 ").unwrap();
        assert!(f.matches(&facts("Extrude 1", "Extrude")));
        assert!(f.matches(&facts("Extrude 10", "Extrude")));
        assert!(!f.matches(&facts("Extrude 2", "Extrude")));
        assert_eq!(Filter::parse("   "), None);
    }

    #[test]
    fn quotes_match_the_exact_name() {
        let f = Filter::parse("\"Extrude 1\"").unwrap();
        assert!(f.exact);
        assert!(f.matches(&facts("Extrude 1", "Extrude")));
        assert!(f.matches(&facts("extrude 1", "Extrude")));
        assert!(!f.matches(&facts("Extrude 10", "Extrude")));
        // A quoted type is not a name.
        assert!(!Filter::parse("\"Extrude\"").unwrap().matches(&facts("Extrude 1", "Extrude")));
        // An unclosed quote still means exact.
        assert!(Filter::parse("\"Extrude 1").unwrap().matches(&facts("Extrude 1", "Extrude")));
    }

    #[test]
    fn prefixes() {
        let fillet = FeatureFacts { name: "Round edges", type_label: "Fillet", folder: Some("Base Features"), error: false, parts: vec!["Gear Cover"], variables: vec![] };
        let bad = FeatureFacts { name: "Hole 1", type_label: "Hole", folder: None, error: true, parts: vec![], variables: vec![] };
        let t = Filter::parse(":type Fillet").unwrap();
        assert_eq!(t.field, FilterField::Type);
        assert!(t.matches(&fillet));
        assert!(!t.matches(&facts("Fillet in the name", "Chamfer")));
        let n = Filter::parse(":NAME round").unwrap();
        assert!(n.matches(&fillet) && !n.matches(&facts("Fillet 1", "Fillet")));
        assert!(Filter::parse(":part gear").unwrap().matches(&fillet));
        assert!(!Filter::parse(":part gear").unwrap().matches(&bad));
        assert!(Filter::parse(":folder \"base features\"").unwrap().matches(&fillet));
        assert!(!Filter::parse(":folder base").unwrap().matches(&bad));
        let e = Filter::parse(":errors").unwrap();
        assert!(e.matches(&bad) && !e.matches(&fillet));
        assert!(Filter::parse(":errors hole").unwrap().matches(&bad));
        assert!(!Filter::parse(":errors fillet").unwrap().matches(&bad));
        // Variables are case-sensitive (P3F.4): the features defining or using one.
        let v = Filter::parse(":variable Depth").unwrap();
        assert_eq!((v.field, v.text.as_str()), (FilterField::Variable, "Depth"));
        assert!(!v.matches(&fillet));
        let uses = FeatureFacts { name: "Extrude 1", type_label: "Extrude", variables: vec!["Depth".into()], ..Default::default() };
        assert!(v.matches(&uses));
        assert!(Filter::parse(":variable #Depth").unwrap().matches(&uses));
        assert!(!Filter::parse(":variable depth").unwrap().matches(&uses));
        assert!(Filter::parse(":variable").unwrap().matches(&uses) && !Filter::parse(":variable").unwrap().matches(&fillet));
        assert!(!Filter::parse(":variable \"Dep\"").unwrap().matches(&uses));
        // An unknown prefix is plain text.
        assert_eq!(Filter::parse(":foo").unwrap().field, FilterField::NameOrType);
    }

    #[test]
    fn sketch_on_a_face_depends_on_its_feature() {
        use crate::document::{ExtrudeFeature, SketchFeature};
        let s1 = FeatureId::new();
        let e1 = FeatureId::new();
        let s2 = FeatureId::new();
        let e2 = FeatureId::new();
        let sketch = |id, plane| Feature { id, name: "S".into(), kind: FeatureKind::Sketch(SketchFeature::new(plane)) };
        let extrude = |id, s: FeatureId| {
            let mut x = ExtrudeFeature::default();
            x.sketches.push(s);
            Feature { id, name: "E".into(), kind: FeatureKind::Extrude(x) }
        };
        let face = cadrs_sketch::FacePlane {
            feature: e1.0,
            face: cadrs_sketch::FaceName::new(e1.0, cadrs_sketch::FaceOrigin::Cap { region: 1, end: true }),
            origin: [0.0, 0.0, 10.0],
            u: [1.0, 0.0, 0.0],
            v: [0.0, 1.0, 0.0],
            seed: None,
        };
        let features = vec![
            sketch(s1, Some(cadrs_sketch::PlaneRef::Top)),
            extrude(e1, s1),
            sketch(s2, Some(cadrs_sketch::PlaneRef::Face(face))),
            extrude(e2, s2),
        ];
        assert_eq!(parents(&features, e1), vec![s1]);
        assert_eq!(parents(&features, s2), vec![e1]);
        assert_eq!(children(&features, e1), vec![s2]);
        assert_eq!(children(&features, s1), vec![e1]);
        assert_eq!(children(&features, e2), Vec::<FeatureId>::new());
        assert_eq!(parents(&features, s1), Vec::<FeatureId>::new());
    }
}
