// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The node's audit record kept honest: its head in the journal, verified at start, and
//! aged out by the baseline's policy (GAP-163, D-104; GAP-152, D-105;
//! `docs/design/DN-23-operator-authentication.md` §14). Human-owned: see
//! `docs/signatures.md`.
//!
//! # The head
//!
//! [`crate::approval::audit_routes`] syncs the log once a tick and then journals the
//! head [`gungnir_security::AuditLog::take_anchor`] says is due; the loop's shutdown
//! journals the closing head. Both go on the bus as `AuditEvent::Anchored`, and the loop
//! appends the bus to the journal, which on a node syncs every envelope.
//!
//! # At start
//!
//! [`verify`] folds the journal newest first back to the last inventory
//! ([`gungnir_security::AnchorLedger::fold_newest_first`]) and checks every segment
//! against it. What it found is journaled as `AuditEvent::Verified` and, when anything
//! is wrong, logged at error level one line per problem, recorded as an
//! `audit.anchor_mismatch` entry in this run's segment, and carried on the node's health
//! line. The node's surface is its log: no route serves its audit record (GAP-179).
//!
//! # Retention
//!
//! [`apply_audit_retention`] beside the session purge, at start and hourly, when the
//! baseline declares a policy. Each removal is journaled and synced **before** the file
//! is deleted, then recorded in the audit log itself, so the verifier always reads a
//! purged segment as purged.

use gungnir_eventing::{Envelope, Event, EventBus, InProcessBus, Receiver};
use gungnir_model::events::{AuditEvent, AuditHead};
use gungnir_model::{MissionTime, SessionId};
use gungnir_security::{
    audit::events, verify_audit_record, AnchorLedger, AuditEntry, AuditLog, AuditVerification,
    HeadStatement, PurgedSegment, SecurityError, SegmentHead, SegmentPurgeReport, SessionHeads,
};
use gungnir_store::retention::RetentionPolicy;
use gungnir_store::{EventJournal, FileEventJournal};
use std::collections::BTreeSet;
use std::path::Path;
use std::time::SystemTime;

/// The journal's form of a head.
#[must_use]
pub fn to_model(head: SegmentHead) -> AuditHead {
    AuditHead {
        segment: head.segment,
        entries: head.entries,
        seq: head.seq,
        hash: head.hash,
    }
}

/// The verifier's form of a journaled head.
#[must_use]
pub fn from_model(head: &AuditHead) -> SegmentHead {
    SegmentHead {
        segment: head.segment.clone(),
        entries: head.entries,
        seq: head.seq,
        hash: head.hash.clone(),
    }
}

/// What one session's envelopes say about the audit log's head, in order.
#[must_use]
pub fn statements(envelopes: &[Envelope]) -> Vec<HeadStatement> {
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

/// The heads the journal holds, read newest session first and no further back than the
/// last inventory. A session that cannot be read is named, not skipped.
///
/// # Errors
///
/// When the journal cannot list its sessions.
pub fn ledger(journal: &dyn EventJournal) -> Result<AnchorLedger, String> {
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

/// Verify the audit record in `audit_dir` against the heads `journal` holds.
///
/// # Errors
///
/// When the journal cannot be listed or the directory cannot be read.
pub fn verify(
    journal: &dyn EventJournal,
    audit_dir: &Path,
    current: Option<&Path>,
) -> Result<AuditVerification, String> {
    let ledger = ledger(journal)?;
    verify_audit_record(audit_dir, &ledger, current).map_err(|e| e.to_string())
}

/// What a verification found, as the journal records it.
#[must_use]
pub fn verified_event(verification: &AuditVerification, at: MissionTime) -> Event {
    Event::Audit(AuditEvent::Verified {
        heads: verification.heads.iter().cloned().map(to_model).collect(),
        segments: verification.segments as u64,
        entries: verification.entries,
        findings: verification.problems(),
        unread_sessions: verification.unread_sessions.clone(),
        at,
    })
}

/// Verify at start and put the outcome on every record the node keeps: the journal, the
/// log, and -- when anything is wrong -- the audit log itself. Returns how many problems
/// were found, for the health line.
///
/// A verification that could not run at all is said at error level and counted as one
/// problem: a node that cannot read its own audit directory must not report it sound.
///
/// # Errors
///
/// Only a failure to publish on the bus, which stops the loop everywhere else too.
pub fn verify_at_start(
    journal: &dyn EventJournal,
    audit_dir: &Path,
    audit: &mut dyn AuditLog,
    bus: &InProcessBus,
    now: MissionTime,
) -> Result<usize, gungnir_eventing::EventingError> {
    let current = audit.current_segment();
    let verification = match verify(journal, audit_dir, current.as_deref()) {
        Ok(v) => v,
        Err(why) => {
            tracing::error!(%why, dir = %audit_dir.display(), "the audit record could not be verified");
            audit.record(AuditEntry::new(
                None,
                events::ANCHOR_MISMATCH,
                now.0,
                format!("the audit record could not be verified: {why}"),
            ));
            return Ok(1);
        }
    };
    bus.publish(now, verified_event(&verification, now))?;
    for why in &verification.unread_sessions {
        tracing::warn!(%why, "a journal session could not be read; an audit head or purge in it was not seen");
    }
    let problems = verification.problems();
    if problems.is_empty() {
        tracing::info!(
            segments = verification.segments,
            entries = verification.entries,
            "audit record verified against the heads the journal holds"
        );
    } else {
        for problem in &problems {
            tracing::error!(%problem, "the audit record does not verify");
        }
        audit.record(AuditEntry::new(
            None,
            events::ANCHOR_MISMATCH,
            now.0,
            format!(
                "verification at start found the audit record damaged: {}",
                problems.join("; ")
            ),
        ));
    }
    Ok(problems.len())
}

/// Journal a head the log handed over.
pub fn publish_anchor(bus: &InProcessBus, head: SegmentHead, closing: bool, now: MissionTime) {
    if let Err(err) = bus.publish(
        now,
        Event::Audit(AuditEvent::Anchored {
            head: to_model(head),
            closing,
            at: now,
        }),
    ) {
        tracing::error!(%err, "the audit log's head could not be journaled");
    }
}

/// The segments a run under a session hold anchored, which retention keeps with the
/// session (D-105).
///
/// # Errors
///
/// When a held session cannot be read: which segments it protects is then unknown, and
/// the caller purges nothing rather than guess.
pub fn held_segments(journal: &FileEventJournal) -> Result<BTreeSet<String>, String> {
    let holds = journal
        .holds()
        .map_err(|e| format!("the journal's holds could not be read: {e}"))?;
    let mut kept = BTreeSet::new();
    for (session, _) in holds {
        let envelopes = journal.read_session(session).map_err(|e| {
            format!(
                "held session {} could not be read, so the audit segments it protects are unknown: {e}",
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

/// Where a node's audit retention puts what it removed.
pub struct AuditRetention<'a> {
    pub audit_dir: &'a Path,
    pub audit: &'a mut dyn AuditLog,
    pub journal: &'a mut FileEventJournal,
    pub journal_rx: &'a Receiver<Envelope>,
    pub session: SessionId,
    pub bus: &'a InProcessBus,
}

/// Apply the baseline's audit-log age to the node's audit segments (GAP-152, D-105).
///
/// A baseline with no policy purges nothing. Each removal is published, the bus drained
/// into the journal and the journal synced **before** the file is deleted, then recorded
/// in the audit log: never a segment gone without a purge on the record. A purge that
/// fails is logged and tried again at the next interval, as the session purge is.
pub fn apply_audit_retention(
    target: AuditRetention<'_>,
    policy: Option<&RetentionPolicy>,
    wall_now: SystemTime,
    now: MissionTime,
) -> Option<SegmentPurgeReport> {
    let policy = policy?;
    let protect = match held_segments(target.journal) {
        Ok(protect) => protect,
        Err(why) => {
            tracing::error!(%why, "audit retention skipped this pass");
            return None;
        }
    };
    let AuditRetention {
        audit_dir,
        audit,
        journal,
        journal_rx,
        session,
        bus,
    } = target;
    let current = audit.current_segment();
    let limit = policy.max_audit_log_age_days;
    let mut record = |purged: &PurgedSegment| -> Result<(), SecurityError> {
        bus.publish(
            now,
            Event::Audit(AuditEvent::Purged {
                segment: purged.segment.clone(),
                entries: purged.entries,
                bytes: purged.bytes,
                idle_days: purged.idle_days,
                max_audit_log_age_days: limit,
                completed: purged.completed,
                at: now,
            }),
        )
        .map_err(|e| SecurityError::AuditUnavailable(format!("publishing the purge: {e}")))?;
        for envelope in journal_rx.try_iter() {
            journal.append(session, &envelope).map_err(|e| {
                SecurityError::AuditUnavailable(format!("journaling the purge: {e}"))
            })?;
        }
        journal
            .sync()
            .map_err(|e| SecurityError::AuditUnavailable(format!("syncing the journal: {e}")))?;
        audit.record(AuditEntry::new(
            None,
            events::PURGED,
            now.0,
            format!(
                "{} purged: {} entries, not written for {:.1} days, past the audit-log age of {limit} days{}",
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
        audit.flush()
    };
    match gungnir_security::purge_expired_segments(
        audit_dir,
        &|days| policy.audit_log_expired(days),
        wall_now,
        current.as_deref(),
        &protect,
        &mut record,
    ) {
        Ok(report) => {
            if !report.purged.is_empty() {
                tracing::info!(
                    removed = report.purged.len(),
                    "audit retention removed segments past the limit"
                );
            }
            Some(report)
        }
        Err(err) => {
            tracing::error!(%err, "audit retention failed; nothing is half-removed, and it is tried again in an hour");
            None
        }
    }
}
