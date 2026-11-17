//! Keys, signed statements, accounts and devices.
//!
//! A person or an organization is an **account**: a long-lived Ed25519 key that rarely signs
//! anything but certificates. Every app install, phone or shop box is a **device** with a key of
//! its own; the account signs a [`DeviceCert`] for it and can later sign a [`Revocation`] (a lost
//! phone). Others see one account; each device keeps its own network id, so two devices of one
//! account can be online at once.

use std::collections::HashMap;
use std::fmt;

use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// An Ed25519 public key: an account's or a device's id.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PublicKey(pub [u8; 32]);

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}…", &crate::hex(&self.0)[..8])
    }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&crate::hex(&self.0))
    }
}

/// An account: a person or an organization.
pub type AccountId = PublicKey;
/// One device of an account (also its network id).
pub type DeviceId = PublicKey;

/// An Ed25519 signature.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Signature(ed25519_dalek::Signature);

/// Why something signed was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The bytes are not a public key.
    BadKey,
    /// The signature does not match.
    BadSignature,
    /// The statement could not be encoded or decoded.
    Encoding,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Error::BadKey => "not a public key",
            Error::BadSignature => "the signature does not match",
            Error::Encoding => "could not encode or decode",
        })
    }
}

impl std::error::Error for Error {}

/// A secret Ed25519 key.
#[derive(Clone)]
pub struct SecretKey(SigningKey);

impl SecretKey {
    /// A new random key.
    pub fn generate() -> Self {
        Self::from_bytes(&crate::random32())
    }

    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        Self(SigningKey::from_bytes(bytes))
    }

    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    pub fn public(&self) -> PublicKey {
        PublicKey(self.0.verifying_key().to_bytes())
    }

    /// Signs `message` for `context`, so a signature made for one kind of statement can never
    /// pass as another.
    pub fn sign(&self, context: &str, message: &[u8]) -> Signature {
        Signature(self.0.sign(&domain(context, message)))
    }
}

impl fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretKey({:?})", self.public())
    }
}

/// Checks `signature` over `message` for `context` by `key`.
pub fn verify(
    key: &PublicKey,
    context: &str,
    message: &[u8],
    signature: &Signature,
) -> Result<(), Error> {
    let key = VerifyingKey::from_bytes(&key.0).map_err(|_| Error::BadKey)?;
    key.verify_strict(&domain(context, message), &signature.0)
        .map_err(|_| Error::BadSignature)
}

fn domain(context: &str, message: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(context.len() + 1 + message.len());
    bytes.extend_from_slice(context.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(message);
    bytes
}

/// A kind of statement someone signs; its context keeps one kind's signatures from passing as
/// another's.
pub trait Statement: Serialize + DeserializeOwned {
    const CONTEXT: &'static str;
}

/// A statement with the key that signed it and its signature.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Signed<T> {
    pub value: T,
    pub signer: PublicKey,
    pub signature: Signature,
}

impl<T: Statement> Signed<T> {
    pub fn new(key: &SecretKey, value: T) -> Self {
        let bytes = postcard::to_allocvec(&value).expect("statements encode");
        let signature = key.sign(T::CONTEXT, &bytes);
        Self {
            value,
            signer: key.public(),
            signature,
        }
    }

    /// The statement, if its signature holds.
    pub fn verify(&self) -> Result<&T, Error> {
        let bytes = postcard::to_allocvec(&self.value).map_err(|_| Error::Encoding)?;
        verify(&self.signer, T::CONTEXT, &bytes, &self.signature)?;
        Ok(&self.value)
    }
}

/// An account vouches for one of its devices.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceCert {
    pub account: AccountId,
    pub device: DeviceId,
    /// What the account calls it ("Rouan's phone").
    pub name: String,
    /// Milliseconds since the Unix epoch.
    pub issued: u64,
}

impl Statement for DeviceCert {
    const CONTEXT: &'static str = "cadrs device certificate 1";
}

/// An account withdraws a device (lost, sold, replaced). Entries the device signed before
/// `from` stay valid.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revocation {
    pub account: AccountId,
    pub device: DeviceId,
    /// Milliseconds since the Unix epoch.
    pub from: u64,
}

impl Statement for Revocation {
    const CONTEXT: &'static str = "cadrs device revocation 1";
}

/// An account's secret key, kept on the devices its owner trusts with it.
#[derive(Clone, Debug)]
pub struct Account {
    key: SecretKey,
}

impl Account {
    pub fn generate() -> Self {
        Self {
            key: SecretKey::generate(),
        }
    }

    pub fn from_key(key: SecretKey) -> Self {
        Self { key }
    }

    pub fn id(&self) -> AccountId {
        self.key.public()
    }

    pub fn key(&self) -> &SecretKey {
        &self.key
    }

    /// Vouches for `device`.
    pub fn certify(&self, device: DeviceId, name: &str, now: u64) -> Signed<DeviceCert> {
        Signed::new(
            &self.key,
            DeviceCert {
                account: self.id(),
                device,
                name: name.to_owned(),
                issued: now,
            },
        )
    }

    /// Withdraws `device` from `from` on.
    pub fn revoke(&self, device: DeviceId, from: u64) -> Signed<Revocation> {
        Signed::new(
            &self.key,
            Revocation {
                account: self.id(),
                device,
                from,
            },
        )
    }
}

/// What a peer knows of other accounts' devices: their certificates and revocations, each
/// checked as it arrives.
#[derive(Clone, Debug, Default)]
pub struct Devices {
    certs: HashMap<DeviceId, DeviceCert>,
    /// From when each device is withdrawn, by the account that withdrew it.
    revoked: HashMap<(DeviceId, AccountId), u64>,
}

impl Devices {
    /// Learns a certificate, if the account signed it.
    pub fn add_cert(&mut self, cert: &Signed<DeviceCert>) -> Result<(), Error> {
        let value = cert.verify()?;
        if cert.signer != value.account {
            return Err(Error::BadSignature);
        }
        self.certs.insert(value.device, value.clone());
        Ok(())
    }

    /// Learns a revocation, if the device's own account signed it.
    pub fn add_revocation(&mut self, revocation: &Signed<Revocation>) -> Result<(), Error> {
        let value = revocation.verify()?;
        if revocation.signer != value.account {
            return Err(Error::BadSignature);
        }
        let from = self
            .revoked
            .entry((value.device, value.account))
            .or_insert(value.from);
        *from = (*from).min(value.from);
        Ok(())
    }

    /// The account `device` signed for at `time` (milliseconds), if it was certified and not yet
    /// revoked then. A revocation only counts when its account is the one that certified the
    /// device.
    pub fn account_of(&self, device: &DeviceId, time: u64) -> Option<AccountId> {
        let cert = self.certs.get(device)?;
        if time < cert.issued {
            return None;
        }
        match self.revoked.get(&(*device, cert.account)) {
            Some(&from) if time >= from => None,
            _ => Some(cert.account),
        }
    }

    /// The devices of `account` that are not revoked.
    pub fn of(&self, account: &AccountId) -> Vec<&DeviceCert> {
        let mut devices: Vec<_> = self
            .certs
            .values()
            .filter(|cert| {
                &cert.account == account && !self.revoked.contains_key(&(cert.device, cert.account))
            })
            .collect();
        devices.sort_by_key(|cert| (cert.issued, cert.device));
        devices
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_are_bound_to_their_context() {
        let key = SecretKey::generate();
        let signature = key.sign("one", b"hello");
        assert!(verify(&key.public(), "one", b"hello", &signature).is_ok());
        assert_eq!(
            verify(&key.public(), "two", b"hello", &signature),
            Err(Error::BadSignature)
        );
        assert_eq!(
            verify(&key.public(), "one", b"hellp", &signature),
            Err(Error::BadSignature)
        );
    }

    #[test]
    fn keys_round_trip() {
        let key = SecretKey::generate();
        assert_eq!(
            SecretKey::from_bytes(&key.to_bytes()).public(),
            key.public()
        );
    }

    #[test]
    fn devices_are_certified_and_revoked() {
        let account = Account::generate();
        let phone = SecretKey::generate().public();
        let mut devices = Devices::default();
        devices
            .add_cert(&account.certify(phone, "phone", 1_000))
            .unwrap();
        assert_eq!(devices.account_of(&phone, 999), None);
        assert_eq!(devices.account_of(&phone, 1_000), Some(account.id()));
        devices
            .add_revocation(&account.revoke(phone, 5_000))
            .unwrap();
        assert_eq!(devices.account_of(&phone, 4_999), Some(account.id()));
        assert_eq!(devices.account_of(&phone, 5_000), None);
        assert!(devices.of(&account.id()).is_empty());
    }

    #[test]
    fn nobody_else_certifies_or_revokes_a_device() {
        let account = Account::generate();
        let thief = Account::generate();
        let phone = SecretKey::generate().public();
        let mut devices = Devices::default();
        // A certificate naming another account than its signer.
        let mut forged = thief.certify(phone, "phone", 0);
        forged.value.account = account.id();
        assert!(devices.add_cert(&forged).is_err());
        devices
            .add_cert(&account.certify(phone, "phone", 0))
            .unwrap();
        // The thief can sign a revocation of its own account's "device"...
        devices.add_revocation(&thief.revoke(phone, 0)).unwrap();
        // ...which does not count against the account that certified it.
        assert_eq!(devices.account_of(&phone, 10), Some(account.id()));
        assert_eq!(devices.of(&account.id()).len(), 1);
    }
}
