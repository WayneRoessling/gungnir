// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The desktop's audit record kept honest: its head in the journal, verified at start and
//! on demand from PN-20, aged out by the baseline's policy, and its earlier runs read back
//! (GAP-163, D-104; GAP-152, D-105, D-106;
//! `docs/design/DN-23-operator-authentication.md` §14). The format and the checks are
//! `gungnir_security`'s and human-owned: see `docs/signatures.md`.
//!
//! # The head
//!
//! [`tick`] journals the head the log says is due, before the frame's journal drain; the
//! log syncs every entry on the desktop, so a head is always over lines on the disk.
//! [`close`] journals the closing head before the session is saved and closed.
//!
//! # Verification
//!
//! [`verify`] runs when the desktop starts -- on its first tick, as retention does, once
//! the live session and the bus exist, and before retention -- and whenever
//! PN-20's "Verify now" is pressed. It reads the journal newest first back to the last
//! inventory, checks every segment against the heads there, and journals what it found
//! (`AuditEvent::Verified`). **Never silent**: each problem is an alert naming the file --
//! "cut: N entries missing", "gone without a purge" -- PN-20 draws every segment with its
//! state and the summary in the warning colour, and the problems are written into the
//! audit log itself as one `audit.anchor_mismatch` entry. A journal session that could
//! not be read is said too, because a head recorded in it was not checked.
//!
//! # Retention and earlier runs
//!
//! [`purge`] is the audit half of `crate::retention::run`: the same schedule, the same
//! opt-in, and the same visibility -- journaled and synced before the file is deleted,
//! recorded in the audit log, counted, and raised as an alert. [`show`] reads an earlier
//! run's segment for PN-20, read only.

use crate::state::AppState;
use crate::update::publish;
use gungnir_eventing::{Envelope, Event};
use gungnir_model::events::{AuditEvent, AuditHead};
use gungnir_model::MissionTime;
use gungnir_security::audit::events;
use gungnir_security::{
    verify_audit_record, AnchorLedger, AuditEntry, AuditLog, AuditVerification, HeadStatement,
    PurgedSegment, SecurityError, SegmentEntries, SegmentHead, SessionHeads,
};
use gungnir_store::EventJournal;
use gungnir_ui::panels::audit::{AuditRecordView, RecordSegmentLine, ShownSegment};
use gungnir_ui::panels::config_editor::AuditLine;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{Instant, SystemTime};

/// What PN-20 shows of the audit record beyond this run's entries.
#[derive(Debug, Default)]
pub struct AuditRecordState {
    /// The last verification, or why it could not run.
    pub last: Option<Result<AuditVerification, String>>,
    /// When it ran, on the mission clock.
    pub verified_at: Option<MissionTime>,
    /// The earlier run's segment PN-20 is showing, by name, or why it could not be read.
    pub shown: Option<(String, Result<SegmentEntries, String>)>,
    /// Audit segments this process removed under the policy.
    pub purged_total: u64,
    /// The last purge failure, so the same one is not raised every hour.
    last_purge_error: Option<String>,
}

/// Where this desktop's audit segments are.
#[must_use]
pub fn audit_dir(state: &AppState) -> PathBuf {
    std::path::Path::new(&state.config.data_dir).join(gungnir_security::AUDIT_DIR)
}

fn to_model(head: SegmentHead) -> AuditHead {
    AuditHead {
        segment: head.segment,
        entries: head.entries,
        seq: head.seq,
        hash: head.hash,
    }
}

fn from_model(head: &AuditHead) -> SegmentHead {
    SegmentHead {
        segment: head.segment.clone(),
        entries: head.entries,
        seq: head.seq,
        hash: head.hash.clone(),
    }
}

/// What one session's envelopes say about the audit log's head, in order.
fn statements(envelopes: &[Envelope]) -> Vec<HeadStatement> {
    envelopes
        .iter()
        .filter_map(|envelope| match &envelope.event {
            Event::Audit(AuditEvent::Anchored { head, .. }) => {
                Some(HeadStatement::Anchored(from_model(head)))
            }
            Event::Audit(AuditEvent::Verified { heads, .. }) => Some(HeadStatement::Verified(
                heads.iter().map(from_model).collect(),
            )),
            Event::Audit(AuditEvent::Purged { segment, .. }) => {
                Some(HeadStatement::Purged(segment.clone()))
            }
            _ => None,
        })
        .collect()
}

/// The heads the journal holds, newest session first, back to the last inventory.
fn ledger(journal: &dyn EventJournal) -> Result<AnchorLedger, String> {
    let mut sessions = journal
        .sessions()
        .map_err(|e| format!("the journal's sessions could not be listed: {e}"))?;
    sessions.sort_unstable_by_key(|s| std::cmp::Reverse(s.0));
    Ok(AnchorLedger::fold_newest_first(sessions.into_iter().map(
        |session| match journal.read_session(session) {
            Ok(envelopes) => SessionHeads::Read(statements(&envelopes)),
            Err(e) => SessionHeads::Unreadable(format!("session {}: {e}", session.0)),
        },
    )))
}

/// Journal the head the log says is due. Called by `update::tick` before the journal
/// drain, so the head is appended in the frame it was taken.
pub fn tick(state: &mut AppState) {
    tick_at(state, Instant::now());
}

/// [`tick`] at a given instant, so a test can make a head due without waiting.
pub fn tick_at(state: &mut AppState, now: Instant) {
    if let Some(head) = state.audit.take_anchor(now) {
        let at = state.clock.now();
        publish(
            state,
            at,
            Event::Audit(AuditEvent::Anchored {
                head: to_model(head),
                closing: false,
                at,
            }),
        );
    }
}

/// Journal the closing head. Called by `AppState::close_session` before it saves.
pub fn close(state: &mut AppState) {
    if let Some(head) = state.audit.take_closing_anchor() {
        let at = state.clock.now();
        publish(
            state,
            at,
            Event::Audit(AuditEvent::Anchored {
                head: to_model(head),
                closing: true,
                at,
            }),
        );
    }
}

/// [`verify`], once: on the first tick, which is when the desktop has started. A later
/// call does nothing; PN-20's "Verify now" calls [`verify`] itself.
pub fn verify_at_start(state: &mut AppState) {
    if state.audit_record.verified_at.is_none() {
        verify(state);
    }
}

/// Verify the audit record against the heads the journal holds, and say what was found
/// everywhere the operator and the record will see it (this module's documentation).
pub fn verify(state: &mut AppState) {
    // This run's own heads, on the disk first: a verification on demand reads the live
    // session too, and an inventory that missed them would forget them until the next.
    if let Err(err) = state.save_session() {
        tracing::warn!(%err, "the journal could not be synced before verifying the audit record");
    }
    let dir = audit_dir(state);
    let current = state.audit.current_segment();
    let outcome = ledger(&state.journal).and_then(|ledger| {
        verify_audit_record(&dir, &ledger, current.as_deref()).map_err(|e| e.to_string())
    });
    let at = state.clock.now();
    match &outcome {
        Ok(verification) => {
            publish(
                state,
                at,
                Event::Audit(AuditEvent::Verified {
                    heads: verification.heads.iter().cloned().map(to_model).collect(),
                    segments: verification.segments as u64,
                    entries: verification.entries,
                    findings: verification.problems(),
                    unread_sessions: verification.unread_sessions.clone(),
                    at,
                }),
            );
            let problems = verification.problems();
            for problem in &problems {
                tracing::error!(%problem, "the audit record does not verify");
                state
                    .alerts
                    .push(format!("Audit record damaged: {problem}"));
            }
            if !problems.is_empty() {
                state.audit.record(AuditEntry::new(
                    None,
                    events::ANCHOR_MISMATCH,
                    at.0,
                    format!(
                        "verification found the audit record damaged: {}",
                        problems.join("; ")
                    ),
                ));
            }
            if !verification.unread_sessions.is_empty() {
                state.alerts.push(format!(
                    "Audit record: {} journal session(s) could not be read, so a head or \
                     purge recorded in them was not checked: {}",
                    verification.unread_sessions.len(),
                    verification.unread_sessions.join("; ")
                ));
            }
            if verification.intact() {
                tracing::info!(
                    segments = verification.segments,
                    entries = verification.entries,
                    "audit record verified against the heads the journal holds"
                );
            }
        }
        Err(why) => {
            tracing::error!(%why, "the audit record could not be verified");
            state
                .alerts
                .push(format!("The audit record could not be verified: {why}"));
            state.audit.record(AuditEntry::new(
                None,
                events::ANCHOR_MISMATCH,
                at.0,
                format!("the audit record could not be verified: {why}"),
            ));
        }
    }
    state.audit_record.last = Some(outcome);
    state.audit_record.verified_at = Some(at);
}

/// The segments a run under a session hold anchored in that session, which retention
/// keeps with it -- the audit record of a session under after-action review.
fn held_segments(state: &AppState) -> Result<BTreeSet<String>, String> {
    let holds = state
        .journal
        .holds()
        .map_err(|e| format!("the journal's holds could not be read: {e}"))?;
    let mut kept = BTreeSet::new();
    for (session, _) in holds {
        let envelopes = state.journal.read_session(session).map_err(|e| {
            format!(
                "held session {} could not be read, so the audit segments it protects are \
                 unknown: {e}",
                session.0
            )
        })?;
        kept.extend(statements(&envelopes).into_iter().filter_map(|s| match s {
            HeadStatement::Anchored(head) => Some(head.segment),
            _ => None,
        }));
    }
    Ok(kept)
}

/// Apply the policy's audit-log age (GAP-152, D-105). Called by `crate::retention::run`
/// with the policy it applied to the sessions; a baseline with none never reaches here.
pub fn purge(
    state: &mut AppState,
    policy: &gungnir_store::retention::RetentionPolicy,
    now: SystemTime,
) {
    let protect = match held_segments(state) {
        Ok(protect) => protect,
        Err(why) => {
            purge_failed(state, &why);
            return;
        }
    };
    let dir = audit_dir(state);
    let current = state.audit.current_segment();
    let limit = policy.max_audit_log_age_days;
    let mut record = |purged: &PurgedSegment| -> Result<(), SecurityError> {
        let at = state.clock.now();
        publish(
            state,
            at,
            Event::Audit(AuditEvent::Purged {
                segment: purged.segment.clone(),
                entries: purged.entries,
                bytes: purged.bytes,
                idle_days: purged.idle_days,
                max_audit_log_age_days: limit,
                completed: purged.completed,
                at,
            }),
        );
        // On the disk before the file is deleted: a purge the journal does not hold
        // would read as a deletion at the next verification. Drained here rather than
        // through `update::journal_pending`, which logs a failed append and carries on,
        // because a failure here must stop the deletion.
        let Some(session) = state.session() else {
            return Err(SecurityError::AuditUnavailable(
                "no live session to journal the purge into".into(),
            ));
        };
        let pending: Vec<Envelope> = state.journal_rx.try_iter().collect();
        let mut failed = None;
        for envelope in pending {
            if let Err(err) = state.journal.append(session, &envelope) {
                failed.get_or_insert(err);
            }
        }
        if let Some(err) = failed {
            return Err(SecurityError::AuditUnavailable(format!(
                "journaling the purge: {err}"
            )));
        }
        state
            .journal
            .sync()
            .map_err(|e| SecurityError::AuditUnavailable(format!("syncing the journal: {e}")))?;
        state.audit.record(AuditEntry::new(
            None,
            events::PURGED,
            at.0,
            format!(
                "{} purged: {} entries, not written for {:.1} days, past the audit-log age of \
                 {limit} days{}",
                purged.segment,
                purged.entries,
                purged.idle_days,
                if purged.completed {
                    " (finishing a purge an interrupted run began)"
                } else {
                    ""
                }
            ),
        ));
        state.audit.flush()
    };
    let outcome = gungnir_security::purge_expired_segments(
        &dir,
        &|days| policy.audit_log_expired(days),
        now,
        current.as_deref(),
        &protect,
        &mut record,
    );
    match outcome {
        Ok(report) => {
            state.audit_record.last_purge_error = None;
            for restored in &report.restored {
                state.alerts.push(format!(
                    "Audit segment {restored} was found marked for purging though the policy \
                     would not purge it; it was restored to the record"
                ));
            }
            if !report.purged.is_empty() {
                state.audit_record.purged_total += report.purged.len() as u64;
                let names: Vec<&str> = report.purged.iter().map(|p| p.segment.as_str()).collect();
                state.alerts.push(format!(
                    "Retention removed {} audit segment(s) not written for more than {limit} \
                     days: {}",
                    names.len(),
                    names.join(", ")
                ));
            }
        }
        Err(err) => purge_failed(state, &err.to_string()),
    }
}

fn purge_failed(state: &mut AppState, why: &str) {
    tracing::error!(%why, "audit retention failed; it is tried again in an hour");
    if state.audit_record.last_purge_error.as_deref() != Some(why) {
        state.alerts.push(format!(
            "Audit retention failed ({why}); no segment is half-removed, and it is tried \
             again in an hour"
        ));
        state.audit_record.last_purge_error = Some(why.to_owned());
    }
}

/// Read one segment from the last verification's list for PN-20, read only.
pub fn show(state: &mut AppState, index: usize) {
    let Some(Ok(verification)) = &state.audit_record.last else {
        return;
    };
    let Some(report) = verification.reports.get(index) else {
        return;
    };
    let segment = report.segment.clone();
    let dir = audit_dir(state);
    let read = gungnir_security::read_segment(&dir, &segment).map_err(|e| e.to_string());
    state.audit_record.shown = Some((segment, read));
}

/// Stop showing an earlier segment.
pub fn hide(state: &mut AppState) {
    state.audit_record.shown = None;
}

/// PN-20's audit-record text, owned so the view can borrow it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordText {
    pub summary: String,
    pub sound: bool,
    /// Each problem, naming its file, then each journal session that could not be read.
    pub problems: Vec<String>,
    /// Each segment's state in words, whether it is sound, and whether it can be shown.
    pub segments: Vec<(String, bool, bool)>,
    pub shown_note: String,
    pub shown_sound: bool,
    pub retention: String,
}

/// The audit record in words, from the last verification.
#[must_use]
pub fn record_text(state: &AppState) -> RecordText {
    let at = state
        .audit_record
        .verified_at
        .map_or(String::new(), |t| format!(" (verified at T+{:.0} s)", t.0));
    let (summary, sound, problems, segments) = match &state.audit_record.last {
        None => (
            "The audit record has not been verified yet.".to_owned(),
            false,
            Vec::new(),
            Vec::new(),
        ),
        Some(Err(why)) => (
            format!("The audit record could not be verified{at}: {why}"),
            false,
            Vec::new(),
            Vec::new(),
        ),
        Some(Ok(v)) => {
            let mut problems = v.problems();
            let damaged = problems.len();
            problems.extend(v.unread_sessions.iter().map(|why| {
                format!("A journal session could not be read, so a head or purge in it was not checked: {why}")
            }));
            let summary = if damaged > 0 {
                format!(
                    "The audit record is DAMAGED{at}: {damaged} problem(s) across {} segment(s).",
                    v.reports.len()
                )
            } else if v.unread_sessions.is_empty() {
                format!(
                    "The audit record verifies against the heads the journal holds{at}: {} \
                     segment(s), {} entries.",
                    v.segments, v.entries
                )
            } else {
                format!(
                    "The audit record's chain verifies{at}, but {} journal session(s) could not \
                     be read, so it was not checked against every head.",
                    v.unread_sessions.len()
                )
            };
            let segments = v
                .reports
                .iter()
                .map(|r| {
                    let readable = !matches!(
                        r.state,
                        gungnir_security::SegmentState::Removed
                            | gungnir_security::SegmentState::PurgeInterrupted
                    );
                    (r.describe(), r.sound(), readable)
                })
                .collect();
            (summary, problems.is_empty(), problems, segments)
        }
    };
    let (shown_note, shown_sound) = match &state.audit_record.shown {
        None => (String::new(), true),
        Some((_, Err(why))) => (format!("It could not be read: {why}"), false),
        Some((_, Ok(read))) if read.breaks.is_empty() && read.unreadable == 0 => (
            format!(
                "{} entries. Its chain verified as it was read; its state against the \
                 journal is the one listed above.",
                read.entries.len()
            ),
            true,
        ),
        Some((_, Ok(read))) => {
            let breaks: Vec<String> = read.breaks.iter().map(ToString::to_string).collect();
            (
                format!(
                    "{} entries; its chain does NOT verify as read: {}",
                    read.entries.len(),
                    breaks.join("; ")
                ),
                false,
            )
        }
    };
    let retention = match state.config.retention {
        Some(policy) => format!(
            "Audit segments not written for more than {} days are purged, oldest first, at \
             start and hourly; this run has purged {}.",
            policy.max_audit_log_age_days, state.audit_record.purged_total
        ),
        None => "Audit segments are never purged: the baseline declares no retention.".into(),
    };
    RecordText {
        summary,
        sound,
        problems,
        segments,
        shown_note,
        shown_sound,
        retention,
    }
}

/// The segment lines PN-20 lists, borrowing `text`.
#[must_use]
pub fn segment_lines(text: &RecordText) -> Vec<RecordSegmentLine<'_>> {
    text.segments
        .iter()
        .map(|(description, sound, readable)| RecordSegmentLine {
            description,
            sound: *sound,
            readable: *readable,
        })
        .collect()
}

/// The entries of the segment PN-20 is showing, oldest first.
#[must_use]
pub fn shown_lines(state: &AppState) -> Vec<AuditLine<'_>> {
    match &state.audit_record.shown {
        Some((_, Ok(read))) => read
            .entries
            .iter()
            .map(|e| AuditLine {
                action: &e.action,
                #[allow(clippy::cast_possible_truncation)]
                mission_time_s: e.mission_time as i64,
                detail: &e.detail,
                operator: e.operator.map(|o| o.0),
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// PN-20's audit-record view.
#[must_use]
pub fn record_view<'a>(
    state: &'a AppState,
    text: &'a RecordText,
    segments: &'a [RecordSegmentLine<'a>],
    lines: &'a [AuditLine<'a>],
) -> AuditRecordView<'a> {
    AuditRecordView {
        summary: &text.summary,
        sound: text.sound,
        problems: &text.problems,
        segments,
        shown: state
            .audit_record
            .shown
            .as_ref()
            .map(|(segment, _)| ShownSegment {
                segment,
                lines,
                note: &text.shown_note,
                sound: text.shown_sound,
            }),
        retention: &text.retention,
    }
}

/// Said once at start, beside the session policy: whether this desktop ages out audit
/// segments, and under what limit.
pub fn announce(config: &gungnir_config::ConfigBaseline) {
    if let Some(policy) = config.retention {
        tracing::info!(
            max_audit_log_age_days = policy.max_audit_log_age_days,
            "audit retention: segments not written for longer than this are purged, oldest first, at start and hourly"
        );
    } else {
        tracing::info!("audit retention is not configured; no audit segment is ever purged");
    }
}
