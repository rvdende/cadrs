//! **Component documents** for Create assembly (P3H.7; PCB7.3, PCB11.1–PCB11.3, X7): the
//! [`ComponentProvider`] that gives each package a stored document of its own in the component
//! folder and references it by version. The stored side (the key on the document, the folder,
//! the undo decision) is [`cadrs_core::pcb::component_docs`].
//!
//! For a package with no component document yet, [`ComponentDocuments`] writes one to the store:
//! named after the package, with **one Part Studio** (named after the package) holding **one
//! part**: the generic ECAD box (the `.emp` outline extruded by the package height, in the
//! package frame: outline in x/y, from z = 0 up), named after the package, coloured like PCB
//! Studio's components, with its **Part number** and **Description**, and an **empty Assembly**
//! ("Assembly 1"). Its history gets a version **V1**. A package that has one already (any
//! earlier Create, any board, any document; found by the key in its metadata, so a renamed or
//! moved document still matches) is reused at its **newest version** (made first when it has
//! none). Each component is then resolved through 3G's [`Resolver`] into frozen copies, and the
//! instances reference that version ([`SourceRef`], version-pinned, DV1).

use std::collections::HashMap;

use cadrs_core::command::CommandError;
use cadrs_core::commands::{AddElement, NewElementKind};
use cadrs_core::document::{Document, ElementKind, FeatureKind};
use cadrs_core::external::{LinkedElement, Resolver, SourceRef};
use cadrs_core::history_log::{HistoryLog, VersionId};
use cadrs_core::ids::{DocumentId, ElementId, PartId};
use cadrs_core::library::{DocumentMeta, FolderEntry, Library, Timestamp};
use cadrs_core::pcb::FolderRef;
use cadrs_core::pcb::component_docs::{ComponentDocument, ComponentKey, ensure_folder, find_component_document};
use cadrs_core::properties::{PropertyKey, PropertyOwner, PropertyValue, SetProperties};
use cadrs_core::store::Store;
use cadrs_core::studio::Studio;
use cadrs_idf::Package;

use crate::colors::{BodyClass, component_kind};
use crate::create_assembly::{ComponentProvider, ComponentSource, Direct, PackageComponent, description};
use crate::geometry::{BodyPlan, MARKER};
use crate::sample::{build_plan, fid, name_hash};

/// The name of a component document's Assembly.
pub const COMPONENT_ASSEMBLY_NAME: &str = "Assembly 1";

/// A component document made or reused by a Create.
#[derive(Clone, Debug, PartialEq)]
pub struct UsedDocument {
    pub document: DocumentId,
    pub name: String,
    pub package: String,
    pub version: VersionId,
    pub version_name: String,
    /// Made by this Create (else reused).
    pub created: bool,
}

/// The component documents of one Create assembly (see the module docs).
pub struct ComponentDocuments {
    store: Store,
    resolver: Resolver,
    folder: FolderEntry,
    user: String,
    now: Timestamp,
    board: String,
    /// The board document (references are resolved from it).
    consumer: Document,
    library: Library,
    made: HashMap<(String, String), PackageComponent>,
    links: Vec<LinkedElement>,
    /// Every component document used, in the order first used.
    pub used: Vec<UsedDocument>,
}

impl ComponentDocuments {
    /// Component documents for a Create in the document `consumer` (its id is what matters) of
    /// the board `board`, in the settings' folder `folder` (made when missing; see
    /// [`ensure_folder`]).
    pub fn new(store: &Store, folder: Option<&FolderRef>, consumer: DocumentId, board: &str, user: &str, now: Timestamp) -> Result<Self, String> {
        let folder = ensure_folder(store, folder, user, now).map_err(|e| format!("The component folder can't be made ({e})"))?;
        let (library, _) = store.list();
        let mut shell = Document::empty("");
        shell.id = consumer;
        Ok(Self {
            store: store.clone(),
            resolver: Resolver::new(store.clone()),
            folder,
            user: user.to_string(),
            now,
            board: board.to_string(),
            consumer: shell,
            library,
            made: HashMap::new(),
            links: Vec::new(),
            used: Vec::new(),
        })
    }

    /// The folder the new documents went in.
    pub fn folder(&self) -> &FolderEntry {
        &self.folder
    }

    /// How many documents this Create made.
    pub fn created(&self) -> usize {
        self.used.iter().filter(|u| u.created).count()
    }

    /// The records for [`cadrs_core::pcb::GeneratedAssembly::documents`].
    pub fn records(&self) -> Vec<ComponentDocument> {
        let mut out: Vec<ComponentDocument> = Vec::new();
        for ((package, part_number), c) in &self.made {
            if let ComponentSource::External { reference, .. } = &c.source
                && let Some(d) = reference.document
            {
                out.push(ComponentDocument { package: package.clone(), part_number: part_number.clone(), document: d });
            }
        }
        out.sort_by(|a, b| a.package.cmp(&b.package).then_with(|| a.part_number.cmp(&b.part_number)));
        out
    }

    fn err(e: impl std::fmt::Display) -> CommandError {
        CommandError::Invalid(e.to_string())
    }

    /// Writes a new component document for `pkg` (see the module docs) and its V1.
    fn create(&mut self, pkg: &Package) -> Result<(DocumentId, ComponentKey, VersionId), CommandError> {
        let mut doc = Document::empty(pkg.name.clone());
        let (studio, asm) = (ElementId::new(), ElementId::new());
        let salt = name_hash(&format!("{}\u{0}{}", pkg.name, pkg.part_number));
        let part = {
            let mut s = Direct(&mut doc);
            s.run(&AddElement { id: studio, kind: NewElementKind::PartStudio, name: Some(pkg.name.clone()), after: None })?;
            let plan = BodyPlan {
                name: pkg.name.clone(),
                class: BodyClass::Component(component_kind(&pkg.name)),
                item: None,
                loops: pkg.loops.clone(),
                z0: 0.0,
                depth: pkg.height.max(MARKER),
            };
            let part = build_plan(&mut s, studio, &plan, fid(salt, 0x10), fid(salt, 0x11))?;
            let mut values = Vec::new();
            if !pkg.part_number.is_empty() {
                values.push((PropertyKey::PartNumber, PropertyValue::Text(pkg.part_number.clone())));
            }
            if let Some(d) = description(pkg) {
                values.push((PropertyKey::Description, PropertyValue::Text(d)));
            }
            if !values.is_empty() {
                s.run(&SetProperties { owners: vec![PropertyOwner::Part { element: studio, part }], values, label: "Component properties".into() })?;
            }
            s.run(&AddElement { id: asm, kind: NewElementKind::Assembly, name: Some(COMPONENT_ASSEMBLY_NAME.into()), after: None })?;
            part
        };
        let key = ComponentKey { package: pkg.name.clone(), part_number: pkg.part_number.clone(), studio, part };
        let mut meta = DocumentMeta::new(&self.user, self.now);
        meta.folder = Some(self.folder.id);
        meta.pcb_component = Some(key.clone());
        meta.description = format!("PCB component {}{}", pkg.name, if pkg.part_number.is_empty() { String::new() } else { format!(" ({})", pkg.part_number) });
        let entry = self.store.create(&doc, &meta).map_err(Self::err)?;
        self.library.upsert(entry);
        let mut log = HistoryLog::start(&doc, self.now, &self.user);
        let v = log.create_version("", &format!("Created by PCB Studio from {}", self.board), self.now, &self.user);
        log.save(&self.store).map_err(Self::err)?;
        Ok((doc.id, key, v))
    }

    /// The Part Studio and part of a reused document at `version`: the key's, else (edited away)
    /// its first Part Studio's first extrude's first part.
    fn part_at(&mut self, document: DocumentId, version: VersionId, key: &ComponentKey) -> Result<(ElementId, PartId), CommandError> {
        let at = self.resolver.document_at(document, version).map_err(Self::err)?;
        if at.element(key.studio).is_some_and(|e| matches!(e.kind, ElementKind::PartStudio { .. })) {
            return Ok((key.studio, key.part));
        }
        let el = at.elements.iter().find(|e| matches!(e.kind, ElementKind::PartStudio { .. })).ok_or_else(|| Self::err(format!("{} has no Part Studio", at.name)))?;
        let f = el.features().iter().find(|f| matches!(f.kind, FeatureKind::Extrude(_)) && !el.is_suppressed(f.id)).ok_or_else(|| Self::err(format!("{} has no part", at.name)))?;
        Ok((el.id, PartId::new(f.id, 0)))
    }
}

impl ComponentProvider for ComponentDocuments {
    fn component(&mut self, _s: &mut dyn Studio, pkg: &Package) -> Result<PackageComponent, CommandError> {
        let k = (pkg.name.clone(), pkg.part_number.clone());
        if let Some(c) = self.made.get(&k) {
            return Ok(c.clone());
        }
        let (document, studio, part, version, created) = match find_component_document(&self.library, &pkg.name, &pkg.part_number) {
            Some((d, key)) => {
                let v = match self.resolver.latest(d) {
                    Some(v) => v.id(),
                    None => self.resolver.create_version(d, "", &format!("Created by PCB Studio for {}", self.board), self.now, &self.user).map_err(Self::err)?,
                };
                let (studio, part) = self.part_at(d, v, &key)?;
                (d, studio, part, v, false)
            }
            None => {
                let (d, key, v) = self.create(pkg)?;
                (d, key.studio, key.part, v, true)
            }
        };
        let reference = SourceRef::version(Some(document), studio, version);
        let snap = self.resolver.resolve(reference, &self.consumer, None).map_err(Self::err)?;
        for l in &snap.links {
            if self.links.iter().all(|x| x.id() != l.id()) {
                self.links.push(l.clone());
            }
        }
        let (name, version_name) = snap.root_link().map(|l| (l.document_name.clone(), l.version_name.clone())).unwrap_or_default();
        self.used.push(UsedDocument { document, name, package: pkg.name.clone(), version, version_name, created });
        let c = PackageComponent { source: ComponentSource::External { reference, copy: snap.root, part }, frame: cadrs_core::assembly::Pose::IDENTITY };
        self.made.insert(k, c.clone());
        Ok(c)
    }

    fn links(&mut self) -> Vec<LinkedElement> {
        std::mem::take(&mut self.links)
    }

    fn documents(&self) -> Vec<ComponentDocument> {
        self.records()
    }
}
