// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! GAP-204: the desktop provisions its own accounts (DN-23; plan 12 tranche A).
//!
//! The defect was that the only writer of an account file shipped in the Linux node
//! binary, so a Windows desktop in the field ran with nobody signed in. These tests drive
//! the desktop binary itself, as the node's tests drive the node's, because the defect was
//! never in the provisioning code: it was that the desktop could not reach it. The binary
//! must answer without opening a window, a journal, or a configuration baseline.

use gungnir_security::SessionAuthority;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

static SCRATCH: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-app-account-{name}-{}-{}",
        std::process::id(),
        SCRATCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn desktop_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_gungnir-app"))
}

/// Run `gungnir-app account ...` with `stdin` supplied, returning (code, stdout, stderr).
fn run_account(args: &[&str], stdin: Option<&str>) -> (i32, String, String) {
    let mut child = Command::new(desktop_binary())
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
    drop(child.stdin.take());
    let out = child.wait_with_output().expect("waited");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// **The point of GAP-204.** An account the desktop binary writes is one the desktop's
/// own session authority signs in with, so a laptop with no node provisions itself.
#[test]
fn an_account_created_by_the_desktop_binary_is_one_the_desktop_signs_in_with() {
    let dir = scratch("roundtrip");
    let path = dir.join("accounts.json");
    let file = path.to_string_lossy().into_owned();

    let (code, said, err) = run_account(
        &["add", file.as_str(), "7", "commander"],
        Some("correct horse"),
    );
    assert_eq!(code, 0, "provisioning must succeed: {err}");
    assert!(said.contains("added operator 7"), "{said}");
    assert!(
        said.contains("recorded in the audit log"),
        "every account change is on the audit record: {said}"
    );
    let text = std::fs::read_to_string(&path).expect("written");
    assert!(!text.contains("correct horse"), "{text}");
    assert!(text.contains("$argon2"), "{text}");

    let store = gungnir_security::FileAccountStore::open(&path).expect("the store opens it");
    let mut authority = gungnir_security::LocalAccountAuthority::new(Box::new(store));
    let session = authority
        .sign_in(
            &gungnir_security::LocalAccountAuthority::credential(
                gungnir_security::OperatorId(7),
                "correct horse",
            ),
            0.0,
        )
        .expect("the provisioned passphrase signs in");
    assert_eq!(session.role, gungnir_security::Role::Commander);
    assert!(authority
        .sign_in(
            &gungnir_security::LocalAccountAuthority::credential(
                gungnir_security::OperatorId(7),
                "wrong"
            ),
            1.0,
        )
        .is_err());
}

/// The usage speaks as the desktop, and the command exits before anything the desktop
/// would otherwise start: no window, no journal directory, no baseline read.
#[test]
fn usage_names_the_desktop_and_nothing_else_starts() {
    let dir = scratch("usage");
    let (code, said, err) = run_account(&[], None);
    assert_eq!(code, 2, "a usage error exits 2: {said}{err}");
    assert!(err.contains("usage: gungnir-app account add"), "{err}");
    assert!(!err.contains("gungnir-node"), "{err}");
    assert!(
        std::fs::read_dir(&dir).expect("scratch").next().is_none(),
        "nothing was written anywhere near the command"
    );
}

#[test]
fn a_second_account_for_the_same_operator_is_refused_unless_replacement_is_asked_for() {
    let dir = scratch("replace");
    let file = dir.join("accounts.json").to_string_lossy().into_owned();
    let (code, ..) = run_account(&["add", file.as_str(), "7", "operator"], Some("one"));
    assert_eq!(code, 0);
    let (code, _, err) = run_account(&["add", file.as_str(), "7", "operator"], Some("two"));
    assert_eq!(
        code, 2,
        "a silent overwrite is a takeover, not provisioning"
    );
    assert!(err.contains("--replace"), "{err}");
    let (code, said, _) = run_account(
        &["add", file.as_str(), "7", "supervisor", "--replace"],
        Some("two"),
    );
    assert_eq!(code, 0);
    assert!(said.contains("replaced operator 7 as Supervisor"), "{said}");
    let (code, said, _) = run_account(&["list", file.as_str()], None);
    assert_eq!(code, 0);
    assert!(said.contains("operator 7 as Supervisor"), "{said}");
    assert!(
        !said.contains("$argon2"),
        "listing never shows the hash: {said}"
    );
}

#[test]
fn an_empty_passphrase_is_refused_and_writes_nothing() {
    let dir = scratch("empty");
    let path = dir.join("accounts.json");
    let file = path.to_string_lossy().into_owned();
    let (code, _, err) = run_account(&["add", file.as_str(), "7", "operator"], Some("  \n"));
    assert_eq!(code, 2);
    assert!(err.contains("no passphrase arrived"), "{err}");
    assert!(
        err.contains("gungnir-app account add"),
        "the example speaks as the desktop: {err}"
    );
    assert!(!path.exists(), "nothing was written");
}
