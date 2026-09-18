// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The node's side of `gungnir-approval` (GAP-132, D-55, D-57, edge (y);
//! `docs/design/DN-31-node-approval-queue.md` §3 and §6).
//!
//! **The queue belongs to the loop, not to a request handler.** `gungnir-api`'s routes
//! accept a decision, hand it to the loop through a channel and wait for the loop's
//! answer, exactly as the sensor-tasking route hands a command over. One loop taking
//! requests in arrival order is what makes "the first valid decision wins" a property of
//! the design rather than the outcome of a race (DN-31 §3), and it is why the node's
//! decision route could not be built inside `gungnir-api`: a queue invented in a request
//! handler would put the recommend-versus-act boundary in the transport.
//!
//! The decision path itself is `gungnir-approval`'s and this module writes none of it
//! again. What is here is the two things only this binary can say -- what the picture is
//! right now ([`gungnir_approval::ApprovalContext`]) and where an effect goes
//! ([`gungnir_approval::ApprovalHost`]) -- and the three loop steps that call it:
//! [`propose`], [`sweep`] and [`answer_decisions`].
//!
//! # Why this is a library module and not part of `main.rs`
//!
//! DN-31 §9 rows 3 to 6 are about what a node does, over the real transport, and a test
//! cannot reach inside a binary. `gungnir-app` has had a `[lib]` beside its `[[bin]]`
//! since it was written for the same reason. So the loop steps live here, the binary
//! calls them, and `gungnir-node/tests/approval_queue.rs` drives the same source rather
//! than a copy of it kept in step by hand.

use gungnir_api::transport::{
    DecisionAnswer, ForwardAnswer, NodeApi, PendingDecision, PendingForward, RefusedDecision,
};
use gungnir_api::v3;
use gungnir_approval::{
    ApprovalContext, ApprovalDesk, ApprovalHost, DeliveryAnswer, HandoffInFlight, HandoffRecord,
    HandoffTransport, PolicyInputs, SignedIn, Submitted,
};
use gungnir_command::{
    ApprovalWorkflow, CommandError, DecisionRecord, ForwardOutcome, OperatorDecision,
};
use gungnir_config::ConfigBaseline;
use gungnir_eventing::{Event, EventBus, InProcessBus};
use gungnir_model::events::LinkEvent;
use gungnir_model::{MissionTime, PlanView, ResourceView, TrackView};
use gungnir_remote::endpoint::{DeliveryOutcome, EndpointClient, PendingDelivery};
use gungnir_security::{actions, AuditEntry, AuditLog, InMemoryAuditLog, OperatorId, Role};

/// What the node holds between ticks for the approval queue.
///
/// The desk and the audit log together, because DN-31 §9 row 4 counts entries per decision
/// **and per refusal** and a node with the one and not the other could record a decision
/// nobody could review. A node owns exactly one.
#[derive(Debug, Default)]
pub struct NodeApproval {
    /// The queue, its record, the engagements and the handoffs.
    pub desk: ApprovalDesk,
    /// One entry per gated action (contract C-04, GAP-059).
    ///
    /// The node's own log, because the node is now where the decision happens: a desktop's
    /// log holds what that console did, and neither is the other's record.
    pub audit: InMemoryAuditLog,
    /// What each outage's reconciliation settled, per plan, as forwarded (GAP-134,
    /// DN-31 §6.8; MT-10 step 5).
    ///
    /// Kept so a settlement is put on the journal once however many times its batch is
    /// sent, and so a second, different settlement of one plan is refused rather than
    /// journaled beside the first. The journal is where a reviewer reads them; this is
    /// only what makes the exactly-once checkable without re-reading it.
    pub settlements: std::collections::BTreeMap<gungnir_model::PlanId, v3::Settlement>,
}

impl NodeApproval {
    /// A desk timed by this deployment's decision settings.
    #[must_use]
    pub fn new(config: &ConfigBaseline) -> Self {
        Self {
            desk: ApprovalDesk::new(config.policy.decisions.clone()),
            audit: InMemoryAuditLog::new(),
            settlements: std::collections::BTreeMap::new(),
        }
    }

    /// The queue as the wire carries it (DN-31 §5.2), in the queue's own order.
    ///
    /// Read off the workflow rather than remembered, so `GET /v3/queue`, the snapshot and
    /// the sweep cannot come to disagree about what is waiting or in what order.
    #[must_use]
    pub fn queue_view(
        &self,
        config: &ConfigBaseline,
        resources: &[ResourceView],
        tracks: &[TrackView],
    ) -> Vec<v3::QueueItemView> {
        let classification = |id: gungnir_model::TrackId| {
            tracks
                .iter()
                .find(|t| t.id == id)
                .map_or(gungnir_model::Classification::Unknown, |t| t.classification)
        };
        self.desk
            .approvals
            .queue()
            .iter()
            .map(|item| v3::QueueItemView {
                item: item.id,
                plan: item.plan.clone(),
                verdict: item.verdict.summary(),
                layer: item.layer,
                submitted: item.submitted,
                expires_at: item.expires_at,
                escalate_at: item.escalate_at,
                offered_to: item.offered_to.clone(),
                // **The role it was submitted to, not the role it has escalated to.**
                // D-15 delegates a case to a role, and DN-31 §5.2 reads the flag as "a
                // pre-delegated item is actionable for the Operator from submission".
                // Escalation adds a role without removing the first (DN-10 §5), so the
                // Operator's delegation still stands on an escalated item and asking about
                // the *last* role offered would say it had lapsed -- which it has not, and
                // which would make the row say a supervisor took it away.
                //
                // Asked of the matrix rather than remembered on the item, so that a
                // baseline applied after submission cannot leave a row claiming a
                // delegation the matrix no longer grants. It still expires and escalates
                // either way (D-59).
                pre_delegated: item.offered_to.first().is_some_and(|role| {
                    gungnir_policy::is_pre_delegated(
                        &config.policy.authority,
                        actions::DECIDE_PLAN,
                        role,
                        &item.plan,
                        resources,
                        &classification,
                    )
                }),
                priority: item.priority,
            })
            .collect()
    }
}

/// Where the node's approval effects go.
///
/// Borrows of what the loop already owns, never copies: an event published through this
/// has to be the event the journal appends this tick, and an audit entry has to reach the
/// log the node keeps.
pub struct NodeHost<'a> {
    bus: &'a InProcessBus,
    audit: &'a mut InMemoryAuditLog,
    /// Who the audit log attributes an entry to, and `None` for an act nobody took -- a
    /// sweep's expiry, or a plan's submission (DN-23 §5 rule 1).
    operator: Option<OperatorId>,
    now: MissionTime,
    endpoints: &'a [gungnir_config::EndpointConfig],
    endpoint_client: Option<&'a EndpointClient>,
}

impl ApprovalHost for NodeHost<'_> {
    /// Publish on the node's bus, which is what its journal appends this tick.
    ///
    /// A failed publish is logged rather than returned, as the trait requires: nothing the
    /// desk could do about it would be safe, and the loop's own `?`-propagated publishes
    /// are the ones that stop the node.
    fn publish(&mut self, at: MissionTime, event: Event) {
        if let Err(err) = self.bus.publish(at, event) {
            tracing::error!(%err, "the approval desk could not publish an event");
        }
    }

    /// **A node has no console**, so an alert is a log line at warning level rather than a
    /// sentence on a strip. Said rather than dropped: these are the sentences that explain
    /// a queue nobody is deciding, and a node that swallowed them would be the quietest
    /// place a supersession could hide.
    fn alert(&mut self, message: String) {
        tracing::warn!(%message, "approval");
    }

    fn audit(&mut self, action: &str, detail: String) {
        self.audit.record(AuditEntry {
            operator: self.operator,
            action: action.to_owned(),
            mission_time: self.now.0,
            detail,
        });
    }

    /// **A no-op on a node, deliberately** (DN-31 §6.5, GAP-137).
    ///
    /// The node's exchange register for handoffs is written by the desktops that hold
    /// them (DN-18 §5 amendment 2, GAP-065), and `publish_exchange` replaces a set rather
    /// than adding to it, so two writers would each silently overwrite the other.
    ///
    /// **Corrected 2026-09-17 (GAP-133).** This said the second writer was a linked
    /// desktop issuing handoffs for a node's plan, and that GAP-133 would remove it.
    /// GAP-133 has: a linked desktop queues no node plan, so it records no decision on
    /// one, and `ApprovalDesk::issue_for` -- the only caller of this trait method -- is
    /// reached from `decide_for` alone. A desktop that is linked and has taken no local
    /// decision never writes the register at all.
    ///
    /// **What still stands in the way is a different writer.** A desktop that falls back
    /// keeps its `NodeLink` (`gungnir-app/src/failover.rs`, `fall_back`), decides on its
    /// own queue while cut off, and queues its whole handoff set on the link's exchange
    /// outbox; the batch is delivered on reconnect and replaces whatever the node holds.
    /// Wiring the node's set in now would make it the set that disappears the first time
    /// any desktop recovers from an outage -- the same failure, found one layer along.
    /// Giving the register a producer per writer is a `gungnir-api` write path and a
    /// DN-18 amendment, so GAP-137 stays open rather than being half-wired here.
    fn republish_handoffs(&mut self, _handoffs: &[HandoffRecord]) {}
}

impl HandoffTransport for NodeHost<'_> {
    fn address_for(&self, endpoint: &str) -> Result<String, String> {
        gungnir_approval::http_address_in(self.endpoints, self.endpoint_client.is_some(), endpoint)
    }

    fn post(&self, address: &str, payload: serde_json::Value) -> Option<Box<dyn HandoffInFlight>> {
        self.endpoint_client
            .map(|client| Box::new(Posted(client.post_json(address, payload))) as Box<_>)
    }
}

/// One post in flight on `gungnir-remote`'s endpoint client.
///
/// The same wrapper the desktop's host has, and for the same reason: the library may not
/// name the client (D-57 refused `gungnir-approval` → `gungnir-remote`, DN-31 §4).
#[derive(Debug)]
struct Posted(PendingDelivery);

impl HandoffInFlight for Posted {
    fn poll(&self) -> Option<DeliveryAnswer> {
        self.0.poll().map(|outcome| match outcome {
            DeliveryOutcome::Accepted { status } => DeliveryAnswer::Accepted { status },
            DeliveryOutcome::Refused { status, body } => DeliveryAnswer::Refused { status, body },
            DeliveryOutcome::Unreachable { reason } => DeliveryAnswer::Unreachable { reason },
        })
    }
}

/// What the loop already has when it calls into the desk.
///
/// Passed as one struct rather than eight arguments, and read once per call: a decision and
/// the engagement it opens are one act and must carry one mission time.
pub struct Frame<'a> {
    pub now: MissionTime,
    pub config: &'a ConfigBaseline,
    pub tracks: &'a [TrackView],
    pub resources: &'a [ResourceView],
    pub geofences: &'a dyn gungnir_policy::GeoService,
    pub bus: &'a InProcessBus,
    pub endpoint_client: Option<&'a EndpointClient>,
}

/// **The role a node acts in when nobody is asking.**
///
/// Only ever the *asking* role of a chain the ladder walk immediately overrides
/// ([`gungnir_approval::offer_to`] asks about every role on the ladder), and the role a
/// handoff's attribution carries for a decision taken with no session -- which no route
/// on this node allows. The lowest rank that may decide at all, so that if it were ever
/// read straight it would claim the least.
const NOBODY_ROLE: Role = Role::Operator;

/// Build the context and the host for one call, and run `f`.
///
/// The clock, the role and the session are read before the state is split, each once, for
/// the reason the desktop's own `with_desk` gives: a decision and the engagement it opens
/// are one act and must carry one mission time, and the operator and the role of a
/// decision come from one session (DN-23 §5 rule 1).
fn with_desk<T>(
    approval: &mut NodeApproval,
    frame: &Frame<'_>,
    signed_in: Option<(OperatorId, Role)>,
    acting: Role,
    f: impl FnOnce(&mut ApprovalDesk, &ApprovalContext<'_>, &PolicyInputs<'_>, &mut NodeHost<'_>) -> T,
) -> T {
    // The fires engine can only place a friendly track in a declared frame
    // (DN-05 §5 rule 1); a deployment with no origin places none, and the chain's caveats
    // say so rather than the engine inventing positions.
    let friendly =
        gungnir_approval::friendly_positions(frame.tracks, frame.config.local_frame().as_ref());
    let cx = ApprovalContext {
        now: frame.now,
        role: acting,
        signed_in: signed_in.map(|(operator, role)| SignedIn {
            operator: operator.0.to_string(),
            role: format!("{role:?}"),
        }),
        tracks: frame.tracks,
        resources: frame.resources,
        config: frame.config,
        // **A node applies no baseline while it runs**: the baseline it opened with is the
        // baseline in force, so nothing it produces is superseded (DN-08 §5). The window
        // rule that decides this on a desktop stays the desktop's, in `status.rs`, which
        // is the one place it lives.
        baseline_supersedes_plans: false,
        // **A node never loses its node**, so D-15's lapse never applies here: the
        // delegations the baseline configures are the delegations in force. The lapse is
        // a cut-off desktop's (DN-31 §6.7).
        delegations: gungnir_policy::Delegations::AsConfigured,
    };
    let policy = PolicyInputs {
        geofences: frame.geofences,
        friendly_positions: friendly.as_deref(),
    };
    let NodeApproval { desk, audit, .. } = approval;
    let mut host = NodeHost {
        bus: frame.bus,
        audit,
        operator: signed_in.map(|(operator, _)| operator),
        now: frame.now,
        endpoints: &frame.config.endpoints,
        endpoint_client: frame.endpoint_client,
    };
    f(desk, &cx, &policy, &mut host)
}

/// Offer a freshly proposed plan to the lowest role that may take it (DN-31 §6.1, §6.2).
///
/// The whole chain runs, for every role on the ladder, and the item is queued for the
/// first role that holds authority for every solution. A plan no role may accept is
/// `Denied { Authority }`, published with the engines that ran, and never queued --
/// which is DN-09 §7's "what must go up" finally having somewhere to go (GAP-113).
///
/// Called in the tick that proposes the plan, so `Queued` reaches the journal and the
/// stream in that tick (MOP-07, DN-31 §6.9).
pub fn propose(approval: &mut NodeApproval, frame: &Frame<'_>, plan: PlanView) -> Submitted {
    with_desk(
        approval,
        frame,
        None,
        NOBODY_ROLE,
        |desk, cx, policy, host| desk.submit_to_ladder(cx, policy, host, plan),
    )
}

/// Expiry and escalation on the node's clock (DN-31 §6.4), and the handoff deliveries
/// that are owed.
///
/// Every tick. Nothing ends silently: an expiry leaves a `DecisionRecord` that is not
/// actionable and names no operator, escalation adds the next role without removing the
/// first, and both reach the bus.
pub fn sweep(approval: &mut NodeApproval, frame: &Frame<'_>) {
    with_desk(
        approval,
        frame,
        None,
        NOBODY_ROLE,
        |desk, cx, _policy, host| {
            desk.sweep(cx, host);
            desk.sweep_engagements(cx, host);
            desk.sweep_handoffs(host, cx.now);
        },
    );
}

/// Take the decisions the routes accepted, in arrival order, and answer each
/// (DN-31 §6.3).
///
/// **This is where "the first valid decision wins" is decided.** The requests arrive in
/// the order the transport accepted them and are taken in that order by one loop, so two
/// operators racing on one item produce one `DecisionRecord` and one `409` naming it,
/// rather than two records and a question about which happened.
///
/// # Errors
///
/// Only a publish that failed on the node's own bus, which stops the node as every other
/// drain in the loop does.
pub fn answer_decisions(
    approval: &mut NodeApproval,
    frame: &Frame<'_>,
    api: &NodeApi,
) -> Result<(), Box<dyn std::error::Error>> {
    for pending in api.take_decisions() {
        let answer = answer_one(approval, frame, &pending);
        // A dropped receiver means the route's reply window closed; the decision still
        // happened and is on the record, which is exactly what the `504` tells the client
        // to come back and find out (DN-31 §6.3).
        let _ = pending.reply.send(answer);
    }
    Ok(())
}

/// One decision request, answered.
fn answer_one(
    approval: &mut NodeApproval,
    frame: &Frame<'_>,
    pending: &PendingDecision,
) -> DecisionAnswer {
    // A request key already in the history is answered with the outcome it produced, and
    // records nothing: that is what makes a retry after a `504` safe rather than a second
    // decision (DN-31 §6.3). Asked before the item is looked at, because the item the
    // first request decided has left the queue.
    if let Some(record) = approval.desk.approvals.for_request(&pending.request) {
        tracing::info!(
            item = %pending.item,
            decision = %record.id,
            "a repeated request key was answered with its first outcome"
        );
        return DecisionAnswer::Recorded(record.id);
    }
    let decision = match &pending.choice {
        v3::DecisionChoice::Accept => OperatorDecision::Accepted,
        v3::DecisionChoice::Override => OperatorDecision::Overridden,
        v3::DecisionChoice::Reject { reason } => OperatorDecision::Rejected {
            reason: reason.clone(),
        },
    };
    // The identifier the route verified, carried whole: nothing is parsed here, so there
    // is no failing branch to answer by attributing the decision to somebody else.
    let signed_in = Some((pending.operator, pending.role));
    let taken = with_desk(
        approval,
        frame,
        signed_in,
        pending.role,
        |desk, cx, _policy, host| {
            desk.decide_for(
                cx,
                host,
                pending.item,
                decision,
                Some(pending.request.clone()),
            )
        },
    );
    match taken {
        Ok(decision) => DecisionAnswer::Recorded(decision),
        // Nothing in the queue answers to that item: it was decided, it expired, or it
        // was never issued here. The append-only history tells the three apart.
        Err(CommandError::NotFound(item)) => {
            let (answer, detail) = already_ended(approval, item);
            audit_refusal(approval, frame, Some(pending.operator), &detail);
            answer
        }
        Err(err) => {
            tracing::error!(%err, item = %pending.item, "the queue refused a decision");
            audit_refusal(
                approval,
                frame,
                Some(pending.operator),
                &format!("item {}: {err}", pending.item),
            );
            DecisionAnswer::Unknown
        }
    }
}

/// What became of an item the queue no longer holds, and what the audit entry says about
/// it (DN-31 §6.3).
///
/// Read from the append-only history, which is the only thing that can tell the three
/// cases apart: a decision that stands, a window that closed, and an identifier this node
/// never issued. Returned as a pair rather than recorded here, because the record is
/// borrowed out of the desk and writing the entry needs the desk back.
fn already_ended(
    approval: &NodeApproval,
    item: gungnir_model::PendingApprovalId,
) -> (DecisionAnswer, String) {
    let Some(record) = approval.desk.approvals.outcome_for(item) else {
        return (
            DecisionAnswer::Unknown,
            format!("item {item}: no such queue item on this node"),
        );
    };
    let refused = match &record.decision {
        OperatorDecision::Expired { at } => v3::DecisionRefused::Expired { at: *at },
        // **The decision that stands, named in full** (DN-31 §6.3): who decided, as which
        // role and when, so PN-07 can say it rather than only that it was too late.
        OperatorDecision::Accepted
        | OperatorDecision::Overridden
        | OperatorDecision::Rejected { .. } => v3::DecisionRefused::AlreadyDecided {
            decision: record.id,
            operator: record.operator_id.clone(),
            role: record.role.clone(),
            at: record.mission_time,
        },
    };
    let detail = format!("item {item}: {refused:?}");
    (DecisionAnswer::Refused(refused), detail)
}

/// Put each forwarded outage on the node's record, whole or not at all (DN-31 §6.8;
/// GAP-134).
///
/// **Checked first, applied second.** Every record in the batch is compared with what the
/// node already holds under its identifier, and every settlement with what the node holds
/// for its plan, before anything is written. A contradiction anywhere refuses the whole
/// batch `409` naming what stands, and nothing is applied: a desktop that sees a refusal
/// has left the node holding none of its outage, never the part that came before the
/// record that disagreed.
///
/// Then each record goes through [`ApprovalDesk::admit_forwarded`] -- recorded once,
/// `Decided` published with its `origin`, one audit entry -- and each settlement not yet
/// on the journal is published under the event the forwarding desktop journaled it as.
/// **Nothing is queued, engaged or handed off**: the desktop did those while it was cut
/// off, and doing them again here would be the double engagement D-58 reports.
pub fn answer_forwarded(approval: &mut NodeApproval, frame: &Frame<'_>, api: &NodeApi) {
    for pending in api.take_forwarded() {
        let answer = take_forwarded(approval, frame, &pending);
        // A dropped receiver is a route whose window closed. What was applied stays
        // applied, and the client sends the batch again to learn that it was.
        let _ = pending.reply.send(answer);
    }
}

/// One forwarded batch, answered.
fn take_forwarded(
    approval: &mut NodeApproval,
    frame: &Frame<'_>,
    pending: &PendingForward,
) -> ForwardAnswer {
    let records: Vec<(DecisionRecord, Option<v3::Settlement>)> = pending
        .decisions
        .iter()
        .map(|f| (record_of(f), f.settled.clone()))
        .collect();
    if let Some(refused) = contradiction(approval, &records) {
        tracing::warn!(
            ?refused,
            "a forwarded outage contradicts the node's record; nothing applied"
        );
        audit_refusal(
            approval,
            frame,
            Some(pending.operator),
            &format!("forwarded decisions: {refused:?}"),
        );
        return ForwardAnswer::Refused(refused);
    }
    let mut accepted = v3::ForwardAccepted {
        recorded: 0,
        already_held: 0,
        settled: 0,
    };
    for (record, settled) in records {
        let plan = record.plan.id;
        let outcome = with_desk(
            approval,
            frame,
            Some((pending.operator, pending.role)),
            pending.role,
            |desk, cx, _policy, host| desk.admit_forwarded(cx, host, record),
        );
        match outcome {
            ForwardOutcome::Recorded => accepted.recorded += 1,
            ForwardOutcome::AlreadyHeld => accepted.already_held += 1,
            // Checked above against the same history, in the same tick, by the one loop
            // that writes it; reaching this would mean the check and the write disagree.
            ForwardOutcome::Contradicts(held) => {
                tracing::error!(decision = %held.id, "a forwarded decision contradicted the record after the check passed");
            }
        }
        if let Some(settlement) = settled {
            if approval.settlements.contains_key(&plan) {
                continue;
            }
            publish_settlement(frame, plan, &settlement);
            approval.settlements.insert(plan, settlement);
            accepted.settled += 1;
        }
    }
    tracing::info!(
        recorded = accepted.recorded,
        already_held = accepted.already_held,
        settled = accepted.settled,
        "a forwarded outage is on the record"
    );
    ForwardAnswer::Accepted(accepted)
}

/// The node's record of a forwarded decision: the forwarding machine's own, field for
/// field, with the machine it came from.
///
/// The route has already refused a verdict no queue could have produced, so the verdict
/// here is the one every queued item carries.
fn record_of(forwarded: &v3::ForwardedDecision) -> DecisionRecord {
    let r = &forwarded.record;
    DecisionRecord {
        id: r.decision,
        item: r.item,
        plan: r.plan.clone(),
        verdict: gungnir_policy::PolicyVerdict::RequiresHumanApproval,
        decision: match &r.choice {
            v3::DecisionChoice::Accept => OperatorDecision::Accepted,
            v3::DecisionChoice::Override => OperatorDecision::Overridden,
            v3::DecisionChoice::Reject { reason } => OperatorDecision::Rejected {
                reason: reason.clone(),
            },
        },
        operator_id: r.operator.clone(),
        role: r.role.clone(),
        request: r.request.clone(),
        origin: Some(forwarded.origin.clone()),
        mission_time: r.at,
    }
}

/// The first thing in a batch that contradicts the node's record, or an earlier element of
/// the same batch, if anything does.
fn contradiction(
    approval: &NodeApproval,
    records: &[(DecisionRecord, Option<v3::Settlement>)],
) -> Option<v3::ForwardRefused> {
    let mut seen: Vec<&DecisionRecord> = Vec::new();
    let mut settled: std::collections::BTreeMap<gungnir_model::PlanId, &v3::Settlement> =
        std::collections::BTreeMap::new();
    for (record, settlement) in records {
        let held = approval
            .desk
            .approvals
            .records()
            .iter()
            .find(|r| r.id == record.id)
            .or_else(|| seen.iter().copied().find(|r| r.id == record.id));
        if let Some(held) = held {
            if held != record {
                return Some(match view_of(held) {
                    Some(view) => v3::ForwardRefused::Contradicts {
                        decision: held.id,
                        held: view,
                    },
                    None => v3::ForwardRefused::ContradictsAnExpiry {
                        decision: held.id,
                        plan: held.plan.id,
                        at: held.mission_time,
                    },
                });
            }
        }
        seen.push(record);
        if let Some(settlement) = settlement {
            let plan = record.plan.id;
            let held = approval
                .settlements
                .get(&plan)
                .or(settled.get(&plan).copied());
            if let Some(held) = held {
                if held != settlement {
                    return Some(v3::ForwardRefused::SettledOtherwise {
                        plan,
                        held: held.clone(),
                    });
                }
            }
            settled.insert(plan, settlement);
        }
    }
    None
}

/// A record as the wire carries it, for a `409` that names what stands, and `None` for an
/// expiry: a record view carries a person's choice and an expiry is not one (DN-10 §3).
fn view_of(record: &DecisionRecord) -> Option<v3::DecisionRecordView> {
    let choice = match &record.decision {
        OperatorDecision::Accepted => v3::DecisionChoice::Accept,
        OperatorDecision::Overridden => v3::DecisionChoice::Override,
        OperatorDecision::Rejected { reason } => v3::DecisionChoice::Reject {
            reason: reason.clone(),
        },
        OperatorDecision::Expired { .. } => return None,
    };
    Some(v3::DecisionRecordView {
        decision: record.id,
        item: record.item,
        plan: record.plan.clone(),
        verdict: record.verdict.summary(),
        choice,
        operator: record.operator_id.clone(),
        role: record.role.clone(),
        request: record.request.clone(),
        at: record.mission_time,
    })
}

/// Put a forwarded settlement on the node's journal under the event its desktop journaled
/// it as, so the two records say one sentence (MT-10 step 5).
///
/// **Not audited here.** A rule's verdict is nobody's act (`gungnir-app`'s failover says why
/// it is journaled and not audited), and a person's resolution was audited where the person
/// made it; the node's audit entry for the batch is the forwarding, one per decision.
fn publish_settlement(frame: &Frame<'_>, plan: gungnir_model::PlanId, settlement: &v3::Settlement) {
    let event = match settlement.clone() {
        v3::Settlement::Rule {
            kept_local,
            ground,
            local,
            remote,
        } => LinkEvent::ConflictArbitrated {
            plan,
            kept_local,
            ground,
            local,
            remote,
            at: frame.now,
        },
        v3::Settlement::Person {
            kept_local,
            operator,
        } => LinkEvent::ConflictResolved {
            plan,
            kept_local,
            operator,
            at: frame.now,
        },
    };
    if let Err(err) = frame.bus.publish(frame.now, Event::Link(event)) {
        tracing::error!(%err, %plan, "a forwarded settlement could not be published");
    }
}

/// Write the audit entries the routes owe for the requests they refused
/// (DN-31 §9 row 4).
///
/// Every decision **and every refusal** writes exactly one entry on the node. The four
/// pre-loop checks refuse before the loop sees anything, so the route records what it
/// refused and the loop -- which owns the log -- writes the entry.
pub fn audit_refused_decisions(approval: &mut NodeApproval, frame: &Frame<'_>, api: &NodeApi) {
    for refusal in api.take_refused_decisions() {
        let RefusedDecision {
            item,
            operator,
            reason,
        } = refusal;
        let who = operator.parse().ok().map(OperatorId);
        let named = item.map_or_else(|| "no item".to_string(), |i| format!("item {i}"));
        audit_refusal(approval, frame, who, &format!("{named}: {reason}"));
    }
}

/// One refusal on the record, under the action it was refused for.
///
/// `plan.decide` rather than an action of its own: an auditor asking what was done about
/// plan decisions on this node must find the refusals beside the decisions, not in a
/// second place they would have to know to look (contract C-04).
fn audit_refusal(
    approval: &mut NodeApproval,
    frame: &Frame<'_>,
    operator: Option<OperatorId>,
    detail: &str,
) {
    approval.audit.record(AuditEntry {
        operator,
        action: actions::DECIDE_PLAN.to_owned(),
        mission_time: frame.now.0,
        detail: format!("refused: {detail}"),
    });
}
