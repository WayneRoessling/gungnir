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
fn the_round_1_baseline_gives_pn_16_a_laydown_pair_us_15_can_compare() {
    // US-15's card has the planner read the laydowns' coverage off PN-16's table and say
    // which option changes it. Two things had to be true before it could, and both are
    // scenario content rather than code, so both are pinned here.
    //
    // 1. `planning_rows` returns `NotComputed` for every row when the baseline declares
    //    no approach to evaluate along, and round-1.json declared none until 2026-09-08
    //    -- the whole table read "not computed". The `approaches` section added that day
    //    is what makes these rows `Computed`.
    // 2. PN-16's coverage is a function of a laydown's *sensor* placements alone
    //    (`laydown_coverage_volumes`), and until 2026-09-08 the only alternative `b`
    //    moved a battery, so its difference column read exactly `0.0` and the card asked
    //    for a difference that could not exist. `c` is the laydown that makes the pair
    //    a comparable one: the same two radars, one of them re-sited.
    //
    // The three rows are asserted individually and by their relationship. A pair whose
    // coverage cannot differ is the exact failure this scenario was changed to remove,
    // so `c`'s delta being non-zero is asserted in its own right and not merely implied
    // by the metre figures.
    use gungnir_ui::panels::planning::LaydownCoverage;

    let (state, dir) = desktop("coverage");
    let gungnir_app::sustainment::PlanningRows::Rows(rows) =
        gungnir_app::sustainment::planning_rows(&state)
    else {
        panic!("round-1.json declares three laydowns, so PN-16 has rows to draw");
    };
    let read = |id: &str| -> (usize, f64, Option<f64>) {
        let row = rows
            .iter()
            .find(|r| r.id.0 == id)
            .unwrap_or_else(|| panic!("round-1.json declares laydown {id:?}"));
        let LaydownCoverage::Computed {
            gap_segments,
            uncovered_m,
            delta_uncovered_m,
        } = &row.coverage
        else {
            panic!(
                "laydown {id:?} has no computed coverage, so US-15's card cannot be run \
                 against this baseline: {:?}",
                row.coverage
            );
        };
        (*gap_segments, *uncovered_m, *delta_uncovered_m)
    };
    assert_eq!(rows.len(), 3, "`current`, `b` and `c`");

    // Measured against the declared upper Vell approach, not chosen. The axis runs
    // ~24 km out from the estuary; with both radars sited as the deployment stands, its
    // outer 7 000 m is seen by nothing and a further stretch by one radar only.
    let (segments, uncovered, delta) = read("current");
    assert_eq!(segments, 2);
    assert!((uncovered - 7000.0).abs() < 1.0, "current {uncovered} m");
    assert_eq!(
        delta, None,
        "the current laydown is what the others differ from"
    );

    // **`b`'s zero is kept, and kept deliberately.** It moves the area-layer battery and
    // nothing else, so it reads "same as today" -- which is a true answer about what the
    // column measures, and the contrast that makes `c`'s number mean something. If
    // someone gives `b` a sensor of its own, this fails and the card's own text about
    // accounting for the zero has to change with it.
    let (segments, uncovered, delta) = read("b");
    assert_eq!(segments, 2);
    assert!((uncovered - 7000.0).abs() < 1.0, "b {uncovered} m");
    assert_eq!(
        delta,
        Some(0.0),
        "`b` differs from `current` only in where a battery stands, which coverage does \
         not read"
    );

    // **`c` is the pair US-15 compares, and this is the assertion that keeps it one.**
    // It re-sites S2 10 km up the declared axis: the dark stretch falls from 7 000 m to
    // 1 750 m, so the difference column reads "5250 m less gap than today". A regression
    // that put S2 back where `current` has it, or anywhere its coverage of this axis
    // matched, would make `delta` zero again and fail here.
    let (segments, uncovered, delta) = read("c");
    assert_eq!(segments, 2);
    assert!((uncovered - 1750.0).abs() < 1.0, "c {uncovered} m");
    let delta = delta.expect("`c` is not the current laydown, so it has a difference");
    assert!(
        (delta + 5250.0).abs() < 1.0,
        "`c` should read 5250 m less uncovered approach than `current`, not {delta}"
    );
    assert!(
        delta != 0.0,
        "a laydown pair whose coverage cannot differ is what US-15's card could not be \
         run against; `c` exists to differ"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn moving_round_1s_forward_radar_trades_redundancy_and_does_not_create_coverage() {
    // **`c`'s gain is not free, and the panel's own columns do not show what it costs.**
    // PN-16 reports uncovered metres and a segment count; it does not report how much of
    // the approach one radar sees alone. The uncovered metres `c` buys come out of the
    // stretch both radars see together rather than out of thin air: forward-siting
    // converts dark approach into single-sensor approach.
    //
    // Its own test rather than a tail on the row assertions above, because it is a
    // different claim -- that one about what US-15 reads, this one about what US-15 is
    // not shown. Asserted at all because `SOURCE.md`, the session document and the gap
    // register all quote these figures, and a scenario edit that turned `c` into a free
    // win would otherwise leave three documents describing a trade that no longer
    // exists.
    //
    // Volumes are built exactly as `sustainment::laydown_coverage_volumes` builds them
    // -- sensors that search or track, placed by the laydown, ranged by the baseline --
    // which is itself the finding this scenario rests on: coverage reads placements and
    // nothing else about a laydown.
    let (state, dir) = desktop("trade");
    let frame = gungnir_app::sustainment::local_frame(&state).expect("round-1 declares an origin");
    let routes: Vec<Vec<[f64; 3]>> = state
        .config
        .approaches
        .iter()
        .map(|a| {
            a.points
                .iter()
                .map(|[lat_rad, lon_rad, alt_m]| {
                    frame.to_enu(gungnir_model::Geodetic {
                        lat_rad: *lat_rad,
                        lon_rad: *lon_rad,
                        alt_m: *alt_m,
                    })
                })
                .collect()
        })
        .collect();
    let approaches: Vec<&[[f64; 3]]> = routes.iter().map(Vec::as_slice).collect();
    let single_sensor_m = |id: &str| -> f64 {
        let laydown = state
            .config
            .laydowns
            .iter()
            .find(|l| l.id.0 == id)
            .unwrap_or_else(|| panic!("laydown {id:?}"));
        let volumes: Vec<(gungnir_model::SensorId, gungnir_analytics::CoverageVolume)> = laydown
            .sensors
            .iter()
            .filter(|s| {
                matches!(
                    s.mode,
                    gungnir_model::SensorMode::Search | gungnir_model::SensorMode::Track
                )
            })
            .map(|s| {
                let max_range_m = state
                    .config
                    .sensors
                    .iter()
                    .find(|d| d.id == s.sensor.0)
                    .expect("a laydown may only place a declared sensor")
                    .max_range_m;
                (
                    s.sensor,
                    gungnir_analytics::CoverageVolume {
                        sensor_enu: s.position_enu,
                        max_range_m,
                        min_elevation_rad: state.config.analytics.coverage_min_elevation_rad,
                    },
                )
            })
            .collect();
        gungnir_analytics::combined_coverage(
            &volumes,
            &gungnir_analytics::FlatTerrainLineOfSight,
            &approaches,
            gungnir_analytics::CoverageParameters {
                sample_spacing_m: state.config.analytics.coverage_sample_spacing_m,
                terrain_masking_applied: false,
            },
        )
        .gap_length_m(gungnir_analytics::GapSeverity::SingleSensor)
    };
    let (current_single, c_single) = (single_sensor_m("current"), single_sensor_m("c"));
    assert!(
        (current_single - 2500.0).abs() < 1.0,
        "current single-sensor {current_single} m"
    );
    assert!(
        (c_single - 7750.0).abs() < 1.0,
        "c single-sensor {c_single} m"
    );
    // 7 000 + 2 500 and 1 750 + 7 750: the length of the axis the two radars do *not*
    // see together is the same either way, because the harbour radar's own reach lies
    // inside the forward radar's in both sitings. That equality is the trade stated as a
    // number, and it is why `c` is an option rather than an improvement.
    assert!(
        ((7000.0 + current_single) - (1750.0 + c_single)).abs() < 1.0,
        "moving a radar should redistribute the approach neither radar pair covers \
         twice, not create coverage: {current_single} vs {c_single}"
    );
    let _ = std::fs::remove_dir_all(dir);
}
