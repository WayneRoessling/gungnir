// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Mutual TLS, verified against a real handshake (GAP-060, D-02).
//!
//! **Certificates are generated here and never checked in.** A private key in the
//! repository is key material in the repository whatever the comment above it says, which
//! is why D-22 admitted `rcgen` as a development dependency and nothing else.
//!
//! The property that matters is the one a configuration mistake would silently lose: a
//! client without a certificate, or with one from another authority, **must not get in**.
//! One-way TLS looks identical from the server's logs and authenticates nobody to it.

use gungnir_api::tls::{acceptor, TlsListener, TlsPaths};
use gungnir_api::transport::{serve_on_listener, NodeApi};
use gungnir_api::v3::SnapshotResponse;
use gungnir_model::SystemHealth;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::{ClientConfig, RootCertStore};
use std::sync::Arc;

/// One authority, and the certificates it signs.
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
    ca_pem: String,
    params: rcgen::CertificateParams,
    key: rcgen::KeyPair,
}

impl Pki {
    fn new(name: &str) -> Self {
        let mut params = rcgen::CertificateParams::new(vec![name.to_owned()]).expect("params");
        params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        let key = rcgen::KeyPair::generate().expect("key");
        let cert = params.self_signed(&key).expect("self-signed");
        Self {
            ca_pem: cert.pem(),
            params,
            key,
        }
    }

    /// A certificate this authority signs, as (certificate PEM, key PEM).
    ///
    /// **`name` is the subject common name as well as the subject alternative name.**
    /// `CertificateParams::new` sets only the latter, and rcgen then names the subject
    /// "rcgen self signed cert" by default -- so every certificate this helper issued
    /// carried that name, and a node reading the party off one saw it rather than the
    /// name asked for here. Nothing noticed until GAP-141 compared a party with a name a
    /// desktop gave itself. `gungnir_remote::identity::issue` has always set the common
    /// name; this is the test authority catching up with it.
    fn issue(&self, name: &str) -> (String, String) {
        let mut params = rcgen::CertificateParams::new(vec![name.to_owned()]).expect("params");
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, name);
        let key = rcgen::KeyPair::generate().expect("key");
        let issuer = rcgen::Issuer::from_params(&self.params, &self.key);
        let cert = params.signed_by(&key, &issuer).expect("signed");
        (cert.pem(), key.serialize_pem())
    }
}

fn write(dir: &std::path::Path, name: &str, contents: &str) -> String {
    let path = dir.join(name);
    std::fs::write(&path, contents).expect("written");
    path.to_string_lossy().into_owned()
}

fn api() -> Arc<NodeApi> {
    Arc::new(NodeApi::new(SnapshotResponse::new(
        Vec::new(),
        None,
        SystemHealth::default(),
        Vec::new(),
    )))
}

/// Build a client that presents `identity`, trusting `roots`.
fn client_config(roots_pem: &str, identity: Option<(&str, &str)>) -> ClientConfig {
    let mut roots = RootCertStore::empty();
    for cert in CertificateDer::pem_slice_iter(roots_pem.as_bytes()) {
        roots.add(cert.expect("a certificate")).expect("added");
    }
    let builder = ClientConfig::builder().with_root_certificates(roots);
    match identity {
        None => builder.with_no_client_auth(),
        Some((cert_pem, key_pem)) => {
            let certs: Vec<CertificateDer<'static>> =
                CertificateDer::pem_slice_iter(cert_pem.as_bytes())
                    .collect::<Result<_, _>>()
                    .expect("certificates");
            let key: PrivateKeyDer<'static> =
                PrivateKeyDer::from_pem_slice(key_pem.as_bytes()).expect("a key");
            builder.with_client_auth_cert(certs, key).expect("identity")
        }
    }
}

/// Start a mutual-TLS node serving `api`. Returns its address and the authority that
/// signs clients.
async fn serve_tls_with(
    dir: &std::path::Path,
    api: Arc<NodeApi>,
) -> (std::net::SocketAddr, Pki, String) {
    let server_pki = Pki::new("localhost");
    let (server_cert, server_key) = server_pki.issue("localhost");
    let client_pki = Pki::new("gungnir-client-ca");

    let paths = TlsPaths {
        certificate_chain: write(dir, "server.pem", &server_cert),
        private_key: write(dir, "server.key", &server_key),
        client_ca: write(dir, "client-ca.pem", &client_pki.ca_pem),
    };
    let acceptor = acceptor(&paths).expect("an acceptor");

    let tcp = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bound");
    let addr = tcp.local_addr().expect("addr");
    let listener = TlsListener::new(tcp, acceptor);
    tokio::spawn(async move {
        let _ = serve_on_listener(listener, api).await;
    });
    (addr, client_pki, server_pki.ca_pem)
}

/// Start a mutual-TLS node. Returns its address and the authority that signs clients.
#[allow(dead_code)]
async fn serve_tls(dir: &std::path::Path) -> (std::net::SocketAddr, Pki, String) {
    let server_pki = Pki::new("localhost");
    let (server_cert, server_key) = server_pki.issue("localhost");
    let client_pki = Pki::new("gungnir-client-ca");

    let paths = TlsPaths {
        certificate_chain: write(dir, "server.pem", &server_cert),
        private_key: write(dir, "server.key", &server_key),
        client_ca: write(dir, "client-ca.pem", &client_pki.ca_pem),
    };
    let acceptor = acceptor(&paths).expect("an acceptor");

    let tcp = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bound");
    let addr = tcp.local_addr().expect("addr");
    let listener = TlsListener::new(tcp, acceptor);
    tokio::spawn(async move {
        let _ = serve_on_listener(listener, api()).await;
    });
    (addr, client_pki, server_pki.ca_pem)
}

/// Connect, then **actually use the connection**, and report whether the node answered.
///
/// Completing the handshake is not the test. Under TLS 1.3 the client finishes before the
/// server has validated its certificate, so `connect` returning `Ok` says nothing about
/// whether the node accepted the peer -- an earlier version of this file asserted on that
/// and passed while an anonymous client was being let in. The rejection arrives on the
/// first read, so the request has to be made.
async fn answers(addr: std::net::SocketAddr, config: ClientConfig) -> bool {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    let Ok(stream) = tokio::net::TcpStream::connect(addr).await else {
        return false;
    };
    let name = ServerName::try_from("localhost").expect("a name");
    let exchange = async {
        let mut tls = connector.connect(name, stream).await.ok()?;
        // Built from bytes so the literal carries no escapes: CRLF is 13, 10.
        let request = [
            b"GET /v3/health HTTP/1.1".as_slice(),
            &[13, 10],
            b"Host: localhost".as_slice(),
            &[13, 10, 13, 10],
        ]
        .concat();
        tls.write_all(&request).await.ok()?;
        let mut buffer = [0u8; 16];
        let read = tls.read(&mut buffer).await.ok()?;
        (read > 0).then_some(())
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), exchange)
        .await
        .is_ok_and(|r| r.is_some())
}

/// A client with a certificate from the authority the node trusts gets in, and the
/// node answers it.
#[tokio::test(flavor = "multi_thread")]
async fn a_client_the_node_trusts_completes_the_handshake() {
    let dir = tempdir("trusted");
    let (addr, client_pki, server_ca) = serve_tls(&dir).await;
    let (cert, key) = client_pki.issue("desktop-1");

    assert!(
        answers(addr, client_config(&server_ca, Some((&cert, &key)))).await,
        "a properly issued client was refused"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// **The property mutual TLS exists for.** A client with no certificate is refused, so
/// the node authenticates its peers and not only itself to them.
#[tokio::test(flavor = "multi_thread")]
async fn a_client_with_no_certificate_is_refused() {
    let dir = tempdir("anonymous");
    let (addr, _client_pki, server_ca) = serve_tls(&dir).await;

    assert!(
        !answers(addr, client_config(&server_ca, None)).await,
        "the node accepted an anonymous client; this is one-way TLS, not mutual"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// A certificate from a different authority is refused. Without this, anyone able to
/// mint their own certificate would be a peer.
#[tokio::test(flavor = "multi_thread")]
async fn a_client_from_another_authority_is_refused() {
    let dir = tempdir("stranger");
    let (addr, _client_pki, server_ca) = serve_tls(&dir).await;

    let stranger = Pki::new("some-other-ca");
    let (cert, key) = stranger.issue("desktop-1");
    assert!(
        !answers(addr, client_config(&server_ca, Some((&cert, &key)))).await,
        "a certificate from an untrusted authority was accepted"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// A certificate and a key that do not belong together are refused at start-up, rather
/// than at the first handshake when a node is already reporting itself healthy.
#[test]
fn a_mismatched_certificate_and_key_are_refused_at_startup() {
    let dir = tempdir("mismatch");
    let pki = Pki::new("localhost");
    let (cert, _) = pki.issue("localhost");
    let (_, other_key) = pki.issue("localhost");

    let paths = TlsPaths {
        certificate_chain: write(&dir, "server.pem", &cert),
        private_key: write(&dir, "server.key", &other_key),
        client_ca: write(&dir, "client-ca.pem", &pki.ca_pem),
    };
    assert!(
        acceptor(&paths).is_err(),
        "a certificate and an unrelated key were accepted"
    );
    let _ = std::fs::remove_dir_all(dir);
}

fn tempdir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-mtls-{name}-{}-{}",
        std::process::id(),
        scratch_id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("created");
    dir
}

// ---------------------------------------------------------------------------------
// GAP-141: a forwarded batch is forwarded by the machine that took it
// ---------------------------------------------------------------------------------

const PASSPHRASE: &str = "correct horse battery staple";
const OPERATOR: u64 = 7;

/// A node that authenticates one operator, for the routes that need a token.
fn api_with_accounts() -> Arc<NodeApi> {
    use gungnir_security::{
        hash_passphrase, Account, InMemoryAccountStore, OperatorId, TokenIssuer,
    };
    let store = InMemoryAccountStore::new(vec![Account {
        operator: OperatorId(OPERATOR),
        role: gungnir_security::Role::Supervisor,
        phc: hash_passphrase(PASSPHRASE).expect("hashed"),
    }]);
    let issuer = TokenIssuer::new(vec![5u8; 32], 300.0).expect("issuer");
    Arc::new(
        NodeApi::new(SnapshotResponse::new(
            Vec::new(),
            None,
            SystemHealth::default(),
            Vec::new(),
        ))
        .with_callers(Arc::new(
            gungnir_api::transport::AccountTokenAuthority::new(Box::new(store), issuer),
        )),
    )
}

/// One request over the TLS connection: the status code and the body.
async fn request(
    addr: std::net::SocketAddr,
    config: ClientConfig,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: &str,
) -> (u16, String) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    let stream = tokio::net::TcpStream::connect(addr)
        .await
        .expect("connected");
    let name = ServerName::try_from("localhost").expect("a name");
    let mut tls = connector.connect(name, stream).await.expect("handshake");
    let crlf = [13u8, 10];
    let mut request = Vec::new();
    request.extend_from_slice(format!("{method} {path} HTTP/1.1").as_bytes());
    request.extend_from_slice(&crlf);
    request.extend_from_slice(b"Host: localhost");
    request.extend_from_slice(&crlf);
    request.extend_from_slice(b"Content-Type: application/json");
    request.extend_from_slice(&crlf);
    if let Some(token) = token {
        request.extend_from_slice(format!("Authorization: Bearer {token}").as_bytes());
        request.extend_from_slice(&crlf);
    }
    request.extend_from_slice(format!("Content-Length: {}", body.len()).as_bytes());
    request.extend_from_slice(&crlf);
    request.extend_from_slice(b"Connection: close");
    request.extend_from_slice(&crlf);
    request.extend_from_slice(&crlf);
    request.extend_from_slice(body.as_bytes());
    tls.write_all(&request).await.expect("wrote the request");

    let mut answer = Vec::new();
    let read = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tls.read_to_end(&mut answer),
    )
    .await;
    assert!(read.is_ok(), "the node did not answer {method} {path}");
    let text = String::from_utf8_lossy(&answer).into_owned();
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or_else(|| panic!("no status line in: {text}"));
    (status, text)
}

/// One decision a desktop took while it was cut off, as the route carries it.
fn forwarded(origin: &str) -> gungnir_api::v3::ForwardedDecision {
    use gungnir_model::events::VerdictSummary;
    use gungnir_model::{DecisionId, MissionTime, PlanId, PlanView};
    gungnir_api::v3::ForwardedDecision {
        record: gungnir_api::v3::DecisionRecordView {
            decision: DecisionId(0x0199_5a3b_7c2d_7e4f_8a1b_2c3d_9f3a_61c2),
            item: None,
            plan: PlanView {
                id: PlanId(11),
                ..PlanView::default()
            },
            verdict: VerdictSummary::RequiresHumanApproval,
            choice: gungnir_api::v3::DecisionChoice::Accept,
            operator: Some("7".into()),
            role: Some("Supervisor".into()),
            request: None,
            at: MissionTime(120.0),
        },
        origin: origin.to_owned(),
        settled: None,
    }
}

/// **GAP-141: the origin is the machine the handshake verified.**
///
/// A desktop's name is a fingerprint of the key its certificate carries, so the node can
/// compare what a batch claims with what the connection proved. A batch naming another
/// machine is refused and nothing of it reaches the loop -- which is what stops one
/// desktop putting decisions on the node's record in another's name.
#[tokio::test(flavor = "multi_thread")]
async fn a_forwarded_batch_naming_another_machine_is_refused() {
    let dir = tempdir("forwarded-origin");
    let api = api_with_accounts();
    let (addr, client_pki, server_ca) = serve_tls_with(&dir, Arc::clone(&api)).await;
    let mine = "desktop-0123456789abcdef";
    let (cert, key) = client_pki.issue(mine);
    let config = || client_config(&server_ca, Some((&cert, &key)));

    let (status, body) = request(
        addr,
        config(),
        "POST",
        "/v3/session",
        None,
        &format!(r#"{{"operator":{OPERATOR},"passphrase":"{PASSPHRASE}"}}"#),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let token = body
        .rsplit_once('{')
        .map(|(_, tail)| format!("{{{tail}"))
        .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
        .and_then(|v| v["token"].as_str().map(str::to_owned))
        .unwrap_or_else(|| panic!("no token in: {body}"));

    // Another machine's name, over this machine's connection.
    let batch = serde_json::to_string(&vec![forwarded("desktop-fedcba9876543210")]).expect("json");
    let (status, body) = request(
        addr,
        config(),
        "POST",
        "/v3/decisions/forwarded",
        Some(&token),
        &batch,
    )
    .await;
    assert_eq!(status, 403, "{body}");
    assert!(body.contains("desktop-fedcba9876543210"), "{body}");
    assert!(body.contains(mine), "{body}");
    assert!(
        api.take_forwarded().is_empty(),
        "a refused batch reached the node loop"
    );

    // Its own name passes the check: what answers then is the rest of the route, which no
    // loop is running to answer here, and that is a different thing from being refused.
    let batch = serde_json::to_string(&vec![forwarded(mine)]).expect("json");
    let (status, body) = request(
        addr,
        config(),
        "POST",
        "/v3/decisions/forwarded",
        Some(&token),
        &batch,
    )
    .await;
    assert_ne!(
        status, 403,
        "a batch naming this machine was refused: {body}"
    );
    let _ = std::fs::remove_dir_all(dir);
}
