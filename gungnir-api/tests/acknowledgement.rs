// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The warned party's answer (GAP-042, DN-03 §5 rule 2): who may acknowledge a warning
//! over the v2 transport, and what the node does with it.
//!
//! The rule this file exists to hold: a certificate acknowledges only what it speaks for.
//! An effector's certificate is admitted to `POST /v2/handoffs/{id}/report` and refused
//! here, because an effector acts on decisions and a warned party is told about threats;
//! one certificate serving both would be able to discharge warnings nobody sent it.
//!
//! The certificates are made here with `rcgen` (dev-only, D-22) and never written to the
//! repository.

use gungnir_api::tls::{self, TlsListener, TlsPaths};
use gungnir_api::transport::{serve_on_listener, AccountTokenAuthority, MachineRole, NodeApi};
use gungnir_api::v2::{SnapshotResponse, WarningAcknowledgementRequest};
use gungnir_model::{AssetId, MissionTime, SensorId, SystemHealth, TrackId};
use gungnir_security::{hash_passphrase, Account, InMemoryAccountStore, OperatorId, TokenIssuer};
use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, Issuer, KeyPair};
use std::sync::Arc;
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
            "gungnir-ack-{name}-{}-{}",
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
        for cert in rustls_pemfile::certs(&mut self.ca_pem.as_bytes()) {
            roots.add(cert.expect("pem")).expect("root");
        }
        roots
    }
}

const PASSPHRASE: &str = "correct horse battery staple";
const SUPERVISOR: u64 = 7;
const ADMINISTRATOR: u64 = 8;

/// A node that knows a supervisor and an administrator, and three machines: sensor 4's
/// receiver, the effector behind the `battery` endpoint, and the harbour master the
/// `port-authority` warning channel reaches.
fn api() -> Arc<NodeApi> {
    let store = InMemoryAccountStore::new(vec![
        Account {
            operator: OperatorId(SUPERVISOR),
            role: gungnir_security::Role::Supervisor,
            phc: hash_passphrase(PASSPHRASE).expect("hashed"),
        },
        Account {
            operator: OperatorId(ADMINISTRATOR),
            role: gungnir_security::Role::Administrator,
            phc: hash_passphrase(PASSPHRASE).expect("hashed"),
        },
    ]);
    let issuer = TokenIssuer::new(vec![5u8; 32], 300.0).expect("issuer");
    Arc::new(
        NodeApi::new(SnapshotResponse::new(
            Vec::new(),
            None,
            SystemHealth::default(),
            Vec::new(),
        ))
        .with_callers(Arc::new(AccountTokenAuthority::new(
            Box::new(store),
            issuer,
        )))
        .with_machine_identities(vec![
            ("radar-4".into(), MachineRole::Sensor(SensorId(4))),
            (
                "battery-1".into(),
                MachineRole::Effector {
                    endpoint: "battery".into(),
                },
            ),
            (
                "harbour-master-1".into(),
                MachineRole::WarnedParty {
                    channel: "port-authority".into(),
                },
            ),
        ]),
    )
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

/// One HTTP/1.1 request over mutual TLS as `party`.
async fn request(
    pki: &Pki,
    addr: std::net::SocketAddr,
    party: &str,
    method: &str,
    path: &str,
    bearer: Option<&str>,
    body: Option<String>,
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
    let body = body.unwrap_or_default();
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
        body.len()
    );
    if let Some(token) = bearer {
        use std::fmt::Write as _;
        let _ = write!(head, "Authorization: Bearer {token}\r\n");
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

fn acknowledgement(at: f64) -> String {
    serde_json::to_string(&WarningAcknowledgementRequest {
        at: MissionTime(at),
    })
    .expect("json")
}

/// Sign in and return the token.
async fn token(pki: &Pki, addr: std::net::SocketAddr, operator: u64) -> String {
    let (status, body) = request(
        pki,
        addr,
        "desk-1",
        "POST",
        "/v2/session",
        None,
        Some(format!(
            "{{\"operator\":{operator},\"passphrase\":\"{PASSPHRASE}\"}}"
        )),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    serde_json::from_str::<serde_json::Value>(&body).expect("json")["token"]
        .as_str()
        .expect("token")
        .to_owned()
}

/// GAP-042: the warning channel's certificate acknowledges, the node queues the fact for
/// the desktop that raised the warning, and no other machine may.
#[tokio::test(flavor = "multi_thread")]
async fn a_warning_channel_certificate_acknowledges_and_no_other_machine_may() {
    let pki = Pki::new("channel");
    let api = api();
    let addr = serve(&pki, Arc::clone(&api)).await;

    let (status, body) = request(
        &pki,
        addr,
        "harbour-master-1",
        "POST",
        "/v2/warnings/1/7/acknowledge",
        None,
        Some(acknowledgement(12.0)),
    )
    .await;
    assert_eq!(status, 202, "{body}");
    let queued = api.take_warning_acknowledgements();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].asset, AssetId(1));
    assert_eq!(queued[0].track, TrackId(7));
    assert_eq!(
        queued[0].party, "port-authority",
        "the channel the certificate speaks for, not the certificate's own name"
    );
    assert_eq!(queued[0].at, MissionTime(12.0));
    assert!(
        api.take_warning_acknowledgements().is_empty(),
        "the queue is drained, not copied"
    );

    // An effector acts on decisions; it is told about no threats and discharges no
    // warnings.
    let (status, body) = request(
        &pki,
        addr,
        "battery-1",
        "POST",
        "/v2/warnings/1/7/acknowledge",
        None,
        Some(acknowledgement(12.0)),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    assert!(body.contains("not for a warned party"), "{body}");

    // And a sensor's certificate is refused for the same reason, with the same message.
    let (status, body) = request(
        &pki,
        addr,
        "radar-4",
        "POST",
        "/v2/warnings/1/7/acknowledge",
        None,
        Some(acknowledgement(12.0)),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    assert!(
        api.take_warning_acknowledgements().is_empty(),
        "a refused caller queued something"
    );

    // The reverse of the same rule: the warned party may not report on a handoff.
    let (status, body) = request(
        &pki,
        addr,
        "harbour-master-1",
        "POST",
        "/v2/handoffs/9/report",
        None,
        Some(
            serde_json::to_string(&gungnir_api::v2::EffectorReportRequest {
                report: gungnir_model::handoff::EffectorReport::Executing {
                    at: MissionTime(12.0),
                },
            })
            .expect("json"),
        ),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    assert!(body.contains("not for an effector"), "{body}");
    let _ = std::fs::remove_dir_all(&pki.dir);
}

/// GAP-042: an operator keying in what came over the radio needs the `warning.acknowledge`
/// action, and the record says which operator rather than which channel.
#[tokio::test(flavor = "multi_thread")]
async fn an_operator_needs_the_action_and_is_recorded_as_an_operator() {
    let pki = Pki::new("operator");
    let api = api();
    let addr = serve(&pki, Arc::clone(&api)).await;

    let supervisor = token(&pki, addr, SUPERVISOR).await;
    let (status, body) = request(
        &pki,
        addr,
        "desk-1",
        "POST",
        "/v2/warnings/1/7/acknowledge",
        Some(&supervisor),
        Some(acknowledgement(3.0)),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    assert!(body.contains("may not acknowledge a warning"), "{body}");
    assert!(api.take_warning_acknowledgements().is_empty());

    let administrator = token(&pki, addr, ADMINISTRATOR).await;
    let (status, body) = request(
        &pki,
        addr,
        "desk-1",
        "POST",
        "/v2/warnings/1/7/acknowledge",
        Some(&administrator),
        Some(acknowledgement(3.0)),
    )
    .await;
    assert_eq!(status, 202, "{body}");
    let queued = api.take_warning_acknowledgements();
    assert_eq!(queued.len(), 1);
    assert_eq!(
        queued[0].party,
        format!("operator:{ADMINISTRATOR}"),
        "a person keyed it in, and the record says so rather than naming a channel"
    );

    // A body that will not parse is refused before anything is recorded.
    let (status, body) = request(
        &pki,
        addr,
        "desk-1",
        "POST",
        "/v2/warnings/1/7/acknowledge",
        Some(&administrator),
        Some("{\"at\":\"noon\"}".to_string()),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    assert!(api.take_warning_acknowledgements().is_empty());

    // A certificate with no machine role and no token is refused where every other route
    // refuses it -- authenticated, and with no agreement saying what it may do (DN-18 §5)
    // -- rather than falling through to an unauthenticated acceptance.
    let (status, body) = request(
        &pki,
        addr,
        "desk-1",
        "POST",
        "/v2/warnings/1/7/acknowledge",
        None,
        Some(acknowledgement(3.0)),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    assert!(body.contains("no exchange agreement"), "{body}");
    assert!(api.take_warning_acknowledgements().is_empty());
    let _ = std::fs::remove_dir_all(&pki.dir);
}
