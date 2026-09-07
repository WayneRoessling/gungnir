//! The two-node harness these tests share: a throwaway certificate authority, a node
//! served over mutual TLS on loopback, and a bounded wait.
//!
//! The same arrangement `tls_link.rs` uses, in a module because two files now need it
//! (GAP-009's launch-warning path and GAP-063's conformance suite over the wire) and a
//! third copy of a certificate authority is a third place the test material could drift.
//!
//! Certificates are made here with `rcgen` and never written to the repository (D-22).
//! The node's identity goes to files under the temporary directory because that is how a
//! node reads its own (`TlsPaths`); the clients hold theirs as PEM text, which is how the
//! desktop reads the environment's. The directory name carries `std::process::id()`, so
//! two concurrent `cargo test` runs cannot delete each other's material.

use gungnir_api::tls::{self, TlsListener, TlsPaths};
use gungnir_api::transport::{serve_on_listener, NodeApi};
use gungnir_remote::LinkTls;
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

pub struct Pki {
    dir: std::path::PathBuf,
    ca_pem: String,
    issuer_params: CertificateParams,
    issuer_key: KeyPair,
}

impl Pki {
    pub fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "gungnir-wire-{name}-{}-{}",
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

    /// A client's material as a host holds it: the roots, and one PEM text with the
    /// certificate and its key, the subject common name being the party.
    pub fn client(&self, party: &str) -> LinkTls {
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

    /// Serve `api` over mutual TLS on an ephemeral loopback port and return its base URL.
    pub async fn serve(&self, api: Arc<NodeApi>) -> String {
        let acceptor = tls::acceptor(&self.server_paths()).expect("acceptor");
        let tcp = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = tcp.local_addr().expect("addr").port();
        tokio::spawn(async move {
            let _ = serve_on_listener(TlsListener::new(tcp, acceptor), api).await;
        });
        format!("https://localhost:{port}")
    }
}

/// Wait for `check` to hold, or fail the test saying what never happened.
///
/// Bounded rather than unbounded: a link that never comes up must fail the test rather
/// than hang a continuous-integration run.
pub async fn until(mut check: impl FnMut() -> bool, what: &str) {
    for _ in 0..200 {
        if check() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("timed out waiting for {what}");
}

/// Wait until a client's **event stream** is following, and answer the next free
/// sequence number.
///
/// A link reports `connected` once the snapshot has been answered, and that is *before*
/// its WebSocket has subscribed -- the snapshot is an HTTP request and the stream is a
/// second connection opened after it. A subscription with `from_seq` 0 means "everything
/// from now" by the v2 contract, so an envelope published in the window between the two
/// reaches nobody, correctly and silently. A test that published once and then waited
/// would be racing that window and would fail perhaps one run in three.
///
/// So a sentinel is published on a fresh sequence number until one comes back, which is
/// proof the subscription is live; the caller publishes what it is actually testing
/// afterwards, from the sequence number returned.
pub async fn until_following(
    api: &NodeApi,
    mut sentinel: impl FnMut(u64) -> gungnir_eventing::Envelope,
    mut arrived: impl FnMut() -> bool,
) -> u64 {
    for seq in 1..=100u64 {
        api.publish_event(sentinel(seq))
            .expect("the node publishes");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        if arrived() {
            return seq + 1;
        }
    }
    panic!("the event stream never began following");
}
