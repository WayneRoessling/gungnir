//! Canonical operational data model, per docs/gungnir-capabilities.md §5.1
//! ("Canonical Operational Data Model" -- Critical). One shared, versioned set of
//! domain types that every service facade, UI, ingest, store, and API crate in this
//! workspace builds against, so "what a track is" is defined exactly once.
//!
//! Identifier and lifecycle primitives (`TrackId`, `TrackStatus`, `ResourceId`) are
//! re-exported from `gungnir-core`, and `Geodetic` from `gungnir-coord`, so the
//! tracking core and this model share the same types without the core depending on
//! this crate (agentic-coding-standards.md §1.1, §1.2). `gungnir-tracking-service`
//! and `gungnir-intercept-service` re-export the view types below as their public
//! contract (ARCHITECTURE.md §7.2).

pub mod anomaly_settings;
pub mod assets;
pub mod effectors;
pub mod events;
pub mod exchange;
pub mod frame;

/// What a sensor is doing.
///
/// Owned here rather than in `gungnir-sensor-management` because a mode change is an
/// event, and `gungnir-model::events` cannot depend on a crate that depends on it.
/// `gungnir-sensor-management` re-exports it, so every existing path still resolves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SensorMode {
    Standby,
    Search,
    Track,
    Calibrating,
    Offline,
}

impl SensorMode {
    /// The transition table. Offline sensors must come back through Standby;
    /// calibration completes into Standby; any online mode may go Offline.
    #[must_use]
    pub fn can_transition_to(self, to: SensorMode) -> bool {
        use SensorMode::{Calibrating, Offline, Search, Track};
        if self == to {
            return true;
        }
        // Everything is permitted except leaving Offline or Calibrating for an
        // active mode without passing through Standby.
        !matches!(
            (self, to),
            (Offline | Calibrating, Search | Track | Calibrating)
        )
    }

    /// Every mode, for an exhaustive control and an exhaustive label check.
    pub const ALL: [SensorMode; 5] = [
        SensorMode::Standby,
        SensorMode::Search,
        SensorMode::Track,
        SensorMode::Calibrating,
        SensorMode::Offline,
    ];
}
pub mod handoff;
pub mod identity;
pub mod plans;
pub mod policy_settings;
/// Mission profiles and algorithm-baseline identity (DN-24 §4).
pub mod profiles;
pub mod provenance;
pub mod quality;
pub mod releasability;
pub mod requirements;
/// Battle-rhythm data (DN-21 §3): schedules and planned sensor downtime, here rather
/// than in one consumer so the design keeps its "no new edges" property.
pub mod rhythm;
pub mod time;
pub mod ui_settings;
pub mod vocabulary;

pub use assets::{
    AssetExtent, AssetId, AssetListView, AssetPriority, DefendedAsset, WarningObligation,
};
pub use effectors::{EffectorLayer, Magazine, RelativeCost};
pub use exchange::{
    ExchangeAgreement, ExchangeFormat, ExchangeItem, ExchangeSet, LaunchWarningReport,
    PeerLaunchWarning, PeerOrigin,
};
pub use frame::LocalFrame;
pub use gungnir_coord::Geodetic;
/// The motion models `gungnir-core` owns, re-exported rather than redefined
/// (`agentic-coding-standards.md` §1.2). `gungnir-assessment` needs the same process
/// noise the tracker runs, so that a prediction's uncertainty is the filter's and not a
/// second opinion about it (GAP-020).
pub use gungnir_core::{ConstantVelocity, MotionModel};
pub use gungnir_core::{ResourceId, TrackId, TrackStatus};
pub use handoff::{
    accept_report, DecisionAttribution, DeliveryState, EffectorReport, Handoff, HandoffError,
};
pub use plans::{
    DecisionId, DeconflictionCheck, DeconflictionKind, DeconflictionResult, FiresPlan, PlanKind,
};
pub use policy_settings::{
    AuthorityRule, AuthoritySettings, ControlStatusSettings, DecisionSettings, FiresSettings,
    IdentificationSettings, PolicySettings, StalenessSettings, ValidityWindow,
    WeaponsControlStatus,
};
pub use profiles::{AlgorithmBaselineId, MissionProfile};
pub use provenance::{Provenance, SourceAuthentication};
pub use quality::Quality;
pub use releasability::{filter_for, permits_single, Filtered, Releasability};
pub use requirements::{
    lapse_overdue, CollectionRequirement, Concurrence, RequirementId, RequirementState,
};
pub use rhythm::{
    absence_is_planned, MaintenanceState, MaintenanceWindow, ProductKind, Schedule,
    ScheduledProduct,
};
pub use time::MissionTime;
pub use ui_settings::{LayoutNode, RoleLayout, UiSettings};
pub use vocabulary::{Term, Vocabulary};

use nalgebra::{SMatrix, SVector};

/// Schema version of every serialized type in this crate. Bump on any breaking
/// change to a view or event; `gungnir-store`, `gungnir-api`, and `gungnir-config`
/// check it with [`check_schema_version`].
///
/// Version 2, 2026-09-05: `PlanView.solutions` became [`PlanView::kind`] so a plan
/// can be an intercept or a fires task (docs/design/DN-05-fires.md). The owner took
/// option B: replaced outright, no deprecated mirror, and the interface path moved
/// from `/v1` to `/v2` in the same change.
///
/// Version 3, 2026-09-06: `DetectionView.measurement` became [`Measurement`], so a
/// sensor that reports a direction and no range has somewhere to put it
/// (docs/design/DN-27-bearing-only-detections.md §4 and §8). Not additive: a consumer
/// holding the old shape cannot read the new one, and the exact-match version rule in
/// `gungnir-remote/tests/wire_conformance.rs` will refuse a peer one version out. That
/// refusal **is** the correct outcome and is the reason the rule exists -- a peer that
/// silently read a bearing as a position would draw a symbol where nothing is.
pub const SCHEMA_VERSION: u32 = 3;

/// Identifier of one recorded mission session.
///
/// Shared by the journal, the session lifecycle, replay, reporting, resilience, and
/// the review workflow, so it lives here rather than in any one of them
/// (agentic-coding-standards.md §1.2). `gungnir-store` re-exports it.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct SessionId(pub u64);

/// Whether a configuration value looks like key material.
///
/// Owned here rather than by `gungnir-security`, which is the crate that cares about
/// custody, because `gungnir-config` validates baselines with it and a foundational
/// crate must not depend on a productization one. Two crates share it, so it lives in
/// the lowest one both reach (agentic-coding-standards.md §1.2);
/// `gungnir-security` re-exports it, so every existing path still resolves.
///
/// True when a baseline value looks like key material rather than a reference.
///
/// Configuration names which provider a deployment uses and its parameters; no key
/// DN-22 §6: no key material, secret, or path to one ever appears in a baseline.
/// Validation rejects a value
/// that looks like material rather than storing it.
///
/// **This is a guard against the obvious mistake, not a guarantee.** Unpadded
/// base64 slips through it, and it must not be relied on as the boundary. The
/// boundary is the schema: there is no field for key material, so a provider
/// configuration has nowhere to put any. This catches somebody pasting a key into
/// a field meant for a provider name.
///
/// A cloud key-service resource path is a **reference**, not material, and is
/// exactly what the baseline should contain, so it must not be flagged.
pub fn looks_like_key_material(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.len() < 32 {
        return false;
    }
    if trimmed.starts_with("-----BEGIN") {
        return true;
    }
    // A path or a URL is a reference to where material lives, not the material.
    if trimmed.contains("://") || trimmed.split('/').count() > 2 {
        return false;
    }
    let hexish = trimmed.chars().all(|c| c.is_ascii_hexdigit());
    // Padded base64 is the shape a pasted key usually takes.
    let padded_base64 = trimmed.ends_with('=')
        && trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=');
    hexish || padded_base64
}

/// Identifier of a sensor as registered in `gungnir-sensor-management` and named in
/// `gungnir-config`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct SensorId(pub u32);

/// Identifier of one command issued to a sensor.
///
/// Owned here rather than by `gungnir-sensor-management`, which issues them, because
/// the event schema has to carry it and the model cannot depend on a crate that depends
/// on the model. `gungnir_sensor_management::tasking` re-exports it, so every existing
/// path still resolves -- the same move `SensorMode` made for GAP-003.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct SensorTaskId(pub u64);

/// A command to a sensor (DN-11 §4).
///
/// Owned here for the same reason as [`SensorTaskId`]: the v2 contract carries it
/// (`POST /v2/sensors/{sensor_id}/task`, GAP-004) and the transport cannot depend on the
/// crate that issues it. `gungnir_sensor_management::tasking` re-exports it, so every
/// existing path still resolves.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "command", rename_all = "kebab-case")]
pub enum SensorCommand {
    SetMode {
        mode: SensorMode,
    },
    Cue {
        target: Geodetic,
        dwell_s: Option<f64>,
    },
    Search {
        area: AssetExtent,
    },
    Calibrate {
        procedure: String,
    },
}

/// Friend/foe/neutral/unknown classification, set by `gungnir-identification`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum Classification {
    #[default]
    Unknown,
    Neutral,
    Friendly,
    Hostile,
}

impl Classification {
    /// How much a track of this affiliation is allowed to threaten an asset, as a
    /// factor on the risk score (GAP-027; MOP-28 requires the score to be monotonic in
    /// it). Hostile counts in full; unknown nearly so, because an unidentified inbound
    /// is the case the score exists for; neutral little; friendly nothing, so a friendly
    /// track is never allocated against. This is affiliation, which the track carries;
    /// platform-class lethality (a missile against a quadcopter) waits on a class field
    /// no track has yet (GAP-018's engine declares affiliation, not class).
    #[must_use]
    pub fn lethality_weight(self) -> f64 {
        match self {
            Classification::Hostile => 1.0,
            Classification::Unknown => 0.8,
            Classification::Neutral => 0.2,
            Classification::Friendly => 0.0,
        }
    }
}

/// What a sensor actually measured. **Not always a position**, and the type says so
/// rather than letting an adapter invent the difference
/// (docs/design/DN-27-bearing-only-detections.md §4; GAP-001's acoustic, passive-RF and
/// spotter halves, and GAP-004's sensor half).
///
/// The enumeration exists to forbid one thing: **a bearing must never be turned into a
/// position by assuming a range** (DN-27 §2). Before it, `DetectionView.measurement` was
/// a `Vector3<f64>`, so an acoustic array's direction of arrival could only enter this
/// system as a place -- at a nominal range, on the terrain, or on the asset the operator
/// fears -- and the invented position was a valid value of its type, indistinguishable
/// downstream from a measured one.
///
/// **Every variant carries its error.** The previous shape carried none, so the gate
/// downstream assumed one. That is tolerable for a radar position whose accuracy the
/// baseline states; it is not tolerable for a bearing, where the error *is* the
/// information (DN-27 §4).
///
/// **Azimuth convention, restated on purpose.** Azimuth is `atan2(east, north)`: a
/// compass bearing, zero at north and increasing to the east, exactly as
/// `gungnir_filters::RangeAzimuthElevation` already models it. Elevation is measured up
/// from the horizontal plane. DN-27 §4 restates it here because the one thing more
/// expensive than an unstated convention is two components each assuming a different one.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Measurement {
    /// A position in the local ENU frame, metres. What radar and AIS report.
    Position {
        enu: nalgebra::Vector3<f64>,
        /// Per-axis variance, metres squared. **Required**: a position with no stated
        /// error is one the tracker has to guess a gate for, and it guesses generously.
        variance_m2: [f64; 3],
    },
    /// Range, azimuth and elevation from the sensor. What a radar reports natively
    /// before anyone converts it, and what `gungnir_filters::RangeAzimuthElevation`
    /// already models.
    RangeAzimuthElevation {
        range_m: f64,
        azimuth_rad: f64,
        elevation_rad: f64,
        /// Variance of `[range_m, azimuth_rad, elevation_rad]` in that order: metres
        /// squared, then radians squared, then radians squared.
        variance: [f64; 3],
    },
    /// A direction and no range. What an acoustic array, a direction finder and a
    /// person report.
    ///
    /// `elevation_rad` is optional because a ground-based direction finder frequently
    /// has none, and a missing elevation is not a zero one: zero means the horizon.
    /// An `Option` that serialised indistinguishably from zero would put every acoustic
    /// detection on the horizon, which is why DN-27 §10 makes the distinction a gated
    /// row rather than a remark.
    Bearing {
        azimuth_rad: f64,
        elevation_rad: Option<f64>,
        /// Angular one-sigma error squared, radians squared. Small here means a large
        /// cross-range error far away: one degree is 17 m across at 1 km and 520 m at
        /// 30 km, so a conversion that kept a constant positional variance would be
        /// wrong at every range but one (DN-27 §6).
        azimuth_variance_rad2: f64,
        elevation_variance_rad2: Option<f64>,
    },
}

impl Measurement {
    /// The position in the local ENU frame, when the measurement **is** one.
    ///
    /// `None` for both angular variants, and that is the point rather than an
    /// omission: neither carries a sensor origin, so neither can be resolved to a place
    /// without one, and DN-27 §2 refuses every shortcut that would supply the missing
    /// range. A caller that needs a position from two crossing bearings asks
    /// `gungnir_coord::cross_bearings` for one and gets the covariance the geometry
    /// gives it (DN-27 §5 rule 2).
    #[must_use]
    pub fn position_enu(&self) -> Option<nalgebra::Vector3<f64>> {
        match self {
            Measurement::Position { enu, .. } => Some(*enu),
            Measurement::RangeAzimuthElevation { .. } | Measurement::Bearing { .. } => None,
        }
    }

    /// Whether this measurement determines a position on its own.
    ///
    /// False for [`Measurement::Bearing`], which is what DN-27 §5 rule 1's prohibition
    /// on initiating a track from a bearing turns on.
    #[must_use]
    pub fn localises(&self) -> bool {
        !matches!(self, Measurement::Bearing { .. })
    }

    /// Whether every number in the measurement, its error included, is finite. The
    /// gateway's first validation rule (`gungnir_ingest::gateway::validate_detection`).
    #[must_use]
    pub fn is_finite(&self) -> bool {
        match self {
            Measurement::Position { enu, variance_m2 } => {
                enu.iter().all(|v| v.is_finite()) && variance_m2.iter().all(|v| v.is_finite())
            }
            Measurement::RangeAzimuthElevation {
                range_m,
                azimuth_rad,
                elevation_rad,
                variance,
            } => {
                range_m.is_finite()
                    && azimuth_rad.is_finite()
                    && elevation_rad.is_finite()
                    && variance.iter().all(|v| v.is_finite())
            }
            Measurement::Bearing {
                azimuth_rad,
                elevation_rad,
                azimuth_variance_rad2,
                elevation_variance_rad2,
            } => {
                azimuth_rad.is_finite()
                    && elevation_rad.is_none_or(f64::is_finite)
                    && azimuth_variance_rad2.is_finite()
                    && elevation_variance_rad2.is_none_or(f64::is_finite)
            }
        }
    }
}

/// The canonical observation: what `gungnir-ingest` hands to the tracking service
/// after validation, and what the journal records. `gungnir-tracking-service`
/// converts it to the core's kinematic `Detection`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DetectionView {
    pub sensor: SensorId,
    /// When the sensor says it observed the target (`gungnir-time` source time).
    pub source_time: MissionTime,
    /// When this system received the observation (`gungnir-time` receipt time).
    pub receipt_time: MissionTime,
    /// What the sensor measured, which is not always a position
    /// (docs/design/DN-27-bearing-only-detections.md §4). Positions are in the local
    /// ENU frame, metres; angles follow [`Measurement`]'s stated convention.
    pub measurement: Measurement,
    pub provenance: Provenance,
}

/// The canonical track type. Kinematic state (position/velocity/covariance) plus
/// the provenance/quality/classification/identity fields the tracking core's own
/// `Track` omits -- see docs/gungnir-capabilities.md §5.1 and §5.2.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TrackView {
    pub id: TrackId,
    pub status: TrackStatus,
    /// `[e, n, u, ve, vn, vu]`: position (m) and velocity (m/s) in the local ENU frame.
    pub state: SVector<f64, 6>,
    pub covariance: SMatrix<f64, 6, 6>,
    pub classification: Classification,
    pub provenance: Provenance,
    pub quality: Quality,
    /// Mission time of the estimate.
    pub mission_time: MissionTime,
    /// Who may receive this track (docs/design/DN-17-releasability.md). Defaults to
    /// `Internal`, so an unmarked track never leaves the deployment.
    #[serde(default)]
    pub releasability: Releasability,
}

impl TrackView {
    /// Position `[e, n, u]`, meters.
    pub fn position_enu(&self) -> [f64; 3] {
        [self.state[0], self.state[1], self.state[2]]
    }

    /// Ground-plus-vertical speed, m/s.
    pub fn speed_mps(&self) -> f64 {
        self.state.fixed_rows::<3>(3).norm()
    }

    /// One-sigma position uncertainty per axis `[e, n, u]`, meters, from the
    /// covariance diagonal. Negative diagonal entries (a PSD violation upstream)
    /// yield `NaN` rather than a panic so the UI can flag them.
    pub fn position_sigma(&self) -> [f64; 3] {
        [
            self.covariance[(0, 0)].sqrt(),
            self.covariance[(1, 1)].sqrt(),
            self.covariance[(2, 2)].sqrt(),
        ]
    }
}

/// A taskable resource as the intercept planner and the UI see it.
///
/// `layer`, `cost`, and `magazine` are what the cheapest-adequate rule reads
/// (docs/design/DN-04-effector-model.md). `layer` has no default because MOE-03 is
/// defined by it; `magazine` is optional because a non-kinetic effector and a
/// sensor-cued camera have no rounds.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ResourceView {
    pub id: ResourceId,
    pub position: Geodetic,
    /// Max simultaneous tracks this resource can be tasked against.
    pub capacity: u32,
    pub ready: bool,
    /// Defence layer. MOE-03 is the fraction of propeller-drone engagements taken
    /// by the inner kinetic layers rather than by area defence.
    pub layer: EffectorLayer,
    /// Relative cost of one round, used only to break ties within a layer.
    #[serde(default)]
    pub cost: RelativeCost,
    /// Rounds held and rounds withheld; `None` for an effector without rounds.
    #[serde(default)]
    pub magazine: Option<Magazine>,
    /// How fast this effector closes on a track, metres per second, for the
    /// constant-velocity closest-approach solution GAP-031 builds
    /// (docs/design/DN-04-effector-model.md §9, amendment 1). `None` means the
    /// deployment has not characterised it, or it has no closing speed (a jammer): no
    /// intercept point can be solved for it, and the solver says so rather than
    /// inventing one. Read by nothing until the solver exists.
    #[serde(default)]
    pub intercept_speed_mps: Option<f64>,
}

impl ResourceView {
    /// True when this resource may be proposed at all: ready, and with rounds left
    /// above its reserve where it has a magazine.
    ///
    /// **A resource at or below its reserve is not adequate.** Eating the reserve
    /// under saturation is a decision for a person, so the recommendation shows the
    /// resource as unavailable with the reason rather than proposing it and relying
    /// on refusal (docs/design/DN-04-effector-model.md §5, rule 4).
    pub fn is_adequate(&self) -> bool {
        self.ready
            && match &self.magazine {
                Some(m) => m.has_allocatable(),
                None => true,
            }
    }
}

/// Identifier of one plan produced by the intercept service.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct PlanId(pub u64);

/// One resource-to-track pairing within a plan, with the geometry the UI draws once
/// the intercept-geometry solver exists (`None` until then).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct InterceptSolutionView {
    pub resource: ResourceId,
    pub track: TrackId,
    pub intercept_point: Option<Geodetic>,
    pub time_to_intercept_s: Option<f64>,
}

/// The canonical plan: the recommendation, whether an intercept or a fires task.
/// Nothing acts on it until `gungnir-policy` and `gungnir-command` have produced a
/// recorded decision.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PlanView {
    pub id: PlanId,
    pub mission_time: MissionTime,
    /// What the plan proposes. Replaced `solutions` at schema version 2
    /// (docs/design/DN-05-fires.md).
    pub kind: PlanKind,
    /// Value function of the allocation policy that produced this plan.
    pub policy_value: f64,
    /// Who may receive this plan (docs/design/DN-17-releasability.md).
    #[serde(default)]
    pub releasability: Releasability,
}

impl PlanView {
    /// An intercept plan over these solutions.
    pub fn intercept(
        id: PlanId,
        mission_time: MissionTime,
        solutions: Vec<InterceptSolutionView>,
        policy_value: f64,
    ) -> Self {
        Self {
            id,
            mission_time,
            kind: PlanKind::Intercept { solutions },
            policy_value,
            releasability: Releasability::default(),
        }
    }

    /// The intercept solutions, empty for a fires plan.
    pub fn solutions(&self) -> &[InterceptSolutionView] {
        self.kind.solutions()
    }

    /// The fires task, if this plan is one.
    pub fn fires(&self) -> Option<&FiresPlan> {
        self.kind.fires()
    }

    /// The raw (resource, track) pairs this plan tasks, in order.
    ///
    /// A fires plan yields its one firing unit and target, so every policy that
    /// walks assignments covers fires without a second code path.
    pub fn assignments(&self) -> Vec<(ResourceId, TrackId)> {
        match &self.kind {
            PlanKind::Intercept { solutions } => {
                solutions.iter().map(|s| (s.resource, s.track)).collect()
            }
            PlanKind::Fires(f) => vec![(f.firing_unit, f.target)],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.kind.is_empty()
    }
}

/// Operator-facing health summary, reported by `gungnir-observability`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SystemHealth {
    pub tracking_healthy: bool,
    pub intercept_healthy: bool,
    pub ingest_healthy: bool,
}

impl SystemHealth {
    pub fn all_healthy(&self) -> bool {
        self.tracking_healthy && self.intercept_healthy && self.ingest_healthy
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("schema version mismatch: expected {expected}, found {found}")]
    SchemaVersion { expected: u32, found: u32 },
}

/// Reject data written by a different schema version.
pub fn check_schema_version(found: u32) -> Result<(), ModelError> {
    if found == SCHEMA_VERSION {
        Ok(())
    } else {
        Err(ModelError::SchemaVersion {
            expected: SCHEMA_VERSION,
            found,
        })
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn sample_track() -> TrackView {
        TrackView {
            id: TrackId(7),
            status: TrackStatus::Confirmed,
            state: SVector::<f64, 6>::new(1.0, 2.0, 3.0, 4.0, 0.0, 3.0),
            covariance: SMatrix::<f64, 6, 6>::identity() * 4.0,
            classification: Classification::Unknown,
            provenance: Provenance::default(),
            quality: Quality::default(),
            mission_time: MissionTime(12.5),
            releasability: Releasability::default(),
        }
    }

    #[test]
    fn track_view_round_trips_through_serde() {
        let t = sample_track();
        let json = serde_json::to_string(&t).expect("serialize");
        let back: TrackView = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(t, back);
    }

    #[test]
    fn plan_view_round_trips_through_serde() {
        let p = PlanView::intercept(
            PlanId(3),
            MissionTime(1.0),
            vec![InterceptSolutionView {
                resource: ResourceId(1),
                track: TrackId(7),
                intercept_point: Some(Geodetic {
                    lat_rad: 0.1,
                    lon_rad: 0.2,
                    alt_m: 30.0,
                }),
                time_to_intercept_s: Some(42.0),
            }],
            9.5,
        );
        let json = serde_json::to_string(&p).expect("serialize");
        let back: PlanView = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(p, back);
        assert_eq!(back.assignments(), vec![(ResourceId(1), TrackId(7))]);
        assert_eq!(back.solutions().len(), 1);
        assert!(back.fires().is_none());
    }

    #[test]
    fn a_fires_plan_tasks_its_firing_unit_against_its_target() {
        // Every policy that walks assignments covers fires without a second path.
        let p = PlanView {
            id: PlanId(4),
            mission_time: MissionTime(2.0),
            kind: PlanKind::Fires(Box::new(plans::FiresPlan {
                target: TrackId(11),
                target_position: Geodetic {
                    lat_rad: 0.3,
                    lon_rad: 0.4,
                    alt_m: 0.0,
                },
                location_error_m: 35.0,
                firing_unit: ResourceId(5),
                time_on_target: None,
                deconfliction: plans::DeconflictionResult::default(),
            })),
            policy_value: 1.0,
            releasability: Releasability::default(),
        };
        assert_eq!(p.assignments(), vec![(ResourceId(5), TrackId(11))]);
        assert!(p.solutions().is_empty());
        assert!(p.fires().is_some());
        assert!(!p.is_empty());
    }

    #[test]
    fn derived_kinematics() {
        let t = sample_track();
        assert_eq!(t.position_enu(), [1.0, 2.0, 3.0]);
        assert!((t.speed_mps() - 5.0).abs() < 1e-12);
        assert_eq!(t.position_sigma(), [2.0, 2.0, 2.0]);
    }

    #[test]
    fn schema_version_is_checked() {
        assert!(check_schema_version(SCHEMA_VERSION).is_ok());
        assert!(matches!(
            check_schema_version(SCHEMA_VERSION + 1),
            Err(ModelError::SchemaVersion { .. })
        ));
    }
}

#[cfg(test)]
mod key_material_tests {
    use super::looks_like_key_material;

    #[test]
    fn a_baseline_value_that_looks_like_key_material_is_recognized() {
        assert!(looks_like_key_material(
            "-----BEGIN PRIVATE KEY-----MIIEvQIBADANBgkqhkiG9w0BAQEFA"
        ));
        assert!(looks_like_key_material(
            "0123456789abcdef0123456789abcdef0123456789abcdef"
        ));
        assert!(looks_like_key_material(
            "TWFuIGlzIGRpc3Rpbmd1aXNoZWQsIG5vdCBvbmx5IGJ5IGhpcw=="
        ));
        // A provider name or a service reference is not material: a reference is
        // exactly what the baseline should contain.
        assert!(!looks_like_key_material("os-keystore"));
        assert!(!looks_like_key_material("cloud-kms"));
        assert!(!looks_like_key_material(
            "projects/example/locations/eu/keyRings/gungnir"
        ));
        assert!(!looks_like_key_material(
            "https://vault.example.internal/v1/gungnir/journal"
        ));
    }

    #[test]
    fn the_material_check_is_a_guard_and_not_the_boundary() {
        // Unpadded base64 slips through, which is why the schema having no field
        // for key material is the actual boundary.
        assert!(!looks_like_key_material(
            "TWFuIGlzIGRpc3Rpbmd1aXNoZWQsIG5vdCBvbmx5IGJ5IGhpcw"
        ));
    }
}
