// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Machine identities on the routes that serve a sensor or an effector (D-02; GAP-002,
//! GAP-004, GAP-040): a certificate speaks for exactly what the baseline says it does.
//!
//! The certificates are made here with `rcgen` (dev-only, D-22) and never written to
//! the repository.

use gungnir_api::tls::{self, TlsListener, TlsPaths};
use gungnir_api::transport::{
    serve_on_listener, AccountTokenAuthority, MachineRole, NodeApi, PendingSensorTask,
};
use gungnir_api::v2::{
    EffectorReportRequest, SensorTaskRequest, SnapshotResponse, SubmitDetectionRequest,
};
use gungnir_model::handoff::EffectorReport;
use gungnir_model::{
    DetectionView, MissionTime, Provenance, SensorCommand, SensorId, SensorMode, SensorTaskId,
    SystemHealth,
};
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
            "gungnir-machine-{name}-{}-{}",
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

/// A node that knows one supervisor, and two machines: sensor 4's receiver and the
/// effector behind the `battery` endpoint.
fn api() -> Arc<NodeApi> {
    let store = InMemoryAccountStore::new(vec![Account {
        operator: OperatorId(7),
        role: gungnir_security::Role::Supervisor,
        phc: hash_passphrase(PASSPHRASE).expect("hashed"),
    }]);
    let issuer = TokenIssuer::new(vec![3u8; 32], 300.0).expect("issuer");
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

fn detection(sensor: u32) -> DetectionView {
    DetectionView {
        sensor: SensorId(sensor),
        source_time: MissionTime(1.0),
        receipt_time: MissionTime(1.5),
        measurement: gungnir_model::Measurement::Position {
            enu: nalgebra::Vector3::new(100.0, 200.0, 30.0),
            variance_m2: [400.0, 400.0, 900.0],
        },
        provenance: Provenance::default(),
    }
}

/// GAP-002: a sensor's certificate submits for its own id, and for no other.
#[tokio::test(flavor = "multi_thread")]
async fn a_sensor_certificate_submits_for_its_own_id_only() {
    let pki = Pki::new("sensor");
    let api = api();
    let addr = serve(&pki, Arc::clone(&api)).await;
    let own = serde_json::to_string(&SubmitDetectionRequest {
        schema_version: gungnir_model::SCHEMA_VERSION,
        detection: detection(4),
    })
    .expect("json");
    let (status, _) = request(
        &pki,
        addr,
        "radar-4",
        "POST",
        "/v2/detections",
        None,
        Some(own),
    )
    .await;
    assert_eq!(status, 202);
    let queued = api.take_machine_submissions();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].sensor, SensorId(4));
    assert!(
        api.take_submissions().is_empty(),
        "a machine's detection never enters the operators' queue"
    );

    let other = serde_json::to_string(&SubmitDetectionRequest {
        schema_version: gungnir_model::SCHEMA_VERSION,
        detection: detection(5),
    })
    .expect("json");
    let (status, body) = request(
        &pki,
        addr,
        "radar-4",
        "POST",
        "/v2/detections",
        None,
        Some(other),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    assert!(body.contains("speaks for sensor 4"), "{body}");
    assert!(api.take_machine_submissions().is_empty());

    // The effector's certificate is not a sensor's.
    let (status, body) = request(
        &pki,
        addr,
        "battery-1",
        "POST",
        "/v2/detections",
        None,
        Some(
            serde_json::to_string(&SubmitDetectionRequest {
                schema_version: gungnir_model::SCHEMA_VERSION,
                detection: detection(4),
            })
            .expect("json"),
        ),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(api.vouched_sensors(), vec![SensorId(4)]);
}

/// GAP-040: an effector reports on a handoff and the node queues it for the record; a
/// sensor's certificate may not.
#[tokio::test(flavor = "multi_thread")]
async fn an_effector_certificate_reports_and_a_sensor_may_not() {
    let pki = Pki::new("effector");
    let api = api();
    let addr = serve(&pki, Arc::clone(&api)).await;
    let report = serde_json::to_string(&EffectorReportRequest {
        report: EffectorReport::Executing {
            at: MissionTime(12.0),
        },
    })
    .expect("json");
    let (status, body) = request(
        &pki,
        addr,
        "battery-1",
        "POST",
        "/v2/handoffs/9/report",
        None,
        Some(report.clone()),
    )
    .await;
    assert_eq!(status, 202, "{body}");
    let queued = api.take_effector_reports();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].decision.0, 9);
    assert_eq!(queued[0].endpoint, "battery");
    assert_eq!(
        queued[0].report,
        EffectorReport::Executing {
            at: MissionTime(12.0)
        }
    );

    let (status, body) = request(
        &pki,
        addr,
        "radar-4",
        "POST",
        "/v2/handoffs/9/report",
        None,
        Some(report),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    assert!(api.take_effector_reports().is_empty());
}

/// GAP-004: an operator's task reaches the node loop's queue and the route answers with
/// the node's task id; a machine may not task a sensor.
#[tokio::test(flavor = "multi_thread")]
async fn an_operator_tasks_a_sensor_through_the_node_loop() {
    let pki = Pki::new("task");
    let api = api();
    let addr = serve(&pki, Arc::clone(&api)).await;
    // Sign in as the supervisor: a desktop on a mutual-TLS link still speaks for an
    // operator, and the certificate's party ("desk-1") has no machine role.
    let (status, body) = request(
        &pki,
        addr,
        "desk-1",
        "POST",
        "/v2/session",
        None,
        Some(format!(
            "{{\"operator\":7,\"passphrase\":\"{PASSPHRASE}\"}}"
        )),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let token = serde_json::from_str::<serde_json::Value>(&body).expect("json")["token"]
        .as_str()
        .expect("token")
        .to_owned();

    // The node loop: answer the one task with id 77.
    let loop_api = Arc::clone(&api);
    let answered = Arc::new(std::sync::Mutex::new(Vec::<PendingSensorTask>::new()));
    let seen = Arc::clone(&answered);
    tokio::spawn(async move {
        for _ in 0..400 {
            for task in loop_api.take_tasks() {
                assert_eq!(task.sensor, SensorId(4));
                assert!(matches!(
                    task.command,
                    SensorCommand::SetMode {
                        mode: SensorMode::Search
                    }
                ));
                let _ = task.reply.send(Ok(SensorTaskId(77)));
                seen.lock().expect("lock").push(PendingSensorTask {
                    sensor: task.sensor,
                    command: task.command,
                    requirement: task.requirement,
                    reply: tokio::sync::oneshot::channel().0,
                });
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    });
    let body = serde_json::to_string(&SensorTaskRequest {
        command: SensorCommand::SetMode {
            mode: SensorMode::Search,
        },
        requirement: None,
    })
    .expect("json");
    let (status, answer) = request(
        &pki,
        addr,
        "desk-1",
        "POST",
        "/v2/sensors/4/task",
        Some(&token),
        Some(body.clone()),
    )
    .await;
    assert_eq!(status, 202, "{answer}");
    assert!(answer.contains("77"), "{answer}");
    assert_eq!(answered.lock().expect("lock").len(), 1);

    // A machine may not task a sensor: the route is internal to the deployment.
    let (status, answer) = request(
        &pki,
        addr,
        "radar-4",
        "POST",
        "/v2/sensors/4/task",
        None,
        Some(body),
    )
    .await;
    assert_eq!(status, 403, "{answer}");
}

/// The refusal `docs/gungnir-api-v1.md`'s amended compatibility rule depends on.
///
/// That rule now requires a new **path** version only where a client that does not know
/// about a change could silently misinterpret a payload, and accepts a schema bump alone
/// where such a client is refused. **That is only honest if the refusal exists**, and
/// until 2026-09-06 no inbound path had one: a caller posting a previous shape was
/// refused where serde happened to be unable to read it and accepted where it could,
/// which is an accident of each particular change rather than a rule.
///
/// Three cases, because the interesting one is the third.
#[tokio::test(flavor = "multi_thread")]
async fn a_caller_speaking_another_schema_version_is_refused_by_name() {
    let pki = Pki::new("sensor");
    let api = api();
    let addr = serve(&pki, Arc::clone(&api)).await;

    // Ahead of this node.
    let ahead = serde_json::to_string(&SubmitDetectionRequest {
        schema_version: gungnir_model::SCHEMA_VERSION + 1,
        detection: detection(4),
    })
    .expect("json");
    let (status, body) = request(
        &pki,
        addr,
        "radar-4",
        "POST",
        "/v2/detections",
        None,
        Some(ahead),
    )
    .await;
    assert_eq!(status, 409, "a newer caller must be refused, not accepted");
    assert!(
        body.contains(&(gungnir_model::SCHEMA_VERSION + 1).to_string())
            && body.contains(&gungnir_model::SCHEMA_VERSION.to_string()),
        "the refusal must name both versions so the caller is not left guessing: {body}"
    );

    // Behind this node.
    let behind = serde_json::to_string(&SubmitDetectionRequest {
        schema_version: gungnir_model::SCHEMA_VERSION - 1,
        detection: detection(4),
    })
    .expect("json");
    let (status, _) = request(
        &pki,
        addr,
        "radar-4",
        "POST",
        "/v2/detections",
        None,
        Some(behind),
    )
    .await;
    assert_eq!(status, 409, "an older caller must be refused too");

    // **The case that matters.** A client written before the field existed sends no
    // version at all. It must be refused, not defaulted to the current one -- defaulting
    // would make every old client silently claim to be current, which is the opposite of
    // what the field is for, and it is the failure the whole guard exists to prevent.
    let silent = serde_json::json!({ "detection": detection(4) }).to_string();
    let (status, body) = request(
        &pki,
        addr,
        "radar-4",
        "POST",
        "/v2/detections",
        None,
        Some(silent),
    )
    .await;
    assert_eq!(
        status, 409,
        "a caller that states no version must be refused rather than assumed current"
    );
    assert!(
        body.contains(&gungnir_model::SCHEMA_VERSION.to_string()),
        "the refusal must say what this node speaks: {body}"
    );

    assert!(
        api.take_machine_submissions().is_empty(),
        "a refused detection must not reach the queue"
    );
}
