// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A persistent keystore for the disconnected desktop (GAP-084; DN-22 amendment 3, §12).
//!
//! **Signed by the owner 2026-09-06** (`gungnir-security` is human-owned). The header
//! here had not caught up with `ARCHITECTURE.md`'s own record of that day; corrected
//! 2026-09-08 after the owner confirmed it directly, the same way DN-26's laydown
//! signature needed a direct confirmation before this register could act on it.
//!
//! DN-22 §5's disconnected row says the operating system's keystore, unlocked at operator
//! login. No crate in the approved stack reaches the OS keystore, and adding one is a
//! §2.9 decision; what the stack does hold is argon2 and AES-GCM, and that is enough to
//! keep every key this desktop owns in **one file sealed under a key derived from the
//! operator's passphrase**, unlocked at sign-in and never at start. The file holds
//! ciphertext and a salt and nothing else; the baseline names the mechanism and no path.
//! A desktop that has not been signed in to journals in the clear and says so, which is
//! §5's fallback rule and is what happens today before the first sign-in.
//!
//! The passphrase is the account passphrase (DN-23), used here to derive a wrapping key
//! with a salt of its own, so the stored PHC verifier and the wrapping key share nothing
//! but their input.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{AeadCore, Aes256Gcm, Key, Nonce};

use crate::asymmetric::{EscrowPublicKey, EscrowedKey, P256KeyProvider};
use crate::keys::{KeyId, KeyProvider, KeyPurpose, KeyState, SignatureScheme};
use crate::SecurityError;

/// The file's shape. Everything but the salt and the nonce is ciphertext.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct SealedFile {
    version: u32,
    salt: Vec<u8>,
    nonce: Vec<u8>,
    ciphertext: Vec<u8>,
}

const FILE_VERSION: u32 = 1;

/// The name the keystore lives under in the data directory. Fixed, so no path sits in
/// the baseline (DN-22 §6).
pub const KEYSTORE_FILE: &str = "keystore.sealed";

/// A P-256 provider whose keys live in a passphrase-sealed file.
pub struct PersistentKeyProvider {
    inner: Mutex<P256KeyProvider>,
    path: PathBuf,
    wrapping_key: [u8; 32],
    salt: Vec<u8>,
}

impl std::fmt::Debug for PersistentKeyProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PersistentKeyProvider")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

fn derive_wrapping_key(passphrase: &str, salt: &[u8]) -> Result<[u8; 32], SecurityError> {
    let mut out = [0u8; 32];
    argon2::Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut out)
        .map_err(|e| {
            SecurityError::KeyProviderUnavailable(format!("deriving the wrapping key: {e}"))
        })?;
    Ok(out)
}

impl PersistentKeyProvider {
    /// Open the keystore in `dir`, or create it when there is none. A file that does not
    /// open under this passphrase is refused, not overwritten: the keys in it may be the
    /// only way to read a year of journals.
    ///
    /// # Errors
    ///
    /// `KeyProviderUnavailable` when the file cannot be read, does not open, or cannot
    /// be written.
    pub fn open_or_create(
        dir: &Path,
        passphrase: &str,
        escrow: Option<EscrowPublicKey>,
    ) -> Result<Self, SecurityError> {
        let path = dir.join(KEYSTORE_FILE);
        let unavailable = |what: String| SecurityError::KeyProviderUnavailable(what);
        if path.is_file() {
            let text = std::fs::read_to_string(&path)
                .map_err(|e| unavailable(format!("reading {}: {e}", path.display())))?;
            let file: SealedFile = serde_json::from_str(&text)
                .map_err(|e| unavailable(format!("{} is not a keystore: {e}", path.display())))?;
            if file.version != FILE_VERSION {
                return Err(unavailable(format!(
                    "{} is keystore version {}, and this build reads {FILE_VERSION}",
                    path.display(),
                    file.version
                )));
            }
            let wrapping_key = derive_wrapping_key(passphrase, &file.salt)?;
            let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&wrapping_key));
            let plaintext = cipher
                .decrypt(Nonce::from_slice(&file.nonce), file.ciphertext.as_slice())
                .map_err(|_| {
                    unavailable(format!(
                        "{} did not open under this passphrase",
                        path.display()
                    ))
                })?;
            let snapshot: crate::asymmetric::KeystoreSnapshot = serde_json::from_slice(&plaintext)
                .map_err(|e| {
                    unavailable(format!(
                        "{} opened but does not hold keys: {e}",
                        path.display()
                    ))
                })?;
            let mut inner = P256KeyProvider::restore(snapshot);
            if let Some(officer) = escrow {
                inner = inner.with_escrow(officer);
            }
            return Ok(Self {
                inner: Mutex::new(inner),
                path,
                wrapping_key,
                salt: file.salt,
            });
        }
        let mut salt = vec![0u8; 16];
        aes_gcm::aead::rand_core::RngCore::fill_bytes(&mut OsRng, &mut salt);
        let wrapping_key = derive_wrapping_key(passphrase, &salt)?;
        let mut inner = P256KeyProvider::new();
        if let Some(officer) = escrow {
            inner = inner.with_escrow(officer);
        }
        let this = Self {
            inner: Mutex::new(inner),
            path,
            wrapping_key,
            salt,
        };
        this.persist()?;
        Ok(this)
    }

    fn persist(&self) -> Result<(), SecurityError> {
        let snapshot = self
            .inner
            .lock()
            .map_err(|_| {
                SecurityError::KeyProviderUnavailable("the keystore lock is poisoned".into())
            })?
            .snapshot();
        let plaintext = serde_json::to_vec(&snapshot).map_err(|e| {
            SecurityError::KeyProviderUnavailable(format!("encoding the keystore: {e}"))
        })?;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.wrapping_key));
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = cipher.encrypt(&nonce, plaintext.as_slice()).map_err(|_| {
            SecurityError::KeyProviderUnavailable("sealing the keystore failed".into())
        })?;
        let file = SealedFile {
            version: FILE_VERSION,
            salt: self.salt.clone(),
            nonce: nonce.to_vec(),
            ciphertext,
        };
        let text = serde_json::to_string(&file).map_err(|e| {
            SecurityError::KeyProviderUnavailable(format!("encoding the keystore: {e}"))
        })?;
        // Write beside, then rename: a crash mid-write must not leave half a keystore.
        let tmp = self.path.with_extension("sealed.tmp");
        std::fs::write(&tmp, text)
            .and_then(|()| std::fs::rename(&tmp, &self.path))
            .map_err(|e| {
                SecurityError::KeyProviderUnavailable(format!(
                    "writing {}: {e}",
                    self.path.display()
                ))
            })
    }

    /// The active key for `purpose`, generating one on first use and persisting it.
    ///
    /// # Errors
    ///
    /// `KeyProviderUnavailable` when the keystore cannot be written.
    pub fn active_or_generate(&self, purpose: KeyPurpose) -> Result<KeyId, SecurityError> {
        let existing = self.active(purpose);
        if let Ok(id) = existing {
            return Ok(id);
        }
        let id = self
            .inner
            .lock()
            .map_err(|_| {
                SecurityError::KeyProviderUnavailable("the keystore lock is poisoned".into())
            })?
            .generate(purpose);
        self.persist()?;
        Ok(id)
    }

    /// The journal key wrapped to the officer (DN-22 §11), for the record beside the
    /// journal.
    ///
    /// # Errors
    ///
    /// As [`P256KeyProvider::escrow_wrap`].
    pub fn escrow_wrap(&self, id: &KeyId) -> Result<EscrowedKey, SecurityError> {
        self.inner
            .lock()
            .map_err(|_| {
                SecurityError::KeyProviderUnavailable("the keystore lock is poisoned".into())
            })?
            .escrow_wrap(id)
    }

    #[must_use]
    pub fn escrow_configured(&self) -> bool {
        self.inner.lock().is_ok_and(|p| p.escrow_configured())
    }

    /// The public half of a signing key, SPKI DER, for a certificate.
    ///
    /// # Errors
    ///
    /// As [`P256KeyProvider::public_key_der`].
    pub fn public_key_der(&self, id: &KeyId) -> Result<Vec<u8>, SecurityError> {
        self.inner
            .lock()
            .map_err(|_| {
                SecurityError::KeyProviderUnavailable("the keystore lock is poisoned".into())
            })?
            .public_key_der(id)
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn with_inner<T>(
        &self,
        f: impl FnOnce(&P256KeyProvider) -> Result<T, SecurityError>,
    ) -> Result<T, SecurityError> {
        let guard = self.inner.lock().map_err(|_| {
            SecurityError::KeyProviderUnavailable("the keystore lock is poisoned".into())
        })?;
        f(&guard)
    }
}

impl KeyProvider for PersistentKeyProvider {
    fn active(&self, purpose: KeyPurpose) -> Result<KeyId, SecurityError> {
        self.with_inner(|p| p.active(purpose))
    }

    fn state(&self, id: &KeyId) -> Result<KeyState, SecurityError> {
        self.with_inner(|p| p.state(id))
    }

    fn seal(&self, id: &KeyId, plaintext: &[u8]) -> Result<Vec<u8>, SecurityError> {
        self.with_inner(|p| p.seal(id, plaintext))
    }

    fn unseal(&self, id: &KeyId, ciphertext: &[u8]) -> Result<Vec<u8>, SecurityError> {
        self.with_inner(|p| p.unseal(id, ciphertext))
    }

    fn rotate(&mut self, purpose: KeyPurpose) -> Result<KeyId, SecurityError> {
        let id = self
            .inner
            .get_mut()
            .map_err(|_| {
                SecurityError::KeyProviderUnavailable("the keystore lock is poisoned".into())
            })?
            .rotate(purpose)?;
        self.persist()?;
        Ok(id)
    }

    fn sign(
        &self,
        id: &KeyId,
        message: &[u8],
        scheme: SignatureScheme,
    ) -> Result<Vec<u8>, SecurityError> {
        self.with_inner(|p| p.sign(id, message, scheme))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asymmetric::EscrowOfficerKey;

    fn dir(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("gungnir-keystore-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("dir");
        d
    }

    #[test]
    fn keys_survive_a_restart_under_the_same_passphrase_and_not_another() {
        let d = dir("restart");
        let first =
            PersistentKeyProvider::open_or_create(&d, "correct horse", None).expect("created");
        let journal = first
            .active_or_generate(KeyPurpose::JournalAtRest)
            .expect("key");
        let sealed = first.seal(&journal, b"decision 9").expect("sealed");
        let identity = first
            .active_or_generate(KeyPurpose::TransportIdentity)
            .expect("key");
        let public = first.public_key_der(&identity).expect("public");
        drop(first);

        let again =
            PersistentKeyProvider::open_or_create(&d, "correct horse", None).expect("opened");
        assert_eq!(
            again.active(KeyPurpose::JournalAtRest).expect("active"),
            journal
        );
        assert_eq!(
            again.unseal(&journal, &sealed).expect("opens"),
            b"decision 9"
        );
        assert_eq!(
            again.public_key_der(&identity).expect("public"),
            public,
            "the same identity"
        );

        let err = PersistentKeyProvider::open_or_create(&d, "wrong", None).expect_err("refused");
        assert!(err.to_string().contains("did not open"), "{err}");
        assert!(
            d.join(KEYSTORE_FILE).is_file(),
            "and the file was not overwritten"
        );
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn the_file_holds_no_plaintext_key_and_an_escrowed_journal_key_is_recoverable() {
        let d = dir("escrow");
        let officer = EscrowOfficerKey::generate();
        let pem = officer.public_pem().expect("pem");
        let store = PersistentKeyProvider::open_or_create(
            &d,
            "correct horse",
            Some(EscrowPublicKey::from_pem(&pem).expect("public")),
        )
        .expect("created");
        let journal = store
            .active_or_generate(KeyPurpose::JournalAtRest)
            .expect("key");
        let sealed = store.seal(&journal, b"decision 41").expect("sealed");
        let escrowed = store.escrow_wrap(&journal).expect("wrapped");
        let recovered = officer.recover(&escrowed).expect("recovered");
        assert_eq!(recovered.unseal(&sealed).expect("opens"), b"decision 41");
        let bytes = std::fs::read(d.join(KEYSTORE_FILE)).expect("file");
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("\"ciphertext\""));
        assert!(
            !text.contains("JournalAtRest"),
            "nothing legible about the keys"
        );
        let _ = std::fs::remove_dir_all(d);
    }
}
