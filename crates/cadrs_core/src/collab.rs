//! Sharing and collaboration between peers, with no server (`cadrs_net` is the transport).
//!
//! - A document's owner keeps its [`Sharing`] in the document's metadata: the room it is
//!   shared in and who may do what. Someone it was shared with keeps a [`SharedInfo`] instead:
//!   whose it is, the link to rejoin it by, and their permission. Peers are named by their
//!   cadrs ID, the text form of their public key.
//! - A [`Snapshot`] is how a whole document travels: on joining, and to catch up. It carries
//!   the imported files the document's features use, so the copy rebuilds the same.
//! - [`pack`] and [`unpack`] are the wire form of anything with a document in it: MessagePack
//!   with field names (self-describing, so the documents' optional fields round-trip, as for
//!   the kernel process's messages), deflated.
//! - [`sync`]: editing a document together, the owner putting everyone's edits in one order.

pub mod comments;
pub mod sync;

use std::io::{Read, Write};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::blobs;
use crate::document::Document;
use crate::store::Store;

/// The most a packed message may grow to when unpacked: a guard against a peer's deflate bomb.
pub const MAX_UNPACKED: u64 = 1 << 30;

/// What someone a document is shared with may do (Onshape's share dialog: **Can edit**,
/// **Can view**).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Permission {
    Edit,
    View,
}

impl Permission {
    pub const ALL: [Permission; 2] = [Permission::Edit, Permission::View];

    pub fn label(self) -> &'static str {
        match self {
            Permission::Edit => "Can edit",
            Permission::View => "Can view",
        }
    }

    pub fn can_edit(self) -> bool {
        self == Permission::Edit
    }
}

/// The share dialog's checkboxes: what else someone may do with the document. With no
/// peer-to-peer server, Copy, Export and Link document are kept by the recipient's cadrs, not
/// enforced by the owner's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rights {
    /// Make a copy of the document.
    pub copy: bool,
    /// Export it.
    pub export: bool,
    /// Share it with others (through the owner).
    pub share: bool,
    /// Comment in it.
    pub comment: bool,
    /// Link to it from another document.
    pub link: bool,
}

impl Rights {
    /// What a permission gives unless the checkboxes say otherwise (Onshape: Can edit is
    /// edit, copy, link, export, share and comment; Can view is view, copy, link, export and
    /// comment).
    pub fn of(permission: Permission) -> Self {
        Self {
            copy: true,
            export: true,
            share: permission == Permission::Edit,
            comment: true,
            link: true,
        }
    }

    /// Only viewing (a link without exporting).
    pub fn none() -> Self {
        Self {
            copy: false,
            export: false,
            share: false,
            comment: false,
            link: false,
        }
    }
}

/// Someone's access to a document: their permission and the checkboxes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Access {
    pub permission: Permission,
    pub rights: Rights,
}

impl Access {
    /// The owner's: everything.
    pub fn owner() -> Self {
        Self {
            permission: Permission::Edit,
            rights: Rights::of(Permission::Edit),
        }
    }
}

/// Someone a document is shared with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    /// Their cadrs ID.
    pub id: String,
    /// Their display name when they were last seen.
    pub name: String,
    pub permission: Permission,
    /// The checkboxes; `None` is the permission's own ([`Rights::of`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rights: Option<Rights>,
    /// The room their link names, once it reached them: the owner sends the link again
    /// until it is the current one (the room changes when someone is removed or link sharing
    /// is turned off).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivered: Option<[u8; 32]>,
}

impl Member {
    pub fn access(&self) -> Access {
        Access {
            permission: self.permission,
            rights: self.rights.unwrap_or(Rights::of(self.permission)),
        }
    }
}

/// How the owner shares a document (kept in its metadata, on the owner's side only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sharing {
    /// The room's topic: the same every time the owner opens the document, so a link keeps
    /// working. Knowing it is what lets a peer in, so it is as secret as the link.
    pub topic: [u8; 32],
    /// The people it is shared with.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<Member>,
    /// Link sharing: anyone with the link gets this permission; `None` when it is off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<Permission>,
    /// **Allow exporting from the link**.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub link_export: bool,
}

impl Sharing {
    /// Sharing with nobody yet, in a fresh room.
    pub fn new(topic: [u8; 32]) -> Self {
        Self {
            topic,
            members: Vec::new(),
            link: None,
            link_export: false,
        }
    }

    /// What `peer` may do, if anything: a member's own access, or the link's for anyone who
    /// knows the room. The owner is not a member and is never asked about.
    pub fn access(&self, peer: &str) -> Option<Access> {
        self.members
            .iter()
            .find(|m| m.id == peer)
            .map(Member::access)
            .or_else(|| self.link_access())
    }

    /// What the link gives.
    pub fn link_access(&self) -> Option<Access> {
        self.link.map(|permission| Access {
            permission,
            rights: Rights {
                export: self.link_export,
                share: false,
                ..Rights::of(permission)
            },
        })
    }

    pub fn permission(&self, peer: &str) -> Option<Permission> {
        self.access(peer).map(|a| a.permission)
    }

    /// Moves to a fresh room: every link made so far stops working (someone removed, link
    /// sharing turned off). The members get the new link from the owner.
    pub fn rotate(&mut self, topic: [u8; 32]) {
        self.topic = topic;
    }
}

/// A document someone shared with this user (kept in its metadata, on their side).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedInfo {
    /// The owner's cadrs ID.
    pub owner: String,
    /// The owner's display name.
    pub owner_name: String,
    /// The link to rejoin it by.
    pub link: String,
    pub permission: Permission,
    /// The checkboxes the owner gave (older copies: the permission's own).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rights: Option<Rights>,
}

impl SharedInfo {
    pub fn access(&self) -> Access {
        Access {
            permission: self.permission,
            rights: self.rights.unwrap_or(Rights::of(self.permission)),
        }
    }
}

/// A document this user made public (Share ▸ Public; `cadrs_net::public` keeps the listing):
/// the revision last published. Edits made since stay private until it is published again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Published {
    /// 1 for the first time it was published, one more each time after.
    pub revision: u32,
    pub time: crate::library::Timestamp,
}

/// The public document this one was forked from (a copy of it as it was published).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForkedFrom {
    pub document: crate::DocumentId,
    /// Its owner's cadrs ID and name.
    pub owner: String,
    pub owner_name: String,
    pub name: String,
    pub revision: u32,
}

/// Someone this user has shared with or been shared by, to pick in the share dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contact {
    /// Their cadrs ID.
    pub id: String,
    pub name: String,
}

/// The contacts, kept in `contacts.ron` next to the documents.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contacts(pub Vec<Contact>);

impl Contacts {
    pub const FILE: &'static str = "contacts.ron";

    pub fn load(root: &std::path::Path) -> Self {
        cadrs_fs::read_to_string(root.join(Self::FILE))
            .ok()
            .and_then(|t| ron::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, root: &std::path::Path) -> std::io::Result<()> {
        let text =
            ron::ser::to_string_pretty(self, Default::default()).map_err(std::io::Error::other)?;
        cadrs_fs::create_dir_all(root)?;
        let path = root.join(Self::FILE);
        let tmp = path.with_extension("tmp");
        cadrs_fs::write(&tmp, text)?;
        cadrs_fs::rename(&tmp, path)
    }

    /// Adds or renames `id`; true if anything changed. The most recent comes first.
    pub fn note(&mut self, id: &str, name: &str) -> bool {
        if self.0.first().is_some_and(|c| c.id == id && c.name == name) {
            return false;
        }
        self.0.retain(|c| c.id != id);
        self.0.insert(
            0,
            Contact {
                id: id.to_string(),
                name: name.to_string(),
            },
        );
        true
    }

    pub fn name(&self, id: &str) -> Option<&str> {
        self.0.iter().find(|c| c.id == id).map(|c| c.name.as_str())
    }
}

/// An imported file a document uses ([`crate::blobs`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Blob {
    pub hash: String,
    pub ext: String,
    #[serde(with = "serde_bytes_vec")]
    pub bytes: Vec<u8>,
}

/// A whole document as one peer sends it to another: after change `seq`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub seq: u64,
    pub document: Document,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blobs: Vec<Blob>,
}

impl Snapshot {
    /// `doc` after change `seq`, with the imported files it uses (from the cache, or else from
    /// the store's copy of the document).
    pub fn of(doc: &Document, seq: u64, store: &Store) -> Result<Self, String> {
        let dir = store.blobs_dir(doc.id);
        let blobs = blobs::used_by(doc)
            .into_iter()
            .map(|(hash, ext)| {
                let bytes = blobs::bytes_for(&hash, &dir)?;
                Ok(Blob { hash, ext, bytes })
            })
            .collect::<Result<_, String>>()?;
        Ok(Self {
            seq,
            document: doc.clone(),
            blobs,
        })
    }

    /// The document, with its files put in the cache (checked against their hashes first: a
    /// peer's bytes are not trusted).
    pub fn install(self) -> Result<Document, String> {
        for b in &self.blobs {
            if blobs::hash_of(&b.bytes) != b.hash {
                return Err(format!("Asset {} failed validation", b.hash));
            }
        }
        for b in self.blobs {
            blobs::insert_known(&b.hash, b.bytes);
        }
        Ok(self.document)
    }
}

/// `x` in its wire form.
pub fn pack<T: Serialize>(x: &T) -> Result<Vec<u8>, String> {
    let raw = rmp_serde::to_vec_named(x).map_err(|e| e.to_string())?;
    let mut z = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
    z.write_all(&raw).map_err(|e| e.to_string())?;
    z.finish().map_err(|e| e.to_string())
}

/// The value [`pack`] made, refusing anything over [`MAX_UNPACKED`].
pub fn unpack<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    let mut raw = Vec::new();
    flate2::read::DeflateDecoder::new(bytes)
        .take(MAX_UNPACKED + 1)
        .read_to_end(&mut raw)
        .map_err(|e| e.to_string())?;
    if raw.len() as u64 > MAX_UNPACKED {
        return Err("message too large".into());
    }
    rmp_serde::from_slice(&raw).map_err(|e| e.to_string())
}

/// Bytes as MessagePack's binary type rather than an array of numbers.
mod serde_bytes_vec {
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(v)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = Vec<u8>;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("bytes")
            }
            fn visit_bytes<E>(self, v: &[u8]) -> Result<Vec<u8>, E> {
                Ok(v.to_vec())
            }
            fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Vec<u8>, E> {
                Ok(v)
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut a: A,
            ) -> Result<Vec<u8>, A::Error> {
                let mut out = Vec::new();
                while let Some(b) = a.next_element()? {
                    out.push(b);
                }
                Ok(out)
            }
        }
        d.deserialize_byte_buf(V)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snapshot_round_trips_through_its_wire_form() {
        let mut doc = Document::new("Bracket");
        doc.name = "Bracket, shared".into();
        let hash = blobs::insert(b"solid cube".to_vec());
        let snap = Snapshot {
            seq: 7,
            document: doc.clone(),
            blobs: vec![Blob {
                hash: hash.clone(),
                ext: "stl".into(),
                bytes: b"solid cube".to_vec(),
            }],
        };
        let back: Snapshot = unpack(&pack(&snap).unwrap()).unwrap();
        assert_eq!(back, snap);
        assert_eq!(back.install().unwrap(), doc);
    }

    #[test]
    fn a_blob_that_does_not_match_its_hash_is_refused() {
        let snap = Snapshot {
            seq: 0,
            document: Document::new("x"),
            blobs: vec![Blob {
                hash: blobs::hash_of(b"one"),
                ext: "step".into(),
                bytes: b"two".to_vec(),
            }],
        };
        assert!(snap.install().is_err());
    }

    #[test]
    fn a_member_permission_wins_over_the_link() {
        let mut s = Sharing::new([1; 32]);
        assert_eq!(s.permission("bob"), None);
        s.link = Some(Permission::View);
        assert_eq!(s.permission("bob"), Some(Permission::View));
        // A link exports only when allowed, and never reshares.
        let link = s.access("bob").unwrap().rights;
        assert!(!link.export && !link.share && link.copy);
        s.members.push(Member {
            id: "bob".into(),
            name: "Bob".into(),
            permission: Permission::Edit,
            rights: None,
            delivered: None,
        });
        assert_eq!(s.permission("bob"), Some(Permission::Edit));
        assert!(s.access("bob").unwrap().rights.share);
        assert_eq!(s.permission("eve"), Some(Permission::View));
    }

    #[test]
    fn contacts_keep_the_latest_first_and_round_trip() {
        let dir = std::env::temp_dir().join(format!("cadrs-contacts-{}", uuid::Uuid::new_v4()));
        let mut c = Contacts::default();
        assert!(c.note("a", "Ann"));
        assert!(c.note("b", "Bo"));
        assert!(!c.note("b", "Bo"));
        assert!(c.note("a", "Ann B."));
        assert_eq!(c.0[0].name, "Ann B.");
        assert_eq!(c.0.len(), 2);
        c.save(&dir).unwrap();
        assert_eq!(Contacts::load(&dir), c);
        let _ = cadrs_fs::remove_dir_all(dir);
    }

    #[test]
    fn garbage_does_not_unpack() {
        assert!(unpack::<Snapshot>(b"not deflate at all").is_err());
    }
}
