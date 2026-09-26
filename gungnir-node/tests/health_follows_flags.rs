// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! GAP-125, the `gungnir-observability` row on the node: "service health flags toggled;
//! health mirrors flags" (`docs/verification-capability-table.md` §2), through the
//! functions the node's loop calls, in `main.rs`'s order: the gateway's tick, then
//! [`picture::read_health`], [`picture::Announcer::health`] and
//! [`picture::publish_picture`].
//!
//! Each service's flag is taken true, false, true while the other two stay healthy. After
//! every tick the node's health, the snapshot a connecting desktop reads, and the bus the
//! journal and every linked desktop read (GAP-161) all say what the services said: one
//! `HealthEvent::Changed` per transition, none for a tick that changed nothing. The
//! desktop is held to the same in `gungnir-app/tests/health_follows_flags.rs`.

use gungnir_api::transport::NodeApi;
use gungnir_api::v3::SnapshotResponse;
use gungnir_eventing::{Event, EventBus, InProcessBus};
use gungnir_ingest::{AllowAllAuthenticator, IngestError, IngestGateway, ProtocolAdapter};
use gungnir_intercept_service::{InterceptService, PlanOutcome};
use gungnir_model::events::HealthEvent;
use gungnir_model::{DetectionView, MissionTime, PlanView, ResourceView, SystemHealth, TrackView};
use gungnir_node::picture;
use gungnir_tracking_service::{PipelineStats, SubmitError, TrackingService};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// A flag the test holds and a service reads.
#[derive(Clone)]
struct Flag(Arc<AtomicBool>);

impl Flag {
    fn new() -> Self {
        Self(Arc::new(AtomicBool::new(true)))
    }
    fn set(&self, value: bool) {
        self.0.store(value, Ordering::SeqCst);
    }
    fn get(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

struct FlaggedTracking(Flag);

impl TrackingService for FlaggedTracking {
    fn submit_detection(&mut self, _: DetectionView) -> Result<(), SubmitError> {
        Ok(())
    }
    fn poll(&mut self, _: MissionTime) {}
    fn tracks(&self) -> &[TrackView] {
        &[]
    }
    fn is_healthy(&self) -> bool {
        self.0.get()
    }
}

struct FlaggedPlanner(Flag);

impl InterceptService for FlaggedPlanner {
    fn plan(&mut self, _: MissionTime, _: &[TrackView], _: &[ResourceView]) -> PlanOutcome {
        PlanOutcome::NoPlan {
            reason: "a test planner plans nothing".into(),
            progress: None,
        }
    }
    fn is_healthy(&self) -> bool {
        self.0.get()
    }
}

/// An adapter whose poll fails while its flag is down, which is what makes the gateway
/// report itself unhealthy for that tick (`IngestGateway::is_healthy`).
struct FlaggedAdapter(Flag);

impl ProtocolAdapter for FlaggedAdapter {
    fn name(&self) -> &'static str {
        "flagged"
    }
    fn poll(&mut self, _: MissionTime) -> Result<Vec<DetectionView>, IngestError> {
        if self.0.get() {
            Ok(Vec::new())
        } else {
            Err(IngestError::Io("the feed is down".into()))
        }
    }
}

#[test]
fn the_nodes_health_follows_each_flag_true_false_true() {
    let (tracking_flag, planning_flag, ingest_flag) = (Flag::new(), Flag::new(), Flag::new());
    let mut tracking = FlaggedTracking(tracking_flag.clone());
    let intercept = FlaggedPlanner(planning_flag.clone());
    let mut gateway = IngestGateway::new(Box::new(AllowAllAuthenticator));
    gateway.add_adapter(Box::new(FlaggedAdapter(ingest_flag.clone())));
    let api = Arc::new(NodeApi::new(SnapshotResponse::new(
        Vec::new(),
        None,
        SystemHealth::default(),
        Vec::new(),
    )));
    let bus = InProcessBus::new();
    let events = bus.subscribe();
    let mut announcer = picture::Announcer::new();
    let mut now = MissionTime(1_789_012_345.1);

    let mut tick = |expect_change: bool, step: &str| {
        now = MissionTime(now.0 + 0.1);
        // The loop's order: ingest first, so an adapter failure is this tick's.
        let _ = gateway.tick(now, &mut tracking);
        let health = picture::read_health(&tracking, &intercept, &gateway);
        let changed = announcer.health(&bus, now, health).expect("published");
        picture::publish_picture(
            &api,
            &[],
            &[],
            PipelineStats::default(),
            &PlanView::default(),
            None,
            health,
            Vec::new(),
        );

        let expected = SystemHealth {
            tracking_healthy: tracking_flag.get(),
            intercept_healthy: planning_flag.get(),
            ingest_healthy: ingest_flag.get(),
        };
        assert_eq!(
            health, expected,
            "{step}: the node read its services wrongly"
        );
        assert_eq!(announcer.current_health(), expected, "{step}");
        assert_eq!(
            api.snapshot().map(|s| s.health),
            Some(expected),
            "{step}: a desktop connecting now would read another health"
        );
        assert_eq!(changed, expect_change, "{step}: the change was misjudged");
        let recorded: Vec<HealthEvent> = events
            .try_iter()
            .filter_map(|e| match e.event {
                Event::Health(h) => Some(h),
                _ => None,
            })
            .collect();
        if expect_change {
            assert_eq!(
                recorded,
                vec![HealthEvent::Changed {
                    tracking_healthy: expected.tracking_healthy,
                    intercept_healthy: expected.intercept_healthy,
                    ingest_healthy: expected.ingest_healthy,
                    at: now,
                }],
                "{step}: a change should put exactly this transition on the record"
            );
        } else {
            assert!(
                recorded.is_empty(),
                "{step}: nothing changed and the record says it did: {recorded:?}"
            );
        }
    };

    tick(true, "the first tick");
    tick(false, "an unchanged tick");
    for (name, flag) in [
        ("tracking", &tracking_flag),
        ("planning", &planning_flag),
        ("ingest", &ingest_flag),
    ] {
        flag.set(false);
        tick(true, &format!("{name} down"));
        tick(false, &format!("{name} still down"));
        flag.set(true);
        tick(true, &format!("{name} back"));
        tick(false, &format!("{name} still back"));
    }
}
