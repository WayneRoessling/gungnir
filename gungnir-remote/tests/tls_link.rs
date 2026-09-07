// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The link over mutual TLS (GAP-060, GAP-041): an operator's desktop and a partner's
//! machine link both reach a node that serves nothing in the clear.
//!
//! The certificates are made here with `rcgen` (dev-only, D-22) and never written to
//! the repository. The node's identity is written to files because that is how the
//! node reads its own (`TlsPaths`); the clients hold theirs as PEM text, which is how
//! the desktop reads the environment's.

use gungnir_api::tls::{self, TlsListener, TlsPaths};
use gungnir_api::transport::{serve_on_listener, AccountTokenAuthority, NodeApi};
use gungnir_api::v2::SnapshotResponse;
use gungnir_model::{
    Classification, ExchangeAgreement, ExchangeFormat, ExchangeItem, ExchangeSet, MissionTime,
    Provenance, Quality, Releasability, SystemHealth, TrackId, TrackStatus, TrackView,
};
use gungnir_remote::link::Credential;
use gungnir_remote::peer::PeerLink;
use gungnir_remote::{connect, LinkTls, RemoteEndpoint};
use gungnir_security::{hash_passphrase, Account, InMemoryAccountStore, OperatorId, TokenIssuer};
use gungnir_tracking_service::TrackingService;
use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, Issuer, KeyPair};
use std::sync::Arc;

/// Makes every scratch directory in this binary its own.
///
/// **A process id is not enough.** It separates two concurrent `cargo test` runs; it does
/// nothing for two tests in this same binary that pass the same name, which share a
/// process and therefore an id. The second one's `remove_dir_all` then wipes the first
/// one's certificate authority mid-handshake, and it surfaces as
/// `InvalidCertificate(BadSignature)` -- which reads like a TLS bug and is a directory
/// bug. With this counter the name is a label rather than an identity.
static SCRATCH: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn scratch_id() -> u32 {
    SCRATCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

struct Pki {
    dir: std::path::PathBuf,
    ca_pem: String,
    issuer_params: CertificateParams,
    issuer_key: KeyPair,
}

impl Pki {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "gungnir-tls-link-{name}-{}-{}",
            std::process::id(),
            scratch_id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let issuer_key = KeyPair::generate().expect("ca key");
        let mut issuer_params = CertificateParams::new(Vec::<String>::new()).expect("params");
        issuer_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        issuer_params
            .distinguished_name
            .push(DnType::CommonName, "gungnir test authority");
        let ca = issuer_params.self_signed(&issuer_key).expect("ca");
        Self {
            dir,
            ca_pem: ca.pem(),
            issuer_params,
            issuer_key,
        }
    }

    fn issuer(&self) -> Issuer<'_, &KeyPair> {
        Issuer::from_params(&self.issuer_params, &self.issuer_key)
    }

    /// The node's identity for `localhost`, on disk the way the node reads it.
    fn server_paths(&self) -> TlsPaths {
        let key = KeyPair::generate().expect("server key");
        let params = CertificateParams::new(vec!["localhost".to_string()]).expect("params");
        let cert = params.signed_by(&key, &self.issuer()).expect("server cert");
        let chain = self.dir.join("server.pem");
        let key_path = self.dir.join("server.key");
        let ca = self.dir.join("ca.pem");
        std::fs::write(&chain, cert.pem()).expect("chain");
        std::fs::write(&key_path, key.serialize_pem()).expect("key");
        std::fs::write(&ca, &self.ca_pem).expect("ca");
        TlsPaths {
            certificate_chain: chain.to_string_lossy().into_owned(),
            private_key: key_path.to_string_lossy().into_owned(),
            client_ca: ca.to_string_lossy().into_owned(),
        }
    }

    /// A client's material as the desktop holds it: the roots, and one PEM text with
    /// the certificate and its key, the subject common name being the party.
    fn client(&self, party: &str) -> LinkTls {
        let key = KeyPair::generate().expect("client key");
        let mut params = CertificateParams::new(Vec::<String>::new()).expect("params");
        params.distinguished_name.push(DnType::CommonName, party);
        let cert = params.signed_by(&key, &self.issuer()).expect("client cert");
        LinkTls {
            trust_roots_pem: vec![self.ca_pem.clone()],
            issued: None,
            identity_pem: Some(format!("{}\n{}", cert.pem(), key.serialize_pem())),
        }
    }
}

fn track(id: u64) -> TrackView {
    TrackView {
        id: TrackId(id),
        status: TrackStatus::Confirmed,
        state: nalgebra::SVector::zeros(),
        covariance: nalgebra::SMatrix::identity(),
        classification: Classification::Unknown,
        provenance: Provenance::default(),
        quality: Quality::default(),
        mission_time: MissionTime(f64::from(u32::try_from(id).unwrap_or(u32::MAX))),
        releasability: Releasability::AllPeers,
    }
}

const PASSPHRASE: &str = "correct horse battery staple";

fn credential() -> Credential {
    Credential {
        operator: 7,
        passphrase: PASSPHRASE.into(),
    }
}

/// A node with one operator account and one exchange agreement, for `partner`.
fn api() -> Arc<NodeApi> {
    let store = InMemoryAccountStore::new(vec![Account {
        operator: OperatorId(7),
        role: gungnir_security::Role::Supervisor,
        phc: hash_passphrase(PASSPHRASE).expect("hashed"),
    }]);
    let issuer = TokenIssuer::new(vec![3u8; 32], 300.0).expect("issuer");
    let snapshot = SnapshotResponse::new(vec![track(1)], None, SystemHealth::default(), Vec::new());
    Arc::new(
        NodeApi::new(snapshot)
            .with_callers(Arc::new(AccountTokenAuthority::new(
                Box::new(store),
                issuer,
            )))
            .with_exchange(ExchangeSet {
                agreements: vec![ExchangeAgreement {
                    party: "partner".into(),
                    inbound: Vec::new(),
                    outbound: vec![ExchangeItem::Tracks],
                    format: ExchangeFormat::Canonical,
                }],
            }),
    )
}

async fn serve(pki: &Pki, api: Arc<NodeApi>) -> String {
    let acceptor = tls::acceptor(&pki.server_paths()).expect("acceptor");
    let tcp = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = tcp.local_addr().expect("addr").port();
    tokio::spawn(async move {
        let _ = serve_on_listener(TlsListener::new(tcp, acceptor), api).await;
    });
    format!("https://localhost:{port}")
}

/// **A deadlock guard, not a performance assertion.** Set on the same reasoning as
/// `gungnir-tracking-service/tests/sample_set_replay.rs`: at 200 iterations this was five
/// seconds, which is generous on an unloaded machine and not on a shared runner doing
/// something else. A correctness test failing for want of CPU says nothing about the
/// link. A minute means a real hang still fails and load no longer does; the loop exits
/// the moment the condition holds, so a passing run costs nothing extra.
const PATIENCE: usize = 2_400;

async fn until(mut check: impl FnMut() -> bool, what: &str) {
    for _ in 0..PATIENCE {
        if check() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("timed out waiting for {what}");
}

/// An operator's desktop over mutual TLS: signs in, takes the snapshot, follows the
/// stream. The same link as over loopback, now spoken to a node that serves nothing in
/// the clear.
#[tokio::test(flavor = "multi_thread")]
async fn an_operator_links_over_mutual_tls() {
    let pki = Pki::new("operator");
    let url = serve(&pki, api()).await;
    let handle = tokio::runtime::Handle::current();
    let endpoint = RemoteEndpoint {
        url,
        tls: pki.client("desk-1"),
    };
    let (mut tracking, _intercept) =
        connect(&endpoint, credential(), &handle).expect("the link starts");
    until(
        || {
            tracking.poll(MissionTime(0.0));
            tracking.is_healthy()
        },
        "the link over TLS to come up",
    )
    .await;
    assert_eq!(tracking.tracks().len(), 1);
}

/// A partner's machine link (GAP-009, GAP-065): no sign-in, the certificate is the
/// identity, and the agreement decides what arrives.
#[tokio::test(flavor = "multi_thread")]
async fn a_partner_links_as_a_machine_and_receives_its_agreement() {
    let pki = Pki::new("partner");
    let url = serve(&pki, api()).await;
    let handle = tokio::runtime::Handle::current();
    let endpoint = RemoteEndpoint {
        url,
        tls: pki.client("partner"),
    };
    let peer = PeerLink::connect(&endpoint, &handle).expect("the peer link starts");
    until(|| peer.connected(), "the partner's link to come up").await;
    let mut got = peer.take_tracks();
    // Same guard as `until` above: one second was enough on this machine and is not a
    // claim about a loaded one.
    for _ in 0..PATIENCE {
        if !got.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        got = peer.take_tracks();
    }
    assert_eq!(
        got.len(),
        1,
        "the snapshot's track reaches the peer adapter"
    );
    assert!(
        peer.take_tracks().is_empty(),
        "a track is handed over once, not on every poll"
    );
}

/// A certificate is who you are; the agreement is what you may have (DN-18). A machine
/// with no agreement is refused, and the link says so rather than reporting connected.
#[tokio::test(flavor = "multi_thread")]
async fn a_machine_with_no_agreement_is_refused_and_says_why() {
    let pki = Pki::new("stranger");
    let url = serve(&pki, api()).await;
    let handle = tokio::runtime::Handle::current();
    let endpoint = RemoteEndpoint {
        url,
        tls: pki.client("stranger"),
    };
    let peer = PeerLink::connect(&endpoint, &handle).expect("the peer link starts");
    until(
        || peer.last_error().is_some(),
        "the stranger's link to be refused",
    )
    .await;
    assert!(!peer.connected());
    let reason = peer.last_error().unwrap_or_default();
    assert!(reason.contains("403"), "{reason}");
}

/// A machine link with no certificate is nobody, and is refused before it starts.
#[test]
fn a_machine_link_needs_an_identity() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let endpoint = RemoteEndpoint {
        url: "https://node.local:7410".into(),
        tls: LinkTls {
            trust_roots_pem: vec!["-----BEGIN CERTIFICATE-----".into()],
            issued: None,
            identity_pem: None,
        },
    };
    let err = PeerLink::connect(&endpoint, runtime.handle()).expect_err("refused");
    assert!(err.to_string().contains("client certificate"), "{err}");
}
