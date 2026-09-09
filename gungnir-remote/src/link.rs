// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The client half of the v2 transport (GAP-041).
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

use gungnir_api::v2::{
    ExchangeProduct, HistoryResponse, PublishExchangeRequest, SensorTaskRequest,
    SensorTaskResponse, SessionRequest, SessionResponse, SnapshotResponse, SubmitDetectionRequest,
    SubscribeRequest,
};
use gungnir_eventing::{Envelope, Event};
use gungnir_intercept_service::PlanView;
use gungnir_model::events::{InterceptEvent, TrackingEvent};
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
    /// The most recent transport failure, for the status strip.
    pub last_error: Option<String>,
    /// Sequence number of the last envelope applied, so a reconnection resumes rather
    /// than replaying what has already been seen.
    pub last_seq: u64,
    /// The session token the link signed in with, for requests the desktop makes
    /// outside the stream (GAP-050's history fetch). Short-lived by design (DN-23 §5);
    /// a request refused for an expired token says so and the desktop signs in again.
    pub token: Option<String>,
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
    pub exchange_outbox: std::collections::VecDeque<OutboundExchange>,
    /// When the node was last heard from at all -- an envelope, a heartbeat, the
    /// snapshot (D-23).
    ///
    /// Wall time, not mission time: this is about the liveness of a socket, and a replayed
    /// session has no socket. `None` until the first snapshot lands, which is a different
    /// claim from "heard a long time ago" and the strip keeps them apart.
    pub last_heard: Option<std::time::Instant>,
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
}

/// One product within an [`OutboundExchange`] (GAP-065).
///
/// Mirrors `gungnir_api::v2::ExchangeProduct` field for field rather than reusing it --
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
        }
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

    /// Hand this desktop's current held set for `item` to the link, to replace what the
    /// node holds (GAP-065, DN-18 §5 amendment 2).
    ///
    /// **A replacement, not an addition**, mirroring
    /// `gungnir_api::transport::NodeApi::publish_exchange`'s own contract: `products` is
    /// this producer's whole current set for `item`, not a diff against what was queued
    /// before. Queuing a fresher batch does not remove an older one already in flight for
    /// the same item; both are sent in order, and since each is a full replacement the
    /// node's held set still converges on the last one applied.
    pub fn queue_exchange(
        &self,
        item: gungnir_model::ExchangeItem,
        products: Vec<ExchangeProductRecord>,
    ) {
        if let Ok(mut p) = self.projection.lock() {
            p.exchange_outbox
                .push_back(OutboundExchange { item, products });
        }
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
    /// True for an `https` endpoint: the stream is `wss` over our own TLS stream.
    tls: bool,
    host: String,
    port: u16,
}

/// Turn the configured base URL into the endpoints of the v2 contract.
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
        history: format!("{base}/v2/history"),
        detections: format!("{base}/v2/detections"),
        session: format!("{base}/v2/session"),
        snapshot: format!("{base}/v2/snapshot"),
        tasks: format!("{base}/v2/sensors"),
        exchange_warnings: format!("{base}/v2/exchange/warnings"),
        exchange_reports: format!("{base}/v2/exchange/reports"),
        exchange_handoffs: format!("{base}/v2/exchange/handoffs"),
        events: format!("{scheme}://{rest}/v2/events"),
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credential {
    pub operator: u64,
    pub passphrase: String,
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
    let projection = Arc::new(Mutex::new(Projection::default()));
    let (revision_tx, revision_rx) = watch::channel(0u64);
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

    let task_projection = Arc::clone(&projection);
    handle.spawn(async move {
        loop {
            if let Err(err) = run_link(
                &urls,
                &tls,
                credential.as_ref(),
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
    })
}

fn set_disconnected(projection: &Arc<Mutex<Projection>>, reason: &str) {
    if let Ok(mut p) = projection.lock() {
        p.connected = false;
        p.last_error = Some(reason.to_owned());
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
    let token = match credential {
        Some(credential) => {
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
            let session: SessionResponse = issued
                .json()
                .await
                .map_err(|e| format!("the session response could not be decoded: {e}"))?;
            session.token
        }
        None => String::new(),
    };

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
        p.token = Some(token.clone());
        p.connected = true;
        p.last_error = None;
        p.last_heard = Some(std::time::Instant::now());
        p.last_seq
    };
    revision.send_modify(|r| *r = r.wrapping_add(1));

    let mut socket = open_stream(urls, tls).await?;

    let subscribe = serde_json::to_string(&SubscribeRequest {
        from_seq,
        token: token.clone(),
    })
    .map_err(|e| format!("could not encode the subscribe frame: {e}"))?;
    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            subscribe.into(),
        ))
        .await
        .map_err(|e| format!("could not subscribe: {e}"))?;

    // Anything at all resets the clock: an envelope, or the server's heartbeat ping,
    // which `tokio-tungstenite` answers for us. Silence past the timeout means the link
    // is gone even though the socket has not said so, which is the case a "connected"
    // light would otherwise get wrong for as long as the operating system kept the
    // connection open.
    //
    // Between frames the outbox is forwarded (§8.4): whatever the desktop queued while
    // the node was away, or since the last flush, goes to `POST /v2/detections` under the
    // same token, and stays queued until the node has said `202`.
    let mut forward = tokio::time::interval(FORWARD_INTERVAL);
    loop {
        tokio::select! {
            _ = forward.tick() => {
                flush_outbox(&client, urls, &token, projection).await;
                flush_tasks(&client, urls, &token, projection).await;
                flush_exchange(&client, urls, &token, projection).await;
            }
            next = tokio::time::timeout(gungnir_api::transport::HEARTBEAT_TIMEOUT, socket.next()) => {
                handle_frame(next, projection)?;
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
        let accepted = with_token(client.post(&urls.detections), token)
            .json(&SubmitDetectionRequest {
                // What this client speaks. The node refuses a mismatch by name
                // rather than leaving it to whether the payload happens to decode.
                schema_version: gungnir_model::SCHEMA_VERSION,
                detection,
            })
            .send()
            .await
            .is_ok_and(|r| r.status().is_success());
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
/// write door; `Tracks` and `Health` keep `/v2/snapshot` and `/v2/health` and are never
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
/// [`NodeLink::queue_exchange`] -- so applying them in order and stopping at the first
/// refusal is enough to converge the node's held set on this desktop's own, the same
/// store-and-forward rule [`flush_outbox`] and [`flush_tasks`] follow.
async fn flush_exchange(
    client: &reqwest::Client,
    urls: &Urls,
    token: &str,
    projection: &Arc<Mutex<Projection>>,
) {
    for _ in 0..16 {
        let Some(batch) = projection
            .lock()
            .ok()
            .and_then(|p| p.exchange_outbox.front().cloned())
        else {
            return;
        };
        let Some(url) = exchange_url(urls, batch.item) else {
            tracing::warn!(
                "{:?} has no exchange publish route; dropping the queued batch",
                batch.item
            );
            if let Ok(mut p) = projection.lock() {
                p.exchange_outbox.pop_front();
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
        let accepted = with_token(client.post(url), token)
            .json(&request)
            .send()
            .await
            .is_ok_and(|r| r.status().is_success());
        if !accepted {
            // Unreachable or refused: keep it queued and try again next tick.
            return;
        }
        if let Ok(mut p) = projection.lock() {
            p.exchange_outbox.pop_front();
        }
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
/// Only the tracking and intercept events change it. The rest are carried on the same
/// stream for the journal and other subscribers, and a client that tried to interpret
/// them would be building a second, divergent picture.
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
        assert_eq!(urls.snapshot, "http://127.0.0.1:7410/v2/snapshot");
        assert_eq!(urls.events, "ws://127.0.0.1:7410/v2/events");
    }

    /// GAP-065, DN-18 §5 amendment 2: the three write doors, one URL apiece; `Tracks` and
    /// `Health` have none, since they keep `/v2/snapshot` and `/v2/health`.
    #[test]
    fn the_three_exchange_items_with_a_write_door_each_resolve_and_the_other_two_do_not() {
        let urls = urls(&endpoint("http://127.0.0.1:7410")).expect("valid");
        assert_eq!(
            urls.exchange_warnings,
            "http://127.0.0.1:7410/v2/exchange/warnings"
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

    #[test]
    fn a_trailing_slash_does_not_double_up() {
        let urls = urls(&endpoint("http://127.0.0.1:7410/")).expect("valid");
        assert_eq!(urls.snapshot, "http://127.0.0.1:7410/v2/snapshot");
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
        assert_eq!(urls.events, "wss://node.local:7410/v2/events");
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
