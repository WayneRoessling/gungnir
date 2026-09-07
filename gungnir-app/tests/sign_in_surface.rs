//! The sign-in surface (GAP-057, DN-23 §5, §8).
//!
//! `AppState::sign_in` had no caller. These are the caller's tests: the desktop builds
//! its authority from the baseline, a good credential attributes the session, a bad one
//! is refused indistinguishably and audited, a sign-out ends it, and a missing store
//! starts the desktop unattributed and says so.

use gungnir_app::session;
use gungnir_app::state::AppState;
use gungnir_config::{
    AuthenticationConfig, AuthenticationProvider, ConfigBaseline, SecurityConfig,
};
use gungnir_security::{hash_passphrase, Account, AuditLog, OperatorId, Role, SessionState};
use gungnir_ui::panels::audit::{SessionAction, SignInDraft};

const PASSPHRASE: &str = "correct horse battery staple";

fn desktop(name: &str, write_store: bool) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("gungnir-sign-in-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("data dir");
    if write_store {
        let accounts = vec![Account {
            operator: OperatorId(7),
            role: Role::SensorManager,
            phc: hash_passphrase(PASSPHRASE).expect("hashed"),
        }];
        std::fs::write(
            dir.join("accounts.json"),
            serde_json::to_string(&accounts).expect("json"),
        )
        .expect("written");
    }
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        security: SecurityConfig {
            authentication: AuthenticationConfig {
                provider: AuthenticationProvider::LocalAccounts {
                    accounts_path: "accounts.json".into(),
                },
                session_lifetime_s: None,
            },
            ..SecurityConfig::default()
        },
        ..ConfigBaseline::default()
    };
    gungnir_config::validate(&config).expect("valid");
    let state = AppState::with_config(config).expect("the desktop starts");
    (state, dir)
}

fn draft(operator: &str, passphrase: &str) -> SignInDraft {
    SignInDraft {
        operator: operator.into(),
        passphrase: passphrase.into(),
        ..SignInDraft::default()
    }
}

/// A good credential attributes the session; the passphrase is gone from the draft.
#[test]
fn a_good_credential_signs_the_operator_in() {
    let (mut state, dir) = desktop("good", true);
    assert!(matches!(
        state.session_state(),
        SessionState::NobodySignedIn
    ));
    let mut d = draft("7", PASSPHRASE);
    session::apply(&mut state, &mut d, SessionAction::SignIn);
    assert_eq!(state.attributed_operator(), Some(OperatorId(7)));
    assert!(
        d.passphrase.is_empty(),
        "the passphrase stayed in the draft"
    );
    assert!(state.accounts.as_ref().is_ok_and(|a| a.len() == 1));

    session::apply(&mut state, &mut d, SessionAction::SignOut);
    assert!(matches!(
        state.session_state(),
        SessionState::NobodySignedIn
    ));
    let _ = std::fs::remove_dir_all(dir);
}

/// **A bad secret and an unknown operator fail the same way** (DN-23 §5 rule 4), each
/// attempt is audited (rule 7), and nothing is attributed.
#[test]
fn a_bad_credential_is_refused_indistinguishably_and_audited() {
    let (mut state, dir) = desktop("bad", true);
    let before = state.audit.entries().len();
    session::apply(&mut state, &mut draft("7", "wrong"), SessionAction::SignIn);
    let wrong_secret = state.alerts.last().cloned().expect("an alert");
    session::apply(
        &mut state,
        &mut draft("99", PASSPHRASE),
        SessionAction::SignIn,
    );
    let unknown = state.alerts.last().cloned().expect("an alert");
    assert_eq!(
        wrong_secret, unknown,
        "the failure told which half was wrong"
    );
    assert!(state.attributed_operator().is_none());
    assert!(
        state.audit.entries().len() >= before + 2,
        "failed attempts were not audited"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// **The desktop starts without its store** (DN-23 §5 rule 5) and says why nobody can
/// sign in; the form is not offered as if it could work.
#[test]
fn a_missing_store_starts_the_desktop_and_says_so() {
    let (state, dir) = desktop("missing", false);
    assert!(matches!(
        state.session_state(),
        SessionState::StoreUnavailable { .. }
    ));
    assert!(state.accounts.is_err());
    let audit = gungnir_app::sustainment::audit_lines(&state);
    let view = session::audit_view(&state, &[], &audit, &[]);
    assert!(!view.can_sign_in);
    assert!(state
        .alerts
        .iter()
        .any(|a| a.contains("account store could not be read")));
    let _ = std::fs::remove_dir_all(dir);
}
