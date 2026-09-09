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
// One scripted session walked start to finish, not several unrelated cases: splitting
// it at an arbitrary line count would need the whole `state`/`dir` setup duplicated
// per fragment for no gain in clarity.
#[allow(clippy::too_many_lines)]
fn the_seed_puts_tracks_and_a_plan_in_front_of_the_participant_and_marks_the_journal() {
    let (mut state, dir) = desktop("seed");
    let (seed, hash) = rehearsal::load_seed(&testdata("round-1-seed.json")).expect("seed loads");
    // 1181 (US-02, refused on readiness) and 1183 (US-01/03/05) as before, plus six more
    // point-layer plans (1201-1206, GAP-097's US-06 fix) so the queue can reach seven
    // with two near expiry.
    assert_eq!(seed.plans.len(), 8);
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
    // T-039 and T-051 (US-06's first queue-filler) both appear at once.
    assert_eq!(state.tracking.tracks().len(), 2, "T-039 and T-051 at once");
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
    // T-039, T-051, T-052 from before, plus the two drones (T-041, T-042) from 20 s.
    assert_eq!(state.tracking.tracks().len(), 5, "the two drones from 20 s");
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
    // GAP-097's US-06 fix: by 75 s the queue should hold seven point-layer plans
    // (1183, 1201-1206; 1181 is area-layer and refused on readiness, never queued),
    // with the two oldest (1201 at 0 s, 1202 at 5 s) closest to the 90 s point-layer
    // expiry and clearly ahead of the rest.
    at(&mut state, 75.0);
    let raw_point_plans: Vec<u64> = state
        .approvals
        .queue()
        .iter()
        .filter(|p| p.layer == gungnir_model::EffectorLayer::Point)
        .map(|p| p.plan.id.0)
        .collect();
    let point_ids: std::collections::HashSet<u64> = raw_point_plans.iter().copied().collect();
    // A distinct-id count alone would not catch GAP-097's other half: the raw queue
    // must not hold the same id more than once either, which `update::tick` on its
    // own could still get wrong (by re-submitting an id it had already announced)
    // without changing how many *distinct* ids end up in it.
    assert_eq!(
        raw_point_plans.len(),
        point_ids.len(),
        "the queue holds a duplicate of some plan id: {raw_point_plans:?}"
    );
    for id in [1183, 1201, 1202, 1203, 1204, 1205, 1206] {
        assert!(
            point_ids.contains(&id),
            "plan {id} should be queued by 75 s"
        );
    }
    // **Eight, corrected from this comment's own prior guess of seven.** GAP-097 is
    // fixed: `DpInterceptService::plan_with_rewards` no longer mints a fresh `PlanId`
    // on every successful solve regardless of pairing, and `update::tick` no longer
    // loses track of what it already announced when a scripted plan also writes
    // `state.last_plan` for display. But the eighth entry here is not a leftover
    // duplicate to eliminate -- it is the live allocator's own genuine, now-stable
    // proposal. None of the seven scripted plans task resource 1 against track 39
    // (39 belongs only to the area-layer plan 1181, against resource 2); resource 1
    // is ready throughout and track 39 is present and un-stale the whole window, so
    // the live solver -- which knows nothing about the scripted schedule and runs
    // every tick regardless of it -- correctly and independently proposes that
    // pairing too, once, and (the fix) never mints a second id for it. The prior
    // comment predicted the fix would bring the count to exactly seven without having
    // implemented it; that prediction did not account for this legitimate eighth
    // proposal, and this assertion is what the fix actually produces, not what an
    // earlier guess expected.
    assert_eq!(
        point_ids.len(),
        8,
        "the seven scripted point-layer plans plus the live allocator's own stable \
         resource-1/track-39 proposal should be queued by 75 s"
    );
    let remaining = |id: u64| {
        state
            .approvals
            .queue()
            .iter()
            .find(|p| p.plan.id.0 == id)
            .and_then(|p| p.time_remaining_s(state.clock.now()))
            .expect("a point-layer plan has an expiry")
    };
    let (oldest_two, rest): (Vec<u64>, Vec<u64>) = [1183_u64, 1201, 1202, 1203, 1204, 1205, 1206]
        .into_iter()
        .partition(|&id| id == 1201 || id == 1202);
    let oldest_max = oldest_two
        .iter()
        .map(|&id| remaining(id))
        .fold(f64::MIN, f64::max);
    let rest_min = rest
        .iter()
        .map(|&id| remaining(id))
        .fold(f64::MAX, f64::min);
    assert!(
        oldest_max < rest_min,
        "1201 and 1202 should be nearer expiry than every other queued plan"
    );

    at(&mut state, 85.0);
    let t39 = state
        .tracking
        .tracks()
        .iter()
        .find(|t| t.id.0 == 39)
        .expect("T-039");
    assert!(
        t39.quality.is_stale,
        "T-039 goes stale after 65 s (US-03: 20 s after P-1183 appears at 45 s)"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_seed_whose_plans_are_not_sorted_by_at_s_is_refused_and_named() {
    // Round-1's own history: 1201 (at_s 0.0) once sat after 1183 (at_s 45.0) in the
    // array, which `tick`'s single cursor would have skipped until 45 s regardless of
    // its own earlier at_s. `load_seed` catches that before a session ever starts.
    let dir =
        std::env::temp_dir().join(format!("gungnir-rehearsal-disorder-{}", std::process::id()));
    let path = dir.join("disordered.json");
    std::fs::create_dir_all(&dir).expect("scratch dir");
    std::fs::write(
        &path,
        serde_json::json!({
            "name": "disordered",
            "plans": [
                { "id": 1, "at_s": 45.0, "resource": 1, "track": 1 },
                { "id": 2, "at_s": 0.0, "resource": 1, "track": 2 }
            ]
        })
        .to_string(),
    )
    .expect("write disordered seed");
    let err = rehearsal::load_seed(&path).expect_err("a disordered seed is refused");
    let text = err.to_string();
    assert!(
        text.contains("plans") && text.contains("not sorted"),
        "the refusal should name the field: {text}"
    );
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

#[test]
fn t_039_is_not_stale_before_65_s_and_is_stale_after() {
    // US-03's card reads "the seed makes T-039 stale 20 s in", the 20 s being measured
    // from P-1183 appearing at 45 s -- so 65 s, corrected 2026-09-08 from an earlier
    // 80 s that missed the card by 35 s. The walk above checks staleness at 85 s only,
    // which any `stale_after_s` at or below 85 satisfies: it would not have failed on
    // the 80 s the card never wanted, and so does not pin the fix it was written for.
    // Bracketing the transition does. Its own state, so the ticks it adds cannot
    // perturb the queue counts the main walk asserts.
    let (mut state, dir) = desktop("stale");
    let (seed, hash) = rehearsal::load_seed(&testdata("round-1-seed.json")).expect("seed loads");
    rehearsal::install(&mut state, seed, hash).expect("installed");
    let t39_is_stale = |state: &AppState| {
        state
            .tracking
            .tracks()
            .iter()
            .find(|t| t.id.0 == 39)
            .expect("T-039 is present throughout")
            .quality
            .is_stale
    };

    // The seed's schedule runs from the *first frame*, not from `install`
    // (`RehearsalPicture::elapsed` captures its origin on the first `poll`), so the
    // session has to be started at 0 before any later time means what the card says.
    at(&mut state, 0.0);
    at(&mut state, 64.0);
    assert!(
        !t39_is_stale(&state),
        "T-039 must still be fresh at 64 s: the stale label is the trigger US-03's \
         clock starts on, and one that fires before P-1183 has been on screen for 20 s \
         measures a different task than the card describes"
    );
    at(&mut state, 66.0);
    assert!(
        t39_is_stale(&state),
        "T-039 must be stale by 66 s (65 s after it appears at at_s 0, which is 20 s \
         after P-1183 appears at 45 s)"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_round_1_baseline_lets_pn_16_compute_coverage_for_both_laydowns() {
    // US-15's card has the planner read both laydowns' coverage off PN-16's table.
    // `planning_rows` returns `NotComputed` for every row when the baseline declares no
    // approach to evaluate along, and round-1.json declared none until 2026-09-08 -- so
    // the card asked for a number the session could not produce. The `approaches`
    // section added that day is what makes these rows `Computed`; this test is what
    // keeps them so.
    use gungnir_ui::panels::planning::LaydownCoverage;

    let (state, dir) = desktop("coverage");
    let gungnir_app::sustainment::PlanningRows::Rows(rows) =
        gungnir_app::sustainment::planning_rows(&state)
    else {
        panic!("round-1.json declares two laydowns, so PN-16 has rows to draw");
    };
    assert_eq!(rows.len(), 2, "`current` and `b`");

    for row in &rows {
        let LaydownCoverage::Computed {
            gap_segments,
            uncovered_m,
            delta_uncovered_m,
        } = &row.coverage
        else {
            panic!(
                "laydown {:?} has no computed coverage, so US-15's card cannot be run \
                 against this baseline: {:?}",
                row.id, row.coverage
            );
        };
        // Measured against the declared upper Vell approach, not chosen: the axis runs
        // ~25 km out from the estuary and both radars together leave two stretches of
        // it uncovered.
        assert_eq!(*gap_segments, 2, "laydown {:?}", row.id);
        assert!(
            (*uncovered_m - 7000.0).abs() < 1.0,
            "laydown {:?} uncovered {uncovered_m} m",
            row.id
        );
        // **The finding US-15's card rests on, pinned so it cannot go quiet.** PN-16's
        // coverage is built from a laydown's *sensor* placements alone
        // (`laydown_coverage_volumes`), and round-1's two laydowns place both radars
        // identically -- `b` moves the area-layer battery and nothing else. So the
        // comparison column reads exactly zero by construction, and a participant asked
        // to "compare" these two on coverage is being asked to read a difference that
        // cannot exist. The day someone gives `b` a sensor of its own this assertion
        // fails, which is the point: the card's premise changes with it.
        if !row.current {
            assert_eq!(
                *delta_uncovered_m,
                Some(0.0),
                "laydown {:?} differs from `current` only in where a battery stands, \
                 which coverage does not read",
                row.id
            );
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}
