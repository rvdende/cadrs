//! The Machine dialog's "From assembly" across documents: Document › Assembly › Version.
//!
//! This document's assemblies are followed at its workspace
//! ([`MachineSource::workspace`]). Another document's are taken at one of its versions, as
//! the Insert dialog takes parts ([`super::InsertDraft`]): the document is opened at the
//! version ([`crate::linked::open`]), its assemblies that are set up as machines are listed,
//! and the one chosen comes with the frozen copies of itself and everything it is made of
//! (its Part Studios, its sub-assemblies: [`cadrs_core::external::LinkSnapshot`]). ✓ stores
//! them with the reference ([`cadrs_core::cam::Edit::SetMachineSource`]); until then the
//! document does not hold them, so what the assembly makes is worked out here, on the
//! document with the copies beside it ([`MachineFrom::taken`]).

use bevy::prelude::*;
use cadrs_core::assembly::machine::Issue;
use cadrs_core::cam::MachineSource;
use cadrs_core::cam::cadrs_cam::machine::Machine;
use cadrs_core::external::{LinkedElement, RefAt};
use cadrs_core::history_log::VersionId;
use cadrs_core::{Document, DocumentId, ElementId};

use super::toast;
use crate::ActiveDocument;

/// What an assembly makes as a machine: the machine and its warnings, or what stands in the
/// way ([`cadrs_core::cam::machine_of`]).
pub type Taken = Result<(Machine, Vec<Issue>), Vec<Issue>>;

/// Where a Machine draft's "From assembly" looks.
#[derive(Debug, Clone, Default)]
pub struct MachineFrom {
    /// 0: this document; 1…: one of `others`.
    pub document: usize,
    /// The library's other documents with versions: (id, name).
    pub others: Vec<(DocumentId, String)>,
    /// The library is still being read (`others` comes when it's done).
    pub others_pending: bool,
    /// The other document chosen, opened at a version.
    pub other: Option<OtherMachines>,
    /// The frozen copies the draft's source needs: a source newly taken from another
    /// document (one the job already had is in the document).
    pub links: Vec<LinkedElement>,
    /// What that source's assembly makes, worked out with the copies.
    pub taken: Option<Taken>,
}

/// Another document a machine is taken from, opened at one of its versions.
#[derive(Debug, Clone)]
pub struct OtherMachines {
    /// Its versions, newest last.
    pub versions: Vec<(VersionId, String)>,
    /// The one opened.
    pub version: usize,
    /// Its assemblies set up as machines there, in tab order.
    pub assemblies: Vec<(ElementId, String)>,
    pub opened: crate::linked::Opened,
}

/// The assemblies of `doc` set up as machines (their Machine Builder's work), in tab order.
pub(crate) fn machine_assemblies(doc: &Document) -> Vec<(ElementId, String)> {
    doc.elements
        .iter()
        .filter(|e| e.assembly_model().is_some_and(|a| a.machine.is_some()))
        .map(|e| (e.id, e.name.clone()))
        .collect()
}

impl OtherMachines {
    /// A document opened at a version, with its versions: its machines there.
    pub fn of(opened: crate::linked::Opened, versions: Vec<(VersionId, String)>) -> OtherMachines {
        let at = opened.version.as_ref().map(|v| v.0);
        let assemblies = machine_assemblies(&opened.doc)
            .into_iter()
            .filter(|(e, _)| opened.snapshot(*e).is_some())
            .collect();
        OtherMachines {
            version: versions
                .iter()
                .position(|v| Some(v.0) == at)
                .unwrap_or(versions.len().saturating_sub(1)),
            versions,
            assemblies,
            opened,
        }
    }

    /// The source that takes assembly `i` of it into `doc`: the reference with the names it
    /// is shown by, the frozen copies it needs and what the assembly makes there.
    pub fn take(
        &self,
        doc: &Document,
        i: usize,
    ) -> Option<(MachineSource, Vec<LinkedElement>, Taken)> {
        let (element, name) = self.assemblies.get(i)?;
        let snap = self.opened.snapshot(*element)?;
        let reference = self.opened.reference(*element)?;
        let with = crate::linked::with_links(doc, &snap.links);
        let taken = cadrs_core::cam::machine_of(&with, snap.root);
        let source = MachineSource {
            reference,
            copy: Some(snap.root),
            source_name: name.clone(),
            document_name: self.opened.name.clone(),
            version_name: self.opened.version_label(),
            key: 0,
        };
        Some((source, snap.links.clone(), taken))
    }
}

/// Opens another document at a version (its newest when `None`) for its machines; a toast
/// says why when it cannot be read.
pub(super) fn load_other(
    world: &mut World,
    document: DocumentId,
    version: Option<VersionId>,
) -> Option<OtherMachines> {
    let versions: Vec<(VersionId, String)> = crate::linked::versions_of(world, document)
        .into_iter()
        .map(|(id, name, _)| (id, name))
        .collect();
    match crate::linked::open(world, document, version) {
        Ok(o) => Some(OtherMachines::of(o, versions)),
        Err(e) => {
            toast(world, &e);
            None
        }
    }
}

impl MachineFrom {
    /// Where the Machine dialog starts for a job whose machine has the source `source`: the
    /// library's other documents, and for a machine of another document that document open
    /// at the version it was taken at.
    pub(super) fn open(world: &mut World, source: Option<&MachineSource>) -> MachineFrom {
        let found = super::other_documents(world);
        let mut from = MachineFrom {
            others_pending: found.is_none(),
            others: found.unwrap_or_default(),
            ..Default::default()
        };
        if !from.others_pending {
            from.find(world, source);
        }
        from
    }

    /// The library's other documents, read since the dialog opened.
    pub(super) fn others_read(
        &mut self,
        world: &mut World,
        others: Vec<(DocumentId, String)>,
        source: Option<&MachineSource>,
    ) {
        self.others = others;
        self.others_pending = false;
        // (Unless a document was chosen meanwhile.)
        if self.document == 0 && self.other.is_none() {
            self.find(world, source);
        }
    }

    /// Opens the document `source` was taken from, at its version, when that is another one.
    fn find(&mut self, world: &mut World, source: Option<&MachineSource>) {
        let this = world.resource::<ActiveDocument>().doc.id;
        let Some(source) = source else { return };
        let Some(id) = source.reference.document.filter(|d| *d != this) else {
            return;
        };
        let at = match self.others.iter().position(|o| o.0 == id) {
            Some(at) => at,
            // (Not in the library any more: still shown, by the name it had.)
            None => {
                self.others.push((id, source.document_name.clone()));
                self.others.len() - 1
            }
        };
        self.document = at + 1;
        let version = match source.reference.at {
            RefAt::Version(v) => Some(v),
            RefAt::Workspace => None,
        };
        self.other = load_other(world, id, version);
    }

    /// Chooses the document (0: this one). The draft's new source: this document's first
    /// assembly that makes a machine at its workspace, or the first of the other document's
    /// newest version.
    pub(super) fn set_document(&mut self, world: &mut World, i: usize) -> Option<MachineSource> {
        self.document = i.min(self.others.len());
        self.other = match self.document {
            0 => None,
            k => load_other(world, self.others[k - 1].0, None),
        };
        self.first(world)
    }

    /// Chooses the other document's version; the same assembly there when it has it, else
    /// its first.
    pub(super) fn set_version(
        &mut self,
        world: &mut World,
        i: usize,
        was: Option<&MachineSource>,
    ) -> Option<MachineSource> {
        let (document, _) = *self.others.get(self.document.checked_sub(1)?)?;
        let version = self.other.as_ref()?.versions.get(i)?.0;
        self.other = load_other(world, document, Some(version));
        let same = self.other.as_ref().and_then(|o| {
            let was = was?.reference.element;
            o.assemblies.iter().position(|(e, _)| *e == was)
        });
        match same {
            Some(k) => self.set_assembly(world, k),
            None => self.first(world),
        }
    }

    /// The first assembly of the chosen document that makes a machine (else its first).
    fn first(&mut self, world: &mut World) -> Option<MachineSource> {
        let doc = world.resource::<ActiveDocument>().doc.clone();
        let Some(o) = &self.other else {
            self.links.clear();
            self.taken = None;
            if self.document > 0 {
                return None;
            }
            let list = machine_assemblies(&doc);
            return list
                .iter()
                .find(|(e, _)| cadrs_core::cam::machine_of(&doc, *e).is_ok())
                .or(list.first())
                .and_then(|(e, _)| MachineSource::workspace(&doc, *e));
        };
        let k = (0..o.assemblies.len())
            .find(|k| o.take(&doc, *k).is_some_and(|t| t.2.is_ok()))
            .unwrap_or(0);
        self.set_assembly(world, k)
    }

    /// Chooses assembly `i` of the chosen document.
    pub(super) fn set_assembly(&mut self, world: &mut World, i: usize) -> Option<MachineSource> {
        let doc = &world.resource::<ActiveDocument>().doc;
        self.links.clear();
        self.taken = None;
        let Some(o) = &self.other else {
            if self.document > 0 {
                return None;
            }
            let (e, _) = *machine_assemblies(doc).get(i)?;
            return MachineSource::workspace(doc, e);
        };
        let (source, links, taken) = o.take(doc, i)?;
        // (Copies the document holds already, the job's own machine among them, are its.)
        if doc.linked_element(source.element()).is_none() {
            self.links = links;
            self.taken = Some(taken);
        }
        Some(source)
    }

    /// What the draft's source makes: as taken with its copies, else from the document.
    pub fn machine(&self, doc: &Document, source: &MachineSource) -> Taken {
        match &self.taken {
            Some(t) => t.clone(),
            None => cadrs_core::cam::machine_of(doc, source.element()),
        }
    }
}
