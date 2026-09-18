// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Feeding the queue, sweeping it, and taking a decision
//! (`docs/design/DN-31-node-approval-queue.md` §3 points 2 to 4; GAP-131, D-57).
//!
//! The approval gate between a proposed plan and anything acting on it (GAP-038), moved
//! here from `gungnir-app` so both binaries run the one implementation. Nothing here
//! executes anything -- that boundary is `gungnir-policy`'s whole purpose, and contract
//! C-01 depends on it: a plan becomes actionable only through a recorded human decision,
//! and the engagement and the handoff hang off that record rather than off the plan.

use crate::{ApprovalContext, ApprovalDesk, ApprovalHost, PolicyInputs};
use gungnir_command::{
    governing_layer, ApprovalWorkflow, CommandError, DecidedBy, OperatorDecision,
    PendingApprovalId, QueueOutcome, Submission,
};
use gungnir_eventing::Event;
use gungnir_model::events::{CommandEvent, InterceptEvent};
use gungnir_model::PlanView;
use gungnir_policy::PolicyVerdict;

/// The last denial the chain returned, kept so an empty queue can say why.
#[derive(Debug, Clone, Default)]
pub struct DenialHistory {
    pub count: usize,
    pub last_reason: Option<String>,
}

/// What the queue sweep has done this session.
///
/// Only escalations are counted here. Expiries are **not**, and that is the point of
/// the DN-10 §3 conformance: an expiry is `OperatorDecision::Expired` in the record, so
/// [`ApprovalDesk::expired_count`] reads the history rather than a parallel counter that
/// could drift from it. An escalation has no record by design -- an escalated item has not
/// ended -- so a counter is the only place it can live.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QueueOutcomeCounts {
    /// Offers made to a higher role. Not decisions, and not failures.
    pub escalated: usize,
}

/// What became of a plan handed to [`ApprovalDesk::submit`].
///
/// A policy verdict alone could not say this: **a superseded plan was never evaluated**,
/// and reporting it as `Denied` would put a refusal nobody made into the record while
/// reporting it as `RequiresHumanApproval` would leave an item waiting that will never be
/// applied. Both are worse than a third case.
#[derive(Debug, Clone, PartialEq)]
pub enum Submitted {
    /// The policy chain ran, and this is what it returned.
    Evaluated(PolicyVerdict),
    /// **Not evaluated.** The baseline in force is outside its validity window, so this
    /// plan is superseded rather than applied (DN-08 §5). `PlanSuperseded` is published,
    /// so the journal records what happened rather than a gap where a plan should be.
    Superseded,
}

impl ApprovalDesk {
    /// Evaluate a plan and queue it if it clears policy.
    ///
    /// Returns what became of it so the caller can record the denial. A denied plan is
    /// never queued: that is `ApprovalWorkflow::submit_for_approval`'s invariant and this
    /// function does not work around it.
    pub fn submit(
        &mut self,
        cx: &ApprovalContext<'_>,
        policy: &PolicyInputs<'_>,
        host: &mut dyn ApprovalHost,
        plan: PlanView,
    ) -> Submitted {
        if let Some(superseded) = self.refuse_superseded(cx, host, &plan) {
            return superseded;
        }
        let verdict = crate::chain::evaluate(cx, policy, &plan);
        self.queue_evaluated(cx, policy, host, plan, verdict, cx.role_name(), false)
    }

    /// Evaluate a plan for every role on the ladder and queue it for the lowest role that
    /// may take it (DN-31 §6.1, GAP-132; GAP-113).
    ///
    /// **The node's submission**, and the difference from [`ApprovalDesk::submit`] is the
    /// asking role: a node has nobody at a console, so there is no one role to run the
    /// authority engine for, and the plan an Operator may not accept is the plan a
    /// Supervisor should be offered. A plan no role on the ladder may accept is denied and
    /// never queued, which is what makes the denial reviewable rather than counted.
    ///
    /// Publishes `Queued` for what it queues, so a desktop following the stream builds the
    /// queue without polling (DN-31 §5.3) and MOP-07 is measured over the whole path
    /// (§6.9). [`ApprovalDesk::submit`] does not: a desktop's own queue is not something a
    /// second machine follows, and a plan queued twice on one stream would be two items.
    pub fn submit_to_ladder(
        &mut self,
        cx: &ApprovalContext<'_>,
        policy: &PolicyInputs<'_>,
        host: &mut dyn ApprovalHost,
        plan: PlanView,
    ) -> Submitted {
        if let Some(superseded) = self.refuse_superseded(cx, host, &plan) {
            return superseded;
        }
        let offering = crate::chain::offer_to(cx, policy, &plan);
        let role = offering
            .offered_to
            .map_or_else(|| cx.role_name(), |r| format!("{r:?}"));
        self.queue_evaluated(cx, policy, host, plan, offering.verdict, role, true)
    }

    /// The supersession check both submission paths open with (DN-08 §5).
    ///
    /// **Before policy, not after.** A plan produced under a baseline that is outside its
    /// window is superseded whatever policy would have said about it, and running the
    /// chain first would put a verdict in the record for a plan that was never going to be
    /// applied.
    fn refuse_superseded(
        &mut self,
        cx: &ApprovalContext<'_>,
        host: &mut dyn ApprovalHost,
        plan: &PlanView,
    ) -> Option<Submitted> {
        if !cx.baseline_supersedes_plans {
            return None;
        }
        let now = cx.now;
        let plan_id = plan.id;
        host.publish(
            now,
            Event::Intercept(InterceptEvent::PlanSuperseded(plan.clone())),
        );
        self.observe_superseded(host, plan_id, now);
        // Said once per plan, not once per frame: `submit` runs only when the plan
        // changes, which is why this is here rather than in the tick.
        // An alert names the plan by its tag and a log field in full (D-61).
        host.alert(format!(
            "Plan {} superseded: the baseline in force is outside its validity \
             window, so nothing produced now will be applied",
            plan_id.short()
        ));
        tracing::warn!(plan = %plan_id, "plan superseded: baseline not in force");
        Some(Submitted::Superseded)
    }

    /// Publish the verdict and queue the plan if it cleared policy.
    ///
    /// One body for both submission paths, so the rule that a denied plan is never queued,
    /// the denial counters and the governing layer cannot come to differ between a node
    /// and a desktop.
    #[allow(clippy::too_many_arguments)]
    fn queue_evaluated(
        &mut self,
        cx: &ApprovalContext<'_>,
        policy: &PolicyInputs<'_>,
        host: &mut dyn ApprovalHost,
        plan: PlanView,
        verdict: PolicyVerdict,
        role: String,
        announce: bool,
    ) -> Submitted {
        // GAP-036: PN-05 lists every fires check with its result, passes included.
        self.fires_checks = crate::chain::fires_checks(cx, policy, &plan);
        // GAP-028: the verdict is on the record with the engines that produced it,
        // whatever it was; a denial that reached only the counter would be a decision
        // nobody could review.
        host.publish(
            cx.now,
            Event::Intercept(InterceptEvent::PlanEvaluated {
                plan: plan.id,
                verdict: verdict.summary(),
                engines: crate::chain::CHAIN_ENGINES
                    .iter()
                    .map(|e| (*e).to_string())
                    .collect(),
            }),
        );
        if let PolicyVerdict::Denied { reason_code } = verdict {
            self.denials.count += 1;
            self.denials.last_reason = Some(format!("{reason_code:?}"));
            return Submitted::Evaluated(verdict);
        }
        // The layer whose window closes first governs the deadline; a plan tasking nothing
        // this deployment knows about has no layer, and policy has already denied it.
        let Some(layer) = governing_layer(&plan, cx.resources, &cx.config.policy.decisions) else {
            tracing::error!(
                plan = %plan.id,
                "a plan that tasks no known resource cleared policy; not queuing it"
            );
            return Submitted::Evaluated(verdict);
        };
        let plan_id = plan.id;
        let submission = Submission {
            plan,
            verdict,
            submitted: cx.now,
            layer,
            // No assessment runs, so every item scores the same and the ordering falls
            // back to time remaining. Reported by `QueueOrder`, not hidden behind a zero.
            priority: 0.0,
            role,
        };
        match self.approvals.submit_for_approval(submission) {
            Ok(id) => {
                if announce {
                    self.announce_queued(host, cx.now, id, plan_id);
                }
            }
            // Unreachable given the branch above, but a silent `let _ =` here would be the
            // place a real refusal went missing.
            Err(err) => tracing::error!(%err, "plan was refused by the approval workflow"),
        }
        Submitted::Evaluated(verdict)
    }

    /// Publish `Queued` for an item just submitted (DN-31 §5.3, §6.2).
    ///
    /// Read back off the queue rather than assembled from the submission, so the deadlines
    /// and the roles on the stream are the ones the queue actually holds: `deadlines` is
    /// the workflow's to compute from the baseline it was built with, and an event carrying
    /// a second computation of them is the one that goes stale.
    fn announce_queued(
        &mut self,
        host: &mut dyn ApprovalHost,
        now: gungnir_model::MissionTime,
        id: PendingApprovalId,
        plan: gungnir_model::PlanId,
    ) {
        let Some(item) = self.approvals.queue().iter().find(|p| p.id == id) else {
            tracing::error!(%id, %plan, "an item was submitted and is not in the queue");
            return;
        };
        let event = CommandEvent::Queued {
            item: id,
            plan,
            layer: item.layer,
            offered_to: item.offered_to.clone(),
            expires_at: item.expires_at,
            escalate_at: item.escalate_at,
        };
        tracing::info!(%id, %plan, offered_to = ?item.offered_to, "plan queued for a decision");
        host.publish(now, Event::Command(event));
    }

    /// Apply expiry and escalation, publishing what happened.
    ///
    /// Called every tick. Nothing ends silently: an expiry leaves a `DecisionRecord` that
    /// is not actionable and names no operator, and both outcomes reach the bus, so the
    /// journal records a decision nobody took as exactly that rather than as a rejection.
    pub fn sweep(&mut self, cx: &ApprovalContext<'_>, host: &mut dyn ApprovalHost) {
        let now = cx.now;
        let ladder = crate::chain::escalation_ladder();
        let refs: Vec<&str> = ladder.iter().map(String::as_str).collect();
        let outcomes = self.approvals.sweep(now, &refs);
        for (plan, id, outcome) in outcomes {
            let event = match &outcome {
                QueueOutcome::Expired { at } => {
                    tracing::warn!(
                        plan = %plan,
                        id = %id,
                        "approval expired with nobody deciding"
                    );
                    host.alert(format!(
                        "Plan {} expired with nobody deciding; this is not a rejection",
                        plan.short()
                    ));
                    CommandEvent::Expired { plan, at: *at }
                }
                QueueOutcome::Escalated { to_role, at } => {
                    self.queue_outcomes.escalated += 1;
                    tracing::info!(plan = %plan, to_role, "approval escalated");
                    CommandEvent::Escalated {
                        plan,
                        to_role: to_role.clone(),
                        at: *at,
                    }
                }
            };
            host.publish(now, Event::Command(event));
        }
    }

    /// Record a decision and publish it.
    ///
    /// The operator and the role come from `cx.signed_in`: whoever the host has verified,
    /// and neither when it has verified nobody (GAP-057, DN-23 §5 rule 1). Naming a role
    /// as though it were a person would put a false attribution into an append-only
    /// record, and a role nobody verified would be ranked by D-03's rule as though
    /// somebody had. **Not the role the host is acting in**, which may be a selection
    /// (the GAP-067 walk, 2026-09-16).
    ///
    /// # Errors
    ///
    /// `CommandError::NotFound` when nothing in the queue answers to `id` -- an identifier
    /// never issued, or an item already decided or expired.
    pub fn decide(
        &mut self,
        cx: &ApprovalContext<'_>,
        host: &mut dyn ApprovalHost,
        id: PendingApprovalId,
        decision: OperatorDecision,
    ) -> Result<(), CommandError> {
        self.decide_for(cx, host, id, decision, None).map(|_| ())
    }

    /// The same, under a client's request key, returning the decision it recorded
    /// (DN-31 §6.3, GAP-132).
    ///
    /// The identifier is returned because the route has to answer with it: `201
    /// DecisionRecorded { decision }`, and the same decision again if the client repeats
    /// the key. **The key is not checked here** -- see
    /// [`gungnir_command::ApprovalWorkflow::for_request`]: a caller that found a key
    /// already recorded must answer with its outcome rather than call this at all, and
    /// checking it in both places would put the rule where it could disagree with itself.
    ///
    /// # Errors
    ///
    /// `CommandError::NotFound`, exactly as [`ApprovalDesk::decide`].
    pub fn decide_for(
        &mut self,
        cx: &ApprovalContext<'_>,
        host: &mut dyn ApprovalHost,
        id: PendingApprovalId,
        decision: OperatorDecision,
        request: Option<gungnir_model::RequestId>,
    ) -> Result<gungnir_model::DecisionId, CommandError> {
        let now = cx.now;
        let operator = cx.signed_in.as_ref().map(|s| s.operator.clone());
        let role = cx.signed_in.as_ref().map(|s| s.role.clone());
        let who = DecidedBy::session(operator, role).with_request(request);
        let record = self.approvals.decide(id, decision, who, now)?;
        tracing::info!(
            plan = %record.plan.id,
            decision_id = %record.id,
            decision = ?record.decision,
            actionable = record.is_actionable(),
            "decision recorded"
        );
        host.publish(now, Event::Command(record.to_event()));
        // GAP-059: the decision is on the audit trail with the verified operator, or none.
        host.audit(
            crate::chain::DECISION_ACTION,
            // The whole identifier: an audit entry is searched for, never glanced at
            // (D-61).
            format!("plan {} {:?}", record.plan.id, record.decision),
        );
        // GAP-043: an actionable decision is the moment an engagement opens (DN-06 §5).
        let opened = self.open_for(cx, host, &record);
        tracing::debug!(decision = %record.id, opened, "engagements opened");
        Ok(record.id)
    }

    /// Withdraw the offers D-15's lapsed delegations granted, and re-offer each item to
    /// the lowest role that still holds it (DN-31 §6.7; GAP-134).
    ///
    /// Called by a host once, at the moment its delegations lapse -- `cx.delegations` is
    /// then `Lapsed`, so [`crate::chain::holds_authority`] asks every question of the
    /// matrix without the delegated rules. **This is what makes the lapse change what an
    /// item is actionable by** and not only what a panel draws: `offered_to` is the
    /// queue's own state, read by [`gungnir_command::PendingApproval::may_be_decided_by`]
    /// and by every row that says who may decide. Plans submitted after the lapse need no
    /// call: the chain already reads the lapsed matrix, so a plan only a delegation could
    /// take is denied by authority and never queued.
    ///
    /// Each item that moved is alerted by plan, and returned so the host can put the
    /// lapse on its record as one event; nothing here publishes on its own, because the
    /// lapse is a fact about the link and the host is what knows the link.
    pub fn lapse_delegations(
        &mut self,
        cx: &ApprovalContext<'_>,
        host: &mut dyn ApprovalHost,
    ) -> Vec<gungnir_command::Reoffered> {
        debug_assert!(
            cx.delegations == gungnir_policy::Delegations::Lapsed,
            "a lapse was applied with the delegations still in force"
        );
        let ladder = crate::chain::escalation_ladder();
        let refs: Vec<&str> = ladder.iter().map(String::as_str).collect();
        let changed = self.approvals.reoffer(&refs, &|item, role| {
            crate::chain::holds_authority(cx, role, &item.plan)
        });
        for moved in &changed {
            let to = if moved.offered_to.is_empty() {
                "nobody -- no role holds it without the delegation, and it will expire".to_owned()
            } else {
                moved.offered_to.join(", ")
            };
            host.alert(format!(
                "Plan {}: the delegation it was offered under has lapsed (D-15); {} may no \
                 longer decide it, and it is now offered to {to}",
                moved.plan.short(),
                moved.withdrawn.join(", "),
            ));
            tracing::warn!(
                plan = %moved.plan,
                item = %moved.item,
                withdrawn = ?moved.withdrawn,
                offered_to = ?moved.offered_to,
                "a lapsed delegation withdrew an offer"
            );
        }
        changed
    }

    /// Put a decision another machine took on this desk's record, once (DN-31 §6.8;
    /// GAP-134).
    ///
    /// **Recorded, published, audited -- and nothing else.** The engagement that decision
    /// opened and the handoff it issued happened on the machine that took it, while it was
    /// cut off; this is the record catching up with them. Opening an engagement here would
    /// be this deployment acting a second time on one decision, which is the double
    /// engagement D-58 exists to report. So there is no `open_for` below, and
    /// `gungnir-app/tests/no_execution_without_decision.rs` still finds exactly one place
    /// each of the two is constructed.
    ///
    /// `Decided` is published only when the record is new, from the record itself
    /// (`to_event`), so the event carries the `origin` and the forwarding machine's own
    /// identifiers and the journal gains one account of the decision however many times it
    /// is forwarded. The envelope is timed by this host's clock, which is when the record
    /// learned of it; the record keeps the time it was taken.
    pub fn admit_forwarded(
        &mut self,
        cx: &ApprovalContext<'_>,
        host: &mut dyn ApprovalHost,
        record: gungnir_command::DecisionRecord,
    ) -> gungnir_command::ForwardOutcome {
        let outcome = self.approvals.admit_forwarded(record.clone());
        if outcome == gungnir_command::ForwardOutcome::Recorded {
            tracing::info!(
                plan = %record.plan.id,
                decision_id = %record.id,
                origin = ?record.origin,
                "a forwarded decision reached the record"
            );
            host.publish(cx.now, Event::Command(record.to_event()));
            // One entry per gated act on this host's trail (C-04), naming the machine and
            // the operator the forwarding machine recorded rather than whoever forwarded it:
            // the entry is about the decision, and the decision was theirs.
            host.audit(
                crate::chain::DECISION_ACTION,
                format!(
                    "forwarded from {}: plan {} {:?} by operator {} as {} at T+{:.0} s",
                    record.origin.as_deref().unwrap_or("an unnamed machine"),
                    record.plan.id,
                    record.decision,
                    record.operator_id.as_deref().unwrap_or("nobody signed in"),
                    record.role.as_deref().unwrap_or("no recorded role"),
                    record.mission_time.0,
                ),
            );
        }
        outcome
    }

    /// Windows that closed with nobody deciding, from the append-only history.
    ///
    /// **Not rejections**: nobody chose. Answerable from the record since the DN-10 §3
    /// conformance; before it, this could only have been derived from a rejection
    /// happening to name no operator, which every rejection does while there is no
    /// operator session.
    #[must_use]
    pub fn expired_count(&self) -> usize {
        self.approvals
            .records()
            .iter()
            .filter(|r| r.is_expiry())
            .count()
    }
}
