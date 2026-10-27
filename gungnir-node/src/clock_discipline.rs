// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The node's side of a clock disciplined from a named sensor (GAP-205; plan 12 tranche
//! A), the same as the desktop's (`gungnir_app::clock_discipline`), by the owner's choice
//! of 2026-10-26 that both binaries run it.
//!
//! Fed from every accepted report from the named sensor and every report the gateway
//! quarantined under a clock rule alone, which carries its stamps; a state change is
//! published as `HealthEvent::ClockDiscipline`, so this node's journal says from when its
//! mission time was the host's and from when it was the sensor's, and logged.

use gungnir_eventing::{Event, EventBus, EventingError};
use gungnir_model::events::{HealthEvent, IngestEvent};
use gungnir_model::{MissionTime, SensorId};
use gungnir_time::DisciplineHandle;

/// Feed one ingest event to the discipline, if there is one.
pub fn observe(handle: Option<&DisciplineHandle>, event: &IngestEvent, applied: f64) {
    let Some(handle) = handle else {
        return;
    };
    match event {
        IngestEvent::Accepted(d) => {
            handle.observe(d.sensor.0, d.source_time, d.receipt_time, applied);
        }
        IngestEvent::Quarantined {
            sensor,
            stamps: Some(s),
            ..
        } => handle.observe(sensor.0, s.source_time, s.receipt_time, applied),
        _ => {}
    }
}

/// Publish a change in the discipline's state, once per change.
///
/// # Errors
///
/// The bus's, when the event could not be published.
pub fn poll(
    handle: Option<&DisciplineHandle>,
    bus: &dyn EventBus,
    now: MissionTime,
) -> Result<(), EventingError> {
    let Some(handle) = handle else {
        return Ok(());
    };
    let Some(state) = handle.poll() else {
        return Ok(());
    };
    let sensor = handle.sensor();
    tracing::info!(
        sensor,
        state = state.kind(),
        ?state,
        "the clock's discipline changed"
    );
    bus.publish(
        now,
        Event::Health(HealthEvent::ClockDiscipline {
            sensor: SensorId(sensor),
            state,
            at: now,
        }),
    )
    .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_eventing::InProcessBus;
    use gungnir_model::events::IngestEvent;
    use gungnir_model::{ClockDisciplineState as S, ClockStamps, LateDataPolicy};
    use gungnir_time::{DisciplinedClockAuthority, TimeAuthority};

    /// A host 40 s behind its radar: every radar report is a clock-rule quarantine, and
    /// the stamps it carries are what lock the clock; the lock is published once.
    #[test]
    fn a_clock_rule_quarantine_locks_the_clock_and_the_lock_is_published() {
        let host: gungnir_time::HostClock = std::sync::Arc::new(|| 1_000.0);
        let (clock, handle) =
            DisciplinedClockAuthority::with_host(LateDataPolicy::default(), 7, host);
        let bus = InProcessBus::new();
        let rx = bus.subscribe();
        poll(Some(&handle), &bus, clock.now()).expect("published");
        let receipt = clock.now();
        let event = IngestEvent::Quarantined {
            sensor: SensorId(7),
            reason: "source time is ahead of receipt time".into(),
            stamps: Some(ClockStamps {
                source_time: MissionTime(1_039.98),
                receipt_time: receipt,
            }),
        };
        observe(Some(&handle), &event, handle.applied_offset());
        poll(Some(&handle), &bus, clock.now()).expect("published");
        poll(Some(&handle), &bus, clock.now()).expect("nothing new");
        let states: Vec<_> = rx
            .try_iter()
            .filter_map(|e| match e.event {
                Event::Health(HealthEvent::ClockDiscipline { state, .. }) => Some(state.kind()),
                _ => None,
            })
            .collect();
        assert_eq!(states, ["waiting", "locked"]);
        assert!(
            (clock.now().0 - 1_039.98).abs() < 1e-6,
            "{:?}",
            handle.state()
        );
        assert!(matches!(handle.state(), S::Locked { .. }));
    }
}
