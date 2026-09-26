// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The node's picture: the services that make it, and what goes out with it (GAP-120,
//! GAP-160).
//!
//! # Why this is a library module and not part of `main.rs`
//!
//! For the reason [`crate::approval`] is one. The backend-switching row of
//! `docs/verification-capability-table.md` §2 asks for one scenario through the embedded
//! and the remote backends and the two desktops' projections compared, and the remote
//! half is only worth running if the node side is the node's own code: a test that built
//! its own tracker and planner beside a `NodeApi` would prove that copy agreed with the
//! desktop, and say nothing about the binary. GAP-120's test found exactly such a
//! disagreement -- this binary built its planner with no local frame, so every plan it
//! proposed carried no intercept point where an embedded desktop's carried one -- and
//! it could only find it because it calls these functions.
//!
//! So the construction of the tracker, the planner and the ingest gateway, the adapter
//! the transport's submissions enter by, what the loop announces about its picture
//! ([`Announcer`]) and the picture published each tick live here; `main.rs` calls them,
//! and `gungnir-app/tests/backend_parity.rs` calls the same ones. The loop itself -- the
//! order the steps run in -- stays in `main.rs`, which is where a reader goes to check it.

use gungnir_api::transport::NodeApi;
use gungnir_api::v3::SnapshotResponse;
use gungnir_config::ConfigBaseline;
use gungnir_eventing::{Event, EventBus, EventingError};
use gungnir_ingest::{AllowListAuthenticator, IngestGateway};
use gungnir_intercept_service::{DpInterceptService, InterceptService, PlanOutcome};
use gungnir_model::events::InterceptEvent;
use gungnir_model::{MissionTime, PlanView, SensorId, SystemHealth};
use gungnir_observability::SnapshotHealthMonitor;
use gungnir_tracking_service::{
    project_pipeline_stats, LiveTrackingService, PipelineStats, SensorPositions, TrackLifecycle,
    TrackingService,
};
use std::sync::Arc;

/// The algorithm baseline the deployment has promoted for its operating profile
/// (GAP-086, GAP-053), or `None` when it governs nothing.
///
/// A baseline whose algorithm configuration is refused is said, and governs nothing:
/// running a configuration the registry would not hold under the promoted one's name
/// would be the fiction DN-24 §7 forbids.
#[must_use]
pub fn promoted_baseline(config: &ConfigBaseline) -> Option<gungnir_modelops::ModelBaseline> {
    match gungnir_modelops::InMemoryModelRegistry::from_baseline(config) {
        Ok(registry) => config
            .operating_profile()
            .and_then(|p| gungnir_modelops::ModelRegistry::promoted(&registry, &p))
            .cloned(),
        Err(err) => {
            tracing::error!(
                %err,
                "the baseline's algorithm configuration was refused; this node governs nothing"
            );
            None
        }
    }
}

/// The sensor positions the tracker needs to place an angular report (GAP-001, DN-27 §4).
///
/// Built from the baseline's own sensor list, which is where a deployment states where
/// each sensor is. Without it every bearing and every range-azimuth-elevation report is
/// refused, which is what happened until 2026-09-07.
///
/// **Geodetic in, ENU out (GAP-104).** `SensorConfig::position` is
/// `[lat_rad, lon_rad, alt_m]`; `SensorPositions` is metres in the local ENU frame. From
/// 2026-09-07 this handed the one straight to the other, which type-checks and placed
/// every sensor a metre or two from the ENU origin. So the conversion goes through the
/// deployment's local frame, the same one this binary converts sensor coverage with.
///
/// **No origin is a refusal, not a fallback.** A deployment that declared no origin has
/// no frame to convert into and there is no sound default for one, so this yields an
/// empty map -- and an empty map refuses every angular report by name
/// (`SubmitError::NotAPosition`) instead of placing a detection somewhere plausible and
/// wrong. It warns, because a silent refusal of every bearing looks exactly like no
/// angular feed reporting.
#[must_use]
pub fn sensor_positions(config: &ConfigBaseline) -> SensorPositions {
    let Some(frame) = config.local_frame() else {
        if !config.sensors.is_empty() {
            tracing::warn!(
                "no local frame origin is declared, so no sensor has a position in \
                 the tracking frame; every bearing and range-azimuth-elevation \
                 report will be refused"
            );
        }
        return SensorPositions::default();
    };
    SensorPositions::from_geodetic(
        &frame,
        config.sensors.iter().map(|s| {
            (
                s.id,
                gungnir_model::Geodetic {
                    lat_rad: s.position[0],
                    lon_rad: s.position[1],
                    alt_m: s.position[2],
                },
            )
        }),
    )
}

/// The node's tracker (GAP-012, GAP-053).
///
/// It judges staleness by the baseline's policy, and it filters as the promoted algorithm
/// baseline says, stamping that baseline's identifier only because it is applying it
/// (DN-24 §7). A baseline naming a filter this build does not implement is refused by name
/// and the tracker stays ungoverned, rather than running a different filter under the
/// promoted one's identity.
///
/// **And it applies the deployment's late-data policy in every case** (GAP-114, D-98):
/// `config.time.late_data` is time discipline, not part of an algorithm baseline, so it
/// governs the pipeline whether a baseline is promoted, applied, refused or absent.
#[must_use]
pub fn tracking_service(
    config: &ConfigBaseline,
    handle: &tokio::runtime::Handle,
    promoted: Option<&gungnir_modelops::ModelBaseline>,
) -> LiveTrackingService {
    // Validation refuses a policy the pipeline would; reaching the error arm means a
    // baseline bypassed it, and the log says what runs instead.
    let with_late_data = |settings: gungnir_tracking_service::PipelineSettings| {
        settings
            .clone()
            .with_late_data(config.time.late_data)
            .unwrap_or_else(|err| {
                tracing::error!(%err, "the baseline's late-data policy is not applied; the tracker runs the default one-second reorder buffer");
                settings
            })
    };
    let settings = promoted.map(|b| {
        // DN-28 §5: the imm-cv-ct fields, built from the baseline's own `TrackingConfig`
        // here rather than in `gungnir-tracking-service`, which may not depend on
        // `gungnir-config`.
        let imm = gungnir_tracking_service::ImmBaselineFields {
            turn_rate_rad_s: b.config.imm_turn_rate_rad_s,
            mode_transition: b.config.imm_mode_transition,
            initial_mode_probabilities: b.config.imm_initial_mode_probabilities,
        };
        (
            b,
            gungnir_tracking_service::PipelineSettings::from_baseline(
                b.config.gate_threshold,
                &b.config.filter_selection,
                &imm,
                b.config.measurement_noise_var,
            ),
        )
    });
    match settings {
        Some((baseline, Ok(settings))) => {
            tracing::info!(baseline = %baseline.id, "the promoted algorithm baseline is applied");
            LiveTrackingService::with_pipeline_settings(handle, with_late_data(settings))
                .with_staleness(config.policy.staleness.clone())
                .with_sensor_positions(sensor_positions(config))
                .with_algorithm_baseline(&baseline.id)
        }
        Some((baseline, Err(err))) => {
            tracing::error!(baseline = %baseline.id, %err, "the promoted algorithm baseline is not applied; the tracker runs its default filter and stays ungoverned");
            LiveTrackingService::with_pipeline_settings(
                handle,
                with_late_data(gungnir_tracking_service::PipelineSettings::default()),
            )
            .with_staleness(config.policy.staleness.clone())
            .with_sensor_positions(sensor_positions(config))
        }
        None => LiveTrackingService::with_pipeline_settings(
            handle,
            with_late_data(gungnir_tracking_service::PipelineSettings::default()),
        )
        .with_staleness(config.policy.staleness.clone())
        .with_sensor_positions(sensor_positions(config)),
    }
}

/// The node's planner, solving geometry in the deployment's frame when it has one
/// (GAP-031).
///
/// **The frame was missing until GAP-120.** This binary built its planner with
/// `DpInterceptService::new` alone while the desktop's embedded backend added the local
/// frame, so the same picture planned on a node produced every pairing with no intercept
/// point and no time to intercept, and a desktop linked to the node showed PN-05 an
/// answer an embedded desktop would never have given. Found by running one scenario
/// through both backends (`gungnir-app/tests/backend_parity.rs`); nothing had compared
/// them before.
///
/// **And its solve budget** (GAP-119, D-81): the node's loop is a tick like the
/// desktop's, so its planner spends at most the baseline's budget a tick and carries a
/// longer solve on to the next. The binary validates a baseline before building from it;
/// one built in code is not, so a bad budget is said loudly and MOP-06's is used, as the
/// desktop's `embedded_planner` does, rather than panicking here.
#[must_use]
pub fn intercept_service(config: &ConfigBaseline) -> DpInterceptService {
    let budget = config.plan_solve_budget().unwrap_or_else(|err| {
        tracing::error!(%err, "the baseline's solve budget is invalid; planning with MOP-06's");
        gungnir_intercept_service::DEFAULT_SOLVE_BUDGET
    });
    DpInterceptService::new(config.allocation_horizon)
        .with_local_frame(config.local_frame())
        .with_solve_budget(budget)
}

/// The ingest gateway, admitting exactly the sources the baseline names: its sensors,
/// and each peer under its own source id (DN-16 §5).
///
/// The feeds are bound onto it afterwards by the binary; this is what every node's
/// gateway starts as.
#[must_use]
pub fn gateway(config: &ConfigBaseline) -> IngestGateway {
    let mut gateway = IngestGateway::new(Box::new(AllowListAuthenticator {
        allowed: config
            .sensors
            .iter()
            .map(|s| SensorId(s.id))
            .chain(config.peers.iter().map(|p| SensorId(p.source_id)))
            .collect(),
    }));
    gateway.set_expected_adapters(config.sensors.len());
    gateway
}

/// Feeds the detections the transport accepted (`POST /v3/detections`) into the
/// gateway.
///
/// **A `ProtocolAdapter` rather than a direct call into the gateway**, so a submission is
/// authenticated against the sensor allow-list and validated by exactly the code a
/// sensor's own feed goes through. `gungnir-ingest` is the trust boundary for external
/// data; a route that reached past it would be a second way in with no checks on it.
pub struct ApiSubmissionAdapter {
    api: Arc<NodeApi>,
}

impl ApiSubmissionAdapter {
    #[must_use]
    pub fn new(api: Arc<NodeApi>) -> Self {
        Self { api }
    }
}

impl gungnir_ingest::ProtocolAdapter for ApiSubmissionAdapter {
    // The trait ties the returned lifetime to `&self`, so a `&'static str` here would
    // not match its signature. The adapters in `gungnir-ingest` are written the same way.
    #[allow(clippy::unnecessary_literal_bound)]
    fn name(&self) -> &str {
        "api-v3-submission"
    }

    fn poll(
        &mut self,
        _now: gungnir_model::MissionTime,
    ) -> Result<Vec<gungnir_model::DetectionView>, gungnir_ingest::IngestError> {
        Ok(self.api.take_submissions())
    }
}

/// What this node's services say about their health, read the way the loop reads it every
/// tick (GAP-125): each flag is the service's own `is_healthy`, never inferred.
///
/// Here rather than in `main.rs` so the test that toggles each flag and watches the node's
/// health follow (`gungnir-node/tests/health_follows_flags.rs`) reads the flags through the
/// function the binary calls.
#[must_use]
pub fn read_health(
    tracking: &dyn TrackingService,
    intercept: &dyn InterceptService,
    gateway: &IngestGateway,
) -> SystemHealth {
    SystemHealth {
        tracking_healthy: tracking.is_healthy(),
        intercept_healthy: intercept.is_healthy(),
        ingest_healthy: gateway.is_healthy(),
    }
}

/// What the loop announces about its picture, and remembers so it announces each thing
/// once: the track lifecycle (GAP-160), the plan (GAP-066) and the health (MOE-06).
///
/// **One owner for "what has already been said".** Each of the three is a comparison with
/// what was said last, and until GAP-120 the loop kept two of them as loose variables and
/// did not have the third at all. They are here so the binary and the backend-switching
/// row's test announce by the same rules; a test that re-derived when a plan is proposed
/// would be testing its own reading of `main.rs`.
#[derive(Debug, Default)]
pub struct Announcer {
    lifecycle: TrackLifecycle,
    last_plan: PlanView,
    /// What the services last reported, and whether a report is a change: the same
    /// monitor the desktop's tick reports through (GAP-125, D-100).
    health: SnapshotHealthMonitor,
}

impl Announcer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Put what changed in the picture on the bus, as `TrackingEvent`s (GAP-160).
    ///
    /// **Why every tick.** Every reader of this node was written against these events --
    /// a linked desktop's projection between snapshots, a partner's stream (DN-18 §6), and
    /// the journal the reports, the replay and the entity fold read. Until GAP-160 nothing
    /// published one, so a desktop linked to this node kept the picture its sign-in
    /// snapshot held for as long as the link stayed up.
    ///
    /// # Errors
    ///
    /// When the bus refuses an event.
    pub fn tracks(
        &mut self,
        bus: &dyn EventBus,
        now: MissionTime,
        tracks: &[gungnir_model::TrackView],
    ) -> Result<(), EventingError> {
        for event in self.lifecycle.changes(tracks) {
            bus.publish(now, Event::Tracking(event))?;
        }
        Ok(())
    }

    /// Propose this tick's plan when it is fresh and not the one last proposed, and hand
    /// it back for the approval queue (GAP-066, GAP-132).
    ///
    /// Only a plan computed for this snapshot is proposed. A stale one published as
    /// `PlanProposed` would be a recommendation nobody made now, and this node is the
    /// system of record for every desktop reading it.
    ///
    /// # Errors
    ///
    /// When the bus refuses the proposal.
    pub fn plan(
        &mut self,
        bus: &dyn EventBus,
        now: MissionTime,
        outcome: &PlanOutcome,
    ) -> Result<Option<PlanView>, EventingError> {
        let plan = outcome.plan().cloned().unwrap_or_default();
        if !outcome.is_fresh() || plan == self.last_plan {
            return Ok(None);
        }
        bus.publish(
            now,
            Event::Intercept(InterceptEvent::PlanProposed(plan.clone())),
        )?;
        self.last_plan = plan.clone();
        Ok(Some(plan))
    }

    /// The plan last proposed, which is the one the snapshot carries.
    #[must_use]
    pub fn last_plan(&self) -> &PlanView {
        &self.last_plan
    }

    /// Put a change in this node's health on the record (MOE-06), and say whether it
    /// changed. A repeat is not published: the transition is the fact, and a linked
    /// desktop reads it as the node's word on its services (GAP-161).
    ///
    /// **What counts as a change is `gungnir-observability`'s answer**
    /// ([`SnapshotHealthMonitor::report`]), the one the desktop's tick takes too
    /// (GAP-125, D-100).
    ///
    /// # Errors
    ///
    /// When the bus refuses the event, which ends the node's loop (`main.rs` returns
    /// it), so a transition refused here is never silently skipped by a later tick.
    pub fn health(
        &mut self,
        bus: &dyn EventBus,
        now: MissionTime,
        health: SystemHealth,
    ) -> Result<bool, EventingError> {
        let Some(changed) = self.health.report(health, now) else {
            return Ok(false);
        };
        bus.publish(now, Event::Health(changed))?;
        Ok(true)
    }

    /// The health last reported, which is the one the snapshot carries.
    #[must_use]
    pub fn current_health(&self) -> SystemHealth {
        use gungnir_observability::HealthMonitor;
        self.health.current_health()
    }
}

/// Publish the tick's picture as the snapshot a desktop reads when it connects.
///
/// Requirements are empty: a node states none of its own -- PN-15 is a desktop panel,
/// and publishing an empty list is different from the field being absent (GAP-005).
///
/// **`bearing_rays`/`pipeline_stats` are the same values `tracking.bearing_rays()`/
/// `tracking.pipeline_stats()` already give an embedded desktop** (GAP-096's wire
/// contract): attached here so a connected one reads the same picture rather than the
/// `TrackingService` trait's defaulted empty answer `gungnir-remote` gave before this
/// entry. Refreshed every tick like `tracks`, so a snapshot taken right after this call
/// is as current as the pipeline's last poll -- there is no separate live update for
/// either between snapshots (see `gungnir_remote::RemoteTrackingService`'s own doc
/// comment for what that means for a connected desktop).
///
/// The tracks and the plan **are** kept live between snapshots, by the events the loop
/// publishes: `TrackingEvent` from [`gungnir_tracking_service::TrackLifecycle`] since
/// GAP-160, and `PlanProposed`. The health is too, by `HealthEvent::Changed`, which a
/// linked desktop has read since GAP-161.
pub fn publish_picture(
    api: &Arc<NodeApi>,
    tracks: &[gungnir_model::TrackView],
    bearing_rays: &[gungnir_model::BearingRayView],
    pipeline_stats: PipelineStats,
    plan: &PlanView,
    health: SystemHealth,
    queue: Vec<gungnir_api::v3::QueueItemView>,
) {
    let snapshot = SnapshotResponse::new(tracks.to_vec(), Some(plan.clone()), health, Vec::new())
        .with_bearing_data(
            bearing_rays.to_vec(),
            project_pipeline_stats(pipeline_stats),
        )
        .with_queue(queue);
    if let Err(err) = api.publish_snapshot(snapshot) {
        tracing::error!(%err, "could not publish the snapshot");
    }
}
