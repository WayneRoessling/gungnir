// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A node does not start on a baseline the pinned key did not sign (GAP-210, DN-22 §15).
//!
//! Driven through the binary, as `account_provisioning.rs` is: the key is made by
//! `baseline-key provision`, the file is signed by `baseline sign`, and the refusal is the
//! node's own exit, so the path from the key to the refusal is the one a deployment runs.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use gungnir_config::{BaselineSignature, ConfigBaseline};
use gungnir_security::baseline_signing::{BaselineTrust, BaselineVerifyingKey, BASELINE_KEY_ENV};

const PASSPHRASE: &str = "a baseline passphrase";

static SCRATCH: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-node-baseline-{name}-{}-{}",
        std::process::id(),
        SCRATCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// Run the node with `args` and `stdin`, in `cwd`, returning (code, stdout, stderr). A
/// node that is still running after a minute was not refused, and is killed.
fn run(cwd: &Path, args: &[&str], stdin: &str, key: Option<&Path>) -> (i32, String, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gungnir-node"));
    command
        .args(args)
        .current_dir(cwd)
        .env_remove(BASELINE_KEY_ENV)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(key) = key {
        command.env(BASELINE_KEY_ENV, key);
    }
    let mut child = command.spawn().expect("spawned");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(stdin.as_bytes())
        .expect("wrote stdin");
    drop(child.stdin.take());
    let deadline = Instant::now() + Duration::from_secs(60);
    while child.try_wait().expect("polled").is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let out = child.wait_with_output().expect("waited");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// The key provisioned through the binary, its public half pinned to a file.
fn provision(dir: &Path) -> PathBuf {
    let keys = dir.join("keys");
    let (code, said, err) = run(
        dir,
        &["baseline-key", "provision", &keys.to_string_lossy()],
        PASSPHRASE,
        None,
    );
    assert_eq!(code, 0, "provisioning failed: {err}");
    let pem = &said[said.find("-----BEGIN").expect("a PEM in what it printed")..];
    let pinned = dir.join("baseline-key.pem");
    std::fs::write(&pinned, pem).expect("pinned");

    // A second provision never replaces the key every host already pins.
    let (code, _, err) = run(
        dir,
        &["baseline-key", "provision", &keys.to_string_lossy()],
        PASSPHRASE,
        None,
    );
    assert_ne!(code, 0, "a second provision replaced the key");
    assert!(!err.is_empty());
    pinned
}

#[test]
fn a_node_refuses_an_unsigned_baseline_and_runs_one_the_cli_signed() {
    let dir = scratch("refuse");
    let pinned = provision(&dir);
    let path = dir.join("baseline.json");
    let file = path.to_string_lossy().into_owned();
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&ConfigBaseline::default()).expect("encodes"),
    )
    .expect("written");

    // Unsigned, with a key pinned: the node stops before it opens a journal.
    let (code, _, err) = run(&dir, &[&file], "", Some(&pinned));
    assert_ne!(code, 0, "the node started on an unsigned baseline: {err}");
    assert!(
        err.contains("UNSIGNED"),
        "the refusal did not say why: {err}"
    );
    assert!(
        !dir.join("gungnir-journal").exists(),
        "a refused node opened its journal"
    );

    // Signed through the CLI, the same file is admitted, by the pinned key.
    let keys = dir.join("keys");
    let (code, said, err) = run(
        &dir,
        &["baseline", "sign", &file, &keys.to_string_lossy()],
        PASSPHRASE,
        None,
    );
    assert_eq!(code, 0, "signing failed: {err}");
    assert!(BaselineSignature::path_for(&path).exists(), "{said}");
    let key = BaselineVerifyingKey::from_file(&pinned).map(Some);
    let (_, trust) = gungnir_node::baseline::load(&file, &key).expect("a signed baseline loads");
    let fingerprint = key
        .as_ref()
        .ok()
        .and_then(Option::as_ref)
        .map(|k| k.fingerprint().to_owned());
    assert!(
        matches!(&trust, BaselineTrust::Signed { fingerprint: f, .. } if Some(f) == fingerprint.as_ref()),
        "{trust:?}"
    );
    assert!(
        said.contains(fingerprint.as_deref().unwrap_or("?")),
        "{said}"
    );

    // A wrong passphrase signs nothing.
    let (code, _, _) = run(
        &dir,
        &["baseline", "sign", &file, &keys.to_string_lossy()],
        "not the passphrase",
        None,
    );
    assert_ne!(code, 0, "a wrong passphrase signed the baseline");

    // Changed after signing, it is refused again; with no key pinned it loads and says so.
    let mut edited = ConfigBaseline::default();
    edited.revision += 1;
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&edited).expect("encodes"),
    )
    .expect("written");
    let refused = gungnir_node::baseline::load(&file, &key).expect_err("a changed baseline");
    assert!(refused.to_string().contains("does NOT verify"), "{refused}");
    let (_, trust) = gungnir_node::baseline::load(&file, &Ok(None)).expect("no key pinned");
    assert_eq!(trust, BaselineTrust::NoKeyPinned);

    // A pin that does not read is refused, not read as no pin.
    let unreadable = Err(format!("{BASELINE_KEY_ENV} names nothing"));
    assert!(gungnir_node::baseline::load(&file, &unreadable).is_err());

    let _ = std::fs::remove_dir_all(&dir);
}
