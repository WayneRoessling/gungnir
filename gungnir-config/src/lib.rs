// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Configuration management, per docs/gungnir-capabilities.md §5.1. Sensors,
//! resources, thresholds, and the deployment backend are supplied here rather than
//! in code, so an operator can add or reconfigure a sensor, or point a desktop at a
//! service node (ARCHITECTURE.md §8), without a rebuild. Validation is mandatory
//! before a baseline is applied; a baseline written by a newer build is refused.

use gungnir_model::{
    AssetExtent, AssetId, AssetListView, AssetPriority, DefendedAsset, EffectorLayer, Geodetic,
    Magazine, PolicySettings, RelativeCost, ResourceId, ResourceView, Term, UiSettings,
    ValidityWindow, Vocabulary, WarningObligation,
};
use std::path::{Path, PathBuf};

/// Highest `ConfigBaseline::version` this build understands.
pub const SUPPORTED_CONFIG_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("validation failed: {0}")]
    Invalid(String),
    #[error("config version {found} is newer than this build supports ({supported})")]
    VersionTooNew { found: u32, supported: u32 },
    #[error("config I/O failed: {0}")]
    Io(String),
    #[error("config encoding failed: {0}")]
    Encoding(String),
    /// An authority rule names an action or a role this build does not know.
    ///
    /// Its own error rather than an `Invalid`, because it is the one validation failure
    /// that is **silent when it is not caught**: a misspelled action matches no request,
    /// so the rule grants nothing while reading, in the file, exactly like a grant
    /// (docs/design/DN-08-policy-configuration.md §6 rule 3).
    #[error("authority rule names an unknown {kind}: {name:?}")]
    UnknownAuthorityName { kind: &'static str, name: String },
    /// The baseline may be read, replayed and inspected, but not promoted now.
    ///
    /// DN-08 §5: expiry never changes a picture retroactively, so this is refused at the
    /// moment of promotion rather than by invalidating a baseline already in force.
    #[error(
        "baseline is outside its validity window at {now:?}: valid from {valid_from:?}{}",
        match valid_until { Some(u) => format!(" until {u:?}"), None => String::new() }
    )]
    NotPromotable {
        now: gungnir_model::MissionTime,
        valid_from: gungnir_model::MissionTime,
        valid_until: Option<gungnir_model::MissionTime>,
    },
    /// The candidate's `revision` does not advance past the one in force. Two
    /// promotions with one number would make "which baseline was this stamped with"
    /// unanswerable.
    #[error(
        "candidate revision {candidate} does not advance past revision {in_force} in force; \
         advance `revision` to promote"
    )]
    RevisionNotAdvanced { in_force: u32, candidate: u32 },
}

/// The action and role names **this build** knows, for checking an authority rule.
///
/// Supplied by the caller rather than read here: the canonical lists live in
/// `gungnir_security::actions::ALL` and `gungnir_security::Role::ALL`, and this crate may
/// not depend on `gungnir-security` (`ARCHITECTURE.md` §7.1). Inverting it this way is
/// what DN-22 §4 did for journal sealing -- the crate that owns the rule declares the
/// question, and the binary that can see both answers it. No new dependency edge.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KnownVocabulary {
    actions: Vec<String>,
    roles: Vec<String>,
}

impl KnownVocabulary {
    /// # Panics
    ///
    /// Never; the bounds are satisfied by any string iterators.
    pub fn new<A, R>(actions: A, roles: R) -> Self
    where
        A: IntoIterator,
        A::Item: Into<String>,
        R: IntoIterator,
        R::Item: Into<String>,
    {
        Self {
            actions: actions.into_iter().map(Into::into).collect(),
            roles: roles.into_iter().map(Into::into).collect(),
        }
    }

    /// True when this vocabulary lists nothing, which no deployment should use.
    ///
    /// A store built with an empty vocabulary would accept every misspelling, so
    /// [`validate_authority_names`] refuses rather than passing them all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty() || self.roles.is_empty()
    }
}

/// The reporting rhythm: what is produced on a cycle rather than on demand.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReportingConfig {
    #[serde(default)]
    pub scheduled: Vec<ScheduledProductConfig>,
    /// How many sessions the cross-session products reach back over (DN-19 §5, DN-21
    /// §6; GAP-025). At least one.
    #[serde(default = "default_retention_sessions")]
    pub retention_sessions: usize,
}

fn default_retention_sessions() -> usize {
    10
}

impl Default for ReportingConfig {
    fn default() -> Self {
        Self {
            scheduled: Vec::new(),
            retention_sessions: default_retention_sessions(),
        }
    }
}

/// One scheduled product, as it appears in the baseline.
///
/// `kind` is a string here and an enum in `gungnir-model`, the same split
/// `AssetConfig::priority` uses: the schema stays readable and the unknown spellings are
/// refused by validation rather than defaulted to something plausible.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ScheduledProductConfig {
    pub name: String,
    /// "handover-summary", "situation-report" or "measures-summary".
    pub kind: String,
    pub period_s: f64,
    #[serde(default)]
    pub offset_s: f64,
    /// Names an entry in `endpoints`. **Absent means the product is produced and held
    /// for a person to read**, which is a real configuration and not a failure.
    #[serde(default)]
    pub deliver_to: Option<String>,
}

impl ScheduledProductConfig {
    /// Parses the baseline's spelling; `None` for anything unrecognized, which
    /// validation rejects rather than defaulting.
    #[must_use]
    pub fn parse_kind(kind: &str) -> Option<gungnir_model::ProductKind> {
        match kind.trim().to_ascii_lowercase().as_str() {
            "handover-summary" => Some(gungnir_model::ProductKind::HandoverSummary),
            "situation-report" => Some(gungnir_model::ProductKind::SituationReport),
            "measures-summary" => Some(gungnir_model::ProductKind::MeasuresSummary),
            _ => None,
        }
    }

    /// The runtime product, or `None` when the kind does not parse.
    ///
    /// Returns rather than defaulting: validation refuses an unknown kind at load, and a
    /// caller that reached here with one should produce nothing rather than the wrong
    /// thing.
    #[must_use]
    pub fn to_product(&self) -> Option<gungnir_model::ScheduledProduct> {
        Some(gungnir_model::ScheduledProduct {
            name: self.name.clone(),
            kind: Self::parse_kind(&self.kind)?,
            schedule: gungnir_model::Schedule {
                period_s: self.period_s,
                offset_s: self.offset_s,
            },
            deliver_to: self.deliver_to.clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SensorConfig {
    pub id: u32,
    /// e.g. "radar", "eo-ir", "ads-b", "lidar"; matched by `gungnir-ingest` adapters.
    pub modality: String,
    /// Geodetic `[lat_rad, lon_rad, alt_m]` of the sensor.
    pub position: [f64; 3],
    /// Nominal detection range, meters; used by `gungnir-sensor-management` coverage.
    #[serde(default = "default_sensor_range_m")]
    pub max_range_m: f64,
    /// The endpoint a command for this sensor goes to (DN-11 §6, GAP-004).
    ///
    /// Names an entry in `endpoints`. **Absent means the sensor is not controllable
    /// from here**, which is DN-11 §5 rule 4 and is the state of every sensor until
    /// GAP-001 brings the adapters. Absent is not a defect to be defaulted away: a
    /// sensor with an invented endpoint would appear commandable and silently fail.
    #[serde(default)]
    pub control_endpoint: Option<String>,
    /// Planned downtime for this sensor (DN-21 §6, GAP-054).
    ///
    /// Empty means no maintenance is planned, which is not the same as "this sensor
    /// never goes down": an unplanned outage is a failure and is reported as one.
    #[serde(default)]
    pub maintenance: Vec<MaintenanceWindowConfig>,
}

/// One planned maintenance window, as it appears in the baseline.
///
/// Times are mission-time seconds, like every other time in this schema, so a replayed
/// session sees the same windows open and close at the same moments.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MaintenanceWindowConfig {
    pub from_s: f64,
    pub to_s: f64,
    /// Why the sensor is down. **Required**: a window with no reason is a gap in the
    /// coverage picture that nobody can account for at handover.
    pub reason: String,
}

impl MaintenanceWindowConfig {
    /// The registry's window for `sensor`, starting in `Planned`.
    #[must_use]
    pub fn to_window(&self, sensor: gungnir_model::SensorId) -> gungnir_model::MaintenanceWindow {
        gungnir_model::MaintenanceWindow {
            sensor,
            from: gungnir_model::MissionTime(self.from_s),
            to: gungnir_model::MissionTime(self.to_s),
            reason: self.reason.clone(),
            state: gungnir_model::MaintenanceState::Planned,
        }
    }
}

fn default_sensor_range_m() -> f64 {
    50_000.0
}

/// A taskable resource the intercept planner may assign.
///
/// `layer` is mandatory and unparsed here: `validate` rejects a baseline whose layer
/// is missing or unrecognized, because MOE-03 is defined by it
/// (docs/design/DN-04-effector-model.md §6).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ResourceConfig {
    /// The endpoint (by name, from `endpoints`) a handoff for this resource is posted
    /// to (DN-07 §6, GAP-040). Absent means manual delivery -- a radio call -- which
    /// the record says rather than pretending an automated handoff happened.
    #[serde(default)]
    pub handoff_endpoint: Option<String>,
    pub id: u32,
    /// Geodetic `[lat_rad, lon_rad, alt_m]`.
    pub position: [f64; 3],
    /// Max simultaneous tracks this resource can be tasked against.
    pub capacity: u32,
    /// Defence layer: "area", "point", "self-defence", or "non-kinetic".
    pub layer: String,
    /// Relative cost of one round; absent means no preference expressed.
    #[serde(default)]
    pub cost: Option<f64>,
    #[serde(default)]
    pub rounds_available: Option<u32>,
    #[serde(default)]
    pub reserve: Option<u32>,
    /// Closing speed in metres per second, for the intercept geometry
    /// (docs/design/DN-04-effector-model.md §9, amendment 1; GAP-031). Optional: absent
    /// means no geometry can be solved for this resource, which the solver reports.
    #[serde(default)]
    pub intercept_speed_mps: Option<f64>,
}

impl ResourceConfig {
    /// The runtime view; resources start `ready` until `gungnir-sensor-management`
    /// or an operator marks them otherwise.
    ///
    /// An unrecognized layer cannot reach here: `validate` rejects the baseline
    /// first. If one does, this reports the outermost layer, which is the most
    /// conservative reading rather than the most permissive, and the resource is
    /// still subject to every policy check.
    pub fn to_view(&self) -> ResourceView {
        ResourceView {
            id: ResourceId(self.id),
            position: Geodetic {
                lat_rad: self.position[0],
                lon_rad: self.position[1],
                alt_m: self.position[2],
            },
            capacity: self.capacity,
            ready: true,
            layer: EffectorLayer::parse(&self.layer).unwrap_or(EffectorLayer::Area),
            cost: self.cost.map_or_else(RelativeCost::default, RelativeCost),
            magazine: match (self.rounds_available, self.reserve) {
                (Some(rounds_available), reserve) => Some(Magazine {
                    rounds_available,
                    reserve: reserve.unwrap_or(0),
                }),
                (None, _) => None,
            },
            intercept_speed_mps: self.intercept_speed_mps,
        }
    }
}

/// A peer node consumed as a source (DN-16 §6, GAP-009). The quality is ours to assign
/// and the age is ours to bound; nothing the peer sends changes either.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PeerConfig {
    pub name: String,
    /// An entry in the endpoint table.
    pub endpoint: String,
    /// The identifier the gateway admits this peer's tracks under; no sensor's.
    pub source_id: u32,
    pub assigned_quality: f32,
    pub max_age_s: f64,
}

/// Who a client certificate speaks for (D-02; GAP-002, GAP-009, GAP-040).
///
/// The certificate's subject common name is the party the transport establishes; this
/// table says what that party is to the deployment. A sensor submits detections for its
/// own id and no other; an effector reports on handoffs; a warned party acknowledges the
/// warnings sent to its channel; a peer is the partner named in `peers`. Public material
/// only: a name and a role, never a certificate.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MachineIdentityConfig {
    pub common_name: String,
    pub speaks_for: MachineRole,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum MachineRole {
    Sensor {
        sensor_id: u32,
    },
    Effector {
        endpoint: String,
    },
    /// The party a warning is owed to, named by the `warning` endpoint its obligations
    /// use (GAP-042, DN-03 §5 rule 2; D-08 for endpoints).
    ///
    /// Kept apart from [`MachineRole::Effector`] although both name a row in `endpoints`,
    /// because an effector acts on a decision and a warned party is told about a threat.
    /// One certificate speaking for both would be able to report an engagement it was
    /// never handed, and the refusal messages on those routes would stop being true.
    WarnedParty {
        channel: String,
    },
    Peer {
        peer: String,
    },
}

/// One ASTERIX radar feed: a socket and the radars it may speak for (GAP-001,
/// `docs/design/handoff-2026-09-06-radar-feed.md` §4 step 1).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RadarFeedConfig {
    pub name: String,
    /// `ip:port` to bind, e.g. `0.0.0.0:8600`.
    pub bind_addr: String,
    #[serde(default)]
    pub multicast: Option<MulticastConfig>,
    pub radars: Vec<RadarBindingConfig>,
}

/// One AIS receiver feed (GAP-010, D-32): the receiver's sensor identity and where its
/// NMEA sentences come from.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AisFeedConfig {
    pub name: String,
    /// The receiver in the sensor list; its detections carry this identity.
    pub sensor_id: u32,
    pub source: AisSource,
}

/// Where an AIS feed's sentences come from.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum AisSource {
    /// A receiver serving NMEA over TCP, `host:port`.
    Tcp { addr: String },
    /// A recording of sentences, one per line.
    File { path: String },
}

/// One 1090ES ADS-B receiver feed (GAP-010): the receiver's sensor identity and where
/// its AVR-format frames come from.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AdsbFeedConfig {
    pub name: String,
    /// The receiver in the sensor list; its detections carry this identity.
    pub sensor_id: u32,
    pub source: AdsbSource,
}

/// Where an ADS-B feed's AVR lines come from. Same shape as [`AisSource`], kept as its
/// own type because the two feeds are configured under their own names in the baseline
/// and a reader should not have to know they happen to share a representation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum AdsbSource {
    /// A receiver serving AVR frames over TCP, `host:port` (`dump1090` and its
    /// relatives' AVR port).
    Tcp { addr: String },
    /// A recording of AVR lines, one per line.
    File { path: String },
}

/// One SAPIENT edge-node feed (GAP-001): a spotter, an acoustic array, or a passive-RF
/// direction finder, all the same adapter and the same message shape
/// (`gungnir_ingest::adapters::sapient`) and told apart only by which node type this
/// feed is configured to accept.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SapientFeedConfig {
    pub name: String,
    /// The node in the sensor list; its siting (`SensorConfig::position`) is where the
    /// adapter places a bearing's origin, exactly as a radar's position places its
    /// range/azimuth/elevation reports.
    pub sensor_id: u32,
    pub node_type: SapientNodeType,
    pub source: SapientSource,
}

/// Which of the three node types this feed is configured to accept. Deliberately one
/// per feed rather than a set: a real deployment binds one adapter instance per feed
/// and each feed is exactly one kind of node
/// (`gungnir_ingest::adapters::sapient::ALL_ACCEPTED_NODE_TYPES`'s own documentation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SapientNodeType {
    Spotter,
    Acoustic,
    PassiveRf,
}

/// Where a SAPIENT feed's protobuf-JSON lines come from. Same shape as [`AisSource`]
/// and [`AdsbSource`]; kept as its own type for the same reason.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SapientSource {
    /// A middleware serving the protobuf-JSON mapping over TCP, `host:port`
    /// (`gungnir_ingest::adapters::sapient`'s own documentation on why JSON and not
    /// the binary wire format).
    Tcp { addr: String },
    /// A recording of messages, one per line.
    File { path: String },
}

/// A multicast group joined on an interface, both IPv4 dotted quads.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MulticastConfig {
    pub group: String,
    pub interface: String,
}

/// Which sensor an ASTERIX SAC/SIC pair is; the position comes from that sensor.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RadarBindingConfig {
    pub sensor_id: u32,
    pub sac: u8,
    pub sic: u8,
}

/// The terrain a deployment masks line of sight against (GAP-023).
///
/// `frame` says what the file's coordinates are, and today the only value is
/// `"local-enu"`: the DEM was prepared in the deployment's local frame (metres east and
/// north of `origin`), because converting a projected or geographic DEM needs a
/// projection library the approved stack does not hold. A file whose own tags contradict
/// that is refused at load, by name.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TerrainConfig {
    /// An ESRI ASCII grid (`.asc`) or a `GeoTIFF` (`.tif`, `.tiff`).
    pub path: String,
    #[serde(default = "default_terrain_frame")]
    pub frame: String,
}

fn default_terrain_frame() -> String {
    "local-enu".to_string()
}

/// A defended asset as the baseline states it (docs/design/DN-01-defended-assets.md).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AssetConfig {
    pub id: u32,
    pub name: String,
    /// Geodetic `[lat_rad, lon_rad, alt_m]`.
    pub position: [f64; 3],
    /// Present for an area asset; absent for a point.
    #[serde(default)]
    pub radius_m: Option<f64>,
    /// "low", "medium", "high", or "critical". An unknown value is rejected.
    pub priority: String,
    #[serde(default)]
    pub warning_lead_time_s: Option<f64>,
    #[serde(default)]
    pub warning_channel: Option<String>,
    /// Metres of closest approach that trigger the warning without a predicted impact
    /// (DN-03 amendment 1). Needs the obligation's other two fields.
    #[serde(default)]
    pub warning_within_m: Option<f64>,
    #[serde(default)]
    pub note: Option<String>,
}

impl AssetConfig {
    /// The runtime asset. Only called after `validate` has accepted the baseline,
    /// so the priority parses; an unparsed one yields the default rather than
    /// panicking, and validation is what stops that ever being reached.
    pub fn to_asset(&self) -> DefendedAsset {
        let center = Geodetic {
            lat_rad: self.position[0],
            lon_rad: self.position[1],
            alt_m: self.position[2],
        };
        DefendedAsset {
            id: AssetId(self.id),
            name: self.name.clone(),
            extent: match self.radius_m {
                Some(radius_m) => AssetExtent::Circle { center, radius_m },
                None => AssetExtent::Point { position: center },
            },
            priority: AssetPriority::parse(&self.priority).unwrap_or_default(),
            warning: match (self.warning_lead_time_s, self.warning_channel.as_ref()) {
                (Some(lead_time_s), Some(channel)) => Some(WarningObligation {
                    lead_time_s,
                    channel: channel.clone(),
                    within_m: self.warning_within_m,
                }),
                _ => None,
            },
            note: self.note.clone(),
        }
    }
}

/// Assessment settings (docs/design/DN-02-prediction-and-approach.md).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AssessmentConfig {
    /// Seconds ahead the picture predicts to, ascending. A prediction beyond the
    /// last horizon is not produced; it is never clamped and presented as if it
    /// had been computed.
    #[serde(default = "default_prediction_horizons_s")]
    pub prediction_horizons_s: Vec<f64>,
    /// Range at and beyond which a track scores zero against any asset.
    #[serde(default = "default_assessment_max_range_m")]
    pub max_range_m: f64,
    /// Seconds after commitment within which an effect is expected, per effector
    /// layer (docs/design/DN-06-engagement-and-effect.md). An engagement whose
    /// window closes with nothing observed is indeterminate, never a success or a
    /// failure.
    #[serde(default)]
    pub effect_window_s: std::collections::BTreeMap<gungnir_model::EffectorLayer, f64>,
    /// Lethality by platform class (GAP-027), a factor on the risk score beside the
    /// affiliation's. Keyed by the class a cooperative source declares (an AIS ship type
    /// maps to `surface.*`), or the catalogue's kinematic class when a classifier
    /// exists. A class not listed weighs 1.0: unknown is not harmless, and a table that
    /// silently zeroed an unlisted class would hide a threat. Values are non-negative.
    #[serde(default)]
    pub lethality_by_class: std::collections::BTreeMap<String, f64>,
}

fn default_prediction_horizons_s() -> Vec<f64> {
    vec![10.0, 30.0, 60.0, 120.0]
}

fn default_assessment_max_range_m() -> f64 {
    50_000.0
}

impl Default for AssessmentConfig {
    fn default() -> Self {
        Self {
            prediction_horizons_s: default_prediction_horizons_s(),
            max_range_m: default_assessment_max_range_m(),
            effect_window_s: std::collections::BTreeMap::new(),
            lethality_by_class: std::collections::BTreeMap::new(),
        }
    }
}

/// An external party the deployment may send to: a warning channel, an effector, a
/// peer. Generic by decision D-08, so an adapter is configuration rather than code.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EndpointConfig {
    /// Referenced by name from a warning obligation, a resource handoff, or a peer.
    pub name: String,
    /// e.g. "warning", "handoff", "peer".
    pub kind: String,
    pub address: String,
}

/// One candidate algorithm configuration, as it appears in the baseline (DN-24 §4).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TrackingProfileConfig {
    /// Names an entry in `mission_profiles`.
    pub profile: String,
    /// Unique within the profile. **This is what a promotion and a rollback name**, and
    /// the registry could not tell two candidates apart without it.
    pub name: String,
    pub filter_selection: String,
    pub gate_threshold: f64,
    /// The coordinated-turn mode's fixed turn rate, radians/second (DN-28 §5). Ignored,
    /// and not validated, unless `filter_selection` is `"imm-cv-ct"` -- see `validate`.
    #[serde(default)]
    pub imm_turn_rate_rad_s: f64,
    /// Row-major over `[constant-velocity, coordinated-turn]`, each row summing to one
    /// (DN-28 §5). Ignored unless `filter_selection` is `"imm-cv-ct"`.
    #[serde(default)]
    pub imm_mode_transition: [[f64; 2]; 2],
    /// Over the same order, summing to one (DN-28 §5). Ignored unless `filter_selection`
    /// is `"imm-cv-ct"`.
    #[serde(default)]
    pub imm_initial_mode_probabilities: [f64; 2],
    /// Measurement-noise variance per axis, m² (east, north, height) -- DN-30 §5.
    /// Defaults to `PipelineSettings::default()`'s own figure so a baseline written
    /// before this field existed keeps behaving exactly as it did; a deployment names
    /// its actual sensor's own variance to correct the mismatch DN-28 §7 found.
    #[serde(default = "default_measurement_noise_var")]
    pub measurement_noise_var: [f64; 3],
    /// The one candidate per profile that is in force when the console opens.
    ///
    /// Exactly one per declared profile: zero means the deployment cannot say what is
    /// running, and two means it cannot say either.
    #[serde(default)]
    pub promoted: bool,
    /// What validated it, for the review that asks.
    ///
    /// Free text: this build has no evidence store, and a structured field nothing
    /// populates would claim more than it holds.
    #[serde(default)]
    pub validated_by: Option<String>,
}

/// A candidate resolved from the baseline, whichever way it was written.
///
/// The point of this type is that a caller never has to know whether the deployment used
/// `tracking` or `tracking_profiles`: [`ConfigBaseline::algorithm_candidates`] answers the
/// same shape for both.
#[derive(Debug, Clone, PartialEq)]
pub struct AlgorithmCandidate {
    pub id: gungnir_model::AlgorithmBaselineId,
    pub config: TrackingConfig,
    pub promoted: bool,
    pub validated_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TrackingConfig {
    /// e.g. "imm-cv-ct"; resolved by `gungnir-modelops` against its registry.
    pub filter_selection: String,
    /// Chi-square gate threshold; must be finite and positive.
    pub gate_threshold: f64,
    /// The `"imm-cv-ct"` selection's own fields (DN-28 §5); see
    /// [`TrackingProfileConfig`]'s fields of the same names for what each means and
    /// when it is validated.
    #[serde(default)]
    pub imm_turn_rate_rad_s: f64,
    #[serde(default)]
    pub imm_mode_transition: [[f64; 2]; 2],
    #[serde(default)]
    pub imm_initial_mode_probabilities: [f64; 2],
    /// Measurement-noise variance per axis, m² (east, north, height) -- DN-30 §5. See
    /// [`TrackingProfileConfig::measurement_noise_var`] for what it means and its
    /// default.
    #[serde(default = "default_measurement_noise_var")]
    pub measurement_noise_var: [f64; 3],
}

/// `PipelineSettings::default()`'s own figure (DN-30), so a baseline predating this
/// field is read as exactly what it already meant rather than as a silent change in
/// behaviour.
fn default_measurement_noise_var() -> [f64; 3] {
    [400.0, 400.0, 900.0]
}

/// Which services-layer backend the desktop uses (ARCHITECTURE.md §8.2).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum BackendConfig {
    /// Services run inside the desktop process (disconnected profile).
    #[default]
    Embedded,
    /// Services run on a `gungnir-node` reached at `endpoint` (connected profiles).
    Remote { endpoint: String },
}

/// How a deployment holds key material (DN-22, GAP-084).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SecurityConfig {
    #[serde(default)]
    pub key_provider: KeyProviderConfig,
    /// How operators sign in (DN-23 §6, GAP-057). Names the provider and its
    /// parameters; **never a credential**, and validation refuses a value that looks like
    /// one.
    #[serde(default)]
    pub authentication: AuthenticationConfig,
    /// What the desktop's endpoint client trusts (GAP-060). Public certificates
    /// inline as PEM: not key material, and inline rather than by path so no path to
    /// anything sits in the baseline. Empty means the platform's store, which the
    /// health line reports as unpinned.
    #[serde(default)]
    pub tls: TlsClientConfig,
    /// Escrow (DN-22 §11, GAP-084): the security officer and the public key journal keys
    /// are wrapped to. **The public half, inline as PEM**; validation refuses a private
    /// key without repeating it. Absent means no escrow, which PN-09 reports.
    #[serde(default)]
    pub escrow: Option<EscrowConfig>,
}

/// The escrow holder and key (DN-22 §11).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EscrowConfig {
    /// The security officer's operator identifier: an account like any other, and the
    /// only one that may recover.
    pub holder: u64,
    pub public_key_pem: String,
}

/// The endpoint client's trust roots (GAP-060).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct TlsClientConfig {
    #[serde(default)]
    pub trust_roots_pem: Vec<String>,
}

/// The operator authentication provider (DN-23 §5, one mechanism per profile).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AuthenticationConfig {
    #[serde(default)]
    pub provider: AuthenticationProvider,
    /// How long a session lasts, in seconds; absent means until sign-out or shutdown,
    /// which is the disconnected desktop's rule.
    #[serde(default)]
    pub session_lifetime_s: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum AuthenticationProvider {
    /// Nobody can sign in; the desktop runs role-selected and attributes nothing.
    #[default]
    None,
    /// Local accounts in a JSON file of `{operator, role, phc}` records, relative to the
    /// data directory. The file holds hashed passphrases (PHC strings), never plaintext.
    LocalAccounts { accounts_path: String },
}

/// Which custody model a deployment uses.
///
/// DN-22 §5 gives the three profiles genuinely different answers, so this is an enum over
/// them rather than a path to a file. **None of the variants carries key material**: each
/// names where material lives, and the provider goes and gets it.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum KeyProviderConfig {
    /// No custody, so nothing is encrypted and the system says so.
    ///
    /// The default, and the honest one: a deployment that has not set up a keystore is
    /// not encrypting, and reporting `EncryptionStatus::NotConfigured` is better than a
    /// health flag claiming protection nobody arranged.
    #[default]
    None,
    /// Keys generated at start-up and held in process memory.
    ///
    /// **Journals sealed under it cannot be read after a restart**, because the key is
    /// gone -- so it is for tests and demonstrations and validation refuses it in a
    /// deployed profile. Named `Ephemeral` rather than `InProcess` because the property
    /// that matters is not where the key lives but how long.
    Ephemeral,
    /// One file in the data directory, sealed under a key derived from the operator's
    /// passphrase and unlocked at sign-in (DN-22 amendment 3, §12; GAP-084). No path
    /// here: the file's name is fixed. Before the first sign-in the desktop journals in
    /// the clear and says so.
    PassphraseSealedFile,
    /// The operating system's keystore on this machine, unlocked at operator login
    /// (DN-22 §5, the disconnected desktop).
    OperatingSystemKeystore { account: String },
    /// A managed key service, off-host, which seals and signs and never releases
    /// material (DN-22 §5, the cloud node).
    ManagedService { endpoint: String, key_ring: String },
}

impl KeyProviderConfig {
    /// Whether a provider for this exists yet.
    ///
    /// The two persistent profiles are designed and unbuilt. Saying so at validation
    /// means a deployment learns it at start-up rather than discovering an unencrypted
    /// journal later.
    #[must_use]
    pub fn is_implemented(&self) -> bool {
        matches!(
            self,
            KeyProviderConfig::None
                | KeyProviderConfig::Ephemeral
                | KeyProviderConfig::PassphraseSealedFile
        )
    }

    /// The register entry that will build this one.
    #[must_use]
    pub fn owning_gap(&self) -> Option<&'static str> {
        match self {
            KeyProviderConfig::None
            | KeyProviderConfig::Ephemeral
            | KeyProviderConfig::PassphraseSealedFile => None,
            KeyProviderConfig::OperatingSystemKeystore { .. }
            | KeyProviderConfig::ManagedService { .. } => Some("GAP-084"),
        }
    }

    /// Every string this configuration carries, for the key-material check.
    fn values(&self) -> Vec<&str> {
        match self {
            KeyProviderConfig::None
            | KeyProviderConfig::Ephemeral
            | KeyProviderConfig::PassphraseSealedFile => Vec::new(),
            KeyProviderConfig::OperatingSystemKeystore { account } => vec![account.as_str()],
            KeyProviderConfig::ManagedService { endpoint, key_ring } => {
                vec![endpoint.as_str(), key_ring.as_str()]
            }
        }
    }
}

/// Settings for the headless service node (ARCHITECTURE.md §8.1).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NodeConfig {
    /// Address `gungnir-api` binds to.
    ///
    /// **Loopback by default, and only loopback is served** (GAP-041). There is no TLS
    /// yet -- encryption in transit is GAP-060, which waits on GAP-084's key custody --
    /// and `gungnir_api::transport::serve` refuses anything routable rather than
    /// carrying command-and-control traffic in plaintext. A deployment that sets a
    /// routable address gets a node that runs its pipeline and journals it, and serves
    /// nobody, with the reason in the log.
    pub bind_addr: String,
    /// Directory for the `gungnir-store` journal.
    pub data_dir: String,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1:7410".into(),
            data_dir: "./gungnir-journal".into(),
        }
    }
}

/// The complete, versioned configuration baseline. Fields added after the first
/// draft carry `#[serde(default)]` so older files still load.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigBaseline {
    /// The **schema** version: which shape of file this is (`SUPPORTED_CONFIG_VERSION`).
    /// It says nothing about how current the content is; that is `revision`.
    pub version: u32,
    /// The deployment's revision of this baseline's **content**, advanced by whoever
    /// edits it. `ConfigStore::apply` refuses a candidate that does not advance past the
    /// revision in force, so two promotions can never carry the same number. This is
    /// what the asset list and the hazard layer are stamped with (DN-01 amendment 1,
    /// DN-14 amendment 1): a survey dated by its schema version was dated by nothing.
    #[serde(default)]
    pub revision: u32,
    #[serde(default)]
    pub sensors: Vec<SensorConfig>,
    /// ASTERIX radar feeds bound at start (GAP-001). Empty means no live radar.
    #[serde(default)]
    pub radar_feeds: Vec<RadarFeedConfig>,
    /// AIS receiver feeds bound at start (GAP-010). Empty means no cooperative source.
    #[serde(default)]
    pub ais_feeds: Vec<AisFeedConfig>,
    /// ADS-B receiver feeds bound at start (GAP-010). Empty means no cooperative source.
    #[serde(default)]
    pub adsb_feeds: Vec<AdsbFeedConfig>,
    /// SAPIENT edge-node feeds bound at start (GAP-001). Empty means no spotter,
    /// acoustic, or passive-RF source.
    #[serde(default)]
    pub sapient_feeds: Vec<SapientFeedConfig>,
    /// Peer nodes consumed as sources (DN-16 §6, GAP-009).
    #[serde(default)]
    pub peers: Vec<PeerConfig>,
    /// What may be exchanged with which party (DN-18 §6, GAP-065). Empty means no
    /// exchange: a party that authenticates and has no agreement can do nothing.
    #[serde(default)]
    pub exchange: Vec<gungnir_model::ExchangeAgreement>,
    /// Client certificates this deployment recognises and what each speaks for (D-02;
    /// GAP-002, GAP-009, GAP-040). Empty means a certificate authenticates a party and
    /// nothing more, which the exchange agreements alone then govern.
    #[serde(default)]
    pub machine_identities: Vec<MachineIdentityConfig>,
    #[serde(default)]
    pub resources: Vec<ResourceConfig>,
    /// The laydowns this deployment can choose between
    /// (`docs/design/DN-26-laydown-options.md` §4, GAP-087).
    ///
    /// **Empty is valid and means the deployment has no alternatives declared, which is a
    /// different statement from having one**: an empty table says "none offered", a
    /// one-row table would say "here are your options" and be lying. Defaulting to empty
    /// also leaves every configuration file written before this field existed valid, and
    /// describing a deployment with no declared alternatives is the honest reading of a
    /// file that does not mention laydowns (DN-26 §8).
    #[serde(default)]
    pub laydowns: Vec<gungnir_model::Laydown>,
    #[serde(default)]
    pub tracking: Option<TrackingConfig>,
    #[serde(default)]
    pub backend: BackendConfig,
    #[serde(default)]
    pub node: Option<NodeConfig>,
    /// Planning horizon (steps) for the Bellman/DP allocator; at least 1.
    #[serde(default = "default_horizon")]
    pub allocation_horizon: usize,
    /// Directory for the desktop's local `gungnir-store` journal.
    #[serde(default = "default_data_dir")]
    pub data_dir: String,
    /// Defended assets (docs/design/DN-01-defended-assets.md). Empty means no list
    /// is configured, which assessment reports rather than scoring everything zero.
    #[serde(default)]
    pub assets: Vec<AssetConfig>,
    /// External parties this deployment may send to (decision D-08).
    #[serde(default)]
    pub endpoints: Vec<EndpointConfig>,
    /// How this deployment decides (docs/design/DN-08-policy-configuration.md).
    /// Every absent setting resolves to the strictest reading.
    #[serde(default)]
    pub policy: PolicySettings,
    /// Prediction horizons and scoring range (docs/design/DN-02-prediction-and-approach.md).
    #[serde(default)]
    pub assessment: AssessmentConfig,
    /// The DEM to mask line of sight against, if any (GAP-023). Absent means flat
    /// terrain, which the coverage report marks as optimistic.
    #[serde(default)]
    pub terrain: Option<TerrainConfig>,
    /// Per-role window arrangement (D-17, GAP-075). Absent means every role uses the
    /// default order `gungnir_workflow::WorkspaceLayout::for_role` produces.
    #[serde(default)]
    pub ui: UiSettings,
    /// Display vocabulary overrides (D-12, GAP-070). Absent means the NATO and joint
    /// terms of `docs/mission/glossary.md`.
    #[serde(default)]
    pub vocabulary: Vocabulary,
    /// Coverage analysis settings (DN-12 §6, GAP-006).
    #[serde(default)]
    pub analytics: AnalyticsConfig,
    /// How long a sensor task waits for an acknowledgement (DN-11 §6, GAP-004).
    #[serde(default = "default_sensor_task_ack_window_s")]
    pub sensor_task_ack_window_s: f64,
    /// Approach axes coverage is reported along (DN-12 §5, GAP-006).
    ///
    /// DN-12 leaves where these come from a mission question and says the caller
    /// supplies them; this is the deployment declaring them, as DN-01 does for the
    /// defended-asset list. Empty means none are declared, and gap detection reports
    /// that rather than inventing an axis to measure against.
    #[serde(default)]
    pub approaches: Vec<ApproachConfig>,
    /// Geodetic anchor of the local ENU frame, as `[lat_rad, lon_rad, alt_m]`
    /// (GAP-007).
    ///
    /// Every ENU coordinate in the system -- `DetectionView::measurement`,
    /// `TrackView::state` -- is relative to this. Absent means the deployment has not
    /// declared one, and there is deliberately no default: guessing an origin from the
    /// first sensor or the mean of the assets would place every geodetic thing
    /// somewhere plausible and wrong, which on a map is worse than placing it nowhere.
    /// Anything that needs both frames says it cannot instead.
    #[serde(default)]
    pub origin: Option<[f64; 3]>,
    /// Booms, nets, barriers, wrecks and shoals (DN-14 §6, GAP-017).
    ///
    /// **Descriptive, never a rule.** A geofence says where we may act; a hazard says what
    /// is there. Nothing in the policy chain reads this list, and a test guards that.
    /// Empty means the deployment declared none, which the picture says rather than
    /// showing a clear harbour.
    #[serde(default)]
    pub hazards: Vec<HazardConfig>,
    /// Geofences (GAP-088): circular areas that are **rules** -- a `no_go` fence denies an
    /// intercept inside it (`DenialReason::NoGoGeofence`). Distinct from `hazards`, which
    /// are survey facts and never rules. Empty means none are declared, and both binaries
    /// say so at start rather than letting the geofence engine pass in silence.
    #[serde(default)]
    pub geofences: Vec<GeofenceConfig>,
    /// When this baseline is valid. Absent means always.
    #[serde(default)]
    pub validity: Option<ValidityWindow>,
    /// Products this deployment produces on a cycle (DN-21 §6, GAP-054).
    #[serde(default)]
    pub reporting: ReportingConfig,
    /// The operating contexts this deployment declares (DN-24 §4, GAP-086).
    ///
    /// Declared by name, the way endpoints are, and referenced by name from the candidates
    /// that belong to them. Empty means the deployment runs one unnamed configuration,
    /// which `algorithm_candidates` reads as the implicit `default` profile.
    #[serde(default)]
    pub mission_profiles: Vec<String>,
    /// Candidate algorithm configurations, one of which is promoted per profile.
    ///
    /// **Mutually exclusive with `tracking`**: both present is two answers to what is in
    /// force.
    #[serde(default)]
    pub tracking_profiles: Vec<TrackingProfileConfig>,
    /// Which profile this deployment is operating in.
    ///
    /// Absent with one profile declared means that one; absent with several is refused
    /// rather than guessed.
    #[serde(default)]
    pub active_profile: Option<String>,
    /// How this deployment holds key material (DN-22 §6, GAP-084).
    ///
    /// **Names a provider and never a secret.** Validation rejects a value that looks
    /// like key material, so a pasted key is refused rather than stored in a file that
    /// is version-controlled, hand-edited and copied between machines.
    #[serde(default)]
    pub security: SecurityConfig,
}

fn default_horizon() -> usize {
    10
}

fn default_data_dir() -> String {
    "./gungnir-journal".into()
}

impl Default for ConfigBaseline {
    fn default() -> Self {
        Self {
            version: SUPPORTED_CONFIG_VERSION,
            reporting: ReportingConfig::default(),
            mission_profiles: Vec::new(),
            tracking_profiles: Vec::new(),
            active_profile: None,
            revision: 0,
            sensors: Vec::new(),
            radar_feeds: Vec::new(),
            ais_feeds: Vec::new(),
            adsb_feeds: Vec::new(),
            sapient_feeds: Vec::new(),
            peers: Vec::new(),
            exchange: Vec::new(),
            machine_identities: Vec::new(),
            resources: Vec::new(),
            // No alternatives declared, which is what a deployment that has not
            // described any actually has (DN-26 §8).
            laydowns: Vec::new(),
            tracking: None,
            backend: BackendConfig::Embedded,
            node: None,
            allocation_horizon: default_horizon(),
            data_dir: default_data_dir(),
            assets: Vec::new(),
            endpoints: Vec::new(),
            policy: PolicySettings::default(),
            assessment: AssessmentConfig::default(),
            terrain: None,
            ui: UiSettings::default(),
            vocabulary: Vocabulary::default(),
            analytics: AnalyticsConfig::default(),
            sensor_task_ack_window_s: default_sensor_task_ack_window_s(),
            approaches: Vec::new(),
            hazards: Vec::new(),
            geofences: Vec::new(),
            origin: None,
            validity: None,
            security: SecurityConfig::default(),
        }
    }
}

impl ConfigBaseline {
    pub fn resource_views(&self) -> Vec<ResourceView> {
        self.resources.iter().map(ResourceConfig::to_view).collect()
    }

    /// The asset list as the picture sees it, stamped with this baseline's version
    /// so a score can be traced to the list that produced it.
    pub fn asset_list(&self) -> AssetListView {
        AssetListView {
            baseline_version: self.revision,
            assets: self.assets.iter().map(AssetConfig::to_asset).collect(),
        }
    }

    /// Every candidate algorithm configuration, however the baseline wrote it (DN-24 §6).
    ///
    /// **A baseline carrying only `tracking` yields one implicit `default` profile with
    /// that configuration promoted**, which is exactly what such a deployment means today.
    /// A baseline carrying neither yields nothing -- no algorithm configuration is in
    /// force, which is the state of every default deployment and is reported rather than
    /// defaulted to a guess.
    #[must_use]
    pub fn algorithm_candidates(&self) -> Vec<AlgorithmCandidate> {
        if !self.tracking_profiles.is_empty() {
            return self
                .tracking_profiles
                .iter()
                .map(|c| AlgorithmCandidate {
                    id: gungnir_model::AlgorithmBaselineId::new(&c.profile, &c.name),
                    config: TrackingConfig {
                        filter_selection: c.filter_selection.clone(),
                        gate_threshold: c.gate_threshold,
                        imm_turn_rate_rad_s: c.imm_turn_rate_rad_s,
                        imm_mode_transition: c.imm_mode_transition,
                        imm_initial_mode_probabilities: c.imm_initial_mode_probabilities,
                        measurement_noise_var: c.measurement_noise_var,
                    },
                    promoted: c.promoted,
                    validated_by: c.validated_by.clone(),
                })
                .collect();
        }
        self.tracking
            .iter()
            .map(|t| AlgorithmCandidate {
                id: gungnir_model::AlgorithmBaselineId::new(
                    gungnir_model::MissionProfile::DEFAULT,
                    "configured",
                ),
                config: t.clone(),
                promoted: true,
                validated_by: None,
            })
            .collect()
    }

    /// The profiles this baseline declares, including the implicit one (DN-24 §6).
    #[must_use]
    pub fn declared_profiles(&self) -> Vec<gungnir_model::MissionProfile> {
        if !self.mission_profiles.is_empty() {
            return self
                .mission_profiles
                .iter()
                .map(gungnir_model::MissionProfile::new)
                .collect();
        }
        if self.tracking.is_some() {
            return vec![gungnir_model::MissionProfile::new(
                gungnir_model::MissionProfile::DEFAULT,
            )];
        }
        Vec::new()
    }

    /// The profile this deployment is operating in, when it has one.
    ///
    /// `None` when nothing is declared at all. Validation has already refused the
    /// ambiguous case -- several profiles and no `active_profile` -- so this never guesses.
    #[must_use]
    pub fn operating_profile(&self) -> Option<gungnir_model::MissionProfile> {
        if let Some(name) = &self.active_profile {
            return Some(gungnir_model::MissionProfile::new(name));
        }
        let mut declared = self.declared_profiles();
        (declared.len() == 1).then(|| declared.remove(0))
    }

    /// True when this baseline may be promoted at `now`. A baseline outside its
    /// window may still be read, replayed, and inspected
    /// (docs/design/DN-08-policy-configuration.md §5).
    pub fn is_promotable_at(&self, now: gungnir_model::MissionTime) -> bool {
        self.validity.is_none_or(|w| w.contains(now))
    }

    fn declares_endpoint(&self, name: &str) -> bool {
        self.endpoints.iter().any(|e| e.name == name)
    }
}

fn has_duplicate_ids(mut ids: Vec<u32>) -> bool {
    ids.sort_unstable();
    ids.windows(2).any(|w| w[0] == w[1])
}

/// Resource rules, including the effector model MOE-03 depends on
/// (docs/design/DN-04-effector-model.md).
/// DN-26 §4: the five refusals a laydown table is validated against.
///
/// Every one of these is a refusal rather than a default, because each of the quiet
/// alternatives produces a comparison that looks like an answer. Guessing which laydown
/// is current, ignoring a placement for a sensor no registry declares, or letting one
/// option omit a sensor another places all yield a coverage number a planner would read
/// as a property of the laydowns.
fn validate_laydowns(baseline: &ConfigBaseline) -> Result<(), ConfigError> {
    if baseline.laydowns.is_empty() {
        return Ok(());
    }

    // 3. A duplicate `LaydownId`. Checked first: every later message names a laydown by
    //    its identifier, and two laydowns sharing one make those messages ambiguous.
    let mut seen: Vec<&str> = Vec::new();
    for l in &baseline.laydowns {
        if seen.contains(&l.id.0.as_str()) {
            return Err(ConfigError::Invalid(format!(
                "two laydowns share the identifier {:?}; a laydown is named so that a comparison can say which option it is describing",
                l.id.0
            )));
        }
        seen.push(l.id.0.as_str());
    }

    // 1. More than one marked current, or none while the table is non-empty.
    let current: Vec<&str> = baseline
        .laydowns
        .iter()
        .filter(|l| l.current)
        .map(|l| l.id.0.as_str())
        .collect();
    match current.len() {
        1 => {}
        0 => {
            return Err(ConfigError::Invalid(format!(
                "{} laydowns are declared and none is marked current; the current laydown is what the deployment is actually running and is the baseline every option is compared against, so it is refused rather than guessed",
                baseline.laydowns.len()
            )))
        }
        _ => {
            return Err(ConfigError::Invalid(format!(
                "laydowns {} are all marked current; exactly one placement is in force",
                current.join(", ")
            )))
        }
    }

    let declared_sensors: Vec<u32> = baseline.sensors.iter().map(|s| s.id).collect();
    let declared_resources: Vec<u32> = baseline.resources.iter().map(|r| r.id).collect();

    for l in &baseline.laydowns {
        // 5. A non-finite coordinate, which would propagate into a coverage answer.
        if l.has_non_finite_coordinate() {
            return Err(ConfigError::Invalid(format!(
                "laydown {} has a non-finite coordinate",
                l.id
            )));
        }
        // 2. An identifier no registry declares.
        for s in &l.sensors {
            if !declared_sensors.contains(&s.sensor.0) {
                return Err(ConfigError::Invalid(format!(
                    "laydown {} places sensor {}, which no sensor in this baseline declares",
                    l.id, s.sensor.0
                )));
            }
        }
        for r in &l.resources {
            if !declared_resources.contains(&r.resource.0) {
                return Err(ConfigError::Invalid(format!(
                    "laydown {} places resource {}, which no resource in this baseline declares",
                    l.id, r.resource.0
                )));
            }
        }
    }

    // 4. A sensor or resource placed in one laydown and absent from another. A laydown is
    //    complete by definition, and a partial one produces a coverage answer with a
    //    silent hole in it: the missing sensor reads as a sensor that contributes nothing
    //    rather than as one nobody said where to put.
    let first = &baseline.laydowns[0];
    let mut sensors = first.sensor_ids();
    sensors.sort_unstable();
    let mut resources = first.resource_ids();
    resources.sort_unstable();
    for l in baseline.laydowns.iter().skip(1) {
        let mut theirs = l.sensor_ids();
        theirs.sort_unstable();
        if theirs != sensors {
            return Err(ConfigError::Invalid(format!(
                "laydowns {} and {} place different sensors; a laydown is a complete placement, so an option that omits a sensor another places would be compared as though that sensor contributed nothing",
                first.id, l.id
            )));
        }
        let mut theirs = l.resource_ids();
        theirs.sort_unstable();
        if theirs != resources {
            return Err(ConfigError::Invalid(format!(
                "laydowns {} and {} place different resources; a laydown is a complete placement",
                first.id, l.id
            )));
        }
    }

    Ok(())
}

fn validate_resources(baseline: &ConfigBaseline) -> Result<(), ConfigError> {
    for r in &baseline.resources {
        if r.position.iter().any(|v| !v.is_finite()) {
            return Err(ConfigError::Invalid(format!(
                "resource {} has a non-finite position",
                r.id
            )));
        }
        if r.capacity == 0 {
            return Err(ConfigError::Invalid(format!(
                "resource {} has zero capacity",
                r.id
            )));
        }
        // MOE-03 is defined by the layer, so a missing or unrecognized one is
        // rejected rather than defaulted (docs/design/DN-04-effector-model.md).
        if EffectorLayer::parse(&r.layer).is_none() {
            return Err(ConfigError::Invalid(format!(
                "resource {} has an unknown effector layer {:?}; expected area, point, self-defence, or non-kinetic",
                r.id, r.layer
            )));
        }
        if let Some(cost) = r.cost {
            if !(cost.is_finite() && cost > 0.0) {
                return Err(ConfigError::Invalid(format!(
                    "resource {} has a non-finite or non-positive cost",
                    r.id
                )));
            }
        }
        // DN-04 §9: a closing speed, when stated, is a speed.
        if let Some(speed) = r.intercept_speed_mps {
            if !(speed.is_finite() && speed > 0.0) {
                return Err(ConfigError::Invalid(format!(
                    "resource {} has a non-finite or non-positive intercept speed",
                    r.id
                )));
            }
        }
        match (r.rounds_available, r.reserve) {
            (Some(rounds), Some(reserve)) if reserve > rounds => {
                return Err(ConfigError::Invalid(format!(
                    "resource {} reserves {reserve} of {rounds} rounds",
                    r.id
                )));
            }
            (None, Some(_)) => {
                return Err(ConfigError::Invalid(format!(
                    "resource {} has a reserve without a magazine",
                    r.id
                )));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Asset-list rules (docs/design/DN-01-defended-assets.md).
fn validate_assets(baseline: &ConfigBaseline) -> Result<(), ConfigError> {
    if has_duplicate_ids(baseline.assets.iter().map(|a| a.id).collect()) {
        return Err(ConfigError::Invalid("duplicate asset id".into()));
    }
    for a in &baseline.assets {
        if a.name.trim().is_empty() {
            return Err(ConfigError::Invalid(format!(
                "asset {} has an empty name",
                a.id
            )));
        }
        if a.position.iter().any(|v| !v.is_finite()) {
            return Err(ConfigError::Invalid(format!(
                "asset {} has a non-finite position",
                a.id
            )));
        }
        // An unknown priority is an error, not a default: a silently downgraded
        // priority is a safety problem.
        if AssetPriority::parse(&a.priority).is_none() {
            return Err(ConfigError::Invalid(format!(
                "asset {} has an unknown priority {:?}; expected low, medium, high, or critical",
                a.id, a.priority
            )));
        }
        if let Some(radius) = a.radius_m {
            if !(radius.is_finite() && radius > 0.0) {
                return Err(ConfigError::Invalid(format!(
                    "asset {} has a non-finite or non-positive radius",
                    a.id
                )));
            }
        }
        if a.warning_within_m.is_some()
            && (a.warning_lead_time_s.is_none() || a.warning_channel.is_none())
        {
            return Err(ConfigError::Invalid(format!(
                "asset {} has a warning distance without a lead time and channel",
                a.id
            )));
        }
        match (a.warning_lead_time_s, a.warning_channel.as_ref()) {
            (Some(lead), Some(channel)) => {
                if !(lead.is_finite() && lead > 0.0) {
                    return Err(ConfigError::Invalid(format!(
                        "asset {} has a non-finite or non-positive warning lead time",
                        a.id
                    )));
                }
                if !baseline.declares_endpoint(channel) {
                    return Err(ConfigError::Invalid(format!(
                        "asset {} warns on undeclared endpoint {channel:?}",
                        a.id
                    )));
                }
                if let Some(within) = a.warning_within_m {
                    if !(within.is_finite() && within > 0.0) {
                        return Err(ConfigError::Invalid(format!(
                            "asset {} has a non-finite or non-positive warning distance",
                            a.id
                        )));
                    }
                }
            }
            (None, None) => {}
            _ => {
                return Err(ConfigError::Invalid(format!(
                    "asset {} has half a warning obligation; give both a lead time and a channel, or neither",
                    a.id
                )));
            }
        }
    }
    Ok(())
}

/// Endpoint rules. Decision D-08 made every external party a named endpoint.
/// The three panels D-17 allows in a second window.
///
/// Deliberately a closed list rather than a capability. The decision dialog is the one
/// that matters: D-17 says decision dialogs stay with the queue, because a decision
/// separated from the queue it came from is a decision taken without its context, and a
/// baseline must not be able to configure that apart.
pub const DETACHABLE_PANELS: [&str; 3] = ["PN-02", "PN-06", "PN-12"];

/// Every panel identifier the interface defines, for validating an arrangement.
///
/// Listed here rather than imported so `gungnir-config` keeps no edge to
/// `gungnir-workflow`; `gungnir-app`'s tests assert the two agree, which is the check
/// that matters -- a baseline naming a panel that does not exist must be refused at
/// load, not silently ignored at draw time.
const KNOWN_PANELS: [&str; 21] = [
    "PN-01", "PN-02", "PN-03", "PN-04", "PN-05", "PN-06", "PN-07", "PN-08", "PN-09", "PN-10",
    "PN-11", "PN-12", "PN-13", "PN-14", "PN-15", "PN-16", "PN-17", "PN-18", "PN-19", "PN-20",
    "PN-21",
];

/// Coverage analysis settings (DN-12 §6).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AnalyticsConfig {
    /// Distance between coverage samples along an approach, metres.
    ///
    /// It appears on every `CoverageReport`, so a coarse run cannot be mistaken for a
    /// fine one. Smaller finds shorter gaps and costs proportionally more.
    #[serde(default = "default_coverage_sample_spacing_m")]
    pub coverage_sample_spacing_m: f64,
    /// Lowest elevation angle a sensor is credited with covering, radians.
    ///
    /// Defaults to the full lower hemisphere, which credits every sensor with more than
    /// most have. A deployment that knows its sensors' masks should say so: an
    /// optimistic elevation limit reports coverage that is not there, and DN-12's whole
    /// point is not doing that.
    #[serde(default = "default_coverage_min_elevation_rad")]
    pub coverage_min_elevation_rad: f64,
    /// Anomaly detector thresholds (DN-15 §6, GAP-021). A detector left out is off.
    #[serde(default)]
    pub anomaly: gungnir_model::anomaly_settings::AnomalySettings,
    /// How many sensor-plan candidates the re-tasking search scores (DN-13 §6, GAP-037).
    ///
    /// Bounds a search that is a handful of sensors times a handful of modes; the default
    /// covers a small sector, and a large one raises it knowingly.
    #[serde(default = "default_max_sensor_plan_candidates")]
    pub max_sensor_plan_candidates: usize,
}

fn default_max_sensor_plan_candidates() -> usize {
    16
}

/// Seconds a sensor task waits for an acknowledgement before it is reported
/// unacknowledged (DN-11 §6, GAP-004).
fn default_sensor_task_ack_window_s() -> f64 {
    10.0
}

fn default_coverage_min_elevation_rad() -> f64 {
    -std::f64::consts::FRAC_PI_2
}

fn default_coverage_sample_spacing_m() -> f64 {
    250.0
}

impl Default for AnalyticsConfig {
    fn default() -> Self {
        Self {
            coverage_sample_spacing_m: default_coverage_sample_spacing_m(),
            coverage_min_elevation_rad: default_coverage_min_elevation_rad(),
            anomaly: gungnir_model::anomaly_settings::AnomalySettings::default(),
            max_sensor_plan_candidates: default_max_sensor_plan_candidates(),
        }
    }
}

/// One approach axis, as a geodetic polyline.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ApproachConfig {
    /// What this axis is called, so a reported gap names somewhere rather than an index.
    pub name: String,
    /// Points as `[lat_rad, lon_rad, alt_m]`, in order along the approach.
    ///
    /// Declare the altitude the axis is actually flown at. Without a terrain model,
    /// line of sight is tested against the ENU tangent plane, and a point at constant
    /// altitude falls below that plane with distance -- roughly 240 m at 55 km -- so a
    /// ground-level axis is reported as hidden past a few kilometres. That is a crude
    /// horizon rather than a bug, but it makes a sea-level axis a poor thing to measure
    /// coverage along. See `ARCHITECTURE.md` §10.
    pub points: Vec<[f64; 3]>,
}

fn validate_sensor_control(baseline: &ConfigBaseline) -> Result<(), ConfigError> {
    if !baseline.sensor_task_ack_window_s.is_finite() || baseline.sensor_task_ack_window_s <= 0.0 {
        return Err(ConfigError::Invalid(format!(
            "sensor_task_ack_window_s must be finite and positive, not {}",
            baseline.sensor_task_ack_window_s
        )));
    }
    for sensor in &baseline.sensors {
        if let Some(endpoint) = &sensor.control_endpoint {
            if !baseline.declares_endpoint(endpoint) {
                return Err(ConfigError::Invalid(format!(
                    "sensor {} names control endpoint {endpoint}, which the endpoint table does not declare",
                    sensor.id
                )));
            }
        }
    }
    Ok(())
}

/// One static hazard as the baseline declares it (DN-14 §6, GAP-017).
///
/// Config-shaped rather than `gungnir_geo::Hazard`: this crate does not depend on the
/// geometry crate, and the desktop converts -- the same split `ApproachConfig` and
/// `AssetConfig` already make. Positions are `[lat_rad, lon_rad, alt_m]`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HazardConfig {
    pub name: String,
    /// One of [`HazardConfig::KINDS`].
    pub kind: String,
    #[serde(flatten)]
    pub shape: HazardShapeConfig,
    /// True when the obstacle stops a surface craft. Stated, not inferred from the kind:
    /// a shoal stops a deep-draught vessel and not a jet ski.
    #[serde(default)]
    pub blocks_surface: bool,
    /// Height above the surface, metres, where it constrains air movement.
    #[serde(default)]
    pub height_m: Option<f64>,
}

impl HazardConfig {
    /// The kinds `gungnir_geo::HazardKind` knows, in its serialised spelling. Kept in
    /// step by a test in `gungnir-app`, which is where the two meet.
    pub const KINDS: [&'static str; 6] = ["boom", "net", "barrier", "wreck", "shoal", "other"];
}

/// A hazard is a line more often than an area: a boom across a harbour mouth is a
/// segment, not a circle.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "shape", rename_all = "kebab-case")]
pub enum HazardShapeConfig {
    Polyline { points: Vec<[f64; 3]> },
    Circle { center: [f64; 3], radius_m: f64 },
}

/// One geofence as the baseline declares it (GAP-088). `center` is `[lat_rad, lon_rad,
/// alt_m]`. Config-shaped; the binaries build `gungnir_geo::Geofence` from it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GeofenceConfig {
    pub name: String,
    pub center: [f64; 3],
    pub radius_m: f64,
    /// True denies intercepts inside the fence; false marks an area without denying.
    #[serde(default)]
    pub no_go: bool,
}

/// DN-22 §11: the escrow key is the public half and names a holder.
fn validate_escrow(baseline: &ConfigBaseline) -> Result<(), ConfigError> {
    let Some(e) = &baseline.security.escrow else {
        return Ok(());
    };
    if e.public_key_pem.contains("PRIVATE KEY") {
        return Err(ConfigError::Invalid(
            "security.escrow.public_key_pem holds a private key; the baseline may carry only the public half".into(),
        ));
    }
    if !e.public_key_pem.contains("-----BEGIN PUBLIC KEY-----") {
        return Err(ConfigError::Invalid(
            "security.escrow.public_key_pem is not a PEM public key".into(),
        ));
    }
    if e.holder == 0 {
        return Err(ConfigError::Invalid(
            "security.escrow.holder names nobody".into(),
        ));
    }
    Ok(())
}

/// DN-16 §6: a peer has a unique name, an endpoint that exists, a quality in range, a
/// positive age, and a source identifier that is no sensor's and no other peer's.
fn validate_peers(baseline: &ConfigBaseline) -> Result<(), ConfigError> {
    let mut names = std::collections::BTreeSet::new();
    let mut sources = std::collections::BTreeSet::new();
    for p in &baseline.peers {
        if !names.insert(p.name.as_str()) {
            return Err(ConfigError::Invalid(format!(
                "peer {:?} is declared twice",
                p.name
            )));
        }
        if !baseline.endpoints.iter().any(|e| e.name == p.endpoint) {
            return Err(ConfigError::Invalid(format!(
                "peer {:?} names endpoint {:?}, which is not in the endpoint table",
                p.name, p.endpoint
            )));
        }
        if !(0.0..=1.0).contains(&p.assigned_quality) {
            return Err(ConfigError::Invalid(format!(
                "peer {:?}: assigned_quality {} is not in 0..=1",
                p.name, p.assigned_quality
            )));
        }
        if !(p.max_age_s.is_finite() && p.max_age_s > 0.0) {
            return Err(ConfigError::Invalid(format!(
                "peer {:?}: max_age_s must be positive",
                p.name
            )));
        }
        if baseline.sensors.iter().any(|s| s.id == p.source_id) || !sources.insert(p.source_id) {
            return Err(ConfigError::Invalid(format!(
                "peer {:?}: source_id {} is a sensor's or another peer's",
                p.name, p.source_id
            )));
        }
    }
    Ok(())
}

/// GAP-060: a trust root is a public certificate and nothing else. A private key in the
/// baseline is refused without repeating it.
fn validate_trust_roots(baseline: &ConfigBaseline) -> Result<(), ConfigError> {
    for (i, pem) in baseline.security.tls.trust_roots_pem.iter().enumerate() {
        if pem.contains("PRIVATE KEY") {
            return Err(ConfigError::Invalid(format!(
                "security.tls.trust_roots_pem[{i}] holds a private key; a baseline may not"
            )));
        }
        if !pem.contains("-----BEGIN CERTIFICATE-----") {
            return Err(ConfigError::Invalid(format!(
                "security.tls.trust_roots_pem[{i}] is not a PEM certificate"
            )));
        }
    }
    Ok(())
}

/// GAP-001: a radar feed names a socket that parses, radars that exist, and SAC/SIC pairs
/// that are bound once. Nothing defaults: an unknown sensor or a duplicate pair is how
/// one radar's plots become another's.
/// DN-18 §6: every agreement names a party that is a configured peer or endpoint, no
/// party appears twice, and the format is one this build can produce.
fn validate_exchange(baseline: &ConfigBaseline) -> Result<(), ConfigError> {
    let mut parties = std::collections::BTreeSet::new();
    for agreement in &baseline.exchange {
        if !parties.insert(agreement.party.as_str()) {
            return Err(ConfigError::Invalid(format!(
                "exchange party {:?} appears twice",
                agreement.party
            )));
        }
        let known = baseline.peers.iter().any(|p| p.name == agreement.party)
            || baseline.endpoints.iter().any(|e| e.name == agreement.party);
        if !known {
            return Err(ConfigError::Invalid(format!(
                "exchange party {:?} is neither a configured peer nor an endpoint",
                agreement.party
            )));
        }
        if agreement.format != gungnir_model::ExchangeFormat::Canonical {
            return Err(ConfigError::Invalid(format!(
                "exchange with {:?} asks for {:?}, and this build converts to no industry \
                 format outbound (GAP-064)",
                agreement.party, agreement.format
            )));
        }
    }
    Ok(())
}

/// D-02: a machine identity names a certificate once, and speaks for something the
/// baseline declares -- a sensor in the list, a handoff endpoint, a warning endpoint, or
/// a peer.
///
/// The endpoint's `kind` is checked, not just its name: a certificate admitted to the
/// warning-acknowledgement route because it matched a `handoff` row would be an effector
/// discharging warnings it was never sent (GAP-042).
fn validate_machine_identities(baseline: &ConfigBaseline) -> Result<(), ConfigError> {
    let mut names = std::collections::BTreeSet::new();
    for m in &baseline.machine_identities {
        if m.common_name.trim().is_empty() {
            return Err(ConfigError::Invalid(
                "machine_identities: a common name is empty".into(),
            ));
        }
        if !names.insert(m.common_name.as_str()) {
            return Err(ConfigError::Invalid(format!(
                "machine identity {:?} is declared twice",
                m.common_name
            )));
        }
        match &m.speaks_for {
            MachineRole::Sensor { sensor_id } => {
                if !baseline.sensors.iter().any(|s| s.id == *sensor_id) {
                    return Err(ConfigError::Invalid(format!(
                        "machine identity {:?} speaks for sensor {sensor_id}, which is not in the sensor list",
                        m.common_name
                    )));
                }
            }
            MachineRole::Effector { endpoint } => {
                let known = baseline
                    .endpoints
                    .iter()
                    .any(|e| e.name == *endpoint && e.kind == "handoff");
                if !known {
                    return Err(ConfigError::Invalid(format!(
                        "machine identity {:?} speaks for endpoint {endpoint:?}, which is not a handoff endpoint",
                        m.common_name
                    )));
                }
            }
            MachineRole::WarnedParty { channel } => {
                let known = baseline
                    .endpoints
                    .iter()
                    .any(|e| e.name == *channel && e.kind == "warning");
                if !known {
                    return Err(ConfigError::Invalid(format!(
                        "machine identity {:?} speaks for channel {channel:?}, which is not a warning endpoint",
                        m.common_name
                    )));
                }
            }
            MachineRole::Peer { peer } => {
                if !baseline.peers.iter().any(|p| p.name == *peer) {
                    return Err(ConfigError::Invalid(format!(
                        "machine identity {:?} speaks for peer {peer:?}, which is not in the peer table",
                        m.common_name
                    )));
                }
            }
        }
    }
    Ok(())
}

/// GAP-010: an AIS feed names a receiver in the sensor list, once, and a source that
/// parses.
fn validate_ais_feeds(baseline: &ConfigBaseline) -> Result<(), ConfigError> {
    let mut names = std::collections::BTreeSet::new();
    let mut receivers = std::collections::BTreeSet::new();
    for feed in &baseline.ais_feeds {
        if !names.insert(feed.name.as_str()) {
            return Err(ConfigError::Invalid(format!(
                "AIS feed {:?} is declared twice",
                feed.name
            )));
        }
        if !baseline.sensors.iter().any(|s| s.id == feed.sensor_id) {
            return Err(ConfigError::Invalid(format!(
                "AIS feed {:?} names sensor {}, which is not in the sensor list",
                feed.name, feed.sensor_id
            )));
        }
        if !receivers.insert(feed.sensor_id) {
            return Err(ConfigError::Invalid(format!(
                "AIS feed {:?} names sensor {}, which another feed already speaks for",
                feed.name, feed.sensor_id
            )));
        }
        match &feed.source {
            AisSource::Tcp { addr } => {
                if addr.parse::<std::net::SocketAddr>().is_err() {
                    return Err(ConfigError::Invalid(format!(
                        "AIS feed {:?}: {addr:?} is not an ip:port",
                        feed.name
                    )));
                }
            }
            AisSource::File { path } => {
                if path.trim().is_empty() {
                    return Err(ConfigError::Invalid(format!(
                        "AIS feed {:?}: the recording path is empty",
                        feed.name
                    )));
                }
            }
        }
    }
    Ok(())
}

/// GAP-010: an ADS-B feed names a receiver in the sensor list, once, and a source that
/// parses. Same shape as [`validate_ais_feeds`], for the sibling feed type.
fn validate_adsb_feeds(baseline: &ConfigBaseline) -> Result<(), ConfigError> {
    let mut names = std::collections::BTreeSet::new();
    let mut receivers = std::collections::BTreeSet::new();
    for feed in &baseline.adsb_feeds {
        if !names.insert(feed.name.as_str()) {
            return Err(ConfigError::Invalid(format!(
                "ADS-B feed {:?} is declared twice",
                feed.name
            )));
        }
        if !baseline.sensors.iter().any(|s| s.id == feed.sensor_id) {
            return Err(ConfigError::Invalid(format!(
                "ADS-B feed {:?} names sensor {}, which is not in the sensor list",
                feed.name, feed.sensor_id
            )));
        }
        if !receivers.insert(feed.sensor_id) {
            return Err(ConfigError::Invalid(format!(
                "ADS-B feed {:?} names sensor {}, which another feed already speaks for",
                feed.name, feed.sensor_id
            )));
        }
        match &feed.source {
            AdsbSource::Tcp { addr } => {
                if addr.parse::<std::net::SocketAddr>().is_err() {
                    return Err(ConfigError::Invalid(format!(
                        "ADS-B feed {:?}: {addr:?} is not an ip:port",
                        feed.name
                    )));
                }
            }
            AdsbSource::File { path } => {
                if path.trim().is_empty() {
                    return Err(ConfigError::Invalid(format!(
                        "ADS-B feed {:?}: the recording path is empty",
                        feed.name
                    )));
                }
            }
        }
    }
    Ok(())
}

/// GAP-001: a SAPIENT feed names a receiver in the sensor list, once, and a source that
/// parses. Same shape as [`validate_ais_feeds`]; `node_type` needs no validation of its
/// own, since the enum has no value the adapter would refuse.
fn validate_sapient_feeds(baseline: &ConfigBaseline) -> Result<(), ConfigError> {
    let mut names = std::collections::BTreeSet::new();
    let mut receivers = std::collections::BTreeSet::new();
    for feed in &baseline.sapient_feeds {
        if !names.insert(feed.name.as_str()) {
            return Err(ConfigError::Invalid(format!(
                "SAPIENT feed {:?} is declared twice",
                feed.name
            )));
        }
        if !baseline.sensors.iter().any(|s| s.id == feed.sensor_id) {
            return Err(ConfigError::Invalid(format!(
                "SAPIENT feed {:?} names sensor {}, which is not in the sensor list",
                feed.name, feed.sensor_id
            )));
        }
        if !receivers.insert(feed.sensor_id) {
            return Err(ConfigError::Invalid(format!(
                "SAPIENT feed {:?} names sensor {}, which another feed already speaks for",
                feed.name, feed.sensor_id
            )));
        }
        match &feed.source {
            SapientSource::Tcp { addr } => {
                if addr.parse::<std::net::SocketAddr>().is_err() {
                    return Err(ConfigError::Invalid(format!(
                        "SAPIENT feed {:?}: {addr:?} is not an ip:port",
                        feed.name
                    )));
                }
            }
            SapientSource::File { path } => {
                if path.trim().is_empty() {
                    return Err(ConfigError::Invalid(format!(
                        "SAPIENT feed {:?}: the recording path is empty",
                        feed.name
                    )));
                }
            }
        }
    }
    Ok(())
}

fn validate_radar_feeds(baseline: &ConfigBaseline) -> Result<(), ConfigError> {
    let mut names = std::collections::BTreeSet::new();
    let mut pairs = std::collections::BTreeSet::new();
    let mut bound = std::collections::BTreeSet::new();
    for feed in &baseline.radar_feeds {
        if !names.insert(feed.name.as_str()) {
            return Err(ConfigError::Invalid(format!(
                "radar feed {:?} is declared twice",
                feed.name
            )));
        }
        if feed.bind_addr.parse::<std::net::SocketAddr>().is_err() {
            return Err(ConfigError::Invalid(format!(
                "radar feed {:?}: bind_addr {:?} is not an ip:port",
                feed.name, feed.bind_addr
            )));
        }
        if let Some(m) = &feed.multicast {
            for (what, value) in [("group", &m.group), ("interface", &m.interface)] {
                if value.parse::<std::net::Ipv4Addr>().is_err() {
                    return Err(ConfigError::Invalid(format!(
                        "radar feed {:?}: multicast {what} {value:?} is not an IPv4 address",
                        feed.name
                    )));
                }
            }
        }
        if feed.radars.is_empty() {
            return Err(ConfigError::Invalid(format!(
                "radar feed {:?} binds no radar",
                feed.name
            )));
        }
        for r in &feed.radars {
            if !baseline.sensors.iter().any(|s| s.id == r.sensor_id) {
                return Err(ConfigError::Invalid(format!(
                    "radar feed {:?} names sensor {}, which is not in the sensor list",
                    feed.name, r.sensor_id
                )));
            }
            if !pairs.insert((r.sac, r.sic)) {
                return Err(ConfigError::Invalid(format!(
                    "radar feed {:?}: SAC/SIC {}/{} is bound twice",
                    feed.name, r.sac, r.sic
                )));
            }
            if !bound.insert(r.sensor_id) {
                return Err(ConfigError::Invalid(format!(
                    "radar feed {:?}: sensor {} is bound to two SAC/SIC pairs",
                    feed.name, r.sensor_id
                )));
            }
        }
    }
    Ok(())
}

/// GAP-023: a terrain entry names a file of a format the loader reads, in a frame the
/// desktop can place. Existence is not checked here: a baseline is validated on machines
/// that do not hold the file, and the loader reports a missing one at start.
fn validate_terrain(baseline: &ConfigBaseline) -> Result<(), ConfigError> {
    let Some(t) = &baseline.terrain else {
        return Ok(());
    };
    let extension = std::path::Path::new(&t.path)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    if !matches!(extension.as_deref(), Some("asc" | "tif" | "tiff")) {
        return Err(ConfigError::Invalid(format!(
            "terrain.path {:?} is not an ESRI ASCII grid (.asc) or a GeoTIFF (.tif, .tiff)",
            t.path
        )));
    }
    if t.frame != "local-enu" {
        return Err(ConfigError::Invalid(format!(
            "terrain.frame {:?} is not supported; only \"local-enu\" is, because no projection \
             library is in the approved stack",
            t.frame
        )));
    }
    Ok(())
}

/// DN-07 §6: a resource's handoff endpoint names an entry in the endpoint table.
fn validate_handoff_endpoints(baseline: &ConfigBaseline) -> Result<(), ConfigError> {
    for r in &baseline.resources {
        if let Some(name) = &r.handoff_endpoint {
            if !baseline.endpoints.iter().any(|e| &e.name == name) {
                return Err(ConfigError::Invalid(format!(
                    "resource {} names handoff endpoint {name:?}, which is not in the endpoint table",
                    r.id
                )));
            }
        }
    }
    Ok(())
}

fn validate_geofences(baseline: &ConfigBaseline) -> Result<(), ConfigError> {
    let mut names: Vec<&str> = baseline.geofences.iter().map(|g| g.name.as_str()).collect();
    names.sort_unstable();
    if names.windows(2).any(|w| w[0] == w[1]) {
        return Err(ConfigError::Invalid("duplicate geofence name".into()));
    }
    for fence in &baseline.geofences {
        if fence.name.trim().is_empty() {
            return Err(ConfigError::Invalid("a geofence has an empty name".into()));
        }
        if fence.center.iter().any(|v| !v.is_finite())
            || !(fence.radius_m.is_finite() && fence.radius_m > 0.0)
        {
            return Err(ConfigError::Invalid(format!(
                "geofence {:?} needs a finite centre and a positive, finite radius",
                fence.name
            )));
        }
    }
    Ok(())
}

/// DN-14 §6: polylines have at least two points, radii are positive, heights are finite.
fn validate_hazards(baseline: &ConfigBaseline) -> Result<(), ConfigError> {
    let mut names: Vec<&str> = baseline.hazards.iter().map(|h| h.name.as_str()).collect();
    names.sort_unstable();
    if names.windows(2).any(|w| w[0] == w[1]) {
        return Err(ConfigError::Invalid("duplicate hazard name".into()));
    }
    let finite = |p: &[f64; 3]| p.iter().all(|v| v.is_finite());
    for hazard in &baseline.hazards {
        if hazard.name.trim().is_empty() {
            return Err(ConfigError::Invalid("a hazard has an empty name".into()));
        }
        if !HazardConfig::KINDS.contains(&hazard.kind.as_str()) {
            return Err(ConfigError::Invalid(format!(
                "hazard {:?} has unknown kind {:?}; one of {:?}",
                hazard.name,
                hazard.kind,
                HazardConfig::KINDS
            )));
        }
        match &hazard.shape {
            HazardShapeConfig::Polyline { points } => {
                // One point is a place, not a boom.
                if points.len() < 2 {
                    return Err(ConfigError::Invalid(format!(
                        "hazard {:?} is a polyline with fewer than two points",
                        hazard.name
                    )));
                }
                if !points.iter().all(finite) {
                    return Err(ConfigError::Invalid(format!(
                        "hazard {:?} has a non-finite point",
                        hazard.name
                    )));
                }
            }
            HazardShapeConfig::Circle { center, radius_m } => {
                if !(finite(center) && radius_m.is_finite() && *radius_m > 0.0) {
                    return Err(ConfigError::Invalid(format!(
                        "hazard {:?} needs a finite centre and a positive, finite radius",
                        hazard.name
                    )));
                }
            }
        }
        if let Some(h) = hazard.height_m {
            if !(h.is_finite() && h > 0.0) {
                return Err(ConfigError::Invalid(format!(
                    "hazard {:?} has a height that is not finite and positive: {h}",
                    hazard.name
                )));
            }
        }
    }
    Ok(())
}

fn validate_approaches(baseline: &ConfigBaseline) -> Result<(), ConfigError> {
    if !baseline.analytics.coverage_sample_spacing_m.is_finite()
        || baseline.analytics.coverage_sample_spacing_m <= 0.0
    {
        return Err(ConfigError::Invalid(format!(
            "coverage_sample_spacing_m must be finite and positive, not {}",
            baseline.analytics.coverage_sample_spacing_m
        )));
    }
    let mut names: Vec<&str> = baseline
        .approaches
        .iter()
        .map(|a| a.name.as_str())
        .collect();
    names.sort_unstable();
    if names.windows(2).any(|w| w[0] == w[1]) {
        return Err(ConfigError::Invalid("duplicate approach name".into()));
    }
    for approach in &baseline.approaches {
        if approach.name.trim().is_empty() {
            return Err(ConfigError::Invalid("an approach has an empty name".into()));
        }
        // One point is a place, not an axis, and sampling it reports a gap of zero
        // length wherever it happens to sit -- which reads as coverage.
        if approach.points.len() < 2 {
            return Err(ConfigError::Invalid(format!(
                "approach {} has fewer than two points, so it is a place and not an axis",
                approach.name
            )));
        }
        for [lat, lon, alt] in &approach.points {
            if !lat.is_finite() || !lon.is_finite() || !alt.is_finite() {
                return Err(ConfigError::Invalid(format!(
                    "approach {} has a non-finite point",
                    approach.name
                )));
            }
            if lat.abs() > std::f64::consts::FRAC_PI_2 || lon.abs() > std::f64::consts::PI {
                return Err(ConfigError::Invalid(format!(
                    "approach {} has a point outside the globe; approaches are in \
                     radians, not degrees",
                    approach.name
                )));
            }
        }
    }
    Ok(())
}

/// The declared origin has to be a position on Earth.
///
/// A non-finite or out-of-range origin is refused rather than clamped: it would place
/// the whole local frame somewhere impossible, and every geodetic thing on the map with
/// it, with no symptom other than the picture being wrong.
fn validate_origin(baseline: &ConfigBaseline) -> Result<(), ConfigError> {
    let Some([lat, lon, alt]) = baseline.origin else {
        return Ok(());
    };
    if !lat.is_finite() || !lon.is_finite() || !alt.is_finite() {
        return Err(ConfigError::Invalid(
            "the local frame origin has a non-finite component".into(),
        ));
    }
    if lat.abs() > std::f64::consts::FRAC_PI_2 {
        return Err(ConfigError::Invalid(format!(
            "the local frame origin's latitude {lat} rad is outside +/- pi/2; the origin \
             is in radians, not degrees"
        )));
    }
    if lon.abs() > std::f64::consts::PI {
        return Err(ConfigError::Invalid(format!(
            "the local frame origin's longitude {lon} rad is outside +/- pi; the origin \
             is in radians, not degrees"
        )));
    }
    Ok(())
}

/// A deployment may rename a term the interface shows; it may not invent one.
///
/// An unknown key is refused rather than ignored. Ignoring it is the tempting choice --
/// the interface would carry on with the default and nothing would break -- and it is
/// the wrong one: an administrator who mistyped `classification.hostle` would see the
/// default word, conclude the override does not work, and have nothing to tell them
/// why. The same reasoning as an unknown panel identifier in an arrangement.
///
/// An empty label is refused for a simpler reason: a term with no word cannot be shown
/// at all, and a blank where a classification should be is the worst possible reading of
/// a track.
fn validate_vocabulary(baseline: &ConfigBaseline) -> Result<(), ConfigError> {
    for (key, label) in &baseline.vocabulary.overrides {
        if Term::from_key(key).is_none() {
            return Err(ConfigError::Invalid(format!(
                "vocabulary override {key} names no term this interface shows"
            )));
        }
        if label.trim().is_empty() {
            return Err(ConfigError::Invalid(format!(
                "vocabulary override {key} has an empty label"
            )));
        }
    }
    Ok(())
}

/// Whether a `PN-xx` identifier is one this schema accepts in an arrangement.
///
/// Exposed so `gungnir-app` can assert it agrees with `gungnir_workflow::PanelId`,
/// which is the check that keeps the two lists from drifting apart.
#[must_use]
pub fn validate_panel_id(pn: &str) -> bool {
    KNOWN_PANELS.contains(&pn)
}

fn validate_ui(baseline: &ConfigBaseline) -> Result<(), ConfigError> {
    for (role, layout) in &baseline.ui.layouts {
        validate_layout_node(role, &layout.main)?;

        let named = layout.main.panels();
        for pn in &layout.detached {
            if !DETACHABLE_PANELS.contains(&pn.as_str()) {
                return Err(ConfigError::Invalid(format!(
                    "role {role} detaches {pn}, which D-17 does not allow in a second \
                     window; only {} may be detached",
                    DETACHABLE_PANELS.join(", ")
                )));
            }
            if !named.contains(&pn.as_str()) {
                return Err(ConfigError::Invalid(format!(
                    "role {role} detaches {pn}, which its layout does not contain"
                )));
            }
        }
        let mut seen: Vec<&str> = named.clone();
        seen.sort_unstable();
        if seen.windows(2).any(|w| w[0] == w[1]) {
            return Err(ConfigError::Invalid(format!(
                "role {role} names the same panel twice"
            )));
        }
    }
    Ok(())
}

fn validate_layout_node(role: &str, node: &gungnir_model::LayoutNode) -> Result<(), ConfigError> {
    use gungnir_model::LayoutNode;
    match node {
        LayoutNode::Panel { pn } => {
            if !KNOWN_PANELS.contains(&pn.as_str()) {
                return Err(ConfigError::Invalid(format!(
                    "role {role} names panel {pn}, which does not exist"
                )));
            }
        }
        LayoutNode::Tabs { children } => {
            if children.is_empty() {
                return Err(ConfigError::Invalid(format!(
                    "role {role} has an empty tab group, which would draw nothing"
                )));
            }
        }
        LayoutNode::Horizontal { children, shares } | LayoutNode::Vertical { children, shares } => {
            if children.is_empty() {
                return Err(ConfigError::Invalid(format!(
                    "role {role} has an empty split, which would draw nothing"
                )));
            }
            if !shares.is_empty() && shares.len() != children.len() {
                return Err(ConfigError::Invalid(format!(
                    "role {role} declares {} shares for {} children",
                    shares.len(),
                    children.len()
                )));
            }
            if shares.iter().any(|s| !s.is_finite() || *s <= 0.0) {
                return Err(ConfigError::Invalid(format!(
                    "role {role} declares a share that is not a positive finite number"
                )));
            }
        }
    }
    for child in node.children() {
        validate_layout_node(role, child)?;
    }
    Ok(())
}

fn validate_endpoints(baseline: &ConfigBaseline) -> Result<(), ConfigError> {
    let mut names: Vec<&str> = baseline.endpoints.iter().map(|e| e.name.as_str()).collect();
    names.sort_unstable();
    if names.windows(2).any(|w| w[0] == w[1]) {
        return Err(ConfigError::Invalid("duplicate endpoint name".into()));
    }
    for e in &baseline.endpoints {
        if e.name.trim().is_empty() || e.kind.trim().is_empty() || e.address.trim().is_empty() {
            return Err(ConfigError::Invalid(format!(
                "endpoint {:?} has an empty name, kind, or address",
                e.name
            )));
        }
    }
    Ok(())
}

/// Authority and decision-timing rules, split out of `validate_policy` so each
/// validator stays readable (docs/design/DN-08-policy-configuration.md).
fn validate_decision_rules(p: &gungnir_model::PolicySettings) -> Result<(), ConfigError> {
    for rule in &p.authority.rules {
        if rule.action.trim().is_empty() || rule.role.trim().is_empty() {
            return Err(ConfigError::Invalid(
                "an authority rule has an empty action or role".into(),
            ));
        }
        // Decision D-15 pre-delegated one specific case, not a general power.
        if rule.pre_delegated && (rule.layer.is_none() || rule.class.is_none()) {
            return Err(ConfigError::Invalid(format!(
                "pre-delegated authority rule for {:?} must name both a layer and a class",
                rule.role
            )));
        }
    }
    for (layer, seconds) in &p.decisions.expiry_s {
        if !(seconds.is_finite() && *seconds > 0.0) {
            return Err(ConfigError::Invalid(format!(
                "decision expiry for {layer:?} must be finite and positive"
            )));
        }
    }
    for (layer, escalate) in &p.decisions.escalate_after_s {
        if !(escalate.is_finite() && *escalate > 0.0) {
            return Err(ConfigError::Invalid(format!(
                "escalation delay for {layer:?} must be finite and positive"
            )));
        }
        // Escalating after expiry is meaningless.
        if let Some(expiry) = p.decisions.expiry_s.get(layer) {
            if escalate >= expiry {
                return Err(ConfigError::Invalid(format!(
                    "escalation delay for {layer:?} is not earlier than its expiry"
                )));
            }
        }
    }
    Ok(())
}

/// Mission profiles and candidate algorithm baselines
/// (docs/design/DN-24-mission-profiles-and-algorithm-baselines.md §6, GAP-086).
///
/// **Rule 2 is the one that matters.** Zero promoted candidates in a profile means the
/// deployment cannot say what is running; two means it cannot say either, and would report
/// whichever the iteration order happened to reach. Both are worse than a refusal at load,
/// because both look like a governed system from every screen.
///
/// # Errors
///
/// [`ConfigError::Invalid`] naming the first rule that does not hold.
fn validate_profiles(baseline: &ConfigBaseline) -> Result<(), ConfigError> {
    // Rule 6, first: with both present nothing below can be checked coherently.
    if baseline.tracking.is_some() && !baseline.tracking_profiles.is_empty() {
        return Err(ConfigError::Invalid(
            "tracking and tracking_profiles are mutually exclusive: a baseline carrying \
             both gives two answers to which configuration is in force"
                .into(),
        ));
    }
    if baseline.mission_profiles.is_empty() && !baseline.tracking_profiles.is_empty() {
        return Err(ConfigError::Invalid(
            "tracking_profiles are declared and mission_profiles is empty, so every \
             candidate names a profile that does not exist"
                .into(),
        ));
    }
    for profile in &baseline.mission_profiles {
        if profile.trim().is_empty() {
            return Err(ConfigError::Invalid(
                "a declared mission profile has no name".into(),
            ));
        }
    }
    if has_duplicate_names(&baseline.mission_profiles) {
        return Err(ConfigError::Invalid(
            "mission_profiles names a profile twice".into(),
        ));
    }

    for candidate in &baseline.tracking_profiles {
        // Rule 1.
        if !baseline
            .mission_profiles
            .iter()
            .any(|p| p == &candidate.profile)
        {
            return Err(ConfigError::Invalid(format!(
                "tracking profile {:?} names undeclared mission profile {:?}",
                candidate.name, candidate.profile
            )));
        }
        if candidate.name.trim().is_empty() {
            return Err(ConfigError::Invalid(format!(
                "a candidate in profile {:?} has no name",
                candidate.profile
            )));
        }
        // Rule 5: the two rules `validate` applies to `tracking`, per candidate.
        if !(candidate.gate_threshold.is_finite() && candidate.gate_threshold > 0.0) {
            return Err(ConfigError::Invalid(format!(
                "candidate {:?} in profile {:?} must have a finite, positive gate threshold",
                candidate.name, candidate.profile
            )));
        }
        if candidate.filter_selection.trim().is_empty() {
            return Err(ConfigError::Invalid(format!(
                "candidate {:?} in profile {:?} selects no filter",
                candidate.name, candidate.profile
            )));
        }
        validate_measurement_noise_var(
            candidate.measurement_noise_var,
            &format!(
                "candidate {:?} in profile {:?}",
                candidate.name, candidate.profile
            ),
        )?;
        if candidate.filter_selection == "imm-cv-ct" {
            validate_imm_fields(
                candidate.imm_turn_rate_rad_s,
                candidate.imm_mode_transition,
                candidate.imm_initial_mode_probabilities,
                &format!(
                    "candidate {:?} in profile {:?}",
                    candidate.name, candidate.profile
                ),
            )?;
        }
    }

    for profile in &baseline.mission_profiles {
        validate_one_profile(baseline, profile)?;
    }

    // Rule 3.
    match &baseline.active_profile {
        Some(active) => {
            if !baseline.mission_profiles.iter().any(|p| p == active) {
                return Err(ConfigError::Invalid(format!(
                    "active_profile {active:?} is not a declared mission profile"
                )));
            }
        }
        None if baseline.mission_profiles.len() > 1 => {
            return Err(ConfigError::Invalid(
                "several mission profiles are declared and none is active; which one a \
                 deployment is operating in is not something to guess"
                    .into(),
            ))
        }
        None => {}
    }
    Ok(())
}

/// Mirrors `gungnir_filters::imm::Imm::new`'s own tolerance exactly (DN-28 §5): a file
/// this module accepts must never be one `Imm::new` refuses at pipeline-construction
/// time, and this crate sits below `gungnir-filters` and cannot import the constant to
/// guarantee that structurally, so it is restated here instead.
const IMM_STOCHASTIC_TOLERANCE: f64 = 1e-9;

/// The `"imm-cv-ct"` selection's own fields, validated against the same rules
/// `gungnir_filters::imm::Imm::new` refuses on (DN-28 §5): a transition row or the
/// initial probabilities outside `[0, 1]` or not summing to one. Called only when a
/// candidate's `filter_selection` is `"imm-cv-ct"` -- these fields are meaningless for
/// every other selection and are not validated for one.
///
/// # Errors
///
/// [`ConfigError::Invalid`], naming `what` (the candidate or the legacy `tracking`
/// field) and which rule failed.
fn validate_imm_fields(
    turn_rate_rad_s: f64,
    mode_transition: [[f64; 2]; 2],
    initial_mode_probabilities: [f64; 2],
    what: &str,
) -> Result<(), ConfigError> {
    if !turn_rate_rad_s.is_finite() {
        return Err(ConfigError::Invalid(format!(
            "{what} selects imm-cv-ct with a non-finite turn rate"
        )));
    }
    for row in mode_transition {
        if row.iter().any(|v| !v.is_finite() || *v < 0.0 || *v > 1.0) {
            return Err(ConfigError::Invalid(format!(
                "{what}'s imm-cv-ct mode transition matrix has an entry outside [0, 1]"
            )));
        }
        let sum: f64 = row.iter().sum();
        if (sum - 1.0).abs() > IMM_STOCHASTIC_TOLERANCE {
            return Err(ConfigError::Invalid(format!(
                "{what}'s imm-cv-ct mode transition matrix has a row that does not sum to one"
            )));
        }
    }
    if initial_mode_probabilities
        .iter()
        .any(|v| !v.is_finite() || *v < 0.0 || *v > 1.0)
    {
        return Err(ConfigError::Invalid(format!(
            "{what}'s imm-cv-ct initial mode probabilities have an entry outside [0, 1]"
        )));
    }
    let sum: f64 = initial_mode_probabilities.iter().sum();
    if (sum - 1.0).abs() > IMM_STOCHASTIC_TOLERANCE {
        return Err(ConfigError::Invalid(format!(
            "{what}'s imm-cv-ct initial mode probabilities do not sum to one"
        )));
    }
    Ok(())
}

/// A candidate's (or the legacy `tracking` field's) measurement-noise variance, checked
/// unconditionally -- unlike the `imm-cv-ct` fields, every filter selection uses this
/// one (DN-30 §5). An axis at or below zero states no error, which is not a measurement
/// noise; DN-28 §7's finding is that `PipelineSettings::default()`'s placeholder figure
/// was the wrong number for every scenario's sensor, not that having a number was wrong
/// -- so this validates a real one rather than accepting anything that parses.
///
/// # Errors
///
/// [`ConfigError::Invalid`], naming `what` (the candidate or `tracking`) and which axis.
fn validate_measurement_noise_var(variance: [f64; 3], what: &str) -> Result<(), ConfigError> {
    const AXES: [&str; 3] = ["east", "north", "height"];
    for (value, axis) in variance.iter().zip(AXES) {
        if !(value.is_finite() && *value > 0.0) {
            return Err(ConfigError::Invalid(format!(
                "{what}'s measurement_noise_var.{axis} must be finite and positive"
            )));
        }
    }
    Ok(())
}

/// Rules 2 and 4 for one profile (DN-24 §6).
///
/// # Errors
///
/// [`ConfigError::Invalid`] when the profile has duplicate candidate names, or anything
/// other than exactly one promoted candidate.
fn validate_one_profile(baseline: &ConfigBaseline, profile: &str) -> Result<(), ConfigError> {
    let in_profile: Vec<&TrackingProfileConfig> = baseline
        .tracking_profiles
        .iter()
        .filter(|c| c.profile == profile)
        .collect();
    // Rule 4.
    let names: Vec<String> = in_profile.iter().map(|c| c.name.clone()).collect();
    if has_duplicate_names(&names) {
        return Err(ConfigError::Invalid(format!(
            "profile {profile:?} has two candidates with the same name, so a rollback could not say which it restored"
        )));
    }
    // Rule 2.
    match in_profile.iter().filter(|c| c.promoted).count() {
        1 => Ok(()),
        0 => Err(ConfigError::Invalid(format!(
            "profile {profile:?} promotes no candidate, so this deployment cannot say which configuration is in force"
        ))),
        n => Err(ConfigError::Invalid(format!(
            "profile {profile:?} promotes {n} candidates; exactly one may be in force"
        ))),
    }
}

fn has_duplicate_names(names: &[String]) -> bool {
    let mut seen: Vec<&str> = Vec::with_capacity(names.len());
    for n in names {
        if seen.contains(&n.as_str()) {
            return true;
        }
        seen.push(n);
    }
    false
}

/// Anomaly detector thresholds (docs/design/DN-15-anomaly-detectors.md §6, GAP-021).
///
/// Every configured threshold is finite and positive. A detector that is not configured
/// is off, which is a deployment's choice and not a defect; a detector configured with a
/// nonsense threshold would fire on everything or nothing, which is the same as being off
/// while claiming to run.
///
/// # Errors
///
/// [`ConfigError::Invalid`] naming the detector and the field.
fn validate_anomaly(baseline: &ConfigBaseline) -> Result<(), ConfigError> {
    let a = &baseline.analytics.anomaly;
    let positive = |name: &str, v: f64| -> Result<(), ConfigError> {
        if v.is_finite() && v > 0.0 {
            Ok(())
        } else {
            Err(ConfigError::Invalid(format!(
                "analytics.anomaly.{name} must be finite and positive"
            )))
        }
    };
    if let Some(l) = a.loitering {
        positive("loitering.max_speed_mps", l.max_speed_mps)?;
        positive("loitering.min_duration_s", l.min_duration_s)?;
    }
    if let Some(k) = a.kinematics {
        positive("kinematics.max_speed_mps", k.max_speed_mps)?;
        positive("kinematics.max_climb_rate_mps", k.max_climb_rate_mps)?;
    }
    if let Some(f) = a.feed {
        // A silence factor below one would flag a feed for being on time.
        if !(f.silence_factor.is_finite() && f.silence_factor >= 1.0) {
            return Err(ConfigError::Invalid(
                "analytics.anomaly.feed.silence_factor must be at least 1".into(),
            ));
        }
        if !(f.rate_tolerance.is_finite() && f.rate_tolerance > 0.0 && f.rate_tolerance <= 1.0) {
            return Err(ConfigError::Invalid(
                "analytics.anomaly.feed.rate_tolerance must be in (0, 1]".into(),
            ));
        }
    }
    if let Some(c) = a.cooperative {
        positive("cooperative.lost_after_s", c.lost_after_s)?;
        positive("cooperative.max_separation_m", c.max_separation_m)?;
    }
    Ok(())
}

/// Battle-rhythm rules (docs/design/DN-21-battle-rhythm.md §6, GAP-054).
///
/// **The endpoint check is the one that matters.** A scheduled product naming an endpoint
/// this baseline does not declare would be produced every cycle and delivered nowhere, and
/// the deployment would look like it was reporting. Absent is a different thing entirely:
/// the product is produced and held for a person, which is a real configuration.
///
/// # Errors
///
/// [`ConfigError::Invalid`] naming the first product or window that does not check out.
fn validate_rhythm(baseline: &ConfigBaseline) -> Result<(), ConfigError> {
    for product in &baseline.reporting.scheduled {
        if product.name.trim().is_empty() {
            return Err(ConfigError::Invalid(
                "a scheduled product has no name".into(),
            ));
        }
        if ScheduledProductConfig::parse_kind(&product.kind).is_none() {
            return Err(ConfigError::Invalid(format!(
                "scheduled product {:?} has unknown kind {:?}",
                product.name, product.kind
            )));
        }
        if !(product.period_s.is_finite() && product.period_s > 0.0) {
            return Err(ConfigError::Invalid(format!(
                "scheduled product {:?} must have a finite, positive period",
                product.name
            )));
        }
        // An offset at or beyond the period is the same rhythm with a confusing
        // description of itself, and a negative one schedules into the past.
        if !(product.offset_s.is_finite()
            && product.offset_s >= 0.0
            && product.offset_s < product.period_s)
        {
            return Err(ConfigError::Invalid(format!(
                "scheduled product {:?} must have an offset in [0, period)",
                product.name
            )));
        }
        if let Some(endpoint) = &product.deliver_to {
            if !baseline.declares_endpoint(endpoint) {
                return Err(ConfigError::Invalid(format!(
                    "scheduled product {:?} delivers to undeclared endpoint {endpoint:?}",
                    product.name
                )));
            }
        }
    }

    for sensor in &baseline.sensors {
        let mut windows: Vec<&MaintenanceWindowConfig> = Vec::new();
        for w in &sensor.maintenance {
            if !(w.from_s.is_finite() && w.to_s.is_finite() && w.to_s > w.from_s) {
                return Err(ConfigError::Invalid(format!(
                    "sensor {} has a maintenance window that does not end after it starts",
                    sensor.id
                )));
            }
            if w.reason.trim().is_empty() {
                return Err(ConfigError::Invalid(format!(
                    "sensor {} has a maintenance window with no reason",
                    sensor.id
                )));
            }
            // Overlapping windows would put one sensor in two maintenance states at
            // once, and the overrun of the first would be hidden by the second.
            if let Some(clash) = windows
                .iter()
                .find(|o| w.from_s < o.to_s && o.from_s < w.to_s)
            {
                return Err(ConfigError::Invalid(format!(
                    "sensor {} has overlapping maintenance windows at {} and {}",
                    sensor.id, clash.from_s, w.from_s
                )));
            }
            windows.push(w);
        }
    }
    Ok(())
}

/// Policy-section rules (docs/design/DN-08-policy-configuration.md).
///
/// Action and role names are checked for **shape only** here: this crate may not depend
/// on `gungnir-security`, so the canonical lists are checked by
/// [`validate_authority_names`], which the caller supplies the vocabulary to and which
/// [`FileConfigStore`] runs on every `apply`. An empty name is rejected here, because a
/// misspelled action grants nothing and looks like a grant.
fn validate_policy(baseline: &ConfigBaseline) -> Result<(), ConfigError> {
    let p = &baseline.policy;
    for (class, threshold) in &p.identification.thresholds {
        if !(threshold.is_finite() && (0.0..=1.0).contains(threshold)) {
            return Err(ConfigError::Invalid(format!(
                "identification threshold for {class:?} is outside 0.0 to 1.0"
            )));
        }
    }
    if !(p.identification.minimum_margin.is_finite()
        && (0.0..=1.0).contains(&p.identification.minimum_margin))
    {
        return Err(ConfigError::Invalid(
            "identification.minimum_margin is outside 0.0 to 1.0".into(),
        ));
    }
    let default_staleness_ok = p.staleness.default_s.is_finite() && p.staleness.default_s > 0.0;
    if !p.staleness.by_class_s.is_empty() && !default_staleness_ok {
        return Err(ConfigError::Invalid(
            "staleness.default_s must be finite and positive when by_class_s is set".into(),
        ));
    }
    for (class, seconds) in &p.staleness.by_class_s {
        if !(seconds.is_finite() && *seconds > 0.0) {
            return Err(ConfigError::Invalid(format!(
                "staleness for {class:?} must be finite and positive"
            )));
        }
    }
    validate_decision_rules(p)?;
    let a = &baseline.assessment;
    if a.prediction_horizons_s.is_empty() {
        return Err(ConfigError::Invalid(
            "assessment.prediction_horizons_s is empty".into(),
        ));
    }
    if a.prediction_horizons_s
        .iter()
        .any(|h| !(h.is_finite() && *h > 0.0))
    {
        return Err(ConfigError::Invalid(
            "assessment.prediction_horizons_s must be finite and positive".into(),
        ));
    }
    if a.prediction_horizons_s.windows(2).any(|w| w[0] >= w[1]) {
        return Err(ConfigError::Invalid(
            "assessment.prediction_horizons_s must be ascending".into(),
        ));
    }
    if !(a.max_range_m.is_finite() && a.max_range_m > 0.0) {
        return Err(ConfigError::Invalid(
            "assessment.max_range_m must be finite and positive".into(),
        ));
    }
    for (layer, window) in &a.effect_window_s {
        if !(window.is_finite() && *window > 0.0) {
            return Err(ConfigError::Invalid(format!(
                "assessment.effect_window_s for {layer:?} must be finite and positive"
            )));
        }
    }
    let fires_ok = p.fires.max_location_error_m.is_finite()
        && p.fires.max_location_error_m > 0.0
        && p.fires.minimum_separation_m.is_finite()
        && p.fires.minimum_separation_m > 0.0;
    if !fires_ok {
        return Err(ConfigError::Invalid(
            "fires limits must be finite and positive".into(),
        ));
    }
    if let Some(w) = &baseline.validity {
        if !w.valid_from.0.is_finite() {
            return Err(ConfigError::Invalid(
                "validity.valid_from is not finite".into(),
            ));
        }
        if let Some(until) = w.valid_until {
            if !until.0.is_finite() || until <= w.valid_from {
                return Err(ConfigError::Invalid(
                    "validity.valid_until must be finite and after valid_from".into(),
                ));
            }
        }
    }
    Ok(())
}

/// The validation rules every baseline must pass before it is applied.
/// DN-22 §6: **no key material, no secret, and no path to one in a baseline.**
///
/// Checked rather than trusted, because a configuration file is version-controlled,
/// hand-edited and copied between machines, and a pasted key in one is the easiest way
/// for material to end up somewhere nobody is watching.
fn validate_security(baseline: &ConfigBaseline) -> Result<(), ConfigError> {
    // DN-23 §5 rule 6: the baseline names where accounts live, never what they are.
    if let AuthenticationProvider::LocalAccounts { accounts_path } =
        &baseline.security.authentication.provider
    {
        if accounts_path.trim().is_empty() {
            return Err(ConfigError::Invalid(
                "security.authentication names local accounts with an empty path".into(),
            ));
        }
        if gungnir_model::looks_like_key_material(accounts_path)
            || accounts_path.contains("$argon2")
        {
            return Err(ConfigError::Invalid(
                "security.authentication.accounts_path looks like credential material; \
                 the baseline names a file, never a secret (DN-23 §5 rule 6)"
                    .into(),
            ));
        }
    }
    if let Some(s) = baseline.security.authentication.session_lifetime_s {
        if !(s.is_finite() && s > 0.0) {
            return Err(ConfigError::Invalid(
                "security.authentication.session_lifetime_s must be finite and positive".into(),
            ));
        }
    }
    let provider = &baseline.security.key_provider;
    for value in provider.values() {
        if gungnir_model::looks_like_key_material(value) {
            // The offending value is **not** repeated in the error: an error message is
            // the easiest way for key material to reach a log.
            return Err(ConfigError::Invalid(
                "the security section contains a value that looks like key material; a \
                 baseline names where keys live and never what they are (DN-22 §6)"
                    .into(),
            ));
        }
    }
    if !provider.is_implemented() {
        return Err(ConfigError::Invalid(format!(
            "the configured key provider is designed and not built ({}); a deployment \
             that started with it would journal in the clear while the baseline said \
             otherwise",
            provider.owning_gap().unwrap_or("GAP-084")
        )));
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
pub fn validate(baseline: &ConfigBaseline) -> Result<(), ConfigError> {
    validate_security(baseline)?;
    if baseline.version > SUPPORTED_CONFIG_VERSION {
        return Err(ConfigError::VersionTooNew {
            found: baseline.version,
            supported: SUPPORTED_CONFIG_VERSION,
        });
    }
    if has_duplicate_ids(baseline.sensors.iter().map(|s| s.id).collect()) {
        return Err(ConfigError::Invalid("duplicate sensor id".into()));
    }
    if has_duplicate_ids(baseline.resources.iter().map(|r| r.id).collect()) {
        return Err(ConfigError::Invalid("duplicate resource id".into()));
    }
    for s in &baseline.sensors {
        if s.modality.trim().is_empty() {
            return Err(ConfigError::Invalid(format!(
                "sensor {} has an empty modality",
                s.id
            )));
        }
        if s.position.iter().any(|v| !v.is_finite())
            || !(s.max_range_m.is_finite() && s.max_range_m > 0.0)
        {
            return Err(ConfigError::Invalid(format!(
                "sensor {} has a non-finite position or range",
                s.id
            )));
        }
    }
    validate_resources(baseline)?;
    validate_laydowns(baseline)?;
    validate_assets(baseline)?;
    validate_endpoints(baseline)?;
    validate_policy(baseline)?;
    validate_rhythm(baseline)?;
    validate_profiles(baseline)?;
    validate_anomaly(baseline)?;
    // DN-13 §6: the candidate bound is positive, or the search scores nothing and reports
    // "no change helps" about a sector it never looked at.
    if baseline.analytics.max_sensor_plan_candidates == 0 {
        return Err(ConfigError::Invalid(
            "analytics.max_sensor_plan_candidates must be at least 1".into(),
        ));
    }
    validate_ui(baseline)?;
    validate_vocabulary(baseline)?;
    validate_origin(baseline)?;
    validate_approaches(baseline)?;
    validate_hazards(baseline)?;
    validate_geofences(baseline)?;
    validate_handoff_endpoints(baseline)?;
    validate_terrain(baseline)?;
    validate_radar_feeds(baseline)?;
    validate_ais_feeds(baseline)?;
    validate_adsb_feeds(baseline)?;
    validate_sapient_feeds(baseline)?;
    validate_exchange(baseline)?;
    validate_machine_identities(baseline)?;
    for (class, weight) in &baseline.assessment.lethality_by_class {
        if !(weight.is_finite() && *weight >= 0.0) {
            return Err(ConfigError::Invalid(format!(
                "assessment.lethality_by_class[{class:?}] must be finite and non-negative"
            )));
        }
    }
    if baseline.reporting.retention_sessions == 0 {
        return Err(ConfigError::Invalid(
            "reporting.retention_sessions must be at least 1".into(),
        ));
    }
    validate_trust_roots(baseline)?;
    validate_escrow(baseline)?;
    validate_peers(baseline)?;
    validate_sensor_control(baseline)?;
    if let Some(t) = &baseline.tracking {
        if !(t.gate_threshold.is_finite() && t.gate_threshold > 0.0) {
            return Err(ConfigError::Invalid(
                "tracking.gate_threshold must be finite and positive".into(),
            ));
        }
        if t.filter_selection.trim().is_empty() {
            return Err(ConfigError::Invalid(
                "tracking.filter_selection is empty".into(),
            ));
        }
        validate_measurement_noise_var(t.measurement_noise_var, "tracking")?;
        if t.filter_selection == "imm-cv-ct" {
            validate_imm_fields(
                t.imm_turn_rate_rad_s,
                t.imm_mode_transition,
                t.imm_initial_mode_probabilities,
                "tracking",
            )?;
        }
    }
    if let BackendConfig::Remote { endpoint } = &baseline.backend {
        if endpoint.trim().is_empty() {
            return Err(ConfigError::Invalid(
                "backend.remote.endpoint is empty".into(),
            ));
        }
    }
    if baseline.allocation_horizon == 0 {
        return Err(ConfigError::Invalid(
            "allocation_horizon must be at least 1".into(),
        ));
    }
    if baseline.data_dir.trim().is_empty() {
        return Err(ConfigError::Invalid("data_dir is empty".into()));
    }
    Ok(())
}

/// Check every authority rule against the names this build knows.
///
/// Separate from [`validate`] because it needs something [`validate`] cannot have: the
/// canonical action and role lists from `gungnir-security`. Run wherever a baseline is
/// put into force -- [`FileConfigStore`] runs it on every `apply`, and both binaries run
/// it at load.
///
/// **Why this is worth its own function rather than a lint.** Every other validation
/// failure in this file is loud: a bad number is refused and nothing runs. A misspelled
/// action is quiet. `AuthoritySettings::rule_for` matches on the exact string, so
/// `"plan.decid"` matches no request, the role is denied every time, and the baseline
/// still reads as though the authority was granted. The person who wrote the rule has no
/// way to tell from the file or from the console that it does nothing.
///
/// # Errors
///
/// [`ConfigError::UnknownAuthorityName`] naming the first rule that does not check out,
/// or [`ConfigError::Invalid`] when the vocabulary itself is empty -- accepting every
/// name because the caller supplied no names would be the failure this exists to prevent.
pub fn validate_authority_names(
    baseline: &ConfigBaseline,
    known: &KnownVocabulary,
) -> Result<(), ConfigError> {
    if known.is_empty() {
        return Err(ConfigError::Invalid(
            "no known action or role names were supplied, so authority rules cannot be checked"
                .into(),
        ));
    }
    for rule in &baseline.policy.authority.rules {
        if !known.actions.iter().any(|a| a == &rule.action) {
            return Err(ConfigError::UnknownAuthorityName {
                kind: "action",
                name: rule.action.clone(),
            });
        }
        if !known.roles.iter().any(|r| r == &rule.role) {
            return Err(ConfigError::UnknownAuthorityName {
                kind: "role",
                name: rule.role.clone(),
            });
        }
    }
    Ok(())
}

/// Loads, validates, and (once approved) hot-swaps a `ConfigBaseline` without a
/// rebuild. Validation is mandatory before a baseline is ever applied.
pub trait ConfigStore {
    fn load(&self) -> Result<ConfigBaseline, ConfigError>;
    fn validate(&self, baseline: &ConfigBaseline) -> Result<(), ConfigError>;
    /// Put a baseline into force at `now`.
    ///
    /// `now` is required rather than read from a clock inside the implementation:
    /// promotion is a time-dependent act (DN-08 §5, a baseline outside its window may not
    /// be promoted), and an implementation that fetched its own time could disagree with
    /// the clock the rest of the session runs on.
    ///
    /// # Errors
    ///
    /// Whatever validation rejects, [`ConfigError::UnknownAuthorityName`] for a rule this
    /// build cannot honour, or [`ConfigError::NotPromotable`] outside the validity window.
    fn apply(
        &mut self,
        baseline: ConfigBaseline,
        now: gungnir_model::MissionTime,
    ) -> Result<(), ConfigError>;
}

/// A JSON file on disk. `apply` validates, writes, and remembers the applied
/// baseline; `load` reads whatever is on disk (validating the version only).
#[derive(Debug, Clone)]
pub struct FileConfigStore {
    path: PathBuf,
    applied: Option<ConfigBaseline>,
    known: KnownVocabulary,
}

impl FileConfigStore {
    /// The vocabulary is a **required** argument, not an option with a permissive
    /// default. A store that could be built without one would let a caller skip the only
    /// check that catches a misspelled authority rule, and skipping it looks exactly like
    /// passing it.
    pub fn new(path: impl Into<PathBuf>, known: KnownVocabulary) -> Self {
        Self {
            path: path.into(),
            applied: None,
            known,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The most recently applied baseline in this process, if any.
    pub fn applied(&self) -> Option<&ConfigBaseline> {
        self.applied.as_ref()
    }
}

impl ConfigStore for FileConfigStore {
    fn load(&self) -> Result<ConfigBaseline, ConfigError> {
        let text =
            std::fs::read_to_string(&self.path).map_err(|e| ConfigError::Io(e.to_string()))?;
        let baseline: ConfigBaseline =
            serde_json::from_str(&text).map_err(|e| ConfigError::Encoding(e.to_string()))?;
        if baseline.version > SUPPORTED_CONFIG_VERSION {
            return Err(ConfigError::VersionTooNew {
                found: baseline.version,
                supported: SUPPORTED_CONFIG_VERSION,
            });
        }
        Ok(baseline)
    }

    fn validate(&self, baseline: &ConfigBaseline) -> Result<(), ConfigError> {
        validate(baseline)?;
        validate_authority_names(baseline, &self.known)
    }

    fn apply(
        &mut self,
        baseline: ConfigBaseline,
        now: gungnir_model::MissionTime,
    ) -> Result<(), ConfigError> {
        validate(&baseline)?;
        validate_authority_names(&baseline, &self.known)?;
        // DN-08 §5: outside its window a baseline may be read, replayed and inspected --
        // it may not be promoted. Checked here rather than in `validate` because it is
        // not a property of the file; the same baseline is promotable tomorrow and not
        // today, and calling that "invalid" would be wrong in both directions.
        if let Some(w) = baseline.validity {
            if !w.contains(now) {
                return Err(ConfigError::NotPromotable {
                    now,
                    valid_from: w.valid_from,
                    valid_until: w.valid_until,
                });
            }
        }
        // The revision in force is what this store applied, or failing that what is on
        // disk; a store with neither (a fresh deployment) has nothing to advance past.
        let in_force = self
            .applied
            .as_ref()
            .map(|b| b.revision)
            .or_else(|| self.load().ok().map(|b| b.revision));
        if let Some(in_force) = in_force {
            if baseline.revision <= in_force {
                return Err(ConfigError::RevisionNotAdvanced {
                    in_force,
                    candidate: baseline.revision,
                });
            }
        }
        let text = serde_json::to_string_pretty(&baseline)
            .map_err(|e| ConfigError::Encoding(e.to_string()))?;
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| ConfigError::Io(e.to_string()))?;
            }
        }
        std::fs::write(&self.path, text).map_err(|e| ConfigError::Io(e.to_string()))?;
        self.applied = Some(baseline);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::MissionTime;

    /// DN-22 §6's criterion, and the one an accreditor asks about: **no baseline field
    /// accepts key material.** The schema is the real boundary -- there is nowhere to put
    /// a key -- and this check catches somebody pasting one into a field meant for a
    /// provider name.
    #[test]
    fn a_pasted_key_is_refused_rather_than_stored() {
        let baseline = ConfigBaseline {
            security: SecurityConfig {
                key_provider: KeyProviderConfig::ManagedService {
                    endpoint: "kms.example".into(),
                    // Somebody pasting the key itself where a resource name belongs.
                    key_ring: "0123456789abcdef0123456789abcdef0123456789abcdef".into(),
                },
                authentication: AuthenticationConfig::default(),
                tls: TlsClientConfig::default(),
                escrow: None,
            },
            ..ConfigBaseline::default()
        };
        let err = validate(&baseline).expect_err("refused");
        let message = err.to_string();
        assert!(message.contains("looks like key material"), "{message}");
        // The offending value must not be repeated: an error message is the easiest way
        // for key material to reach a log.
        assert!(
            !message.contains("0123456789abcdef"),
            "the key is in the error"
        );
    }

    /// A resource path is a **reference** to where material lives, which is exactly what
    /// a baseline should contain. Flagging it would make the check unusable.
    #[test]
    fn a_key_service_resource_path_is_not_mistaken_for_material() {
        let baseline = ConfigBaseline {
            security: SecurityConfig {
                key_provider: KeyProviderConfig::ManagedService {
                    endpoint: "https://kms.example.gov/v1".into(),
                    key_ring: "projects/gungnir/locations/eu/keyRings/journal".into(),
                },
                authentication: AuthenticationConfig::default(),
                tls: TlsClientConfig::default(),
                escrow: None,
            },
            ..ConfigBaseline::default()
        };
        // Refused for being unbuilt, not for looking like a key -- which is the point.
        let message = validate(&baseline).expect_err("unbuilt").to_string();
        assert!(!message.contains("looks like key material"), "{message}");
        assert!(message.contains("GAP-084"), "{message}");
    }

    /// A provider that is designed and unbuilt is refused at validation, so a deployment
    /// learns it at start-up rather than discovering an unencrypted journal later.
    #[test]
    fn an_unbuilt_provider_is_refused_at_validation() {
        for provider in [
            KeyProviderConfig::OperatingSystemKeystore {
                account: "gungnir".into(),
            },
            KeyProviderConfig::ManagedService {
                endpoint: "https://kms.example.gov".into(),
                key_ring: "journal".into(),
            },
        ] {
            let baseline = ConfigBaseline {
                security: SecurityConfig {
                    key_provider: provider.clone(),
                    authentication: AuthenticationConfig::default(),
                    tls: TlsClientConfig::default(),
                    escrow: None,
                },
                ..ConfigBaseline::default()
            };
            let message = validate(&baseline).expect_err("refused").to_string();
            assert!(message.contains("designed and not built"), "{message}");
            assert_eq!(provider.owning_gap(), Some("GAP-084"));
            assert!(!provider.is_implemented());
        }
    }

    /// The default is no custody, which is the honest state of every deployment that has
    /// not set a keystore up. It validates, because refusing it would stop the system
    /// running for want of a feature it is truthfully reporting it does not have.
    #[test]
    fn the_default_is_no_custody_and_it_validates() {
        let baseline = ConfigBaseline::default();
        assert_eq!(baseline.security.key_provider, KeyProviderConfig::None);
        assert!(validate(&baseline).is_ok());
        assert!(baseline.security.key_provider.is_implemented());
    }

    /// The ephemeral provider validates and is named for the property that matters:
    /// a journal sealed under it cannot be read after a restart.
    #[test]
    fn the_ephemeral_provider_validates_and_is_named_for_what_it_loses() {
        let baseline = ConfigBaseline {
            security: SecurityConfig {
                key_provider: KeyProviderConfig::Ephemeral,
                authentication: AuthenticationConfig::default(),
                tls: TlsClientConfig::default(),
                escrow: None,
            },
            ..ConfigBaseline::default()
        };
        assert!(validate(&baseline).is_ok());
    }

    /// The section round-trips, so a baseline written by one version reads in another.
    #[test]
    fn the_security_section_round_trips() {
        let baseline = ConfigBaseline {
            security: SecurityConfig {
                key_provider: KeyProviderConfig::OperatingSystemKeystore {
                    account: "gungnir".into(),
                },
                authentication: AuthenticationConfig::default(),
                tls: TlsClientConfig::default(),
                escrow: None,
            },
            ..ConfigBaseline::default()
        };
        let json = serde_json::to_string(&baseline).expect("encoded");
        let back: ConfigBaseline = serde_json::from_str(&json).expect("decoded");
        assert_eq!(back.security, baseline.security);
    }

    /// A baseline written before this section existed still loads, defaulting to no
    /// custody. Refusing it would make every existing baseline invalid.
    #[test]
    fn a_baseline_without_the_section_still_loads() {
        let json = serde_json::to_string(&ConfigBaseline::default()).expect("encoded");
        let section = format!(
            r#","security":{}"#,
            serde_json::to_string(&SecurityConfig::default()).expect("encoded")
        );
        let stripped = json.replace(&section, "");
        assert_ne!(
            stripped, json,
            "the section was not where the test expected it"
        );
        let back: ConfigBaseline = serde_json::from_str(&stripped).expect("decoded");
        assert_eq!(back.security.key_provider, KeyProviderConfig::None);
    }

    /// D-17's rule that decision dialogs stay with the queue is enforced at load, not
    /// at draw time: a baseline that detached PN-07 would separate a decision from the
    /// queue it came from, and refusing it is the only place that cannot be forgotten.
    #[test]
    fn a_baseline_cannot_detach_the_decision_dialog() {
        use gungnir_model::{LayoutNode, RoleLayout, UiSettings};
        use std::collections::BTreeMap;

        let mut baseline = ConfigBaseline {
            ui: UiSettings {
                scene_3d: false,
                layouts: BTreeMap::from([(
                    "Operator".to_owned(),
                    RoleLayout {
                        main: LayoutNode::stack(&["PN-06", "PN-07"]),
                        detached: vec!["PN-07".to_owned()],
                    },
                )]),
            },
            ..ConfigBaseline::default()
        };
        let err = validate(&baseline).expect_err("detaching PN-07 must be refused");
        assert!(
            err.to_string().contains("PN-07"),
            "the refusal must name the panel: {err}"
        );

        // The three D-17 allows are accepted.
        for pn in DETACHABLE_PANELS {
            baseline.ui.layouts.insert(
                "Operator".to_owned(),
                RoleLayout {
                    main: LayoutNode::stack(&["PN-06", "PN-02", "PN-12"]),
                    detached: vec![pn.to_owned()],
                },
            );
            validate(&baseline).unwrap_or_else(|e| panic!("{pn} must be detachable: {e}"));
        }
    }

    /// A panel that does not exist is refused rather than silently ignored at draw
    /// time: an administrator who mistyped an identifier would otherwise get a screen
    /// missing a panel with nothing to say why.
    #[test]
    fn a_baseline_naming_an_unknown_panel_is_refused() {
        use gungnir_model::{LayoutNode, RoleLayout, UiSettings};
        use std::collections::BTreeMap;

        let baseline = ConfigBaseline {
            ui: UiSettings {
                scene_3d: false,
                layouts: BTreeMap::from([(
                    "Operator".to_owned(),
                    RoleLayout {
                        main: LayoutNode::stack(&["PN-06", "PN-99"]),
                        detached: Vec::new(),
                    },
                )]),
            },
            ..ConfigBaseline::default()
        };
        let err = validate(&baseline).expect_err("PN-99 does not exist");
        assert!(err.to_string().contains("PN-99"));
    }

    /// Empty containers, mismatched shares and duplicated panels are all refused: each
    /// would draw a screen that does not match what the baseline appears to describe.
    #[test]
    fn malformed_arrangements_are_refused() {
        use gungnir_model::{LayoutNode, RoleLayout, UiSettings};
        use std::collections::BTreeMap;

        let bad = [
            LayoutNode::Vertical {
                children: Vec::new(),
                shares: Vec::new(),
            },
            LayoutNode::Horizontal {
                children: vec![
                    LayoutNode::Panel { pn: "PN-06".into() },
                    LayoutNode::Panel { pn: "PN-03".into() },
                ],
                shares: vec![0.5],
            },
            LayoutNode::Horizontal {
                children: vec![
                    LayoutNode::Panel { pn: "PN-06".into() },
                    LayoutNode::Panel { pn: "PN-03".into() },
                ],
                shares: vec![0.5, -1.0],
            },
            LayoutNode::stack(&["PN-06", "PN-06"]),
        ];
        for main in bad {
            let baseline = ConfigBaseline {
                ui: UiSettings {
                    scene_3d: false,
                    layouts: BTreeMap::from([(
                        "Operator".to_owned(),
                        RoleLayout {
                            main: main.clone(),
                            detached: Vec::new(),
                        },
                    )]),
                },
                ..ConfigBaseline::default()
            };
            assert!(
                validate(&baseline).is_err(),
                "this arrangement should have been refused: {main:?}"
            );
        }
    }

    /// A mistyped override key is refused rather than ignored. Ignoring it would leave
    /// the administrator looking at the default word with nothing to say why their
    /// rename did not take.
    #[test]
    fn a_vocabulary_override_for_an_unknown_term_is_refused() {
        use gungnir_model::Vocabulary;
        use std::collections::BTreeMap;

        let baseline = ConfigBaseline {
            vocabulary: Vocabulary {
                overrides: BTreeMap::from([(
                    "classification.hostle".to_owned(),
                    "Rouge".to_owned(),
                )]),
            },
            ..ConfigBaseline::default()
        };
        let err = validate(&baseline).expect_err("a mistyped key must be refused");
        assert!(
            err.to_string().contains("classification.hostle"),
            "the refusal must name the key: {err}"
        );
    }

    /// A term with no word cannot be shown, and a blank where a classification should
    /// be is the worst possible reading of a track.
    #[test]
    fn an_empty_vocabulary_label_is_refused() {
        use gungnir_model::Vocabulary;
        use std::collections::BTreeMap;

        for label in ["", "   "] {
            let baseline = ConfigBaseline {
                vocabulary: Vocabulary {
                    overrides: BTreeMap::from([(
                        "classification.hostile".to_owned(),
                        label.to_owned(),
                    )]),
                },
                ..ConfigBaseline::default()
            };
            assert!(
                validate(&baseline).is_err(),
                "an empty label was accepted for classification.hostile"
            );
        }
    }

    /// A real override is accepted and every other term keeps its default.
    #[test]
    fn a_valid_vocabulary_override_is_accepted() {
        use gungnir_model::{Classification, Vocabulary};
        use std::collections::BTreeMap;

        let baseline = ConfigBaseline {
            vocabulary: Vocabulary {
                overrides: BTreeMap::from([(
                    "classification.friendly".to_owned(),
                    "Blue".to_owned(),
                )]),
            },
            ..ConfigBaseline::default()
        };
        validate(&baseline).expect("a known key with a real label is valid");
        assert_eq!(
            baseline.vocabulary.classification(Classification::Friendly),
            "Blue"
        );
        assert_eq!(
            baseline.vocabulary.classification(Classification::Hostile),
            "Hostile"
        );
    }

    /// Degrees written where radians were meant is the mistake this catches, and it
    /// is the likely one: 55.0 is a plausible-looking latitude and is nine times round
    /// the planet in radians.
    #[test]
    fn a_local_frame_origin_in_degrees_is_refused() {
        let baseline = ConfigBaseline {
            origin: Some([55.0, 12.0, 0.0]),
            ..ConfigBaseline::default()
        };
        let err = validate(&baseline).expect_err("55 rad is not a latitude");
        assert!(
            err.to_string().contains("radians"),
            "the refusal must say what the units are: {err}"
        );

        let ok = ConfigBaseline {
            origin: Some([55.0_f64.to_radians(), 12.0_f64.to_radians(), 0.0]),
            ..ConfigBaseline::default()
        };
        validate(&ok).expect("a radian origin is valid");
    }

    /// A baseline with no origin is valid: most deployments have not declared one, and
    /// the consequence is that coverage cannot be placed, not that the baseline is bad.
    #[test]
    fn a_baseline_with_no_origin_is_valid() {
        let baseline = ConfigBaseline::default();
        assert!(baseline.origin.is_none());
        validate(&baseline).expect("no origin is not an error");
    }

    /// A non-finite origin would put the whole local frame nowhere.
    #[test]
    fn a_non_finite_origin_is_refused() {
        for bad in [
            [f64::NAN, 0.0, 0.0],
            [0.0, f64::INFINITY, 0.0],
            [0.0, 0.0, f64::NAN],
        ] {
            let baseline = ConfigBaseline {
                origin: Some(bad),
                ..ConfigBaseline::default()
            };
            assert!(validate(&baseline).is_err(), "{bad:?} was accepted");
        }
    }

    /// A baseline with no `ui` section is the ordinary case and stays valid.
    #[test]
    fn a_baseline_with_no_arrangement_is_valid() {
        let baseline = ConfigBaseline::default();
        assert!(baseline.ui.layouts.is_empty());
        validate(&baseline).expect("the default baseline is valid");
    }

    #[test]
    fn default_baseline_is_valid() {
        assert!(validate(&ConfigBaseline::default()).is_ok());
    }

    /// GAP-042, D-02: the certificate that may acknowledge a warning speaks for a
    /// `warning` endpoint, and the `kind` is what is checked -- a handoff row of the same
    /// name is an effector, and admitting it here would let an effector discharge
    /// warnings it was never sent.
    #[test]
    fn a_warned_party_speaks_for_a_warning_endpoint_and_not_a_handoff_one() {
        let mut b = ConfigBaseline {
            endpoints: vec![EndpointConfig {
                name: "port-authority".into(),
                kind: "handoff".into(),
                address: "http://127.0.0.1:9100/handoff".into(),
            }],
            machine_identities: vec![MachineIdentityConfig {
                common_name: "port-authority-1".into(),
                speaks_for: MachineRole::WarnedParty {
                    channel: "port-authority".into(),
                },
            }],
            ..ConfigBaseline::default()
        };
        let err = validate(&b).expect_err("a handoff endpoint is not a warning channel");
        assert!(err.to_string().contains("not a warning endpoint"), "{err}");

        b.endpoints[0].kind = "warning".into();
        validate(&b).expect("the channel is a warning endpoint now");

        // And a channel no endpoint declares is refused, as every other role's target is.
        b.machine_identities[0].speaks_for = MachineRole::WarnedParty {
            channel: "harbour-master".into(),
        };
        assert!(matches!(validate(&b), Err(ConfigError::Invalid(_))));
    }

    #[test]
    fn duplicate_sensor_ids_are_rejected() {
        let mut b = ConfigBaseline::default();
        let s = SensorConfig {
            id: 1,
            modality: "radar".into(),
            position: [0.0; 3],
            max_range_m: 1.0,
            control_endpoint: None,
            maintenance: Vec::new(),
        };
        b.sensors = vec![s.clone(), s];
        assert!(matches!(validate(&b), Err(ConfigError::Invalid(_))));
    }

    #[test]
    fn newer_version_is_refused() {
        let b = ConfigBaseline {
            version: SUPPORTED_CONFIG_VERSION + 1,
            ..ConfigBaseline::default()
        };
        assert!(matches!(
            validate(&b),
            Err(ConfigError::VersionTooNew { .. })
        ));
    }

    #[test]
    fn empty_remote_endpoint_is_rejected() {
        let b = ConfigBaseline {
            backend: BackendConfig::Remote {
                endpoint: " ".into(),
            },
            ..ConfigBaseline::default()
        };
        assert!(matches!(validate(&b), Err(ConfigError::Invalid(_))));
    }

    #[test]
    fn zero_capacity_resource_is_rejected() {
        let b = ConfigBaseline {
            resources: vec![ResourceConfig {
                handoff_endpoint: None,
                id: 1,
                position: [0.0; 3],
                capacity: 0,
                layer: "point".into(),
                cost: None,
                rounds_available: None,
                reserve: None,
                intercept_speed_mps: None,
            }],
            ..ConfigBaseline::default()
        };
        assert!(matches!(validate(&b), Err(ConfigError::Invalid(_))));
    }

    #[test]
    fn a_non_positive_intercept_speed_is_rejected_and_a_positive_one_reaches_the_view() {
        let resource = |speed: Option<f64>| ResourceConfig {
            handoff_endpoint: None,
            id: 1,
            position: [0.0; 3],
            capacity: 1,
            layer: "point".into(),
            cost: None,
            rounds_available: None,
            reserve: None,
            intercept_speed_mps: speed,
        };
        for bad in [0.0, -5.0, f64::NAN, f64::INFINITY] {
            let b = ConfigBaseline {
                resources: vec![resource(Some(bad))],
                ..ConfigBaseline::default()
            };
            assert!(
                matches!(validate(&b), Err(ConfigError::Invalid(_))),
                "{bad}"
            );
        }
        let b = ConfigBaseline {
            resources: vec![resource(Some(250.0))],
            ..ConfigBaseline::default()
        };
        validate(&b).expect("a positive speed is valid");
        assert_eq!(b.resources[0].to_view().intercept_speed_mps, Some(250.0));
        assert_eq!(resource(None).to_view().intercept_speed_mps, None);
    }

    #[test]
    fn a_radar_feed_needs_a_parsing_socket_known_sensors_and_unique_pairs() {
        let feed = |radars: Vec<RadarBindingConfig>, addr: &str| ConfigBaseline {
            sensors: vec![SensorConfig {
                id: 1,
                modality: "radar".into(),
                position: [0.9, 0.2, 10.0],
                max_range_m: 20_000.0,
                control_endpoint: None,
                maintenance: Vec::new(),
            }],
            radar_feeds: vec![RadarFeedConfig {
                name: "north".into(),
                bind_addr: addr.into(),
                multicast: None,
                radars,
            }],
            ..ConfigBaseline::default()
        };
        let one = |sensor_id, sac, sic| RadarBindingConfig {
            sensor_id,
            sac,
            sic,
        };
        validate(&feed(vec![one(1, 7, 3)], "0.0.0.0:8600")).expect("valid");
        // GAP-010: an AIS feed is validated the same way.
        let ais = |sensor_id, source| ConfigBaseline {
            sensors: vec![SensorConfig {
                id: 10,
                modality: "ais".into(),
                position: [0.9, 0.2, 10.0],
                max_range_m: 60_000.0,
                control_endpoint: None,
                maintenance: Vec::new(),
            }],
            ais_feeds: vec![AisFeedConfig {
                name: "kal".into(),
                sensor_id,
                source,
            }],
            ..ConfigBaseline::default()
        };
        validate(&ais(
            10,
            AisSource::Tcp {
                addr: "127.0.0.1:10110".into(),
            },
        ))
        .expect("a tcp feed");
        validate(&ais(
            10,
            AisSource::File {
                path: "testdata/ais/ais-nmea.log".into(),
            },
        ))
        .expect("a recorded feed");
        assert!(matches!(
            validate(&ais(
                11,
                AisSource::Tcp {
                    addr: "127.0.0.1:10110".into()
                }
            )),
            Err(ConfigError::Invalid(_))
        ));
        assert!(matches!(
            validate(&ais(
                10,
                AisSource::Tcp {
                    addr: "nowhere".into()
                }
            )),
            Err(ConfigError::Invalid(_))
        ));
        assert!(matches!(
            validate(&feed(vec![one(1, 7, 3)], "nowhere")),
            Err(ConfigError::Invalid(_))
        ));
        assert!(matches!(
            validate(&feed(vec![one(9, 7, 3)], "0.0.0.0:8600")),
            Err(ConfigError::Invalid(_))
        ));
        assert!(matches!(
            validate(&feed(vec![], "0.0.0.0:8600")),
            Err(ConfigError::Invalid(_))
        ));
        assert!(matches!(
            validate(&feed(vec![one(1, 7, 3), one(1, 7, 4)], "0.0.0.0:8600")),
            Err(ConfigError::Invalid(_))
        ));
    }

    #[test]
    fn an_adsb_feed_needs_a_known_sensor_a_unique_name_and_a_parsing_source() {
        let sensor = || SensorConfig {
            id: 20,
            modality: "ads-b".into(),
            position: [0.9, 0.2, 30.0],
            max_range_m: 400_000.0,
            control_endpoint: None,
            maintenance: Vec::new(),
        };
        let adsb = |sensor_id, source| ConfigBaseline {
            sensors: vec![sensor()],
            adsb_feeds: vec![AdsbFeedConfig {
                name: "lhr".into(),
                sensor_id,
                source,
            }],
            ..ConfigBaseline::default()
        };
        validate(&adsb(
            20,
            AdsbSource::Tcp {
                addr: "127.0.0.1:30003".into(),
            },
        ))
        .expect("a tcp feed");
        validate(&adsb(
            20,
            AdsbSource::File {
                path: "testdata/adsb/lax-messages-first40000.txt".into(),
            },
        ))
        .expect("a recorded feed");
        assert!(matches!(
            validate(&adsb(
                21,
                AdsbSource::Tcp {
                    addr: "127.0.0.1:30003".into()
                }
            )),
            Err(ConfigError::Invalid(_))
        ));
        assert!(matches!(
            validate(&adsb(
                20,
                AdsbSource::Tcp {
                    addr: "nowhere".into()
                }
            )),
            Err(ConfigError::Invalid(_))
        ));
        assert!(matches!(
            validate(&adsb(20, AdsbSource::File { path: "  ".into() })),
            Err(ConfigError::Invalid(_))
        ));
        let mut two = adsb(
            20,
            AdsbSource::Tcp {
                addr: "127.0.0.1:30003".into(),
            },
        );
        two.adsb_feeds.push(AdsbFeedConfig {
            name: "lhr".into(),
            sensor_id: 20,
            source: AdsbSource::Tcp {
                addr: "127.0.0.1:30004".into(),
            },
        });
        assert!(matches!(validate(&two), Err(ConfigError::Invalid(_))));
    }

    #[test]
    fn a_sapient_feed_needs_a_known_sensor_a_unique_name_a_node_type_and_a_parsing_source() {
        let sensor = || SensorConfig {
            id: 30,
            modality: "sapient".into(),
            position: [0.9, 0.2, 2.0],
            max_range_m: 5_000.0,
            control_endpoint: None,
            maintenance: Vec::new(),
        };
        let sapient = |sensor_id, node_type, source| ConfigBaseline {
            sensors: vec![sensor()],
            sapient_feeds: vec![SapientFeedConfig {
                name: "op-1".into(),
                sensor_id,
                node_type,
                source,
            }],
            ..ConfigBaseline::default()
        };
        for node_type in [
            SapientNodeType::Spotter,
            SapientNodeType::Acoustic,
            SapientNodeType::PassiveRf,
        ] {
            validate(&sapient(
                30,
                node_type,
                SapientSource::Tcp {
                    addr: "127.0.0.1:40000".into(),
                },
            ))
            .unwrap_or_else(|e| panic!("{node_type:?} is a valid feed: {e}"));
        }
        validate(&sapient(
            30,
            SapientNodeType::Spotter,
            SapientSource::File {
                path: "testdata/sapient/spotter-session.jsonl".into(),
            },
        ))
        .expect("a recorded feed");
        assert!(matches!(
            validate(&sapient(
                31,
                SapientNodeType::Spotter,
                SapientSource::Tcp {
                    addr: "127.0.0.1:40000".into()
                }
            )),
            Err(ConfigError::Invalid(_))
        ));
        assert!(matches!(
            validate(&sapient(
                30,
                SapientNodeType::Spotter,
                SapientSource::Tcp {
                    addr: "nowhere".into()
                }
            )),
            Err(ConfigError::Invalid(_))
        ));
        assert!(matches!(
            validate(&sapient(
                30,
                SapientNodeType::Acoustic,
                SapientSource::File { path: "  ".into() }
            )),
            Err(ConfigError::Invalid(_))
        ));
        let mut two = sapient(
            30,
            SapientNodeType::Spotter,
            SapientSource::Tcp {
                addr: "127.0.0.1:40000".into(),
            },
        );
        two.sapient_feeds.push(SapientFeedConfig {
            name: "op-1".into(),
            sensor_id: 30,
            node_type: SapientNodeType::Acoustic,
            source: SapientSource::Tcp {
                addr: "127.0.0.1:40001".into(),
            },
        });
        assert!(matches!(validate(&two), Err(ConfigError::Invalid(_))));
    }

    #[test]
    fn a_peer_needs_an_endpoint_a_quality_in_range_and_a_source_id_of_its_own() {
        let peer = |source_id, quality: f32, age: f64| ConfigBaseline {
            sensors: vec![SensorConfig {
                id: 1,
                modality: "radar".into(),
                position: [0.9, 0.2, 10.0],
                max_range_m: 20_000.0,
                control_endpoint: None,
                maintenance: Vec::new(),
            }],
            endpoints: vec![EndpointConfig {
                name: "kal".into(),
                kind: "peer".into(),
                address: "http://kal.local:7410".into(),
            }],
            peers: vec![PeerConfig {
                name: "kal-cell".into(),
                endpoint: "kal".into(),
                source_id,
                assigned_quality: quality,
                max_age_s: age,
            }],
            ..ConfigBaseline::default()
        };
        validate(&peer(900, 0.6, 30.0)).expect("valid");
        assert!(
            matches!(validate(&peer(1, 0.6, 30.0)), Err(ConfigError::Invalid(_))),
            "a sensor's id"
        );
        assert!(matches!(
            validate(&peer(900, 1.5, 30.0)),
            Err(ConfigError::Invalid(_))
        ));
        assert!(matches!(
            validate(&peer(900, 0.6, 0.0)),
            Err(ConfigError::Invalid(_))
        ));
    }

    #[test]
    fn an_escrow_section_carries_the_public_half_and_a_holder() {
        let escrow = |pem: &str, holder| ConfigBaseline {
            security: SecurityConfig {
                escrow: Some(EscrowConfig {
                    holder,
                    public_key_pem: pem.into(),
                }),
                ..SecurityConfig::default()
            },
            ..ConfigBaseline::default()
        };
        validate(&escrow(
            "-----BEGIN PUBLIC KEY-----\nMFkw\n-----END PUBLIC KEY-----",
            7,
        ))
        .expect("valid");
        let err = validate(&escrow(
            "-----BEGIN PRIVATE KEY-----\nMIGH\n-----END PRIVATE KEY-----",
            7,
        ))
        .expect_err("refused");
        assert!(!err.to_string().contains("MIGH"), "never repeated");
        assert!(matches!(
            validate(&escrow(
                "-----BEGIN PUBLIC KEY-----\nMFkw\n-----END PUBLIC KEY-----",
                0
            )),
            Err(ConfigError::Invalid(_))
        ));
    }

    #[test]
    fn a_terrain_entry_needs_a_known_format_and_the_local_frame() {
        let mut b = ConfigBaseline {
            terrain: Some(TerrainConfig {
                path: "dem/site.asc".into(),
                frame: "local-enu".into(),
            }),
            ..ConfigBaseline::default()
        };
        validate(&b).expect("an ASCII grid in the local frame is valid");
        b.terrain = Some(TerrainConfig {
            path: "dem/site.png".into(),
            frame: "local-enu".into(),
        });
        assert!(matches!(validate(&b), Err(ConfigError::Invalid(_))));
        b.terrain = Some(TerrainConfig {
            path: "dem/site.tif".into(),
            frame: "EPSG:32633".into(),
        });
        assert!(matches!(validate(&b), Err(ConfigError::Invalid(_))));
    }

    #[test]
    fn file_store_round_trips() {
        let path = std::env::temp_dir().join(format!("gungnir-config-{}.json", std::process::id()));
        let mut store = FileConfigStore::new(&path, test_vocabulary());
        let baseline = ConfigBaseline {
            sensors: vec![SensorConfig {
                id: 3,
                modality: "ads-b".into(),
                position: [0.1, 0.2, 5.0],
                max_range_m: 100.0,
                control_endpoint: None,
                maintenance: Vec::new(),
            }],
            resources: vec![ResourceConfig {
                handoff_endpoint: None,
                id: 9,
                position: [0.1, 0.2, 0.0],
                capacity: 2,
                layer: "point".into(),
                cost: None,
                rounds_available: None,
                reserve: None,
                intercept_speed_mps: None,
            }],
            backend: BackendConfig::Remote {
                endpoint: "http://node.local:7410".into(),
            },
            ..ConfigBaseline::default()
        };
        store
            .apply(baseline.clone(), MissionTime(0.0))
            .expect("apply");
        assert_eq!(store.load().expect("load"), baseline);
        assert_eq!(store.applied(), Some(&baseline));
        assert_eq!(baseline.resource_views()[0].id, ResourceId(9));
        let _ = std::fs::remove_file(&path);
    }

    /// Stands in for `gungnir_security::actions::ALL` and `Role::ALL`, which this crate
    /// may not reach. The binaries pass the real lists.
    fn test_vocabulary() -> KnownVocabulary {
        KnownVocabulary::new(["plan.decide", "config.apply"], ["Supervisor", "Commander"])
    }

    fn with_rule(action: &str, role: &str) -> ConfigBaseline {
        let mut b = ConfigBaseline::default();
        b.policy.authority.rules.push(gungnir_model::AuthorityRule {
            action: action.into(),
            role: role.into(),
            layer: None,
            class: None,
            pre_delegated: false,
        });
        b
    }

    /// **The failure this check exists for.** A misspelled action passes every other
    /// validation, matches no request, and reads in the file exactly like a grant --
    /// so the rule silently grants nothing and nobody can tell.
    #[test]
    fn an_authority_rule_naming_an_unknown_action_is_refused() {
        let b = with_rule("plan.decid", "Supervisor");
        // Shape validation is perfectly happy with it, which is the whole problem.
        assert!(validate(&b).is_ok());
        match validate_authority_names(&b, &test_vocabulary()) {
            Err(ConfigError::UnknownAuthorityName { kind, name }) => {
                assert_eq!(kind, "action");
                assert_eq!(name, "plan.decid");
            }
            other => panic!("a misspelled action was accepted: {other:?}"),
        }
    }

    /// The same for a role: a rule for "Comander" grants the real Commander nothing.
    #[test]
    fn an_authority_rule_naming_an_unknown_role_is_refused() {
        let b = with_rule("plan.decide", "Comander");
        assert!(validate(&b).is_ok());
        match validate_authority_names(&b, &test_vocabulary()) {
            Err(ConfigError::UnknownAuthorityName { kind, name }) => {
                assert_eq!(kind, "role");
                assert_eq!(name, "Comander");
            }
            other => panic!("a misspelled role was accepted: {other:?}"),
        }
    }

    #[test]
    fn a_correctly_named_authority_rule_passes() {
        let b = with_rule("plan.decide", "Supervisor");
        assert!(validate_authority_names(&b, &test_vocabulary()).is_ok());
    }

    /// **An empty vocabulary refuses rather than accepting everything.** A caller that
    /// supplied no names would otherwise pass every misspelling, and passing looks
    /// identical to checking.
    #[test]
    fn an_empty_vocabulary_refuses_rather_than_accepting_every_name() {
        let b = with_rule("plan.decide", "Supervisor");
        assert!(matches!(
            validate_authority_names(&b, &KnownVocabulary::default()),
            Err(ConfigError::Invalid(_))
        ));
    }

    /// DN-08 §5: a baseline outside its window may be read and inspected, and may not be
    /// promoted. Before this, `is_promotable_at` had no caller and an expired baseline
    /// went into force without objection.
    #[test]
    fn an_expired_baseline_is_not_promoted() {
        let path =
            std::env::temp_dir().join(format!("gungnir-expired-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut store = FileConfigStore::new(&path, test_vocabulary());
        let baseline = ConfigBaseline {
            validity: Some(gungnir_model::ValidityWindow {
                valid_from: MissionTime(100.0),
                valid_until: Some(MissionTime(200.0)),
            }),
            ..ConfigBaseline::default()
        };

        match store.apply(baseline.clone(), MissionTime(250.0)) {
            Err(ConfigError::NotPromotable {
                now, valid_until, ..
            }) => {
                assert_eq!(now, MissionTime(250.0));
                assert_eq!(valid_until, Some(MissionTime(200.0)));
            }
            other => panic!("an expired baseline was promoted: {other:?}"),
        }
        // Refused before anything was written: nothing was promoted, so nothing is on
        // disk and nothing is recorded as applied.
        assert!(!path.exists(), "an expired baseline was written to disk");
        assert!(store.applied().is_none());

        // Not yet open is refused too, and for the same reason.
        assert!(matches!(
            store.apply(baseline.clone(), MissionTime(50.0)),
            Err(ConfigError::NotPromotable { .. })
        ));

        // Inside the window it promotes normally.
        store
            .apply(baseline, MissionTime(150.0))
            .expect("inside its window");
        assert!(store.applied().is_some());
        let _ = std::fs::remove_file(&path);
    }

    /// A promotion advances the revision, or it is not a promotion: the second apply of
    /// the same content is refused, and so is a lower number, before anything is
    /// written. The first apply into an empty store accepts any revision.
    #[test]
    fn a_promotion_must_advance_the_revision() {
        let path =
            std::env::temp_dir().join(format!("gungnir-revision-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut store = FileConfigStore::new(&path, test_vocabulary());
        let baseline = ConfigBaseline {
            revision: 3,
            ..ConfigBaseline::default()
        };
        store
            .apply(baseline.clone(), MissionTime(0.0))
            .expect("a fresh store takes any revision");

        assert!(matches!(
            store.apply(baseline.clone(), MissionTime(1.0)),
            Err(ConfigError::RevisionNotAdvanced {
                in_force: 3,
                candidate: 3
            })
        ));
        let older = ConfigBaseline {
            revision: 2,
            ..ConfigBaseline::default()
        };
        assert!(matches!(
            store.apply(older, MissionTime(2.0)),
            Err(ConfigError::RevisionNotAdvanced { .. })
        ));
        let newer = ConfigBaseline {
            revision: 4,
            ..ConfigBaseline::default()
        };
        store
            .apply(newer.clone(), MissionTime(3.0))
            .expect("advances");
        assert_eq!(store.load().expect("load").revision, 4);

        // A store that applied nothing itself still reads what is on disk.
        let mut second = FileConfigStore::new(&path, test_vocabulary());
        assert!(matches!(
            second.apply(newer, MissionTime(4.0)),
            Err(ConfigError::RevisionNotAdvanced { in_force: 4, .. })
        ));
        let _ = std::fs::remove_file(&path);
    }

    /// The asset list is stamped with the revision, not the schema version.
    #[test]
    fn the_asset_list_is_stamped_with_the_revision() {
        let baseline = ConfigBaseline {
            revision: 7,
            ..ConfigBaseline::default()
        };
        assert_eq!(baseline.asset_list().baseline_version, 7);
        assert_ne!(baseline.version, 7);
    }

    // --- DN-21 battle rhythm -------------------------------------------------

    fn product(name: &str, kind: &str, period: f64, offset: f64) -> ScheduledProductConfig {
        ScheduledProductConfig {
            name: name.into(),
            kind: kind.into(),
            period_s: period,
            offset_s: offset,
            deliver_to: None,
        }
    }

    fn with_products(products: Vec<ScheduledProductConfig>) -> ConfigBaseline {
        ConfigBaseline {
            reporting: ReportingConfig {
                scheduled: products,
                retention_sessions: 10,
            },
            ..ConfigBaseline::default()
        }
    }

    #[test]
    fn a_well_formed_rhythm_validates_and_converts() {
        let b = with_products(vec![product(
            "watch handover",
            "handover-summary",
            43200.0,
            0.0,
        )]);
        assert!(validate(&b).is_ok());
        let p = b.reporting.scheduled[0].to_product().expect("known kind");
        assert_eq!(p.kind, gungnir_model::ProductKind::HandoverSummary);
        // Twelve hours, and the schedule agrees about when the next one is due.
        assert_eq!(
            p.schedule.next_due(MissionTime(0.0)),
            Some(MissionTime(43200.0))
        );
    }

    /// An unknown kind is refused rather than defaulted. A product that quietly became a
    /// situation report because its spelling was not recognized would be produced on
    /// schedule and be the wrong thing every time.
    #[test]
    fn an_unknown_product_kind_is_refused() {
        let b = with_products(vec![product(
            "watch handover",
            "handover_summary",
            100.0,
            0.0,
        )]);
        assert!(matches!(validate(&b), Err(ConfigError::Invalid(_))));
        assert!(b.reporting.scheduled[0].to_product().is_none());
    }

    #[test]
    fn a_period_that_is_not_positive_is_refused() {
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let b = with_products(vec![product("sitrep", "situation-report", bad, 0.0)]);
            assert!(
                matches!(validate(&b), Err(ConfigError::Invalid(_))),
                "period {bad} was accepted"
            );
        }
    }

    /// An offset at or beyond the period describes the same rhythm confusingly; a
    /// negative one schedules into the past.
    #[test]
    fn an_offset_outside_the_period_is_refused() {
        for bad in [-1.0, 100.0, 150.0] {
            let b = with_products(vec![product("sitrep", "situation-report", 100.0, bad)]);
            assert!(
                matches!(validate(&b), Err(ConfigError::Invalid(_))),
                "offset {bad} was accepted"
            );
        }
        let ok = with_products(vec![product("sitrep", "situation-report", 100.0, 99.9)]);
        assert!(validate(&ok).is_ok());
    }

    /// **The check that matters.** A product delivering to an endpoint the baseline does
    /// not declare would be produced every cycle and delivered nowhere, and the
    /// deployment would look like it was reporting.
    #[test]
    fn a_product_delivering_to_an_undeclared_endpoint_is_refused() {
        let mut b = with_products(vec![ScheduledProductConfig {
            deliver_to: Some("higher-command".into()),
            ..product("sitrep", "situation-report", 100.0, 0.0)
        }]);
        assert!(matches!(validate(&b), Err(ConfigError::Invalid(_))));

        // Held for a person to read is a real configuration, not a failure.
        b.reporting.scheduled[0].deliver_to = None;
        assert!(validate(&b).is_ok());
    }

    fn sensor_with(windows: Vec<MaintenanceWindowConfig>) -> ConfigBaseline {
        ConfigBaseline {
            sensors: vec![SensorConfig {
                id: 1,
                modality: "radar".into(),
                position: [0.0; 3],
                max_range_m: 1000.0,
                control_endpoint: None,
                maintenance: windows,
            }],
            ..ConfigBaseline::default()
        }
    }

    fn maintenance(from: f64, to: f64) -> MaintenanceWindowConfig {
        MaintenanceWindowConfig {
            from_s: from,
            to_s: to,
            reason: "antenna swap".into(),
        }
    }

    #[test]
    fn a_maintenance_window_must_end_after_it_starts() {
        for (from, to) in [(20.0, 10.0), (10.0, 10.0), (10.0, f64::NAN)] {
            let b = sensor_with(vec![maintenance(from, to)]);
            assert!(
                matches!(validate(&b), Err(ConfigError::Invalid(_))),
                "window {from}..{to} was accepted"
            );
        }
    }

    /// **Overlapping windows would put one sensor in two maintenance states at once**, and
    /// the overrun of the first would be hidden by the second -- which is the one case
    /// this whole feature exists to surface.
    #[test]
    fn overlapping_maintenance_windows_for_one_sensor_are_refused() {
        let b = sensor_with(vec![maintenance(10.0, 30.0), maintenance(20.0, 40.0)]);
        assert!(matches!(validate(&b), Err(ConfigError::Invalid(_))));

        // Touching windows do not overlap: one ends where the next begins.
        let ok = sensor_with(vec![maintenance(10.0, 30.0), maintenance(30.0, 40.0)]);
        assert!(validate(&ok).is_ok());
    }

    /// A window with no reason is a hole in the coverage picture nobody can account for at
    /// handover, which is exactly when somebody asks.
    #[test]
    fn a_maintenance_window_must_say_why() {
        let b = sensor_with(vec![MaintenanceWindowConfig {
            reason: "  ".into(),
            ..maintenance(10.0, 20.0)
        }]);
        assert!(matches!(validate(&b), Err(ConfigError::Invalid(_))));
    }

    #[test]
    fn a_window_converts_to_the_registry_type_as_planned() {
        let w = maintenance(10.0, 20.0).to_window(gungnir_model::SensorId(3));
        assert_eq!(w.sensor, gungnir_model::SensorId(3));
        assert_eq!(w.state, gungnir_model::MaintenanceState::Planned);
        assert!(w.is_open_at(MissionTime(15.0)));
    }

    // --- DN-24 mission profiles ----------------------------------------------

    fn candidate(profile: &str, name: &str, promoted: bool) -> TrackingProfileConfig {
        TrackingProfileConfig {
            profile: profile.into(),
            name: name.into(),
            filter_selection: "imm-cv-ct".into(),
            gate_threshold: 9.21,
            // A well-formed imm-cv-ct triple (DN-28 §5): this helper names the selection
            // to exercise DN-24's promotion/rollback rules, not the IMM's own math, and
            // an invalid triple here would fail validation for a reason unrelated to
            // whatever a test using it is actually checking.
            imm_turn_rate_rad_s: 0.05,
            imm_mode_transition: [[0.97, 0.03], [0.03, 0.97]],
            imm_initial_mode_probabilities: [0.9, 0.1],
            // DN-30 §5: a well-formed figure, distinct from the default, so a test using
            // this helper is exercising DN-24's rules rather than the noise value itself.
            measurement_noise_var: [625.0, 3600.0, 22500.0],
            promoted,
            validated_by: None,
        }
    }

    fn with_profiles(
        profiles: &[&str],
        candidates: Vec<TrackingProfileConfig>,
        active: Option<&str>,
    ) -> ConfigBaseline {
        ConfigBaseline {
            mission_profiles: profiles.iter().map(|p| (*p).to_owned()).collect(),
            tracking_profiles: candidates,
            active_profile: active.map(ToOwned::to_owned),
            ..ConfigBaseline::default()
        }
    }

    #[test]
    fn a_well_formed_profile_set_validates_and_resolves() {
        let b = with_profiles(
            &["air-defence", "counter-uas"],
            vec![
                candidate("air-defence", "imm baseline", true),
                candidate("air-defence", "tighter gate", false),
                candidate("counter-uas", "short range", true),
            ],
            Some("air-defence"),
        );
        assert!(validate(&b).is_ok(), "{:?}", validate(&b));
        assert_eq!(b.algorithm_candidates().len(), 3);
        assert_eq!(
            b.operating_profile(),
            Some(gungnir_model::MissionProfile::new("air-defence"))
        );
    }

    /// **DN-30 §5: unlike the `imm-cv-ct` fields, every filter selection is held to
    /// this rule.** A zero or negative axis states no error, which
    /// `PipelineSettings::new_filter` would otherwise build a measurement-noise matrix
    /// from silently.
    #[test]
    fn a_non_positive_measurement_noise_axis_is_refused_regardless_of_filter_selection() {
        let mut bad = candidate("air-defence", "imm baseline", true);
        bad.filter_selection = "kf-cv".into();
        bad.measurement_noise_var = [625.0, 0.0, 22500.0];
        let b = with_profiles(&["air-defence"], vec![bad], None);
        match validate(&b) {
            Err(ConfigError::Invalid(m)) => {
                assert!(m.contains("measurement_noise_var.north"), "{m}");
            }
            other => panic!("a zero measurement-noise axis was accepted: {other:?}"),
        }
    }

    /// **DN-28 §5: a file this function accepts must never be one `Imm::new` refuses.**
    /// A candidate naming `imm-cv-ct` is validated against the same rules, so a
    /// misconfigured transition matrix is refused at config load rather than reaching
    /// `FusionPipeline::new_filter` at runtime.
    #[test]
    fn an_imm_cv_ct_candidate_with_a_transition_row_that_does_not_sum_to_one_is_refused() {
        let mut bad = candidate("air-defence", "imm baseline", true);
        bad.imm_mode_transition = [[0.9, 0.2], [0.03, 0.97]];
        let b = with_profiles(&["air-defence"], vec![bad], None);
        match validate(&b) {
            Err(ConfigError::Invalid(m)) => {
                assert!(m.contains("does not sum to one"), "{m}");
            }
            other => panic!("a malformed transition matrix was accepted: {other:?}"),
        }
    }

    /// The same rule for the initial mode probabilities, and for an entry outside
    /// `[0, 1]` rather than only a bad sum.
    #[test]
    fn an_imm_cv_ct_candidate_with_an_out_of_range_initial_probability_is_refused() {
        let mut bad = candidate("air-defence", "imm baseline", true);
        bad.imm_initial_mode_probabilities = [1.5, -0.5];
        let b = with_profiles(&["air-defence"], vec![bad], None);
        match validate(&b) {
            Err(ConfigError::Invalid(m)) => assert!(m.contains("[0, 1]"), "{m}"),
            other => panic!("an out-of-range probability was accepted: {other:?}"),
        }
    }

    /// A candidate naming a different filter is not held to imm-cv-ct's rules at all:
    /// these fields are meaningless for it and default to a row of zeros, which would
    /// fail the same checks if they were ever applied.
    #[test]
    fn imm_fields_are_not_validated_for_a_non_imm_candidate() {
        let mut cv = candidate("air-defence", "linear", true);
        cv.filter_selection = "kf-cv".into();
        cv.imm_mode_transition = [[0.0, 0.0], [0.0, 0.0]];
        cv.imm_initial_mode_probabilities = [0.0, 0.0];
        let b = with_profiles(&["air-defence"], vec![cv], None);
        assert!(validate(&b).is_ok(), "{:?}", validate(&b));
    }

    /// **The rule the whole schema turns on.** A profile promoting nothing means the
    /// deployment cannot say what is running; promoting two means it cannot say either and
    /// would report whichever the iteration order reached.
    #[test]
    fn a_profile_must_promote_exactly_one_candidate() {
        let none = with_profiles(
            &["air-defence"],
            vec![candidate("air-defence", "imm baseline", false)],
            None,
        );
        assert!(matches!(validate(&none), Err(ConfigError::Invalid(_))));

        let two = with_profiles(
            &["air-defence"],
            vec![
                candidate("air-defence", "imm baseline", true),
                candidate("air-defence", "tighter gate", true),
            ],
            None,
        );
        match validate(&two) {
            Err(ConfigError::Invalid(m)) => assert!(m.contains("exactly one"), "{m}"),
            other => panic!("two promoted candidates were accepted: {other:?}"),
        }
    }

    /// A candidate in a profile nobody declared can never be selected and reads like one
    /// that can. Same rule as endpoints, same reason.
    #[test]
    fn a_candidate_naming_an_undeclared_profile_is_refused() {
        let b = with_profiles(
            &["air-defence"],
            vec![
                candidate("air-defence", "imm baseline", true),
                candidate("maritime", "long range", true),
            ],
            None,
        );
        assert!(matches!(validate(&b), Err(ConfigError::Invalid(_))));
    }

    /// A rollback names a candidate. Two candidates with one name make the record
    /// ambiguous about what was restored.
    #[test]
    fn two_candidates_with_one_name_in_a_profile_are_refused() {
        let b = with_profiles(
            &["air-defence"],
            vec![
                candidate("air-defence", "baseline", true),
                candidate("air-defence", "baseline", false),
            ],
            None,
        );
        match validate(&b) {
            Err(ConfigError::Invalid(m)) => assert!(m.contains("rollback"), "{m}"),
            other => panic!("duplicate candidate names were accepted: {other:?}"),
        }
        // The same name in a different profile is a different baseline and is fine.
        let ok = with_profiles(
            &["air-defence", "counter-uas"],
            vec![
                candidate("air-defence", "baseline", true),
                candidate("counter-uas", "baseline", true),
            ],
            Some("air-defence"),
        );
        assert!(validate(&ok).is_ok());
    }

    /// **Two answers to what is in force is worse than either answer.**
    #[test]
    fn tracking_and_tracking_profiles_are_mutually_exclusive() {
        let mut b = with_profiles(
            &["air-defence"],
            vec![candidate("air-defence", "imm baseline", true)],
            None,
        );
        b.tracking = Some(TrackingConfig {
            filter_selection: "imm-cv-ct".into(),
            gate_threshold: 9.21,
            imm_turn_rate_rad_s: 0.05,
            imm_mode_transition: [[0.97, 0.03], [0.03, 0.97]],
            imm_initial_mode_probabilities: [0.9, 0.1],
            measurement_noise_var: [625.0, 3600.0, 22500.0],
        });
        match validate(&b) {
            Err(ConfigError::Invalid(m)) => assert!(m.contains("mutually exclusive"), "{m}"),
            other => panic!("a baseline with both was accepted: {other:?}"),
        }
    }

    /// Several profiles and no active one is refused rather than guessed: which context a
    /// deployment is operating in is not something to pick for it.
    #[test]
    fn several_profiles_without_an_active_one_are_refused() {
        let b = with_profiles(
            &["air-defence", "counter-uas"],
            vec![
                candidate("air-defence", "imm baseline", true),
                candidate("counter-uas", "short range", true),
            ],
            None,
        );
        assert!(matches!(validate(&b), Err(ConfigError::Invalid(_))));

        // One profile needs no active declaration: there is nothing to choose.
        let one = with_profiles(
            &["air-defence"],
            vec![candidate("air-defence", "imm baseline", true)],
            None,
        );
        assert!(validate(&one).is_ok());
        assert_eq!(
            one.operating_profile(),
            Some(gungnir_model::MissionProfile::new("air-defence"))
        );
    }

    #[test]
    fn an_active_profile_that_is_not_declared_is_refused() {
        let b = with_profiles(
            &["air-defence"],
            vec![candidate("air-defence", "imm baseline", true)],
            Some("maritime"),
        );
        assert!(matches!(validate(&b), Err(ConfigError::Invalid(_))));
    }

    /// **Nothing existing breaks.** A baseline written before this schema means "this is
    /// what we run", and it is read as exactly that rather than reinterpreted.
    #[test]
    fn a_baseline_with_only_tracking_is_one_implicit_default_profile() {
        let b = ConfigBaseline {
            tracking: Some(TrackingConfig {
                filter_selection: "imm-cv-ct".into(),
                gate_threshold: 9.21,
                imm_turn_rate_rad_s: 0.05,
                imm_mode_transition: [[0.97, 0.03], [0.03, 0.97]],
                imm_initial_mode_probabilities: [0.9, 0.1],
                measurement_noise_var: [625.0, 3600.0, 22500.0],
            }),
            ..ConfigBaseline::default()
        };
        assert!(validate(&b).is_ok());

        let candidates = b.algorithm_candidates();
        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].promoted);
        assert_eq!(candidates[0].id.profile.as_str(), "default");
        assert_eq!(
            b.operating_profile(),
            Some(gungnir_model::MissionProfile::new("default"))
        );
    }

    /// The default deployment declares no algorithm configuration at all, and says so
    /// rather than being given a guessed one.
    #[test]
    fn a_baseline_with_neither_has_no_configuration_in_force() {
        let b = ConfigBaseline::default();
        assert!(validate(&b).is_ok());
        assert!(b.algorithm_candidates().is_empty());
        assert!(b.declared_profiles().is_empty());
        assert!(b.operating_profile().is_none());
    }

    /// A candidate is held to the same two rules `tracking` is: a bad gate is refused
    /// wherever it is written.
    #[test]
    fn a_candidate_is_validated_like_the_single_tracking_config() {
        let mut b = with_profiles(
            &["air-defence"],
            vec![candidate("air-defence", "imm baseline", true)],
            None,
        );
        b.tracking_profiles[0].gate_threshold = -1.0;
        assert!(matches!(validate(&b), Err(ConfigError::Invalid(_))));

        b.tracking_profiles[0].gate_threshold = 9.21;
        b.tracking_profiles[0].filter_selection = "  ".into();
        assert!(matches!(validate(&b), Err(ConfigError::Invalid(_))));
    }

    #[test]
    fn first_draft_file_without_new_fields_still_loads() {
        let json = r#"{"version":1,"sensors":[],"tracking":null}"#;
        let b: ConfigBaseline = serde_json::from_str(json).expect("parse");
        assert_eq!(b.backend, BackendConfig::Embedded);
        assert_eq!(b.allocation_horizon, 10);
        assert!(b.resources.is_empty());
    }

    // --- DN-01 defended assets ------------------------------------------------

    fn asset(id: u32, priority: &str) -> AssetConfig {
        AssetConfig {
            id,
            name: format!("asset-{id}"),
            position: [0.1, 0.2, 0.0],
            radius_m: None,
            priority: priority.into(),
            warning_lead_time_s: None,
            warning_channel: None,
            warning_within_m: None,
            note: None,
        }
    }

    /// DN-03 amendment 1: the distance rides on a whole obligation and must be positive.
    #[test]
    fn a_warning_distance_needs_the_obligation_and_a_positive_value() {
        let mut a = asset(1, "high");
        a.warning_within_m = Some(500.0);
        let alone = ConfigBaseline {
            assets: vec![a.clone()],
            ..ConfigBaseline::default()
        };
        assert!(matches!(validate(&alone), Err(ConfigError::Invalid(_))));
        a.warning_lead_time_s = Some(120.0);
        a.warning_channel = Some("harbour-master".into());
        let endpoints = vec![EndpointConfig {
            name: "harbour-master".into(),
            kind: "warning".into(),
            address: "https://port.example/warn".into(),
        }];
        let whole = ConfigBaseline {
            assets: vec![a.clone()],
            endpoints: endpoints.clone(),
            ..ConfigBaseline::default()
        };
        validate(&whole).expect("a whole obligation with a distance");
        assert_eq!(
            whole.assets[0].to_asset().warning.and_then(|w| w.within_m),
            Some(500.0)
        );
        a.warning_within_m = Some(-1.0);
        let negative = ConfigBaseline {
            assets: vec![a],
            endpoints,
            ..ConfigBaseline::default()
        };
        assert!(matches!(validate(&negative), Err(ConfigError::Invalid(_))));
    }

    #[test]
    fn an_unknown_asset_priority_is_rejected_rather_than_defaulted() {
        let b = ConfigBaseline {
            assets: vec![asset(1, "urgent")],
            ..ConfigBaseline::default()
        };
        let err = validate(&b).expect_err("unknown priority must be rejected");
        assert!(
            format!("{err}").contains("unknown priority"),
            "the error must name the problem: {err}"
        );

        let ok = ConfigBaseline {
            assets: vec![asset(1, "critical")],
            ..ConfigBaseline::default()
        };
        assert!(validate(&ok).is_ok());
    }

    #[test]
    fn duplicate_asset_ids_are_rejected() {
        let b = ConfigBaseline {
            assets: vec![asset(1, "high"), asset(1, "low")],
            ..ConfigBaseline::default()
        };
        assert!(matches!(validate(&b), Err(ConfigError::Invalid(_))));
    }

    #[test]
    fn half_a_warning_obligation_is_rejected() {
        let mut a = asset(1, "high");
        a.warning_lead_time_s = Some(120.0);
        let b = ConfigBaseline {
            assets: vec![a],
            ..ConfigBaseline::default()
        };
        assert!(matches!(validate(&b), Err(ConfigError::Invalid(_))));
    }

    #[test]
    fn a_warning_channel_must_name_a_declared_endpoint() {
        let mut a = asset(1, "high");
        a.warning_lead_time_s = Some(120.0);
        a.warning_channel = Some("harbour-master".into());

        let undeclared = ConfigBaseline {
            assets: vec![a.clone()],
            ..ConfigBaseline::default()
        };
        assert!(matches!(
            validate(&undeclared),
            Err(ConfigError::Invalid(_))
        ));

        let declared = ConfigBaseline {
            assets: vec![a],
            endpoints: vec![EndpointConfig {
                name: "harbour-master".into(),
                kind: "warning".into(),
                address: "https://port.example/warn".into(),
            }],
            ..ConfigBaseline::default()
        };
        assert!(validate(&declared).is_ok());
    }

    #[test]
    fn an_asset_list_carries_the_baseline_revision_and_its_extent() {
        let mut area = asset(2, "medium");
        area.radius_m = Some(800.0);
        // The revision, not the schema version: the schema version is the same for
        // every promotion and would date nothing (DN-01 amendment 1).
        let b = ConfigBaseline {
            version: 1,
            revision: 5,
            assets: vec![asset(1, "high"), area],
            ..ConfigBaseline::default()
        };
        let list = b.asset_list();
        assert_eq!(list.baseline_version, 5);
        assert!(!list.is_unconfigured());
        assert!(matches!(
            list.assets[0].extent,
            gungnir_model::AssetExtent::Point { .. }
        ));
        assert!(matches!(
            list.assets[1].extent,
            gungnir_model::AssetExtent::Circle { .. }
        ));
    }

    #[test]
    fn a_baseline_with_no_assets_is_valid_and_reports_itself_unconfigured() {
        let b = ConfigBaseline::default();
        assert!(validate(&b).is_ok());
        assert!(
            b.asset_list().is_unconfigured(),
            "an empty list must be reported, not scored as zero"
        );
    }

    // --- DN-04 effector model -------------------------------------------------

    fn resource(id: u32, layer: &str) -> ResourceConfig {
        ResourceConfig {
            handoff_endpoint: None,
            id,
            position: [0.1, 0.2, 0.0],
            capacity: 1,
            layer: layer.into(),
            cost: None,
            rounds_available: None,
            reserve: None,
            intercept_speed_mps: None,
        }
    }

    #[test]
    fn a_missing_or_unknown_effector_layer_is_rejected() {
        for bad in ["", "gun", "layer-1"] {
            let b = ConfigBaseline {
                resources: vec![resource(1, bad)],
                ..ConfigBaseline::default()
            };
            let err = validate(&b).expect_err("MOE-03 depends on the layer");
            assert!(format!("{err}").contains("unknown effector layer"), "{err}");
        }
        for good in [
            "area",
            "point",
            "self-defence",
            "self_defense",
            "non-kinetic",
        ] {
            let b = ConfigBaseline {
                resources: vec![resource(1, good)],
                ..ConfigBaseline::default()
            };
            assert!(validate(&b).is_ok(), "{good} should parse");
        }
    }

    #[test]
    fn a_reserve_larger_than_the_magazine_is_rejected() {
        let mut r = resource(1, "point");
        r.rounds_available = Some(2);
        r.reserve = Some(5);
        let b = ConfigBaseline {
            resources: vec![r],
            ..ConfigBaseline::default()
        };
        assert!(matches!(validate(&b), Err(ConfigError::Invalid(_))));
    }

    #[test]
    fn a_reserve_without_a_magazine_is_rejected() {
        let mut r = resource(1, "point");
        r.reserve = Some(2);
        let b = ConfigBaseline {
            resources: vec![r],
            ..ConfigBaseline::default()
        };
        assert!(matches!(validate(&b), Err(ConfigError::Invalid(_))));
    }

    #[test]
    fn a_resource_at_its_reserve_is_not_adequate() {
        let mut r = resource(1, "point");
        r.rounds_available = Some(3);
        r.reserve = Some(3);
        let view = r.to_view();
        assert!(view.ready);
        assert!(
            !view.is_adequate(),
            "eating the reserve is a decision for a person"
        );

        let mut spare = resource(2, "point");
        spare.rounds_available = Some(4);
        spare.reserve = Some(3);
        assert!(spare.to_view().is_adequate());
    }

    #[test]
    fn a_resource_without_a_magazine_is_adequate_when_ready() {
        let view = resource(1, "non-kinetic").to_view();
        assert!(view.magazine.is_none());
        assert!(view.is_adequate());
    }

    // --- DN-08 policy configuration -------------------------------------------

    #[test]
    fn an_absent_policy_section_resolves_to_the_strictest_reading() {
        let b = ConfigBaseline::default();
        assert!(validate(&b).is_ok());
        assert_eq!(
            b.policy
                .control_status
                .for_layer(gungnir_model::EffectorLayer::Area),
            gungnir_model::WeaponsControlStatus::Hold,
            "silence about authority denies"
        );
        assert!(!b
            .policy
            .authority
            .permits("plan.decide", "operator", None, None));
        assert_eq!(
            b.policy
                .decisions
                .expiry_for(gungnir_model::EffectorLayer::Area),
            None,
            "silence about expiry preserves"
        );
    }

    #[test]
    fn an_out_of_range_identification_threshold_is_rejected() {
        let mut b = ConfigBaseline::default();
        b.policy.identification.thresholds.insert("uas".into(), 1.5);
        assert!(matches!(validate(&b), Err(ConfigError::Invalid(_))));
    }

    #[test]
    fn an_authority_rule_with_an_empty_action_is_rejected() {
        let mut b = ConfigBaseline::default();
        b.policy.authority.rules.push(gungnir_model::AuthorityRule {
            action: "  ".into(),
            role: "operator".into(),
            layer: None,
            class: None,
            pre_delegated: false,
        });
        assert!(matches!(validate(&b), Err(ConfigError::Invalid(_))));
    }

    #[test]
    fn a_pre_delegated_rule_must_name_a_layer_and_a_class() {
        let mut b = ConfigBaseline::default();
        b.policy.authority.rules.push(gungnir_model::AuthorityRule {
            action: "plan.decide".into(),
            role: "operator".into(),
            layer: Some(gungnir_model::EffectorLayer::Point),
            class: None,
            pre_delegated: true,
        });
        assert!(
            matches!(validate(&b), Err(ConfigError::Invalid(_))),
            "D-15 pre-delegated one case, not a general power"
        );

        b.policy.authority.rules[0].class = Some("uas-hostile".into());
        assert!(validate(&b).is_ok());
    }

    #[test]
    fn escalation_must_be_earlier_than_expiry() {
        let mut b = ConfigBaseline::default();
        let layer = gungnir_model::EffectorLayer::Area;
        b.policy.decisions.expiry_s.insert(layer, 30.0);
        b.policy.decisions.escalate_after_s.insert(layer, 30.0);
        assert!(matches!(validate(&b), Err(ConfigError::Invalid(_))));

        b.policy.decisions.escalate_after_s.insert(layer, 20.0);
        assert!(validate(&b).is_ok());
    }

    #[test]
    fn a_baseline_outside_its_validity_window_is_not_promotable() {
        use gungnir_model::MissionTime;
        let b = ConfigBaseline {
            validity: Some(gungnir_model::ValidityWindow {
                valid_from: MissionTime(100.0),
                valid_until: Some(MissionTime(200.0)),
            }),
            ..ConfigBaseline::default()
        };
        assert!(
            validate(&b).is_ok(),
            "it is a valid baseline, just not in force"
        );
        assert!(!b.is_promotable_at(MissionTime(50.0)));
        assert!(b.is_promotable_at(MissionTime(150.0)));
        assert!(!b.is_promotable_at(MissionTime(250.0)));

        let always = ConfigBaseline::default();
        assert!(always.is_promotable_at(MissionTime(0.0)));
    }

    #[test]
    fn a_validity_window_that_ends_before_it_starts_is_rejected() {
        use gungnir_model::MissionTime;
        let b = ConfigBaseline {
            validity: Some(gungnir_model::ValidityWindow {
                valid_from: MissionTime(200.0),
                valid_until: Some(MissionTime(100.0)),
            }),
            ..ConfigBaseline::default()
        };
        assert!(matches!(validate(&b), Err(ConfigError::Invalid(_))));
    }

    #[test]
    fn duplicate_endpoint_names_are_rejected() {
        let e = |name: &str| EndpointConfig {
            name: name.into(),
            kind: "warning".into(),
            address: "https://example/x".into(),
        };
        let b = ConfigBaseline {
            endpoints: vec![e("a"), e("a")],
            ..ConfigBaseline::default()
        };
        assert!(matches!(validate(&b), Err(ConfigError::Invalid(_))));
    }

    #[test]
    fn prediction_horizons_must_be_positive_and_ascending() {
        let mut b = ConfigBaseline::default();
        assert!(validate(&b).is_ok(), "the defaults are valid");

        b.assessment.prediction_horizons_s = vec![30.0, 10.0];
        assert!(matches!(validate(&b), Err(ConfigError::Invalid(_))));

        b.assessment.prediction_horizons_s = vec![10.0, -5.0];
        assert!(matches!(validate(&b), Err(ConfigError::Invalid(_))));

        b.assessment.prediction_horizons_s = Vec::new();
        assert!(matches!(validate(&b), Err(ConfigError::Invalid(_))));

        b.assessment.prediction_horizons_s = vec![10.0, 30.0, 60.0];
        assert!(validate(&b).is_ok());
    }
}

#[cfg(test)]
mod hazard_tests {
    use super::*;

    fn boom(shape: HazardShapeConfig) -> HazardConfig {
        HazardConfig {
            name: "harbour boom".into(),
            kind: "boom".into(),
            shape,
            blocks_surface: true,
            height_m: Some(1.5),
        }
    }

    fn with(hazards: Vec<HazardConfig>) -> ConfigBaseline {
        ConfigBaseline {
            hazards,
            ..ConfigBaseline::default()
        }
    }

    #[test]
    fn a_boom_across_the_harbour_mouth_is_valid() {
        let baseline = with(vec![boom(HazardShapeConfig::Polyline {
            points: vec![[0.96, 0.21, 0.0], [0.96, 0.2101, 0.0]],
        })]);
        validate(&baseline).expect("a two-point polyline is a boom");
    }

    /// DN-14 §6, all three rules, plus the kind list the desktop maps from.
    #[test]
    fn the_three_shape_rules_and_the_kind_list_are_enforced() {
        let one_point = with(vec![boom(HazardShapeConfig::Polyline {
            points: vec![[0.96, 0.21, 0.0]],
        })]);
        assert!(validate(&one_point).is_err(), "a one-point polyline passed");

        let bad_radius = with(vec![boom(HazardShapeConfig::Circle {
            center: [0.96, 0.21, 0.0],
            radius_m: 0.0,
        })]);
        assert!(validate(&bad_radius).is_err(), "a zero radius passed");

        let mut tall = boom(HazardShapeConfig::Circle {
            center: [0.96, 0.21, 0.0],
            radius_m: 50.0,
        });
        tall.height_m = Some(f64::INFINITY);
        assert!(
            validate(&with(vec![tall])).is_err(),
            "an infinite height passed"
        );

        let mut odd = boom(HazardShapeConfig::Circle {
            center: [0.96, 0.21, 0.0],
            radius_m: 50.0,
        });
        odd.kind = "minefield".into();
        let err = validate(&with(vec![odd])).expect_err("an unknown kind passed");
        assert!(err.to_string().contains("minefield"), "{err}");
    }

    /// The serialised shape is the one an operator writes: `shape: polyline` beside the
    /// name, not a nested object.
    #[test]
    fn the_json_shape_is_flat() {
        let json = r#"{"name":"net","kind":"net","shape":"circle","center":[0.96,0.21,0.0],"radius_m":40.0}"#;
        let parsed: HazardConfig = serde_json::from_str(json).expect("parses");
        assert!(
            matches!(parsed.shape, HazardShapeConfig::Circle { radius_m, .. } if (radius_m - 40.0).abs() < f64::EPSILON)
        );
        assert!(
            !parsed.blocks_surface,
            "blocks_surface defaults to false, not guessed from the kind"
        );
    }
}

#[cfg(test)]
mod geofence_tests {
    use super::*;

    #[test]
    fn a_no_go_fence_is_valid_and_a_zero_radius_is_not() {
        let good = ConfigBaseline {
            geofences: vec![GeofenceConfig {
                name: "harbour approach".into(),
                center: [0.96, 0.21, 0.0],
                radius_m: 800.0,
                no_go: true,
            }],
            ..ConfigBaseline::default()
        };
        validate(&good).expect("a positive radius is a fence");
        let bad = ConfigBaseline {
            geofences: vec![GeofenceConfig {
                name: "nothing".into(),
                center: [0.96, 0.21, 0.0],
                radius_m: 0.0,
                no_go: true,
            }],
            ..ConfigBaseline::default()
        };
        assert!(validate(&bad).is_err());
    }
}

#[cfg(test)]
mod authentication_tests {
    use super::*;

    #[test]
    fn local_accounts_name_a_file_and_never_a_secret() {
        let good = ConfigBaseline {
            security: SecurityConfig {
                authentication: AuthenticationConfig {
                    provider: AuthenticationProvider::LocalAccounts {
                        accounts_path: "accounts.json".into(),
                    },
                    session_lifetime_s: Some(3600.0),
                },
                ..SecurityConfig::default()
            },
            ..ConfigBaseline::default()
        };
        validate(&good).expect("a path is a path");
        let bad = ConfigBaseline {
            security: SecurityConfig {
                authentication: AuthenticationConfig {
                    provider: AuthenticationProvider::LocalAccounts {
                        accounts_path: "$argon2id$v=19$m=19456,t=2,p=1$abc$def".into(),
                    },
                    session_lifetime_s: None,
                },
                ..SecurityConfig::default()
            },
            ..ConfigBaseline::default()
        };
        assert!(validate(&bad).is_err(), "a PHC string passed as a path");
    }
}
