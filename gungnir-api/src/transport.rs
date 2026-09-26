// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The v3 transport: JSON over HTTP and a WebSocket event stream (GAP-041).
//!
//! Decided in `docs/gungnir-api-v1.md` and D-18; the crates are `axum` with its `ws`
//! feature, recorded in `agentic-coding-standards.md` §2.9.
//!
//! # What this serves, and what it refuses
//!
//! **The read paths are real**: a snapshot, a health summary, and an event stream that
//! honours `SubscribeRequest::from_seq`. A desktop can connect to a node and follow the
//! live picture, which is the half of MT-10 that was impossible before.
//!
//! **Every route but `POST /v3/session` requires a session token** (DN-23 §6, GAP-057).
//! The token is minted by the node against its own account store and verified per
//! request; a caller who presents none, or a bad one, gets `401`. **No route believes a
//! body that names its own operator**: the caller is whoever the token says, and nobody
//! else. [`v3::DecisionRequest`] names no operator at all, for that reason. The plan-keyed
//! `ApprovalRequest`, which named its own operator in the body and which no route had
//! served since GAP-132, was removed by GAP-138.
//!
//! A node with no caller authority configured refuses every route but the session one,
//! and says so. That is a deployment with no account store, which is the default: it
//! runs its pipeline, journals it, and serves nobody.
//!
//! **`POST /v3/detections` is served, and so is the decision route** -- which until
//! GAP-132 was the one write path that refused. A submitted detection is queued for the
//! ingest gateway, which authenticates and validates it exactly as it does a sensor's --
//! see [`NodeApi::submit_detection`].
//!
//! # The queue, and why its routes wait for the loop
//!
//! D-55 gave the node the approval queue for the desktops linked to it, so
//! `GET /v3/queue` serves what is waiting and `POST /v3/queue/{item}/decision` takes a
//! person's decision on one item. **Neither decides anything here.** The queue lives on
//! the node loop, and the decision route hands its request over through
//! [`PendingDecision`] and waits for the loop's answer, exactly as the sensor-tasking
//! route hands a command over: one loop taking requests in arrival order is what makes
//! "the first valid decision wins" a property of the design rather than the outcome of a
//! race (`docs/design/DN-31-node-approval-queue.md` §3). A queue invented in a request
//! handler would put the recommend-versus-act boundary in the transport, which is the
//! sentence this module carried while the route refused.
//!
//! The route does answer four questions of its own before the loop sees anything, because
//! each is about the caller rather than about the queue: the token, the permission, whether
//! the item is offered to that role, and whether a rejection says why (§6.3). Each refusal
//! is recorded for the loop to audit, so the node's record holds one entry per decision and
//! per refusal alike.
//!
//! **There is no `/v3/plans/{plan_id}/decision`**: a decision is taken on the queue item,
//! which is what carries the deadline and the roles it is offered to, and a plan-keyed door
//! beside it would be a second place those four checks could differ. The retired `/v2` one
//! names the queue route as its successor rather than its own path, which is the one place
//! a retired route's successor is not simply the same path under `/v3`.
//!
//! # What the outside world may say back, and what it may be sent
//!
//! Two routes exist so an outside party can answer something this deployment sent it:
//! `POST /v3/handoffs/{decision_id}/report` (GAP-040) and
//! `POST /v3/warnings/{asset_id}/{track_id}/acknowledge` (GAP-042). Both take a machine
//! whose certificate speaks for the right thing or an operator holding the matching
//! action, and both **queue rather than apply**: the node holds neither a handoff nor a
//! warning ledger, so it puts the fact on the record and the desktop that issued the one
//! or raised the other applies it.
//!
//! `GET /v3/exchange/{warnings,reports,handoffs}` (GAP-065) are DN-18's three remaining
//! items, gated by the agreement and the marking together with the restrictive one
//! deciding, and reporting what they withheld. Tracks and health keep their existing
//! doors, `/v3/snapshot` and `/v3/health`.
//!
//! **`POST` on those same three paths (GAP-065, DN-18 §5 amendment 2, human-owned;
//! signatures: docs/signatures.md) is the write path DN-18's own amendment 1 said
//! neither existed nor was decided.** The caller is this deployment's own desktop,
//! posting under its operator session token what it currently holds; the node replaces
//! its held set for that item and the existing `GET` route serves it onward, still
//! through both of §5's gates. Unlike the two routes above, this is not an outside party
//! answering something -- it is this deployment telling its own node about itself -- so
//! it takes `PUBLISH_EXCHANGE` rather than a machine identity, and it applies the
//! replacement synchronously rather than queuing it for the node loop.
//!
//! A launch warning (GAP-009, DN-16 §5) takes no door of its own either: it is a message
//! on the event stream, released to a party by the same two gates through
//! [`NodeApi::releases`]. DN-16 §6 asked for no new endpoint on our side and it gets
//! none. **Nothing in this workspace issues one**; what exists is the shape a peer's
//! warning arrives in and the gate it would leave by.
//!
//! # A non-finite float on the wire (GAP-153, D-96)
//!
//! **The stream, `GET /v3/history` and `GET /v3/snapshot` carry every float as it was**,
//! NaN and the infinities included, in the journal's own lossless form
//! ([`gungnir_eventing::nonfinite`], D-77). A frame or body whose floats are all finite is
//! exactly what `serde_json` writes, byte for byte; one that carries a non-finite float
//! is [`gungnir_eventing::nonfinite::MARKER`] and the escaped JSON, served with
//! [`LOSSLESS_JSON`] as its content type. Plain `serde_json` wrote such a value as `null`,
//! which the desktop could not decode and took for the node ending the stream.
//!
//! **An envelope is encoded once, when it is offered** ([`NodeApi::publish_event`]), and
//! the line is proved to read back before any subscriber sees it; every subscriber is sent
//! that same line. An envelope with no faithful line is refused there with
//! [`ApiError::Unencodable`] and counted ([`NodeApi::unencodable_envelopes`]), so no
//! stream can carry a frame its desktop cannot read, and none is ended by one.
//!
//! # Loopback in the clear, or anywhere with mutual TLS
//!
//! [`serve`] and [`bind`] refuse any address that is not loopback, because a
//! command-and-control surface accepting plaintext connections from the network would be
//! worse than one that does not start.
//!
//! **A node serving mutual TLS is not bound by that** (GAP-060): see [`crate::tls`],
//! which builds the acceptor, and `serve_on_listener`, which serves any listener. The
//! restriction is on plaintext, not on the address.
//!
//! # `/v3`, and the retired `/v2`
//!
//! Every route is served under `crate::API_VERSION`, and its path is built by `crate::path`
//! from `crate::routes`, which the desktop's client builds its URLs from too (GAP-130).
//! `/v2` was retired on 2026-09-17, when decision, plan and queue-item identifiers became
//! UUID v7 written as strings (D-56, D-60) and every payload carrying one changed. **Its
//! routes are still routed** ([`RETIRED`]): each authenticates its caller exactly as its
//! `/v3` successor does -- so a caller the successor would refuse is refused in the same
//! words, and a node tells nobody unauthenticated where a route went -- and then answers
//! `410 Gone` naming the successor. The event stream answers before upgrading, because its
//! token travels in the first frame, which a retired route never reads.
//!
//! # What the routes leave on the node's audit record (GAP-111, D-87)
//!
//! **Every sign-in attempt, every refusal, and every act a role-gated route performs**
//! leaves exactly one [`AuditEntry`], which the handler puts in [`NodeApi`]'s
//! [`AuditOutbox`] and the node loop writes to its hash-chained log once a tick
//! ([`NodeApi::take_audit`]); a handler never touches the disk. The entry names the
//! verified operator and the verified machine where there is one, and never an operator
//! that was only claimed (DN-23 §5 rule 1). In particular:
//!
//! - `POST /v3/session`: `session.sign_in` naming the operator and the role granted, or
//!   `session.rejected` naming nobody, with the identifier that was tried in the detail.
//!   The passphrase is never in either.
//! - A request refused for who is asking -- no valid token, a machine on a route internal
//!   to the deployment, a party whose agreement does not cover the item -- is
//!   `access.refused`. A verified operator refused for want of a permission is recorded
//!   under the permission, so an auditor asking what was done about an action finds the
//!   refusals beside the acts.
//! - An act a role-gated route performs is recorded under its action with its outcome.
//!   The decision routes are the exception in form only: their refusals and decisions
//!   were already audited by the loop, one each (DN-31 §9 row 4), and are left there.
//! - **Reads that are served are not recorded one by one**: a desktop polls, and a
//!   session that reads was established by a sign-in that was. The picture's routes now
//!   ask for `picture.view`, which every role holds but the security officer (D-30), and
//!   a refusal of one is recorded.
//! - **A detection accepted onto the gateway's queue is not recorded** either: it is a
//!   desktop forwarding its sensors, a data path the gateway journals, and a person's act
//!   only in the sense that somebody is signed in. A refusal of the caller is
//!   recorded; a body that will not decode or speaks another schema is answered and not
//!   recorded, as the gateway journals rather than audits what it quarantines.

use crate::tls::{Peer, PlainListener, TlsListener};
use crate::{routes, v3, ApiError};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use gungnir_eventing::{nonfinite, Envelope};
use gungnir_model::SystemHealth;
use gungnir_model::{
    AssetId, DecisionId, ExchangeItem, ExchangeSet, MissionTime, SensorId, SensorTaskId, TrackId,
};
use gungnir_security::audit::events;
use gungnir_security::{
    actions, AuditDrain, AuditEntry, AuditOutbox, AuthFailure, MissionTimeSeconds, OperatorId,
    OperatorSession,
};
use std::collections::{BTreeMap, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::broadcast;

/// How many envelopes a subscriber may fall behind, and how far back `from_seq` reaches.
///
/// The contract says a client asking for a `from_seq` older than the node's retention
/// gets an error and must take a fresh snapshot. This is that retention: an in-memory
/// window, **not** the journal's. It is smaller than the journal, and the error is the
/// same, so a client cannot mistake truncation for completeness.
pub const BACKLOG_CAPACITY: usize = 8_192;

/// How often the server pings an idle event stream.
///
/// **Without this a dead link looks alive.** A quiet node and a node whose host has
/// vanished produce exactly the same thing on a TCP connection -- nothing -- and a
/// half-open connection can stay open for a very long time. A desktop showing a picture
/// under a "connected" light, from a node that stopped existing minutes ago, is the
/// worst failure this transport can have: the operator has no way to tell.
///
/// So the server pings, the client answers automatically, and a client that hears
/// nothing at all within [`HEARTBEAT_TIMEOUT`] treats the link as gone.
///
/// **2 s, down from 10 s (D-23, 2026-09-06).** The connectivity budget asks that an
/// operator can see a link going stale within 2 s of the last beat, and PN-01 shows
/// "heard N s ago" driven by this beat -- so on a healthy quiet link the display must read
/// as fresh, which a 10 s beat could not manage. A WebSocket ping is a handful of bytes:
/// half a ping per second per desktop, five per second per node at the ten-desktop
/// fan-out budget.
pub const HEARTBEAT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

/// How many beats a client will miss before giving up on the link.
///
/// Three, plus the margin below. One missed beat is a slow network; three in a row is a
/// node that has stopped, and a single stall must not cost a full reconnect -- sign-in,
/// snapshot, resubscribe -- on the intermittent links the connected profile is for.
pub const HEARTBEAT_MISSES_TOLERATED: u32 = 3;

/// Slack past the last tolerated beat, so a beat that arrives fractionally late is not
/// counted as missed.
const HEARTBEAT_MARGIN: std::time::Duration = std::time::Duration::from_secs(1);

/// How long a client waits for anything at all before giving up on the link: 7 s.
///
/// **Derived, not written down.** This used to be an independent constant of 35 s beside
/// an interval of 10 s, and the two were edited separately -- which is how a 2 s
/// connectivity budget came to sit next to a 35 s timeout without anyone noticing
/// (GAP-056). A timeout expressed as beats cannot drift from the beat.
pub const HEARTBEAT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(
    HEARTBEAT_INTERVAL.as_secs() * HEARTBEAT_MISSES_TOLERATED as u64 + HEARTBEAT_MARGIN.as_secs(),
);

/// A token and when it stops being believed.
#[derive(Debug, Clone, PartialEq)]
pub struct Issued {
    pub token: String,
    pub expires_s: MissionTimeSeconds,
}

/// How a node authenticates API callers (DN-23 §6).
///
/// A trait so `gungnir-api` holds no account store and no signing key: custody belongs to
/// the host (DN-22 §4), and the node constructs the implementation.
pub trait CallerAuthority: Send + Sync {
    /// Verify a credential and mint a token.
    fn sign_in(
        &self,
        operator: u64,
        passphrase: &str,
        now: MissionTimeSeconds,
    ) -> Result<Issued, AuthFailure>;

    /// Verify a presented token.
    fn verify(&self, token: &str, now: MissionTimeSeconds) -> Result<OperatorSession, AuthFailure>;
}

/// The caller authority a node builds from an account store and a signing key.
///
/// Lives here rather than in the node because the composition is the same wherever it is
/// used, and a second copy in a test would be a second place the back-off could differ
/// from the real one.
pub struct AccountTokenAuthority {
    store: Box<dyn gungnir_security::AccountStore>,
    issuer: gungnir_security::TokenIssuer,
    back_off: gungnir_security::BackOff,
    /// Consecutive failures and when the last one was, per operator. Behind a mutex
    /// because requests are concurrent, unlike the desktop's single-threaded authority.
    failures: Mutex<std::collections::BTreeMap<u64, (u32, MissionTimeSeconds)>>,
}

impl std::fmt::Debug for AccountTokenAuthority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountTokenAuthority")
            .finish_non_exhaustive()
    }
}

impl AccountTokenAuthority {
    #[must_use]
    pub fn new(
        store: Box<dyn gungnir_security::AccountStore>,
        issuer: gungnir_security::TokenIssuer,
    ) -> Self {
        Self {
            store,
            issuer,
            back_off: gungnir_security::BackOff::default(),
            failures: Mutex::new(std::collections::BTreeMap::new()),
        }
    }
}

impl CallerAuthority for AccountTokenAuthority {
    fn sign_in(
        &self,
        operator: u64,
        passphrase: &str,
        now: MissionTimeSeconds,
    ) -> Result<Issued, AuthFailure> {
        let id = gungnir_security::OperatorId(operator);

        // Back off before doing any work, so a flood of attempts costs the attacker
        // rather than the node: argon2 is deliberately expensive, and a node that
        // hashed on every request would be denying itself service.
        if let Ok(failures) = self.failures.lock() {
            if let Some((count, last)) = failures.get(&operator) {
                let delay = self.back_off.delay_after(*count);
                let elapsed = now - last;
                if elapsed < delay {
                    return Err(AuthFailure::TooFast {
                        retry_after_s: delay - elapsed,
                    });
                }
            }
        }

        match gungnir_security::verify_account(self.store.as_ref(), id, passphrase) {
            Ok(account) => {
                if let Ok(mut failures) = self.failures.lock() {
                    failures.remove(&operator);
                }
                let session = OperatorSession {
                    operator: id,
                    role: account.role,
                    established: now,
                    // Replaced by the issuer's own expiry when the token is minted; a
                    // node-issued session always expires.
                    expires: None,
                };
                let token = self
                    .issuer
                    .mint(&session, now)
                    .map_err(|_| AuthFailure::Unavailable)?;
                Ok(Issued {
                    token,
                    expires_s: now + self.issuer.lifetime_s(),
                })
            }
            Err(failure) => {
                if !matches!(failure, AuthFailure::Unavailable) {
                    if let Ok(mut failures) = self.failures.lock() {
                        let entry = failures.entry(operator).or_insert((0, now));
                        entry.0 = entry.0.saturating_add(1);
                        entry.1 = now;
                    }
                }
                Err(failure)
            }
        }
    }

    fn verify(&self, token: &str, now: MissionTimeSeconds) -> Result<OperatorSession, AuthFailure> {
        self.issuer.verify(token, now)
    }
}

/// One envelope as the node offered it: the envelope, for the routes that filter by
/// party, and the line every subscriber is sent (GAP-153, D-96).
#[derive(Debug)]
struct Offered {
    envelope: Envelope,
    line: String,
}

/// The content type of a body in the lossless form (GAP-153, D-96): a leading
/// [`nonfinite::MARKER`] and escaped JSON, which is not JSON and is not labelled as JSON.
/// A body whose floats are all finite is `application/json`, as it always was.
pub const LOSSLESS_JSON: &str = "application/vnd.gungnir.lossless-json";

/// A `200` body in the lossless form: plain JSON when every float is finite, the marked
/// form when one is not (GAP-153, D-96).
///
/// For the read routes whose bodies carry the picture's floats -- the history and the
/// snapshot. **Proved to read back** like an offered envelope, so the body a desktop is
/// given is one it can decode; a body that cannot be is a `500` naming why, never a `200`
/// the desktop would misread.
fn lossless_json<T>(value: &T) -> Response
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    match nonfinite::to_faithful_line(value) {
        Ok(line) => {
            let content_type = if line.starts_with(nonfinite::MARKER) {
                LOSSLESS_JSON
            } else {
                "application/json"
            };
            (
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, content_type)],
                line,
            )
                .into_response()
        }
        Err(why) => problem(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("the node could not write this answer faithfully: {why}"),
        ),
    }
}

/// What a node publishes and what the routes read.
///
/// The tick loop owns every service, so the transport never touches one. Each tick the
/// loop publishes a fresh snapshot and forwards the bus's envelopes here; the routes read
/// only this. That keeps a single owner for mutable state and means a slow client cannot
/// stall the loop.
pub struct NodeApi {
    snapshot: RwLock<v3::SnapshotResponse>,
    /// The coverage answer this node last computed (GAP-006).
    ///
    /// Published by the tick like the snapshot, because computing it in a request handler
    /// would put a sampling loop on the request path and let a caller's polling rate
    /// decide the node's load.
    coverage: RwLock<v3::CoverageResponse>,
    events: broadcast::Sender<Arc<Offered>>,
    backlog: Mutex<VecDeque<Arc<Offered>>>,
    /// Envelopes refused by [`Self::publish_event`] because they have no faithful line
    /// (GAP-153, D-96).
    unencodable: AtomicU64,
    /// Mission time as of the last published snapshot, which is what token expiry is
    /// judged against. The node's clock, not the transport's: a token minted against one
    /// clock and checked against another would expire at a time nobody chose.
    now: RwLock<MissionTimeSeconds>,
    /// Whoever verifies callers. `None` means this node authenticates nobody and
    /// therefore serves nobody, which is the default deployment.
    callers: Option<Arc<dyn CallerAuthority>>,
    /// Detections submitted over the API, waiting for the gateway to poll them.
    ///
    /// A queue rather than a direct call, so a submission enters through the same
    /// `ProtocolAdapter` boundary a sensor's does and is authenticated and validated by
    /// the same code. A route that reached into the gateway would be a second, unchecked
    /// way in -- and `gungnir-ingest` is the trust boundary for external data.
    submissions: Mutex<Vec<gungnir_model::DetectionView>>,
    /// The exchange agreements in force (DN-18, GAP-065): what each party may receive.
    /// Empty means no party may receive anything, which is the default deployment.
    exchange: ExchangeSet,
    /// What each client certificate speaks for (D-02; GAP-002, GAP-040), by subject
    /// common name. Empty means a certificate is a party and nothing more.
    identities: std::collections::HashMap<String, MachineRole>,
    /// Detections a machine submitted for the sensor its certificate speaks for. A
    /// separate queue from `submissions` so the node can admit them under the
    /// authenticator that may stamp `MachineIdentity` (GAP-002).
    machine_submissions: Mutex<Vec<gungnir_model::DetectionView>>,
    /// Sensor tasks waiting for the node loop to issue them (GAP-004).
    tasks: Mutex<Vec<PendingSensorTask>>,
    /// Decisions waiting for the node loop to take them (GAP-132, DN-31 §6.3). A
    /// separate queue from `tasks` for the same reason `reports` is separate from
    /// `submissions`: two different facts, arriving under different authority.
    decisions: Mutex<Vec<PendingDecision>>,
    /// Decision requests the routes refused, waiting for the loop to audit them
    /// (GAP-132, DN-31 §9 row 4). The caller has already been answered; what is owed is
    /// the entry in the record.
    refusals: Mutex<Vec<RefusedDecision>>,
    /// Outages' decisions waiting for the node loop to put them on its record (GAP-134,
    /// DN-31 §6.8). A queue of its own for the reason `decisions` has one: a different
    /// fact, and one the loop answers differently -- a batch is taken whole or not at all.
    forwarded: Mutex<Vec<PendingForward>>,
    /// Effector reports waiting for the node loop to put on the record (GAP-040).
    reports: Mutex<Vec<EffectorReportRecord>>,
    /// Warning acknowledgements waiting for the node loop to put on the record
    /// (GAP-042). A separate queue from `reports` for the same reason `reports` is
    /// separate from `submissions`: the two are different facts, arriving under
    /// different authority, and one queue would make the drain guess which.
    acknowledgements: Mutex<Vec<WarningAcknowledgement>>,
    /// What this deployment holds for exchange, per item and **per producer** (DN-18 §5
    /// amendment 3, GAP-065, GAP-137). An item with no producer has had nothing published
    /// for it, which is answered as [`v3::ExchangeResponse::NotHeld`] and never as an
    /// empty list.
    ///
    /// **One set per writer, merged on read.** This node issues handoffs of its own since
    /// GAP-132 and every desktop it serves publishes its own set; with one set per item
    /// the register held whichever writer wrote last, and which one a partner saw
    /// depended on tick order.
    ///
    /// **Each set carries when it was written** (GAP-145), because a merged answer is
    /// only as current as its quietest producer and a partner cannot see the producers.
    exchange_products: RwLock<BTreeMap<ExchangeItem, BTreeMap<ExchangeProducer, ProducerSet>>>,
    /// What the routes owe the node's audit record, waiting for the loop (GAP-111, D-87).
    ///
    /// An outbox rather than the log itself for the reason `refusals` is one: the log is
    /// the loop's, and a handler that wrote to a file would put the disk on the request
    /// path. Rate-limited per lane, so a flood from nobody in particular is bounded in
    /// memory and on disk and counted rather than recorded one by one.
    audit: AuditOutbox,
}

/// Who wrote a set into this deployment's exchange register (GAP-137, DN-18 §5
/// amendment 3).
///
/// **A publish replaces this producer's set and nothing else**, and a read merges every
/// producer's ([`NodeApi::exchange_all`]). Before GAP-137 the register held one set per
/// item, so the node's own handoffs and a desktop's published set would each have erased
/// the other.
///
/// **Never on the wire.** A partner reads the merged set and learns nothing about how
/// many desktops this deployment runs or which one holds what, which is a fact about our
/// own topology rather than about the products; the response shape is the one DN-18 §6
/// already fixed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExchangeProducer {
    /// This node itself: what its own approval desk holds (GAP-132, GAP-137). Ordered
    /// first, so a merged read puts the node's own set before any desktop's.
    Node,
    /// A desktop that published over the write path, named by the common name its client
    /// certificate was verified under -- since GAP-141 the digest of its own key, which
    /// is the same name across its restarts and different from every other desktop's.
    Party(String),
    /// A writer this transport could not name, because the link carries no client
    /// certificate.
    ///
    /// **Every unnamed writer shares this one set**, so two desktops publishing over a
    /// plaintext link still overwrite each other. That is what the whole register was
    /// before GAP-137, and telling two desktops apart is what mutual TLS buys: a
    /// deployment that wants them distinguished configures certificates for them.
    Unidentified,
}

/// One producer's answer for an item, and when this node took it (GAP-137, GAP-145).
///
/// **Nothing expires it.** A handoff is a decision that was taken, and dropping one
/// because the console that issued it went quiet would delete a true thing to hide an
/// unknown one. What the age is for is the answer a partner reads: `as_of` on
/// [`v3::ExchangeResponse`] carries the oldest of these, so a partner can tell a
/// deployment that holds nothing new from one whose producer stopped talking. A desktop
/// refreshes its own set whenever its link comes back (GAP-145), so the age of a live
/// deployment's answer is bounded by its link rather than by its handoffs.
#[derive(Debug, Clone)]
struct ProducerSet {
    answer: v3::ExchangeResponse,
    written: MissionTimeSeconds,
}

/// How many producers one item admits (GAP-137).
///
/// A guard against an unbounded register rather than a policy about deployments: a
/// deployment runs a handful of desktops, and this only bites when names churn -- an
/// ephemeral desktop (D-67) is a new name every run.
///
/// **A producer already in the register is never refused**, and a new one beyond this is,
/// with the reason said. Refusing is what keeps the failure visible: the desktop's link
/// keeps the batch queued and reports the backlog (DN-18 §5), where evicting somebody
/// else's set would have served a partner a stale one and said nothing.
pub const PRODUCERS_PER_ITEM: usize = 64;

/// What a client certificate speaks for (D-02). The node builds this from the
/// baseline's `machine_identities`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachineRole {
    Sensor(SensorId),
    Effector {
        endpoint: String,
    },
    /// The party a warning is owed to, named by the `warning` endpoint its obligations
    /// use (GAP-042, DN-03 §5 rule 2). Distinct from [`MachineRole::Effector`] even
    /// though both name a row in the endpoint table: an effector acts on a decision and
    /// a warned party is told about a threat, and a certificate that was both could
    /// report an engagement it was never handed.
    WarnedParty {
        channel: String,
    },
    Peer {
        name: String,
    },
}

/// A sensor task the route accepted and the node loop has not yet issued (GAP-004).
///
/// The registry lives on the node loop, so the route hands the command over and waits
/// for the answer rather than reaching into the registry from a request handler.
pub struct PendingSensorTask {
    pub sensor: SensorId,
    pub command: gungnir_model::SensorCommand,
    pub requirement: Option<gungnir_model::RequirementId>,
    /// The node's task id, or the registry's refusal in its own words.
    pub reply: tokio::sync::oneshot::Sender<Result<SensorTaskId, String>>,
}

impl std::fmt::Debug for PendingSensorTask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingSensorTask")
            .field("sensor", &self.sensor)
            .field("command", &self.command)
            .finish_non_exhaustive()
    }
}

/// A decision the route accepted and the node loop has not yet taken (GAP-132,
/// DN-31 §6.3).
///
/// The second of these carriers, after [`PendingSensorTask`] and for the same reason: the
/// queue lives on the node loop, so the route hands the request over and waits for the
/// loop's answer rather than deciding in a request handler. `refuse_decision` used to say
/// why that mattered -- a queue invented in a request handler would put the
/// recommend-versus-act boundary in the transport -- and one loop taking requests in
/// arrival order is what makes "the first valid decision wins" a property of the design
/// rather than the outcome of a race (DN-31 §3).
pub struct PendingDecision {
    pub item: gungnir_model::PendingApprovalId,
    pub choice: v3::DecisionChoice,
    /// The client's key, so a retry is the same request (DN-31 §5.2).
    pub request: gungnir_model::RequestId,
    /// The verified session the decision is attributed to. Both halves or neither, read
    /// from one token (DN-23 §5 rule 1).
    ///
    /// **The identifier, not its text.** A decision's attribution is what D-53's
    /// arbitration ranks and what an audit entry is searched by. Carrying it as a string
    /// puts a parse between the token and the record, and a parse has a failing branch
    /// that has to answer to somebody: the loop's answered `unwrap_or_default()`, which
    /// would have attributed the decision to operator 0. There is nothing to parse if
    /// nothing is written down.
    pub operator: gungnir_security::OperatorId,
    pub role: gungnir_security::Role,
    /// The machine the connection was verified as -- a desktop's name since GAP-141 --
    /// for the audit entry (GAP-111); `None` over plaintext.
    pub party: Option<String>,
    /// What the loop decided, or why it would not.
    pub reply: tokio::sync::oneshot::Sender<DecisionAnswer>,
}

/// What the node loop says about one decision request (DN-31 §6.3).
///
/// The three outcomes the route turns into `201`, `409` and `404`. A refusal the *route*
/// made -- an expired token, a missing permission, a role the item is not offered to, a
/// rejection with no reason -- never reaches the loop and is not in here.
#[derive(Debug, Clone, PartialEq)]
pub enum DecisionAnswer {
    /// Recorded now, or recorded before under the same request key. The client cannot
    /// tell the two apart, and does not need to: both mean "this request produced that
    /// decision, exactly once".
    Recorded(DecisionId),
    /// The item takes no decision now, and why.
    Refused(v3::DecisionRefused),
    /// No item, decided or queued, answers to that identifier on this node.
    Unknown,
}

impl std::fmt::Debug for PendingDecision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingDecision")
            .field("item", &self.item)
            .field("choice", &self.choice)
            .field("role", &self.role)
            .finish_non_exhaustive()
    }
}

/// An outage's decisions the route accepted and the node loop has not yet put on its
/// record (GAP-134, DN-31 §6.8).
///
/// The third of these carriers and for the same reason as the other two: the node's
/// record is the loop's. **One batch is one outage**, taken by the loop whole or not at
/// all, which is what stops a desktop that fails part-way through from leaving the node
/// holding the first half of what it decided.
pub struct PendingForward {
    pub decisions: Vec<v3::ForwardedDecision>,
    /// Who forwarded it, verified from the token. **Not who decided**: that is each
    /// record's own account, taken at a console this node could not see, and the audit
    /// entry the loop writes names both.
    pub operator: gungnir_security::OperatorId,
    pub role: gungnir_security::Role,
    /// The machine the connection was verified as, for the audit entry (GAP-111).
    pub party: Option<String>,
    pub reply: tokio::sync::oneshot::Sender<ForwardAnswer>,
}

impl std::fmt::Debug for PendingForward {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingForward")
            .field("decisions", &self.decisions.len())
            .field("role", &self.role)
            .finish_non_exhaustive()
    }
}

/// What the node loop says about one forwarded batch (DN-31 §7).
#[derive(Debug, Clone, PartialEq)]
pub enum ForwardAnswer {
    /// `202`: the whole batch is on the record, counted.
    Accepted(v3::ForwardAccepted),
    /// `409`: a record contradicts one already held, and nothing was applied. Boxed
    /// because it can carry a whole record, plan included, and the answer is otherwise
    /// three counters.
    Refused(Box<v3::ForwardRefused>),
}

/// A decision request the route refused, for the node loop to audit (GAP-132).
///
/// DN-31 §9 row 4: **every decision and every refusal writes exactly one audit entry on
/// the node**, and the four pre-loop checks refuse before the loop sees anything. The
/// audit log is the loop's, so a refusal is queued for it exactly as an effector's report
/// is -- one fact, recorded once, by the one owner of the record. Nothing is owed to the
/// caller in return, so there is no reply channel: it has already been answered.
#[derive(Debug, Clone, PartialEq)]
pub struct RefusedDecision {
    /// The item the caller named, and `None` when the path did not parse as one.
    pub item: Option<gungnir_model::PendingApprovalId>,
    /// Who was refused: the operator's identifier, or `unauthenticated` when no token
    /// resolved. Never invented.
    pub operator: String,
    /// The machine the connection was verified as, whether or not a token resolved
    /// (GAP-111): a refused desktop is named by its key even when its session was not.
    pub party: Option<String>,
    /// Why, in the words the caller was given.
    pub reason: String,
}

/// An effector's report the route accepted and the node loop has not yet recorded.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectorReportRecord {
    pub decision: DecisionId,
    /// The endpoint the certificate speaks for, or `operator:<id>`.
    pub endpoint: String,
    pub report: gungnir_model::handoff::EffectorReport,
}

/// A warned party's acknowledgement the route accepted and the node loop has not yet put
/// on the record (GAP-042, DN-03 §5 rule 2).
///
/// **The node owns no warnings**: a desktop raises them against its own assets and holds
/// the ledger. So this follows [`EffectorReportRecord`] exactly -- the node records the
/// fact for every desktop and the one that raised the warning applies it, or rejects it
/// as naming a pair it never raised. A node that tried to apply it would need a ledger it
/// has no assets to build.
#[derive(Debug, Clone, PartialEq)]
pub struct WarningAcknowledgement {
    pub asset: AssetId,
    pub track: TrackId,
    /// The warning channel the certificate speaks for, or `operator:<id>`.
    pub party: String,
    /// The time the party gave. Its own claim, recorded as such; the envelope the node
    /// publishes carries the time this deployment recorded it, so neither is invented.
    pub at: MissionTime,
}

impl std::fmt::Debug for NodeApi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeApi")
            .field("subscribers", &self.events.receiver_count())
            .finish_non_exhaustive()
    }
}

impl NodeApi {
    #[must_use]
    pub fn new(snapshot: v3::SnapshotResponse) -> Self {
        let (events, _) = broadcast::channel(BACKLOG_CAPACITY);
        Self {
            snapshot: RwLock::new(snapshot),
            coverage: RwLock::new(v3::CoverageResponse::NotComputed {
                reason: "this node has not computed a coverage answer yet".into(),
            }),
            events,
            backlog: Mutex::new(VecDeque::with_capacity(BACKLOG_CAPACITY)),
            unencodable: AtomicU64::new(0),
            now: RwLock::new(0.0),
            callers: None,
            submissions: Mutex::new(Vec::new()),
            exchange: ExchangeSet::default(),
            identities: std::collections::HashMap::new(),
            machine_submissions: Mutex::new(Vec::new()),
            tasks: Mutex::new(Vec::new()),
            decisions: Mutex::new(Vec::new()),
            refusals: Mutex::new(Vec::new()),
            forwarded: Mutex::new(Vec::new()),
            reports: Mutex::new(Vec::new()),
            acknowledgements: Mutex::new(Vec::new()),
            exchange_products: RwLock::new(BTreeMap::new()),
            audit: AuditOutbox::new(),
        }
    }

    /// Everything the routes owe the node's audit record since the last call, and what
    /// the rate limits counted instead of recording (GAP-111, D-87).
    ///
    /// The node loop calls this once a tick and records it with
    /// [`AuditDrain::record_into`], the way it takes the decisions and the refusals.
    pub fn take_audit(&self) -> AuditDrain {
        self.audit.drain()
    }

    /// One entry the routes owe, at this node's clock, naming the machine the connection
    /// was verified as where there was one and the address it came from.
    fn audit(
        &self,
        peer: &Peer,
        operator: Option<OperatorId>,
        action: &str,
        detail: impl std::fmt::Display,
    ) {
        self.audit.push(
            AuditEntry::new(
                operator,
                action,
                self.now(),
                format!("{detail} (from {})", peer.addr),
            )
            .by_party(peer.party.clone()),
        );
    }

    /// Install what each client certificate speaks for (D-02).
    #[must_use]
    pub fn with_machine_identities(mut self, identities: Vec<(String, MachineRole)>) -> Self {
        self.identities = identities.into_iter().collect();
        self
    }

    /// The sensors client certificates may speak for, for the node's machine-submission
    /// authenticator (GAP-002).
    #[must_use]
    pub fn vouched_sensors(&self) -> Vec<SensorId> {
        self.identities
            .values()
            .filter_map(|role| match role {
                MachineRole::Sensor(id) => Some(*id),
                _ => None,
            })
            .collect()
    }

    /// Take the machine-submitted detections since the last call (GAP-002).
    #[must_use]
    pub fn take_machine_submissions(&self) -> Vec<gungnir_model::DetectionView> {
        self.machine_submissions
            .lock()
            .map(|mut queue| std::mem::take(&mut *queue))
            .unwrap_or_default()
    }

    /// Take the sensor tasks the routes accepted since the last call (GAP-004). The node
    /// loop issues each through its registry and answers on the task's `reply`.
    #[must_use]
    pub fn take_tasks(&self) -> Vec<PendingSensorTask> {
        self.tasks
            .lock()
            .map(|mut queue| std::mem::take(&mut *queue))
            .unwrap_or_default()
    }

    /// Take the decisions the routes accepted since the last call, **in arrival order**
    /// (GAP-132, DN-31 §6.3).
    ///
    /// The order is the property, not an implementation detail: the loop takes them in
    /// the order they arrived and the first valid one on an item wins, so two operators
    /// racing on one item get one decision and one `409` rather than two decisions. The
    /// loop answers each through its `reply`.
    #[must_use]
    pub fn take_decisions(&self) -> Vec<PendingDecision> {
        self.decisions
            .lock()
            .map(|mut queue| std::mem::take(&mut *queue))
            .unwrap_or_default()
    }

    /// Record that a route refused a decision request, for the loop to audit
    /// (GAP-132, DN-31 §9 row 4).
    ///
    /// A poisoned lock loses the entry and says so in the log rather than failing the
    /// request: the caller has already been refused correctly, and turning a refusal into
    /// a `500` would tell them the opposite of what happened.
    pub fn refuse_decision(&self, refusal: RefusedDecision) {
        if let Ok(mut queue) = self.refusals.lock() {
            queue.push(refusal);
        } else {
            tracing::error!(
                ?refusal,
                "the refusal queue lock was poisoned; this refusal reaches no audit entry"
            );
        }
    }

    /// Take the forwarded batches the route accepted since the last call, in arrival
    /// order (GAP-134, DN-31 §6.8). The loop answers each through its `reply`.
    #[must_use]
    pub fn take_forwarded(&self) -> Vec<PendingForward> {
        self.forwarded
            .lock()
            .map(|mut queue| std::mem::take(&mut *queue))
            .unwrap_or_default()
    }

    /// Take the decision requests the routes refused since the last call, for the loop to
    /// audit (GAP-132, DN-31 §9 row 4).
    #[must_use]
    pub fn take_refused_decisions(&self) -> Vec<RefusedDecision> {
        self.refusals
            .lock()
            .map(|mut queue| std::mem::take(&mut *queue))
            .unwrap_or_default()
    }

    /// The queue as the node last published it (DN-31 §5.3).
    ///
    /// Read off the published snapshot rather than from a register of its own, so
    /// `GET /v3/queue` and the `queue` field of `GET /v3/snapshot` cannot disagree about
    /// what is waiting: one publish, two doors. Empty for a node whose loop has published
    /// nothing yet, which is what such a node honestly has.
    #[must_use]
    pub fn queue(&self) -> Vec<v3::QueueItemView> {
        self.snapshot
            .read()
            .map(|s| s.queue.clone())
            .unwrap_or_default()
    }

    /// Take the effector reports the route accepted since the last call (GAP-040).
    #[must_use]
    pub fn take_effector_reports(&self) -> Vec<EffectorReportRecord> {
        self.reports
            .lock()
            .map(|mut queue| std::mem::take(&mut *queue))
            .unwrap_or_default()
    }

    /// Take the warning acknowledgements the route accepted since the last call
    /// (GAP-042). The node loop publishes each as `WarningEvent::Acknowledged`, and the
    /// desktop that raised the warning applies it to its ledger.
    #[must_use]
    pub fn take_warning_acknowledgements(&self) -> Vec<WarningAcknowledgement> {
        self.acknowledgements
            .lock()
            .map(|mut queue| std::mem::take(&mut *queue))
            .unwrap_or_default()
    }

    /// Publish what `producer` holds for `item`, for the exchange routes (DN-18 §5,
    /// GAP-065, GAP-137). Called by whoever owns the products, once they are marked.
    ///
    /// Publishing an empty list is a claim -- "we hold none of this right now" -- and is
    /// different from never having published, which is [`NodeApi::withhold_exchange`].
    ///
    /// **It replaces this producer's set and no other's** (DN-18 §5 amendment 3). A read
    /// merges what every producer holds, so a desktop republishing its handoffs leaves
    /// the node's own set and every other desktop's exactly where they were.
    pub fn publish_exchange(
        &self,
        producer: ExchangeProducer,
        item: ExchangeItem,
        products: Vec<v3::ExchangeProduct>,
    ) -> Result<(), ApiError> {
        self.write_exchange(
            producer,
            item,
            v3::ExchangeResponse::Held {
                item,
                products,
                withheld: 0,
                // Filled in on the way out, from the oldest producer's write.
                as_of: None,
            },
        )
    }

    /// Say that `producer` holds none of `item` and why (DN-18 §5, GAP-065, GAP-137).
    ///
    /// An empty list would read to a partner as "there are none", which is a different
    /// claim and one a producer that keeps no such ledger cannot make. The reason travels
    /// so the partner knows to ask somebody else rather than concluding the sector is
    /// quiet.
    ///
    /// **Corrected 2026-09-23 (GAP-137).** This said that a node holds no warnings, no
    /// reports and no handoffs of its own. Since GAP-132 it holds handoffs: it runs an
    /// approval queue and issues them, so for that one item it publishes a set -- empty
    /// until it has issued one -- and withholds the other two.
    pub fn withhold_exchange(
        &self,
        producer: ExchangeProducer,
        item: ExchangeItem,
        reason: impl Into<String>,
    ) -> Result<(), ApiError> {
        self.write_exchange(
            producer,
            item,
            v3::ExchangeResponse::NotHeld {
                item,
                reason: reason.into(),
                as_of: None,
            },
        )
    }

    /// Put one producer's answer for `item` into the register (GAP-137).
    ///
    /// Bounded by [`PRODUCERS_PER_ITEM`]: a producer already in the register always
    /// writes, and a new one beyond the bound is refused with [`ApiError::NoRoom`] rather
    /// than evicting a set somebody else is being served from.
    fn write_exchange(
        &self,
        producer: ExchangeProducer,
        item: ExchangeItem,
        answer: v3::ExchangeResponse,
    ) -> Result<(), ApiError> {
        let mut slot = self
            .exchange_products
            .write()
            .map_err(|_| ApiError::Transport("the exchange lock was poisoned".into()))?;
        let written = self.now();
        let sets = slot.entry(item).or_default();
        if !sets.contains_key(&producer) && sets.len() >= PRODUCERS_PER_ITEM {
            return Err(ApiError::NoRoom(format!(
                "{item:?} already holds sets from {PRODUCERS_PER_ITEM} producers; \
                 {producer:?} was not admitted and published nothing"
            )));
        }
        sets.insert(producer, ProducerSet { answer, written });
        Ok(())
    }

    /// Everything held for `item`, unfiltered and merged across producers: what an
    /// operator inside the deployment sees. `None` when the lock is unreadable.
    ///
    /// **The merge is what the producer key is for** (GAP-137, DN-18 §5 amendment 3).
    /// Held sets are concatenated in producer order, this node's own first, and their
    /// withheld counts summed. The answer is [`v3::ExchangeResponse::NotHeld`] only when
    /// no producer holds any, and then it carries what each of them said rather than one
    /// of their reasons at random.
    #[must_use]
    pub fn exchange_all(&self, item: ExchangeItem) -> Option<v3::ExchangeResponse> {
        let held = self.exchange_products.read().ok()?;
        let mut products: Vec<v3::ExchangeProduct> = Vec::new();
        let mut withheld = 0usize;
        let mut reasons: Vec<String> = Vec::new();
        let mut any_held = false;
        let mut as_of: Option<MissionTimeSeconds> = None;
        for set in held.get(&item).into_iter().flat_map(BTreeMap::values) {
            // The oldest write across every producer, held or not: an answer is as
            // current as its quietest contributor (GAP-145).
            as_of = Some(as_of.map_or(set.written, |oldest: f64| oldest.min(set.written)));
            match &set.answer {
                v3::ExchangeResponse::Held {
                    products: one_set,
                    withheld: one_count,
                    ..
                } => {
                    any_held = true;
                    products.extend(one_set.iter().cloned());
                    withheld += one_count;
                }
                v3::ExchangeResponse::NotHeld { reason, .. } => reasons.push(reason.clone()),
            }
        }
        let as_of = as_of.map(gungnir_model::MissionTime);
        if any_held {
            return Some(v3::ExchangeResponse::Held {
                item,
                products,
                withheld,
                as_of,
            });
        }
        Some(v3::ExchangeResponse::NotHeld {
            item,
            reason: if reasons.is_empty() {
                "this deployment has published nothing for exchange under this item".to_string()
            } else {
                reasons.join("; ")
            },
            as_of,
        })
    }

    /// What `party` may receive of `item` (DN-18 §5, GAP-065): every product put through
    /// [`ExchangeSet::may_send`], which is the agreement gate and the marking gate with
    /// the restrictive one deciding, and a count of what that removed.
    ///
    /// **Counted, never silently dropped**, for the reason `SnapshotResponse::withheld`
    /// exists: a partner told its list is partial can ask; one that is not told believes
    /// it has everything.
    #[must_use]
    pub fn exchange_for(&self, party: &str, item: ExchangeItem) -> Option<v3::ExchangeResponse> {
        match self.exchange_all(item)? {
            held @ v3::ExchangeResponse::NotHeld { .. } => Some(held),
            v3::ExchangeResponse::Held {
                item,
                products,
                withheld,
                as_of,
            } => {
                let total = products.len();
                let products: Vec<v3::ExchangeProduct> = products
                    .into_iter()
                    .filter(|p| self.exchange.may_send(party, item, &p.releasability))
                    .collect();
                Some(v3::ExchangeResponse::Held {
                    item,
                    withheld: withheld + (total - products.len()),
                    products,
                    as_of,
                })
            }
        }
    }

    /// Install the exchange agreements (DN-18 §6).
    #[must_use]
    pub fn with_exchange(mut self, exchange: ExchangeSet) -> Self {
        self.exchange = exchange;
        self
    }

    /// The agreements in force.
    #[must_use]
    pub fn exchange(&self) -> &ExchangeSet {
        &self.exchange
    }

    /// The snapshot as `party` may receive it (DN-17 §5, DN-18 §5): every item filtered
    /// by the agreement and the marking, the restrictive one deciding, and the count of
    /// what was withheld on the response.
    #[must_use]
    pub fn snapshot_for(&self, party: &str) -> Option<v3::SnapshotResponse> {
        let full = self.snapshot()?;
        let total_tracks = full.tracks.len();
        let tracks: Vec<_> = full
            .tracks
            .into_iter()
            .filter(|t| {
                self.exchange
                    .may_send(party, ExchangeItem::Tracks, &t.releasability)
            })
            .collect();
        let mut withheld = total_tracks - tracks.len();
        // A plan is a recommendation for this deployment's own effectors; no exchange
        // item covers it, and the requirements are internal by the same reasoning.
        withheld += usize::from(full.plan.is_some());
        withheld += full.requirements.len();
        // GAP-096's wire contract: a retained bearing and the pipeline's own counters
        // are this deployment's internal sensor picture, exactly like the plan and the
        // requirements above -- DN-18 names no exchange item for either, so a machine
        // party gets none of it, counted rather than silently dropped. An operator's own
        // desktop is the only caller `snapshot` hands the unfiltered `full` to.
        withheld += full.bearing_rays.len();
        withheld += usize::from(full.pipeline_stats != gungnir_model::PipelineStatsView::default());
        // GAP-132: the queue is what this deployment's own people are deciding, and DN-18
        // names no exchange item for it -- the same reasoning as the plan two lines above,
        // which is a recommendation for this deployment's own effectors. Withheld and
        // counted, never silently empty.
        withheld += full.queue.len();
        let health = if self.exchange.may_send(
            party,
            ExchangeItem::Health,
            &gungnir_model::Releasability::AllPeers,
        ) {
            full.health
        } else {
            withheld += 1;
            SystemHealth::default()
        };
        Some(v3::SnapshotResponse {
            schema_version: full.schema_version,
            tracks,
            plan: None,
            health,
            requirements: Vec::new(),
            bearing_rays: Vec::new(),
            pipeline_stats: gungnir_model::PipelineStatsView::default(),
            withheld,
            queue: Vec::new(),
            // Stamped by the route that answers, as on the operator's own snapshot
            // (GAP-140). A partner reads its own deadlines from nothing here -- the
            // queue is withheld from it entirely -- but the field says what this node's
            // clock was all the same, rather than a `None` that would read as unknown.
            node_time: None,
        })
    }

    /// Whether `party` may receive `envelope` (GAP-062): tracking events whose track
    /// the agreement and the marking both release, a deletion, which names an
    /// identifier and nothing marked, and a launch warning this deployment issued.
    /// Everything else on the stream is internal.
    ///
    /// **The launch warning is gated under [`ExchangeItem::Warnings`]** (GAP-009,
    /// DN-16 §5). DN-18 §5 names no item of its own for it, and inventing a sixth
    /// `ExchangeItem` here would be a change to the model DN-18 owns, made by the crate
    /// that publishes it rather than by the note. `Warnings` is the closest item the
    /// agreement can express, so a party that may not receive warnings does not receive
    /// launch warnings either -- which is the restrictive reading. Recorded for the
    /// owner as a question for a DN-16/DN-18 amendment, not settled here.
    ///
    /// The two `Received` and `Refused` variants are never released: they are this
    /// deployment's record of what *its* peers told it, and forwarding them would let a
    /// partner read our peer list off the stream.
    #[must_use]
    pub fn releases(&self, party: &str, envelope: &Envelope) -> bool {
        use gungnir_model::events::{LaunchWarningEvent, TrackingEvent};
        match &envelope.event {
            gungnir_eventing::Event::Tracking(
                TrackingEvent::TrackInitiated(track) | TrackingEvent::TrackUpdated(track),
            ) => self
                .exchange
                .may_send(party, ExchangeItem::Tracks, &track.releasability),
            gungnir_eventing::Event::Tracking(TrackingEvent::TrackDeleted(_)) => self
                .exchange
                .for_party(party)
                .is_some_and(|a| a.sends(ExchangeItem::Tracks)),
            gungnir_eventing::Event::LaunchWarning(LaunchWarningEvent::Issued(warning)) => self
                .exchange
                .may_send(party, ExchangeItem::Warnings, &warning.releasability),
            _ => false,
        }
    }

    /// Install the authority that verifies callers.
    #[must_use]
    pub fn with_callers(mut self, callers: Arc<dyn CallerAuthority>) -> Self {
        self.callers = Some(callers);
        self
    }

    /// Whether this node can authenticate anybody.
    #[must_use]
    pub fn authenticates_callers(&self) -> bool {
        self.callers.is_some()
    }

    /// Replace the published coverage answer. Called by the tick.
    pub fn publish_coverage(&self, coverage: v3::CoverageResponse) -> Result<(), ApiError> {
        let mut slot = self
            .coverage
            .write()
            .map_err(|_| ApiError::Transport("the coverage lock was poisoned".into()))?;
        *slot = coverage;
        Ok(())
    }

    #[must_use]
    pub fn coverage(&self) -> Option<v3::CoverageResponse> {
        self.coverage.read().ok().map(|c| c.clone())
    }

    /// Advance the transport's view of mission time. Called with each snapshot.
    pub fn set_now(&self, now: MissionTimeSeconds) {
        if let Ok(mut slot) = self.now.write() {
            *slot = now;
        }
    }

    fn now(&self) -> MissionTimeSeconds {
        self.now.read().map(|n| *n).unwrap_or_default()
    }

    /// Take everything submitted over the API since the last call.
    ///
    /// The node's ingest adapter polls this, so a submission is authenticated and
    /// validated by the gateway like any sensor feed.
    #[must_use]
    pub fn take_submissions(&self) -> Vec<gungnir_model::DetectionView> {
        self.submissions
            .lock()
            .map(|mut queue| std::mem::take(&mut *queue))
            .unwrap_or_default()
    }

    /// Queue a detection for the gateway. **Not an acceptance**: the gateway may still
    /// quarantine it, and `IngestEvent::Quarantined` appears on the stream when it does.
    fn submit_detection(&self, detection: gungnir_model::DetectionView) -> Result<(), ApiError> {
        let mut queue = self
            .submissions
            .lock()
            .map_err(|_| ApiError::Transport("the submission queue lock was poisoned".into()))?;
        queue.push(detection);
        Ok(())
    }

    /// Replace the published snapshot. Called once per tick by the node.
    ///
    /// A poisoned lock is reported rather than unwrapped: the node logs it and keeps
    /// running on the previous snapshot, which is stale but true of some moment, whereas
    /// a panicking node serves nothing at all.
    pub fn publish_snapshot(&self, snapshot: v3::SnapshotResponse) -> Result<(), ApiError> {
        let mut slot = self
            .snapshot
            .write()
            .map_err(|_| ApiError::Transport("the snapshot lock was poisoned".into()))?;
        *slot = snapshot;
        Ok(())
    }

    /// Offer one envelope to every subscriber, and keep it for `from_seq` replay.
    ///
    /// Returns without error when nobody is listening: a node with no connected desktop
    /// is the normal case, not a failure.
    ///
    /// **Encoded here, once, and proved to read back** (GAP-153, D-96): the line every
    /// subscriber is sent is the one made now, in the lossless form a NaN or an infinity
    /// survives ([`nonfinite`]). The node binary journals the envelope before it offers it,
    /// and the journal holds the same line to the same test (D-77), so this refusal is
    /// the transport's own guarantee rather than one it borrows from its host.
    ///
    /// # Errors
    ///
    /// [`ApiError::Unencodable`], counted in [`Self::unencodable_envelopes`], for an
    /// envelope with no faithful line: offering it would send every subscriber a frame it
    /// could not read, and a desktop that reconnected from before it would be sent it
    /// again, every time. Refused, the stream has a gap in `seq` that a client sees.
    /// [`ApiError::Transport`] when the backlog's lock was poisoned.
    pub fn publish_event(&self, envelope: Envelope) -> Result<(), ApiError> {
        let line = match nonfinite::to_faithful_line(&envelope) {
            Ok(line) => line,
            Err(why) => {
                self.unencodable.fetch_add(1, Ordering::Relaxed);
                return Err(ApiError::Unencodable {
                    seq: envelope.seq,
                    why,
                });
            }
        };
        let offered = Arc::new(Offered { envelope, line });
        {
            let mut backlog = self
                .backlog
                .lock()
                .map_err(|_| ApiError::Transport("the backlog lock was poisoned".into()))?;
            if backlog.len() == BACKLOG_CAPACITY {
                backlog.pop_front();
            }
            backlog.push_back(Arc::clone(&offered));
        }
        // `Err` here means no receivers, which is not a problem worth reporting.
        let _ = self.events.send(offered);
        Ok(())
    }

    /// How many envelopes [`Self::publish_event`] has refused for having no faithful line
    /// since this node started (GAP-153, D-96). Zero on a healthy node; each refusal is
    /// also the error the caller was returned.
    #[must_use]
    pub fn unencodable_envelopes(&self) -> u64 {
        self.unencodable.load(Ordering::Relaxed)
    }

    /// How many event streams are currently subscribed.
    ///
    /// **Subscribed, not connected**, and the difference is the point. [`stream_events`]
    /// takes its receiver as its very first act, before it reads the client's subscribe
    /// frame, so a non-zero count means an envelope published from here on will reach
    /// that client -- whereas a link reporting `connected` has only had its *snapshot*
    /// answered over HTTP and may not have opened its socket yet.
    ///
    /// A test that publishes into that window loses the envelope silently and for good:
    /// a `from_seq` 0 subscription means "everything from now" by the contract, so
    /// [`NodeApi::backlog_since`] will not replay it either. `gungnir-app`'s failover
    /// end-to-end test waits on this before publishing for exactly that reason.
    #[must_use]
    pub fn subscriber_count(&self) -> usize {
        self.events.receiver_count()
    }

    #[must_use]
    pub fn snapshot(&self) -> Option<v3::SnapshotResponse> {
        self.snapshot.read().ok().map(|s| s.clone())
    }

    /// Envelopes from `from_seq` onward, or `None` when the window no longer reaches
    /// that far back and the client must take a fresh snapshot.
    ///
    /// `from_seq` 0 means "everything from now" per the contract, which is an empty
    /// backlog rather than the whole window.
    #[must_use]
    pub fn backlog_since(&self, from_seq: u64) -> Option<Vec<Envelope>> {
        self.offered_since(from_seq)
            .map(|offered| offered.iter().map(|o| o.envelope.clone()).collect())
    }

    /// [`Self::backlog_since`], as offered: each envelope with the line it is sent as.
    fn offered_since(&self, from_seq: u64) -> Option<Vec<Arc<Offered>>> {
        if from_seq == 0 {
            return Some(Vec::new());
        }
        let backlog = self.backlog.lock().ok()?;
        match backlog.front() {
            // Nothing retained yet: there is nothing this client has missed.
            None => Some(Vec::new()),
            Some(oldest) if oldest.envelope.seq <= from_seq => Some(
                backlog
                    .iter()
                    .filter(|o| o.envelope.seq >= from_seq)
                    .cloned()
                    .collect(),
            ),
            // The window has moved past what was asked for. Saying so is the contract's
            // own rule: a gap the client cannot see would let it believe it had an
            // unbroken stream.
            Some(_) => None,
        }
    }

    fn subscribe(&self) -> broadcast::Receiver<Arc<Offered>> {
        self.events.subscribe()
    }
}

/// The routes: every one under `/v3`, and the retired `/v2` ones answering `410 Gone`.
///
/// Each path is `crate::path` of a `crate::routes` constant, the pair the desktop's client
/// builds its URLs from, so the node and the desktop cannot disagree about where a route
/// is (GAP-130).
pub fn router(api: Arc<NodeApi>) -> Router {
    let served = Router::new()
        // One `route` call for the two methods on a path, as everywhere below: the pair
        // reads as one door with two directions.
        .route(
            &crate::path(routes::SESSION),
            post(sign_in).get(session_status),
        )
        .route(&crate::path(routes::SNAPSHOT), get(snapshot))
        .route(&crate::path(routes::HEALTH), get(health))
        .route(&crate::path(routes::COVERAGE), get(coverage))
        .route(&crate::path(routes::EVENTS), get(events))
        .route(&crate::path(routes::HISTORY), get(history))
        .route(&crate::path(routes::DETECTIONS), post(submit_detection))
        .route(&crate::path(routes::SENSOR_TASK), post(task_sensor))
        .route(&crate::path(routes::HANDOFF_REPORT), post(effector_report))
        .route(
            &crate::path(routes::WARNING_ACKNOWLEDGE),
            post(acknowledge_warning),
        )
        // DN-18's three items that had no door (GAP-065). Tracks and health keep theirs:
        // `/v3/snapshot` and `/v3/health` are already the two-gate paths for those, and a
        // second door to the same picture is a second place the gates could differ. Each
        // now carries both doors on the same path (GAP-065, DN-18 §5 amendment 2): `GET`
        // for a partner reading what this deployment holds, `POST` for the desktop that
        // holds it telling this node what that now is.
        .route(
            &crate::path(routes::EXCHANGE_WARNINGS),
            get(exchange_warnings).post(publish_warnings),
        )
        .route(
            &crate::path(routes::EXCHANGE_REPORTS),
            get(exchange_reports).post(publish_reports),
        )
        .route(
            &crate::path(routes::EXCHANGE_HANDOFFS),
            get(exchange_handoffs).post(publish_handoffs),
        )
        // GAP-132: the queue, and the decision on one of its items. There is no
        // `/v3/plans/{plan_id}/decision`: a decision is taken on the item, which is what
        // carries the deadline and the roles it is offered to, and a second door keyed by
        // plan would be a second place the four checks could differ.
        .route(&crate::path(routes::QUEUE), get(queue))
        .route(&crate::path(routes::QUEUE_DECISION), post(decide_queued))
        // GAP-134: what a desktop decided while it was cut off. Not in `RETIRED`: it is
        // new in `/v3`, and a `/v2` client never had it to call.
        .route(
            &crate::path(routes::DECISIONS_FORWARDED),
            post(forward_decisions),
        );
    RETIRED
        .iter()
        .fold(served, |router, retired| {
            let path = format!("/{}{}", crate::RETIRED_API_VERSION, retired.route);
            let authentication = retired.authentication;
            let successor = retired.successor;
            let answer = move |State(api): State<Arc<NodeApi>>,
                               ConnectInfo(peer): ConnectInfo<Peer>,
                               headers: axum::http::HeaderMap,
                               uri: axum::http::Uri| async move {
                gone(&api, &headers, &peer, uri.path(), authentication, successor)
            };
            match retired.method {
                RetiredMethod::Get => router.route(&path, get(answer)),
                RetiredMethod::Post => router.route(&path, post(answer)),
            }
        })
        .with_state(api)
}

/// How a route establishes who is calling, before it does anything else.
///
/// Named so a retired `/v2` route can do exactly what its `/v3` successor does (GAP-130).
/// Each variant is the check the successor's handler opens with; what a handler checks
/// after that -- a role's permission, an agreement's items, a certificate's role -- is
/// authorization, and a retired route authorizes nothing, because it does nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Authentication {
    /// `POST /session`: nothing, because signing in is what establishes identity.
    SignIn,
    /// An operator's token, or a machine party with an exchange agreement ([`caller`]).
    Caller,
    /// An operator's token alone ([`operator_caller`]), naming the route as the
    /// successor's refusal does.
    Operator(&'static str),
    /// A machine whose certificate speaks for something in this deployment, or else an
    /// operator's token ([`machine_identity`], then [`operator_caller`]).
    MachineOrOperator(&'static str),
    /// The event stream, whose token travels in the first frame after the upgrade: a
    /// retired stream never upgrades, so it never reads one.
    Stream,
}

/// A method a retired route answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetiredMethod {
    Get,
    Post,
}

/// One `/v2` route: its path below the version, its method, how its successor
/// authenticates, and -- where the successor is not the same path under `/v3` -- what it
/// is instead.
#[derive(Debug, Clone, Copy)]
struct RetiredRoute {
    route: &'static str,
    method: RetiredMethod,
    authentication: Authentication,
    /// The successor path, when it is not this route's own path under `/v3`.
    ///
    /// `None` for every route whose shape and meaning survived the version move, which is
    /// all but one: the successor is then the request's own path, parameters included, so
    /// a client is told exactly where to send what it sent. `Some` is for a route whose
    /// *key* changed -- `/v2/plans/{plan_id}/decision` became
    /// `/v3/queue/{item}/decision` in GAP-132 -- where the caller's own path cannot be
    /// rewritten into the successor, because a plan identifier is not a queue item's.
    /// The template is named instead, which is honest about what the client has to do
    /// (DN-31 §7; amendment 1's erratum, which fixed the successor at
    /// `/v3/plans/{plan_id}/decision` only "until GAP-132 builds §7's queue routes").
    successor: Option<&'static str>,
}

/// Every `/v2` route as it stood when `/v3` replaced it on 2026-09-17 (GAP-130).
///
/// **Frozen.** A route added under `/v3` later never existed under `/v2`, and a request
/// for it there is not found rather than gone; only what a `/v2` client could have called
/// is answered with its successor.
const RETIRED: &[RetiredRoute] = &[
    RetiredRoute {
        route: routes::SESSION,
        method: RetiredMethod::Post,
        authentication: Authentication::SignIn,
        successor: None,
    },
    RetiredRoute {
        route: routes::SESSION,
        method: RetiredMethod::Get,
        authentication: Authentication::Operator("the session route"),
        successor: None,
    },
    RetiredRoute {
        route: routes::SNAPSHOT,
        method: RetiredMethod::Get,
        authentication: Authentication::Caller,
        successor: None,
    },
    RetiredRoute {
        route: routes::HEALTH,
        method: RetiredMethod::Get,
        authentication: Authentication::Caller,
        successor: None,
    },
    RetiredRoute {
        route: routes::COVERAGE,
        method: RetiredMethod::Get,
        authentication: Authentication::Operator("coverage"),
        successor: None,
    },
    RetiredRoute {
        route: routes::EVENTS,
        method: RetiredMethod::Get,
        authentication: Authentication::Stream,
        successor: None,
    },
    RetiredRoute {
        route: routes::HISTORY,
        method: RetiredMethod::Get,
        authentication: Authentication::Caller,
        successor: None,
    },
    RetiredRoute {
        route: routes::DETECTIONS,
        method: RetiredMethod::Post,
        authentication: Authentication::MachineOrOperator("detection submission"),
        successor: None,
    },
    RetiredRoute {
        route: routes::SENSOR_TASK,
        method: RetiredMethod::Post,
        authentication: Authentication::Operator("sensor tasking"),
        successor: None,
    },
    RetiredRoute {
        route: routes::HANDOFF_REPORT,
        method: RetiredMethod::Post,
        authentication: Authentication::MachineOrOperator("effector reporting"),
        successor: None,
    },
    RetiredRoute {
        route: routes::WARNING_ACKNOWLEDGE,
        method: RetiredMethod::Post,
        authentication: Authentication::MachineOrOperator("warning acknowledgement"),
        successor: None,
    },
    RetiredRoute {
        route: routes::EXCHANGE_WARNINGS,
        method: RetiredMethod::Get,
        authentication: Authentication::Caller,
        successor: None,
    },
    RetiredRoute {
        route: routes::EXCHANGE_WARNINGS,
        method: RetiredMethod::Post,
        authentication: Authentication::Operator("publishing to exchange"),
        successor: None,
    },
    RetiredRoute {
        route: routes::EXCHANGE_REPORTS,
        method: RetiredMethod::Get,
        authentication: Authentication::Caller,
        successor: None,
    },
    RetiredRoute {
        route: routes::EXCHANGE_REPORTS,
        method: RetiredMethod::Post,
        authentication: Authentication::Operator("publishing to exchange"),
        successor: None,
    },
    RetiredRoute {
        route: routes::EXCHANGE_HANDOFFS,
        method: RetiredMethod::Get,
        authentication: Authentication::Caller,
        successor: None,
    },
    RetiredRoute {
        route: routes::EXCHANGE_HANDOFFS,
        method: RetiredMethod::Post,
        authentication: Authentication::Operator("publishing to exchange"),
        successor: None,
    },
    RetiredRoute {
        route: routes::PLAN_DECISION,
        method: RetiredMethod::Post,
        authentication: Authentication::Operator("the decision route"),
        // The one route whose successor is not its own path under `/v3`: GAP-132 keyed the
        // decision on the queue item rather than on the plan, and `/v3` serves no
        // plan-keyed decision route at all. Naming the client's own path would send it to
        // a `404`; naming the template says what actually changed.
        successor: Some(routes::QUEUE_DECISION),
    },
];

/// A retired route's answer: the successor's authentication, then `410 Gone` naming the
/// successor (GAP-130, DN-31 §5.1).
///
/// The successor is the request's own path under `/v3`, parameters included, so a client
/// is told exactly where to send what it sent. It is in the message for a person and in
/// `successor` for a program.
fn gone(
    api: &NodeApi,
    headers: &axum::http::HeaderMap,
    peer: &Peer,
    path: &str,
    authentication: Authentication,
    named_successor: Option<&'static str>,
) -> Response {
    let authenticated = match authentication {
        Authentication::SignIn | Authentication::Stream => Ok(()),
        Authentication::Caller => caller(api, headers, peer, path).map(|_| ()),
        Authentication::Operator(what) => operator_caller(api, headers, peer, what).map(|_| ()),
        Authentication::MachineOrOperator(what) => match machine_identity(api, headers, peer) {
            Some(_) => Ok(()),
            None => operator_caller(api, headers, peer, what).map(|_| ()),
        },
    };
    if let Err(refused) = authenticated {
        return refused;
    }
    let below = path
        .strip_prefix(&format!("/{}", crate::RETIRED_API_VERSION))
        .unwrap_or(path);
    // The request's own path under `/v3`, unless the route's key changed and the caller's
    // path cannot be rewritten into the successor at all (see `RetiredRoute::successor`).
    let successor = crate::path(named_successor.unwrap_or(below));
    (
        StatusCode::GONE,
        Json(serde_json::json!({
            "error": StatusCode::GONE.as_u16(),
            "message": format!(
                "/{} was retired on 2026-09-17: decision, plan and queue-item identifiers \
                 became UUID v7 written as strings (D-56, D-60), so every payload carrying \
                 one changed. This route is now {successor}.",
                crate::RETIRED_API_VERSION
            ),
            "successor": successor,
        })),
    )
        .into_response()
}

/// Who is asking (GAP-062, D-02): an operator with a session token, or a machine whose
/// client certificate the TLS handshake verified and whose party has an agreement.
#[derive(Debug, Clone, PartialEq)]
pub enum Caller {
    Operator(OperatorSession),
    Machine { party: String },
}

/// Why a caller was refused before anything about it was verified: the status and the
/// sentence the caller and the audit entry both get.
type Unverified = (StatusCode, String);

/// Resolve the caller, or say why not.
///
/// Every route but `POST /v3/session` goes through this. A bearer token is tried first,
/// because a desktop on a mutual-TLS link still speaks for an operator; a connection
/// with a party and no token is a machine, and a machine with no agreement is refused
/// here (DN-18 §5: no agreement, no exchange). A node with no authority configured
/// refuses operators here rather than at each route, so there is one place the answer
/// is decided.
fn resolve_caller(
    api: &NodeApi,
    headers: &axum::http::HeaderMap,
    peer: &Peer,
) -> Result<Caller, Unverified> {
    let has_token = headers.get(axum::http::header::AUTHORIZATION).is_some();
    if let (false, Some(party)) = (has_token, &peer.party) {
        if api.exchange.for_party(party).is_none() {
            return Err((
                StatusCode::FORBIDDEN,
                format!(
                    "party {party:?} is authenticated and has no exchange agreement with \
                     this deployment; authentication answers who you are, the agreement \
                     answers what you may do (DN-18)"
                ),
            ));
        }
        return Ok(Caller::Machine {
            party: party.clone(),
        });
    }
    resolve_operator(api, headers).map(Caller::Operator)
}

/// The caller, or a refusal that is on the node's audit record (GAP-111, D-87).
///
/// `what` names the route in the entry, so an auditor reading `access.refused` can tell a
/// desktop whose session lapsed while it polled the snapshot from a probe of the
/// decision route.
///
/// The error is a whole `Response` and therefore large. Boxing it would buy nothing: it
/// is constructed once per refused request and returned immediately.
#[allow(clippy::result_large_err)]
fn caller(
    api: &NodeApi,
    headers: &axum::http::HeaderMap,
    peer: &Peer,
    what: &str,
) -> Result<Caller, Response> {
    resolve_caller(api, headers, peer).map_err(|(status, why)| {
        api.audit(
            peer,
            None,
            events::ACCESS_REFUSED,
            format_args!("{what}: {status}: {why}"),
        );
        problem(status, &why)
    })
}

/// A machine whose certificate speaks for something in this deployment (D-02), when the
/// connection carries no token. Tried by the routes that serve a sensor or an effector
/// before the ordinary caller resolution, which needs an exchange agreement.
fn machine_identity(
    api: &NodeApi,
    headers: &axum::http::HeaderMap,
    peer: &Peer,
) -> Option<(String, MachineRole)> {
    if headers.get(axum::http::header::AUTHORIZATION).is_some() {
        return None;
    }
    let party = peer.party.as_ref()?;
    let role = api.identities.get(party)?;
    Some((party.clone(), role.clone()))
}

/// An operator from the `Authorization` header, or why not.
fn resolve_operator(
    api: &NodeApi,
    headers: &axum::http::HeaderMap,
) -> Result<OperatorSession, Unverified> {
    let Some(callers) = api.callers.as_ref() else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "this node authenticates nobody: no account store is configured, so it serves \
             its pipeline and journals it and answers no caller"
                .into(),
        ));
    };
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default();
    // One message for a missing, malformed, forged and expired token alike: telling them
    // apart tells a prober which half to work on.
    callers
        .verify(token, api.now())
        .map_err(|failure| (StatusCode::UNAUTHORIZED, failure.to_string()))
}

/// `POST /v3/session`: the one route reachable without a token.
///
/// **Every attempt is audited** (DN-23 §5 rule 7, GAP-111): `session.sign_in` naming the
/// operator the credential verified and the role the session carries, or
/// `session.rejected` naming nobody -- the identifier in the request was claimed, not
/// verified (rule 1), so it goes in the detail. The passphrase goes nowhere.
async fn sign_in(
    State(api): State<Arc<NodeApi>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    Json(request): Json<v3::SessionRequest>,
) -> Response {
    let Some(callers) = api.callers.as_ref() else {
        let why = "this node has no account store configured, so nobody can sign in";
        api.audit(
            &peer,
            None,
            events::SIGN_IN_REJECTED,
            format_args!("operator {} was tried: {why}", request.operator),
        );
        return problem(StatusCode::SERVICE_UNAVAILABLE, why);
    };
    let now = api.now();
    match callers.sign_in(request.operator, &request.passphrase, now) {
        Ok(issued) => {
            // The role is read back off the token just minted, so the entry says what the
            // session will be believed to be rather than what the store said a moment ago.
            let role = callers.verify(&issued.token, now).map_or_else(
                |failure| format!("a token that does not verify here ({failure})"),
                |session| format!("{:?}", session.role),
            );
            api.audit(
                &peer,
                Some(OperatorId(request.operator)),
                events::SIGN_IN,
                format_args!(
                    "signed in as {role}; the session expires at mission time {}",
                    issued.expires_s
                ),
            );
            Json(v3::SessionResponse {
                token: issued.token,
                expires_s: issued.expires_s,
            })
            .into_response()
        }
        Err(failure) => {
            api.audit(
                &peer,
                None,
                events::SIGN_IN_REJECTED,
                format_args!("operator {} was tried: {failure}", request.operator),
            );
            problem(StatusCode::UNAUTHORIZED, &failure.to_string())
        }
    }
}

/// A route an operator alone may use: the write paths, and what is internal to the
/// deployment. A machine caller is told so rather than served a shape it cannot use.
fn resolve_operator_only(caller: Caller, what: &str) -> Result<OperatorSession, Unverified> {
    match caller {
        Caller::Operator(session) => Ok(session),
        Caller::Machine { party } => Err((
            StatusCode::FORBIDDEN,
            format!("{what} is internal to this deployment and not an exchange item; party {party:?} may not use it"),
        )),
    }
}

/// The caller, refused unless it is an operator: the routes that are internal to the
/// deployment and never an exchange item. A refusal is on the audit record.
#[allow(clippy::result_large_err)]
fn operator_caller(
    api: &NodeApi,
    headers: &axum::http::HeaderMap,
    peer: &Peer,
    what: &str,
) -> Result<OperatorSession, Response> {
    unaudited_operator_caller(api, headers, peer, what).map_err(|(status, why)| {
        api.audit(
            peer,
            None,
            events::ACCESS_REFUSED,
            format_args!("{what}: {status}: {why}"),
        );
        problem(status, &why)
    })
}

/// [`operator_caller`] without the audit entry, for the two decision routes, whose
/// refusals the loop records under `plan.decide` (DN-31 §9 row 4): recording one here as
/// well would put two entries on the record for one refusal.
fn unaudited_operator_caller(
    api: &NodeApi,
    headers: &axum::http::HeaderMap,
    peer: &Peer,
    what: &str,
) -> Result<OperatorSession, Unverified> {
    resolve_caller(api, headers, peer).and_then(|c| resolve_operator_only(c, what))
}

/// `picture.view`, asked of an operator on a route that serves the picture (GAP-111).
///
/// Every role holds it but the security officer, who "operates nothing -- no decision,
/// tasking, configuration, or picture" (D-30). Until GAP-111 the snapshot, history,
/// coverage, stream and exchange routes served any valid token, so a security officer's
/// session read the picture its role withholds; `GET /v3/queue` alone asked. A refusal is
/// on the audit record under the action; a read that is served is not (D-87).
#[allow(clippy::result_large_err)]
fn may_view_picture(
    api: &NodeApi,
    peer: &Peer,
    session: &OperatorSession,
    what: &str,
) -> Result<(), Response> {
    if gungnir_security::authz::role_permits(session.role, actions::VIEW_PICTURE) {
        return Ok(());
    }
    let why = format!(
        "role {:?} may not read {what} ({})",
        session.role,
        actions::VIEW_PICTURE
    );
    api.audit(
        peer,
        Some(session.operator),
        actions::VIEW_PICTURE,
        format_args!("refused: {why}"),
    );
    Err(problem(StatusCode::FORBIDDEN, &why))
}

/// `GET /v3/session`: who the caller is, so a desktop can tell an expired session from
/// an unreachable node.
async fn session_status(
    State(api): State<Arc<NodeApi>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    headers: axum::http::HeaderMap,
) -> Response {
    match operator_caller(&api, &headers, &peer, "the session route") {
        Err(response) => response,
        Ok(session) => Json(v3::SessionStatus {
            operator: session.operator.0,
            role: format!("{:?}", session.role),
            expires_s: session.expires.unwrap_or_default(),
        })
        .into_response(),
    }
}

/// Serve the contract in the clear, until the future is dropped.
///
/// Refuses any address that is not loopback: see the module documentation. The refusal is
/// an error rather than a warning because a node that logged and carried on would still
/// be listening. For a routable address, configure TLS and use `serve_on_listener` with a
/// `tls::TlsListener`.
pub async fn serve(addr: SocketAddr, api: Arc<NodeApi>) -> Result<(), ApiError> {
    if !addr.ip().is_loopback() {
        return Err(ApiError::UnprotectedBind(addr));
    }
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| ApiError::Transport(format!("could not bind {addr}: {e}")))?;
    axum::serve(
        PlainListener(listener),
        router(api).into_make_service_with_connect_info::<Peer>(),
    )
    .await
    .map_err(|e| ApiError::Transport(e.to_string()))
}

/// Bind without serving, so a caller can learn the port an ephemeral bind chose.
///
/// Exists for tests, which must not race a background task for a port number.
pub async fn bind(addr: SocketAddr) -> Result<tokio::net::TcpListener, ApiError> {
    if !addr.ip().is_loopback() {
        return Err(ApiError::UnprotectedBind(addr));
    }
    tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| ApiError::Transport(format!("could not bind {addr}: {e}")))
}

/// Serve mutual TLS.
///
/// One router for both this and the plaintext case, so the contract cannot differ
/// between a loopback node and one serving mutual TLS (GAP-060); the only difference is
/// that a connection here carries the party its certificate names (GAP-062).
pub async fn serve_on_listener(listener: TlsListener, api: Arc<NodeApi>) -> Result<(), ApiError> {
    axum::serve(
        listener,
        router(api).into_make_service_with_connect_info::<Peer>(),
    )
    .await
    .map_err(|e| ApiError::Transport(e.to_string()))
}

/// Serve on an already-bound plaintext listener.
pub async fn serve_on(
    listener: tokio::net::TcpListener,
    api: Arc<NodeApi>,
) -> Result<(), ApiError> {
    axum::serve(
        PlainListener(listener),
        router(api).into_make_service_with_connect_info::<Peer>(),
    )
    .await
    .map_err(|e| ApiError::Transport(e.to_string()))
}

async fn snapshot(
    State(api): State<Arc<NodeApi>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    headers: axum::http::HeaderMap,
) -> Response {
    let snapshot = match caller(&api, &headers, &peer, "the snapshot") {
        Err(response) => return response,
        Ok(Caller::Operator(session)) => {
            if let Err(refused) = may_view_picture(&api, &peer, &session, "the snapshot") {
                return refused;
            }
            api.snapshot()
        }
        // GAP-062: a party sees what its agreement and the markings release, and how
        // much it did not see.
        Ok(Caller::Machine { party }) => api.snapshot_for(&party),
    };
    match snapshot {
        Some(mut snapshot) => {
            // GAP-140: this node's clock as it answers. The deadlines in `queue` are in
            // this time, and a desktop that draws them against its own is wrong by the
            // difference between two machines with nothing saying so.
            snapshot.node_time = Some(gungnir_model::MissionTime(api.now()));
            // GAP-153: a diverged track's NaN covariance is carried, not written `null`.
            lossless_json(&snapshot)
        }
        None => problem(
            StatusCode::INTERNAL_SERVER_ERROR,
            "the node's snapshot is unreadable",
        ),
    }
}

/// `GET /v3/history?since_seq=N`: the retained envelopes from `N` (GAP-050).
///
/// `410 Gone` when the window has moved past `N`: the client's outage is longer than the
/// node retains, and saying so is the contract's rule for the stream as well.
async fn history(
    State(api): State<Arc<NodeApi>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    headers: axum::http::HeaderMap,
    axum::extract::Query(query): axum::extract::Query<v3::HistoryQuery>,
) -> Response {
    let party = match caller(&api, &headers, &peer, "the history") {
        Err(response) => return response,
        Ok(Caller::Operator(session)) => {
            if let Err(refused) = may_view_picture(&api, &peer, &session, "the history") {
                return refused;
            }
            None
        }
        Ok(Caller::Machine { party }) => Some(party),
    };
    match api.backlog_since(query.since_seq) {
        Some(envelopes) => {
            let total = envelopes.len();
            let envelopes: Vec<Envelope> = match &party {
                None => envelopes,
                Some(party) => envelopes
                    .into_iter()
                    .filter(|e| api.releases(party, e))
                    .collect(),
            };
            // GAP-153, D-96: the lossless form, so a NaN in the history is the NaN the
            // journal holds rather than a `null` that fails the reconciliation's read.
            lossless_json(&v3::HistoryResponse {
                since_seq: query.since_seq,
                withheld: total - envelopes.len(),
                envelopes,
            })
        }
        None => problem(
            StatusCode::GONE,
            &format!(
                "seq {} is older than this node retains ({BACKLOG_CAPACITY} envelopes); the \
                 journal for that period is on the node's disk, not on this route",
                query.since_seq
            ),
        ),
    }
}

/// `GET /v3/coverage`: the gaps along the configured approaches, with the parameters
/// that found them.
async fn coverage(
    State(api): State<Arc<NodeApi>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    headers: axum::http::HeaderMap,
) -> Response {
    match operator_caller(&api, &headers, &peer, "coverage") {
        Err(response) => return response,
        Ok(session) => {
            if let Err(refused) = may_view_picture(&api, &peer, &session, "coverage") {
                return refused;
            }
        }
    }
    match api.coverage() {
        Some(coverage) => Json(coverage).into_response(),
        None => problem(
            StatusCode::INTERNAL_SERVER_ERROR,
            "the node's coverage answer is unreadable",
        ),
    }
}

async fn health(
    State(api): State<Arc<NodeApi>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    headers: axum::http::HeaderMap,
) -> Response {
    match caller(&api, &headers, &peer, "health") {
        Err(response) => return response,
        // Any operator, the security officer included: its layout is the audit and
        // health panels (D-30), and health is not the picture.
        Ok(Caller::Operator(_)) => {}
        // DN-18: health is an exchange item of its own.
        Ok(Caller::Machine { party }) => {
            if !api.exchange.may_send(
                &party,
                ExchangeItem::Health,
                &gungnir_model::Releasability::AllPeers,
            ) {
                let why = format!("the agreement with {party:?} does not send health");
                api.audit(
                    &peer,
                    None,
                    events::ACCESS_REFUSED,
                    format_args!("health: {why}"),
                );
                return problem(StatusCode::FORBIDDEN, &why);
            }
        }
    }
    match api.snapshot() {
        Some(snapshot) => Json(snapshot.health).into_response(),
        None => Json(SystemHealth::default()).into_response(),
    }
}

/// `POST /v3/detections`: queue a detection for the ingest gateway.
///
/// **Queued, not accepted.** The gateway authenticates the sensor and validates the
/// detection on its next tick exactly as it does a sensor feed, and quarantines it with a
/// reason if it fails -- which appears on the event stream. Answering `202` rather than
/// `204` says precisely that: it has been taken, not that it has been believed.
async fn submit_detection(
    State(api): State<Arc<NodeApi>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    headers: axum::http::HeaderMap,
    body: Result<Json<v3::SubmitDetectionRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    // A peer's tracks enter through the peer adapter under DN-16, never this route.
    // A sensor with a certificate submits for its own id (GAP-002); everyone else is an
    // operator.
    let vouched = match machine_identity(&api, &headers, &peer) {
        Some((_, MachineRole::Sensor(id))) => Some(id),
        Some((party, role)) => {
            let why = format!(
                "party {party:?} speaks for {role:?}, not for a sensor; it may not submit \
                 detections"
            );
            api.audit(
                &peer,
                None,
                events::ACCESS_REFUSED,
                format_args!("detection submission: {why}"),
            );
            return problem(StatusCode::FORBIDDEN, &why);
        }
        None => {
            if let Err(response) = operator_caller(&api, &headers, &peer, "detection submission") {
                return response;
            }
            None
        }
    };
    let Ok(Json(request)) = body else {
        return problem(
            StatusCode::BAD_REQUEST,
            "the detection could not be decoded",
        );
    };
    // The canonical model this caller speaks, checked before its payload is read as
    // anything. `docs/gungnir-api-v1.md`'s compatibility rule was amended on 2026-09-06
    // to require a new path version only where a client that does not know about a
    // change could silently misinterpret a payload, and to accept a schema bump alone
    // where such a client is refused. **This is that refusal**, and until it was written
    // there was none on any inbound path: a caller posting a previous shape was refused
    // only where serde happened to be unable to read it, which answers "the body did not
    // decode" and says nothing about versions.
    if let Err(err) = v3::refuse_other_schema(request.schema_version) {
        return problem(StatusCode::CONFLICT, &err.to_string());
    }
    match vouched {
        Some(id) if request.detection.sensor != id => {
            let why = format!(
                "this certificate speaks for sensor {}, and the detection names sensor {}",
                id.0, request.detection.sensor.0
            );
            api.audit(
                &peer,
                None,
                actions::SUBMIT_DETECTION,
                format_args!("refused: {why}"),
            );
            problem(StatusCode::FORBIDDEN, &why)
        }
        Some(_) => match api.machine_submissions.lock() {
            Ok(mut queue) => {
                queue.push(request.detection);
                StatusCode::ACCEPTED.into_response()
            }
            Err(_) => problem(
                StatusCode::INTERNAL_SERVER_ERROR,
                "the machine submission queue lock was poisoned",
            ),
        },
        None => match api.submit_detection(request.detection) {
            Ok(()) => StatusCode::ACCEPTED.into_response(),
            Err(err) => problem(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
        },
    }
}

/// How long the task route waits for the node loop to issue a command. The loop ticks
/// far faster than this; the bound exists so a stalled loop answers rather than hangs.
const TASK_REPLY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// `POST /v3/sensors/{sensor_id}/task` (GAP-004): an operator with the `sensor.task`
/// action asks the node's registry to command a sensor.
///
/// Answered with the node's task id once the loop has issued it, so the caller can
/// match the `SensorTaskEvent`s that follow on the stream. The registry's refusal (not
/// controllable, an illegal mode transition) is a `409` in the registry's own words;
/// nothing is recorded for a refused command, by DN-11 §5.
async fn task_sensor(
    State(api): State<Arc<NodeApi>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    axum::extract::Path(sensor_id): axum::extract::Path<u32>,
    headers: axum::http::HeaderMap,
    body: Result<Json<v3::SensorTaskRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let session = match operator_caller(&api, &headers, &peer, "sensor tasking") {
        Ok(session) => session,
        Err(response) => return response,
    };
    // One entry for every request past the door, whatever became of it (GAP-111).
    let record = |detail: &dyn std::fmt::Display| {
        api.audit(
            &peer,
            Some(session.operator),
            actions::TASK_SENSOR,
            format_args!("sensor {sensor_id}: {detail}"),
        );
    };
    let refuse = |status: StatusCode, why: &str| {
        record(&format_args!("refused: {why}"));
        problem(status, why)
    };
    if !gungnir_security::authz::role_permits(session.role, actions::TASK_SENSOR) {
        return refuse(
            StatusCode::FORBIDDEN,
            &format!(
                "role {:?} may not command a sensor ({})",
                session.role,
                actions::TASK_SENSOR
            ),
        );
    }
    let Ok(Json(request)) = body else {
        return refuse(StatusCode::BAD_REQUEST, "the task could not be decoded");
    };
    let commanded = format!("{:?}", request.command);
    let (reply, answer) = tokio::sync::oneshot::channel();
    {
        let Ok(mut queue) = api.tasks.lock() else {
            return refuse(
                StatusCode::INTERNAL_SERVER_ERROR,
                "the task queue lock was poisoned",
            );
        };
        queue.push(PendingSensorTask {
            sensor: SensorId(sensor_id),
            command: request.command,
            requirement: request.requirement,
            reply,
        });
    }
    match tokio::time::timeout(TASK_REPLY_TIMEOUT, answer).await {
        Ok(Ok(Ok(task))) => {
            record(&format_args!("commanded {commanded}: task {}", task.0));
            (StatusCode::ACCEPTED, Json(v3::SensorTaskResponse { task })).into_response()
        }
        Ok(Ok(Err(reason))) => {
            record(&format_args!(
                "{commanded} was refused by the registry: {reason}"
            ));
            problem(StatusCode::CONFLICT, &reason)
        }
        Ok(Err(_)) => refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "the node loop dropped the task without answering",
        ),
        // Not "nothing was done": the loop may issue it after the window. Said as what it
        // is, since the task event on the journal is what settles it.
        Err(_) => {
            let why = "the node loop did not issue the task within the reply window";
            record(&format_args!(
                "{commanded}: not answered in time; the journal's task events say whether it was issued"
            ));
            problem(StatusCode::GATEWAY_TIMEOUT, why)
        }
    }
}

/// `POST /v3/handoffs/{decision_id}/report` (GAP-040): what the effector says.
///
/// A machine whose certificate speaks for a handoff endpoint, or an operator with the
/// `effector.report` action keying in what came over the radio. The node knows no
/// handoffs -- a desktop issues them -- so it puts the report on the record for every
/// desktop and the issuing one applies it or rejects it as naming an unknown decision.
async fn effector_report(
    State(api): State<Arc<NodeApi>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    decision: Result<axum::extract::Path<DecisionId>, axum::extract::rejection::PathRejection>,
    headers: axum::http::HeaderMap,
    body: Result<Json<v3::EffectorReportRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let (endpoint, operator) = match machine_identity(&api, &headers, &peer) {
        Some((_, MachineRole::Effector { endpoint })) => (endpoint, None),
        Some((party, role)) => {
            let why = format!("party {party:?} speaks for {role:?}, not for an effector");
            api.audit(
                &peer,
                None,
                events::ACCESS_REFUSED,
                format_args!("effector reporting: {why}"),
            );
            return problem(StatusCode::FORBIDDEN, &why);
        }
        None => match operator_caller(&api, &headers, &peer, "effector reporting") {
            Ok(session) => {
                if !gungnir_security::authz::role_permits(session.role, actions::EFFECTOR_REPORT) {
                    let why = format!("role {:?} may not record an effector report", session.role);
                    api.audit(
                        &peer,
                        Some(session.operator),
                        actions::EFFECTOR_REPORT,
                        format_args!("refused: {why}"),
                    );
                    return problem(StatusCode::FORBIDDEN, &why);
                }
                (
                    format!("operator:{}", session.operator.0),
                    Some(session.operator),
                )
            }
            Err(response) => return response,
        },
    };
    // One entry for every report past the door, whatever became of it (GAP-111): the
    // machine the certificate verified, or the operator keying it in.
    let record = |detail: &dyn std::fmt::Display| {
        api.audit(&peer, operator, actions::EFFECTOR_REPORT, detail);
    };
    // Either written form of the identifier (D-60): the hyphenated UUID a desktop issues
    // since GAP-130, or the decimal number an effector written before it sends. Anything
    // else is refused in the problem shape every other refusal here takes, naming both
    // forms, rather than with axum's plain-text rejection a client cannot read.
    let Ok(axum::extract::Path(decision)) = decision else {
        let why = "the decision in the path is not an identifier: expected the hyphenated UUID \
                   form (xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx) or a decimal number";
        record(&format_args!("refused: {why}"));
        return problem(StatusCode::BAD_REQUEST, why);
    };
    let Ok(Json(request)) = body else {
        let why = "the report could not be decoded";
        record(&format_args!("refused: decision {decision}: {why}"));
        return problem(StatusCode::BAD_REQUEST, why);
    };
    let Ok(mut queue) = api.reports.lock() else {
        let why = "the report queue lock was poisoned";
        record(&format_args!("refused: decision {decision}: {why}"));
        return problem(StatusCode::INTERNAL_SERVER_ERROR, why);
    };
    queue.push(EffectorReportRecord {
        decision,
        endpoint: endpoint.clone(),
        report: request.report,
    });
    drop(queue);
    record(&format_args!(
        "decision {decision}: a report from {endpoint} taken for the record"
    ));
    StatusCode::ACCEPTED.into_response()
}

/// `POST /v3/warnings/{asset_id}/{track_id}/acknowledge` (GAP-042, DN-03 §5 rule 2): the
/// warned party says it was told.
///
/// A machine whose certificate speaks for a warning channel, or an operator with the
/// `warning.acknowledge` action keying in what came over the radio -- the same two ways in
/// that [`effector_report`] has, because it is the same shape of fact: an outside party
/// answering something this deployment sent it.
///
/// **The node knows no warnings** -- a desktop raises them against its own defended
/// assets -- so it queues the acknowledgement for the record exactly as it does an
/// effector report, and the desktop that raised the warning applies it or rejects it as
/// naming a pair it never raised. Without this route a delivered warning sat in `Sent`
/// and then went `Late` for ever, because `Warning::acknowledged` had no caller.
async fn acknowledge_warning(
    State(api): State<Arc<NodeApi>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    axum::extract::Path((asset_id, track_id)): axum::extract::Path<(u32, u64)>,
    headers: axum::http::HeaderMap,
    body: Result<Json<v3::WarningAcknowledgementRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let (party, operator) = match machine_identity(&api, &headers, &peer) {
        Some((_, MachineRole::WarnedParty { channel })) => (channel, None),
        Some((party, role)) => {
            let why = format!("party {party:?} speaks for {role:?}, not for a warned party");
            api.audit(
                &peer,
                None,
                events::ACCESS_REFUSED,
                format_args!("warning acknowledgement: {why}"),
            );
            return problem(StatusCode::FORBIDDEN, &why);
        }
        None => match operator_caller(&api, &headers, &peer, "warning acknowledgement") {
            Ok(session) => {
                if !gungnir_security::authz::role_permits(
                    session.role,
                    actions::ACKNOWLEDGE_WARNING,
                ) {
                    let why = format!(
                        "role {:?} may not acknowledge a warning on a party's behalf",
                        session.role
                    );
                    api.audit(
                        &peer,
                        Some(session.operator),
                        actions::ACKNOWLEDGE_WARNING,
                        format_args!("refused: {why}"),
                    );
                    return problem(StatusCode::FORBIDDEN, &why);
                }
                (
                    format!("operator:{}", session.operator.0),
                    Some(session.operator),
                )
            }
            Err(response) => return response,
        },
    };
    let record = |detail: &dyn std::fmt::Display| {
        api.audit(
            &peer,
            operator,
            actions::ACKNOWLEDGE_WARNING,
            format_args!("asset {asset_id}, track {track_id}: {detail}"),
        );
    };
    let Ok(Json(request)) = body else {
        let why = "the acknowledgement could not be decoded";
        record(&format_args!("refused: {why}"));
        return problem(StatusCode::BAD_REQUEST, why);
    };
    let Ok(mut queue) = api.acknowledgements.lock() else {
        let why = "the acknowledgement queue lock was poisoned";
        record(&format_args!("refused: {why}"));
        return problem(StatusCode::INTERNAL_SERVER_ERROR, why);
    };
    queue.push(WarningAcknowledgement {
        asset: AssetId(asset_id),
        track: TrackId(track_id),
        party: party.clone(),
        at: request.at,
    });
    drop(queue);
    record(&format_args!(
        "acknowledged by {party}, taken for the record"
    ));
    StatusCode::ACCEPTED.into_response()
}

/// `GET /v3/exchange/warnings` (DN-18 §5, GAP-065).
async fn exchange_warnings(
    State(api): State<Arc<NodeApi>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    headers: axum::http::HeaderMap,
) -> Response {
    serve_exchange(&api, &headers, &peer, ExchangeItem::Warnings)
}

/// `GET /v3/exchange/reports` (DN-18 §5, GAP-065).
async fn exchange_reports(
    State(api): State<Arc<NodeApi>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    headers: axum::http::HeaderMap,
) -> Response {
    serve_exchange(&api, &headers, &peer, ExchangeItem::Reports)
}

/// `GET /v3/exchange/handoffs` (DN-18 §5, GAP-065).
async fn exchange_handoffs(
    State(api): State<Arc<NodeApi>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    headers: axum::http::HeaderMap,
) -> Response {
    serve_exchange(&api, &headers, &peer, ExchangeItem::Handoffs)
}

/// `POST /v3/exchange/warnings` (GAP-065, DN-18 §5 amendment 2).
async fn publish_warnings(
    State(api): State<Arc<NodeApi>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    headers: axum::http::HeaderMap,
    body: Result<Json<v3::PublishExchangeRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    publish_exchange_item(&api, &headers, &peer, body, ExchangeItem::Warnings)
}

/// `POST /v3/exchange/reports` (GAP-065, DN-18 §5 amendment 2).
async fn publish_reports(
    State(api): State<Arc<NodeApi>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    headers: axum::http::HeaderMap,
    body: Result<Json<v3::PublishExchangeRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    publish_exchange_item(&api, &headers, &peer, body, ExchangeItem::Reports)
}

/// `POST /v3/exchange/handoffs` (GAP-065, DN-18 §5 amendment 2).
async fn publish_handoffs(
    State(api): State<Arc<NodeApi>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    headers: axum::http::HeaderMap,
    body: Result<Json<v3::PublishExchangeRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    publish_exchange_item(&api, &headers, &peer, body, ExchangeItem::Handoffs)
}

/// The three exchange publish routes, which differ only in the item (GAP-065, DN-18 §5
/// amendment 2). **`gungnir-api` write path: human-owned** (`docs/agentic-workflow.md`;
/// signatures: docs/signatures.md).
///
/// The caller is this deployment's own desktop link, posting under its own operator
/// session token to tell its node what it now holds -- the same caller [`task_sensor`]
/// answers to, and unlike the caller [`effector_report`] and [`acknowledge_warning`]
/// answer to: those are an outside party telling this deployment something happened,
/// this is this deployment telling its own node something about itself. So there is no
/// machine-identity path here and no queue for the node loop to drain: `publish_exchange`
/// replaces the held set synchronously, and the handler answers as soon as it has.
///
/// **Which set it replaces is the caller's own** (GAP-137, DN-18 §5 amendment 3). The
/// producer is the common name this connection was verified under, which since GAP-141 is
/// a digest of the desktop's own key: one set per desktop, the node's own beside them,
/// and a read merges them. A link with no client certificate names nobody, so every such
/// writer shares [`ExchangeProducer::Unidentified`] and they overwrite each other as
/// before -- said in that variant's own doc comment rather than left to be discovered.
///
/// **`PUBLISH_EXCHANGE` is not `RELEASE_PRODUCT`.** The action a caller must hold is the
/// one for transmitting a product, not the one for marking it releasable in the first
/// place; see the constant's own doc comment for why the two stay apart.
fn publish_exchange_item(
    api: &NodeApi,
    headers: &axum::http::HeaderMap,
    peer: &Peer,
    body: Result<Json<v3::PublishExchangeRequest>, axum::extract::rejection::JsonRejection>,
    item: ExchangeItem,
) -> Response {
    let session = match operator_caller(api, headers, peer, "publishing to exchange") {
        Ok(session) => session,
        Err(response) => return response,
    };
    // One entry for every request past the door, whatever became of it (GAP-111).
    let record = |detail: &dyn std::fmt::Display| {
        api.audit(
            peer,
            Some(session.operator),
            actions::PUBLISH_EXCHANGE,
            format_args!("{item:?}: {detail}"),
        );
    };
    let refuse = |status: StatusCode, why: &str| {
        record(&format_args!("refused: {why}"));
        problem(status, why)
    };
    if !gungnir_security::authz::role_permits(session.role, actions::PUBLISH_EXCHANGE) {
        return refuse(
            StatusCode::FORBIDDEN,
            &format!(
                "role {:?} may not publish to exchange ({})",
                session.role,
                actions::PUBLISH_EXCHANGE
            ),
        );
    }
    let Ok(Json(request)) = body else {
        return refuse(StatusCode::BAD_REQUEST, "the products could not be decoded");
    };
    let producer = match &peer.party {
        Some(party) => ExchangeProducer::Party(party.clone()),
        None => ExchangeProducer::Unidentified,
    };
    let count = request.products.len();
    match api.publish_exchange(producer, item, request.products) {
        Ok(()) => {
            record(&format_args!(
                "{count} products now held for exchange from this writer"
            ));
            StatusCode::ACCEPTED.into_response()
        }
        // 507, not 500: the register is intact and this deployment is misconfigured. The
        // desktop's link keeps the batch and retries it, which shows as a backlog
        // (DN-18 §5) rather than as a set that quietly went missing.
        Err(e @ ApiError::NoRoom(_)) => refuse(StatusCode::INSUFFICIENT_STORAGE, &e.to_string()),
        Err(e) => refuse(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

/// The three exchange read routes, which differ only in the item (GAP-065).
///
/// **The agreement gate is answered before the products are read**, with a `403` naming
/// the item, exactly as `/v3/health` answers a party whose agreement does not send health.
/// It is the coarse half of DN-18 §5's two gates and it is about the caller rather than
/// about any one product, so a party with no agreement for an item learns that and not how
/// many of them there were. The marking gate is then applied per product by
/// [`NodeApi::exchange_for`], and what it removes is counted on the response.
///
/// An operator inside the deployment sees everything its role may, as on `/v3/snapshot`:
/// the two gates govern what leaves the deployment, and `picture.view` what its own
/// watch may read (GAP-111).
fn serve_exchange(
    api: &NodeApi,
    headers: &axum::http::HeaderMap,
    peer: &Peer,
    item: ExchangeItem,
) -> Response {
    let what = format!("the exchange's {item:?}");
    let held = match caller(api, headers, peer, &what) {
        Err(response) => return response,
        Ok(Caller::Operator(session)) => {
            if let Err(refused) = may_view_picture(api, peer, &session, &what) {
                return refused;
            }
            api.exchange_all(item)
        }
        Ok(Caller::Machine { party }) => {
            if !api
                .exchange
                .for_party(&party)
                .is_some_and(|a| a.sends(item))
            {
                let why = format!("the agreement with {party:?} does not send {item:?}");
                api.audit(
                    peer,
                    None,
                    events::ACCESS_REFUSED,
                    format_args!("{what}: {why}"),
                );
                return problem(StatusCode::FORBIDDEN, &why);
            }
            api.exchange_for(&party, item)
        }
    };
    match held {
        Some(held) => Json(held).into_response(),
        None => problem(
            StatusCode::INTERNAL_SERVER_ERROR,
            "the node's exchange register is unreadable",
        ),
    }
}

/// Why a decision request was refused before the loop saw it: the status, the item it
/// named if it named a readable one, and the sentence the caller and the audit entry both
/// get.
type DecisionRefusal = (StatusCode, Option<gungnir_model::PendingApprovalId>, String);

/// Checks 2 to 4 of DN-31 §6.3's table, and the decoding either side of them.
///
/// Separated from [`decide_queued`] so the list reads as a list. Every arm returns the
/// same three things, because every refusal here owes the caller an answer *and* the
/// node's record an entry saying the same words (§9 row 4).
///
/// # Errors
///
/// The refusal, which the caller turns into both.
fn vet_decision(
    api: &NodeApi,
    session: &OperatorSession,
    item: &Result<
        axum::extract::Path<gungnir_model::PendingApprovalId>,
        axum::extract::rejection::PathRejection,
    >,
    body: Result<Json<v3::DecisionRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<(gungnir_model::PendingApprovalId, v3::DecisionRequest), DecisionRefusal> {
    let Ok(axum::extract::Path(item)) = *item else {
        return Err((
            StatusCode::BAD_REQUEST,
            None,
            "the queue item in the path is neither a hyphenated identifier nor a decimal \
             number (D-60)"
                .into(),
        ));
    };
    let Ok(Json(request)) = body else {
        return Err((
            StatusCode::BAD_REQUEST,
            Some(item),
            "the decision could not be decoded".into(),
        ));
    };
    if request.item != item {
        return Err((
            StatusCode::BAD_REQUEST,
            Some(item),
            format!(
                "the decision names item {} and the path names {item}; a body meant for one \
                 item is not applied to another",
                request.item
            ),
        ));
    }
    // 2. The permission, and the stricter one for an override.
    let action = match request.choice {
        v3::DecisionChoice::Override => gungnir_security::actions::OVERRIDE_PLAN,
        v3::DecisionChoice::Accept | v3::DecisionChoice::Reject { .. } => {
            gungnir_security::actions::DECIDE_PLAN
        }
    };
    if !gungnir_security::authz::role_permits(session.role, action) {
        return Err((
            StatusCode::FORBIDDEN,
            Some(item),
            format!(
                "role {:?} may not take this decision ({action})",
                session.role
            ),
        ));
    }
    // 3. Whether this item is offered to that role. Answered from the queue the loop last
    // published: a role the item was never offered to is refused at the door rather than
    // occupying the loop, and an item the published queue does not hold goes to the loop,
    // which is the only place that can say whether it was decided, expired or never
    // issued.
    let role_name = format!("{:?}", session.role);
    if let Some(row) = api.queue().into_iter().find(|row| row.item == item) {
        if !row.offered_to.contains(&role_name) {
            return Err((
                StatusCode::FORBIDDEN,
                Some(item),
                format!(
                    "item {item} is offered to {:?} and not to {role_name}",
                    row.offered_to
                ),
            ));
        }
    }
    // 4. A rejection says why (DN-10 §3): MOE-01 tells a considered rejection from an
    // abandoned decision by the reason alone, so an empty one is refused rather than
    // recorded as a blank.
    if let v3::DecisionChoice::Reject { reason } = &request.choice {
        if reason.trim().is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                Some(item),
                "a rejection carries a reason; an empty one is refused rather than recorded \
                 as a blank (DN-10 §3)"
                    .into(),
            ));
        }
    }
    Ok((item, request))
}

/// How long the decision route waits for the node loop to take a decision.
///
/// The same bound the sensor-task route uses and for the same reason: the loop ticks far
/// faster, and the bound exists so a stalled loop answers rather than hangs. **A `504`
/// does not mean nothing was recorded** (DN-31 §6.3): the client retries with the same
/// request key and is told which.
const DECISION_REPLY_TIMEOUT: std::time::Duration = TASK_REPLY_TIMEOUT;

/// `GET /v3/queue` (DN-31 §7, GAP-132): what this node is waiting for a person to decide.
///
/// The queue the loop last published, in its own order -- time remaining, then priority
/// (DN-10 §5) -- so every desktop linked to this node shows the same queue in the same
/// order. Read authority, `picture.view`: seeing what is waiting is not deciding it, and
/// a role that may not decide still has to be able to see that somebody must.
async fn queue(
    State(api): State<Arc<NodeApi>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    headers: axum::http::HeaderMap,
) -> Response {
    let session = match operator_caller(&api, &headers, &peer, "the approval queue") {
        Ok(session) => session,
        Err(response) => return response,
    };
    if !gungnir_security::authz::role_permits(session.role, actions::VIEW_PICTURE) {
        let why = format!(
            "role {:?} may not read the queue ({})",
            session.role,
            actions::VIEW_PICTURE
        );
        api.audit(
            &peer,
            Some(session.operator),
            actions::VIEW_PICTURE,
            format_args!("refused: {why}"),
        );
        return problem(StatusCode::FORBIDDEN, &why);
    }
    Json(api.queue()).into_response()
}

/// `POST /v3/queue/{item}/decision` (DN-31 §6.3, GAP-132): a person decides one item.
///
/// **Four checks here, in this order, and then the loop.** Each is a question the route
/// can answer without the queue's state, and each refuses without recording, publishing
/// or engaging anything:
///
/// 1. a valid, unexpired operator token -- `401`, and a decision under an expired session
///    is not recorded at all (DN-23 §5 rule 2);
/// 2. `plan.decide`, or `plan.override` for an override -- `403` naming the role and the
///    action;
/// 3. the item is offered to the token's role -- `403` naming the roles it is offered to;
/// 4. a rejection carries a non-empty reason -- `400` (DN-10 §3).
///
/// Then the request goes to the node loop, which takes the requests in arrival order: an
/// item still pending is decided and answered `201`; one already decided is `409
/// AlreadyDecided` naming the decision that stands; an expired one is `409 Expired`; a
/// request key already recorded is answered with its first outcome and records nothing.
///
/// Every refusal above is queued for the loop to audit, because the audit log is the
/// loop's and DN-31 §9 row 4 wants exactly one entry per decision *and per refusal*.
///
/// Checks 2 to 4 and the decoding are [`vet_decision`], so that what this function shows
/// is the shape of the route -- authenticate, vet, hand over, wait -- and the checks are
/// read as one list in the order DN-31 §6.3's table gives them.
async fn decide_queued(
    State(api): State<Arc<NodeApi>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    item: Result<
        axum::extract::Path<gungnir_model::PendingApprovalId>,
        axum::extract::rejection::PathRejection,
    >,
    headers: axum::http::HeaderMap,
    body: Result<Json<v3::DecisionRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    // 1. Who is asking. An unauthenticated caller is refused before anything about this
    // node's queue is said, and nothing about the attempt reaches the record under a name
    // nobody verified -- the entry says `unauthenticated`, which is what it was.
    // Unaudited here: the refusal below is the loop's to record, under `plan.decide`.
    let session = match unaudited_operator_caller(&api, &headers, &peer, "the decision route") {
        Ok(session) => session,
        Err((status, why)) => {
            api.refuse_decision(RefusedDecision {
                item: None,
                operator: "unauthenticated".into(),
                party: peer.party.clone(),
                reason: "no valid operator session".into(),
            });
            return problem(status, &why);
        }
    };
    // The refusal path records who was refused as text, because "unauthenticated" above is
    // one of its values and no identifier stands for it. The decision path below carries
    // the identifier itself; see `PendingDecision::operator`.
    let refused_as = session.operator.0.to_string();
    let (item, request) = match vet_decision(&api, &session, &item, body) {
        Ok(vetted) => vetted,
        Err((status, item, why)) => {
            api.refuse_decision(RefusedDecision {
                item,
                operator: refused_as,
                party: peer.party.clone(),
                reason: why.clone(),
            });
            return problem(status, &why);
        }
    };
    let (reply, answer) = tokio::sync::oneshot::channel();
    {
        let Ok(mut queue) = api.decisions.lock() else {
            return problem(
                StatusCode::INTERNAL_SERVER_ERROR,
                "the decision queue lock was poisoned",
            );
        };
        queue.push(PendingDecision {
            item,
            choice: request.choice,
            request: request.request,
            operator: session.operator,
            role: session.role,
            party: peer.party.clone(),
            reply,
        });
    }
    match tokio::time::timeout(DECISION_REPLY_TIMEOUT, answer).await {
        Ok(Ok(DecisionAnswer::Recorded(decision))) => {
            (StatusCode::CREATED, Json(v3::DecisionRecorded { decision })).into_response()
        }
        Ok(Ok(DecisionAnswer::Refused(refused))) => {
            (StatusCode::CONFLICT, Json(refused)).into_response()
        }
        Ok(Ok(DecisionAnswer::Unknown)) => problem(
            StatusCode::NOT_FOUND,
            &format!("this node has no queue item {item}, decided or waiting"),
        ),
        Ok(Err(_)) => problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "the node loop dropped the decision without answering",
        ),
        // **Not "nothing was recorded"** (DN-31 §6.3): the loop may have taken it. The
        // client retries with the same request key and is told which.
        Err(_) => problem(
            StatusCode::GATEWAY_TIMEOUT,
            "the node loop did not decide within the reply window; retry with the same \
             request key to learn whether it was recorded",
        ),
    }
}

/// `POST /v3/decisions/forwarded` (DN-31 §6.8 and §7, GAP-134): what a desktop decided
/// while it was cut off from this node, put on the node's record once.
///
/// **The same shape as the decision route -- authenticate, vet, hand over, wait -- and
/// the same reason**: the node's record belongs to the loop. The checks here are about the
/// caller and about whether a record could be a record at all:
///
/// 1. a valid, unexpired operator token -- `401`;
/// 2. `plan.decide` -- `403`: forwarding a decision is putting one on this node's record;
/// 3. the body decodes, names an origin, and every record is one a queue could have
///    produced -- queued under `RequiresHumanApproval`, and a rejection with its reason
///    -- or `400`. A record no queue could have produced is not refused *as a
///    contradiction*: it is not a decision.
///
/// Then the loop takes the batch whole: `202 ForwardAccepted` counting what was new and
/// what was already held, or `409 ForwardRefused` naming the record that stands, with
/// nothing applied. A `504` means the loop did not answer in time **and does not mean
/// nothing was recorded**, exactly as on the decision route: the client sends the same
/// batch again and learns which, and a batch already on the record answers with everything
/// `already_held`.
///
/// Every refusal here is queued for the loop to audit, as the decision route's are.
async fn forward_decisions(
    State(api): State<Arc<NodeApi>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    headers: axum::http::HeaderMap,
    body: Result<Json<Vec<v3::ForwardedDecision>>, axum::extract::rejection::JsonRejection>,
) -> Response {
    // Unaudited here, as on the decision route: the loop records the refusal.
    let session = match unaudited_operator_caller(&api, &headers, &peer, "forwarding decisions") {
        Ok(session) => session,
        Err((status, why)) => {
            api.refuse_decision(RefusedDecision {
                item: None,
                operator: "unauthenticated".into(),
                party: peer.party.clone(),
                reason: "forwarding decisions: no valid operator session".into(),
            });
            return problem(status, &why);
        }
    };
    let refused_as = session.operator.0.to_string();
    let refuse = |status: StatusCode, why: String| {
        api.refuse_decision(RefusedDecision {
            item: None,
            operator: refused_as.clone(),
            party: peer.party.clone(),
            reason: format!("forwarding decisions: {why}"),
        });
        problem(status, &why)
    };
    if !gungnir_security::authz::role_permits(session.role, gungnir_security::actions::DECIDE_PLAN)
    {
        return refuse(
            StatusCode::FORBIDDEN,
            format!(
                "role {:?} may not put decisions on this node's record ({})",
                session.role,
                gungnir_security::actions::DECIDE_PLAN
            ),
        );
    }
    let decisions = match body {
        Ok(Json(decisions)) => decisions,
        Err(err) => {
            return refuse(
                StatusCode::BAD_REQUEST,
                format!("the forwarded decisions could not be decoded: {err}"),
            )
        }
    };
    if let Err(why) = vet_forwarded(&decisions) {
        return refuse(StatusCode::BAD_REQUEST, why);
    }
    // **The origin is the machine the handshake verified, where there was one** (GAP-141).
    // A desktop's name is a fingerprint of the key its certificate carries, so this
    // compares two views of one key rather than trusting a string in the body. A
    // plaintext caller has no party and nothing to compare: the node has no claim about
    // which machine it is talking to either way, and says so by not pretending to check.
    if let Some(party) = peer.party.as_deref() {
        if let Some(other) = decisions
            .iter()
            .map(|d| d.origin.as_str())
            .find(|origin| *origin != party)
        {
            return refuse(
                StatusCode::FORBIDDEN,
                format!(
                    "a forwarded decision names origin {other:?} and this connection was \
                     verified as {party:?}; a batch is forwarded by the machine that took it"
                ),
            );
        }
    }
    let (reply, answer) = tokio::sync::oneshot::channel();
    {
        let Ok(mut queue) = api.forwarded.lock() else {
            return problem(
                StatusCode::INTERNAL_SERVER_ERROR,
                "the forwarding queue lock was poisoned",
            );
        };
        queue.push(PendingForward {
            decisions,
            operator: session.operator,
            role: session.role,
            party: peer.party.clone(),
            reply,
        });
    }
    match tokio::time::timeout(DECISION_REPLY_TIMEOUT, answer).await {
        Ok(Ok(ForwardAnswer::Accepted(accepted))) => {
            (StatusCode::ACCEPTED, Json(accepted)).into_response()
        }
        Ok(Ok(ForwardAnswer::Refused(refused))) => {
            (StatusCode::CONFLICT, Json(refused)).into_response()
        }
        Ok(Err(_)) => problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "the node loop dropped the forwarded decisions without answering",
        ),
        Err(_) => problem(
            StatusCode::GATEWAY_TIMEOUT,
            "the node loop did not answer within the reply window; send the same batch \
             again to learn whether it was recorded",
        ),
    }
}

/// Check 3 of [`forward_decisions`]: every element is a record a queue could have
/// produced.
///
/// # Errors
///
/// The sentence the caller and the audit entry both get.
fn vet_forwarded(decisions: &[v3::ForwardedDecision]) -> Result<(), String> {
    for forwarded in decisions {
        let record = &forwarded.record;
        if forwarded.origin.trim().is_empty() {
            return Err(format!(
                "decision {} names no origin; a forwarded decision says which machine took \
                 it (DN-31 §5.2)",
                record.decision
            ));
        }
        if record.verdict != gungnir_model::events::VerdictSummary::RequiresHumanApproval {
            return Err(format!(
                "decision {} was taken on a plan whose verdict was {:?}; only a plan that \
                 requires a person's approval is ever queued, so this is not a decision",
                record.decision, record.verdict
            ));
        }
        if let v3::DecisionChoice::Reject { reason } = &record.choice {
            if reason.trim().is_empty() {
                return Err(format!(
                    "decision {} is a rejection with no reason (DN-10 §3)",
                    record.decision
                ));
            }
        }
    }
    Ok(())
}

fn problem(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(serde_json::json!({ "error": status.as_u16(), "message": message })),
    )
        .into_response()
}

/// The event stream. Its token arrives in the subscribe frame rather than a header,
/// because a WebSocket client cannot always set one on the upgrade.
async fn events(
    ws: WebSocketUpgrade,
    State(api): State<Arc<NodeApi>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
) -> Response {
    ws.on_upgrade(move |socket| stream_events(socket, api, peer))
}

/// Who the event stream is for: `None` for an operator, the party for a machine with an
/// agreement, or the sentence the socket is told before it closes.
///
/// Every refusal is on the audit record (GAP-111), as on the other routes, and an operator
/// is asked `picture.view`: the stream is the picture as it changes, so it asks what the
/// snapshot asks (D-30).
fn stream_caller(api: &NodeApi, peer: &Peer, token: &str) -> Result<Option<String>, String> {
    let refused = |why: String| {
        api.audit(
            peer,
            None,
            events::ACCESS_REFUSED,
            format_args!("the event stream: {why}"),
        );
        why
    };
    if token.is_empty() {
        return match &peer.party {
            Some(party) if api.exchange.for_party(party).is_some() => Ok(Some(party.clone())),
            Some(party) => Err(refused(format!(
                "party {party:?} has no exchange agreement"
            ))),
            None => Err(refused("no token and no party".into())),
        };
    }
    let session = match api.callers.as_ref() {
        None => Err("this node authenticates nobody".to_owned()),
        Some(callers) => callers
            .verify(token, api.now())
            .map_err(|failure| failure.to_string()),
    }
    .map_err(refused)?;
    if gungnir_security::authz::role_permits(session.role, actions::VIEW_PICTURE) {
        return Ok(None);
    }
    let why = format!(
        "role {:?} may not read the event stream ({})",
        session.role,
        actions::VIEW_PICTURE
    );
    api.audit(
        peer,
        Some(session.operator),
        actions::VIEW_PICTURE,
        format_args!("refused: {why}"),
    );
    Err(why)
}

/// The event stream: read the subscribe frame, send what was missed, then follow live.
///
/// Subscribing **before** reading the backlog is deliberate. The other order has a hole:
/// an envelope published between reading the backlog and subscribing would be in neither,
/// and the client would have a silent gap in a stream whose whole purpose is that gaps
/// are detectable.
async fn stream_events(mut socket: WebSocket, api: Arc<NodeApi>, peer: Peer) {
    let mut live = api.subscribe();

    let Some(request) = read_subscribe(&mut socket).await else {
        let _ = socket
            .send(Message::Text(
                "expected a SubscribeRequest as the first frame".into(),
            ))
            .await;
        return;
    };

    // The stream is a read path like any other: an operator's token, or a machine's
    // party with an agreement (GAP-062), and a party's stream is filtered like its
    // snapshot.
    let party = match stream_caller(&api, &peer, &request.token) {
        Ok(party) => party,
        Err(why) => {
            let _ = socket.send(Message::Text(why.into())).await;
            return;
        }
    };
    let releases = |envelope: &Envelope| match &party {
        None => true,
        Some(party) => api.releases(party, envelope),
    };

    let Some(backlog) = api.offered_since(request.from_seq) else {
        // The contract's own rule: too far back, so say so and let the client take a
        // fresh snapshot rather than handing it a stream with a hole in it.
        let _ = socket
            .send(Message::Text(
                format!(
                    "seq {} is older than this node retains; take a fresh snapshot",
                    request.from_seq
                )
                .into(),
            ))
            .await;
        return;
    };

    let mut sent_through = 0;
    for offered in backlog {
        sent_through = sent_through.max(offered.envelope.seq);
        if !releases(&offered.envelope) {
            continue;
        }
        if send_offered(&mut socket, &offered).await.is_err() {
            return;
        }
    }

    let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
    // The first tick fires immediately; skip it so a client is not pinged before it has
    // had a chance to read the backlog.
    heartbeat.tick().await;

    loop {
        tokio::select! {
            // A ping on an idle stream. `send` failing is how a closed connection is
            // noticed on a node that has published nothing for a while.
            _ = heartbeat.tick() => {
                if socket.send(Message::Ping(Vec::new().into())).await.is_err() {
                    return;
                }
            }
            received = live.recv() => match received {
                // Already delivered from the backlog. Sending it twice would break the
                // contract's "in `seq` order" guarantee for a client that keys on it.
                Ok(offered) if offered.envelope.seq <= sent_through => {}
                Ok(offered) if !releases(&offered.envelope) => {}
                Ok(offered) => {
                    if send_offered(&mut socket, &offered).await.is_err() {
                        return;
                    }
                }
                // The client fell behind further than the window. It is told, rather
                // than silently skipping ahead, for the same reason the backlog case is.
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    let _ = socket
                        .send(Message::Text(
                            format!("missed {n} envelopes; take a fresh snapshot").into(),
                        ))
                        .await;
                    return;
                }
                Err(broadcast::error::RecvError::Closed) => return,
            },
        }
    }
}

async fn read_subscribe(socket: &mut WebSocket) -> Option<v3::SubscribeRequest> {
    loop {
        match socket.recv().await? {
            Ok(Message::Text(text)) => return serde_json::from_str(&text).ok(),
            // Ping/pong and empty frames are the transport's business, not the
            // contract's; keep waiting for the frame the contract asks for.
            Ok(Message::Ping(_) | Message::Pong(_)) => {}
            Ok(_) | Err(_) => return None,
        }
    }
}

/// Send one envelope as the line it was offered as (GAP-153, D-96): encoded once, in the
/// lossless form, when [`NodeApi::publish_event`] took it.
async fn send_offered(socket: &mut WebSocket, offered: &Offered) -> Result<(), ()> {
    socket
        .send(Message::Text(offered.line.as_str().into()))
        .await
        .map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::MissionTime;

    #[allow(clippy::cast_precision_loss)]
    fn envelope(seq: u64) -> Envelope {
        Envelope {
            seq,
            mission_time: MissionTime(seq as f64),
            event: gungnir_eventing::Event::Sensor(
                gungnir_model::events::SensorEvent::ModeChanged {
                    sensor: gungnir_model::SensorId(1),
                    from: gungnir_model::SensorMode::Standby,
                    to: gungnir_model::SensorMode::Search,
                    at: MissionTime(seq as f64),
                },
            ),
        }
    }

    fn api() -> NodeApi {
        NodeApi::new(v3::SnapshotResponse::new(
            Vec::new(),
            None,
            SystemHealth::default(),
            Vec::new(),
        ))
    }

    /// `from_seq` 0 means "everything from now", which is an empty backlog rather than
    /// the whole retained window. A client that asked for nothing must not be handed
    /// history it never asked for.
    #[test]
    fn from_seq_zero_replays_nothing() {
        let api = api();
        for seq in 1..=5 {
            api.publish_event(envelope(seq)).expect("published");
        }
        assert_eq!(api.backlog_since(0), Some(Vec::new()));
    }

    #[test]
    fn a_backlog_starts_at_the_sequence_asked_for() {
        let api = api();
        for seq in 1..=5 {
            api.publish_event(envelope(seq)).expect("published");
        }
        let seqs: Vec<u64> = api
            .backlog_since(3)
            .expect("3 is still retained")
            .iter()
            .map(|e| e.seq)
            .collect();
        assert_eq!(seqs, vec![3, 4, 5]);
    }

    /// The contract's rule, and the one worth protecting: a client asking for further
    /// back than the window reaches is told, rather than handed a stream with a hole.
    #[test]
    fn a_sequence_older_than_the_window_is_refused_rather_than_truncated() {
        let api = api();
        for seq in 1..=(BACKLOG_CAPACITY as u64 + 10) {
            api.publish_event(envelope(seq)).expect("published");
        }
        assert_eq!(
            api.backlog_since(1),
            None,
            "a client was given a truncated stream as though it were complete"
        );
        // The newest end of the window still works, so the refusal is about the window
        // and not about everything.
        let recent = BACKLOG_CAPACITY as u64 + 5;
        assert!(api.backlog_since(recent).is_some());
    }

    /// A node with nothing published yet has missed nothing, which is different from
    /// having lost what it never had.
    #[test]
    fn an_empty_node_replays_nothing_rather_than_refusing() {
        assert_eq!(api().backlog_since(42), Some(Vec::new()));
    }

    /// The client's patience must exceed the server's pace, or a healthy idle link
    /// would be torn down between heartbeats. Checked rather than assumed because the
    /// two constants are edited independently and the failure is intermittent.
    #[test]
    fn the_heartbeat_timeout_allows_several_missed_pings() {
        assert!(
            HEARTBEAT_TIMEOUT >= HEARTBEAT_INTERVAL * 3,
            "a single slow heartbeat would drop a healthy link"
        );
        // D-23: the timeout is derived from the beat, so this is the relationship and not
        // a coincidence of two numbers.
        assert_eq!(
            HEARTBEAT_TIMEOUT,
            HEARTBEAT_INTERVAL * HEARTBEAT_MISSES_TOLERATED + HEARTBEAT_MARGIN
        );
        assert_eq!(HEARTBEAT_TIMEOUT.as_secs(), 7);
    }

    fn product(id: &str, releasability: gungnir_model::Releasability) -> v3::ExchangeProduct {
        v3::ExchangeProduct {
            id: id.into(),
            at: MissionTime(1.0),
            releasability,
            body: serde_json::Value::Null,
        }
    }

    /// GAP-065: an item nothing has published is `NotHeld` with a reason, and publishing
    /// an empty list is a different answer -- "we hold none right now" rather than "we do
    /// not produce these". A bare empty list would collapse the two.
    #[test]
    fn an_unpublished_item_is_not_reported_as_an_empty_one() {
        let api = api();
        assert!(matches!(
            api.exchange_all(ExchangeItem::Reports),
            Some(v3::ExchangeResponse::NotHeld { .. })
        ));
        api.withhold_exchange(
            ExchangeProducer::Node,
            ExchangeItem::Reports,
            "this node produces no reports",
        )
        .expect("withheld");
        match api.exchange_all(ExchangeItem::Reports) {
            Some(v3::ExchangeResponse::NotHeld { item, reason, .. }) => {
                assert_eq!(item, ExchangeItem::Reports);
                assert_eq!(reason, "this node produces no reports");
            }
            other => panic!("expected a reason, got {other:?}"),
        }
        api.publish_exchange(ExchangeProducer::Node, ExchangeItem::Reports, Vec::new())
            .expect("published");
        assert!(matches!(
            api.exchange_all(ExchangeItem::Reports),
            Some(v3::ExchangeResponse::Held { withheld: 0, .. })
        ));
    }

    /// Every handoff id the register holds for `Handoffs`, merged, in producer order.
    fn held_ids(api: &NodeApi) -> Vec<String> {
        match api.exchange_all(ExchangeItem::Handoffs) {
            Some(v3::ExchangeResponse::Held { products, .. }) => {
                products.into_iter().map(|p| p.id).collect()
            }
            other => panic!("expected a held set, got {other:?}"),
        }
    }

    /// GAP-137, DN-18 §5 amendment 3: one item, two producers. A publish replaces the
    /// set of the producer that wrote it and leaves every other where it was, and a read
    /// merges them with this node's own set first.
    ///
    /// **This is the whole reason the key exists.** Before it, the node's own handoffs and
    /// a desktop's published set each overwrote the other, and which one a partner saw
    /// depended on which wrote last.
    #[test]
    fn a_publish_replaces_one_producer_s_set_and_leaves_the_others() {
        use gungnir_model::Releasability;
        let api = api();
        let desk = || ExchangeProducer::Party("desktop-aaaa".into());
        api.publish_exchange(
            ExchangeProducer::Node,
            ExchangeItem::Handoffs,
            vec![product("node-1", Releasability::AllPeers)],
        )
        .expect("the node published");
        api.publish_exchange(
            desk(),
            ExchangeItem::Handoffs,
            vec![product("desk-1", Releasability::AllPeers)],
        )
        .expect("the desktop published");
        assert_eq!(
            held_ids(&api),
            vec!["node-1", "desk-1"],
            "a second writer replaced the first instead of joining it"
        );

        // The desktop republishes its whole set, as its host does on every issue.
        api.publish_exchange(
            desk(),
            ExchangeItem::Handoffs,
            vec![
                product("desk-1", Releasability::AllPeers),
                product("desk-2", Releasability::AllPeers),
            ],
        )
        .expect("the desktop republished");
        assert_eq!(
            held_ids(&api),
            vec!["node-1", "desk-1", "desk-2"],
            "the node's own set did not survive a desktop's republish"
        );

        // And the node's own, which is what `republish_handoffs` does each time the desk
        // issues one: a replacement, so the desktop's set is untouched.
        api.publish_exchange(
            ExchangeProducer::Node,
            ExchangeItem::Handoffs,
            vec![product("node-2", Releasability::AllPeers)],
        )
        .expect("the node republished");
        assert_eq!(
            held_ids(&api),
            vec!["node-2", "desk-1", "desk-2"],
            "the node replaced more than its own set"
        );
    }

    /// GAP-145: the answer says when its **least recently refreshed** producer wrote,
    /// because a merged set is only as current as its quietest contributor and a partner
    /// cannot see the producers.
    ///
    /// Nothing expires: a handoff is a decision that was taken, so the age travels and
    /// the products stay. What moves the age is a producer writing again -- which a
    /// desktop does whenever its link comes back (GAP-145).
    #[test]
    fn the_answer_says_when_its_quietest_producer_last_wrote() {
        use gungnir_model::Releasability;
        let api = api();
        let desk = || ExchangeProducer::Party("desktop-aaaa".into());
        api.set_now(10.0);
        api.publish_exchange(
            ExchangeProducer::Node,
            ExchangeItem::Handoffs,
            vec![product("node-1", Releasability::AllPeers)],
        )
        .expect("the node published");
        api.set_now(20.0);
        api.publish_exchange(
            desk(),
            ExchangeItem::Handoffs,
            vec![product("desk-1", Releasability::AllPeers)],
        )
        .expect("the desktop published");
        assert_eq!(
            as_of(&api),
            Some(gungnir_model::MissionTime(10.0)),
            "the age is the oldest producer's write, not the newest"
        );

        // The node republishes, so the desktop is now the quietest.
        api.set_now(30.0);
        api.publish_exchange(
            ExchangeProducer::Node,
            ExchangeItem::Handoffs,
            vec![product("node-2", Releasability::AllPeers)],
        )
        .expect("the node republished");
        assert_eq!(
            as_of(&api),
            Some(gungnir_model::MissionTime(20.0)),
            "a republish did not move the age off the producer that wrote it"
        );
        assert_eq!(
            held_ids(&api),
            vec!["node-2", "desk-1"],
            "the quiet producer's products were dropped for their age"
        );

        // A producer that holds none is as old as its claim: the age is about the answer,
        // not about the products in it.
        api.set_now(40.0);
        api.withhold_exchange(
            ExchangeProducer::Node,
            ExchangeItem::Reports,
            "this node publishes no reports",
        )
        .expect("withheld");
        match api.exchange_all(ExchangeItem::Reports) {
            Some(v3::ExchangeResponse::NotHeld { as_of, .. }) => {
                assert_eq!(as_of, Some(gungnir_model::MissionTime(40.0)));
            }
            other => panic!("expected a reason, got {other:?}"),
        }

        // And an item nothing has ever been published for has no age at all.
        match api.exchange_all(ExchangeItem::Warnings) {
            Some(v3::ExchangeResponse::NotHeld { as_of: None, .. }) => {}
            other => panic!("an unpublished item was given an age: {other:?}"),
        }
    }

    /// When the least recently refreshed part of the handoff answer was written.
    fn as_of(api: &NodeApi) -> Option<gungnir_model::MissionTime> {
        match api.exchange_all(ExchangeItem::Handoffs) {
            Some(v3::ExchangeResponse::Held { as_of, .. }) => as_of,
            other => panic!("expected a held set, got {other:?}"),
        }
    }

    /// GAP-137: when every producer withholds, the answer is `NotHeld` and carries what
    /// each of them said. One producer holding anything makes the answer `Held`, because
    /// the merged set is what the deployment holds.
    #[test]
    fn every_producer_s_reason_travels_when_none_holds_any() {
        use gungnir_model::Releasability;
        let api = api();
        api.withhold_exchange(
            ExchangeProducer::Node,
            ExchangeItem::Reports,
            "this node publishes no reports",
        )
        .expect("the node withheld");
        api.withhold_exchange(
            ExchangeProducer::Party("desktop-aaaa".into()),
            ExchangeItem::Reports,
            "this desktop has generated none",
        )
        .expect("the desktop withheld");
        match api.exchange_all(ExchangeItem::Reports) {
            Some(v3::ExchangeResponse::NotHeld { reason, .. }) => assert_eq!(
                reason, "this node publishes no reports; this desktop has generated none",
                "a partner was told one producer's reason and not the other's"
            ),
            other => panic!("expected a reason, got {other:?}"),
        }
        api.publish_exchange(
            ExchangeProducer::Party("desktop-bbbb".into()),
            ExchangeItem::Reports,
            vec![product("report-1", Releasability::AllPeers)],
        )
        .expect("a third producer published");
        assert!(
            matches!(
                api.exchange_all(ExchangeItem::Reports),
                Some(v3::ExchangeResponse::Held { .. })
            ),
            "one producer holding a product must not be hidden by two that hold none"
        );
    }

    /// GAP-137: the register is bounded. A producer already in it always writes; a new one
    /// beyond the bound is refused, and nothing already held changes -- the batch stays on
    /// the desktop's link as a backlog rather than evicting a set a partner is served from.
    #[test]
    fn a_new_producer_beyond_the_bound_is_refused_and_changes_nothing() {
        use gungnir_model::Releasability;
        let api = api();
        for n in 0..PRODUCERS_PER_ITEM {
            api.publish_exchange(
                ExchangeProducer::Party(format!("desktop-{n:04}")),
                ExchangeItem::Handoffs,
                vec![product(&format!("handoff-{n}"), Releasability::AllPeers)],
            )
            .expect("within the bound");
        }
        let refused = api
            .publish_exchange(
                ExchangeProducer::Party("desktop-one-too-many".into()),
                ExchangeItem::Handoffs,
                vec![product("unheard", Releasability::AllPeers)],
            )
            .expect_err("the bound admitted one more");
        assert!(
            matches!(refused, ApiError::NoRoom(_)),
            "refused for the wrong reason: {refused}"
        );
        let ids = held_ids(&api);
        assert_eq!(ids.len(), PRODUCERS_PER_ITEM, "a set was evicted");
        assert!(!ids.iter().any(|id| id == "unheard"));

        // A producer already in the register is not refused, bound or no bound.
        api.publish_exchange(
            ExchangeProducer::Party("desktop-0000".into()),
            ExchangeItem::Handoffs,
            vec![product("handoff-0-again", Releasability::AllPeers)],
        )
        .expect("a known producer is never refused");
        assert!(held_ids(&api).iter().any(|id| id == "handoff-0-again"));
    }

    /// DN-18 §5's two gates, and the count. The marking gate is checked with an agreement
    /// that permits the item, so a shortcut checking only the agreement would pass every
    /// product through and fail here.
    #[test]
    fn a_party_receives_only_what_both_gates_permit_and_is_told_how_many_it_did_not() {
        use gungnir_model::{ExchangeAgreement, ExchangeFormat, Releasability};
        let mut api = api();
        api.exchange = ExchangeSet {
            agreements: vec![ExchangeAgreement {
                party: "sector-north".into(),
                inbound: Vec::new(),
                outbound: vec![ExchangeItem::Warnings],
                format: ExchangeFormat::Canonical,
            }],
        };
        api.publish_exchange(
            ExchangeProducer::Node,
            ExchangeItem::Warnings,
            vec![
                product("a", Releasability::Internal),
                product("b", Releasability::parties(["sector-north"])),
                product("c", Releasability::AllPeers),
                product("d", Releasability::parties(["sector-south"])),
            ],
        )
        .expect("published");

        match api.exchange_for("sector-north", ExchangeItem::Warnings) {
            Some(v3::ExchangeResponse::Held {
                products, withheld, ..
            }) => {
                let ids: Vec<String> = products.into_iter().map(|p| p.id).collect();
                assert_eq!(ids, vec!["b".to_string(), "c".to_string()]);
                assert_eq!(withheld, 2);
            }
            other => panic!("expected products, got {other:?}"),
        }

        // The agreement gate alone: a party it does not name receives nothing, however
        // permissive the markings are, and the count is every product there was.
        match api.exchange_for("sector-south", ExchangeItem::Warnings) {
            Some(v3::ExchangeResponse::Held {
                products, withheld, ..
            }) => {
                assert!(products.is_empty(), "{products:?}");
                assert_eq!(withheld, 4);
            }
            other => panic!("expected an empty holding, got {other:?}"),
        }

        // An operator inside the deployment sees all four: the gates govern what leaves.
        match api.exchange_all(ExchangeItem::Warnings) {
            Some(v3::ExchangeResponse::Held {
                products, withheld, ..
            }) => {
                assert_eq!(products.len(), 4);
                assert_eq!(withheld, 0);
            }
            other => panic!("expected products, got {other:?}"),
        }
    }

    /// GAP-042: the acknowledgement queue is drained by the node loop, once. A second
    /// drain that returned the same record would put the fact on the record twice, and a
    /// desktop would try to discharge an already-discharged warning.
    #[test]
    fn a_warning_acknowledgement_is_taken_once() {
        let api = api();
        assert!(api.take_warning_acknowledgements().is_empty());
        api.acknowledgements
            .lock()
            .expect("not poisoned")
            .push(WarningAcknowledgement {
                asset: AssetId(1),
                track: TrackId(7),
                party: "port-authority".into(),
                at: MissionTime(4.0),
            });
        assert_eq!(api.take_warning_acknowledgements().len(), 1);
        assert!(api.take_warning_acknowledgements().is_empty());
    }

    /// The window is bounded, so a long-running node cannot grow it without limit.
    #[test]
    fn the_window_is_bounded() {
        let api = api();
        for seq in 1..=(BACKLOG_CAPACITY as u64 * 2) {
            api.publish_event(envelope(seq)).expect("published");
        }
        let backlog = api.backlog.lock().expect("not poisoned");
        assert_eq!(backlog.len(), BACKLOG_CAPACITY);
    }
}
