// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! This desktop's machine identity runs out, and says so (GAP-211, DN-22 §16).
//!
//! The identity a desktop holds is the one `AppState` issues at start; time is moved by
//! asking about a later moment, because the certificate's window is the wall clock's and a
//! test cannot wait ninety days.

use std::time::{Duration, SystemTime};

use gungnir_app::session::identity_concern;
use gungnir_app::state::AppState;
use gungnir_config::ConfigBaseline;

const DAY: Duration = Duration::from_hours(24);

fn desktop() -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("gungnir-identity-expiry-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        ..ConfigBaseline::default()
    };
    (AppState::with_config(config).expect("desktop state"), dir)
}

#[test]
fn a_desktop_identity_is_quiet_when_new_and_named_when_it_runs_out() {
    let (mut state, dir) = desktop();
    let now = SystemTime::now();
    let identity = &state
        .machine_identity
        .as_ref()
        .expect("the desktop issued its own identity (GAP-141)")
        .key;
    assert!(
        identity.days_left(now).is_some_and(|d| d >= 89),
        "{identity:?}"
    );
    assert_eq!(identity_concern(identity, now), None);

    // Asked about without the re-issue that the tick would make, as a desktop whose
    // re-issue never ran: five days left is under the seven-day warning, then past the end.
    let (key, said) = identity_concern(identity, now + DAY * 85).expect("expiring");
    assert_eq!(key, "expiring");
    assert!(
        said.contains(identity.name()) && said.contains("expires in"),
        "{said}"
    );
    let (key, said) = identity_concern(identity, now + DAY * 91).expect("expired");
    assert_eq!(key, "expired");
    assert!(said.contains("has expired"), "{said}");

    // A new identity raises nothing on the tick.
    let before = state.alerts.len();
    gungnir_app::session::keep_identity_current(&mut state);
    assert_eq!(state.alerts.len(), before, "{:?}", state.alerts);
    assert_eq!(state.identity_alerted, None);
    let _ = std::fs::remove_dir_all(&dir);
}
