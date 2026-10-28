// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A desktop session idle past D-143's timeout locks: the workspace stays the operator's,
//! every action is refused, and only a sign-in unlocks it (GAP-212, DN-23 §17).
//!
//! The authority is the one the desktop builds, given a clock the test moves by hand,
//! because the idle timeout is fifteen minutes on a clock that cannot be set.

use std::sync::Arc;

use gungnir_app::state::AppState;
use gungnir_config::ConfigBaseline;
use gungnir_security::{
    actions, hash_passphrase, Account, InMemoryAccountStore, LocalAccountAuthority,
    ManualMonotonic, OperatorId, Role, IDLE_LOCK_S,
};

const PASSPHRASE: &str = "correct horse battery";

fn desktop(clock: &ManualMonotonic) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("gungnir-session-lock-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        ..ConfigBaseline::default()
    };
    let mut state = AppState::with_config(config).expect("desktop state");
    let store = InMemoryAccountStore::new(vec![Account {
        operator: OperatorId(7),
        role: Role::Supervisor,
        phc: hash_passphrase(PASSPHRASE).expect("hashed"),
    }]);
    state.session =
        Box::new(LocalAccountAuthority::new(Box::new(store)).with_clock(Arc::new(clock.clone())));
    (state, dir)
}

fn sign_in(state: &mut AppState) {
    state
        .sign_in(&LocalAccountAuthority::credential(
            OperatorId(7),
            PASSPHRASE,
        ))
        .expect("signs in");
}

#[test]
fn an_idle_desktop_locks_refuses_every_action_and_a_sign_in_unlocks_it() {
    let clock = ManualMonotonic::default();
    let (mut state, dir) = desktop(&clock);
    // The selected role holds what the signed-in one does, so a lock that fell back to
    // the selection would show here as an action still permitted.
    state.set_role(Role::Supervisor);
    sign_in(&mut state);
    assert!(state.may(actions::DECIDE_PLAN));

    // Activity keeps it alive.
    clock.advance(IDLE_LOCK_S - 1.0);
    state.touch_session();
    clock.advance(IDLE_LOCK_S - 1.0);
    assert!(!state.is_locked());

    clock.advance(1.0);
    assert!(state.is_locked());
    assert_eq!(state.role(), Role::Supervisor, "the workspace changed role");
    assert_eq!(
        state.attributed_operator(),
        None,
        "a lock attributed an act"
    );
    for action in [
        actions::DECIDE_PLAN,
        actions::TASK_SENSOR,
        actions::ASSIGN_ROLE,
        actions::APPLY_CONFIG,
    ] {
        assert!(!state.may(action), "{action} was permitted while locked");
    }
    let mut editor = gungnir_app::sustainment::ConfigEditorState::default();
    let refused = editor
        .apply(&mut state)
        .expect_err("a locked console applied a baseline");
    assert!(refused.contains("locked"), "{refused}");

    // Moving the pointer does not unlock it; signing in does.
    state.touch_session();
    assert!(state.is_locked());
    sign_in(&mut state);
    assert!(!state.is_locked());
    assert!(state.may(actions::DECIDE_PLAN));
    let _ = std::fs::remove_dir_all(&dir);
}

/// D-143's two numbers live in `gungnir-security`, which enforces them, and in
/// `gungnir-config`, which refuses a baseline asking for more and may not depend on the
/// other. This holds the two copies equal.
#[test]
// Two copies of one constant, compared exactly because they must be the same number.
#[allow(clippy::float_cmp)]
fn the_configuration_and_the_enforcement_agree_on_d143() {
    assert_eq!(
        gungnir_config::SESSION_MAX_S,
        gungnir_security::SESSION_MAX_S
    );
    assert_eq!(gungnir_config::IDLE_LOCK_S, gungnir_security::IDLE_LOCK_S);
}
