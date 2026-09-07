//! What the desktop reports about journal encryption (GAP-084, DN-22 §5).
//!
//! **The desktop starts either way.** DN-22 §5's disconnected fallback says a console
//! whose keystore is unavailable runs and says so, rather than refusing or -- far worse --
//! appearing to encrypt. These are the tests behind that sentence.

use gungnir_app::state::AppState;
use gungnir_config::{ConfigBaseline, KeyProviderConfig, SecurityConfig};
use gungnir_security::EncryptionStatus;

fn desktop(name: &str, provider: KeyProviderConfig) -> (AppState, std::path::PathBuf) {
    let dir =
        std::env::temp_dir().join(format!("gungnir-encryption-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        security: SecurityConfig {
            key_provider: provider,
            authentication: gungnir_config::AuthenticationConfig::default(),
            tls: gungnir_config::TlsClientConfig::default(),
            escrow: None,
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
        KeyProviderConfig::OperatingSystemKeystore {
            account: "gungnir".into(),
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
