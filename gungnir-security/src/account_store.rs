// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A node's own accounts, sealed under a key the operating system's keystore holds
//! (GAP-057's node half; D-39; `docs/design/DN-23-operator-authentication.md` amendment
//! 2).
//!
//! **Human-owned** (docs/agentic-workflow.md); signed by the owner 2026-09-08
//! (`ARCHITECTURE.md` §10 item 104, reviewed with items 103 and 111).
//!
//! `keystore.rs`'s `PersistentKeyProvider` seals a `P256KeyProvider` snapshot in one
//! file under a key argon2 derives from a string that `os_keystore` supplies. This
//! module does the same thing to a node's `Vec<Account>` instead: the same sealed-file
//! shape, the same derivation, the same source for the string -- and nothing else
//! shared, because a `P256KeyProvider` snapshot and an account list are typed for
//! different things and forcing one through the other's constructor would be the wrong
//! kind of reuse.
//!
//! **A distinct service name from the desktop's own.** [`os_keystore::wrapping_secret`]
//! takes the service as a parameter for exactly this: [`NODE_KEYSTORE_SERVICE`] keeps a
//! node's entries out of the desktop's `gungnir-desktop-keystore` namespace, so the two
//! never collide when both run on one machine.
//!
//! **Why a node, which DN-22 amendment 4 says has no operator login to unlock at.**
//! That amendment is about the *key provider* row: §5 assigns the disconnected
//! desktop's `OperatingSystemKeystore` custody specifically to an interactively
//! logged-in operator's own session unlocking it, and a node has no such session to
//! wait for -- its key-provider row stays `ManagedService`, unbuilt, for that reason.
//! This module answers a different question: not "whose login unlocks this" but
//! "is a sealed file better than a plaintext one for accounts a node already has to
//! keep somewhere." A node's own process identity -- the Windows service account it
//! runs as, a Linux keyring a systemd unit has been given access to -- can hold a
//! keystore entry with no human present at all, which is a real, narrower claim than
//! amendment 4 makes for the desktop. Where no such facility is reachable this module's
//! error path fires exactly as `os_keystore::wrapping_secret` documents, and the node
//! reports `SecurityError::AccountStoreUnavailable` -- honest, not a gap in coverage,
//! matching `gungnir-security/tests/os_keystore.rs`'s own precedent for a headless
//! Linux runner with no Secret Service session on its bus.

use std::path::{Path, PathBuf};
use std::sync::RwLock;

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{AeadCore, Aes256Gcm, Key, Nonce};

use crate::session::{Account, AccountStore};
use crate::{OperatorId, Role, SecurityError};

/// The service name a node's account-store entries live under in the OS keystore --
/// distinct from `os_keystore::DESKTOP_KEYSTORE_SERVICE` so a node and a desktop
/// sharing a machine never address the same entry.
const NODE_KEYSTORE_SERVICE: &str = "gungnir-node-accounts";

/// The file's shape. Everything but the salt and the nonce is ciphertext -- the same
/// layout `keystore.rs::SealedFile` uses, kept as a separate private type rather than a
/// shared one because the two seal different payloads and have no other reason to agree.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct SealedFile {
    version: u32,
    salt: Vec<u8>,
    nonce: Vec<u8>,
    ciphertext: Vec<u8>,
}

const FILE_VERSION: u32 = 1;

/// The name the store lives under in the node's data directory.
pub const ACCOUNT_STORE_FILE: &str = "accounts.sealed";

fn derive_wrapping_key(secret: &str, salt: &[u8]) -> Result<[u8; 32], SecurityError> {
    let mut out = [0u8; 32];
    argon2::Argon2::default()
        .hash_password_into(secret.as_bytes(), salt, &mut out)
        .map_err(|e| {
            SecurityError::AccountStoreUnavailable(format!("deriving the wrapping key: {e}"))
        })?;
    Ok(out)
}

/// A node's accounts, sealed in one file under a key the operating system's keystore
/// holds rather than a typed passphrase -- there being no operator login on a headless
/// node to type one at (GAP-057).
pub struct EncryptedAccountStore {
    accounts: RwLock<Vec<Account>>,
    path: PathBuf,
    wrapping_key: [u8; 32],
    salt: Vec<u8>,
}

impl std::fmt::Debug for EncryptedAccountStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EncryptedAccountStore")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl EncryptedAccountStore {
    /// Open the store in `dir`, or create an empty one when there is none, using a
    /// secret the operating system's own keystore holds (`account` names which entry,
    /// exactly as `KeyProviderConfig::OperatingSystemKeystore`'s `account` does).
    ///
    /// # Errors
    ///
    /// `KeyProviderUnavailable` when this platform has no reachable keystore.
    /// `AccountStoreUnavailable` when the file exists and does not open under the
    /// stored secret, or cannot be written.
    pub fn open_or_create(dir: &Path, account: &str) -> Result<Self, SecurityError> {
        let secret = crate::os_keystore::wrapping_secret(NODE_KEYSTORE_SERVICE, account)?;
        Self::open_or_create_with_secret(dir, &secret)
    }

    /// The same, with the wrapping secret supplied directly rather than drawn from the
    /// OS keystore -- so a test can exercise the sealed-file logic without touching the
    /// real backend, the same reason `keystore.rs`'s own tests call `open_or_create`
    /// with a plain string instead of `open_or_create_via_os_keystore`.
    fn open_or_create_with_secret(dir: &Path, secret: &str) -> Result<Self, SecurityError> {
        let path = dir.join(ACCOUNT_STORE_FILE);
        let unavailable = SecurityError::AccountStoreUnavailable;
        if path.is_file() {
            let text = std::fs::read_to_string(&path)
                .map_err(|e| unavailable(format!("reading {}: {e}", path.display())))?;
            let file: SealedFile = serde_json::from_str(&text).map_err(|e| {
                unavailable(format!("{} is not an account store: {e}", path.display()))
            })?;
            if file.version != FILE_VERSION {
                return Err(unavailable(format!(
                    "{} is account-store version {}, and this build reads {FILE_VERSION}",
                    path.display(),
                    file.version
                )));
            }
            let wrapping_key = derive_wrapping_key(secret, &file.salt)?;
            let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&wrapping_key));
            let plaintext = cipher
                .decrypt(Nonce::from_slice(&file.nonce), file.ciphertext.as_slice())
                .map_err(|_| {
                    unavailable(format!("{} did not open under this secret", path.display()))
                })?;
            let accounts: Vec<Account> = serde_json::from_slice(&plaintext).map_err(|e| {
                unavailable(format!(
                    "{} opened but does not hold accounts: {e}",
                    path.display()
                ))
            })?;
            return Ok(Self {
                accounts: RwLock::new(accounts),
                path,
                wrapping_key,
                salt: file.salt,
            });
        }
        let mut salt = vec![0u8; 16];
        aes_gcm::aead::rand_core::RngCore::fill_bytes(&mut OsRng, &mut salt);
        let wrapping_key = derive_wrapping_key(secret, &salt)?;
        let this = Self {
            accounts: RwLock::new(Vec::new()),
            path,
            wrapping_key,
            salt,
        };
        this.persist()?;
        Ok(this)
    }

    fn persist(&self) -> Result<(), SecurityError> {
        let plaintext = {
            let accounts = self.accounts.read().map_err(|_| {
                SecurityError::AccountStoreUnavailable("the account store lock is poisoned".into())
            })?;
            serde_json::to_vec(&*accounts).map_err(|e| {
                SecurityError::AccountStoreUnavailable(format!("encoding the account store: {e}"))
            })?
        };
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.wrapping_key));
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = cipher.encrypt(&nonce, plaintext.as_slice()).map_err(|_| {
            SecurityError::AccountStoreUnavailable("sealing the account store failed".into())
        })?;
        let file = SealedFile {
            version: FILE_VERSION,
            salt: self.salt.clone(),
            nonce: nonce.to_vec(),
            ciphertext,
        };
        let text = serde_json::to_string(&file).map_err(|e| {
            SecurityError::AccountStoreUnavailable(format!("encoding the account store: {e}"))
        })?;
        // Write beside, then rename: a crash mid-write must not leave half a store.
        let tmp = self.path.with_extension("sealed.tmp");
        std::fs::write(&tmp, text)
            .and_then(|()| std::fs::rename(&tmp, &self.path))
            .map_err(|e| {
                SecurityError::AccountStoreUnavailable(format!(
                    "writing {}: {e}",
                    self.path.display()
                ))
            })
    }

    /// The accounts as PN-20 lists them: operator and role, never the hash.
    ///
    /// # Errors
    ///
    /// `AccountStoreUnavailable` if the lock is poisoned.
    pub fn listing(&self) -> Result<Vec<(OperatorId, Role)>, SecurityError> {
        let accounts = self.accounts.read().map_err(|_| {
            SecurityError::AccountStoreUnavailable("the account store lock is poisoned".into())
        })?;
        Ok(accounts.iter().map(|a| (a.operator, a.role)).collect())
    }

    /// Add a new account, or replace an existing operator's when `replace` is set --
    /// the same rule `gungnir-node account add --replace` follows for the plaintext
    /// store, so provisioning reads the same either way.
    ///
    /// # Errors
    ///
    /// `SecurityError::Forbidden` when the operator already has an account and
    /// `replace` is false. `AccountStoreUnavailable` when the file cannot be written.
    pub fn add(
        &self,
        operator: OperatorId,
        role: Role,
        phc: String,
        replace: bool,
    ) -> Result<(), SecurityError> {
        {
            let mut accounts = self.accounts.write().map_err(|_| {
                SecurityError::AccountStoreUnavailable("the account store lock is poisoned".into())
            })?;
            match accounts.iter_mut().find(|a| a.operator == operator) {
                Some(existing) if replace => {
                    existing.role = role;
                    existing.phc = phc;
                }
                Some(_) => {
                    return Err(SecurityError::Forbidden(format!(
                        "operator {} already has an account; pass replace to overwrite it",
                        operator.0
                    )));
                }
                None => accounts.push(Account {
                    operator,
                    role,
                    phc,
                }),
            }
        }
        self.persist()
    }

    /// Change an account's role (GAP-057, PN-20's node-facing equivalent). Mirrors
    /// `FileAccountStore::assign_role`: the passphrase hash is untouched, and an
    /// unknown operator is refused rather than created.
    ///
    /// # Errors
    ///
    /// `SecurityError::Forbidden` for an operator the store does not hold.
    pub fn assign_role(&self, operator: OperatorId, role: Role) -> Result<(), SecurityError> {
        {
            let mut accounts = self.accounts.write().map_err(|_| {
                SecurityError::AccountStoreUnavailable("the account store lock is poisoned".into())
            })?;
            let account = accounts
                .iter_mut()
                .find(|a| a.operator == operator)
                .ok_or_else(|| {
                    SecurityError::Forbidden(format!(
                        "no account for operator {}: creating one needs a passphrase nobody has given",
                        operator.0
                    ))
                })?;
            account.role = role;
        }
        self.persist()
    }
}

impl AccountStore for EncryptedAccountStore {
    fn account(&self, operator: OperatorId) -> Result<Option<Account>, SecurityError> {
        let accounts = self.accounts.read().map_err(|_| {
            SecurityError::AccountStoreUnavailable("the account store lock is poisoned".into())
        })?;
        Ok(accounts.iter().find(|a| a.operator == operator).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "gungnir-account-store-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("dir");
        d
    }

    #[test]
    fn accounts_survive_a_restart_under_the_same_secret_and_not_another() {
        let d = dir("restart");
        let first = EncryptedAccountStore::open_or_create_with_secret(&d, "correct horse")
            .expect("created");
        first
            .add(OperatorId(7), Role::Operator, "phc-of-7".into(), false)
            .expect("added");
        drop(first);

        let again = EncryptedAccountStore::open_or_create_with_secret(&d, "correct horse")
            .expect("reopened");
        assert_eq!(
            again.account(OperatorId(7)).expect("looked up"),
            Some(Account {
                operator: OperatorId(7),
                role: Role::Operator,
                phc: "phc-of-7".into(),
            })
        );

        let err =
            EncryptedAccountStore::open_or_create_with_secret(&d, "wrong").expect_err("refused");
        assert!(err.to_string().contains("did not open"), "{err}");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_second_add_without_replace_is_refused_and_with_it_overwrites() {
        let d = dir("replace");
        let store = EncryptedAccountStore::open_or_create_with_secret(&d, "correct horse")
            .expect("created");
        store
            .add(OperatorId(1), Role::Operator, "first-phc".into(), false)
            .expect("added");
        let err = store
            .add(OperatorId(1), Role::Supervisor, "second-phc".into(), false)
            .expect_err("refused");
        assert!(matches!(err, SecurityError::Forbidden(_)));
        store
            .add(OperatorId(1), Role::Supervisor, "second-phc".into(), true)
            .expect("replaced");
        let account = store
            .account(OperatorId(1))
            .expect("looked up")
            .expect("present");
        assert_eq!(account.role, Role::Supervisor);
        assert_eq!(account.phc, "second-phc");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn assign_role_changes_the_role_and_leaves_the_hash_alone() {
        let d = dir("assign-role");
        let store = EncryptedAccountStore::open_or_create_with_secret(&d, "correct horse")
            .expect("created");
        store
            .add(OperatorId(3), Role::Operator, "unchanged-phc".into(), false)
            .expect("added");
        store
            .assign_role(OperatorId(3), Role::Administrator)
            .expect("assigned");
        let account = store
            .account(OperatorId(3))
            .expect("looked up")
            .expect("present");
        assert_eq!(account.role, Role::Administrator);
        assert_eq!(account.phc, "unchanged-phc");

        let err = store
            .assign_role(OperatorId(99), Role::Administrator)
            .expect_err("refused");
        assert!(matches!(err, SecurityError::Forbidden(_)));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn an_unknown_operator_is_not_an_error_it_is_none() {
        let d = dir("unknown");
        let store = EncryptedAccountStore::open_or_create_with_secret(&d, "correct horse")
            .expect("created");
        assert_eq!(store.account(OperatorId(404)).expect("looked up"), None);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn the_file_holds_no_plaintext_account() {
        let d = dir("opaque");
        let store = EncryptedAccountStore::open_or_create_with_secret(&d, "correct horse")
            .expect("created");
        store
            .add(
                OperatorId(7),
                Role::SecurityOfficer,
                "$argon2id$v=19$m=19456,t=2,p=1$abc$def".into(),
                false,
            )
            .expect("added");
        let bytes = std::fs::read(d.join(ACCOUNT_STORE_FILE)).expect("file");
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("\"ciphertext\""));
        assert!(
            !text.contains("argon2id"),
            "nothing legible about the accounts"
        );
        assert!(
            !text.contains("SecurityOfficer"),
            "nothing legible about the accounts"
        );
        let _ = std::fs::remove_dir_all(d);
    }
}
