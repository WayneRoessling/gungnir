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
//! line.
//!
//! # Read from a linked desktop (GAP-179, D-116)
//!
//! [`NodeAuditRecord`] keeps what the node knows of its record between starts: the last
//! verification, and the heads the journal holds, taken off the node's own bus as it
//! journals them. [`NodeAuditRecord::answer_reads`] answers `GET /v3/audit` once a tick:
//! each read is recorded as one `audit.read` entry **before** its page is read, then
//! answered with the last verification -- or one run now, when the reader asks -- and the
//! page of entries asked for. A verification on request reads the audit directory against
//! the heads held in memory rather than folding the journal again, so a reader cannot make
//! the node read its whole running session on the loop that tracks.
//!
//! # Retention
//!
//! [`apply_audit_retention`] beside the session purge, at start and hourly, when the
//! baseline declares a policy. Each removal is journaled and synced **before** the file
//! is deleted, then recorded in the audit log itself, so the verifier always reads a
//! purged segment as purged.

use gungnir_api::transport::{NodeApi, PendingAuditRead};
use gungnir_api::v3;
use gungnir_eventing::{Envelope, Event, EventBus, InProcessBus, Receiver};
use gungnir_model::events::{AuditEvent, AuditHead};
use gungnir_model::{MissionTime, SessionId};
use gungnir_security::{
    actions, audit::events, verify_audit_record, AnchorLedger, AuditEntry, AuditLog,
    AuditVerification, HeadStatement, PurgedSegment, SecurityError, SegmentHead,
    SegmentPurgeReport, SegmentState, SessionHeads,
};
use gungnir_store::retention::RetentionPolicy;
use gungnir_store::{EventJournal, FileEventJournal};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
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
    let outcome = verify(journal, audit_dir, current.as_deref());
    report(&outcome, audit_dir, audit, bus, now, "at start")
}

/// Say what a verification found on every record the node keeps, as
/// [`verify_at_start`] documents, and return how many problems it counts. `when` is "at
/// start" or "on request", for the entry and the log.
fn report(
    outcome: &Result<AuditVerification, String>,
    audit_dir: &Path,
    audit: &mut dyn AuditLog,
    bus: &InProcessBus,
    now: MissionTime,
    when: &str,
) -> Result<usize, gungnir_eventing::EventingError> {
    let verification = match outcome {
        Ok(v) => v,
        Err(why) => {
            tracing::error!(%why, dir = %audit_dir.display(), when, "the audit record could not be verified");
            audit.record(AuditEntry::new(
                None,
                events::ANCHOR_MISMATCH,
                now.0,
                format!("the audit record could not be verified {when}: {why}"),
            ));
            return Ok(1);
        }
    };
    bus.publish(now, verified_event(verification, now))?;
    for why in &verification.unread_sessions {
        tracing::warn!(%why, "a journal session could not be read; an audit head or purge in it was not seen");
    }
    let problems = verification.problems();
    if problems.is_empty() {
        tracing::info!(
            segments = verification.segments,
            entries = verification.entries,
            when,
            "audit record verified against the heads the journal holds"
        );
    } else {
        for problem in &problems {
            tracing::error!(%problem, when, "the audit record does not verify");
        }
        audit.record(AuditEntry::new(
            None,
            events::ANCHOR_MISMATCH,
            now.0,
            format!(
                "verification {when} found the audit record damaged: {}",
                problems.join("; ")
            ),
        ));
    }
    Ok(problems.len())
}

/// The node's last verification, when it ran, and whether a reader asked for it.
#[derive(Debug)]
struct LastVerification {
    outcome: Result<AuditVerification, String>,
    at: MissionTime,
    on_request: bool,
}

/// What the node knows of its own audit record between starts, for `GET /v3/audit`
/// (GAP-179, D-116; `docs/design/DN-23-operator-authentication.md` §15). Human-owned:
/// see `docs/signatures.md`.
///
/// **The heads are held as the node journals them.** The record subscribes to the node's
/// bus before anything about the audit log is published, and applies each `Anchored`,
/// `Verified` and `Purged` it carries -- the statements the journal appends, in the order
/// it appends them -- to the ledger the start's verification left. A verification a reader
/// asks for is then [`gungnir_security::verify_audit_record`] against that ledger: it
/// reads the audit directory, which it must, and not the running session's journal, which
/// on a node that has run for days is most of its disk.
#[derive(Debug)]
pub struct NodeAuditRecord {
    audit_dir: PathBuf,
    /// The node's bus, drained once a tick by [`Self::answer_reads`].
    watch: Receiver<Envelope>,
    /// The heads the journal holds. `None` until a verification has read the journal's
    /// heads -- and so while the one at start could not -- because an empty ledger would
    /// verify every earlier segment as merely unanchored, which is not what the journal
    /// says of them.
    ledger: Option<AnchorLedger>,
    last: Option<LastVerification>,
}

impl NodeAuditRecord {
    /// Subscribe to the node's bus. Called before [`Self::verify_at_start`], so the
    /// inventory it journals is the first statement the record takes off the bus.
    #[must_use]
    pub fn new(audit_dir: &Path, bus: &InProcessBus) -> Self {
        Self {
            audit_dir: audit_dir.to_path_buf(),
            watch: bus.subscribe(),
            ledger: None,
            last: None,
        }
    }

    /// [`verify_at_start`](self::verify_at_start), keeping what it found for the route.
    ///
    /// # Errors
    ///
    /// As the free function: only a failure to publish on the bus.
    pub fn verify_at_start(
        &mut self,
        journal: &dyn EventJournal,
        audit: &mut dyn AuditLog,
        bus: &InProcessBus,
        now: MissionTime,
    ) -> Result<usize, gungnir_eventing::EventingError> {
        let current = audit.current_segment();
        let outcome = verify(journal, &self.audit_dir, current.as_deref());
        let problems = report(&outcome, &self.audit_dir, audit, bus, now, "at start")?;
        self.took_the_journal(&outcome);
        self.last = Some(LastVerification {
            outcome,
            at: now,
            on_request: false,
        });
        Ok(problems)
    }

    /// Once a verification has read the journal, the heads are held from here on: the
    /// inventory it journaled is on the bus, and [`Self::observe`] takes it first.
    fn took_the_journal(&mut self, outcome: &Result<AuditVerification, String>) {
        if outcome.is_ok() && self.ledger.is_none() {
            self.ledger = Some(AnchorLedger::default());
        }
    }

    /// Apply what the node has journaled about its audit log since the last call.
    pub fn observe(&mut self) {
        for envelope in self.watch.try_iter() {
            let Some(ledger) = self.ledger.as_mut() else {
                continue;
            };
            for statement in statements(std::slice::from_ref(&envelope)) {
                ledger.apply(statement);
            }
        }
    }

    /// Verify again now, as a reader asked, and say what was found as the verification at
    /// start does: journaled, logged, and an `audit.anchor_mismatch` entry when anything
    /// is wrong.
    ///
    /// Against the heads held in memory; while none are held -- the verification at start
    /// could not read the journal -- the journal is folded again, the start's way, so a
    /// node whose journal came back is not stuck with that failure until it restarts.
    ///
    /// # Errors
    ///
    /// Only a failure to publish on the bus.
    pub fn verify_now(
        &mut self,
        journal: &dyn EventJournal,
        audit: &mut dyn AuditLog,
        bus: &InProcessBus,
        now: MissionTime,
    ) -> Result<(), gungnir_eventing::EventingError> {
        self.observe();
        // The running segment's chain is checked too, so what it holds goes to the disk
        // first.
        if let Err(err) = audit.flush() {
            tracing::error!(%err, "the audit log could not be synced before verifying it");
        }
        let current = audit.current_segment();
        let outcome = match &self.ledger {
            Some(ledger) => verify_audit_record(&self.audit_dir, ledger, current.as_deref())
                .map_err(|e| e.to_string()),
            None => verify(journal, &self.audit_dir, current.as_deref()),
        };
        report(&outcome, &self.audit_dir, audit, bus, now, "on request")?;
        self.took_the_journal(&outcome);
        self.last = Some(LastVerification {
            outcome,
            at: now,
            on_request: true,
        });
        Ok(())
    }

    /// The last verification as it is sent.
    #[must_use]
    pub fn verification(&self) -> v3::AuditVerificationView {
        match &self.last {
            None => v3::AuditVerificationView::NotRun,
            Some(LastVerification {
                outcome: Err(reason),
                at,
                ..
            }) => v3::AuditVerificationView::Failed {
                reason: reason.clone(),
                at: *at,
            },
            Some(LastVerification {
                outcome: Ok(v),
                at,
                on_request,
            }) => v3::AuditVerificationView::Ran {
                at: *at,
                on_request: *on_request,
                segments: v.segments as u64,
                entries: v.entries,
                problems: v.problems(),
                unread_sessions: v.unread_sessions.clone(),
                reports: v
                    .reports
                    .iter()
                    .map(|r| v3::AuditSegmentView {
                        segment: r.segment.clone(),
                        entries: r.entries,
                        description: r.describe(),
                        sound: r.sound(),
                        readable: !matches!(
                            r.state,
                            SegmentState::Removed | SegmentState::PurgeInterrupted
                        ),
                    })
                    .collect(),
            },
        }
    }

    /// The page `query` asks for, read from the disk now, or `None` when it names no
    /// segment.
    fn page(&self, query: &v3::AuditQuery) -> Option<v3::AuditPageView> {
        let segment = query.segment.clone()?;
        Some(
            match gungnir_security::read_segment(&self.audit_dir, &segment) {
                Ok(read) => {
                    let total = read.entries.len() as u64;
                    let from = query.from.min(total);
                    let skip = usize::try_from(from).unwrap_or(usize::MAX);
                    let take = usize::try_from(query.page_size()).unwrap_or(usize::MAX);
                    v3::AuditPageView::Read {
                        segment,
                        total,
                        from,
                        entries: read
                            .entries
                            .iter()
                            .skip(skip)
                            .take(take)
                            .map(|e| v3::AuditEntryView {
                                operator: e.operator.map(|o| o.0),
                                party: e.party.clone(),
                                action: e.action.clone(),
                                mission_time: e.mission_time,
                                detail: e.detail.clone(),
                            })
                            .collect(),
                        unreadable: read.unreadable as u64,
                        breaks: read.breaks.iter().map(ToString::to_string).collect(),
                    }
                }
                Err(err) => v3::AuditPageView::Unreadable {
                    segment,
                    reason: err.to_string(),
                },
            },
        )
    }

    /// Answer every read of the audit record the route accepted since the last tick, in
    /// arrival order (GAP-179, D-116). Called once a tick, before `audit_routes`, so the
    /// same tick's sync and anchor cover the entries written here.
    ///
    /// For each read: verify again first when the query asks; then record the read, one
    /// `audit.read` entry naming the operator, the machine and the address, and sync it,
    /// so a page of the running segment holds the read that fetched it; then read the
    /// page and answer. A reader who went away before the answer is still on the record,
    /// which is right: the read happened.
    ///
    /// # Errors
    ///
    /// Only a failure to publish a verification on the bus.
    pub fn answer_reads(
        &mut self,
        audit: &mut dyn AuditLog,
        journal: &dyn EventJournal,
        api: &NodeApi,
        bus: &InProcessBus,
        now: MissionTime,
    ) -> Result<(), gungnir_eventing::EventingError> {
        self.observe();
        for read in api.take_audit_reads() {
            let PendingAuditRead {
                query,
                operator,
                role,
                party,
                from,
                reply,
            } = read;
            if query.verify {
                self.verify_now(journal, audit, bus, now)?;
            }
            audit.record(
                AuditEntry::new(
                    Some(operator),
                    actions::READ_AUDIT,
                    now.0,
                    format!("{} as {role:?} (from {from})", describe_read(&query)),
                )
                .by_party(party),
            );
            if let Err(err) = audit.flush() {
                tracing::error!(%err, "the audit log could not be synced before a page of it was read");
            }
            let answer = v3::AuditRecordResponse {
                at: now,
                verification: self.verification(),
                page: self.page(&query),
            };
            // A reader that went away is not an error: the read is on the record.
            let _ = reply.send(answer);
        }
        Ok(())
    }
}

/// A read in the audit entry's words: what was asked for, not what was found, because the
/// entry is written before the page is read.
fn describe_read(query: &v3::AuditQuery) -> String {
    let verified = if query.verify {
        "verified the audit record again and read "
    } else {
        "read "
    };
    match &query.segment {
        None => format!("{verified}the audit record's verification and segment list"),
        Some(segment) => format!(
            "{verified}up to {} entries of {segment} from entry {}, with the verification",
            query.page_size(),
            query.from
        ),
    }
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
