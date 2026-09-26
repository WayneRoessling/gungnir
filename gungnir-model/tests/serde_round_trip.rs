// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The `gungnir-model` row's criterion, "`serde` round-trip of every view and event type"
//! with zero loss, and "a mismatched schema version" refused (GAP-117,
//! `docs/verification-capability-table.md` §2).
//!
//! Until this file, the round trips covered four view types and one event variant, so a
//! field that did not survive the journal or the wire would have gone unseen in 20 of
//! 21 event enums (the GAP-067 walk, 2026-09-16).
//!
//! # How a new variant or a new type is made impossible to forget
//!
//! Three mechanisms, each catching what the one before cannot:
//!
//! 1. **Every value is a full struct literal**, with no `..Default::default()`, so a
//!    field added to a view, an event variant or any type they carry stops this file
//!    compiling until the field is given a value here.
//! 2. **Every event enum is matched exhaustively** (`variant_of`, one per enum, with no
//!    wildcard arm), so a new variant stops this file compiling until it is named.
//! 3. **The source is read** (`events.rs`, and every module for a `*View` type): a new
//!    event enum, a new variant, or a new view type whose value is missing from the
//!    tables below fails the test by name, even where the exhaustive match was updated
//!    and the table was not.
//!
//! # Why the numbers are what they are
//!
//! Every float is non-dyadic (0.1, 1/3, a Unix time with a fractional part), because a
//! dyadic value such as 0.5 survives a lossy float formatter and would prove nothing.
//! `f32` fields get their own non-dyadic values: a formatter that printed an `f32` as
//! the nearest `f64` and read it back would change it. Every value is finite:
//! non-finite floats are GAP-153's, fixed separately.

use gungnir_model::arbitration::{ArbitrationGround, ConflictSide, SideOutcome};
use gungnir_model::events::{
    CalibrationEvent, CommandEvent, EngagementEvent, EngagementSide, GovernanceEvent, HandoffEvent,
    HealthEvent, IdentityEvent, IngestEvent, InterceptEvent, LaunchWarningEvent, LinkEvent,
    RehearsalEvent, ReplayEvent, RequirementEvent, RetentionEvent, ReviewEvent, RhythmEvent,
    SensorEvent, SensorTaskEvent, TrackingEvent, VerdictSummary, WarningEvent,
};
use gungnir_model::identity::GlobalEntityId;
use gungnir_model::{
    check_schema_version, AlgorithmBaselineId, AssetExtent, AssetId, AssetListView, AssetPriority,
    BearingRayView, Classification, CollectionRequirement, Concurrence, DecisionId,
    DeconflictionCheck, DeconflictionKind, DeconflictionResult, DefendedAsset, DetectionView,
    EffectorLayer, EffectorReport, FiresPlan, Geodetic, InterceptSolutionView, LaunchWarningReport,
    LaydownId, Magazine, Measurement, MissionProfile, MissionTime, ModelError, PeerLaunchWarning,
    PeerOrigin, PendingApprovalId, PipelineStatsView, PlanId, PlanKind, PlanView, ProductKind,
    Provenance, Quality, RehearsalOrigin, RelativeCost, Releasability, RequestId, RequirementId,
    RequirementState, ResourceId, ResourceView, SensorId, SensorMode, SensorTaskId, SessionId,
    SourceAuthentication, TestTrackNumber, TrackId, TrackStatus, TrackView, WarningObligation,
    SCHEMA_VERSION,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;

// ---------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------

/// Mission times with fractional parts no binary fraction states exactly.
const T0: MissionTime = MissionTime(1_789_012_345.123_456_7);
const T1: MissionTime = MissionTime(1_789_012_401.3);
const T2: MissionTime = MissionTime(0.1);

const THIRD: f64 = 0.333_333_333_333_333_3;
const TENTH: f64 = 0.1;

/// A position no power of two divides: latitude and longitude in radians of an
/// arbitrary site, and a height with a tenth.
fn place() -> Geodetic {
    Geodetic {
        lat_rad: 0.959_931_088_596_881_3,
        lon_rad: -0.209_439_510_239_319_55,
        alt_m: 123.4,
    }
}

fn parties() -> Releasability {
    Releasability::Parties {
        parties: BTreeSet::from(["coalition-east".to_string(), "host-nation".to_string()]),
    }
}

fn provenance() -> Provenance {
    Provenance {
        source_sensor_ids: vec![3, 17],
        calibration_baseline_version: Some("cb-2026-09".into()),
        algorithm_version: "ekf/0.1.0".into(),
        peer: Some(peer_origin()),
        conversion_loss: Some("height from Mode C, not geometric".into()),
        authentication: SourceAuthentication::MachineIdentity,
        rehearsal: Some(RehearsalOrigin {
            scenario: TestTrackNumber(4),
            laydown: LaydownId("layout-c".into()),
            seed: 1_701,
        }),
    }
}

fn peer_origin() -> PeerOrigin {
    PeerOrigin {
        peer: "partner-north".into(),
        remote_track: "PN-0042".into(),
        peer_time: T1,
        receipt_time: MissionTime(1_789_012_401.7),
        assigned_quality: 0.7_f32,
    }
}

fn quality() -> Quality {
    Quality {
        association_confidence: 0.1_f32,
        latency_s: 0.333_333_34_f32,
        is_stale: true,
    }
}

/// A track whose every state and covariance entry is non-dyadic.
fn track(id: u64) -> TrackView {
    let mut state = nalgebra::SVector::<f64, 6>::zeros();
    let mut covariance = nalgebra::SMatrix::<f64, 6, 6>::zeros();
    for i in 0..6 {
        #[allow(clippy::cast_precision_loss)]
        let k = (i + 1) as f64;
        state[i] = k * 1_000.1 + THIRD;
        for j in 0..6 {
            #[allow(clippy::cast_precision_loss)]
            let l = (j + 1) as f64;
            covariance[(i, j)] = if i == j { k * 10.3 } else { TENTH / (k + l) };
        }
    }
    TrackView {
        id: TrackId(id),
        status: TrackStatus::Coasting,
        state,
        covariance,
        classification: Classification::Hostile,
        provenance: provenance(),
        quality: quality(),
        mission_time: T0,
        releasability: parties(),
    }
}

/// One detection per measurement kind, so each of `Measurement`'s variants crosses the
/// wire inside the view that carries it.
fn detections() -> Vec<DetectionView> {
    let measurements = [
        Measurement::Position {
            enu: nalgebra::Vector3::new(1_234.5, -6_789.1, 300.3),
            variance_m2: [25.1, 25.2, 100.3],
        },
        Measurement::RangeAzimuthElevation {
            range_m: 12_345.6,
            azimuth_rad: 1.1,
            elevation_rad: 0.07,
            variance: [9.9, 1.0e-5, 2.0e-5 + 1.0e-7],
        },
        Measurement::Bearing {
            azimuth_rad: 2.2,
            elevation_rad: Some(0.3),
            azimuth_variance_rad2: 3.046_174_197_867_086e-4,
            elevation_variance_rad2: Some(1.1e-3),
        },
    ];
    measurements
        .into_iter()
        .enumerate()
        .map(|(i, measurement)| DetectionView {
            sensor: SensorId(u32::try_from(i).unwrap_or_default() + 7),
            source_time: T0,
            receipt_time: T1,
            measurement,
            provenance: provenance(),
        })
        .collect()
}

fn solution() -> InterceptSolutionView {
    InterceptSolutionView {
        resource: ResourceId(40),
        track: TrackId(70),
        intercept_point: Some(place()),
        time_to_intercept_s: Some(47.3),
    }
}

/// An intercept plan and a fires plan, so both of `PlanKind`'s shapes are carried.
fn plans() -> Vec<PlanView> {
    vec![
        PlanView {
            id: PlanId(0x0199_5a3b_7c2d_7e4f_8a1b_2c3d_9f3a_61c2),
            mission_time: T0,
            kind: PlanKind::Intercept {
                solutions: vec![solution()],
            },
            policy_value: 0.7,
            releasability: parties(),
        },
        PlanView {
            id: PlanId(0x0199_5a3b_7c2d_7e4f_8a1b_2c3d_9f3a_61c3),
            mission_time: T1,
            kind: PlanKind::Fires(Box::new(FiresPlan {
                target: TrackId(71),
                target_position: place(),
                location_error_m: 12.3,
                firing_unit: ResourceId(41),
                time_on_target: Some(T1),
                deconfliction: DeconflictionResult {
                    checks: vec![DeconflictionCheck {
                        kind: DeconflictionKind::NoFireArea,
                        passed: false,
                        detail: "inside NFA-3".into(),
                    }],
                },
            })),
            policy_value: THIRD,
            releasability: Releasability::AllPeers,
        },
    ]
}

fn resources() -> Vec<ResourceView> {
    vec![
        ResourceView {
            id: ResourceId(40),
            position: place(),
            capacity: 2,
            ready: true,
            layer: EffectorLayer::Point,
            cost: RelativeCost(2.7),
            magazine: Some(Magazine {
                rounds_available: 8,
                reserve: 2,
            }),
            intercept_speed_mps: Some(412.3),
        },
        ResourceView {
            id: ResourceId(41),
            position: place(),
            capacity: 1,
            ready: false,
            layer: EffectorLayer::NonKinetic,
            cost: RelativeCost(0.1),
            magazine: None,
            intercept_speed_mps: None,
        },
    ]
}

fn bearing_ray() -> BearingRayView {
    BearingRayView {
        sensor: SensorId(9),
        origin_enu: [10.1, -20.2, 3.3],
        azimuth_rad: 0.9,
        elevation_rad: Some(0.11),
        azimuth_one_sigma_rad: 0.017_453_292_519_943_295,
        valid_until: T1,
    }
}

fn pipeline_stats() -> PipelineStatsView {
    PipelineStatsView {
        accepted: 101,
        too_late: 3,
        epochs: 57,
        associated: 88,
        initiated: 9,
        bearings_offered: 21,
        bearings_updated: 13,
        bearings_retained: 5,
        bearings_expired: 2,
        bearings_refused: 1,
    }
}

fn assets() -> AssetListView {
    AssetListView {
        baseline_version: 12,
        assets: vec![
            DefendedAsset {
                id: AssetId(1),
                name: "Fuel farm".into(),
                extent: AssetExtent::Circle {
                    center: place(),
                    radius_m: 250.5,
                },
                priority: AssetPriority::Critical,
                warning: Some(WarningObligation {
                    lead_time_s: 90.3,
                    channel: "radio-2".into(),
                    within_m: Some(5_000.1),
                }),
                note: Some("night shift only".into()),
            },
            DefendedAsset {
                id: AssetId(2),
                name: "Gate".into(),
                extent: AssetExtent::Point { position: place() },
                priority: AssetPriority::Low,
                warning: None,
                note: None,
            },
        ],
    }
}

fn requirement() -> CollectionRequirement {
    CollectionRequirement {
        id: RequirementId(5),
        title: "Watch the northern approach".into(),
        priority: AssetPriority::High,
        area: AssetExtent::Circle {
            center: place(),
            radius_m: 1_000.1,
        },
        needed_by: Some(T1),
        state: RequirementState::Declined {
            by: operator(),
            reason: "no sensor in range".into(),
        },
    }
}

fn operator() -> Concurrence {
    Concurrence::Operator {
        id: "7".into(),
        role: "SensorManager".into(),
    }
}

fn baseline() -> AlgorithmBaselineId {
    AlgorithmBaselineId {
        profile: MissionProfile("urban-cuas".into()),
        name: "ekf-tuned-2026-09".into(),
    }
}

fn launch_report() -> LaunchWarningReport {
    LaunchWarningReport {
        id: "LW-3".into(),
        what: "rocket launch, bearing 045".into(),
        at: T0,
        releasability: parties(),
    }
}

fn decision() -> DecisionId {
    DecisionId(0x0199_5a3b_7c2d_7e4f_8a1b_2c3d_9f3a_0001)
}

fn plan_id() -> PlanId {
    PlanId(0x0199_5a3b_7c2d_7e4f_8a1b_2c3d_9f3a_61c2)
}

// ---------------------------------------------------------------------------
// One value per variant of every event enum
// ---------------------------------------------------------------------------

fn tracking_events() -> Vec<TrackingEvent> {
    vec![
        TrackingEvent::TrackInitiated(track(1)),
        TrackingEvent::TrackUpdated(track(2)),
        TrackingEvent::TrackCoasting(TrackId(3)),
        TrackingEvent::TrackDeleted(TrackId(4)),
    ]
}

fn intercept_events() -> Vec<InterceptEvent> {
    let plans = plans();
    vec![
        InterceptEvent::PlanProposed(plans[0].clone()),
        InterceptEvent::PlanApproved(plans[1].clone()),
        InterceptEvent::PlanSuperseded(plans[0].clone()),
        InterceptEvent::PlanEvaluated {
            plan: plan_id(),
            verdict: VerdictSummary::Denied {
                reason: "inside the no-fire area".into(),
            },
            engines: vec!["geofence".into(), "control-status".into()],
        },
    ]
}

fn ingest_events() -> Vec<IngestEvent> {
    let mut out: Vec<IngestEvent> = detections()
        .into_iter()
        .map(IngestEvent::Accepted)
        .collect();
    out.push(IngestEvent::Quarantined {
        sensor: SensorId(3),
        reason: "timestamp 0.1 s in the future".into(),
    });
    out.push(IngestEvent::NotAccepted {
        sensor: SensorId(4),
        reason: "the pipeline is gone".into(),
    });
    out
}

fn sensor_events() -> Vec<SensorEvent> {
    vec![SensorEvent::ModeChanged {
        sensor: SensorId(3),
        from: SensorMode::Search,
        to: SensorMode::Calibrating,
        at: T0,
    }]
}

fn identity_events() -> Vec<IdentityEvent> {
    vec![
        IdentityEvent::Correlated {
            track: TrackId(3),
            entity: GlobalEntityId(0x0199_5a3b_7c2d_7e4f_8a1b_2c3d_0000_0001),
            confidence: 0.83,
            basis: "kinematic and class agreement".into(),
            at: T0,
        },
        IdentityEvent::Minted {
            track: TrackId(4),
            entity: GlobalEntityId(0x0199_5a3b_7c2d_7e4f_8a1b_2c3d_0000_0002),
            at: T1,
        },
    ]
}

fn calibration_events() -> Vec<CalibrationEvent> {
    vec![
        CalibrationEvent::ReferenceObserved {
            sensor: SensorId(3),
            reference: "survey-RP-7".into(),
            truth_enu: [100.1, 200.2, 3.3],
            observed_enu: [101.7, 198.9, 3.1],
            observation_variance_m2: [4.1, 4.2, 9.3],
            at: T0,
        },
        CalibrationEvent::RegistrationApplied {
            sensor: SensorId(3),
            offset_enu: [1.6, -1.3, -0.2],
            residual_spread_m: 0.7,
            references: 6,
            at: T1,
        },
        CalibrationEvent::RegistrationRefused {
            sensor: SensorId(4),
            reason: "one reference is not enough".into(),
            at: T2,
        },
    ]
}

fn sensor_task_events() -> Vec<SensorTaskEvent> {
    vec![
        SensorTaskEvent::Issued {
            task: SensorTaskId(11),
            sensor: SensorId(3),
            at: T0,
        },
        SensorTaskEvent::Acknowledged {
            task: SensorTaskId(11),
            sensor: SensorId(3),
            at: T1,
        },
        SensorTaskEvent::Failed {
            task: SensorTaskId(12),
            sensor: SensorId(4),
            reason: "mode not supported".into(),
            at: T1,
        },
        SensorTaskEvent::Unacknowledged {
            task: SensorTaskId(13),
            sensor: SensorId(5),
            at: T2,
        },
    ]
}

fn governance_events() -> Vec<GovernanceEvent> {
    vec![
        GovernanceEvent::InForceAtStart {
            baseline: baseline(),
            at: T0,
        },
        GovernanceEvent::NoneInForce {
            profile: Some(MissionProfile("maritime".into())),
            at: T0,
        },
        GovernanceEvent::Promoted {
            baseline: baseline(),
            by: "7".into(),
            at: T1,
        },
        GovernanceEvent::RolledBack {
            profile: MissionProfile("urban-cuas".into()),
            restored: Some(baseline()),
            by: "7".into(),
            at: T1,
        },
        GovernanceEvent::PromotionRefused {
            baseline: baseline(),
            reason: "no evidence attached".into(),
            at: T2,
        },
    ]
}

fn rhythm_events() -> Vec<RhythmEvent> {
    vec![
        RhythmEvent::ProductDue {
            name: "sitrep".into(),
            kind: ProductKind::SituationReport,
            due: T0,
        },
        RhythmEvent::ProductHeld {
            name: "sitrep".into(),
            at: T0,
        },
        RhythmEvent::ProductUndelivered {
            name: "measures".into(),
            endpoint: "https://higher.example/reports".into(),
            reason: "no delivery path".into(),
            at: T1,
        },
        RhythmEvent::HandoverAcknowledged {
            by: "9".into(),
            period: (T0, T1),
            outstanding: true,
            at: T1,
        },
        RhythmEvent::MaintenanceOpened {
            sensor: SensorId(3),
            until: T1,
            reason: "antenna service".into(),
            at: T0,
        },
        RhythmEvent::MaintenanceCompleted {
            sensor: SensorId(3),
            at: T1,
        },
        RhythmEvent::MaintenanceOverrun {
            sensor: SensorId(4),
            window_closed: T1,
            reason: "antenna service".into(),
            at: T2,
        },
    ]
}

fn requirement_events() -> Vec<RequirementEvent> {
    vec![
        RequirementEvent::Stated {
            requirement: requirement(),
            at: T0,
        },
        RequirementEvent::Tasked {
            requirement: RequirementId(5),
            task: SensorTaskId(11),
            by: operator(),
            at: T0,
        },
        RequirementEvent::Declined {
            requirement: RequirementId(6),
            by: Concurrence::UnattributedRole {
                role: "SensorManager".into(),
            },
            reason: "no sensor in range".into(),
            at: T1,
        },
        RequirementEvent::Satisfied {
            requirement: RequirementId(5),
            evidence: "track 70 observed".into(),
            at: T1,
        },
        RequirementEvent::Lapsed {
            requirement: RequirementId(7),
            at: T2,
        },
    ]
}

fn command_events() -> Vec<CommandEvent> {
    vec![
        CommandEvent::Decided {
            plan: plan_id(),
            decision: decision(),
            accepted: true,
            operator: Some("7".into()),
            role: Some("Supervisor".into()),
            verdict: VerdictSummary::RequiresHumanApproval,
            rationale: Some("closing at 0.1 km/s".into()),
            request: Some(RequestId::new("console-2/17").unwrap_or_else(|e| panic!("{e}"))),
            item: Some(PendingApprovalId(0x0199_5a3b_7c2d_7e4f_8a1b_2c3d_9f3a_0002)),
            overridden: true,
            origin: Some("desktop-2".into()),
        },
        CommandEvent::Expired {
            plan: plan_id(),
            at: T1,
        },
        CommandEvent::Queued {
            item: PendingApprovalId(0x0199_5a3b_7c2d_7e4f_8a1b_2c3d_9f3a_0003),
            plan: plan_id(),
            layer: EffectorLayer::SelfDefence,
            offered_to: vec!["Operator".into(), "Supervisor".into()],
            expires_at: Some(T1),
            escalate_at: Some(MissionTime(1_789_012_371.9)),
        },
        CommandEvent::Escalated {
            plan: plan_id(),
            to_role: "Supervisor".into(),
            at: T2,
        },
    ]
}

fn engagement_events() -> Vec<EngagementEvent> {
    vec![
        EngagementEvent::Opened {
            decision: decision(),
            plan: plan_id(),
            track: TrackId(70),
        },
        EngagementEvent::Executing {
            decision: decision(),
            at: T0,
        },
        EngagementEvent::Closed {
            decision: decision(),
            outcome: gungnir_model::events::engagement_outcome::EFFECTIVE_TRACK_INFERRED.into(),
            at: T1,
        },
    ]
}

fn handoff_events() -> Vec<HandoffEvent> {
    vec![
        HandoffEvent::Issued {
            decision: decision(),
            endpoint: Some("https://battery-2.example/handoff".into()),
            at: T0,
        },
        HandoffEvent::Manual {
            decision: decision(),
            at: T0,
        },
        HandoffEvent::Undelivered {
            decision: decision(),
            endpoint: "https://battery-2.example/handoff".into(),
            reason: "connection refused".into(),
            at: T1,
        },
        HandoffEvent::Delivered {
            decision: decision(),
            endpoint: "https://battery-2.example/handoff".into(),
            attempts: 3,
            at: T1,
        },
        HandoffEvent::Refused {
            decision: decision(),
            endpoint: "https://battery-2.example/handoff".into(),
            reason: "not in our sector".into(),
            at: T1,
        },
        HandoffEvent::Reported {
            decision: decision(),
            endpoint: "operator:7".into(),
            report: EffectorReport::Completed {
                at: T2,
                effective: true,
                detail: "splash at 0.3 km".into(),
            },
            at: T2,
        },
    ]
}

fn review_events() -> Vec<ReviewEvent> {
    vec![
        ReviewEvent::Opened {
            session: SessionId(4),
            operator: Some("9".into()),
            at: T0,
        },
        ReviewEvent::FindingRecorded {
            session: SessionId(4),
            finding: 1,
            kind: "practice".into(),
            refers_to: Some(T1),
            operator: None,
            at: T0,
        },
        ReviewEvent::FindingPromoted {
            session: SessionId(4),
            finding: 1,
            gap: "GAP-999".into(),
            operator: Some("9".into()),
            at: T1,
        },
        ReviewEvent::Concluded {
            session: SessionId(4),
            findings: 3,
            operator: Some("9".into()),
            at: T1,
        },
        ReviewEvent::Closed {
            session: SessionId(4),
            operator: None,
            at: T2,
        },
    ]
}

fn verdicts() -> Vec<VerdictSummary> {
    vec![
        VerdictSummary::Approved,
        VerdictSummary::Denied {
            reason: "friendly within 0.1 nm".into(),
        },
        VerdictSummary::RequiresHumanApproval,
    ]
}

fn health_events() -> Vec<HealthEvent> {
    vec![HealthEvent::Changed {
        tracking_healthy: true,
        intercept_healthy: false,
        ingest_healthy: true,
        at: T1,
    }]
}

fn warning_events() -> Vec<WarningEvent> {
    vec![
        WarningEvent::Raised {
            asset: AssetId(1),
            track: TrackId(70),
            due_by: T1,
            at: T0,
        },
        WarningEvent::Sent {
            asset: AssetId(1),
            track: TrackId(70),
            at: T0,
        },
        WarningEvent::Acknowledged {
            asset: AssetId(1),
            track: TrackId(70),
            party: "radio-2".into(),
            at: T1,
        },
        WarningEvent::Failed {
            asset: AssetId(1),
            track: TrackId(71),
            reason: "channel down".into(),
            at: T1,
        },
        WarningEvent::Late {
            asset: AssetId(1),
            track: TrackId(72),
            at: T1,
        },
        WarningEvent::Waived {
            asset: AssetId(2),
            track: TrackId(73),
            operator: "9".into(),
            reason: "asset evacuated".into(),
            at: T2,
        },
        WarningEvent::Closed {
            asset: AssetId(2),
            track: TrackId(73),
            final_state: "waived".into(),
            at: T2,
        },
    ]
}

fn launch_warning_events() -> Vec<LaunchWarningEvent> {
    vec![
        LaunchWarningEvent::Issued(launch_report()),
        LaunchWarningEvent::Received(PeerLaunchWarning {
            peer: "partner-north".into(),
            report: launch_report(),
            receipt_time: T1,
        }),
        LaunchWarningEvent::Refused {
            peer: "partner-north".into(),
            reason: "unsigned".into(),
            at: T2,
        },
    ]
}

fn retention_events() -> Vec<RetentionEvent> {
    vec![
        RetentionEvent::Purged {
            session: SessionId(2),
            idle_days: 30.7,
            max_session_age_days: 30,
            bytes: 1_048_577,
            at: T0,
        },
        RetentionEvent::Completed {
            session: SessionId(3),
            at: T1,
        },
    ]
}

fn link_events() -> Vec<LinkEvent> {
    vec![
        LinkEvent::FellBack {
            endpoint: "https://node.example:8443".into(),
            silent_s: 5.1,
            at: T0,
            last_seq: 4_321,
        },
        LinkEvent::Restored {
            endpoint: "https://node.example:8443".into(),
            fallback_since: T0,
            at: T1,
        },
        LinkEvent::SwitchedBack {
            endpoint: "https://node.example:8443".into(),
            at: T1,
            merged: 12,
            conflicts: 1,
            node_history: true,
        },
        LinkEvent::ConflictResolved {
            plan: plan_id(),
            kept_local: true,
            operator: Some("7".into()),
            at: T1,
        },
        LinkEvent::ConflictArbitrated {
            plan: plan_id(),
            kept_local: false,
            ground: ArbitrationGround::HigherRole,
            local: ConflictSide {
                outcome: SideOutcome::Rejected,
                operator: Some("7".into()),
                role: Some("Operator".into()),
                at: T0,
            },
            remote: ConflictSide {
                outcome: SideOutcome::Accepted,
                operator: Some("9".into()),
                role: Some("Supervisor".into()),
                at: MissionTime(1_789_012_345.7),
            },
            at: T1,
        },
        LinkEvent::BothActed {
            track: TrackId(70),
            local: EngagementSide {
                decision: decision(),
                plan: plan_id(),
                at: T0,
            },
            remote: EngagementSide {
                decision: DecisionId(0x0199_5a3b_7c2d_7e4f_8a1b_2c3d_9f3a_0009),
                plan: PlanId(0x0199_5a3b_7c2d_7e4f_8a1b_2c3d_9f3a_0010),
                at: MissionTime(1_789_012_346.3),
            },
            at: T1,
        },
        LinkEvent::DelegationsLapsed {
            endpoint: "https://node.example:8443".into(),
            cut_off_since: T0,
            lapse_s: Some(300.3),
            withdrawn: vec![plan_id()],
            at: T2,
        },
    ]
}

fn rehearsal_events() -> Vec<RehearsalEvent> {
    vec![RehearsalEvent::Started {
        seed: "usability-round-2".into(),
        seed_sha256: "9f3a61c2".repeat(8),
        at: T0,
    }]
}

fn replay_events() -> Vec<ReplayEvent> {
    vec![
        ReplayEvent::Opened {
            session: SessionId(4),
            at: T0,
        },
        ReplayEvent::Closed {
            session: SessionId(4),
            stepped: 1_234,
            at: T1,
        },
    ]
}

// ---------------------------------------------------------------------------
// Exhaustive names: a new variant stops this file compiling
// ---------------------------------------------------------------------------

/// The variant each value is, by an exhaustive match per enum with no wildcard arm.
///
/// The name returned must equal the variant's tag in the JSON, which the test checks,
/// so a mislabelled arm fails as surely as a missing one.
trait Variant {
    fn variant(&self) -> &'static str;
}

impl Variant for TrackingEvent {
    fn variant(&self) -> &'static str {
        match self {
            Self::TrackInitiated(_) => "TrackInitiated",
            Self::TrackUpdated(_) => "TrackUpdated",
            Self::TrackCoasting(_) => "TrackCoasting",
            Self::TrackDeleted(_) => "TrackDeleted",
        }
    }
}

impl Variant for InterceptEvent {
    fn variant(&self) -> &'static str {
        match self {
            Self::PlanProposed(_) => "PlanProposed",
            Self::PlanApproved(_) => "PlanApproved",
            Self::PlanSuperseded(_) => "PlanSuperseded",
            Self::PlanEvaluated { .. } => "PlanEvaluated",
        }
    }
}

impl Variant for IngestEvent {
    fn variant(&self) -> &'static str {
        match self {
            Self::Accepted(_) => "Accepted",
            Self::Quarantined { .. } => "Quarantined",
            Self::NotAccepted { .. } => "NotAccepted",
        }
    }
}

impl Variant for SensorEvent {
    fn variant(&self) -> &'static str {
        match self {
            Self::ModeChanged { .. } => "ModeChanged",
        }
    }
}

impl Variant for IdentityEvent {
    fn variant(&self) -> &'static str {
        match self {
            Self::Correlated { .. } => "Correlated",
            Self::Minted { .. } => "Minted",
        }
    }
}

impl Variant for CalibrationEvent {
    fn variant(&self) -> &'static str {
        match self {
            Self::ReferenceObserved { .. } => "ReferenceObserved",
            Self::RegistrationApplied { .. } => "RegistrationApplied",
            Self::RegistrationRefused { .. } => "RegistrationRefused",
        }
    }
}

impl Variant for SensorTaskEvent {
    fn variant(&self) -> &'static str {
        match self {
            Self::Issued { .. } => "Issued",
            Self::Acknowledged { .. } => "Acknowledged",
            Self::Failed { .. } => "Failed",
            Self::Unacknowledged { .. } => "Unacknowledged",
        }
    }
}

impl Variant for GovernanceEvent {
    fn variant(&self) -> &'static str {
        match self {
            Self::InForceAtStart { .. } => "InForceAtStart",
            Self::NoneInForce { .. } => "NoneInForce",
            Self::Promoted { .. } => "Promoted",
            Self::RolledBack { .. } => "RolledBack",
            Self::PromotionRefused { .. } => "PromotionRefused",
        }
    }
}

impl Variant for RhythmEvent {
    fn variant(&self) -> &'static str {
        match self {
            Self::ProductDue { .. } => "ProductDue",
            Self::ProductHeld { .. } => "ProductHeld",
            Self::ProductUndelivered { .. } => "ProductUndelivered",
            Self::HandoverAcknowledged { .. } => "HandoverAcknowledged",
            Self::MaintenanceOpened { .. } => "MaintenanceOpened",
            Self::MaintenanceCompleted { .. } => "MaintenanceCompleted",
            Self::MaintenanceOverrun { .. } => "MaintenanceOverrun",
        }
    }
}

impl Variant for RequirementEvent {
    fn variant(&self) -> &'static str {
        match self {
            Self::Stated { .. } => "Stated",
            Self::Tasked { .. } => "Tasked",
            Self::Declined { .. } => "Declined",
            Self::Satisfied { .. } => "Satisfied",
            Self::Lapsed { .. } => "Lapsed",
        }
    }
}

impl Variant for CommandEvent {
    fn variant(&self) -> &'static str {
        match self {
            Self::Decided { .. } => "Decided",
            Self::Expired { .. } => "Expired",
            Self::Queued { .. } => "Queued",
            Self::Escalated { .. } => "Escalated",
        }
    }
}

impl Variant for EngagementEvent {
    fn variant(&self) -> &'static str {
        match self {
            Self::Opened { .. } => "Opened",
            Self::Executing { .. } => "Executing",
            Self::Closed { .. } => "Closed",
        }
    }
}

impl Variant for HandoffEvent {
    fn variant(&self) -> &'static str {
        match self {
            Self::Issued { .. } => "Issued",
            Self::Manual { .. } => "Manual",
            Self::Undelivered { .. } => "Undelivered",
            Self::Delivered { .. } => "Delivered",
            Self::Refused { .. } => "Refused",
            Self::Reported { .. } => "Reported",
        }
    }
}

impl Variant for ReviewEvent {
    fn variant(&self) -> &'static str {
        match self {
            Self::Opened { .. } => "Opened",
            Self::FindingRecorded { .. } => "FindingRecorded",
            Self::FindingPromoted { .. } => "FindingPromoted",
            Self::Concluded { .. } => "Concluded",
            Self::Closed { .. } => "Closed",
        }
    }
}

impl Variant for VerdictSummary {
    fn variant(&self) -> &'static str {
        match self {
            Self::Approved => "Approved",
            Self::Denied { .. } => "Denied",
            Self::RequiresHumanApproval => "RequiresHumanApproval",
        }
    }
}

impl Variant for HealthEvent {
    fn variant(&self) -> &'static str {
        match self {
            Self::Changed { .. } => "Changed",
        }
    }
}

impl Variant for WarningEvent {
    fn variant(&self) -> &'static str {
        match self {
            Self::Raised { .. } => "Raised",
            Self::Sent { .. } => "Sent",
            Self::Acknowledged { .. } => "Acknowledged",
            Self::Failed { .. } => "Failed",
            Self::Late { .. } => "Late",
            Self::Waived { .. } => "Waived",
            Self::Closed { .. } => "Closed",
        }
    }
}

impl Variant for LaunchWarningEvent {
    fn variant(&self) -> &'static str {
        match self {
            Self::Issued(_) => "Issued",
            Self::Received(_) => "Received",
            Self::Refused { .. } => "Refused",
        }
    }
}

impl Variant for RetentionEvent {
    fn variant(&self) -> &'static str {
        match self {
            Self::Purged { .. } => "Purged",
            Self::Completed { .. } => "Completed",
        }
    }
}

impl Variant for LinkEvent {
    fn variant(&self) -> &'static str {
        match self {
            Self::FellBack { .. } => "FellBack",
            Self::Restored { .. } => "Restored",
            Self::SwitchedBack { .. } => "SwitchedBack",
            Self::ConflictResolved { .. } => "ConflictResolved",
            Self::ConflictArbitrated { .. } => "ConflictArbitrated",
            Self::BothActed { .. } => "BothActed",
            Self::DelegationsLapsed { .. } => "DelegationsLapsed",
        }
    }
}

impl Variant for RehearsalEvent {
    fn variant(&self) -> &'static str {
        match self {
            Self::Started { .. } => "Started",
        }
    }
}

impl Variant for ReplayEvent {
    fn variant(&self) -> &'static str {
        match self {
            Self::Opened { .. } => "Opened",
            Self::Closed { .. } => "Closed",
        }
    }
}

// ---------------------------------------------------------------------------
// The round trip
// ---------------------------------------------------------------------------

/// Encode, decode, and require the same value back -- and the same text again, so a
/// value that decodes equal but re-encodes differently (a float printed two ways) is
/// caught as well.
fn round_trip<T: Serialize + DeserializeOwned + PartialEq + Debug>(what: &str, value: &T) {
    let text = serde_json::to_string(value).unwrap_or_else(|e| panic!("{what}: encode: {e}"));
    let back: T =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{what}: decode {text}: {e}"));
    assert_eq!(
        &back, value,
        "{what} did not survive the round trip: {text}"
    );
    let again = serde_json::to_string(&back).unwrap_or_else(|e| panic!("{what}: re-encode: {e}"));
    assert_eq!(again, text, "{what} re-encoded differently");
}

/// The tag serde writes for an externally tagged enum value: the string of a unit
/// variant, or the one key of any other.
///
/// Read back from the text rather than taken with `serde_json::to_value`, which refuses
/// a `GlobalEntityId` outright: it is written as a 128-bit number, which a
/// `serde_json::Value` cannot hold (GAP-175, found by this file). Parsing the text rounds
/// that number to a float, which does not matter to a tag.
fn json_tag<T: Serialize>(value: &T) -> String {
    let text = serde_json::to_string(value).unwrap_or_else(|e| panic!("encode: {e}"));
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(serde_json::Value::String(tag)) => tag,
        Ok(serde_json::Value::Object(map)) if map.len() == 1 => {
            map.keys().next().cloned().unwrap_or_default()
        }
        other => panic!("not an externally tagged enum value: {other:?}"),
    }
}

/// Round-trip every value of one enum and return the variants they covered, checking
/// each exhaustive-match name against the tag serde wrote.
fn enum_round_trip<T>(name: &str, values: &[T]) -> BTreeSet<String>
where
    T: Serialize + DeserializeOwned + PartialEq + Debug + Variant,
{
    let mut covered = BTreeSet::new();
    for value in values {
        let variant = value.variant();
        assert_eq!(
            json_tag(value),
            variant,
            "{name}: the exhaustive match names this value {variant}"
        );
        round_trip(&format!("{name}::{variant}"), value);
        covered.insert(variant.to_string());
    }
    covered
}

/// Every event enum's values, round-tripped, by enum name.
fn event_tables() -> BTreeMap<&'static str, BTreeSet<String>> {
    BTreeMap::from([
        (
            "TrackingEvent",
            enum_round_trip("TrackingEvent", &tracking_events()),
        ),
        (
            "InterceptEvent",
            enum_round_trip("InterceptEvent", &intercept_events()),
        ),
        (
            "IngestEvent",
            enum_round_trip("IngestEvent", &ingest_events()),
        ),
        (
            "SensorEvent",
            enum_round_trip("SensorEvent", &sensor_events()),
        ),
        (
            "IdentityEvent",
            enum_round_trip("IdentityEvent", &identity_events()),
        ),
        (
            "CalibrationEvent",
            enum_round_trip("CalibrationEvent", &calibration_events()),
        ),
        (
            "SensorTaskEvent",
            enum_round_trip("SensorTaskEvent", &sensor_task_events()),
        ),
        (
            "GovernanceEvent",
            enum_round_trip("GovernanceEvent", &governance_events()),
        ),
        (
            "RhythmEvent",
            enum_round_trip("RhythmEvent", &rhythm_events()),
        ),
        (
            "RequirementEvent",
            enum_round_trip("RequirementEvent", &requirement_events()),
        ),
        (
            "CommandEvent",
            enum_round_trip("CommandEvent", &command_events()),
        ),
        (
            "EngagementEvent",
            enum_round_trip("EngagementEvent", &engagement_events()),
        ),
        (
            "HandoffEvent",
            enum_round_trip("HandoffEvent", &handoff_events()),
        ),
        (
            "ReviewEvent",
            enum_round_trip("ReviewEvent", &review_events()),
        ),
        (
            "VerdictSummary",
            enum_round_trip("VerdictSummary", &verdicts()),
        ),
        (
            "HealthEvent",
            enum_round_trip("HealthEvent", &health_events()),
        ),
        (
            "WarningEvent",
            enum_round_trip("WarningEvent", &warning_events()),
        ),
        (
            "LaunchWarningEvent",
            enum_round_trip("LaunchWarningEvent", &launch_warning_events()),
        ),
        (
            "RetentionEvent",
            enum_round_trip("RetentionEvent", &retention_events()),
        ),
        ("LinkEvent", enum_round_trip("LinkEvent", &link_events())),
        (
            "RehearsalEvent",
            enum_round_trip("RehearsalEvent", &rehearsal_events()),
        ),
        (
            "ReplayEvent",
            enum_round_trip("ReplayEvent", &replay_events()),
        ),
    ])
}

// ---------------------------------------------------------------------------
// Reading the source, so a new enum, variant or view cannot be left out
// ---------------------------------------------------------------------------

const EVENTS_RS: &str = include_str!("../src/events.rs");

/// Every `pub enum` in `events.rs`, with the variants its body declares.
///
/// The body is read by indentation, which `rustfmt` fixes: a variant is a line at four
/// spaces that starts with a capital letter, and the enum ends at the first `}` in
/// column one. Doc comments and attributes start with `/` or `#` and are skipped.
fn events_in_source() -> BTreeMap<String, BTreeSet<String>> {
    let mut out = BTreeMap::new();
    let mut current: Option<(String, BTreeSet<String>)> = None;
    for line in EVENTS_RS.lines() {
        if let Some((name, variants)) = current.as_mut() {
            if line.starts_with('}') {
                out.insert(std::mem::take(name), std::mem::take(variants));
                current = None;
            } else if let Some(rest) = line.strip_prefix("    ") {
                if rest.starts_with(|c: char| c.is_ascii_uppercase()) {
                    let ident: String = rest
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                        .collect();
                    variants.insert(ident);
                }
            }
        } else if let Some(rest) = line.strip_prefix("pub enum ") {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            current = Some((name, BTreeSet::new()));
        }
    }
    out
}

/// Every module of the crate, by path, for the `*View` scan.
const SOURCES: &[(&str, &str)] = &[
    ("lib.rs", include_str!("../src/lib.rs")),
    (
        "anomaly_settings.rs",
        include_str!("../src/anomaly_settings.rs"),
    ),
    ("arbitration.rs", include_str!("../src/arbitration.rs")),
    ("assets.rs", include_str!("../src/assets.rs")),
    ("effectors.rs", include_str!("../src/effectors.rs")),
    ("events.rs", include_str!("../src/events.rs")),
    ("exchange.rs", include_str!("../src/exchange.rs")),
    ("frame.rs", include_str!("../src/frame.rs")),
    ("handoff.rs", include_str!("../src/handoff.rs")),
    ("identifier.rs", include_str!("../src/identifier.rs")),
    ("identity.rs", include_str!("../src/identity.rs")),
    ("laydown.rs", include_str!("../src/laydown.rs")),
    ("plans.rs", include_str!("../src/plans.rs")),
    (
        "policy_settings.rs",
        include_str!("../src/policy_settings.rs"),
    ),
    ("profiles.rs", include_str!("../src/profiles.rs")),
    ("provenance.rs", include_str!("../src/provenance.rs")),
    ("quality.rs", include_str!("../src/quality.rs")),
    ("releasability.rs", include_str!("../src/releasability.rs")),
    ("requirements.rs", include_str!("../src/requirements.rs")),
    ("retention.rs", include_str!("../src/retention.rs")),
    ("rhythm.rs", include_str!("../src/rhythm.rs")),
    ("time.rs", include_str!("../src/time.rs")),
    (
        "uas_identification.rs",
        include_str!("../src/uas_identification.rs"),
    ),
    ("uas_platform.rs", include_str!("../src/uas_platform.rs")),
    ("ui_settings.rs", include_str!("../src/ui_settings.rs")),
    ("vocabulary.rs", include_str!("../src/vocabulary.rs")),
];

/// Every `pub struct` or `pub enum` whose name ends in `View`, across the crate.
fn views_in_source() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (_, text) in SOURCES {
        for line in text.lines() {
            let rest = line
                .strip_prefix("pub struct ")
                .or_else(|| line.strip_prefix("pub enum "));
            if let Some(rest) = rest {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if name.ends_with("View") {
                    out.insert(name);
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// **Every variant of every event enum survives `serde_json` with nothing lost**, and
/// the tables cover exactly what `events.rs` declares: a new enum or a new variant with
/// no value here fails by name.
#[test]
fn every_event_variant_round_trips_and_every_one_is_covered() {
    let covered = event_tables();
    let declared = events_in_source();
    let declared_names: BTreeSet<&str> = declared.keys().map(String::as_str).collect();
    let covered_names: BTreeSet<&str> = covered.keys().copied().collect();
    assert_eq!(
        covered_names, declared_names,
        "every enum in events.rs needs a table here, and every table an enum there"
    );
    for (name, variants) in &declared {
        assert_eq!(
            covered.get(name.as_str()),
            Some(variants),
            "{name}: the values here must cover every variant events.rs declares"
        );
    }
    // A guard on the scan itself, so a parser that silently found nothing cannot pass:
    // the GAP-067 walk counted 21 event enums and about 77 variants, and VerdictSummary
    // is the twenty-second enum the file declares.
    assert!(declared.len() >= 22, "{declared:?}");
    let variants: usize = declared.values().map(BTreeSet::len).sum();
    assert!(
        variants >= 80,
        "only {variants} variants found: {declared:?}"
    );
}

/// The struct `events.rs` declares beside the enums, carried by `LinkEvent::BothActed`
/// and round-tripped there; here on its own as well, so a field it gains is exercised
/// even if that variant's value were ever narrowed.
#[test]
fn the_engagement_side_round_trips_on_its_own() {
    round_trip(
        "EngagementSide",
        &EngagementSide {
            decision: decision(),
            plan: plan_id(),
            at: T1,
        },
    );
}

/// **Every `*View` type survives `serde_json` with nothing lost**, and the list here is
/// the list the source declares: a new view type with no value here fails by name.
#[test]
fn every_view_type_round_trips_and_every_one_is_covered() {
    let mut covered = BTreeSet::new();
    let mut check = |name: &'static str, count: usize| {
        assert!(count > 0, "{name}: no value");
        covered.insert(name.to_string());
    };

    let tracks = [track(1)];
    for (i, t) in tracks.iter().enumerate() {
        round_trip(&format!("TrackView {i}"), t);
    }
    check("TrackView", tracks.len());

    let detections = detections();
    for (i, d) in detections.iter().enumerate() {
        round_trip(&format!("DetectionView {i}"), d);
    }
    check("DetectionView", detections.len());

    let plans = plans();
    for (i, p) in plans.iter().enumerate() {
        round_trip(&format!("PlanView {i}"), p);
    }
    check("PlanView", plans.len());

    round_trip("InterceptSolutionView", &solution());
    check("InterceptSolutionView", 1);

    let resources = resources();
    for (i, r) in resources.iter().enumerate() {
        round_trip(&format!("ResourceView {i}"), r);
    }
    check("ResourceView", resources.len());

    round_trip("BearingRayView", &bearing_ray());
    let no_elevation = BearingRayView {
        elevation_rad: None,
        ..bearing_ray()
    };
    round_trip("BearingRayView without elevation", &no_elevation);
    check("BearingRayView", 2);

    round_trip("PipelineStatsView", &pipeline_stats());
    check("PipelineStatsView", 1);

    round_trip("AssetListView", &assets());
    check("AssetListView", 1);

    assert_eq!(
        covered,
        views_in_source(),
        "every *View type in gungnir-model needs a round trip here"
    );
}

/// The `*View` scan reads every module `lib.rs` declares, so a view in a new module is
/// found: a module missing from [`SOURCES`] fails here by name.
#[test]
fn the_view_scan_reads_every_module_of_the_crate() {
    let (_, lib) = SOURCES[0];
    let declared: BTreeSet<String> = lib
        .lines()
        .filter_map(|line| line.strip_prefix("pub mod "))
        .map(|rest| format!("{}.rs", rest.trim_end_matches(';')))
        .chain(std::iter::once("lib.rs".to_string()))
        .collect();
    let scanned: BTreeSet<String> = SOURCES.iter().map(|(p, _)| (*p).to_string()).collect();
    assert_eq!(scanned, declared);
}

/// The elevation of a bearing is `None` or a number, and the two stay apart through the
/// wire (DN-27 §10: a missing elevation read back as zero would put the detection on
/// the horizon). Checked on the JSON as well as by equality, because equality of two
/// values that both lost the distinction would prove nothing.
#[test]
fn a_missing_elevation_does_not_come_back_as_zero() {
    let ray = BearingRayView {
        elevation_rad: None,
        ..bearing_ray()
    };
    let text = serde_json::to_string(&ray).unwrap_or_else(|e| panic!("{e}"));
    assert!(text.contains("\"elevation_rad\":null"), "{text}");
    let back: BearingRayView = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(back.elevation_rad, None);
}

/// **A schema one version older is refused**, with both versions named. The crate's own
/// test checked `SCHEMA_VERSION + 1`; a rule of "this version or older" would pass that
/// and still read a journal whose shapes it does not know.
#[test]
fn an_older_schema_version_is_refused_with_both_versions_named() {
    let older = SCHEMA_VERSION
        .checked_sub(1)
        .unwrap_or_else(|| panic!("SCHEMA_VERSION starts above zero"));
    match check_schema_version(older) {
        Err(ModelError::SchemaVersion { expected, found }) => {
            assert_eq!((expected, found), (SCHEMA_VERSION, older));
        }
        other => panic!("version {older} was not refused as a mismatch: {other:?}"),
    }
    assert!(check_schema_version(SCHEMA_VERSION).is_ok());
    assert!(matches!(
        check_schema_version(SCHEMA_VERSION + 1),
        Err(ModelError::SchemaVersion { .. })
    ));
}
