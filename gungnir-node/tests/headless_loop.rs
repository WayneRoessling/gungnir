// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The node's own run cycle, end to end (GAP-110): start on the default config, journal
//! at least one envelope, report health, interrupt it, and exit cleanly.
//!
//! Until this test existed, that cycle was checked only by a person running the binary
//! (`verification-capability-table.md` §2, the `gungnir-node` Headless loop row). This
//! drives the real binary, over a real interrupt, because the thing at risk is exactly
//! what a person watching a terminal would notice and a unit test cannot: the process
//! actually starting, actually responding to Ctrl-C, and actually flushing the journal
//! before it exits, rather than being killed out from under an unfinished write.
//!
//! **`#[cfg(unix)]`, not both platforms.** `gungnir-node` stops on
//! `tokio::signal::ctrl_c()` (`CLAUDE.md`), and on Unix `kill -INT <pid>` delivers
//! exactly that signal -- the standard, reliable mechanism this test uses. On Windows,
//! `CLAUDE.md` already says a smoke run from Git Bash cannot signal the node at all and
//! has to be stopped with `taskkill`, which is a hard kill and would prove nothing about
//! the shutdown path this test exists to check. Three genuine attempts at a real
//! `CTRL_C_EVENT` were tried building this file and are recorded in
//! `docs/record/2026-09-23/node-headless-loop-has-a-test.md` rather than left half
//! -working here: `GenerateConsoleCtrlEvent` targeting the child's own process group
//! (compiles, reports success, the child never reacts -- that event only supports group
//! 0); broadcasting to group 0 with `SetConsoleCtrlHandler(NULL, TRUE)` protecting this
//! test process (the protection did not hold -- this test's own process was killed,
//! `STATUS_CONTROL_C_EXIT`); and attaching to a console the child was given of its own
//! and broadcasting there (both processes survived, and the child still never reacted).
//! Shipping any of those as the Windows path would have been exactly the kind of claim
//! this workspace's culture refuses: a test that looks like it covers a platform and
//! does not. CI gates this row on `ubuntu-latest`, where the real mechanism is used.

#![cfg(unix)]

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn node_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_gungnir-node"))
}

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gungnir-node-headless-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// Sends this process's own `SIGINT` to `child` -- the same signal
/// `tokio::signal::ctrl_c()` waits on, never a hard kill. No new dependency for one
/// call: `kill` is on every Unix CI image and developer machine this workspace targets,
/// and `-INT` names the signal so this is never mistaken for `-9`.
fn interrupt(child: &Child) {
    let status = Command::new("kill")
        .arg("-INT")
        .arg(child.id().to_string())
        .status()
        .expect("kill runs");
    assert!(status.success(), "kill -INT {} failed", child.id());
}

fn spawn_node(cwd: &std::path::Path) -> Child {
    Command::new(node_binary())
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("gungnir-node spawns")
}

/// Reads `reader` line by line onto `tx`, so the caller can watch for a marker without
/// blocking the child on a full pipe buffer. Returns when the pipe closes.
fn pump(reader: impl std::io::Read + Send + 'static, tx: mpsc::Sender<String>) {
    std::thread::spawn(move || {
        for line in BufReader::new(reader).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                return;
            }
        }
    });
}

/// Waits up to `timeout` for a line matching `predicate` to arrive on `rx`, draining
/// (and keeping) every line seen along the way so a failure can show the whole log.
fn wait_for(
    rx: &mpsc::Receiver<String>,
    timeout: Duration,
    predicate: impl Fn(&str) -> bool,
) -> Vec<String> {
    let deadline = Instant::now() + timeout;
    let mut seen = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return seen;
        }
        match rx.recv_timeout(remaining.min(Duration::from_millis(100))) {
            Ok(line) => {
                let matched = predicate(&line);
                seen.push(line);
                if matched {
                    return seen;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return seen,
        }
    }
}

/// GAP-110's own criterion: start on the default config in a scratch directory, let it
/// run, interrupt it, and check the three things a person watching the terminal would --
/// it started, it stopped cleanly, and the journal holds what it saw.
#[test]
fn starts_journals_and_stops_cleanly_on_interrupt() {
    let dir = scratch();
    let mut child = spawn_node(&dir);

    let (tx, rx) = mpsc::channel();
    pump(child.stdout.take().expect("stdout piped"), tx.clone());
    pump(child.stderr.take().expect("stderr piped"), tx);

    // Confirms the node is actually up and past its own start-up logging, not merely
    // that the process exists -- `main.rs` logs this once the live session is open and
    // the loop is about to begin.
    let startup_log = wait_for(&rx, Duration::from_secs(20), |l| {
        l.contains("opened live session")
    });
    assert!(
        startup_log
            .iter()
            .any(|l| l.contains("opened live session")),
        "the node never logged that it had opened a live session: {startup_log:?}"
    );

    // At least one tick (50 ms) has to pass for the first health snapshot to journal;
    // generous so a slow CI runner is not what this test is measuring.
    std::thread::sleep(Duration::from_millis(500));

    interrupt(&child);

    let shutdown_log = wait_for(&rx, Duration::from_secs(20), |l| {
        l.contains("gungnir-node stopped; journal flushed and session closed")
    });
    assert!(
        shutdown_log
            .iter()
            .any(|l| l.contains("gungnir-node stopped; journal flushed and session closed")),
        "the node never logged a clean stop after the interrupt: {shutdown_log:?}"
    );

    let status = child.wait().expect("node process waited on");
    assert!(
        status.success(),
        "gungnir-node did not exit cleanly: {status:?}"
    );

    // The journal a person watching would have expected: one session, at least one
    // envelope in it (the first health snapshot, with zero sensors configured).
    let journal =
        gungnir_store::FileEventJournal::open(dir.join("gungnir-journal")).expect("journal opens");
    let sessions = gungnir_store::EventJournal::sessions(&journal).expect("sessions readable");
    assert_eq!(
        sessions.len(),
        1,
        "expected exactly one session: {sessions:?}"
    );
    let envelopes =
        gungnir_store::EventJournal::read_session(&journal, sessions[0]).expect("session readable");
    assert!(
        !envelopes.is_empty(),
        "the journal holds no envelopes from a session that ran"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
