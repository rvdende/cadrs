//! Stable identifiers. They are UUIDs so they survive save/load and never collide across
//! documents.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! id_type {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        pub struct $name(pub Uuid);

        impl $name {
            /// A new random id.
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            /// A deterministic id, for tests and scripted scenarios.
            pub const fn from_u128(v: u128) -> Self {
                Self(Uuid::from_u128(v))
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

id_type!(
    /// Identifies a document.
    DocumentId
);
id_type!(
    /// Identifies an element (a tab: Part Studio or Assembly) within a document.
    ElementId
);
id_type!(
    /// Identifies a feature within a Part Studio.
    FeatureId
);
id_type!(
    /// Identifies a folder on the documents page.
    FolderId
);
id_type!(
    /// Identifies a document label on the documents page (P3E.1, TD3.3, TD3.8).
    LabelId
);

/// Identifies a part (P3.3): the feature that made it and which of the bodies that feature made
/// (0 for the first). A part keeps its id while later features add to it, cut it or trim it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PartId {
    pub feature: FeatureId,
    #[serde(default)]
    pub index: u32,
}

impl PartId {
    pub const fn new(feature: FeatureId, index: u32) -> Self {
        Self { feature, index }
    }
}
