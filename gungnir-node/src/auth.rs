// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Who may sign in to this node (GAP-057's node half, docs/design/DN-23-operator-authentication.md).
//! **Signed by the owner 2026-09-06.**
//!
//! The account store comes from the baseline's authentication section; the token
//! signing key comes from the environment, because a baseline may carry neither key
//! material nor a path to it (DN-22 §6). Each half missing is reported as what it is:
//! a node with no store authenticates nobody and says so; a node with no key mints no
//! token and says so. Neither silently serves its picture to whoever asks.

use gungnir_api::transport::AccountTokenAuthority;
use gungnir_config::{AuthenticationProvider, ConfigBaseline};

/// The environment variable the signing key is read from.
pub const TOKEN_KEY_ENV: &str = "GUNGNIR_TOKEN_KEY";

/// Why no caller authority was built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoAuthority {
    /// No signing key in the environment.
    NoSigningKey,
    /// The key was refused by the issuer (too short, for instance).
    KeyRefused(String),
    /// The baseline names no account provider.
    NoAccountProvider,
    /// The baseline names a file that could not be opened.
    StoreUnavailable(String),
}

impl std::fmt::Display for NoAuthority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NoAuthority::NoSigningKey => write!(f, "no token signing key in {TOKEN_KEY_ENV}"),
            NoAuthority::KeyRefused(why) => write!(f, "the token signing key was refused: {why}"),
            NoAuthority::NoAccountProvider => {
                write!(f, "the baseline names no authentication provider")
            }
            NoAuthority::StoreUnavailable(why) => {
                write!(f, "the account store is unavailable: {why}")
            }
        }
    }
}

/// Build the authority from the baseline and the environment.
///
/// # Errors
///
/// `NoAuthority`, naming the half that is missing.
pub fn build_caller_authority(
    config: &ConfigBaseline,
) -> Result<AccountTokenAuthority, NoAuthority> {
    let key = std::env::var(TOKEN_KEY_ENV).map_err(|_| NoAuthority::NoSigningKey)?;
    build_with_key(config, key.into_bytes())
}

/// The same, with the key supplied, so a test need not touch the environment.
///
/// # Errors
///
/// `NoAuthority`, as above.
pub fn build_with_key(
    config: &ConfigBaseline,
    key: Vec<u8>,
) -> Result<AccountTokenAuthority, NoAuthority> {
    let lifetime = config
        .security
        .authentication
        .session_lifetime_s
        .unwrap_or(900.0);
    let issuer = gungnir_security::TokenIssuer::new(key, lifetime)
        .map_err(|e| NoAuthority::KeyRefused(e.to_string()))?;
    let store: Box<dyn gungnir_security::AccountStore> =
        match &config.security.authentication.provider {
            AuthenticationProvider::None => return Err(NoAuthority::NoAccountProvider),
            AuthenticationProvider::LocalAccounts { accounts_path } => Box::new(
                gungnir_security::FileAccountStore::open(std::path::Path::new(accounts_path))
                    .map_err(|e| NoAuthority::StoreUnavailable(e.to_string()))?,
            ),
            AuthenticationProvider::OsKeystoreAccounts { account } => {
                let data_dir = config.node.clone().unwrap_or_default().data_dir;
                Box::new(
                    gungnir_security::EncryptedAccountStore::open_or_create(
                        std::path::Path::new(&data_dir),
                        account,
                    )
                    .map_err(|e| NoAuthority::StoreUnavailable(e.to_string()))?,
                )
            }
        };
    Ok(AccountTokenAuthority::new(store, issuer))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_api::transport::CallerAuthority;
    use gungnir_config::AuthenticationConfig;

    fn baseline_with_store(dir: &std::path::Path) -> ConfigBaseline {
        let phc = gungnir_security::hash_passphrase("correct horse").expect("hashed");
        let accounts = serde_json::json!([
            {"operator": 7, "role": "Operator", "phc": phc}
        ]);
        let path = dir.join("accounts.json");
        std::fs::write(&path, accounts.to_string()).expect("written");
        let mut config = ConfigBaseline::default();
        config.security.authentication = AuthenticationConfig {
            provider: AuthenticationProvider::LocalAccounts {
                accounts_path: path.to_string_lossy().into_owned(),
            },
            session_lifetime_s: Some(60.0),
        };
        config
    }

    #[test]
    fn a_file_account_signs_in_and_its_token_verifies_until_it_expires() {
        let dir = std::env::temp_dir().join(format!("gungnir-node-auth-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let config = baseline_with_store(&dir);
        let authority = build_with_key(
            &config,
            b"a-signing-key-of-adequate-length-0123456789".to_vec(),
        )
        .expect("built");
        assert!(authority.sign_in(7, "wrong", 0.0).is_err());
        let issued = authority
            .sign_in(7, "correct horse", 10.0)
            .expect("signed in");
        let session = authority.verify(&issued.token, 20.0).expect("verifies");
        assert_eq!(session.operator, gungnir_security::OperatorId(7));
        assert!(
            authority.verify(&issued.token, 10.0 + 61.0).is_err(),
            "expired"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn each_missing_half_is_named() {
        let config = ConfigBaseline::default();
        assert!(matches!(
            build_with_key(&config, vec![0; 32]),
            Err(NoAuthority::NoAccountProvider)
        ));
        let mut missing = ConfigBaseline::default();
        missing.security.authentication.provider = AuthenticationProvider::LocalAccounts {
            accounts_path: "Z:/nowhere/accounts.json".into(),
        };
        assert!(matches!(
            build_with_key(&missing, vec![0; 32]),
            Err(NoAuthority::StoreUnavailable(_))
        ));
    }

    /// GAP-057's node half, D-39: an OS-keystore-backed account store builds the same
    /// authority a file-backed one does. Honest either way, the same rule
    /// `gungnir-security/tests/account_store_os_keystore.rs` follows: where this
    /// machine has no reachable keystore, `StoreUnavailable` is the documented fallback
    /// and not a failure of this test.
    #[test]
    fn an_os_keystore_account_signs_in_where_a_backend_is_reachable() {
        let dir = std::env::temp_dir().join(format!(
            "gungnir-node-auth-os-keystore-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("dir");
        let account = format!("node-auth-test-{}", std::process::id());

        // Provisioned directly against `gungnir-security`, the same way
        // `gungnir-node account add-os-keystore` will: this test is about `build_with_key`
        // wiring the provider through, not about provisioning itself.
        let provisioned = gungnir_security::EncryptedAccountStore::open_or_create(&dir, &account)
            .and_then(|store| {
                let phc = gungnir_security::hash_passphrase("correct horse")?;
                store.add(
                    gungnir_security::OperatorId(7),
                    gungnir_security::Role::Operator,
                    phc,
                    false,
                )?;
                Ok(())
            });

        let config = ConfigBaseline {
            node: Some(gungnir_config::NodeConfig {
                data_dir: dir.to_string_lossy().into_owned(),
                ..gungnir_config::NodeConfig::default()
            }),
            security: gungnir_config::SecurityConfig {
                authentication: AuthenticationConfig {
                    provider: AuthenticationProvider::OsKeystoreAccounts {
                        account: account.clone(),
                    },
                    session_lifetime_s: Some(60.0),
                },
                ..gungnir_config::SecurityConfig::default()
            },
            ..ConfigBaseline::default()
        };

        match provisioned {
            Ok(()) => {
                let authority = build_with_key(
                    &config,
                    b"a-signing-key-of-adequate-length-0123456789".to_vec(),
                )
                .expect("built despite a successful provisioning above");
                let issued = authority
                    .sign_in(7, "correct horse", 10.0)
                    .expect("signed in");
                let session = authority.verify(&issued.token, 20.0).expect("verifies");
                assert_eq!(session.operator, gungnir_security::OperatorId(7));

                let entry = keyring::v1::Entry::new("gungnir-node-accounts", &account)
                    .expect("the same entry");
                let _ = entry.delete_credential();
            }
            Err(err) => {
                let reason = err.to_string();
                assert!(
                    reason.contains("keystore"),
                    "an unrelated failure, not the documented no-keystore fallback: {reason}"
                );
                assert!(matches!(
                    build_with_key(&config, vec![0; 32]),
                    Err(NoAuthority::StoreUnavailable(_))
                ));
            }
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
