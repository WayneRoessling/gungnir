//! The passphrase-sealed keystore on the desktop (GAP-084; DN-22 amendment 3, §12).
//!
//! Opened at sign-in with the passphrase the operator just presented, never at start,
//! never from the baseline. Once open: the journal seals under the store's journal key
//! from this line on (earlier lines stay as they were written, plaintext, and the record
//! keeps that truth); the escrow record is written beside the journal when the baseline
//! names an officer, so the key can be recovered without this desktop; and the strip
//! says "encrypting" only once the journal actually is.

use std::sync::Arc;

use gungnir_config::KeyProviderConfig;
use gungnir_security::{
    EncryptionStatus, EscrowPublicKey, KeyId, KeyProvider, KeyPurpose, PersistentKeyProvider,
};
use gungnir_store::sealing::JournalSealer;
use gungnir_store::StoreError;

use crate::state::AppState;

struct KeystoreSealer {
    provider: Arc<PersistentKeyProvider>,
    key: KeyId,
}

impl JournalSealer for KeystoreSealer {
    fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>, StoreError> {
        self.provider
            .seal(&self.key, plaintext)
            .map_err(|e| StoreError::Sealing(e.to_string()))
    }

    fn unseal(&self, sealed: &[u8]) -> Result<Vec<u8>, StoreError> {
        self.provider
            .unseal(&self.key, sealed)
            .map_err(|e| StoreError::Sealing(e.to_string()))
    }
}

/// The escrow record's file name beside the journal, per journal key version.
#[must_use]
pub fn escrow_record_name(key: &KeyId) -> String {
    format!("escrow-journal-v{}.json", key.version)
}

/// Open the keystore with the passphrase a sign-in just verified. Nothing happens for a
/// baseline that names another provider.
pub fn unlock(state: &mut AppState, passphrase: &str) {
    if !matches!(
        state.config.security.key_provider,
        KeyProviderConfig::PassphraseSealedFile
    ) || state.keystore.is_some()
    {
        return;
    }
    let escrow = match &state.config.security.escrow {
        Some(e) => match EscrowPublicKey::from_pem(&e.public_key_pem) {
            Ok(key) => Some(key),
            Err(err) => {
                state.alerts.push(format!(
                    "the escrow public key in the baseline is unusable ({err}); journal keys \
                     will not be escrowed"
                ));
                None
            }
        },
        None => None,
    };
    let dir = std::path::PathBuf::from(&state.config.data_dir);
    let provider = match PersistentKeyProvider::open_or_create(&dir, passphrase, escrow) {
        Ok(p) => Arc::new(p),
        Err(err) => {
            state.encryption = EncryptionStatus::UnavailableWritingPlaintext {
                reason: format!("the keystore did not open: {err}"),
            };
            state.alerts.push(format!(
                "journal encryption is off: the keystore did not open ({err})"
            ));
            return;
        }
    };
    let key = match provider.active_or_generate(KeyPurpose::JournalAtRest) {
        Ok(key) => key,
        Err(err) => {
            state.encryption = EncryptionStatus::UnavailableWritingPlaintext {
                reason: format!("no journal key: {err}"),
            };
            state
                .alerts
                .push(format!("journal encryption is off: no journal key ({err})"));
            return;
        }
    };
    if provider.escrow_configured() {
        match provider.escrow_wrap(&key) {
            Ok(record) => {
                let path = dir.join(escrow_record_name(&key));
                match serde_json::to_string(&record)
                    .map_err(|e| e.to_string())
                    .and_then(|text| std::fs::write(&path, text).map_err(|e| e.to_string()))
                {
                    Ok(()) => state.alerts.push(format!(
                        "journal key v{} escrowed to the security officer ({})",
                        key.version,
                        path.display()
                    )),
                    Err(err) => state.alerts.push(format!(
                        "the escrow record could not be written ({err}); the journal key is not \
                         recoverable without this desktop"
                    )),
                }
            }
            Err(err) => state.alerts.push(format!("escrow failed: {err}")),
        }
    }
    state.journal.seal_with(Box::new(KeystoreSealer {
        provider: Arc::clone(&provider),
        key,
    }));
    state.encryption = EncryptionStatus::Active {
        provider: "passphrase-sealed keystore".into(),
    };
    state.keystore = Some(provider);
    state.alerts.push(
        "keystore opened at sign-in; the journal seals from here on (earlier lines are as \
         they were written)"
            .into(),
    );
}
