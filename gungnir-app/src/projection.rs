// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Whose approval queue is in force, and the node's queue while it is the node's
//! (GAP-133, D-55; `docs/design/DN-31-node-approval-queue.md` §6, whose clauses DN-31
//! §6.5 and §6.6 are what this module builds).
//!
//! # Where the two paths part, and why there
//!
//! [`in_force`] is the boundary, and the tick asks it once, at the step where a fresh
//! plan would have been queued. **While a desktop is linked the node holds the queue**,
//! so the desktop submits nothing; while it is cut off, or deployed with no node at all,
//! its own [`crate::desk`] does everything it did before, untouched.
//!
//! The boundary sits at submission and not at deciding, at engaging or at handing off,
//! because those three are downstream of a recorded decision and of nothing else:
//! `gungnir_approval::ApprovalDesk::decide_for` is the only thing that opens an
//! engagement, and it opens one only from a `DecisionRecord` that `is_actionable`
//! (contract C-01, `gungnir-app/tests/no_execution_without_decision.rs`). A desktop that
//! never queues a node plan therefore never records a decision on one, never opens an
//! engagement for one and never issues a handoff for one -- **DN-31 §6.5's one issuer is
//! a consequence of one queue, not a second rule bolted beside it.** A filter inside
//! `decisions::submit` would have been the opposite: three places to keep in step, each
//! able to drift.
//!
//! # What a linked desktop does instead
//!
//! It projects. `gungnir-remote`'s link keeps the node's queue up to date from the
//! node's own picture and event stream (`gungnir_remote::queue`), [`tick`] copies it into
//! the state once per frame, and PN-06 draws it. A decision goes out through
//! [`decide`] to `POST /v3/queue/{item}/decision` and the node's answer comes back
//! through the same link; nothing about it is recorded here.
//!
//! # The request key
//!
//! `gungnir-app/{operator}/{item}`. It is **derived rather than minted**, which is the
//! whole point: a retry after a `504` has to carry the same key, and a key nothing has
//! to remember cannot be forgotten by a process that restarted between the post and the
//! retry. Two consoles with different operators signed in produce different keys for the
//! same item, so each is its own request. Two consoles with the *same* operator signed in
//! produce the same key on purpose -- that is one person deciding one item, and the node
//! answering the second with the first's outcome is exactly the idempotency the key
//! exists for (DN-31 §5.2, §6.3).
//!
//! With nobody signed in there is no key, and no decision: the node's route needs a
//! token, so a linked desktop cannot take a decision under DN-23 §5 rule 5's
//! role-selected fallback at all. That is stricter than the cut-off desktop, which may
//! decide with nobody signed in and records no role for it (D-53), and PN-06 and PN-07
//! say which of the two an operator is looking at.

use crate::state::AppState;
use gungnir_config::BackendConfig;
use gungnir_model::{MissionTime, PendingApprovalId};
use gungnir_remote::queue::{
    DecisionAnswer, DecisionOutcome, Ended, OutboundDecision, QueueSnapshot,
};
use gungnir_ui::panels::approval_queue::{
    PendingId, QueueAuthority, QueueRow, TimeRemaining, Verdict,
};

/// Whose approval queue this desktop is showing and deciding in (DN-31 §6.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueInForce {
    /// **The node's.** This desktop shows the node's queue and decides through the
    /// node's route. It queues nothing, opens no engagement and issues no handoff for a
    /// plan the node proposed.
    Node { endpoint: String },
    /// **This desktop's own**, because it is cut off from its node or was deployed with
    /// none. Everything DN-31 §9 row 10 pins is unchanged: it queues, decides, engages
    /// and hands off locally.
    ThisDesktop,
}

/// Which queue is in force right now.
///
/// Read from `state.backend`, which is the one field the failover switch moves: it is
/// `Remote` while the link is up and `Embedded` from the moment `failover::fall_back`
/// runs until `failover::switch_back` succeeds, and a desktop deployed on its own is
/// `Embedded` for its whole life. A desktop whose backend is `Remote` and whose link is
/// gone for good would be answered `Node` here for at most one heartbeat timeout, after
/// which the failover has moved the backend; until then PN-06 shows the queue it last
/// had with the link's own freshness beside it on PN-01, which is what `last_heard_age`
/// is drawn for.
#[must_use]
pub fn in_force(state: &AppState) -> QueueInForce {
    match &state.backend {
        BackendConfig::Remote { endpoint } => QueueInForce::Node {
            endpoint: endpoint.clone(),
        },
        BackendConfig::Embedded => QueueInForce::ThisDesktop,
    }
}

/// Whether the node holds the queue right now.
#[must_use]
pub fn node_holds_the_queue(state: &AppState) -> bool {
    matches!(in_force(state), QueueInForce::Node { .. })
}

/// What this desktop knows about the node's queue, and what it has asked of it.
#[derive(Debug, Default)]
pub struct ProjectionState {
    /// The node's queue as of this tick, taken from the link once per frame rather than
    /// read under its lock by every panel that draws.
    pub queue: QueueSnapshot,
    /// Decisions posted and not yet answered, for PN-07 to say a decision is still with
    /// the node rather than leaving a dialog that looks as though nothing happened.
    pub in_flight: Vec<OutboundDecision>,
    /// The node's answer to the decision this desktop last posted, until the operator
    /// has closed it (DN-31 §6.6).
    pub answer: Option<DecisionOutcome>,
    /// The two clocks, taken together the last time the node said what its own was
    /// (GAP-140). `None` until a snapshot carrying one has been read, which is every
    /// desktop that has never linked and every node built before the field existed.
    pub node_clock: Option<NodeClock>,
}

/// The node's clock and this desktop's, read at one moment (GAP-140).
///
/// **A pair, not a difference.** Keeping both is what lets the countdown advance on this
/// desktop's own clock between snapshots while staying on the node's scale: the offset is
/// `node - ours` and it holds for as long as the two tick at the same rate, which is what
/// two clocks do between being set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NodeClock {
    /// What the node said its clock was.
    pub node: MissionTime,
    /// What this desktop's clock said when that was read.
    pub ours: MissionTime,
}

impl NodeClock {
    /// How far ahead of this desktop the node's clock is, in seconds; negative behind.
    #[must_use]
    pub fn skew_s(self) -> f64 {
        self.node.0 - self.ours.0
    }

    /// The node's clock now, from this desktop's own clock and the offset.
    #[must_use]
    pub fn node_now(self, ours_now: MissionTime) -> MissionTime {
        MissionTime(ours_now.0 + self.skew_s())
    }
}

/// How far two clocks may differ before PN-01 says so, in seconds (GAP-140).
///
/// **One second, because that is a digit.** A queue row's countdown is drawn to the
/// second, so a smaller difference cannot change what a person reads; a larger one means
/// the row and the node disagree about a deadline by something that is on the screen.
/// The countdown itself is drawn against the node's clock whatever the difference -- this
/// is only the threshold for telling somebody about it.
pub const CLOCK_SKEW_TOLERANCE_S: f64 = 1.0;

/// Copy the node's queue out of the link, and take its answers to what this desktop
/// posted. Called every tick.
///
/// A no-op with no link, which is every cut-off and every standalone desktop: the state
/// keeps whatever it last held, and [`in_force`] is what stops a panel reading it.
pub fn tick(state: &mut AppState) {
    let Some(link) = state.link.clone() else {
        return;
    };
    state.projection.queue = link.queue();
    state.projection.in_flight = link.decisions_in_flight();
    // GAP-140: the two clocks, read together the first frame that sees a new reading from
    // the node. Re-read only when the node's own value changes, which is once per
    // connection: taking it every frame would re-measure the offset against a snapshot
    // that has not moved and make it drift by exactly the age of that snapshot.
    if let Some(node) = link.node_time() {
        if state.projection.node_clock.is_none_or(|c| c.node != node) {
            state.projection.node_clock = Some(NodeClock {
                node,
                ours: state.clock.now(),
            });
        }
    }
    for outcome in link.take_decision_outcomes() {
        alert_for(state, &outcome);
        // The dialog is open on the item this answers; showing the answer is what closes
        // it (DN-31 §6.6), so the answer is held rather than logged and dropped.
        if state.selected_approval() == Some(PendingId(outcome.item.0)) {
            state.projection.answer = Some(outcome);
        }
    }
}

/// Say what the node answered, on the alert list as well as in the dialog.
///
/// A refusal and a rejection both reach the strip, because an operator who has moved on
/// from PN-07 still has to learn that the decision they took did not stand. A `201` says
/// nothing: the item leaving PN-06 is the acknowledgement, and an alert per accepted
/// decision would be an alert list nobody reads.
fn alert_for(state: &mut AppState, outcome: &DecisionOutcome) {
    let message = match &outcome.answer {
        DecisionAnswer::Recorded { .. } => return,
        DecisionAnswer::Refused(refused) => refusal_sentence(refused),
        DecisionAnswer::Rejected { status, reason } => format!(
            "The node would not take your decision on item {} ({status}): {reason}. \
             Nothing was recorded, here or there.",
            outcome.item.short()
        ),
    };
    state.alerts.push(message);
}

/// A `409` in words (DN-31 §6.6).
#[must_use]
pub fn refusal_sentence(refused: &gungnir_remote::queue::DecisionRefused) -> String {
    use gungnir_remote::queue::DecisionRefused as R;
    match refused {
        R::AlreadyDecided {
            decision,
            operator,
            role,
            at,
        } => {
            let who = match (operator.as_deref(), role.as_deref()) {
                (Some(operator), Some(role)) => format!("operator {operator} as {role}"),
                (Some(operator), None) => {
                    format!("operator {operator}, whose role was not recorded")
                }
                // Only a desktop's own queue records a decision with no operator
                // (DN-23 §5 rule 1); the node's route needs a token, so this is a
                // decision forwarded from a cut-off desktop.
                (None, _) => "nobody this node can name".to_owned(),
            };
            format!(
                "Already decided by {who} at T+{:.0} s; that decision stands (decision {}).",
                at.0,
                decision.short()
            )
        }
        R::Expired { at } => format!(
            "The window closed at T+{:.0} s with nobody deciding. That is not a rejection: \
             nobody chose.",
            at.0
        ),
    }
}

/// Take a decision on one of the node's queue items (DN-31 §6.3, §6.6).
///
/// # Errors
///
/// A sentence for the operator when the decision cannot even be posted: no link, or
/// nobody signed in. **Neither records anything locally**, which is the property that
/// separates this from the cut-off path.
pub fn decide(
    state: &mut AppState,
    item: PendingApprovalId,
    choice: gungnir_remote::queue::DecisionChoice,
) -> Result<(), String> {
    let Some(link) = state.link.clone() else {
        return Err("this desktop decides through its node, and no link is up".to_owned());
    };
    let Some(session) = state.signed_in() else {
        return Err(
            "nobody is signed in: the node authorizes every decision against the caller's \
             role, so a decision cannot be taken from this console until somebody signs in"
                .to_owned(),
        );
    };
    let key = gungnir_model::RequestId::new(format!("gungnir-app/{}/{item}", session.operator.0))
        .map_err(|e| format!("this desktop could not form a request key: {e}"))?;
    link.queue_decision(OutboundDecision {
        request: key,
        item,
        choice,
        attempts: 0,
    });
    Ok(())
}

/// PN-06's rows from the node's queue (DN-31 §6.6, §8).
///
/// The node's order, never re-sorted here: "both desktops show the same queue in the same
/// order" (DN-31 §9 row 7) is only true if neither of them decides the order.
///
/// `may_decide` is **both** halves of the question the node's route asks -- the role holds
/// `plan.decide`, and the item is offered to that role -- so a row is actionable here
/// exactly when the node would accept a decision on it. With nobody signed in no row is
/// actionable, because the route needs a token: that is a stricter rule than the cut-off
/// desktop's and PN-06 says so rather than enabling a control the node will refuse.
#[must_use]
pub fn queue_rows(state: &AppState) -> Vec<QueueRow<'_>> {
    // GAP-140: the node's clock, where this desktop knows it. These deadlines are the
    // node's, and drawing them against this console's clock is wrong by the difference.
    let now = node_now(state);
    let signed_in = state.signed_in();
    let role_name = signed_in.as_ref().map(|s| format!("{:?}", s.role));
    let may_decide = signed_in.as_ref().is_some_and(|s| {
        gungnir_security::authz::role_permits(s.role, crate::decisions::DECISION_ACTION)
    });
    state
        .projection
        .queue
        .items()
        .iter()
        .map(|item| QueueRow {
            id: PendingId(item.item.0),
            plan_id: item.plan.id,
            assignments: item.plan.assignments().len(),
            verdict: Verdict::RequiresHumanApproval,
            time_remaining: match seconds_remaining(item.expires_at, now) {
                Some(left) => TimeRemaining::Seconds(left),
                // The node's governing layer configures no expiry, which DN-10 makes
                // mean the item is preserved until somebody decides it.
                None => TimeRemaining::NoExpiryConfigured,
            },
            // The node's answer, not a second computation of it against this desktop's
            // baseline: D-15's delegation is the deployment's and the node is where it
            // was asked (DN-31 §5.2).
            pre_delegated: item.pre_delegated,
            may_decide: may_decide
                && role_name
                    .as_ref()
                    .is_some_and(|role| item.offered_to.iter().any(|r| r == role)),
            offered_to: &item.offered_to,
            // Escalation adds a role without removing the first (DN-10 §5), so an item
            // offered to more than one role has been escalated, and the first is the one
            // it was submitted to.
            escalated_from: (item.offered_to.len() > 1)
                .then(|| item.offered_to.first().map(String::as_str))
                .flatten(),
        })
        .collect()
}

/// Where a decision taken on PN-07 goes (DN-31 §6.6).
#[must_use]
pub fn route(state: &AppState) -> gungnir_ui::panels::decision_dialog::DecisionRoute<'_> {
    use gungnir_ui::panels::decision_dialog::DecisionRoute;
    match &state.backend {
        BackendConfig::Remote { endpoint } => DecisionRoute::Node { endpoint },
        BackendConfig::Embedded => DecisionRoute::ThisDesktop,
    }
}

/// PN-07's answer line, owned for the frame (GAP-133, DN-31 §6.6).
///
/// Owned rather than a `NodeAnswer` because that borrows its sentence, and the sentence
/// is built here: returning a borrowed view of a string this function made would not
/// outlive the call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnswerLine {
    Waiting { attempts: u32 },
    Refused { sentence: String },
    Rejected { status: u16, reason: String },
}

/// What the node has said about the decision taken on this item, if anything.
///
/// `Waiting` while the post is still in the outbox, which a `504` can make last several
/// attempts; then the answer, which closes the dialog.
#[must_use]
pub fn answer_line(state: &AppState, item: PendingApprovalId) -> Option<AnswerLine> {
    if let Some(outcome) = state.projection.answer.as_ref() {
        if outcome.item == item {
            return Some(match &outcome.answer {
                // A `201` is not drawn: the item leaving PN-06 is the acknowledgement,
                // and the dialog is closed by the same tick that takes the answer.
                DecisionAnswer::Recorded { .. } => return None,
                DecisionAnswer::Refused(refused) => AnswerLine::Refused {
                    sentence: refusal_sentence(refused),
                },
                DecisionAnswer::Rejected { status, reason } => AnswerLine::Rejected {
                    status: *status,
                    reason: reason.clone(),
                },
            });
        }
    }
    state
        .projection
        .in_flight
        .iter()
        .find(|d| d.item == item)
        .map(|d| AnswerLine::Waiting {
            attempts: d.attempts,
        })
}

/// Why an item is no longer on PN-06, where this desktop saw it end (DN-31 §6.6).
///
/// `None` where it did not, which is honest rather than a guess: an item can leave the
/// node's queue while this desktop's link is down, and saying "somebody decided it" would
/// be a claim about a record this desktop has not read.
#[must_use]
pub fn ended_sentence(state: &AppState, item: PendingId) -> Option<String> {
    let ended = state
        .projection
        .queue
        .ended
        .iter()
        .find(|e| e.item == Some(PendingApprovalId(item.0)))?;
    Some(match &ended.ended {
        Ended::Decided {
            accepted,
            operator,
            role,
            at,
            ..
        } => {
            let who = match (operator.as_deref(), role.as_deref()) {
                (Some(operator), Some(role)) => format!("operator {operator} as {role}"),
                (Some(operator), None) => {
                    format!("operator {operator}, whose role was not recorded")
                }
                (None, _) => "somebody this node cannot name".to_owned(),
            };
            format!(
                "That item was {} by {who} at T+{:.0} s on the node.",
                if *accepted { "accepted" } else { "rejected" },
                at.0
            )
        }
        Ended::Expired { at } => format!(
            "That item's window closed at T+{:.0} s with nobody deciding. That is not a \
             rejection: nobody chose.",
            at.0
        ),
    })
}

/// What PN-06 says about whose queue this is.
#[must_use]
pub fn authority(state: &AppState) -> QueueAuthority<'_> {
    match &state.backend {
        BackendConfig::Remote { endpoint } => QueueAuthority::Node { endpoint },
        BackendConfig::Embedded => QueueAuthority::ThisDesktop,
    }
}

/// The decisions the node has taken on its queue that this desktop has seen, newest
/// first, for PN-06's *decided on the node* section (DN-31 §9 row 7).
#[must_use]
pub fn decided_rows(state: &AppState) -> Vec<gungnir_ui::panels::approval_queue::DecidedRow<'_>> {
    use gungnir_ui::panels::approval_queue::{DecidedBy, DecidedRow};
    state
        .projection
        .queue
        .ended
        .iter()
        .rev()
        .map(|ended| DecidedRow {
            plan: ended.plan,
            by: match &ended.ended {
                Ended::Decided {
                    accepted,
                    operator,
                    role,
                    at,
                    ..
                } => DecidedBy::Person {
                    accepted: *accepted,
                    operator: operator.as_deref(),
                    role: role.as_deref(),
                    at: *at,
                },
                Ended::Expired { at } => DecidedBy::Expiry { at: *at },
            },
        })
        .collect()
}

/// How many of the node's queue items have expired and escalated in this period, and
/// what its decisions were by role, for PN-17 (DN-31 §8).
///
/// Counted off the projection rather than kept as a running total, so a commander reading
/// the panel is reading what this desktop has actually seen of the node's queue and not a
/// tally that could drift from it.
#[must_use]
pub fn node_queue_stats(state: &AppState) -> NodeQueueStats {
    let items = state.projection.queue.items();
    let mut counts = NodeQueueStats {
        pending: items.len(),
        escalated: items.iter().filter(|i| i.offered_to.len() > 1).count(),
        ..NodeQueueStats::default()
    };
    for ended in &state.projection.queue.ended {
        match &ended.ended {
            Ended::Decided { role, .. } => {
                counts.decided += 1;
                let role = role
                    .clone()
                    .unwrap_or_else(|| "no role recorded".to_owned());
                match counts.by_role.iter_mut().find(|(r, _)| *r == role) {
                    Some((_, count)) => *count += 1,
                    None => counts.by_role.push((role, 1)),
                }
            }
            Ended::Expired { .. } => counts.expired += 1,
        }
    }
    counts
}

/// The node's queue in the period, as PN-17 reports it (DN-31 §8).
///
/// **Escalations are not counted here.** An escalation is published as
/// `CommandEvent::Escalated` and the projection takes it as a reason to re-read the
/// node's queue rather than as a tally, so this desktop cannot claim a count it did not
/// keep; what it can say is which items are offered above the role that was first asked,
/// which is [`NodeQueueStats::escalated`] read off the queue itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NodeQueueStats {
    pub pending: usize,
    pub decided: usize,
    pub expired: usize,
    /// Items waiting that are offered to more than one role, so escalation has added at
    /// least one.
    pub escalated: usize,
    /// Decisions by the role that took them, in first-seen order. A decision the node
    /// recorded with no role -- one forwarded from a cut-off desktop where nobody was
    /// signed in (D-53) -- is counted under a name that says so.
    pub by_role: Vec<(String, usize)>,
}

/// PN-17's by-role lines, borrowed for the frame (DN-31 §8).
#[must_use]
pub fn decisions_by_role(
    stats: &NodeQueueStats,
) -> Vec<gungnir_ui::panels::commander_summary::DecisionsByRole<'_>> {
    stats
        .by_role
        .iter()
        .map(
            |(role, count)| gungnir_ui::panels::commander_summary::DecisionsByRole {
                role,
                count: *count,
            },
        )
        .collect()
}

/// Mission time is only meaningful beside a deadline, so this is where a row's countdown
/// is turned into one. Kept next to [`queue_rows`], its only caller, so the two cannot
/// come to read `expires_at` differently.
#[must_use]
pub fn seconds_remaining(expires_at: Option<MissionTime>, now: MissionTime) -> Option<f64> {
    expires_at.map(|at| at.0 - now.0)
}

/// **The clock a node's deadline means** (GAP-140): the node's, where this desktop has
/// been told what it is, and this desktop's own where it has not.
///
/// `QueueItemView::expires_at` is the node's mission time. Both machines run a wall
/// clock, so both are seconds since the epoch and they agree only as far as the two
/// machines do; a console a minute fast showed every item on the node's queue a minute
/// closer to expiry than it was, and nothing said so. The node refuses a late decision
/// either way (`409 Expired`, DN-31 §6.3) -- what was wrong was what the operator was
/// told, on the one countdown they work to under saturation.
#[must_use]
pub fn node_now(state: &AppState) -> MissionTime {
    let ours = state.clock.now();
    state
        .projection
        .node_clock
        .map_or(ours, |c| c.node_now(ours))
}

/// How far the node's clock is from this desktop's, in seconds, **and only when it is far
/// enough to matter** ([`CLOCK_SKEW_TOLERANCE_S`], GAP-140).
///
/// `None` where the two agree to within a second, and where this desktop has never been
/// told the node's clock at all: PN-01 says nothing rather than drawing a zero that would
/// claim the two were compared when they were not.
#[must_use]
pub fn clock_skew_s(state: &AppState) -> Option<f64> {
    state
        .projection
        .node_clock
        .map(NodeClock::skew_s)
        .filter(|skew| skew.abs() > CLOCK_SKEW_TOLERANCE_S)
}
