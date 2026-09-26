// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The node's audit record against the heads its journal holds (GAP-163, D-104) and aged
//! out by the baseline's policy (GAP-152, D-105), through the functions `main.rs` calls:
//! [`gungnir_node::audit_record`] over a real journal, a real bus and a real
//! `FileAuditLog` synced as the node syncs it, once a tick.

use gungnir_eventing::{Envelope, Event, EventBus as _, InProcessBus, Receiver};
use gungnir_model::events::AuditEvent;
use gungnir_model::{MissionTime, SessionId};
use gungnir_node::audit_record::{self, AuditRetention};
use gungnir_security::audit::events;
use gungnir_security::{
    AuditEntry, AuditLog, AuditSync, FileAuditLog, OperatorId, ANCHOR_EVERY_ENTRIES,
};
use gungnir_store::retention::RetentionPolicy;
use gungnir_store::{EventJournal, FileEventJournal};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

const NOW: MissionTime = MissionTime(1_000.0);

fn data_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-node-audit-record-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// One node process: its journal, its bus, the journal's subscription, its audit log.
struct Node {
    journal: FileEventJournal,
    bus: InProcessBus,
    rx: Receiver<Envelope>,
    audit: FileAuditLog,
    session: SessionId,
    audit_dir: PathBuf,
}

impl Node {
    fn start(dir: &Path, session: u64) -> Self {
        let bus = InProcessBus::new();
        let rx = bus.subscribe();
        let audit_dir = dir.join(gungnir_security::AUDIT_DIR);
        Self {
            journal: FileEventJournal::open(dir).expect("journal"),
            bus,
            rx,
            audit: FileAuditLog::open(&audit_dir, AuditSync::OnFlush, NOW.0).expect("audit log"),
            session: SessionId(session),
            audit_dir,
        }
    }

    /// What the loop does at the end of a tick: append the bus to the journal.
    fn drain(&mut self) {
        for envelope in self.rx.try_iter() {
            self.journal
                .append(self.session, &envelope)
                .expect("journaled");
        }
    }

    /// `n` entries in one tick, synced once and anchored if due, as `audit_routes` does.
    fn tick(&mut self, n: u64, at: Instant) {
        for i in 0..n {
            #[allow(clippy::cast_precision_loss)]
            self.audit.record(AuditEntry::new(
                Some(OperatorId(3)),
                "sensor.task",
                i as f64,
                format!("task {i}"),
            ));
        }
        self.audit.flush().expect("synced");
        if let Some(head) = self.audit.take_anchor(at) {
            audit_record::publish_anchor(&self.bus, head, false, NOW);
        }
        self.drain();
    }

    /// What the loop does at shutdown.
    fn close(mut self) -> String {
        let head = self.audit.take_closing_anchor().expect("a closing head");
        audit_record::publish_anchor(&self.bus, head.clone(), true, NOW);
        self.drain();
        head.segment
    }

    fn verify(&mut self) -> usize {
        let problems = audit_record::verify_at_start(
            &self.journal,
            &self.audit_dir,
            &mut self.audit,
            &self.bus,
            NOW,
        )
        .expect("published");
        self.audit.flush().expect("synced");
        self.drain();
        problems
    }

    fn retention(&mut self, policy: RetentionPolicy, days: u64) -> usize {
        audit_record::apply_audit_retention(
            AuditRetention {
                audit_dir: &self.audit_dir,
                audit: &mut self.audit,
                journal: &mut self.journal,
                journal_rx: &self.rx,
                session: self.session,
                bus: &self.bus,
            },
            Some(&policy),
            SystemTime::now() + Duration::from_hours(days * 24),
            NOW,
        )
        .expect("a policy was given")
        .purged
        .len()
    }
}

fn journaled(dir: &Path, session: u64) -> Vec<AuditEvent> {
    FileEventJournal::open(dir)
        .expect("a second reader")
        .read_session(SessionId(session))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|e| match e.event {
            Event::Audit(a) => Some(a),
            _ => None,
        })
        .collect()
}

fn cut_lines(path: &Path, n: usize) {
    let text = std::fs::read_to_string(path).expect("read");
    let mut lines: Vec<&str> = text.lines().collect();
    lines.truncate(lines.len() - n);
    std::fs::write(path, lines.join("\n") + "\n").expect("cut");
}

#[test]
fn a_burst_is_anchored_by_count_and_a_cut_after_it_is_named_at_the_next_start() {
    let dir = data_dir("cut");
    let mut node = Node::start(&dir, 1);
    assert_eq!(node.verify(), 0, "an empty record verifies");
    // A flood's worth in one tick: anchored by count, without waiting for the interval.
    node.tick(ANCHOR_EVERY_ENTRIES + 6, Instant::now());
    let anchored = journaled(&dir, 1)
        .into_iter()
        .filter_map(|e| match e {
            AuditEvent::Anchored {
                head,
                closing: false,
                ..
            } => Some(head.entries),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(anchored, vec![ANCHOR_EVERY_ENTRIES + 6]);
    let segment = node.close();

    cut_lines(&dir.join(gungnir_security::AUDIT_DIR).join(&segment), 5);
    let mut node = Node::start(&dir, 2);
    assert_eq!(node.verify(), 1);
    let found = journaled(&dir, 2)
        .into_iter()
        .find_map(|e| match e {
            AuditEvent::Verified { findings, .. } => Some(findings),
            _ => None,
        })
        .expect("the verification is journaled");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].contains(&segment) && found[0].contains("5 entries are missing"),
        "{found:?}"
    );
    let entry = node
        .audit
        .entries()
        .iter()
        .find(|e| e.action == events::ANCHOR_MISMATCH)
        .expect("the finding is in the audit record itself");
    assert!(entry.detail.contains(&segment));
    drop(node);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_run_killed_after_its_last_head_verifies_and_a_deleted_segment_does_not() {
    let dir = data_dir("killed");
    let mut node = Node::start(&dir, 1);
    node.tick(3, Instant::now() + 2 * gungnir_security::ANCHOR_INTERVAL);
    assert!(
        journaled(&dir, 1)
            .iter()
            .any(|e| matches!(e, AuditEvent::Anchored { head, .. } if head.entries == 3)),
        "a head is due by the interval"
    );
    // Two more entries, synced, and the process gone before any head covers them.
    node.tick(2, Instant::now());
    drop(node);
    let mut node = Node::start(&dir, 2);
    assert_eq!(node.verify(), 0);
    node.tick(1, Instant::now());
    let second = node.close();

    // The first run's segment removed by hand: named, as the inventory knew it.
    let first = "audit-000001.jsonl";
    assert_ne!(first, second);
    std::fs::remove_file(dir.join(gungnir_security::AUDIT_DIR).join(first)).expect("removed");
    let mut node = Node::start(&dir, 3);
    assert_eq!(node.verify(), 1);
    let findings = journaled(&dir, 3)
        .into_iter()
        .find_map(|e| match e {
            AuditEvent::Verified { findings, .. } => Some(findings),
            _ => None,
        })
        .expect("journaled");
    assert!(
        findings[0].contains(first) && findings[0].contains("is gone"),
        "{findings:?}"
    );
    drop(node);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn retention_journals_each_purge_before_the_file_goes_and_honours_a_session_hold() {
    let dir = data_dir("retention");
    let policy = RetentionPolicy {
        max_session_age_days: 3_650,
        max_audit_log_age_days: 30,
    };
    for session in 1..=3 {
        let mut node = Node::start(&dir, session);
        node.verify();
        node.tick(2, Instant::now());
        node.close();
    }
    // Session 1's run is under review: its segment stays, and the purge stops there.
    FileEventJournal::open(&dir)
        .expect("reader")
        .hold(SessionId(1), "after-action review")
        .expect("held");
    let mut node = Node::start(&dir, 4);
    node.verify();
    assert_eq!(node.retention(policy, 40), 0);
    assert!(FileEventJournal::open(&dir)
        .expect("reader")
        .release(SessionId(1))
        .expect("released"));

    // Released: the two oldest go, the newest (the chain's head) stays.
    assert_eq!(node.retention(policy, 40), 2);
    let purged: Vec<String> = journaled(&dir, 4)
        .into_iter()
        .filter_map(|e| match e {
            AuditEvent::Purged { segment, .. } => Some(segment),
            _ => None,
        })
        .collect();
    assert_eq!(purged, vec!["audit-000001.jsonl", "audit-000002.jsonl"]);
    assert_eq!(
        node.audit
            .entries()
            .iter()
            .filter(|e| e.action == events::PURGED)
            .count(),
        2
    );
    node.close();

    // What the purge removed is not a deletion to the next start.
    let mut node = Node::start(&dir, 5);
    assert_eq!(node.verify(), 0);
    drop(node);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_baseline_without_a_policy_purges_nothing() {
    let dir = data_dir("none");
    let mut node = Node::start(&dir, 1);
    node.tick(1, Instant::now());
    let report = audit_record::apply_audit_retention(
        AuditRetention {
            audit_dir: &node.audit_dir,
            audit: &mut node.audit,
            journal: &mut node.journal,
            journal_rx: &node.rx,
            session: node.session,
            bus: &node.bus,
        },
        None,
        SystemTime::now() + Duration::from_hours(4_000 * 24),
        NOW,
    );
    assert!(report.is_none());
    assert_eq!(
        std::fs::read_dir(&node.audit_dir).expect("listed").count(),
        1,
        "the one segment is still there"
    );
    drop(node);
    let _ = std::fs::remove_dir_all(dir);
}
