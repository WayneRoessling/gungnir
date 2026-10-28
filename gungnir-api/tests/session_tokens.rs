// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A node's session tokens follow the account store, can be ended, and back off per
//! address (GAP-212, DN-23 §17).
//!
//! Over the real routes on plain loopback, with a file-backed account store so a test can
//! change an operator's role the way `gungnir-node account` or PN-20 would.

use std::fmt::Write as _;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use gungnir_api::transport::{bind, serve_on, AccountTokenAuthority, CallerAuthority, NodeApi};
use gungnir_api::v3::SnapshotResponse;
use gungnir_model::SystemHealth;
use gungnir_security::{
    hash_passphrase, Account, AuthFailure, FileAccountStore, OperatorId, Role, TokenIssuer,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const OPERATOR: u64 = 7;
const ADMINISTRATOR: u64 = 9;
const PASSPHRASE: &str = "correct horse battery";

static SCRATCH: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// An account file holding `accounts`, written where a test can rewrite it.
fn accounts_file(accounts: &[(u64, Role)]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-session-tokens-{}-{}",
        std::process::id(),
        SCRATCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("accounts.json");
    write_accounts(&path, accounts);
    path
}

fn write_accounts(path: &std::path::Path, accounts: &[(u64, Role)]) {
    let phc = hash_passphrase(PASSPHRASE).expect("hashed");
    let accounts: Vec<Account> = accounts
        .iter()
        .map(|(operator, role)| Account {
            operator: OperatorId(*operator),
            role: *role,
            phc: phc.clone(),
        })
        .collect();
    std::fs::write(path, serde_json::to_string(&accounts).expect("encodes")).expect("written");
}

fn authority(path: &std::path::Path) -> AccountTokenAuthority {
    AccountTokenAuthority::new(
        Box::new(FileAccountStore::open(path).expect("store")),
        TokenIssuer::new(vec![7u8; 32], 3_600.0).expect("issuer"),
    )
}

async fn serve(path: &std::path::Path) -> std::net::SocketAddr {
    let api = Arc::new(
        NodeApi::new(SnapshotResponse::new(
            Vec::new(),
            None,
            SystemHealth::default(),
            Vec::new(),
        ))
        .with_callers(Arc::new(authority(path))),
    );
    api.set_now(100.0);
    let listener = bind("127.0.0.1:0".parse().expect("address"))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = serve_on(listener, api).await;
    });
    addr
}

/// One HTTP/1.1 request over plain loopback: the status and the body.
async fn request(
    addr: std::net::SocketAddr,
    method: &str,
    path: &str,
    bearer: Option<&str>,
    json_body: Option<&str>,
) -> (u16, String) {
    let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n");
    if let Some(token) = bearer {
        let _ = write!(head, "Authorization: Bearer {token}\r\n");
    }
    let body = json_body.unwrap_or_default();
    if json_body.is_some() {
        head.push_str("Content-Type: application/json\r\n");
    }
    let _ = write!(head, "Content-Length: {}\r\n\r\n", body.len());
    stream.write_all(head.as_bytes()).await.expect("write head");
    stream.write_all(body.as_bytes()).await.expect("write body");
    let mut raw = Vec::new();
    let _ = stream.read_to_end(&mut raw).await;
    let text = String::from_utf8_lossy(&raw).into_owned();
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .expect("status line");
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_owned())
        .unwrap_or_default();
    (status, body)
}

async fn sign_in(addr: std::net::SocketAddr, operator: u64) -> String {
    let body = serde_json::json!({ "operator": operator, "passphrase": PASSPHRASE }).to_string();
    let (status, body) = request(addr, "POST", "/v3/session", None, Some(&body)).await;
    assert_eq!(status, 200, "sign-in failed: {body}");
    let value: serde_json::Value = serde_json::from_str(&body).expect("a session");
    value["token"].as_str().expect("a token").to_owned()
}

async fn role_of(addr: std::net::SocketAddr, token: &str) -> Result<String, u16> {
    let (status, body) = request(addr, "GET", "/v3/session", Some(token), None).await;
    if status != 200 {
        return Err(status);
    }
    let value: serde_json::Value = serde_json::from_str(&body).expect("a session status");
    Ok(value["role"].as_str().expect("a role").to_owned())
}

/// The role a token acts in is the store's now, not the one it was minted under: a
/// promotion and a demotion take effect at the next request, and a removed account's
/// token is refused and stays refused.
#[tokio::test(flavor = "multi_thread")]
async fn a_token_acts_in_the_role_the_store_holds_now() {
    let path = accounts_file(&[(OPERATOR, Role::Operator)]);
    let addr = serve(&path).await;
    let token = sign_in(addr, OPERATOR).await;
    assert_eq!(role_of(addr, &token).await, Ok("Operator".to_owned()));

    write_accounts(&path, &[(OPERATOR, Role::Supervisor)]);
    assert_eq!(role_of(addr, &token).await, Ok("Supervisor".to_owned()));

    write_accounts(&path, &[(OPERATOR, Role::Analyst)]);
    assert_eq!(role_of(addr, &token).await, Ok("Analyst".to_owned()));

    write_accounts(&path, &[]);
    assert_eq!(role_of(addr, &token).await, Err(401));
    write_accounts(&path, &[(OPERATOR, Role::Operator)]);
    assert_eq!(
        role_of(addr, &token).await,
        Err(401),
        "a removed account's token came back with the account"
    );
}

/// `DELETE /v3/session` ends the caller's own session: the token is refused afterwards.
#[tokio::test(flavor = "multi_thread")]
async fn signing_out_ends_the_token() {
    let path = accounts_file(&[(OPERATOR, Role::Operator)]);
    let addr = serve(&path).await;
    let token = sign_in(addr, OPERATOR).await;
    let (status, body) = request(addr, "DELETE", "/v3/session", Some(&token), None).await;
    assert_eq!(status, 204, "{body}");
    assert_eq!(role_of(addr, &token).await, Err(401));
}

/// `POST /v3/session/revoke` ends every session an operator holds, for an administrator
/// and for nobody else.
#[tokio::test(flavor = "multi_thread")]
async fn an_administrator_ends_another_operators_sessions() {
    let path = accounts_file(&[
        (OPERATOR, Role::Operator),
        (ADMINISTRATOR, Role::Administrator),
    ]);
    let addr = serve(&path).await;
    let first = sign_in(addr, OPERATOR).await;
    let second = sign_in(addr, OPERATOR).await;
    let admin = sign_in(addr, ADMINISTRATOR).await;
    let body = serde_json::json!({ "operator": ADMINISTRATOR }).to_string();

    let (status, _) = request(
        addr,
        "POST",
        "/v3/session/revoke",
        Some(&first),
        Some(&body),
    )
    .await;
    assert_eq!(status, 403, "an operator ended an administrator's sessions");

    let body = serde_json::json!({ "operator": OPERATOR }).to_string();
    let (status, answer) = request(
        addr,
        "POST",
        "/v3/session/revoke",
        Some(&admin),
        Some(&body),
    )
    .await;
    assert_eq!(status, 200, "{answer}");
    let value: serde_json::Value = serde_json::from_str(&answer).expect("an answer");
    assert_eq!(value["revoked"], 2);
    assert_eq!(role_of(addr, &first).await, Err(401));
    assert_eq!(role_of(addr, &second).await, Err(401));
    assert!(
        role_of(addr, &admin).await.is_ok(),
        "the administrator's own session ended"
    );
}

/// The back-off is keyed by address and operator, and by address alone: failures from
/// one address do not slow the operator signing in from another, and one address failing
/// across operators is slowed for all of them.
#[test]
fn the_back_off_is_keyed_by_address_and_operator() {
    let path = accounts_file(&[(OPERATOR, Role::Operator), (8, Role::Operator)]);
    let authority = authority(&path);
    let stranger = Some(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 66)));
    let console = Some(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 7)));
    for _ in 0..4 {
        let _ = authority.sign_in(OPERATOR, "a guess", 0.0, stranger);
    }
    assert!(matches!(
        authority.sign_in(OPERATOR, PASSPHRASE, 0.0, stranger),
        Err(AuthFailure::TooFast { .. })
    ));
    assert!(
        authority
            .sign_in(OPERATOR, PASSPHRASE, 0.0, console)
            .is_ok(),
        "a stranger's failures locked the operator out of their own console"
    );
    assert!(
        matches!(
            authority.sign_in(8, PASSPHRASE, 0.0, stranger),
            Err(AuthFailure::TooFast { .. })
        ),
        "an address guessing across operators was not slowed"
    );
}
