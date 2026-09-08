// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Exercises `EncryptedAccountStore::open_or_create` against whatever operating-system
//! keystore this machine actually has (GAP-057's node half; D-39). A separate binary
//! from `gungnir-security`'s unit tests for the same reason `os_keystore.rs` is:
//! `keyring::v1::Entry` latches the real platform backend as `keyring-core`'s
//! process-wide default on first use, which would race the unit tests' mock store if
//! the two ran in one process.
//!
//! **Honest either way, not skipped** -- the same rule `tests/os_keystore.rs` follows.
//! Where a real backend is reachable, the store round-trips for real and this test
//! cleans up its own entry; where none is reachable (a headless Linux CI runner with no
//! Secret Service session on its bus), `EncryptedAccountStore`'s own error path is what
//! fires, which is the documented fallback and not a gap in coverage.

use gungnir_security::{Account, AccountStore, EncryptedAccountStore, OperatorId, Role};

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!(
        "gungnir-account-store-os-keystore-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("dir");
    d
}

#[test]
fn the_real_backend_round_trips_or_leaves_the_node_honestly_refusing() {
    let account = format!("integration-test-{}", std::process::id());
    let d = dir("round-trip");

    match EncryptedAccountStore::open_or_create(&d, &account) {
        Ok(first) => {
            first
                .add(OperatorId(7), Role::Operator, "phc-of-7".into(), false)
                .expect("added");
            drop(first);

            // The same account reopens the same file: the OS keystore handed back the
            // same secret it stored the first time.
            let again = EncryptedAccountStore::open_or_create(&d, &account)
                .expect("reopened under the same OS-held secret");
            assert_eq!(
                again.account(OperatorId(7)).expect("looked up"),
                Some(Account {
                    operator: OperatorId(7),
                    role: Role::Operator,
                    phc: "phc-of-7".into(),
                })
            );

            // Clean up what this test put in the real keystore. `keyring`'s v1 facade
            // is already the process default by now (opening the store forced it), so
            // this reaches the same entry `EncryptedAccountStore` created.
            let entry =
                keyring::v1::Entry::new("gungnir-node-accounts", &account).expect("the same entry");
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
