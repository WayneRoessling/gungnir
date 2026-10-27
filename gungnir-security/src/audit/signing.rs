// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The desktop's audit anchors, signed under a key the operator's session does not hold
//! (GAP-208, D-142; `docs/design/DN-23-operator-authentication.md` §16).
//!
//! **Human-owned** (`gungnir-security`).
//!
//! # The problem this answers
//!
//! The audit chain is unkeyed: anyone who can write the file can recompute it (see
//! [`super::anchor`]). Its head is kept in the event journal, which is sealed under the
//! desktop's keystore, and on a disconnected desktop that keystore is unlocked by the
//! operator's own passphrase or OS session. So the operator who can rewrite the audit file
//! can also reseal the journal with a head that matches the rewrite.
//!
//! # What this adds
//!
//! An [`AUDIT_SIGNING_DIR`] keystore of its own, the same sealed-file format as the
//! desktop's keystore (DN-22 amendment 3), holding one `AuditSigning` key. Its passphrase
//! is the **administrator's**, not the operator's (the owner's choice of 2026-10-27): it
//! is provisioned once with `audit-key provision`, which prints the public half for the
//! baseline, and an administrator unlocks it when the desktop starts. Every anchor is then
//! signed, and a verifier holding only the baseline's public key can tell an anchor this
//! key made from one written by anybody else.
//!
//! # What it does not claim
//!
//! Once unlocked, the key is in the running desktop's memory, and the operator runs that
//! process. Someone able to read its memory can sign. What this stops is the rewrite made
//! with a text editor and the operator's own keystore, which is the attack the review
//! found; what stops the rest is the node, which holds the heads forwarded while linked
//! and judges every signature against the baseline's key.

use std::path::Path;

use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};
use p256::pkcs8::DecodePublicKey;

use super::SegmentHead;
use crate::keys::{KeyId, KeyProvider, KeyPurpose, SignatureScheme};
use crate::keystore::{PersistentKeyProvider, KEYSTORE_FILE};
use crate::SecurityError;

/// The directory under the data directory that holds the audit-signing keystore, apart
/// from the desktop's own keystore so the two are never opened by one passphrase.
pub const AUDIT_SIGNING_DIR: &str = "audit-signing";

/// The shortest administrator passphrase `audit-key provision` accepts.
///
/// The sealed file is only as strong as this passphrase against an offline attack on a
/// copied file (DN-22 amendment 3 says the same of the desktop's), and the people it
/// keeps out are the people sitting at the machine.
pub const MIN_PASSPHRASE_CHARS: usize = 12;

/// The scheme every anchor is signed under, as a verifier and the record name it.
pub const SCHEME: &str = "ecdsa-p256-sha256";

/// The bytes an anchor's signature covers: a fixed label and version, then the head's
/// four fields, one per line. Written out rather than serialized, so no change in a
/// serializer can change what was signed.
#[must_use]
pub fn anchor_message(head: &SegmentHead) -> Vec<u8> {
    format!(
        "gungnir-audit-anchor/1\n{}\n{}\n{}\n{}",
        head.segment, head.entries, head.seq, head.hash
    )
    .into_bytes()
}

/// An anchor's signature: the key version that made it and the DER-encoded ECDSA
/// signature.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AnchorSignature {
    pub key_version: u32,
    pub der: Vec<u8>,
}

/// The audit-signing key, unlocked (GAP-208).
pub struct AuditSigner {
    provider: PersistentKeyProvider,
    key: KeyId,
}

impl std::fmt::Debug for AuditSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuditSigner")
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

impl AuditSigner {
    /// Make the audit-signing keystore under `data_dir`, sealed under `passphrase`, and
    /// return the public half as PEM for the baseline's `security.audit_signing`.
    ///
    /// # Errors
    ///
    /// A passphrase shorter than [`MIN_PASSPHRASE_CHARS`]; a keystore already there,
    /// which is never replaced, because anchors signed under it would stop verifying; or
    /// one that cannot be written.
    pub fn provision(data_dir: &Path, passphrase: &str) -> Result<String, SecurityError> {
        if passphrase.chars().count() < MIN_PASSPHRASE_CHARS {
            return Err(SecurityError::KeyProviderUnavailable(format!(
                "an audit-signing passphrase needs at least {MIN_PASSPHRASE_CHARS} characters"
            )));
        }
        let dir = data_dir.join(AUDIT_SIGNING_DIR);
        if dir.join(KEYSTORE_FILE).exists() {
            return Err(SecurityError::KeyProviderUnavailable(format!(
                "{} already holds an audit-signing key; it is never replaced, because every \
                 anchor signed under it would stop verifying",
                dir.display()
            )));
        }
        std::fs::create_dir_all(&dir).map_err(|e| {
            SecurityError::KeyProviderUnavailable(format!("creating {}: {e}", dir.display()))
        })?;
        let provider = PersistentKeyProvider::open_or_create(&dir, passphrase, None)?;
        let key = provider.active_or_generate(KeyPurpose::AuditSigning)?;
        public_pem(&provider.public_key_der(&key)?)
    }

    /// Open the audit-signing keystore under `data_dir` with the administrator's
    /// passphrase. **Never creates one**: a desktop with none says so and runs unsigned.
    ///
    /// # Errors
    ///
    /// No keystore provisioned; one that does not open under this passphrase; or one that
    /// holds no audit-signing key.
    pub fn unlock(data_dir: &Path, passphrase: &str) -> Result<Self, SecurityError> {
        let dir = data_dir.join(AUDIT_SIGNING_DIR);
        if !dir.join(KEYSTORE_FILE).is_file() {
            return Err(SecurityError::KeyProviderUnavailable(format!(
                "no audit-signing key is provisioned in {}; an administrator runs \
                 `audit-key provision` once",
                dir.display()
            )));
        }
        let provider = PersistentKeyProvider::open_or_create(&dir, passphrase, None)?;
        let key = provider.active(KeyPurpose::AuditSigning)?;
        Ok(Self { provider, key })
    }

    /// Sign one anchor.
    ///
    /// # Errors
    ///
    /// When the provider refuses, which an unlocked key does not.
    pub fn sign(&self, head: &SegmentHead) -> Result<AnchorSignature, SecurityError> {
        let der = self.provider.sign(
            &self.key,
            &anchor_message(head),
            SignatureScheme::EcdsaP256Sha256,
        )?;
        Ok(AnchorSignature {
            key_version: self.key.version,
            der,
        })
    }

    /// The public half, PEM, for comparing with the baseline's.
    ///
    /// # Errors
    ///
    /// When the key cannot be encoded.
    pub fn public_key_pem(&self) -> Result<String, SecurityError> {
        public_pem(&self.provider.public_key_der(&self.key)?)
    }
}

fn public_pem(der: &[u8]) -> Result<String, SecurityError> {
    VerifyingKey::from_public_key_der(der)
        .and_then(|k| {
            p256::pkcs8::EncodePublicKey::to_public_key_pem(&k, p256::pkcs8::LineEnding::LF)
        })
        .map_err(|e| SecurityError::KeyProviderUnavailable(format!("encoding the key: {e}")))
}

/// What a verifier makes of one anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureCheck {
    /// Made by the key the baseline names.
    Valid,
    /// Signed, and not by that key, or not over this head.
    Invalid,
}

/// The baseline's audit-signing public key, for verifying anchors on the node and on the
/// desktop (GAP-208).
#[derive(Debug, Clone)]
pub struct AuditVerifyingKey(VerifyingKey);

impl AuditVerifyingKey {
    /// Read the public half from the baseline's PEM.
    ///
    /// # Errors
    ///
    /// When it is not a P-256 public key.
    pub fn from_pem(pem: &str) -> Result<Self, SecurityError> {
        VerifyingKey::from_public_key_pem(pem)
            .map(Self)
            .map_err(|e| {
                SecurityError::KeyProviderUnavailable(format!(
                    "the audit-signing public key is not a P-256 public key: {e}"
                ))
            })
    }

    /// Whether `der` is this key's signature over `head`.
    #[must_use]
    pub fn check(&self, head: &SegmentHead, der: &[u8]) -> SignatureCheck {
        match Signature::from_der(der) {
            Ok(signature) if self.0.verify(&anchor_message(head), &signature).is_ok() => {
                SignatureCheck::Valid
            }
            _ => SignatureCheck::Invalid,
        }
    }
}

/// Which of a set of heads were signed, and by what (GAP-208): the part of a
/// verification the gap's action asks for, kept apart from whether the file reaches them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SigningReport {
    /// Whether the baseline names a key to check against at all.
    pub key_configured: bool,
    /// Segments whose head the key made, with the key version.
    pub signed: Vec<(String, u32)>,
    /// Segments whose head carries no signature.
    pub unsigned: Vec<String>,
    /// Segments whose head carries a signature the key did not make.
    pub invalid: Vec<String>,
}

impl SigningReport {
    /// The damage among them: a signature that does not verify was written by something
    /// other than the deployment's key, which an unsigned head need not have been.
    #[must_use]
    pub fn problems(&self) -> Vec<String> {
        self.invalid
            .iter()
            .map(|segment| {
                format!(
                    "{segment}'s head carries a signature the deployment's audit-signing key \
                     did not make"
                )
            })
            .collect()
    }

    /// One sentence for PN-20.
    #[must_use]
    pub fn summary(&self) -> String {
        if !self.key_configured {
            return "No audit-signing key is configured, so no head is signed or checked."
                .to_owned();
        }
        let mut said = format!(
            "{} head(s) signed by the deployment's audit-signing key",
            self.signed.len()
        );
        if !self.unsigned.is_empty() {
            let _ = std::fmt::Write::write_fmt(
                &mut said,
                format_args!(
                    "; {} UNSIGNED, taken while the key was locked: {}",
                    self.unsigned.len(),
                    self.unsigned.join(", ")
                ),
            );
        }
        if !self.invalid.is_empty() {
            let _ = std::fmt::Write::write_fmt(
                &mut said,
                format_args!(
                    "; {} with a signature that does NOT verify: {}",
                    self.invalid.len(),
                    self.invalid.join(", ")
                ),
            );
        }
        said.push('.');
        said
    }
}

/// Sort `heads` by their signatures against `key` (GAP-208). With no key, every head is
/// unchecked and the report says so rather than calling them unsigned.
pub fn signing_report<'a>(
    heads: impl IntoIterator<Item = &'a SegmentHead>,
    key: Option<&AuditVerifyingKey>,
) -> SigningReport {
    let mut report = SigningReport {
        key_configured: key.is_some(),
        ..SigningReport::default()
    };
    let Some(key) = key else {
        return report;
    };
    for head in heads {
        match &head.signature {
            None => report.unsigned.push(head.segment.clone()),
            Some(signature) => match key.check(head, &signature.der) {
                SignatureCheck::Valid => report
                    .signed
                    .push((head.segment.clone(), signature.key_version)),
                SignatureCheck::Invalid => report.invalid.push(head.segment.clone()),
            },
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gungnir-audit-signing-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn head(entries: u64, hash: &str) -> SegmentHead {
        SegmentHead {
            segment: "audit-000001.jsonl".into(),
            entries,
            seq: entries,
            hash: hash.into(),
            signature: None,
        }
    }

    const ADMIN: &str = "correct horse battery staple";

    /// Provisioned once, unlocked by the administrator's passphrase only, and an anchor it
    /// signs verifies under the printed public key and under nothing else.
    #[test]
    fn a_provisioned_key_signs_anchors_the_baseline_key_verifies() {
        let dir = temp("round-trip");
        let pem = AuditSigner::provision(&dir, ADMIN).expect("provisioned");
        assert!(pem.contains("BEGIN PUBLIC KEY") && !pem.contains("PRIVATE"));
        assert!(
            AuditSigner::provision(&dir, ADMIN).is_err(),
            "never replaced"
        );
        assert!(
            AuditSigner::unlock(&dir, "the operator's guess").is_err(),
            "another passphrase does not open it"
        );
        let signer = AuditSigner::unlock(&dir, ADMIN).expect("unlocked");
        assert_eq!(signer.public_key_pem().expect("pem"), pem);
        let verifier = AuditVerifyingKey::from_pem(&pem).expect("a key");
        let anchored = head(10, "abc");
        let signature = signer.sign(&anchored).expect("anchored");
        assert_eq!(signature.key_version, 1);
        assert_eq!(
            verifier.check(&anchored, &signature.der),
            SignatureCheck::Valid
        );
        assert_eq!(
            verifier.check(&head(11, "abc"), &signature.der),
            SignatureCheck::Invalid,
            "a signature does not carry to another head"
        );
        assert_eq!(
            verifier.check(&anchored, b"not a signature"),
            SignatureCheck::Invalid
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn nothing_is_created_by_unlocking_and_a_short_passphrase_is_refused() {
        let dir = temp("refusals");
        assert!(AuditSigner::unlock(&dir, ADMIN).is_err());
        assert!(
            !dir.join(AUDIT_SIGNING_DIR).exists(),
            "unlock never creates"
        );
        assert!(AuditSigner::provision(&dir, "short").is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The report sorts heads three ways, and only a signature that does not verify is
    /// damage: a head taken while the key was locked is said, not condemned.
    #[test]
    fn the_report_names_signed_unsigned_and_forged_heads() {
        let dir = temp("report");
        let pem = AuditSigner::provision(&dir, ADMIN).expect("provisioned");
        let signer = AuditSigner::unlock(&dir, ADMIN).expect("unlocked");
        let key = AuditVerifyingKey::from_pem(&pem).expect("a key");
        let mut anchored = head(4, "a");
        anchored.signature = Some(signer.sign(&anchored).expect("anchored"));
        let mut forged = head(9, "rewritten");
        forged.segment = "audit-000002.jsonl".into();
        // A signature over the original head, carried onto a rewritten one.
        forged.signature = anchored.signature.clone();
        let mut unsigned = head(2, "b");
        unsigned.segment = "audit-000003.jsonl".into();
        let report = signing_report([&anchored, &forged, &unsigned], Some(&key));
        assert_eq!(report.signed, vec![("audit-000001.jsonl".to_owned(), 1)]);
        assert_eq!(report.invalid, vec!["audit-000002.jsonl".to_owned()]);
        assert_eq!(report.unsigned, vec!["audit-000003.jsonl".to_owned()]);
        assert_eq!(report.problems().len(), 1);
        assert!(
            report.summary().contains("UNSIGNED"),
            "{}",
            report.summary()
        );
        let unchecked = signing_report([&anchored], None);
        assert!(!unchecked.key_configured && unchecked.unsigned.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A key from another deployment does not verify this one's anchors.
    #[test]
    fn another_deployments_key_does_not_verify() {
        let ours = temp("ours");
        let theirs = temp("theirs");
        let _ = AuditSigner::provision(&ours, ADMIN).expect("ours");
        let their_pem = AuditSigner::provision(&theirs, ADMIN).expect("theirs");
        let signer = AuditSigner::unlock(&ours, ADMIN).expect("unlocked");
        let anchored = head(3, "def");
        let signature = signer.sign(&anchored).expect("anchored");
        let verifier = AuditVerifyingKey::from_pem(&their_pem).expect("a key");
        assert_eq!(
            verifier.check(&anchored, &signature.der),
            SignatureCheck::Invalid
        );
        let _ = std::fs::remove_dir_all(ours);
        let _ = std::fs::remove_dir_all(theirs);
    }
}
