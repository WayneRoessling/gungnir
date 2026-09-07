//! The desktop tick harness (GAP-056).
//!
//! `docs/performance-budgets.md` says the desktop budgets are measured by "a
//! `criterion` harness around `gungnir-app::update::tick` fed by `gungnir-scenario`
//! output", and that it comes first because it covers the most budgets. This is that
//! harness. It measures the four budgets that are properties of one frame:
//!
//! | Budget | `performance-budgets.md` value | Exercised here by |
//! |---|---|---|
//! | Per-frame `update()` (ingest + poll + plan + journal) | p99 under 4 ms | `tick/scenario_3` |
//! | `tracks()` and `is_healthy()` snapshot calls | p99 under 1 ms | `snapshot_calls` |
//! | Journal append cost per frame | under 1 ms for 50 envelopes | `journal_append_50` |
//! | Startup to first frame | under 3 s | `startup` |
//!
//! # What these numbers currently mean
//!
//! `gungnir_fusion_async::PIPELINE_IMPLEMENTED` has been `true` since 2026-09-06
//! (GAP-011), so the tracking stage produces tracks and the frame time below is made
//! of every stage: ingest, tracking, planning, eventing, and journal. **These are
//! therefore measurements of the budgeted quantity**, which they were not when this
//! harness was written -- which is what writing the harness first was for. The
//! snapshot-latency row is now measured against a populated snapshot in
//! `tests/frame_budgets.rs`; it stays Draft in
//! `docs/verification-capability-table.md` §2 because the owner has not walked it
//! (GAP-067), not because the number is missing.
//!
//! What the frame does **not** yet carry is the estimators that are separate rows and
//! separate gaps: the pipeline runs one constant-velocity Kalman filter per track, so
//! GAP-011's remainder, GAP-015 and GAP-013 will add cost this number does not have.
//!
//! Per `../../benches/README.md`, no benchmark here asserts a pass or a fail against
//! an absolute number; the regression gate compares against the previous baseline.
//! The budget assertions that *can* honestly be made today are separate tests, in
//! `tests/frame_budgets.rs`.

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use gungnir_app::state::AppState;
use gungnir_app::update;
use gungnir_config::{ConfigBaseline, SensorConfig};
use gungnir_ingest::adapters::recorded::RecordedFeedAdapter;
use gungnir_model::{DetectionView, MissionTime, Provenance, SensorId};
use gungnir_scenario::{GeneratedTimeline, Scenario, ScenarioGenerator};
use gungnir_time::ReplayClockAuthority;
use rand::rngs::StdRng;
use rand::SeedableRng;
use std::hint::black_box;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The per-axis measurement variance this fixture states for its detections, metres
/// squared (DN-27 §4: a position with no stated error is one the tracker has to guess a
/// gate for, and it guesses generously). The workspace's default radar figures.
const BASELINE_VARIANCE_M2: [f64; 3] = [400.0, 400.0, 900.0];

/// Scratch root for the journals and recorded feeds this harness writes. Under the
/// system temp directory, never in the workspace.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join(format!("gungnir-app-tick-bench-{}", std::process::id()))
        .join(name);
    std::fs::create_dir_all(&dir).expect("scratch directory is creatable");
    dir
}

fn generate(scenario: &Scenario, seed: u64) -> GeneratedTimeline {
    ScenarioGenerator::new(StdRng::seed_from_u64(seed)).generate(scenario)
}

/// Write a generated timeline as a recorded feed, the format
/// `RecordedFeedAdapter` reads and `docs/test-tracks/data-format.md` §3 specifies.
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
            },
        };
        let line = serde_json::to_string(&view).expect("DetectionView serializes");
        writeln!(file, "{line}").expect("feed file is writable");
    }
}

/// A baseline that journals into `dir` and admits every sensor the timeline uses.
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
                // The gateway admits by id; the geodetic position is not read on the
                // frame path, so the scenario origin stands in for it.
                position: [0.951_412_060_9, 0.178_023_583_7, 0.0],
                max_range_m: 200_000.0,
                control_endpoint: None,
                maintenance: Vec::new(),
            })
            .collect(),
        data_dir: dir.to_string_lossy().into_owned(),
        ..ConfigBaseline::default()
    }
}

/// Frame period the desktop redraws at, seconds: `main.rs`'s `REPAINT_INTERVAL`.
const FRAME_S: f64 = 1.0 / 30.0;

/// Frames per criterion iteration in the tick group, so the reported time is that
/// many frames' work: divide by it for the per-frame figure. A single frame is a few
/// microseconds and rebuilding the desktop around it costs tens of milliseconds, so
/// timing one frame per setup would measure the setup's shadow rather than the frame.
const FRAMES_PER_SAMPLE: usize = 300;

/// A desktop wired to replay `timeline` through the real ingest gateway, on a
/// **replay clock** rather than the wall clock.
///
/// This matters more than it looks. `AppState::new` installs a `WallClockAuthority`,
/// whose `now()` is Unix seconds -- around 1.8e9. A recorded scenario's source times
/// are mission seconds from zero, so on the very first tick every detection in the
/// file is already "due" and the whole feed drains in one frame. Measured that way a
/// frame takes a quarter of a second and means nothing. `ReplayClockAuthority` is
/// what `gungnir-time` provides for exactly this, and stepping it one frame at a
/// time is what makes the measurement a frame.
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

/// Advance the replay clock by one frame and run one tick.
///
/// The clock is boxed behind `dyn TimeAuthority`, which has no `advance`, so the
/// harness keeps its own and reinstalls it. That is one small allocation per frame
/// inside the measured region; against a 4 ms budget it is noise, and it is the same
/// on every run, so it does not distort a regression comparison.
fn step(state: &mut AppState, clock: &mut ReplayClockAuthority) {
    clock.advance(FRAME_S);
    state.clock = Box::new(*clock);
    update::tick(black_box(state));
}

/// Per-frame `update()`: the budget that covers ingest, poll, plan, and journal.
///
/// Scenario 3 is the one `performance-budgets.md` names for this budget: three
/// sensors at mismatched rates, so a frame has a realistic mixture of arrivals rather
/// than one sensor's steady drip.
fn bench_tick(c: &mut Criterion) {
    let timeline = generate(
        &Scenario::UrbanConvoy {
            injected_bias_m: nalgebra::Vector3::new(40.0, -25.0, 5.0),
        },
        7,
    );
    let mut group = c.benchmark_group("tick");
    group.bench_function("scenario_3_300_frames", |b| {
        b.iter_batched_ref(
            || {
                (
                    state_for(&timeline, "tick-scenario-3"),
                    ReplayClockAuthority {
                        current: MissionTime(0.0),
                    },
                )
            },
            |(state, clock)| {
                for _ in 0..FRAMES_PER_SAMPLE {
                    step(state, clock);
                }
            },
            BatchSize::PerIteration,
        );
    });
    group.finish();
}

/// The snapshot calls the UI makes every frame. These must stay cheap enough that a
/// panel can call them without thinking about it.
fn bench_snapshot_calls(c: &mut Criterion) {
    let timeline = generate(&Scenario::DenseSwarm { target_count: 200 }, 4);
    let mut state = state_for(&timeline, "snapshot");
    let mut clock = ReplayClockAuthority {
        current: MissionTime(0.0),
    };
    for _ in 0..300 {
        step(&mut state, &mut clock);
    }

    let mut group = c.benchmark_group("snapshot_calls");
    group.bench_function("tracks", |b| {
        b.iter(|| black_box(state.tracking.tracks().len()));
    });
    group.bench_function("is_healthy", |b| {
        b.iter(|| black_box(state.tracking.is_healthy()));
    });
    group.finish();
}

/// The journal append budget: 50 envelopes in under 1 ms. Unlike the frame budget,
/// this one measures a fully implemented path -- `gungnir-store` is real -- so the
/// number here is the budgeted quantity, not a placeholder.
///
/// Measured under `DurabilityPolicy::desktop()`, the buffered D-04 profile the desktop
/// runs, because that is the profile the budget is stated for. The node profile fsyncs
/// every envelope and is measured against its own, different budget.
fn bench_journal_append(c: &mut Criterion) {
    use gungnir_eventing::{Event, EventBus, InProcessBus};
    use gungnir_model::events::IngestEvent;
    use gungnir_store::{EventJournal, FileEventJournal, SessionId};

    let dir = scratch("journal");
    let mut group = c.benchmark_group("journal_append_50");
    group.bench_function("fifty_envelopes", |b| {
        b.iter_batched_ref(
            || {
                let journal = FileEventJournal::open_with_policy(
                    dir.clone(),
                    gungnir_store::DurabilityPolicy::desktop(),
                )
                .expect("journal opens");
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
                (journal, rx.try_iter().collect::<Vec<_>>())
            },
            |(journal, envelopes)| {
                for envelope in envelopes.iter() {
                    journal
                        .append(SessionId(1), envelope)
                        .expect("journal append succeeds");
                }
            },
            BatchSize::LargeInput,
        );
    });
    group.finish();
}

/// Startup to first frame: config load, runtime, journal open, services, one tick.
fn bench_startup(c: &mut Criterion) {
    let timeline = generate(&Scenario::ManeuveringAircraft, 1);
    let mut group = c.benchmark_group("startup");
    // Sampling is deliberately small: each iteration opens a tokio runtime and a
    // journal, and the budget is 3 s, so a handful of samples is enough to see a
    // regression of the size that matters.
    group.sample_size(10);
    group.bench_function("to_first_frame", |b| {
        b.iter(|| {
            let mut state = state_for(&timeline, "startup");
            let mut clock = ReplayClockAuthority {
                current: MissionTime(0.0),
            };
            step(&mut state, &mut clock);
            black_box(state.health.tracking_healthy)
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_tick,
    bench_snapshot_calls,
    bench_journal_append,
    bench_startup
);
criterion_main!(benches);
