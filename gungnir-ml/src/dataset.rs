//! From a test-track set to a training dataset (`docs/ml/data-pipeline.md`; GAP-079).
//!
//! The ML-01 rows are one per entity per truth tick, with the truth's class and side as
//! the labels and the kinematic features over the entity's own history -- the set's
//! `truth.jsonl` is the track proxy, because no tracker runs here (and the pipeline that
//! would is GAP-011), and a row says so through `track_id` being the entity's ordinal.
//! `sensor_count` and `sensor_mix` come from `detections-truth.jsonl`: the detections
//! the entity actually caused inside the window. `cooperative_present` is false for
//! every row of a set, which is stated rather than guessed: the set records no
//! cooperative declaration per entity.
//!
//! Split **by scenario** (§3), never by row; every dataset carries the set's four input
//! versions, the generator, the feature schema and the split, and is identified by a
//! content hash over its rows (§5).

use crate::features::{KinematicExtractor, FEATURE_NAMES, SCHEMA_VERSION};
use crate::MlError;
use arrow::array::{
    ArrayRef, BooleanArray, Float32Array, Float64Array, StringArray, UInt32Array, UInt64Array,
};
use arrow::record_batch::RecordBatch;
use gungnir_interop::dataset::{classification_rows_schema, DATASET_VERSION};
use gungnir_model::{
    Classification, MissionTime, Provenance, Quality, Releasability, TrackId, TrackStatus,
    TrackView,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

/// The split a scenario belongs to (`docs/ml/data-pipeline.md` §3).
#[must_use]
pub fn split_for(scenario: &str) -> &'static str {
    match scenario {
        "TT-01" | "TT-03" | "TT-04" | "TT-06" | "TT-08" => "train",
        "TT-02" | "TT-07" => "validation",
        "TT-05" | "TT-09" | "TT-10" => "test",
        _ => "unassigned",
    }
}

/// The versions a dataset carries (§5).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SetProvenance {
    pub scenario: String,
    pub variant: String,
    pub seed: u64,
    pub generator: String,
    pub catalogue_version: String,
    pub classes_version: String,
    pub sensors_version: String,
    pub scenarios_version: String,
}

/// One ML-01 row (§2).
#[derive(Debug, Clone, PartialEq)]
pub struct ClassificationRow {
    pub entity_id: String,
    pub track_id: u64,
    pub mission_time: f64,
    pub features: Vec<f32>,
    pub label_class: String,
    pub label_side: String,
}

/// A dataset from one set.
#[derive(Debug, Clone, PartialEq)]
pub struct Dataset {
    pub provenance: SetProvenance,
    pub split: &'static str,
    pub feature_schema_version: u32,
    pub rows: Vec<ClassificationRow>,
    /// Rows per class, for the card (§4).
    pub rows_by_class: BTreeMap<String, usize>,
}

#[derive(serde::Deserialize)]
struct TruthLine {
    t: f64,
    entity: String,
    class: String,
    side: String,
    pos: [f64; 3],
    vel: [f64; 3],
    alive: bool,
}

#[derive(serde::Deserialize)]
struct DetectionTruthLine {
    line: usize,
    entity: Option<String>,
}

#[derive(serde::Deserialize)]
struct DetectionLine {
    sensor: u32,
    source_time: f64,
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
}

fn read(path: &Path) -> Result<String, MlError> {
    std::fs::read_to_string(path)
        .map_err(|e| MlError::ManifestInvalid(format!("{}: {e}", path.display())))
}

fn lines<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Vec<T>, MlError> {
    read(path)?
        .lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(n, l)| {
            serde_json::from_str(l)
                .map_err(|e| MlError::ManifestInvalid(format!("{}:{}: {e}", path.display(), n + 1)))
        })
        .collect()
}

/// Extract the ML-01 rows from one set directory.
///
/// # Errors
///
/// [`MlError::ManifestInvalid`] when a file of the set is missing or does not parse.
pub fn extract_set(dir: &Path) -> Result<Dataset, MlError> {
    let metadata: Metadata = serde_json::from_str(&read(&dir.join("metadata.json"))?)
        .map_err(|e| MlError::ManifestInvalid(format!("metadata.json: {e}")))?;
    let truth: Vec<TruthLine> = lines(&dir.join("truth.jsonl"))?;
    let detections: Vec<DetectionLine> = lines(&dir.join("detections.jsonl"))?;
    let detections_truth: Vec<DetectionTruthLine> = lines(&dir.join("detections-truth.jsonl"))?;

    // The detections each entity caused, by source time, for the sensor columns.
    let mut caused: HashMap<String, Vec<(f64, u32)>> = HashMap::new();
    for dt in &detections_truth {
        let Some(entity) = &dt.entity else {
            continue;
        };
        if let Some(d) = detections.get(dt.line.wrapping_sub(1)) {
            caused
                .entry(entity.clone())
                .or_default()
                .push((d.source_time, d.sensor));
        }
    }

    let mut ordinals: HashMap<String, u64> = HashMap::new();
    let mut extractor = KinematicExtractor::new(10);
    let none = HashSet::new();
    let window_s = metadata.truth_tick_s * 10.0;
    let mut rows = Vec::with_capacity(truth.len());
    let mut rows_by_class = BTreeMap::new();
    for line in &truth {
        if !line.alive {
            extractor.forget(TrackId(ordinal(&mut ordinals, &line.entity)));
            continue;
        }
        let track_id = ordinal(&mut ordinals, &line.entity);
        let sensors: Vec<u32> = caused
            .get(&line.entity)
            .map(|v| {
                v.iter()
                    .filter(|(t, _)| *t <= line.t && *t > line.t - window_s)
                    .map(|(_, s)| *s)
                    .collect()
            })
            .unwrap_or_default();
        let view = TrackView {
            id: TrackId(track_id),
            status: TrackStatus::Confirmed,
            state: nalgebra::SVector::<f64, 6>::new(
                line.pos[0],
                line.pos[1],
                line.pos[2],
                line.vel[0],
                line.vel[1],
                line.vel[2],
            ),
            covariance: nalgebra::SMatrix::identity(),
            classification: match line.side.as_str() {
                "red" => Classification::Hostile,
                "blue" => Classification::Friendly,
                "civil" => Classification::Neutral,
                _ => Classification::Unknown,
            },
            provenance: Provenance {
                source_sensor_ids: sensors,
                ..Provenance::default()
            },
            quality: Quality::default(),
            mission_time: MissionTime(line.t),
            releasability: Releasability::default(),
        };
        let batch = extractor.extract(&[view], &none);
        let features = batch.rows.into_iter().next().unwrap_or_default();
        *rows_by_class.entry(line.class.clone()).or_insert(0) += 1;
        rows.push(ClassificationRow {
            entity_id: line.entity.clone(),
            track_id,
            mission_time: line.t,
            features,
            label_class: line.class.clone(),
            label_side: line.side.clone(),
        });
    }
    Ok(Dataset {
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
        feature_schema_version: SCHEMA_VERSION,
        rows,
        rows_by_class,
    })
}

fn ordinal(ordinals: &mut HashMap<String, u64>, entity: &str) -> u64 {
    let next = ordinals.len() as u64 + 1;
    *ordinals.entry(entity.to_owned()).or_insert(next)
}

impl Dataset {
    /// The rows in the catalogue's Arrow form (`gungnir.ml.classification-rows`).
    ///
    /// # Errors
    ///
    /// [`MlError::Inference`] is not raised here; an Arrow error building the batch is
    /// reported as [`MlError::ManifestInvalid`] with Arrow's words.
    pub fn to_record_batch(&self) -> Result<RecordBatch, MlError> {
        let n = self.rows.len();
        let p = &self.provenance;
        let source_set = format!("{}-{}", p.scenario, p.variant);
        let mut columns: Vec<ArrayRef> = vec![
            std::sync::Arc::new(UInt32Array::from(vec![DATASET_VERSION; n])),
            std::sync::Arc::new(UInt32Array::from(vec![self.feature_schema_version; n])),
            std::sync::Arc::new(StringArray::from(vec![source_set.as_str(); n])),
            std::sync::Arc::new(StringArray::from(vec![p.scenario.as_str(); n])),
            std::sync::Arc::new(UInt64Array::from(vec![p.seed; n])),
            std::sync::Arc::new(StringArray::from(
                self.rows
                    .iter()
                    .map(|r| r.entity_id.as_str())
                    .collect::<Vec<_>>(),
            )),
            std::sync::Arc::new(UInt64Array::from(
                self.rows.iter().map(|r| r.track_id).collect::<Vec<_>>(),
            )),
            std::sync::Arc::new(Float64Array::from(
                self.rows.iter().map(|r| r.mission_time).collect::<Vec<_>>(),
            )),
        ];
        for (i, name) in FEATURE_NAMES.iter().enumerate() {
            let values: Vec<f32> = self.rows.iter().map(|r| r.features[i]).collect();
            let column: ArrayRef = match *name {
                "cooperative_present" => std::sync::Arc::new(BooleanArray::from(
                    values.iter().map(|v| *v > 0.5).collect::<Vec<_>>(),
                )),
                "sensor_mix" => std::sync::Arc::new(StringArray::from(
                    values.iter().map(|v| format!("{v:.3}")).collect::<Vec<_>>(),
                )),
                _ => std::sync::Arc::new(Float32Array::from(values)),
            };
            columns.push(column);
        }
        columns.push(std::sync::Arc::new(StringArray::from(
            self.rows
                .iter()
                .map(|r| r.label_class.as_str())
                .collect::<Vec<_>>(),
        )));
        columns.push(std::sync::Arc::new(StringArray::from(
            self.rows
                .iter()
                .map(|r| r.label_side.as_str())
                .collect::<Vec<_>>(),
        )));
        columns.push(std::sync::Arc::new(StringArray::from(vec![self.split; n])));
        RecordBatch::try_new(std::sync::Arc::new(classification_rows_schema()), columns)
            .map_err(|e| MlError::ManifestInvalid(format!("arrow: {e}")))
    }

    /// The content hash that identifies this dataset (§5): over every row's values and
    /// labels, the split, the feature schema and the set's versions, so the same set
    /// through the same extractor gives the same hash and anything else does not.
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
        h.update(self.split.as_bytes());
        for r in &self.rows {
            h.update(r.entity_id.as_bytes());
            h.update(r.track_id.to_le_bytes());
            h.update(r.mission_time.to_le_bytes());
            for f in &r.features {
                h.update(f.to_le_bytes());
            }
            h.update(r.label_class.as_bytes());
            h.update([0]);
            h.update(r.label_side.as_bytes());
            h.update([0]);
        }
        format!("{:x}", h.finalize())
    }
}
