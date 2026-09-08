// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! What the desktop reports about journal encryption (GAP-084, DN-22 §5).
//!
//! **The desktop starts either way.** DN-22 §5's disconnected fallback says a console
//! whose keystore is unavailable runs and says so, rather than refusing or -- far worse --
//! appearing to encrypt. These are the tests behind that sentence.

use gungnir_app::state::AppState;
use gungnir_config::{ConfigBaseline, EscrowConfig, KeyProviderConfig, SecurityConfig};
use gungnir_security::EncryptionStatus;

fn desktop(name: &str, provider: KeyProviderConfig) -> (AppState, std::path::PathBuf) {
    desktop_with_escrow(name, provider, None)
}

fn desktop_with_escrow(
    name: &str,
    provider: KeyProviderConfig,
    escrow: Option<EscrowConfig>,
) -> (AppState, std::path::PathBuf) {
    let dir =
        std::env::temp_dir().join(format!("gungnir-encryption-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        security: SecurityConfig {
            key_provider: provider,
            authentication: gungnir_config::AuthenticationConfig::default(),
            tls: gungnir_config::TlsClientConfig::default(),
            escrow,
        },
        ..ConfigBaseline::default()
    };
    (
        AppState::with_config(config).expect("the desktop starts"),
        dir,
    )
}

/// The default deployment: nothing configured, nothing claimed.
#[test]
fn a_deployment_with_no_provider_reports_not_configured() {
    let (state, dir) = desktop("none", KeyProviderConfig::None);
    assert_eq!(state.encryption, EncryptionStatus::NotConfigured);
    assert!(!state.encryption.is_encrypting());
    // Not a fault, so it does not shout: nobody asked for encryption here.
    assert!(state.encryption.operator_warning().is_some());
    let _ = std::fs::remove_dir_all(dir);
}

/// The ephemeral provider encrypts, and **says the thing that would otherwise be a
/// silent trap**: the journal it produces is real ciphertext nothing will read again.
#[test]
fn the_ephemeral_provider_encrypts_and_warns_that_the_journal_dies_with_it() {
    let (state, dir) = desktop("ephemeral", KeyProviderConfig::Ephemeral);
    assert!(state.encryption.is_encrypting());
    assert!(
        state
            .alerts
            .iter()
            .any(|a| a.contains("cannot be read after the application closes")),
        "no warning that the journal is unreadable after a restart: {:?}",
        state.alerts
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// **The failure DN-22 §5 cares about most.** A provider that is designed and not built
/// leaves the journal in the clear, and the desktop **starts**, reports the fault, and
/// does not claim encryption.
#[test]
fn an_unbuilt_provider_leaves_the_desktop_running_and_honest() {
    let (state, dir) = desktop(
        "unbuilt",
        KeyProviderConfig::ManagedService {
            endpoint: "https://kms.example.gov".into(),
            key_ring: "journal".into(),
        },
    );

    match &state.encryption {
        EncryptionStatus::UnavailableWritingPlaintext { reason } => {
            assert!(reason.contains("GAP-084"), "{reason}");
        }
        other => panic!("expected an honest unencrypted state, got {other:?}"),
    }
    assert!(
        !state.encryption.is_encrypting(),
        "a deployment that is not encrypting reported that it was"
    );
    assert!(
        state.alerts.iter().any(|a| a.contains("encryption is off")),
        "the operator was not told: {:?}",
        state.alerts
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// D-39: the OS keystore is unlocked at operator login, so unlike the passphrase-sealed
/// file it encrypts from the first frame -- no sign-in needed. Honest either way: on a
/// machine with no reachable keystore (a headless Linux CI runner) this is DN-22 §5's
/// fallback, exercised for real rather than skipped.
#[test]
fn the_os_keystore_provider_encrypts_from_start_or_says_plainly_why_not() {
    let account = format!("encryption-status-test-{}", std::process::id());
    let (state, dir) = desktop(
        "os-keystore",
        KeyProviderConfig::OperatingSystemKeystore {
            account: account.clone(),
        },
    );

    match &state.encryption {
        EncryptionStatus::Active { provider } => {
            assert_eq!(provider, "os-keystore");
            assert!(
                state.keystore.is_some(),
                "the provider should be reachable the same way a passphrase sign-in leaves it"
            );
            let entry = keyring::v1::Entry::new("gungnir-desktop-keystore", &account)
                .expect("the same entry the provider created");
            let _ = entry.delete_credential();
        }
        EncryptionStatus::UnavailableWritingPlaintext { reason } => {
            assert!(!reason.is_empty());
            assert!(state.keystore.is_none());
        }
        other @ EncryptionStatus::NotConfigured => {
            panic!("expected either an active or an honestly-unavailable state, got {other:?}")
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// **A gap this test exists because of**: `build_encryption`'s OS-keystore arm opened the
/// provider and sealed the journal but never called the escrow helper `unlock` already
/// used, so a baseline naming an officer would have silently gone unescrowed the moment
/// it named the OS keystore instead of the passphrase-sealed file. Escrow is a property
/// of the journal key, not of which provider unlocked it -- `write_escrow_record` is now
/// shared by both, and this proves the OS-keystore side of that sharing rather than just
/// the passphrase side `gungnir-app/tests/keystore.rs` already covered.
#[test]
fn the_os_keystore_provider_escrows_the_journal_key_same_as_the_passphrase_file() {
    let officer = gungnir_security::EscrowOfficerKey::generate();
    let pem = officer.public_pem().expect("pem");
    let account = format!("encryption-status-escrow-test-{}", std::process::id());
    let (state, dir) = desktop_with_escrow(
        "os-keystore-escrow",
        KeyProviderConfig::OperatingSystemKeystore {
            account: account.clone(),
        },
        Some(EscrowConfig {
            holder: 9,
            public_key_pem: pem,
        }),
    );

    let cleanup = || {
        let _ = keyring::v1::Entry::new("gungnir-desktop-keystore", &account)
            .and_then(|e| e.delete_credential());
        let _ = std::fs::remove_dir_all(&dir);
    };

    let store = match &state.encryption {
        EncryptionStatus::Active { .. } => state.keystore.as_ref().expect("opened"),
        EncryptionStatus::UnavailableWritingPlaintext { reason } => {
            // DN-22 §5's honest fallback on a machine with no reachable keystore; there
            // is no journal key to escrow, and asserting one exists would be the
            // fabricated pass this whole feature was built not to produce.
            assert!(!reason.is_empty());
            cleanup();
            return;
        }
        other @ EncryptionStatus::NotConfigured => {
            cleanup();
            panic!("expected either an active or an honestly-unavailable state, got {other:?}")
        }
    };
    let key = gungnir_security::KeyProvider::active(
        store.as_ref(),
        gungnir_security::KeyPurpose::JournalAtRest,
    )
    .expect("journal key");
    let record_path = dir.join(gungnir_app::keystore::escrow_record_name(&key));
    let record: gungnir_security::EscrowedKey =
        serde_json::from_str(&std::fs::read_to_string(&record_path).expect("escrow record"))
            .expect("parses");
    let recovered = officer.recover(&record).expect("the officer recovers it");
    let sealed =
        gungnir_security::KeyProvider::seal(store.as_ref(), &key, b"decision 39").expect("sealed");
    assert_eq!(recovered.unseal(&sealed).expect("opens"), b"decision 39");
    cleanup();
}

/// The status the strip draws follows the deployment, not the configuration: an
/// unencrypted deployment cannot produce an `Active` strip.
#[test]
fn the_strip_state_follows_what_is_actually_happening() {
    use gungnir_ui::panels::status_strip::EncryptionState;

    let (state, dir) = desktop("strip", KeyProviderConfig::None);
    let drawn = gungnir_app::status::encryption_state(&state.encryption);
    assert_eq!(drawn, EncryptionState::NotConfigured);
    assert!(drawn.warning().is_some());
    // Never configured is not a fault; a keystore that could not be reached is.
    assert!(!drawn.is_fault());
    assert!(EncryptionState::UnavailableWritingPlaintext { reason: "locked" }.is_fault());
    let _ = std::fs::remove_dir_all(dir);
}
