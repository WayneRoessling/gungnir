// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Engagements: from a decision to an outcome (GAP-043, DN-06), moved here with the rest
//! of the decision path (`docs/design/DN-31-node-approval-queue.md` §3 point 4; GAP-131,
//! D-57).
//!
//! `gungnir_intercept_service::engagement` owns the state machine. This is the half that
//! decides *when* it moves: **open** on an actionable decision, one per solution;
//! **observe** the engaged track's own lifecycle, which is the only evidence this build can
//! see and is labelled as the weak evidence it is; **close** `Indeterminate` when the
//! window passes with nothing observed, because "we do not know" is a result and a false
//! success or failure is not (DN-06 §5).
//!
//! What this does not do, and says so: effector reports (`Executing`, corroborated
//! outcomes) arrive through [`ApprovalDesk::apply_report`] (GAP-040,
//! DN-07); a person aborting with a reason wants a control the panels do not have; fires
//! tasks (DN-05) open nothing here, and the decision is alerted rather than silently
//! unassessed.

use crate::{ApprovalContext, ApprovalDesk, ApprovalHost};
use gungnir_command::DecisionRecord;
use gungnir_eventing::Event;
use gungnir_intercept_service::engagement::{EffectEvidence, EffectSource, Engagement};
use gungnir_model::events::{engagement_outcome as outcome, EngagementEvent};
use gungnir_model::{DecisionId, MissionTime, PlanId, TrackId};
use std::collections::HashSet;

impl ApprovalDesk {
    /// Open the engagements an actionable decision implies: one per solution (DN-06 §8,
    /// "every accepted decision opens exactly one engagement" -- per solution, since a plan
    /// with two assignments is two things the effectors were asked to do).
    ///
    /// Returns how many opened. **Fewer than the solutions is alerted, never silent**: a
    /// layer with no `assessment.effect_window_s` has no moment at which an effect was
    /// expected, and an engagement with a guessed window would close on a guess.
    pub fn open_for(
        &mut self,
        cx: &ApprovalContext<'_>,
        host: &mut dyn ApprovalHost,
        record: &DecisionRecord,
    ) -> usize {
        if !record.is_actionable() {
            return 0;
        }
        let now = record.mission_time;
        if record.plan.fires().is_some() {
            host.alert(format!(
                "decision {} accepted a fires task; effect assessment for fires is not built \
                 (DN-05), so no engagement was opened",
                record.id.short()
            ));
            return 0;
        }
        // GAP-040: the handoff is issued from the same record, at the same moment.
        self.issue_for(cx, host, record);
        let mut opened = 0;
        for solution in record.plan.solutions() {
            let Some(layer) = cx
                .resources
                .iter()
                .find(|r| r.id == solution.resource)
                .map(|r| r.layer)
            else {
                host.alert(format!(
                    "decision {} tasks resource {}, which the baseline does not list; no \
                     engagement opened",
                    record.id.short(),
                    solution.resource.0
                ));
                continue;
            };
            let Some(window_s) = cx.config.assessment.effect_window_s.get(&layer).copied() else {
                host.alert(format!(
                    "decision {} against track {}: no assessment.effect_window_s is configured \
                     for the {layer:?} layer, so there is no moment an effect is expected by; \
                     no engagement opened",
                    record.id.short(),
                    solution.track.0
                ));
                continue;
            };
            self.engagements.push(Engagement::open(
                record.id,
                record.plan.id,
                solution.track,
                solution.resource,
                now,
                window_s,
            ));
            opened += 1;
            host.publish(
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
    pub fn observe_superseded(
        &mut self,
        host: &mut dyn ApprovalHost,
        plan: PlanId,
        now: MissionTime,
    ) {
        let mut closed = Vec::new();
        for e in self
            .engagements
            .iter_mut()
            .filter(|e| e.plan == plan && e.state.is_open())
        {
            if e.abort("the plan was superseded", now).is_ok() {
                closed.push(e.decision);
            }
        }
        for decision in closed {
            publish_closed(host, decision, outcome::ABORTED, now);
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
    /// "Leaves" needs "was there": a plan against a track the picture never showed after
    /// the decision cannot be judged by its absence, and closes indeterminate.
    pub fn sweep_engagements(&mut self, cx: &ApprovalContext<'_>, host: &mut dyn ApprovalHost) {
        let now = cx.now;
        let present: HashSet<TrackId> = cx.tracks.iter().map(|t| t.id).collect();
        let mut closed = Vec::new();

        for e in self.engagements.iter_mut().filter(|e| e.state.is_open()) {
            let seen = self.engaged_seen.contains(&e.decision);
            let here = present.contains(&e.track);
            if here {
                self.engaged_seen.insert(e.decision);
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
                            e.track.0,
                            e.decision.short()
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
                            e.track.0,
                            e.decision.short()
                        )),
                    ));
                }
            }
        }

        for (decision, label, alert) in closed {
            publish_closed(host, decision, label, now);
            // DN-06 §7: an ineffective or indeterminate close is an interruption; a success
            // is not.
            if let Some(alert) = alert {
                host.alert(alert);
            }
        }
    }
}

/// The one place an engagement's close goes on the record: the sweep's inferred closes,
/// the abort, and since GAP-135 the close an effector reports
/// (`handoffs::ApprovalDesk::apply_report`).
pub(crate) fn publish_closed(
    host: &mut dyn ApprovalHost,
    decision: DecisionId,
    label: &str,
    at: MissionTime,
) {
    host.publish(
        at,
        Event::Engagement(EngagementEvent::Closed {
            decision,
            outcome: label.to_string(),
            at,
        }),
    );
}
