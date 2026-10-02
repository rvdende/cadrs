//! Assembly commands (P3B.1). Each edits one Assembly tab ([`Scope::Element`]), so it is one
//! undo step: Undo removes inserted instances one by one, as the Insert dialog's "Undo to
//! remove instances" does.

use crate::command::{Command, CommandError, Scope};
use crate::document::Document;
use crate::ids::ElementId;

use super::mate::{MateFeature, MateId, MateKind};
use super::{Assembly, Instance, InstanceId, Pose};

pub(crate) fn assembly_mut(doc: &mut Document, element: ElementId) -> Result<&mut Assembly, CommandError> {
    doc.element_mut(element)
        .ok_or(CommandError::ElementNotFound(element))?
        .assembly_model_mut()
        .ok_or_else(|| CommandError::Invalid("not an assembly".into()))
}

fn check_ids(asm: &Assembly, ids: &[InstanceId]) -> Result<(), CommandError> {
    if ids.is_empty() {
        return Err(CommandError::Invalid("no instances".into()));
    }
    match ids.iter().find(|i| asm.instance(**i).is_none()) {
        Some(i) => Err(CommandError::Invalid(format!("instance {i} not found"))),
        None => Ok(()),
    }
}

/// Inserts an instance (Insert dialog, A2.3). It gets the next number of its source
/// (`<n>`); its source must be a part of a Part Studio of the document.
#[derive(Debug, Clone)]
pub struct InsertInstance {
    pub element: ElementId,
    pub instance: Instance,
}

impl Command for InsertInstance {
    fn label(&self) -> String {
        "Insert instance".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let src = self.instance.source.element();
        match self.instance.source {
            super::InstanceSource::Part { .. } => {
                if !doc.element(src).is_some_and(|e| matches!(e.kind, crate::document::ElementKind::PartStudio { .. })) {
                    return Err(CommandError::Invalid("the source is not a Part Studio of this document".into()));
                }
            }
            // A17.2: another Assembly tab, which must not be (or contain) this one.
            // A2.4: a whole Part Studio as one rigid instance.
            super::InstanceSource::Studio { .. } => {
                if !doc.element(src).is_some_and(|e| matches!(e.kind, crate::document::ElementKind::PartStudio { .. })) {
                    return Err(CommandError::Invalid("the source is not a Part Studio of this document".into()));
                }
                if self.instance.parts.is_empty() {
                    return Err(CommandError::Invalid("a rigid Part Studio instance needs a part".into()));
                }
            }
            super::InstanceSource::Assembly { .. } => {
                if doc.element(src).and_then(|e| e.assembly_model()).is_none() {
                    return Err(CommandError::Invalid("the source is not an Assembly of this document".into()));
                }
                if super::structure::contains_assembly(doc, src, self.element) {
                    return Err(CommandError::Invalid("an assembly can't contain itself".into()));
                }
            }
        }
        let doc_id = doc.id;
        let asm = assembly_mut(doc, self.element)?;
        if asm.instance(self.instance.id).is_some() {
            return Err(CommandError::Invalid("instance id already in use".into()));
        }
        let mut inst = self.instance.clone();
        inst.index = asm.next_index_of(doc_id, &inst);
        asm.instances.push(inst);
        Ok(())
    }
}

/// Deletes instances (the instance menu's Delete).
#[derive(Debug, Clone)]
pub struct DeleteInstances {
    pub element: ElementId,
    pub instances: Vec<InstanceId>,
}

impl Command for DeleteInstances {
    fn label(&self) -> String {
        "Delete instances".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        // The parts of deleted subassemblies are gone too.
        let mut gone = self.instances.clone();
        if let Some(asm) = doc.element(self.element).and_then(|e| e.assembly_model()) {
            gone.extend(super::structure::occurrences(doc, asm).into_iter().filter(|o| self.instances.contains(&o.top)).map(|o| o.id));
        }
        let asm = assembly_mut(doc, self.element)?;
        check_ids(asm, &self.instances)?;
        // P3B.8: a Replicate whose seed (or the seed mate's other instance) goes takes its copies
        // with it; a copy deleted leaves it.
        let mut with: Vec<InstanceId> = Vec::new();
        for f in &asm.mates {
            let MateKind::Replicate(r) = &f.kind else { continue };
            let seed_mate_gone = asm.mate(r.seed_mate).is_none_or(|m| m.instances().iter().any(|i| gone.contains(i)));
            if gone.contains(&r.seed) || seed_mate_gone {
                with.extend(r.instances.iter().copied());
            }
        }
        gone.extend(with.iter().copied());
        let gone_top: Vec<InstanceId> = self.instances.iter().chain(with.iter()).copied().collect();
        asm.instances.retain(|i| !gone_top.contains(&i.id));
        for v in &mut asm.exploded_views {
            for st in &mut v.steps {
                st.instances.retain(|i| !gone_top.contains(i));
            }
            v.steps.retain(|st| !st.instances.is_empty());
        }
        // Their mates go too; a group loses them (and goes when fewer than two are left).
        let gone = &gone;
        // P3B.7: their explicit connectors go, and the mates on them.
        let lost: Vec<super::connector::LocalConnectorId> =
            asm.connectors.iter().filter(|c| gone.contains(&c.connector.instance)).map(|c| c.id).collect();
        asm.connectors.retain(|c| !lost.contains(&c.id));
        let on_lost = |c: &super::connector::MateConnector| matches!(c.anchor, super::connector::ConnectorAnchor::Local { id } if lost.contains(&id));
        asm.mates.retain_mut(|f| match &mut f.kind {
            MateKind::Mate(m) => !m.connectors.iter().any(|c| gone.contains(&c.instance) || on_lost(c)),
            MateKind::Group { instances } => {
                instances.retain(|i| !gone.contains(i));
                instances.len() >= 2
            }
            MateKind::Relation(_) | MateKind::Variable(_) => true,
            MateKind::Replicate(r) => {
                if gone.contains(&r.seed) {
                    return false;
                }
                let keep: Vec<bool> = r.instances.iter().map(|i| !gone.contains(i)).collect();
                let mut k = keep.iter();
                r.targets.retain(|_| *k.next().unwrap_or(&true));
                let mut k = keep.iter();
                r.instances.retain(|_| *k.next().unwrap_or(&true));
                !r.instances.is_empty()
            }
        });
        // A Replicate whose seed mate went goes too.
        let mates: Vec<MateId> = asm.mates.iter().map(|m| m.id).collect();
        asm.retain_mates(|f| !matches!(&f.kind, MateKind::Replicate(r) if !mates.contains(&r.seed_mate)));
        // P3B.9: a relation goes with its mates.
        asm.drop_orphan_relations();
        super::folders::tidy(asm);
        Ok(())
    }
}

/// Places instances (triad drags, Move to origin, Align / Anti-align with Z, rotate 90° / 180°,
/// the Insert dialog's click placement).
#[derive(Debug, Clone)]
pub struct MoveInstances {
    pub element: ElementId,
    pub poses: Vec<(InstanceId, Pose)>,
    /// "Move to origin", "Anti-align with Z", …
    pub label: String,
}

impl Command for MoveInstances {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        if self.poses.is_empty() {
            return Err(CommandError::Invalid("no instances".into()));
        }
        // Top-level instances, or parts of subassemblies (the solver's placements).
        super::structure::place(doc, self.element, &self.poses)
    }
}

/// Hides or shows instances (the eye, Hide, Hide other / all instances, Show all, Y, Shift+Y).
#[derive(Debug, Clone)]
pub struct SetInstancesHidden {
    pub element: ElementId,
    pub instances: Vec<InstanceId>,
    pub hidden: bool,
}

impl Command for SetInstancesHidden {
    fn label(&self) -> String {
        if self.hidden { "Hide instances".into() } else { "Show instances".into() }
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let asm = assembly_mut(doc, self.element)?;
        check_ids(asm, &self.instances)?;
        for i in &mut asm.instances {
            if self.instances.contains(&i.id) {
                i.hidden = self.hidden;
            }
        }
        Ok(())
    }
}

/// Fixes or unfixes instances (A3.7).
#[derive(Debug, Clone)]
pub struct SetInstancesFixed {
    pub element: ElementId,
    pub instances: Vec<InstanceId>,
    pub fixed: bool,
}

impl Command for SetInstancesFixed {
    fn label(&self) -> String {
        if self.fixed { "Fix".into() } else { "Unfix".into() }
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let asm = assembly_mut(doc, self.element)?;
        check_ids(asm, &self.instances)?;
        for i in &mut asm.instances {
            if self.instances.contains(&i.id) {
                i.fixed = self.fixed;
            }
        }
        Ok(())
    }
}

fn check_feature(doc: &Document, element: ElementId, f: &MateFeature) -> Result<(), CommandError> {
    let asm = doc.element(element).and_then(|e| e.assembly_model()).ok_or_else(|| CommandError::Invalid("not an assembly".into()))?;
    // A mate names occurrences (parts inside subassemblies too); a group, top-level instances.
    match &f.kind {
        MateKind::Mate(m) => {
            let occ: std::collections::HashSet<InstanceId> = super::structure::occurrences(doc, asm).into_iter().map(|o| o.id).collect();
            // P3B.7: the Origin (A16.3) is a mate's instance too.
            if let Some(c) = m.all_connectors().find(|c| !c.is_origin() && !occ.contains(&c.instance) && asm.instance(c.instance).is_none()) {
                return Err(CommandError::Invalid(format!("instance {} not found", c.instance)));
            }
        }
        MateKind::Group { .. } | MateKind::Replicate(_) => check_ids(asm, &f.instances())?,
        // P3B.9: a relation's mates must be there and fit its type.
        MateKind::Relation(r) => {
            if let Some(why) = super::relation::check(r, &asm.mates) {
                return Err(CommandError::Invalid(why));
            }
        }
        MateKind::Variable(v) => {
            if let Some(why) = v.problem() {
                return Err(CommandError::Invalid(why.into()));
            }
        }
    }
    match &f.kind {
        // A12.1, A12.2: one or two tabs, and no instance both a tab and a width.
        MateKind::Mate(m) if m.mate_type == super::mate::MateType::Width => {
            if m.tabs.is_empty() || m.tabs.len() > 2 {
                Err(CommandError::Invalid("a Width mate needs one or two tabs".into()))
            } else if m.tabs.iter().any(|t| m.connectors.iter().any(|w| w.instance == t.instance)) {
                Err(CommandError::Invalid("an instance can't be both a tab and a width".into()))
            } else {
                Ok(())
            }
        }
        MateKind::Mate(m) if m.connectors[0].instance == m.connectors[1].instance => {
            Err(CommandError::Invalid("a mate needs two instances".into()))
        }
        MateKind::Group { instances } if instances.len() < 2 => Err(CommandError::Invalid("a group needs two instances".into())),
        _ => Ok(()),
    }
}

/// Adds a mate or a group (✓ of the mate or Group dialog, A6.3, A13.2) at the end of the Mate
/// Features list, with the placements the solver found for it: one undo step.
#[derive(Debug, Clone)]
pub struct AddMateFeature {
    pub element: ElementId,
    pub feature: MateFeature,
    pub poses: Vec<(InstanceId, Pose)>,
}

impl Command for AddMateFeature {
    fn label(&self) -> String {
        format!("Add {}", self.feature.name)
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        check_feature(doc, self.element, &self.feature)?;
        let asm = assembly_mut(doc, self.element)?;
        if asm.mate(self.feature.id).is_some() {
            return Err(CommandError::Invalid("mate id already in use".into()));
        }
        asm.mates.push(self.feature.clone());
        // P3F.4: offsets naming variables follow them.
        let units = doc.units;
        if let Ok(asm) = assembly_mut(doc, self.element) {
            super::vars::refresh(asm, &units);
        }
        super::structure::place(doc, self.element, &self.poses)
    }
}

/// Replaces a mate or a group (its dialog's ✓ after an edit), with the solved placements.
#[derive(Debug, Clone)]
pub struct SetMateFeature {
    pub element: ElementId,
    pub feature: MateFeature,
    pub poses: Vec<(InstanceId, Pose)>,
}

impl Command for SetMateFeature {
    fn label(&self) -> String {
        format!("Edit {}", self.feature.name)
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        check_feature(doc, self.element, &self.feature)?;
        let asm = assembly_mut(doc, self.element)?;
        let Some(slot) = asm.mates.iter_mut().find(|m| m.id == self.feature.id) else {
            return Err(CommandError::Invalid("mate not found".into()));
        };
        *slot = self.feature.clone();
        // P3F.4: a changed variable reaches the offsets that name it.
        let units = doc.units;
        if let Ok(asm) = assembly_mut(doc, self.element) {
            super::vars::refresh(asm, &units);
        }
        super::structure::place(doc, self.element, &self.poses)
    }
}

/// Adds or replaces an assembly's own explicit mate connector (P3B.7, A22.2: the Mate connector
/// dialog's ✓ in an assembly): one undo step.
#[derive(Debug, Clone)]
pub struct SetLocalConnector {
    pub element: ElementId,
    pub connector: super::connector::LocalConnector,
}

impl Command for SetLocalConnector {
    fn label(&self) -> String {
        format!("Mate connector: {}", self.connector.name)
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let asm = doc.element(self.element).and_then(|e| e.assembly_model()).ok_or_else(|| CommandError::Invalid("not an assembly".into()))?;
        let owner = self.connector.connector.instance;
        let occ: std::collections::HashSet<InstanceId> = super::structure::occurrences(doc, asm).into_iter().map(|o| o.id).collect();
        if !occ.contains(&owner) && asm.instance(owner).is_none() {
            return Err(CommandError::Invalid(format!("instance {owner} not found")));
        }
        let asm = assembly_mut(doc, self.element)?;
        let after = asm.mates.len();
        match asm.connectors.iter_mut().find(|c| c.id == self.connector.id) {
            // An edit keeps its place in the list.
            Some(slot) => *slot = super::connector::LocalConnector { listed_after: self.connector.listed_after.or(slot.listed_after), ..self.connector.clone() },
            // A new one comes after the mate features there are.
            None => asm.connectors.push(super::connector::LocalConnector { listed_after: self.connector.listed_after.or(Some(after)), ..self.connector.clone() }),
        }
        Ok(())
    }
}

/// Deletes an assembly's own explicit mate connector, and the mates on it.
#[derive(Debug, Clone)]
pub struct DeleteLocalConnector {
    pub element: ElementId,
    pub id: super::connector::LocalConnectorId,
}

impl Command for DeleteLocalConnector {
    fn label(&self) -> String {
        "Delete mate connector".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let asm = assembly_mut(doc, self.element)?;
        if asm.local_connector(self.id).is_none() {
            return Err(CommandError::Invalid("mate connector not found".into()));
        }
        asm.connectors.retain(|c| c.id != self.id);
        let id = self.id;
        asm.retain_mates(|f| {
            !f.mate().is_some_and(|m| m.all_connectors().any(|c| matches!(c.anchor, super::connector::ConnectorAnchor::Local { id: x } if x == id)))
        });
        super::folders::tidy(asm);
        Ok(())
    }
}

/// Deletes mates and groups (the mate menu's Delete).
#[derive(Debug, Clone)]
pub struct DeleteMateFeatures {
    pub element: ElementId,
    pub mates: Vec<MateId>,
}

impl Command for DeleteMateFeatures {
    fn label(&self) -> String {
        "Delete mates".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let asm = assembly_mut(doc, self.element)?;
        if self.mates.is_empty() || self.mates.iter().any(|m| asm.mate(*m).is_none()) {
            return Err(CommandError::Invalid("mate not found".into()));
        }
        // P3B.8: a Replicate deleted takes its copies; so does deleting its seed mate.
        let mut copies: Vec<InstanceId> = Vec::new();
        for f in &asm.mates {
            if let MateKind::Replicate(r) = &f.kind
                && (self.mates.contains(&f.id) || self.mates.contains(&r.seed_mate))
            {
                copies.extend(r.instances.iter().copied());
            }
        }
        asm.retain_mates(|m| !self.mates.contains(&m.id) && !matches!(&m.kind, MateKind::Replicate(r) if self.mates.contains(&r.seed_mate)));
        // P3B.9: a relation goes with its mates.
        asm.drop_orphan_relations();
        if !copies.is_empty() {
            asm.instances.retain(|i| !copies.contains(&i.id));
            for v in &mut asm.exploded_views {
                for st in &mut v.steps {
                    st.instances.retain(|i| !copies.contains(i));
                }
                v.steps.retain(|st| !st.instances.is_empty());
            }
        }
        super::folders::tidy(asm);
        Ok(())
    }
}

/// Renames a mate or a group (the mate menu's Rename, A6.13).
#[derive(Debug, Clone)]
pub struct RenameMateFeature {
    pub element: ElementId,
    pub mate: MateId,
    pub name: String,
}

impl Command for RenameMateFeature {
    fn label(&self) -> String {
        "Rename mate".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err(CommandError::Invalid("a mate needs a name".into()));
        }
        let asm = assembly_mut(doc, self.element)?;
        let f = asm.mates.iter_mut().find(|m| m.id == self.mate).ok_or_else(|| CommandError::Invalid("mate not found".into()))?;
        f.name = name.to_string();
        Ok(())
    }
}

/// Suppresses or unsuppresses mates and groups (the mate menu's Suppress, A6.13): a suppressed
/// mate stays in the list and is ignored by the solver.
#[derive(Debug, Clone)]
pub struct SetMatesSuppressed {
    pub element: ElementId,
    pub mates: Vec<MateId>,
    pub suppressed: bool,
}

impl Command for SetMatesSuppressed {
    fn label(&self) -> String {
        if self.suppressed { "Suppress mates".into() } else { "Unsuppress mates".into() }
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let asm = assembly_mut(doc, self.element)?;
        if self.mates.is_empty() || self.mates.iter().any(|m| asm.mate(*m).is_none()) {
            return Err(CommandError::Invalid("mate not found".into()));
        }
        for f in &mut asm.mates {
            if self.mates.contains(&f.id) {
                f.suppressed = self.suppressed;
            }
        }
        Ok(())
    }
}

/// **Edit** a rigid Part Studio instance (A2.4): the parts it holds. Mates on a part taken out
/// go. One undo step.
#[derive(Debug, Clone)]
pub struct SetStudioParts {
    pub element: ElementId,
    pub instance: InstanceId,
    pub parts: Vec<crate::ids::PartId>,
}

impl Command for SetStudioParts {
    fn label(&self) -> String {
        "Edit rigid Part Studio instance".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        if self.parts.is_empty() {
            return Err(CommandError::Invalid("a rigid Part Studio instance needs a part".into()));
        }
        let asm = assembly_mut(doc, self.element)?;
        let inst = asm.instance_mut(self.instance).ok_or_else(|| CommandError::Invalid(format!("instance {} not found", self.instance)))?;
        if !inst.source.is_studio() {
            return Err(CommandError::Invalid("not a rigid Part Studio instance".into()));
        }
        let removed: Vec<InstanceId> = inst
            .parts
            .iter()
            .filter(|p| !self.parts.contains(p))
            .map(|p| super::structure::derive(self.instance, super::structure::studio_part_key(*p)))
            .collect();
        inst.parts = self.parts.clone();
        asm.retain_mates(|f| !f.mate().is_some_and(|m| m.all_connectors().any(|c| removed.contains(&c.instance))));
        super::folders::tidy(asm);
        Ok(())
    }
}
