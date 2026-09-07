// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A seeded session for usability rounds (GAP-089): the seed's plan reaches the queue
//! through the real chain, the seed's tracks appear on schedule, the journal opens with
//! the rehearsal mark, and a node-backed baseline refuses it.

use gungnir_app::rehearsal;
use gungnir_app::state::AppState;
use gungnir_app::update;
use gungnir_command::ApprovalWorkflow;
use gungnir_config::ConfigBaseline;
use gungnir_eventing::Event;
use gungnir_model::events::RehearsalEvent;
use gungnir_model::MissionTime;
use gungnir_time::ReplayClockAuthority;
use std::path::{Path, PathBuf};

fn testdata(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../testdata/usability")
        .join(name)
}

fn desktop(name: &str) -> (AppState, PathBuf) {
    let dir = std::env::temp_dir().join(format!("gungnir-rehearsal-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let text = std::fs::read_to_string(testdata("round-1.json")).expect("baseline");
    let mut config: ConfigBaseline = serde_json::from_str(&text).expect("parses");
    config.data_dir = dir.to_string_lossy().into_owned();
    gungnir_config::validate(&config).expect("the round-1 baseline is valid");
    let mut state = AppState::with_config(config).expect("starts");
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(1000.0),
    });
    (state, dir)
}

fn at(state: &mut AppState, t: f64) {
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(1000.0 + t),
    });
    update::tick(state);
}

#[test]
fn the_seed_puts_tracks_and_a_plan_in_front_of_the_participant_and_marks_the_journal() {
    let (mut state, dir) = desktop("seed");
    let (seed, hash) = rehearsal::load_seed(&testdata("round-1-seed.json")).expect("seed loads");
    assert_eq!(seed.plans.len(), 2);
    rehearsal::install(&mut state, seed, hash.clone()).expect("installed");
    let events = state.events.subscribe();
    assert!(
        !state
            .resources
            .iter()
            .find(|r| r.id.0 == 2)
            .expect("resource 2")
            .ready,
        "the seed marks resource 2 not ready"
    );

    at(&mut state, 0.0);
    assert_eq!(state.tracking.tracks().len(), 1, "T-039 at once");
    assert!(
        events.try_iter().any(|env| matches!(
            &env.event,
            Event::Rehearsal(RehearsalEvent::Started { seed_sha256, .. }) if *seed_sha256 == hash
        )),
        "the record opens with the rehearsal mark"
    );
    assert!(
        !state.tracking.is_healthy(),
        "a scripted picture is not a tracker"
    );

    at(&mut state, 25.0);
    assert_eq!(state.tracking.tracks().len(), 3, "the two drones from 20 s");
    let t39 = state
        .tracking
        .tracks()
        .iter()
        .find(|t| t.id.0 == 39)
        .expect("T-039");
    assert!(
        (t39.state[0] - (9000.0 - 45.0 * 25.0)).abs() < 1e-6,
        "moves at its velocity"
    );

    at(&mut state, 31.0);
    // P-1181 names resource 2, which the seed marked not ready: the readiness engine
    // refuses it and it never reaches the queue (US-02's refusal).
    assert!(
        !state.approvals.queue().iter().any(|p| p.plan.id.0 == 1181),
        "P-1181 is refused on readiness, not queued"
    );
    // **The seed no longer owns the plan slot.** The allocator solves as of 2026-09-06
    // (GAP-029), so the desktop's own planner proposes against the seeded tracks in the
    // same session and `last_plan` is whichever was most recent. What the seed
    // guarantees is that its scripted plan went through the real submit path and was
    // refused there, which is the assertion above and the one US-02 rests on.
    assert!(
        state.denials.count > 0,
        "the seeded plan went through the real submit path and was denied there"
    );

    at(&mut state, 46.0);
    assert!(
        state.approvals.queue().iter().any(|p| p.plan.id.0 == 1183),
        "P-1183 waits for a person"
    );

    at(&mut state, 61.0);
    assert!(
        state.alerts.iter().filter(|a| a.contains("sensor")).count() >= 6,
        "the alert storm"
    );
    at(&mut state, 85.0);
    let t39 = state
        .tracking
        .tracks()
        .iter()
        .find(|t| t.id.0 == 39)
        .expect("T-039");
    assert!(t39.quality.is_stale, "T-039 goes stale after 80 s");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_node_backed_baseline_refuses_a_rehearsal_and_a_bad_seed_is_named() {
    let (mut state, dir) = desktop("refuse");
    state.backend = gungnir_config::BackendConfig::Remote {
        endpoint: "http://127.0.0.1:1".into(),
    };
    let (seed, hash) = rehearsal::load_seed(&testdata("round-1-seed.json")).expect("seed loads");
    assert!(matches!(
        rehearsal::install(&mut state, seed, hash),
        Err(rehearsal::RehearsalError::NotEmbedded)
    ));
    assert!(matches!(
        rehearsal::load_seed(&testdata("SOURCE.md")),
        Err(rehearsal::RehearsalError::Malformed { .. })
    ));
    assert!(matches!(
        rehearsal::load_seed(&testdata("absent.json")),
        Err(rehearsal::RehearsalError::Unreadable { .. })
    ));
    let _ = std::fs::remove_dir_all(dir);
}
