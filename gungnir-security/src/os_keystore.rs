// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The operating system's keystore as the source of the disconnected desktop's wrapping
//! secret (DN-22 §5; D-39; GAP-084).
//!
//! **Human-owned** (docs/agentic-workflow.md: `gungnir-security` decides who can read
//! what); written and gated, not signed.
//!
//! `keystore.rs`'s `PersistentKeyProvider` already does everything DN-22 amendment 3
//! asks for: one sealed file holding an AES-256-GCM-wrapped `P256KeyProvider` snapshot,
//! under a wrapping key argon2 derives from a string. That amendment used an
//! operator-typed passphrase for the string because no crate reached the OS keystore yet
//! -- the condition D-39 has now closed. This module supplies the same string from the
//! OS keystore instead, so `PersistentKeyProvider::open_or_create_via_os_keystore`
//! reuses the file format, the derivation, and every existing test unchanged; the only
//! new code is where the string comes from.
//!
//! **Why a generated secret rather than the key material itself.** The backends this
//! reaches -- Windows Credential Manager, macOS Keychain, Linux Secret Service -- are
//! general-purpose password stores, not key-management services: none offers a
//! rotate-in-place key hierarchy, only one named secret per account. Generating one
//! high-entropy secret, unlocking it at login, and feeding it through the passphrase
//! path the amendment already designed keeps one file format and one set of tests for
//! both providers, rather than a second custody scheme wearing a different name.
//!
//! **Why production code goes through `keyring::v1::Entry` and tests go through
//! `keyring_core::Entry` directly**: `docs/agentic-coding-standards.md` §2.9, "OS
//! keystore". In short, `v1::Entry::new`'s first call latches the real platform backend
//! as `keyring-core`'s process-wide default and never checks whether one is already
//! set, so a test wanting the mock store cannot go through it at all -- it has already
//! lost the race to the real backend the moment any `v1::Entry` exists in that process.
//! [`ensure_real_backend`] forces the real backend once, the way production start-up
//! needs; the tests below install the mock first instead and talk to
//! `keyring_core::Entry`, the type `v1::Entry` itself wraps.

use crate::SecurityError;

/// The name this desktop's keystore entries live under. Fixed, the same reason
/// `keystore.rs::KEYSTORE_FILE` is fixed: the baseline names an account, never a path or
/// a service string, and two names for the same thing would be two things to keep in
/// sync for no benefit.
const SERVICE: &str = "gungnir-desktop-keystore";

/// Force the real platform-native backend as `keyring-core`'s process-wide default, the
/// one time a process needs it done. Idempotent to call more than once: `store_status`
/// is a static read after the first call installs it.
fn ensure_real_backend() -> Result<(), SecurityError> {
    keyring::v1::Entry::store_status().as_ref().map_err(|err| {
        SecurityError::KeyProviderUnavailable(format!(
            "no operating-system keystore is available on this platform: {err}"
        ))
    })?;
    Ok(())
}

/// 32 random bytes, hex-encoded. High-entropy enough that argon2's cost against it buys
/// nothing beyond what it already buys against a human passphrase -- it is fed through
/// unchanged so `open_or_create` needs no second code path.
fn generate_secret() -> String {
    let mut bytes = [0u8; 32];
    aes_gcm::aead::rand_core::RngCore::fill_bytes(&mut aes_gcm::aead::OsRng, &mut bytes);
    bytes.iter().fold(String::with_capacity(64), |mut hex, b| {
        use std::fmt::Write;
        let _ = write!(hex, "{b:02x}");
        hex
    })
}

/// The entry's stored secret, or a freshly generated one written back when this account
/// has none yet.
fn ensure_secret(entry: &keyring_core::Entry) -> Result<String, SecurityError> {
    match entry.get_password() {
        Ok(secret) => Ok(secret),
        Err(keyring_core::Error::NoEntry) => {
            let secret = generate_secret();
            entry.set_password(&secret).map_err(|err| {
                SecurityError::KeyProviderUnavailable(format!(
                    "the operating-system keystore refused to store a new secret: {err}"
                ))
            })?;
            Ok(secret)
        }
        Err(err) => Err(SecurityError::KeyProviderUnavailable(format!(
            "the operating-system keystore did not return the stored secret: {err}"
        ))),
    }
}

/// The wrapping secret DN-22 §5's disconnected row names: unlocked at operator login
/// rather than typed at sign-in, and otherwise exactly
/// `PersistentKeyProvider::open_or_create`'s passphrase argument.
///
/// `account` names which secret within [`SERVICE`], so two deployments on one machine
/// do not collide on the same keystore entry; the config baseline supplies it
/// (`KeyProviderConfig::OperatingSystemKeystore { account }`) rather than this module
/// inventing one, the same way the baseline names a mechanism and never a secret
/// (DN-22 §6).
///
/// # Errors
///
/// `KeyProviderUnavailable` when this platform has no reachable keystore -- DN-22 §5's
/// fallback applies exactly as it does for a file that will not open: the desktop still
/// starts and journals in the clear -- or the store refuses the operation.
pub(crate) fn wrapping_secret(account: &str) -> Result<String, SecurityError> {
    ensure_real_backend()?;
    let entry = keyring_core::Entry::new(SERVICE, account).map_err(|err| {
        SecurityError::KeyProviderUnavailable(format!(
            "could not address the operating-system keystore: {err}"
        ))
    })?;
    ensure_secret(&entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Once;

    // One mock store for every test in this binary, installed once: `set_default_store`
    // has no "already set" guard, so two tests racing to install their own would corrupt
    // each other's. Distinct account names per test keep them from seeing each other's
    // secrets in the one store they share. The real backend is never touched here --
    // that is `gungnir-security/tests/os_keystore.rs`, a separate binary so the two
    // cannot race over which default is installed.
    static MOCK: Once = Once::new();

    fn mock_entry(account: &str) -> keyring_core::Entry {
        MOCK.call_once(|| {
            keyring_core::set_default_store(keyring_core::mock::Store::new().expect("mock store"));
        });
        keyring_core::Entry::new(SERVICE, account).expect("mock entry")
    }

    #[test]
    fn a_first_run_generates_and_stores_a_secret() {
        let entry = mock_entry("first-run");
        let secret = ensure_secret(&entry).expect("generated");
        assert_eq!(secret.len(), 64, "32 bytes hex-encoded: {secret}");
        assert_eq!(
            entry.get_password().expect("stored"),
            secret,
            "the store now holds what was generated"
        );
    }

    #[test]
    fn a_second_call_returns_the_stored_secret_rather_than_generating_another() {
        let entry = mock_entry("second-call");
        let first = ensure_secret(&entry).expect("generated");
        let second = ensure_secret(&entry).expect("reused");
        assert_eq!(
            first, second,
            "a second boot must unlock the same keystore file"
        );
    }

    #[test]
    fn two_generated_secrets_are_not_the_same_bytes() {
        assert_ne!(generate_secret(), generate_secret());
    }

    /// A fault distinct from "nothing stored yet" -- a locked store, say -- must not be
    /// read as first-run and overwritten with a fresh secret, which would orphan
    /// whatever the real entry already protects.
    #[test]
    fn a_fault_other_than_missing_is_reported_and_never_treated_as_first_run() {
        let entry = mock_entry("fault");
        let mock: &keyring_core::mock::Cred =
            entry.as_any().downcast_ref().expect("mock credential");
        mock.set_error(keyring_core::Error::NoStorageAccess("locked".into()));
        let err =
            ensure_secret(&entry).expect_err("the fault surfaces rather than papering over it");
        assert!(err.to_string().contains("did not return"), "{err}");
        // And nothing was written: the error the mock injects is cleared after one call
        // (its own documented behaviour), so a second attempt proves no secret exists --
        // generating one now would be the first-run path, not evidence of an overwrite.
        assert!(entry.get_password().is_err(), "still nothing stored");
    }
}
