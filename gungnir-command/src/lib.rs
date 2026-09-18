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
//! The queue is timed as of GAP-034 and GAP-035: [`InMemoryApprovalWorkflow`] holds
//! [`PendingApproval`]s carrying the deadlines [`queue::deadlines`] computes from the
//! baseline, [`ApprovalWorkflow::sweep`] applies expiry and escalation, and the desktop
//! tick calls it every frame.
//!
//! `DecisionRecord::to_event` carries the verdict and the rationale as of GAP-047's
//! resolution, so MOE-05 can be read from the journal alone.
//!
//! # How an expiry is told from a rejection
//!
//! An expiry is its own decision, `OperatorDecision::Expired`, with no operator and no
//! role, and `DecisionRecord::is_expiry` answers from the record itself
//! (`docs/design/DN-10-queue-expiry-and-escalation.md` §3). It no longer rests on a
//! missing operator: a decided record names the signed-in operator and role, and names
//! neither when nobody is signed in (DN-23 §5 rule 1), so a `None` operator can be a
//! person's decision too. The event stream says the same thing: an expiry publishes
//! `CommandEvent::Expired` and a decision `CommandEvent::Decided`. `ARCHITECTURE.md` §10
//! item 45 recorded the question while the record could not answer it.

pub mod queue;

pub use queue::{
    deadlines, due, escalation_ladder, expiry_record, governing_layer, next_role, order_queue,
    LadderRung, PendingApproval, QueueOutcome,
};

use gungnir_model::events::CommandEvent;
use gungnir_model::{
    DecisionId, DecisionSettings, EffectorLayer, MissionTime, PlanId, PlanView, RequestId,
};
use gungnir_policy::{DenialReason, PolicyVerdict};

/// What ended a pending approval.
///
/// This is `docs/design/DN-10-queue-expiry-and-escalation.md` §3's type, conformed to
/// on 2026-09-05 with amendment 1 (§9). Two things had drifted from the note and both
/// mattered:
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

/// Who took a decision, under what request, and where (DN-23 §5 rule 1; DN-31 §5.2).
///
/// One argument rather than four, because all four are read from one caller's one act and
/// passing them separately is how a record comes to name an operator with no role. It has
/// a `Default` -- every field `None` -- which is the honest shape of a decision nobody
/// signed in for and no route carried, and the only shape an expiry can have.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DecidedBy {
    pub operator: Option<String>,
    /// The role the operator's authenticated session carried, never a selection.
    pub role: Option<String>,
    /// The client's idempotency key, when a route carried one (DN-31 §5.2).
    pub request: Option<RequestId>,
    /// The machine a forwarded decision was taken on. Filled by GAP-134; `None` here
    /// means "taken on this machine", which every decision in this build is.
    pub origin: Option<String>,
}

impl DecidedBy {
    /// A decision attributed to a verified session, with no request key.
    #[must_use]
    pub fn session(operator: Option<String>, role: Option<String>) -> Self {
        Self {
            operator,
            role,
            ..Self::default()
        }
    }

    /// The same, under a client's request key (DN-31 §6.3).
    #[must_use]
    pub fn with_request(mut self, request: Option<RequestId>) -> Self {
        self.request = request;
        self
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DecisionRecord {
    /// Minted here, and the key everything else refers to. A service facade may
    /// not depend on this crate, so engagement state keys on this identifier
    /// rather than on the record (docs/design/DN-06-engagement-and-effect.md §2).
    pub id: DecisionId,
    /// The queue item this ended, so "what became of item X" is answerable from the
    /// append-only history rather than from an index beside it (GAP-132; DN-31 §6.3's
    /// `409 AlreadyDecided`, which names the decision that stands for an item).
    ///
    /// `None` for a record written before GAP-132, and defaulted for that reason: the
    /// change is additive and `SCHEMA_VERSION` stands.
    #[serde(default)]
    pub item: Option<PendingApprovalId>,
    pub plan: PlanView,
    pub verdict: PolicyVerdict,
    pub decision: OperatorDecision,
    pub operator_id: Option<String>,
    /// The role the deciding operator's authenticated session carried, as the caller of
    /// [`ApprovalWorkflow::decide`] supplied it, and `None` when no session did (DN-23 §5
    /// rule 1). Carried into `CommandEvent::Decided::role` so D-03's rule can rank the
    /// decision if an outage leaves it in conflict with another (the GAP-067 walk,
    /// 2026-09-16). A string rather than a role because this crate cannot see
    /// `gungnir_security::Role`, the same reason `Concurrence::Operator` carries one.
    #[serde(default)]
    pub role: Option<String>,
    /// The client's idempotency key, when a route carried one (DN-31 §5.2, GAP-132).
    ///
    /// What makes a retry the same request rather than a second decision: a key already
    /// in this history is answered with the outcome it produced, and nothing is recorded.
    /// `None` for a decision no route carried a key for -- a desktop's own queue, an
    /// expiry, and every record written before GAP-132. Defaulted for that reason.
    #[serde(default)]
    pub request: Option<RequestId>,
    /// The machine a forwarded decision was taken on (DN-31 §5.3, §6.8). Filled by
    /// GAP-134; `None` means "taken here", which every decision in this build is.
    #[serde(default)]
    pub origin: Option<String>,
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
                role: self.role.clone(),
                // MOE-05 reads both from the journal.
                // The denial reason goes over in its debug spelling: the model may not
                // depend on `DenialReason`, and the spelling is stable per variant.
                verdict: self.verdict.summary(),
                rationale: match &self.decision {
                    OperatorDecision::Rejected { reason } => Some(reason.clone()),
                    _ => None,
                },
                // Both from the record, for the reason the whole event is: there is no
                // path that writes one thing to the history and another to the bus.
                request: self.request.clone(),
                origin: self.origin.clone(),
            },
        }
    }
}

/// Identifies one item in the approval queue.
///
/// **Re-exported, not declared here, since GAP-132.** The type moved to `gungnir-model`
/// so `CommandEvent::Queued` can name the item a node queued, and the model may not
/// depend on this crate (`docs/design/DN-31-node-approval-queue.md` §5.3). This crate
/// still mints it, in [`ApprovalWorkflow::submit_for_approval`], and everything that
/// named `gungnir_command::PendingApprovalId` still does. Nothing about the type changed.
pub use gungnir_model::PendingApprovalId;

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("no pending approval with id {0}")]
    NotFound(PendingApprovalId),
    #[error("plan was denied by policy ({0:?}) and cannot be submitted for approval")]
    DeniedByPolicy(DenialReason),
}

/// What became of a decision another machine took and forwarded
/// (`docs/design/DN-31-node-approval-queue.md` §6.8; GAP-134).
///
/// **The record is appended and nothing else happens.** A forwarded decision was taken on
/// the forwarding machine's own queue, on a plan its own planner proposed; the engagement
/// it opened and the handoff it issued happened there, while it was cut off. Admitting it
/// here says what that machine did, so the node's record holds the outage. Opening a
/// second engagement on the strength of it would be this deployment acting twice on one
/// decision, which is the double engagement D-58 exists to *report* and this crate must
/// not *cause*.
#[derive(Debug, Clone, PartialEq)]
pub enum ForwardOutcome {
    /// Appended to the history, once.
    Recorded,
    /// The same identifier is already held and says the same thing. Acknowledged, and
    /// nothing recorded: this is what makes a whole batch safe to send again after a
    /// connection failed part-way through it.
    AlreadyHeld,
    /// The same identifier is already held and says something **different**. Refused,
    /// naming the record that stands, because an append-only record cannot hold two
    /// accounts of one decision and picking one here would be this crate choosing.
    Contradicts(Box<DecisionRecord>),
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

/// One queue item whose offers changed when a delegation lapsed (D-15, DN-31 §6.7).
#[derive(Debug, Clone, PartialEq)]
pub struct Reoffered {
    pub plan: PlanId,
    pub item: PendingApprovalId,
    /// The roles that may no longer take it. Never empty: an item that changed changed
    /// because at least one role lost the authority for it.
    pub withdrawn: Vec<String>,
    /// Who may take it now, which is empty where no role on the ladder holds it.
    pub offered_to: Vec<String>,
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
    ///
    /// The operator and the role in `who` come from one authenticated session and are both
    /// `None` when there is none: a role is never recorded without the operator it was
    /// verified for (DN-23 §5 rule 1), because a role nobody verified would be ranked by
    /// D-03's rule as though somebody had.
    ///
    /// **This does not check `who.request` against the history.** Answering a repeated
    /// request with its first outcome is the caller's, because the caller is the one that
    /// must then answer rather than record (DN-31 §6.3); [`ApprovalWorkflow::for_request`]
    /// is the question to ask first.
    fn decide(
        &mut self,
        id: PendingApprovalId,
        decision: OperatorDecision,
        who: DecidedBy,
        now: MissionTime,
    ) -> Result<DecisionRecord, CommandError>;

    /// Admit a decision another machine took while it was cut off, forwarded on
    /// reconnect (DN-31 §6.8; GAP-134).
    ///
    /// **Keyed on the decision's own identifier, and idempotent on it.** The record is
    /// appended once; the same identifier again is acknowledged and records nothing, and
    /// the same identifier saying something else is refused naming what stands. Nothing
    /// is queued, decided, engaged or handed off -- see [`ForwardOutcome`] for why that
    /// is the whole of it.
    ///
    /// The record keeps the mission time the forwarding machine recorded, because that is
    /// when the decision was taken; when *this* deployment learned of it is the mission
    /// time of the envelope its caller publishes. Both are on the record and neither is
    /// invented, which is the treatment an effector's own `at` already gets beside
    /// `HandoffEvent::Reported`.
    fn admit_forwarded(&mut self, record: DecisionRecord) -> ForwardOutcome;

    /// Withdraw the offers a role no longer holds the authority for, and re-offer the
    /// item to the lowest role on the ladder that does (D-15, DN-31 §6.7; GAP-134).
    ///
    /// `holds` answers one question and one only: may this role take this item *now*,
    /// under the authority matrix in force. It is the caller's because the matrix and the
    /// engine are `gungnir-policy`'s and this crate has no view on who may be asked.
    ///
    /// **An item is never left offered to nobody while somebody could take it.** A lapse
    /// is not an escalation -- it removes a role rather than adding one, because the
    /// authority itself went away -- so the item goes to the lowest rung that still holds
    /// it rather than climbing one step. Where no rung holds it, the item is offered to
    /// nobody and stops escalating, because there is nowhere for it to go; it still
    /// expires, and nothing accepts it (DN-10 §5, contract C-01).
    ///
    /// Returns only the items that changed.
    fn reoffer(
        &mut self,
        ladder: &[&str],
        holds: &dyn Fn(&PendingApproval, &str) -> bool,
    ) -> Vec<Reoffered>;

    /// Append-only history, oldest first.
    fn records(&self) -> &[DecisionRecord];

    /// What became of one queue item, from the history rather than from an index beside
    /// it (DN-31 §6.3): the decision that stands, or the expiry that ended it.
    ///
    /// `None` means the item is still queued, or was never issued by this workflow.
    fn outcome_for(&self, item: PendingApprovalId) -> Option<&DecisionRecord> {
        self.records().iter().find(|r| r.item == Some(item))
    }

    /// The decision a client's request key already produced (DN-31 §6.3).
    ///
    /// A repeated key is answered with this and records nothing, which is what makes a
    /// retry after a `504` safe: the client learns which of the two things happened rather
    /// than taking a second decision to find out.
    fn for_request(&self, request: &RequestId) -> Option<&DecisionRecord> {
        self.records()
            .iter()
            .find(|r| r.request.as_ref() == Some(request))
    }

    /// The plans waiting, for callers that want only those.
    fn pending(&self) -> Vec<(PendingApprovalId, &PlanView)> {
        self.queue().iter().map(|p| (p.id, &p.plan)).collect()
    }
}

/// The queue and the record, in one process.
///
/// **No identifier counter** since GAP-130: every queue item and every decision is minted
/// as a UUID v7 at the moment it is created ([`mint`]), so two workflows -- a node's and a
/// desktop's, or one desktop's before and after a restart -- can never hand out the same
/// identifier, and identifiers from one process still sort in the order they were minted.
#[derive(Debug, Default)]
pub struct InMemoryApprovalWorkflow {
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
}

/// A new identifier: a UUID v7, held as its 128 bits (D-56, GAP-130).
///
/// v7 because its first 48 bits are a millisecond timestamp, so identifiers need no
/// coordination between machines and still sort by creation; `uuid` orders every v7 one
/// process mints, including two in the same millisecond. Minted here rather than in
/// `gungnir-model`, which takes `uuid` without a generator (D-11).
fn mint() -> u128 {
    uuid::Uuid::now_v7().as_u128()
}

impl ApprovalWorkflow for InMemoryApprovalWorkflow {
    fn submit_for_approval(
        &mut self,
        submission: Submission,
    ) -> Result<PendingApprovalId, CommandError> {
        if let PolicyVerdict::Denied { reason_code } = submission.verdict {
            return Err(CommandError::DeniedByPolicy(reason_code));
        }
        let id = PendingApprovalId(mint());
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
            let decision_id = DecisionId(mint());
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
        who: DecidedBy,
        now: MissionTime,
    ) -> Result<DecisionRecord, CommandError> {
        debug_assert!(
            who.role.is_none() || who.operator.is_some(),
            "a role was recorded with no operator: DN-23 §5 rule 1"
        );
        let index = self
            .queue
            .iter()
            .position(|p| p.id == id)
            .ok_or(CommandError::NotFound(id))?;
        let decision_id = DecisionId(mint());
        let item = self.queue.remove(index);
        let record = DecisionRecord {
            id: decision_id,
            item: Some(id),
            plan: item.plan,
            verdict: item.verdict,
            decision,
            operator_id: who.operator,
            role: who.role,
            request: who.request,
            origin: who.origin,
            mission_time: now,
        };
        self.records.push(record.clone());
        Ok(record)
    }

    fn admit_forwarded(&mut self, record: DecisionRecord) -> ForwardOutcome {
        // The identifier, not the request key and not the queue item. A forwarded
        // decision's item belongs to the forwarding machine's queue, which this workflow
        // never issued, and its request key is `None` for a decision taken at a console
        // rather than over a route. The `DecisionId` is the one thing that is the same
        // fact on both machines (D-56: minted where the thing is created, unique across
        // machines and restarts), which is why DN-31 §6.8 keys the exactly-once on it.
        if let Some(held) = self.records.iter().find(|r| r.id == record.id) {
            return if *held == record {
                ForwardOutcome::AlreadyHeld
            } else {
                ForwardOutcome::Contradicts(Box::new(held.clone()))
            };
        }
        self.records.push(record);
        ForwardOutcome::Recorded
    }

    fn reoffer(
        &mut self,
        ladder: &[&str],
        holds: &dyn Fn(&PendingApproval, &str) -> bool,
    ) -> Vec<Reoffered> {
        let mut changed = Vec::new();
        for item in &mut self.queue {
            let kept: Vec<String> = item
                .offered_to
                .iter()
                .filter(|role| holds(item, role))
                .cloned()
                .collect();
            if kept.len() == item.offered_to.len() {
                continue;
            }
            let withdrawn: Vec<String> = item
                .offered_to
                .iter()
                .filter(|role| !kept.contains(role))
                .cloned()
                .collect();
            let offered_to = if kept.is_empty() {
                // Down the ladder from the bottom, so the item goes to the lowest role
                // that still holds it rather than to the highest that happens to.
                ladder
                    .iter()
                    .find(|role| holds(item, role))
                    .map(|role| vec![(*role).to_owned()])
                    .unwrap_or_default()
            } else {
                kept
            };
            if offered_to.is_empty() {
                // Nowhere to escalate to: every rung has been asked. Stop the clock
                // rather than report an escalation every frame for an item that cannot
                // move. The expiry stands, and no path accepts on expiry (C-01).
                item.escalate_at = None;
            }
            item.offered_to = offered_to.clone();
            changed.push(Reoffered {
                plan: item.plan.id,
                item: item.id,
                withdrawn,
                offered_to,
            });
        }
        changed
    }

    fn records(&self) -> &[DecisionRecord] {
        &self.records
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::PlanId;

    fn plan(id: u128) -> PlanView {
        PlanView {
            id: PlanId(id),
            ..PlanView::default()
        }
    }

    /// A submission at t=0 on the Point layer, offered to the operator.
    fn submission(id: u128, verdict: PolicyVerdict) -> Submission {
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
                DecidedBy::session(Some("op-1".into()), None),
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
            uuid::Uuid::from_u128(record.id.0).get_version(),
            Some(uuid::Version::SortRand),
            "the workflow mints the identifier, as a UUID v7 (D-56)"
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
                    DecidedBy::session(Some("op".into()), None),
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
                DecidedBy::session(None, None),
                MissionTime(0.0)
            ),
            Err(CommandError::NotFound(_))
        ));
    }

    /// The `gungnir-command` Decision recording row of
    /// `docs/verification-capability-table.md` §2: "a `DecisionRecord` per decision; no
    /// plan actionable without one". Each of the three choices a person can make appends
    /// exactly one record, the newest record is the decision just taken, and a `decide`
    /// that names nothing in the queue appends nothing.
    ///
    /// The expected actionability is DN-10 §3's definition, not `is_actionable`'s:
    /// accepting acts on the plan, overriding acts on the operator's own substitute (the
    /// plan stored in the record is the one they acted on), and rejecting declines it.
    #[test]
    fn every_decide_appends_exactly_one_record_carrying_that_decision() {
        let mut wf = InMemoryApprovalWorkflow::new();
        let queued: Vec<PendingApprovalId> = (1..=3)
            .map(|n| {
                wf.submit_for_approval(submission(n, PolicyVerdict::RequiresHumanApproval))
                    .expect("submit")
            })
            .collect();
        assert!(
            wf.records().is_empty(),
            "queueing a plan is not deciding it"
        );

        // `submission(n, ..)` queues plan n, so the k-th queued item is plan k + 1.
        let decisions = [
            (queued[0], PlanId(1), OperatorDecision::Accepted, 1.0, true),
            (
                queued[1],
                PlanId(2),
                OperatorDecision::Overridden,
                2.0,
                true,
            ),
            (
                queued[2],
                PlanId(3),
                OperatorDecision::Rejected {
                    reason: "track is a friendly airliner".into(),
                },
                3.0,
                false,
            ),
        ];
        for (id, plan, decision, at, actionable) in decisions {
            let before = wf.records().len();
            let returned = wf
                .decide(
                    id,
                    decision.clone(),
                    DecidedBy::session(Some("op-1".into()), Some("Operator".into())),
                    MissionTime(at),
                )
                .expect("decide");
            assert_eq!(
                wf.records().len(),
                before + 1,
                "{decision:?} did not append exactly one record"
            );
            let newest = wf.records().last().expect("a record");
            assert_eq!(newest.decision, decision);
            assert_eq!(newest.plan.id, plan, "the newest record is another plan's");
            assert_eq!(newest.operator_id.as_deref(), Some("op-1"));
            assert_eq!(newest.role.as_deref(), Some("Operator"));
            assert_eq!(newest.mission_time, MissionTime(at));
            assert_eq!(newest.is_actionable(), actionable, "{decision:?}");
            assert_eq!(
                newest, &returned,
                "the record kept is not the record returned"
            );
        }

        // Nothing in the queue answers to either of these: an identifier never issued,
        // and one already decided, which left the queue when it was. A second record for
        // plan 1 would be two decisions where a person took one.
        let history = wf.records().to_vec();
        for id in [PendingApprovalId(99), queued[0]] {
            assert!(matches!(
                wf.decide(
                    id,
                    OperatorDecision::Accepted,
                    DecidedBy::session(Some("op-1".into()), Some("Operator".into())),
                    MissionTime(4.0)
                ),
                Err(CommandError::NotFound(missing)) if missing == id
            ));
            assert_eq!(
                wf.records(),
                history.as_slice(),
                "a refused decide on {id:?} changed the history"
            );
        }
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
                DecidedBy::session(None, None),
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
                DecidedBy::session(None, None),
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
                DecidedBy::session(None, None),
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
                DecidedBy::session(None, None),
                MissionTime(1.0),
            )
            .expect("decide");
        assert!(!record.is_actionable());
    }

    /// The role a session supplied reaches the record and the event, so D-03's rule can
    /// rank the decision later; a decision given none records none, and an expiry never
    /// carries one (the GAP-067 walk, 2026-09-16).
    #[test]
    fn the_deciding_role_reaches_the_record_and_the_event_and_is_never_invented() {
        let mut wf = timed();
        let signed_in = wf
            .submit_for_approval(submission(1, PolicyVerdict::RequiresHumanApproval))
            .expect("submit");
        let record = wf
            .decide(
                signed_in,
                OperatorDecision::Accepted,
                DecidedBy::session(Some("7".into()), Some("Supervisor".into())),
                MissionTime(5.0),
            )
            .expect("decide");
        assert_eq!(record.role.as_deref(), Some("Supervisor"));
        assert!(matches!(
            record.to_event(),
            CommandEvent::Decided { operator: Some(op), role: Some(role), .. }
                if op == "7" && role == "Supervisor"
        ));

        let nobody = wf
            .submit_for_approval(submission(2, PolicyVerdict::RequiresHumanApproval))
            .expect("submit");
        let record = wf
            .decide(
                nobody,
                OperatorDecision::Rejected {
                    reason: "friendly airliner".into(),
                },
                DecidedBy::session(None, None),
                MissionTime(6.0),
            )
            .expect("decide");
        assert!(matches!(
            record.to_event(),
            CommandEvent::Decided {
                operator: None,
                role: None,
                ..
            }
        ));

        wf.submit_for_approval(submission(3, PolicyVerdict::RequiresHumanApproval))
            .expect("submit");
        wf.sweep(MissionTime(100.0), &LADDER);
        let expiry = wf.records().last().expect("the expiry left a record");
        assert!(expiry.is_expiry());
        assert_eq!(expiry.role, None, "nobody decided, so no role did");
    }

    /// A record as another machine forwards it: identifiers minted there, the time it
    /// was taken there, and the machine it came from.
    fn forwarded(id: u128, plan_id: u128, origin: &str, at: f64) -> DecisionRecord {
        DecisionRecord {
            id: DecisionId(id),
            item: None,
            plan: plan(plan_id),
            verdict: PolicyVerdict::RequiresHumanApproval,
            decision: OperatorDecision::Accepted,
            operator_id: Some("7".into()),
            role: Some("Supervisor".into()),
            request: None,
            origin: Some(origin.to_owned()),
            mission_time: MissionTime(at),
        }
    }

    /// DN-31 §6.8: the record is appended once, keyed on the decision's own identifier,
    /// and forwarding the same one again records nothing.
    ///
    /// The zero beside the non-zero: the first call has to record something, or the
    /// second call recording nothing would prove nothing at all.
    #[test]
    fn a_forwarded_decision_is_recorded_once_and_the_same_one_again_records_nothing() {
        let mut wf = InMemoryApprovalWorkflow::new();
        let record = forwarded(9001, 5, "desk-a", 110.0);

        assert_eq!(wf.records().len(), 0);
        assert_eq!(
            wf.admit_forwarded(record.clone()),
            ForwardOutcome::Recorded,
            "the first forwarding has to record it"
        );
        assert_eq!(wf.records().len(), 1);
        assert_eq!(wf.records()[0].origin.as_deref(), Some("desk-a"));
        assert_eq!(
            wf.records()[0].mission_time,
            MissionTime(110.0),
            "the record keeps the time the decision was taken, not the time it arrived"
        );

        assert_eq!(
            wf.admit_forwarded(record.clone()),
            ForwardOutcome::AlreadyHeld
        );
        assert_eq!(wf.admit_forwarded(record), ForwardOutcome::AlreadyHeld);
        assert_eq!(
            wf.records().len(),
            1,
            "forwarding the same decision again appended a second account of it"
        );
    }

    /// One identifier, two different accounts of what happened: refused naming the one
    /// that stands, rather than appended beside it or silently preferred.
    #[test]
    fn a_forwarded_decision_that_contradicts_one_already_held_is_refused() {
        let mut wf = InMemoryApprovalWorkflow::new();
        let held = forwarded(9001, 5, "desk-a", 110.0);
        assert_eq!(wf.admit_forwarded(held.clone()), ForwardOutcome::Recorded);

        let mut other = held.clone();
        other.decision = OperatorDecision::Rejected {
            reason: "friendly airliner".into(),
        };
        assert_eq!(
            wf.admit_forwarded(other),
            ForwardOutcome::Contradicts(Box::new(held)),
            "the refusal names the record that stands"
        );
        assert_eq!(wf.records().len(), 1, "a refused forwarding recorded one");
    }

    /// A forwarded decision is a fact about another machine's queue, so it never ends an
    /// item on this one, however the two identifiers happen to line up.
    #[test]
    fn a_forwarded_decision_ends_nothing_in_this_queue() {
        let mut wf = timed();
        let item = wf
            .submit_for_approval(submission(5, PolicyVerdict::RequiresHumanApproval))
            .expect("submit");
        assert_eq!(wf.queue().len(), 1);

        assert_eq!(
            wf.admit_forwarded(forwarded(9001, 5, "desk-a", 110.0)),
            ForwardOutcome::Recorded
        );
        assert_eq!(
            wf.queue().len(),
            1,
            "the plan is still waiting for a person here; the other machine decided its own"
        );
        assert_eq!(
            wf.outcome_for(item),
            None,
            "a forwarded decision was read as ending an item this workflow issued"
        );
    }

    /// D-15 lapsing: the role that held an item only by delegation loses it, and the item
    /// goes to the lowest rung that still holds the authority rather than to nobody.
    #[test]
    fn a_lapse_withdraws_the_offer_and_re_offers_the_item_to_a_role_that_holds_it() {
        let mut wf = timed();
        let item = wf
            .submit_for_approval(submission(5, PolicyVerdict::RequiresHumanApproval))
            .expect("submit");

        // Nothing has changed: the operator still holds it, and nothing is reported.
        assert!(
            wf.reoffer(&LADDER, &|_, _| true).is_empty(),
            "an unchanged queue reported a change"
        );
        assert_eq!(wf.queue()[0].offered_to, ["operator"]);

        // The delegation lapses: the operator no longer holds it, the supervisor does.
        let changed = wf.reoffer(&LADDER, &|_, role| role != "operator");
        assert_eq!(
            changed,
            vec![Reoffered {
                plan: PlanId(5),
                item,
                withdrawn: vec!["operator".into()],
                offered_to: vec!["supervisor".into()],
            }]
        );
        assert_eq!(wf.queue()[0].offered_to, ["supervisor"]);
        assert!(
            !wf.queue()[0].may_be_decided_by("operator"),
            "the item is still actionable by the role whose delegation lapsed"
        );
        assert!(wf.queue()[0].may_be_decided_by("supervisor"));
        assert!(
            wf.queue()[0].escalate_at.is_some(),
            "an item somebody may still take goes on escalating (D-59)"
        );
    }

    /// An escalated item keeps the rungs that hold the authority on their own account:
    /// a lapse withdraws a delegation, not an escalation.
    #[test]
    fn a_lapse_keeps_every_role_that_still_holds_the_item() {
        let mut wf = timed();
        wf.submit_for_approval(submission(5, PolicyVerdict::RequiresHumanApproval))
            .expect("submit");
        wf.sweep(MissionTime(21.0), &LADDER);
        assert_eq!(
            wf.queue()[0].offered_to,
            ["operator", "supervisor"],
            "escalation adds a role without removing the first (DN-10 §5)"
        );

        let changed = wf.reoffer(&LADDER, &|_, role| role != "operator");
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].withdrawn, ["operator"]);
        assert_eq!(wf.queue()[0].offered_to, ["supervisor"]);
    }

    /// No rung holds it: offered to nobody, the escalation clock stopped because there is
    /// nowhere to go, and still expiring rather than quietly accepted (C-01).
    #[test]
    fn an_item_no_role_holds_is_offered_to_nobody_and_still_expires() {
        let mut wf = timed();
        wf.submit_for_approval(submission(5, PolicyVerdict::RequiresHumanApproval))
            .expect("submit");

        let changed = wf.reoffer(&LADDER, &|_, _| false);
        assert_eq!(changed.len(), 1);
        assert!(changed[0].offered_to.is_empty());
        assert!(wf.queue()[0].offered_to.is_empty());
        assert_eq!(
            wf.queue()[0].escalate_at,
            None,
            "an item with nowhere to go kept an escalation clock running"
        );

        wf.sweep(MissionTime(31.0), &LADDER);
        assert!(wf.queue().is_empty());
        let record = wf.records().last().expect("the expiry left a record");
        assert!(record.is_expiry(), "an item nobody held was not expired");
        assert!(
            !record.is_actionable(),
            "an item nobody could take became actionable"
        );
    }
}
