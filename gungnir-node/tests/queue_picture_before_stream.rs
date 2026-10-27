// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! **A desktop that reads the queue on a queue event reads a queue that holds it**
//! (GAP-201; DN-31 §6.6).
//!
//! A linked desktop answers `CommandEvent::Queued` and `Escalated` by reading
//! `GET /v3/queue` at once, and that route serves the `queue` of the snapshot the node's
//! loop last published (`NodeApi::queue`). Until GAP-201 the loop offered the tick's
//! envelopes and only then published the tick's picture, so a fast desktop could read the
//! queue as it was before the item, take that as its picture and clear the mark the frame
//! had set. The item never showed on that desktop until the queue moved again. It failed
//! once on CI in `gungnir-app/tests/linked_plan_standing.rs` and never locally.
//!
//! **The window is forced, not waited for.** `picture::publish_picture` takes the tick's
//! envelopes as an iterator and offers each one as it draws it, so the iterator here is a
//! hook that runs at the moment an envelope is about to reach a desktop: it reads the
//! queue exactly as the route would at that moment. Under the old order every read in
//! this file comes back without the tick's item, on any machine at any speed.

use gungnir_api::transport::NodeApi;
use gungnir_api::v3::{QueueItemView, SnapshotResponse};
use gungnir_config::{ConfigBaseline, ResourceConfig};
use gungnir_eventing::{Envelope, Event, EventBus, InProcessBus, Receiver};
use gungnir_geo::InMemoryGeoService;
use gungnir_model::events::CommandEvent;
use gungnir_model::policy_settings::AuthorityRule;
use gungnir_model::{
    Classification, EffectorLayer, InterceptSolutionView, MissionTime, PlanId, PlanKind, PlanView,
    Provenance, Quality, Releasability, ResourceId, ResourceView, SystemHealth, TrackId,
    TrackStatus, TrackView, WeaponsControlStatus,
};
use gungnir_node::approval::{self, Frame, NodeApproval};
use gungnir_node::picture;
use gungnir_security::actions;
use gungnir_tracking_service::PipelineStats;
use std::sync::Arc;

const SUBMITTED: f64 = 100.0;
const ESCALATE_AFTER_S: f64 = 10.0;

/// A point effector, weapons free, an Operator who may decide a point engagement against
/// a hostile track and a Supervisor it escalates to.
fn baseline() -> ConfigBaseline {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-node-queue-before-stream-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let mut config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        resources: vec![ResourceConfig {
            handoff_endpoint: None,
            id: 0,
            position: [0.0, 0.0, 0.0],
            capacity: 4,
            layer: "point".into(),
            cost: None,
            rounds_available: None,
            reserve: None,
            intercept_speed_mps: Some(400.0),
        }],
        ..ConfigBaseline::default()
    };
    config
        .policy
        .control_status
        .by_layer
        .insert(EffectorLayer::Point, WeaponsControlStatus::Free);
    for role in ["Operator", "Supervisor"] {
        config.policy.authority.rules.push(AuthorityRule {
            action: actions::DECIDE_PLAN.into(),
            role: role.into(),
            layer: Some(EffectorLayer::Point),
            class: Some("hostile".into()),
            pre_delegated: false,
        });
    }
    config
        .policy
        .decisions
        .escalate_after_s
        .insert(EffectorLayer::Point, ESCALATE_AFTER_S);
    config
        .assessment
        .effect_window_s
        .insert(EffectorLayer::Point, 60.0);
    gungnir_config::validate(&config).expect("the baseline is valid");
    config
}

fn hostile(id: u64) -> TrackView {
    TrackView {
        id: TrackId(id),
        status: TrackStatus::Confirmed,
        state: nalgebra::SVector::zeros(),
        covariance: nalgebra::SMatrix::identity(),
        classification: Classification::Hostile,
        provenance: Provenance::default(),
        quality: Quality::default(),
        mission_time: MissionTime(SUBMITTED),
        releasability: Releasability::default(),
    }
}

fn plan(track: u64) -> PlanView {
    PlanView {
        id: PlanId(1),
        mission_time: MissionTime(SUBMITTED),
        kind: PlanKind::Intercept {
            solutions: vec![InterceptSolutionView {
                resource: ResourceId(0),
                track: TrackId(track),
                intercept_point: None,
                time_to_intercept_s: Some(10.0),
            }],
        },
        ..PlanView::default()
    }
}

/// The node's loop state, without a transport: what is under test is the order in which
/// the loop's two doors open, not the HTTP behind them.
struct Node {
    api: Arc<NodeApi>,
    config: ConfigBaseline,
    resources: Vec<ResourceView>,
    geo: InMemoryGeoService,
    bus: InProcessBus,
    api_rx: Receiver<Envelope>,
    approval: NodeApproval,
}

/// One queue event, and the queue `GET /v3/queue` would have served at the moment it was
/// offered.
struct Offered {
    event: CommandEvent,
    queue: Vec<QueueItemView>,
}

impl Node {
    fn new() -> Self {
        let config = baseline();
        let bus = InProcessBus::new();
        let api_rx = bus.subscribe();
        let api = Arc::new(NodeApi::new(SnapshotResponse::new(
            Vec::new(),
            None,
            SystemHealth::default(),
            Vec::new(),
        )));
        Self {
            api,
            resources: config.resource_views(),
            geo: InMemoryGeoService::new(Vec::new(), Vec::new()),
            approval: NodeApproval::new(&config),
            config,
            bus,
            api_rx,
        }
    }

    /// One tick in `main.rs`'s order for the queue: propose, sweep, then the picture and
    /// the tick's envelopes through `publish_picture`. Returns every queue event offered,
    /// each with the queue the route would have served as it was offered.
    fn step(&mut self, now: f64, tracks: &[TrackView], proposal: Option<PlanView>) -> Vec<Offered> {
        let now = MissionTime(now);
        let frame = Frame {
            now,
            config: &self.config,
            tracks,
            resources: &self.resources,
            geofences: &self.geo,
            bus: &self.bus,
            endpoint_client: None,
            effectors: None,
            api: &self.api,
        };
        if let Some(plan) = proposal {
            let _ = approval::propose(&mut self.approval, &frame, plan);
        }
        approval::sweep(&mut self.approval, &frame);
        self.api.set_now(now.0);
        let queue = self
            .approval
            .queue_view(&self.config, &self.resources, tracks);

        let mut offered = Vec::new();
        let api = Arc::clone(&self.api);
        // The hook: drawn by `publish_picture` immediately before it offers the envelope.
        let envelopes = self.api_rx.try_iter().inspect(|envelope| {
            if let Event::Command(
                event @ (CommandEvent::Queued { .. } | CommandEvent::Escalated { .. }),
            ) = &envelope.event
            {
                offered.push(Offered {
                    event: event.clone(),
                    queue: api.queue(),
                });
            }
        });
        picture::publish_picture(
            &self.api,
            tracks,
            &[],
            PipelineStats::default(),
            None,
            &PlanView::default(),
            None,
            SystemHealth::default(),
            queue,
            self.approval.held_layers(&self.config),
            envelopes,
        );
        offered
    }
}

#[test]
fn a_queue_event_is_offered_only_once_the_queue_it_announces_is_served() {
    let mut node = Node::new();
    let tracks = [hostile(7)];

    // Queued: the item is in the queue a desktop reads on the frame.
    let queued = node.step(SUBMITTED, &tracks, Some(plan(7)));
    assert_eq!(queued.len(), 1, "one plan, one Queued");
    let Offered { event, queue } = &queued[0];
    let CommandEvent::Queued { item, .. } = event else {
        panic!("expected Queued, got {event:?}");
    };
    assert!(
        queue.iter().any(|row| row.item == *item),
        "Queued for {item:?} was offered while GET /v3/queue served {:?}: a desktop \
         reading on the frame takes a picture without the item and clears its mark",
        queue.iter().map(|row| row.item).collect::<Vec<_>>()
    );

    // Escalated: the item is offered to the new role in the queue a desktop reads.
    let escalated = node.step(SUBMITTED + ESCALATE_AFTER_S + 1.0, &tracks, None);
    assert_eq!(
        escalated.len(),
        1,
        "one item past its escalation, one Escalated"
    );
    let Offered { event, queue } = &escalated[0];
    let CommandEvent::Escalated { to_role, .. } = event else {
        panic!("expected Escalated, got {event:?}");
    };
    assert!(
        queue
            .iter()
            .any(|row| row.item == *item && row.offered_to.contains(to_role)),
        "Escalated to {to_role} was offered while GET /v3/queue served {queue:?}"
    );
}
