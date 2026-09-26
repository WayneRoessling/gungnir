// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The desktop's audit record against the heads its journal holds (GAP-163, D-104), aged
//! out by the baseline's policy (GAP-152, D-105), and read back on PN-20 (D-106).
//!
//! Every test runs real desktops one after another on one data directory, as a watch
//! floor restarts them, and damages the audit directory between runs the way somebody
//! able to write it could.

use gungnir_app::audit_record;
use gungnir_app::retention;
use gungnir_app::state::AppState;
use gungnir_config::ConfigBaseline;
use gungnir_eventing::Event;
use gungnir_model::events::AuditEvent;
use gungnir_security::audit::events;
use gungnir_security::{AuditLog, SegmentState, ANCHOR_INTERVAL};
use gungnir_store::retention::RetentionPolicy;
use gungnir_store::{EventJournal, FileEventJournal};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

fn data_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-app-audit-record-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn config(dir: &Path, retention: Option<RetentionPolicy>) -> ConfigBaseline {
    ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        retention,
        ..ConfigBaseline::default()
    }
}

fn start(dir: &Path) -> AppState {
    start_with(dir, None)
}

/// A desktop started and through its first tick, which is when it verifies its audit
/// record and first applies retention.
fn start_with(dir: &Path, retention: Option<RetentionPolicy>) -> AppState {
    let mut state = AppState::with_config(config(dir, retention)).expect("the desktop starts");
    gungnir_app::update::tick(&mut state);
    state
}

/// `n` audited acts, as a person at the console takes them.
fn acts(state: &mut AppState, n: usize) {
    for i in 0..n {
        gungnir_app::audit::record(state, "plan.decide", format!("act {i}"));
    }
}

/// A run that closes cleanly after `n` acts, returning its segment's name.
fn closed_run(dir: &Path, n: usize) -> String {
    let mut state = start(dir);
    acts(&mut state, n);
    let segment = segment_of(&state);
    state.close_session().expect("closed");
    segment
}

fn segment_of(state: &AppState) -> String {
    state
        .audit
        .segment()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .expect("this run wrote a segment")
}

fn cut_lines(path: &Path, n: usize) {
    let text = std::fs::read_to_string(path).expect("read");
    let mut lines: Vec<&str> = text.lines().collect();
    lines.truncate(lines.len() - n);
    std::fs::write(path, lines.join("\n") + "\n").expect("cut");
}

fn audit_dir(dir: &Path) -> PathBuf {
    dir.join(gungnir_security::AUDIT_DIR)
}

/// Every audit event the journal in `dir` holds, in order, read by a second reader.
fn journaled(dir: &Path) -> Vec<AuditEvent> {
    let journal = FileEventJournal::open(dir).expect("a second reader");
    let mut sessions = journal.sessions().expect("listed");
    sessions.sort_unstable_by_key(|s| s.0);
    sessions
        .into_iter()
        .flat_map(|s| journal.read_session(s).expect("read"))
        .filter_map(|e| match e.event {
            Event::Audit(a) => Some(a),
            _ => None,
        })
        .collect()
}

fn verified(state: &AppState) -> &gungnir_security::AuditVerification {
    match &state.audit_record.last {
        Some(Ok(v)) => v,
        other => panic!("no verification: {other:?}"),
    }
}

#[test]
fn an_intact_record_verifies_across_restarts_and_the_close_is_journaled() {
    let dir = data_dir("intact");
    let first = closed_run(&dir, 3);
    let state = start(&dir);
    let v = verified(&state);
    assert!(v.intact(), "{:?}", v.problems());
    assert!(
        state.alerts.iter().all(|a| !a.contains("Audit record")),
        "{:?}",
        state.alerts
    );
    assert_eq!(v.reports[0].segment, first);
    assert_eq!(v.reports[0].state, SegmentState::Anchored { beyond: 0 });
    // The closing head, then this start's inventory, are on the journal.
    let mut state = state;
    state.save_session().expect("saved");
    let events = journaled(&dir);
    assert!(events.iter().any(|e| matches!(
        e,
        AuditEvent::Anchored { head, closing: true, .. } if head.segment == first && head.entries == 3
    )));
    assert!(events
        .iter()
        .any(|e| matches!(e, AuditEvent::Verified { findings, .. } if findings.is_empty())));
    let text = audit_record::record_text(&state);
    assert!(
        text.sound && text.summary.contains("verifies"),
        "{}",
        text.summary
    );
    drop(state);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_cut_tail_is_named_with_its_missing_count_at_every_start() {
    let dir = data_dir("cut");
    let segment = closed_run(&dir, 6);
    cut_lines(&audit_dir(&dir).join(&segment), 2);

    // The chain alone still verifies: the gap this closes.
    assert!(gungnir_security::verify_audit_dir(&audit_dir(&dir))
        .expect("read")
        .intact());

    let mut state = start(&dir);
    let alert = state
        .alerts
        .iter()
        .find(|a| a.contains("Audit record damaged"))
        .unwrap_or_else(|| panic!("no alert: {:?}", state.alerts));
    assert!(
        alert.contains(&segment) && alert.contains("2 entries are missing"),
        "{alert}"
    );
    // Inside the record too, once.
    let mismatches: Vec<_> = state
        .audit
        .entries()
        .iter()
        .filter(|e| e.action == events::ANCHOR_MISMATCH)
        .collect();
    assert_eq!(mismatches.len(), 1);
    assert!(mismatches[0].detail.contains(&segment));
    // PN-20 says it in the warning colour, never muted.
    let text = audit_record::record_text(&state);
    assert!(
        !text.sound && text.summary.contains("DAMAGED"),
        "{}",
        text.summary
    );
    assert!(text
        .segments
        .iter()
        .any(|(d, sound, _)| !sound && d.contains("CUT: 2 entries missing")));
    state.close_session().expect("closed");

    // The next start finds it again: the inventory kept the head it should reach.
    let state = start(&dir);
    assert!(
        verified(&state)
            .problems()
            .iter()
            .any(|p| p.contains(&segment) && p.contains("2 entries are missing")),
        "{:?}",
        verified(&state).problems()
    );
    drop(state);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn entries_after_the_last_head_verify_whether_the_run_closed_or_not() {
    let dir = data_dir("beyond");
    // A periodic head, two more acts, and the process gone without closing.
    let mut state = start(&dir);
    acts(&mut state, 3);
    audit_record::tick_at(&mut state, Instant::now() + ANCHOR_INTERVAL);
    acts(&mut state, 2);
    state.save_session().expect("journal on disk");
    drop(state);
    let state = start(&dir);
    let v = verified(&state);
    assert!(v.intact(), "{:?}", v.problems());
    assert_eq!(v.reports[0].state, SegmentState::Anchored { beyond: 2 });
    drop(state);

    // The same, then a clean close: the closing head covers them.
    let mut state = start(&dir);
    acts(&mut state, 3);
    audit_record::tick_at(&mut state, Instant::now() + ANCHOR_INTERVAL);
    acts(&mut state, 2);
    let segment = segment_of(&state);
    state.close_session().expect("closed");
    let state = start(&dir);
    let v = verified(&state);
    assert!(v.intact(), "{:?}", v.problems());
    let report = v
        .reports
        .iter()
        .find(|r| r.segment == segment)
        .expect("listed");
    assert_eq!(report.state, SegmentState::Anchored { beyond: 0 });
    drop(state);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_deleted_segment_is_reported_by_name() {
    let dir = data_dir("deleted");
    let first = closed_run(&dir, 2);
    let _second = closed_run(&dir, 2);
    std::fs::remove_file(audit_dir(&dir).join(&first)).expect("deleted");
    let state = start(&dir);
    assert!(
        state
            .alerts
            .iter()
            .any(|a| a.contains(&first) && a.contains("is gone")),
        "{:?}",
        state.alerts
    );
    drop(state);
    let _ = std::fs::remove_dir_all(dir);
}

fn policy(audit_days: u32) -> RetentionPolicy {
    RetentionPolicy {
        max_session_age_days: 3_650,
        max_audit_log_age_days: audit_days,
    }
}

#[test]
fn retention_purges_old_segments_on_the_record_and_verification_reads_them_as_purged() {
    let dir = data_dir("purge");
    let first = closed_run(&dir, 2);
    let second = closed_run(&dir, 2);
    let third = closed_run(&dir, 2);

    // Without a policy nothing goes, whatever the age.
    let mut state = start(&dir);
    retention::run(
        &mut state,
        SystemTime::now() + Duration::from_hours(400 * 24),
    );
    assert_eq!(state.audit_record.purged_total, 0);
    state.close_session().expect("closed");

    let mut state = start_with(&dir, Some(policy(30)));
    retention::run(
        &mut state,
        SystemTime::now() + Duration::from_hours(40 * 24),
    );
    // Oldest first; the newest, which holds the chain's head, is kept.
    assert!(!audit_dir(&dir).join(&first).exists());
    assert!(!audit_dir(&dir).join(&second).exists());
    assert!(audit_dir(&dir).join(&third).exists());
    assert_eq!(state.audit_record.purged_total, 2);
    assert!(state
        .alerts
        .iter()
        .any(|a| a.contains("Retention removed 2 audit segment(s)") && a.contains(&first)));
    // Each removal is an entry in the audit log and an event on the journal.
    let purged: Vec<_> = state
        .audit
        .entries()
        .iter()
        .filter(|e| e.action == events::PURGED)
        .collect();
    assert_eq!(purged.len(), 2);
    assert!(journaled(&dir).iter().any(
        |e| matches!(e, AuditEvent::Purged { segment, max_audit_log_age_days: 30, .. } if *segment == first)
    ));
    state.close_session().expect("closed");

    // A purge is not a deletion: the next start reports nothing.
    let state = start(&dir);
    let v = verified(&state);
    assert!(v.intact(), "{:?}", v.problems());
    drop(state);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_held_session_keeps_the_segment_its_run_wrote() {
    let dir = data_dir("held");
    let mut state = start(&dir);
    acts(&mut state, 2);
    let held_session = state.session().expect("live");
    state.close_session().expect("closed");
    closed_run(&dir, 1);
    FileEventJournal::open(&dir)
        .expect("the administrator's view of the journal")
        .hold(held_session, "after-action review")
        .expect("held");
    let mut state = start_with(&dir, Some(policy(30)));
    retention::run(
        &mut state,
        SystemTime::now() + Duration::from_hours(40 * 24),
    );
    assert_eq!(state.audit_record.purged_total, 0, "{:?}", state.alerts);
    // Released, the same segment goes: it was the hold that kept it.
    assert!(FileEventJournal::open(&dir)
        .expect("reader")
        .release(held_session)
        .expect("released"));
    retention::run(
        &mut state,
        SystemTime::now() + Duration::from_hours(40 * 24),
    );
    assert_eq!(state.audit_record.purged_total, 1, "{:?}", state.alerts);
    drop(state);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn pn20_shows_an_earlier_run_read_only_with_its_state() {
    let dir = data_dir("pn20");
    let first = closed_run(&dir, 3);
    let mut state = start(&dir);
    let index = verified(&state)
        .reports
        .iter()
        .position(|r| r.segment == first)
        .expect("listed");
    audit_record::show(&mut state, index);
    let lines = audit_record::shown_lines(&state);
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[2].detail, "act 2");
    let text = audit_record::record_text(&state);
    assert!(text.shown_sound, "{}", text.shown_note);
    // On demand, from the panel: a second verification, journaled like the first.
    gungnir_app::session::apply(
        &mut state,
        &mut gungnir_ui::panels::audit::SignInDraft::default(),
        gungnir_ui::panels::audit::SessionAction::VerifyAuditRecord,
    );
    state.save_session().expect("saved");
    let inventories = journaled(&dir)
        .into_iter()
        .filter(|e| matches!(e, AuditEvent::Verified { .. }))
        .count();
    assert!(inventories >= 3, "two starts and one on demand");
    audit_record::hide(&mut state);
    assert!(audit_record::shown_lines(&state).is_empty());
    drop(state);
    let _ = std::fs::remove_dir_all(dir);
}
