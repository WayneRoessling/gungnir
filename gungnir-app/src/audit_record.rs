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
use gungnir_security::audit::signing::AuditVerifyingKey;
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
    /// Which of the heads the last verification read were signed, and by what (GAP-208).
    pub signing: Option<gungnir_security::audit::signing::SigningReport>,
    /// What the last check against the node's heads found, on reconnect (GAP-208):
    /// `None` until one has run.
    pub against_node: Option<Vec<String>>,
}

/// Where this desktop's audit segments are.
#[must_use]
pub fn audit_dir(state: &AppState) -> PathBuf {
    std::path::Path::new(&state.config.data_dir).join(gungnir_security::AUDIT_DIR)
}

pub fn to_model(head: SegmentHead) -> AuditHead {
    AuditHead {
        segment: head.segment,
        entries: head.entries,
        seq: head.seq,
        hash: head.hash,
        // GAP-208: carried as hex on the journal.
        signature: head
            .signature
            .map(|s| gungnir_model::events::AnchorSignature::from_der(s.key_version, &s.der)),
    }
}

pub fn from_model(head: &AuditHead) -> SegmentHead {
    SegmentHead {
        segment: head.segment.clone(),
        entries: head.entries,
        seq: head.seq,
        hash: head.hash.clone(),
        // GAP-208: hex that is not a signature reads as one that does not verify.
        signature: head.signature.as_ref().map(|s| {
            gungnir_security::audit::signing::AnchorSignature {
                key_version: s.key_version,
                der: s.der().unwrap_or_default(),
            }
        }),
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

/// The baseline's audit-signing public key, read at start (GAP-208). A key the baseline
/// names and that does not read is an alert, and nothing is then checked against it.
pub fn verifying_key(
    config: &gungnir_config::ConfigBaseline,
    alerts: &mut Vec<String>,
) -> Option<AuditVerifyingKey> {
    let pem = &config.security.audit_signing.as_ref()?.public_key_pem;
    match AuditVerifyingKey::from_pem(pem) {
        Ok(key) => {
            alerts.push(
                "Audit anchors are unsigned until an administrator unlocks the audit-signing \
                 key on PN-20"
                    .into(),
            );
            Some(key)
        }
        Err(err) => {
            alerts.push(format!(
                "The baseline's audit-signing key does not read ({err}); anchors are neither \
                 signed nor checked"
            ));
            None
        }
    }
}

/// `head`, signed when the audit-signing key is unlocked (GAP-208). A head the signer
/// refuses goes unsigned, with an error logged, rather than not at all.
fn sign(state: &AppState, mut head: SegmentHead) -> SegmentHead {
    if head.signature.is_some() {
        return head;
    }
    if let Some(signer) = &state.audit_signer {
        match signer.sign(&head) {
            Ok(signature) => head.signature = Some(signature),
            Err(err) => tracing::error!(%err, "an audit head could not be signed"),
        }
    }
    head
}

/// Unlock the audit-signing key with the administrator's passphrase (GAP-208, D-142),
/// from PN-20. Recorded either way, attributed to whoever is signed in: a failed attempt
/// is somebody trying the administrator's passphrase.
///
/// Refused when the baseline names no key, and when the key that opens is not the one the
/// baseline names: anchors signed by it would verify nowhere.
pub fn unlock_signing(state: &mut AppState, passphrase: &str) {
    use gungnir_security::audit::signing::AuditSigner;
    let Some(key) = state.audit_key.clone() else {
        state.alerts.push(
            "No audit-signing key is configured in the baseline; there is nothing to unlock".into(),
        );
        return;
    };
    let data_dir = std::path::PathBuf::from(&state.config.data_dir);
    let opened = AuditSigner::unlock(&data_dir, passphrase)
        .map_err(|e| e.to_string())
        .and_then(|signer| {
            // The same key as the baseline's, by a signature over a probe head: two PEM
            // encodings of one key need not be byte-equal.
            let probe = SegmentHead {
                segment: "probe".into(),
                entries: 0,
                seq: 0,
                hash: String::new(),
                signature: None,
            };
            let signature = signer.sign(&probe).map_err(|e| e.to_string())?;
            match key.check(&probe, &signature.der) {
                gungnir_security::audit::signing::SignatureCheck::Valid => Ok(signer),
                gungnir_security::audit::signing::SignatureCheck::Invalid => {
                    Err("the key that opened is not the one the baseline names".to_owned())
                }
            }
        });
    match opened {
        Ok(signer) => {
            state.audit_signer = Some(signer);
            crate::audit::record(
                state,
                events::SIGNING_UNLOCK,
                "the audit-signing key was unlocked; anchors from now on are signed",
            );
            state
                .alerts
                .push("Audit signing unlocked: anchors from now on are signed".into());
        }
        Err(why) => {
            crate::audit::record(state, events::SIGNING_UNLOCK, format!("refused: {why}"));
            state
                .alerts
                .push(format!("Audit signing was not unlocked: {why}"));
        }
    }
}

/// Hand what is owed to the node, and check the file against what the node holds
/// (GAP-208). Called every tick; does nothing without a link.
///
/// **On reconnect** the link reads the node's heads for this desktop once; the file is
/// checked against them before anything new is forwarded, and the result goes to the node
/// with the heads. A segment found cut or rewritten against them is damage, said as the
/// local verification says it. A segment gone is the local verification's to find: the
/// node does not know which segments retention purged.
pub fn forward(state: &mut AppState) {
    let Some(link) = state.link.clone() else {
        // Nothing to forward to: a desktop with no node holds its heads in its journal.
        state.audit_forward.clear();
        return;
    };
    let findings = link
        .take_node_audit_heads()
        .map(|held| check_against_node(state, &held));
    let heads = std::mem::take(&mut state.audit_forward);
    link.queue_audit_anchors(heads, findings);
}

/// The file against the node's heads for it: the cuts and rewrites found, each in words.
fn check_against_node(state: &mut AppState, held: &[AuditHead]) -> Vec<String> {
    let mut ledger = AnchorLedger::default();
    for head in held {
        ledger.apply(HeadStatement::Anchored(from_model(head)));
    }
    let dir = audit_dir(state);
    let at = state.clock.now();
    // Every segment, this run's included: the local verification leaves the current one
    // to its own run's anchors, and a node head for it is exactly what that run cannot
    // vouch for once its file has been edited under it.
    let findings: Vec<String> = match verify_audit_record(&dir, &ledger, None) {
        Ok(v) => v
            .findings
            .iter()
            .filter(|f| !matches!(f, gungnir_security::AnchorFinding::Removed { .. }))
            .map(ToString::to_string)
            .collect(),
        Err(err) => vec![format!(
            "the audit record could not be checked against the node's heads: {err}"
        )],
    };
    for finding in &findings {
        tracing::error!(%finding, "the audit record does not reach a head the node holds");
        state.alerts.push(format!(
            "Audit record does not reach the node's head: {finding}"
        ));
    }
    if !findings.is_empty() {
        state.audit.record(AuditEntry::new(
            None,
            events::ANCHOR_MISMATCH,
            at.0,
            format!(
                "checked against the node's heads on reconnect: {}",
                findings.join("; ")
            ),
        ));
    }
    state.audit_record.against_node = Some(findings.clone());
    findings
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
        let head = to_model(sign(state, head));
        state.audit_forward.push(head.clone());
        publish(
            state,
            at,
            Event::Audit(AuditEvent::Anchored {
                head,
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
        let head = to_model(sign(state, head));
        // Queued for the node as it closes; the link carries it if it is still up.
        if let Some(link) = &state.link {
            link.queue_audit_anchors(vec![head.clone()], None);
        }
        publish(
            state,
            at,
            Event::Audit(AuditEvent::Anchored {
                head,
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
        // GAP-208: which of the heads the file is judged against were signed.
        state.audit_record.signing = Some(gungnir_security::audit::signing::signing_report(
            ledger.heads(),
            state.audit_key.as_ref(),
        ));
        verify_audit_record(&dir, &ledger, current.as_deref()).map_err(|e| e.to_string())
    });
    let at = state.clock.now();
    match &outcome {
        Ok(verification) => {
            // A signature that does not verify is damage, said with the rest.
            let mut problems = verification.problems();
            problems.extend(
                state
                    .audit_record
                    .signing
                    .as_ref()
                    .map(gungnir_security::audit::signing::SigningReport::problems)
                    .unwrap_or_default(),
            );
            // GAP-208: the inventory is signed too, or a rewritten journal could carry a
            // forged one; and it is what the node is sent, so a restarted desktop's whole
            // record reaches it.
            let heads: Vec<AuditHead> = verification
                .heads
                .iter()
                .cloned()
                .map(|h| to_model(sign(state, h)))
                .collect();
            state.audit_forward.extend(heads.iter().cloned());
            publish(
                state,
                at,
                Event::Audit(AuditEvent::Verified {
                    heads,
                    segments: verification.segments as u64,
                    entries: verification.entries,
                    findings: problems.clone(),
                    unread_sessions: verification.unread_sessions.clone(),
                    at,
                }),
            );
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
    /// Whether anchors are signed, and which heads were (GAP-208).
    pub signing: String,
    /// A key is configured and nobody has unlocked it this run.
    pub signing_locked: bool,
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
    let (signing, signing_locked) = signing_text(state);
    RecordText {
        summary,
        sound,
        problems,
        segments,
        shown_note,
        shown_sound,
        retention,
        signing,
        signing_locked,
    }
}

/// PN-20's signing sentence (GAP-208): whether anchors are being signed, which heads the
/// last verification found signed, and what the check against the node found; and
/// whether the unlock field is offered.
fn signing_text(state: &AppState) -> (String, bool) {
    use std::fmt::Write as _;
    let signing_locked = state.audit_key.is_some() && state.audit_signer.is_none();
    let now_signing = match (&state.audit_key, &state.audit_signer) {
        (None, _) => "No audit-signing key is configured in the baseline, so anchors are not \
                      signed."
            .to_owned(),
        (Some(_), None) => "Audit anchors are UNSIGNED: no administrator has unlocked the \
                            audit-signing key this run."
            .to_owned(),
        (Some(_), Some(_)) => {
            "Audit anchors are signed by the deployment's audit-signing key.".to_owned()
        }
    };
    let mut signing = match &state.audit_record.signing {
        Some(report) if report.key_configured => format!("{now_signing} {}", report.summary()),
        _ => now_signing,
    };
    if let Some(found) = &state.audit_record.against_node {
        if found.is_empty() {
            signing
                .push_str(" Checked against the node's heads on reconnect: it reaches every one.");
        } else {
            let _ = write!(
                signing,
                " Checked against the node's heads on reconnect: {} problem(s).",
                found.len()
            );
        }
    }
    (signing, signing_locked)
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
        signing: &text.signing,
        signing_locked: text.signing_locked,
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
