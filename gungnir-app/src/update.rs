// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Per-frame update/tick logic (non-render). Never do I/O, allocation-heavy work, or
//! blocking calls here beyond the journal append, which is a small buffered write
//! (rust-ui-architecture-coding-standards.md §2's immediate-mode implication). The
//! order is: ingest -> tracking -> planning -> policy and approval -> events ->
//! health -> journal.
//!
//! The policy-and-approval step (GAP-038) sits between planning and anything that
//! could act, which is the position contract C-01 requires: a plan becomes actionable
//! only through a recorded human decision. It runs on the frames where the plan
//! changes, because re-queuing an unchanged plan every frame would fill the queue with
//! the same decision.

use crate::state::AppState;
use gungnir_eventing::Event;
use gungnir_model::events::InterceptEvent;
use gungnir_model::SystemHealth;
use gungnir_store::EventJournal;

pub fn tick(state: &mut AppState) {
    let now = state.clock.now();

    // 1. Ingest: adapters -> validation/quarantine -> tracking service.
    for ingest_event in state.ingest.tick(now, state.tracking.as_mut()) {
        // GAP-008: every accepted detection is a clock-skew sample. Read here, off the
        // event, so the gateway is not asked to do anything it does not already do.
        if let gungnir_model::events::IngestEvent::Accepted(d) = &ingest_event {
            state
                .clock_skew
                .observe(d.sensor.0, d.source_time, d.receipt_time);
            // MOP-09: a source whose skew exceeds the late-data policy is flagged, once,
            // the moment the estimate says so; the health panel carries it from then on.
            let policy = state.clock.late_data_policy();
            if state.clock_skew.is_out_of_sync(d.sensor.0, policy)
                && state.skew_alerted.insert(d.sensor.0)
            {
                let skew = state.clock_skew.skew_s(d.sensor.0).unwrap_or(0.0);
                state.alerts.push(format!(
                    "sensor {} clock is out of sync: {skew:+.3} s against the late-data \
                     policy; its detections are being mis-timed",
                    d.sensor.0
                ));
            }
        }
        // GAP-021: the feed detectors read per-sensor statistics kept from these events.
        crate::anomaly::observe_ingest(&mut state.anomaly, &ingest_event, now);
        publish(state, now, Event::Ingest(ingest_event));
    }

    // 1c. The node link (GAP-050): silent past the heartbeat timeout, fall back to
    //     embedded services and say so; answering again, mark the reconciliation due.
    crate::session::sweep_expiry(state);
    crate::failover::tick(state);
    crate::node_tasks::sweep(state);

    // 2. Tracking: pull pipeline output into the snapshot.
    state.tracking.poll(now);
    // 2d. A bearing that matched no track is retained by the pipeline and shown, not
    //     dropped (DN-27 §5 rule 3): the tick after it first appears in
    //     `tracking.bearing_rays()` raises one alert, because it classifies without
    //     localising and DN-27 §7 places that case on the alert list rather than on the
    //     map (GAP-096).
    crate::bearings::tick(state);

    // 1a. What the radars said about themselves this frame (GAP-064), after the
    //     gateway has polled the adapters that queue it.
    crate::radar::observe_services(state);
    // 1a'. What the peers said this frame that was not a track (GAP-009, DN-16 §5): a
    //      launch warning raises an alert naming the peer and goes on the record, and
    //      creates no track, because a track we have not observed is one we cannot
    //      maintain.
    crate::peers::tick(state);
    crate::cooperative::tick(state);
    crate::adsb::tick(state);
    crate::sapient::tick(state);
    crate::identity::tick(state);

    // 1b. The terrain, if one is loading (GAP-023): polled here so a slow file never
    //     stalls a frame, and refused by name if its frame is not the picture's.
    crate::terrain::poll(state);

    // 1b'. The configured point-cloud pair, if one is loading (GAP-098): same
    //      off-frame loader pattern as terrain, on its own channel so the two never
    //      contend over one loader thread.
    crate::pointcloud::poll(state);

    // 1b''. Registration against that pair, once it is loaded (GAP-024): the caller
    //       `crate::fusion::FusionBackend::engine_for` was missing until GAP-098 gave
    //       it a real target to build against. Right after `poll` so a pair that
    //       completes loading this tick is registered the same tick, not one frame
    //       late; a no-op for an incomplete or absent pair (GAP-098's all-or-nothing
    //       rule).
    crate::pointcloud::register(state);

    // 2a. A seeded session (GAP-089): the mark once, then the plans and alerts the
    //     schedule brings due, through the same chain a live plan takes.
    crate::rehearsal::tick(state);

    // 2b. The warnings this frame's exposures oblige (GAP-042, DN-03), evaluated on
    //     every frame because an obligation triggers on the clock.
    crate::warnings::tick(state);

    // 2c. What the endpoints answered (GAP-040): delivered, refused, or unreachable and
    //     due again.
    crate::deliveries::sweep(state);

    // 3. Planning against the snapshot; publish only when the plan changes.
    //
    //    **A stale plan is not proposed** (GAP-066). `PlanOutcome` separates a plan
    //    computed for this snapshot from the last one that succeeded and from never
    //    having had one; publishing `PlanProposed` for a stale plan would put a
    //    recommendation in the journal that nothing recommended now.
    let outcome = state
        .intercept
        .plan(now, state.tracking.tracks(), &state.resources);
    // GAP-030: what the planner would not propose this tick, for PN-05.
    state.withheld = state.intercept.withheld();
    let plan = match &outcome {
        gungnir_intercept_service::PlanOutcome::Fresh(plan) => plan.clone(),
        not_fresh => {
            tracing::debug!(?not_fresh, "no fresh plan this tick");
            state.last_plan.clone()
        }
    };
    let mut submitted = None;
    // **Compared by id against `last_live_plan_id`, not by value against
    // `state.last_plan` (GAP-097).** `state.last_plan` is what PN-04/PN-05 draw, and
    // `rehearsal.rs`'s scripted plans write it too; comparing the live planner's own
    // output against a field something else also writes means an unrelated scripted
    // submission makes the very next unchanged live plan look new again.
    // `DpInterceptService::fresh_plan` never reuses a `PlanId` for a different
    // assignment, so the id alone -- tracked here and touched only by this step --
    // answers "have I already announced this one" without that interference.
    if outcome.is_fresh() && state.last_live_plan_id != Some(plan.id) {
        publish(
            state,
            now,
            Event::Intercept(InterceptEvent::PlanProposed(plan.clone())),
        );
        state.last_plan = plan.clone();
        state.last_live_plan_id = Some(plan.id);

        // 3b. Policy, then the approval queue. A denied plan is recorded as denied and
        //     never queued; a plan that clears waits for a person. Neither path makes
        //     anything actionable, which is the property C-01 turns on.
        let outcome = crate::decisions::submit(state, plan);
        tracing::debug!(?outcome, "plan submitted");
        submitted = Some(outcome);
    }

    // 3b'. The options beside the plan, and the rehearsal for the selected track
    //      (GAP-032). Called every frame so that pointing at a different track re-asks
    //      the question, and it costs a solve only when the plan or the selection moved.
    crate::decisions::refresh_support(state, submitted.as_ref());

    // 3c. Expiry and escalation, every frame rather than only when the plan changes:
    //     a window closes on the clock, not on new input. Nothing ends silently --
    //     every expiry leaves a record that is not actionable and names no operator.
    crate::decisions::sweep(state);

    // 3c'. Engagements (GAP-043, DN-06): the engaged track's lifecycle is the evidence,
    //      judged against the window that opened with the decision; a window that closes
    //      with nothing observed closes indeterminate, never as success or failure.
    crate::engagements::sweep(state);

    // 3d. The same argument for sensor commands: an acknowledgement window closes on
    //     the clock. Nothing acknowledges today, so a command against a configured
    //     endpoint always ends here -- unacknowledged, alerted, and never retried.
    crate::sustainment::sweep_sensor_tasks(state);

    // 3e. And collection requirements, whose needed-by times pass on the same clock. A
    //     lapse is not a decline: nobody refused it, and the record says which happened.
    crate::requirements::sweep(state);

    // 3f. The watch's rhythm (GAP-054): products the schedule brings due, and maintenance
    //     windows opening, closing, or closing with the sensor still down. On the clock
    //     for the same reason as the three above -- and on *mission* time, so a replayed
    //     session produces the same products at the same moments.
    crate::rhythm::tick(state);

    // 3h. The anomaly detectors (GAP-021, D-13): pure rules over this frame's picture and
    //     feeds, raising each finding once and naming what it cannot know.
    crate::anomaly::tick(state);

    // 3g. Once per session, journal which algorithm configuration the deployment opened
    //     with (GAP-086). Here rather than in `AppState::with_config`, because that
    //     function builds the bus it would have to publish on.
    if !state.governance_recorded {
        state.governance_recorded = true;
        crate::governance::publish_in_force(state);
    }

    // 4. Health is what the services report, never inferred. A change goes on the
    //    record (MOE-06): a decision taken after it was journaled was taken with the
    //    degraded state on the strip, which draws the flags every frame.
    state.health = SystemHealth {
        tracking_healthy: state.tracking.is_healthy(),
        intercept_healthy: state.intercept.is_healthy(),
        ingest_healthy: state.ingest.is_healthy(),
    };
    if state.health_journaled != Some(state.health) {
        state.health_journaled = Some(state.health);
        publish(
            state,
            now,
            Event::Health(gungnir_model::events::HealthEvent::Changed {
                tracking_healthy: state.health.tracking_healthy,
                intercept_healthy: state.health.intercept_healthy,
                ingest_healthy: state.health.ingest_healthy,
                at: now,
            }),
        );
    }

    // 5. Journal everything the bus carried this frame, then honour the D-04 fsync
    //    interval. `sync_if_due` is cheap when nothing is owed: one elapsed-time
    //    check.
    journal_pending(state);
    if let Err(err) = state.journal.sync_if_due() {
        if !state.journal_failed {
            state.journal_failed = true;
            tracing::error!(%err, "journal fsync failed; recent envelopes are not durable");
            state.alerts.push(format!("Journal fsync failed: {err}"));
        }
    }
}

/// Publish one event, logging a failure rather than losing it silently.
///
/// `pub(crate)` because `decisions::submit` publishes `PlanSuperseded` from outside this
/// module, and a second copy of the failure handling would be a second place it drifts.
pub fn publish(state: &mut AppState, now: gungnir_model::MissionTime, event: Event) {
    if let Err(err) = state.events.publish(now, event) {
        tracing::error!(%err, "event publish failed");
    }
}

/// Append everything the bus has carried since the last call.
///
/// Public because [`crate::state::AppState::save_session`] needs exactly this before it
/// syncs, and two copies of it would be two places the failure handling could drift.
pub fn journal_pending(state: &mut AppState) {
    let Some(session) = state.session() else {
        return;
    };
    let pending: Vec<_> = state.journal_rx.try_iter().collect();
    for envelope in pending {
        if let Err(err) = state.journal.append(session, &envelope) {
            if !state.journal_failed {
                state.journal_failed = true;
                tracing::error!(%err, "journal append failed; session will not be replayable");
                state.alerts.push(format!("Journal write failed: {err}"));
            }
        }
    }
}
