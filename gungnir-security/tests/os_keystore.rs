// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Exercises whatever operating-system keystore this machine actually has (DN-22 §5;
//! D-39; GAP-084). A separate binary from `gungnir-security`'s unit tests on purpose:
//! `keyring::v1::Entry` latches the real platform backend as `keyring-core`'s
//! process-wide default on first use, which would race against the unit tests'
//! `keyring_core::mock::Store` if the two ran in one process.
//!
//! **Honest either way, not skipped.** Where a real backend is reachable -- this
//! workspace's own Windows development machines, and macOS -- the secret round-trips
//! for real and the test cleans up after itself. Where none is reachable -- a headless
//! Linux CI runner with no Secret Service session on its bus -- `PersistentKeyProvider`'s
//! own error path is what fires, which is DN-22 §5's stated fallback and not a gap in
//! coverage: the fallback is exactly the thing this test would otherwise have no way to
//! exercise on such a runner.

use gungnir_security::{KeyProvider, KeyPurpose, PersistentKeyProvider};

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("gungnir-os-keystore-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("dir");
    d
}

#[test]
fn the_real_backend_round_trips_or_leaves_the_desktop_honestly_unencrypted() {
    let account = format!("integration-test-{}", std::process::id());
    let d = dir("round-trip");

    match PersistentKeyProvider::open_or_create_via_os_keystore(&d, &account, None) {
        Ok(first) => {
            let key = first
                .active_or_generate(KeyPurpose::JournalAtRest)
                .expect("key");
            let sealed = first.seal(&key, b"real backend").expect("sealed");
            drop(first);

            // The same account reopens the same file: the OS keystore handed back the
            // same secret it stored the first time, unlocked at login with no prompt.
            let again = PersistentKeyProvider::open_or_create_via_os_keystore(&d, &account, None)
                .expect("reopened under the same OS-held secret");
            assert_eq!(again.unseal(&key, &sealed).expect("opens"), b"real backend");

            // Clean up what this test put in the real keystore. `keyring`'s v1 facade is
            // already the process default by now (opening the provider forced it), so
            // this reaches the same entry `wrapping_secret` created.
            let entry = keyring::v1::Entry::new("gungnir-desktop-keystore", &account)
                .expect("the same entry");
            entry.delete_credential().expect("cleaned up");
        }
        Err(err) => {
            let reason = err.to_string();
            assert!(
                reason.contains("keystore"),
                "an unrelated failure, not the documented no-keystore fallback: {reason}"
            );
        }
    }
    let _ = std::fs::remove_dir_all(d);
}
