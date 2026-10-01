//! Data for the documents page that is not the library itself (P3E.1):
//!
//! - [`SAMPLES`]: the bundled sample documents (TD4.1, the course stand-ins in `fixtures/`),
//!   listed under Explore cadrs and opened as an editable copy
//!   ([`crate::store::Store::copy_from_file`]).
//! - [`versions`]: the details panel's versions list (TD3.8), the document's
//!   [`crate::history_log::HistoryLog`] versions, newest first.

use std::path::{Path, PathBuf};

use crate::history_log::HistoryLog;
use crate::ids::DocumentId;
use crate::library::Timestamp;
use crate::store::{Store, StoreError};

/// A bundled sample document: `fixtures/<key>.cadrs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SampleDoc {
    pub key: &'static str,
    /// The name shown in the list (and the copy's default name).
    pub title: &'static str,
    /// The course it comes from.
    pub course: &'static str,
    pub description: &'static str,
}

/// Who the samples belong to (the details panel's Owner).
pub const SAMPLE_OWNER: &str = "cadrs samples";

/// The bundled samples, in list order.
pub const SAMPLES: &[SampleDoc] = &[
    SampleDoc {
        key: "linked_block_standin",
        title: "Block source",
        course: "Linked documents",
        description: "A 50 × 30 × 25 mm aluminium block, the source document of the Linked documents course.",
    },
    SampleDoc {
        key: "motor_mount_standin",
        title: "Starting an Assembly",
        course: "Introduction to Assemblies",
        description: "The motor mount parts the Assemblies course starts from.",
    },
    SampleDoc {
        key: "step_stool_standin",
        title: "Creating Mate Connectors",
        course: "Introduction to Assemblies",
        description: "A step stool: a Part Studio and an assembly to add mate connectors to.",
    },
    SampleDoc {
        key: "pneumatic_cylinder_standin",
        title: "Pneumatic Cylinder",
        course: "Introduction to Assemblies",
        description: "The pneumatic cylinder of the assembly exercises.",
    },
    SampleDoc {
        key: "ujoint_flange_standin",
        title: "Universal Joint Drawing",
        course: "Introduction to Drawings",
        description: "A universal joint flange to make a drawing of.",
    },
    SampleDoc {
        key: "gear_cover_standin",
        title: "Jackhammer",
        course: "Inspection and Repair",
        description: "A jackhammer gear cover with features to inspect and repair.",
    },
    SampleDoc {
        key: "drill_standin",
        title: "Drill",
        course: "Test Drive",
        description: "A drill's carburetor and drill body: the starting document of the Test Drive exercise.",
    },
];

impl SampleDoc {
    /// `dir/<key>.cadrs`.
    pub fn path(&self, dir: &Path) -> PathBuf {
        dir.join(format!("{}.cadrs", self.key))
    }
}

/// A row of the details panel's versions list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionRow {
    pub name: String,
    pub description: String,
    pub time: Timestamp,
    pub user: String,
}

/// Document `id`'s versions, newest first (none when it has no history yet).
pub fn versions(store: &Store, id: DocumentId) -> Result<Vec<VersionRow>, StoreError> {
    let Some(log) = HistoryLog::load(store, id)? else {
        return Ok(Vec::new());
    };
    Ok(log
        .versions()
        .iter()
        .rev()
        .map(|v| VersionRow { name: v.name().to_string(), description: v.description().to_string(), time: v.time(), user: v.user().to_string() })
        .collect())
}
