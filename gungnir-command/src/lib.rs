// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Human approval workflow, per docs/gungnir-capabilities.md §5.4. Consumes
//! `gungnir_policy::PolicyVerdict` and exposes the accept/override/reject actions
//! gungnir-ui's Intercept Planning Panel calls into. Every decision is a
//! [`DecisionRecord`], which the audit log in `gungnir-security` and the event bus
//! (as `gungnir_model::events::CommandEvent`) both consume
//! (docs/gungnir-capabilities.md §5.6, operator-action audit trail).
//!
//! Invariants: a plan the policy denied can never enter the queue; no plan is
//! actionable without a record; records are append-only.
//!
//! The queue is timed as of GAP-034 and GAP-035 (**signed by the owner on
//! 2026-09-05**): [`InMemoryApprovalWorkflow`] holds [`PendingApproval`]s carrying the
//! deadlines [`queue::deadlines`] computes from the baseline, [`ApprovalWorkflow::sweep`]
//! applies expiry and escalation, and the desktop tick calls it every frame.
//!
//! `DecisionRecord::to_event` carries the verdict and the rationale as of GAP-047's
//! resolution (**signed by the owner on 2026-09-06**), so MOE-05 can be read from the
//! journal alone.
//!
//! # A distinction the record cannot yet make
//!
//! An expiry is recorded as `Rejected` with `operator_id: None`, and
//! `docs/design/DN-10-queue-expiry-and-escalation.md` §3 intends that `None` to be what
//! tells an expiry from a rejection. It will be, once a decided record carries an
//! operator id; none does, because there is no operator session (GAP-057), so a human
//! rejection is `None` too and the two are **not** distinguishable here today.
//!
//! What *is* distinguishable, and is the durable answer, is the event: an expiry
//! publishes `CommandEvent::Expired` and a decision publishes `CommandEvent::Decided`,
//! and it is the event stream rather than this in-memory list that reaches the journal.
//! `ARCHITECTURE.md` §10 item 45 carries the open question about the record model.

pub mod queue;

pub use queue::{
    deadlines, due, expiry_record, governing_layer, next_role, order_queue, PendingApproval,
    QueueOutcome,
};

use gungnir_model::events::CommandEvent;
use gungnir_model::{DecisionId, DecisionSettings, EffectorLayer, MissionTime, PlanId, PlanView};
use gungnir_policy::{DenialReason, PolicyVerdict};

/// What ended a pending approval.
///
/// This is `docs/design/DN-10-queue-expiry-and-escalation.md` §3's type, conformed to
/// on 2026-09-05 and **signed by the owner** the same day with amendment 1 (§9). Two things had drifted from the signed note and both mattered:
///
/// - `Rejected` carried no reason, so PN-07 collected one from the operator and threw
///   it away. MOE-01 needs a considered rejection to be distinguishable from an
///   abandoned decision, and the reason is the only thing that distinguishes them.
/// - There was no `Expired`, so an expiry was recorded as a rejection with no operator
///   and could only be told from a real rejection by the *absence* of an operator id --
///   which works once there is an operator session to supply one, and there is not
///   (GAP-057). The distinction is now in the type and does not wait on anything.
///
/// `Escalated` appears in DN-10 §3's list and deliberately does **not** appear here: an
/// escalated item has not ended, it is offered to one more role and stays in the queue.
/// A `DecisionRecord` for it would put an entry in the append-only history for something
/// that has not happened yet. Escalation is a `CommandEvent::Escalated` on the bus, which
/// is where a non-terminal fact belongs. Recorded as an amendment in the note.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum OperatorDecision {
    Accepted,
    /// The operator substituted their own assignment; the plan stored in the record
    /// is the one they acted on.
    Overridden,
    /// A person considered this and declined it, and said why.
    Rejected {
        reason: String,
    },
    /// The window closed with nobody deciding. **Not a rejection**: nobody chose, and
    /// an after-action review has to be able to tell the two apart.
    Expired {
        at: MissionTime,
    },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DecisionRecord {
    /// Minted here, and the key everything else refers to. A service facade may
    /// not depend on this crate, so engagement state keys on this identifier
    /// rather than on the record (docs/design/DN-06-engagement-and-effect.md §2).
    pub id: DecisionId,
    pub plan: PlanView,
    pub verdict: PolicyVerdict,
    pub decision: OperatorDecision,
    pub operator_id: Option<String>,
    pub mission_time: MissionTime,
}

impl DecisionRecord {
    pub fn is_actionable(&self) -> bool {
        matches!(
            self.decision,
            OperatorDecision::Accepted | OperatorDecision::Overridden
        )
    }

    /// Whether the window closed with nobody deciding.
    ///
    /// The question MOE-01 asks of the history, answerable from the record itself since
    /// the DN-10 §3 conformance rather than by inspecting whether an operator happens to
    /// be named.
    pub fn is_expiry(&self) -> bool {
        matches!(self.decision, OperatorDecision::Expired { .. })
    }

    /// The event this record publishes.
    ///
    /// An expiry publishes `Expired`, not `Decided { accepted: false }`. Deriving the
    /// event from the record rather than building it beside the record is what stops the
    /// two disagreeing: there is no path that writes an expiry to the history and a
    /// rejection to the bus.
    pub fn to_event(&self) -> CommandEvent {
        match &self.decision {
            OperatorDecision::Expired { at } => CommandEvent::Expired {
                plan: self.plan.id,
                at: *at,
            },
            OperatorDecision::Accepted
            | OperatorDecision::Overridden
            | OperatorDecision::Rejected { .. } => CommandEvent::Decided {
                plan: self.plan.id,
                decision: self.id,
                accepted: self.is_actionable(),
                operator: self.operator_id.clone(),
                // MOE-05 reads both from the journal (signed by the owner 2026-09-06).
                // The denial reason goes over in its debug spelling: the model may not
                // depend on `DenialReason`, and the spelling is stable per variant.
                verdict: self.verdict.summary(),
                rationale: match &self.decision {
                    OperatorDecision::Rejected { reason } => Some(reason.clone()),
                    _ => None,
                },
            },
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct PendingApprovalId(pub u64);

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("no pending approval with id {0:?}")]
    NotFound(PendingApprovalId),
    #[error("plan was denied by policy ({0:?}) and cannot be submitted for approval")]
    DeniedByPolicy(DenialReason),
}

/// Everything a plan needs to become a *timed* decision.
///
/// The plan and the verdict were enough while the queue was untimed. They are not
/// enough for a queue that expires and escalates: the deadline comes from when the item
/// was submitted and which layer it engages, and the ordering needs the priority. There
/// is deliberately no `Default` -- a submission with a made-up submission time would get
/// a made-up deadline, and the operator would read a countdown that meant nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct Submission {
    pub plan: PlanView,
    pub verdict: PolicyVerdict,
    pub submitted: MissionTime,
    /// The layer whose window governs this plan; see [`queue::governing_layer`].
    pub layer: EffectorLayer,
    /// Highest risk score among the plan's tracks, for the ordering tie-break. Zero
    /// when no assessment has run, in which case ordering is by time remaining alone
    /// and the panel says so rather than implying a priority order it does not have.
    pub priority: f32,
    /// The role this item is offered to first.
    pub role: String,
}

pub trait ApprovalWorkflow: Send + Sync {
    /// Queue a plan for a human decision. Refused if the verdict is a denial.
    fn submit_for_approval(
        &mut self,
        submission: Submission,
    ) -> Result<PendingApprovalId, CommandError>;

    /// The queue, ordered: time remaining ascending, then priority descending.
    ///
    /// Ordering is applied on every mutation rather than on read, which is exact
    /// rather than an approximation: time remaining is `expires_at - now`, and
    /// subtracting the same `now` from every item preserves their order, so the
    /// sequence only changes when an item is added or removed.
    fn queue(&self) -> &[PendingApproval];

    /// Apply expiry and escalation at `now`, returning what happened to each item.
    ///
    /// `ladder` is the escalation order, lowest authority first. Nothing ends
    /// silently: every expiry leaves a `DecisionRecord` in [`ApprovalWorkflow::records`]
    /// and every outcome is returned so the caller can publish it.
    ///
    /// **No configuration makes an expiry accept.** The record an expiry leaves is built
    /// by [`queue::expiry_record`], which is not actionable and names no operator, and
    /// `queue`'s own exhaustive search over the settings space guards that.
    fn sweep(
        &mut self,
        now: MissionTime,
        ladder: &[&str],
    ) -> Vec<(PlanId, PendingApprovalId, QueueOutcome)>;

    /// Record the operator's decision; the plan leaves the queue.
    fn decide(
        &mut self,
        id: PendingApprovalId,
        decision: OperatorDecision,
        operator_id: Option<String>,
        now: MissionTime,
    ) -> Result<DecisionRecord, CommandError>;

    /// Append-only history, oldest first.
    fn records(&self) -> &[DecisionRecord];

    /// The plans waiting, for callers that want only those.
    fn pending(&self) -> Vec<(PendingApprovalId, &PlanView)> {
        self.queue().iter().map(|p| (p.id, &p.plan)).collect()
    }
}

#[derive(Debug, Default)]
pub struct InMemoryApprovalWorkflow {
    next_id: u64,
    next_decision: u64,
    queue: Vec<PendingApproval>,
    records: Vec<DecisionRecord>,
    /// The deadlines of the baseline in force. Held rather than passed per call so a
    /// queue cannot end up with items timed against two different baselines; applying a
    /// new baseline replaces the workflow.
    settings: DecisionSettings,
}

impl InMemoryApprovalWorkflow {
    /// A workflow with no deadlines configured, which means nothing ever expires.
    ///
    /// That is DN-08's default read the way DN-10 requires: silence about expiry
    /// preserves, because silently discarding a decision nobody took loses information.
    pub fn new() -> Self {
        Self::default()
    }

    /// A workflow timed by this deployment's decision settings.
    pub fn with_settings(settings: DecisionSettings) -> Self {
        Self {
            settings,
            ..Self::default()
        }
    }

    fn mint_decision_id(&mut self) -> DecisionId {
        self.next_decision += 1;
        DecisionId(self.next_decision)
    }
}

impl ApprovalWorkflow for InMemoryApprovalWorkflow {
    fn submit_for_approval(
        &mut self,
        submission: Submission,
    ) -> Result<PendingApprovalId, CommandError> {
        if let PolicyVerdict::Denied { reason_code } = submission.verdict {
            return Err(CommandError::DeniedByPolicy(reason_code));
        }
        self.next_id += 1;
        let id = PendingApprovalId(self.next_id);
        let (expires_at, escalate_at) =
            queue::deadlines(&self.settings, submission.layer, submission.submitted);
        self.queue.push(PendingApproval {
            id,
            plan: submission.plan,
            verdict: submission.verdict,
            submitted: submission.submitted,
            layer: submission.layer,
            expires_at,
            escalate_at,
            escalated_from: None,
            offered_to: vec![submission.role],
            priority: submission.priority,
        });
        queue::order_queue(&mut self.queue, submission.submitted);
        Ok(id)
    }

    fn queue(&self) -> &[PendingApproval] {
        &self.queue
    }

    fn sweep(
        &mut self,
        now: MissionTime,
        ladder: &[&str],
    ) -> Vec<(PlanId, PendingApprovalId, QueueOutcome)> {
        let (expired, to_escalate) = queue::due(&self.queue, now);
        let mut outcomes = Vec::new();

        for id in expired {
            let Some(index) = self.queue.iter().position(|p| p.id == id) else {
                continue;
            };
            let decision_id = self.mint_decision_id();
            let item = self.queue.remove(index);
            let record = queue::expiry_record(&item, decision_id, now);
            debug_assert!(
                !record.is_actionable() && record.operator_id.is_none(),
                "an expiry became actionable or named an operator"
            );
            self.records.push(record);
            outcomes.push((item.plan.id, id, QueueOutcome::Expired { at: now }));
        }

        for id in to_escalate {
            let Some(item) = self.queue.iter_mut().find(|p| p.id == id) else {
                continue;
            };
            let Some(current) = item.current_role().map(str::to_owned) else {
                continue;
            };
            match queue::next_role(ladder, &current) {
                Some(next) => {
                    item.escalated_from = Some(current);
                    item.offered_to.push(next.to_owned());
                    // The clock restarts, which is what bounds escalation at once per
                    // rank step (DN-10 §5).
                    item.escalate_at = self
                        .settings
                        .escalate_after_for(item.layer)
                        .map(|s| MissionTime(now.0 + s));
                    outcomes.push((
                        item.plan.id,
                        id,
                        QueueOutcome::Escalated {
                            to_role: next.to_owned(),
                            at: now,
                        },
                    ));
                }
                // Already at the top of the ladder. Stop asking rather than reporting
                // an escalation every frame for an item that cannot go higher.
                None => item.escalate_at = None,
            }
        }

        if !outcomes.is_empty() {
            queue::order_queue(&mut self.queue, now);
        }
        outcomes
    }

    fn decide(
        &mut self,
        id: PendingApprovalId,
        decision: OperatorDecision,
        operator_id: Option<String>,
        now: MissionTime,
    ) -> Result<DecisionRecord, CommandError> {
        let index = self
            .queue
            .iter()
            .position(|p| p.id == id)
            .ok_or(CommandError::NotFound(id))?;
        let decision_id = self.mint_decision_id();
        let item = self.queue.remove(index);
        let record = DecisionRecord {
            id: decision_id,
            plan: item.plan,
            verdict: item.verdict,
            decision,
            operator_id,
            mission_time: now,
        };
        self.records.push(record.clone());
        Ok(record)
    }

    fn records(&self) -> &[DecisionRecord] {
        &self.records
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::PlanId;

    fn plan(id: u64) -> PlanView {
        PlanView {
            id: PlanId(id),
            ..PlanView::default()
        }
    }

    /// A submission at t=0 on the Point layer, offered to the operator.
    fn submission(id: u64, verdict: PolicyVerdict) -> Submission {
        Submission {
            plan: plan(id),
            verdict,
            submitted: MissionTime(0.0),
            layer: EffectorLayer::Point,
            priority: 0.0,
            role: "operator".to_owned(),
        }
    }

    /// A workflow that expires Point items after 30 s and escalates them after 20 s.
    fn timed() -> InMemoryApprovalWorkflow {
        let mut settings = DecisionSettings::default();
        settings.expiry_s.insert(EffectorLayer::Point, 30.0);
        settings.escalate_after_s.insert(EffectorLayer::Point, 20.0);
        InMemoryApprovalWorkflow::with_settings(settings)
    }

    const LADDER: [&str; 3] = ["operator", "supervisor", "commander"];

    #[test]
    fn denied_plan_cannot_be_submitted() {
        let mut wf = InMemoryApprovalWorkflow::new();
        let err = wf
            .submit_for_approval(submission(
                1,
                PolicyVerdict::Denied {
                    reason_code: DenialReason::NoGoGeofence,
                },
            ))
            .unwrap_err();
        assert!(matches!(
            err,
            CommandError::DeniedByPolicy(DenialReason::NoGoGeofence)
        ));
        assert!(wf.pending().is_empty());
    }

    #[test]
    fn every_decision_is_recorded_and_leaves_the_queue() {
        let mut wf = InMemoryApprovalWorkflow::new();
        let id = wf
            .submit_for_approval(submission(1, PolicyVerdict::RequiresHumanApproval))
            .expect("submit");
        assert_eq!(wf.pending().len(), 1);
        let record = wf
            .decide(
                id,
                OperatorDecision::Accepted,
                Some("op-1".into()),
                MissionTime(5.0),
            )
            .expect("decide");
        assert!(record.is_actionable());
        assert!(wf.pending().is_empty());
        assert_eq!(wf.records().len(), 1);
        assert!(matches!(
            record.to_event(),
            CommandEvent::Decided {
                plan: PlanId(1),
                decision,
                accepted: true,
                operator: Some(op),
                rationale: None,
                ..
            } if decision == record.id && op == "op-1"
        ));
        assert_eq!(
            record.id,
            DecisionId(1),
            "the workflow mints the identifier"
        );
    }

    #[test]
    fn every_decision_gets_its_own_identifier() {
        // Engagement state keys on this, so two decisions must never share one.
        let mut wf = InMemoryApprovalWorkflow::new();
        let mut ids = Vec::new();
        for n in 1..=3 {
            let id = wf
                .submit_for_approval(submission(n, PolicyVerdict::RequiresHumanApproval))
                .expect("submit");
            let record = wf
                .decide(
                    id,
                    OperatorDecision::Accepted,
                    Some("op".into()),
                    MissionTime(0.0),
                )
                .expect("decide");
            ids.push(record.id);
        }
        let mut unique = ids.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), ids.len(), "identifiers are unique");
    }

    #[test]
    fn unknown_id_is_an_error() {
        let mut wf = InMemoryApprovalWorkflow::new();
        assert!(matches!(
            wf.decide(
                PendingApprovalId(99),
                OperatorDecision::Rejected {
                    reason: "test".into()
                },
                None,
                MissionTime(0.0)
            ),
            Err(CommandError::NotFound(_))
        ));
    }

    /// The point of GAP-034: an item nobody decides leaves the queue with a record
    /// that is not a rejection and names nobody. Before this, `queue.rs` could compute
    /// that outcome and nothing applied it, so a decision nobody took simply sat there.
    #[test]
    fn an_item_nobody_decides_expires_with_a_record() {
        let mut wf = timed();
        let id = wf
            .submit_for_approval(submission(1, PolicyVerdict::RequiresHumanApproval))
            .expect("submit");
        assert_eq!(wf.queue().len(), 1);
        assert_eq!(
            wf.queue()[0].time_remaining_s(MissionTime(10.0)),
            Some(20.0),
            "the countdown PN-06 shows comes from the deadline, not from a guess"
        );

        // Before the window closes, nothing happens.
        assert!(wf.sweep(MissionTime(10.0), &LADDER).is_empty() || wf.queue().len() == 1);

        let outcomes = wf.sweep(MissionTime(31.0), &LADDER);
        let expiries: Vec<_> = outcomes
            .iter()
            .filter(|(_, _, o)| matches!(o, QueueOutcome::Expired { .. }))
            .collect();
        assert_eq!(expiries.len(), 1);
        assert_eq!(expiries[0].1, id);
        assert!(wf.queue().is_empty(), "an expired item stays in the queue");

        let record = wf.records().last().expect("an expiry leaves a record");
        assert!(
            !record.is_actionable(),
            "an expiry became actionable: this is contract C-01"
        );
        assert!(
            record.operator_id.is_none(),
            "nobody decided; a false operator is worse than a null"
        );
    }

    /// Escalation offers the item upward without taking it from the role that had it.
    #[test]
    fn escalation_adds_the_higher_role_and_is_bounded() {
        // A long window, so the ladder can be walked to the top before anything
        // expires. `timed()`'s 30 s expiry would remove the item at the second step,
        // which would test expiry rather than the escalation bound.
        let mut settings = DecisionSettings::default();
        settings.expiry_s.insert(EffectorLayer::Point, 3_600.0);
        settings.escalate_after_s.insert(EffectorLayer::Point, 20.0);
        let mut wf = InMemoryApprovalWorkflow::with_settings(settings);
        wf.submit_for_approval(submission(1, PolicyVerdict::RequiresHumanApproval))
            .expect("submit");

        let outcomes = wf.sweep(MissionTime(21.0), &LADDER);
        assert!(outcomes.iter().any(
            |(_, _, o)| matches!(o, QueueOutcome::Escalated { to_role, .. }
                                      if to_role == "supervisor")
        ));
        let item = &wf.queue()[0];
        assert!(
            item.may_be_decided_by("operator"),
            "the operator lost an item they were deciding"
        );
        assert!(item.may_be_decided_by("supervisor"));

        // Not again at the same instant: the clock was reset by the step just taken.
        assert!(wf
            .sweep(MissionTime(21.0), &LADDER)
            .iter()
            .all(|(_, _, o)| !matches!(o, QueueOutcome::Escalated { .. })));

        // The next step is due when its own clock runs out, and it stops at the top.
        assert!(wf.sweep(MissionTime(41.0), &LADDER).iter().any(
            |(_, _, o)| matches!(o, QueueOutcome::Escalated { to_role, .. }
                                      if to_role == "commander")
        ));
        assert!(wf.queue()[0].may_be_decided_by("commander"));
        for t in [61.0, 81.0, 101.0] {
            assert!(
                wf.sweep(MissionTime(t), &LADDER)
                    .iter()
                    .all(|(_, _, o)| !matches!(o, QueueOutcome::Escalated { .. })),
                "escalation looped past the top of the ladder at t={t}"
            );
        }
    }

    /// An expired item is not also escalated, and expiry wins: an item that is gone is
    /// not offered upward.
    #[test]
    fn an_expired_item_is_not_offered_upward() {
        let mut wf = timed();
        wf.submit_for_approval(submission(1, PolicyVerdict::RequiresHumanApproval))
            .expect("submit");
        let outcomes = wf.sweep(MissionTime(100.0), &LADDER);
        assert_eq!(outcomes.len(), 1);
        assert!(matches!(outcomes[0].2, QueueOutcome::Expired { .. }));
    }

    /// GAP-035: the queue an operator sees is ordered, and time pressure outranks
    /// severity, because a high-priority item with two minutes left can wait behind a
    /// lower-priority one with ten seconds left and the reverse loses both.
    #[test]
    fn the_queue_is_ordered_by_time_then_priority() {
        let mut settings = DecisionSettings::default();
        settings.expiry_s.insert(EffectorLayer::Area, 300.0);
        settings.expiry_s.insert(EffectorLayer::Point, 30.0);
        let mut wf = InMemoryApprovalWorkflow::with_settings(settings);

        let mut area = submission(1, PolicyVerdict::RequiresHumanApproval);
        area.layer = EffectorLayer::Area;
        area.priority = 0.9;
        wf.submit_for_approval(area).expect("submit");

        let mut point = submission(2, PolicyVerdict::RequiresHumanApproval);
        point.priority = 0.1;
        wf.submit_for_approval(point).expect("submit");

        assert_eq!(
            wf.queue().iter().map(|p| p.plan.id.0).collect::<Vec<_>>(),
            vec![2, 1],
            "thirty seconds left must come before five minutes, whatever the priority"
        );

        // No expiry anywhere: nothing ever leaves, which is the safe reading.
        let mut untimed = InMemoryApprovalWorkflow::new();
        untimed
            .submit_for_approval(submission(3, PolicyVerdict::RequiresHumanApproval))
            .expect("submit");
        assert!(untimed.queue()[0].expires_at.is_none());
        assert!(untimed.queue()[0].no_expiry_reason().is_some());
        assert!(untimed.sweep(MissionTime(1_000_000.0), &LADDER).is_empty());
        assert_eq!(untimed.queue().len(), 1, "silence about expiry preserves");
    }

    /// The property DN-10 §3 exists for, and the one the implementation had lost: an
    /// expiry and a rejection are different records, **without** relying on an operator
    /// id to tell them apart. Both carry `operator_id: None` here, which is what every
    /// decision looks like until there is an operator session (GAP-057), and they are
    /// still distinguishable.
    #[test]
    fn an_expiry_is_distinguishable_from_a_rejection_with_no_operator() {
        let mut wf = timed();
        let id = wf
            .submit_for_approval(submission(1, PolicyVerdict::RequiresHumanApproval))
            .expect("submit");
        let rejection = wf
            .decide(
                id,
                OperatorDecision::Rejected {
                    reason: "friendly airliner".into(),
                },
                None,
                MissionTime(5.0),
            )
            .expect("decide");

        let id2 = wf
            .submit_for_approval(submission(2, PolicyVerdict::RequiresHumanApproval))
            .expect("submit");
        wf.sweep(MissionTime(100.0), &LADDER);
        let expiry = wf
            .records()
            .iter()
            .find(|r| r.plan.id == PlanId(2))
            .expect("the expiry left a record");

        assert_eq!(rejection.operator_id, None);
        assert_eq!(expiry.operator_id, None, "the ambiguous case, on purpose");

        assert!(!rejection.is_expiry(), "a rejection is not an expiry");
        assert!(expiry.is_expiry());
        assert_ne!(
            rejection.decision, expiry.decision,
            "the two records were indistinguishable"
        );
        assert!(!rejection.is_actionable() && !expiry.is_actionable());
        let _ = id2;
    }

    /// A rejection carries the reason a person gave. PN-07 collects one and would not
    /// let the operator reject without it; before the DN-10 §3 conformance the record
    /// had nowhere to put it and it was discarded.
    #[test]
    fn a_rejection_carries_its_reason_into_the_record() {
        let mut wf = timed();
        let id = wf
            .submit_for_approval(submission(1, PolicyVerdict::RequiresHumanApproval))
            .expect("submit");
        let record = wf
            .decide(
                id,
                OperatorDecision::Rejected {
                    reason: "track is a friendly airliner".into(),
                },
                None,
                MissionTime(5.0),
            )
            .expect("decide");
        match &record.decision {
            OperatorDecision::Rejected { reason } => {
                assert_eq!(reason, "track is a friendly airliner");
            }
            other => panic!("expected a rejection, got {other:?}"),
        }
    }

    /// The record and the bus cannot disagree about what happened, because the event is
    /// derived from the record. An expiry that published `Decided { accepted: false }`
    /// would tell a peer node a person rejected the plan.
    #[test]
    fn an_expiry_publishes_an_expiry_not_a_rejection() {
        let mut wf = timed();
        wf.submit_for_approval(submission(3, PolicyVerdict::RequiresHumanApproval))
            .expect("submit");
        wf.sweep(MissionTime(100.0), &LADDER);
        let expiry = wf.records().last().expect("a record");
        assert!(matches!(
            expiry.to_event(),
            CommandEvent::Expired { plan, .. } if plan == PlanId(3)
        ));

        let mut wf2 = timed();
        let id = wf2
            .submit_for_approval(submission(4, PolicyVerdict::RequiresHumanApproval))
            .expect("submit");
        let rejection = wf2
            .decide(
                id,
                OperatorDecision::Rejected {
                    reason: "no".into(),
                },
                None,
                MissionTime(1.0),
            )
            .expect("decide");
        assert!(matches!(
            rejection.to_event(),
            CommandEvent::Decided {
                accepted: false,
                ..
            }
        ));
    }

    #[test]
    fn rejected_is_not_actionable() {
        let mut wf = InMemoryApprovalWorkflow::new();
        let id = wf
            .submit_for_approval(submission(2, PolicyVerdict::RequiresHumanApproval))
            .expect("submit");
        let record = wf
            .decide(
                id,
                OperatorDecision::Rejected {
                    reason: "friendly airliner".into(),
                },
                None,
                MissionTime(1.0),
            )
            .expect("decide");
        assert!(!record.is_actionable());
    }
}
