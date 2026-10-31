// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Budget gates for the desktop frame (GAP-056).
//!
//! `../benches/app_tick.rs` is the `criterion` harness `docs/performance-budgets.md`
//! specifies; per `../../benches/README.md` a benchmark never asserts against an
//! absolute number. These are the assertions. Each one names the budget it is
//! guarding and states, in its own doc comment, whether the thing it measures is the
//! budgeted quantity yet.
//!
//! # Which budgets can honestly be gated today
//!
//! | Budget | Gated here | Why |
//! |---|---|---|
//! | Startup to first frame, under 3 s | **yes** | Every stage it covers is implemented. |
//! | Journal append, under 1 ms for 50 envelopes | **yes** since GAP-085 | Implemented, and now meets its budget under the D-04 desktop profile. |
//! | Per-frame `update()`, p99 under 4 ms | **yes** since 2026-09-15 | Gated in the GAP-067 walk, on measurements the owner read first: p99 16 microseconds release, 189 microseconds debug, against 4 ms. Release only, the convention this file already applies to the journal budget. Those frames held no track; since 2026-10-30 the convoy is in the picture first. |
//! | egui pass, p99 under 8 ms at Scenario 4 counts | **yes** since 2026-09-16 | The `gungnir-ui` §2 row's budget, agreed under D-16 and never measured once until the GAP-067 walk went looking for its test and found that nothing anywhere timed an egui pass. Measured from 2026-09-15 and gated by the owner in the same walk on reading the figures. Release only. Over the operator's workspace as well as the commander's since GAP-226, because only the operator's has the track table. Over the viewport and its layer assembly as well as the dock tree since GAP-255, because until then the picture was outside every budget; through the desktop's own `gungnir_app::picture` since GAP-257, so the assembly timed is the assembly run. |
//! | `tracks()`/`is_healthy()`, p99 under 1 ms | **yes** since 2026-09-15 | Gated in the same walk. Both calls measure p99 100 ns against a 1 ms budget -- four orders of magnitude -- so this one is asserted in **both** profiles; see its own documentation for why the usual release-only rule is not what protects it. |
//!
//! | MOP-14, coverage recompute over terrain under 10 s | **yes** since 2026-10-29 | GAP-246's dead-spot field, the first coverage computed over terrain as an area. Measured on a 1 000 by 1 000 DEM at 363 ms, 645 ms once the line of sight became exact, and gated the same day when the owner agreed `docs/design/DN-34-sensor-dead-spots.md` §11's rows, this one's included. Release only, the convention the rows above follow. |
//!
//! **What changed on 2026-09-15 was not the code.** Both rows were measured and printed
//! for over a week and deliberately not asserted, because promoting a Draft row to a gate
//! is the owner's confirmation under D-16 and not a test's to take. The walk took it
//! (GAP-067, `ARCHITECTURE.md` §10 item 132), on these figures, and the assertions below
//! are the one-line change this file was shaped to accept.

use gungnir_app::state::AppState;
use gungnir_app::update;
use gungnir_config::{ConfigBaseline, SensorConfig};
use gungnir_eventing::{Event, EventBus, InProcessBus};
use gungnir_ingest::adapters::recorded::RecordedFeedAdapter;
use gungnir_model::events::IngestEvent;
use gungnir_model::{DetectionView, MissionTime, Provenance, SensorId};
use gungnir_scenario::{GeneratedTimeline, Scenario, ScenarioGenerator};
use gungnir_store::{DurabilityPolicy, EventJournal, FileEventJournal, SessionId};
use gungnir_time::ReplayClockAuthority;
use rand::rngs::StdRng;
use rand::SeedableRng;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The per-axis measurement variance this fixture states for its detections, metres
/// squared (DN-27 §4: a position with no stated error is one the tracker has to guess a
/// gate for, and it guesses generously). The workspace's default radar figures.
const BASELINE_VARIANCE_M2: [f64; 3] = [400.0, 400.0, 900.0];

/// Frame period the desktop redraws at: `main.rs`'s `REPAINT_INTERVAL`.
const FRAME_S: f64 = 1.0 / 30.0;

// The budgets, verbatim from docs/performance-budgets.md.
const BUDGET_FRAME_P99: Duration = Duration::from_millis(4);
const BUDGET_SNAPSHOT_P99: Duration = Duration::from_millis(1);
const BUDGET_JOURNAL_50: Duration = Duration::from_millis(1);
const BUDGET_STARTUP: Duration = Duration::from_secs(3);
/// The `gungnir-ui` §2 row: "egui pass p99 under 8 ms at Scenario 4 track counts"
/// (agreed 2026-09-04, D-16).
const BUDGET_EGUI_P99: Duration = Duration::from_millis(8);
/// MOP-14, `docs/mission/measures.md`: "time to recompute coverage over terrain for the
/// sector after a sensor change, under 10 s" (confirmed 2026-09-04).
const BUDGET_COVERAGE_RECOMPUTE: Duration = Duration::from_secs(10);
/// Enough passes for a p99 to mean something without making the suite slow: the same
/// order as the snapshot budget's sample count, scaled down because a full tree render
/// costs far more than a `tracks()` call.
const EGUI_PASSES: usize = 200;

/// Runs the journal budget takes the median of. Fifty file writes is noisy enough that
/// one sample flakes against a 1 ms threshold; see the test's own documentation.
const JOURNAL_RUNS: usize = 9;

/// Runs the startup budget takes the best of; see that test's own documentation for why
/// the minimum rather than the median.
const STARTUP_RUNS: usize = 3;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join(format!("gungnir-app-frame-budgets-{}", std::process::id()))
        .join(name);
    std::fs::create_dir_all(&dir).expect("scratch directory is creatable");
    dir
}

fn write_feed(timeline: &GeneratedTimeline, path: &Path) {
    let mut file = std::fs::File::create(path).expect("feed file is writable");
    for o in &timeline.observations {
        let view = DetectionView {
            sensor: SensorId(o.detection.sensor_id),
            source_time: MissionTime(o.detection.timestamp_s),
            receipt_time: MissionTime(o.receipt_time_s),
            measurement: gungnir_model::Measurement::Position {
                enu: o.detection.measurement,
                variance_m2: BASELINE_VARIANCE_M2,
            },
            provenance: Provenance {
                source_sensor_ids: vec![o.detection.sensor_id],
                calibration_baseline_version: o.calibration.clone(),
                algorithm_version: "gungnir-scenario 0.1.0".to_owned(),
                peer: None,
                conversion_loss: None,
                authentication: gungnir_model::SourceAuthentication::default(),
                rehearsal: None,
            },
        };
        writeln!(
            file,
            "{}",
            serde_json::to_string(&view).expect("DetectionView serializes")
        )
        .expect("feed file is writable");
    }
}

fn config_for(timeline: &GeneratedTimeline, dir: &Path) -> ConfigBaseline {
    let mut sensors: Vec<u32> = timeline.sensors.iter().map(|s| s.id).collect();
    sensors.sort_unstable();
    sensors.dedup();
    ConfigBaseline {
        sensors: sensors
            .into_iter()
            .map(|id| SensorConfig {
                id,
                modality: "radar".to_owned(),
                position: [0.951_412_060_9, 0.178_023_583_7, 0.0],
                max_range_m: 200_000.0,
                control_endpoint: None,
                maintenance: Vec::new(),
                detection_model: None,
                azimuth_sector: None,
                elevation_band: None,
            })
            .collect(),
        data_dir: dir.to_string_lossy().into_owned(),
        ..ConfigBaseline::default()
    }
}

fn state_for(timeline: &GeneratedTimeline, name: &str) -> AppState {
    let dir = scratch(name);
    let feed = dir.join("feed.jsonl");
    write_feed(timeline, &feed);
    let mut state =
        AppState::with_config(config_for(timeline, &dir)).expect("desktop state builds");
    let adapter = RecordedFeedAdapter::open(&feed).expect("the written feed parses");
    state.ingest.add_adapter(Box::new(adapter));
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(0.0),
    });
    state
}

fn generate(scenario: &Scenario, seed: u64) -> GeneratedTimeline {
    ScenarioGenerator::new(StdRng::seed_from_u64(seed)).generate(scenario)
}

/// Scenario 4's target count: the dense swarm the budgets below are stated at.
const SCENARIO_4_TARGETS: usize = 200;

/// Scenario 3's convoy: the four vehicles the per-frame `update()` budget is stated over.
const SCENARIO_3_CONVOY: usize = 4;

/// Replay `timeline` into `state` until the picture holds Scenario 4's track count.
///
/// **The budgets are stated "at Scenario 4 track counts", and until GAP-226 the tests
/// measured at four.** They ticked ten seconds of the replay and stopped waiting at the
/// first track, so the egui pass and the snapshot calls were timed against a picture of
/// a handful of tracks. On this seed the picture passes 200 tracks about 45 s in, and
/// settles near 340 with the swarm's false tracks.
fn drive_to_scenario_4_counts(
    timeline: &GeneratedTimeline,
    state: &mut AppState,
    clock: &mut ReplayClockAuthority,
) {
    drive_to_track_count(timeline, state, clock, SCENARIO_4_TARGETS, "Scenario 4");
}

/// Replay `timeline` into `state` until the picture holds `count` tracks.
///
/// Gives the asynchronous pipeline a moment every two seconds of mission time, as the
/// desktop's own frames would. Bounded twice: by the timeline's own end, then by a minute
/// of wall time for a pipeline still working through it, as `sample_set_replay.rs`
/// allows. A replay that never reaches the count fails here rather than letting a budget
/// pass on a smaller picture.
fn drive_to_track_count(
    timeline: &GeneratedTimeline,
    state: &mut AppState,
    clock: &mut ReplayClockAuthority,
    count: usize,
    scenario: &str,
) {
    let end = timeline
        .observations
        .iter()
        .map(|o| o.receipt_time_s)
        .fold(0.0_f64, f64::max);
    while clock.current.0 < end && state.tracking.tracks().len() < count {
        for _ in 0..60 {
            clock.advance(FRAME_S);
            state.clock = Box::new(*clock);
            update::tick(state);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    for _ in 0..12_000 {
        if state.tracking.tracks().len() >= count {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
        state.tracking.poll(clock.current);
    }
    assert!(
        state.tracking.tracks().len() >= count,
        "the replay ended with {} tracks, short of {scenario}'s {count}, so a budget \
         measured now would be measured on a smaller picture",
        state.tracking.tracks().len()
    );
}

/// The p99 of a sorted-in-place sample. Nearest-rank, which for the sample sizes here
/// is the honest reading: no interpolation between two observations that were never
/// observed.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn p99(samples: &mut [Duration]) -> Duration {
    assert!(!samples.is_empty(), "no samples");
    samples.sort_unstable();
    let rank = ((samples.len() as f64) * 0.99).ceil() as usize;
    samples[rank.saturating_sub(1).min(samples.len() - 1)]
}

/// The wall time left between two timed frames: the pipeline's share of the frame period.
/// The desktop leaves it a whole 33 ms; one is enough for a fresh output to be waiting at
/// most polls, and keeps 3 000 frames to a few seconds.
const FRAME_GAP: Duration = Duration::from_millis(1);

/// Run `frames` more frames of the replay in `state`, [`FRAME_GAP`] apart, timing each.
/// Returns the samples and the fewest and most tracks any timed frame held.
fn frame_samples(
    state: &mut AppState,
    clock: &mut ReplayClockAuthority,
    frames: usize,
) -> (Vec<Duration>, usize, usize) {
    let mut samples = Vec::with_capacity(frames);
    let (mut fewest, mut most) = (usize::MAX, 0);
    for _ in 0..frames {
        clock.advance(FRAME_S);
        state.clock = Box::new(*clock);
        let start = Instant::now();
        update::tick(state);
        samples.push(start.elapsed());
        let held = state.tracking.tracks().len();
        fewest = fewest.min(held);
        most = most.max(held);
        std::thread::sleep(FRAME_GAP);
    }
    (samples, fewest, most)
}

/// **Gated.** Startup to first frame: config, tokio runtime, journal open, services
/// construction, and one tick. Every stage is implemented, so this is the budgeted
/// quantity. It excludes the eframe window creation `main.rs` does around it, which
/// no headless test can measure; the budget's own note scopes it to "runtime, journal
/// open, services construction".
/// # Measured as the best of several runs, and why that is the honest statistic
///
/// A single sample was gated here until 2026-09-06, and it failed whenever cargo ran
/// several test binaries at once while passing alone -- a performance claim decided by
/// how busy the machine was rather than by the code. The journal budget below already
/// took a median for the same reason, and its note says so.
///
/// The **minimum** is right for this one where a median is right for that one. A budget
/// asks whether this work *can* fit in the time allowed; the least contended sample is
/// the closest a loaded machine gets to answering that, and a run slower than the budget
/// only ever proves the machine was busy. The journal test measures file writes, whose
/// spread is the thing being characterised, so a median suits it. If every one of these
/// runs exceeds three seconds, that is a real regression and it still fails.
#[test]
fn startup_to_first_frame_is_within_budget() {
    let timeline = generate(&Scenario::ManeuveringAircraft, 1);
    let mut best = Duration::from_secs(u64::from(u32::MAX));
    for run in 0..STARTUP_RUNS {
        let start = Instant::now();
        let mut state = state_for(&timeline, &format!("startup-{run}"));
        let mut clock = ReplayClockAuthority {
            current: MissionTime(0.0),
        };
        clock.advance(FRAME_S);
        state.clock = Box::new(clock);
        update::tick(&mut state);
        best = best.min(start.elapsed());
    }
    println!("startup to first frame: {best:?} best of {STARTUP_RUNS} (budget {BUDGET_STARTUP:?})");
    assert!(
        best < BUDGET_STARTUP,
        "startup took {best:?} at its fastest of {STARTUP_RUNS} runs, over the {BUDGET_STARTUP:?} budget -- every run was over it, so this is the code and not a busy machine"
    );
}

/// **Gated 2026-09-15** (GAP-067 walk). Per-frame `update()` under Scenario 3, the
/// scenario `performance-budgets.md` names for this budget.
///
/// Every stage of the frame is real as of 2026-09-06: ingest, tracking (GAP-011),
/// planning, eventing and journal. **The frame's share of tracking is the hand-off and
/// the poll, by design**: the filtering runs on the pipeline task off the frame thread,
/// which is the property `rust-ui-architecture-coding-standards.md` asks for and the
/// reason a frame stays inside its budget while a tracker runs.
///
/// **Asserted in release only**, the rule this file already applies to the journal
/// budget and for the same reason: a debug-profile timing is not the budgeted quantity.
/// The margin is why that matters here rather than for the snapshot row below -- p99 is
/// 21 times under budget in debug, but the *worst* single frame is only three times
/// under it, which is close enough to the limit that a loaded runner could reach it.
///
/// # Timed with the convoy in the picture, since 2026-10-30
///
/// **Until then every frame this test timed held no track at all.** It ran 3 000 frames
/// from the replay's start back to back, 100 s of mission time in about 10 ms of wall
/// time, and the pipeline delivered nothing inside that, so the poll and the plan were
/// timed against an empty picture: p99 8 to 9 microseconds release. The same question
/// GAP-226 asked of the egui and snapshot gates found it. The test now drives the replay
/// until the picture holds the convoy's four vehicles and leaves [`FRAME_GAP`] between
/// timed frames, so most polls find a fresh output and the plan has tracks to answer;
/// a timed frame with no track fails it. Release figures, three runs of this file: p99
/// 85 to 103 microseconds over 4 to 10 tracks.
///
/// **Scenario 3 and not Scenario 4**, because `performance-budgets.md` states this budget
/// against Scenario 3, unlike the egui and snapshot budgets beside it. At Scenario 4's
/// count the same frames measured p99 4.9 to 5.3 ms release (216 to 349 tracks, three
/// runs), over 4 ms; that is a figure for the owner, recorded in
/// `docs/record/2026-10-30/the-per-frame-gate-timed-an-empty-picture.md`, and not this
/// row's criterion. Nearly all of it was identity resolution scanning every lineage for
/// each new track id, and with that made cheap the same frames measure p99 1.7 to 2.1 ms
/// (`docs/record/2026-10-30/the-slow-frames-at-scenario-4-were-identity.md`).
#[test]
fn per_frame_update_meets_its_budget() {
    let timeline = generate(
        &Scenario::UrbanConvoy {
            injected_bias_m: nalgebra::Vector3::new(40.0, -25.0, 5.0),
        },
        7,
    );
    let mut state = state_for(&timeline, "frame");
    let mut clock = ReplayClockAuthority {
        current: MissionTime(0.0),
    };
    drive_to_track_count(
        &timeline,
        &mut state,
        &mut clock,
        SCENARIO_3_CONVOY,
        "Scenario 3",
    );
    let (mut samples, fewest, most) = frame_samples(&mut state, &mut clock, 3_000);
    let worst = samples.iter().copied().max().unwrap_or_default();
    let p = p99(&mut samples);
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    println!(
        "per-frame update(): p99 {p:?}, worst {worst:?} over {} frames holding {fewest} to \
         {most} tracks (budget {BUDGET_FRAME_P99:?}, {profile} profile)",
        samples.len()
    );
    assert!(
        !samples.is_empty(),
        "the frame harness produced no measurements"
    );
    assert!(
        fewest > 0,
        "a timed frame held no track, so the poll and the plan were timed against an \
         empty picture"
    );
    if cfg!(debug_assertions) {
        println!(
            "  not asserted in debug; the budget is a release figure and ci.yml enforces it with --release"
        );
    } else {
        assert!(
            p < BUDGET_FRAME_P99,
            "per-frame update() p99 was {p:?} over {} frames, over the {BUDGET_FRAME_P99:?} budget",
            samples.len()
        );
    }
}

/// **Gated 2026-09-15** (GAP-067 walk). `tracks()` and `is_healthy()` latency.
///
/// The row's criterion is "p99 under 1 ms for both calls at Scenario 4 track counts".
///
/// The pipeline is asynchronous, so the frame loop finishes long before it has drained
/// the detections those frames submitted; the test therefore **waits for the snapshot
/// to populate before measuring**, and asserts that it did. Measuring an empty
/// snapshot and calling it the row's quantity is exactly the reading this file exists
/// to avoid.
///
/// **Asserted in both profiles, unlike every other budget in this file**, and the
/// reason is the margin rather than a different principle: both calls measure p99
/// 100 ns against a 1 ms budget, which is four orders of magnitude, so no profile and no
/// runner load can carry them to the limit. What a debug assertion buys is the one
/// regression the release-only rule would miss until the next release run -- a change
/// that makes `tracks()` do work, such as cloning the snapshot on every call, which
/// would blow a 10,000x margin in any profile. The release-only rule protects a budget
/// from the profile; here there is nothing to protect it from.
#[test]
fn snapshot_calls_meet_their_budget() {
    let timeline = generate(&Scenario::DenseSwarm { target_count: 200 }, 4);
    let mut state = state_for(&timeline, "snapshot");
    let mut clock = ReplayClockAuthority {
        current: MissionTime(0.0),
    };
    // **The wall-time bound inside is the guard and not the budget.** The budgets
    // asserted further down are what this test measures; two seconds of patience for a
    // background task was a timing assumption sitting inside a performance test, which is
    // the worst place for one -- it would fail the budget suite for a reason that has
    // nothing to do with a budget. A minute, on the same reasoning as
    // `sample_set_replay.rs`.
    drive_to_scenario_4_counts(&timeline, &mut state, &mut clock);

    let mut tracks_samples = Vec::with_capacity(10_000);
    let mut health_samples = Vec::with_capacity(10_000);
    for _ in 0..10_000 {
        let start = Instant::now();
        let n = state.tracking.tracks().len();
        tracks_samples.push(start.elapsed());
        let start = Instant::now();
        let healthy = state.tracking.is_healthy();
        health_samples.push(start.elapsed());
        std::hint::black_box((n, healthy));
    }
    let tracks_p99 = p99(&mut tracks_samples);
    let health_p99 = p99(&mut health_samples);
    println!(
        "tracks(): p99 {tracks_p99:?}; is_healthy(): p99 {health_p99:?}; track count {} \
         (budget {BUDGET_SNAPSHOT_P99:?})",
        state.tracking.tracks().len()
    );
    assert!(
        state.tracking.is_healthy(),
        "the pipeline is running and health must say so"
    );
    assert!(
        tracks_p99 < BUDGET_SNAPSHOT_P99,
        "tracks() p99 was {tracks_p99:?}, over the {BUDGET_SNAPSHOT_P99:?} budget"
    );
    assert!(
        health_p99 < BUDGET_SNAPSHOT_P99,
        "is_healthy() p99 was {health_p99:?}, over the {BUDGET_SNAPSHOT_P99:?} budget"
    );
}

/// **Gated in release.** The egui pass at Scenario 4 track counts.
///
/// **This budget had never been measured.** The `gungnir-ui` §2 row has carried "egui
/// pass p99 under 8 ms at Scenario 4 track counts" since D-16 agreed it on 2026-09-04,
/// and the companion table named *this file* as its test -- but this file times
/// `update()` and the two snapshot calls, and nothing anywhere in the workspace timed an
/// egui pass. The GAP-067 walk found that on 2026-09-15 by going looking for the test,
/// and this is the measurement it was missing.
///
/// **What is timed is the frame an operator sees**, not one panel: the dock tree over the
/// commander's arrangement and, since GAP-226, the operator's, which is the one with the
/// track table, and **since GAP-255 the viewport beside it**, drawn through
/// `gungnir_viewport3d::render` from the layers `main.rs` assembles every frame, through
/// `RenderProbe`, which runs a real egui pass headlessly -- layout, paint and all -- which
/// is the quantity the row names. Until GAP-255 the viewport, the layer assembly and every
/// glyph were outside every budget (`docs/review/2026-10-30-ui-review.md` §4.3 finding 17).
/// The state is driven by Scenario 4 first, so the panels and the picture draw against a
/// populated picture rather than an empty one; measuring an empty workspace and calling it
/// the row's figure is the same mistake `snapshot_calls_meet_their_budget` documents for
/// its own row. The criterion is the row's, unchanged; what moved is how much of the frame
/// it is held to.
///
/// **Asserted in release, printed in debug**, since the GAP-067 walk of 2026-09-16: the
/// budget is a release figure, and `ci.yml`'s release step enforces it with `--release`,
/// the same arrangement as `journal_append_meets_its_budget`. The debug run keeps
/// printing, because a debug timing on a shared runner is a statement about the runner.
#[test]
fn the_egui_pass_meets_its_budget() {
    let timeline = generate(&Scenario::DenseSwarm { target_count: 200 }, 4);
    let mut state = state_for(&timeline, "egui");
    let mut clock = ReplayClockAuthority {
        current: MissionTime(0.0),
    };
    // A workspace drawn against a handful of tracks is not this row.
    drive_to_scenario_4_counts(&timeline, &mut state, &mut clock);

    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    // GAP-226: the commander's workspace has no track table, so the operator's is timed
    // too. It is the one that draws PN-03 against the whole picture, the panel whose cost
    // grows with the track count.
    for (role, must_say) in [
        (gungnir_security::Role::Commander, None),
        (gungnir_security::Role::Operator, Some(" tracks, ")),
    ] {
        let mut samples = egui_pass_samples(&state, role, must_say);
        let worst = samples.iter().copied().max().unwrap_or_default();
        let p = p99(&mut samples);
        println!(
            "egui pass ({role:?} workspace, {} tracks): p99 {p:?}, worst {worst:?} over {} \
             passes (budget {BUDGET_EGUI_P99:?}, {profile} profile)",
            state.tracking.tracks().len(),
            samples.len()
        );
        if cfg!(debug_assertions) {
            println!(
                "  not asserted in debug; the budget is a release figure and ci.yml enforces it with --release"
            );
        } else {
            assert!(
                p < BUDGET_EGUI_P99,
                "the egui pass over the {role:?} workspace took p99 {p:?} over {EGUI_PASSES} \
                 passes, over the {BUDGET_EGUI_P99:?} budget"
            );
        }
    }
}

/// Time `EGUI_PASSES` full passes over `role`'s frame as the desktop lays it out: the
/// dock tree in the dashboard on the left, as `main.rs`'s `draw_workspace` puts it, and
/// the picture in the centre, drawn through the same `gungnir_app::picture` calls the
/// desktop's `draw_picture` makes (GAP-255, GAP-257): the layers are synced on every pass
/// and recomputed only when the state they read changed, which over a still state is once.
/// `must_say` is text a panel the measurement is about draws, so a workspace that left
/// that panel out, or tabbed it out of sight, fails rather than timing something smaller;
/// the viewport's own status line is held to the same rule.
fn egui_pass_samples(
    state: &AppState,
    role: gungnir_security::Role,
    must_say: Option<&str>,
) -> Vec<Duration> {
    let mut sustainment = gungnir_app::sustainment::SustainmentState::default();
    let probe = gungnir_ui::harness::RenderProbe::new();
    let layout = gungnir_workflow::WorkspaceLayout::for_role(role);
    let arrangement = gungnir_app::dock::default_arrangement(&layout);
    let mut tree = gungnir_app::dock::tree_from(&arrangement);
    let mut viewport = gungnir_viewport3d::ViewportState::new();
    let mut picture = gungnir_app::picture::PictureCache::default();
    let palette = state.palette;

    let mut samples = Vec::with_capacity(EGUI_PASSES);
    let mut drew_text = false;
    let mut said = must_say.is_none();
    let mut drew_picture = false;
    for _ in 0..EGUI_PASSES {
        let start = Instant::now();
        let (_, frame) = probe.draw(|ui| {
            egui::Panel::left("dashboard")
                .resizable(true)
                .default_size(palette.dashboard_default_width)
                .show_inside(ui, |ui| {
                    let mut behavior =
                        gungnir_app::dock::PanelBehavior::new(state, &mut sustainment);
                    tree.ui(&mut behavior, ui);
                });
            egui::CentralPanel::default().show_inside(ui, |ui| {
                picture.sync(state);
                let sources = gungnir_app::picture::Sources::of(state);
                gungnir_app::picture::render_projection(ui, &picture, &sources, &mut viewport);
            });
        });
        samples.push(start.elapsed());
        drew_text |= !frame.texts.is_empty();
        said |= must_say.is_some_and(|text| frame.says(text));
        drew_picture |= frame.says("Top-down projection (");
    }
    // GAP-257: the state did not change between passes, so every layer was computed on the
    // first and lent after. A count above one is the per-frame reassembly coming back.
    assert_eq!(
        picture.rebuilds(),
        gungnir_app::picture::Rebuilds {
            coverage: 1,
            rules: 1,
            predictions: 1,
            preview: 1,
            dead_spot_label: 1,
        },
        "a layer was reassembled on a frame whose inputs had not changed"
    );
    assert!(
        drew_text,
        "the {role:?} workspace drew no text at all, so what was timed was not a real pass"
    );
    assert!(
        said,
        "the {role:?} workspace never drew {must_say:?}, so the panel this measures was not in it"
    );
    assert!(
        drew_picture,
        "the {role:?} frame never drew the viewport's status line, so the picture was not in \
         what was timed"
    );
    assert!(
        viewport.glyphs.len() >= SCENARIO_4_TARGETS,
        "the picture held {} glyphs, short of Scenario 4's {SCENARIO_4_TARGETS}",
        viewport.glyphs.len()
    );
    samples
}

/// GAP-257: the picture's layers are recomputed when, and only when, the state each reads
/// changes. A still state is computed once and lent to every frame after; a selection
/// recomputes the preview alone; a chosen sensor recomputes the dead-spot label alone; and
/// tracks that moved recompute the predictions and leave the rule layers, which read the
/// baseline and not the tracks, where they were.
#[test]
fn the_picture_is_reassembled_only_when_its_inputs_change() {
    let timeline = generate(&Scenario::DenseSwarm { target_count: 200 }, 4);
    let mut state = state_for(&timeline, "picture-cache");
    let mut clock = ReplayClockAuthority {
        current: MissionTime(0.0),
    };
    drive_to_track_count(
        &timeline,
        &mut state,
        &mut clock,
        50,
        "the cache's first picture",
    );

    let mut picture = gungnir_app::picture::PictureCache::default();
    picture.sync(&state);
    let once = gungnir_app::picture::Rebuilds {
        coverage: 1,
        rules: 1,
        predictions: 1,
        preview: 1,
        dead_spot_label: 1,
    };
    assert_eq!(
        picture.rebuilds(),
        once,
        "the first sync computes every group"
    );
    for _ in 0..5 {
        picture.sync(&state);
    }
    assert_eq!(picture.rebuilds(), once, "a still state is computed once");

    state.select_laydown(gungnir_model::LaydownId("west-2".into()));
    picture.sync(&state);
    assert_eq!(
        picture.rebuilds(),
        gungnir_app::picture::Rebuilds { preview: 2, ..once },
        "a selection recomputes the preview and nothing else"
    );

    state.dead_spots.for_sensor = Some(SensorId(1));
    picture.sync(&state);
    assert_eq!(
        picture.rebuilds(),
        gungnir_app::picture::Rebuilds {
            preview: 2,
            dead_spot_label: 2,
            ..once
        },
        "a chosen sensor recomputes its label and nothing else"
    );

    let before = state.tracking.tracks().to_vec();
    drive_to_track_count(&timeline, &mut state, &mut clock, 80, "a moved picture");
    assert_ne!(
        before,
        state.tracking.tracks(),
        "the drive moved the tracks"
    );
    picture.sync(&state);
    let after = picture.rebuilds();
    assert_eq!(
        after.predictions, 2,
        "moved tracks recompute the predictions"
    );
    assert_eq!(
        after.rules, 1,
        "the rule layers read the baseline, not the tracks"
    );
    assert_eq!(after.preview, 2);
    assert_eq!(after.dead_spot_label, 2);
}

/// **Gated.** Journal append: 50 envelopes in under 1 ms, under the D-04 desktop
/// profile the desktop actually runs (`DurabilityPolicy::desktop()`).
///
/// This is the one desktop budget whose whole path is implemented, and until GAP-085
/// it was the only one failing: `FileEventJournal::append` opened the session file,
/// wrote one line, and closed it once per envelope, which measured about 5.7 ms in
/// release and 8.3 ms in debug. It now holds the file open behind a buffered writer
/// and fsyncs on the D-04 schedule.
///
/// The profile matters to what this measures, and the test names it rather than
/// relying on the default: under `DurabilityPolicy::node()` fifty envelopes are fifty
/// fsyncs and this budget is neither met nor meant to be. The node's own budget is
/// the different one, "an accepted envelope is on disk within 100 ms".
///
/// # Why a median rather than one sample
///
/// This test measured a single append run until 2026-09-05, and it flaked: repeated
/// runs on an idle machine gave 612 µs to 953 µs, and one run under load reached
/// 1.01 ms and failed. Fifty file writes is a noisy quantity, and one sample of it is
/// the wrong statistic -- the rest of the harness reports p99 over thousands of frames
/// for exactly this reason. The threshold is unchanged at the budget's 1 ms; what
/// changed is that the measurement is now the median of [`JOURNAL_RUNS`] runs.
///
/// Note also that `cargo test` builds in debug, where this work costs roughly four
/// times its release cost -- 157 µs release against about 665 µs debug. Holding a
/// debug measurement to a budget stated for the shipping build is deliberate
/// conservatism, but it leaves the margin thin, which is why the statistic has to be
/// the right one.
///
/// # Why the assertion is release-only (owner decision, 2026-09-07)
///
/// That conservatism stopped being conservative and became wrong. On a GitHub Actions
/// runner this measured a median of **1.844781 ms** against the 1 ms budget -- worse
/// than the 1.0681 ms a Windows development machine gives. The budget in
/// `performance-budgets.md` is a product figure whose evidence is 157 µs in release; a
/// debug build is not the thing that budget describes, so failing on it reported a
/// defect that did not exist.
///
/// **What makes the debug build slow is the encode, not the I/O** (GAP-092, measured
/// 2026-09-07 after the first reading blamed a shared runner's throttled disk). On a
/// P-core, `serde_json::to_string` alone is ~500 µs of a ~515 µs append and the file
/// write is 15-50 µs -- nineteen twentieths against one. `%TEMP%` on C: and `target/`
/// on D: measure the same to within 2%, so the filesystem is not the variable and
/// moving this test's scratch directory would change nothing. Pinning to each of the
/// twenty logical CPUs of an 8 P-core, 12 E-core machine splits 8 fast from 12 slow on
/// the P/E boundary exactly, which reproduces the entire spread with no disk involved.
/// In release the same path is 45 µs on a P-core and 157 µs on an E-core.
///
/// The decision is unchanged by that; fifty `serde_json` encodes at `opt-level = 0`
/// cost about a millisecond on ordinary hardware whatever the disk does. It is written
/// down because the wrong reason points at the wrong fix.
///
/// The budget itself is unchanged at 1 ms. What changed is which build it is asserted
/// against. The measurement still runs on every profile and is always printed, so the
/// debug figure stays visible rather than disappearing.
///
/// **`ci.yml` runs this test a second time with `--release`, and that is where the gate
/// is enforced.** Without that step a release-only budget would be a gate no job could
/// ever fail -- which is worse than the flaky one it replaced, because it would look
/// green while measuring nothing.
#[test]
fn journal_append_meets_its_budget() {
    let dir = scratch("journal");
    let bus = InProcessBus::new();
    let rx = bus.subscribe();
    let bus: Box<dyn EventBus> = Box::new(bus);
    for i in 0..50 {
        bus.publish(
            MissionTime(f64::from(i)),
            Event::Ingest(IngestEvent::Accepted(DetectionView {
                sensor: SensorId(1),
                source_time: MissionTime(f64::from(i)),
                receipt_time: MissionTime(f64::from(i) + 0.1),
                measurement: gungnir_model::Measurement::Position {
                    enu: nalgebra::Vector3::new(f64::from(i), 1.0, 2.0),
                    variance_m2: BASELINE_VARIANCE_M2,
                },
                provenance: Provenance::default(),
            })),
        )
        .expect("publish succeeds");
    }
    let envelopes: Vec<_> = rx.try_iter().collect();
    assert_eq!(envelopes.len(), 50);

    let mut journal = FileEventJournal::open_with_policy(dir, DurabilityPolicy::desktop())
        .expect("journal opens");
    assert!(
        !journal.policy().syncs_every_envelope(),
        "this budget is stated for the buffered desktop profile"
    );

    // One session throughout, which is what the budget describes: the recurring
    // per-frame cost on a live session. A fresh session per run would switch sessions
    // each time, and a switch fsyncs the file being left -- charging this measurement
    // for durability work that a frame does not do.
    let session = SessionId(1);
    let mut samples = Vec::with_capacity(JOURNAL_RUNS);
    for _ in 0..JOURNAL_RUNS {
        let start = Instant::now();
        for envelope in &envelopes {
            journal
                .append(session, envelope)
                .expect("journal append succeeds");
        }
        samples.push(start.elapsed());
    }
    samples.sort_unstable();
    let median = samples[samples.len() / 2];
    let worst = samples.last().copied().unwrap_or_default();

    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    println!(
        "journal append, 50 envelopes: median {median:?}, worst {worst:?} over {JOURNAL_RUNS} runs (budget {BUDGET_JOURNAL_50:?}, {profile} profile)"
    );
    if cfg!(debug_assertions) {
        // Measured and reported, not asserted: see this test's documentation. The gate
        // runs in `ci.yml`'s release step.
        println!(
            "  not asserted in debug; the budget is a release figure and ci.yml enforces it with --release"
        );
    } else {
        assert!(
            median < BUDGET_JOURNAL_50,
            "journaling 50 envelopes took a median of {median:?} over {JOURNAL_RUNS} runs, over the {BUDGET_JOURNAL_50:?} budget"
        );
    }
}

/// A 1 000 by 1 000 ESRI ASCII DEM of 30 m cells in the local frame, centred on the
/// origin: two superposed swells of rolling hills between 0 and 900 m.
fn write_hills_dem(path: &Path) {
    const SIDE: u32 = 1_000;
    const CELL_M: f64 = 30.0;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path).expect("writable"));
    let half = f64::from(SIDE) * CELL_M / 2.0;
    writeln!(
        f,
        "ncols {SIDE}\nnrows {SIDE}\nxllcorner {}\nyllcorner {}\ncellsize {CELL_M}\nNODATA_value -9999",
        -half, -half
    )
    .expect("header");
    for row in 0..SIDE {
        let north = half - (f64::from(row) + 0.5) * CELL_M;
        let line: Vec<String> = (0..SIDE)
            .map(|c| {
                let east = -half + (f64::from(c) + 0.5) * CELL_M;
                let h = 300.0 * ((east / 2_000.0).sin() * (north / 2_700.0).cos())
                    + 150.0 * (east / 700.0 + north / 900.0).sin()
                    + 450.0;
                format!("{h:.1}")
            })
            .collect();
        writeln!(f, "{}", line.join(" ")).expect("a row");
    }
}

/// **Gated 2026-10-29** (GAP-246, MOP-14: "time to recompute coverage over terrain for the
/// sector after a sensor change, under 10 s"), with DN-34 §11's last row, agreed by the
/// owner that day after reading the figure this test printed: 363 ms. The line of sight
/// became exact the same day (DN-34 amendment 1 item 1), and the figure is 645 ms since,
/// on the same machine. The dead-spot field
/// over a 1 000 by 1 000 synthetic DEM at 30 m, three radars switched from standby to
/// search. Two halves, both asserted:
///
/// - **No frame waits on it.** The tick that sees the change hands the work to the worker
///   and returns with the old field still the one drawn: structural, so asserted in any
///   profile would hold, and asserted here once. The slowest tick meanwhile is printed.
/// - **Under 10 s** from that tick to the one that holds a field computed under the change.
///   Release only, because the budget is a release figure and `ci.yml` runs this file with
///   `--release`, the convention every budget above follows.
#[test]
fn the_dead_spot_recompute_meets_mop_14() {
    if cfg!(debug_assertions) {
        println!("dead-spot recompute: not measured in debug; the budget is a release figure");
        return;
    }
    let dir = scratch("dead-spots");
    let mut state = hills_desktop(&dir);
    let wait = |state: &mut AppState, done: &dyn Fn(&AppState) -> bool| -> Duration {
        let mut slowest = Duration::ZERO;
        for _ in 0..12_000 {
            let tick = Instant::now();
            update::tick(state);
            slowest = slowest.max(tick.elapsed());
            if done(state) {
                return slowest;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!(
            "never settled: {}",
            gungnir_app::dead_spots::health_line(state)
        );
    };
    wait(&mut state, &|s| {
        s.height_field.is_some() && s.dead_spots.computed.is_some()
    });
    for id in 1..=3 {
        gungnir_app::sustainment::record_observed_mode(
            &mut state,
            id,
            gungnir_model::SensorMode::Search,
        )
        .expect("searching");
    }
    let started = Instant::now();
    update::tick(&mut state);
    assert!(
        state.dead_spots.computing()
            && state
                .dead_spots
                .computed
                .as_ref()
                .is_some_and(|c| c.inputs.sensors.is_empty()),
        "the tick that saw the change waited for the field: {}",
        gungnir_app::dead_spots::health_line(&state)
    );
    let slowest = wait(&mut state, &|s| {
        s.dead_spots
            .computed
            .as_ref()
            .is_some_and(|c| c.inputs.sensors.len() == 3)
            && !s.dead_spots.stale
    });
    let recompute = started.elapsed();
    let computed = state.dead_spots.computed.as_ref().expect("a field");
    println!(
        "dead-spot recompute: {recompute:?} from the change to the field ({} by {} cells of \
         {:.0} m, the worker took {:?}); slowest tick meanwhile {slowest:?} \
         (MOP-14 budget {BUDGET_COVERAGE_RECOMPUTE:?}, release profile)",
        computed.field.columns,
        computed.field.rows,
        computed.field.parameters.cell_m,
        computed.took
    );
    assert!(
        recompute < BUDGET_COVERAGE_RECOMPUTE,
        "the dead-spot field took {recompute:?} from the change, over MOP-14's \
         {BUDGET_COVERAGE_RECOMPUTE:?}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// **Measured, not gated** (GAP-252, `docs/design/DN-35-terrain-in-the-three-d-scene.md`
/// §8.1 row g, a draft until the owner agrees it). The viewport's egui pass over the
/// 1 000 by 1 000 hills DEM, in the scene path with the GL pass holding the surface and in
/// the projection, against the egui budget the row names; and the once-per-load build of
/// the surface both renderers draw, which is what used to happen every frame.
///
/// Two halves. **Structural, asserted in every profile:** the scene's egui pass draws no
/// terrain mesh, so the ground is not drawn twice and not built in egui per frame.
/// **Timed, printed:** the build, and the p99 of each path. Release only, as every budget
/// above, and not asserted, because the row is not yet a criterion.
#[test]
#[allow(clippy::too_many_lines)]
fn the_scene_path_draws_no_egui_terrain_and_the_surface_is_built_once() {
    let dir = scratch("scene-ground");
    let mut state = hills_desktop(&dir);
    for _ in 0..12_000 {
        update::tick(&mut state);
        if !matches!(
            state.terrain,
            gungnir_app::terrain::TerrainStatus::Loading { .. }
                | gungnir_app::terrain::TerrainStatus::NotConfigured
        ) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(state.terrain.is_masking(), "{:?}", state.terrain);
    let layer = gungnir_app::terrain::layer(&state.terrain, &state.data).expect("placed");
    let palette = state.palette;

    let started = Instant::now();
    let surface = gungnir_viewport3d::layers::terrain_surface(&palette, layer).expect("a surface");
    let build = started.elapsed();
    let kept = surface.positions.len();

    let inputs = || gungnir_viewport3d::layers::LayerInputs {
        coverage: gungnir_viewport3d::layers::CoverageLayer::Circles {
            circles: &[],
            nominal: None,
        },
        gaps: &[],
        hazards: &[],
        geofences: &[],
        predictions: &[],
        terrain: Some(layer),
        terrain_tiles: None,
        point_clouds: &[],
        laydown_preview: None,
        dense_group: None,
        dead_spots: gungnir_viewport3d::layers::DeadSpotLayer::NotComputed { reason: "a test" },
    };
    let screen = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1_600.0, 1_000.0),
        )),
        ..egui::RawInput::default()
    };
    let is_terrain = |s: &egui::epaint::ClippedShape| matches!(&s.shape, egui::Shape::Mesh(m) if m.vertices.len() == kept);
    let ctx = egui::Context::default();
    let mut viewport = gungnir_viewport3d::ViewportState::new();
    viewport.view.meters_per_px = 20.0;
    let plan = state.last_plan.clone();
    let pass = |scene: bool, viewport: &mut gungnir_viewport3d::ViewportState| {
        let started = Instant::now();
        let output = ctx.run_ui(screen(), |ui| {
            if scene {
                gungnir_viewport3d::prepare_3d(
                    ui,
                    &palette,
                    viewport,
                    &[],
                    &[],
                    &plan,
                    inputs(),
                    Some(layer.revision),
                );
            } else {
                gungnir_viewport3d::render(ui, &palette, viewport, &[], &[], &plan, inputs());
            }
        });
        (
            started.elapsed(),
            output.shapes.iter().filter(|s| is_terrain(s)).count(),
        )
    };
    // The first frame builds the surface; every frame after reads it.
    let (load, _) = pass(true, &mut viewport);
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    for (scene, name) in [(true, "scene"), (false, "projection")] {
        let mut samples = Vec::with_capacity(EGUI_PASSES);
        for _ in 0..EGUI_PASSES {
            let (took, terrain_meshes) = pass(scene, &mut viewport);
            assert_eq!(
                terrain_meshes,
                usize::from(!scene),
                "the {name}'s egui pass drew the terrain {terrain_meshes} times"
            );
            samples.push(took);
        }
        let worst = samples.iter().copied().max().unwrap_or_default();
        let p = p99(&mut samples);
        println!(
            "viewport egui pass, {name} path, terrain {} vertices kept: p99 {p:?}, worst {worst:?} \
             over {} passes (egui budget {BUDGET_EGUI_P99:?}, {profile} profile; not asserted, \
             DN-35 §8.1 row g is a draft)",
            kept,
            samples.len()
        );
    }
    println!(
        "terrain surface built once per load: {build:?} over {} by {} vertices; the load frame \
         took {load:?} ({profile} profile)",
        layer.columns, layer.rows
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// A desktop over the hills DEM with three 12 km radars at standby, ready to switch on.
fn hills_desktop(dir: &Path) -> AppState {
    let dem = dir.join("hills.asc");
    write_hills_dem(&dem);
    let origin = [0.959_931, 0.209_44, 0.0];
    let frame = gungnir_model::LocalFrame::new(gungnir_model::Geodetic {
        lat_rad: origin[0],
        lon_rad: origin[1],
        alt_m: origin[2],
    });
    let at = |e: f64, n: f64| {
        let g = frame.to_geodetic([e, n, 1_200.0]);
        [g.lat_rad, g.lon_rad, g.alt_m]
    };
    let sensors: Vec<SensorConfig> = [(-8_000.0, -5_000.0), (7_000.0, -3_000.0), (0.0, 9_000.0)]
        .iter()
        .zip(1u32..)
        .map(|(&(e, n), id)| SensorConfig {
            id,
            modality: "radar".into(),
            position: at(e, n),
            max_range_m: 12_000.0,
            control_endpoint: None,
            maintenance: Vec::new(),
            detection_model: None,
            azimuth_sector: None,
            elevation_band: None,
        })
        .collect();
    let config = ConfigBaseline {
        data_dir: dir.join("data").to_string_lossy().into_owned(),
        origin: Some(origin),
        sensors,
        terrain: Some(gungnir_config::TerrainConfig {
            path: Some(dem.to_string_lossy().into_owned()),
            store: None,
            manifest_sha256: None,
            frame: "local-enu".into(),
            vertical: None,
            nad83_realisation: None,
            vertical_sigma_m: None,
        }),
        geoid_grid_dir: Some(dir.join("no-grid-here").to_string_lossy().into_owned()),
        ..ConfigBaseline::default()
    };
    gungnir_config::validate(&config).expect("valid");
    AppState::with_config(config).expect("starts")
}
