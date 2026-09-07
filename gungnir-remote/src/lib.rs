// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Remote backends for the connected deployment profiles (ARCHITECTURE.md §8.2).
//! [`RemoteTrackingService`] and [`RemoteInterceptService`] implement the same two
//! traits the embedded services do, over the `gungnir-api` v2 contract: subscribe
//! to the node's event stream to keep a local track/plan projection, forward
//! detections and decisions to the node, and queue outbound detections while the
//! link is down (store-and-forward, §8.4).
//!
//! **Status (GAP-041):** the read half is real. [`connect`] starts a link that fetches a
//! snapshot and follows the node's event stream, and the two services project it. The
//! write half is not: the node refuses every write path because nothing can authenticate
//! a caller (GAP-057, GAP-060), so a submitted detection stays in the outbox and
//! [`RemoteTrackingService::outbox_len`] keeps counting. That is store-and-forward doing
//! exactly what §8.4 says, against a link that will not take a write yet.
//!
//! There is no TLS (GAP-060), so a node serves loopback only and an `https` endpoint is
//! refused rather than downgraded.

pub mod endpoint;
pub mod identity;
pub mod link;
pub mod peer;

use gungnir_intercept_service::{InterceptService, PlanOutcome, PlanView, ResourceView};
use gungnir_tracking_service::{DetectionView, MissionTime, TrackView, TrackingService};
use link::NodeLink;

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
        for cert in rustls_pemfile::certs(&mut pem.as_bytes()) {
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
            let certs: Vec<_> = rustls_pemfile::certs(&mut identity.as_bytes())
                .collect::<Result<_, _>>()
                .map_err(|e| format!("the client certificate does not parse: {e}"))?;
            let key = rustls_pemfile::private_key(&mut identity.as_bytes())
                .map_err(|e| format!("the client key does not parse: {e}"))?
                .ok_or_else(|| "the client identity holds no private key".to_owned())?;
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
    outbox: Vec<DetectionView>,
    dropped: u64,
    connected: bool,
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
            outbox: Vec::new(),
            dropped: 0,
            connected: false,
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
        if projection.connected {
            self.cache.clone_from(&projection.tracks);
        }
    }

    fn tracks(&self) -> &[TrackView] {
        &self.cache
    }

    fn is_healthy(&self) -> bool {
        self.connected
    }
}

/// Plan projection fed by the node; the node runs the allocator.
pub struct RemoteInterceptService {
    endpoint: RemoteEndpoint,
    last_plan: PlanView,
    connected: bool,
    link: Option<NodeLink>,
    /// When `last_plan` was last read from a connected node (GAP-066). `None` before the
    /// first one arrives, which is a different thing from a plan that has gone stale.
    plan_read_at: Option<MissionTime>,
}

impl RemoteInterceptService {
    pub fn detached(endpoint: RemoteEndpoint) -> Self {
        Self {
            endpoint,
            last_plan: PlanView::default(),
            connected: false,
            link: None,
            plan_read_at: None,
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
    /// The node's plan, not one computed here.
    ///
    /// `tracks` and `resources` are ignored on purpose: the node runs the allocator, and
    /// a desktop that planned locally while connected would show a recommendation the
    /// node had never made and could not be held to.
    fn plan(
        &mut self,
        now: MissionTime,
        _tracks: &[TrackView],
        _resources: &[ResourceView],
    ) -> PlanOutcome {
        if let Some(projection) = self.link.as_ref().and_then(link::NodeLink::read) {
            self.connected = projection.connected;
            if projection.connected {
                self.last_plan.clone_from(&projection.plan);
                self.plan_read_at = Some(now);
                return PlanOutcome::Fresh(self.last_plan.clone());
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
            },
            None => PlanOutcome::NoPlan {
                reason: "no plan has been received from the node".to_owned(),
            },
        }
    }

    fn is_healthy(&self) -> bool {
        self.connected
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
            PlanOutcome::NoPlan { reason } => assert!(reason.contains("node"), "{reason}"),
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
}
