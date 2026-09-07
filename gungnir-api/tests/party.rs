// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A party-bearing caller (GAP-062, GAP-065; DN-17 §5, DN-18 §5): a peer that presents a
//! client certificate the node's authority signed is a machine caller whose party is
//! the certificate's subject, and what it receives is decided by the agreement and the
//! marking together. A party with no agreement is refused; an operator inside the
//! deployment sees everything, as before.
//!
//! The certificates are made here with `rcgen` (dev-only, D-22) and never written to
//! the repository. The client is `tokio-rustls` speaking HTTP/1.1 by hand, so no client
//! crate joins the API's dependencies for a test.

use std::sync::Arc;

use gungnir_api::tls::{self, TlsListener, TlsPaths};
use gungnir_api::transport::{serve_on_listener, NodeApi};
use gungnir_api::v2::SnapshotResponse;
use gungnir_model::{
    Classification, ExchangeAgreement, ExchangeFormat, ExchangeItem, ExchangeSet, MissionTime,
    Provenance, Quality, Releasability, SystemHealth, TrackId, TrackStatus, TrackView,
};
use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, Issuer, KeyPair};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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
            "gungnir-party-{name}-{}-{}",
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

    /// A server identity for localhost, written to files the node reads.
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

    /// A client identity whose subject common name is `party`.
    fn client(&self, party: &str) -> (Vec<u8>, Vec<u8>) {
        let key = KeyPair::generate().expect("client key");
        let mut params = CertificateParams::new(Vec::<String>::new()).expect("params");
        params.distinguished_name.push(DnType::CommonName, party);
        let cert = params.signed_by(&key, &self.issuer()).expect("client cert");
        (cert.der().to_vec(), key.serialize_der())
    }

    fn roots(&self) -> rustls::RootCertStore {
        let mut roots = rustls::RootCertStore::empty();
        for cert in rustls_pemfile::certs(&mut self.ca_pem.as_bytes()) {
            roots.add(cert.expect("pem")).expect("root");
        }
        roots
    }
}

fn track(id: u64, releasability: Releasability) -> TrackView {
    TrackView {
        id: TrackId(id),
        status: TrackStatus::Confirmed,
        state: nalgebra::SVector::zeros(),
        covariance: nalgebra::SMatrix::identity(),
        classification: Classification::Unknown,
        provenance: Provenance::default(),
        quality: Quality::default(),
        mission_time: MissionTime(1.0),
        releasability,
    }
}

async fn serve(pki: &Pki, api: Arc<NodeApi>) -> std::net::SocketAddr {
    let acceptor = tls::acceptor(&pki.server_paths()).expect("acceptor");
    let tcp = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = tcp.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = serve_on_listener(TlsListener::new(tcp, acceptor), api).await;
    });
    addr
}

/// One HTTP/1.1 request over mutual TLS as `party`; the status and the body.
async fn get(pki: &Pki, addr: std::net::SocketAddr, party: &str, path: &str) -> (u16, String) {
    let (cert, key) = pki.client(party);
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(pki.roots())
        .with_client_auth_cert(
            vec![rustls::pki_types::CertificateDer::from(cert)],
            rustls::pki_types::PrivateKeyDer::try_from(key).expect("key der"),
        )
        .expect("client config");
    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    let tcp = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let name = rustls::pki_types::ServerName::try_from("localhost").expect("name");
    let mut stream = connector.connect(name, tcp).await.expect("handshake");
    let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.expect("write");
    let mut raw = Vec::new();
    let _ = stream.read_to_end(&mut raw).await;
    let text = String::from_utf8_lossy(&raw).into_owned();
    let status: u16 = text
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .expect("status line");
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    (status, body)
}

fn api_with(agreements: Vec<ExchangeAgreement>) -> Arc<NodeApi> {
    let snapshot = SnapshotResponse::new(
        vec![
            track(1, Releasability::Internal),
            track(2, Releasability::parties(["sector-north"])),
            track(3, Releasability::AllPeers),
        ],
        Some(gungnir_model::PlanView::default()),
        SystemHealth {
            tracking_healthy: true,
            ..SystemHealth::default()
        },
        Vec::new(),
    );
    Arc::new(NodeApi::new(snapshot).with_exchange(ExchangeSet { agreements }))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_party_receives_what_its_agreement_and_the_markings_release() {
    let pki = Pki::new("release");
    let api = api_with(vec![ExchangeAgreement {
        party: "sector-north".into(),
        inbound: Vec::new(),
        outbound: vec![ExchangeItem::Tracks],
        format: ExchangeFormat::Canonical,
    }]);
    let addr = serve(&pki, api).await;
    let (status, body) = get(&pki, addr, "sector-north", "/v2/snapshot").await;
    assert_eq!(status, 200, "{body}");
    let snapshot: SnapshotResponse = serde_json::from_str(&body).expect("snapshot");
    let ids: Vec<u64> = snapshot.tracks.iter().map(|t| t.id.0).collect();
    assert_eq!(ids, vec![2, 3], "the internal track stays home");
    assert!(snapshot.plan.is_none(), "a plan is not an exchange item");
    // One track, one plan, and health (not in the agreement) were withheld, and the
    // response says so.
    assert_eq!(snapshot.withheld, 3);
    assert!(
        !snapshot.health.tracking_healthy,
        "health withheld reads as no claim"
    );

    // Health is its own item; coverage is internal to the deployment.
    let (status, _) = get(&pki, addr, "sector-north", "/v2/health").await;
    assert_eq!(status, 403);
    let (status, _) = get(&pki, addr, "sector-north", "/v2/coverage").await;
    assert_eq!(status, 403);
    let _ = std::fs::remove_dir_all(&pki.dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_party_with_no_agreement_is_refused_and_a_health_agreement_sends_health() {
    let pki = Pki::new("refuse");
    let api = api_with(vec![ExchangeAgreement {
        party: "sector-east".into(),
        inbound: Vec::new(),
        outbound: vec![ExchangeItem::Health],
        format: ExchangeFormat::Canonical,
    }]);
    let addr = serve(&pki, api).await;
    // Authenticated, no agreement: nothing (DN-18 §5).
    let (status, body) = get(&pki, addr, "nobody", "/v2/snapshot").await;
    assert_eq!(status, 403, "{body}");
    assert!(body.contains("no exchange agreement"), "{body}");
    // An agreement that sends health and no tracks.
    let (status, body) = get(&pki, addr, "sector-east", "/v2/health").await;
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("tracking_healthy\":true"), "{body}");
    let (status, body) = get(&pki, addr, "sector-east", "/v2/snapshot").await;
    assert_eq!(status, 200);
    let snapshot: SnapshotResponse = serde_json::from_str(&body).expect("snapshot");
    assert!(snapshot.tracks.is_empty());
    assert_eq!(snapshot.withheld, 4, "three tracks and a plan");
    // The write paths stay an operator's.
    let (status, _) = get(&pki, addr, "sector-east", "/v2/plans/1/decision").await;
    assert!(status == 403 || status == 405, "{status}");
    let _ = std::fs::remove_dir_all(&pki.dir);
}

/// The subject reader on a certificate `rcgen` made, and on things that are not one.
#[test]
fn the_subject_common_name_is_read_from_the_certificate() {
    let pki = Pki::new("cn");
    let (cert, _) = pki.client("sector-north");
    assert_eq!(
        tls::subject_common_name(&cert).as_deref(),
        Some("sector-north")
    );
    assert_eq!(tls::subject_common_name(&[]), None);
    assert_eq!(tls::subject_common_name(&[0x30, 0x82, 0xFF]), None);
    assert_eq!(tls::subject_common_name(&cert[..cert.len() / 2]), None);
    let _ = std::fs::remove_dir_all(&pki.dir);
}
