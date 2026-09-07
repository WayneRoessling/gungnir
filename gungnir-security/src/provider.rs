//! A key provider that actually holds keys (GAP-084, DN-22 amendment 1).
//!
//! `KeyProvider` was a trait with no implementor until 2026-09-05, so nothing could be
//! encrypted and nothing could obtain a TLS identity. This is the in-process
//! implementation the on-prem profile uses, and the shape a keystore- or
//! service-backed one takes.
//!
//! # The sealed form
//!
//! Amendment 1 (b): `<KeyId: purpose, version> || <96-bit nonce> || <ciphertext || tag>`.
//!
//! **The `KeyId` is on the ciphertext because rotation must not rewrite existing data.**
//! DN-22 §5 says a retired key still reads what it protected, and that is only possible
//! if the sealed bytes say which key protected them. Without the header, rotating would
//! either orphan every existing journal or force a re-encryption pass that rewrites an
//! append-only record -- which AP-08 forbids.
//!
//! The nonce is drawn fresh per operation. Reusing one under a single AES-GCM key is the
//! failure that turns the cipher from safe into catastrophic, so it is never derived from
//! anything a caller controls.
//!
//! # What this implementation is not
//!
//! It holds key bytes in process memory, which is the on-prem answer in DN-22 §5's table
//! and **not** the cloud one. A cloud deployment wants a provider whose `seal` and `sign`
//! call a managed service, and the trait is shaped so that one can exist -- the reason
//! there is no getter.

use crate::keys::{KeyId, KeyProvider, KeyPurpose, KeyState, SignatureScheme};
use crate::SecurityError;
use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{AeadCore, Aes256Gcm, Nonce};
use std::collections::BTreeMap;

/// How many bytes the sealed header takes: two for the purpose and version, twelve for
/// the nonce.
const HEADER: usize = 2 + 12;

/// One key version.
struct Version {
    key: aes_gcm::Key<Aes256Gcm>,
    state: KeyState,
}

/// One symmetric key as the keystore seals it (crate-private).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct SymmetricKeySnapshot {
    pub purpose_code: u8,
    pub version: u32,
    pub key: [u8; 32],
    pub state: KeyState,
    pub active: bool,
}

/// A provider holding its keys in process memory.
///
/// Constructed by the binary, because custody belongs to the host (DN-22 §4).
#[derive(Default)]
pub struct InProcessKeyProvider {
    versions: BTreeMap<(u8, u32), Version>,
    active: BTreeMap<u8, u32>,
}

impl std::fmt::Debug for InProcessKeyProvider {
    /// Never prints a key. A `Debug` that leaked one would undo the module.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InProcessKeyProvider")
            .field("versions", &self.versions.len())
            .finish_non_exhaustive()
    }
}

fn purpose_code(purpose: KeyPurpose) -> u8 {
    match purpose {
        KeyPurpose::TransportIdentity => 1,
        KeyPurpose::JournalAtRest => 2,
        KeyPurpose::BaselineSigning => 3,
    }
}

fn purpose_from_code(code: u8) -> Option<KeyPurpose> {
    match code {
        1 => Some(KeyPurpose::TransportIdentity),
        2 => Some(KeyPurpose::JournalAtRest),
        3 => Some(KeyPurpose::BaselineSigning),
        _ => None,
    }
}

impl InProcessKeyProvider {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Mint the first key for a purpose. Returns its identifier.
    ///
    /// Generating rather than accepting bytes is deliberate: a constructor taking key
    /// material would be the getter's mirror image, and every caller that could pass a
    /// key in is a caller that had one to pass.
    pub fn generate(&mut self, purpose: KeyPurpose) -> KeyId {
        let code = purpose_code(purpose);
        let version = self.active.get(&code).map_or(1, |v| v + 1);
        self.versions.insert(
            (code, version),
            Version {
                key: Aes256Gcm::generate_key(&mut OsRng),
                state: KeyState::Active,
            },
        );
        self.active.insert(code, version);
        KeyId { purpose, version }
    }

    /// Mark a key destroyed. Everything it protected becomes permanently unreadable.
    ///
    /// The caller is responsible for [`crate::keys::may_destroy`]: this is the mechanism,
    /// not the policy.
    pub fn destroy(&mut self, id: &KeyId) -> Result<(), SecurityError> {
        let entry = self
            .versions
            .get_mut(&(purpose_code(id.purpose), id.version))
            .ok_or_else(|| SecurityError::UnknownKey(format!("{id:?}")))?;
        entry.state = KeyState::Destroyed;
        Ok(())
    }

    // `&KeyId` rather than by value to match the trait's own signature, so the two
    // read the same way.
    /// Every key and its state, for the persistent keystore to seal (crate-private,
    /// DN-22 amendment 3). The material leaves this struct only into another sealed
    /// form.
    pub(crate) fn snapshot(&self) -> Vec<SymmetricKeySnapshot> {
        self.versions
            .iter()
            .map(|((code, version), v)| SymmetricKeySnapshot {
                purpose_code: *code,
                version: *version,
                key: v.key.into(),
                state: v.state,
                active: self.active.get(code) == Some(version),
            })
            .collect()
    }

    pub(crate) fn restore(keys: Vec<SymmetricKeySnapshot>) -> Self {
        let mut this = Self::default();
        for k in keys {
            this.versions.insert(
                (k.purpose_code, k.version),
                Version {
                    key: *aes_gcm::Key::<Aes256Gcm>::from_slice(&k.key),
                    state: k.state,
                },
            );
            if k.active {
                this.active.insert(k.purpose_code, k.version);
            }
        }
        this
    }

    /// The raw key, for the escrow wrap and nothing else (DN-22 §11). Crate-private:
    /// the provider boundary exists so no caller outside custody sees key material.
    /// Signed by the owner 2026-09-06 with the asymmetric provider.
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub(crate) fn key_material(&self, id: &KeyId) -> Result<[u8; 32], SecurityError> {
        let entry = self
            .versions
            .get(&(purpose_code(id.purpose), id.version))
            .ok_or_else(|| SecurityError::UnknownKey(format!("{id:?}")))?;
        if entry.state == KeyState::Destroyed {
            return Err(SecurityError::UnknownKey(format!("{id:?} is destroyed")));
        }
        Ok(entry.key.into())
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn cipher(&self, id: &KeyId) -> Result<Aes256Gcm, SecurityError> {
        let entry = self
            .versions
            .get(&(purpose_code(id.purpose), id.version))
            .ok_or_else(|| SecurityError::UnknownKey(format!("{id:?}")))?;
        if entry.state == KeyState::Destroyed {
            // A destroyed key does not read what it protected. Saying so is the whole
            // difference between retirement and destruction (DN-22 §5).
            return Err(SecurityError::UnknownKey(format!(
                "{id:?} is destroyed; what it protected is permanently unreadable"
            )));
        }
        Ok(Aes256Gcm::new(&entry.key))
    }
}

impl KeyProvider for InProcessKeyProvider {
    fn active(&self, purpose: KeyPurpose) -> Result<KeyId, SecurityError> {
        self.active
            .get(&purpose_code(purpose))
            .map(|version| KeyId {
                purpose,
                version: *version,
            })
            .ok_or_else(|| SecurityError::UnknownKey(format!("no key for {purpose:?}")))
    }

    fn state(&self, id: &KeyId) -> Result<KeyState, SecurityError> {
        self.versions
            .get(&(purpose_code(id.purpose), id.version))
            .map(|v| v.state)
            .ok_or_else(|| SecurityError::UnknownKey(format!("{id:?}")))
    }

    fn seal(&self, id: &KeyId, plaintext: &[u8]) -> Result<Vec<u8>, SecurityError> {
        if self.state(id)? == KeyState::Retired {
            // A retired key reads and does not write, which is what retirement means.
            return Err(SecurityError::KeyProviderUnavailable(format!(
                "{id:?} is retired; new material uses the active key"
            )));
        }
        let cipher = self.cipher(id)?;
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = cipher
            .encrypt(&nonce, plaintext)
            .map_err(|_| SecurityError::KeyProviderUnavailable("sealing failed".into()))?;

        let mut sealed = Vec::with_capacity(HEADER + ciphertext.len());
        sealed.push(purpose_code(id.purpose));
        // One byte for the version: a deployment rotating more than 255 times wants a
        // wider field, and truncating silently would make old material unreadable.
        sealed.push(u8::try_from(id.version).map_err(|_| {
            SecurityError::KeyProviderUnavailable(
                "key version does not fit the sealed header".into(),
            )
        })?);
        sealed.extend_from_slice(&nonce);
        sealed.extend_from_slice(&ciphertext);
        Ok(sealed)
    }

    fn unseal(&self, id: &KeyId, sealed: &[u8]) -> Result<Vec<u8>, SecurityError> {
        if sealed.len() < HEADER {
            return Err(SecurityError::KeyProviderUnavailable(
                "the sealed material is too short to carry its header".into(),
            ));
        }
        // The header decides which key reads this, not the caller's argument: material
        // sealed under a retired version must still open after a rotation, and a caller
        // holding only the active id is the normal case.
        let (header, body) = sealed.split_at(HEADER);
        let carried = KeyId {
            purpose: purpose_from_code(header[0]).ok_or_else(|| {
                SecurityError::KeyProviderUnavailable("unknown key purpose in the header".into())
            })?,
            version: u32::from(header[1]),
        };
        if carried.purpose != id.purpose {
            return Err(SecurityError::KeyProviderUnavailable(format!(
                "sealed under {:?} and opened as {:?}",
                carried.purpose, id.purpose
            )));
        }
        let cipher = self.cipher(&carried)?;
        let nonce = Nonce::from_slice(&header[2..HEADER]);
        cipher.decrypt(nonce, body).map_err(|_| {
            // One message whether the key was wrong or the bytes were altered: an
            // attacker probing a journal learns nothing from which.
            SecurityError::KeyProviderUnavailable("the sealed material did not open".into())
        })
    }

    fn rotate(&mut self, purpose: KeyPurpose) -> Result<KeyId, SecurityError> {
        let code = purpose_code(purpose);
        // The previous version retires and keeps reading. **Nothing existing is
        // rewritten**: DN-22 §5, and AP-08 for an append-only record.
        if let Some(previous) = self.active.get(&code).copied() {
            if let Some(entry) = self.versions.get_mut(&(code, previous)) {
                entry.state = KeyState::Retired;
            }
        }
        Ok(self.generate(purpose))
    }

    fn sign(
        &self,
        id: &KeyId,
        message: &[u8],
        scheme: SignatureScheme,
    ) -> Result<Vec<u8>, SecurityError> {
        // Amendment 1 (a) adds this so a TLS identity can exist without a getter. This
        // implementation holds symmetric keys only, so it can authenticate a message and
        // cannot produce the public-key signature a handshake needs -- and says so rather
        // than returning something a caller might mistake for one.
        let _ = (id, message);
        Err(SecurityError::AuthenticationUnavailable(format!(
            "{scheme:?} needs an asymmetric key; this provider holds symmetric keys only \
             (GAP-060 brings the transport identity)"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> (InProcessKeyProvider, KeyId) {
        let mut provider = InProcessKeyProvider::new();
        let id = provider.generate(KeyPurpose::JournalAtRest);
        (provider, id)
    }

    #[test]
    fn sealed_material_opens_again() {
        let (provider, id) = provider();
        let sealed = provider.seal(&id, b"a journal line").expect("sealed");
        assert_ne!(sealed, b"a journal line", "the plaintext was stored as-is");
        assert_eq!(
            provider.unseal(&id, &sealed).expect("opened"),
            b"a journal line"
        );
    }

    /// The nonce is fresh per operation, so the same plaintext seals differently twice.
    /// Identical ciphertext would tell an observer that two journal lines were equal.
    #[test]
    fn the_same_plaintext_seals_differently_each_time() {
        let (provider, id) = provider();
        let first = provider.seal(&id, b"same").expect("sealed");
        let second = provider.seal(&id, b"same").expect("sealed");
        assert_ne!(first, second, "a nonce was reused");
    }

    /// Altering one byte makes it fail to open. This is what "authenticated" buys: a
    /// journal that was edited does not decrypt to something plausible.
    #[test]
    fn tampered_material_does_not_open() {
        let (provider, id) = provider();
        let mut sealed = provider.seal(&id, b"a journal line").expect("sealed");
        let last = sealed.len() - 1;
        sealed[last] ^= 0x01;
        assert!(provider.unseal(&id, &sealed).is_err());
    }

    /// **The property rotation exists for.** Material sealed under the old version still
    /// opens after a rotation, without anything being rewritten -- because the sealed
    /// bytes carry the key that protected them (amendment 1 b).
    #[test]
    fn rotation_leaves_existing_material_readable_and_unmodified() {
        let (mut provider, first) = provider();
        let sealed = provider
            .seal(&first, b"written before the rotation")
            .expect("sealed");
        let before = sealed.clone();

        let second = provider.rotate(KeyPurpose::JournalAtRest).expect("rotated");
        assert_ne!(second.version, first.version);
        assert_eq!(provider.state(&first).expect("known"), KeyState::Retired);
        assert_eq!(provider.state(&second).expect("known"), KeyState::Active);

        // Opened with the *active* id, which is all a caller normally holds: the header
        // decides which version reads it.
        assert_eq!(
            provider.unseal(&second, &sealed).expect("still opens"),
            b"written before the rotation"
        );
        assert_eq!(sealed, before, "rotation rewrote existing material");
    }

    /// New material uses the active key; a retired one reads and does not write.
    #[test]
    fn a_retired_key_reads_but_does_not_seal() {
        let (mut provider, first) = provider();
        provider.rotate(KeyPurpose::JournalAtRest).expect("rotated");
        assert!(
            provider.seal(&first, b"late").is_err(),
            "a retired key sealed new material"
        );
    }

    /// Destruction is not retirement: what a destroyed key protected is gone, and the
    /// provider says so rather than failing as though the bytes were corrupt.
    #[test]
    fn a_destroyed_key_makes_its_material_unreadable_and_says_so() {
        let (mut provider, id) = provider();
        let sealed = provider.seal(&id, b"a journal line").expect("sealed");
        provider.destroy(&id).expect("destroyed");

        assert_eq!(provider.state(&id).expect("known"), KeyState::Destroyed);
        let err = provider.unseal(&id, &sealed).expect_err("unreadable");
        assert!(err.to_string().contains("destroyed"), "{err}");
    }

    /// The criterion DN-22 §8 puts first, and the one the trait's shape enforces: no
    /// consumer can obtain key bytes. Checked here as a statement of intent -- the
    /// provider's `Debug` must not leak them either.
    #[test]
    fn the_provider_never_prints_its_keys() {
        let (provider, _) = provider();
        let printed = format!("{provider:?}");
        assert!(printed.contains("versions"), "{printed}");
        assert_eq!(printed.matches("key").count(), 0, "{printed}");
    }

    /// A symmetric provider cannot make a TLS signature and refuses rather than
    /// returning something a caller might mistake for one.
    #[test]
    fn signing_is_refused_by_a_symmetric_provider() {
        let mut provider = InProcessKeyProvider::new();
        let id = provider.generate(KeyPurpose::TransportIdentity);
        let err = provider
            .sign(&id, b"transcript", SignatureScheme::EcdsaP256Sha256)
            .expect_err("refused");
        assert!(err.to_string().contains("asymmetric"), "{err}");
    }

    /// Material sealed for one purpose does not open as another, so a journal key cannot
    /// be used to read something that was protected as a transport identity.
    #[test]
    fn material_does_not_cross_purposes() {
        let mut provider = InProcessKeyProvider::new();
        let journal = provider.generate(KeyPurpose::JournalAtRest);
        let transport = provider.generate(KeyPurpose::TransportIdentity);
        let sealed = provider.seal(&journal, b"a journal line").expect("sealed");
        assert!(provider.unseal(&transport, &sealed).is_err());
    }

    #[test]
    fn short_or_malformed_material_is_refused_without_panicking() {
        let (provider, id) = provider();
        for bad in [vec![], vec![0u8; 3], vec![9u8; 40]] {
            assert!(provider.unseal(&id, &bad).is_err());
        }
    }
}
