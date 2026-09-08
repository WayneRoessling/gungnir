// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! DN-18's three remaining exchange items over the wire (GAP-065): warnings, reports and
//! handoffs served to a party through both of §5's gates.
//!
//! The criterion DN-18 §8 calls the one that proves the gates are independent, and the one
//! an implementation shortcut would break by checking only the agreement, is here twice:
//! an item the agreement does not cover is refused outright, and an item the agreement
//! covers whose marking forbids this party is withheld and **counted**. A partner told its
//! list is partial can ask for the rest; one that is not told believes it has everything.
//!
//! The certificates are made here with `rcgen` (dev-only, D-22) and never written to the
//! repository.

use rustls::pki_types::pem::PemObject;
use std::sync::Arc;

use gungnir_api::tls::{self, TlsListener, TlsPaths};
use gungnir_api::transport::{serve_on_listener, AccountTokenAuthority, NodeApi};
use gungnir_api::v2::{ExchangeProduct, ExchangeResponse, PublishExchangeRequest, SnapshotResponse};
use gungnir_model::{
    ExchangeAgreement, ExchangeFormat, ExchangeItem, ExchangeSet, MissionTime, Releasability,
    SystemHealth,
};
use gungnir_security::{hash_passphrase, Account, InMemoryAccountStore, OperatorId, TokenIssuer};
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
            "gungnir-exchange-{name}-{}-{}",
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

/// One HTTP/1.1 GET over mutual TLS as `party`; the status and the body.
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

/// One HTTP/1.1 request over mutual TLS as `party`, optionally bearing a token and a
/// body; the status and the body. Generalizes [`get`] for the write routes, which need a
/// method, a token, and a body that a bare `GET` never does.
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

/// Sign in as `operator` and return the token.
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

fn product(id: &str, at: f64, releasability: Releasability) -> ExchangeProduct {
    ExchangeProduct {
        id: id.into(),
        at: MissionTime(at),
        releasability,
        body: serde_json::json!({ "id": id }),
    }
}

const PASSPHRASE: &str = "correct horse battery staple";
/// Holds `PUBLISH_EXCHANGE` (GAP-065, written and gated, signed by the owner the same day).
const COMMANDER: u64 = 70;
/// Holds neither `PUBLISH_EXCHANGE` nor `RELEASE_PRODUCT`.
const OPERATOR: u64 = 71;

/// A deployment that sends `sector-north` all three items and `sector-east` health alone,
/// and that holds three warnings, two handoffs, and no reports at all.
fn api() -> Arc<NodeApi> {
    let store = InMemoryAccountStore::new(vec![
        Account {
            operator: OperatorId(COMMANDER),
            role: gungnir_security::Role::Commander,
            phc: hash_passphrase(PASSPHRASE).expect("hashed"),
        },
        Account {
            operator: OperatorId(OPERATOR),
            role: gungnir_security::Role::Operator,
            phc: hash_passphrase(PASSPHRASE).expect("hashed"),
        },
    ]);
    let issuer = TokenIssuer::new(vec![9u8; 32], 300.0).expect("issuer");
    let api = Arc::new(
        NodeApi::new(SnapshotResponse::new(
            Vec::new(),
            None,
            SystemHealth::default(),
            Vec::new(),
        ))
        .with_exchange(ExchangeSet {
            agreements: vec![
                ExchangeAgreement {
                    party: "sector-north".into(),
                    inbound: Vec::new(),
                    outbound: vec![
                        ExchangeItem::Warnings,
                        ExchangeItem::Reports,
                        ExchangeItem::Handoffs,
                    ],
                    format: ExchangeFormat::Canonical,
                },
                ExchangeAgreement {
                    party: "sector-east".into(),
                    inbound: Vec::new(),
                    outbound: vec![ExchangeItem::Health],
                    format: ExchangeFormat::Canonical,
                },
            ],
        })
        .with_callers(Arc::new(AccountTokenAuthority::new(
            Box::new(store),
            issuer,
        ))),
    );
    api.publish_exchange(
        ExchangeItem::Warnings,
        vec![
            product("asset-1/track-7", 10.0, Releasability::Internal),
            product(
                "asset-2/track-8",
                11.0,
                Releasability::parties(["sector-north"]),
            ),
            product("asset-3/track-9", 12.0, Releasability::AllPeers),
        ],
    )
    .expect("warnings published");
    // Both marked internal: the agreement covers handoffs and no marking releases one.
    api.publish_exchange(
        ExchangeItem::Handoffs,
        vec![
            product("decision-1", 20.0, Releasability::Internal),
            product("decision-2", 21.0, Releasability::Internal),
        ],
    )
    .expect("handoffs published");
    api
}

fn held(body: &str) -> (Vec<String>, usize) {
    match serde_json::from_str::<ExchangeResponse>(body).expect("exchange response") {
        ExchangeResponse::Held {
            products, withheld, ..
        } => (products.into_iter().map(|p| p.id).collect(), withheld),
        ExchangeResponse::NotHeld { item, reason } => {
            panic!("expected products for {item:?}, got: {reason}")
        }
    }
}

/// DN-18 §5's two gates on the warnings route: the agreement covers warnings, so what the
/// party receives is decided by the markings alone, and what they removed is counted.
#[tokio::test(flavor = "multi_thread")]
async fn the_marking_decides_which_warnings_a_covered_party_receives_and_the_rest_are_counted() {
    let pki = Pki::new("warnings");
    let addr = serve(&pki, api()).await;
    let (status, body) = get(&pki, addr, "sector-north", "/v2/exchange/warnings").await;
    assert_eq!(status, 200, "{body}");
    let (ids, withheld) = held(&body);
    assert_eq!(
        ids,
        vec!["asset-2/track-8", "asset-3/track-9"],
        "the internal warning stays home"
    );
    assert_eq!(withheld, 1, "and the party is told one was withheld");
    let _ = std::fs::remove_dir_all(&pki.dir);
}

/// The second gate on its own: the agreement sends handoffs and every marking forbids this
/// party, so the list is empty **and the count says two**. An empty list with a zero count
/// would tell the partner there are none, which is a different claim and a false one.
#[tokio::test(flavor = "multi_thread")]
async fn an_agreement_that_covers_an_item_does_not_override_the_marking() {
    let pki = Pki::new("marking");
    let addr = serve(&pki, api()).await;
    let (status, body) = get(&pki, addr, "sector-north", "/v2/exchange/handoffs").await;
    assert_eq!(status, 200, "{body}");
    let (ids, withheld) = held(&body);
    assert!(ids.is_empty(), "{ids:?}");
    assert_eq!(withheld, 2);
    let _ = std::fs::remove_dir_all(&pki.dir);
}

/// The first gate: an item the agreement does not cover is refused, and the refusal names
/// the item rather than saying how many there were.
#[tokio::test(flavor = "multi_thread")]
async fn an_item_the_agreement_does_not_cover_is_refused() {
    let pki = Pki::new("agreement");
    let addr = serve(&pki, api()).await;
    for path in [
        "/v2/exchange/warnings",
        "/v2/exchange/reports",
        "/v2/exchange/handoffs",
    ] {
        let (status, body) = get(&pki, addr, "sector-east", path).await;
        assert_eq!(status, 403, "{path}: {body}");
        assert!(body.contains("does not send"), "{path}: {body}");
        assert!(
            !body.contains("asset-3/track-9"),
            "a refusal leaked a product: {body}"
        );
    }
    // No agreement at all: nothing, on every route (DN-18 §5).
    let (status, body) = get(&pki, addr, "nobody", "/v2/exchange/warnings").await;
    assert_eq!(status, 403, "{body}");
    assert!(body.contains("no exchange agreement"), "{body}");
    let _ = std::fs::remove_dir_all(&pki.dir);
}

/// An item nothing has published is `not-held` with a reason, not an empty list: "we hold
/// none of these" and "we released none of them to you" are opposite claims about a
/// sector, and the second is what an empty list under a covering agreement would mean.
#[tokio::test(flavor = "multi_thread")]
async fn an_item_this_deployment_publishes_nothing_for_says_so() {
    let pki = Pki::new("notheld");
    let addr = serve(&pki, api()).await;
    let (status, body) = get(&pki, addr, "sector-north", "/v2/exchange/reports").await;
    assert_eq!(status, 200, "{body}");
    match serde_json::from_str::<ExchangeResponse>(&body).expect("exchange response") {
        ExchangeResponse::NotHeld { item, reason } => {
            assert_eq!(item, ExchangeItem::Reports);
            assert!(!reason.is_empty(), "a state with no reason is not one");
        }
        ExchangeResponse::Held { products, .. } => {
            panic!("an unpublished item was reported as held: {products:?}")
        }
    }
    let _ = std::fs::remove_dir_all(&pki.dir);
}

fn publish(products: Vec<ExchangeProduct>) -> String {
    serde_json::to_string(&PublishExchangeRequest { products }).expect("json")
}

/// GAP-065, DN-18 §5 amendment 2: an operator holding `PUBLISH_EXCHANGE` replaces what
/// this node holds for an item, and a covered party then reads the new set through the
/// existing `GET` route -- the write path DN-18 amendment 1 said neither existed nor was
/// decided.
///
/// **Replaces, not appends.** `api()` already publishes three warnings; posting one more
/// leaves exactly the one just posted, which is [`gungnir_api::transport::NodeApi::publish_exchange`]'s
/// own contract and not something a caller could tell from the write response alone.
#[tokio::test(flavor = "multi_thread")]
async fn an_operator_holding_the_action_replaces_what_the_node_holds() {
    let pki = Pki::new("publish");
    let api = api();
    let addr = serve(&pki, Arc::clone(&api)).await;
    let commander = token(&pki, addr, COMMANDER).await;

    let (status, body) = request(
        &pki,
        addr,
        "desk-1",
        "POST",
        "/v2/exchange/warnings",
        Some(&commander),
        Some(publish(vec![product(
            "asset-9/track-1",
            30.0,
            Releasability::AllPeers,
        )])),
    )
    .await;
    assert_eq!(status, 202, "{body}");

    let (status, body) = get(&pki, addr, "sector-north", "/v2/exchange/warnings").await;
    assert_eq!(status, 200, "{body}");
    let (ids, withheld) = held(&body);
    assert_eq!(
        ids,
        vec!["asset-9/track-1"],
        "the posted set replaced the three warnings api() had published, not joined them"
    );
    assert_eq!(withheld, 0);
    let _ = std::fs::remove_dir_all(&pki.dir);
}

/// An operator with no `PUBLISH_EXCHANGE` is refused, and nothing already held changes.
#[tokio::test(flavor = "multi_thread")]
async fn an_operator_without_the_action_may_not_publish() {
    let pki = Pki::new("forbidden");
    let api = api();
    let addr = serve(&pki, Arc::clone(&api)).await;
    let operator = token(&pki, addr, OPERATOR).await;

    let (status, body) = request(
        &pki,
        addr,
        "desk-1",
        "POST",
        "/v2/exchange/warnings",
        Some(&operator),
        Some(publish(vec![product(
            "asset-9/track-1",
            30.0,
            Releasability::AllPeers,
        )])),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    assert!(body.contains("may not publish to exchange"), "{body}");

    let (status, body) = get(&pki, addr, "sector-north", "/v2/exchange/warnings").await;
    assert_eq!(status, 200, "{body}");
    let (ids, _) = held(&body);
    assert_eq!(
        ids,
        vec!["asset-2/track-8", "asset-3/track-9"],
        "the refused write changed nothing api() had already published"
    );
    let _ = std::fs::remove_dir_all(&pki.dir);
}

/// A body that will not parse is refused before anything already held changes, and a
/// caller with neither a token nor a recognized machine role gets the same refusal every
/// other write route gives it.
#[tokio::test(flavor = "multi_thread")]
async fn a_malformed_publish_changes_nothing_and_an_unauthenticated_one_is_refused() {
    let pki = Pki::new("malformed");
    let api = api();
    let addr = serve(&pki, Arc::clone(&api)).await;
    let commander = token(&pki, addr, COMMANDER).await;

    let (status, body) = request(
        &pki,
        addr,
        "desk-1",
        "POST",
        "/v2/exchange/handoffs",
        Some(&commander),
        Some("{\"products\":\"not a list\"}".to_string()),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    let (status, body) = get(&pki, addr, "sector-north", "/v2/exchange/handoffs").await;
    assert_eq!(status, 200, "{body}");
    let (ids, withheld) = held(&body);
    assert!(ids.is_empty(), "both handoffs api() published stay internal");
    assert_eq!(withheld, 2, "unchanged from api()'s own two handoffs");

    // No bearer token and no machine role for "desk-1": the same "no exchange agreement"
    // refusal every other route gives an authenticated caller nobody has agreed with
    // (DN-18 §5).
    let (status, body) = request(
        &pki,
        addr,
        "desk-1",
        "POST",
        "/v2/exchange/handoffs",
        None,
        Some(publish(vec![product("decision-9", 1.0, Releasability::AllPeers)])),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    assert!(body.contains("no exchange agreement"), "{body}");
    let _ = std::fs::remove_dir_all(&pki.dir);
}
