//! The keys a device keeps on disk: its own key, its account's key (on devices trusted with it)
//! and the certificate that ties the two.
//!
//! The device key is the identity's existing key file (`identity.key`, 32 raw bytes: the same
//! key the network uses), so a device's id is its cadrs ID. Beside it:
//!
//! - `<key>.account`: the account's secret key. The first device of an account makes it; a
//!   paired device doesn't get it.
//! - `<key>.cert`: the account's certificate for this device.
//!
//! Files go through `cadrs_fs`, so this works in a page too.

use std::io;
use std::path::{Path, PathBuf};

use crate::keys::{Account, AccountId, DeviceCert, DeviceId, SecretKey, Signed};

/// A device's keys.
#[derive(Clone, Debug)]
pub struct DeviceKeys {
    pub device: SecretKey,
    /// The account's key, if this device holds it.
    pub account: Option<Account>,
    /// The account's certificate for this device.
    pub cert: Signed<DeviceCert>,
}

impl DeviceKeys {
    pub fn device_id(&self) -> DeviceId {
        self.device.public()
    }

    pub fn account_id(&self) -> AccountId {
        self.cert.value.account
    }

    fn account_path(key: &Path) -> PathBuf {
        sibling(key, "account")
    }

    fn cert_path(key: &Path) -> PathBuf {
        sibling(key, "cert")
    }

    /// Loads the keys beside the device key file `key`. What is missing is made: the device key
    /// (in the network's format), and, unless the device was paired, an account of its own
    /// that certifies it as `name`.
    pub fn load_or_create(key: &Path, name: &str, now: u64) -> io::Result<Self> {
        let device = match cadrs_fs::read(key) {
            Ok(bytes) => {
                let bytes: [u8; 32] = bytes.as_slice().try_into().map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("{} is not a 32-byte key", key.display()),
                    )
                })?;
                SecretKey::from_bytes(&bytes)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let device = SecretKey::generate();
                write_secret(key, &device.to_bytes())?;
                device
            }
            Err(e) => return Err(e),
        };
        let account = match cadrs_fs::read(Self::account_path(key)) {
            Ok(bytes) => {
                let bytes: [u8; 32] = bytes.as_slice().try_into().map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "not a 32-byte account key")
                })?;
                Some(Account::from_key(SecretKey::from_bytes(&bytes)))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        let cert = match cadrs_fs::read(Self::cert_path(key)) {
            Ok(bytes) => postcard::from_bytes::<Signed<DeviceCert>>(&bytes)
                .ok()
                .filter(|cert| {
                    cert.verify().is_ok()
                        && cert.value.device == device.public()
                        && account
                            .as_ref()
                            .is_none_or(|a| a.id() == cert.value.account)
                }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        let (account, cert) = match (account, cert) {
            (account, Some(cert)) => (account, cert),
            (Some(account), None) => {
                let cert = account.certify(device.public(), name, now);
                write_secret(&Self::cert_path(key), &encode(&cert))?;
                (Some(account), cert)
            }
            (None, None) => {
                let account = Account::generate();
                write_secret(&Self::account_path(key), &account.key().to_bytes())?;
                let cert = account.certify(device.public(), name, now);
                write_secret(&Self::cert_path(key), &encode(&cert))?;
                (Some(account), cert)
            }
        };
        Ok(Self {
            device,
            account,
            cert,
        })
    }

    /// Joins another account: keeps the certificate a pairing brought, and drops this device's
    /// own account key (its account had only this device). Refuses a certificate that isn't
    /// this device's.
    pub fn join(&mut self, key: &Path, cert: Signed<DeviceCert>) -> io::Result<()> {
        if cert.verify().is_err() || cert.value.device != self.device.public() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the certificate is not for this device",
            ));
        }
        write_secret(&Self::cert_path(key), &encode(&cert))?;
        if self
            .account
            .as_ref()
            .is_some_and(|a| a.id() != cert.value.account)
        {
            let _ = cadrs_fs::remove_file(Self::account_path(key));
            self.account = None;
        }
        self.cert = cert;
        Ok(())
    }
}

fn encode(cert: &Signed<DeviceCert>) -> Vec<u8> {
    postcard::to_allocvec(cert).expect("certificates encode")
}

fn sibling(key: &Path, extension: &str) -> PathBuf {
    let mut name = key.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(extension);
    key.with_file_name(name)
}

/// Writes a secret: to a temporary name then renamed, readable by its owner only.
fn write_secret(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        cadrs_fs::create_dir_all(parent)?;
    }
    let tmp = sibling(path, "tmp");
    cadrs_fs::write(&tmp, bytes)?;
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    cadrs_fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("cadrs-keystore-{name}-{}", std::process::id()));
        let _ = cadrs_fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_first_device_makes_its_account_and_keeps_it() {
        let dir = dir("first");
        let key = dir.join("identity.key");
        let first = DeviceKeys::load_or_create(&key, "laptop", 1).unwrap();
        assert!(first.account.is_some());
        assert_eq!(cadrs_fs::read(&key).unwrap().len(), 32);
        let again = DeviceKeys::load_or_create(&key, "laptop", 2).unwrap();
        assert_eq!(again.device_id(), first.device_id());
        assert_eq!(again.account_id(), first.account_id());
        assert_eq!(again.cert, first.cert);
        let _ = cadrs_fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_existing_network_key_keeps_its_id() {
        let dir = dir("existing");
        let key = dir.join("identity.key");
        cadrs_fs::create_dir_all(&dir).unwrap();
        cadrs_fs::write(&key, [7u8; 32]).unwrap();
        let keys = DeviceKeys::load_or_create(&key, "laptop", 1).unwrap();
        assert_eq!(keys.device_id(), SecretKey::from_bytes(&[7; 32]).public());
        let _ = cadrs_fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_paired_phone_joins_the_account() {
        let dir = dir("paired");
        let laptop = DeviceKeys::load_or_create(&dir.join("laptop.key"), "laptop", 1).unwrap();
        let phone_key = dir.join("phone.key");
        let mut phone = DeviceKeys::load_or_create(&phone_key, "phone", 1).unwrap();
        assert_ne!(phone.account_id(), laptop.account_id());

        let cert = laptop
            .account
            .as_ref()
            .unwrap()
            .certify(phone.device_id(), "phone", 2);
        phone.join(&phone_key, cert).unwrap();
        assert!(phone.account.is_none());
        let reloaded = DeviceKeys::load_or_create(&phone_key, "phone", 3).unwrap();
        assert_eq!(reloaded.account_id(), laptop.account_id());
        assert!(reloaded.account.is_none());

        // Someone else's certificate is refused.
        let other = laptop
            .account
            .as_ref()
            .unwrap()
            .certify(laptop.device_id(), "x", 2);
        assert!(phone.join(&phone_key, other).is_err());
        let _ = cadrs_fs::remove_dir_all(&dir);
    }
}
