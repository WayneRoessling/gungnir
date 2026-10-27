// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Configuration baselines signed under `BaselineSigning`, and the rule each binary admits
//! one by (GAP-210; DN-22 §15; `ARCHITECTURE.md` §8.5).
//!
//! **Human-owned** (`gungnir-security`).
//!
//! A baseline carries the authority rules, the sensor allow-list, the exchange agreements
//! and the escrow key, so an unsigned one is the most valuable thing on a host to tamper
//! with. The purpose has existed since DN-22; until GAP-210 nothing signed or checked
//! anything with it.
//!
//! # What is signed, and where the key comes from
//!
//! The bytes are the host's to supply -- this crate cannot see a baseline -- and
//! `gungnir_config::ConfigBaseline::signing_bytes` gives them: the baseline's own compact
//! serialization under a fixed label, so the signature survives PN-14's apply writing the
//! file back pretty-printed, and breaks on any change of content. The signature sits
//! beside the baseline in `<baseline>.sig`.
//!
//! **The key that verifies is never in the baseline**, or whoever rewrote the baseline
//! would rewrite the key with it. It is a PEM file the deployment pins outside it, named by
//! [`BASELINE_KEY_ENV`], the way `GUNGNIR_TLS_CERT` names a node's identity.
//!
//! # The rule ([`BaselineTrust::admit`], the owner's choices of 2026-10-27)
//!
//! | | A node, or a desktop on the Remote backend | A disconnected desktop |
//! |---|---|---|
//! | Signed by the pinned key | Runs | Runs |
//! | Unsigned, or signed by anything else | **Refused** | Runs, and says so |
//! | No key pinned | Runs, and says baselines are unverified | Runs, and says so |
//! | A pinned key that does not read | **Refused** | Runs, and says so |
//!
//! The signing key lives wherever baselines are authored, sealed under a passphrase:
//! `baseline-key provision` makes it, `baseline sign` signs a file. It is never needed on a
//! host that only runs a baseline.

use std::path::Path;

use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};
use p256::pkcs8::{DecodePublicKey, EncodePublicKey};
use sha2::{Digest, Sha256};

use crate::keys::{KeyId, KeyProvider, KeyPurpose, SignatureScheme};
use crate::keystore::{PersistentKeyProvider, KEYSTORE_FILE};
use crate::SecurityError;

/// The environment variable naming the pinned baseline-signing public key file.
pub const BASELINE_KEY_ENV: &str = "GUNGNIR_BASELINE_KEY";

/// The directory, under the one `baseline-key provision` is given, holding the key.
pub const BASELINE_SIGNING_DIR: &str = "baseline-signing";

/// The scheme every baseline is signed under.
pub const SCHEME: &str = "ecdsa-p256-sha256";

/// The shortest passphrase `baseline-key provision` accepts, for the reason
/// `audit::signing::MIN_PASSPHRASE_CHARS` gives.
pub const MIN_PASSPHRASE_CHARS: usize = 12;

/// A key's fingerprint: the first sixteen hex digits of the SHA-256 of its public half,
/// SPKI DER. What an audit entry and a `.sig` file name the signer by.
#[must_use]
pub fn fingerprint_of(public_der: &[u8]) -> String {
    let digest = Sha256::digest(public_der);
    digest
        .iter()
        .take(8)
        .fold(String::with_capacity(16), |mut s, b| {
            let _ = std::fmt::Write::write_fmt(&mut s, format_args!("{b:02x}"));
            s
        })
}

/// The baseline-signing key, unlocked where baselines are authored (GAP-210).
pub struct BaselineSigner {
    provider: PersistentKeyProvider,
    key: KeyId,
}

impl std::fmt::Debug for BaselineSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BaselineSigner")
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

/// One baseline signature: who made it and the DER signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MadeSignature {
    pub key_version: u32,
    pub key_fingerprint: String,
    pub der: Vec<u8>,
}

impl BaselineSigner {
    /// Make the key under `dir`, sealed under `passphrase`, and return its public half as
    /// PEM for the deployment to pin. An existing key is never replaced: every baseline
    /// signed under it would stop verifying.
    ///
    /// # Errors
    ///
    /// A short passphrase, an existing key, or a directory that cannot be written.
    pub fn provision(dir: &Path, passphrase: &str) -> Result<String, SecurityError> {
        if passphrase.chars().count() < MIN_PASSPHRASE_CHARS {
            return Err(SecurityError::KeyProviderUnavailable(format!(
                "a baseline-signing passphrase needs at least {MIN_PASSPHRASE_CHARS} characters"
            )));
        }
        let dir = dir.join(BASELINE_SIGNING_DIR);
        if dir.join(KEYSTORE_FILE).exists() {
            return Err(SecurityError::KeyProviderUnavailable(format!(
                "{} already holds a baseline-signing key; it is never replaced, because every \
                 baseline signed under it would stop verifying",
                dir.display()
            )));
        }
        std::fs::create_dir_all(&dir).map_err(|e| {
            SecurityError::KeyProviderUnavailable(format!("creating {}: {e}", dir.display()))
        })?;
        let provider = PersistentKeyProvider::open_or_create(&dir, passphrase, None)?;
        let key = provider.active_or_generate(KeyPurpose::BaselineSigning)?;
        public_pem(&provider.public_key_der(&key)?)
    }

    /// Open the key under `dir`. Never creates one.
    ///
    /// # Errors
    ///
    /// None provisioned, a wrong passphrase, or no baseline-signing key in the store.
    pub fn unlock(dir: &Path, passphrase: &str) -> Result<Self, SecurityError> {
        let dir = dir.join(BASELINE_SIGNING_DIR);
        if !dir.join(KEYSTORE_FILE).is_file() {
            return Err(SecurityError::KeyProviderUnavailable(format!(
                "no baseline-signing key is provisioned in {}; run `baseline-key provision` \
                 there once",
                dir.display()
            )));
        }
        let provider = PersistentKeyProvider::open_or_create(&dir, passphrase, None)?;
        let key = provider.active(KeyPurpose::BaselineSigning)?;
        Ok(Self { provider, key })
    }

    /// Sign a baseline's signing bytes.
    ///
    /// # Errors
    ///
    /// When the provider refuses or the key cannot be encoded.
    pub fn sign(&self, bytes: &[u8]) -> Result<MadeSignature, SecurityError> {
        let der = self
            .provider
            .sign(&self.key, bytes, SignatureScheme::EcdsaP256Sha256)?;
        Ok(MadeSignature {
            key_version: self.key.version,
            key_fingerprint: fingerprint_of(&self.provider.public_key_der(&self.key)?),
            der,
        })
    }
}

fn public_pem(der: &[u8]) -> Result<String, SecurityError> {
    VerifyingKey::from_public_key_der(der)
        .and_then(|k| k.to_public_key_pem(p256::pkcs8::LineEnding::LF))
        .map_err(|e| SecurityError::KeyProviderUnavailable(format!("encoding the key: {e}")))
}

/// The pinned baseline-signing public key (GAP-210).
#[derive(Debug, Clone)]
pub struct BaselineVerifyingKey {
    key: VerifyingKey,
    fingerprint: String,
}

impl BaselineVerifyingKey {
    /// Read the public half from PEM. Whitespace around it is not part of it: the PEM
    /// parser refuses a second trailing newline, and `baseline-key provision`'s own output
    /// saved to a file ends with one.
    ///
    /// # Errors
    ///
    /// When it is not a P-256 public key.
    pub fn from_pem(pem: &str) -> Result<Self, String> {
        let pem = format!("{}\n", pem.trim());
        let key = VerifyingKey::from_public_key_pem(&pem)
            .map_err(|e| format!("not a P-256 public key: {e}"))?;
        let der = key
            .to_public_key_der()
            .map_err(|e| format!("the key does not encode: {e}"))?;
        Ok(Self {
            fingerprint: fingerprint_of(der.as_bytes()),
            key,
        })
    }

    /// The key [`BASELINE_KEY_ENV`] names: `Ok(None)` when it names none.
    ///
    /// # Errors
    ///
    /// A file that cannot be read or is not a key: a pin the deployment made and that
    /// does not hold, which is not the same thing as no pin.
    pub fn pinned() -> Result<Option<Self>, String> {
        match std::env::var(BASELINE_KEY_ENV) {
            Ok(path) if !path.trim().is_empty() => {
                Self::from_file(Path::new(path.trim())).map(Some)
            }
            _ => Ok(None),
        }
    }

    /// The key in `path`.
    ///
    /// # Errors
    ///
    /// As [`Self::pinned`].
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let pem = std::fs::read_to_string(path)
            .map_err(|e| format!("{BASELINE_KEY_ENV} names {}: {e}", path.display()))?;
        Self::from_pem(&pem)
            .map_err(|e| format!("{BASELINE_KEY_ENV} names {}: {e}", path.display()))
    }

    #[must_use]
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    fn verifies(&self, bytes: &[u8], der: &[u8]) -> bool {
        Signature::from_der(der).is_ok_and(|s| self.key.verify(bytes, &s).is_ok())
    }
}

/// What a baseline's signature says about it, against the pinned key (GAP-210).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaselineTrust {
    /// Signed by the pinned key, this version.
    Signed {
        fingerprint: String,
        key_version: u32,
    },
    /// No `.sig` beside it.
    Unsigned,
    /// A `.sig` that is not the pinned key's signature over this baseline, and why.
    BadSignature(String),
    /// The deployment pins no key, so nothing can be checked.
    NoKeyPinned,
    /// The deployment pins a key that does not read.
    KeyUnreadable(String),
    /// The built-in default baseline: no file, so nothing anyone could have edited.
    BuiltIn,
}

/// The signature beside a baseline, as the host read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentedSignature {
    pub key_version: u32,
    pub key_fingerprint: String,
    pub der: Vec<u8>,
}

impl BaselineTrust {
    /// Judge `bytes` and the signature beside them against `key`.
    #[must_use]
    pub fn judge(
        key: &Result<Option<BaselineVerifyingKey>, String>,
        bytes: &[u8],
        signature: Option<&PresentedSignature>,
    ) -> Self {
        let key = match key {
            Err(why) => return Self::KeyUnreadable(why.clone()),
            Ok(None) => return Self::NoKeyPinned,
            Ok(Some(key)) => key,
        };
        let Some(signature) = signature else {
            return Self::Unsigned;
        };
        if signature.key_fingerprint != key.fingerprint {
            return Self::BadSignature(format!(
                "signed by key {}, and the pinned key is {}",
                signature.key_fingerprint, key.fingerprint
            ));
        }
        if key.verifies(bytes, &signature.der) {
            Self::Signed {
                fingerprint: key.fingerprint.clone(),
                key_version: signature.key_version,
            }
        } else {
            Self::BadSignature(
                "the signature does not verify over this baseline: it was changed after it \
                 was signed"
                    .into(),
            )
        }
    }

    /// Whether a host may run this baseline: `Ok` with a warning to say, or `Err` with
    /// the refusal. `connected` is a node, or a desktop on the Remote backend.
    ///
    /// # Errors
    ///
    /// In a connected profile, an unsigned or mis-signed baseline, or a pinned key that
    /// does not read.
    pub fn admit(&self, connected: bool) -> Result<Option<String>, String> {
        let said = self.describe();
        match self {
            Self::Signed { .. } | Self::BuiltIn => Ok(None),
            Self::Unsigned | Self::BadSignature(_) | Self::KeyUnreadable(_) if connected => {
                Err(said)
            }
            // No key pinned, in any profile; or a disconnected desktop, whatever it read.
            Self::NoKeyPinned | Self::Unsigned | Self::BadSignature(_) | Self::KeyUnreadable(_) => {
                Ok(Some(said))
            }
        }
    }

    /// One sentence, for an alert, a log line and an audit entry.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Signed {
                fingerprint,
                key_version,
            } => format!("signed by baseline key {fingerprint} (version {key_version})"),
            Self::Unsigned => "UNSIGNED: no signature beside it".to_owned(),
            Self::BadSignature(why) => format!("its signature does NOT verify: {why}"),
            Self::NoKeyPinned => {
                format!("not verified: no baseline-signing key is pinned ({BASELINE_KEY_ENV})")
            }
            Self::KeyUnreadable(why) => {
                format!("not verified: the pinned key does not read ({why})")
            }
            Self::BuiltIn => "the built-in default, read from no file".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASS: &str = "the configuration authority's passphrase";

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gungnir-baseline-signing-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn presented(made: &MadeSignature) -> PresentedSignature {
        PresentedSignature {
            key_version: made.key_version,
            key_fingerprint: made.key_fingerprint.clone(),
            der: made.der.clone(),
        }
    }

    /// Signed, it verifies under the pinned key and only over the bytes it signed; the
    /// rule refuses the rest in a connected profile and says it on a desktop.
    #[test]
    fn a_signed_baseline_runs_and_a_changed_or_unsigned_one_is_refused_when_connected() {
        let dir = temp("judge");
        let pem = BaselineSigner::provision(&dir, PASS).expect("provisioned");
        assert!(
            BaselineSigner::provision(&dir, PASS).is_err(),
            "never replaced"
        );
        assert!(BaselineSigner::unlock(&dir, "a guess at it").is_err());
        let signer = BaselineSigner::unlock(&dir, PASS).expect("unlocked");
        let key: Result<Option<BaselineVerifyingKey>, String> =
            BaselineVerifyingKey::from_pem(&pem).map(Some);
        let bytes = b"gungnir-baseline/1\n{\"version\":1}";
        let made = signer.sign(bytes).expect("signed");
        assert_eq!(
            made.key_fingerprint,
            key.as_ref()
                .expect("a key")
                .as_ref()
                .expect("pinned")
                .fingerprint()
        );

        let verified = BaselineTrust::judge(&key, bytes, Some(&presented(&made)));
        assert!(matches!(
            verified,
            BaselineTrust::Signed { key_version: 1, .. }
        ));
        assert_eq!(verified.admit(true), Ok(None));

        let changed = BaselineTrust::judge(
            &key,
            b"gungnir-baseline/1\n{\"version\":2}",
            Some(&presented(&made)),
        );
        assert!(matches!(changed, BaselineTrust::BadSignature(_)));
        assert!(changed.admit(true).is_err());
        assert!(changed.admit(false).expect("a desktop runs").is_some());

        let bare = BaselineTrust::judge(&key, bytes, None);
        assert_eq!(bare, BaselineTrust::Unsigned);
        assert!(bare.admit(true).is_err());

        let unpinned = BaselineTrust::judge(&Ok(None), bytes, None);
        assert_eq!(unpinned, BaselineTrust::NoKeyPinned);
        assert!(unpinned.admit(true).expect("runs").is_some(), "and says so");

        let broken = BaselineTrust::judge(&Err("no such file".into()), bytes, None);
        assert!(broken.admit(true).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Another deployment's key, presented as its own fingerprint, is named rather than
    /// tried.
    #[test]
    fn a_signature_by_another_key_is_named() {
        let ours = temp("ours");
        let theirs = temp("theirs");
        let pem = BaselineSigner::provision(&ours, PASS).expect("ours");
        let _ = BaselineSigner::provision(&theirs, PASS).expect("theirs");
        let signer = BaselineSigner::unlock(&theirs, PASS).expect("unlocked");
        let bytes = b"anything";
        let made = signer.sign(bytes).expect("signed");
        let key = BaselineVerifyingKey::from_pem(&pem).map(Some);
        let judged = BaselineTrust::judge(&key, bytes, Some(&presented(&made)));
        assert!(
            matches!(&judged, BaselineTrust::BadSignature(why) if why.contains(&made.key_fingerprint)),
            "{judged:?}"
        );
        let _ = std::fs::remove_dir_all(ours);
        let _ = std::fs::remove_dir_all(theirs);
    }
}
