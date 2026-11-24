//! How what cadrs keeps and sends is written, where it has to outlive the version that wrote it:
//! MessagePack with the fields' and variants' names.
//!
//! An organization's operations, the letters between clients and shops, the posts between
//! devices and the files the app keeps are read by other versions of cadrs than the one that
//! wrote them: a member who hasn't updated, a shop that has. Written with their names, a type
//! can grow:
//!
//! - **A new field** is given `#[serde(default)]`. Older data reads with the default; an older
//!   reader skips the field. So a new field must be one that is safe to ignore.
//! - **A new variant** (a new operation, a new kind of letter) is [`Error::Unknown`] to an older
//!   reader, which can then say "a newer cadrs wrote this" rather than "this is broken".
//! - **Never** rename or remove a field or a variant, or change what one means: write a new one.
//!
//! What is *signed as a value* ([`crate::keys::Signed`]: certificates, contact cards, receipts)
//! is not written this way: a signature needs one encoding and no other, so those are postcard
//! and their types are frozen, a changed one being a new statement with a new context. What is
//! signed and has to grow is signed as these bytes instead ([`crate::keys::SignedBytes`]).

use std::fmt;

use serde::Serialize;
use serde::de::DeserializeOwned;

/// Why bytes couldn't be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// A variant this version doesn't know: written by a newer cadrs.
    Unknown(String),
    /// Not what was expected at all.
    Malformed(String),
}

impl Error {
    /// Whether a newer cadrs wrote it (rather than it being broken).
    pub fn is_newer(&self) -> bool {
        matches!(self, Error::Unknown(_))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unknown(what) => write!(f, "written by a newer cadrs ({what})"),
            Error::Malformed(why) => write!(f, "can't be read ({why})"),
        }
    }
}

impl std::error::Error for Error {}

/// `value` as bytes.
pub fn encode<T: Serialize>(value: &T) -> Vec<u8> {
    rmp_serde::to_vec_named(value).expect("values encode")
}

/// The value in `bytes`.
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, Error> {
    rmp_serde::from_slice(bytes).map_err(|e| {
        let why = e.to_string();
        // serde's own message for a variant's name it has no arm for.
        if why.starts_with("unknown variant") {
            Error::Unknown(why)
        } else {
            Error::Malformed(why)
        }
    })
}

/// Reads a file [`save_file`] wrote; the default when there is none. One that is there but
/// can't be read (damaged, or a newer cadrs's) is moved aside as `<file>.unreadable`, so the
/// next save doesn't write an empty one over what it held. Every file a device keeps of its own
/// (contacts, requests, shops, a mailbox) is read this way.
pub fn load_file<T: serde::de::DeserializeOwned + Default>(path: &std::path::Path) -> T {
    let Ok(bytes) = cadrs_fs::read(path) else {
        return T::default();
    };
    decode(&bytes).unwrap_or_else(|_| {
        set_aside(path);
        T::default()
    })
}

/// Moves a file that can't be read out of the way, as `<file>.unreadable` (or, when one was
/// set aside before and is still there, `<file>.unreadable.2` and so on: none is written over).
pub fn set_aside(path: &std::path::Path) {
    let named = |n: u32| {
        let mut aside = path.as_os_str().to_owned();
        aside.push(".unreadable");
        if n > 1 {
            aside.push(format!(".{n}"));
        }
        std::path::PathBuf::from(aside)
    };
    let free = (1..=99).map(named).find(|aside| !cadrs_fs::exists(aside));
    // A hundred of them already: the oldest name is used again rather than losing this one.
    let _ = cadrs_fs::rename(path, free.unwrap_or_else(|| named(1)));
}

/// Writes `value` to `path` whole or not at all: to a file beside it first, then in its place,
/// so a crash halfway leaves what was there.
pub fn save_file<T: Serialize>(path: &std::path::Path, value: &T) -> std::io::Result<()> {
    write_whole(path, &encode(value))
}

/// As [`save_file`], for bytes.
pub fn write_whole(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        cadrs_fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    cadrs_fs::write(&tmp, bytes)?;
    cadrs_fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{Account, Signed};
    use serde::Deserialize;
    use std::collections::BTreeMap;

    #[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
    struct Profile1 {
        name: String,
        about: String,
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
    struct Profile2 {
        name: String,
        about: String,
        #[serde(default)]
        logo: Option<[u8; 32]>,
        #[serde(default)]
        links: Vec<String>,
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
    enum Op1 {
        Found { name: String, id: u128 },
        SetProfile(Profile1),
        Stock { material: u128, sheets: i64 },
    }

    /// The same operations a version later: a variant added first, fields added, the order
    /// changed.
    #[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
    enum Op2 {
        SetLocation {
            id: u128,
            name: Option<String>,
        },
        Stock {
            material: u128,
            sheets: i64,
            #[serde(default)]
            location: Option<u128>,
        },
        Found {
            name: String,
            id: u128,
        },
        SetProfile(Profile2),
    }

    #[test]
    fn older_data_reads_as_the_grown_type() {
        let id = u128::MAX - 5;
        assert_eq!(
            decode::<Op2>(&encode(&Op1::Found {
                name: "Cape Waterjet".into(),
                id
            })),
            Ok(Op2::Found {
                name: "Cape Waterjet".into(),
                id
            })
        );
        assert_eq!(
            decode::<Op2>(&encode(&Op1::SetProfile(Profile1 {
                name: "a".into(),
                about: "b".into()
            }))),
            Ok(Op2::SetProfile(Profile2 {
                name: "a".into(),
                about: "b".into(),
                logo: None,
                links: Vec::new()
            }))
        );
        assert_eq!(
            decode::<Op2>(&encode(&Op1::Stock {
                material: 7,
                sheets: -3
            })),
            Ok(Op2::Stock {
                material: 7,
                sheets: -3,
                location: None
            })
        );
    }

    #[test]
    fn an_older_reader_skips_new_fields_and_names_new_variants() {
        let grown = encode(&Op2::Stock {
            material: 7,
            sheets: 2,
            location: Some(9),
        });
        assert_eq!(
            decode::<Op1>(&grown),
            Ok(Op1::Stock {
                material: 7,
                sheets: 2
            })
        );
        let new = encode(&Op2::SetLocation { id: 1, name: None });
        let refused = decode::<Op1>(&new).unwrap_err();
        assert!(refused.is_newer(), "{refused:?}");
        // Broken bytes are not "newer".
        let broken = decode::<Op1>(&[0xc1, 0x00, 0xff]).unwrap_err();
        assert!(!broken.is_newer(), "{broken:?}");
    }

    /// The shapes the documents use: maps by a 128-bit id, signed statements inside, tuples.
    #[test]
    fn the_documents_shapes_round_trip() {
        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        struct Book {
            by_id: BTreeMap<u128, Profile1>,
            cert: Signed<crate::keys::DeviceCert>,
            chat: Vec<(bool, String, u64)>,
            x: f64,
            #[serde(with = "serde_bytes")]
            blob: Vec<u8>,
        }
        let account = Account::generate();
        let device = crate::keys::SecretKey::generate();
        let book = Book {
            by_id: BTreeMap::from([(
                u128::MAX - 1,
                Profile1 {
                    name: "n".into(),
                    about: "a".into(),
                },
            )]),
            cert: account.certify(device.public(), "bench box", 5),
            chat: vec![(true, "hi".into(), u64::MAX)],
            x: 0.1 + 0.2,
            blob: vec![0, 255, 7],
        };
        let back: Book = decode(&encode(&book)).unwrap();
        assert_eq!(back, book);
        // The statement inside still verifies: its signature is over its own encoding.
        assert!(back.cert.verify().is_ok());
    }

    /// A file that can't be read is kept, not written over.
    #[test]
    fn an_unreadable_file_is_set_aside() {
        let dir = std::env::temp_dir().join(format!("cadrs-codec-{}", std::process::id()));
        let _ = cadrs_fs::remove_dir_all(&dir);
        let path = dir.join("contacts.bin");
        assert_eq!(load_file::<Vec<String>>(&path), Vec::<String>::new());
        save_file(&path, &vec!["Alex".to_owned()]).unwrap();
        assert_eq!(load_file::<Vec<String>>(&path), ["Alex"]);
        assert!(!dir.join("contacts.bin.tmp").exists());
        // Damaged: read as none, and still there under another name after the next save.
        cadrs_fs::write(&path, [0xc1, 0x00, 0x17]).unwrap();
        assert_eq!(load_file::<Vec<String>>(&path), Vec::<String>::new());
        save_file(&path, &Vec::<String>::new()).unwrap();
        assert_eq!(
            cadrs_fs::read(dir.join("contacts.bin.unreadable")).unwrap(),
            [0xc1, 0x00, 0x17]
        );
        // A second one doesn't take the first's place.
        cadrs_fs::write(&path, [0xc1, 0x42]).unwrap();
        assert_eq!(load_file::<Vec<String>>(&path), Vec::<String>::new());
        assert_eq!(
            cadrs_fs::read(dir.join("contacts.bin.unreadable")).unwrap(),
            [0xc1, 0x00, 0x17]
        );
        assert_eq!(
            cadrs_fs::read(dir.join("contacts.bin.unreadable.2")).unwrap(),
            [0xc1, 0x42]
        );
        let _ = cadrs_fs::remove_dir_all(&dir);
    }
}
