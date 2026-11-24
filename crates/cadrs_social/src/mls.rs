//! End-to-end encryption for groups of devices, with MLS (RFC 9420, through openmls).
//!
//! Every organization document, request-for-quote thread, direct conversation and channel is one
//! MLS group of devices. A device signs in MLS with its own device key ([`crate::keys`]), and its
//! MLS identity is its device id, so who is in a group can be checked against device
//! certificates.
//!
//! **Commits without a server.** MLS needs the group's commits (adding or removing someone) in
//! one order. Here they travel as entries of the group's log ([`crate::log`]), which every peer
//! orders the same way, and each peer hands them to [`Group::receive`] in that order. The first
//! commit of an epoch wins; a later commit for an epoch that has passed is [`Received::Stale`].
//! If ours is the one that lost, we drop it ([`Received::OursLost`]) and the caller makes it
//! again on the new epoch.
//!
//! The group's state lives in memory for now; keeping it on disk comes with the app's use of it.

use openmls::prelude::tls_codec::{Deserialize as _, Serialize as _};
use openmls::prelude::*;
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::OpenMlsRustCrypto;

use crate::keys::{DeviceId, PublicKey, SecretKey};

const CIPHERSUITE: Ciphersuite = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;
/// Messages sent in an epoch still open this many epochs later (they cross commits in the log).
const PAST_EPOCHS: usize = 16;

/// Why an MLS step failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

fn err(e: impl std::fmt::Debug) -> Error {
    Error(format!("{e:?}"))
}

/// One device's MLS side: its keys, and the store of its groups' secrets.
pub struct Member {
    provider: OpenMlsRustCrypto,
    signer: SignatureKeyPair,
    credential: CredentialWithKey,
    device: DeviceId,
}

impl Member {
    /// The MLS side of the device with key `device`.
    pub fn new(device: &SecretKey) -> Self {
        let public = device.public();
        let signer = SignatureKeyPair::from_raw(
            SignatureScheme::ED25519,
            device.to_bytes().to_vec(),
            public.0.to_vec(),
        );
        let credential = CredentialWithKey {
            credential: BasicCredential::new(public.0.to_vec()).into(),
            signature_key: public.0.to_vec().into(),
        };
        Self {
            provider: OpenMlsRustCrypto::default(),
            signer,
            credential,
            device: public,
        }
    }

    pub fn device(&self) -> DeviceId {
        self.device
    }

    /// A key package: what someone needs to add this device to a group. Each is used once.
    pub fn key_package(&self) -> Result<Vec<u8>, Error> {
        let bundle = KeyPackage::builder()
            .build(
                CIPHERSUITE,
                &self.provider,
                &self.signer,
                self.credential.clone(),
            )
            .map_err(err)?;
        bundle.key_package().tls_serialize_detached().map_err(err)
    }
}

/// What came of a message handed to [`Group::receive`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Received {
    /// A message, and the device that sent it.
    Message { from: DeviceId, data: Vec<u8> },
    /// A commit, now applied: the group is at a new epoch.
    Applied,
    /// A commit for an epoch already past: another commit came first. Ignored.
    Stale,
    /// Another commit came before ours; ours is dropped and the other applied. Make ours again.
    OursLost,
}

/// One MLS group as one device sees it.
pub struct Group {
    group: MlsGroup,
    /// Our commit waiting for its place in the log.
    pending: Option<Vec<u8>>,
}

fn join_config() -> MlsGroupJoinConfig {
    MlsGroupJoinConfig::builder()
        .use_ratchet_tree_extension(true)
        .max_past_epochs(PAST_EPOCHS)
        .build()
}

fn device_of(credential: &Credential) -> Option<DeviceId> {
    let bytes: [u8; 32] = credential.serialized_content().try_into().ok()?;
    Some(PublicKey(bytes))
}

impl Group {
    /// A new group, `id` naming it (a log id), with `member` its only device.
    pub fn create(member: &Member, id: &[u8]) -> Result<Self, Error> {
        let config = MlsGroupCreateConfig::builder()
            .ciphersuite(CIPHERSUITE)
            .use_ratchet_tree_extension(true)
            .max_past_epochs(PAST_EPOCHS)
            .build();
        let group = MlsGroup::new_with_group_id(
            &member.provider,
            &member.signer,
            &config,
            GroupId::from_slice(id),
            member.credential.clone(),
        )
        .map_err(err)?;
        Ok(Self {
            group,
            pending: None,
        })
    }

    /// Joins a group from the welcome a member made when adding us.
    pub fn join(member: &Member, welcome: &[u8]) -> Result<Self, Error> {
        let message = MlsMessageIn::tls_deserialize_exact(welcome).map_err(err)?;
        let MlsMessageBodyIn::Welcome(welcome) = message.extract() else {
            return Err(Error("not a welcome".into()));
        };
        let group =
            StagedWelcome::new_from_welcome(&member.provider, &join_config(), welcome, None)
                .map_err(err)?
                .into_group(&member.provider)
                .map_err(err)?;
        Ok(Self {
            group,
            pending: None,
        })
    }

    pub fn epoch(&self) -> u64 {
        self.group.epoch().as_u64()
    }

    /// The devices in the group.
    pub fn devices(&self) -> Vec<DeviceId> {
        let mut devices: Vec<DeviceId> = self
            .group
            .members()
            .filter_map(|m| device_of(&m.credential))
            .collect();
        devices.sort();
        devices
    }

    /// Our commit adding devices by their key packages, and the welcome for them. The commit
    /// goes into the log; the welcome to the new devices once the commit has applied.
    pub fn add(
        &mut self,
        member: &Member,
        key_packages: &[Vec<u8>],
    ) -> Result<(Vec<u8>, Vec<u8>), Error> {
        let packages = key_packages
            .iter()
            .map(|bytes| {
                KeyPackageIn::tls_deserialize_exact(bytes.as_slice())
                    .map_err(err)?
                    .validate(member.provider.crypto(), ProtocolVersion::Mls10)
                    .map_err(err)
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.drop_pending(member)?;
        let (commit, welcome, _) = self
            .group
            .add_members(&member.provider, &member.signer, &packages)
            .map_err(err)?;
        let commit = commit.to_bytes().map_err(err)?;
        self.pending = Some(commit.clone());
        Ok((commit, welcome.to_bytes().map_err(err)?))
    }

    /// Our commit removing devices.
    pub fn remove(&mut self, member: &Member, devices: &[DeviceId]) -> Result<Vec<u8>, Error> {
        let leaves: Vec<LeafNodeIndex> = self
            .group
            .members()
            .filter(|m| device_of(&m.credential).is_some_and(|d| devices.contains(&d)))
            .map(|m| m.index)
            .collect();
        if leaves.is_empty() {
            return Err(Error("none of them are in the group".into()));
        }
        self.drop_pending(member)?;
        let (commit, _, _) = self
            .group
            .remove_members(&member.provider, &member.signer, &leaves)
            .map_err(err)?;
        let commit = commit.to_bytes().map_err(err)?;
        self.pending = Some(commit.clone());
        Ok(commit)
    }

    fn drop_pending(&mut self, member: &Member) -> Result<(), Error> {
        if self.pending.take().is_some() {
            self.group
                .clear_pending_commit(member.provider.storage())
                .map_err(err)?;
        }
        Ok(())
    }

    /// Encrypts a message for the group.
    pub fn encrypt(&mut self, member: &Member, data: &[u8]) -> Result<Vec<u8>, Error> {
        self.group
            .create_message(&member.provider, &member.signer, data)
            .map_err(err)?
            .to_bytes()
            .map_err(err)
    }

    /// Takes in a message or commit from the log, in the log's order (our own commits too).
    pub fn receive(&mut self, member: &Member, bytes: &[u8]) -> Result<Received, Error> {
        if self.pending.as_deref() == Some(bytes) {
            self.pending = None;
            self.group
                .merge_pending_commit(&member.provider)
                .map_err(err)?;
            return Ok(Received::Applied);
        }
        let message = MlsMessageIn::tls_deserialize_exact(bytes).map_err(err)?;
        let message = message.try_into_protocol_message().map_err(err)?;
        let is_commit = message.is_handshake_message();
        if is_commit && message.epoch() < self.group.epoch() {
            return Ok(Received::Stale);
        }
        let ours_lost = is_commit && self.pending.is_some();
        if ours_lost {
            self.drop_pending(member)?;
        }
        let processed = self
            .group
            .process_message(&member.provider, message)
            .map_err(err)?;
        let from = processed.credential().clone();
        match processed.into_content() {
            ProcessedMessageContent::ApplicationMessage(message) => Ok(Received::Message {
                from: device_of(&from).ok_or_else(|| Error("not a device".into()))?,
                data: message.into_bytes(),
            }),
            ProcessedMessageContent::StagedCommitMessage(commit) => {
                self.group
                    .merge_staged_commit(&member.provider, *commit)
                    .map_err(err)?;
                Ok(if ours_lost {
                    Received::OursLost
                } else {
                    Received::Applied
                })
            }
            _ => Err(Error("proposals travel inside commits here".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member() -> Member {
        Member::new(&SecretKey::generate())
    }

    /// Delivers `bytes` to every group in `groups`, as the log would, in order.
    fn deliver(groups: &mut [(&Member, &mut Group)], bytes: &[u8]) -> Vec<Received> {
        groups
            .iter_mut()
            .map(|(member, group)| group.receive(member, bytes).unwrap())
            .collect()
    }

    #[test]
    fn a_shop_and_a_designer_talk() {
        let shop = member();
        let designer = member();
        let mut shop_group = Group::create(&shop, b"rfq 1").unwrap();
        let (commit, welcome) = shop_group
            .add(&shop, &[designer.key_package().unwrap()])
            .unwrap();
        assert_eq!(
            shop_group.receive(&shop, &commit).unwrap(),
            Received::Applied
        );
        let mut designer_group = Group::join(&designer, &welcome).unwrap();
        assert_eq!(shop_group.devices(), designer_group.devices());
        assert_eq!(shop_group.epoch(), designer_group.epoch());

        let sealed = designer_group
            .encrypt(&designer, b"6 off in 10 mm mild steel")
            .unwrap();
        assert!(!sealed.windows(5).any(|w| w == b"steel"));
        assert_eq!(
            shop_group.receive(&shop, &sealed).unwrap(),
            Received::Message {
                from: designer.device(),
                data: b"6 off in 10 mm mild steel".to_vec()
            }
        );
    }

    #[test]
    fn a_removed_device_reads_nothing_after() {
        let (a, b, c) = (member(), member(), member());
        let mut ga = Group::create(&a, b"org").unwrap();
        let (commit, welcome) = ga
            .add(&a, &[b.key_package().unwrap(), c.key_package().unwrap()])
            .unwrap();
        ga.receive(&a, &commit).unwrap();
        let mut gb = Group::join(&b, &welcome).unwrap();
        let mut gc = Group::join(&c, &welcome).unwrap();

        let commit = ga.remove(&a, &[c.device()]).unwrap();
        deliver(&mut [(&a, &mut ga), (&b, &mut gb)], &commit);
        assert_eq!(ga.devices().len(), 2);

        let secret = ga.encrypt(&a, b"new prices").unwrap();
        assert!(matches!(
            gb.receive(&b, &secret).unwrap(),
            Received::Message { .. }
        ));
        assert!(gc.receive(&c, &secret).is_err());
    }

    /// Two admins commit in the same epoch while apart. The log puts one first; every device
    /// applies that one, and the other's author learns it lost and makes it again.
    #[test]
    fn colliding_commits_resolve_the_same_way_everywhere() {
        let (a, b, x, y) = (member(), member(), member(), member());
        let mut ga = Group::create(&a, b"org").unwrap();
        let (commit, welcome) = ga.add(&a, &[b.key_package().unwrap()]).unwrap();
        ga.receive(&a, &commit).unwrap();
        let mut gb = Group::join(&b, &welcome).unwrap();

        // Both add someone at once.
        let (from_a, welcome_x) = ga.add(&a, &[x.key_package().unwrap()]).unwrap();
        let (from_b, _) = gb.add(&b, &[y.key_package().unwrap()]).unwrap();

        // The log orders a's first.
        let first = deliver(&mut [(&a, &mut ga), (&b, &mut gb)], &from_a);
        assert_eq!(first, vec![Received::Applied, Received::OursLost]);
        let second = deliver(&mut [(&a, &mut ga), (&b, &mut gb)], &from_b);
        assert_eq!(second, vec![Received::Stale, Received::Stale]);
        assert_eq!(ga.devices(), gb.devices());
        assert_eq!(ga.epoch(), gb.epoch());

        // x joins from a's welcome; b makes its commit again, on the new epoch.
        let mut gx = Group::join(&x, &welcome_x).unwrap();
        let (again, welcome_y) = gb.add(&b, &[y.key_package().unwrap()]).unwrap();
        let applied = deliver(&mut [(&a, &mut ga), (&b, &mut gb), (&x, &mut gx)], &again);
        assert_eq!(applied, vec![Received::Applied; 3]);
        let mut gy = Group::join(&y, &welcome_y).unwrap();
        assert_eq!(gy.devices().len(), 4);

        let hello = gy.encrypt(&y, b"hello").unwrap();
        for (m, g) in [(&a, &mut ga), (&b, &mut gb), (&x, &mut gx)] {
            assert!(matches!(
                g.receive(m, &hello).unwrap(),
                Received::Message { .. }
            ));
        }
    }

    #[test]
    fn a_message_from_before_a_commit_still_opens() {
        let (a, b, c) = (member(), member(), member());
        let mut ga = Group::create(&a, b"chat").unwrap();
        let (commit, welcome) = ga.add(&a, &[b.key_package().unwrap()]).unwrap();
        ga.receive(&a, &commit).unwrap();
        let mut gb = Group::join(&b, &welcome).unwrap();

        // b writes, then a's commit is ordered before b's message reaches a.
        let early = gb.encrypt(&b, b"early").unwrap();
        let (commit, _) = ga.add(&a, &[c.key_package().unwrap()]).unwrap();
        deliver(&mut [(&a, &mut ga), (&b, &mut gb)], &commit);
        assert!(matches!(
            ga.receive(&a, &early).unwrap(),
            Received::Message { .. }
        ));
    }
}
