// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The client half of the v3 transport (GAP-041).
//!
//! `reqwest` for the snapshot and health requests, `tokio-tungstenite` for the event
//! stream, per D-18 and `agentic-coding-standards.md` §2.9.
//!
//! # Why a background task and a shared projection
//!
//! `TrackingService::poll` and `InterceptService::plan` are called from the desktop's
//! frame loop and are synchronous; the transport is asynchronous. So one task owns the
//! link and writes into a [`Projection`] both services read. The frame loop never blocks
//! on the network, which is the property that matters: a node that stops answering must
//! slow nothing on screen.
//!
//! # Connected means connected
//!
//! [`Projection::connected`] is set when a snapshot has actually been received, and
//! cleared the moment the stream ends or a request fails. It is never optimistic --
//! `connect` returning `Ok` means a task was spawned, not that a node answered, and
//! `is_healthy` keeps saying false until one does. A "connected" light that came on
//! because a URL parsed would be exactly the health flag AP-02 forbids.
//!
//! # TLS (GAP-060)
//!
//! An `http` endpoint is spoken in the clear, which the node serves on loopback only. An
//! `https` endpoint is spoken over mutual TLS: `reqwest` is built with the baseline's
//! trust roots and the desktop's client certificate, and the event stream is
//! `tokio-tungstenite` over a `tokio-rustls` stream built from the same material, since
//! that crate deliberately carries no TLS of its own (§2.9). An `https` endpoint with no
//! pinned roots is refused rather than trusted blindly, and one written as `http` is
//! never upgraded: what the operator wrote is what is spoken.

use gungnir_api::path;
use gungnir_api::routes;
use gungnir_api::v3::{
    ExchangeProduct, HistoryResponse, PublishExchangeRequest, SensorTaskRequest,
    SensorTaskResponse, SessionRequest, SessionResponse, SnapshotResponse, SubmitDetectionRequest,
    SubscribeRequest,
};
use gungnir_eventing::{Envelope, Event};
use gungnir_intercept_service::PlanView;
use gungnir_model::events::{HealthEvent, InterceptEvent, TrackingEvent};
use gungnir_model::{BearingRayView, DetectionView, PipelineStatsView};
use gungnir_tracking_service::TrackView;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, watch};

use crate::{LinkTls, RemoteEndpoint, RemoteError};
// The Sink and Stream halves of the WebSocket. Named here because sending and receiving
// a frame needs the traits in scope.
use futures_util::{SinkExt, StreamExt};

/// How long to wait between reconnection attempts.
///
/// The link retries rather than giving up: §8.4's store-and-forward assumes a desktop
/// that reconnects on its own, and an operator should not have to restart the
/// application because a node was rebooted.
const RECONNECT_DELAY: std::time::Duration = std::time::Duration::from_secs(2);

/// The node's heartbeat, re-exported so the desktop can judge a link's silence against it
/// without a manifest edge to `gungnir-api` (D-23). One owner for the number: this crate
/// already depends on the transport, and the binary depends on this crate.
pub use gungnir_api::transport::{HEARTBEAT_INTERVAL, HEARTBEAT_TIMEOUT};

/// What the link task has learned, and what the two services read.
#[derive(Debug, Default)]
pub struct Projection {
    pub tracks: Vec<TrackView>,
    pub plan: PlanView,
    /// Bearings the node's pipeline retained but matched to no track, as of the last
    /// snapshot (GAP-096's wire contract).
    ///
    /// **Refreshed at snapshot time only** -- the initial connection and each
    /// reconnect. Unlike `tracks` and `plan`, which the event stream also keeps live
    /// between snapshots (`Event::Tracking`'s two variants and `Event::Intercept`'s),
    /// no envelope variant carries a bearing or a counter update today, so a connected
    /// desktop's bearing picture ages until the next reconnect: a real answer, current
    /// as of the last time this link actually asked, not a live one.
    pub bearing_rays: Vec<BearingRayView>,
    /// The node pipeline's own bearing counters, as of the last snapshot (GAP-096's
    /// wire contract). Refreshed on the same cadence as `bearing_rays`, for the same
    /// reason.
    pub pipeline_stats: PipelineStatsView,
    /// True only once a node has answered. Cleared as soon as it stops.
    pub connected: bool,
    /// What the node last said about its own services (GAP-161): the snapshot's `health`
    /// on each connection, then every `HealthEvent::Changed` on the stream. `None` until
    /// a node has said anything at all.
    ///
    /// **Why the link keeps it.** A desktop's status strip draws the tracking pipeline
    /// and the planner as healthy or not, and on a linked desktop both are the node's.
    /// Until GAP-161 the two remote services answered `is_healthy` with whether the link
    /// was up, so a node whose tracker had stopped was shown to every linked operator as
    /// tracking -- the health flag `CLAUDE.md` forbids, on the path an operator relies on
    /// most. The node already published both; nothing here read them.
    ///
    /// Kept across a disconnect as the last thing the node said, and not read while the
    /// link is down: the services report unhealthy then on `connected` alone.
    pub node_health: Option<gungnir_model::SystemHealth>,
    /// When this link's task started asking (GAP-142).
    ///
    /// **So a node that has never answered can be judged silent.** `last_heard` is `None`
    /// until a snapshot lands, and a link that has never been heard is not a link that
    /// has gone quiet -- which left a desktop whose node was unreachable from the moment
    /// it signed in neither linked nor fallen back. Silence is measured from here until
    /// there is something later to measure it from.
    pub started: Option<std::time::Instant>,
    /// The node's own clock as of the last snapshot (GAP-140), for the offset a desktop
    /// measures between the two machines.
    ///
    /// **Per connection, which is enough.** Two clocks that tick at the same rate keep
    /// the offset they had when it was measured; what changes it is a machine's clock
    /// being set, and a desktop that reconnects measures it again. `None` from a node
    /// that does not send one.
    pub node_time: Option<gungnir_model::MissionTime>,
    /// The most recent transport failure, for the status strip.
    pub last_error: Option<String>,
    /// Sequence number of the last envelope applied, so a reconnection resumes rather
    /// than replaying what has already been seen.
    pub last_seq: u64,
    /// The session token the link signed in with, for requests the desktop makes
    /// outside the stream (GAP-050's history fetch). Short-lived by design (DN-23 §5),
    /// and renewed by the link while it is up (GAP-165): this is always the current one.
    pub token: Option<String>,
    /// The node refused the current token on a request since the last forward tick
    /// (GAP-165): the link signs in again before it offers anything more. Set by any
    /// request answered `401`, and cleared by the renewal.
    pub token_refused: bool,
    /// How many times this link has renewed its session without reconnecting (GAP-165):
    /// ahead of the token's expiry, or because the node refused it.
    pub session_renewals: u64,
    /// Detections waiting to reach the node (`ARCHITECTURE.md` §8.4 store-and-forward,
    /// GAP-050): queued by the remote service, and by the desktop during an outage, and
    /// posted by the link task whenever the node answers. Bounded; the oldest is dropped
    /// and counted, never silently.
    pub outbox: std::collections::VecDeque<DetectionView>,
    /// Detections the node accepted (`202`) from the outbox.
    pub forwarded: u64,
    /// Detections dropped from a full outbox.
    pub dropped: u64,
    /// Tracks the node has changed since the last `take_changed_tracks` (GAP-009): a
    /// peer link feeds these to the peer adapter as they arrive.
    pub changed: Vec<gungnir_model::TrackId>,
    /// Launch warnings the node has issued since the last `take_launch_warnings`
    /// (GAP-009, DN-16 §5).
    ///
    /// **A queue of its own, beside the tracks and never inside them.** DN-16 §5 makes a
    /// launch warning a distinct message because it is a statement about the future with
    /// no kinematic state; folding it into `tracks` or `changed` would be the very thing
    /// the note refuses, and would end with a track nobody observed. Bounded, oldest
    /// dropped and counted, like the other two queues here.
    pub launch_warnings: std::collections::VecDeque<gungnir_model::LaunchWarningReport>,
    /// Launch warnings dropped from a full queue. Counted, never silent.
    pub launch_warnings_dropped: u64,
    /// Envelopes the host acts on rather than projects (GAP-004, GAP-040): sensor task
    /// events naming the node's task ids, and effector reports. Bounded; the oldest is
    /// dropped and counted.
    pub inbox: std::collections::VecDeque<Envelope>,
    pub inbox_dropped: u64,
    /// Sensor tasks the desktop's registry handed the link to deliver (GAP-004), and the
    /// node's answer to each.
    pub task_outbox: std::collections::VecDeque<OutboundTask>,
    pub task_outcomes: Vec<TaskOutcome>,
    /// This desktop's current held sets for exchange, waiting to replace what the node
    /// holds (GAP-065, DN-18 §5 amendment 2). Store-and-forward like `task_outbox`: an
    /// unreachable node leaves a batch queued for the next tick rather than dropping it.
    /// No outcome queue beside it, unlike `task_outbox`'s `task_outcomes` -- a publish
    /// generates no node-issued identifier for anything here to wait on.
    ///
    /// **At most one batch per item** (GAP-146, DN-18 §13, D-76). Every batch is the
    /// producer's whole set, so a newer one makes any older one waiting for the same item
    /// obsolete: [`NodeLink::queue_exchange`] replaces it in place and counts the
    /// replacement in [`ExchangePublishing::superseded`]. That is the bound, and it drops
    /// nothing the node should still end up with.
    pub exchange_outbox: std::collections::VecDeque<OutboundExchange>,
    /// Where this link's exchange publishing stands: what the node took, what it would
    /// not, and whether the link has stopped offering (GAP-146, DN-18 §13, D-75).
    pub exchange: ExchangePublishing,
    /// When the node was last heard from at all -- an envelope, a heartbeat, the
    /// snapshot (D-23).
    ///
    /// Wall time, not mission time: this is about the liveness of a socket, and a replayed
    /// session has no socket. `None` until the first snapshot lands, which is a different
    /// claim from "heard a long time ago" and the strip keeps them apart.
    pub last_heard: Option<std::time::Instant>,
    /// The node's approval queue, as this desktop sees it (GAP-133, DN-31 §6.6).
    ///
    /// Not a queue of this desktop's own: while a desktop is linked the node holds the
    /// queue (D-55), and this is a projection of it. See [`crate::queue`] for which of
    /// the picture and the stream is authoritative for what.
    pub queue: crate::queue::NodeQueue,
    /// Decisions this desktop has taken on the node's queue, on their way to
    /// `POST /v3/queue/{item}/decision`, and the node's answer to each (GAP-133).
    ///
    /// Store-and-forward like `task_outbox`, with one difference that matters: a post
    /// that went unanswered is **retried under the same request key** rather than
    /// abandoned, because a `504` does not mean nothing was recorded (DN-31 §6.3). The
    /// key is what makes the retry the same request instead of a second decision.
    pub decision_outbox: std::collections::VecDeque<crate::queue::OutboundDecision>,
    pub decision_outcomes: Vec<crate::queue::DecisionOutcome>,
    /// Outages' decisions on their way to `POST /v3/decisions/forwarded`, oldest first,
    /// and the node's answer to each (GAP-134, DN-31 §6.8).
    ///
    /// Retried under the same identifiers until the node answers, as the decision outbox
    /// is and for the same reason: a `504` does not mean nothing was recorded. It is safe
    /// to send a batch again because the node keys it on each decision's own identifier
    /// and answers a repeat `already_held`.
    pub forward_outbox: std::collections::VecDeque<crate::queue::OutboundForward>,
    pub forward_replies: Vec<crate::queue::ForwardReply>,
}

/// A sensor task on its way to the node (GAP-004).
#[derive(Debug, Clone, PartialEq)]
pub struct OutboundTask {
    /// The desktop registry's id, which the answer is matched back to.
    pub local: gungnir_model::SensorTaskId,
    pub sensor: gungnir_model::SensorId,
    pub command: gungnir_model::SensorCommand,
    pub requirement: Option<gungnir_model::RequirementId>,
}

/// The node's answer to an outbound task: its own task id, or the refusal.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskOutcome {
    pub local: gungnir_model::SensorTaskId,
    pub outcome: Result<gungnir_model::SensorTaskId, String>,
}

/// What this desktop currently holds for one exchange item, on its way to replace what
/// the node holds (GAP-065, DN-18 §5 amendment 2).
#[derive(Debug, Clone, PartialEq)]
pub struct OutboundExchange {
    pub item: gungnir_model::ExchangeItem,
    pub products: Vec<ExchangeProductRecord>,
    /// Which queuing this batch came from, counted per link (GAP-146).
    ///
    /// A batch can be replaced by a newer one for the same item while it is on its way
    /// to the node, and the flush has to remove the batch it sent rather than whichever
    /// one now stands at that place in the outbox -- or the newer set would be lost.
    pub generation: u64,
}

/// Where a link's exchange publishing stands (GAP-146, DN-18 §13, D-75).
///
/// **Four answers, not two.** A publish the node took; one that met no answer or an
/// answer that says try again; one the node refused because of **who is asking**; and one
/// it refused because of **what was sent**. The link used to treat all but the first as
/// "try the same batch again next tick", which for an Operator's console -- refused `403`
/// on every publish, because `PUBLISH_EXCHANGE` is not an Operator's -- meant a retry four
/// times a second for as long as the console ran, and nothing said on any panel.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExchangePublishing {
    /// Publish requests this link has made, answered or not.
    pub posts: u64,
    /// Sets the node took (`2xx`).
    pub delivered: u64,
    /// Sets replaced by a newer set for the same item before they were sent. Nothing is
    /// lost by one: the newer set is the whole of what the older one said and more.
    pub superseded: u64,
    /// Sets the node rejected on their own merits (a `4xx` other than `401`/`403`), each
    /// dropped: sending the same bytes again would earn the same answer, and the next set
    /// for the item replaces it anyway.
    pub rejected: u64,
    /// The most recent of those, for the line PN-09 draws.
    pub last_rejection: Option<PublishRefusal>,
    /// **The node refused this link as a publisher** (`401` or `403`), and the link has
    /// stopped offering until it signs in again. The queued sets stay, newest per item.
    pub refused: Option<PublishRefusal>,
    /// Sets are waiting on a node that did not answer, or answered "not now".
    pub retrying: Option<PublishRetry>,
    /// Not before this instant is the next retry made. The interval doubles per attempt
    /// from one forward tick to [`PUBLISH_RETRY_CEILING`], so a node that answers `507` for
    /// an hour is asked about a hundred and twenty times, not fourteen thousand.
    pub retry_at: Option<std::time::Instant>,
    /// The last generation handed out; see [`OutboundExchange::generation`].
    pub generation: u64,
}

impl ExchangePublishing {
    /// A new session is the change a refused publisher was waiting for (GAP-146, D-75):
    /// whoever signed in this time may be allowed what the last one was not, so what is
    /// held is offered once more, now. So is anything that was backing off, because a
    /// node that answers a fresh sign-in is not the node that was failing a moment ago.
    pub fn signed_in_again(&mut self) {
        if let Some(refused) = self.refused.take() {
            tracing::info!(
                status = refused.status,
                "the link signed in again; offering the held exchange sets once more"
            );
        }
        self.retrying = None;
        self.retry_at = None;
    }
}

/// A publish the node would not take, and the node's own words for why (GAP-146).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishRefusal {
    pub item: gungnir_model::ExchangeItem,
    pub status: u16,
    /// The node's `message`, or its body where that does not decode; never invented.
    pub reason: String,
}

/// Sets waiting on a node that has not taken them yet (GAP-146).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishRetry {
    /// Consecutive attempts without a delivery, on this connection.
    pub attempts: u32,
    pub last_failure: String,
}

/// What one publish request came back with (GAP-146, D-75).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishAnswer {
    /// `2xx`: the node holds the set.
    Delivered,
    /// No answer, or `408`, `425`, `429` or a `5xx`: the node may take it later, so the
    /// same set is offered again after a growing interval. `507` is here: the register is
    /// full and says so, and the backlog is the visible consequence (DN-18 §11).
    Retry,
    /// `401` or `403`: **who is asking** may not publish. Nothing about the set would
    /// change that, so the link stops offering until it signs in again.
    CallerRefused,
    /// Any other `4xx`: **what was sent** was not acceptable. That set is dropped and
    /// counted; the next set for the item is offered as usual.
    SetRejected,
}

/// How a publish's status is read (GAP-146, D-75). `None` is no answer at all.
#[must_use]
pub fn publish_answer(status: Option<u16>) -> PublishAnswer {
    match status {
        None | Some(408 | 425 | 429 | 500..=599) => PublishAnswer::Retry,
        Some(200..=299) => PublishAnswer::Delivered,
        Some(401 | 403) => PublishAnswer::CallerRefused,
        // A 1xx or 3xx is not something this client asked for; it is not an acceptance,
        // and treating it as one would claim a publish that did not happen.
        Some(_) => PublishAnswer::SetRejected,
    }
}

/// The longest a link waits between two retries of a waiting set (GAP-146).
pub const PUBLISH_RETRY_CEILING: std::time::Duration = std::time::Duration::from_secs(30);

/// How long after `attempts` consecutive failures the next retry is made: one forward
/// tick after the first, doubling, and never longer than [`PUBLISH_RETRY_CEILING`].
#[must_use]
pub fn publish_retry_delay(attempts: u32) -> std::time::Duration {
    let doublings = attempts.saturating_sub(1).min(16);
    FORWARD_INTERVAL
        .saturating_mul(1u32 << doublings)
        .min(PUBLISH_RETRY_CEILING)
}

/// Where this link's exchange publishing stands, and what is waiting (GAP-146).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExchangeStanding {
    /// The items with a set waiting for the node, in the order they will be offered.
    pub waiting: Vec<gungnir_model::ExchangeItem>,
    pub publishing: ExchangePublishing,
}

/// One product within an [`OutboundExchange`] (GAP-065).
///
/// Mirrors `gungnir_api::v3::ExchangeProduct` field for field rather than reusing it --
/// the same choice [`OutboundTask`] makes against `SensorTaskRequest`. `gungnir-app` has
/// no production edge to `gungnir-api` (`gungnir-app/Cargo.toml`: the dependency is
/// dev-only, for an end-to-end test), so the type a desktop producer builds has to come
/// from `gungnir-model` and standard types alone; `flush_exchange`, which lives in this
/// crate and already depends on `gungnir-api` for the wire contract, is where a record
/// becomes the request the node actually reads.
#[derive(Debug, Clone, PartialEq)]
pub struct ExchangeProductRecord {
    pub id: String,
    pub at: gungnir_model::MissionTime,
    pub releasability: gungnir_model::Releasability,
    pub body: serde_json::Value,
}

/// How many host-bound envelopes the link holds before dropping the oldest.
pub const INBOX_CAPACITY: usize = 4096;

/// How many launch warnings the link holds before dropping the oldest (GAP-009).
///
/// Much smaller than [`INBOX_CAPACITY`] because a peer that issues thousands of launch
/// warnings between two frames is a peer that is faulty or hostile, and the bound is
/// what stops one from filling this desktop's memory. Anything past it is counted in
/// [`Projection::launch_warnings_dropped`], so a host can say that warnings were lost
/// rather than reporting a quiet peer.
pub const LAUNCH_WARNING_CAPACITY: usize = 256;

/// A live link to one node.
#[derive(Debug, Clone)]
pub struct NodeLink {
    projection: Arc<Mutex<Projection>>,
    /// Bumped by the link task every time it mutates the projection, including a
    /// disconnect. See [`NodeLink::changes`].
    revision: watch::Receiver<u64>,
    /// Dropped when the last service is dropped, which is what stops the task.
    _shutdown: Arc<mpsc::Sender<()>>,
    /// Who the link signs in as at its next connection, shared with the task so that a
    /// sign-in on the desktop can change it without replacing the link (GAP-143). `None`
    /// for a machine link, whose identity is its certificate.
    credential: Arc<Mutex<Option<Credential>>>,
    /// Started as a machine (D-02). A sign-in never turns one into an operator's link.
    machine: bool,
}

impl NodeLink {
    /// A link no task drives, whose projection a test sets by hand (GAP-050): the
    /// desktop's failover judges silence and reconnection from the projection alone,
    /// and this is how a test makes a node go quiet without a socket.
    #[must_use]
    pub fn scripted() -> Self {
        let (shutdown_tx, _shutdown_rx) = mpsc::channel::<()>(1);
        // No task ever sends on this, so `changes()` on a scripted link reports the
        // channel closed rather than hanging: correct enough, since nothing scripts a
        // link and then waits on it to change itself.
        let (_revision_tx, revision_rx) = watch::channel(0u64);
        Self {
            projection: Arc::new(Mutex::new(Projection::default())),
            revision: revision_rx,
            _shutdown: Arc::new(shutdown_tx),
            credential: Arc::new(Mutex::new(None)),
            machine: false,
        }
    }

    /// Sign in as `credential` from the link's next connection on, keeping everything the
    /// link holds (GAP-143).
    ///
    /// **Why this and not a new link.** A sign-in during an outage used to build a new
    /// link and put the remote services back, which ended the outage with no person
    /// switching back (D-15) and threw away what the old link was carrying for the node:
    /// the observations queued while cut off, the exchange outbox and the forwarding
    /// (DN-31 §6.8, §13). Replacing only the credential leaves all of that where it is.
    /// A connection already open keeps the token it signed in with; the change takes
    /// effect when the link next signs in, which during an outage is when the node
    /// answers again.
    ///
    /// # Errors
    ///
    /// `RemoteError::InvalidEndpoint` on a machine link: its identity is its certificate
    /// (D-02), and a person's sign-in on the desktop is not a reason to change what a
    /// machine presents.
    pub fn replace_credential(&self, credential: Credential) -> Result<(), RemoteError> {
        if self.machine {
            return Err(RemoteError::InvalidEndpoint(
                "a machine link signs in with its certificate, not an operator's credential".into(),
            ));
        }
        let mut held = self
            .credential
            .lock()
            .map_err(|_| RemoteError::Client("the link's credential lock was poisoned".into()))?;
        *held = Some(credential);
        Ok(())
    }

    /// The operator the link will sign in as at its next connection, if it signs in at
    /// all. Never the passphrase.
    #[must_use]
    pub fn signs_in_as(&self) -> Option<u64> {
        self.credential
            .lock()
            .ok()
            .and_then(|held| held.as_ref().map(|c| c.operator))
    }

    /// Set what the projection says about liveness. Meant for tests and for nothing on
    /// the live path, which is the task's to write.
    pub fn script_liveness(&self, connected: bool, last_heard: Option<std::time::Instant>) {
        if let Ok(mut p) = self.projection.lock() {
            p.connected = connected;
            p.last_heard = last_heard;
        }
    }

    /// Read the projection. Returns `None` only if the link task panicked while holding
    /// the lock, which the caller reports rather than panicking in the frame loop.
    #[must_use]
    pub fn read(&self) -> Option<std::sync::MutexGuard<'_, Projection>> {
        self.projection.lock().ok()
    }

    #[must_use]
    pub fn connected(&self) -> bool {
        self.read().is_some_and(|p| p.connected)
    }

    /// How long this link has been silent: since the last thing heard from the node, or
    /// since the link started where nothing has ever been heard (GAP-142).
    ///
    /// `None` only before the task has started asking at all, which is the one state
    /// where there is nothing to measure from and nothing to conclude.
    #[must_use]
    pub fn silent_for(&self) -> Option<std::time::Duration> {
        self.read()
            .and_then(|p| p.last_heard.or(p.started))
            .map(|from| from.elapsed())
    }

    /// The node's own clock as of the last snapshot (GAP-140).
    ///
    /// What the caller does with it is measure the offset against its own clock once per
    /// connection; the link does not, because the clock a desktop draws against is the
    /// app's authority and this task has none.
    #[must_use]
    pub fn node_time(&self) -> Option<gungnir_model::MissionTime> {
        self.read().and_then(|p| p.node_time)
    }

    /// A receiver that changes every time the link task mutates the projection or its
    /// connection state, for a caller that wants to wait on the next mutation instead
    /// of polling the projection on a fixed interval.
    ///
    /// **For tests, and nothing on the live path reads this** -- the frame loop still
    /// reads the projection synchronously, exactly as the module documentation
    /// describes. It exists because a fixed-interval poll loop needs *every one* of a
    /// few thousand wake-ups to be scheduled promptly to finish inside its own budget,
    /// and under a `cargo test --workspace` run competing for the same cores, that
    /// stopped being true for `a_deleted_track_leaves_the_projection` (flaky in CI
    /// twice, reproduced locally once; see that test and `tests/transport.rs`'s
    /// `until`). Waiting on this instead needs the *link task* to be scheduled once per
    /// real change, which is the thing the test is actually waiting for.
    #[must_use]
    pub fn changes(&self) -> watch::Receiver<u64> {
        self.revision.clone()
    }

    /// The token the last sign-in issued, if the link has signed in.
    #[must_use]
    pub fn token(&self) -> Option<String> {
        self.read().and_then(|p| p.token.clone())
    }

    /// How many times this link has renewed its session while staying up (GAP-165).
    #[must_use]
    pub fn session_renewals(&self) -> u64 {
        self.read().map_or(0, |p| p.session_renewals)
    }

    /// The last envelope sequence applied, or 0 before any.
    #[must_use]
    pub fn last_seq(&self) -> u64 {
        self.read().map_or(0, |p| p.last_seq)
    }

    /// Set the token and sequence by hand, for a test that scripts a link (GAP-050).
    pub fn script_session(&self, token: Option<String>, last_seq: u64) {
        if let Ok(mut p) = self.projection.lock() {
            p.token = token;
            p.last_seq = last_seq;
        }
    }

    /// Tracks changed since the last call, in the order they changed (GAP-009).
    #[must_use]
    pub fn take_changed_tracks(&self) -> Vec<TrackView> {
        let Ok(mut p) = self.projection.lock() else {
            return Vec::new();
        };
        let ids = std::mem::take(&mut p.changed);
        ids.iter()
            .filter_map(|id| p.tracks.iter().find(|t| t.id == *id).cloned())
            .collect()
    }

    /// Launch warnings the node has issued since the last call, oldest first (GAP-009,
    /// DN-16 §5).
    ///
    /// Separate from [`NodeLink::take_changed_tracks`] on purpose: a caller that wanted
    /// both has to ask for both, and no caller can receive a launch warning while
    /// believing it asked for tracks.
    #[must_use]
    pub fn take_launch_warnings(&self) -> Vec<gungnir_model::LaunchWarningReport> {
        self.projection
            .lock()
            .map(|mut p| p.launch_warnings.drain(..).collect())
            .unwrap_or_default()
    }

    /// Launch warnings dropped because the queue was full (GAP-009).
    #[must_use]
    pub fn launch_warnings_dropped(&self) -> u64 {
        self.read().map_or(0, |p| p.launch_warnings_dropped)
    }

    /// Envelopes the host acts on, oldest first (GAP-004, GAP-040).
    #[must_use]
    pub fn take_inbox(&self) -> Vec<Envelope> {
        self.projection
            .lock()
            .map(|mut p| p.inbox.drain(..).collect())
            .unwrap_or_default()
    }

    /// Hand a sensor task to the link for delivery to the node (GAP-004).
    pub fn queue_task(&self, task: OutboundTask) {
        if let Ok(mut p) = self.projection.lock() {
            p.task_outbox.push_back(task);
        }
    }

    /// The node's answers to delivered tasks since the last call.
    #[must_use]
    pub fn take_task_outcomes(&self) -> Vec<TaskOutcome> {
        self.projection
            .lock()
            .map(|mut p| std::mem::take(&mut p.task_outcomes))
            .unwrap_or_default()
    }

    /// The node's approval queue as of now (GAP-133, DN-31 §6.6).
    ///
    /// An owned copy taken once per tick. `waiting` is `None` where this desktop has not
    /// been told what is waiting -- before the first picture, and after the link goes
    /// down -- which PN-06 draws differently from a queue that is empty.
    #[must_use]
    pub fn queue(&self) -> crate::queue::QueueSnapshot {
        self.read().map(|p| p.queue.snapshot()).unwrap_or_default()
    }

    /// Take a decision on one of the node's queue items (DN-31 §6.3, §6.6).
    ///
    /// **Nothing is recorded on this desktop by calling this.** The decision is the
    /// node's to take, journal and act on; what happens here is a post, and the answer
    /// comes back through [`NodeLink::take_decision_outcomes`]. `request` is minted by
    /// the caller once per decision a person takes, and every retry of that decision
    /// carries it unchanged.
    pub fn queue_decision(&self, decision: crate::queue::OutboundDecision) {
        if let Ok(mut p) = self.projection.lock() {
            p.decision_outbox.push_back(decision);
        }
    }

    /// The node's answers to the decisions posted since the last call.
    #[must_use]
    pub fn take_decision_outcomes(&self) -> Vec<crate::queue::DecisionOutcome> {
        self.projection
            .lock()
            .map(|mut p| std::mem::take(&mut p.decision_outcomes))
            .unwrap_or_default()
    }

    /// Decisions posted and not yet answered, oldest first (GAP-133).
    ///
    /// Read by PN-07 so a decision that keeps meeting a `504` is on screen as one still
    /// in flight rather than one that quietly never landed.
    #[must_use]
    pub fn decisions_in_flight(&self) -> Vec<crate::queue::OutboundDecision> {
        self.read()
            .map(|p| p.decision_outbox.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Hand an outage's decisions to the link, for `POST /v3/decisions/forwarded`
    /// (GAP-134, DN-31 §6.8).
    ///
    /// **Nothing is recorded anywhere by calling this.** The batch is posted whole when
    /// the node answers, and the node's answer comes back through
    /// [`NodeLink::take_forward_replies`]. An empty batch is not queued: there is nothing
    /// for the node to take, and an empty post would be a `202` that claimed an outage
    /// had been forwarded when nothing was decided in it.
    pub fn queue_forward(&self, decisions: Vec<crate::queue::ForwardedDecision>) {
        if decisions.is_empty() {
            return;
        }
        if let Ok(mut p) = self.projection.lock() {
            p.forward_outbox.push_back(crate::queue::OutboundForward {
                decisions,
                attempts: 0,
            });
        }
    }

    /// The node's answers to the batches posted since the last call.
    #[must_use]
    pub fn take_forward_replies(&self) -> Vec<crate::queue::ForwardReply> {
        self.projection
            .lock()
            .map(|mut p| std::mem::take(&mut p.forward_replies))
            .unwrap_or_default()
    }

    /// Batches posted and not yet answered, oldest first (GAP-134), for PN-18.
    #[must_use]
    pub fn forwards_in_flight(&self) -> Vec<crate::queue::OutboundForward> {
        self.read()
            .map(|p| p.forward_outbox.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Hand this desktop's current held set for `item` to the link, to replace what the
    /// node holds **for this desktop** (GAP-065, DN-18 §5 amendment 2, GAP-137).
    ///
    /// **A replacement, not an addition**, mirroring
    /// `gungnir_api::transport::NodeApi::publish_exchange`'s own contract: `products` is
    /// this producer's whole current set for `item`, not a diff against what was queued
    /// before.
    ///
    /// **So a newer set replaces an older one still waiting** (GAP-146, D-76), in place,
    /// and the replacement is counted in [`ExchangePublishing::superseded`]. The outbox
    /// therefore holds at most one set per item however long the node does not take them
    /// -- an Operator's console refused `403` used to add one batch per handoff for as long
    /// as it ran -- and what it holds is exactly what the node should end up with. A set
    /// already on its way is not recalled; the flush removes the set it sent by its
    /// generation, so the newer one stays and is sent after it.
    ///
    /// **What it replaces is this desktop's set alone** (DN-18 §5 amendment 3). The node
    /// keys the register on the name this link's certificate was verified under, so the
    /// node's own handoffs and every other desktop's survive this batch -- which is what
    /// lets a desktop that fell back publish its whole set on reconnect without erasing
    /// what the node decided while it was gone.
    pub fn queue_exchange(
        &self,
        item: gungnir_model::ExchangeItem,
        products: Vec<ExchangeProductRecord>,
    ) {
        let Ok(mut guard) = self.projection.lock() else {
            return;
        };
        let p = &mut *guard;
        p.exchange.generation = p.exchange.generation.wrapping_add(1);
        let batch = OutboundExchange {
            item,
            products,
            generation: p.exchange.generation,
        };
        match p.exchange_outbox.iter_mut().find(|b| b.item == item) {
            Some(waiting) => {
                *waiting = batch;
                p.exchange.superseded += 1;
            }
            None => p.exchange_outbox.push_back(batch),
        }
    }

    /// Where this link's exchange publishing stands, and what is waiting (GAP-146), for
    /// PN-09. `None` only if the link task panicked holding the lock.
    #[must_use]
    pub fn exchange_standing(&self) -> Option<ExchangeStanding> {
        self.read().map(|p| ExchangeStanding {
            waiting: p.exchange_outbox.iter().map(|b| b.item).collect(),
            publishing: p.exchange.clone(),
        })
    }

    /// Queue a detection for the node (§8.4). Taken whether or not the node answers;
    /// a full outbox drops its oldest and counts the drop.
    pub fn queue_outbound(&self, detection: DetectionView) {
        if let Ok(mut p) = self.projection.lock() {
            if p.outbox.len() >= crate::OUTBOX_CAPACITY {
                p.outbox.pop_front();
                p.dropped += 1;
                if p.dropped == 1 {
                    tracing::warn!("the link's outbox is full; dropping the oldest detections");
                }
            }
            p.outbox.push_back(detection);
        }
    }

    /// Detections still waiting for the node.
    #[must_use]
    pub fn outbox_len(&self) -> usize {
        self.read().map_or(0, |p| p.outbox.len())
    }

    /// Detections the node has accepted from the outbox.
    #[must_use]
    pub fn forwarded(&self) -> u64 {
        self.read().map_or(0, |p| p.forwarded)
    }

    /// Detections dropped from a full outbox.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.read().map_or(0, |p| p.dropped)
    }

    /// How long since the node was last heard from, or `None` if it never has been.
    ///
    /// Read by PN-01 every frame. Ages while the link is silent and keeps ageing after the
    /// task has declared it gone, so the strip shows a number that grows rather than a
    /// light that is simply off -- an operator can see *how* stale the picture is.
    #[must_use]
    pub fn last_heard_age(&self) -> Option<std::time::Duration> {
        self.read()
            .and_then(|p| p.last_heard)
            .map(|at| at.elapsed())
    }
}

/// The URLs a node endpoint resolves to.
#[derive(Debug)]
struct Urls {
    session: String,
    snapshot: String,
    events: String,
    history: String,
    detections: String,
    tasks: String,
    /// The three write doors DN-18 §5 amendment 2 added (GAP-065): one item per URL,
    /// exactly as the node's own routes are three concrete paths rather than one
    /// parameterized by item.
    exchange_warnings: String,
    exchange_reports: String,
    exchange_handoffs: String,
    /// The node's approval queue, and the prefix a decision's URL is built on (GAP-133,
    /// DN-31 §7). One URL and one prefix rather than two paths, exactly as `tasks` is the
    /// prefix `SENSOR_TASK` is filled in from.
    queue: String,
    /// Where an outage's decisions go when the node answers again (GAP-134, DN-31 §7).
    forwarded: String,
    /// True for an `https` endpoint: the stream is `wss` over our own TLS stream.
    tls: bool,
    host: String,
    port: u16,
}

/// Turn the configured base URL into the endpoints of the contract.
///
/// Every path comes from `gungnir_api::path` and `gungnir_api::routes`, the pair the
/// node's router is built from, so the desktop cannot ask for a route where the node does
/// not serve it (GAP-130).
///
/// `https` needs pinned trust roots and is refused without them; `http` is never
/// upgraded. See the module documentation.
fn urls(endpoint: &RemoteEndpoint) -> Result<Urls, RemoteError> {
    let base = endpoint.url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err(RemoteError::InvalidEndpoint("empty url".into()));
    }
    let (tls, rest) = if let Some(rest) = base.strip_prefix("http://") {
        (false, rest)
    } else if let Some(rest) = base.strip_prefix("https://") {
        if endpoint.tls.trust_roots_pem.is_empty() {
            return Err(RemoteError::InvalidEndpoint(format!(
                "{base}: an https endpoint needs pinned trust roots \
                 (security.tls.trust_roots_pem, GAP-060); with none the node's certificate \
                 could not be checked, and the link is refused rather than spoken in \
                 plaintext or trusted blindly"
            )));
        }
        (true, rest)
    } else {
        return Err(RemoteError::InvalidEndpoint(format!(
            "{base}: the endpoint must be http or https; anything else is refused rather \
             than guessed at (GAP-060), and http is never upgraded to plaintext-looking \
             https"
        )));
    };
    if rest.is_empty() {
        return Err(RemoteError::InvalidEndpoint(format!("{base}: no host")));
    }
    let parsed = reqwest::Url::parse(base)
        .map_err(|e| RemoteError::InvalidEndpoint(format!("{base}: {e}")))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| RemoteError::InvalidEndpoint(format!("{base}: no host")))?
        .to_owned();
    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| RemoteError::InvalidEndpoint(format!("{base}: no port")))?;
    let scheme = if tls { "wss" } else { "ws" };
    Ok(Urls {
        history: format!("{base}{}", path(routes::HISTORY)),
        detections: format!("{base}{}", path(routes::DETECTIONS)),
        session: format!("{base}{}", path(routes::SESSION)),
        snapshot: format!("{base}{}", path(routes::SNAPSHOT)),
        tasks: format!("{base}{}", path(routes::SENSORS)),
        exchange_warnings: format!("{base}{}", path(routes::EXCHANGE_WARNINGS)),
        exchange_reports: format!("{base}{}", path(routes::EXCHANGE_REPORTS)),
        exchange_handoffs: format!("{base}{}", path(routes::EXCHANGE_HANDOFFS)),
        queue: format!("{base}{}", path(routes::QUEUE)),
        forwarded: format!("{base}{}", path(routes::DECISIONS_FORWARDED)),
        events: format!("{scheme}://{rest}{}", path(routes::EVENTS)),
        tls,
        host,
        port,
    })
}

/// The HTTP client for one endpoint: the pinned roots and the identity, or a plain one.
fn http_client(tls: &LinkTls) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder();
    for (i, pem) in tls.trust_roots_pem.iter().enumerate() {
        let cert = reqwest::Certificate::from_pem(pem.as_bytes())
            .map_err(|e| format!("trust root {i} does not parse: {e}"))?;
        builder = builder.add_root_certificate(cert);
    }
    // The identity goes in as a whole rustls configuration rather than as a PEM, because
    // an identity whose private half lives in a `KeyProvider` cannot be expressed as one.
    // `reqwest::Identity::from_pem` was the only path here until 2026-09-06 and it is the
    // reason GAP-060's desktop half could not be built: the type demanded the very thing
    // custody exists to prevent. Both this client and the event stream are handed the
    // same configuration, so they present the same certificate.
    if tls.has_identity() {
        builder = builder.tls_backend_preconfigured(crate::client_config(tls)?);
    }
    builder
        .build()
        .map_err(|e| format!("the HTTP client could not be built: {e}"))
}

/// The TLS connector for the event stream, from the same material as the HTTP client.
fn ws_connector(tls: &LinkTls) -> Result<tokio_rustls::TlsConnector, String> {
    // The same configuration the HTTP client is given; see `crate::client_config`.
    Ok(tokio_rustls::TlsConnector::from(Arc::new(
        crate::client_config(tls)?,
    )))
}

/// The byte stream under the WebSocket: a TCP socket, or TLS over one.
trait Io: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> Io for T {}

type Socket = tokio_tungstenite::WebSocketStream<Box<dyn Io>>;

/// Open the event stream: in the clear, or over our own TLS stream (the module
/// documentation says why the WebSocket crate does not do this itself).
async fn open_stream(urls: &Urls, tls: &LinkTls) -> Result<Socket, String> {
    let tcp = tokio::net::TcpStream::connect((urls.host.as_str(), urls.port))
        .await
        .map_err(|e| format!("could not reach {}:{}: {e}", urls.host, urls.port))?;
    let io: Box<dyn Io> = if urls.tls {
        let connector = ws_connector(tls)?;
        let name = rustls::pki_types::ServerName::try_from(urls.host.clone())
            .map_err(|e| format!("{} is not a usable server name: {e}", urls.host))?;
        let stream = connector
            .connect(name, tcp)
            .await
            .map_err(|e| format!("TLS handshake with {} failed: {e}", urls.host))?;
        Box::new(stream)
    } else {
        Box::new(tcp)
    };
    let (socket, _) = tokio_tungstenite::client_async(urls.events.as_str(), io)
        .await
        .map_err(|e| format!("event stream refused: {e}"))?;
    Ok(socket)
}

/// What the desktop signs in with.
///
/// Held for the life of the link because a reconnection has to sign in again: a token is
/// short-lived by design (DN-23 §5 rule 2) and is never renewed on use.
///
/// **`Debug` never prints the passphrase.** Since GAP-143 the link holds its credential
/// where the desktop can replace it, inside a `NodeLink`, which derives `Debug` and sits
/// inside the application state; a derived `Debug` here would put the passphrase into any
/// log line or panic message that formatted one of them.
#[derive(Clone, PartialEq, Eq)]
pub struct Credential {
    pub operator: u64,
    pub passphrase: String,
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credential")
            .field("operator", &self.operator)
            .field("passphrase", &"<redacted>")
            .finish()
    }
}

/// Start the link task as an operator. Returns immediately; nothing is connected yet.
///
/// # Errors
///
/// `RemoteError::InvalidEndpoint` for a URL the link cannot speak to.
pub fn start(
    endpoint: &RemoteEndpoint,
    credential: Credential,
    handle: &tokio::runtime::Handle,
) -> Result<NodeLink, RemoteError> {
    start_with(endpoint, Some(credential), handle)
}

/// Start the link task as a machine: no sign-in, the client certificate is the identity
/// (D-02), and the node serves what the party's exchange agreement lists (DN-18). A
/// peer link (GAP-009) is one of these.
///
/// # Errors
///
/// `RemoteError::InvalidEndpoint` for a URL the link cannot speak to, and for an
/// endpoint with no identity: a machine with no certificate is nobody.
pub fn start_as_machine(
    endpoint: &RemoteEndpoint,
    handle: &tokio::runtime::Handle,
) -> Result<NodeLink, RemoteError> {
    if endpoint.tls.identity_pem.is_none() {
        return Err(RemoteError::InvalidEndpoint(format!(
            "{}: a machine link needs a client certificate (D-02); none is held",
            endpoint.url
        )));
    }
    start_with(endpoint, None, handle)
}

fn start_with(
    endpoint: &RemoteEndpoint,
    credential: Option<Credential>,
    handle: &tokio::runtime::Handle,
) -> Result<NodeLink, RemoteError> {
    let urls = urls(endpoint)?;
    let tls = endpoint.tls.clone();
    // GAP-142: silence is measured from here until there is something heard to measure
    // it from, so a node that never answers is judged rather than waited on for ever.
    let projection = Arc::new(Mutex::new(Projection {
        started: Some(std::time::Instant::now()),
        ..Projection::default()
    }));
    let (revision_tx, revision_rx) = watch::channel(0u64);
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
    let machine = credential.is_none();
    let credential = Arc::new(Mutex::new(credential));

    let task_projection = Arc::clone(&projection);
    let task_credential = Arc::clone(&credential);
    handle.spawn(async move {
        loop {
            // Read afresh for every connection, so a credential replaced while the node
            // was silent is the one the next sign-in uses (GAP-143).
            let current = task_credential.lock().ok().and_then(|held| held.clone());
            if let Err(err) = run_link(
                &urls,
                &tls,
                current.as_ref(),
                &task_projection,
                &revision_tx,
            )
            .await
            {
                set_disconnected(&task_projection, &err);
            } else {
                set_disconnected(&task_projection, "the node closed the stream");
            }
            // A disconnect is itself a change a waiter on `changes()` needs to see,
            // rather than sitting out the rest of its own patience for a projection
            // update that stopped coming.
            revision_tx.send_modify(|r| *r = r.wrapping_add(1));
            tokio::select! {
                // A closed channel means every service was dropped, so stop.
                _ = shutdown_rx.recv() => return,
                () = tokio::time::sleep(RECONNECT_DELAY) => {}
            }
        }
    });

    Ok(NodeLink {
        projection,
        revision: revision_rx,
        _shutdown: Arc::new(shutdown_tx),
        credential,
        machine,
    })
}

fn set_disconnected(projection: &Arc<Mutex<Projection>>, reason: &str) {
    if let Ok(mut p) = projection.lock() {
        p.connected = false;
        p.last_error = Some(reason.to_owned());
        // What the node was waiting on a moment ago is no longer something this desktop
        // can claim to know, so PN-06 says it has not been told rather than drawing a
        // list nothing is maintaining (GAP-133). What ended stays ended, and the decision
        // outbox stays too: a decision posted and never answered is retried under the
        // same key when the node comes back, which is how the client learns whether it
        // was recorded (DN-31 §6.3).
        p.queue.disconnected();
    }
}

/// One connection: sign in (an operator), snapshot, then follow the stream until it
/// ends.
///
/// Signing in first because every route but the session one needs the token (DN-23 §6).
/// A node with no account store answers `503` here, and the link says so rather than
/// retrying as though the network were at fault. A machine link has no sign-in: its
/// certificate is its identity, and the token stays empty.
async fn run_link(
    urls: &Urls,
    tls: &LinkTls,
    credential: Option<&Credential>,
    projection: &Arc<Mutex<Projection>>,
    revision: &watch::Sender<u64>,
) -> Result<(), String> {
    let client = http_client(tls)?;
    let issued = match credential {
        Some(credential) => Some(sign_in(&client, urls, credential).await?),
        None => None,
    };
    let token = issued
        .as_ref()
        .map_or_else(String::new, |issued| issued.token.clone());

    let response = with_token(client.get(&urls.snapshot), &token)
        .send()
        .await
        .map_err(|e| format!("snapshot request failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("snapshot returned {}", response.status()));
    }
    let snapshot: SnapshotResponse = response
        .json()
        .await
        .map_err(|e| format!("snapshot could not be decoded: {e}"))?;
    // GAP-063: the catalogue's rule is an exact version match, and a picture projected
    // from another schema would be read as this one's.
    if snapshot.schema_version != gungnir_model::SCHEMA_VERSION {
        return Err(format!(
            "the node speaks schema version {}, this desktop speaks {}; refusing to project it",
            snapshot.schema_version,
            gungnir_model::SCHEMA_VERSION
        ));
    }

    let node_time = snapshot.node_time;
    // Resume from where the last connection stopped, so a reconnection does not replay
    // what has already been applied.
    let from_seq = {
        let mut p = projection
            .lock()
            .map_err(|_| "the projection lock was poisoned".to_owned())?;
        p.changed = snapshot.tracks.iter().map(|t| t.id).collect();
        p.tracks = snapshot.tracks;
        if let Some(plan) = snapshot.plan {
            p.plan = plan;
        }
        // GAP-096's wire contract: the same snapshot that carries tracks now carries the
        // node's retained bearings and pipeline counters too. See `Projection::
        // bearing_rays`'s own doc comment for why this is a per-(re)connect refresh and
        // not a live one.
        p.bearing_rays = snapshot.bearing_rays;
        p.pipeline_stats = snapshot.pipeline_stats;
        // GAP-161: the node's own word on its services, kept live by the stream from here.
        p.node_health = Some(snapshot.health);
        p.token = Some(token.clone());
        p.node_time = snapshot.node_time;
        p.connected = true;
        p.last_error = None;
        p.last_heard = Some(std::time::Instant::now());
        p.exchange.signed_in_again();
        p.last_seq
    };
    revision.send_modify(|r| *r = r.wrapping_add(1));
    // GAP-165: how long the node's token lasts, read off the node's own clock -- the
    // expiry it issued against the time the snapshot just said it is.
    let mut session = LinkSession {
        renew_after: issued
            .as_ref()
            .and_then(|issued| renew_after(issued.expires_s, node_time)),
        issued_at: std::time::Instant::now(),
        token,
    };

    let mut socket = open_stream(urls, tls).await?;

    let subscribe = serde_json::to_string(&SubscribeRequest {
        from_seq,
        token: session.token.clone(),
    })
    .map_err(|e| format!("could not encode the subscribe frame: {e}"))?;
    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            subscribe.into(),
        ))
        .await
        .map_err(|e| format!("could not subscribe: {e}"))?;

    // The starting picture of the node's approval queue (GAP-133, DN-31 §6.6), taken
    // **after** the subscribe frame has gone. `SnapshotResponse` carries the same `queue`
    // field and was fetched above, before the stream existed; reading it there would
    // leave an item queued in between in neither place until the next reconnection. See
    // `crate::queue`'s module documentation for the whole rule.
    refresh_queue(&client, urls, &session.token, projection, true).await;

    // Anything at all resets the clock: an envelope, or the server's heartbeat ping,
    // which `tokio-tungstenite` answers for us. Silence past the timeout means the link
    // is gone even though the socket has not said so, which is the case a "connected"
    // light would otherwise get wrong for as long as the operating system kept the
    // connection open.
    //
    // Between frames the outbox is forwarded (§8.4): whatever the desktop queued while
    // the node was away, or since the last flush, goes to `POST /v3/detections` under the
    // same token, and stays queued until the node has said `202`.
    let mut forward = tokio::time::interval(FORWARD_INTERVAL);
    loop {
        tokio::select! {
            _ = forward.tick() => {
                // GAP-165: a live link keeps its session, ahead of the token's expiry and
                // again whenever the node has refused it, before anything is offered.
                session.keep(&client, urls, credential, projection).await?;
                let token = session.token.as_str();
                flush_outbox(&client, urls, token, projection).await;
                flush_tasks(&client, urls, token, projection).await;
                flush_exchange(&client, urls, token, projection).await;
                flush_decisions(&client, urls, token, projection).await;
                flush_forwarded(&client, urls, token, projection).await;
                // A picture the stream asked for and a failed fetch left owing. The
                // stream's own branch below takes it as soon as the event arrives; this
                // is what bounds the retry at one forward interval rather than at the
                // next frame, which on a quiet node is one heartbeat away. A no-op unless
                // a fetch is actually owed.
                refresh_queue(&client, urls, token, projection, false).await;
            }
            next = tokio::time::timeout(gungnir_api::transport::HEARTBEAT_TIMEOUT, socket.next()) => {
                handle_frame(next, projection)?;
                // A `Queued` or `Escalated` this frame means the node's queue has moved,
                // so the picture is taken again at once rather than on the forward
                // interval: MOP-07 measures the whole path from a plan being proposed to
                // an approval control being available on a desktop, and 250 ms of
                // deliberate wait inside a 500 ms budget would be spent for nothing
                // (GAP-133, DN-31 §6.9). A no-op unless the stream asked for it.
                refresh_queue(&client, urls, &session.token, projection, false).await;
            }
        }
        // Unconditional rather than only after a confirmed mutation: a spurious wake
        // costs a waiter one extra predicate check, and that is far cheaper than a
        // second place this function could forget to signal a real one.
        revision.send_modify(|r| *r = r.wrapping_add(1));
    }
}

/// How often the outbox is offered to the node while the link is up.
const FORWARD_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);

/// How far into a token's life the link renews it (GAP-165): three quarters, so the
/// renewal has a quarter of the lifetime -- nearly four minutes at the node's default --
/// to reach a node that is slow to answer before the old token lapses.
const RENEW_AT_FRACTION: f64 = 0.75;

/// Sign in with `credential` and take the node's answer.
///
/// # Errors
///
/// When the node cannot be reached, refuses the credential, or answers something that
/// is not a session.
async fn sign_in(
    client: &reqwest::Client,
    urls: &Urls,
    credential: &Credential,
) -> Result<SessionResponse, String> {
    let issued = client
        .post(&urls.session)
        .json(&SessionRequest {
            operator: credential.operator,
            passphrase: credential.passphrase.clone(),
        })
        .send()
        .await
        .map_err(|e| format!("sign-in request failed: {e}"))?;
    if !issued.status().is_success() {
        return Err(format!("the node refused the sign-in: {}", issued.status()));
    }
    issued
        .json()
        .await
        .map_err(|e| format!("the session response could not be decoded: {e}"))
}

/// When, after a token is issued, the link renews it (GAP-165): [`RENEW_AT_FRACTION`] of
/// the lifetime the node gave it, measured as the expiry it issued against the time it
/// said it was when the link asked. `None` where the node sent no time (a node older
/// than GAP-140) or the two do not make a lifetime, in which case the link renews only
/// when the node refuses the token.
#[must_use]
pub fn renew_after(
    expires_s: f64,
    node_time: Option<gungnir_model::MissionTime>,
) -> Option<std::time::Duration> {
    let lifetime_s = expires_s - node_time?.0;
    if !lifetime_s.is_finite() || lifetime_s <= 0.0 {
        return None;
    }
    std::time::Duration::try_from_secs_f64(lifetime_s * RENEW_AT_FRACTION).ok()
}

/// A connection's session with the node, kept for as long as the connection is up
/// (GAP-165).
///
/// **Why the link renews and nothing else does.** A node-issued token always expires
/// (DN-23 §5) -- after the baseline's session lifetime, or the node's own 900 s where the
/// baseline names none -- and the stream is authenticated once, when it subscribes. Until
/// GAP-165 nothing asked for a new token while the stream stayed up, so from the moment
/// the first one lapsed every write the desktop made was refused `401`: its detections
/// and exchange sets waited in their outboxes, and every decision an operator took on
/// the node's queue came back refused, for as long as the stream happened to stay open.
/// The desktop's own session is what decides how long this link may act for -- a desktop
/// whose session expires drops the link (`gungnir-app`'s `session::sweep_expiry`) -- so
/// the link keeps the node's side current for exactly as long as it is up.
struct LinkSession {
    token: String,
    issued_at: std::time::Instant,
    renew_after: Option<std::time::Duration>,
}

impl LinkSession {
    /// Renew the session if it is due or the node has refused it, and say so in the
    /// projection.
    ///
    /// A machine link has no session to renew: its certificate is its identity (D-02).
    ///
    /// # Errors
    ///
    /// When the renewal fails. The connection then ends and the link reconnects -- signing
    /// in from the start -- which is how a refused or unreachable renewal becomes visible
    /// as a link that is down, rather than a link that looks up while the node refuses it.
    async fn keep(
        &mut self,
        client: &reqwest::Client,
        urls: &Urls,
        credential: Option<&Credential>,
        projection: &Arc<Mutex<Projection>>,
    ) -> Result<(), String> {
        let refused = projection
            .lock()
            .is_ok_and(|mut p| std::mem::take(&mut p.token_refused));
        let Some(credential) = credential else {
            return Ok(());
        };
        let due = self
            .renew_after
            .is_some_and(|after| self.issued_at.elapsed() >= after);
        if !(refused || due) {
            return Ok(());
        }
        let issued = sign_in(client, urls, credential)
            .await
            .map_err(|err| format!("the session could not be renewed: {err}"))?;
        tracing::info!(
            refused,
            expires_s = issued.expires_s,
            "the link renewed its session with the node"
        );
        self.token = issued.token;
        self.issued_at = std::time::Instant::now();
        if let Ok(mut p) = projection.lock() {
            p.token = Some(self.token.clone());
            p.session_renewals = p.session_renewals.saturating_add(1);
        }
        Ok(())
    }
}

/// The node refused the token this request carried (GAP-165): renew before the next
/// offer. A `401` is never an answer about what was sent -- the same request under a
/// current token is a different request.
fn note_refused_token(projection: &Arc<Mutex<Projection>>, status: reqwest::StatusCode) -> bool {
    if status != reqwest::StatusCode::UNAUTHORIZED {
        return false;
    }
    if let Ok(mut p) = projection.lock() {
        p.token_refused = true;
    }
    true
}

/// Post what is queued, oldest first, stopping at the first refusal so nothing is
/// reordered and nothing is lost: a detection leaves the outbox only on `202`.
async fn flush_outbox(
    client: &reqwest::Client,
    urls: &Urls,
    token: &str,
    projection: &Arc<Mutex<Projection>>,
) {
    for _ in 0..64 {
        let Some(detection) = projection
            .lock()
            .ok()
            .and_then(|p| p.outbox.front().cloned())
        else {
            return;
        };
        let answered = with_token(client.post(&urls.detections), token)
            .json(&SubmitDetectionRequest {
                // What this client speaks. The node refuses a mismatch by name
                // rather than leaving it to whether the payload happens to decode.
                schema_version: gungnir_model::SCHEMA_VERSION,
                detection,
            })
            .send()
            .await;
        let accepted = match answered {
            Ok(response) => {
                note_refused_token(projection, response.status());
                response.status().is_success()
            }
            Err(_) => false,
        };
        if !accepted {
            return;
        }
        if let Ok(mut p) = projection.lock() {
            p.outbox.pop_front();
            p.forwarded += 1;
        }
    }
}

/// Deliver the queued sensor tasks, oldest first (GAP-004). Each answer -- the node's
/// task id or its refusal -- goes back to the desktop through the outcomes; an
/// unreachable node leaves the task queued for the next tick, which is the same
/// store-and-forward rule the detections follow.
async fn flush_tasks(
    client: &reqwest::Client,
    urls: &Urls,
    token: &str,
    projection: &Arc<Mutex<Projection>>,
) {
    for _ in 0..16 {
        let Some(task) = projection
            .lock()
            .ok()
            .and_then(|p| p.task_outbox.front().cloned())
        else {
            return;
        };
        let url = format!("{}/{}/task", urls.tasks, task.sensor.0);
        let request = SensorTaskRequest {
            command: task.command.clone(),
            requirement: task.requirement,
        };
        let outcome = match with_token(client.post(&url), token)
            .json(&request)
            .send()
            .await
        {
            // Unreachable: keep it queued and try again next tick.
            Err(_) => return,
            // A lapsed token is not the node's answer to the task (GAP-165): keep it
            // queued, and the link renews before the next tick offers it again.
            Ok(response) if note_refused_token(projection, response.status()) => return,
            Ok(response) if response.status().is_success() => {
                match response.json::<SensorTaskResponse>().await {
                    Ok(answer) => Ok(answer.task),
                    Err(e) => Err(format!("the node's answer could not be decoded: {e}")),
                }
            }
            Ok(response) => {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                Err(format!("the node answered {status}: {body}"))
            }
        };
        if let Ok(mut p) = projection.lock() {
            p.task_outbox.pop_front();
            p.task_outcomes.push(TaskOutcome {
                local: task.local,
                outcome,
            });
        }
    }
}

/// `Warnings`, `Reports` and `Handoffs` are the three items DN-18 §5 amendment 2 gave a
/// write door; `Tracks` and `Health` keep `/v3/snapshot` and `/v3/health` and are never
/// queued by anything this crate builds. `None` rather than a fourth URL nothing would
/// ever use, so a caller error shows up as a dropped batch and a warning instead of a
/// silently wrong URL.
fn exchange_url(urls: &Urls, item: gungnir_model::ExchangeItem) -> Option<&str> {
    match item {
        gungnir_model::ExchangeItem::Warnings => Some(&urls.exchange_warnings),
        gungnir_model::ExchangeItem::Reports => Some(&urls.exchange_reports),
        gungnir_model::ExchangeItem::Handoffs => Some(&urls.exchange_handoffs),
        gungnir_model::ExchangeItem::Tracks | gungnir_model::ExchangeItem::Health => None,
    }
}

/// Deliver the queued exchange publishes, oldest first (GAP-065, DN-18 §5 amendment 2).
/// Each is a full replacement of the node's held set for its item -- see
/// [`NodeLink::queue_exchange`] -- so offering them in order converges the node's held
/// set on this desktop's own.
///
/// **What happens next depends on the answer** (GAP-146, DN-18 §13, D-75; see
/// [`publish_answer`]). A delivery removes the set it sent. No answer, or "not now",
/// keeps it and waits a growing interval before offering it again. A refusal of the
/// caller keeps every set and stops offering until the link signs in again, which is
/// what a `403` for an Operator's console needs: one request per sign-in rather than
/// four a second for as long as the console runs. A rejection of the set drops that one
/// set, counted, and goes on with the rest.
async fn flush_exchange(
    client: &reqwest::Client,
    urls: &Urls,
    token: &str,
    projection: &Arc<Mutex<Projection>>,
) {
    for _ in 0..16 {
        let Some(batch) = projection.lock().ok().and_then(|p| {
            if p.exchange.refused.is_some()
                || p.exchange
                    .retry_at
                    .is_some_and(|at| std::time::Instant::now() < at)
            {
                return None;
            }
            p.exchange_outbox.front().cloned()
        }) else {
            return;
        };
        let Some(url) = exchange_url(urls, batch.item) else {
            tracing::warn!(
                "{:?} has no exchange publish route; dropping the queued batch",
                batch.item
            );
            if let Ok(mut p) = projection.lock() {
                remove_sent(&mut p, batch.generation);
                p.exchange.rejected += 1;
                p.exchange.last_rejection = Some(PublishRefusal {
                    item: batch.item,
                    status: 0,
                    reason: format!("{:?} has no exchange publish route", batch.item),
                });
            }
            continue;
        };
        let request = PublishExchangeRequest {
            products: batch
                .products
                .iter()
                .map(|p| ExchangeProduct {
                    id: p.id.clone(),
                    at: p.at,
                    releasability: p.releasability.clone(),
                    body: p.body.clone(),
                })
                .collect(),
        };
        let (status, reason) = match with_token(client.post(url), token)
            .json(&request)
            .send()
            .await
        {
            Err(err) => (None, format!("the node could not be reached: {err}")),
            // A lapsed token refuses the session, not this console (GAP-165): the set
            // stays queued and nothing is recorded as a refusal of the publisher, which
            // `publish_answer` reads a `401` as and which only a new sign-in would lift.
            Ok(response) if note_refused_token(projection, response.status()) => return,
            Ok(response) => {
                let status = response.status().as_u16();
                let body = if response.status().is_success() {
                    String::new()
                } else {
                    response.text().await.unwrap_or_default()
                };
                (Some(status), node_reason(&body))
            }
        };
        let Ok(mut p) = projection.lock() else {
            return;
        };
        if !record_publish(&mut p, &batch, status, reason) {
            return;
        }
    }
}

/// Fold one publish answer into the projection (GAP-146, D-75), and say whether the flush
/// may go on to the next set: after a delivery or a rejected set it may; after "not now"
/// or a refusal of the caller nothing more is offered this tick.
fn record_publish(
    p: &mut Projection,
    batch: &OutboundExchange,
    status: Option<u16>,
    reason: String,
) -> bool {
    p.exchange.posts += 1;
    match publish_answer(status) {
        PublishAnswer::Delivered => {
            remove_sent(p, batch.generation);
            p.exchange.delivered += 1;
            p.exchange.retrying = None;
            p.exchange.retry_at = None;
            true
        }
        PublishAnswer::Retry => {
            let attempts = p
                .exchange
                .retrying
                .as_ref()
                .map_or(0, |r| r.attempts)
                .saturating_add(1);
            let last_failure = match status {
                Some(status) => format!("{status}: {reason}"),
                None => reason,
            };
            p.exchange.retry_at = Some(std::time::Instant::now() + publish_retry_delay(attempts));
            p.exchange.retrying = Some(PublishRetry {
                attempts,
                last_failure,
            });
            false
        }
        PublishAnswer::CallerRefused => {
            let status = status.unwrap_or_default();
            tracing::warn!(
                status,
                %reason,
                item = ?batch.item,
                "the node refused this console as an exchange publisher; holding the \
                 queued sets and offering nothing more until the link signs in again"
            );
            p.exchange.refused = Some(PublishRefusal {
                item: batch.item,
                status,
                reason,
            });
            p.exchange.retrying = None;
            p.exchange.retry_at = None;
            false
        }
        PublishAnswer::SetRejected => {
            let status = status.unwrap_or_default();
            tracing::warn!(
                status,
                %reason,
                item = ?batch.item,
                "the node rejected an exchange set as sent; dropping it"
            );
            remove_sent(p, batch.generation);
            p.exchange.rejected += 1;
            p.exchange.last_rejection = Some(PublishRefusal {
                item: batch.item,
                status,
                reason,
            });
            true
        }
    }
}

/// Remove the set that was sent, and only it (GAP-146): a newer set queued for the same
/// item while this one was on its way has taken its place and a different generation,
/// and is still to be sent.
fn remove_sent(p: &mut Projection, generation: u64) {
    p.exchange_outbox.retain(|b| b.generation != generation);
}

/// The node's own words from a refusal's body: the `message` of its problem document, or
/// the body itself where that does not decode, bounded so a node that answers with a
/// page of HTML does not put the page on PN-09.
fn node_reason(body: &str) -> String {
    let reason = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(str::to_owned))
        .unwrap_or_else(|| body.trim().to_owned());
    if reason.is_empty() {
        return "the node gave no reason".to_owned();
    }
    reason.chars().take(300).collect()
}

/// Take a picture of the node's approval queue, and replace the projection's waiting
/// list with it (GAP-133, DN-31 §6.6).
///
/// `force` is the starting picture, taken once per connection right after the
/// subscription; otherwise this returns without asking unless the stream has said the
/// queue moved. **Nothing is asked for while the node's queue is still**, which is what
/// keeps this event-driven rather than a poll.
///
/// A failed fetch leaves the projection as it was and says so: the stale mark stays set,
/// so the next frame asks again. It does not clear the queue, because an unanswered
/// request is not evidence that nothing is waiting.
async fn refresh_queue(
    client: &reqwest::Client,
    urls: &Urls,
    token: &str,
    projection: &Arc<Mutex<Projection>>,
    force: bool,
) {
    if !force {
        let wanted = projection.lock().is_ok_and(|p| p.queue.is_stale());
        if !wanted {
            return;
        }
    }
    let response = match with_token(client.get(&urls.queue), token).send().await {
        Ok(response) if response.status().is_success() => response,
        Ok(response) if note_refused_token(projection, response.status()) => return,
        Ok(response) => {
            tracing::warn!(
                status = %response.status(),
                "the node refused the approval queue; keeping the last picture"
            );
            return;
        }
        Err(err) => {
            tracing::warn!(%err, "could not read the node's approval queue");
            return;
        }
    };
    match response.json::<Vec<gungnir_api::v3::QueueItemView>>().await {
        Ok(items) => {
            if let Ok(mut p) = projection.lock() {
                p.queue.take_picture(items);
            }
        }
        Err(err) => tracing::warn!(%err, "the node's approval queue could not be decoded"),
    }
}

/// Post the decisions this desktop has taken on the node's queue, oldest first
/// (GAP-133, DN-31 §6.3).
///
/// **Answers are told from non-answers.** `flush_outbox` stops at the first refusal and
/// tries the same item again next tick; `flush_exchange` reads the answer too, for its
/// own reasons (GAP-146, [`publish_answer`]); this one distinguishes an *answer* from *no
/// answer*. A `201`, a `409`
/// and a `400`/`403` are answers and are delivered to the caller, which then knows what
/// stands. A `401` is not: it refuses the link's lapsed token, not the decision, so the
/// link renews its session and the same request goes again (GAP-165). A `504` -- the node's loop did not reply inside the route's window -- is
/// not an answer and **does not mean nothing was recorded**, so the same request key goes
/// back to the same route until the node says which. That is the whole reason the key
/// exists (DN-31 §5.2).
async fn flush_decisions(
    client: &reqwest::Client,
    urls: &Urls,
    token: &str,
    projection: &Arc<Mutex<Projection>>,
) {
    for _ in 0..16 {
        let Some(decision) = projection
            .lock()
            .ok()
            .and_then(|p| p.decision_outbox.front().cloned())
        else {
            return;
        };
        let url = format!("{}/{}/decision", urls.queue, decision.item);
        let request = gungnir_api::v3::DecisionRequest {
            request: decision.request.clone(),
            item: decision.item,
            choice: decision.choice.clone(),
        };
        let response = match with_token(client.post(&url), token)
            .json(&request)
            .send()
            .await
        {
            Ok(response) => response,
            // Unreachable. Keep it queued under the same key and try again: the node may
            // have recorded it before the connection failed, and the key is what lets the
            // retry find out instead of deciding twice.
            Err(err) => {
                tracing::warn!(%err, item = %decision.item, "the node could not be reached with a decision");
                count_attempt(projection);
                return;
            }
        };
        // A lapsed token is not the node's answer to the decision (GAP-165): the same
        // request, under the same key, goes again once the link has renewed.
        if note_refused_token(projection, response.status()) {
            count_attempt(projection);
            return;
        }
        let status = response.status().as_u16();
        if crate::queue::retry_under_same_key(status) {
            tracing::info!(
                item = %decision.item,
                status,
                request = %decision.request,
                "the node has not answered this decision yet; retrying under the same key"
            );
            count_attempt(projection);
            return;
        }
        let body = response.text().await.unwrap_or_default();
        let answer = answer_of(status, &body);
        if let Ok(mut p) = projection.lock() {
            p.decision_outbox.pop_front();
            // A `201` or a `409` both mean the item is no longer waiting on a person, and
            // this desktop has that first-hand. The item leaves PN-06 now rather than
            // when the stream catches up, so a second click cannot earn a refusal naming
            // a decision that has already been made (DN-31 §6.3, §6.6).
            if matches!(
                answer,
                crate::queue::DecisionAnswer::Recorded { .. }
                    | crate::queue::DecisionAnswer::Refused(_)
            ) {
                p.queue.recorded_here(decision.item);
            }
            p.decision_outcomes.push(crate::queue::DecisionOutcome {
                request: decision.request,
                item: decision.item,
                answer,
            });
        }
    }
}

/// Post the outages' batches, oldest first (GAP-134, DN-31 §6.8).
///
/// **The same rule as [`flush_decisions`]**: a `202`, a `409` and a `400`/`403` are
/// answers and are delivered; a `504` or `503`, a `401` (renewed first, GAP-165), or no
/// connection at all, is not, and the same batch goes again. That is safe because the node keys the batch on each decision's
/// own identifier, answering a repeat `already_held` and recording nothing -- which is
/// "forwarding twice records nothing new" seen from this side.
async fn flush_forwarded(
    client: &reqwest::Client,
    urls: &Urls,
    token: &str,
    projection: &Arc<Mutex<Projection>>,
) {
    for _ in 0..4 {
        let Some(batch) = projection
            .lock()
            .ok()
            .and_then(|p| p.forward_outbox.front().cloned())
        else {
            return;
        };
        let response = match with_token(client.post(&urls.forwarded), token)
            .json(&batch.decisions)
            .send()
            .await
        {
            Ok(response) => response,
            Err(err) => {
                tracing::warn!(%err, decisions = batch.decisions.len(), "the node could not be reached with an outage's decisions");
                count_forward_attempt(projection);
                return;
            }
        };
        // As for a decision (GAP-165): renewed, then the same batch again.
        if note_refused_token(projection, response.status()) {
            count_forward_attempt(projection);
            return;
        }
        let status = response.status().as_u16();
        if crate::queue::retry_under_same_key(status) {
            tracing::info!(
                status,
                decisions = batch.decisions.len(),
                "the node has not answered an outage's decisions yet; sending the same batch again"
            );
            count_forward_attempt(projection);
            return;
        }
        let body = response.text().await.unwrap_or_default();
        let reply = forward_reply_of(status, &body);
        if let Ok(mut p) = projection.lock() {
            p.forward_outbox.pop_front();
            p.forward_replies.push(reply);
        }
    }
}

/// One more attempt against the batch at the head of the forward outbox.
fn count_forward_attempt(projection: &Arc<Mutex<Projection>>) {
    if let Ok(mut p) = projection.lock() {
        if let Some(head) = p.forward_outbox.front_mut() {
            head.attempts = head.attempts.saturating_add(1);
        }
    }
}

/// What the node's answer to a batch means (DN-31 §7). A body that does not decode is
/// reported as the status and the text, never guessed at, for the reason [`answer_of`]
/// gives.
fn forward_reply_of(status: u16, body: &str) -> crate::queue::ForwardReply {
    use crate::queue::ForwardReply;
    if status == 202 {
        return match serde_json::from_str::<gungnir_api::v3::ForwardAccepted>(body) {
            Ok(accepted) => ForwardReply::Accepted(accepted),
            Err(err) => ForwardReply::Rejected {
                status,
                reason: format!(
                    "the node took the outage's decisions and its answer could not be read: {err}"
                ),
            },
        };
    }
    if status == 409 {
        return match serde_json::from_str::<gungnir_api::v3::ForwardRefused>(body) {
            Ok(refused) => ForwardReply::Refused(Box::new(refused)),
            Err(err) => ForwardReply::Rejected {
                status,
                reason: format!("the node refused the outage's decisions for a reason this desktop could not read: {err}"),
            },
        };
    }
    ForwardReply::Rejected {
        status,
        reason: body.trim().to_owned(),
    }
}

/// One more attempt against the decision at the head of the outbox.
///
/// Counted rather than only logged, so PN-07 can say a decision is still in flight after
/// several tries instead of showing a dialog that looks as though nothing happened.
fn count_attempt(projection: &Arc<Mutex<Projection>>) {
    if let Ok(mut p) = projection.lock() {
        if let Some(head) = p.decision_outbox.front_mut() {
            head.attempts = head.attempts.saturating_add(1);
        }
    }
}

/// What the node's answer to a decision means (DN-31 §6.3).
///
/// A body that does not decode is reported as the status and the text rather than
/// guessed at: this is the answer a person is shown on PN-07, and inventing a refusal
/// reason would be worse than saying the node answered something this desktop could not
/// read.
fn answer_of(status: u16, body: &str) -> crate::queue::DecisionAnswer {
    use crate::queue::DecisionAnswer;
    if status == 201 {
        return match serde_json::from_str::<gungnir_api::v3::DecisionRecorded>(body) {
            Ok(recorded) => DecisionAnswer::Recorded {
                decision: recorded.decision,
            },
            Err(err) => DecisionAnswer::Rejected {
                status,
                reason: format!("the node recorded a decision this desktop could not read: {err}"),
            },
        };
    }
    if status == 409 {
        return match serde_json::from_str::<gungnir_api::v3::DecisionRefused>(body) {
            Ok(refused) => DecisionAnswer::Refused(refused),
            Err(err) => DecisionAnswer::Rejected {
                status,
                reason: format!("the node refused this decision for a reason this desktop could not read: {err}"),
            },
        };
    }
    DecisionAnswer::Rejected {
        status,
        reason: body.trim().to_owned(),
    }
}

type Frame = Result<
    Option<Result<tokio_tungstenite::tungstenite::Message, tokio_tungstenite::tungstenite::Error>>,
    tokio::time::error::Elapsed,
>;

/// One frame of the stream, or the reason the stream is over.
fn handle_frame(next: Frame, projection: &Arc<Mutex<Projection>>) -> Result<(), String> {
    let frame = match next {
        Err(_) => {
            return Err(format!(
                "the node sent nothing for {} s, not even a heartbeat",
                gungnir_api::transport::HEARTBEAT_TIMEOUT.as_secs()
            ))
        }
        Ok(None) => return Err("the node closed the stream".to_string()),
        Ok(Some(frame)) => frame,
    };
    let frame = frame.map_err(|e| format!("event stream failed: {e}"))?;
    // Every frame counts, the heartbeat ping included: what is being measured is
    // whether the node is there, not whether it has anything to say.
    if let Ok(mut p) = projection.lock() {
        p.last_heard = Some(std::time::Instant::now());
    }
    let tokio_tungstenite::tungstenite::Message::Text(text) = frame else {
        return Ok(());
    };
    // The node sends plain text rather than an envelope when it cannot serve the
    // stream -- a `from_seq` older than it retains, or a subscriber that fell
    // behind. Both mean the projection is no longer trustworthy, so say so and let
    // the reconnection take a fresh snapshot.
    let Ok(envelope) = serde_json::from_str::<Envelope>(&text) else {
        return Err(format!("the node ended the stream: {text}"));
    };
    apply(projection, &envelope)
}

/// What a history fetch came back with (GAP-050).
#[derive(Debug, Clone, PartialEq)]
pub enum HistoryOutcome {
    /// The envelopes from the requested sequence onward.
    Complete(Vec<Envelope>),
    /// The node's window no longer reaches that far back (`410 Gone`).
    Gone { reason: String },
    /// The request failed: refused, or the node could not be reached.
    Unreachable { reason: String },
}

/// A history fetch in flight. Poll it from the frame loop; it never blocks.
#[derive(Debug)]
pub struct PendingHistory {
    rx: std::sync::mpsc::Receiver<HistoryOutcome>,
}

impl PendingHistory {
    /// The outcome once it has arrived.
    #[must_use]
    pub fn poll(&self) -> Option<HistoryOutcome> {
        match self.rx.try_recv() {
            Ok(outcome) => Some(outcome),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(HistoryOutcome::Unreachable {
                reason: "the fetch task ended without answering".into(),
            }),
        }
    }
}

/// Fetch the node's retained envelopes from `since_seq` (GAP-050), with `token`.
///
/// # Errors
///
/// `RemoteError::InvalidEndpoint` for an endpoint the link would refuse too.
pub fn fetch_history(
    endpoint: &RemoteEndpoint,
    token: &str,
    since_seq: u64,
    handle: &tokio::runtime::Handle,
) -> Result<PendingHistory, RemoteError> {
    let urls = urls(endpoint)?;
    let (tx, rx) = std::sync::mpsc::channel();
    let token = token.to_owned();
    let tls = endpoint.tls.clone();
    handle.spawn(async move {
        let client = match http_client(&tls) {
            Ok(client) => client,
            Err(reason) => {
                let _ = tx.send(HistoryOutcome::Unreachable { reason });
                return;
            }
        };
        // The query is written by hand: `reqwest`'s `query` feature is off in §2.9's
        // pin, and one integer needs no encoder.
        let outcome = match with_token(
            client.get(format!("{}?since_seq={since_seq}", urls.history)),
            &token,
        )
        .send()
        .await
        {
            Ok(response) if response.status().is_success() => {
                match response.json::<HistoryResponse>().await {
                    Ok(history) => HistoryOutcome::Complete(history.envelopes),
                    Err(e) => HistoryOutcome::Unreachable {
                        reason: format!("the history response could not be decoded: {e}"),
                    },
                }
            }
            Ok(response) if response.status() == reqwest::StatusCode::GONE => {
                HistoryOutcome::Gone {
                    reason: response
                        .text()
                        .await
                        .unwrap_or_default()
                        .chars()
                        .take(200)
                        .collect(),
                }
            }
            Ok(response) => HistoryOutcome::Unreachable {
                reason: format!("the node answered {}", response.status()),
            },
            Err(e) => HistoryOutcome::Unreachable {
                reason: e.to_string(),
            },
        };
        let _ = tx.send(outcome);
    });
    Ok(PendingHistory { rx })
}

/// A bearer token on the request when there is one. A machine link carries none: its
/// certificate is its identity, and an empty `Authorization` header would read as a
/// malformed operator rather than a machine.
fn with_token(request: reqwest::RequestBuilder, token: &str) -> reqwest::RequestBuilder {
    if token.is_empty() {
        request
    } else {
        request.bearer_auth(token)
    }
}

/// Fold one envelope into the projection.
///
/// The tracking, intercept and command events change it, and the rest are carried on the
/// same stream for the journal and other subscribers: a client that tried to interpret
/// those would be building a second, divergent picture. The command events are here
/// because since D-55 the node's queue is the queue, so reading it is projecting what the
/// node holds rather than deciding anything a second time.
fn apply(projection: &Arc<Mutex<Projection>>, envelope: &Envelope) -> Result<(), String> {
    let mut p = projection
        .lock()
        .map_err(|_| "the projection lock was poisoned".to_owned())?;
    p.last_seq = p.last_seq.max(envelope.seq);
    match &envelope.event {
        Event::Tracking(
            TrackingEvent::TrackInitiated(track) | TrackingEvent::TrackUpdated(track),
        ) => {
            match p.tracks.iter_mut().find(|t| t.id == track.id) {
                Some(existing) => *existing = track.clone(),
                None => p.tracks.push(track.clone()),
            }
            if !p.changed.contains(&track.id) {
                p.changed.push(track.id);
            }
        }
        // GAP-009, DN-16 §5: a launch warning is taken as itself and never becomes a
        // track. Only `Issued` is taken -- `Received` and `Refused` on a node's stream
        // are that node's record of what *its* peers told it, and reading them here
        // would turn a second-hand report into a first-hand one.
        Event::LaunchWarning(gungnir_model::events::LaunchWarningEvent::Issued(warning)) => {
            if p.launch_warnings.len() >= LAUNCH_WARNING_CAPACITY {
                p.launch_warnings.pop_front();
                p.launch_warnings_dropped += 1;
                if p.launch_warnings_dropped == 1 {
                    tracing::warn!("the link's launch-warning queue is full; dropping the oldest");
                }
            }
            p.launch_warnings.push_back(warning.clone());
        }
        // For the host, not the picture (GAP-004, GAP-040).
        Event::SensorTask(_) | Event::Handoff(_) => {
            if p.inbox.len() >= INBOX_CAPACITY {
                p.inbox.pop_front();
                p.inbox_dropped += 1;
            }
            p.inbox.push_back(envelope.clone());
        }
        Event::Tracking(TrackingEvent::TrackDeleted(id)) => {
            p.tracks.retain(|t| t.id != *id);
        }
        Event::Intercept(
            InterceptEvent::PlanProposed(plan) | InterceptEvent::PlanApproved(plan),
        ) => {
            p.plan = plan.clone();
        }
        // GAP-161: the node's services, as the node reports them. See
        // `Projection::node_health`.
        Event::Health(HealthEvent::Changed {
            tracking_healthy,
            intercept_healthy,
            ingest_healthy,
            ..
        }) => {
            p.node_health = Some(gungnir_model::SystemHealth {
                tracking_healthy: *tracking_healthy,
                intercept_healthy: *intercept_healthy,
                ingest_healthy: *ingest_healthy,
            });
        }
        // The node's approval queue (GAP-133, DN-31 §6.6). Taken as itself rather than
        // put on the inbox, because unlike a sensor task or an effector report this is a
        // *picture* the desktop projects and not an act the host performs: PN-06 draws it
        // and nothing in `gungnir-app` records anything from it.
        //
        // These four variants were dropped by this function's `_ => {}` until GAP-133, so
        // a node decided in its own queue and every linked desktop saw nothing of it.
        Event::Command(command) => p.queue.note(command, envelope.mission_time),
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(url: &str) -> RemoteEndpoint {
        RemoteEndpoint::plain(url)
    }

    #[test]
    fn a_base_url_becomes_the_two_contract_endpoints() {
        let urls = urls(&endpoint("http://127.0.0.1:7410")).expect("valid");
        assert_eq!(urls.snapshot, "http://127.0.0.1:7410/v3/snapshot");
        assert_eq!(urls.events, "ws://127.0.0.1:7410/v3/events");
    }

    /// GAP-143: a machine link keeps presenting its certificate whoever signs in on the
    /// desktop, and since a link now holds its operator's credential where `Debug` can
    /// reach it, no formatting of either prints the passphrase.
    #[tokio::test]
    async fn a_machine_link_takes_no_credential_and_no_link_prints_a_passphrase() {
        let secret = "correct horse battery staple";
        let machine = start_with(
            &endpoint("http://127.0.0.1:9"),
            None,
            &tokio::runtime::Handle::current(),
        )
        .expect("started");
        let refused = machine.replace_credential(Credential {
            operator: 7,
            passphrase: secret.into(),
        });
        assert!(
            refused.is_err(),
            "a machine link took an operator's credential"
        );
        assert_eq!(machine.signs_in_as(), None);

        let operator = NodeLink::scripted();
        operator
            .replace_credential(Credential {
                operator: 7,
                passphrase: secret.into(),
            })
            .expect("an operator's link takes one");
        assert_eq!(operator.signs_in_as(), Some(7));
        let shown = format!(
            "{operator:?} {machine:?} {:?}",
            Credential {
                operator: 7,
                passphrase: secret.into(),
            }
        );
        assert!(
            !shown.contains(secret),
            "a passphrase reached a Debug string: {shown}"
        );
        assert!(shown.contains("<redacted>"), "{shown}");
    }

    /// GAP-065, DN-18 §5 amendment 2: the three write doors, one URL apiece; `Tracks` and
    /// `Health` have none, since they keep `/v3/snapshot` and `/v3/health`.
    #[test]
    fn the_three_exchange_items_with_a_write_door_each_resolve_and_the_other_two_do_not() {
        let urls = urls(&endpoint("http://127.0.0.1:7410")).expect("valid");
        assert_eq!(
            urls.exchange_warnings,
            "http://127.0.0.1:7410/v3/exchange/warnings"
        );
        assert_eq!(
            exchange_url(&urls, gungnir_model::ExchangeItem::Warnings),
            Some(urls.exchange_warnings.as_str())
        );
        assert_eq!(
            exchange_url(&urls, gungnir_model::ExchangeItem::Reports),
            Some(urls.exchange_reports.as_str())
        );
        assert_eq!(
            exchange_url(&urls, gungnir_model::ExchangeItem::Handoffs),
            Some(urls.exchange_handoffs.as_str())
        );
        assert_eq!(
            exchange_url(&urls, gungnir_model::ExchangeItem::Tracks),
            None
        );
        assert_eq!(
            exchange_url(&urls, gungnir_model::ExchangeItem::Health),
            None
        );
    }

    /// GAP-065: queuing hands the whole batch to the outbox, oldest first, and does not
    /// touch a batch already queued for a different item.
    #[test]
    fn queueing_an_exchange_batch_appends_to_the_outbox() {
        let link = NodeLink::scripted();
        link.queue_exchange(
            gungnir_model::ExchangeItem::Handoffs,
            vec![ExchangeProductRecord {
                id: "decision-1".into(),
                at: gungnir_model::MissionTime(1.0),
                releasability: gungnir_model::Releasability::AllPeers,
                body: serde_json::json!({"decision": 1}),
            }],
        );
        let p = link.read().expect("projection");
        assert_eq!(p.exchange_outbox.len(), 1);
        assert_eq!(
            p.exchange_outbox[0].item,
            gungnir_model::ExchangeItem::Handoffs
        );
        assert_eq!(p.exchange_outbox[0].products[0].id, "decision-1");
    }

    fn product(id: &str) -> ExchangeProductRecord {
        ExchangeProductRecord {
            id: id.into(),
            at: gungnir_model::MissionTime(1.0),
            releasability: gungnir_model::Releasability::AllPeers,
            body: serde_json::json!({ "id": id }),
        }
    }

    /// GAP-146, D-76: the outbox holds at most one set per item however many are queued
    /// while nothing is taken, the one it holds is the newest, and every replacement is
    /// counted. A set for another item is left alone.
    #[test]
    fn a_newer_set_replaces_a_waiting_one_and_the_replacement_is_counted() {
        use gungnir_model::ExchangeItem::{Handoffs, Warnings};
        let link = NodeLink::scripted();
        link.queue_exchange(Warnings, vec![product("w-1")]);
        for n in 1..=500u32 {
            let set = (1..=n).map(|i| product(&format!("h-{i}"))).collect();
            link.queue_exchange(Handoffs, set);
        }
        let standing = link.exchange_standing().expect("readable");
        assert_eq!(
            standing.waiting,
            vec![Warnings, Handoffs],
            "one set per item, in the order each item was first queued"
        );
        assert_eq!(standing.publishing.superseded, 499);
        let p = link.read().expect("projection");
        let handoffs = p
            .exchange_outbox
            .iter()
            .find(|b| b.item == Handoffs)
            .expect("held");
        assert_eq!(
            handoffs.products.len(),
            500,
            "the newest set is the one held"
        );
        assert_eq!(handoffs.products[499].id, "h-500");
        assert_eq!(
            p.exchange_outbox[0].products[0].id, "w-1",
            "a set for another item is not touched"
        );
    }

    /// GAP-146: removing what was sent removes that generation only, so a newer set
    /// queued while the older one was on its way survives to be sent after it.
    #[test]
    fn a_set_replaced_while_in_flight_is_not_lost_when_the_old_one_lands() {
        let link = NodeLink::scripted();
        link.queue_exchange(gungnir_model::ExchangeItem::Handoffs, vec![product("h-1")]);
        let sent = link.read().expect("projection").exchange_outbox[0].generation;
        link.queue_exchange(
            gungnir_model::ExchangeItem::Handoffs,
            vec![product("h-1"), product("h-2")],
        );
        let mut p = link.projection.lock().expect("projection");
        remove_sent(&mut p, sent);
        assert_eq!(
            p.exchange_outbox.len(),
            1,
            "the newer set was removed with the old"
        );
        assert_eq!(p.exchange_outbox[0].products.len(), 2);
    }

    /// GAP-146, D-75: a refusal of who is asking is told from one of what was sent, and
    /// both from a node that did not answer. `507` -- the register is full -- is a node
    /// saying "not now", and stays one.
    #[test]
    fn a_publish_answer_is_read_for_what_it_says_about_the_caller() {
        assert_eq!(publish_answer(Some(202)), PublishAnswer::Delivered);
        assert_eq!(publish_answer(Some(200)), PublishAnswer::Delivered);
        for status in [401u16, 403] {
            assert_eq!(publish_answer(Some(status)), PublishAnswer::CallerRefused);
        }
        for status in [400u16, 404, 405, 409, 413, 415, 422] {
            assert_eq!(publish_answer(Some(status)), PublishAnswer::SetRejected);
        }
        for status in [
            None,
            Some(408),
            Some(429),
            Some(500),
            Some(503),
            Some(504),
            Some(507),
        ] {
            assert_eq!(publish_answer(status), PublishAnswer::Retry, "{status:?}");
        }
        assert_eq!(
            publish_answer(Some(302)),
            PublishAnswer::SetRejected,
            "a redirect is not an acceptance"
        );
    }

    /// GAP-146: a node that keeps answering "not now" is asked less and less often, from
    /// one forward tick to the ceiling, and never more rarely than the ceiling.
    #[test]
    fn a_retry_backs_off_to_a_ceiling() {
        assert_eq!(publish_retry_delay(1), FORWARD_INTERVAL);
        assert_eq!(publish_retry_delay(2), FORWARD_INTERVAL * 2);
        assert_eq!(publish_retry_delay(3), FORWARD_INTERVAL * 4);
        assert_eq!(publish_retry_delay(8), PUBLISH_RETRY_CEILING);
        assert_eq!(publish_retry_delay(u32::MAX), PUBLISH_RETRY_CEILING);
        assert_eq!(publish_retry_delay(0), FORWARD_INTERVAL);
    }

    /// The node's problem document is read for its message, and anything else is shown
    /// as it came, bounded.
    #[test]
    fn a_refusal_is_given_in_the_node_s_own_words() {
        assert_eq!(
            node_reason(
                r#"{"error":403,"message":"role Operator may not publish to exchange (exchange.publish)"}"#
            ),
            "role Operator may not publish to exchange (exchange.publish)"
        );
        assert_eq!(node_reason("  plain text  "), "plain text");
        assert_eq!(node_reason(""), "the node gave no reason");
        assert_eq!(node_reason(&"x".repeat(1000)).len(), 300);
    }

    #[test]
    fn a_trailing_slash_does_not_double_up() {
        let urls = urls(&endpoint("http://127.0.0.1:7410/")).expect("valid");
        assert_eq!(urls.snapshot, "http://127.0.0.1:7410/v3/snapshot");
    }

    /// An https endpoint with no pinned roots is refused rather than trusted blindly or
    /// quietly downgraded; with roots it is spoken as wss.
    #[test]
    fn an_https_endpoint_needs_pinned_roots_and_is_then_wss() {
        let err = urls(&endpoint("https://node.local:7410")).expect_err("refused");
        assert!(err.to_string().contains("trust roots"), "{err}");
        let mut with_roots = endpoint("https://node.local:7410");
        with_roots
            .tls
            .trust_roots_pem
            .push("-----BEGIN CERTIFICATE-----".into());
        let urls = urls(&with_roots).expect("spoken");
        assert!(urls.tls);
        assert_eq!(urls.events, "wss://node.local:7410/v3/events");
        assert_eq!(urls.host, "node.local");
        assert_eq!(urls.port, 7410);
    }

    #[test]
    fn an_empty_or_hostless_url_is_refused() {
        assert!(urls(&endpoint("   ")).is_err());
        assert!(urls(&endpoint("http://")).is_err());
    }

    /// A fresh projection claims nothing: no tracks, no plan, and not connected.
    #[test]
    fn a_fresh_projection_is_not_connected() {
        let projection = Projection::default();
        assert!(!projection.connected);
        assert!(projection.tracks.is_empty());
        assert_eq!(projection.last_seq, 0);
    }
}
