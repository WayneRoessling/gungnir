// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! From a test-track set to the ML-04 anomaly rows (`docs/ml/data-pipeline.md` §2;
//! `docs/ml/use-cases.md` ML-04; GAP-079).
//!
//! One row per source per window and one per track per window, in the catalogue's
//! `gungnir.ml.anomaly-rows` form. A window is [`WINDOW_TICKS`] truth ticks, the same
//! span the ML-01 rows' kinematic window covers, and windows do not overlap.
//!
//! # Where the features come from
//!
//! ML-04's inputs, as far as a test-track set can supply them:
//!
//! * **Per source**: detections per scan against the sensor's configured update period;
//!   the fraction of detections associated with no entity; the residual of the associated
//!   ones; a clock-skew estimate; and the gap since the last receipt.
//! * **Per track**: speed; the residual of the detections the entity caused; whether a
//!   cooperative source (a sensor whose definition says `cooperative`, which is AIS and
//!   ADS-B in these sets) reported it in the window; that report's residual; and the gap
//!   since its last cooperative report.
//!
//! **The set's truth stands in for the tracker**, as it does for the ML-01 rows: a
//! detection's association is `detections-truth.jsonl`'s, and a residual is measured
//! against the causing entity's true position at the detection's stamped source time. No
//! tracker runs here (GAP-011 is the pipeline that would), and a row's numbers are the
//! ones a perfect tracker would see -- the honest limit of a synthetic dataset, which the
//! model card has to state.
//!
//! **The clock-skew estimate is the one a live node can compute without truth**: the
//! window's mean apparent latency (receipt time minus source time) less the sensor's
//! configured mean latency. The generator applies electronic-attack skew as a lagging
//! clock -- the source time is stamped behind the observation and the receipt time is not
//! -- so a skewed sensor's apparent latency rises by exactly its skew. An honest clock
//! reads slightly above zero, because latency jitter is one-sided.
//!
//! Two of ML-04's per-track inputs are **not** here, and are left out rather than
//! approximated: speed and heading consistency with the declared class needs the class
//! envelopes, which this crate does not read, and dwell inside a sensitive area needs the
//! scenario's areas, which a set does not carry. Speed is the column a consumer would
//! compare against an envelope.
//!
//! # The labels
//!
//! * **A source's** come from `environment.json`, the per-sensor form of the scenario's
//!   events (`docs/test-tracks/data-format.md` §10): `ea_skew` is `clock-skew`,
//!   `ea_dropout` is `dropout`, and `sensor_lost` is `sensor-lost`, for every window the
//!   event's interval overlaps. Other events -- sea state, a link lost between nodes, a
//!   peer's launch warning -- are not a source misbehaving and label nothing.
//! * **A track's** come from `entities.json`, for every window the entity is alive in: an
//!   AIS spoofing offset is `spoofed-position`, an intermittent ADS-B transponder is
//!   `intermittent-transponder`, and a cooperative signal switched off is
//!   `cooperative-silent`. The last is derived, because `entities.json` records the
//!   signal an entity emits and not the scenario's `ais: false` that silenced it: an
//!   entity emitting `none` is silent when another entity of the same platform in the set
//!   emits `ais` or `adsb`, which is exactly the case the generator writes `none` for
//!   (`emission_key` in `docs/test-tracks/tools/gen_tracks.py`). A platform no entity in
//!   the set shows emitting cannot be judged, and labels nothing. A decoy is not an
//!   anomaly here: it claims nothing it contradicts, and telling it apart is ML-01's
//!   question.
//!
//! **A label is the planted truth, whether or not the window could show it.** TT-05's
//! sample keeps its spoofed coaster on the northern lane, outside every sensor's coverage,
//! so its rows are labelled and carry no cooperative residual. A consumer evaluating a
//! detector filters on `detections`; the dataset does not pretend the anomaly was seen.
//!
//! `label_kind` is `none` or every label active in the window, sorted and joined with
//! `+`.

use crate::dataset::{lines, read, split_for, SetProvenance};
use crate::MlError;
use arrow::array::{
    ArrayRef, BooleanArray, Float32Array, Float64Array, StringArray, UInt32Array, UInt64Array,
};
use arrow::record_batch::RecordBatch;
use gungnir_interop::dataset::{anomaly_rows_schema, DATASET_VERSION};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;

/// The anomaly rows' own feature schema version, separate from the ML-01 extractor's.
pub const ANOMALY_FEATURE_SCHEMA_VERSION: u32 = 1;

/// A window is this many truth ticks: twenty seconds in every committed set.
pub const WINDOW_TICKS: f64 = 10.0;

/// The label of a window with no planted anomaly active.
pub const NO_ANOMALY: &str = "none";

/// Which kind of subject a row describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SubjectKind {
    Source,
    Track,
}

impl SubjectKind {
    /// The name the `subject_kind` column carries.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            SubjectKind::Source => "source",
            SubjectKind::Track => "track",
        }
    }
}

/// One ML-04 row. A feature that does not apply to the row's kind, or has nothing to
/// measure in its window, is `None` (see `gungnir_interop::dataset::anomaly_rows_schema`).
#[derive(Debug, Clone, PartialEq)]
pub struct AnomalyRow {
    pub subject_kind: SubjectKind,
    /// The sensor's id for a source, the entity's for a track.
    pub subject_id: String,
    pub window_start: f64,
    pub window_end: f64,
    /// Detections in the window: the sensor's, or the ones the entity caused.
    pub detections: u32,
    pub detections_per_scan: Option<f32>,
    pub unassociated_fraction: Option<f32>,
    pub residual_rms_m: Option<f32>,
    pub clock_skew_s: Option<f32>,
    pub receipt_gap_s: Option<f32>,
    pub speed_mps: Option<f32>,
    pub cooperative_present: Option<bool>,
    pub cooperative_residual_m: Option<f32>,
    pub cooperative_gap_s: Option<f32>,
    pub label_kind: String,
}

impl AnomalyRow {
    /// Whether any planted anomaly is active in the window.
    #[must_use]
    pub fn label_anomaly(&self) -> bool {
        self.label_kind != NO_ANOMALY
    }
}

/// The ML-04 rows from one set.
#[derive(Debug, Clone, PartialEq)]
pub struct AnomalyDataset {
    pub provenance: SetProvenance,
    pub split: &'static str,
    pub feature_schema_version: u32,
    pub window_s: f64,
    pub rows: Vec<AnomalyRow>,
    /// Rows per `label_kind`, for the card: an anomaly detector's set is mostly `none`,
    /// and the count says by how much.
    pub rows_by_label: BTreeMap<String, usize>,
}

#[derive(serde::Deserialize)]
struct Metadata {
    scenario: String,
    variant: String,
    seed: u64,
    generator: String,
    catalogue_version: String,
    classes_version: String,
    sensors_version: String,
    scenarios_version: String,
    truth_tick_s: f64,
    duration_s: f64,
}

#[derive(serde::Deserialize)]
struct TruthLine {
    t: f64,
    entity: String,
    pos: [f64; 3],
    vel: [f64; 3],
    alive: bool,
}

#[derive(serde::Deserialize)]
struct DetectionLine {
    sensor: u32,
    source_time: f64,
    receipt_time: f64,
    measurement: Measurement,
}

/// A detection's measurement. The committed sets are position feeds
/// (`docs/test-tracks/data-format.md` §3); anything else has no residual to measure.
#[derive(serde::Deserialize)]
struct Measurement {
    #[serde(rename = "Position")]
    position: Option<Position>,
}

#[derive(serde::Deserialize)]
struct Position {
    enu: [f64; 3],
}

#[derive(serde::Deserialize)]
struct DetectionTruthLine {
    line: usize,
    entity: Option<String>,
}

#[derive(serde::Deserialize)]
struct SensorsFile {
    sensors: Vec<SensorDef>,
}

#[derive(serde::Deserialize)]
struct SensorDef {
    id: u32,
    params: SensorParams,
}

#[derive(serde::Deserialize)]
struct SensorParams {
    update_period_s: f64,
    latency_s: Latency,
    #[serde(default)]
    cooperative: bool,
}

#[derive(serde::Deserialize)]
struct Latency {
    mean: f64,
}

#[derive(serde::Deserialize)]
struct EnvironmentFile {
    events: Vec<EnvironmentEvent>,
}

#[derive(serde::Deserialize)]
struct EnvironmentEvent {
    kind: String,
    sensor: Option<u32>,
    t: f64,
    until_s: Option<f64>,
}

#[derive(serde::Deserialize)]
struct EntitiesFile {
    entities: Vec<EntityDef>,
}

#[derive(serde::Deserialize)]
struct EntityDef {
    id: String,
    platform: String,
    adsb_intermittent: Option<f64>,
    ais_spoof_offset_m: Option<[f64; 2]>,
    signature: Signature,
}

#[derive(serde::Deserialize)]
struct Signature {
    emission: String,
}

/// The cooperative signals a platform's peers can show it should be emitting.
const COOPERATIVE_EMISSIONS: [&str; 2] = ["ais", "adsb"];

/// One detection, joined to the entity that caused it.
struct Observed {
    sensor: u32,
    source_time: f64,
    receipt_time: f64,
    position: Option<[f64; 3]>,
    entity: Option<String>,
}

/// An entity's truth, in time order, for the residuals.
struct Trajectory(Vec<TruthLine>);

impl Trajectory {
    /// The entity's position at `t`: the last tick at or before it, carried forward by
    /// that tick's velocity. A tick is two seconds and the motion within one is close to
    /// constant, which is all a residual needs.
    fn position_at(&self, t: f64) -> Option<[f64; 3]> {
        let after = self.0.partition_point(|l| l.t <= t);
        let line = self.0.get(after.checked_sub(1)?)?;
        let dt = t - line.t;
        Some([
            line.pos[0] + line.vel[0] * dt,
            line.pos[1] + line.vel[1] * dt,
            line.pos[2] + line.vel[2] * dt,
        ])
    }
}

fn parse<T: serde::de::DeserializeOwned>(dir: &Path, name: &str) -> Result<T, MlError> {
    serde_json::from_str(&read(&dir.join(name))?)
        .map_err(|e| MlError::ManifestInvalid(format!("{name}: {e}")))
}

/// Extract the ML-04 rows from one set directory.
///
/// # Errors
///
/// [`MlError::ManifestInvalid`] when a file of the set is missing or does not parse, or
/// the set's tick or duration cannot make a window.
pub fn extract_anomaly_set(dir: &Path) -> Result<AnomalyDataset, MlError> {
    let metadata: Metadata = parse(dir, "metadata.json")?;
    let sensors: SensorsFile = parse(dir, "sensors.json")?;
    let environment: EnvironmentFile = parse(dir, "environment.json")?;
    let entities: EntitiesFile = parse(dir, "entities.json")?;
    let truth: Vec<TruthLine> = lines(&dir.join("truth.jsonl"))?;
    let detections: Vec<DetectionLine> = lines(&dir.join("detections.jsonl"))?;
    let detections_truth: Vec<DetectionTruthLine> = lines(&dir.join("detections-truth.jsonl"))?;

    let window_s = metadata.truth_tick_s * WINDOW_TICKS;
    if !(window_s.is_finite() && window_s > 0.0 && metadata.duration_s.is_finite()) {
        return Err(MlError::ManifestInvalid(format!(
            "metadata.json: a tick of {} s over {} s makes no window",
            metadata.truth_tick_s, metadata.duration_s
        )));
    }
    // Whole windows only: a short last window would compare a rate over a different span.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let window_count = (metadata.duration_s / window_s).floor().max(0.0) as usize;
    #[allow(clippy::cast_precision_loss)]
    let spans: Vec<(f64, f64)> = (0..window_count)
        .map(|k| (k as f64 * window_s, (k + 1) as f64 * window_s))
        .collect();

    let set = JoinedSet::new(truth, detections, detections_truth, spans, window_s);
    let mut rows = set.source_rows(&sensors.sensors, &environment.events);
    let cooperative: BTreeSet<u32> = sensors
        .sensors
        .iter()
        .filter(|s| s.params.cooperative)
        .map(|s| s.id)
        .collect();
    rows.extend(set.track_rows(&entities.entities, &cooperative));

    let mut rows_by_label = BTreeMap::new();
    for row in &rows {
        *rows_by_label.entry(row.label_kind.clone()).or_insert(0) += 1;
    }
    Ok(AnomalyDataset {
        split: split_for(&metadata.scenario),
        provenance: SetProvenance {
            scenario: metadata.scenario,
            variant: metadata.variant,
            seed: metadata.seed,
            generator: metadata.generator,
            catalogue_version: metadata.catalogue_version,
            classes_version: metadata.classes_version,
            sensors_version: metadata.sensors_version,
            scenarios_version: metadata.scenarios_version,
        },
        feature_schema_version: ANOMALY_FEATURE_SCHEMA_VERSION,
        window_s,
        rows,
        rows_by_label,
    })
}

/// A set's detections joined to their causes and its truth by entity, over its windows.
struct JoinedSet {
    observed: Vec<Observed>,
    trajectories: HashMap<String, Trajectory>,
    /// Entities in the order the truth first names them, so rows come out in a fixed order.
    order: Vec<String>,
    spans: Vec<(f64, f64)>,
    window_s: f64,
}

impl JoinedSet {
    fn new(
        truth: Vec<TruthLine>,
        detections: Vec<DetectionLine>,
        detections_truth: Vec<DetectionTruthLine>,
        spans: Vec<(f64, f64)>,
        window_s: f64,
    ) -> Self {
        // `detections-truth.jsonl` numbers its lines from one.
        let mut cause: Vec<Option<String>> = vec![None; detections.len()];
        for dt in detections_truth {
            if let Some(slot) = dt.line.checked_sub(1).and_then(|i| cause.get_mut(i)) {
                *slot = dt.entity;
            }
        }
        let observed = detections
            .into_iter()
            .zip(cause)
            .map(|(d, entity)| Observed {
                sensor: d.sensor,
                source_time: d.source_time,
                receipt_time: d.receipt_time,
                position: d.measurement.position.map(|p| p.enu),
                entity,
            })
            .collect();
        let mut order: Vec<String> = Vec::new();
        let mut trajectories: HashMap<String, Trajectory> = HashMap::new();
        for line in truth {
            if !trajectories.contains_key(&line.entity) {
                order.push(line.entity.clone());
            }
            trajectories
                .entry(line.entity.clone())
                .or_insert_with(|| Trajectory(Vec::new()))
                .0
                .push(line);
        }
        for trajectory in trajectories.values_mut() {
            trajectory.0.sort_by(|a, b| a.t.total_cmp(&b.t));
        }
        Self {
            observed,
            trajectories,
            order,
            spans,
            window_s,
        }
    }

    /// The distance from a detection to its causing entity's true position at the
    /// detection's stamped source time; `None` for a false alarm or a non-position.
    fn residual(&self, o: &Observed) -> Option<f64> {
        let entity = o.entity.as_ref()?;
        let at = self.trajectories.get(entity)?.position_at(o.source_time)?;
        let p = o.position?;
        Some(((p[0] - at[0]).powi(2) + (p[1] - at[1]).powi(2) + (p[2] - at[2]).powi(2)).sqrt())
    }

    /// One row per sensor per window.
    fn source_rows(&self, sensors: &[SensorDef], events: &[EnvironmentEvent]) -> Vec<AnomalyRow> {
        let mut rows = Vec::new();
        for sensor in sensors {
            let own: Vec<&Observed> = self
                .observed
                .iter()
                .filter(|o| o.sensor == sensor.id)
                .collect();
            let scans = self.window_s / sensor.params.update_period_s;
            for &(start, end) in &self.spans {
                let inside: Vec<&Observed> = own
                    .iter()
                    .copied()
                    .filter(|o| o.source_time >= start && o.source_time < end)
                    .collect();
                let count = inside.len();
                #[allow(clippy::cast_precision_loss)]
                let unassociated = (count > 0).then(|| {
                    inside.iter().filter(|o| o.entity.is_none()).count() as f64 / count as f64
                });
                let clock_skew = mean(inside.iter().map(|o| o.receipt_time - o.source_time))
                    .map(|latency| latency - sensor.params.latency_s.mean);
                let last_receipt = latest(own.iter().map(|o| o.receipt_time).filter(|t| *t <= end));
                #[allow(clippy::cast_precision_loss)]
                rows.push(AnomalyRow {
                    subject_kind: SubjectKind::Source,
                    subject_id: sensor.id.to_string(),
                    window_start: start,
                    window_end: end,
                    detections: u32::try_from(count).unwrap_or(u32::MAX),
                    detections_per_scan: Some(narrow(count as f64 / scans)),
                    unassociated_fraction: unassociated.map(narrow),
                    residual_rms_m: rms(inside.iter().filter_map(|o| self.residual(o))).map(narrow),
                    clock_skew_s: clock_skew.map(narrow),
                    // Since the set began, for a source that has never reported.
                    receipt_gap_s: Some(narrow(end - last_receipt.unwrap_or(0.0))),
                    speed_mps: None,
                    cooperative_present: None,
                    cooperative_residual_m: None,
                    cooperative_gap_s: None,
                    label_kind: source_label(events, sensor.id, start, end),
                });
            }
        }
        rows
    }

    /// One row per entity per window it is alive in.
    fn track_rows(&self, entities: &[EntityDef], cooperative: &BTreeSet<u32>) -> Vec<AnomalyRow> {
        let flags: HashMap<&str, &EntityDef> =
            entities.iter().map(|e| (e.id.as_str(), e)).collect();
        let cooperative_platforms: BTreeSet<&str> = entities
            .iter()
            .filter(|e| COOPERATIVE_EMISSIONS.contains(&e.signature.emission.as_str()))
            .map(|e| e.platform.as_str())
            .collect();
        let mut rows = Vec::new();
        for entity in &self.order {
            let Some(trajectory) = self.trajectories.get(entity) else {
                continue;
            };
            let caused: Vec<&Observed> = self
                .observed
                .iter()
                .filter(|o| o.entity.as_deref() == Some(entity.as_str()))
                .collect();
            let label = track_label(flags.get(entity.as_str()).copied(), &cooperative_platforms);
            for &(start, end) in &self.spans {
                let Some(last_alive) = trajectory
                    .0
                    .iter()
                    .rev()
                    .find(|l| l.alive && l.t >= start && l.t < end)
                else {
                    continue;
                };
                let inside: Vec<&Observed> = caused
                    .iter()
                    .copied()
                    .filter(|o| o.source_time >= start && o.source_time < end)
                    .collect();
                let reported: Vec<&Observed> = inside
                    .iter()
                    .copied()
                    .filter(|o| cooperative.contains(&o.sensor))
                    .collect();
                let last_report = latest(
                    caused
                        .iter()
                        .filter(|o| cooperative.contains(&o.sensor) && o.source_time < end)
                        .map(|o| o.source_time),
                );
                let v = last_alive.vel;
                rows.push(AnomalyRow {
                    subject_kind: SubjectKind::Track,
                    subject_id: entity.clone(),
                    window_start: start,
                    window_end: end,
                    detections: u32::try_from(inside.len()).unwrap_or(u32::MAX),
                    detections_per_scan: None,
                    unassociated_fraction: None,
                    residual_rms_m: rms(inside.iter().filter_map(|o| self.residual(o))).map(narrow),
                    clock_skew_s: None,
                    receipt_gap_s: None,
                    speed_mps: Some(narrow((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt())),
                    cooperative_present: Some(!reported.is_empty()),
                    cooperative_residual_m: rms(reported.iter().filter_map(|o| self.residual(o)))
                        .map(narrow),
                    cooperative_gap_s: last_report.map(|t| narrow(end - t)),
                    label_kind: label.clone(),
                });
            }
        }
        rows
    }
}

/// The latest of some times, if there are any.
fn latest(times: impl Iterator<Item = f64>) -> Option<f64> {
    times.fold(None, |acc, t| Some(acc.map_or(t, |a: f64| a.max(t))))
}

/// The labels a source carries in `[start, end)`.
fn source_label(events: &[EnvironmentEvent], sensor: u32, start: f64, end: f64) -> String {
    let active: BTreeSet<&str> = events
        .iter()
        .filter(|e| e.sensor == Some(sensor))
        .filter(|e| e.t < end && e.until_s.unwrap_or(f64::INFINITY) > start)
        .filter_map(|e| match e.kind.as_str() {
            "ea_skew" => Some("clock-skew"),
            "ea_dropout" => Some("dropout"),
            "sensor_lost" => Some("sensor-lost"),
            _ => None,
        })
        .collect();
    joined(active)
}

/// The labels a track carries for its whole life.
fn track_label(entity: Option<&EntityDef>, cooperative_platforms: &BTreeSet<&str>) -> String {
    let mut active = BTreeSet::new();
    if let Some(e) = entity {
        if e.ais_spoof_offset_m.is_some() {
            active.insert("spoofed-position");
        }
        if e.adsb_intermittent.is_some() {
            active.insert("intermittent-transponder");
        }
        if e.signature.emission == "none" && cooperative_platforms.contains(e.platform.as_str()) {
            active.insert("cooperative-silent");
        }
    }
    joined(active)
}

fn joined(active: BTreeSet<&str>) -> String {
    if active.is_empty() {
        NO_ANOMALY.to_owned()
    } else {
        active.into_iter().collect::<Vec<_>>().join("+")
    }
}

fn mean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let (sum, n) = values.fold((0.0, 0u32), |(s, n), v| (s + v, n + 1));
    (n > 0).then(|| sum / f64::from(n))
}

fn rms(values: impl Iterator<Item = f64>) -> Option<f64> {
    mean(values.map(|v| v * v)).map(f64::sqrt)
}

/// A feature in the schema's `Float32`.
#[allow(clippy::cast_possible_truncation)]
fn narrow(v: f64) -> f32 {
    v as f32
}

impl AnomalyDataset {
    /// The rows in the catalogue's Arrow form (`gungnir.ml.anomaly-rows`).
    ///
    /// # Errors
    ///
    /// An Arrow error building the batch, as [`MlError::ManifestInvalid`] with Arrow's
    /// words.
    pub fn to_record_batch(&self) -> Result<RecordBatch, MlError> {
        let n = self.rows.len();
        let p = &self.provenance;
        let source_set = format!("{}-{}", p.scenario, p.variant);
        let f32s = |f: fn(&AnomalyRow) -> Option<f32>| -> ArrayRef {
            Arc::new(Float32Array::from(
                self.rows.iter().map(f).collect::<Vec<_>>(),
            ))
        };
        let columns: Vec<ArrayRef> = vec![
            Arc::new(UInt32Array::from(vec![DATASET_VERSION; n])),
            Arc::new(UInt32Array::from(vec![self.feature_schema_version; n])),
            Arc::new(StringArray::from(vec![source_set.as_str(); n])),
            Arc::new(StringArray::from(vec![p.scenario.as_str(); n])),
            Arc::new(UInt64Array::from(vec![p.seed; n])),
            Arc::new(StringArray::from(
                self.rows
                    .iter()
                    .map(|r| r.subject_kind.as_str())
                    .collect::<Vec<_>>(),
            )),
            Arc::new(StringArray::from(
                self.rows
                    .iter()
                    .map(|r| r.subject_id.as_str())
                    .collect::<Vec<_>>(),
            )),
            Arc::new(Float64Array::from(
                self.rows.iter().map(|r| r.window_start).collect::<Vec<_>>(),
            )),
            Arc::new(Float64Array::from(
                self.rows.iter().map(|r| r.window_end).collect::<Vec<_>>(),
            )),
            Arc::new(UInt32Array::from(
                self.rows.iter().map(|r| r.detections).collect::<Vec<_>>(),
            )),
            f32s(|r| r.detections_per_scan),
            f32s(|r| r.unassociated_fraction),
            f32s(|r| r.residual_rms_m),
            f32s(|r| r.clock_skew_s),
            f32s(|r| r.receipt_gap_s),
            f32s(|r| r.speed_mps),
            Arc::new(BooleanArray::from(
                self.rows
                    .iter()
                    .map(|r| r.cooperative_present)
                    .collect::<Vec<_>>(),
            )),
            f32s(|r| r.cooperative_residual_m),
            f32s(|r| r.cooperative_gap_s),
            Arc::new(BooleanArray::from(
                self.rows
                    .iter()
                    .map(AnomalyRow::label_anomaly)
                    .collect::<Vec<_>>(),
            )),
            Arc::new(StringArray::from(
                self.rows
                    .iter()
                    .map(|r| r.label_kind.as_str())
                    .collect::<Vec<_>>(),
            )),
            Arc::new(StringArray::from(vec![self.split; n])),
        ];
        RecordBatch::try_new(Arc::new(anomaly_rows_schema()), columns)
            .map_err(|e| MlError::ManifestInvalid(format!("arrow: {e}")))
    }

    /// The content hash that identifies this dataset (§5), over the same things the
    /// ML-01 hash covers: every row, the split, the feature schema and the set's versions.
    /// An absent feature hashes differently from any present value, zero included.
    #[must_use]
    pub fn content_hash(&self) -> String {
        let mut h = Sha256::new();
        let p = &self.provenance;
        for field in [
            &p.scenario,
            &p.variant,
            &p.generator,
            &p.catalogue_version,
            &p.classes_version,
            &p.sensors_version,
            &p.scenarios_version,
        ] {
            h.update(field.as_bytes());
            h.update([0]);
        }
        h.update(p.seed.to_le_bytes());
        h.update(self.feature_schema_version.to_le_bytes());
        h.update(self.window_s.to_le_bytes());
        h.update(self.split.as_bytes());
        for r in &self.rows {
            h.update(r.subject_kind.as_str().as_bytes());
            h.update([0]);
            h.update(r.subject_id.as_bytes());
            h.update([0]);
            h.update(r.window_start.to_le_bytes());
            h.update(r.window_end.to_le_bytes());
            h.update(r.detections.to_le_bytes());
            for v in [
                r.detections_per_scan,
                r.unassociated_fraction,
                r.residual_rms_m,
                r.clock_skew_s,
                r.receipt_gap_s,
                r.speed_mps,
                r.cooperative_residual_m,
                r.cooperative_gap_s,
            ] {
                match v {
                    Some(v) => {
                        h.update([1]);
                        h.update(v.to_le_bytes());
                    }
                    None => h.update([0]),
                }
            }
            h.update([match r.cooperative_present {
                None => 0,
                Some(false) => 1,
                Some(true) => 2,
            }]);
            h.update(r.label_kind.as_bytes());
            h.update([0]);
        }
        format!("{:x}", h.finalize())
    }
}
