// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Rehearsing a candidate laydown against a committed test-track recording (GAP-045,
//! GAP-105, PN-16's rehearsal section): what **this laydown's own sensors, where it puts
//! them,** would have detected of the targets the recording holds, run through the live
//! pipeline on a throwaway desktop.
//!
//! # Re-observed from a recording, never generated
//!
//! The detections a rehearsal feeds are produced here, at rehearsal time, by
//! re-observing the recording's truth (docs/design/DN-32-re-observation-for-a-laydown.md,
//! D-64): for each sensor the laydown places, in the mode it places it, at the position
//! it gives it, with **the deployment's own detection model** for that sensor
//! (`SensorConfig::detection_model`, §5.4), `gungnir_sensor_sim::reobserve` answers scan
//! by scan whether it would have seen each target where the recording says the target
//! was. Nothing here makes a target: `gungnir-sensor-sim` holds the observation model and
//! not the world model, and `gungnir-scenario` -- which makes worlds -- is still never a
//! dependency of this crate (`gungnir-app/tests/dependency_graph.rs`'s
//! `scenario_misuse`). The recording read is one of the ten committed
//! `testdata/tracks/samples/TT-0N-sample/` sets: its truth, its two re-observation
//! sidecars (`entities.json`, `environment.json`) and its metadata, with the sensor
//! catalogue's export beside them (`docs/test-tracks/data-format.md` §10 and §11).
//!
//! **This is the one module in `gungnir-app` that may name `gungnir_sensor_sim`**
//! (DN-32 §6 mechanism 5, `tests/architecture_compliance.rs`). What it hands the rest of
//! the desktop is a [`RehearsalRecord`] of counts and names, never an observation.
//!
//! # Contained (DN-32 §6)
//!
//! Every re-observed detection carries `Provenance::rehearsal`, set from the mark the
//! simulation put on it; the throwaway desktop's gateway is the one construction that
//! admits such a detection (`IngestGateway::for_rehearsal`), and every live gateway
//! refuses and counts one. The run is on its own `AppState` with its own journal in a
//! scratch directory, so nothing it produces reaches the live desktop's picture or
//! record.
//!
//! # What a rehearsal refuses, and what it does not claim
//!
//! - **A sensor with no detection model, or one the catalogue does not hold, is refused
//!   by name and nothing runs** (DN-32 §5.4). A recording sensor that shares its
//!   identifier is never borrowed: the two are different sensors whose identifiers
//!   coincide by accident.
//! - **A recording's sensor-specific events are not applied** to the deployment's sensors
//!   -- its losses and jamming windows name the recording's own sensors -- and the record
//!   counts them (D-73). Its sea state is applied: it names no sensor.
//! - **Same laydown, same answer; different laydowns, differences only where they
//!   differ** (D-74). Every draw is keyed by the recording's seed and the sensor and
//!   target it concerns, never by the laydown, so a sensor two laydowns place identically
//!   produces identical detections under both.
//! - **Not the recording's own detections**, even for the current laydown with the
//!   recording's own sensors (DN-32 §7): equal statistics, not equal lines.
//!
//! # Frame (DN-32 §5.5)
//!
//! A laydown's `position_enu` is read about the recording's own fictional origin, not the
//! deployment's real one: a laydown is rehearsed as an arrangement relative to the
//! recording's geography. The throwaway configuration's sensors and resources are placed
//! by converting those offsets to geodetic about that same origin with
//! `gungnir-coord::Wgs84`, which is exact because everything downstream reads them back
//! into the same frame.
//!
//! # What a `RehearsalRecord` reports, and why nothing here is a distance
//!
//! Tracks the pipeline formed, and how the approval queue it drove behaved, as a real
//! tick loop measured them; and, per sensor the laydown places, what it re-observed. No
//! first-engagement range: DN-02 §7 says that is an aggregate of predictions over a
//! rehearsal, and GAP-020 carries its rule.
//!
//! # A measurement of the recording, not of the machine
//!
//! The fusion pipeline runs on its own task, and a replay's frames cost so little wall
//! clock that the task is routinely still behind when the last frame ends. A run
//! therefore ends its detection stream and waits for the pipeline's own end-of-stream
//! flush before it reads anything off; if it never arrives the run reports nothing
//! (`RehearsalError::DidNotSettle`) rather than a number an operator would read as a
//! property of the laydown.

use gungnir_command::ApprovalWorkflow;
use gungnir_config::{ConfigBaseline, ResourceConfig, SensorConfig};
use gungnir_coord::{CoordTransform, Enu, Geodetic, Wgs84};
use gungnir_ingest::adapters::recorded::RecordedFeedAdapter;
use gungnir_ingest::{AllowListAuthenticator, IngestGateway};
use gungnir_model::{
    AzimuthSector, DetectionView, Laydown, LaydownId, LocalFrame, MissionTime, Provenance,
    RehearsalOrigin, SensorId, SensorMode, TestTrackNumber,
};
use gungnir_sensor_sim as sim;
use gungnir_time::ReplayClockAuthority;
use std::path::{Path, PathBuf};

use crate::state::AppState;
use crate::update;

fn fixture_dir(scenario: TestTrackNumber, testdata_root: &Path) -> PathBuf {
    testdata_root
        .join("tracks/samples")
        .join(format!("{}-sample", scenario.label()))
}

/// The sensor catalogue's JSON export, beside the sample sets
/// (`docs/test-tracks/data-format.md` §11).
fn catalogue_path(testdata_root: &Path) -> PathBuf {
    testdata_root.join("tracks/sensor-models.json")
}

/// The per-axis variance every detection in a recorded set states, which is the
/// tracking baseline's own (`docs/test-tracks/data-format.md` §3). A re-observed
/// detection states the same, so the pipeline weighs it exactly as it weighs the
/// recording's.
const BASELINE_VARIANCE_M2: [f64; 3] = [400.0, 400.0, 900.0];

/// What a re-observed detection names as the algorithm that produced it.
const ALGORITHM_VERSION: &str = "gungnir-sensor-sim 0.1.0 (re-observed)";

/// The pieces of `metadata.json` a rehearsal reads.
#[derive(serde::Deserialize)]
struct Metadata {
    scenario: String,
    seed: u64,
    duration_s: f64,
    truth_tick_s: f64,
    origin: MetadataOrigin,
}

#[derive(serde::Deserialize)]
struct MetadataOrigin {
    /// Degrees, per `data-format.md`'s "angles degrees" -- not radians.
    lat: f64,
    lon: f64,
    alt_m: f64,
}

/// What one sensor the laydown places did in a rehearsal.
#[derive(Debug, Clone, PartialEq)]
pub struct SensorRehearsal {
    pub sensor: SensorId,
    /// The mode the laydown places it in.
    pub mode: SensorMode,
    /// Where the laydown places it, ENU metres about the recording's origin (DN-32 §5.5).
    pub position_enu: [f64; 3],
    /// The way it was pointed: the laydown's re-aim, else the sensor's declared sector,
    /// against true north where it stands (GAP-118); `None` is the full circle. A target
    /// outside it was not seen, on top of the detection model's own field of regard.
    pub azimuth_sector: Option<AzimuthSector>,
    /// The detection model it was re-observed with, or `None` when its mode does not
    /// observe (standby, calibrating, offline) and it was not run at all.
    pub detection_model: Option<String>,
    /// Detections of a recorded target.
    pub detections: usize,
    pub false_alarms: usize,
}

impl SensorRehearsal {
    /// Whether the sensor was re-observing: placed in a mode that observes.
    #[must_use]
    pub fn observed(&self) -> bool {
        self.detection_model.is_some()
    }
}

/// What a rehearsal measured. Every field is read off a real run; none is estimated or
/// interpolated to fill in a number the run did not produce.
#[derive(Debug, Clone, PartialEq)]
pub struct RehearsalRecord {
    pub scenario: TestTrackNumber,
    pub laydown: LaydownId,
    /// The recording's seed, from which every random stream of the run derived.
    pub seed: u64,
    /// Tracks the pipeline had formed by the end of the run.
    pub tracks_formed: usize,
    /// Every plan that entered the approval queue during the run, decided or still
    /// pending when the run ended.
    pub decisions_raised: usize,
    /// The subset of those that expired rather than being decided (DN-10).
    pub decisions_expired: usize,
    /// One entry per sensor the laydown places, in the laydown's order.
    pub sensors: Vec<SensorRehearsal>,
    /// The recording's sensor-specific events -- losses and electronic-attack windows,
    /// which name the recording's own sensors -- that were not applied (D-73).
    pub recording_events_not_applied: usize,
}

/// One sensor whose re-observed detections differ between two rehearsals of the same
/// recording (DN-32 §10's round-1 row: "the record says which sensor the difference came
/// from").
#[derive(Debug, Clone, PartialEq)]
pub struct SensorDifference {
    pub sensor: SensorId,
    /// Detections of a recorded target under the first record, then the second.
    pub detections: (usize, usize),
    /// Whether the two laydowns place the sensor in different places, modes or sectors.
    pub moved: bool,
}

/// The sensors whose re-observed detections differ between two rehearsals of **the same
/// recording**, or `None` when the two re-observed different recordings, which are not
/// comparable. A sensor placed by one laydown and not the other cannot occur: a
/// validated baseline's laydowns place the same sensors (DN-26 §4 rule 4).
///
/// Because every draw is keyed by sensor and target and not by laydown (D-74), a sensor
/// both laydowns place identically produces identical detections under both, so every
/// entry here comes from geometry -- the sensor's own, or, for a cued sensor, a cueing
/// sensor's -- and `moved` says which.
#[must_use]
// `moved` compares two laydowns' declared positions exactly, as declared: a sensor either
// stands where the other laydown puts it or it does not, and no computation stands
// between the two values.
#[allow(clippy::float_cmp)]
pub fn sensors_that_differ(
    a: &RehearsalRecord,
    b: &RehearsalRecord,
) -> Option<Vec<SensorDifference>> {
    if a.scenario != b.scenario || a.seed != b.seed {
        return None;
    }
    Some(
        a.sensors
            .iter()
            .filter_map(|x| {
                let y = b.sensors.iter().find(|y| y.sensor == x.sensor)?;
                (x.detections != y.detections).then(|| SensorDifference {
                    sensor: x.sensor,
                    detections: (x.detections, y.detections),
                    moved: x.position_enu != y.position_enu
                        || x.mode != y.mode
                        || x.azimuth_sector != y.azimuth_sector,
                })
            })
            .collect(),
    )
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
    #[error(
        "laydown {laydown} places sensor {sensor}, which names no detection model; a \
         rehearsal re-observes with the deployment's own model for each sensor and does \
         not borrow a recording's sensor that shares its identifier (DN-32 §5.4). Give \
         sensor {sensor} a `detection_model` from the sensor catalogue"
    )]
    NoDetectionModel { laydown: LaydownId, sensor: u32 },
    #[error(
        "sensor {sensor} names the detection model {model:?}, which the sensor catalogue \
         ({catalogue}) does not hold"
    )]
    UnknownDetectionModel {
        sensor: u32,
        model: String,
        catalogue: PathBuf,
    },
    #[error("laydown {laydown} places sensor {sensor}, which this baseline does not declare")]
    UndeclaredSensor { laydown: LaydownId, sensor: u32 },
    #[error(
        "laydown {laydown} places no sensor in a mode that observes (search or track), so \
         there is nothing to re-observe {scenario} with"
    )]
    NothingObserves {
        laydown: LaydownId,
        scenario: String,
    },
    #[error("the recording could not be re-observed: {0}")]
    ReObservation(#[from] sim::ReObservationError),
    #[error(
        "the pipeline never reported the end of the run: it had taken {taken} of the \
         {submitted} detection(s) fed to it, and what it formed is not this scenario's \
         answer"
    )]
    DidNotSettle { submitted: u64, taken: u64 },
    #[error("a rehearsal's own throwaway desktop would not start: {0}")]
    State(#[from] crate::state::AppError),
}

fn read_text(path: &Path) -> Result<String, RehearsalError> {
    std::fs::read_to_string(path).map_err(|source| RehearsalError::Read {
        path: path.to_path_buf(),
        source,
    })
}

fn read_json<T: serde::de::DeserializeOwned>(
    path: &Path,
    what: &'static str,
) -> Result<T, RehearsalError> {
    serde_json::from_str(&read_text(path)?).map_err(|source| RehearsalError::Parse {
        path: path.to_path_buf(),
        what,
        source,
    })
}

fn read_lines<T: serde::de::DeserializeOwned>(
    path: &Path,
    what: &'static str,
) -> Result<Vec<T>, RehearsalError> {
    read_text(path)?
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            serde_json::from_str(l).map_err(|source| RehearsalError::Parse {
                path: path.to_path_buf(),
                what,
                source,
            })
        })
        .collect()
}

/// A laydown's ENU offset, re-expressed as the geodetic position a `ResourceConfig` or
/// `SensorConfig` takes, about `origin`. Exact for the same origin used both here and
/// wherever the position is next read back into a local frame -- see the module doc for
/// why this is a self-consistent detour rather than a claim about where the equipment
/// sits on the real Earth.
fn geodetic_of(position_enu: [f64; 3], origin: Geodetic) -> [f64; 3] {
    let enu = Enu {
        e_m: position_enu[0],
        n_m: position_enu[1],
        u_m: position_enu[2],
    };
    let g = Wgs84::ecef_to_geodetic(Wgs84::enu_to_ecef(enu, origin));
    [g.lat_rad, g.lon_rad, g.alt_m]
}

/// A mode in which a placed sensor observes (DN-26 §5: standby sensors contribute
/// nothing to coverage, and nothing to a rehearsal either).
fn observes(mode: SensorMode) -> bool {
    matches!(mode, SensorMode::Search | SensorMode::Track)
}

/// A sensor the laydown places, the deployment's declaration of it, and -- when its mode
/// observes -- the catalogue model it names.
type Resolved<'a> = (&'a SensorConfig, Option<sim::SensorParams>);

/// Every sensor `laydown` places, resolved. Refuses by name before anything runs.
fn resolve_sensors<'a>(
    laydown: &Laydown,
    base_sensors: &'a [SensorConfig],
    catalogue: &sim::SensorCatalogue,
    catalogue_path: &Path,
) -> Result<Vec<Resolved<'a>>, RehearsalError> {
    laydown
        .sensors
        .iter()
        .map(|p| {
            let declared = base_sensors
                .iter()
                .find(|s| s.id == p.sensor.0)
                .ok_or_else(|| RehearsalError::UndeclaredSensor {
                    laydown: laydown.id.clone(),
                    sensor: p.sensor.0,
                })?;
            if !observes(p.mode) {
                return Ok((declared, None));
            }
            let name = declared.detection_model.as_deref().ok_or_else(|| {
                RehearsalError::NoDetectionModel {
                    laydown: laydown.id.clone(),
                    sensor: p.sensor.0,
                }
            })?;
            let model =
                catalogue
                    .model(name)
                    .ok_or_else(|| RehearsalError::UnknownDetectionModel {
                        sensor: p.sensor.0,
                        model: name.to_owned(),
                        catalogue: catalogue_path.to_path_buf(),
                    })?;
            Ok((declared, Some(model.clone())))
        })
        .collect()
}

/// A throwaway configuration: the laydown's sensors and resources, placed about the
/// recording's own origin (DN-32 §5.5), every other field the deployment's defaults.
fn config_for(
    placed: &[(&SensorConfig, [f64; 3], Option<AzimuthSector>)],
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
        sensors: placed
            .iter()
            .map(|(s, enu, sector)| SensorConfig {
                position: geodetic_of(*enu, origin),
                // A rehearsal commands nothing, and a throwaway desktop has no endpoint
                // to send a command to.
                control_endpoint: None,
                maintenance: Vec::new(),
                azimuth_sector: *sector,
                ..(*s).clone()
            })
            .collect(),
        resources,
        origin: Some([origin.lat_rad, origin.lon_rad, origin.alt_m]),
        data_dir: dir.to_string_lossy().into_owned(),
        ..ConfigBaseline::default()
    }
}

/// The model's rehearsal marker for a simulation mark: the conversion DN-32 §6
/// mechanism 1 requires be total. The mark names the recording by its label and the
/// placement by the laydown's identifier; this module set both, so a mark that says
/// anything else is a defect and is reported as `None`.
fn rehearsal_origin(
    mark: &sim::SimulationMark,
    scenario: TestTrackNumber,
    laydown: &LaydownId,
) -> Option<RehearsalOrigin> {
    (mark.scenario == scenario.label() && mark.placement == laydown.0).then(|| RehearsalOrigin {
        scenario,
        laydown: laydown.clone(),
        seed: mark.seed,
    })
}

/// A re-observed observation as the detection the gateway reads, marked.
fn detection_of(
    o: &sim::Observation,
    scenario: TestTrackNumber,
    laydown: &LaydownId,
) -> Option<DetectionView> {
    let sensor = SensorId(u32::try_from(o.sensor()).ok()?);
    let m = o.measurement();
    Some(DetectionView {
        sensor,
        source_time: MissionTime(o.source_time().f()),
        receipt_time: MissionTime(o.receipt_time().f()),
        measurement: gungnir_model::Measurement::Position {
            enu: nalgebra::Vector3::new(m[0].f(), m[1].f(), m[2].f()),
            variance_m2: BASELINE_VARIANCE_M2,
        },
        provenance: Provenance {
            source_sensor_ids: vec![sensor.0],
            algorithm_version: ALGORITHM_VERSION.to_owned(),
            rehearsal: Some(rehearsal_origin(o.mark(), scenario, laydown)?),
            ..Provenance::default()
        },
    })
}

const FRAME_S: f64 = 1.0 / 30.0;

/// How many times a run polls for the pipeline's end-of-stream flush before it gives
/// up, and how long it pauses between polls.
///
/// **A deadlock guard, not a performance assertion** -- the reasoning
/// `gungnir-tracking-service/tests/sample_set_replay.rs` gives for its own bound. The
/// loop leaves the moment the flush arrives, so a healthy run pays a poll or two; a
/// pipeline that has genuinely stopped ends the run with
/// [`RehearsalError::DidNotSettle`] after about ten seconds, against a replay that
/// already costs seconds. Ten seconds is not an opinion about how fast the flush should
/// be: it is long enough that a machine cannot fail this by being slow.
const SETTLE_POLLS: usize = 5_000;
const SETTLE_PAUSE: std::time::Duration = std::time::Duration::from_millis(2);

/// A counter, not just the process id: two rehearsals of the same laydown against the
/// same scenario -- exactly what comparing two candidates does -- would otherwise share
/// one scratch path, and concurrent runs raced deleting and recreating it out from under
/// each other.
static NEXT_RUN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Rehearse `laydown` against the recording `scenario`: re-observe its truth with the
/// sensors the laydown places, where it places them, with the deployment's own detection
/// models, and run what they would have detected through the live pipeline on a fresh
/// throwaway desktop. The live one, its journal and its picture, are never touched.
///
/// `testdata_root` is the workspace's `testdata/` directory; `base_sensors` and
/// `base_resources` are the deployment's own declarations, whose detection models and
/// non-position fields (capacity, layer, cost, closing speed) a bare placement does not
/// carry.
///
/// # Errors
///
/// Refused by name, before anything runs, if the laydown places a sensor with no
/// detection model or one the catalogue does not hold, places a sensor the baseline does
/// not declare, or places none that observes. Otherwise, if the recording's files cannot
/// be read or do not parse or re-observe, the throwaway desktop cannot start, or the
/// pipeline never caught up with the detections it was fed
/// ([`RehearsalError::DidNotSettle`]) -- in which case there is no honest number to
/// report and the run says so rather than reporting a low one.
#[allow(clippy::too_many_lines)]
pub fn run(
    testdata_root: &Path,
    scenario: TestTrackNumber,
    laydown: &Laydown,
    base_sensors: &[SensorConfig],
    base_resources: &[ResourceConfig],
) -> Result<RehearsalRecord, RehearsalError> {
    let fixture = fixture_dir(scenario, testdata_root);
    let metadata: Metadata = read_json(&fixture.join("metadata.json"), "recording metadata")?;
    let catalogue_file = catalogue_path(testdata_root);
    let catalogue: sim::SensorCatalogue =
        read_json(&catalogue_file, "the sensor catalogue export")?;

    // Every refusal before any work: a laydown that cannot be rehearsed runs nothing.
    let resolved = resolve_sensors(laydown, base_sensors, &catalogue, &catalogue_file)?;
    let origin = Geodetic {
        lat_rad: metadata.origin.lat.to_radians(),
        lon_rad: metadata.origin.lon.to_radians(),
        alt_m: metadata.origin.alt_m,
    };
    let frame = LocalFrame::new(origin);
    // Each placed sensor's sector: the laydown's re-aim if it states one, else the
    // sensor's declared sector (GAP-118, D-84), surveyed against true north where it
    // stands and turned into the recording's frame there.
    let sectors: Vec<Option<AzimuthSector>> = laydown
        .sensors
        .iter()
        .zip(&resolved)
        .map(|(p, (declared, _))| p.azimuth_sector.or(declared.azimuth_sector))
        .collect();
    let placed: Vec<sim::PlacedSensor> = laydown
        .sensors
        .iter()
        .zip(&resolved)
        .zip(&sectors)
        .filter_map(|((p, (_, model)), sector)| {
            model.as_ref().map(|m| sim::PlacedSensor {
                id: i64::from(p.sensor.0),
                model: m.clone(),
                position: p.position_enu,
                sector: sector.map(|s| {
                    let in_frame = frame.sector_in_frame(s, frame.to_geodetic(p.position_enu));
                    sim::Sector {
                        boresight_rad: in_frame.boresight_rad,
                        width_rad: in_frame.width_rad,
                    }
                }),
            })
        })
        .collect();
    if placed.is_empty() {
        return Err(RehearsalError::NothingObserves {
            laydown: laydown.id.clone(),
            scenario: scenario.label(),
        });
    }

    let entities: sim::EntitiesFile =
        read_json(&fixture.join("entities.json"), "recording entities")?;
    let environment: sim::EnvironmentFile =
        read_json(&fixture.join("environment.json"), "recording environment")?;
    let truth: Vec<sim::TruthRecord> = read_lines(&fixture.join("truth.jsonl"), "truth")?;
    let recording = sim::Recording {
        scenario: metadata.scenario,
        seed: metadata.seed,
        duration_s: metadata.duration_s,
        tick_s: metadata.truth_tick_s,
        truth,
        entities: entities.entities,
        environment: environment.events,
    };
    let reobserved = sim::reobserve(
        &recording,
        &placed,
        sim::SensorEvents::NotApplied,
        &laydown.id.0,
    )?;
    let views: Vec<DetectionView> = reobserved
        .observations
        .iter()
        .filter_map(|o| detection_of(o, scenario, &laydown.id))
        .collect();

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

    let config_sensors: Vec<(&SensorConfig, [f64; 3], Option<AzimuthSector>)> = laydown
        .sensors
        .iter()
        .zip(&resolved)
        .zip(&sectors)
        .map(|((p, (declared, _)), sector)| (*declared, p.position_enu, *sector))
        .collect();
    let config = config_for(&config_sensors, origin, laydown, base_resources, &dir);
    let mut state = AppState::with_config(config)?;
    // DN-32 §6 mechanism 2: the throwaway desktop's gateway is the one construction that
    // admits a re-observed detection. It still authenticates each against the sensors
    // this laydown places and validates it as a live gateway would.
    let mut gateway = IngestGateway::for_rehearsal(Box::new(AllowListAuthenticator {
        allowed: laydown.sensor_ids(),
    }));
    gateway.set_expected_adapters(1);
    gateway.add_adapter(Box::new(RecordedFeedAdapter::from_views(
        format!("rehearsal:{}/{}", scenario.label(), laydown.id),
        views,
    )));
    state.ingest = gateway;

    let mut clock = ReplayClockAuthority {
        current: MissionTime(0.0),
    };
    state.clock = Box::new(clock);
    // Every recording's duration is a small positive number of seconds (TT-01's is 480);
    // truncation and sign loss are not live concerns for this cast.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let frames = (recording.duration_s / FRAME_S).ceil() as usize;
    for _ in 0..frames {
        clock.advance(FRAME_S);
        state.clock = Box::new(clock);
        update::tick(&mut state);
    }

    // End the detection stream and wait for the pipeline's own end-of-stream flush,
    // which is both the point at which the last reorder horizon is processed and the one
    // unambiguous signal that everything before it has been (GAP-045).
    state.tracking.finish();
    let mut flushed = false;
    for _ in 0..SETTLE_POLLS {
        state.tracking.poll(clock.current);
        if !state.tracking.is_healthy() {
            flushed = true;
            break;
        }
        std::thread::sleep(SETTLE_PAUSE);
    }
    if !flushed {
        let counters = state.tracking.pipeline_stats();
        let taken =
            counters.accepted + counters.too_late + counters.not_finite + counters.bearings_offered;
        let submitted = state.ingest.stats().accepted;
        drop(state);
        let _ = std::fs::remove_dir_all(&dir);
        return Err(RehearsalError::DidNotSettle { submitted, taken });
    }

    let records = state.desk.approvals.records();
    let decisions_expired = records.iter().filter(|r| r.is_expiry()).count();
    let decisions_raised = records.len() + state.desk.approvals.queue().len();
    let sensors = laydown
        .sensors
        .iter()
        .zip(&resolved)
        .zip(&sectors)
        .map(|((p, (declared, model)), sector)| {
            let tally = reobserved
                .per_sensor
                .iter()
                .find(|t| t.sensor == i64::from(p.sensor.0));
            SensorRehearsal {
                sensor: p.sensor,
                mode: p.mode,
                position_enu: p.position_enu,
                azimuth_sector: *sector,
                detection_model: model.as_ref().and(declared.detection_model.clone()),
                detections: tally.map_or(0, |t| t.detections),
                false_alarms: tally.map_or(0, |t| t.false_alarms),
            }
        })
        .collect();
    let record = RehearsalRecord {
        scenario,
        laydown: laydown.id.clone(),
        seed: recording.seed,
        tracks_formed: state.tracking.tracks().len(),
        decisions_raised,
        decisions_expired,
        sensors,
        recording_events_not_applied: reobserved.sensor_events_not_applied,
    };
    // Dropped before cleanup, deliberately: the journal's file handle is still open on
    // `state`, and `remove_dir_all` racing an open handle fails silently on Windows.
    drop(state);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::laydown::{ResourcePlacement, SensorPlacement};
    use gungnir_model::ResourceId;

    fn base_resources() -> Vec<ResourceConfig> {
        let text = serde_json::json!([
            {"id": 1, "position": [0.0, 0.0, 0.0], "capacity": 2, "layer": "point"}
        ])
        .to_string();
        serde_json::from_str(&text).expect("the fixture resource config parses")
    }

    fn radar(id: u32, model: Option<&str>) -> SensorConfig {
        let mut text = serde_json::json!({
            "id": id, "modality": "radar", "position": [0.0, 0.0, 0.0], "max_range_m": 15000.0
        });
        if let Some(m) = model {
            text["detection_model"] = m.into();
        }
        serde_json::from_value(text).expect("the fixture sensor config parses")
    }

    fn laydown_placing(resource_position_enu: [f64; 3]) -> Laydown {
        Laydown {
            id: LaydownId("candidate".into()),
            intent: "test fixture".into(),
            sensors: vec![SensorPlacement {
                sensor: SensorId(1),
                position_enu: [0.0, 0.0, 25.0],
                mode: SensorMode::Search,
                azimuth_sector: None,
            }],
            resources: vec![ResourcePlacement {
                resource: ResourceId(1),
                position_enu: resource_position_enu,
            }],
            current: true,
        }
    }

    fn catalogue() -> sim::SensorCatalogue {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../testdata/tracks/sensor-models.json");
        read_json(&path, "catalogue").expect("the committed catalogue export parses")
    }

    /// East, north and up each move a different geodetic coordinate (near the equator
    /// and prime meridian, where this is easiest to see without the other two axes'
    /// projection folding in): east moves longitude, north moves latitude, up moves
    /// altitude and nothing else.
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

    /// A laydown's resource *and sensor* placements are real inputs to the configuration
    /// a run builds, not inert plumbing (GAP-045, GAP-105).
    #[test]
    fn a_laydowns_placements_reach_the_built_configuration() {
        let origin = Geodetic {
            lat_rad: 0.1,
            lon_rad: 0.2,
            alt_m: 50.0,
        };
        let sensor = radar(1, Some("radar.short"));
        let close = config_for(
            &[(&sensor, [100.0, 0.0, 25.0], None)],
            origin,
            &laydown_placing([500.0, 0.0, 0.0]),
            &base_resources(),
            Path::new("."),
        );
        let far = config_for(
            &[(&sensor, [9_000.0, 0.0, 25.0], None)],
            origin,
            &laydown_placing([50_000.0, 50_000.0, 0.0]),
            &base_resources(),
            Path::new("."),
        );
        #[allow(clippy::float_cmp)]
        {
            assert_ne!(close.resources[0].position, far.resources[0].position);
            assert_ne!(close.sensors[0].position, far.sensors[0].position);
        }
        assert_eq!(
            close.sensors[0].detection_model.as_deref(),
            Some("radar.short")
        );
        // A laydown that never mentions a resource leaves it where the deployment had it.
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

    /// DN-32 §5.4: a sensor with no model, or one the catalogue does not hold, is refused
    /// by name, and a sensor in a mode that does not observe needs none.
    #[test]
    fn a_sensor_is_resolved_to_its_own_named_model_or_refused_by_name() {
        let catalogue = catalogue();
        let path = Path::new("sensor-models.json");
        let laydown = laydown_placing([0.0, 0.0, 0.0]);

        let named = [radar(1, Some("radar.short"))];
        let resolved = resolve_sensors(&laydown, &named, &catalogue, path).expect("resolves");
        assert_eq!(resolved[0].1.as_ref(), catalogue.model("radar.short"));

        let unnamed = [radar(1, None)];
        let err = resolve_sensors(&laydown, &unnamed, &catalogue, path)
            .expect_err("no model is a refusal");
        assert!(
            matches!(err, RehearsalError::NoDetectionModel { sensor: 1, .. }),
            "{err}"
        );
        assert!(err.to_string().contains("sensor 1"), "{err}");

        let unknown = [radar(1, Some("radar.imaginary"))];
        let err = resolve_sensors(&laydown, &unknown, &catalogue, path)
            .expect_err("an unknown model is a refusal");
        assert!(err.to_string().contains("radar.imaginary"), "{err}");

        let mut standby = laydown.clone();
        standby.sensors[0].mode = SensorMode::Standby;
        let resolved =
            resolve_sensors(&standby, &unnamed, &catalogue, path).expect("standby needs none");
        assert!(resolved[0].1.is_none());
    }

    #[test]
    fn a_mark_this_module_did_not_set_converts_to_nothing() {
        let laydown = LaydownId("c".into());
        let mark = sim::SimulationMark {
            scenario: "TT-01".into(),
            placement: "c".into(),
            seed: 1701,
        };
        assert_eq!(
            rehearsal_origin(&mark, TestTrackNumber(1), &laydown),
            Some(RehearsalOrigin {
                scenario: TestTrackNumber(1),
                laydown: laydown.clone(),
                seed: 1701,
            })
        );
        assert_eq!(rehearsal_origin(&mark, TestTrackNumber(2), &laydown), None);
    }
}
