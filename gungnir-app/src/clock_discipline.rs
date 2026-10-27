// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The desktop's side of a clock disciplined from a named sensor (GAP-205; plan 12
//! tranche A): feeding it, journaling its changes, and saying them.
//!
//! The clock itself is `gungnir_time::DisciplinedClockAuthority`. This module feeds it
//! the named sensor's reports, from two places: every accepted report, and every report
//! the gateway quarantined under a clock rule alone, which carries its stamps
//! (`IngestEvent::Quarantined::stamps`). The second is what lets a host running behind a
//! GNSS radar hear the radar at all, because the gateway refuses a report more than a
//! second ahead of its receipt, and that is exactly the case this exists for.
//!
//! **Each state change is journaled** as `HealthEvent::ClockDiscipline` and raised as an
//! alert: the record says from when mission time was the host's and from when it was the
//! sensor's. **A lock forgets every skew estimate** (`ClockSkewEstimator::reset`), because
//! every offset measured before was measured against a different clock and the least
//! offset would otherwise keep a source out of sync for ever.

use crate::state::AppState;
use gungnir_eventing::Event;
use gungnir_model::events::{HealthEvent, IngestEvent};
use gungnir_model::{ClockDisciplineState, MissionTime, SensorId};

/// Feed one ingest event to the discipline, if there is one and it is a report from the
/// disciplining sensor with stamps to read.
pub fn observe(state: &AppState, event: &IngestEvent, applied: f64) {
    let Some(handle) = state.clock_discipline.as_ref() else {
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

/// Journal and say a change in the discipline's state, once per change.
pub fn poll(state: &mut AppState, now: MissionTime) {
    let Some(handle) = state.clock_discipline.as_ref() else {
        return;
    };
    let Some(changed) = handle.poll() else {
        return;
    };
    let sensor = handle.sensor();
    if matches!(changed, ClockDisciplineState::Locked { .. }) {
        state.clock_skew.reset();
        state.skew_alerted.clear();
    }
    state.alerts.push(sentence(sensor, &changed));
    crate::update::publish(
        state,
        now,
        Event::Health(HealthEvent::ClockDiscipline {
            sensor: SensorId(sensor),
            state: changed,
            at: now,
        }),
    );
}

/// The discipline's state in PN-09's and the alert list's words.
#[must_use]
pub fn sentence(sensor: u32, state: &ClockDisciplineState) -> String {
    match state {
        ClockDisciplineState::Waiting => format!(
            "Clock disciplined by sensor {sensor}: waiting for its first report; mission time \
             is this host's clock until then"
        ),
        ClockDisciplineState::Locked {
            offset_s, samples, ..
        } => format!(
            "Clock disciplined by sensor {sensor}: locked; this host's clock is {} by {:.3} s, \
             over {samples} report(s)",
            if *offset_s >= 0.0 { "ahead" } else { "behind" },
            offset_s.abs()
        ),
        ClockDisciplineState::Holdover {
            offset_s,
            last_sample,
        } => format!(
            "Clock disciplined by sensor {sensor}: holding over; nothing heard since T+{:.0} s, \
             and the last offset ({offset_s:+.3} s) is still applied",
            last_sample.0
        ),
        ClockDisciplineState::Refused { offset_s } => format!(
            "Clock discipline refused: sensor {sensor} reads {:.0} s from this host's clock, \
             more than the {:.0} s a discipline will step; set this host's clock by hand",
            offset_s.abs(),
            gungnir_time::MAX_DISCIPLINE_OFFSET_S
        ),
    }
}
