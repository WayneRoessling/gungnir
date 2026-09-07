// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The plan-07 test-track library as typed data (GAP-016, D-31).
//!
//! `docs/test-tracks/tools/gen_tracks.py` composes the TT-01 to TT-10 sets from four
//! YAML files: `scenarios.yaml` (geography, routes, sensor sets, scenarios),
//! `classes.yaml` (kinematic class profiles), `sensors.yaml` (sensor models) and
//! `catalogue.yaml` with its per-domain includes (platforms). This module reads the same
//! files into Rust types through `yaml_serde` and checks the cross-references the Python
//! generator resolves at run time: every entity names a class the profiles hold and a
//! platform the catalogue holds under that class, every route and sensor position names a
//! point that exists, every sensor instance names a sensor type, and every scenario names
//! sensor sets that are declared.
//!
//! [`crate::tracks`] is the composition over these types: the reference generator ported
//! so a TT set can be regenerated from Rust and compared with the committed one byte for
//! byte. That is why numbers are [`Num`] rather than `f64` here: the reference is Python,
//! and whether a YAML value was written `0` or `0.0` shows in its output.
//!
//! Unknown keys are kept, not rejected: the YAML is the documentation's source of truth
//! and carries prose fields (`narrative`, `randomization`, `expected`) the generator does
//! not read; a loader that refused a new documentary key would make the docs answer to
//! the code.

use std::collections::BTreeMap;
use std::path::Path;

pub use crate::pynum::Num;

/// A free-form YAML value, for the documentary fields the generator does not interpret.
/// Maps keep the file's key order, because the generator writes some of them back out.
#[derive(Debug, Clone, PartialEq, Default, serde::Deserialize)]
#[serde(untagged)]
pub enum Scalar {
    #[default]
    Null,
    Bool(bool),
    Integer(i64),
    Number(f64),
    Text(String),
    List(Vec<Scalar>),
    Map(OrderedMap),
}

impl Scalar {
    /// The value under `key`, for a map.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Scalar> {
        match self {
            Scalar::Map(m) => m.get(key),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_num(&self) -> Option<Num> {
        match self {
            Scalar::Integer(i) => Some(Num::Int(*i)),
            Scalar::Number(x) => Some(Num::Float(*x)),
            _ => None,
        }
    }

    /// An integer, as the YAML wrote one (a float is not rounded into one).
    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Scalar::Integer(i) => Some(*i),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Scalar::Text(s) => Some(s),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Scalar::Bool(b) => Some(*b),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_list(&self) -> Option<&[Scalar]> {
        match self {
            Scalar::List(l) => Some(l),
            _ => None,
        }
    }
}

/// A mapping key: a word, or a number (`sea_state_dropout: {4: 0.3, 5: 0.45}` keys a
/// table by sea state).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Deserialize)]
#[serde(untagged)]
pub enum Key {
    Number(i64),
    Text(String),
}

impl Key {
    #[must_use]
    pub fn as_text(&self) -> String {
        match self {
            Key::Number(n) => n.to_string(),
            Key::Text(s) => s.clone(),
        }
    }
}

/// A YAML mapping in file order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OrderedMap(pub Vec<(Key, Scalar)>);

impl OrderedMap {
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Scalar> {
        self.0
            .iter()
            .find(|(k, _)| matches!(k, Key::Text(t) if t == key))
            .map(|(_, v)| v)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Key, &Scalar)> {
        self.0.iter().map(|(k, v)| (k, v))
    }
}

impl<'de> serde::Deserialize<'de> for OrderedMap {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = OrderedMap;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a mapping")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut out = Vec::new();
                while let Some((k, v)) = map.next_entry::<Key, Scalar>()? {
                    out.push((k, v));
                }
                Ok(OrderedMap(out))
            }
        }
        deserializer.deserialize_map(V)
    }
}

/// A route node or a sensor position: a named point, or ENU metres.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(untagged)]
pub enum Place {
    Named(String),
    Enu([Num; 3]),
}

/// A figure the YAML writes either as one value or as a `[min, max]` range.
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
#[serde(untagged)]
pub enum Range {
    Fixed(Num),
    Span([Num; 2]),
}

impl Range {
    #[must_use]
    pub fn bounds(self) -> [Num; 2] {
        match self {
            Range::Fixed(v) => [v, v],
            Range::Span(s) => s,
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct Origin {
    pub name: String,
    pub lat: Num,
    pub lon: Num,
    pub alt_m: Num,
}

/// One sensor instance in a sensor set.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct SensorInstance {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: i64,
    pub name: String,
    pub pos: Place,
    #[serde(default)]
    pub calibration: Option<String>,
    #[serde(default)]
    pub overrides: OrderedMap,
}

/// One entity line of a scenario: `count` platforms of a class, spawned in a window.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct EntitySpec {
    pub class: String,
    pub platform: String,
    pub side: String,
    pub count: Num,
    pub spawn_s: [Num; 2],
    /// A route name, `"none"`, or absent for a stationary entity.
    #[serde(default)]
    pub route: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub decoy: bool,
    #[serde(default)]
    pub no_terminal: bool,
    #[serde(default)]
    pub reverse: bool,
    #[serde(default)]
    pub emitting: Option<bool>,
    #[serde(default)]
    pub launch: Option<String>,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub target_group: Option<String>,
    #[serde(default)]
    pub at: Option<Place>,
    #[serde(default)]
    pub spacing_m: Option<Num>,
    #[serde(default)]
    pub cover_gap_s: Option<[Num; 2]>,
    /// The names of the class's phases this entity runs; absent means all of them.
    #[serde(default)]
    pub phases: Option<Vec<String>>,
    #[serde(default)]
    pub adsb: Option<bool>,
    #[serde(default)]
    pub iff: Option<bool>,
    #[serde(default)]
    pub ais: Option<bool>,
    #[serde(default)]
    pub adsb_intermittent: Option<Num>,
    #[serde(default)]
    pub ais_spoof_offset_m: Option<Vec<Num>>,
}

/// A scenario event as the YAML wrote it, in its key order, because the generator
/// writes it back out unchanged.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct EventSpec(pub OrderedMap);

impl EventSpec {
    #[must_use]
    pub fn t(&self) -> Option<Num> {
        self.0.get("t").and_then(Scalar::as_num)
    }

    #[must_use]
    pub fn kind(&self) -> &str {
        self.0.get("kind").and_then(Scalar::as_str).unwrap_or("")
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Scalar> {
        self.0.get(key)
    }
}

/// The reduction of a scenario to its committed sample set.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct SampleSpec {
    pub duration_s: Num,
    pub entity_scale: Num,
    pub seed: u64,
    #[serde(default)]
    pub sensors: Option<Vec<String>>,
    #[serde(default)]
    pub variant: Option<String>,
    #[serde(default)]
    pub events: Option<Vec<EventSpec>>,
}

/// One sensor moved by a laydown variant.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct SensorMove {
    pub sensor: i64,
    pub pos: Place,
}

/// A laydown variant of a scenario.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct VariantSpec {
    pub id: String,
    #[serde(default)]
    pub sensors: Vec<String>,
    #[serde(default)]
    pub moves: Vec<SensorMove>,
}

/// A scenario's entity lines, or `inherit` for one derived from its `base`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(untagged)]
pub enum Entities {
    Inherit(String),
    Lines(Vec<EntitySpec>),
}

impl Default for Entities {
    fn default() -> Self {
        Entities::Lines(Vec::new())
    }
}

impl Entities {
    /// The lines, empty for an inheriting scenario (its base has them).
    #[must_use]
    pub fn lines(&self) -> &[EntitySpec] {
        match self {
            Entities::Inherit(_) => &[],
            Entities::Lines(lines) => lines,
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct ScenarioSpec {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub vignette: Option<String>,
    #[serde(default)]
    pub thread: Option<String>,
    #[serde(default)]
    pub narrative: Option<String>,
    pub duration_s: Num,
    #[serde(default)]
    pub start_time_of_day: Option<String>,
    #[serde(default)]
    pub sensors: Vec<String>,
    #[serde(default)]
    pub entities: Entities,
    #[serde(default)]
    pub events: Vec<EventSpec>,
    #[serde(default)]
    pub expected: OrderedMap,
    #[serde(default)]
    pub sample: Option<SampleSpec>,
    /// A scenario derived from another (`base`) with named variants.
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub variants: Vec<VariantSpec>,
}

/// `scenarios.yaml`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct ScenarioLibrary {
    pub version: String,
    pub origin: Origin,
    pub truth_tick_s: Num,
    pub points: BTreeMap<String, [Num; 3]>,
    pub routes: BTreeMap<String, Vec<Place>>,
    pub sensor_sets: BTreeMap<String, Vec<SensorInstance>>,
    pub scenarios: Vec<ScenarioSpec>,
}

/// One phase of a class's representative mission.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct PhaseSpec {
    pub name: String,
    pub model: String,
    pub duration_s: Range,
    pub speed_mps: Range,
    pub altitude_m: Range,
    #[serde(default)]
    pub weave: Option<Num>,
    #[serde(default)]
    pub radius_m: Option<Range>,
    #[serde(default)]
    pub stop_prob: Option<Num>,
    #[serde(default)]
    pub glide_ratio: Option<Range>,
    #[serde(default)]
    pub fire_stop_s: Option<Range>,
    #[serde(default)]
    pub apogee_m: Option<Range>,
    #[serde(flatten)]
    pub rest: BTreeMap<String, Scalar>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct ClassProfile {
    pub id: String,
    pub domain: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub envelope: BTreeMap<String, [Num; 2]>,
    pub phases: Vec<PhaseSpec>,
    #[serde(default)]
    pub randomization: Scalar,
    #[serde(default)]
    pub signatures: BTreeMap<String, String>,
    #[serde(default)]
    pub sensors: Vec<String>,
    #[serde(default)]
    pub threads: Vec<String>,
    #[serde(default)]
    pub scenarios: Vec<String>,
}

/// `classes.yaml`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct ClassProfiles {
    pub version: String,
    pub classes: Vec<ClassProfile>,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
pub struct Noise {
    pub range_m: Num,
    pub cross_m: Num,
    pub height_m: Num,
}

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

/// One sensor model, with the fields the observation model reads typed and the rest
/// kept.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct SensorType {
    pub id: String,
    pub name: String,
    pub signature_key: String,
    pub range_m: BTreeMap<String, Num>,
    pub pd_in_range: Num,
    pub update_period_s: Num,
    pub noise: Noise,
    pub field_of_regard_deg: [Num; 2],
    pub altitude_m: [Num; 2],
    #[serde(default)]
    pub horizon: bool,
    pub latency_s: Latency,
    pub dropout: Num,
    pub out_of_order: Num,
    pub false_alarms_per_scan: Num,
    pub ea: ElectronicAttack,
    #[serde(default)]
    pub cued: bool,
    #[serde(default)]
    pub moving_only: bool,
    #[serde(default)]
    pub sea_state_dropout: BTreeMap<i64, Num>,
    #[serde(default)]
    pub confidence: Option<String>,
    #[serde(flatten)]
    pub rest: BTreeMap<String, Scalar>,
}

/// `sensors.yaml`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct SensorModels {
    pub version: String,
    pub types: Vec<SensorType>,
    /// Emission descriptions grouped under the emission class a sensor detects.
    #[serde(default)]
    pub emission_map: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct Platform {
    pub id: String,
    pub name: String,
    pub class: String,
    pub side: String,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub altitude_m: Option<[Num; 2]>,
    #[serde(default)]
    pub rcs_class: Option<String>,
    #[serde(default)]
    pub ir_class: Option<String>,
    #[serde(default)]
    pub acoustic_class: Option<String>,
    #[serde(default)]
    pub emissions: Option<String>,
    #[serde(default)]
    pub confidence: Option<String>,
    #[serde(flatten)]
    pub rest: BTreeMap<String, Scalar>,
}

/// `catalogue.yaml`: the index.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct CatalogueIndex {
    pub version: String,
    #[serde(default)]
    pub policy: Option<String>,
    pub includes: Vec<String>,
}

/// One `catalogue-<domain>.yaml`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct CatalogueFile {
    pub domain: String,
    #[serde(default)]
    pub reviewer: Option<String>,
    pub platforms: Vec<Platform>,
}

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("could not read {path}: {reason}")]
    Io { path: String, reason: String },
    #[error("{path} did not parse: {reason}")]
    Yaml { path: String, reason: String },
}

/// The four files, loaded and joined.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackLibrary {
    pub scenarios: ScenarioLibrary,
    pub classes: ClassProfiles,
    pub sensors: SensorModels,
    pub catalogue: CatalogueIndex,
    pub platforms: Vec<Platform>,
}

fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, LibraryError> {
    let text = std::fs::read_to_string(path).map_err(|e| LibraryError::Io {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    yaml_serde::from_str(&text).map_err(|e| LibraryError::Yaml {
        path: path.display().to_string(),
        reason: e.to_string(),
    })
}

impl TrackLibrary {
    /// Load the library from the directory holding the four files.
    ///
    /// # Errors
    ///
    /// A file that cannot be read or does not parse, by name.
    pub fn load(dir: &Path) -> Result<Self, LibraryError> {
        let catalogue: CatalogueIndex = read(&dir.join("catalogue.yaml"))?;
        let mut platforms = Vec::new();
        for include in &catalogue.includes {
            let file: CatalogueFile = read(&dir.join(include))?;
            platforms.extend(file.platforms);
        }
        Ok(Self {
            scenarios: read(&dir.join("scenarios.yaml"))?,
            classes: read(&dir.join("classes.yaml"))?,
            sensors: read(&dir.join("sensors.yaml"))?,
            catalogue,
            platforms,
        })
    }

    /// Every cross-reference that does not resolve, as one line each. Empty means the
    /// generator's inputs are consistent.
    #[must_use]
    pub fn unresolved(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let lib = &self.scenarios;
        let point_ok = |p: &Place| match p {
            Place::Named(name) => lib.points.contains_key(name),
            Place::Enu(_) => true,
        };
        for (name, route) in &lib.routes {
            for node in route {
                if !point_ok(node) {
                    problems.push(format!("route {name} names an unknown point {node:?}"));
                }
            }
        }
        for (set, sensors) in &lib.sensor_sets {
            for s in sensors {
                if !point_ok(&s.pos) {
                    problems.push(format!(
                        "sensor {} in set {set} stands at an unknown point",
                        s.id
                    ));
                }
                if !self.sensors.types.iter().any(|t| t.id == s.kind) {
                    problems.push(format!(
                        "sensor {} in set {set} is of unknown type {}",
                        s.id, s.kind
                    ));
                }
            }
        }
        for scenario in &lib.scenarios {
            for set in &scenario.sensors {
                if !lib.sensor_sets.contains_key(set) {
                    problems.push(format!("{} names an unknown sensor set {set}", scenario.id));
                }
            }
            if let Some(base) = &scenario.base {
                if !lib.scenarios.iter().any(|s| &s.id == base) {
                    problems.push(format!(
                        "{} derives from an unknown scenario {base}",
                        scenario.id
                    ));
                }
            } else if matches!(scenario.entities, Entities::Inherit(_)) {
                problems.push(format!(
                    "{} inherits entities and names no base",
                    scenario.id
                ));
            }
            for e in scenario.entities.lines() {
                let class = self.classes.classes.iter().find(|c| c.id == e.class);
                if class.is_none() {
                    problems.push(format!("{} uses an unknown class {}", scenario.id, e.class));
                }
                match self.platforms.iter().find(|p| p.id == e.platform) {
                    None => problems.push(format!(
                        "{} uses an unknown platform {}",
                        scenario.id, e.platform
                    )),
                    Some(p) if p.class != e.class => problems.push(format!(
                        "{}: platform {} is of class {}, not {}",
                        scenario.id, e.platform, p.class, e.class
                    )),
                    Some(_) => {}
                }
                if let Some(route) = e.route.as_deref().filter(|r| *r != "none") {
                    if !lib.routes.contains_key(route) {
                        problems.push(format!("{} uses an unknown route {route}", scenario.id));
                    }
                }
                for place in [e.launch.as_ref(), e.target.as_ref()].into_iter().flatten() {
                    if !lib.points.contains_key(place) && !lib.routes.contains_key(place) {
                        problems.push(format!("{} names an unknown place {place}", scenario.id));
                    }
                }
                if let Some(at) = &e.at {
                    if !point_ok(at) {
                        problems.push(format!(
                            "{} places an entity at an unknown point",
                            scenario.id
                        ));
                    }
                }
            }
        }
        for platform in &self.platforms {
            if !self.classes.classes.iter().any(|c| c.id == platform.class) {
                problems.push(format!(
                    "platform {} is of an unknown class {}",
                    platform.id, platform.class
                ));
            }
        }
        problems
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docs() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/test-tracks")
    }

    /// The committed library loads and every reference resolves. This is the test a
    /// docs change runs into when it renames a point, a class or a sensor type.
    #[test]
    fn the_plan_07_library_loads_and_its_references_resolve() {
        let lib = TrackLibrary::load(&docs()).expect("loads");
        assert_eq!(lib.scenarios.version, lib.classes.version);
        assert!(
            lib.scenarios.scenarios.len() >= 10,
            "{}",
            lib.scenarios.scenarios.len()
        );
        assert!(lib.platforms.len() >= 20, "{}", lib.platforms.len());
        assert!(!lib.sensors.types.is_empty());
        assert!(!lib.sensors.emission_map.is_empty());
        let problems = lib.unresolved();
        assert!(problems.is_empty(), "{problems:#?}");
        // TT-01 as the docs describe it: forty-five drones in four lines, a sample seed.
        let tt01 = lib
            .scenarios
            .scenarios
            .iter()
            .find(|s| s.id == "TT-01")
            .expect("TT-01");
        assert_eq!(
            tt01.entities
                .lines()
                .iter()
                .map(|e| e.count.as_i64_lossy())
                .sum::<i64>(),
            45
        );
        assert_eq!(tt01.sample.as_ref().map(|s| s.seed), Some(1701));
        assert!(tt01.entities.lines().iter().any(|e| e.decoy));
        // A derived scenario inherits its base's entities and must name a base that exists.
        let inheriting = lib
            .scenarios
            .scenarios
            .iter()
            .filter(|s| matches!(s.entities, Entities::Inherit(_)))
            .count();
        assert!(inheriting >= 2, "{inheriting}");
        // A YAML integer stays an integer, which the generator's output depends on.
        let long = lib
            .sensors
            .types
            .iter()
            .find(|t| t.id == "radar.long")
            .expect("radar.long");
        assert!(long.range_m["large"].is_int());
        assert!(!long.update_period_s.is_int());
        assert!(long.ea.skew_s.is_int());
    }

    #[test]
    fn a_dangling_reference_is_named() {
        let mut lib = TrackLibrary::load(&docs()).expect("loads");
        if let Entities::Lines(lines) = &mut lib.scenarios.scenarios[0].entities {
            lines[0].class = "air.imaginary".into();
        }
        let problems = lib.unresolved();
        assert!(
            problems.iter().any(|p| p.contains("air.imaginary")),
            "{problems:?}"
        );
    }

    #[test]
    fn a_missing_file_is_an_error_by_name() {
        let err = TrackLibrary::load(Path::new("nowhere-at-all")).expect_err("missing");
        assert!(err.to_string().contains("catalogue.yaml"), "{err}");
    }

    #[test]
    fn a_mapping_keeps_its_file_order() {
        let m: OrderedMap =
            yaml_serde::from_str("{t: 1500, kind: link_lost, node: KAL, until: 2160}")
                .expect("map");
        let keys: Vec<String> = m.iter().map(|(k, _)| k.as_text()).collect();
        assert_eq!(keys, ["t", "kind", "node", "until"]);
        assert_eq!(
            m.get("until").and_then(Scalar::as_num),
            Some(Num::Int(2160))
        );
    }
}
