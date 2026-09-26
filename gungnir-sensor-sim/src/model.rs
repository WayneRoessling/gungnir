// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A sensor's detection model: the parameters [`crate::observe`] reads.
//!
//! The same fields `docs/test-tracks/sensors.yaml` gives each sensor type and
//! `docs/test-tracks/sensor-models.md` defines. `gungnir-scenario` builds one from its
//! YAML-typed `SensorType`; a rehearsal reads one from the JSON export of that catalogue
//! (`testdata/tracks/sensor-models.json`, docs/design/DN-32-re-observation-for-a-laydown.md
//! §5.4), which is why the type deserialises and ignores what it does not read: the
//! export carries each type's `name` and `confidence` for a reader, and a fixture's own
//! `sensors.json` carries extras such as `gnss_dependent`.

use crate::pynum::Num;
use std::collections::BTreeMap;

/// Measurement noise, one standard deviation per axis, metres.
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
pub struct Noise {
    pub range_m: Num,
    pub cross_m: Num,
    pub height_m: Num,
}

/// Delivery latency: a mean plus the absolute value of a zero-mean jitter draw.
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
pub struct Latency {
    pub mean: Num,
    pub jitter: Num,
}

/// Electronic-attack sensitivities a scenario may apply.
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
pub struct ElectronicAttack {
    pub skew_s: Num,
    pub dropout_multiplier: Num,
    pub fa_multiplier: Num,
}

/// A fixed registration bias added to every measurement, metres east and north.
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
pub struct Bias {
    #[serde(default = "zero")]
    pub east: Num,
    #[serde(default = "zero")]
    pub north: Num,
}

fn zero() -> Num {
    Num::Float(0.0)
}

/// One sensor's detection model.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct SensorParams {
    /// Which signature class selects the range band: `rcs`, `ir`, `acoustic`, or any
    /// other key for an emission class.
    pub signature_key: String,
    /// Detection range per signature class, metres. A class with no band is not seen.
    pub range_m: BTreeMap<String, Num>,
    /// Probability of detection inside the band, before dropout.
    pub pd_in_range: Num,
    pub update_period_s: Num,
    pub noise: Noise,
    /// Bearings the sensor sees, degrees clockwise from north; `[0, 360]` is all round.
    pub field_of_regard_deg: [Num; 2],
    /// Target heights the sensor sees, metres.
    pub altitude_m: [Num; 2],
    /// Whether the 4/3-earth radar horizon limits detection.
    #[serde(default)]
    pub horizon: bool,
    pub latency_s: Latency,
    /// Probability that an opportunity is lost outright.
    pub dropout: Num,
    /// Fraction of detections delivered half a second to a second and a half late.
    pub out_of_order: Num,
    pub false_alarms_per_scan: Num,
    pub ea: ElectronicAttack,
    /// Sees only what another, uncued sensor has seen in the last ten seconds.
    #[serde(default)]
    pub cued: bool,
    /// Sees only a target moving at a metre a second or more.
    #[serde(default)]
    pub moving_only: bool,
    /// Extra dropout per sea state.
    #[serde(default)]
    pub sea_state_dropout: BTreeMap<i64, Num>,
    /// A registration bias, where the sensor has one.
    #[serde(default)]
    pub bias_m: Option<Bias>,
}

/// The JSON export of `docs/test-tracks/sensors.yaml`: every sensor type by its
/// identifier (docs/design/DN-32-re-observation-for-a-laydown.md §5.4). A deployment
/// sensor names one of these as its detection model.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct SensorCatalogue {
    /// Always 1 for this shape.
    pub format: u32,
    /// The `version` of the `sensors.yaml` it was exported from.
    pub sensors_version: String,
    /// Every sensor type, by identifier (`radar.long`, `eo-ir`, `acoustic`, ...).
    pub types: BTreeMap<String, CatalogueEntry>,
}

/// One type in the catalogue export: a readable name, and the model.
///
/// The model is its own member rather than flattened beside the name: a flattened map
/// loses `serde_json`'s reading of `sea_state_dropout`'s `"4"` as the integer 4.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct CatalogueEntry {
    pub name: String,
    /// The catalogue's own statement of how far to trust the numbers
    /// (`assumption` for every type today).
    #[serde(default)]
    pub confidence: Option<String>,
    pub model: SensorParams,
}

impl SensorCatalogue {
    /// The model a sensor type names, if the catalogue holds it.
    #[must_use]
    pub fn model(&self, id: &str) -> Option<&SensorParams> {
        self.types.get(id).map(|e| &e.model)
    }
}
