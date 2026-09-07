// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Provisioning the accounts a node authenticates against (GAP-057, DN-23).
//!
//! The thing under test is the one that was missing: until 2026-09-07 `hash_passphrase`
//! had no caller outside tests, so the account file a node needs could not be written by
//! any shipped path. These tests drive the binary itself rather than the module, because
//! the defect was never in the hashing -- it was that nothing reachable called it.

use gungnir_api::transport::CallerAuthority;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// A scratch directory per test. The process id separates concurrent `cargo test` runs
/// and the counter separates tests inside this binary, which share one process.
static SCRATCH: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-node-account-{name}-{}-{}",
        std::process::id(),
        SCRATCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn node_binary() -> PathBuf {
    // `CARGO_BIN_EXE_<name>` is set by cargo for the crate's own binaries.
    PathBuf::from(env!("CARGO_BIN_EXE_gungnir-node"))
}

/// Run `gungnir-node account ...` with `stdin` supplied, returning (code, stdout, stderr).
fn run_account(args: &[&str], stdin: Option<&str>) -> (i32, String, String) {
    let mut child = Command::new(node_binary())
        .arg("account")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawned");
    {
        let pipe = child.stdin.as_mut().expect("stdin");
        if let Some(text) = stdin {
            pipe.write_all(text.as_bytes()).expect("wrote passphrase");
        }
    }
    // Dropping the handle closes the pipe, which is what makes `read_to_string` return.
    drop(child.stdin.take());
    let out = child.wait_with_output().expect("waited");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn an_account_created_by_the_binary_is_one_the_node_can_authenticate() {
    let dir = scratch("roundtrip");
    let path = dir.join("accounts.json");
    let file = path.to_string_lossy().into_owned();

    let (code, said, err) = run_account(
        &["add", file.as_str(), "7", "operator"],
        Some("correct horse"),
    );
    assert_eq!(code, 0, "provisioning must succeed: {err}");
    assert!(
        said.contains("added operator 7"),
        "it must say what it did: {said}"
    );

    // The file is what `FileAccountStore` reads, and the passphrase is not in it.
    let text = std::fs::read_to_string(&path).expect("written");
    assert!(
        !text.contains("correct horse"),
        "the passphrase must never reach the file: {text}"
    );
    assert!(
        text.contains("$argon2"),
        "the file must hold a PHC string: {text}"
    );

    // **The point of the test.** The store opens it and the account verifies, so the
    // path from provisioning to authentication is closed rather than assumed.
    let store = gungnir_security::FileAccountStore::open(&path).expect("the store opens it");
    let listing = store.listing();
    assert_eq!(listing.len(), 1);
    assert_eq!(listing[0].0, gungnir_security::OperatorId(7));
    assert_eq!(listing[0].1, gungnir_security::Role::Operator);

    let issuer = gungnir_security::TokenIssuer::new(
        b"a-signing-key-of-adequate-length-0123456789".to_vec(),
        60.0,
    )
    .expect("issuer");
    let authority = gungnir_api::transport::AccountTokenAuthority::new(Box::new(store), issuer);
    assert!(
        authority.sign_in(7, "correct horse", 0.0).is_ok(),
        "the provisioned passphrase must sign in"
    );
    assert!(
        authority.sign_in(7, "wrong", 0.0).is_err(),
        "a wrong passphrase must not"
    );
}

#[test]
fn a_second_account_for_the_same_operator_is_refused_unless_replacement_is_asked_for() {
    let dir = scratch("replace");
    let file = dir.join("accounts.json").to_string_lossy().into_owned();

    let (code, _, _) = run_account(&["add", file.as_str(), "3", "planner"], Some("first"));
    assert_eq!(code, 0);

    // Without --replace this must refuse, and must say the file is unchanged: silently
    // overwriting a credential is how an account is taken over rather than provisioned.
    let (code, _, err) = run_account(&["add", file.as_str(), "3", "planner"], Some("second"));
    assert_ne!(code, 0, "a duplicate must be refused");
    assert!(
        err.contains("--replace"),
        "the refusal must say the way out: {err}"
    );

    let store =
        gungnir_security::FileAccountStore::open(std::path::Path::new(&file)).expect("open");
    let issuer = gungnir_security::TokenIssuer::new(
        b"a-signing-key-of-adequate-length-0123456789".to_vec(),
        60.0,
    )
    .expect("issuer");
    let authority = gungnir_api::transport::AccountTokenAuthority::new(Box::new(store), issuer);
    assert!(
        authority.sign_in(3, "first", 0.0).is_ok(),
        "the refused write must have left the original passphrase in force"
    );

    let (code, said, _) = run_account(
        &["add", file.as_str(), "3", "planner", "--replace"],
        Some("second"),
    );
    assert_eq!(code, 0);
    assert!(said.contains("replaced"), "it must say it replaced: {said}");
}

#[test]
fn an_empty_passphrase_is_refused_and_writes_nothing() {
    let dir = scratch("empty");
    let path = dir.join("accounts.json");
    let file = path.to_string_lossy().into_owned();

    let (code, _, err) = run_account(&["add", file.as_str(), "1", "operator"], Some("   \n"));
    assert_ne!(code, 0, "an empty passphrase must be refused");
    assert!(
        err.contains("standard input"),
        "the refusal must say where it looked: {err}"
    );
    assert!(
        !path.exists(),
        "a refused provisioning must not leave a file behind"
    );
}

#[test]
fn a_corrupt_account_file_is_refused_rather_than_treated_as_empty() {
    let dir = scratch("corrupt");
    let path = dir.join("accounts.json");
    std::fs::write(&path, "{ this is not a list of accounts").expect("written");
    let file = path.to_string_lossy().into_owned();

    let (code, _, err) = run_account(&["add", file.as_str(), "9", "analyst"], Some("passphrase"));
    assert_ne!(code, 0, "a corrupt file must be refused");
    assert!(
        err.contains("Nothing was changed"),
        "the refusal must say the file was left alone: {err}"
    );
    // Treating a corrupt file as "no accounts" would discard every account in it on the
    // next write, which is the failure this refusal exists to prevent.
    let after = std::fs::read_to_string(&path).expect("still there");
    assert_eq!(after, "{ this is not a list of accounts");
}

#[test]
fn an_unknown_role_is_refused_and_names_the_ones_that_exist() {
    let dir = scratch("role");
    let file = dir.join("accounts.json").to_string_lossy().into_owned();
    let (code, _, err) = run_account(&["add", file.as_str(), "1", "wizard"], Some("passphrase"));
    assert_ne!(code, 0);
    assert!(
        err.contains("security-officer"),
        "it must list the roles: {err}"
    );
}

#[test]
fn listing_shows_operators_and_roles_and_never_the_hash() {
    let dir = scratch("list");
    let file = dir.join("accounts.json").to_string_lossy().into_owned();
    run_account(&["add", file.as_str(), "4", "commander"], Some("one"));
    run_account(&["add", file.as_str(), "5", "sensor-manager"], Some("two"));

    let (code, said, _) = run_account(&["list", file.as_str()], None);
    assert_eq!(code, 0);
    assert!(said.contains("operator 4 as Commander"), "{said}");
    assert!(said.contains("operator 5 as SensorManager"), "{said}");
    assert!(
        !said.contains("$argon2"),
        "a listing must never repeat the hash: {said}"
    );
}
