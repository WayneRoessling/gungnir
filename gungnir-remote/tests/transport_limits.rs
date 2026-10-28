// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The transport's limits (GAP-214): TLS 1.3 only, a deadline on the handshake and on the
//! stream's first frame, and an explicit body limit.
//!
//! Over real sockets and the real acceptor and router. Two tests wait out a ten-second
//! deadline, so they are the slow ones in this file and each waits once.

use std::sync::Arc;
use std::time::{Duration, Instant};

use gungnir_api::transport::{bind, serve_on, serve_on_listener, NodeApi, MAX_BODY_BYTES};
use gungnir_api::v3::SnapshotResponse;
use gungnir_model::SystemHealth;
use gungnir_remote::identity::{
    issue_node_peer_identity_ephemeral, issue_node_serving_identity, IssuedIdentity,
};
use gungnir_remote::LinkTls;
use rustls::pki_types::ServerName;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

static SCRATCH: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn api() -> Arc<NodeApi> {
    Arc::new(NodeApi::new(SnapshotResponse::new(
        Vec::new(),
        None,
        SystemHealth::default(),
        Vec::new(),
    )))
}

/// A node serving over mutual TLS on loopback.
struct TlsNode {
    addr: std::net::SocketAddr,
    /// The node's certificate, which a client trusts.
    serving_pem: String,
    /// An identity the node pins.
    client: Arc<IssuedIdentity>,
    dir: std::path::PathBuf,
}

impl TlsNode {
    /// The product's own client configuration for the pinned identity.
    fn client_config(&self) -> rustls::ClientConfig {
        gungnir_remote::client_config(&LinkTls {
            trust_roots_pem: vec![self.serving_pem.clone()],
            issued: Some(Arc::clone(&self.client)),
            identity_pem: None,
            revoked_keys: Vec::new(),
        })
        .expect("the client builds")
    }
}

impl Drop for TlsNode {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

async fn tls_node() -> TlsNode {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-transport-limits-{}-{}",
        std::process::id(),
        SCRATCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch");
    let blocked = dir.join("blocked");
    std::fs::write(&blocked, b"not a directory").expect("blocked");
    let serving = issue_node_serving_identity(&blocked, vec!["localhost".into()]).expect("issued");
    let serving_pem = serving.certificate_pem.clone();
    let client = issue_node_peer_identity_ephemeral().expect("issued");
    let client_ca = dir.join("clients.pem");
    std::fs::write(&client_ca, &client.certificate_pem).expect("pinned");
    let acceptor = gungnir_api::tls::acceptor_with_resolver(
        Arc::new(IssuedIdentity::from(serving)),
        &client_ca.to_string_lossy(),
        &[],
    )
    .expect("the acceptor builds");
    let tcp = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bound");
    let addr = tcp.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = serve_on_listener(gungnir_api::tls::TlsListener::new(tcp, acceptor), api()).await;
    });
    TlsNode {
        addr,
        serving_pem,
        client: client.key,
        dir,
    }
}

async fn handshake(addr: std::net::SocketAddr, config: rustls::ClientConfig) -> Result<(), String> {
    let tcp = tokio::net::TcpStream::connect(addr)
        .await
        .map_err(|e| e.to_string())?;
    let name = ServerName::try_from("localhost").expect("a name");
    let mut stream = tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect(name, tcp)
        .await
        .map_err(|e| e.to_string())?;
    stream
        .write_all(b"GET /v3/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .map_err(|e| e.to_string())?;
    let mut answer = Vec::new();
    stream
        .read_to_end(&mut answer)
        .await
        .map_err(|e| e.to_string())?;
    String::from_utf8_lossy(&answer)
        .starts_with("HTTP/1.1 ")
        .then_some(())
        .ok_or_else(|| "no HTTP answer".to_owned())
}

/// The node serves TLS 1.3 and refuses a client that offers only 1.2, with the same
/// trust and the same pinned identity, so what differs is the version alone.
#[tokio::test(flavor = "multi_thread")]
async fn a_tls_1_2_client_is_refused() {
    use rustls::pki_types::pem::PemObject;
    let node = tls_node().await;
    handshake(node.addr, node.client_config())
        .await
        .expect("a TLS 1.3 client is served");

    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(
            rustls::pki_types::CertificateDer::from_pem_slice(node.serving_pem.as_bytes())
                .expect("the node's certificate"),
        )
        .expect("a usable root");
    let old = rustls::ClientConfig::builder_with_protocol_versions(&[&rustls::version::TLS12])
        .with_root_certificates(roots)
        .with_client_cert_resolver(
            Arc::clone(&node.client) as Arc<dyn rustls::client::ResolvesClientCert>
        );
    let refused = handshake(node.addr, old)
        .await
        .expect_err("a client offering only TLS 1.2 was served");
    assert!(refused.to_lowercase().contains("version"), "{refused}");
}

/// A client that opens a connection and never handshakes holds up the next one for the
/// handshake deadline and no longer: handshakes are taken one at a time.
#[tokio::test(flavor = "multi_thread")]
async fn a_stalled_handshake_does_not_hold_the_node_up() {
    let node = tls_node().await;
    let _stalled = tokio::net::TcpStream::connect(node.addr)
        .await
        .expect("connect");
    let started = Instant::now();
    let served = tokio::time::timeout(
        Duration::from_secs(25),
        handshake(node.addr, node.client_config()),
    )
    .await;
    assert!(
        matches!(served, Ok(Ok(()))),
        "the next client was not served: {served:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(20));
}

async fn plain_node() -> std::net::SocketAddr {
    let listener = bind("127.0.0.1:0".parse().expect("address"))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = serve_on(listener, api()).await;
    });
    addr
}

/// An upgraded stream that never sends its `SubscribeRequest` is told why and closed,
/// rather than held open.
#[tokio::test(flavor = "multi_thread")]
async fn a_stream_that_never_subscribes_is_closed() {
    use futures_util::StreamExt;
    let addr = plain_node().await;
    let (mut socket, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/v3/events"))
        .await
        .expect("upgraded");
    let said = tokio::time::timeout(Duration::from_secs(25), socket.next())
        .await
        .expect("the node did not close the stream");
    let text = match said {
        Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => text.to_string(),
        other => panic!("expected the node's reason, got {other:?}"),
    };
    assert!(text.contains("no SubscribeRequest within"), "{text}");
}

/// A body past `MAX_BODY_BYTES` is refused before any route reads it.
#[tokio::test(flavor = "multi_thread")]
async fn an_oversized_body_is_refused() {
    let addr = plain_node().await;
    let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let body = vec![b' '; MAX_BODY_BYTES + 1];
    let head = format!(
        "POST /v3/session HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await.expect("head");
    let _ = stream.write_all(&body).await;
    let mut answer = Vec::new();
    let _ = stream.read_to_end(&mut answer).await;
    let answer = String::from_utf8_lossy(&answer);
    assert!(answer.starts_with("HTTP/1.1 413"), "{answer}");
}
