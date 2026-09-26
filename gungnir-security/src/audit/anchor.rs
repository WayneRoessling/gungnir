// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The audit log's head, held outside the file (GAP-163, D-104,
//! `docs/design/DN-23-operator-authentication.md` §14).
//!
//! # Why a head outside the file
//!
//! The hash chain finds an edited, removed, inserted or torn line, because every line
//! names the one before it. It cannot find the removal of the **last** lines: what is
//! left is a shorter chain that verifies. Nor can it find a tail rewritten whole, because
//! the chain is unkeyed and anyone able to write the file can recompute it. Both are
//! found by comparing the file with a head kept somewhere its writer cannot reach, and
//! the owner chose the event journal (2026-09-26): durable, append-only, sealed under the
//! deployment's key (DN-22), and already where each binary records what it did.
//!
//! # What the journal holds, and how it is read
//!
//! Three statements, each `gungnir_model::events::AuditEvent` on the journal and a
//! [`HeadStatement`] here:
//!
//! - **Anchored**: this run's segment reached this head. Periodically and at close.
//! - **Verified**: the inventory a verification left -- the head every segment is expected
//!   to reach from then on. It **supersedes everything before it**, so a reader walks the
//!   journal newest first and stops at the first session holding one
//!   ([`AnchorLedger::fold_newest_first`]): a start reads one run's journal back, not the
//!   deployment's life.
//! - **Purged**: retention removed this segment on purpose (D-105). A segment gone with
//!   no purge recorded is reported removed; one with a purge is not.
//!
//! A session that cannot be read -- sealed under an ephemeral key, or damaged -- is not
//! skipped in silence: it is named in [`super::AuditVerification::unread_sessions`], because
//! a head or a purge recorded in it was not seen.
//!
//! # Carried forward
//!
//! A segment found cut or gone keeps the head it was expected to reach in the inventory,
//! so every later verification finds it again. A cut is evidence; a verifier that
//! accepted what was left as the new baseline would report it once, at one start, and
//! never again.

use super::{
    file_name, retention, scan_segment, segment_number, segments, AuditVerification, SegmentHead,
};
use crate::SecurityError;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// One thing the journal says about the audit log's head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadStatement {
    /// A run's segment reached this head.
    Anchored(SegmentHead),
    /// A verification's inventory: the head every segment is expected to reach.
    /// Supersedes every statement before it.
    Verified(Vec<SegmentHead>),
    /// Retention removed this segment, by file name.
    Purged(String),
}

/// One journal session, as the host could read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionHeads {
    /// Its statements about the audit log, in the order it journaled them.
    Read(Vec<HeadStatement>),
    /// It could not be read, and why, naming the session.
    Unreadable(String),
}

/// What the journal says each segment should reach (GAP-163, D-104).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnchorLedger {
    expected: BTreeMap<String, SegmentHead>,
    unread: Vec<String>,
    sessions_read: usize,
}

impl AnchorLedger {
    /// Fold a journal's sessions, **newest first**, stopping after the first session
    /// that holds an inventory: that inventory already accounts for everything before
    /// it. The iterator is consumed no further, so a host that reads each session only
    /// when asked reads one run's journal back, however long the deployment has been up.
    pub fn fold_newest_first<I>(sessions: I) -> Self
    where
        I: IntoIterator<Item = SessionHeads>,
    {
        let mut read: Vec<Vec<HeadStatement>> = Vec::new();
        let mut unread = Vec::new();
        for session in sessions {
            match session {
                SessionHeads::Unreadable(why) => unread.push(why),
                SessionHeads::Read(statements) => {
                    let inventory = statements
                        .iter()
                        .any(|s| matches!(s, HeadStatement::Verified(_)));
                    read.push(statements);
                    if inventory {
                        break;
                    }
                }
            }
        }
        let mut ledger = Self {
            sessions_read: read.len(),
            unread,
            ..Self::default()
        };
        for statements in read.into_iter().rev() {
            for statement in statements {
                ledger.apply(statement);
            }
        }
        ledger
    }

    /// Apply one statement, in journal order.
    pub fn apply(&mut self, statement: HeadStatement) {
        match statement {
            HeadStatement::Anchored(head) => {
                self.expected.insert(head.segment.clone(), head);
            }
            HeadStatement::Verified(heads) => {
                self.expected = heads
                    .into_iter()
                    .map(|head| (head.segment.clone(), head))
                    .collect();
            }
            HeadStatement::Purged(segment) => {
                self.expected.remove(&segment);
            }
        }
    }

    /// The head the journal holds for a segment, by file name.
    #[must_use]
    pub fn expected(&self, segment: &str) -> Option<&SegmentHead> {
        self.expected.get(segment)
    }

    /// Whether the journal held any head at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.expected.is_empty()
    }

    /// Sessions that could not be read, each naming itself and why.
    #[must_use]
    pub fn unread(&self) -> &[String] {
        &self.unread
    }

    /// How many sessions the fold read.
    #[must_use]
    pub fn sessions_read(&self) -> usize {
        self.sessions_read
    }
}

/// Where the record does not reach what the journal holds for it (GAP-163).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnchorFinding {
    /// The segment holds fewer entries than the journal's head for it: its tail was cut.
    Cut {
        segment: String,
        anchored: u64,
        present: u64,
    },
    /// The segment holds at least as many entries, but the one at the head's position is
    /// not the head: it was rewritten from there or before.
    Diverged { segment: String, line: u64 },
    /// The journal holds a head for a segment that is gone, and no purge of it.
    Removed { segment: String, anchored: u64 },
}

impl AnchorFinding {
    /// The segment the finding names.
    #[must_use]
    pub fn segment(&self) -> &str {
        match self {
            Self::Cut { segment, .. }
            | Self::Diverged { segment, .. }
            | Self::Removed { segment, .. } => segment,
        }
    }
}

impl std::fmt::Display for AnchorFinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cut {
                segment,
                anchored,
                present,
            } => write!(
                f,
                "{segment} was cut: the journal holds its head at entry {anchored} and \
                 {present} remain, so {} entries are missing",
                anchored.saturating_sub(*present)
            ),
            Self::Diverged { segment, line } => write!(
                f,
                "{segment} line {line} is not the entry the journal holds as its head \
                 there: the segment was rewritten from that line or before it"
            ),
            Self::Removed { segment, anchored } => write!(
                f,
                "{segment} is gone: the journal holds a head of {anchored} entries for it \
                 and records no retention purge of it"
            ),
        }
    }
}

/// How one segment stands against the journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SegmentState {
    /// It reaches the last head the journal holds for it. `beyond` entries follow that
    /// head: written after its run's last anchor by a run that stopped without closing,
    /// which is the window D-104 accepts.
    Anchored { beyond: u64 },
    /// The journal holds no head for it yet -- a run that stopped before its first
    /// anchor, or a segment written before GAP-163. Its chain is checked; a cut of it
    /// would not show until the inventory this verification leaves anchors it.
    Unanchored,
    /// The segment this run is writing, anchored by this run as it goes.
    Current,
    /// Cut: `missing` entries the journal's head covers are not in it.
    Cut { missing: u64 },
    /// Rewritten at or before the head's line.
    Diverged { line: u64 },
    /// Gone with no purge on the journal.
    Removed,
    /// Being removed by a retention purge that was interrupted; the next purge finishes
    /// it and records it (D-105).
    PurgeInterrupted,
}

/// One segment, as a verification found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentReport {
    /// The file name.
    pub segment: String,
    /// Complete lines in it now.
    pub entries: u64,
    /// Chain breaks found inside it.
    pub breaks: usize,
    pub state: SegmentState,
}

impl SegmentReport {
    /// No break and no finding.
    #[must_use]
    pub fn sound(&self) -> bool {
        self.breaks == 0
            && matches!(
                self.state,
                SegmentState::Anchored { .. }
                    | SegmentState::Unanchored
                    | SegmentState::Current
                    | SegmentState::PurgeInterrupted
            )
    }

    /// The state in words, for the operator's audit panel.
    #[must_use]
    pub fn describe(&self) -> String {
        let breaks = match self.breaks {
            0 => String::new(),
            1 => "; its chain breaks once".to_owned(),
            n => format!("; its chain breaks {n} times"),
        };
        let state = match &self.state {
            SegmentState::Anchored { beyond: 0 } => {
                "verified to the head the journal holds".to_owned()
            }
            SegmentState::Anchored { beyond } => format!(
                "verified to the head the journal holds; the last {beyond} were written \
                 after it by a run that did not close"
            ),
            SegmentState::Unanchored => {
                "chain verified; the journal holds no head for it before this verification"
                    .to_owned()
            }
            SegmentState::Current => "this run's, anchored as it is written".to_owned(),
            SegmentState::Cut { missing } => format!("CUT: {missing} entries missing"),
            SegmentState::Diverged { line } => format!("REWRITTEN at or before line {line}"),
            SegmentState::Removed => "REMOVED without a retention purge".to_owned(),
            SegmentState::PurgeInterrupted => {
                "being purged; the next retention pass finishes it".to_owned()
            }
        };
        format!(
            "{}: {} entries, {state}{breaks}",
            self.segment, self.entries
        )
    }
}

/// Verify every segment in `dir` as [`super::verify_audit_dir`] does, and each against the
/// head `ledger` holds for it (GAP-163, D-104).
///
/// `current` is the segment this run is writing, when it has one: its chain is checked,
/// and it is not compared with a head, because its own run anchors it as it goes. It is
/// left out of the inventory unless the journal already holds a head for it, which is
/// carried so a verification on demand does not forget one.
///
/// # Errors
///
/// `SecurityError::AuditUnavailable` when the directory or a segment cannot be read.
pub fn verify_audit_record(
    dir: &Path,
    ledger: &AnchorLedger,
    current: Option<&Path>,
) -> Result<AuditVerification, SecurityError> {
    let mut verification = AuditVerification {
        unread_sessions: ledger.unread.clone(),
        ..AuditVerification::default()
    };
    let current = current.map(file_name);
    let purging: BTreeSet<String> = retention::purging(dir)?
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    let mut seen = BTreeSet::new();
    let mut last: Option<(u64, String)> = None;
    for (_, path) in segments(dir)? {
        let name = file_name(&path);
        seen.insert(name.clone());
        let is_current = current.as_deref() == Some(name.as_str());
        let expect = ledger.expected.get(&name);
        let probe = if is_current {
            None
        } else {
            expect.map(|e| e.entries)
        };
        let scan = scan_segment(&path, last.as_ref(), probe)?;
        verification.segments += 1;
        verification.entries += scan.entries;
        let breaks = scan.breaks.len();
        verification.breaks.extend(scan.breaks.iter().cloned());
        let state = if is_current {
            verification.heads.extend(expect.cloned());
            SegmentState::Current
        } else {
            compare(&mut verification, &name, &scan, expect)
        };
        verification.reports.push(SegmentReport {
            segment: name,
            entries: scan.entries,
            breaks,
            state,
        });
        if scan.entries > 0 {
            last = scan.last;
        }
    }
    for (name, expected) in &ledger.expected {
        if seen.contains(name) {
            continue;
        }
        let state = if purging.contains(name) {
            SegmentState::PurgeInterrupted
        } else {
            verification.findings.push(AnchorFinding::Removed {
                segment: name.clone(),
                anchored: expected.entries,
            });
            SegmentState::Removed
        };
        verification.heads.push(expected.clone());
        verification.reports.push(SegmentReport {
            segment: name.clone(),
            entries: 0,
            breaks: 0,
            state,
        });
    }
    let order = |name: &str| segment_number(name).unwrap_or(u64::MAX);
    verification
        .reports
        .sort_by_key(|r| (order(&r.segment), r.segment.clone()));
    verification
        .heads
        .sort_by_key(|h| (order(&h.segment), h.segment.clone()));
    verification
        .findings
        .sort_by_key(|f| (order(f.segment()), f.segment().to_owned()));
    Ok(verification)
}

/// One listed segment against the head the journal holds for it: its state, with any
/// finding and the head it is expected to reach from now on added to `verification`.
fn compare(
    verification: &mut AuditVerification,
    name: &str,
    scan: &super::SegmentScan,
    expect: Option<&SegmentHead>,
) -> SegmentState {
    let file_head = if scan.entries > 0 {
        scan.last.clone().map(|(seq, hash)| SegmentHead {
            segment: name.to_owned(),
            entries: scan.entries,
            seq,
            hash,
        })
    } else {
        None
    };
    match expect {
        None => {
            verification.heads.extend(file_head);
            SegmentState::Unanchored
        }
        Some(e) if scan.entries < e.entries => {
            verification.findings.push(AnchorFinding::Cut {
                segment: name.to_owned(),
                anchored: e.entries,
                present: scan.entries,
            });
            verification.heads.push(e.clone());
            SegmentState::Cut {
                missing: e.entries - scan.entries,
            }
        }
        Some(e) if scan.probed.as_ref() == Some(&(e.seq, e.hash.clone())) => {
            verification.heads.extend(file_head);
            SegmentState::Anchored {
                beyond: scan.entries - e.entries,
            }
        }
        Some(e) => {
            verification.findings.push(AnchorFinding::Diverged {
                segment: name.to_owned(),
                line: e.entries,
            });
            verification.heads.push(e.clone());
            SegmentState::Diverged { line: e.entries }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{AuditEntry, AuditLog, AuditSync, FileAuditLog};
    use super::*;
    use crate::OperatorId;
    use std::path::PathBuf;
    use std::time::Instant;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("gungnir-anchor-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn entry(i: u64) -> AuditEntry {
        #[allow(clippy::cast_precision_loss)]
        AuditEntry::new(
            Some(OperatorId(1)),
            "plan.decide",
            i as f64,
            format!("entry {i}"),
        )
    }

    /// One run: `n` entries, closed, returning the closing head.
    fn run(dir: &Path, n: u64) -> SegmentHead {
        let mut log = FileAuditLog::open(dir, AuditSync::EveryEntry, 0.0).expect("opened");
        for i in 0..n {
            log.record(entry(i));
        }
        log.take_closing_anchor().expect("a closing head")
    }

    fn cut_lines(path: &Path, n: usize) {
        let text = std::fs::read_to_string(path).expect("read");
        let mut lines: Vec<&str> = text.lines().collect();
        lines.truncate(lines.len() - n);
        std::fs::write(path, lines.join("\n") + "\n").expect("written");
    }

    #[test]
    fn a_cut_tail_is_found_naming_the_file_and_the_missing_count() {
        let dir = temp("cut");
        let head = run(&dir, 10);
        let mut ledger = AnchorLedger::default();
        ledger.apply(HeadStatement::Anchored(head.clone()));
        let before = verify_audit_record(&dir, &ledger, None).expect("read");
        assert!(before.intact(), "{:?}", before.problems());

        cut_lines(&dir.join(&head.segment), 3);
        // The chain alone cannot see it: this is the limit GAP-163 was filed for.
        assert!(super::super::verify_audit_dir(&dir).expect("read").intact());
        let after = verify_audit_record(&dir, &ledger, None).expect("read");
        assert!(!after.intact());
        assert_eq!(
            after.findings,
            vec![AnchorFinding::Cut {
                segment: head.segment.clone(),
                anchored: 10,
                present: 7
            }]
        );
        let said = after.findings[0].to_string();
        assert!(
            said.contains(&head.segment) && said.contains("3 entries are missing"),
            "{said}"
        );
        // Carried forward: the inventory keeps the head, so the next start finds it again.
        assert_eq!(after.heads, vec![head]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_tail_rewritten_with_a_fresh_chain_is_found_where_the_head_was() {
        let dir = temp("rewritten");
        let head = run(&dir, 4);
        let path = dir.join(&head.segment);
        cut_lines(&path, 2);
        // Anyone can recompute an unkeyed chain: append two new lines that verify.
        let text = std::fs::read_to_string(&path).expect("read");
        let last = text.lines().last().expect("a line");
        let (prefix, _) = super::super::split_line(last).expect("a line of the format");
        let mut forged = text.clone();
        let mut prev = prefix.hash;
        for seq in prefix.seq + 1..prefix.seq + 3 {
            let json = serde_json::to_string(&entry(90 + seq)).expect("encoded");
            let (line, hash) = super::super::chain_line(seq, &prev, &json);
            forged.push_str(&line);
            prev = hash;
        }
        std::fs::write(&path, forged).expect("written");
        assert!(super::super::verify_audit_dir(&dir).expect("read").intact());

        let mut ledger = AnchorLedger::default();
        ledger.apply(HeadStatement::Anchored(head.clone()));
        let found = verify_audit_record(&dir, &ledger, None).expect("read");
        assert_eq!(
            found.findings,
            vec![AnchorFinding::Diverged {
                segment: head.segment,
                line: 4
            }]
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_file_longer_than_its_last_head_verifies_and_says_how_much_is_beyond_it() {
        let dir = temp("beyond");
        let mut log = FileAuditLog::open(&dir, AuditSync::EveryEntry, 0.0).expect("opened");
        for i in 0..5 {
            log.record(entry(i));
        }
        let later = Instant::now() + super::super::ANCHOR_INTERVAL;
        let head = log.take_anchor(later).expect("due by the interval");
        for i in 5..8 {
            log.record(entry(i));
        }
        // The run stops without closing: the journal's head is older than the file.
        drop(log);
        let mut ledger = AnchorLedger::default();
        ledger.apply(HeadStatement::Anchored(head));
        let verified = verify_audit_record(&dir, &ledger, None).expect("read");
        assert!(verified.intact(), "{:?}", verified.problems());
        assert_eq!(
            verified.reports[0].state,
            SegmentState::Anchored { beyond: 3 }
        );
        assert_eq!(
            verified.heads[0].entries, 8,
            "the inventory takes the file's head"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_removed_file_is_named_and_a_purged_one_is_not() {
        let dir = temp("removed");
        let first = run(&dir, 3);
        let second = run(&dir, 2);
        let mut ledger = AnchorLedger::default();
        ledger.apply(HeadStatement::Verified(vec![first.clone(), second.clone()]));
        std::fs::remove_file(dir.join(&first.segment)).expect("removed");

        let found = verify_audit_record(&dir, &ledger, None).expect("read");
        assert_eq!(
            found.findings,
            vec![AnchorFinding::Removed {
                segment: first.segment.clone(),
                anchored: 3
            }]
        );
        assert!(found.findings[0].to_string().contains(&first.segment));

        ledger.apply(HeadStatement::Purged(first.segment.clone()));
        let purged = verify_audit_record(&dir, &ledger, None).expect("read");
        assert!(purged.intact(), "{:?}", purged.problems());
        assert_eq!(purged.heads, vec![second]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_fold_stops_at_the_newest_inventory_and_names_what_it_could_not_read() {
        let head = |segment: &str, entries| SegmentHead {
            segment: segment.into(),
            entries,
            seq: entries,
            hash: "x".into(),
        };
        let mut asked = 0;
        let sessions = vec![
            SessionHeads::Unreadable("session 9: sealed under a key this process lacks".into()),
            SessionHeads::Read(vec![
                HeadStatement::Verified(vec![head("audit-000001.jsonl", 3)]),
                HeadStatement::Anchored(head("audit-000002.jsonl", 4)),
            ]),
            // Older than the inventory, and never read.
            SessionHeads::Read(vec![HeadStatement::Anchored(head("audit-000000.jsonl", 1))]),
        ];
        let ledger = AnchorLedger::fold_newest_first(sessions.into_iter().inspect(|_| asked += 1));
        assert_eq!(asked, 2, "the session before the inventory is not read");
        assert!(ledger.expected("audit-000000.jsonl").is_none());
        assert_eq!(
            ledger.expected("audit-000002.jsonl").map(|h| h.entries),
            Some(4)
        );
        assert_eq!(ledger.unread().len(), 1);
    }

    #[test]
    fn the_current_segment_is_checked_but_not_compared_and_its_head_is_carried() {
        let dir = temp("current");
        let mut log = FileAuditLog::open(&dir, AuditSync::EveryEntry, 0.0).expect("opened");
        for i in 0..3 {
            log.record(entry(i));
        }
        let head = log.take_closing_anchor().expect("head");
        let current = log.current_segment().expect("a segment");
        let mut ledger = AnchorLedger::default();
        ledger.apply(HeadStatement::Anchored(head.clone()));
        let verified = verify_audit_record(&dir, &ledger, Some(&current)).expect("read");
        assert!(verified.intact());
        assert_eq!(verified.reports[0].state, SegmentState::Current);
        assert_eq!(verified.heads, vec![head]);
        drop(log);
        let _ = std::fs::remove_dir_all(dir);
    }
}
