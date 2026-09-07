//! Engagements: from a decision to an outcome (GAP-043, DN-06).
//!
//! `gungnir_intercept_service::engagement` owns the state machine and never had a caller.
//! This is the desktop's side of it: **open** on an actionable decision, one per solution;
//! **observe** the engaged track's own lifecycle, which is the only evidence this build can
//! see and is labelled as the weak evidence it is; **close** `Indeterminate` when the
//! window passes with nothing observed, because "we do not know" is a result and a false
//! success or failure is not (DN-06 §5).
//!
//! What this does not do, and says so: effector reports (`Executing`, corroborated
//! outcomes) wait on the handoff channel (GAP-040, DN-07); a person aborting with a reason
//! wants a PN-05 control that is not built; fires tasks (DN-05) open nothing here, and the
//! decision is alerted rather than silently unassessed.

use crate::state::AppState;
use crate::update::publish;
use gungnir_command::DecisionRecord;
use gungnir_eventing::Event;
use gungnir_intercept_service::engagement::{
    EffectEvidence, EffectSource, EffectTally, Engagement, EngagementState,
};
use gungnir_model::events::{engagement_outcome as outcome, EngagementEvent};
use gungnir_model::{MissionTime, PlanId, TrackId};
use gungnir_ui::panels::commander_summary::OutcomeCounts;
use std::collections::HashSet;

/// The outcome as the bus spells it (DN-06 §6: a string, because the model may not
/// depend on the facade's enum).
#[must_use]
pub fn outcome_label(state: &EngagementState) -> Option<&'static str> {
    Some(match state {
        EngagementState::Committed | EngagementState::Executing => return None,
        EngagementState::Effective { evidence } if evidence.source.is_corroborated() => {
            outcome::EFFECTIVE_CORROBORATED
        }
        EngagementState::Effective { .. } => outcome::EFFECTIVE_TRACK_INFERRED,
        EngagementState::Ineffective { evidence } if evidence.source.is_corroborated() => {
            outcome::INEFFECTIVE_CORROBORATED
        }
        EngagementState::Ineffective { .. } => outcome::INEFFECTIVE_TRACK_INFERRED,
        EngagementState::Aborted { .. } => outcome::ABORTED,
        EngagementState::Indeterminate { .. } => outcome::INDETERMINATE,
    })
}

/// Open the engagements an actionable decision implies: one per solution (DN-06 §8,
/// "every accepted decision opens exactly one engagement" -- per solution, since a plan
/// with two assignments is two things the effectors were asked to do).
///
/// Returns how many opened. **Fewer than the solutions is alerted, never silent**: a
/// layer with no `assessment.effect_window_s` has no moment at which an effect was
/// expected, and an engagement with a guessed window would close on a guess.
pub fn open_for(state: &mut AppState, record: &DecisionRecord) -> usize {
    if !record.is_actionable() {
        return 0;
    }
    let now = record.mission_time;
    if record.plan.fires().is_some() {
        state.alerts.push(format!(
            "decision {} accepted a fires task; effect assessment for fires is not built \
             (DN-05), so no engagement was opened",
            record.id.0
        ));
        return 0;
    }
    // GAP-040: the handoff is issued from the same record, at the same moment.
    crate::handoffs::issue_for(state, record);
    let mut opened = 0;
    for solution in record.plan.solutions() {
        let Some(layer) = state
            .resources
            .iter()
            .find(|r| r.id == solution.resource)
            .map(|r| r.layer)
        else {
            state.alerts.push(format!(
                "decision {} tasks resource {}, which the baseline does not list; no \
                 engagement opened",
                record.id.0, solution.resource.0
            ));
            continue;
        };
        let Some(window_s) = state.config.assessment.effect_window_s.get(&layer).copied() else {
            state.alerts.push(format!(
                "decision {} against track {}: no assessment.effect_window_s is configured \
                 for the {layer:?} layer, so there is no moment an effect is expected by; \
                 no engagement opened",
                record.id.0, solution.track.0
            ));
            continue;
        };
        state.engagements.push(Engagement::open(
            record.id,
            record.plan.id,
            solution.track,
            solution.resource,
            now,
            window_s,
        ));
        opened += 1;
        publish(
            state,
            now,
            Event::Engagement(EngagementEvent::Opened {
                decision: record.id,
                plan: record.plan.id,
                track: solution.track,
            }),
        );
    }
    opened
}

/// The plan was superseded (DN-08 §5): its open engagements are abandoned before an
/// effect could be judged, which is what `Aborted` means (DN-06 §5).
///
/// Only that plan's. A *new* proposal is not a supersession of an accepted plan -- the
/// planner re-solves every tick, and treating each re-solve as an abort would leave
/// nothing ever assessed.
pub fn observe_superseded(state: &mut AppState, plan: PlanId, now: MissionTime) {
    let mut closed = Vec::new();
    for e in state
        .engagements
        .iter_mut()
        .filter(|e| e.plan == plan && e.state.is_open())
    {
        if e.abort("the plan was superseded", now).is_ok() {
            closed.push(e.decision);
        }
    }
    for decision in closed {
        publish_closed(state, decision, outcome::ABORTED, now);
    }
}

/// Every frame, after the picture has been pulled (DN-06 §5, the transition table):
///
/// - the engaged track **leaves the picture inside the window** -> `Effective`, on
///   track-lifecycle evidence, which the record says may be destruction, masking, or
///   the tracker dropping it;
/// - the track **is still there when the window closes** -> `Ineffective`, same source;
/// - the window closes and neither was observed -> `Indeterminate`.
///
/// "Leaves" needs "was there": a plan against a track the picture never showed after the
/// decision cannot be judged by its absence, and closes indeterminate.
pub fn sweep(state: &mut AppState) {
    let now = state.clock.now();
    let present: HashSet<TrackId> = state.tracking.tracks().iter().map(|t| t.id).collect();
    let mut closed = Vec::new();

    for e in state.engagements.iter_mut().filter(|e| e.state.is_open()) {
        let seen = state.engaged_seen.contains(&e.decision);
        let here = present.contains(&e.track);
        if here {
            state.engaged_seen.insert(e.decision);
        }
        let evidence = |detail: &str| EffectEvidence {
            source: EffectSource::TrackLifecycle,
            observed_at: now,
            detail: detail.to_string(),
        };
        if !e.window_has_closed(now) {
            if seen && !here {
                let ok = e
                    .close_effective(evidence(
                        "the engaged track left the picture inside the effect window; \
                         destroyed, masked, or dropped by the tracker -- the evidence does \
                         not say which",
                    ))
                    .is_ok();
                if ok {
                    closed.push((e.decision, outcome::EFFECTIVE_TRACK_INFERRED, None));
                }
            }
            continue;
        }
        if here {
            let ok = e
                .close_ineffective(evidence(
                    "the engaged track persisted past the effect window",
                ))
                .is_ok();
            if ok {
                closed.push((
                    e.decision,
                    outcome::INEFFECTIVE_TRACK_INFERRED,
                    Some(format!(
                        "engagement of track {} (decision {}) ineffective: the track is still \
                         in the picture after the effect window",
                        e.track.0, e.decision.0
                    )),
                ));
            }
        } else {
            let reason = if seen {
                "the track left the picture, but not observably inside the effect window"
            } else {
                "the effect window closed and the picture never showed the engaged track"
            };
            if e.close_indeterminate(reason, now).is_ok() {
                closed.push((
                    e.decision,
                    outcome::INDETERMINATE,
                    Some(format!(
                        "engagement of track {} (decision {}) indeterminate: {reason}",
                        e.track.0, e.decision.0
                    )),
                ));
            }
        }
    }

    for (decision, label, alert) in closed {
        publish_closed(state, decision, label, now);
        // DN-06 §7: an ineffective or indeterminate close is an interruption; a success
        // is not.
        if let Some(alert) = alert {
            state.alerts.push(alert);
        }
    }
}

fn publish_closed(
    state: &mut AppState,
    decision: gungnir_model::DecisionId,
    label: &str,
    at: MissionTime,
) {
    publish(
        state,
        at,
        Event::Engagement(EngagementEvent::Closed {
            decision,
            outcome: label.to_string(),
            at,
        }),
    );
}

/// The facade's tally, as PN-17 takes it.
#[must_use]
pub fn outcome_counts(state: &AppState) -> OutcomeCounts {
    let t = EffectTally::of(&state.engagements);
    OutcomeCounts {
        open: t.open,
        effective_corroborated: t.effective_corroborated,
        effective_track_inferred: t.effective_track_inferred,
        ineffective_corroborated: t.ineffective_corroborated,
        ineffective_track_inferred: t.ineffective_track_inferred,
        indeterminate: t.indeterminate,
        aborted: t.aborted,
    }
}
