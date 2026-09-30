//! Commands of P3.6: the applied features (Fillet, Chamfer, Shell, Hole), reordering several
//! features at once, and the feature list's folders.

use super::*;
use crate::document::FeatureFolder;

/// Replaces a feature's parameters with `kind` (of the same kind), as the applied features'
/// dialogs do on every change. A Hole's name follows its callout until it is renamed (PS15.10).
#[derive(Debug, Clone)]
pub struct SetFeature {
    pub element: ElementId,
    pub feature: FeatureId,
    pub kind: FeatureKind,
    pub label: String,
}

impl Command for SetFeature {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let f = part_studio_features(doc, self.element)?
            .iter_mut()
            .find(|f| f.id == self.feature)
            .ok_or_else(|| CommandError::Invalid("feature not found".into()))?;
        if std::mem::discriminant(&f.kind) != std::mem::discriminant(&self.kind) {
            return Err(CommandError::Invalid("a feature can't change its type".into()));
        }
        f.kind = self.kind.clone();
        if let FeatureKind::Hole(h) = &f.kind
            && !h.renamed
        {
            f.name = h.spec.callout();
        }
        // P3F.4: a Variable listed by its name follows a rename of the variable.
        if let FeatureKind::Variable(v) = &f.kind
            && (f.name.starts_with('#') || f.name.starts_with("Variable "))
            && !v.name.is_empty()
        {
            f.name = format!("#{}", v.name);
        }
        refresh(doc, self.element);
        Ok(())
    }
}

impl AddFeature {
    /// A Variable (P3F.4), listed as "#name".
    pub fn variable(element: ElementId, feature: FeatureId, x: crate::variables::VariableFeature) -> Self {
        Self { element, feature, base_name: "Variable".into(), kind: FeatureKind::Variable(x) }
    }

    /// A Fillet ("Fillet N").
    pub fn fillet(element: ElementId, feature: FeatureId, x: crate::applied::FilletFeature) -> Self {
        Self { element, feature, base_name: "Fillet".into(), kind: FeatureKind::Fillet(x) }
    }

    /// A Chamfer ("Chamfer N").
    pub fn chamfer(element: ElementId, feature: FeatureId, x: crate::applied::ChamferFeature) -> Self {
        Self { element, feature, base_name: "Chamfer".into(), kind: FeatureKind::Chamfer(x) }
    }

    /// A Shell ("Shell N").
    pub fn shell(element: ElementId, feature: FeatureId, x: crate::applied::ShellFeature) -> Self {
        Self { element, feature, base_name: "Shell".into(), kind: FeatureKind::Shell(x) }
    }

    /// A Hole, named by its callout.
    pub fn hole(element: ElementId, feature: FeatureId, x: crate::applied::HoleFeature) -> Self {
        Self { element, feature, base_name: "Hole".into(), kind: FeatureKind::Hole(x) }
    }

    /// A Draft ("Draft N", P3.10).
    pub fn draft(element: ElementId, feature: FeatureId, x: crate::draft::DraftFeature) -> Self {
        Self { element, feature, base_name: "Draft".into(), kind: FeatureKind::Draft(x) }
    }

    /// A Transform ("Transform N").
    pub fn transform(element: ElementId, feature: FeatureId, x: crate::transform::TransformFeature) -> Self {
        Self { element, feature, base_name: "Transform".into(), kind: FeatureKind::Transform(x) }
    }
}

/// Moves features (a dragged row, or a whole folder) so the first of them lands at `to` (an
/// index in the list without them), keeping their order, and puts them in `folder` (or in no
/// folder) (P3.6, PS11.3). A folder's features stay together: a feature dropped between two of
/// a folder's features joins it.
#[derive(Debug, Clone)]
pub struct MoveFeatures {
    pub element: ElementId,
    pub features: Vec<FeatureId>,
    pub to: usize,
    pub folder: Option<FeatureId>,
    pub label: String,
}

impl Command for MoveFeatures {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let el = doc.element_mut(self.element).ok_or(CommandError::ElementNotFound(self.element))?;
        let features = el
            .features_mut()
            .ok_or_else(|| CommandError::Invalid("features need a Part Studio".into()))?;
        let mut moved: Vec<Feature> = Vec::new();
        for id in &self.features {
            let i = features
                .iter()
                .position(|f| f.id == *id)
                .ok_or_else(|| CommandError::Invalid("feature not found".into()))?;
            moved.push(features.remove(i));
        }
        if self.to > features.len() {
            return Err(CommandError::Invalid("no such place in the feature list".into()));
        }
        for (k, f) in moved.into_iter().enumerate() {
            features.insert(self.to + k, f);
        }
        let order: Vec<FeatureId> = features.iter().map(|f| f.id).collect();
        let moved_folders = self.folder;
        if let Some(folders) = el.folders_mut() {
            // A folder being moved keeps its features; otherwise they leave their folders.
            let whole: Vec<FeatureId> = folders
                .iter()
                .filter(|f| !f.features.is_empty() && f.features.iter().all(|x| self.features.contains(x)))
                .map(|f| f.id)
                .collect();
            for f in folders.iter_mut() {
                if !whole.contains(&f.id) {
                    f.features.retain(|x| !self.features.contains(x));
                }
            }
            if let Some(target) = moved_folders
                && let Some(f) = folders.iter_mut().find(|f| f.id == target)
            {
                for x in &self.features {
                    if !f.features.contains(x) {
                        f.features.push(*x);
                    }
                }
            }
            normalize_folders(folders, &order);
        }
        refresh(doc, self.element);
        Ok(())
    }
}

/// Keeps each folder's features in list order and contiguous: a feature between two of a
/// folder's features joins it.
pub fn normalize_folders(folders: &mut [FeatureFolder], order: &[FeatureId]) {
    for f in folders.iter_mut() {
        f.features.retain(|x| order.contains(x));
        f.features.sort_by_key(|x| order.iter().position(|o| o == x));
    }
    for i in 0..folders.len() {
        let (Some(first), Some(last)) = (folders[i].features.first(), folders[i].features.last()) else {
            continue;
        };
        let a = order.iter().position(|o| o == first).unwrap_or(0);
        let b = order.iter().position(|o| o == last).unwrap_or(0);
        let span: Vec<FeatureId> = order[a..=b].to_vec();
        for (j, other) in folders.iter_mut().enumerate() {
            if j != i {
                other.features.retain(|x| !span.contains(x));
            }
        }
        folders[i].features = span;
    }
}

/// Puts features into a new folder ("Folder N", or `name`) at the first of them (P3.6).
#[derive(Debug, Clone)]
pub struct CreateFolder {
    pub element: ElementId,
    pub folder: FeatureId,
    pub name: Option<String>,
    pub features: Vec<FeatureId>,
}

impl Command for CreateFolder {
    fn label(&self) -> String {
        "Create folder".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let el = doc.element_mut(self.element).ok_or(CommandError::ElementNotFound(self.element))?;
        let order: Vec<FeatureId> = el.features().iter().map(|f| f.id).collect();
        let name = match &self.name {
            Some(n) => non_empty(n)?,
            None => {
                let mut n = 1;
                loop {
                    let name = format!("Folder {n}");
                    if !el.folders().iter().any(|f| f.name == name) {
                        break name;
                    }
                    n += 1;
                }
            }
        };
        let folders = el
            .folders_mut()
            .ok_or_else(|| CommandError::Invalid("folders need a Part Studio".into()))?;
        if folders.iter().any(|f| f.id == self.folder) {
            return Err(CommandError::Invalid("folder id already in use".into()));
        }
        for f in folders.iter_mut() {
            f.features.retain(|x| !self.features.contains(x));
        }
        folders.push(FeatureFolder {
            id: self.folder,
            name,
            features: self.features.clone(),
            open: false,
        });
        normalize_folders(folders, &order);
        Ok(())
    }
}

/// Unpacks a folder: its features stay where they are, the folder goes.
#[derive(Debug, Clone)]
pub struct UnpackFolder {
    pub element: ElementId,
    pub folder: FeatureId,
}

impl Command for UnpackFolder {
    fn label(&self) -> String {
        "Unpack folder".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let folders = doc
            .element_mut(self.element)
            .ok_or(CommandError::ElementNotFound(self.element))?
            .folders_mut()
            .ok_or_else(|| CommandError::Invalid("folders need a Part Studio".into()))?;
        let n = folders.len();
        folders.retain(|f| f.id != self.folder);
        if folders.len() == n {
            return Err(CommandError::Invalid("folder not found".into()));
        }
        Ok(())
    }
}

/// Opens or closes a folder, or renames it.
#[derive(Debug, Clone)]
pub struct SetFolder {
    pub element: ElementId,
    pub folder: FeatureId,
    pub open: Option<bool>,
    pub name: Option<String>,
}

impl Command for SetFolder {
    fn label(&self) -> String {
        match (&self.name, self.open) {
            (Some(n), _) => format!("Rename folder to {}", n.trim()),
            (None, Some(true)) => "Open folder".into(),
            _ => "Close folder".into(),
        }
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let name = self.name.as_deref().map(non_empty).transpose()?;
        let f = doc
            .element_mut(self.element)
            .ok_or(CommandError::ElementNotFound(self.element))?
            .folders_mut()
            .and_then(|v| v.iter_mut().find(|f| f.id == self.folder))
            .ok_or_else(|| CommandError::Invalid("folder not found".into()))?;
        if let Some(o) = self.open {
            f.open = o;
        }
        if let Some(n) = name {
            f.name = n;
        }
        Ok(())
    }
}
