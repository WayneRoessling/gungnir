// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The v2 transport: JSON over HTTP and a WebSocket event stream (GAP-041).
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
//! **Every route but `POST /v2/session` requires a session token** (DN-23 §6, GAP-057).
//! The token is minted by the node against its own account store and verified per
//! request; a caller who presents none, or a bad one, gets `401`. `ApprovalRequest`
//! carries an operator in its body and that field is **not** believed: the caller is
//! whoever the token says, and nobody else.
//!
//! A node with no caller authority configured refuses every route but the session one,
//! and says so. That is a deployment with no account store, which is the default: it
//! runs its pipeline, journals it, and serves nobody.
//!
//! **`POST /v2/detections` is served; `POST /v2/plans/{plan_id}/decision` is not**, and
//! for a different reason than before. A submitted detection is queued for the ingest
//! gateway, which authenticates and validates it exactly as it does a sensor's -- see
//! [`NodeApi::submit_detection`]. A plan decision has nothing to decide against: **this
//! node runs no approval queue.** The desktop routes plans through the policy chain and
//! the queue (GAP-038); a node publishes `PlanProposed` and stops. Serving the route
//! would mean inventing a queue here, so it returns `501` naming that, which is now the
//! true reason rather than the authentication one.
//!
//! Routing a refused endpoint rather than leaving it absent is deliberate: a `404` would
//! tell a client the endpoint is not part of v2, which is false.
//!
//! # What the outside world may say back, and what it may be sent
//!
//! Two routes exist so an outside party can answer something this deployment sent it:
//! `POST /v2/handoffs/{decision_id}/report` (GAP-040) and
//! `POST /v2/warnings/{asset_id}/{track_id}/acknowledge` (GAP-042). Both take a machine
//! whose certificate speaks for the right thing or an operator holding the matching
//! action, and both **queue rather than apply**: the node holds neither a handoff nor a
//! warning ledger, so it puts the fact on the record and the desktop that issued the one
//! or raised the other applies it.
//!
//! `GET /v2/exchange/{warnings,reports,handoffs}` (GAP-065) are DN-18's three remaining
//! items, gated by the agreement and the marking together with the restrictive one
//! deciding, and reporting what they withheld. Tracks and health keep their existing
//! doors, `/v2/snapshot` and `/v2/health`.
//!
//! **`POST` on those same three paths (GAP-065, DN-18 §5 amendment 2, human-owned,
//! signed by the owner the same day) is the write path DN-18's own amendment 1 said
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
//! # Loopback in the clear, or anywhere with mutual TLS
//!
//! [`serve`] and [`bind`] refuse any address that is not loopback, because a
//! command-and-control surface accepting plaintext connections from the network would be
//! worse than one that does not start.
//!
//! **A node serving mutual TLS is not bound by that** (GAP-060): see [`crate::tls`],
//! which builds the acceptor, and `serve_on_listener`, which serves any listener. The
//! restriction is on plaintext, not on the address.

use crate::tls::{Peer, PlainListener, TlsListener};
use crate::{v2, ApiError};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use gungnir_eventing::Envelope;
use gungnir_model::SystemHealth;
use gungnir_model::{
    AssetId, DecisionId, ExchangeItem, ExchangeSet, MissionTime, SensorId, SensorTaskId, TrackId,
};
use gungnir_security::{AuthFailure, MissionTimeSeconds, OperatorSession};
use std::collections::{BTreeMap, VecDeque};
use std::net::SocketAddr;
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

/// What a node publishes and what the routes read.
///
/// The tick loop owns every service, so the transport never touches one. Each tick the
/// loop publishes a fresh snapshot and forwards the bus's envelopes here; the routes read
/// only this. That keeps a single owner for mutable state and means a slow client cannot
/// stall the loop.
pub struct NodeApi {
    snapshot: RwLock<v2::SnapshotResponse>,
    /// The coverage answer this node last computed (GAP-006).
    ///
    /// Published by the tick like the snapshot, because computing it in a request handler
    /// would put a sampling loop on the request path and let a caller's polling rate
    /// decide the node's load.
    coverage: RwLock<v2::CoverageResponse>,
    events: broadcast::Sender<Envelope>,
    backlog: Mutex<VecDeque<Envelope>>,
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
    /// Effector reports waiting for the node loop to put on the record (GAP-040).
    reports: Mutex<Vec<EffectorReportRecord>>,
    /// Warning acknowledgements waiting for the node loop to put on the record
    /// (GAP-042). A separate queue from `reports` for the same reason `reports` is
    /// separate from `submissions`: the two are different facts, arriving under
    /// different authority, and one queue would make the drain guess which.
    acknowledgements: Mutex<Vec<WarningAcknowledgement>>,
    /// What this deployment holds for exchange, per item (DN-18 §5, GAP-065). Absent
    /// means nothing has been published for that item, which is answered as
    /// [`v2::ExchangeResponse::NotHeld`] and never as an empty list.
    exchange_products: RwLock<BTreeMap<ExchangeItem, v2::ExchangeResponse>>,
}

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
    pub fn new(snapshot: v2::SnapshotResponse) -> Self {
        let (events, _) = broadcast::channel(BACKLOG_CAPACITY);
        Self {
            snapshot: RwLock::new(snapshot),
            coverage: RwLock::new(v2::CoverageResponse::NotComputed {
                reason: "this node has not computed a coverage answer yet".into(),
            }),
            events,
            backlog: Mutex::new(VecDeque::with_capacity(BACKLOG_CAPACITY)),
            now: RwLock::new(0.0),
            callers: None,
            submissions: Mutex::new(Vec::new()),
            exchange: ExchangeSet::default(),
            identities: std::collections::HashMap::new(),
            machine_submissions: Mutex::new(Vec::new()),
            tasks: Mutex::new(Vec::new()),
            reports: Mutex::new(Vec::new()),
            acknowledgements: Mutex::new(Vec::new()),
            exchange_products: RwLock::new(BTreeMap::new()),
        }
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

    /// Publish what this deployment holds for `item`, for the exchange routes (DN-18 §5,
    /// GAP-065). Called by whoever owns the products, once they are marked.
    ///
    /// Publishing an empty list is a claim -- "we hold none of this right now" -- and is
    /// different from never having published, which is [`NodeApi::withhold_exchange`].
    pub fn publish_exchange(
        &self,
        item: ExchangeItem,
        products: Vec<v2::ExchangeProduct>,
    ) -> Result<(), ApiError> {
        let mut slot = self
            .exchange_products
            .write()
            .map_err(|_| ApiError::Transport("the exchange lock was poisoned".into()))?;
        slot.insert(
            item,
            v2::ExchangeResponse::Held {
                item,
                products,
                withheld: 0,
            },
        );
        Ok(())
    }

    /// Say that this deployment holds none of `item` and why (DN-18 §5, GAP-065).
    ///
    /// A node holds no warnings, no reports and no handoffs of its own -- a desktop does
    /// -- and an empty list would read to a partner as "there are none", which is a
    /// different claim and a false one. The reason travels so the partner knows to ask
    /// somebody else rather than concluding the sector is quiet.
    pub fn withhold_exchange(
        &self,
        item: ExchangeItem,
        reason: impl Into<String>,
    ) -> Result<(), ApiError> {
        let mut slot = self
            .exchange_products
            .write()
            .map_err(|_| ApiError::Transport("the exchange lock was poisoned".into()))?;
        slot.insert(
            item,
            v2::ExchangeResponse::NotHeld {
                item,
                reason: reason.into(),
            },
        );
        Ok(())
    }

    /// Everything held for `item`, unfiltered: what an operator inside the deployment
    /// sees. `None` when the lock is unreadable.
    #[must_use]
    pub fn exchange_all(&self, item: ExchangeItem) -> Option<v2::ExchangeResponse> {
        let held = self.exchange_products.read().ok()?;
        Some(
            held.get(&item)
                .cloned()
                .unwrap_or_else(|| v2::ExchangeResponse::NotHeld {
                    item,
                    reason: "this deployment has published nothing for exchange under this item"
                        .into(),
                }),
        )
    }

    /// What `party` may receive of `item` (DN-18 §5, GAP-065): every product put through
    /// [`ExchangeSet::may_send`], which is the agreement gate and the marking gate with
    /// the restrictive one deciding, and a count of what that removed.
    ///
    /// **Counted, never silently dropped**, for the reason `SnapshotResponse::withheld`
    /// exists: a partner told its list is partial can ask; one that is not told believes
    /// it has everything.
    #[must_use]
    pub fn exchange_for(&self, party: &str, item: ExchangeItem) -> Option<v2::ExchangeResponse> {
        match self.exchange_all(item)? {
            held @ v2::ExchangeResponse::NotHeld { .. } => Some(held),
            v2::ExchangeResponse::Held {
                item,
                products,
                withheld,
            } => {
                let total = products.len();
                let products: Vec<v2::ExchangeProduct> = products
                    .into_iter()
                    .filter(|p| self.exchange.may_send(party, item, &p.releasability))
                    .collect();
                Some(v2::ExchangeResponse::Held {
                    item,
                    withheld: withheld + (total - products.len()),
                    products,
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
    pub fn snapshot_for(&self, party: &str) -> Option<v2::SnapshotResponse> {
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
        Some(v2::SnapshotResponse {
            schema_version: full.schema_version,
            tracks,
            plan: None,
            health,
            requirements: Vec::new(),
            withheld,
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
    pub fn publish_coverage(&self, coverage: v2::CoverageResponse) -> Result<(), ApiError> {
        let mut slot = self
            .coverage
            .write()
            .map_err(|_| ApiError::Transport("the coverage lock was poisoned".into()))?;
        *slot = coverage;
        Ok(())
    }

    #[must_use]
    pub fn coverage(&self) -> Option<v2::CoverageResponse> {
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
    pub fn publish_snapshot(&self, snapshot: v2::SnapshotResponse) -> Result<(), ApiError> {
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
    pub fn publish_event(&self, envelope: Envelope) -> Result<(), ApiError> {
        {
            let mut backlog = self
                .backlog
                .lock()
                .map_err(|_| ApiError::Transport("the backlog lock was poisoned".into()))?;
            if backlog.len() == BACKLOG_CAPACITY {
                backlog.pop_front();
            }
            backlog.push_back(envelope.clone());
        }
        // `Err` here means no receivers, which is not a problem worth reporting.
        let _ = self.events.send(envelope);
        Ok(())
    }

    #[must_use]
    pub fn snapshot(&self) -> Option<v2::SnapshotResponse> {
        self.snapshot.read().ok().map(|s| s.clone())
    }

    /// Envelopes from `from_seq` onward, or `None` when the window no longer reaches
    /// that far back and the client must take a fresh snapshot.
    ///
    /// `from_seq` 0 means "everything from now" per the contract, which is an empty
    /// backlog rather than the whole window.
    #[must_use]
    pub fn backlog_since(&self, from_seq: u64) -> Option<Vec<Envelope>> {
        if from_seq == 0 {
            return Some(Vec::new());
        }
        let backlog = self.backlog.lock().ok()?;
        match backlog.front() {
            // Nothing retained yet: there is nothing this client has missed.
            None => Some(Vec::new()),
            Some(oldest) if oldest.seq <= from_seq => Some(
                backlog
                    .iter()
                    .filter(|e| e.seq >= from_seq)
                    .cloned()
                    .collect(),
            ),
            // The window has moved past what was asked for. Saying so is the contract's
            // own rule: a gap the client cannot see would let it believe it had an
            // unbroken stream.
            Some(_) => None,
        }
    }

    fn subscribe(&self) -> broadcast::Receiver<Envelope> {
        self.events.subscribe()
    }
}

/// The v2 routes.
pub fn router(api: Arc<NodeApi>) -> Router {
    Router::new()
        // One `route` call for the two methods: axum panics on a second registration of
        // the same path, and a panic at start-up is not how a node should learn this.
        .route("/v2/session", post(sign_in).get(session_status))
        .route("/v2/snapshot", get(snapshot))
        .route("/v2/health", get(health))
        .route("/v2/coverage", get(coverage))
        .route("/v2/events", get(events))
        .route("/v2/history", get(history))
        .route("/v2/detections", post(submit_detection))
        .route("/v2/sensors/{sensor_id}/task", post(task_sensor))
        .route("/v2/handoffs/{decision_id}/report", post(effector_report))
        .route(
            "/v2/warnings/{asset_id}/{track_id}/acknowledge",
            post(acknowledge_warning),
        )
        // DN-18's three items that had no door (GAP-065). Tracks and health keep theirs:
        // `/v2/snapshot` and `/v2/health` are already the two-gate paths for those, and a
        // second door to the same picture is a second place the gates could differ. Each
        // now carries both doors on the same path (GAP-065, DN-18 §5 amendment 2): `GET`
        // for a partner reading what this deployment holds, `POST` for the desktop that
        // holds it telling this node what that now is.
        .route(
            "/v2/exchange/warnings",
            get(exchange_warnings).post(publish_warnings),
        )
        .route(
            "/v2/exchange/reports",
            get(exchange_reports).post(publish_reports),
        )
        .route(
            "/v2/exchange/handoffs",
            get(exchange_handoffs).post(publish_handoffs),
        )
        .route("/v2/plans/{plan_id}/decision", post(refuse_decision))
        .with_state(api)
}

/// Who is asking (GAP-062, D-02): an operator with a session token, or a machine whose
/// client certificate the TLS handshake verified and whose party has an agreement.
#[derive(Debug, Clone, PartialEq)]
pub enum Caller {
    Operator(OperatorSession),
    Machine { party: String },
}

/// Resolve the caller, or say why not.
///
/// Every route but `POST /v2/session` goes through this. A bearer token is tried first,
/// because a desktop on a mutual-TLS link still speaks for an operator; a connection
/// with a party and no token is a machine, and a machine with no agreement is refused
/// here (DN-18 §5: no agreement, no exchange). A node with no authority configured
/// refuses operators here rather than at each route, so there is one place the answer
/// is decided.
///
/// The error is a whole `Response` and therefore large. Boxing it would buy nothing: it
/// is constructed once per refused request and returned immediately.
#[allow(clippy::result_large_err)]
fn caller(api: &NodeApi, headers: &axum::http::HeaderMap, peer: &Peer) -> Result<Caller, Response> {
    let has_token = headers.get(axum::http::header::AUTHORIZATION).is_some();
    if let (false, Some(party)) = (has_token, &peer.party) {
        if api.exchange.for_party(party).is_none() {
            return Err(problem(
                StatusCode::FORBIDDEN,
                &format!(
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
    operator(api, headers).map(Caller::Operator)
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
#[allow(clippy::result_large_err)]
fn operator(api: &NodeApi, headers: &axum::http::HeaderMap) -> Result<OperatorSession, Response> {
    let Some(callers) = api.callers.as_ref() else {
        return Err(problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "this node authenticates nobody: no account store is configured, so it serves \
             its pipeline and journals it and answers no caller",
        ));
    };
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default();
    callers.verify(token, api.now()).map_err(|failure| {
        // One message for a missing, malformed, forged and expired token alike: telling
        // them apart tells a prober which half to work on.
        problem(StatusCode::UNAUTHORIZED, &failure.to_string())
    })
}

/// `POST /v2/session`: the one route reachable without a token.
async fn sign_in(
    State(api): State<Arc<NodeApi>>,
    Json(request): Json<v2::SessionRequest>,
) -> Response {
    let Some(callers) = api.callers.as_ref() else {
        return problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "this node has no account store configured, so nobody can sign in",
        );
    };
    match callers.sign_in(request.operator, &request.passphrase, api.now()) {
        Ok(issued) => Json(v2::SessionResponse {
            token: issued.token,
            expires_s: issued.expires_s,
        })
        .into_response(),
        Err(failure) => problem(StatusCode::UNAUTHORIZED, &failure.to_string()),
    }
}

/// A route an operator alone may use: the write paths, and what is internal to the
/// deployment. A machine caller is told so rather than served a shape it cannot use.
#[allow(clippy::result_large_err)]
fn operator_only(caller: Caller, what: &str) -> Result<OperatorSession, Response> {
    match caller {
        Caller::Operator(session) => Ok(session),
        Caller::Machine { party } => Err(problem(
            StatusCode::FORBIDDEN,
            &format!("{what} is internal to this deployment and not an exchange item; party {party:?} may not use it"),
        )),
    }
}

/// The caller, refused unless it is an operator: the routes that are internal to the
/// deployment and never an exchange item.
#[allow(clippy::result_large_err)]
fn operator_caller(
    api: &NodeApi,
    headers: &axum::http::HeaderMap,
    peer: &Peer,
    what: &str,
) -> Result<OperatorSession, Response> {
    match caller(api, headers, peer) {
        Ok(c) => operator_only(c, what),
        Err(response) => Err(response),
    }
}

/// `GET /v2/session`: who the caller is, so a desktop can tell an expired session from
/// an unreachable node.
async fn session_status(
    State(api): State<Arc<NodeApi>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    headers: axum::http::HeaderMap,
) -> Response {
    match operator_caller(&api, &headers, &peer, "the session route") {
        Err(response) => response,
        Ok(session) => Json(v2::SessionStatus {
            operator: session.operator.0,
            role: format!("{:?}", session.role),
            expires_s: session.expires.unwrap_or_default(),
        })
        .into_response(),
    }
}

/// Serve the v2 contract in the clear, until the future is dropped.
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
    let snapshot = match caller(&api, &headers, &peer) {
        Err(response) => return response,
        Ok(Caller::Operator(_)) => api.snapshot(),
        // GAP-062: a party sees what its agreement and the markings release, and how
        // much it did not see.
        Ok(Caller::Machine { party }) => api.snapshot_for(&party),
    };
    match snapshot {
        Some(snapshot) => Json(snapshot).into_response(),
        None => problem(
            StatusCode::INTERNAL_SERVER_ERROR,
            "the node's snapshot is unreadable",
        ),
    }
}

/// `GET /v2/history?since_seq=N`: the retained envelopes from `N` (GAP-050).
///
/// `410 Gone` when the window has moved past `N`: the client's outage is longer than the
/// node retains, and saying so is the contract's rule for the stream as well.
async fn history(
    State(api): State<Arc<NodeApi>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    headers: axum::http::HeaderMap,
    axum::extract::Query(query): axum::extract::Query<v2::HistoryQuery>,
) -> Response {
    let party = match caller(&api, &headers, &peer) {
        Err(response) => return response,
        Ok(Caller::Operator(_)) => None,
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
            Json(v2::HistoryResponse {
                since_seq: query.since_seq,
                withheld: total - envelopes.len(),
                envelopes,
            })
            .into_response()
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

/// `GET /v2/coverage`: the gaps along the configured approaches, with the parameters
/// that found them.
async fn coverage(
    State(api): State<Arc<NodeApi>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    headers: axum::http::HeaderMap,
) -> Response {
    if let Err(response) = operator_caller(&api, &headers, &peer, "coverage") {
        return response;
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
    match caller(&api, &headers, &peer) {
        Err(response) => return response,
        Ok(Caller::Operator(_)) => {}
        // DN-18: health is an exchange item of its own.
        Ok(Caller::Machine { party }) => {
            if !api.exchange.may_send(
                &party,
                ExchangeItem::Health,
                &gungnir_model::Releasability::AllPeers,
            ) {
                return problem(
                    StatusCode::FORBIDDEN,
                    &format!("the agreement with {party:?} does not send health"),
                );
            }
        }
    }
    match api.snapshot() {
        Some(snapshot) => Json(snapshot.health).into_response(),
        None => Json(SystemHealth::default()).into_response(),
    }
}

/// `POST /v2/detections`: queue a detection for the ingest gateway.
///
/// **Queued, not accepted.** The gateway authenticates the sensor and validates the
/// detection on its next tick exactly as it does a sensor feed, and quarantines it with a
/// reason if it fails -- which appears on the event stream. Answering `202` rather than
/// `204` says precisely that: it has been taken, not that it has been believed.
async fn submit_detection(
    State(api): State<Arc<NodeApi>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    headers: axum::http::HeaderMap,
    body: Result<Json<v2::SubmitDetectionRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    // A peer's tracks enter through the peer adapter under DN-16, never this route.
    // A sensor with a certificate submits for its own id (GAP-002); everyone else is an
    // operator.
    let vouched = match machine_identity(&api, &headers, &peer) {
        Some((_, MachineRole::Sensor(id))) => Some(id),
        Some((party, role)) => {
            return problem(
                StatusCode::FORBIDDEN,
                &format!(
                    "party {party:?} speaks for {role:?}, not for a sensor; it may not submit \
                     detections"
                ),
            );
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
    if let Err(err) = v2::refuse_other_schema(request.schema_version) {
        return problem(StatusCode::CONFLICT, &err.to_string());
    }
    match vouched {
        Some(id) if request.detection.sensor != id => problem(
            StatusCode::FORBIDDEN,
            &format!(
                "this certificate speaks for sensor {}, and the detection names sensor {}",
                id.0, request.detection.sensor.0
            ),
        ),
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

/// `POST /v2/sensors/{sensor_id}/task` (GAP-004): an operator with the `sensor.task`
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
    body: Result<Json<v2::SensorTaskRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let session = match operator_caller(&api, &headers, &peer, "sensor tasking") {
        Ok(session) => session,
        Err(response) => return response,
    };
    if !gungnir_security::authz::role_permits(session.role, gungnir_security::actions::TASK_SENSOR)
    {
        return problem(
            StatusCode::FORBIDDEN,
            &format!(
                "role {:?} may not command a sensor ({})",
                session.role,
                gungnir_security::actions::TASK_SENSOR
            ),
        );
    }
    let Ok(Json(request)) = body else {
        return problem(StatusCode::BAD_REQUEST, "the task could not be decoded");
    };
    let (reply, answer) = tokio::sync::oneshot::channel();
    {
        let Ok(mut queue) = api.tasks.lock() else {
            return problem(
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
            (StatusCode::ACCEPTED, Json(v2::SensorTaskResponse { task })).into_response()
        }
        Ok(Ok(Err(reason))) => problem(StatusCode::CONFLICT, &reason),
        Ok(Err(_)) => problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "the node loop dropped the task without answering",
        ),
        Err(_) => problem(
            StatusCode::GATEWAY_TIMEOUT,
            "the node loop did not issue the task within the reply window",
        ),
    }
}

/// `POST /v2/handoffs/{decision_id}/report` (GAP-040): what the effector says.
///
/// A machine whose certificate speaks for a handoff endpoint, or an operator with the
/// `effector.report` action keying in what came over the radio. The node knows no
/// handoffs -- a desktop issues them -- so it puts the report on the record for every
/// desktop and the issuing one applies it or rejects it as naming an unknown decision.
async fn effector_report(
    State(api): State<Arc<NodeApi>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    axum::extract::Path(decision_id): axum::extract::Path<u64>,
    headers: axum::http::HeaderMap,
    body: Result<Json<v2::EffectorReportRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let endpoint = match machine_identity(&api, &headers, &peer) {
        Some((_, MachineRole::Effector { endpoint })) => endpoint,
        Some((party, role)) => {
            return problem(
                StatusCode::FORBIDDEN,
                &format!("party {party:?} speaks for {role:?}, not for an effector"),
            );
        }
        None => match operator_caller(&api, &headers, &peer, "effector reporting") {
            Ok(session) => {
                if !gungnir_security::authz::role_permits(
                    session.role,
                    gungnir_security::actions::EFFECTOR_REPORT,
                ) {
                    return problem(
                        StatusCode::FORBIDDEN,
                        &format!("role {:?} may not record an effector report", session.role),
                    );
                }
                format!("operator:{}", session.operator.0)
            }
            Err(response) => return response,
        },
    };
    let Ok(Json(request)) = body else {
        return problem(StatusCode::BAD_REQUEST, "the report could not be decoded");
    };
    match api.reports.lock() {
        Ok(mut queue) => {
            queue.push(EffectorReportRecord {
                decision: DecisionId(decision_id),
                endpoint,
                report: request.report,
            });
            StatusCode::ACCEPTED.into_response()
        }
        Err(_) => problem(
            StatusCode::INTERNAL_SERVER_ERROR,
            "the report queue lock was poisoned",
        ),
    }
}

/// `POST /v2/warnings/{asset_id}/{track_id}/acknowledge` (GAP-042, DN-03 §5 rule 2): the
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
    body: Result<Json<v2::WarningAcknowledgementRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let party = match machine_identity(&api, &headers, &peer) {
        Some((_, MachineRole::WarnedParty { channel })) => channel,
        Some((party, role)) => {
            return problem(
                StatusCode::FORBIDDEN,
                &format!("party {party:?} speaks for {role:?}, not for a warned party"),
            );
        }
        None => match operator_caller(&api, &headers, &peer, "warning acknowledgement") {
            Ok(session) => {
                if !gungnir_security::authz::role_permits(
                    session.role,
                    gungnir_security::actions::ACKNOWLEDGE_WARNING,
                ) {
                    return problem(
                        StatusCode::FORBIDDEN,
                        &format!(
                            "role {:?} may not acknowledge a warning on a party's behalf",
                            session.role
                        ),
                    );
                }
                format!("operator:{}", session.operator.0)
            }
            Err(response) => return response,
        },
    };
    let Ok(Json(request)) = body else {
        return problem(
            StatusCode::BAD_REQUEST,
            "the acknowledgement could not be decoded",
        );
    };
    match api.acknowledgements.lock() {
        Ok(mut queue) => {
            queue.push(WarningAcknowledgement {
                asset: AssetId(asset_id),
                track: TrackId(track_id),
                party,
                at: request.at,
            });
            StatusCode::ACCEPTED.into_response()
        }
        Err(_) => problem(
            StatusCode::INTERNAL_SERVER_ERROR,
            "the acknowledgement queue lock was poisoned",
        ),
    }
}

/// `GET /v2/exchange/warnings` (DN-18 §5, GAP-065).
async fn exchange_warnings(
    State(api): State<Arc<NodeApi>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    headers: axum::http::HeaderMap,
) -> Response {
    serve_exchange(&api, &headers, &peer, ExchangeItem::Warnings)
}

/// `GET /v2/exchange/reports` (DN-18 §5, GAP-065).
async fn exchange_reports(
    State(api): State<Arc<NodeApi>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    headers: axum::http::HeaderMap,
) -> Response {
    serve_exchange(&api, &headers, &peer, ExchangeItem::Reports)
}

/// `GET /v2/exchange/handoffs` (DN-18 §5, GAP-065).
async fn exchange_handoffs(
    State(api): State<Arc<NodeApi>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    headers: axum::http::HeaderMap,
) -> Response {
    serve_exchange(&api, &headers, &peer, ExchangeItem::Handoffs)
}

/// `POST /v2/exchange/warnings` (GAP-065, DN-18 §5 amendment 2).
async fn publish_warnings(
    State(api): State<Arc<NodeApi>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    headers: axum::http::HeaderMap,
    body: Result<Json<v2::PublishExchangeRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    publish_exchange_item(&api, &headers, &peer, body, ExchangeItem::Warnings)
}

/// `POST /v2/exchange/reports` (GAP-065, DN-18 §5 amendment 2).
async fn publish_reports(
    State(api): State<Arc<NodeApi>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    headers: axum::http::HeaderMap,
    body: Result<Json<v2::PublishExchangeRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    publish_exchange_item(&api, &headers, &peer, body, ExchangeItem::Reports)
}

/// `POST /v2/exchange/handoffs` (GAP-065, DN-18 §5 amendment 2).
async fn publish_handoffs(
    State(api): State<Arc<NodeApi>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    headers: axum::http::HeaderMap,
    body: Result<Json<v2::PublishExchangeRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    publish_exchange_item(&api, &headers, &peer, body, ExchangeItem::Handoffs)
}

/// The three exchange publish routes, which differ only in the item (GAP-065, DN-18 §5
/// amendment 2). **`gungnir-api` write path: human-owned, signed by the owner the same
/// day** (`docs/agentic-workflow.md`).
///
/// The caller is this deployment's own desktop link, posting under its own operator
/// session token to tell its node what it now holds -- the same caller [`task_sensor`]
/// answers to, and unlike the caller [`effector_report`] and [`acknowledge_warning`]
/// answer to: those are an outside party telling this deployment something happened,
/// this is this deployment telling its own node something about itself. So there is no
/// machine-identity path here and no queue for the node loop to drain: `publish_exchange`
/// replaces the held set synchronously, and the handler answers as soon as it has.
///
/// **`PUBLISH_EXCHANGE` is not `RELEASE_PRODUCT`.** The action a caller must hold is the
/// one for transmitting a product, not the one for marking it releasable in the first
/// place; see the constant's own doc comment for why the two stay apart.
fn publish_exchange_item(
    api: &NodeApi,
    headers: &axum::http::HeaderMap,
    peer: &Peer,
    body: Result<Json<v2::PublishExchangeRequest>, axum::extract::rejection::JsonRejection>,
    item: ExchangeItem,
) -> Response {
    let session = match operator_caller(api, headers, peer, "publishing to exchange") {
        Ok(session) => session,
        Err(response) => return response,
    };
    if !gungnir_security::authz::role_permits(
        session.role,
        gungnir_security::actions::PUBLISH_EXCHANGE,
    ) {
        return problem(
            StatusCode::FORBIDDEN,
            &format!(
                "role {:?} may not publish to exchange ({})",
                session.role,
                gungnir_security::actions::PUBLISH_EXCHANGE
            ),
        );
    }
    let Ok(Json(request)) = body else {
        return problem(StatusCode::BAD_REQUEST, "the products could not be decoded");
    };
    match api.publish_exchange(item, request.products) {
        Ok(()) => StatusCode::ACCEPTED.into_response(),
        Err(e) => problem(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

/// The three exchange read routes, which differ only in the item (GAP-065).
///
/// **The agreement gate is answered before the products are read**, with a `403` naming
/// the item, exactly as `/v2/health` answers a party whose agreement does not send health.
/// It is the coarse half of DN-18 §5's two gates and it is about the caller rather than
/// about any one product, so a party with no agreement for an item learns that and not how
/// many of them there were. The marking gate is then applied per product by
/// [`NodeApi::exchange_for`], and what it removes is counted on the response.
///
/// An operator inside the deployment sees everything, as on `/v2/snapshot`: the two gates
/// govern what leaves the deployment, not what its own watch may read.
fn serve_exchange(
    api: &NodeApi,
    headers: &axum::http::HeaderMap,
    peer: &Peer,
    item: ExchangeItem,
) -> Response {
    let held = match caller(api, headers, peer) {
        Err(response) => return response,
        Ok(Caller::Operator(_)) => api.exchange_all(item),
        Ok(Caller::Machine { party }) => {
            if !api
                .exchange
                .for_party(&party)
                .is_some_and(|a| a.sends(item))
            {
                return problem(
                    StatusCode::FORBIDDEN,
                    &format!("the agreement with {party:?} does not send {item:?}"),
                );
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

/// `POST /v2/plans/{plan_id}/decision`: refused, and no longer for want of a caller.
///
/// **This node runs no approval queue.** The desktop routes plans through the policy
/// chain and the queue (GAP-038); a node publishes `PlanProposed` and stops. There is
/// nothing here for a decision to be about, and inventing a queue in a request handler
/// would put the recommend-versus-act boundary in the transport.
async fn refuse_decision(
    State(api): State<Arc<NodeApi>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    headers: axum::http::HeaderMap,
) -> Response {
    // Authenticated first: an unauthenticated caller learns nothing about what this node
    // does or does not run.
    if let Err(response) = operator_caller(&api, &headers, &peer, "the decision route") {
        return response;
    }
    problem(
        StatusCode::NOT_IMPLEMENTED,
        "this node runs no approval queue, so there is nothing here to decide. Plans are \
         decided on a desktop, which routes them through the policy chain first.",
    )
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
    let party: Option<String> = if request.token.is_empty() {
        match &peer.party {
            Some(party) if api.exchange.for_party(party).is_some() => Some(party.clone()),
            Some(party) => {
                let _ = socket
                    .send(Message::Text(
                        format!("party {party:?} has no exchange agreement").into(),
                    ))
                    .await;
                return;
            }
            None => {
                let _ = socket
                    .send(Message::Text("no token and no party".into()))
                    .await;
                return;
            }
        }
    } else {
        let authenticated = match api.callers.as_ref() {
            None => Err("this node authenticates nobody".to_owned()),
            Some(callers) => callers
                .verify(&request.token, api.now())
                .map(|_| ())
                .map_err(|failure| failure.to_string()),
        };
        if let Err(reason) = authenticated {
            let _ = socket.send(Message::Text(reason.into())).await;
            return;
        }
        None
    };
    let releases = |envelope: &Envelope| match &party {
        None => true,
        Some(party) => api.releases(party, envelope),
    };

    let Some(backlog) = api.backlog_since(request.from_seq) else {
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
    for envelope in backlog {
        sent_through = sent_through.max(envelope.seq);
        if !releases(&envelope) {
            continue;
        }
        if send_envelope(&mut socket, &envelope).await.is_err() {
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
                Ok(envelope) if envelope.seq <= sent_through => {}
                Ok(envelope) if !releases(&envelope) => {}
                Ok(envelope) => {
                    if send_envelope(&mut socket, &envelope).await.is_err() {
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

async fn read_subscribe(socket: &mut WebSocket) -> Option<v2::SubscribeRequest> {
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

async fn send_envelope(socket: &mut WebSocket, envelope: &Envelope) -> Result<(), ()> {
    let Ok(text) = serde_json::to_string(envelope) else {
        return Err(());
    };
    socket
        .send(Message::Text(text.into()))
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
        NodeApi::new(v2::SnapshotResponse::new(
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

    fn product(id: &str, releasability: gungnir_model::Releasability) -> v2::ExchangeProduct {
        v2::ExchangeProduct {
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
            Some(v2::ExchangeResponse::NotHeld { .. })
        ));
        api.withhold_exchange(ExchangeItem::Reports, "this node produces no reports")
            .expect("withheld");
        match api.exchange_all(ExchangeItem::Reports) {
            Some(v2::ExchangeResponse::NotHeld { item, reason }) => {
                assert_eq!(item, ExchangeItem::Reports);
                assert_eq!(reason, "this node produces no reports");
            }
            other => panic!("expected a reason, got {other:?}"),
        }
        api.publish_exchange(ExchangeItem::Reports, Vec::new())
            .expect("published");
        assert!(matches!(
            api.exchange_all(ExchangeItem::Reports),
            Some(v2::ExchangeResponse::Held { withheld: 0, .. })
        ));
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
            Some(v2::ExchangeResponse::Held {
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
            Some(v2::ExchangeResponse::Held {
                products, withheld, ..
            }) => {
                assert!(products.is_empty(), "{products:?}");
                assert_eq!(withheld, 4);
            }
            other => panic!("expected an empty holding, got {other:?}"),
        }

        // An operator inside the deployment sees all four: the gates govern what leaves.
        match api.exchange_all(ExchangeItem::Warnings) {
            Some(v2::ExchangeResponse::Held {
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
