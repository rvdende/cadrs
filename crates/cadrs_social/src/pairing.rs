//! Pairing a new device (a phone) with an account by scanning a QR code.
//!
//! 1. A device that holds the account key shows a [`PairingOffer`] as a QR code: a one-time
//!    secret, the account and where to reach this device, valid for a few minutes.
//! 2. The new device makes its own key and answers with a [`PairingRequest`], signed by that key
//!    and proven with the secret, so only someone who saw the QR code can ask.
//! 3. Both screens show a six-digit code made from the secret and the new device's key. The
//!    owner confirms they match: someone who photographed the QR code and asked first would show a
//!    different code on the account's screen than on the owner's phone.
//! 4. The account signs a [`DeviceCert`] for the device; the device checks it was signed by the
//!    account the QR code named.
//!
//! The transport is the caller's (the offer carries an opaque address); this module only makes
//! and checks the messages.

use serde::{Deserialize, Serialize};

use crate::keys::{self, Account, AccountId, DeviceCert, DeviceId, SecretKey, Signature, Signed};

const REQUEST_CONTEXT: &str = "cadrs pairing request 1";
/// What a QR code's text starts with.
pub const PREFIX: &str = "cadrs-pair:";
/// How long an offer stays valid, in milliseconds.
pub const VALID_FOR: u64 = 5 * 60 * 1000;

/// What the QR code carries.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairingOffer {
    pub secret: [u8; 32],
    pub account: AccountId,
    /// How to reach the offering device (an encoded network address).
    pub address: Vec<u8>,
    /// Milliseconds since the Unix epoch.
    pub expires: u64,
}

/// Why pairing failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// Not a pairing code.
    NotAnOffer,
    Expired,
    /// The request doesn't prove it saw this offer's secret.
    WrongSecret,
    /// This offer was already used.
    Used,
    Signature(keys::Error),
    /// The certificate isn't from the account the QR code named, or isn't for this device.
    WrongCertificate,
}

impl PairingOffer {
    pub fn new(account: AccountId, address: Vec<u8>, now: u64) -> Self {
        Self {
            secret: crate::random32(),
            account,
            address,
            expires: now + VALID_FOR,
        }
    }

    /// The text for the QR code.
    pub fn to_text(&self) -> String {
        let bytes = postcard::to_allocvec(self).expect("offers encode");
        format!(
            "{PREFIX}{}",
            data_encoding::BASE32_NOPAD.encode(&bytes).to_lowercase()
        )
    }

    pub fn from_text(text: &str) -> Result<Self, Error> {
        let body = text.trim().strip_prefix(PREFIX).ok_or(Error::NotAnOffer)?;
        let bytes = data_encoding::BASE32_NOPAD
            .decode(body.to_uppercase().as_bytes())
            .map_err(|_| Error::NotAnOffer)?;
        postcard::from_bytes(&bytes).map_err(|_| Error::NotAnOffer)
    }

    /// The new device's answer.
    pub fn request(
        &self,
        device: &SecretKey,
        name: &str,
        now: u64,
    ) -> Result<PairingRequest, Error> {
        if now > self.expires {
            return Err(Error::Expired);
        }
        let proof = proof(&self.secret, &device.public(), name);
        let body = RequestBody {
            device: device.public(),
            name: name.to_owned(),
            proof,
        };
        let bytes = postcard::to_allocvec(&body).expect("requests encode");
        Ok(PairingRequest {
            signature: device.sign(REQUEST_CONTEXT, &bytes),
            body,
        })
    }

    /// The code both screens show.
    pub fn code(&self, device: &DeviceId) -> String {
        let hash = blake3::keyed_hash(&self.secret, &device.0);
        let n = u32::from_le_bytes(hash.as_bytes()[..4].try_into().expect("4 bytes"));
        format!("{:06}", n % 1_000_000)
    }

    /// The new device checks the certificate it got back.
    pub fn accept(&self, device: &DeviceId, cert: &Signed<DeviceCert>) -> Result<(), Error> {
        let value = cert.verify().map_err(Error::Signature)?;
        if cert.signer != self.account || value.account != self.account || value.device != *device {
            return Err(Error::WrongCertificate);
        }
        Ok(())
    }
}

fn proof(secret: &[u8; 32], device: &DeviceId, name: &str) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new_keyed(secret);
    hasher.update(&device.0);
    hasher.update(name.as_bytes());
    *hasher.finalize().as_bytes()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct RequestBody {
    device: DeviceId,
    name: String,
    proof: [u8; 32],
}

/// A new device asks to join the account.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairingRequest {
    body: RequestBody,
    signature: Signature,
}

impl PairingRequest {
    pub fn device(&self) -> DeviceId {
        self.body.device
    }

    pub fn name(&self) -> &str {
        &self.body.name
    }
}

/// The account's side of one offer: checks requests and, once the owner confirms the code,
/// certifies the device. One device per offer.
#[derive(Debug)]
pub struct Pairing {
    pub offer: PairingOffer,
    used: bool,
}

impl Pairing {
    pub fn new(offer: PairingOffer) -> Self {
        Self { offer, used: false }
    }

    /// Checks a request; on success the code to show the owner.
    pub fn check(&self, request: &PairingRequest, now: u64) -> Result<String, Error> {
        if self.used {
            return Err(Error::Used);
        }
        if now > self.offer.expires {
            return Err(Error::Expired);
        }
        let body = &request.body;
        let bytes = postcard::to_allocvec(body).expect("requests encode");
        keys::verify(&body.device, REQUEST_CONTEXT, &bytes, &request.signature)
            .map_err(Error::Signature)?;
        if proof(&self.offer.secret, &body.device, &body.name) != body.proof {
            return Err(Error::WrongSecret);
        }
        Ok(self.offer.code(&body.device))
    }

    /// The owner confirmed the codes match: certify the device.
    pub fn confirm(
        &mut self,
        account: &Account,
        request: &PairingRequest,
        now: u64,
    ) -> Result<Signed<DeviceCert>, Error> {
        self.check(request, now)?;
        self.used = true;
        Ok(account.certify(request.body.device, &request.body.name, now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::Devices;

    #[test]
    fn a_phone_pairs() {
        let account = Account::generate();
        let offer = PairingOffer::new(account.id(), b"desktop".to_vec(), 1_000);
        let text = offer.to_text();
        assert!(text.starts_with(PREFIX));
        assert_eq!(text, text.to_lowercase());

        // The phone scans it.
        let scanned = PairingOffer::from_text(&text).unwrap();
        assert_eq!(scanned, offer);
        let phone = SecretKey::generate();
        let request = scanned.request(&phone, "phone", 2_000).unwrap();

        let mut pairing = Pairing::new(offer);
        let code = pairing.check(&request, 2_000).unwrap();
        assert_eq!(code, scanned.code(&phone.public()));
        assert_eq!(code.len(), 6);
        let cert = pairing.confirm(&account, &request, 2_000).unwrap();
        scanned.accept(&phone.public(), &cert).unwrap();

        let mut devices = Devices::default();
        devices.add_cert(&cert).unwrap();
        assert_eq!(
            devices.account_of(&phone.public(), 2_000),
            Some(account.id())
        );

        // An offer pairs one device.
        assert_eq!(pairing.confirm(&account, &request, 2_000), Err(Error::Used));
    }

    #[test]
    fn requests_need_the_secret_and_a_fresh_offer() {
        let account = Account::generate();
        let offer = PairingOffer::new(account.id(), vec![], 1_000);
        let phone = SecretKey::generate();
        let pairing = Pairing::new(offer.clone());

        let mut guessed = offer.clone();
        guessed.secret = [0; 32];
        let request = guessed.request(&phone, "phone", 1_000).unwrap();
        assert_eq!(pairing.check(&request, 1_000), Err(Error::WrongSecret));

        let request = offer.request(&phone, "phone", 1_000).unwrap();
        assert_eq!(
            pairing.check(&request, 1_000 + VALID_FOR + 1),
            Err(Error::Expired)
        );
        assert_eq!(
            offer.request(&phone, "phone", 1_000 + VALID_FOR + 1),
            Err(Error::Expired)
        );
    }

    #[test]
    fn someone_who_photographed_the_code_shows_a_different_code() {
        let account = Account::generate();
        let offer = PairingOffer::new(account.id(), vec![], 1_000);
        let phone = SecretKey::generate();
        let snooper = SecretKey::generate();
        assert_ne!(offer.code(&phone.public()), offer.code(&snooper.public()));
    }

    #[test]
    fn a_certificate_from_another_account_is_refused() {
        let account = Account::generate();
        let other = Account::generate();
        let offer = PairingOffer::new(account.id(), vec![], 1_000);
        let phone = SecretKey::generate();
        let cert = other.certify(phone.public(), "phone", 1_000);
        assert_eq!(
            offer.accept(&phone.public(), &cert),
            Err(Error::WrongCertificate)
        );
        let cert = account.certify(SecretKey::generate().public(), "phone", 1_000);
        assert_eq!(
            offer.accept(&phone.public(), &cert),
            Err(Error::WrongCertificate)
        );
    }

    #[test]
    fn garbage_is_not_an_offer() {
        assert_eq!(PairingOffer::from_text("hello"), Err(Error::NotAnOffer));
        assert_eq!(
            PairingOffer::from_text("cadrs-pair:!!"),
            Err(Error::NotAnOffer)
        );
    }
}
