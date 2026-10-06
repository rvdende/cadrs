//! Commands of P3.9: feature-list management (PS3, PS13): suppressing features, moving the
//! rollback bar and deleting a folder with its features.

use super::*;

/// Suppresses or unsuppresses features: a suppressed feature stays in the list (greyed, struck
/// through) but is left out of the rebuild ([`Element::active_features`]).
#[derive(Debug, Clone)]
pub struct SetSuppressed {
    pub element: ElementId,
    pub features: Vec<FeatureId>,
    pub suppressed: bool,
    /// Shown in the undo menu ("Suppress Fillet 1").
    pub label: String,
}

impl Command for SetSuppressed {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let el = doc.element_mut(self.element).ok_or(CommandError::ElementNotFound(self.element))?;
        if self.features.iter().any(|f| el.feature(*f).is_none()) {
            return Err(CommandError::Invalid("feature not found".into()));
        }
        let order: Vec<FeatureId> = el.features().iter().map(|f| f.id).collect();
        let ElementKind::PartStudio { suppressed, .. } = &mut el.kind else {
            return Err(CommandError::Invalid("features need a Part Studio".into()));
        };
        for f in &self.features {
            let has = suppressed.contains(f);
            if self.suppressed && !has {
                suppressed.push(*f);
            } else if !self.suppressed && has {
                suppressed.retain(|x| x != f);
            }
        }
        // In list order, so saved files don't depend on the order of the clicks.
        suppressed.sort_by_key(|x| order.iter().position(|o| o == x));
        Ok(())
    }
}

/// Sets or removes a feature's suppression variable (IR5.5, Dynamic suppression ▸ Suppress by
/// variable…): with `#withHole`, the feature is suppressed while `#withHole` is 0. One undo step
/// each way; the variables are evaluated again (a Variable it suppresses defines nothing below).
#[derive(Debug, Clone)]
pub struct SetSuppressByVariable {
    pub element: ElementId,
    pub feature: FeatureId,
    /// `None` removes it.
    pub rule: Option<crate::variables::SuppressByVariable>,
}

impl Command for SetSuppressByVariable {
    fn label(&self) -> String {
        match &self.rule {
            Some(r) => format!("Suppress by {}", r.label()),
            None => "Remove suppression variable".into(),
        }
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        if let Some(r) = &self.rule
            && cadrs_sketch::units::variable_names(&r.expr).is_empty()
        {
            return Err(CommandError::Invalid("pick a variable".into()));
        }
        let f = part_studio_features(doc, self.element)?
            .iter_mut()
            .find(|f| f.id == self.feature)
            .ok_or_else(|| CommandError::Invalid("feature not found".into()))?;
        f.suppress_by = self.rule.clone();
        refresh(doc, self.element);
        Ok(())
    }
}

/// Moves the rollback bar (PS13.1): `index` features stay above it (`None`, or the length of
/// the list: at the end).
#[derive(Debug, Clone)]
pub struct SetRollback {
    pub element: ElementId,
    pub index: Option<usize>,
}

impl Command for SetRollback {
    fn label(&self) -> String {
        match self.index {
            None => "Roll to end".into(),
            Some(_) => "Move rollback bar".into(),
        }
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let el = doc.element_mut(self.element).ok_or(CommandError::ElementNotFound(self.element))?;
        let n = el.features().len();
        let ElementKind::PartStudio { rollback, .. } = &mut el.kind else {
            return Err(CommandError::Invalid("features need a Part Studio".into()));
        };
        *rollback = match self.index {
            Some(i) if i > n => return Err(CommandError::Invalid("no such place in the feature list".into())),
            Some(i) if i == n => None,
            other => other,
        };
        Ok(())
    }
}

/// Deletes a folder and the features in it (PS3.3).
#[derive(Debug, Clone)]
pub struct DeleteFolder {
    pub element: ElementId,
    pub folder: FeatureId,
}

impl Command for DeleteFolder {
    fn label(&self) -> String {
        "Delete folder".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let el = doc.element_mut(self.element).ok_or(CommandError::ElementNotFound(self.element))?;
        let inside = el
            .folders()
            .iter()
            .find(|f| f.id == self.folder)
            .map(|f| f.features.clone())
            .ok_or_else(|| CommandError::Invalid("folder not found".into()))?;
        let removed: Vec<usize> = el
            .features()
            .iter()
            .enumerate()
            .filter(|(_, f)| inside.contains(&f.id))
            .map(|(i, _)| i)
            .collect();
        if let Some(features) = el.features_mut() {
            features.retain(|f| !inside.contains(&f.id));
        }
        if let Some(folders) = el.folders_mut() {
            folders.retain(|f| f.id != self.folder);
        }
        forget_removed(doc, self.element, &removed);
        refresh(doc, self.element);
        Ok(())
    }
}

/// After features at `removed` (their indices before the removal) left the list: the rollback
/// bar keeps its place among the others, and the suppressed list forgets them.
pub(super) fn forget_removed(doc: &mut Document, element: ElementId, removed: &[usize]) {
    let Some(el) = doc.element_mut(element) else { return };
    let ids: Vec<FeatureId> = el.features().iter().map(|f| f.id).collect();
    let n = ids.len();
    let ElementKind::PartStudio { rollback, suppressed, .. } = &mut el.kind else { return };
    suppressed.retain(|f| ids.contains(f));
    if let Some(r) = rollback {
        let above = removed.iter().filter(|i| **i < *r).count();
        *r -= above;
        if *r >= n {
            *rollback = None;
        }
    }
}
