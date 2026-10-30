//! Document model, IDs, persistence and the command/undo layer for cadrs. No Bevy here.
//!
//! Every document mutation goes through a [`Command`] executed by a [`History`], which records
//! snapshots of the edited scope (the whole document, or a single element) so it can undo and
//! redo. Edits on the documents page (rename, move to trash) go through [`LibraryCommand`]s and a
//! [`LibraryHistory`] in the same way.

pub mod advanced;
pub mod analysis;
pub mod appearance;
pub mod applied;
pub mod assembly;
pub mod background;
pub mod blob_store;
pub mod blobs;
pub mod brep;
pub mod cam;
pub mod check;
pub mod collab;
pub mod command;
pub mod commands;
pub mod derived;
pub mod direct_edit;
pub mod document;
pub mod documents_page;
pub mod draft;
pub mod drawing_assembly;
pub mod drawing_export;
pub mod drawing_source;
pub mod dxf_export;
pub mod dxf_import;
pub mod export;
pub mod external;
pub mod feature_list;
pub mod flat_drawing;
pub mod flat_export;
pub mod history_log;
pub mod hole;
pub mod identity;
pub mod ids;
pub mod import;
pub mod library;
pub mod link_update;
pub mod links;
pub mod mate;
pub mod material;
pub mod measure;
pub mod move_doc;
pub mod named_views;
pub mod native;
pub mod parts;
pub mod pattern;
pub mod pcb;
pub mod picture;
pub mod plane;
pub mod preferences;
pub mod properties;
pub mod rebuild;
pub mod render;
pub mod repair;
pub mod samples;
pub mod section;
pub mod sheetmetal;
pub mod sheetmetal_features;
pub mod sheetmetal_flat;
pub mod sheetmetal_form;
pub mod sheetmetal_joint;
pub mod sheetmetal_loft;
pub mod sheetmetal_tools;
pub mod simulation;
// Starts programs: not in a browser.
#[cfg(not(target_arch = "wasm32"))]
pub mod slicer;
pub mod solid;
pub mod store;
pub mod studio;
pub mod surfacing;
pub mod tab_tree;
pub mod thread;
pub mod thumbnail;
pub mod time;
pub mod transform;
pub mod variable_studio;
pub mod variables;
pub mod vector_import;
pub mod views;
pub mod workspace_merge;

pub use appearance::Appearance;
pub use command::{Command, CommandError, History, Scope};
pub use document::{
    AxisRef, BodyType, BooleanFeature, BooleanKind, BooleanOp, DeletePartFeature, DirectionRef,
    Document, EdgeRef, Element, ElementKind, EndCondition, EndType, ExtrudeFeature, FaceRef,
    Feature, FeatureKind, Offset, PartProps, RegionRef, RevolveFeature, RevolveType, SketchFeature,
    ThinWall, UpTo, VertexRef,
};
pub use ids::{DocumentId, ElementId, FeatureId, FolderId, LabelId, PartId};
pub use library::{
    DocumentEntry, DocumentMeta, Filter, FolderEntry, ItemType, LabelEntry, Library,
    LibraryCommand, LibraryHistory, SortDir, SortKey, Timestamp,
};
pub use material::Material;
pub use parts::{Part, PartKind};
pub use solid::{EdgeTag, Solid, SolidEdge, SolidFace};
pub use store::{DocumentFile, Store, StoreError};
