// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A revoked key is refused at the handshake, on both sides (GAP-211, DN-22 §16).
//!
//! Over real TCP and the real configurations: the node's acceptor as
//! `gungnir_api::tls::acceptor_with_resolver` builds it, serving through the node's own
//! re-issuing identity, and the client as `gungnir_remote::client_config` builds it for a
//! desktop or a node's peer link. Each identity is pinned exactly as a deployment pins it,
//! so what refuses is the deny list and nothing else.

use std::sync::Arc;

use gungnir_remote::identity::{
    issue_node_peer_identity_ephemeral, issue_node_serving_identity, IssuedIdentity,
};
use gungnir_remote::LinkTls;
use rustls::pki_types::{CertificateDer, ServerName};

static SCRATCH: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn scratch() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-revoked-keys-{}-{}",
        std::process::id(),
        SCRATCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch");
    dir
}

fn fingerprint(der: &[u8]) -> String {
    gungnir_api::tls::certificate_key_fingerprint(&CertificateDer::from(der.to_vec()))
        .expect("parses")
}

/// Which side lists the other's key.
#[derive(Clone, Copy)]
enum Revoked {
    Nothing,
    NodeListsTheClient,
    ClientListsTheNode,
}

/// One handshake: the node's accept and the client's connect, each as an outcome.
async fn handshake(revoked: Revoked) -> (Result<(), String>, Result<(), String>) {
    let dir = scratch();
    // A file where the keystore directory would be forces the ephemeral path, as the
    // identity module's own tests do, so nothing is left in this machine's keystore.
    let blocked = dir.join("blocked");
    std::fs::write(&blocked, b"not a directory").expect("blocked");
    let serving = issue_node_serving_identity(&blocked, vec!["localhost".into()]).expect("issued");
    let serving_pem = serving.certificate_pem.clone();
    let serving_key = fingerprint(&serving.certificate_der);
    let client = issue_node_peer_identity_ephemeral().expect("issued");
    let client_key = fingerprint(&client.key.certificate_der());
    assert!(
        client.name.ends_with(&client_key),
        "{} is not named by its key {client_key}",
        client.name
    );
    let client_ca = dir.join("clients.pem");
    std::fs::write(&client_ca, &client.certificate_pem).expect("pinned");
    let (node_list, client_list) = match revoked {
        Revoked::Nothing => (Vec::new(), Vec::new()),
        Revoked::NodeListsTheClient => (vec![client_key], Vec::new()),
        Revoked::ClientListsTheNode => (Vec::new(), vec![serving_key]),
    };

    let acceptor = gungnir_api::tls::acceptor_with_resolver(
        Arc::new(IssuedIdentity::from(serving)),
        &client_ca.to_string_lossy(),
        &node_list,
    )
    .expect("the acceptor builds");
    let tls = LinkTls {
        trust_roots_pem: vec![serving_pem],
        issued: Some(client.key),
        identity_pem: None,
        revoked_keys: client_list,
    };
    let config = gungnir_remote::client_config(&tls).expect("the client builds");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bound");
    let addr = listener.local_addr().expect("addr");
    let node = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.map_err(|e| e.to_string())?;
        let mut stream = acceptor.accept(tcp).await.map_err(|e| e.to_string())?;
        // TLS 1.3 completes the client's half before the node has judged the client's
        // certificate, so the node answers one byte and the client waits to read it.
        tokio::io::AsyncWriteExt::write_all(&mut stream, b"k")
            .await
            .map_err(|e| e.to_string())
    });
    let outcome = async {
        let tcp = tokio::net::TcpStream::connect(addr)
            .await
            .map_err(|e| e.to_string())?;
        let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
        let name = ServerName::try_from("localhost").expect("a name");
        let mut stream = connector
            .connect(name, tcp)
            .await
            .map_err(|e| e.to_string())?;
        let mut byte = [0u8; 1];
        tokio::io::AsyncReadExt::read_exact(&mut stream, &mut byte)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    .await;
    let node = node.await.expect("the node task");
    let _ = std::fs::remove_dir_all(&dir);
    (node, outcome)
}

#[tokio::test(flavor = "multi_thread")]
async fn with_nothing_revoked_the_pinned_identities_link() {
    let (node, client) = handshake(Revoked::Nothing).await;
    assert!(node.is_ok(), "{node:?}");
    assert!(client.is_ok(), "{client:?}");
}

/// The node refuses a client whose key it lists, though that client's certificate is
/// pinned in its client authority file.
#[tokio::test(flavor = "multi_thread")]
async fn a_node_refuses_a_client_whose_key_is_revoked() {
    let (node, client) = handshake(Revoked::NodeListsTheClient).await;
    let node = node.expect_err("the node accepted a revoked key");
    assert!(node.contains("revoked"), "{node}");
    assert!(
        client.is_err(),
        "the client read from a node that refused it"
    );
}

/// A client refuses a node whose key it lists, though the node's certificate is its
/// trust root.
#[tokio::test(flavor = "multi_thread")]
async fn a_client_refuses_a_node_whose_key_is_revoked() {
    let (node, client) = handshake(Revoked::ClientListsTheNode).await;
    let client = client.expect_err("the client accepted a revoked node");
    assert!(client.contains("revoked"), "{client}");
    assert!(
        node.is_err(),
        "the node completed a handshake its client refused"
    );
}
