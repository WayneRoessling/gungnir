//! The node load harness (GAP-056).
//!
//! `docs/performance-budgets.md` says node budgets are exercised by "the same tick harness
//! in `gungnir-node`, plus load generation through the recorded-feed adapter at scenario
//! rates". This is the load generation half.
//!
//! | Budget | `performance-budgets.md` value | Exercised here by |
//! |---|---|---|
//! | Detection ingest throughput | 5,000 detections/s sustained per node | `node_ingest/scenario_4_feed` |
//! | Journal durability | an accepted envelope is on disk within 100 ms | `node_journal/sync_every_envelope_50` |
//!
//! # Why this is a separate harness from the desktop's
//!
//! Not because the code differs — the gateway and the journal are the same crates — but
//! because **the durability profile does**. D-04 gives the node `SyncEveryEnvelope` and the
//! desktop `Buffered { 5 s }`, so fifty envelopes on the node are fifty fsyncs and on the
//! desktop are none. Running the desktop's harness and calling the number a node figure
//! would report the wrong quantity by two orders of magnitude, which is the mistake this
//! file exists to make impossible.
//!
//! # What these numbers currently mean
//!
//! `gungnir_fusion_async::PIPELINE_IMPLEMENTED` has been true since 2026-09-06 (GAP-011),
//! so detections enter the tracking stage and tracks come out of it. **The whole path
//! measured here is real**: adapter, decode, authentication, validation, quarantine, the
//! tracking stage, and the journal append with the node's fsync policy. This is therefore a
//! measurement of the node's loop and not only of ingest and durability, which is what it
//! was when the harness was written.
//!
//! One caveat that is not the pipeline's: the tracker here is built with
//! `PipelineSettings::default()` and not with a deployment's promoted baseline (GAP-053), so
//! the gate threshold is the default rather than a configured one.
//!
//! Per `../../benches/README.md` no benchmark here asserts against an absolute number; the
//! regression gate compares against the previous baseline.

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use gungnir_config::{ConfigBaseline, SensorConfig};
use gungnir_eventing::{Event, EventBus, InProcessBus};
use gungnir_ingest::adapters::recorded::RecordedFeedAdapter;
use gungnir_ingest::{AllowListAuthenticator, IngestGateway};
use gungnir_model::{DetectionView, MissionTime, Provenance, SensorId};
use gungnir_scenario::{GeneratedTimeline, Scenario, ScenarioGenerator};
use gungnir_store::{DurabilityPolicy, EventJournal, FileEventJournal, SessionId};
use gungnir_tracking_service::LiveTrackingService;
use rand::rngs::StdRng;
use rand::SeedableRng;
use std::hint::black_box;
use std::io::Write;
use std::path::{Path, PathBuf};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join(format!("gungnir-node-load-bench-{}", std::process::id()))
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch directory is creatable");
    dir
}

fn generate(scenario: &Scenario, seed: u64) -> GeneratedTimeline {
    ScenarioGenerator::new(StdRng::seed_from_u64(seed)).generate(scenario)
}

/// Write a generated timeline as a recorded feed, the format `RecordedFeedAdapter` reads.
fn write_feed(timeline: &GeneratedTimeline, path: &Path) {
    let mut file = std::fs::File::create(path).expect("feed file is writable");
    for o in &timeline.observations {
        let view = DetectionView {
            sensor: SensorId(o.detection.sensor_id),
            source_time: MissionTime(o.detection.timestamp_s),
            receipt_time: MissionTime(o.receipt_time_s),
            measurement: gungnir_model::Measurement::Position {
                enu: o.detection.measurement,
                variance_m2: [400.0, 400.0, 900.0],
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
            })
            .collect(),
        data_dir: dir.to_string_lossy().into_owned(),
        ..ConfigBaseline::default()
    }
}

fn gateway_for(config: &ConfigBaseline, feed: &Path) -> IngestGateway {
    let mut gateway = IngestGateway::new(Box::new(AllowListAuthenticator {
        allowed: config.sensors.iter().map(|s| SensorId(s.id)).collect(),
    }));
    gateway.set_expected_adapters(1);
    gateway.add_adapter(Box::new(
        RecordedFeedAdapter::open(feed).expect("the written feed parses"),
    ));
    gateway
}

/// Scenario 4, the dense one: it is what `performance-budgets.md` names for the ingest
/// throughput row, and a sparse scenario would measure an empty loop.
fn dense() -> Scenario {
    Scenario::DenseSwarm { target_count: 200 }
}

/// Detections through the whole gateway path at scenario rates.
///
/// One iteration drives the gateway until the feed is exhausted, so the reported time is
/// the cost of ingesting the whole timeline: divide by the observation count for the
/// per-detection figure the 5,000/s budget is about.
fn ingest(c: &mut Criterion) {
    let dir = scratch("ingest");
    let timeline = generate(&dense(), 4);
    let feed = dir.join("feed.jsonl");
    write_feed(&timeline, &feed);
    let config = config_for(&timeline, &dir);
    let observations = timeline.observations.len();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("runtime");

    let mut group = c.benchmark_group("node_ingest");
    group.throughput(criterion::Throughput::Elements(
        observations.try_into().unwrap_or(u64::MAX),
    ));
    group.bench_function("scenario_4_feed", |b| {
        b.iter_batched(
            || {
                (
                    gateway_for(&config, &feed),
                    LiveTrackingService::new(runtime.handle()),
                )
            },
            |(mut gateway, mut tracking)| {
                // The node's loop advances a second at a time through the timeline.
                let mut now = 0.0_f64;
                let mut seen = 0usize;
                while seen < observations && now < 3600.0 {
                    let events = gateway.tick(MissionTime(now), &mut tracking);
                    seen += events.len();
                    black_box(&events);
                    now += 1.0;
                }
                black_box(seen)
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

/// The node's durability profile, which is the half of D-04 the desktop harness cannot
/// measure: **every envelope is an fsync here**.
fn journal_sync_per_envelope(c: &mut Criterion) {
    let dir = scratch("journal");
    let bus = InProcessBus::new();
    let rx = bus.subscribe();

    let mut group = c.benchmark_group("node_journal");
    group.throughput(criterion::Throughput::Elements(50));
    group.bench_function("sync_every_envelope_50", |b| {
        b.iter_batched(
            || {
                let session = dir.join("s");
                let _ = std::fs::remove_dir_all(&session);
                FileEventJournal::open_with_policy(&session, DurabilityPolicy::SyncEveryEnvelope)
                    .expect("journal opens")
            },
            |mut journal| {
                for i in 0..50u32 {
                    bus.publish(
                        MissionTime(f64::from(i)),
                        Event::Rhythm(gungnir_model::events::RhythmEvent::ProductHeld {
                            name: "load".to_owned(),
                            at: MissionTime(f64::from(i)),
                        }),
                    )
                    .expect("published");
                }
                for envelope in rx.try_iter() {
                    journal
                        .append(SessionId(1), &envelope)
                        .expect("append succeeds");
                }
                black_box(journal.sync())
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

criterion_group!(benches, ingest, journal_sync_per_envelope);
criterion_main!(benches);
