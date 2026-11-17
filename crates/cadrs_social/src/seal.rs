//! Sealing a message for one device, so a mailbox (or anyone else) holding it can't read it.
//!
//! A device's key is Ed25519; its X25519 twin (the same key on the Montgomery curve) takes part
//! in a Diffie-Hellman with a fresh key made for each message. The shared secret, through
//! HKDF-SHA256 with both public keys, keys ChaCha20-Poly1305. The sealed bytes are the fresh
//! public key, then the ciphertext. Only the device's own secret key opens them, and a change to
//! any byte is refused.
//!
//! Groups (an organization's members, a chat with several people) use MLS ([`crate::mls`]); this
//! is for store-and-forward of a message to one device.

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use curve25519_dalek::montgomery::MontgomeryPoint;
use ed25519_dalek::{SigningKey, VerifyingKey};
use hkdf::Hkdf;
use sha2::Sha256;

use crate::keys::{DeviceId, SecretKey};

const INFO: &[u8] = b"cadrs seal 1";

/// Why sealed bytes couldn't be opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// Not a device key.
    BadKey,
    /// Not for this device, or changed on the way.
    CannotOpen,
}

fn twin(device: &DeviceId) -> Result<MontgomeryPoint, Error> {
    let key = VerifyingKey::from_bytes(&device.0).map_err(|_| Error::BadKey)?;
    Ok(key.to_montgomery())
}

fn cipher(
    shared: &MontgomeryPoint,
    ephemeral: &MontgomeryPoint,
    to: &MontgomeryPoint,
) -> ChaCha20Poly1305 {
    let mut info = INFO.to_vec();
    info.extend_from_slice(ephemeral.as_bytes());
    info.extend_from_slice(to.as_bytes());
    let mut key = [0u8; 32];
    Hkdf::<Sha256>::new(None, shared.as_bytes())
        .expand(&info, &mut key)
        .expect("32 bytes is a valid length");
    ChaCha20Poly1305::new(Key::from_slice(&key))
}

/// Seals `message` for device `to`.
pub fn seal(to: &DeviceId, message: &[u8]) -> Result<Vec<u8>, Error> {
    let to = twin(to)?;
    let ephemeral_secret = crate::random32();
    let ephemeral = MontgomeryPoint::mul_base_clamped(ephemeral_secret);
    let shared = to.mul_clamped(ephemeral_secret);
    // Each message has its own key, so one nonce is enough.
    let sealed = cipher(&shared, &ephemeral, &to)
        .encrypt(Nonce::from_slice(&[0; 12]), message)
        .map_err(|_| Error::CannotOpen)?;
    let mut out = ephemeral.as_bytes().to_vec();
    out.extend(sealed);
    Ok(out)
}

/// Opens bytes sealed for the device whose key is `key`.
pub fn open(key: &SecretKey, sealed: &[u8]) -> Result<Vec<u8>, Error> {
    if sealed.len() < 32 {
        return Err(Error::CannotOpen);
    }
    let (ephemeral, ciphertext) = sealed.split_at(32);
    let ephemeral = MontgomeryPoint(ephemeral.try_into().expect("32 bytes"));
    let signing = SigningKey::from_bytes(&key.to_bytes());
    let secret = signing.to_scalar_bytes();
    let me = twin(&key.public())?;
    let shared = ephemeral.mul_clamped(secret);
    cipher(&shared, &ephemeral, &me)
        .decrypt(Nonce::from_slice(&[0; 12]), ciphertext)
        .map_err(|_| Error::CannotOpen)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_device_opens_it() {
        let shop_box = SecretKey::generate();
        let sealed = seal(&shop_box.public(), b"a request for a quote").unwrap();
        assert!(!sealed.windows(5).any(|w| w == b"quote"));
        assert_eq!(open(&shop_box, &sealed).unwrap(), b"a request for a quote");
        let someone = SecretKey::generate();
        assert_eq!(open(&someone, &sealed), Err(Error::CannotOpen));
    }

    #[test]
    fn a_changed_byte_is_refused() {
        let device = SecretKey::generate();
        let mut sealed = seal(&device.public(), b"hello").unwrap();
        let last = sealed.len() - 1;
        sealed[last] ^= 1;
        assert_eq!(open(&device, &sealed), Err(Error::CannotOpen));
        assert_eq!(open(&device, b"short"), Err(Error::CannotOpen));
    }

    #[test]
    fn each_seal_differs() {
        let device = SecretKey::generate();
        assert_ne!(
            seal(&device.public(), b"x").unwrap(),
            seal(&device.public(), b"x").unwrap()
        );
    }
}
