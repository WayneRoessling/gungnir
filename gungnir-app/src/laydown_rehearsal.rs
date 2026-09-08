// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Running a committed test-track scenario through the live pipeline against a
//! candidate laydown's resource placement (GAP-045, PN-16's rehearsal section).
//!
//! **This replays a fixture; it does not generate one.** `gungnir-scenario` is a
//! test/bench dependency only -- its own `Cargo.toml` says so, and
//! `gungnir-app/tests/dependency_graph.rs`'s `scenario_misuse` check refuses any
//! production crate an edge to it. A rehearsal instead reads one of the ten committed
//! `testdata/tracks/samples/TT-0N-sample/` fixtures directly: `detections.jsonl`
//! through the same [`RecordedFeedAdapter`] a live sensor feed uses, `metadata.json`
//! for the scenario's own duration and fictional origin, `sensors.json` for which
//! sensor ids it speaks. `docs/test-tracks/data-format.md` documents the format and
//! `gungnir-app/benches/app_tick.rs` already proves the mechanism -- generate (there),
//! or in this case read, a feed; register it on a real `IngestGateway`; step a
//! [`ReplayClockAuthority`] through [`update::tick`]. This module is that same
//! mechanism pointed at a fixture file instead of a freshly generated one, run against
//! a throwaway [`AppState`] so nothing here touches the live desktop's own journal or
//! picture.
//!
//! **What "as seen through this laydown" honestly means today, and what it does not.**
//! A laydown's resource placements are real inputs here: the throwaway configuration's
//! `resources` take their positions from the candidate laydown rather than the current
//! deployment's, so the intercept geometry a rehearsal reports reflects where *this*
//! option would put its effectors. A laydown's *sensor* placements are not: the
//! detections in `detections.jsonl` were captured once, from the fixture's own sensor
//! geometry, and nothing about which laydown is selected changes a single line in that
//! file. Moving a sensor in a laydown therefore changes nothing a rehearsal can show
//! yet -- a real, open limitation, not a decision, and closing it means teaching
//! `gungnir-scenario` to generate against an arbitrary sensor placement, which is its
//! own increment.
//!
//! **What a `RehearsalRecord` reports, and why nothing here is a distance.** WF-16's
//! mockup shows a range at which a stream was "first engaged"; DN-02 §7 already says
//! that number is the aggregate of predictions over a rehearsal, and no note has yet
//! specified how they aggregate (`docs/design/DN-26-laydown-options.md` never
//! addresses it, `GAP-020`'s own remaining item is exactly this). Inventing an
//! aggregation rule here to make the mockup's shape would be deciding it without the
//! review such a rule needs, which is the discipline `GAP-087`'s own text already
//! applies to this exact panel. What this module reports instead is only what a real
//! tick loop actually measured: tracks the pipeline formed, and how the approval queue
//! it drove behaved.

use gungnir_command::ApprovalWorkflow;
use gungnir_config::{ConfigBaseline, ResourceConfig, SensorConfig};
use gungnir_coord::{CoordTransform, Enu, Geodetic, Wgs84};
use gungnir_ingest::adapters::recorded::RecordedFeedAdapter;
use gungnir_model::{Laydown, LaydownId, MissionTime, TestTrackNumber};
use gungnir_time::ReplayClockAuthority;
use std::path::{Path, PathBuf};

use crate::state::AppState;
use crate::update;

fn fixture_dir(scenario: TestTrackNumber, testdata_root: &Path) -> PathBuf {
    testdata_root
        .join("tracks/samples")
        .join(format!("{}-sample", scenario.label()))
}

/// The pieces of a fixture this module actually reads. Only the fields a rehearsal
/// needs are named; `metadata.json` and `sensors.json` carry a great deal more that
/// `docs/test-tracks/data-format.md` documents and this never looks at.
#[derive(serde::Deserialize)]
struct Metadata {
    duration_s: f64,
    origin: MetadataOrigin,
}

#[derive(serde::Deserialize)]
struct MetadataOrigin {
    /// Degrees, per `data-format.md`'s "angles degrees" -- not radians.
    lat: f64,
    lon: f64,
    alt_m: f64,
}

#[derive(serde::Deserialize)]
struct SensorsFile {
    sensors: Vec<SensorEntry>,
}

#[derive(serde::Deserialize)]
struct SensorEntry {
    id: u32,
}

/// What a rehearsal actually measured. Every field is read off a real tick loop; none
/// is estimated or interpolated to fill in a number the run did not produce (see the
/// module doc for why first-engagement range is not one of these fields).
#[derive(Debug, Clone, PartialEq)]
pub struct RehearsalRecord {
    pub scenario: TestTrackNumber,
    pub laydown: LaydownId,
    /// Tracks the pipeline had formed by the end of the run.
    pub tracks_formed: usize,
    /// Every plan that entered the approval queue during the run, decided or still
    /// pending when the run ended.
    pub decisions_raised: usize,
    /// The subset of those that expired rather than being decided (DN-10).
    pub decisions_expired: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum RehearsalError {
    #[error("reading {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} does not parse as {what}: {source}")]
    Parse {
        path: PathBuf,
        what: &'static str,
        #[source]
        source: serde_json::Error,
    },
    #[error("the fixture's own feed could not be read back: {0}")]
    Feed(#[from] gungnir_ingest::IngestError),
    #[error("a rehearsal's own throwaway desktop would not start: {0}")]
    State(#[from] crate::state::AppError),
}

fn read_json<T: serde::de::DeserializeOwned>(
    path: &Path,
    what: &'static str,
) -> Result<T, RehearsalError> {
    let text = std::fs::read_to_string(path).map_err(|source| RehearsalError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_str(&text).map_err(|source| RehearsalError::Parse {
        path: path.to_path_buf(),
        what,
        source,
    })
}

/// A laydown's ENU resource offset, re-expressed as the geodetic position
/// `ResourceConfig` takes, about `origin`. Exact for the same origin used both here and
/// wherever the position is next read back into a local frame (`DpInterceptService::
/// with_local_frame`) -- see the module doc for why this is a self-consistent detour
/// rather than a claim about where the resource sits on the real Earth.
fn geodetic_of(position_enu: [f64; 3], origin: Geodetic) -> [f64; 3] {
    let enu = Enu {
        e_m: position_enu[0],
        n_m: position_enu[1],
        u_m: position_enu[2],
    };
    let g = Wgs84::ecef_to_geodetic(Wgs84::enu_to_ecef(enu, origin));
    [g.lat_rad, g.lon_rad, g.alt_m]
}

/// A throwaway configuration admitting the fixture's own sensors and carrying
/// `laydown`'s resource placements, about the fixture's own fictional origin (the
/// module doc explains why the deployment's real origin is not used here).
fn config_for(
    sensors: &[SensorEntry],
    origin: Geodetic,
    laydown: &Laydown,
    base_resources: &[ResourceConfig],
    dir: &Path,
) -> ConfigBaseline {
    let resources = base_resources
        .iter()
        .map(|r| {
            let placed = laydown.resources.iter().find(|p| p.resource.0 == r.id);
            match placed {
                Some(p) => ResourceConfig {
                    position: geodetic_of(p.position_enu, origin),
                    ..r.clone()
                },
                None => r.clone(),
            }
        })
        .collect();
    ConfigBaseline {
        sensors: sensors
            .iter()
            .map(|s| SensorConfig {
                id: s.id,
                modality: "radar".to_owned(),
                // Not read on the frame path (`benches/app_tick.rs` established this);
                // the fixture's own origin stands in for it, the same way that bench's
                // fixed value does for a generated scenario.
                position: [origin.lat_rad, origin.lon_rad, origin.alt_m],
                max_range_m: 200_000.0,
                control_endpoint: None,
                maintenance: Vec::new(),
            })
            .collect(),
        resources,
        origin: Some([origin.lat_rad, origin.lon_rad, origin.alt_m]),
        data_dir: dir.to_string_lossy().into_owned(),
        ..ConfigBaseline::default()
    }
}

const FRAME_S: f64 = 1.0 / 30.0;

/// A counter, not just the process id: two rehearsals of the same laydown against the
/// same scenario -- exactly what comparing two candidate resource placements does --
/// would otherwise share one scratch path, and concurrent runs (this crate's own test
/// suite included) raced deleting and recreating it out from under each other.
static NEXT_RUN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Run `scenario` through the live pipeline against `laydown`'s resource placement, in
/// a fresh throwaway desktop -- the live one, its journal and its picture, are never
/// touched.
///
/// `testdata_root` is the workspace's `testdata/` directory; `base_resources` is the
/// deployment's own declared resources, whose non-position fields (capacity, layer,
/// cost, closing speed) a bare `ResourcePlacement` does not carry and this run still
/// needs.
///
/// # Errors
///
/// If the named fixture's files cannot be read or do not parse, or the throwaway
/// desktop they would drive cannot start.
pub fn run(
    testdata_root: &Path,
    scenario: TestTrackNumber,
    laydown: &Laydown,
    base_resources: &[ResourceConfig],
) -> Result<RehearsalRecord, RehearsalError> {
    let fixture = fixture_dir(scenario, testdata_root);
    let metadata: Metadata = read_json(&fixture.join("metadata.json"), "fixture metadata")?;
    let sensors: SensorsFile = read_json(&fixture.join("sensors.json"), "fixture sensor list")?;
    let origin = Geodetic {
        lat_rad: metadata.origin.lat.to_radians(),
        lon_rad: metadata.origin.lon.to_radians(),
        alt_m: metadata.origin.alt_m,
    };

    let run_id = NEXT_RUN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "gungnir-rehearsal-{}-{}-{}-{run_id}",
        scenario.label(),
        laydown.id.0,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|source| RehearsalError::Read {
        path: dir.clone(),
        source,
    })?;

    let config = config_for(&sensors.sensors, origin, laydown, base_resources, &dir);
    let mut state = AppState::with_config(config)?;
    let adapter = RecordedFeedAdapter::open(&fixture.join("detections.jsonl"))?;
    state.ingest.add_adapter(Box::new(adapter));

    let mut clock = ReplayClockAuthority {
        current: MissionTime(0.0),
    };
    state.clock = Box::new(clock);
    // Every fixture's duration is a small positive number of seconds (TT-01's is 480);
    // truncation and sign loss are not live concerns for this cast.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let frames = (metadata.duration_s / FRAME_S).ceil() as usize;
    for _ in 0..frames {
        clock.advance(FRAME_S);
        state.clock = Box::new(clock);
        update::tick(&mut state);
    }

    let records = state.approvals.records();
    let decisions_expired = records.iter().filter(|r| r.is_expiry()).count();
    let decisions_raised = records.len() + state.approvals.queue().len();
    let record = RehearsalRecord {
        scenario,
        laydown: laydown.id.clone(),
        tracks_formed: state.tracking.tracks().len(),
        decisions_raised,
        decisions_expired,
    };
    // Dropped before cleanup, deliberately: the journal's file handle is still open on
    // `state`, and `remove_dir_all` racing an open handle fails silently on Windows
    // (caught by two runs sharing a scratch path leaving one fewer track than a fresh
    // directory would -- the earlier run's stale journal bled into the later one).
    drop(state);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::{LaydownId, ResourceId};

    fn base_resources() -> Vec<ResourceConfig> {
        let text = serde_json::json!([
            {"id": 1, "position": [0.0, 0.0, 0.0], "capacity": 2, "layer": "point"}
        ])
        .to_string();
        serde_json::from_str(&text).expect("the fixture resource config parses")
    }

    fn laydown_placing(resource_position_enu: [f64; 3]) -> Laydown {
        Laydown {
            id: LaydownId("candidate".into()),
            intent: "test fixture".into(),
            sensors: Vec::new(),
            resources: vec![gungnir_model::laydown::ResourcePlacement {
                resource: ResourceId(1),
                position_enu: resource_position_enu,
            }],
            current: true,
        }
    }

    /// East, north and up each move a different geodetic coordinate (near the equator
    /// and prime meridian, where this is easiest to see without the other two axes'
    /// projection folding in): east moves longitude, north moves latitude, up moves
    /// altitude and nothing else. Exercises the actual conversion this module runs,
    /// not a restatement of `gungnir-coord`'s own already-tested round trip.
    #[test]
    fn geodetic_of_moves_the_axis_it_should_and_none_of_the_others() {
        let origin = Geodetic {
            lat_rad: 0.0,
            lon_rad: 0.0,
            alt_m: 0.0,
        };
        let base = geodetic_of([0.0, 0.0, 0.0], origin);
        let east = geodetic_of([1000.0, 0.0, 0.0], origin);
        let north = geodetic_of([0.0, 1000.0, 0.0], origin);
        let up = geodetic_of([0.0, 0.0, 1000.0], origin);

        assert!(
            (east[1] - base[1]).abs() > 1e-6 && (east[0] - base[0]).abs() < 1e-9,
            "east should move longitude and not latitude: {east:?} vs {base:?}"
        );
        assert!(
            (north[0] - base[0]).abs() > 1e-6 && (north[1] - base[1]).abs() < 1e-9,
            "north should move latitude and not longitude: {north:?} vs {base:?}"
        );
        assert!(
            (up[2] - base[2] - 1000.0).abs() < 1e-6
                && (up[0] - base[0]).abs() < 1e-9
                && (up[1] - base[1]).abs() < 1e-9,
            "up should move altitude by exactly its own offset and nothing else: \
             {up:?} vs {base:?}"
        );
    }

    /// The property GAP-045 actually needs from a laydown: its resource placement is a
    /// real input `config_for` reads, not inert plumbing that a rehearsal ignores.
    /// Checked directly on the configuration a run would build, rather than on the
    /// tracking pipeline's own output -- track formation is independent of resource
    /// geometry by construction (resources feed intercept planning, which reads
    /// tracks, never the other way around) and is, separately, not exactly
    /// reproducible under heavy concurrent system load, which is a real property of
    /// `gungnir-fusion-async`'s pipeline under scheduling variance and not something
    /// this harness's own tests should depend on or paper over.
    #[test]
    fn a_laydowns_resource_placement_reaches_the_built_configuration() {
        let origin = Geodetic {
            lat_rad: 0.1,
            lon_rad: 0.2,
            alt_m: 50.0,
        };
        let close = config_for(
            &[],
            origin,
            &laydown_placing([500.0, 0.0, 0.0]),
            &base_resources(),
            Path::new("."),
        );
        let far = config_for(
            &[],
            origin,
            &laydown_placing([50_000.0, 50_000.0, 0.0]),
            &base_resources(),
            Path::new("."),
        );

        // Exact comparison is deliberate, not a float-tolerance oversight: both sides
        // are `geodetic_of`'s output for two different inputs (or, below, a value the
        // function never touched), not two independent computations that could drift.
        #[allow(clippy::float_cmp)]
        {
            assert_ne!(
                close.resources[0].position, far.resources[0].position,
                "two different placements produced the same configured position"
            );
        }
        // And a laydown that never mentions a resource leaves it where the deployment's
        // own configuration already had it, rather than dropping or zeroing it.
        let unmentioned = config_for(
            &[],
            origin,
            &Laydown {
                resources: Vec::new(),
                ..laydown_placing([0.0, 0.0, 0.0])
            },
            &base_resources(),
            Path::new("."),
        );
        #[allow(clippy::float_cmp)]
        {
            assert_eq!(
                unmentioned.resources[0].position,
                base_resources()[0].position
            );
        }
    }
}
