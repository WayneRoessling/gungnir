// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! `/v3`, the retired `/v2`, and an identifier in a path (GAP-130; D-56, D-60;
//! `docs/design/DN-31-node-approval-queue.md` §5.1 and amendment 1).
//!
//! When decision, plan and queue-item identifiers became UUID v7 written as strings, every
//! payload carrying one changed type and the interface moved to `/v3` whole. `/v2` stays
//! routed, and these tests hold what it answers: **each retired route authenticates its
//! caller exactly as its `/v3` successor does**, so a caller the successor would refuse is
//! refused the same way and in the same words, and only then is told `410 Gone` with the
//! successor's path; and the retired event stream answers before any upgrade, because its
//! token would travel in a frame it never reads.
//!
//! The retired surface is written out here route by route rather than read from the
//! transport's own table, so a route dropped from that table fails here instead of being
//! agreed with. The certificates are made with `rcgen` (dev-only, D-22) and never written
//! to the repository.

use gungnir_api::tls::{self, TlsListener, TlsPaths};
use gungnir_api::transport::{
    bind, serve_on, serve_on_listener, AccountTokenAuthority, MachineRole, NodeApi,
};
use gungnir_api::v3::{EffectorReportRequest, SnapshotResponse};
use gungnir_model::handoff::EffectorReport;
use gungnir_model::{
    DecisionId, ExchangeAgreement, ExchangeFormat, ExchangeItem, ExchangeSet, MissionTime,
    SensorId, SystemHealth,
};
use gungnir_security::{hash_passphrase, Account, InMemoryAccountStore, OperatorId, TokenIssuer};
use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, Issuer, KeyPair};
use rustls::pki_types::pem::PemObject;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Makes every scratch directory in this binary its own (see `machine.rs` for why a
/// process id alone is not enough).
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
            "gungnir-retired-{name}-{}-{}",
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

    fn client(&self, party: &str) -> (Vec<u8>, Vec<u8>) {
        let key = KeyPair::generate().expect("client key");
        let mut params = CertificateParams::new(Vec::<String>::new()).expect("params");
        params.distinguished_name.push(DnType::CommonName, party);
        let cert = params.signed_by(&key, &self.issuer()).expect("client cert");
        (cert.der().to_vec(), key.serialize_der())
    }

    fn roots(&self) -> rustls::RootCertStore {
        let mut roots = rustls::RootCertStore::empty();
        for cert in rustls::pki_types::CertificateDer::pem_slice_iter(self.ca_pem.as_bytes()) {
            roots.add(cert.expect("pem")).expect("root");
        }
        roots
    }
}

const PASSPHRASE: &str = "correct horse battery staple";
const SUPERVISOR: u64 = 7;

/// A v7-shaped decision identifier and its written form.
const V7: u128 = 0x0199_5a3b_7c2d_7e4f_8a1b_2c3d_9f3a_61c2;
const V7_TEXT: &str = "01995a3b-7c2d-7e4f-8a1b-2c3d9f3a61c2";

/// A node that knows a supervisor, an effector's certificate (`battery-1`), and one party
/// with an exchange agreement (`sector-north`).
fn api() -> Arc<NodeApi> {
    let store = InMemoryAccountStore::new(vec![Account {
        operator: OperatorId(SUPERVISOR),
        role: gungnir_security::Role::Supervisor,
        phc: hash_passphrase(PASSPHRASE).expect("hashed"),
    }]);
    let issuer = TokenIssuer::new(vec![4u8; 32], 300.0).expect("issuer");
    Arc::new(
        NodeApi::new(SnapshotResponse::new(
            Vec::new(),
            None,
            SystemHealth::default(),
            Vec::new(),
        ))
        .with_exchange(ExchangeSet {
            agreements: vec![ExchangeAgreement {
                party: "sector-north".into(),
                inbound: Vec::new(),
                outbound: vec![
                    ExchangeItem::Tracks,
                    ExchangeItem::Health,
                    ExchangeItem::Warnings,
                    ExchangeItem::Reports,
                    ExchangeItem::Handoffs,
                ],
                format: ExchangeFormat::Canonical,
            }],
        })
        .with_callers(Arc::new(AccountTokenAuthority::new(
            Box::new(store),
            issuer,
        )))
        .with_machine_identities(vec![
            (
                "battery-1".into(),
                MachineRole::Effector {
                    endpoint: "battery".into(),
                },
            ),
            ("radar-4".into(), MachineRole::Sensor(SensorId(4))),
        ]),
    )
}

/// How a retired route's successor establishes who is calling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Door {
    /// Signing in: nothing to authenticate.
    SignIn,
    /// An operator's token or a party with an agreement.
    Caller,
    /// An operator's token alone.
    Operator,
    /// A machine identity, or else an operator's token.
    MachineOrOperator,
    /// The event stream.
    Stream,
}

/// Every route `/v2` served on 2026-09-17: method, path below the version, and the door.
const RETIRED: [(&str, &str, Door); 18] = [
    ("POST", "/session", Door::SignIn),
    ("GET", "/session", Door::Operator),
    ("GET", "/snapshot", Door::Caller),
    ("GET", "/health", Door::Caller),
    ("GET", "/coverage", Door::Operator),
    ("GET", "/events", Door::Stream),
    ("GET", "/history?since_seq=0", Door::Caller),
    ("POST", "/detections", Door::MachineOrOperator),
    ("POST", "/sensors/4/task", Door::Operator),
    ("POST", "/handoffs/9/report", Door::MachineOrOperator),
    ("POST", "/warnings/1/7/acknowledge", Door::MachineOrOperator),
    ("GET", "/exchange/warnings", Door::Caller),
    ("POST", "/exchange/warnings", Door::Operator),
    ("GET", "/exchange/reports", Door::Caller),
    ("POST", "/exchange/reports", Door::Operator),
    ("GET", "/exchange/handoffs", Door::Caller),
    ("POST", "/exchange/handoffs", Door::Operator),
    ("POST", "/plans/1/decision", Door::Operator),
];

/// Write one HTTP/1.1 request and read the whole answer: the status and the body.
async fn exchange<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> (u16, String) {
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (name, value) in headers {
        use std::fmt::Write as _;
        let _ = write!(head, "{name}: {value}\r\n");
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await.expect("write");
    stream.write_all(body.as_bytes()).await.expect("write body");
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

/// A node on loopback in the clear, where a caller carries no certificate.
async fn serve_plain(api: Arc<NodeApi>) -> std::net::SocketAddr {
    let listener = bind("127.0.0.1:0".parse().expect("address"))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = serve_on(listener, api).await;
    });
    addr
}

async fn plain(
    addr: std::net::SocketAddr,
    method: &str,
    path: &str,
    token: Option<&str>,
) -> (u16, String) {
    let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let bearer = token.map(|t| format!("Bearer {t}"));
    let headers: Vec<(&str, &str)> = bearer
        .as_deref()
        .map(|b| vec![("Authorization", b)])
        .unwrap_or_default();
    exchange(&mut stream, method, path, &headers, "").await
}

/// A node serving mutual TLS, where every caller is the party its certificate names.
async fn serve_tls(pki: &Pki, api: Arc<NodeApi>) -> std::net::SocketAddr {
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

async fn as_party(
    pki: &Pki,
    addr: std::net::SocketAddr,
    party: &str,
    method: &str,
    path: &str,
    body: &str,
) -> (u16, String) {
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
    exchange(&mut stream, method, path, &[], body).await
}

async fn sign_in(addr: std::net::SocketAddr) -> String {
    let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let (status, body) = exchange(
        &mut stream,
        "POST",
        "/v3/session",
        &[],
        &format!("{{\"operator\":{SUPERVISOR},\"passphrase\":\"{PASSPHRASE}\"}}"),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    serde_json::from_str::<serde_json::Value>(&body).expect("json")["token"]
        .as_str()
        .expect("token")
        .to_owned()
}

/// The problem a retired route answers with, checked for the successor it names.
fn assert_gone(route: &str, status: u16, body: &str) {
    assert_eq!(status, 410, "/v2{route} was not gone: {body}");
    let problem: serde_json::Value =
        serde_json::from_str(body).unwrap_or_else(|e| panic!("/v2{route}: {e}: {body}"));
    let below = route.split('?').next().unwrap_or(route);
    let successor = format!("/v3{below}");
    assert_eq!(
        problem["successor"].as_str(),
        Some(successor.as_str()),
        "{body}"
    );
    assert!(
        problem["message"]
            .as_str()
            .is_some_and(|m| m.contains(&successor) && m.contains("D-56")),
        "the message does not name the successor and why: {body}"
    );
}

/// **An operator's session is told where every route went, and nobody else is told
/// anything** (GAP-130). Without a token, each retired route refuses exactly as its
/// successor refuses -- `401`, the same status on both -- apart from signing in and the
/// event stream, which authenticate nobody and are gone to everyone. With a token, all of
/// them are gone, each naming its own successor.
#[tokio::test(flavor = "multi_thread")]
async fn a_retired_route_authenticates_as_its_successor_and_then_names_it() {
    let addr = serve_plain(api()).await;

    for (method, route, door) in RETIRED {
        let (v2, v2_body) = plain(addr, method, &format!("/v2{route}"), None).await;
        match door {
            Door::SignIn | Door::Stream => assert_gone(route, v2, &v2_body),
            Door::Caller | Door::Operator | Door::MachineOrOperator => {
                let (v3, v3_body) = plain(addr, method, &format!("/v3{route}"), None).await;
                assert_eq!(
                    (v2, v2_body.clone()),
                    (v3, v3_body),
                    "{method} /v2{route} did not refuse an unauthenticated caller as its \
                     successor does"
                );
                assert_eq!(v2, 401, "{method} /v2{route}: {v2_body}");
            }
        }
    }

    let token = sign_in(addr).await;
    for (method, route, _) in RETIRED {
        let (status, body) = plain(addr, method, &format!("/v2{route}"), Some(&token)).await;
        assert_gone(route, status, &body);
    }

    // A route `/v2` never had is not found there, rather than gone.
    let (status, _) = plain(addr, "GET", "/v2/queue", Some(&token)).await;
    assert_eq!(status, 404);
}

/// **A machine is refused where its successor refuses it, in the successor's words, and
/// told where the rest went** (GAP-130). A party with an exchange agreement reaches the
/// read routes and is told they moved; the routes internal to the deployment refuse it on
/// both versions alike; an effector's certificate is told its report route moved; and a
/// party with neither an agreement nor an identity is refused everywhere a caller is
/// resolved.
#[tokio::test(flavor = "multi_thread")]
async fn a_machine_is_refused_as_its_successor_would_refuse_it() {
    let pki = Pki::new("machine");
    let addr = serve_tls(&pki, api()).await;

    for (method, route, door) in RETIRED {
        let (v2, v2_body) = as_party(
            &pki,
            addr,
            "sector-north",
            method,
            &format!("/v2{route}"),
            "",
        )
        .await;
        match door {
            Door::SignIn | Door::Stream | Door::Caller => assert_gone(route, v2, &v2_body),
            Door::Operator | Door::MachineOrOperator => {
                let (v3, v3_body) = as_party(
                    &pki,
                    addr,
                    "sector-north",
                    method,
                    &format!("/v3{route}"),
                    "",
                )
                .await;
                assert_eq!(v2, 403, "{method} /v2{route}: {v2_body}");
                assert_eq!(
                    (v2, v2_body),
                    (v3, v3_body),
                    "{method} /v2{route} refused a party differently from its successor"
                );
            }
        }
    }

    let report = serde_json::to_string(&EffectorReportRequest {
        report: EffectorReport::Acknowledged {
            at: MissionTime(3.0),
        },
    })
    .expect("json");
    let (status, body) = as_party(
        &pki,
        addr,
        "battery-1",
        "POST",
        "/v2/handoffs/9/report",
        &report,
    )
    .await;
    assert_gone("/handoffs/9/report", status, &body);
    let (status, body) = as_party(
        &pki,
        addr,
        "battery-1",
        "POST",
        "/v3/handoffs/9/report",
        &report,
    )
    .await;
    assert_eq!(
        status, 202,
        "the successor refused what the retired route pointed at: {body}"
    );

    for route in ["/snapshot", "/health", "/exchange/handoffs"] {
        let (v2, v2_body) = as_party(&pki, addr, "nobody", "GET", &format!("/v2{route}"), "").await;
        let (v3, v3_body) = as_party(&pki, addr, "nobody", "GET", &format!("/v3{route}"), "").await;
        assert_eq!(v2, 403, "/v2{route}: {v2_body}");
        assert_eq!((v2, v2_body), (v3, v3_body), "/v2{route}");
    }
}

/// The retired event stream is gone **before** any upgrade: its token would travel in the
/// first frame after the upgrade, which a retired route never reads, so there is nothing to
/// authenticate and nothing to upgrade to.
#[tokio::test(flavor = "multi_thread")]
async fn the_retired_event_stream_is_gone_before_any_upgrade() {
    let addr = serve_plain(api()).await;
    let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let (status, body) = exchange(
        &mut stream,
        "GET",
        "/v2/events",
        &[
            ("Upgrade", "websocket"),
            ("Connection", "Upgrade"),
            ("Sec-WebSocket-Version", "13"),
            ("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ=="),
        ],
        "",
    )
    .await;
    assert_ne!(status, 101, "a retired stream upgraded");
    assert_gone("/events", status, &body);
}

/// **D-60 in a path**: the report route reads a decision in the hyphenated form a desktop
/// writes, in upper case too, and in the decimal form a client written before GAP-130
/// sends; any other text is a `400` problem naming both forms, and nothing is queued for
/// it.
#[tokio::test(flavor = "multi_thread")]
async fn a_decision_in_the_report_path_reads_in_either_written_form_and_nothing_else() {
    let pki = Pki::new("path");
    let api = api();
    let addr = serve_tls(&pki, Arc::clone(&api)).await;
    let report = serde_json::to_string(&EffectorReportRequest {
        report: EffectorReport::Executing {
            at: MissionTime(5.0),
        },
    })
    .expect("json");

    for (text, expected) in [
        (V7_TEXT.to_owned(), DecisionId(V7)),
        (V7_TEXT.to_uppercase(), DecisionId(V7)),
        ("7".to_owned(), DecisionId(7)),
    ] {
        let (status, body) = as_party(
            &pki,
            addr,
            "battery-1",
            "POST",
            &format!("/v3/handoffs/{text}/report"),
            &report,
        )
        .await;
        assert_eq!(status, 202, "{text}: {body}");
        let queued = api.take_effector_reports();
        assert_eq!(queued.len(), 1, "{text}");
        assert_eq!(queued[0].decision, expected, "{text}");
    }

    for text in ["not-a-decision", "01995a3b7c2d7e4f8a1b2c3d9f3a61c2", "-7"] {
        let (status, body) = as_party(
            &pki,
            addr,
            "battery-1",
            "POST",
            &format!("/v3/handoffs/{text}/report"),
            &report,
        )
        .await;
        assert_eq!(status, 400, "{text}: {body}");
        let problem: serde_json::Value = serde_json::from_str(&body).unwrap_or_else(|e| {
            panic!("{text}: the refusal is not a readable problem: {e}: {body}")
        });
        let message = problem["message"].as_str().unwrap_or_default();
        assert!(
            message.contains("hyphenated") && message.contains("decimal"),
            "{text}: the refusal does not name both forms: {body}"
        );
        assert!(api.take_effector_reports().is_empty(), "{text} was queued");
    }
}
