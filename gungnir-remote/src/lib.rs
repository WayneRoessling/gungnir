// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Remote backends for the connected deployment profiles (ARCHITECTURE.md §8.2).
//! [`RemoteTrackingService`] and [`RemoteInterceptService`] implement the same two
//! traits the embedded services do, over the `gungnir-api` v3 contract: subscribe
//! to the node's event stream to keep a local track/plan projection, forward
//! detections to the node, and queue outbound detections while the link is down
//! (store-and-forward, §8.4).
//!
//! **Status.** [`connect`] starts a link that fetches a snapshot and follows the node's
//! event stream, and the two services project it (GAP-041). A detection submitted while
//! linked is forwarded to `POST /v3/detections` under the signed-in operator's token, and
//! held in the outbox, oldest dropped and counted, while the node does not answer
//! (GAP-050).
//!
//! **Corrected 2026-09-17: the node holds the queue (D-55, GAP-132, GAP-133.)** This
//! module said until then that "decisions are not forwarded: a node runs no approval
//! queue and refuses the decision route, so the desktop decides the node's plan in its
//! own queue (GAP-129)". That was true of the build it described and is now false of
//! both halves. A node queues what it proposes and serves `POST /v3/queue/{item}/
//! decision`, and [`queue`] is this crate's side of it: a linked desktop **projects** the
//! node's queue and decides through that route, and never queues, engages or hands off a
//! plan the node proposed (`docs/design/DN-31-node-approval-queue.md` §6, clauses DN-31
//! §6.5 and §6.6).
//! Forwarding decisions taken while cut off is GAP-134's and is not built here.
//!
//! **Corrected 2026-09-07: TLS exists.** An `https` endpoint speaks mutual TLS --
//! `link`'s own module comment describes it in full -- and only an `https` endpoint
//! with no pinned trust roots is refused rather than downgraded; `http` still means
//! loopback in the clear. GAP-060's remaining scope is key custody (the passphrase-
//! sealed keystore and the escrow record, GAP-084), not the handshake, which
//! `gungnir-api/tests/mutual_tls.rs` gates today.

pub mod endpoint;
pub mod identity;
pub mod link;
pub mod peer;
pub mod queue;

use gungnir_intercept_service::{
    InterceptService, InterimBound, PlanOutcome, PlanView, ResourceView,
};
use gungnir_model::{BearingRayView, PipelineStatsView, PlanStandingView};
use gungnir_tracking_service::{
    DetectionView, MissionTime, PipelineStats, TrackView, TrackingService,
};
use link::NodeLink;
use rustls::pki_types::pem::{self, PemObject};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteEndpoint {
    /// Base URL of the node's `gungnir-api`, e.g. `http://node.local:7410` or
    /// `https://node.local:7410`.
    pub url: String,
    /// What the link trusts and who it is (GAP-060). Irrelevant to an `http` endpoint.
    pub tls: LinkTls,
}

impl RemoteEndpoint {
    /// An endpoint with no TLS material: loopback in the clear, and the tests.
    #[must_use]
    pub fn plain(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            tls: LinkTls::default(),
        }
    }
}

/// The client side of mutual TLS (GAP-060, D-02).
///
/// The roots come from the baseline (`security.tls.trust_roots_pem`, public material).
///
/// # Two ways to hold an identity, and only one of them is meant for a deployment
///
/// [`Self::issued`] is a certificate this host made itself over a key that **never leaves
/// its `KeyProvider`**. That is the intended path, and it is why
/// [`crate::identity::issue`] exists.
///
/// [`Self::identity_pem`] is a certificate **and its private key** as text. It is the
/// development fallback, read from the environment by the host and never from a baseline,
/// which the workspace rule forbids from naming key material or a path to one. It was the
/// only path until 2026-09-06, and calling it a fallback while it was the only way to get
/// an identity described an intention rather than the code.
///
/// When both are set the issued one wins, because a key in a `String` is the weaker claim.
/// `Debug` says which is held and nothing of either.
#[derive(Clone, Default)]
pub struct LinkTls {
    /// Authorities the link trusts to have signed the node's certificate. **Empty means
    /// an `https` endpoint is refused**: the platform's store is not consulted, because a
    /// defended network's nodes are signed by the deployment's own authority.
    pub trust_roots_pem: Vec<String>,
    /// The identity this host issued from its own key provider. Preferred.
    pub issued: Option<std::sync::Arc<rustls::sign::CertifiedKey>>,
    /// A certificate chain and its private key as PEM: the development fallback. `None`
    /// in both fields connects without a client certificate, which a node configured per
    /// D-02 refuses at the handshake and the link reports.
    pub identity_pem: Option<String>,
}

impl LinkTls {
    /// Whether this host can present a client certificate at all.
    #[must_use]
    pub fn has_identity(&self) -> bool {
        self.issued.is_some() || self.identity_pem.is_some()
    }
}

/// Compared by what is presented, not by which object holds it.
///
/// Hand-written because `rustls::sign::CertifiedKey` holds a `dyn SigningKey` and cannot
/// derive equality -- a signer is a capability rather than a value. Two links are the same
/// link when they trust the same roots and would present the same certificate, so the
/// comparison is on the certificate's DER and not on the signer behind it.
impl PartialEq for LinkTls {
    fn eq(&self, other: &Self) -> bool {
        fn chain(tls: &LinkTls) -> Option<Vec<Vec<u8>>> {
            tls.issued
                .as_ref()
                .map(|k| k.cert.iter().map(|c| c.as_ref().to_vec()).collect())
        }
        self.trust_roots_pem == other.trust_roots_pem
            && self.identity_pem == other.identity_pem
            && chain(self) == chain(other)
    }
}

impl Eq for LinkTls {}

impl std::fmt::Debug for LinkTls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LinkTls")
            .field("trust_roots", &self.trust_roots_pem.len())
            .field("issued", &self.issued.is_some())
            .field("identity_pem", &self.identity_pem.is_some())
            .finish()
    }
}

/// Presents this host's issued certificate to any node that asks for one.
///
/// It offers the same certificate whatever the server's acceptable-issuer list says. That
/// is correct here and would not be in general: a deployment's node trusts one authority,
/// and a client with one identity that guessed it was unwanted and presented nothing would
/// turn a clear handshake refusal into a silent anonymous connection.
#[derive(Debug)]
struct IssuedClientCert(std::sync::Arc<rustls::sign::CertifiedKey>);

impl rustls::client::ResolvesClientCert for IssuedClientCert {
    fn resolve(
        &self,
        _acceptable_issuers: &[&[u8]],
        _sigschemes: &[rustls::SignatureScheme],
    ) -> Option<std::sync::Arc<rustls::sign::CertifiedKey>> {
        Some(std::sync::Arc::clone(&self.0))
    }

    fn has_certs(&self) -> bool {
        true
    }
}

/// One `rustls::ClientConfig` for both the HTTP client and the event stream.
///
/// **Built once and shared deliberately.** The two used to be built separately from the
/// same PEM, which was tolerable while both could read a private key out of a string and
/// is not now: an identity whose key lives in a provider cannot be handed to `reqwest` as
/// PEM at all, so the only way both paths present the same certificate is for both to be
/// given the same configuration.
///
/// # Errors
///
/// When a trust root or the fallback identity does not parse, naming which.
pub fn client_config(tls: &LinkTls) -> Result<rustls::ClientConfig, String> {
    let mut roots = rustls::RootCertStore::empty();
    for (i, pem) in tls.trust_roots_pem.iter().enumerate() {
        for cert in CertificateDer::pem_slice_iter(pem.as_bytes()) {
            let cert = cert.map_err(|e| format!("trust root {i} does not parse: {e}"))?;
            roots
                .add(cert)
                .map_err(|e| format!("trust root {i} is not usable: {e}"))?;
        }
    }
    let builder = rustls::ClientConfig::builder().with_root_certificates(roots);
    // The issued identity wins over the PEM fallback; see `LinkTls`.
    if let Some(issued) = &tls.issued {
        return Ok(
            builder.with_client_cert_resolver(std::sync::Arc::new(IssuedClientCert(
                std::sync::Arc::clone(issued),
            ))),
        );
    }
    match &tls.identity_pem {
        None => Ok(builder.with_no_client_auth()),
        Some(identity) => {
            let certs: Vec<_> = CertificateDer::pem_slice_iter(identity.as_bytes())
                .collect::<Result<_, _>>()
                .map_err(|e| format!("the client certificate does not parse: {e}"))?;
            // As in `gungnir-api`'s reader, the key's parse error is not forwarded: it can
            // quote the offending line, and that line is key material.
            let key = PrivateKeyDer::from_pem_slice(identity.as_bytes()).map_err(|e| match e {
                pem::Error::NoItemsFound => "the client identity holds no private key".to_owned(),
                _ => "the client key does not parse".to_owned(),
            })?;
            builder
                .with_client_auth_cert(certs, key)
                .map_err(|e| format!("the client identity is not usable: {e}"))
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RemoteError {
    /// Kept for the write half, which no build can carry yet.
    #[error("remote transport not implemented in this build; cannot reach {0}")]
    TransportNotImplemented(String),
    #[error("invalid endpoint: {0}")]
    InvalidEndpoint(String),
    /// The endpoint client could not be built (GAP-040): a trust root that does not
    /// parse, or the HTTP client itself.
    #[error("endpoint client: {0}")]
    Client(String),
}

/// Largest number of detections held while disconnected before the oldest are
/// dropped; sized for minutes of a single sensor at tens of hertz.
pub const OUTBOX_CAPACITY: usize = 100_000;

/// Establish both remote services against one node.
///
/// Returns as soon as the link task is spawned. **`Ok` does not mean a node answered**:
/// both services report `is_healthy()` false until a snapshot has actually been received,
/// and the desktop keeps whatever it was showing. Blocking here to prove reachability
/// would stall start-up on an unreachable node, and reporting success on a spawned task
/// would be the health flag AP-02 forbids -- so the call succeeds and the honesty lives
/// in `is_healthy`.
pub fn connect(
    endpoint: &RemoteEndpoint,
    credential: link::Credential,
    runtime: &tokio::runtime::Handle,
) -> Result<(RemoteTrackingService, RemoteInterceptService), RemoteError> {
    let (tracking, intercept, _) = connect_with_link(endpoint, credential, runtime)?;
    Ok((tracking, intercept))
}

/// As [`connect`], and the link itself, so the desktop can judge its silence against the
/// heartbeat after the services are boxed (GAP-050).
///
/// # Errors
///
/// As [`connect`].
pub fn connect_with_link(
    endpoint: &RemoteEndpoint,
    credential: link::Credential,
    runtime: &tokio::runtime::Handle,
) -> Result<(RemoteTrackingService, RemoteInterceptService, NodeLink), RemoteError> {
    let link = link::start(endpoint, credential, runtime)?;
    Ok((
        RemoteTrackingService::linked(endpoint.clone(), link.clone()),
        RemoteInterceptService::linked(endpoint.clone(), link.clone()),
        link,
    ))
}

/// Track projection fed by the node's event stream, with an outbox for detections
/// submitted while disconnected.
pub struct RemoteTrackingService {
    endpoint: RemoteEndpoint,
    cache: Vec<TrackView>,
    /// Mirrors `link::Projection::bearing_rays` (GAP-096's wire contract); see that
    /// field's own doc comment for the per-(re)connect refresh cadence.
    bearing_rays: Vec<BearingRayView>,
    /// Mirrors `link::Projection::pipeline_stats` (GAP-096's wire contract).
    pipeline_stats: PipelineStatsView,
    outbox: Vec<DetectionView>,
    dropped: u64,
    connected: bool,
    /// Whether the node last reported its tracking pipeline healthy (GAP-161); see
    /// `link::Projection::node_health`.
    node_tracking_healthy: bool,
    /// The live link, when this service was built by [`connect`]. `None` for a detached
    /// service, which is every one built before GAP-041 and the ones tests use.
    link: Option<NodeLink>,
}

impl RemoteTrackingService {
    /// A service that has never connected; everything submitted is queued.
    pub fn detached(endpoint: RemoteEndpoint) -> Self {
        Self {
            endpoint,
            cache: Vec::new(),
            bearing_rays: Vec::new(),
            pipeline_stats: PipelineStatsView::default(),
            outbox: Vec::new(),
            dropped: 0,
            connected: false,
            node_tracking_healthy: false,
            link: None,
        }
    }

    /// A service reading a live link.
    #[must_use]
    pub fn linked(endpoint: RemoteEndpoint, link: NodeLink) -> Self {
        Self {
            link: Some(link),
            ..Self::detached(endpoint)
        }
    }

    /// The most recent transport failure, for PN-01.
    #[must_use]
    pub fn last_error(&self) -> Option<String> {
        self.link
            .as_ref()
            .and_then(link::NodeLink::read)
            .and_then(|p| p.last_error.clone())
    }

    pub fn endpoint(&self) -> &RemoteEndpoint {
        &self.endpoint
    }

    /// How long since the node was last heard from (D-23), `None` when it never has been
    /// or there is no link at all.
    #[must_use]
    pub fn last_heard_age(&self) -> Option<std::time::Duration> {
        self.link.as_ref().and_then(link::NodeLink::last_heard_age)
    }

    /// Detections waiting for the node: on the link's outbox when linked, here when
    /// detached.
    pub fn outbox_len(&self) -> usize {
        self.outbox.len() + self.link.as_ref().map_or(0, link::NodeLink::outbox_len)
    }

    pub fn dropped(&self) -> u64 {
        self.dropped + self.link.as_ref().map_or(0, link::NodeLink::dropped)
    }

    /// Detections the node has accepted from the outbox (§8.4).
    pub fn forwarded(&self) -> u64 {
        self.link.as_ref().map_or(0, link::NodeLink::forwarded)
    }

    /// Apply a snapshot received from the node (the event-stream consumer calls this).
    pub fn apply_snapshot(&mut self, tracks: Vec<TrackView>) {
        self.cache = tracks;
    }

    /// Take everything queued for forwarding once the link returns.
    pub fn drain_outbox(&mut self) -> Vec<DetectionView> {
        std::mem::take(&mut self.outbox)
    }
}

impl TrackingService for RemoteTrackingService {
    /// # Errors
    ///
    /// Never today: the outbox always takes the detection, dropping the oldest when it is
    /// full. **The drop is still a loss** and `dropped` counts it; it is not reported here
    /// because the detection being submitted was accepted, and reporting an error for it
    /// would blame the wrong one.
    fn submit_detection(
        &mut self,
        detection: DetectionView,
    ) -> Result<(), gungnir_tracking_service::SubmitError> {
        // Linked: the link's outbox, which its task forwards whenever the node answers
        // (GAP-050). Detached: held here, counted on PN-01, forwarded by nothing.
        if let Some(link) = &self.link {
            link.queue_outbound(detection);
            return Ok(());
        }
        if self.outbox.len() >= OUTBOX_CAPACITY {
            self.outbox.remove(0);
            self.dropped += 1;
            if self.dropped == 1 {
                tracing::warn!(endpoint = %self.endpoint.url, "remote outbox full; dropping oldest detections");
            }
        }
        self.outbox.push(detection);
        Ok(())
    }

    /// Copy the link's projection into the cache.
    ///
    /// The outbox is flushed by the link task, not here: forwarding is asynchronous and
    /// the frame loop never waits on the network. The count on PN-01 is what has not
    /// been accepted yet.
    fn poll(&mut self, _now: MissionTime) {
        let Some(link) = self.link.as_ref() else {
            return;
        };
        let Some(projection) = link.read() else {
            // The link task panicked while holding the lock. Report unhealthy and keep
            // the last picture rather than clearing it: stale and labelled beats empty.
            self.connected = false;
            return;
        };
        self.connected = projection.connected;
        self.node_tracking_healthy = projection.node_health.is_some_and(|h| h.tracking_healthy);
        if projection.connected {
            self.cache.clone_from(&projection.tracks);
            // GAP-096's wire contract: the same projection tracks came from also
            // carries the node's retained bearings and pipeline counters, refreshed on
            // the cadence `link::Projection::bearing_rays`'s doc comment describes.
            self.bearing_rays.clone_from(&projection.bearing_rays);
            self.pipeline_stats = projection.pipeline_stats;
        }
    }

    fn tracks(&self) -> &[TrackView] {
        &self.cache
    }

    /// The node's retained bearings as of the last snapshot (GAP-096's wire contract):
    /// real data read over the link, in place of the [`TrackingService`] trait's
    /// defaulted empty answer.
    fn bearing_rays(&self) -> &[BearingRayView] {
        &self.bearing_rays
    }

    /// The node pipeline's own counters as of the last snapshot (GAP-096's wire
    /// contract), converted back from the wire view into the same `PipelineStats`
    /// [`LiveTrackingService`](gungnir_tracking_service::LiveTrackingService) returns to
    /// an embedded caller -- see `gungnir_tracking_service::pipeline_stats_from_view`.
    fn pipeline_stats(&self) -> PipelineStats {
        gungnir_tracking_service::pipeline_stats_from_view(self.pipeline_stats)
    }

    /// Linked, **and** the node reports its tracking pipeline healthy (GAP-161).
    ///
    /// A link that is up to a node whose tracker has stopped is not a working tracker,
    /// and the status strip reading this is the only place a linked operator would learn
    /// that the picture has stopped moving.
    fn is_healthy(&self) -> bool {
        self.connected && self.node_tracking_healthy
    }
}

/// Plan projection fed by the node; the node runs the allocator.
pub struct RemoteInterceptService {
    endpoint: RemoteEndpoint,
    last_plan: PlanView,
    connected: bool,
    /// Whether the node last reported its planner healthy (GAP-161).
    node_intercept_healthy: bool,
    link: Option<NodeLink>,
    /// When `last_plan` was last known to answer the node's picture, on this desktop's
    /// clock (GAP-066, GAP-157): the last read of a current or interim plan, or the node's
    /// own `computed_at` for a stale one. `None` before the first plan arrives, which is a
    /// different thing from a plan that has gone stale.
    plan_read_at: Option<MissionTime>,
    /// The node's clock and this desktop's, read together the first call that sees a new
    /// reading from the node (GAP-157, by GAP-140's rule): what converts a time the node
    /// stamped into one on the clock this service's caller passes as `now`.
    ///
    /// **Measured here as well as in the app**, which measures the same pair for the
    /// queue's deadlines, because the planner's contract is that every time in a
    /// [`PlanOutcome`] is on its caller's clock and this service has no other way to keep
    /// it. Both take the node's reading once per connection against the tick's `now`, so
    /// they agree.
    node_clock: Option<(MissionTime, MissionTime)>,
}

impl RemoteInterceptService {
    pub fn detached(endpoint: RemoteEndpoint) -> Self {
        Self {
            endpoint,
            last_plan: PlanView::default(),
            connected: false,
            node_intercept_healthy: false,
            link: None,
            plan_read_at: None,
            node_clock: None,
        }
    }

    /// A time the node stamped, on this desktop's clock (GAP-157).
    ///
    /// Through the offset measured this connection. A node that sends no clock leaves the
    /// time as it is -- the desktop draws against its own clock and says nothing it cannot
    /// support, as it does for the queue's deadlines (GAP-140).
    fn ours(&self, node_time: MissionTime) -> MissionTime {
        match self.node_clock {
            Some((node, ours)) => MissionTime(node_time.0 - (node.0 - ours.0)),
            None => node_time,
        }
    }

    /// The node's plan, labelled with what the node says about it (GAP-157, D-94).
    ///
    /// **What an embedded desktop would be told, in the node's words.** Current is fresh;
    /// interim is the node's one-step answer with the node's bound (GAP-156); stale is
    /// the node's last good plan, computed when the node says -- converted to this
    /// desktop's clock, so the age PN-05 draws is the age on the node's clock -- and why;
    /// no plan is no plan. A node that says nothing, one built before it could, is taken
    /// as it always was: its plan, fresh, with its health flag beside it (GAP-161).
    fn outcome_for(&mut self, standing: Option<PlanStandingView>, now: MissionTime) -> PlanOutcome {
        let on_the_node = |reason: &str| format!("on the node, {reason}");
        match standing {
            None | Some(PlanStandingView::Current) => {
                self.plan_read_at = Some(now);
                PlanOutcome::Fresh(self.last_plan.clone())
            }
            Some(PlanStandingView::Interim {
                value_at_least,
                optimum_at_most,
                reason,
            }) => {
                self.plan_read_at = Some(now);
                PlanOutcome::Interim {
                    plan: self.last_plan.clone(),
                    bound: InterimBound {
                        value_at_least,
                        optimum_at_most,
                    },
                    reason: on_the_node(&reason),
                    // The node's progress stays on the node: it changes every tick, and
                    // the wire carries what changes with the picture (D-94).
                    progress: None,
                }
            }
            Some(PlanStandingView::Stale {
                computed_at,
                reason,
            }) => {
                let computed_at = self.ours(computed_at);
                self.plan_read_at = Some(computed_at);
                PlanOutcome::Stale {
                    plan: self.last_plan.clone(),
                    computed_at,
                    reason: on_the_node(&reason),
                    progress: None,
                }
            }
            Some(PlanStandingView::NoPlan { reason }) => PlanOutcome::NoPlan {
                reason: on_the_node(&reason),
                progress: None,
            },
        }
    }

    /// A service reading a live link.
    #[must_use]
    pub fn linked(endpoint: RemoteEndpoint, link: NodeLink) -> Self {
        Self {
            link: Some(link),
            ..Self::detached(endpoint)
        }
    }

    pub fn endpoint(&self) -> &RemoteEndpoint {
        &self.endpoint
    }

    pub fn apply_plan(&mut self, plan: PlanView) {
        self.last_plan = plan;
    }
}

impl InterceptService for RemoteInterceptService {
    /// The node's plan, not one computed here, with what the node says about it.
    ///
    /// `tracks` and `resources` are ignored on purpose: the node runs the allocator, and
    /// a desktop that planned locally while connected would show a recommendation the
    /// node had never made and could not be held to.
    ///
    /// **Linked, the node's standing decides the answer** (GAP-157): until then this
    /// returned the node's plan `Fresh` whenever the link was up, so a node whose planner
    /// had fallen behind had its last good plan drawn on every linked desktop with no
    /// stale line and no age. See [`RemoteInterceptService::outcome_for`].
    fn plan(
        &mut self,
        now: MissionTime,
        _tracks: &[TrackView],
        _resources: &[ResourceView],
    ) -> PlanOutcome {
        let linked = self.link.as_ref().and_then(link::NodeLink::read).map(|p| {
            (
                p.connected,
                p.node_health.is_some_and(|h| h.intercept_healthy),
                p.node_time,
                p.connected
                    .then(|| (p.plan.clone(), p.plan_standing.clone())),
            )
        });
        if let Some((connected, healthy, node_time, current)) = linked {
            self.connected = connected;
            self.node_intercept_healthy = healthy;
            // GAP-140's rule: the node's reading paired with this call's clock, once per
            // new reading -- re-pairing an unchanged reading every call would drift the
            // offset by exactly the age of the snapshot it came from.
            if let Some(node) = node_time {
                if self.node_clock.is_none_or(|(seen, _)| seen != node) {
                    self.node_clock = Some((node, now));
                }
            }
            if let Some((plan, standing)) = current {
                self.last_plan = plan;
                return self.outcome_for(standing, now);
            }
        }
        // **Detached.** The plan on screen is whatever the node last sent, and saying so
        // is the whole point of this type: a desktop that lost its link used to return
        // that plan indistinguishably from a live one (GAP-066).
        match self.plan_read_at {
            Some(computed_at) => PlanOutcome::Stale {
                plan: self.last_plan.clone(),
                computed_at,
                reason: "the link to the node is down".to_owned(),
                progress: None,
            },
            None => PlanOutcome::NoPlan {
                reason: "no plan has been received from the node".to_owned(),
                progress: None,
            },
        }
    }

    /// Linked, **and** the node reports its planner healthy (GAP-161), for the reason
    /// [`RemoteTrackingService::is_healthy`] gives.
    fn is_healthy(&self) -> bool {
        self.connected && self.node_intercept_healthy
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::{Provenance, SensorId};

    fn endpoint() -> RemoteEndpoint {
        RemoteEndpoint::plain("http://node.local:7410")
    }

    fn detection(i: f64) -> DetectionView {
        DetectionView {
            sensor: SensorId(1),
            source_time: MissionTime(i),
            receipt_time: MissionTime(i),
            measurement: gungnir_model::Measurement::Position {
                enu: nalgebra::Vector3::new(i, 0.0, 0.0),
                variance_m2: [400.0, 400.0, 900.0],
            },
            provenance: Provenance::default(),
        }
    }

    /// A link that has been started but has reached nothing is not healthy, and
    /// `connect` returning `Ok` must not be read as a node having answered.
    #[test]
    fn a_started_link_is_not_healthy_until_a_node_answers() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        // Nothing is listening on this port; the link task will keep retrying.
        let (tracking, mut intercept) = connect(
            &RemoteEndpoint::plain("http://127.0.0.1:1"),
            link::Credential {
                operator: 7,
                passphrase: "unused; nothing is listening".into(),
            },
            runtime.handle(),
        )
        .expect("the link starts");
        assert!(
            !tracking.is_healthy(),
            "a link that reached nothing reported healthy"
        );
        assert!(!intercept.is_healthy());
        // **Not an empty plan** (GAP-066): a link that reached nothing has never received
        // one, and an empty plan would say the node had considered the sector and
        // proposed nothing.
        match intercept.plan(MissionTime(0.0), &[], &[]) {
            PlanOutcome::NoPlan { reason, .. } => assert!(reason.contains("node"), "{reason}"),
            other => panic!("an unconnected service produced {other:?}"),
        }
    }

    #[test]
    fn an_invalid_endpoint_is_refused_before_anything_is_spawned() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        assert!(matches!(
            connect(
                &RemoteEndpoint::plain(" "),
                link::Credential {
                    operator: 7,
                    passphrase: "unused; the endpoint is refused first".into(),
                },
                runtime.handle()
            ),
            Err(RemoteError::InvalidEndpoint(_))
        ));
    }

    #[test]
    fn detached_tracking_service_queues_detections_and_is_unhealthy() {
        let mut svc = RemoteTrackingService::detached(endpoint());
        svc.submit_detection(detection(1.0)).expect("queued");
        svc.submit_detection(detection(2.0)).expect("queued");
        assert_eq!(svc.outbox_len(), 2);
        assert!(!svc.is_healthy());
        assert!(svc.tracks().is_empty());
        let drained = svc.drain_outbox();
        assert_eq!(drained.len(), 2);
        assert_eq!(svc.outbox_len(), 0);
    }

    #[test]
    fn detached_intercept_service_returns_last_applied_plan() {
        let mut svc = RemoteInterceptService::detached(endpoint());
        let plan = PlanView {
            policy_value: 2.0,
            ..PlanView::default()
        };
        svc.apply_plan(plan.clone());
        // Applied but never read from a connected node, so it is not a fresh answer and
        // there is no read time to age it from.
        match svc.plan(MissionTime(0.0), &[], &[]) {
            PlanOutcome::NoPlan { .. } => {}
            other => panic!("a detached service reported {other:?}"),
        }
        assert!(!svc.is_healthy());
    }

    /// A linked service whose projection the test sets: connected, the node's clock at
    /// `node_time`, and `plan` as the node's plan.
    fn linked_to(node_time: f64, plan: &PlanView) -> (NodeLink, RemoteInterceptService) {
        let link = NodeLink::scripted();
        link.script_liveness(true, Some(std::time::Instant::now()));
        if let Some(mut p) = link.read() {
            p.node_time = Some(MissionTime(node_time));
            p.plan = plan.clone();
        }
        let svc = RemoteInterceptService::linked(endpoint(), link.clone());
        (link, svc)
    }

    fn standing(link: &NodeLink, standing: Option<PlanStandingView>) {
        if let Some(mut p) = link.read() {
            p.plan_standing = standing;
        }
    }

    /// **GAP-157, D-94.** The node's word on its plan decides what a linked desktop is
    /// told, and a time the node stamped is drawn on the desktop's clock: here the node's
    /// clock is a thousand seconds ahead, so a plan the node computed at 990 is twenty
    /// seconds old when the desktop's clock reads 10 (the node's reads 1010) -- not minus
    /// nine hundred and eighty, which is what subtracting across the two clocks gives.
    #[test]
    fn a_linked_service_tells_what_the_node_says_about_its_plan() {
        let plan = PlanView {
            policy_value: 3.0,
            ..PlanView::default()
        };
        let (link, mut svc) = linked_to(1000.0, &plan);

        // Measured on the first call that sees the node's reading: node 1000, ours 0.
        standing(&link, Some(PlanStandingView::Current));
        assert!(matches!(
            svc.plan(MissionTime(0.0), &[], &[]),
            PlanOutcome::Fresh(p) if p == plan
        ));

        standing(
            &link,
            Some(PlanStandingView::Stale {
                computed_at: MissionTime(990.0),
                reason: "the solve did not finish inside its 4 ms budget".into(),
            }),
        );
        match svc.plan(MissionTime(10.0), &[], &[]) {
            PlanOutcome::Stale {
                plan: p,
                computed_at,
                reason,
                progress,
            } => {
                assert_eq!(p, plan);
                assert_eq!(computed_at, MissionTime(-10.0));
                assert!(
                    (10.0 - computed_at.0 - 20.0).abs() < 1e-9,
                    "age on the node's clock"
                );
                assert!(reason.starts_with("on the node, "), "{reason}");
                assert!(reason.contains("4 ms budget"), "{reason}");
                assert_eq!(progress, None, "the node's progress stays on the node");
            }
            other => panic!("the node said stale and the desktop was told {other:?}"),
        }

        standing(
            &link,
            Some(PlanStandingView::Interim {
                value_at_least: 3.0,
                optimum_at_most: 4.0,
                reason: "the exact solver takes at most 16 tracks".into(),
            }),
        );
        match svc.plan(MissionTime(11.0), &[], &[]) {
            PlanOutcome::Interim { plan: p, bound, .. } => {
                assert_eq!(p, plan);
                assert!((bound.share_of_optimum() - 0.75).abs() < 1e-12);
            }
            other => panic!("the node said interim and the desktop was told {other:?}"),
        }

        standing(
            &link,
            Some(PlanStandingView::NoPlan {
                reason: "no solve has finished".into(),
            }),
        );
        assert!(matches!(
            svc.plan(MissionTime(12.0), &[], &[]),
            PlanOutcome::NoPlan { .. }
        ));

        // A node that says nothing is taken as it always was.
        standing(&link, None);
        assert!(svc.plan(MissionTime(13.0), &[], &[]).is_fresh());
    }

    /// A stale plan goes on ageing from when the node computed it once the link drops,
    /// not from when the desktop last read it.
    #[test]
    fn a_stale_node_plan_keeps_its_age_when_the_link_drops() {
        let plan = PlanView::default();
        let (link, mut svc) = linked_to(500.0, &plan);
        standing(
            &link,
            Some(PlanStandingView::Stale {
                computed_at: MissionTime(470.0),
                reason: "behind".into(),
            }),
        );
        let _ = svc.plan(MissionTime(100.0), &[], &[]);
        link.script_liveness(false, None);
        match svc.plan(MissionTime(105.0), &[], &[]) {
            PlanOutcome::Stale {
                computed_at,
                reason,
                ..
            } => {
                assert_eq!(computed_at, MissionTime(70.0));
                assert!(reason.contains("link to the node is down"), "{reason}");
            }
            other => panic!("{other:?}"),
        }
    }
}
