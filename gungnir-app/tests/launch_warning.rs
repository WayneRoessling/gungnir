// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Declaring a launch warning (GAP-009, DN-16 amendment 2): the manual producer
//! `gungnir_model::events::LaunchWarningEvent::Issued` had no caller for, end to end
//! against a real journal -- the same reason `gungnir-node/tests/account_provisioning.rs`
//! and `gungnir-app/tests/sapient_task_ack.rs` drive their own subjects through a real
//! one rather than a mock.

use gungnir_app::launch_warning::{declare, recover, LaunchWarningError, Recovered};
use gungnir_app::state::AppState;
use gungnir_config::ConfigBaseline;
use gungnir_model::{MissionTime, Releasability};
use gungnir_security::Role;
use gungnir_store::FileEventJournal;
use gungnir_time::ReplayClockAuthority;

/// A deterministic clock, the same reason `gungnir-app/tests/anomalies.rs` sets one:
/// the wall clock's own precision at this magnitude (whole seconds since 1970 plus a
/// sub-microsecond fraction, right at an `f64`'s significant-digit limit) makes two
/// samples taken moments apart compare unequal even though nothing but time passing
/// separates them, which a "declare, reopen, compare exactly" test cannot tolerate.
fn desktop(name: &str) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-launch-warning-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        ..ConfigBaseline::default()
    };
    let mut state = AppState::with_config(config).expect("the desktop starts");
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(1_000.0),
    });
    (state, dir)
}

#[test]
fn a_role_without_release_product_is_refused_and_nothing_is_recorded() {
    let (mut state, dir) = desktop("forbidden");
    state.set_role(Role::Analyst);
    let err = declare(&mut state, "a rocket left the pad", Releasability::Internal)
        .expect_err("Analyst does not hold RELEASE_PRODUCT");
    assert!(matches!(err, LaunchWarningError::Forbidden));
    assert!(state.issued_launch_warnings.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_empty_description_is_refused_and_nothing_is_recorded() {
    let (mut state, dir) = desktop("empty");
    state.set_role(Role::Commander);
    let err = declare(&mut state, "   ", Releasability::Internal).expect_err("blank description");
    assert!(matches!(err, LaunchWarningError::EmptyDescription));
    assert!(state.issued_launch_warnings.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_declared_warning_is_stored_and_two_declarations_get_distinct_ids() {
    let (mut state, dir) = desktop("stored");
    state.set_role(Role::IntelligenceAnalyst);
    let first = declare(&mut state, "one launch", Releasability::AllPeers)
        .expect("IntelligenceAnalyst holds RELEASE_PRODUCT");
    let second = declare(&mut state, "a second launch", Releasability::Internal).expect("declared");
    assert_ne!(first.id, second.id);
    assert_eq!(first.what, "one launch");
    assert_eq!(first.releasability, Releasability::AllPeers);
    assert_eq!(state.issued_launch_warnings, vec![first, second]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn commander_also_holds_release_product() {
    let (mut state, dir) = desktop("commander");
    state.set_role(Role::Commander);
    declare(&mut state, "a drone left a launcher", Releasability::Internal)
        .expect("Commander holds RELEASE_PRODUCT, the same grant GAP-065 gave it PUBLISH_EXCHANGE alongside");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn recovery_of_an_empty_journal_says_nothing_issued() {
    let (state, dir) = desktop("recover-empty");
    let journal = FileEventJournal::open(&dir).expect("journal");
    let (issued, recovered) = recover(&journal);
    assert!(issued.is_empty());
    assert_eq!(recovered, Recovered::NothingIssued);
    drop(state);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_declared_warning_survives_a_restart() {
    let (mut state, dir) = desktop("recover-restart");
    state.set_role(Role::Commander);
    let declared =
        declare(&mut state, "a missile left a silo", Releasability::Internal).expect("declared");
    state.save_session().expect("saved");
    // The journal handle inside `state` must close before a fresh one reopens the
    // same files, or the reopen races the still-open handle on some platforms.
    drop(state);

    let journal = FileEventJournal::open(&dir).expect("reopened");
    let (issued, recovered) = recover(&journal);
    assert_eq!(issued, vec![declared]);
    assert!(matches!(recovered, Recovered::FromJournal { .. }));
    drop(journal);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_second_desktop_life_continues_the_serial_rather_than_colliding() {
    let (mut state, dir) = desktop("continue-serial");
    state.set_role(Role::Commander);
    let first = declare(&mut state, "first life", Releasability::Internal).expect("declared");
    state.save_session().expect("saved");
    drop(state);

    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        ..ConfigBaseline::default()
    };
    let mut state = AppState::with_config(config).expect("reopens");
    assert_eq!(state.issued_launch_warnings, vec![first.clone()]);
    state.set_role(Role::Commander);
    let second = declare(&mut state, "second life", Releasability::Internal).expect("declared");
    assert_ne!(
        first.id, second.id,
        "the recovered life must not reuse an id"
    );
    let _ = std::fs::remove_dir_all(dir);
}
