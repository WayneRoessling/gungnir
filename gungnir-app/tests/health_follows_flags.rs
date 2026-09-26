// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! GAP-125, the `gungnir-observability` row on the desktop: "service health flags
//! toggled; health mirrors flags" (`docs/verification-capability-table.md` §2), through
//! the desktop's own tick.
//!
//! Each service's flag is taken true, false, true while the other two stay healthy, and
//! after every tick the health the strip, PN-09 and PN-07 read (`AppState::health`) is
//! exactly what the three services report, and the bus -- which is what the journal
//! records -- carries one `HealthEvent::Changed` per transition and none for a tick that
//! changed nothing. The node's loop is held to the same in
//! `gungnir-node/tests/health_follows_flags.rs`; both report through
//! `gungnir_observability::SnapshotHealthMonitor` (D-100).
//!
//! The flags are the services' own `is_healthy`, not a value this test writes into the
//! state: tracking and planning are test doubles whose answer the test holds, and ingest
//! is the real `IngestGateway` with an adapter whose poll the test makes fail, because
//! a failed poll is what makes a gateway unhealthy.

use gungnir_app::state::AppState;
use gungnir_app::update;
use gungnir_config::ConfigBaseline;
use gungnir_eventing::{Envelope, Event, Receiver};
use gungnir_ingest::{AllowAllAuthenticator, IngestError, IngestGateway, ProtocolAdapter};
use gungnir_intercept_service::{InterceptService, PlanOutcome};
use gungnir_model::events::HealthEvent;
use gungnir_model::{DetectionView, MissionTime, ResourceView, SystemHealth, TrackView};
use gungnir_tracking_service::{SubmitError, TrackingService};
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

/// The health events the bus carried since the last call.
fn health_events(events: &Receiver<Envelope>) -> Vec<HealthEvent> {
    events
        .try_iter()
        .filter_map(|e| match e.event {
            Event::Health(h) => Some(h),
            _ => None,
        })
        .collect()
}

#[test]
fn the_desktops_health_follows_each_flag_true_false_true() {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-health-follows-flags-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        ..ConfigBaseline::default()
    };
    let mut state = AppState::with_config(config).expect("the desktop starts");
    let (tracking, planning, ingest) = (Flag::new(), Flag::new(), Flag::new());
    state.tracking = Box::new(FlaggedTracking(tracking.clone()));
    state.intercept = Box::new(FlaggedPlanner(planning.clone()));
    let mut gateway = IngestGateway::new(Box::new(AllowAllAuthenticator));
    gateway.add_adapter(Box::new(FlaggedAdapter(ingest.clone())));
    state.ingest = gateway;
    let events = state.events.subscribe();

    // One tick with the flags as set, then the health on screen and the record.
    let tick = |state: &mut AppState, expect_change: bool, step: &str| {
        update::tick(state);
        let expected = SystemHealth {
            tracking_healthy: tracking.get(),
            intercept_healthy: planning.get(),
            ingest_healthy: ingest.get(),
        };
        assert_eq!(
            state.health(),
            expected,
            "{step}: the desktop's health is not what its services report"
        );
        let recorded = health_events(&events);
        if expect_change {
            assert_eq!(
                recorded.len(),
                1,
                "{step}: a change should put exactly one transition on the record: \
                 {recorded:?}"
            );
            let HealthEvent::Changed {
                tracking_healthy,
                intercept_healthy,
                ingest_healthy,
                ..
            } = recorded[0];
            assert_eq!(
                SystemHealth {
                    tracking_healthy,
                    intercept_healthy,
                    ingest_healthy,
                },
                expected,
                "{step}: the record says something other than the services did"
            );
        } else {
            assert!(
                recorded.is_empty(),
                "{step}: nothing changed and the record says it did: {recorded:?}"
            );
        }
    };

    tick(&mut state, true, "the first tick");
    tick(&mut state, false, "an unchanged tick");
    for (name, flag) in [
        ("tracking", &tracking),
        ("planning", &planning),
        ("ingest", &ingest),
    ] {
        flag.set(false);
        tick(&mut state, true, &format!("{name} down"));
        tick(&mut state, false, &format!("{name} still down"));
        flag.set(true);
        tick(&mut state, true, &format!("{name} back"));
        tick(&mut state, false, &format!("{name} still back"));
    }
    let _ = std::fs::remove_dir_all(dir);
}
