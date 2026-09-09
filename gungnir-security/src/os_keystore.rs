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

/// The name the disconnected desktop's own keystore entries live under (D-39,
/// `keystore.rs::PersistentKeyProvider`). Fixed, the same reason
/// `keystore.rs::KEYSTORE_FILE` is fixed: the baseline names an account, never a path or
/// a service string, and two names for the same thing would be two things to keep in
/// sync for no benefit.
///
/// **Not the only service this module serves.** [`wrapping_secret`] takes its own
/// service name as a parameter -- `gungnir-node`'s account store (GAP-057,
/// `account_store.rs`) needs a wrapping secret from the identical mechanism under a
/// different name, so the two never collide in the same OS keystore even when a node
/// and a desktop share a machine.
///
/// **Public since 2026-09-08 (GAP-060's remaining slice).**
/// `PersistentKeyProvider::open_or_create_via_os_keystore` gained the same `service`
/// parameter `wrapping_secret` already had, because it too now backs more than one
/// purpose (the desktop's own keystore here, and a node's and a desktop's TLS identity
/// in `gungnir-remote::identity`) and can no longer bake in a single fixed name the way
/// `EncryptedAccountStore` still does for its one purpose. `gungnir-app`, the one
/// caller outside this crate, needs this constant to keep naming the same service
/// rather than growing its own copy of the string.
pub const DESKTOP_KEYSTORE_SERVICE: &str = "gungnir-desktop-keystore";

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

/// Whatever the entry holds right now, as the wrapping secret -- the read every caller's
/// correctness rests on, named rather than inlined so the race note below has something
/// to point at and so a test can stage the losing interleaving against it directly.
fn read_back(entry: &keyring_core::Entry) -> Result<String, SecurityError> {
    entry.get_password().map_err(|err| {
        SecurityError::KeyProviderUnavailable(format!(
            "the operating-system keystore did not return the stored secret: {err}"
        ))
    })
}

/// The entry's stored secret, or a freshly generated one written back when this account
/// has none yet -- and then **re-read, so what is returned is what the store actually
/// holds** rather than what this process generated.
///
/// **Why the re-read (2026-09-08).** Two processes starting against the same account
/// both see `NoEntry`, both generate, and the second `set_password` wins. Returning the
/// generated secret unchecked would have the loser seal its `keystore.sealed` under a
/// secret the store no longer holds -- and `PersistentKeyProvider::open_or_create`
/// correctly refuses a file that will not open rather than overwriting it, so that file
/// would be unopenable from then on. The keys in it may be the only way to read a year
/// of journals (`keystore.rs`'s own words on exactly that refusal), so the loser adopts
/// the winner's secret rather than diverging from it.
///
/// **What this does not claim.** It narrows the window; it does not close it. A writer
/// landing between this `set_password` and this read-back is adopted; one landing after
/// the read-back but before the caller seals its own file is not, and that caller's file
/// is then orphaned exactly as it would have been before. Closing it completely needs a
/// compare-and-swap or a lock across the store, and none of the three backends this
/// reaches offers one -- so the residual is stated here rather than implied away.
///
/// **A failed read-back is a fault, not a reason to fall back on the generated value.**
/// If the store cannot confirm what it holds, sealing under an unconfirmed secret is the
/// same risk with the evidence removed; the error surfaces and the caller takes DN-22
/// §5's honest fallback -- journal in the clear, or an ephemeral identity -- instead.
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
            read_back(entry)
        }
        Err(err) => Err(SecurityError::KeyProviderUnavailable(format!(
            "the operating-system keystore did not return the stored secret: {err}"
        ))),
    }
}

/// A wrapping secret from the operating system's keystore: unlocked at operator login
/// rather than typed at sign-in, and shaped exactly like
/// `PersistentKeyProvider::open_or_create`'s passphrase argument, whatever `service` and
/// `account` name.
///
/// `service` separates the callers that share this mechanism (the desktop's own
/// keystore, a node's account store) so they never collide on the same OS-keystore
/// entry; `account` then separates deployments within one such service, so two
/// deployments on one machine do not collide either. The config baseline supplies
/// `account` rather than this module inventing one -- `KeyProviderConfig::
/// OperatingSystemKeystore { account }`, `AuthenticationProvider::OsKeystoreAccounts {
/// account }` -- the same way the baseline names a mechanism and never a secret
/// (DN-22 §6).
///
/// # Errors
///
/// `KeyProviderUnavailable` when this platform has no reachable keystore -- DN-22 §5's
/// fallback applies exactly as it does for a file that will not open: the desktop still
/// starts and journals in the clear -- or the store refuses the operation.
pub(crate) fn wrapping_secret(service: &str, account: &str) -> Result<String, SecurityError> {
    ensure_real_backend()?;
    let entry = keyring_core::Entry::new(service, account).map_err(|err| {
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

    // `service` is a parameter here for the same reason it is one on `wrapping_secret`
    // itself: this module now backs more than the desktop's own keystore, and
    // `two_different_services_for_the_same_account_do_not_share_a_secret` below is the
    // test that exists to prove those services stay independent.
    fn mock_entry(service: &str, account: &str) -> keyring_core::Entry {
        MOCK.call_once(|| {
            keyring_core::set_default_store(keyring_core::mock::Store::new().expect("mock store"));
        });
        keyring_core::Entry::new(service, account).expect("mock entry")
    }

    #[test]
    fn a_first_run_generates_and_stores_a_secret() {
        let entry = mock_entry(DESKTOP_KEYSTORE_SERVICE, "first-run");
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
        let entry = mock_entry(DESKTOP_KEYSTORE_SERVICE, "second-call");
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

    /// The first-run race [`ensure_secret`]'s own doc names, staged in the order that
    /// loses it: this process generates and stores, another process's `set_password`
    /// lands next, and only then does the read-back run. What comes out must be the
    /// store's value, not this process's -- because sealing `keystore.sealed` under a
    /// secret the store no longer holds leaves that file permanently unopenable, which
    /// `open_or_create` is right to refuse and cannot repair.
    ///
    /// Staged against [`read_back`] rather than through `ensure_secret` end to end
    /// because the interleaving is *inside* that function: two real processes would
    /// interleave there, and a single-threaded test cannot suspend itself mid-call. The
    /// step being proved is the one the fix added.
    #[test]
    fn a_writer_that_wins_the_first_run_race_is_adopted_rather_than_diverged_from() {
        let ours = mock_entry(DESKTOP_KEYSTORE_SERVICE, "first-run-race");
        let generated = generate_secret();
        ours.set_password(&generated)
            .expect("our own write lands first");

        let theirs = mock_entry(DESKTOP_KEYSTORE_SERVICE, "first-run-race");
        let winner = generate_secret();
        theirs
            .set_password(&winner)
            .expect("the other process wins");

        let adopted = read_back(&ours).expect("the store answers");
        assert_eq!(
            adopted, winner,
            "the loser must seal its file under what the store actually holds"
        );
        assert_ne!(
            adopted, generated,
            "returning the locally generated secret is the orphaning bug this closes"
        );
    }

    /// The other half of the same fix: a read-back that cannot answer is a fault, and
    /// must not quietly degrade to the generated value -- that would seal a file under a
    /// secret nothing has confirmed. The caller's own honest fallback (DN-22 §5) covers
    /// it from there.
    #[test]
    fn a_read_back_that_cannot_answer_is_an_error_and_not_the_generated_value() {
        let entry = mock_entry(DESKTOP_KEYSTORE_SERVICE, "read-back-fault");
        entry.set_password(&generate_secret()).expect("stored");
        let mock: &keyring_core::mock::Cred =
            entry.as_any().downcast_ref().expect("mock credential");
        mock.set_error(keyring_core::Error::NoStorageAccess("locked".into()));
        let err = read_back(&entry).expect_err("an unconfirmable secret is a fault");
        assert!(err.to_string().contains("did not return"), "{err}");
    }

    /// The property the `service` parameter exists for (2026-09-08, GAP-060's
    /// remaining slice): the same account name under two different services must not
    /// address the same secret, the way `NODE_KEYSTORE_SERVICE` already keeps a node's
    /// accounts out of the desktop's own keystore. `"gungnir-node-tls-identity"` is a
    /// literal here rather than an imported constant because it is `gungnir-remote`'s
    /// own, private to that crate's `identity.rs`; this test only needs *a* second
    /// service name, not that specific one.
    #[test]
    fn two_different_services_for_the_same_account_do_not_share_a_secret() {
        let a = mock_entry(DESKTOP_KEYSTORE_SERVICE, "shared-account-name");
        let b = mock_entry("gungnir-node-tls-identity", "shared-account-name");
        let secret_a = ensure_secret(&a).expect("generated for service a");
        let secret_b = ensure_secret(&b).expect("generated for service b");
        assert_ne!(
            secret_a, secret_b,
            "the same account under two different services must not collide"
        );
        assert_eq!(
            ensure_secret(&a).expect("reused"),
            secret_a,
            "service a is unaffected by service b existing"
        );
        assert_eq!(
            ensure_secret(&b).expect("reused"),
            secret_b,
            "service b is unaffected by service a existing"
        );
    }

    /// A fault distinct from "nothing stored yet" -- a locked store, say -- must not be
    /// read as first-run and overwritten with a fresh secret, which would orphan
    /// whatever the real entry already protects.
    #[test]
    fn a_fault_other_than_missing_is_reported_and_never_treated_as_first_run() {
        let entry = mock_entry(DESKTOP_KEYSTORE_SERVICE, "fault");
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
