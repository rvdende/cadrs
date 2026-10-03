//! Onshape's sheet metal **Form** and **Tag (Form)** features (P3I.9, SM20;
//! `reference/onshape/sheetmetal/raw/help-sheet_metal_form.txt`, dialogs
//! `help/feature-tools/formed-03-02.png`, `forms-selectPS-dialog-01.png`, `form-06a.png`).
//!
//! - **Tag (Form)** ([`TagFormFeature`], SM20.2) is authored in the form's own Part Studio: the
//!   **Part to add** (material the form adds), the **Part to remove** (material it cuts away), an
//!   optional **Sketch for flat view** (construction-only: the outline the flat pattern shows) and
//!   the **Form origin mate connector**. The form's origin lies on the sheet's face, its Z pointing
//!   out of the sheet; the sheet is below it (`z` from `−thickness` to 0). A Part Studio that is a
//!   form has a Length variable named `thickness`, which the Form feature drives with the sheet
//!   metal model's thickness (Onshape's configuration variable of that name).
//! - **Form** ([`FormFeature`], SM20.1) places it: **Form Part Studio** (picked in the *Select Part
//!   Studio* dialog from the **Current document**, **Other documents** or the **Libraries**: cadrs
//!   ships its own sheet metal forms library, [`crate::samples::sheetmetal_forms`]), the form's
//!   variables (Length, Width, Height, …), **Location(s)** (sketch points, a whole sketch's points,
//!   vertices or mate connectors), **Target face(s)** of an active sheet metal model and the
//!   **opposite direction** arrow. Each location gets a copy: the add part united with the
//!   folded part, then the remove part cut from it.
//! - Rules (SM20.3): forms can't touch side walls, rolled walls, rips, joints or corners
//!   ([`cadrs_sheetmetal::forms::check_footprint`]); each placed form goes into its flat-pattern
//!   part's `forms` (outline and centermark) for the flat view, DXF and drawings.
//!
//! cadrs has variables but no configurations: a form's "configuration variables" are its Part
//! Studio's Variable features, set by the Form feature for its copy of the studio.

use serde::{Deserialize, Serialize};

use crate::document::{FaceRef, Feature, FeatureKind};
use crate::ids::{DocumentId, ElementId, FeatureId, PartId};
use crate::mate::ConnectorRef;

/// The Tag feature's types (Onshape's Tag has a type select; cadrs has Form only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TagType {
    #[default]
    Form,
}

/// A Tag (Form) feature.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct TagFormFeature {
    #[serde(default)]
    pub tag: TagType,
    /// Part to add.
    #[serde(default)]
    pub add: Vec<PartId>,
    /// Part to remove.
    #[serde(default)]
    pub remove: Vec<PartId>,
    /// Sketch for flat view (its construction curves).
    #[serde(default)]
    pub sketch: Option<FeatureId>,
    /// Form origin mate connector.
    #[serde(default)]
    pub origin: Option<ConnectorRef>,
}

impl TagFormFeature {
    pub fn problem(&self) -> Option<&'static str> {
        if self.add.is_empty() && self.remove.is_empty() {
            return Some("Select a part to add or a part to remove");
        }
        if self.origin.is_none() {
            return Some("Select the form origin mate connector");
        }
        None
    }

    pub fn parents(&self) -> Vec<FeatureId> {
        let mut v: Vec<FeatureId> = self.add.iter().chain(&self.remove).map(|p| p.feature).collect();
        v.extend(self.sketch);
        v.extend(self.origin.and_then(|c| c.parent()));
        v.sort();
        v.dedup();
        v
    }
}

/// The forms of cadrs's own sheet metal forms library (never Onshape's).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LibraryForm {
    Louver,
    /// A bridge lance.
    Lance,
    Dimple,
    Emboss,
    ExtrudedHole,
}

impl LibraryForm {
    pub const ALL: [LibraryForm; 5] = [LibraryForm::Louver, LibraryForm::Lance, LibraryForm::Dimple, LibraryForm::Emboss, LibraryForm::ExtrudedHole];

    pub fn label(self) -> &'static str {
        match self {
            LibraryForm::Louver => "Louver",
            LibraryForm::Lance => "Bridge lance",
            LibraryForm::Dimple => "Dimple",
            LibraryForm::Emboss => "Emboss",
            LibraryForm::ExtrudedHole => "Extruded hole",
        }
    }

    /// Its Type in the library (Onshape's second level).
    pub fn kind(self) -> &'static str {
        match self {
            LibraryForm::Louver | LibraryForm::Lance => "Cut forms",
            LibraryForm::Dimple | LibraryForm::Emboss => "Raised forms",
            LibraryForm::ExtrudedHole => "Holes",
        }
    }

    /// Its variables and their defaults (mm, degrees): what the picker shows below the form.
    pub fn variables(self) -> Vec<FormVariable> {
        let l = |n: &str, v: f64| FormVariable::length(n, v);
        match self {
            LibraryForm::Louver => vec![l("Length", 40.0), l("Width", 8.0), l("Height", 4.0)],
            LibraryForm::Lance => vec![l("Length", 30.0), l("Width", 6.0), l("Height", 3.0)],
            LibraryForm::Dimple => vec![l("Diameter", 16.0), l("Height", 3.0)],
            LibraryForm::Emboss => vec![l("Length", 30.0), l("Width", 20.0), l("Height", 2.5)],
            LibraryForm::ExtrudedHole => vec![l("Diameter", 8.0), l("Height", 4.0)],
        }
    }
}

/// The name of cadrs's forms library.
pub const LIBRARY_NAME: &str = "cadrs sheet metal forms";

/// A form variable as the Form feature sets it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormVariable {
    /// As the form names it ("Length"; a Variable feature's name).
    pub name: String,
    pub expr: String,
    pub value: f64,
    /// A length (mm) or an angle (degrees).
    #[serde(default)]
    pub angle: bool,
}

impl FormVariable {
    pub fn length(name: &str, mm: f64) -> Self {
        FormVariable { name: name.into(), expr: format!("{} mm", crate::sheetmetal::plain(mm)), value: mm, angle: false }
    }
}

/// Where the form comes from (the Select Part Studio dialog's tabs).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FormSource {
    /// A Part Studio of this document (its features copied in when picked).
    Current { element: ElementId },
    /// A Part Studio of another document.
    Other { document: DocumentId, element: ElementId },
    /// cadrs's forms library.
    Library(LibraryForm),
}

/// The picked form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormPick {
    pub source: FormSource,
    /// The Part Studio's (form's) name, and its document's.
    pub name: String,
    #[serde(default)]
    pub document_name: String,
    /// A document's form: its Part Studio's features (a library form's are made from its
    /// variables at each rebuild).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub studio: Vec<Feature>,
}

/// One Location(s) entry.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum FormLocation {
    /// A sketch point, a vertex or a mate connector (explicit or implicit).
    Connector(ConnectorRef),
    /// Every point of a sketch ("Vertices of Sketch 2").
    SketchPoints(FeatureId),
}

impl FormLocation {
    pub fn parent(&self) -> Option<FeatureId> {
        match self {
            FormLocation::Connector(c) => c.parent(),
            FormLocation::SketchPoints(s) => Some(*s),
        }
    }
}

/// A Form feature.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct FormFeature {
    #[serde(default)]
    pub form: Option<FormPick>,
    /// The form's variables as set here.
    #[serde(default)]
    pub variables: Vec<FormVariable>,
    #[serde(default)]
    pub locations: Vec<FormLocation>,
    #[serde(default)]
    pub targets: Vec<FaceRef>,
    /// The opposite direction arrow: the form on the other side of the sheet.
    #[serde(default)]
    pub flip: bool,
}

impl FormFeature {
    pub fn problem(&self) -> Option<&'static str> {
        if self.form.is_none() {
            return Some("Select a form Part Studio");
        }
        if self.locations.is_empty() {
            return Some("Select the form locations");
        }
        if self.targets.is_empty() {
            return Some("Select the target faces");
        }
        if self.variables.iter().any(|v| v.value.is_nan() || (!v.angle && v.value <= 0.0)) {
            return Some("A form variable is out of range");
        }
        None
    }

    pub fn is_empty(&self) -> bool {
        self.form.is_none() && self.locations.is_empty() && self.targets.is_empty()
    }

    pub fn parents(&self) -> Vec<FeatureId> {
        let mut v: Vec<FeatureId> = self.locations.iter().filter_map(FormLocation::parent).collect();
        v.extend(self.targets.iter().map(|f| FeatureId(f.face.op)));
        v.sort();
        v.dedup();
        v
    }

    /// The sketches its locations come from (their points stay shown).
    pub fn sketch_ids(&self) -> Vec<FeatureId> {
        self.locations
            .iter()
            .filter_map(|l| match l {
                FormLocation::SketchPoints(s) => Some(*s),
                FormLocation::Connector(ConnectorRef::Implicit(crate::mate::ConnectorOrigin::SketchPoint { sketch, .. })) => Some(*sketch),
                _ => None,
            })
            .collect()
    }
}

/// The variables of a document form: its Part Studio's Variable features, less `thickness`
/// (driven by the sheet metal model).
pub fn studio_variables(studio: &[Feature]) -> Vec<FormVariable> {
    studio
        .iter()
        .filter_map(|f| match &f.kind {
            FeatureKind::Variable(v) if !v.name.eq_ignore_ascii_case("thickness") => Some(FormVariable {
                name: v.name.clone(),
                expr: v.expr.clone(),
                value: v.value,
                angle: v.kind == crate::variables::VariableKind::Angle,
            }),
            _ => None,
        })
        .collect()
}

/// A Part Studio's form tag, if it has one (its first Tag (Form) feature).
pub fn tag_of(studio: &[Feature]) -> Option<&TagFormFeature> {
    studio.iter().find_map(|f| match &f.kind {
        FeatureKind::TagForm(t) => Some(t),
        _ => None,
    })
}

/// The studio a form's copy is built from: a library form made from its variables, or the
/// document form's features with its variables (and `thickness`) set and the expressions that
/// use them refreshed.
pub fn form_studio(pick: &FormPick, variables: &[FormVariable], thickness: f64) -> Result<Vec<Feature>, String> {
    match &pick.source {
        FormSource::Library(form) => crate::samples::sheetmetal_forms::studio(*form, variables, thickness).map_err(|e| e.to_string()),
        _ => {
            let mut fs = pick.studio.clone();
            if tag_of(&fs).is_none() {
                return Err(format!("{} has no Tag (Form) feature", pick.name));
            }
            let mm = crate::sheetmetal::plain(thickness);
            for f in &mut fs {
                if let FeatureKind::Variable(v) = &mut f.kind {
                    if v.name.eq_ignore_ascii_case("thickness") {
                        v.expr = format!("{mm} mm");
                    } else if let Some(x) = variables.iter().find(|x| x.name == v.name) {
                        v.expr = x.expr.clone();
                    }
                }
            }
            crate::variables::refresh(&mut fs, &[], &cadrs_sketch::units::Units::default());
            Ok(fs)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn problems_in_dialog_order() {
        let mut f = FormFeature::default();
        assert_eq!(f.problem(), Some("Select a form Part Studio"));
        f.form = Some(FormPick { source: FormSource::Library(LibraryForm::Louver), name: "Louver".into(), document_name: LIBRARY_NAME.into(), studio: vec![] });
        f.variables = LibraryForm::Louver.variables();
        assert_eq!(f.problem(), Some("Select the form locations"));
        let t = TagFormFeature::default();
        assert_eq!(t.problem(), Some("Select a part to add or a part to remove"));
        let s = ron::to_string(&f).unwrap();
        assert_eq!(ron::from_str::<FormFeature>(&s).unwrap(), f);
    }
}
