// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! `POST` and `GET /v3/audit/anchors` over mutual TLS (GAP-208, D-142): a linked desktop
//! forwards the heads of its own audit log and reads back what the node holds for it.
//!
//! **Whose heads they are is the certificate's.** A desktop's heads are queued under the
//! name its certificate was verified under, whatever the body says, and a desktop reads
//! back its own and never another's. Both routes ask for a signed-in session; neither
//! asks for a role, because the record is the desktop's.
//!
//! The certificates are made here with `rcgen` (dev-only, D-22), as in `party.rs`, and the
//! client is `tokio-rustls` speaking HTTP/1.1 by hand.

use rustls::pki_types::pem::PemObject;
use std::sync::Arc;

use gungnir_api::tls::{self, TlsListener, TlsPaths};
use gungnir_api::transport::{serve_on_listener, AccountTokenAuthority, NodeApi};
use gungnir_api::v3::{AuditAnchorsHeld, SnapshotResponse};
use gungnir_model::events::AuditHead;
use gungnir_model::SystemHealth;
use gungnir_security::{hash_passphrase, Account, InMemoryAccountStore, OperatorId, TokenIssuer};
use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, Issuer, KeyPair};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const PASSPHRASE: &str = "correct horse battery staple";

struct Pki {
    dir: std::path::PathBuf,
    ca_pem: String,
    issuer_params: CertificateParams,
    issuer_key: KeyPair,
}

impl Pki {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "gungnir-audit-anchors-{name}-{}",
            std::process::id()
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

fn api() -> Arc<NodeApi> {
    let store = InMemoryAccountStore::new(vec![Account {
        operator: OperatorId(7),
        role: gungnir_security::Role::Operator,
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
        .with_callers(Arc::new(AccountTokenAuthority::new(
            Box::new(store),
            issuer,
        ))),
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

/// One HTTP/1.1 request over mutual TLS as `party`: the status and the body.
async fn request(
    pki: &Pki,
    addr: std::net::SocketAddr,
    party: &str,
    method: &str,
    path: &str,
    token: Option<&str>,
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
    let auth = token.map_or(String::new(), |t| format!("Authorization: Bearer {t}\r\n"));
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n{auth}\
         Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
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

async fn sign_in(pki: &Pki, addr: std::net::SocketAddr, party: &str) -> String {
    let body = serde_json::json!({"operator": 7, "passphrase": PASSPHRASE}).to_string();
    let (status, answer) = request(pki, addr, party, "POST", "/v3/session", None, Some(body)).await;
    assert_eq!(status, 200, "{answer}");
    let answer = answer
        .lines()
        .find(|l| l.trim_start().starts_with('{'))
        .unwrap_or(&answer)
        .to_owned();
    let session: gungnir_api::v3::SessionResponse =
        serde_json::from_str(&answer).expect("a session");
    session.token
}

fn head(entries: u64) -> AuditHead {
    AuditHead {
        segment: "audit-000001.jsonl".into(),
        entries,
        seq: entries,
        hash: "abc".into(),
        signature: None,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_desktops_heads_are_held_under_its_certificate_and_read_back_by_it_alone() {
    let pki = Pki::new("held");
    let api = api();
    let addr = serve(&pki, Arc::clone(&api)).await;
    let token = sign_in(&pki, addr, "desktop-a").await;
    let path = "/v3/audit/anchors";

    // Without a session the certificate alone is a machine, refused on an operator route
    // before anything is queued.
    let body = serde_json::json!({"heads": [head(3)]}).to_string();
    let (status, _) = request(
        &pki,
        addr,
        "desktop-a",
        "POST",
        path,
        None,
        Some(body.clone()),
    )
    .await;
    assert_eq!(status, 403);
    assert!(api.take_forwarded_anchors().is_empty());

    // Taken, under the certificate's name.
    let (status, answer) = request(
        &pki,
        addr,
        "desktop-a",
        "POST",
        path,
        Some(&token),
        Some(body),
    )
    .await;
    assert_eq!(status, 202, "{answer}");
    let queued = api.take_forwarded_anchors();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].party, "desktop-a");
    assert_eq!(queued[0].heads, vec![head(3)]);

    // What the loop then holds, read back by this desktop and by no other.
    api.publish_held_anchors(std::iter::once(("desktop-a".to_owned(), vec![head(3)])).collect());
    let (status, answer) = request(&pki, addr, "desktop-a", "GET", path, Some(&token), None).await;
    assert_eq!(status, 200, "{answer}");
    let json = answer
        .lines()
        .find(|l| l.trim_start().starts_with('{'))
        .unwrap_or(&answer)
        .to_owned();
    let held: AuditAnchorsHeld = serde_json::from_str(&json).expect("held heads");
    assert_eq!(held.heads, vec![head(3)]);
    let other = sign_in(&pki, addr, "desktop-b").await;
    let (_, answer) = request(&pki, addr, "desktop-b", "GET", path, Some(&other), None).await;
    assert!(!answer.contains("audit-000001"), "{answer}");

    // A body past the bound is refused whole.
    let many: Vec<AuditHead> = (0..=gungnir_api::v3::MAX_FORWARDED_ANCHORS as u64)
        .map(head)
        .collect();
    let body = serde_json::json!({ "heads": many }).to_string();
    let (status, _) = request(
        &pki,
        addr,
        "desktop-a",
        "POST",
        path,
        Some(&token),
        Some(body),
    )
    .await;
    assert_eq!(status, 413);
    assert!(api.take_forwarded_anchors().is_empty());
}
