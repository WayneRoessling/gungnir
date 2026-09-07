//! The passphrase-sealed keystore on the desktop (GAP-084, DN-22 amendment 3): a desktop
//! starts with the journal in the clear and says so; a sign-in opens the keystore, seals
//! the journal from there on, and writes the escrow record beside it; a second start
//! opens the same keys under the same passphrase.

use gungnir_app::keystore;
use gungnir_app::state::AppState;
use gungnir_config::{
    AuthenticationConfig, AuthenticationProvider, ConfigBaseline, EscrowConfig, KeyProviderConfig,
    SecurityConfig,
};
use gungnir_security::{
    EncryptionStatus, EscrowOfficerKey, EscrowedKey, KeyPurpose, KEYSTORE_FILE,
};
use gungnir_ui::panels::audit::{SessionAction, SignInDraft};

fn desktop(dir: &std::path::Path, officer_pem: Option<String>) -> AppState {
    let phc = gungnir_security::hash_passphrase("correct horse").expect("hashed");
    let accounts = dir.join("accounts.json");
    std::fs::write(
        &accounts,
        serde_json::json!([{"operator": 7, "role": "Operator", "phc": phc}]).to_string(),
    )
    .expect("accounts");
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        security: SecurityConfig {
            key_provider: KeyProviderConfig::PassphraseSealedFile,
            authentication: AuthenticationConfig {
                provider: AuthenticationProvider::LocalAccounts {
                    accounts_path: accounts.to_string_lossy().into_owned(),
                },
                session_lifetime_s: None,
            },
            escrow: officer_pem.map(|public_key_pem| EscrowConfig {
                holder: 9,
                public_key_pem,
            }),
            ..SecurityConfig::default()
        },
        ..ConfigBaseline::default()
    };
    gungnir_config::validate(&config).expect("valid");
    AppState::with_config(config).expect("starts")
}

fn sign_in(state: &mut AppState) {
    let mut draft = SignInDraft {
        operator: "7".into(),
        passphrase: "correct horse".into(),
        ..SignInDraft::default()
    };
    gungnir_app::session::apply(state, &mut draft, SessionAction::SignIn);
}

#[test]
fn a_sign_in_opens_the_keystore_seals_the_journal_and_escrows_the_key() {
    let dir = std::env::temp_dir().join(format!("gungnir-keystore-app-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let officer = EscrowOfficerKey::generate();
    let pem = officer.public_pem().expect("pem");

    let mut state = desktop(&dir, Some(pem.clone()));
    assert!(
        matches!(
            state.encryption,
            EncryptionStatus::UnavailableWritingPlaintext { .. }
        ),
        "before a sign-in the journal is in the clear and the strip says so: {:?}",
        state.encryption
    );
    assert!(state.keystore.is_none());

    sign_in(&mut state);
    assert!(state.attributed_operator().is_some(), "{:?}", state.alerts);
    assert!(
        matches!(state.encryption, EncryptionStatus::Active { .. }),
        "{:?}",
        state.alerts
    );
    let store = state.keystore.as_ref().expect("opened");
    assert!(dir.join(KEYSTORE_FILE).is_file());
    let key = gungnir_security::KeyProvider::active(store.as_ref(), KeyPurpose::JournalAtRest)
        .expect("journal key");
    let record_path = dir.join(keystore::escrow_record_name(&key));
    let record: EscrowedKey =
        serde_json::from_str(&std::fs::read_to_string(&record_path).expect("escrow record"))
            .expect("parses");
    let recovered = officer.recover(&record).expect("the officer recovers it");
    let sealed =
        gungnir_security::KeyProvider::seal(store.as_ref(), &key, b"decision 12").expect("sealed");
    assert_eq!(recovered.unseal(&sealed).expect("opens"), b"decision 12");

    // A second start under the same passphrase opens the same key.
    drop(state);
    let mut again = desktop(&dir, Some(pem));
    sign_in(&mut again);
    let store = again.keystore.as_ref().expect("reopened");
    assert_eq!(
        gungnir_security::KeyProvider::active(store.as_ref(), KeyPurpose::JournalAtRest)
            .expect("key"),
        key
    );
    let _ = std::fs::remove_dir_all(dir);
}
