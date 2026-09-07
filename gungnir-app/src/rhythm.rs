// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The watch's rhythm on the tick: scheduled products, planned downtime, handover
//! (GAP-054, `docs/design/DN-21-battle-rhythm.md`).
//!
//! # The scheduler runs on mission time
//!
//! Products are due at times the *schedule* names, and this publishes them with those
//! times rather than with the moment the tick happened to notice. A wall-clock scheduler
//! would make a replayed session produce a different set of products at different moments,
//! which breaks the determinism the journal rests on (AP-08). DN-21 §8 makes that its
//! first pass criterion precisely because a wall-clock implementation passes every other
//! check.
//!
//! # What is honestly empty here
//!
//! `open_engagements` and `warnings_owed` are always empty, because engagement tracking
//! (GAP-043) and the warning function (GAP-042) are not built. They are empty because
//! **nothing in this build can owe a warning or open an engagement**, not because the
//! summary looked and found none -- and [`HandoverSummaryView::unavailable`] says so on
//! the panel rather than letting a blank line read as "all clear".

use crate::state::AppState;
use gungnir_eventing::Event;
use gungnir_model::events::RhythmEvent;
use gungnir_model::{MaintenanceState, MissionTime, ProductKind, SensorId};
use gungnir_reporting::{HandoverSummary, MaintenanceWindow, RhythmError};
use gungnir_sensor_management::SensorRegistry;

/// What the rhythm needs to remember between frames.
///
/// Small on purpose: the schedules live in the baseline and the windows live on the sensor
/// records, so the only state here is where the scheduler has got to and the summary
/// somebody is part-way through completing.
#[derive(Debug, Default)]
pub struct RhythmState {
    /// The mission time the scheduler has run to. Products due in `(swept_to, now]` fire.
    ///
    /// `None` before the first tick: the first frame establishes the mark rather than
    /// firing every product whose schedule ever passed, which on a session starting at
    /// Unix seconds would be a very large number of them.
    swept_to: Option<MissionTime>,
    /// The summary being handed over, once one has come due.
    handover: Option<HandoverSummary>,
}

impl RhythmState {
    /// The summary awaiting notes and an acknowledgement, when one is open.
    #[must_use]
    pub fn handover(&self) -> Option<&HandoverSummary> {
        self.handover.as_ref()
    }
}

/// Run the rhythm for this frame.
///
/// Called every tick, like the decision sweep and for the same reason: a window closes on
/// the clock, not on new input.
pub fn tick(state: &mut AppState) {
    let now = state.clock.now();
    advance_maintenance(state, now);
    produce_due_products(state, now);
    state.rhythm.swept_to = Some(now);
}

/// Move every maintenance window to the state it should be in, and say what changed.
///
/// **An overrun is the only one that alerts.** A window opening or closing on schedule is
/// the system doing what somebody planned; a window closing with the sensor still down is
/// the thing nobody planned.
fn advance_maintenance(state: &mut AppState, now: MissionTime) {
    for (sensor, next) in state.sensors.advance_maintenance(now) {
        let window = window_for(state, sensor, now);
        let event = match next {
            MaintenanceState::Active => RhythmEvent::MaintenanceOpened {
                sensor,
                until: window.as_ref().map_or(now, |w| w.to),
                reason: reason_of(window.as_ref()),
                at: now,
            },
            MaintenanceState::Completed => RhythmEvent::MaintenanceCompleted { sensor, at: now },
            MaintenanceState::Overrun => {
                state.alerts.push(format!(
                    "Sensor {} did not return from planned maintenance ({}); it is off the \
                     air and nobody has said why",
                    sensor.0,
                    reason_of(window.as_ref())
                ));
                RhythmEvent::MaintenanceOverrun {
                    sensor,
                    window_closed: window.as_ref().map_or(now, |w| w.to),
                    reason: reason_of(window.as_ref()),
                    at: now,
                }
            }
            // `advance_maintenance` never reports a return to Planned; it reports
            // transitions, and Planned is where a window starts.
            MaintenanceState::Planned => continue,
        };
        crate::update::publish(state, now, Event::Rhythm(event));
    }
}

fn window_for(state: &AppState, sensor: SensorId, now: MissionTime) -> Option<MaintenanceWindow> {
    state.sensors.get(sensor).and_then(|s| {
        s.maintenance
            .iter()
            .find(|w| w.is_open_at(now) || w.state == MaintenanceState::Overrun)
            .cloned()
    })
}

fn reason_of(window: Option<&MaintenanceWindow>) -> String {
    window.map_or_else(|| "reason not recorded".to_owned(), |w| w.reason.clone())
}

/// Publish every product the schedule brought due since the last tick.
///
/// The `due` time on each event is the schedule's, not this frame's, which is what makes a
/// replay reproduce the same set at the same moments.
fn produce_due_products(state: &mut AppState, now: MissionTime) {
    let Some(from) = state.rhythm.swept_to else {
        // First frame: establish the mark. Firing everything a schedule ever passed
        // would, on a session whose clock starts at Unix seconds, produce decades of
        // handover summaries in one frame.
        return;
    };
    let products: Vec<_> = state
        .config
        .reporting
        .scheduled
        .iter()
        .filter_map(|p| p.to_product().map(|product| (p.clone(), product)))
        .collect();

    for (_, product) in products {
        for due in product.schedule.due_between(from, now) {
            crate::update::publish(
                state,
                due,
                Event::Rhythm(RhythmEvent::ProductDue {
                    name: product.name.clone(),
                    kind: product.kind,
                    due,
                }),
            );
            if product.kind == ProductKind::HandoverSummary {
                state.rhythm.handover = Some(assemble_handover(state, due));
            }
            match &product.deliver_to {
                // Produced and held for a person to read. A real configuration, and the
                // reason this is not the same event as the one below.
                None => crate::update::publish(
                    state,
                    due,
                    Event::Rhythm(RhythmEvent::ProductHeld {
                        name: product.name.clone(),
                        at: due,
                    }),
                ),
                // **Never a silent drop.** There is no delivery path (GAP-040), so a
                // product with an endpoint is recorded as owed. A deployment that believed
                // it was reporting to higher command and was not is exactly what DN-21 §5's
                // delivery rule is there to prevent.
                Some(endpoint) => crate::update::publish(
                    state,
                    due,
                    Event::Rhythm(RhythmEvent::ProductUndelivered {
                        name: product.name.clone(),
                        endpoint: endpoint.clone(),
                        reason: "no delivery path exists yet (GAP-040)".to_owned(),
                        at: due,
                    }),
                ),
            }
        }
    }
}

/// Assemble what the incoming watch needs, from the record.
///
/// Every field but the notes comes from state the system already holds; the notes are the
/// outgoing watch's judgement and the part that matters most (DN-21 §5).
#[must_use]
pub fn assemble_handover(state: &AppState, at: MissionTime) -> HandoverSummary {
    let from = state.rhythm.swept_to.unwrap_or(at);
    let degraded = state
        .sensors
        .sensors()
        .iter()
        .filter(|s| s.absence_is_a_fault(at))
        .map(|s| s.id)
        .collect();
    HandoverSummary {
        period: (from, at),
        outgoing: Some(format!("{:?}", state.role())),
        open_alerts: state.alerts.len(),
        // Empty because nothing in this build opens an engagement (GAP-043) or owes a
        // warning (GAP-042), not because the record was searched and came back clean.
        // The panel says which, rather than drawing a blank that reads as "all clear".
        open_engagements: Vec::new(),
        pending_approvals: {
            use gungnir_command::ApprovalWorkflow;
            state.approvals.pending().len()
        },
        expired_approvals: crate::decisions::expired_count(state),
        sensors_degraded: degraded,
        maintenance_due: state.sensors.maintenance_outstanding(at),
        warnings_owed: Vec::new(),
        baseline_version: state.config.version,
        notes: None,
        acknowledged_by: None,
    }
}

/// Record the incoming watch taking over.
///
/// **This is what MOE-13 counts**, which is why it publishes: an acknowledgement that only
/// changed a field in memory would leave the measure with nothing to count.
///
/// # Errors
///
/// When no handover is open, or the summary refuses the acknowledgement -- an empty name,
/// or a second acknowledgement of one already taken.
pub fn acknowledge_handover(state: &mut AppState, by: &str) -> Result<(), String> {
    let now = state.clock.now();
    let Some(summary) = state.rhythm.handover.as_mut() else {
        return Err("no handover is open".to_owned());
    };
    summary
        .acknowledge(by, now)
        .map_err(|e: RhythmError| e.to_string())?;
    let (period, outstanding) = (summary.period, summary.has_outstanding_work());
    crate::update::publish(
        state,
        now,
        Event::Rhythm(RhythmEvent::HandoverAcknowledged {
            by: by.to_owned(),
            period,
            outstanding,
            at: now,
        }),
    );
    Ok(())
}

/// Add the outgoing watch's notes to the open handover.
///
/// # Errors
///
/// When no handover is open, or it has already been acknowledged -- the record of what one
/// watch told the next is not rewritten after the next watch accepted it.
pub fn set_handover_notes(state: &mut AppState, notes: impl Into<String>) -> Result<(), String> {
    let Some(summary) = state.rhythm.handover.as_mut() else {
        return Err("no handover is open".to_owned());
    };
    if summary.is_complete() {
        return Err("this handover was already acknowledged".to_owned());
    }
    let notes = notes.into();
    summary.notes = (!notes.trim().is_empty()).then_some(notes);
    Ok(())
}
