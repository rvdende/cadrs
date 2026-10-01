//! Document model, IDs, persistence and the command/undo layer for cadrs. No Bevy here.
//!
//! Every document mutation goes through a [`Command`] executed by a [`History`], which records
//! snapshots of the edited scope (the whole document, or a single element) so it can undo and
//! redo. Edits on the documents page (rename, move to trash) go through [`LibraryCommand`]s and a
//! [`LibraryHistory`] in the same way.

pub mod advanced;
pub mod assembly;
pub mod applied;
pub mod appearance;
pub mod blobs;
pub mod command;
pub mod commands;
pub mod derived;
pub mod document;
pub mod documents_page;
pub mod drawing_assembly;
pub mod drawing_export;
pub mod drawing_source;
pub mod draft;
pub mod export;
pub mod external;
pub mod feature_list;
pub mod ids;
pub mod hole;
pub mod import;
pub mod dxf_export;
pub mod history_log;
pub mod library;
pub mod link_update;
pub mod links;
pub mod material;
pub mod move_doc;
pub mod mate;
pub mod named_views;
pub mod measure;
pub mod brep;
pub mod parts;
pub mod pcb;
pub mod pattern;
pub mod plane;
pub mod properties;
pub mod rebuild;
pub mod render;
pub mod repair;
pub mod samples;
pub mod section;
pub mod simulation;
pub mod solid;
pub mod store;
pub mod studio;
pub mod surfacing;
pub mod tab_tree;
pub mod thumbnail;
pub mod time;
pub mod variables;
pub mod transform;
pub mod views;

pub use command::{Command, CommandError, History, Scope};
pub use document::{
    BodyType, BooleanFeature, BooleanKind, BooleanOp, DeletePartFeature, DirectionRef, Document,
    EdgeRef, Element, ElementKind, EndCondition, EndType, ExtrudeFeature, FaceRef, Feature,
    FeatureKind, Offset, PartProps, RegionRef, AxisRef, RevolveFeature, RevolveType, SketchFeature,
    ThinWall, UpTo, VertexRef,
};
pub use appearance::Appearance;
pub use material::Material;
pub use parts::{Part, PartKind};
pub use solid::{EdgeTag, Solid, SolidEdge, SolidFace};
pub use ids::{DocumentId, ElementId, FeatureId, FolderId, LabelId, PartId};
pub use library::{
    DocumentEntry, DocumentMeta, Filter, FolderEntry, ItemType, LabelEntry, Library, LibraryCommand,
    LibraryHistory, SortDir, SortKey, Timestamp,
};
pub use store::{DocumentFile, Store, StoreError};
