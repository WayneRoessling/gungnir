// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The node's approval queue as a linked desktop sees it, and the decisions it takes
//! through the node's route (GAP-133, D-55; `docs/design/DN-31-node-approval-queue.md`
//! §6.5 and §6.6).
//!
//! **While a desktop is linked the node owns the queue.** This module holds what the
//! desktop is allowed to have instead: a projection of the node's queue, and an outbox
//! for the decisions it takes through `POST /v3/queue/{item}/decision`. Nothing here
//! queues, expires, escalates, opens an engagement or issues a handoff -- those are the
//! node's, and a desktop that did any of them while linked would be the second issuer
//! §6.5 exists to forbid.
//!
//! # Which of the two sources is authoritative
//!
//! The queue reaches a desktop two ways, and they answer different questions.
//!
//! - **The picture** (`GET /v3/queue`) is the node's whole queue, in the node's own
//!   order, with every field computed once on the node: the plan, the verdict, the
//!   deadlines, the roles it is offered to, D-15's delegation flag and the priority. It
//!   is what makes a desktop that joins late able to see what is waiting instead of
//!   being blind until the next event.
//! - **The stream** (`CommandEvent::Queued`, `Escalated`, `Decided`, `Expired`) is what
//!   says *when* the queue moved, with no polling, and it is the only thing that carries
//!   **what became of an item**: who decided it, as which role and at what time. The
//!   picture cannot carry that, because an item that has been decided is no longer in
//!   it.
//!
//! So neither is a copy of the other, and the rule between them is:
//!
//! > **The picture is authoritative for membership and order. The stream is
//! > authoritative for an ending, always.**
//!
//! They can disagree in exactly one direction. A picture is taken at a moment on the
//! node and reaches this desktop later, so it can still list an item that has since been
//! decided; the stream's ending is then the newer fact and wins. The reverse cannot
//! happen: an item that has ended never returns to a queue, so a picture can never
//! contradict an ending, only lag behind one. [`NodeQueue::take_picture`] therefore
//! replaces the waiting list wholesale and then re-applies every ending already known,
//! which is what stops a reconnection's fresh picture from resurrecting a decided item.
//!
//! # Why the picture is taken after the subscription, and re-taken on a change
//!
//! `run_link` fetches `GET /v3/snapshot` *before* it opens the event stream, and
//! `SnapshotResponse` carries the same `queue` field. Reading it there would leave a
//! window: an item queued between the snapshot being served and the subscription taking
//! effect would be in neither, and would sit unseen until the next reconnection. A
//! picture taken **after** the subscribe frame has gone cannot miss one -- anything
//! queued before it is in the picture, anything after it is on the stream -- so that is
//! where the starting picture is taken, from the route rather than from the snapshot.
//! The two return the same value: `NodeApi::queue` reads the snapshot's own `queue`
//! field.
//!
//! A `Queued` or `Escalated` event then marks the projection stale and the link takes a
//! fresh picture at once. That is not polling -- nothing is asked for while the node's
//! queue is still -- and it is what keeps every field of a row the node's own single
//! computation. Building a row out of the stream alone would mean joining three events
//! (`PlanProposed` for the plan, `PlanEvaluated` for the verdict, `Queued` for the
//! deadlines) and *still* inventing `pre_delegated` and `priority` locally, which are
//! answers only the node's authority matrix and assessment can give.

use gungnir_model::events::CommandEvent;
use gungnir_model::{DecisionId, MissionTime, PendingApprovalId, PlanId, RequestId};

/// The three wire types a desktop reads and builds, re-exported rather than mirrored.
///
/// [`OutboundTask`](crate::link::OutboundTask) mirrors `SensorTaskRequest` field for
/// field because `gungnir-app` has no production edge to `gungnir-api` and a desktop
/// *producer* should name no type from it. These three are the other case: a
/// `QueueItemView` and a `DecisionRefused` are the **node's** answers, read back whole,
/// and a mirror of either would be a second description of what a route already defines
/// -- the `409` most of all, which is the sentence PN-07 shows a person. A re-export adds
/// no manifest edge and keeps one definition
/// (`docs/agentic-coding-standards.md` §1.2; the workspace rule against redefining a type
/// another crate owns).
pub use gungnir_api::v3::{DecisionChoice, DecisionRefused, QueueItemView};

/// The forwarded-decision wire types (GAP-134, DN-31 §5.2), re-exported for the same
/// reason as the three above: an outage's decisions are the **forwarding machine's own
/// record**, and the node's `202` and `409` are its answers, read back whole. A mirror of
/// any of them would be a second description of what the route defines, and the `409` is
/// the sentence PN-18 shows a person.
pub use gungnir_api::v3::{
    DecisionRecordView, ForwardAccepted, ForwardRefused, ForwardedDecision, Settlement,
};

/// How many ended items a link remembers, oldest dropped.
///
/// PN-06 shows what has lately been decided so that an operator at one console sees a
/// decision taken at another (DN-31 §9 row 7). It is a recent-history list and not a
/// record: the record is the node's journal, and this is bounded so a long watch cannot
/// grow it without limit.
pub const ENDED_CAPACITY: usize = 256;

/// What became of an item that has left the node's queue.
#[derive(Debug, Clone, PartialEq)]
pub enum Ended {
    /// A person decided it. **Named in full**, because DN-31 §6.6 asks PN-06 and PN-07
    /// to say who decided, as which role and when.
    Decided {
        decision: DecisionId,
        accepted: bool,
        /// `None` when the decision was taken with nobody signed in, which only a
        /// desktop's own queue allows (DN-23 §5 rule 1, D-53). Never invented.
        operator: Option<String>,
        role: Option<String>,
        at: MissionTime,
    },
    /// The window closed with nobody deciding. **Not a rejection**: nobody chose
    /// (DN-10 §6).
    Expired { at: MissionTime },
}

/// One item that has left the node's queue, and what became of it.
///
/// Keyed by plan, because `CommandEvent::Decided`, `Expired` and `Escalated` name a plan
/// and only `Queued` names the item. The queue item is carried as well where this
/// desktop knew which item the plan was waiting in, and is `None` where it never saw the
/// `Queued` -- a decision on an item that was already in the node's queue before this
/// link came up.
#[derive(Debug, Clone, PartialEq)]
pub struct EndedItem {
    pub plan: PlanId,
    pub item: Option<PendingApprovalId>,
    pub ended: Ended,
}

/// The node's queue, projected onto one desktop.
#[derive(Debug, Default)]
pub struct NodeQueue {
    /// The node's whole queue as of the last picture, in the node's order. **`None`
    /// until a picture has arrived**, which is a different claim from an empty queue and
    /// one PN-06 keeps apart: "nothing is waiting" and "this desktop has not been told
    /// what is waiting" are not the same sentence.
    waiting: Option<Vec<QueueItemView>>,
    /// What has ended since this link came up, oldest first, bounded by
    /// [`ENDED_CAPACITY`].
    ended: std::collections::VecDeque<EndedItem>,
    /// Items the node has answered `201` for on **this** desktop's own request, before
    /// the stream has said who decided them and when (DN-31 §6.3).
    ///
    /// A `201` is first-hand knowledge that an item is decided -- the node names the
    /// decision it recorded -- so the item stops being something waiting on a person the
    /// moment it arrives. It is *not* knowledge of the record: who decided, as which role
    /// and at what mission time are the node's to say and reach this desktop on the
    /// stream a moment later, so nothing here is written into [`NodeQueue::ended`] until
    /// they do. Bounded by [`ENDED_CAPACITY`] like the endings themselves.
    ///
    /// Without this the deciding console kept showing its own accepted item as waiting
    /// until the stream caught up -- one or two ticks in which a second click would earn
    /// a `409` naming the operator's own decision.
    settled: std::collections::VecDeque<PendingApprovalId>,
    /// The stream has said the node's queue moved and no picture has been taken since.
    stale: bool,
}

impl NodeQueue {
    /// Replace the waiting list with a freshly taken picture (DN-31 §6.6).
    ///
    /// Every ending already known is re-applied afterwards, so a picture taken before a
    /// decision reached this desktop cannot put the decided item back on PN-06. That is
    /// the whole of the rule between the two sources; see the module documentation.
    pub fn take_picture(&mut self, items: Vec<QueueItemView>) {
        self.waiting = Some(items);
        self.stale = false;
        let ended: Vec<PlanId> = self.ended.iter().map(|e| e.plan).collect();
        if let Some(waiting) = self.waiting.as_mut() {
            waiting.retain(|item| {
                !ended.contains(&item.plan.id) && !self.settled.contains(&item.item)
            });
        }
    }

    /// The node answered `201` for a decision this desktop posted (DN-31 §6.3).
    ///
    /// The item leaves PN-06 at once. What became of it -- who decided, as which role,
    /// at what mission time -- is not written here: only the node's record says that, and
    /// it arrives on the stream as `Decided`. Filling those in from this desktop's own
    /// session would be recording a claim about the node's record that this desktop had
    /// not read.
    pub fn recorded_here(&mut self, item: PendingApprovalId) {
        if let Some(waiting) = self.waiting.as_mut() {
            waiting.retain(|i| i.item != item);
        }
        if self.settled.contains(&item) {
            return;
        }
        if self.settled.len() >= ENDED_CAPACITY {
            self.settled.pop_front();
        }
        self.settled.push_back(item);
    }

    /// Fold one of the node's command events into the projection.
    ///
    /// `Queued` and `Escalated` change what is waiting and in what order, and the node
    /// is the one place those are computed, so they mark the projection stale and the
    /// link takes a fresh picture. `Decided` and `Expired` end an item **here and now**,
    /// from the event alone, because the outcome is what the picture cannot carry and
    /// what PN-06 has to show.
    ///
    /// `at` is the **envelope's** mission time. `CommandEvent::Decided` carries no time
    /// of its own -- the envelope it travels in is what timestamps it -- and there is one
    /// door here rather than two so that no caller can record a decision as taken at T+0
    /// by taking the shorter one. `Expired` does carry its own time and keeps it: the
    /// moment a window closed is the queue's own arithmetic, not the moment the event
    /// was published.
    pub fn note(&mut self, event: &CommandEvent, at: MissionTime) {
        match event {
            CommandEvent::Queued { .. } | CommandEvent::Escalated { .. } => self.stale = true,
            CommandEvent::Decided {
                plan,
                decision,
                accepted,
                operator,
                role,
                ..
            } => self.end(
                *plan,
                Ended::Decided {
                    decision: *decision,
                    accepted: *accepted,
                    operator: operator.clone(),
                    role: role.clone(),
                    at,
                },
            ),
            CommandEvent::Expired { plan, at } => self.end(*plan, Ended::Expired { at: *at }),
        }
    }

    /// Record an ending and drop the item from the waiting list.
    ///
    /// Idempotent on the plan: a stream that replayed an ending after a reconnection
    /// records it once, because an item can only end once and a second entry would make
    /// PN-06 show one decision twice.
    fn end(&mut self, plan: PlanId, ended: Ended) {
        if self.ended.iter().any(|e| e.plan == plan) {
            return;
        }
        let item = self
            .waiting
            .as_ref()
            .and_then(|w| w.iter().find(|i| i.plan.id == plan))
            .map(|i| i.item);
        if let Some(waiting) = self.waiting.as_mut() {
            waiting.retain(|i| i.plan.id != plan);
        }
        if self.ended.len() >= ENDED_CAPACITY {
            self.ended.pop_front();
        }
        self.ended.push_back(EndedItem { plan, item, ended });
        // An ending changes nothing about what is still waiting, so it does not on its
        // own justify a fresh picture; the node's next `Queued` or `Escalated` will.
    }

    /// Whether the stream has moved the queue since the last picture was taken.
    #[must_use]
    pub fn is_stale(&self) -> bool {
        self.stale
    }

    /// Forget the picture, keeping what has ended.
    ///
    /// Called when the link goes down: what the node was waiting on a moment ago is no
    /// longer something this desktop can claim to know, and PN-06 says it has not been
    /// told rather than showing a list that stopped being maintained. The endings stay,
    /// because a decision that happened stays happened.
    pub fn disconnected(&mut self) {
        self.waiting = None;
        self.stale = false;
    }

    /// The queue as of now, for one frame.
    #[must_use]
    pub fn snapshot(&self) -> QueueSnapshot {
        QueueSnapshot {
            waiting: self.waiting.clone(),
            ended: self.ended.iter().cloned().collect(),
        }
    }
}

/// The node's queue as one desktop frame sees it.
///
/// An owned copy taken once per tick rather than a borrow held across the frame: the
/// link task writes the projection from its own thread, and a panel that held the lock
/// while it drew would block it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct QueueSnapshot {
    /// What the node is waiting on, in the node's order, or `None` where this desktop
    /// has not been told -- before the first picture, and after the link goes down.
    pub waiting: Option<Vec<QueueItemView>>,
    /// What has ended since this link came up, oldest first.
    pub ended: Vec<EndedItem>,
}

impl QueueSnapshot {
    /// What became of this plan, if this desktop saw it end.
    #[must_use]
    pub fn outcome_for(&self, plan: PlanId) -> Option<&EndedItem> {
        self.ended.iter().find(|e| e.plan == plan)
    }

    /// The items waiting, or an empty slice where nothing has been received.
    ///
    /// For a caller that only wants to iterate; a caller that has to tell "nothing is
    /// waiting" from "nothing has been received" reads [`QueueSnapshot::waiting`].
    #[must_use]
    pub fn items(&self) -> &[QueueItemView] {
        self.waiting.as_deref().unwrap_or(&[])
    }
}

/// A decision this desktop has taken on the node's queue, on its way to the node
/// (DN-31 §6.3, §6.6).
#[derive(Debug, Clone, PartialEq)]
pub struct OutboundDecision {
    /// Chosen by this desktop and journaled by the node beside the decision, so a retry
    /// after a `504` is the same request rather than a second decision (DN-31 §5.2).
    /// **The key is what makes the retry safe**, and it is minted once when the operator
    /// clicks, never per attempt.
    pub request: RequestId,
    pub item: PendingApprovalId,
    pub choice: DecisionChoice,
    /// How many times this has been posted. Counted so a decision that keeps meeting a
    /// `504` is visible as a decision still in flight rather than as one that quietly
    /// never landed.
    pub attempts: u32,
}

/// What the node answered a decision with (DN-31 §6.3).
#[derive(Debug, Clone, PartialEq)]
pub enum DecisionAnswer {
    /// `201`: the decision this request recorded, or recorded before under the same key.
    Recorded { decision: DecisionId },
    /// `409`: the item takes no decision now, and why. PN-07 shows who decided, as which
    /// role and when, then closes (DN-31 §6.6).
    Refused(DecisionRefused),
    /// `400`, `401` or `403`: the node would not take it. **Nothing was recorded** --
    /// not on the node and not here (DN-31 §6.3, §6.6).
    Rejected { status: u16, reason: String },
}

/// One decision, answered.
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionOutcome {
    pub request: RequestId,
    pub item: PendingApprovalId,
    pub answer: DecisionAnswer,
}

/// An outage's decisions on their way to `POST /v3/decisions/forwarded` (GAP-134,
/// DN-31 §6.8).
///
/// **One outage, one batch, sent whole.** The node takes a batch whole or not at all, so
/// holding the outage as one value here is what keeps the two ends agreeing about what
/// "sent" means: there is no half-sent outage on either side.
#[derive(Debug, Clone, PartialEq)]
pub struct OutboundForward {
    pub decisions: Vec<ForwardedDecision>,
    /// How many times the batch has been posted. A batch that keeps meeting a `504` is
    /// shown as still in flight rather than as one that quietly never landed.
    pub attempts: u32,
}

/// What the node answered an outage's batch with (DN-31 §7).
#[derive(Debug, Clone, PartialEq)]
pub enum ForwardReply {
    /// `202`: the whole batch is on the node's record.
    Accepted(ForwardAccepted),
    /// `409`: it contradicts what the node holds, and **none of it was applied**. Boxed
    /// because it can carry a whole record, plan included.
    Refused(Box<ForwardRefused>),
    /// `400`, `401` or `403`: the node would not take it, and nothing was recorded.
    Rejected { status: u16, reason: String },
}

/// Whether a status means the decision should be posted again under the same key.
///
/// `504` is the one DN-31 §6.3 names: the loop did not answer inside the route's reply
/// window, **which does not mean nothing was recorded**. Retrying under the same key is
/// how the client learns which, and it is safe precisely because the key makes the
/// second post the same request. `503` is the node's "the loop is gone", which is the
/// same situation from the other side. Everything else is an answer and is delivered to
/// the caller.
#[must_use]
pub fn retry_under_same_key(status: u16) -> bool {
    matches!(status, 503 | 504)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::events::VerdictSummary;
    use gungnir_model::{EffectorLayer, PlanView};

    fn item(plan: u128, id: u128) -> QueueItemView {
        QueueItemView {
            item: PendingApprovalId(id),
            plan: PlanView {
                id: PlanId(plan),
                ..PlanView::default()
            },
            verdict: VerdictSummary::RequiresHumanApproval,
            layer: EffectorLayer::Point,
            submitted: MissionTime(1.0),
            expires_at: None,
            escalate_at: None,
            offered_to: vec!["Operator".into()],
            pre_delegated: false,
            priority: 0.0,
        }
    }

    fn decided(plan: u128) -> CommandEvent {
        CommandEvent::Decided {
            plan: PlanId(plan),
            decision: DecisionId(7),
            accepted: true,
            operator: Some("11".into()),
            role: Some("Operator".into()),
            verdict: VerdictSummary::RequiresHumanApproval,
            rationale: None,
            request: None,
            origin: None,
        }
    }

    /// Nothing received and nothing waiting are different claims, and PN-06 draws them
    /// differently: a desktop that has not been told is not a desktop with a calm queue.
    #[test]
    fn a_queue_never_received_is_not_an_empty_queue() {
        let mut q = NodeQueue::default();
        assert!(q.snapshot().waiting.is_none());
        q.take_picture(Vec::new());
        assert_eq!(q.snapshot().waiting, Some(Vec::new()));
    }

    /// The rule between the two sources, in the one direction they can disagree: a
    /// picture taken before a decision reached this desktop must not put the decided
    /// item back on the queue.
    #[test]
    fn a_stale_picture_never_resurrects_a_decided_item() {
        let mut q = NodeQueue::default();
        q.take_picture(vec![item(1, 100)]);
        q.note(&decided(1), MissionTime(12.0));
        assert!(q.snapshot().items().is_empty());

        // The node served this before it took the decision; it still lists the item.
        q.take_picture(vec![item(1, 100)]);
        assert!(
            q.snapshot().items().is_empty(),
            "an ending is monotone: a picture can lag behind one, never contradict it"
        );
        assert_eq!(q.snapshot().ended.len(), 1);
    }

    /// The console that decided an item must not go on showing it as waiting until the
    /// stream catches up: it has the node's `201` in hand, and a second click in that
    /// window would earn a `409` naming the operator's own decision.
    #[test]
    fn an_item_this_desktop_has_a_receipt_for_leaves_the_queue_at_once() {
        let mut q = NodeQueue::default();
        q.take_picture(vec![item(1, 100), item(2, 200)]);
        q.recorded_here(PendingApprovalId(100));
        assert_eq!(
            q.snapshot().items().len(),
            1,
            "the decided item is no longer waiting on a person"
        );
        assert!(
            q.snapshot().ended.is_empty(),
            "a 201 says an item was decided and not who decided it or when; only the \
             node's record says that, and it arrives on the stream"
        );
        // And a picture taken before the node's queue had caught up cannot bring it back.
        q.take_picture(vec![item(1, 100), item(2, 200)]);
        assert_eq!(q.snapshot().items().len(), 1);

        // The stream then supplies the record.
        q.note(&decided(1), MissionTime(9.0));
        assert_eq!(q.snapshot().ended.len(), 1);
    }

    /// A replayed stream after a reconnection must not show one decision twice.
    #[test]
    fn an_ending_seen_twice_is_recorded_once() {
        let mut q = NodeQueue::default();
        q.take_picture(vec![item(1, 100)]);
        q.note(&decided(1), MissionTime(12.0));
        q.note(&decided(1), MissionTime(12.0));
        assert_eq!(q.snapshot().ended.len(), 1);
    }

    /// The outcome carries who decided and when, because that is what the picture
    /// cannot say and what the other console has to read (DN-31 §9 row 7).
    #[test]
    fn an_ending_names_who_decided_and_when() {
        let mut q = NodeQueue::default();
        q.take_picture(vec![item(1, 100)]);
        q.note(&decided(1), MissionTime(12.0));
        let snapshot = q.snapshot();
        let outcome = snapshot.outcome_for(PlanId(1)).expect("the plan ended");
        assert_eq!(outcome.item, Some(PendingApprovalId(100)));
        match &outcome.ended {
            Ended::Decided {
                operator, role, at, ..
            } => {
                assert_eq!(operator.as_deref(), Some("11"));
                assert_eq!(role.as_deref(), Some("Operator"));
                assert_eq!(*at, MissionTime(12.0));
            }
            Ended::Expired { .. } => panic!("a decision is not an expiry"),
        }
    }

    /// `Queued` and `Escalated` are what move the queue, so they ask for a fresh
    /// picture; an ending changes nothing that is still waiting.
    #[test]
    fn only_a_move_of_the_queue_asks_for_a_fresh_picture() {
        let mut q = NodeQueue::default();
        q.take_picture(Vec::new());
        assert!(!q.is_stale());
        q.note(
            &CommandEvent::Queued {
                item: PendingApprovalId(1),
                plan: PlanId(1),
                layer: EffectorLayer::Point,
                offered_to: vec!["Operator".into()],
                expires_at: None,
                escalate_at: None,
            },
            MissionTime(2.0),
        );
        assert!(q.is_stale());
        q.take_picture(vec![item(1, 1)]);
        assert!(!q.is_stale());
        q.note(&decided(1), MissionTime(3.0));
        assert!(
            !q.is_stale(),
            "an ending is applied from the event; it needs no picture to confirm it"
        );
    }

    /// A link that goes down stops knowing what is waiting, and says so rather than
    /// leaving a list nothing maintains on screen. What ended stays ended.
    #[test]
    fn a_link_that_goes_down_forgets_the_queue_and_not_the_endings() {
        let mut q = NodeQueue::default();
        q.take_picture(vec![item(1, 100), item(2, 200)]);
        q.note(&decided(1), MissionTime(4.0));
        q.disconnected();
        let snapshot = q.snapshot();
        assert!(snapshot.waiting.is_none());
        assert_eq!(snapshot.ended.len(), 1);
    }

    /// `504` does not mean nothing was recorded, so it is retried under the same key
    /// rather than answered; a refusal is an answer and is delivered.
    #[test]
    fn only_an_unanswered_post_is_retried() {
        assert!(retry_under_same_key(504));
        assert!(retry_under_same_key(503));
        for answered in [201u16, 400, 401, 403, 404, 409] {
            assert!(
                !retry_under_same_key(answered),
                "{answered} is an answer; retrying it would ask a question already settled"
            );
        }
    }
}
