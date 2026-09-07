// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The asymmetric provider (GAP-084; docs/design/DN-22-key-management.md §9a and §11).
//!
//! **Signed by the owner 2026-09-06** (`gungnir-security` is human-owned).
//!
//! Two things the symmetric provider cannot do. A **signature** for a transport identity
//! or a baseline: ECDSA over P-256 with SHA-256, DER-encoded, so a TLS handshake or a
//! baseline check can verify it with the public half and nothing else. And **escrow**
//! (§11): a journal's data key wrapped to the security officer's public key -- ECDH over
//! P-256, HKDF-SHA-256 over the shared secret, AES-256-GCM over the key -- so a record can
//! be recovered by the officer, offline, without the officer being able to read anything
//! live, and without the private half ever entering a node or a desktop.
//!
//! The private halves live here in process memory, which is DN-22 §5's on-prem answer.
//! The persistent keystores are still unbuilt and said so on the status strip.

use std::collections::BTreeMap;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{AeadCore, Aes256Gcm, Key, Nonce};
use hmac::{Mac, SimpleHmac};
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use p256::elliptic_curve::rand_core::OsRng;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::pkcs8::{DecodePublicKey, EncodePublicKey};
use p256::{ecdh, PublicKey, SecretKey};
use sha2::Sha256;

use crate::keys::{KeyId, KeyProvider, KeyPurpose, KeyState, SignatureScheme};
use crate::provider::InProcessKeyProvider;
use crate::SecurityError;

struct SigningVersion {
    key: SigningKey,
    state: KeyState,
}

fn purpose_code(purpose: KeyPurpose) -> u8 {
    match purpose {
        KeyPurpose::TransportIdentity => 1,
        KeyPurpose::JournalAtRest => 2,
        KeyPurpose::BaselineSigning => 3,
    }
}

/// Holds P-256 signing keys for the two signing purposes and delegates the journal's
/// symmetric keys to the in-process provider.
#[derive(Default)]
pub struct P256KeyProvider {
    symmetric: InProcessKeyProvider,
    signing: BTreeMap<(u8, u32), SigningVersion>,
    active_signing: BTreeMap<u8, u32>,
    escrow: Option<EscrowPublicKey>,
}

impl std::fmt::Debug for P256KeyProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("P256KeyProvider")
            .field("signing_keys", &self.signing.len())
            .field("escrow", &self.escrow.is_some())
            .finish_non_exhaustive()
    }
}

impl P256KeyProvider {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// With the security officer's public key, so journal keys can be escrowed (§11).
    #[must_use]
    pub fn with_escrow(mut self, officer: EscrowPublicKey) -> Self {
        self.escrow = Some(officer);
        self
    }

    #[must_use]
    pub fn escrow_configured(&self) -> bool {
        self.escrow.is_some()
    }

    /// A new active key for `purpose`.
    pub fn generate(&mut self, purpose: KeyPurpose) -> KeyId {
        if purpose == KeyPurpose::JournalAtRest {
            return self.symmetric.generate(purpose);
        }
        let code = purpose_code(purpose);
        let version = self.active_signing.get(&code).map_or(1, |v| v + 1);
        self.signing.insert(
            (code, version),
            SigningVersion {
                key: SigningKey::random(&mut OsRng),
                state: KeyState::Active,
            },
        );
        self.active_signing.insert(code, version);
        KeyId { purpose, version }
    }

    fn signing_key(&self, id: KeyId) -> Result<&SigningVersion, SecurityError> {
        let entry = self
            .signing
            .get(&(purpose_code(id.purpose), id.version))
            .ok_or_else(|| SecurityError::UnknownKey(format!("{id:?}")))?;
        if entry.state == KeyState::Destroyed {
            return Err(SecurityError::UnknownKey(format!("{id:?} is destroyed")));
        }
        Ok(entry)
    }

    /// The public half of a signing key, SEC1 uncompressed, for a certificate or a
    /// verifier. Never the private half: there is no getter for that, by design.
    ///
    /// # Errors
    ///
    /// `SecurityError::UnknownKey` for a key this provider does not hold, or a
    /// symmetric one.
    pub fn public_key_sec1(&self, id: &KeyId) -> Result<Vec<u8>, SecurityError> {
        let entry = self.signing_key(*id)?;
        Ok(VerifyingKey::from(&entry.key)
            .to_encoded_point(false)
            .as_bytes()
            .to_vec())
    }

    /// The public half as `SubjectPublicKeyInfo` DER, for a certificate (D-29).
    ///
    /// # Errors
    ///
    /// As [`Self::public_key_sec1`].
    pub fn public_key_der(&self, id: &KeyId) -> Result<Vec<u8>, SecurityError> {
        let entry = self.signing_key(*id)?;
        VerifyingKey::from(&entry.key)
            .to_public_key_der()
            .map(|d| d.as_bytes().to_vec())
            .map_err(|e| {
                SecurityError::KeyProviderUnavailable(format!("encoding the public key: {e}"))
            })
    }

    /// Every key this provider holds, for the persistent keystore to seal (DN-22
    /// amendment 3). Crate-private; the material goes only into another sealed form.
    pub(crate) fn snapshot(&self) -> KeystoreSnapshot {
        KeystoreSnapshot {
            symmetric: self.symmetric.snapshot(),
            signing: self
                .signing
                .iter()
                .map(|((code, version), v)| SigningKeySnapshot {
                    purpose_code: *code,
                    version: *version,
                    key: v.key.to_bytes().to_vec(),
                    state: v.state,
                    active: self.active_signing.get(code) == Some(version),
                })
                .collect(),
        }
    }

    /// A provider from a keystore's contents. Escrow is set afterwards; it is
    /// configuration, not key material.
    pub(crate) fn restore(snapshot: KeystoreSnapshot) -> Self {
        let mut this = Self {
            symmetric: InProcessKeyProvider::restore(snapshot.symmetric),
            ..Self::default()
        };
        for k in snapshot.signing {
            let Ok(key) = SigningKey::from_slice(&k.key) else {
                continue;
            };
            this.signing.insert(
                (k.purpose_code, k.version),
                SigningVersion {
                    key,
                    state: k.state,
                },
            );
            if k.active {
                this.active_signing.insert(k.purpose_code, k.version);
            }
        }
        this
    }

    /// Wrap a journal key to the officer's public key (§11). The result carries the
    /// ephemeral public half and the nonce it needs; nothing in it opens without the
    /// officer's private half.
    ///
    /// # Errors
    ///
    /// `KeyProviderUnavailable` when no escrow key is configured, when `id` is not a
    /// journal key, or when wrapping fails.
    pub fn escrow_wrap(&self, id: &KeyId) -> Result<EscrowedKey, SecurityError> {
        let officer = self.escrow.as_ref().ok_or_else(|| {
            SecurityError::KeyProviderUnavailable(
                "no escrow key is configured; the journal key cannot be recovered by anyone".into(),
            )
        })?;
        if id.purpose != KeyPurpose::JournalAtRest {
            return Err(SecurityError::KeyProviderUnavailable(format!(
                "{id:?} is not a journal key; only journal keys are escrowed"
            )));
        }
        let material = self.symmetric.key_material(id)?;
        let ephemeral = ecdh::EphemeralSecret::random(&mut OsRng);
        let ephemeral_public = PublicKey::from(&ephemeral).to_encoded_point(false);
        let shared = ephemeral.diffie_hellman(&officer.0);
        let wrap_key = hkdf_sha256(shared.raw_secret_bytes(), ephemeral_public.as_bytes())?;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&wrap_key));
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let wrapped = cipher
            .encrypt(&nonce, material.as_slice())
            .map_err(|_| SecurityError::KeyProviderUnavailable("escrow wrapping failed".into()))?;
        Ok(EscrowedKey {
            key: *id,
            ephemeral_public_sec1: ephemeral_public.as_bytes().to_vec(),
            nonce: nonce.into(),
            wrapped,
        })
    }
}

impl KeyProvider for P256KeyProvider {
    fn active(&self, purpose: KeyPurpose) -> Result<KeyId, SecurityError> {
        if purpose == KeyPurpose::JournalAtRest {
            return self.symmetric.active(purpose);
        }
        self.active_signing
            .get(&purpose_code(purpose))
            .map(|version| KeyId {
                purpose,
                version: *version,
            })
            .ok_or_else(|| SecurityError::UnknownKey(format!("no key for {purpose:?}")))
    }

    fn state(&self, id: &KeyId) -> Result<KeyState, SecurityError> {
        if id.purpose == KeyPurpose::JournalAtRest {
            return self.symmetric.state(id);
        }
        self.signing
            .get(&(purpose_code(id.purpose), id.version))
            .map(|v| v.state)
            .ok_or_else(|| SecurityError::UnknownKey(format!("{id:?}")))
    }

    fn seal(&self, id: &KeyId, plaintext: &[u8]) -> Result<Vec<u8>, SecurityError> {
        if id.purpose != KeyPurpose::JournalAtRest {
            return Err(SecurityError::KeyProviderUnavailable(format!(
                "{id:?} is a signing key; it seals nothing"
            )));
        }
        self.symmetric.seal(id, plaintext)
    }

    fn unseal(&self, id: &KeyId, ciphertext: &[u8]) -> Result<Vec<u8>, SecurityError> {
        self.symmetric.unseal(id, ciphertext)
    }

    fn rotate(&mut self, purpose: KeyPurpose) -> Result<KeyId, SecurityError> {
        if purpose == KeyPurpose::JournalAtRest {
            return self.symmetric.rotate(purpose);
        }
        let code = purpose_code(purpose);
        if let Some(previous) = self.active_signing.get(&code).copied() {
            if let Some(entry) = self.signing.get_mut(&(code, previous)) {
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
        if scheme != SignatureScheme::EcdsaP256Sha256 {
            return Err(SecurityError::AuthenticationUnavailable(format!(
                "{scheme:?} is not this provider's scheme; it signs ECDSA P-256 with SHA-256"
            )));
        }
        let entry = self.signing_key(*id)?;
        if entry.state == KeyState::Retired {
            return Err(SecurityError::KeyProviderUnavailable(format!(
                "{id:?} is retired; new signatures use the active key"
            )));
        }
        let signature: Signature = entry.key.sign(message);
        Ok(signature.to_der().as_bytes().to_vec())
    }
}

/// What the keystore seals: every key and its state (DN-22 amendment 3). Public only
/// as a type the keystore names; its fields are crate-private.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KeystoreSnapshot {
    pub(crate) symmetric: Vec<crate::provider::SymmetricKeySnapshot>,
    pub(crate) signing: Vec<SigningKeySnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct SigningKeySnapshot {
    pub purpose_code: u8,
    pub version: u32,
    pub key: Vec<u8>,
    pub state: KeyState,
    pub active: bool,
}

/// The officer's public key, as the baseline carries it inline (§11).
#[derive(Clone)]
pub struct EscrowPublicKey(PublicKey);

impl std::fmt::Debug for EscrowPublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EscrowPublicKey(P-256)")
    }
}

impl EscrowPublicKey {
    /// # Errors
    ///
    /// `KeyProviderUnavailable` when the PEM is not a P-256 public key. A private key
    /// is refused without being repeated in the error.
    pub fn from_pem(pem: &str) -> Result<Self, SecurityError> {
        if pem.contains("PRIVATE KEY") {
            return Err(SecurityError::KeyProviderUnavailable(
                "the escrow key must be the public half; a private key was given".into(),
            ));
        }
        PublicKey::from_public_key_pem(pem)
            .map(Self)
            .map_err(|e| SecurityError::KeyProviderUnavailable(format!("escrow public key: {e}")))
    }
}

/// A journal key wrapped to the officer (§11). Serialisable, because it travels with
/// the segment it protects.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EscrowedKey {
    pub key: KeyId,
    pub ephemeral_public_sec1: Vec<u8>,
    pub nonce: [u8; 12],
    pub wrapped: Vec<u8>,
}

/// The officer's private half. **Exists only in the recovery tool and in tests**; no
/// binary constructs one, and there is no way to load it from a baseline.
pub struct EscrowOfficerKey(SecretKey);

impl std::fmt::Debug for EscrowOfficerKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EscrowOfficerKey(P-256, private)")
    }
}

impl EscrowOfficerKey {
    /// A fresh officer key pair, for the recovery tool's key ceremony and for tests.
    #[must_use]
    pub fn generate() -> Self {
        Self(SecretKey::random(&mut OsRng))
    }

    /// The public half as PEM, for the baseline.
    ///
    /// # Errors
    ///
    /// When the key cannot be encoded, which a freshly generated one always can.
    pub fn public_pem(&self) -> Result<String, SecurityError> {
        use p256::pkcs8::EncodePublicKey;
        self.0
            .public_key()
            .to_public_key_pem(p256::pkcs8::LineEnding::LF)
            .map_err(|e| {
                SecurityError::KeyProviderUnavailable(format!("encoding the public key: {e}"))
            })
    }

    /// Recover a wrapped journal key. This is the audited act (`KEY_ESCROW_RECOVER`);
    /// the caller records who did it and which segment.
    ///
    /// # Errors
    ///
    /// `KeyProviderUnavailable` when the wrapped key does not open under this officer.
    pub fn recover(&self, escrowed: &EscrowedKey) -> Result<RecoveredDataKey, SecurityError> {
        let ephemeral =
            PublicKey::from_sec1_bytes(&escrowed.ephemeral_public_sec1).map_err(|_| {
                SecurityError::KeyProviderUnavailable(
                    "the escrow record's ephemeral key is malformed".into(),
                )
            })?;
        let shared = ecdh::diffie_hellman(self.0.to_nonzero_scalar(), ephemeral.as_affine());
        let wrap_key = hkdf_sha256(shared.raw_secret_bytes(), &escrowed.ephemeral_public_sec1)?;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&wrap_key));
        let material = cipher
            .decrypt(
                Nonce::from_slice(&escrowed.nonce),
                escrowed.wrapped.as_slice(),
            )
            .map_err(|_| {
                SecurityError::KeyProviderUnavailable(
                    "the escrowed key did not open under this officer".into(),
                )
            })?;
        let bytes: [u8; 32] = material.try_into().map_err(|_| {
            SecurityError::KeyProviderUnavailable("the recovered key is not 32 bytes".into())
        })?;
        Ok(RecoveredDataKey {
            key: escrowed.key,
            bytes,
        })
    }
}

/// A journal key recovered offline: opens what was sealed under it, and nothing else.
pub struct RecoveredDataKey {
    pub key: KeyId,
    bytes: [u8; 32],
}

impl std::fmt::Debug for RecoveredDataKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecoveredDataKey")
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

impl RecoveredDataKey {
    /// Open material sealed by the provider under this key: the same framing the
    /// in-process provider writes (two header bytes, a twelve-byte nonce, the body).
    ///
    /// # Errors
    ///
    /// `KeyProviderUnavailable` when the material is not this key's or is corrupt.
    pub fn unseal(&self, sealed: &[u8]) -> Result<Vec<u8>, SecurityError> {
        const HEADER: usize = 2 + 12;
        if sealed.len() < HEADER {
            return Err(SecurityError::KeyProviderUnavailable(
                "sealed material too short".into(),
            ));
        }
        let (header, body) = sealed.split_at(HEADER);
        if header[0] != purpose_code(self.key.purpose) || u32::from(header[1]) != self.key.version {
            return Err(SecurityError::KeyProviderUnavailable(format!(
                "sealed under another key than {:?}",
                self.key
            )));
        }
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.bytes));
        cipher
            .decrypt(Nonce::from_slice(&header[2..HEADER]), body)
            .map_err(|_| {
                SecurityError::KeyProviderUnavailable("the sealed material did not open".into())
            })
    }
}

/// HKDF-SHA-256, one output block, over HMAC the stack already holds (§11: no new crate).
fn hkdf_sha256(ikm: &[u8], salt: &[u8]) -> Result<[u8; 32], SecurityError> {
    let mac_err = |_| SecurityError::KeyProviderUnavailable("HKDF key length".into());
    let mut extract = <SimpleHmac<Sha256> as Mac>::new_from_slice(salt).map_err(mac_err)?;
    extract.update(ikm);
    let prk = extract.finalize().into_bytes();
    let mut expand = <SimpleHmac<Sha256> as Mac>::new_from_slice(&prk).map_err(mac_err)?;
    expand.update(b"gungnir journal escrow v1");
    expand.update(&[1u8]);
    Ok(expand.finalize().into_bytes().into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::signature::Verifier;

    #[test]
    fn a_signature_verifies_under_the_public_half_and_a_retired_key_signs_nothing_new() {
        let mut provider = P256KeyProvider::new();
        let id = provider.generate(KeyPurpose::TransportIdentity);
        let signature = provider
            .sign(
                &id,
                b"handshake transcript",
                SignatureScheme::EcdsaP256Sha256,
            )
            .expect("signed");
        let public = provider.public_key_sec1(&id).expect("public half");
        let verifier = VerifyingKey::from_sec1_bytes(&public).expect("sec1");
        let parsed = Signature::from_der(&signature).expect("der");
        verifier
            .verify(b"handshake transcript", &parsed)
            .expect("verifies");
        assert!(verifier.verify(b"another transcript", &parsed).is_err());
        assert!(
            provider.sign(&id, b"x", SignatureScheme::Ed25519).is_err(),
            "only its own scheme"
        );
        let next = provider
            .rotate(KeyPurpose::TransportIdentity)
            .expect("rotated");
        assert!(
            provider
                .sign(&id, b"x", SignatureScheme::EcdsaP256Sha256)
                .is_err(),
            "retired"
        );
        assert!(provider
            .sign(&next, b"x", SignatureScheme::EcdsaP256Sha256)
            .is_ok());
    }

    #[test]
    fn a_journal_key_is_recovered_by_the_officer_and_by_nobody_else() {
        let officer = EscrowOfficerKey::generate();
        let pem = officer.public_pem().expect("pem");
        let mut provider =
            P256KeyProvider::new().with_escrow(EscrowPublicKey::from_pem(&pem).expect("public"));
        let journal = provider.generate(KeyPurpose::JournalAtRest);
        let sealed = provider
            .seal(&journal, b"decision 41: accepted")
            .expect("sealed");

        let escrowed = provider.escrow_wrap(&journal).expect("wrapped");
        let recovered = officer.recover(&escrowed).expect("the officer recovers it");
        assert_eq!(
            recovered.unseal(&sealed).expect("opens"),
            b"decision 41: accepted"
        );

        let impostor = EscrowOfficerKey::generate();
        assert!(
            impostor.recover(&escrowed).is_err(),
            "another key opens nothing"
        );
        // The node holds only the public half: nothing on the provider can recover.
        let signing = provider.generate(KeyPurpose::BaselineSigning);
        assert!(
            provider.escrow_wrap(&signing).is_err(),
            "only journal keys are escrowed"
        );
    }

    #[test]
    fn a_private_key_offered_as_the_escrow_key_is_refused_without_being_repeated() {
        let err = EscrowPublicKey::from_pem(
            "-----BEGIN PRIVATE KEY-----\nMIG...\n-----END PRIVATE KEY-----",
        )
        .expect_err("refused");
        assert!(!err.to_string().contains("MIG"));
        assert!(P256KeyProvider::new()
            .escrow_wrap(&KeyId {
                purpose: KeyPurpose::JournalAtRest,
                version: 1
            })
            .is_err());
    }
}
