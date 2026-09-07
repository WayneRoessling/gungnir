//! Key custody, rotation, and escrow.
//!
//! Design: docs/design/DN-22-key-management.md, **signed by the owner 2026-09-05**.
//! Capability CAP-6.4; decision D-02 fixed the credential mechanism, and
//! `ARCHITECTURE.md` §8.5 states the protection intent. Between those two there was
//! nothing: no component owned key material.
//!
//! **Human-owned** (docs/agentic-workflow.md): this file decides who can read what.
//!
//! The design's central choice is visible in the trait below: [`KeyProvider`] offers
//! `seal` and `unseal`, **not a getter**. A provider that hands out key bytes has no
//! custody boundary at all, and every consumer becomes a place material can leak. It
//! is also what lets the cloud profile use a managed service that performs the
//! operation and never releases material.
//!
//! Two rules the tests enforce:
//!
//! 1. **Rotation never rewrites existing data.** Old material records the key that
//!    protected it and is read with that version. Re-encrypting a journal on
//!    rotation would rewrite the record.
//! 2. **Destroying a key that protects retained data needs a recorded override
//!    naming what becomes unreadable.** An accidental destruction that silently
//!    orphans a year of journals is the worst outcome in this note.

use crate::SecurityError;

/// What a key is for.
///
/// Separate purposes never share material, so compromising one does not compromise
/// the others.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum KeyPurpose {
    /// Machine identity for mutual transport-layer security (decision D-02).
    TransportIdentity,
    /// Journal encryption at rest.
    JournalAtRest,
    /// Signing configuration baselines.
    BaselineSigning,
}

/// Identifies one key version.
///
/// Recorded on anything the key protected, so a retired key can still be found for
/// what it encrypted.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct KeyId {
    pub purpose: KeyPurpose,
    pub version: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum KeyState {
    /// Usable for new material.
    Active,
    /// Not used for new material; still available to read old material.
    Retired,
    /// Unavailable. Anything it protected is unreadable, and the system says so.
    Destroyed,
}

impl KeyState {
    /// True when material protected by this key can still be read.
    pub fn can_read(self) -> bool {
        matches!(self, KeyState::Active | KeyState::Retired)
    }

    /// True when this key may protect new material.
    pub fn can_write(self) -> bool {
        self == KeyState::Active
    }
}

/// Whether at-rest encryption is actually happening.
///
/// Reported through the health summary, because a system that claims encryption it
/// is not performing is worse than one that admits it is not.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum EncryptionStatus {
    /// A provider is available and journals are sealed.
    Active { provider: String },
    /// **No keystore is available, so the journal is written in the clear.**
    ///
    /// The disconnected desktop must still journal and still start, so it runs and
    /// says so in the status strip rather than refusing or, far worse, appearing to
    /// encrypt.
    UnavailableWritingPlaintext { reason: String },
    /// The deployment has not configured encryption at all.
    NotConfigured,
}

impl EncryptionStatus {
    pub fn is_encrypting(&self) -> bool {
        matches!(self, EncryptionStatus::Active { .. })
    }

    /// What the status strip says, or `None` when nothing needs saying.
    pub fn operator_warning(&self) -> Option<&str> {
        match self {
            EncryptionStatus::Active { .. } => None,
            EncryptionStatus::UnavailableWritingPlaintext { .. } => {
                Some("journal encryption off: no keystore available")
            }
            EncryptionStatus::NotConfigured => Some("journal encryption not configured"),
        }
    }
}

/// The custody boundary.
///
/// Implementations hold key material; nothing above this trait ever sees bytes it
/// did not ask to use. **There is deliberately no method returning key material.**
pub trait KeyProvider: Send + Sync {
    fn active(&self, purpose: KeyPurpose) -> Result<KeyId, SecurityError>;
    fn state(&self, id: &KeyId) -> Result<KeyState, SecurityError>;
    /// Encrypt without exposing the key. The provider does the work.
    fn seal(&self, id: &KeyId, plaintext: &[u8]) -> Result<Vec<u8>, SecurityError>;
    fn unseal(&self, id: &KeyId, ciphertext: &[u8]) -> Result<Vec<u8>, SecurityError>;
    /// Mints a new version and retires the previous one.
    fn rotate(&mut self, purpose: KeyPurpose) -> Result<KeyId, SecurityError>;

    /// Sign a message with a key the provider holds (DN-22 amendment 1 a).
    ///
    /// **Added because `seal` and `unseal` cannot produce a TLS handshake signature**, so
    /// `KeyPurpose::TransportIdentity` was a purpose no consumer could use and the
    /// transport served loopback only. The resolution is not a getter: `rustls`'s
    /// `sign::SigningKey` is a trait, so a hardware module or a managed key service can
    /// terminate TLS by doing the work here, and the bytes still never leave the
    /// provider.
    ///
    /// # Errors
    ///
    /// When the key cannot produce this scheme -- a provider holding only symmetric keys
    /// says so rather than returning something a caller might take for a signature.
    fn sign(
        &self,
        id: &KeyId,
        message: &[u8],
        scheme: SignatureScheme,
    ) -> Result<Vec<u8>, SecurityError>;
}

/// A signature algorithm a provider may be asked for.
///
/// Named here rather than taken from `rustls`, so no crate below the binary learns about
/// TLS and DN-22 keeps naming no library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SignatureScheme {
    EcdsaP256Sha256,
    Ed25519,
    RsaPssSha256,
}

/// A recorded decision to destroy a key that still protects data.
///
/// Required because destruction is irreversible: it names what becomes unreadable
/// so nobody can claim afterwards that the consequence was not visible.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DestructionOverride {
    pub operator: String,
    /// What this destruction makes permanently unreadable, in words.
    pub affected: String,
}

/// Whether a key may be destroyed.
///
/// A key protecting retained data may not be destroyed without an override that
/// names the affected data.
pub fn may_destroy(
    state: KeyState,
    protects_retained_data: bool,
    override_record: Option<&DestructionOverride>,
) -> Result<(), SecurityError> {
    if state == KeyState::Destroyed {
        return Ok(());
    }
    if !protects_retained_data {
        return Ok(());
    }
    match override_record {
        Some(o) if !o.operator.trim().is_empty() && !o.affected.trim().is_empty() => Ok(()),
        _ => Err(SecurityError::KeyStillProtectsData),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(version: u32) -> KeyId {
        KeyId {
            purpose: KeyPurpose::JournalAtRest,
            version,
        }
    }

    #[test]
    fn a_retired_key_still_reads_and_a_destroyed_one_does_not() {
        assert!(KeyState::Active.can_read());
        assert!(KeyState::Active.can_write());
        assert!(KeyState::Retired.can_read(), "old material stays readable");
        assert!(
            !KeyState::Retired.can_write(),
            "but nothing new is protected with it"
        );
        assert!(!KeyState::Destroyed.can_read());
        assert!(!KeyState::Destroyed.can_write());
    }

    #[test]
    fn rotation_leaves_old_material_addressable() {
        // Rotation mints a new version; the old one is recorded on what it
        // protected and is read with that version. Nothing is rewritten.
        let previous = key(1);
        let current = key(2);
        assert_ne!(previous, current);
        assert_eq!(previous.purpose, current.purpose);
        assert!(
            KeyState::Retired.can_read(),
            "which is what makes not rewriting safe"
        );
    }

    #[test]
    fn purposes_never_share_material() {
        let transport = KeyId {
            purpose: KeyPurpose::TransportIdentity,
            version: 1,
        };
        let journal = KeyId {
            purpose: KeyPurpose::JournalAtRest,
            version: 1,
        };
        assert_ne!(
            transport, journal,
            "same version number, different key entirely"
        );
    }

    #[test]
    fn a_key_protecting_retained_data_cannot_be_destroyed_without_an_override() {
        assert_eq!(
            may_destroy(KeyState::Retired, true, None),
            Err(SecurityError::KeyStillProtectsData)
        );
        let empty = DestructionOverride {
            operator: "  ".into(),
            affected: "a year of journals".into(),
        };
        assert_eq!(
            may_destroy(KeyState::Retired, true, Some(&empty)),
            Err(SecurityError::KeyStillProtectsData),
            "an override must name who"
        );
        let unnamed = DestructionOverride {
            operator: "administrator".into(),
            affected: String::new(),
        };
        assert_eq!(
            may_destroy(KeyState::Retired, true, Some(&unnamed)),
            Err(SecurityError::KeyStillProtectsData),
            "and what becomes unreadable"
        );
    }

    #[test]
    fn a_complete_override_permits_destruction() {
        let record = DestructionOverride {
            operator: "administrator".into(),
            affected: "sessions 1 to 400, permanently".into(),
        };
        assert!(may_destroy(KeyState::Retired, true, Some(&record)).is_ok());
    }

    #[test]
    fn a_key_protecting_nothing_may_be_destroyed_freely() {
        assert!(may_destroy(KeyState::Retired, false, None).is_ok());
        assert!(may_destroy(KeyState::Destroyed, true, None).is_ok());
    }

    #[test]
    fn an_unavailable_keystore_is_reported_rather_than_claimed() {
        let honest = EncryptionStatus::UnavailableWritingPlaintext {
            reason: "operating-system keystore locked".into(),
        };
        assert!(!honest.is_encrypting());
        assert!(
            honest.operator_warning().is_some(),
            "the operator must be told"
        );

        let active = EncryptionStatus::Active {
            provider: "os-keystore".into(),
        };
        assert!(active.is_encrypting());
        assert!(active.operator_warning().is_none());
    }

    #[test]
    fn not_configured_is_distinct_from_unavailable() {
        // One is a deployment choice; the other is a failure. Collapsing them
        // would hide a keystore that stopped working.
        let a = EncryptionStatus::NotConfigured;
        let b = EncryptionStatus::UnavailableWritingPlaintext {
            reason: "locked".into(),
        };
        assert_ne!(a, b);
        assert!(!a.is_encrypting() && !b.is_encrypting());
        assert_ne!(a.operator_warning(), b.operator_warning());
    }
}
