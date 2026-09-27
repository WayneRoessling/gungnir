// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The node's audit record on a linked desktop's PN-20 (GAP-179, D-116;
//! `docs/design/DN-23-operator-authentication.md` §15).
//!
//! A node verifies its audit record against the heads its journal holds at every start
//! (GAP-163), and until GAP-179 said what it found only in its own log. `GET /v3/audit`
//! serves the record and that verification to a role holding `audit.read` -- the
//! administrator and the commander, the owner's grant -- and this module draws it on PN-20
//! beside the desktop's own record, read only.
//!
//! **Read when a person asks, never polled.** Every read is an `audit.read` entry on the
//! node's record, so a desktop that polled would fill the node's record with reads nobody
//! made. PN-20 reads it on "Read the node's record", "Verify the node's record now", and a
//! segment's "Show", "Older" and "Newer".
//!
//! **Said, not left blank**: a desktop with no node draws no section; one whose signed-in
//! role lacks `audit.read` says so and sends nothing; a read the node refused shows the
//! node's reason; a node that could not be reached is said to be, and whatever an earlier
//! read showed stays, labelled with when it was read.

use crate::state::AppState;
use gungnir_model::MissionTime;
use gungnir_remote::link::{
    AuditPageView, AuditQuery, AuditRecordOutcome, AuditRecordResponse, AuditVerificationView,
    PendingAuditRecord, AUDIT_PAGE_LIMIT,
};
use gungnir_security::actions;
use gungnir_ui::panels::audit::{NodeAuditRecordView, NodeShownPage, RecordSegmentLine};
use gungnir_ui::panels::config_editor::AuditLine;

/// What PN-20 holds of the node's audit record.
#[derive(Debug, Default)]
pub struct NodeAuditState {
    /// A read on its way, and what it asked.
    pending: Option<(PendingAuditRecord, AuditQuery)>,
    /// The last record the node sent, and when it arrived on this desktop's clock.
    read: Option<(Box<AuditRecordResponse>, MissionTime)>,
    /// The page of a segment shown, and the query that fetched it.
    page: Option<(AuditPageView, AuditQuery)>,
    /// Why the last read did not answer, when it did not.
    failed: Option<String>,
}

/// The node this desktop is configured to link to, when it has one.
fn node_endpoint(state: &AppState) -> Option<String> {
    match &state.config.backend {
        gungnir_config::BackendConfig::Remote { endpoint } => Some(endpoint.clone()),
        gungnir_config::BackendConfig::Embedded => None,
    }
}

/// Why nothing can be read from the node now, or `None` when a read can be sent.
fn why_not(state: &AppState) -> Option<String> {
    let Some(session) = state.signed_in() else {
        return Some(
            "Nobody is signed in. Reading the node's audit record needs a session whose role \
             holds audit.read, which the Administrator and the Commander hold."
                .to_owned(),
        );
    };
    if !gungnir_security::authz::role_permits(session.role, actions::READ_AUDIT) {
        return Some(format!(
            "Reading the node's audit record needs audit.read, which the Administrator and \
             the Commander hold; operator {} is signed in as {:?}.",
            session.operator.0, session.role
        ));
    }
    let Some(link) = state.link.as_ref() else {
        return Some(
            "This desktop has no link to its node, so the node's record cannot be read.".to_owned(),
        );
    };
    if link.token().is_none() {
        return Some(
            "The link holds no session with the node yet, so the node's record cannot be \
             read; it is read with the session the link signed in with."
                .to_owned(),
        );
    }
    None
}

/// Send one read. Refused here, with the reason as an alert, when [`why_not`] says so:
/// a read the node would refuse is still an entry on its record.
fn request(state: &mut AppState, query: AuditQuery) {
    if state.node_audit.pending.is_some() {
        return;
    }
    if let Some(why) = why_not(state) {
        state.alerts.push(why);
        return;
    }
    let (Some(endpoint), Some(token)) = (
        node_endpoint(state),
        state
            .link
            .as_ref()
            .and_then(gungnir_remote::link::NodeLink::token),
    ) else {
        return;
    };
    match gungnir_remote::link::fetch_audit_record(
        &gungnir_remote::RemoteEndpoint {
            url: endpoint,
            tls: crate::session::link_tls(state),
        },
        &token,
        &query,
        state.runtime.handle(),
    ) {
        Ok(pending) => state.node_audit.pending = Some((pending, query)),
        Err(err) => {
            state.node_audit.failed = Some(err.to_string());
            state
                .alerts
                .push(format!("The node's audit record could not be read: {err}"));
        }
    }
}

/// "Read the node's record": its last verification and its segments.
pub fn read(state: &mut AppState) {
    request(state, AuditQuery::default());
}

/// "Verify the node's record now".
pub fn verify(state: &mut AppState) {
    request(
        state,
        AuditQuery {
            verify: true,
            ..AuditQuery::default()
        },
    );
}

/// "Show" on the node's segment at `index`: its newest page.
pub fn show(state: &mut AppState, index: usize) {
    let Some((record, _)) = &state.node_audit.read else {
        return;
    };
    let AuditVerificationView::Ran { reports, .. } = &record.verification else {
        return;
    };
    let Some(report) = reports.get(index) else {
        return;
    };
    let query = AuditQuery {
        segment: Some(report.segment.clone()),
        from: report.entries.saturating_sub(AUDIT_PAGE_LIMIT),
        ..AuditQuery::default()
    };
    request(state, query);
}

/// "Older" or "Newer" on the page shown.
pub fn page(state: &mut AppState, older: bool) {
    let Some((_, shown)) = &state.node_audit.page else {
        return;
    };
    let size = shown.page_size();
    let query = AuditQuery {
        from: if older {
            shown.from.saturating_sub(size)
        } else {
            shown.from + size
        },
        ..shown.clone()
    };
    request(state, query);
}

/// "Close" on the page shown.
pub fn hide(state: &mut AppState) {
    state.node_audit.page = None;
}

/// The tick step: take a read's answer when it lands.
pub fn poll(state: &mut AppState) {
    let Some(outcome) = state
        .node_audit
        .pending
        .as_ref()
        .and_then(|(pending, _)| pending.poll())
    else {
        return;
    };
    let Some((_, query)) = state.node_audit.pending.take() else {
        return;
    };
    let now = state.clock.now();
    match outcome {
        AuditRecordOutcome::Read(mut record) => {
            state.node_audit.failed = None;
            state.node_audit.page = record.page.take().map(|page| (page, query));
            if let AuditVerificationView::Ran { problems, .. } = &record.verification {
                if !problems.is_empty() {
                    state.alerts.push(format!(
                        "The node's audit record is damaged: {}",
                        problems.join("; ")
                    ));
                }
            }
            state.node_audit.read = Some((record, now));
        }
        AuditRecordOutcome::Refused { status, reason } => {
            let why = format!("The node refused the read ({status}): {reason}");
            state.alerts.push(why.clone());
            state.node_audit.failed = Some(why);
        }
        AuditRecordOutcome::Unreachable { reason } => {
            let why = format!("The node could not be reached: {reason}");
            state.alerts.push(why.clone());
            state.node_audit.failed = Some(why);
        }
    }
}

/// One entry of a page as PN-20 lists it, owned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryText {
    pub action: String,
    pub mission_time_s: i64,
    pub detail: String,
    pub operator: Option<u64>,
}

/// A page of the node's segment in words, owned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageText {
    pub segment: String,
    pub entries: Vec<EntryText>,
    pub note: String,
    pub sound: bool,
    pub has_older: bool,
    pub has_newer: bool,
}

/// PN-20's account of the node's audit record, owned so the view can borrow it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeRecordText {
    pub node: String,
    pub summary: String,
    pub sound: bool,
    pub note: String,
    pub can_read: bool,
    pub reading: bool,
    pub problems: Vec<String>,
    /// Each segment's state in words, whether it is sound, and whether it can be shown.
    pub segments: Vec<(String, bool, bool)>,
    pub page: Option<PageText>,
}

/// A verification in words: one sentence, whether it is sound, each problem, and each
/// segment's state with whether it is sound and can be shown.
struct Said {
    summary: String,
    sound: bool,
    problems: Vec<String>,
    segments: Vec<(String, bool, bool)>,
}

impl Said {
    /// A sentence with nothing listed under it.
    fn only(summary: String, sound: bool) -> Self {
        Self {
            summary,
            sound,
            problems: Vec::new(),
            segments: Vec::new(),
        }
    }
}

/// The verification in one sentence, with when it ran on the node and when it was read
/// here, and whether it is sound.
fn verification_sentence(verification: &AuditVerificationView, read_at: MissionTime) -> Said {
    match verification {
        AuditVerificationView::NotRun => Said::only(
            format!(
                "The node has not verified its audit record in this run (read at T+{:.0} s).",
                read_at.0
            ),
            false,
        ),
        AuditVerificationView::Failed { reason, at } => Said::only(
            format!(
                "The node could not verify its audit record (at node T+{:.0} s, read at \
                 T+{:.0} s): {reason}",
                at.0, read_at.0
            ),
            false,
        ),
        AuditVerificationView::Ran {
            at,
            on_request,
            segments,
            entries,
            problems,
            unread_sessions,
            reports,
        } => {
            let when = format!(
                "verified {} at node T+{:.0} s, read at T+{:.0} s",
                if *on_request {
                    "on request"
                } else {
                    "at start"
                },
                at.0,
                read_at.0
            );
            let damaged = problems.len();
            let summary = if damaged > 0 {
                format!(
                    "The node's audit record is DAMAGED ({when}): {damaged} problem(s) across \
                     {} segment(s).",
                    reports.len()
                )
            } else if unread_sessions.is_empty() {
                format!(
                    "The node's audit record verifies against the heads its journal holds \
                     ({when}): {segments} segment(s), {entries} entries."
                )
            } else {
                format!(
                    "The node's audit record's chain verifies ({when}), but {} of its journal \
                     session(s) could not be read, so it was not checked against every head.",
                    unread_sessions.len()
                )
            };
            let mut said = problems.clone();
            said.extend(unread_sessions.iter().map(|why| {
                format!(
                    "A node journal session could not be read, so a head or purge in it was \
                     not checked: {why}"
                )
            }));
            let lines = reports
                .iter()
                .map(|r| (r.description.clone(), r.sound, r.readable))
                .collect();
            Said {
                summary,
                sound: said.is_empty(),
                problems: said,
                segments: lines,
            }
        }
    }
}

fn page_text(page: &AuditPageView) -> PageText {
    match page {
        AuditPageView::Unreadable { segment, reason } => PageText {
            segment: segment.clone(),
            entries: Vec::new(),
            note: format!("It could not be read on the node: {reason}"),
            sound: false,
            has_older: false,
            has_newer: false,
        },
        AuditPageView::Read {
            segment,
            total,
            from,
            entries,
            unreadable,
            breaks,
        } => {
            let shown = entries.len() as u64;
            let range = if shown == 0 {
                format!("No entries from entry {} of {total}.", from + 1)
            } else {
                format!("Entries {} to {} of {total}.", from + 1, from + shown)
            };
            let chain = if breaks.is_empty() && *unreadable == 0 {
                "Its chain verified as it was read; its state against the node's journal is \
                 the one listed above."
                    .to_owned()
            } else {
                format!(
                    "Its chain does NOT verify as read ({unreadable} line(s) unreadable): {}",
                    breaks.join("; ")
                )
            };
            PageText {
                segment: segment.clone(),
                entries: entries
                    .iter()
                    .map(|e| EntryText {
                        action: e.action.clone(),
                        #[allow(clippy::cast_possible_truncation)]
                        mission_time_s: e.mission_time as i64,
                        detail: match &e.party {
                            Some(party) => format!("{} [machine {party}]", e.detail),
                            None => e.detail.clone(),
                        },
                        operator: e.operator,
                    })
                    .collect(),
                note: format!("{range} {chain}"),
                sound: breaks.is_empty() && *unreadable == 0,
                has_older: *from > 0,
                has_newer: from + shown < *total,
            }
        }
    }
}

/// The node's audit record in words, or `None` for a desktop with no node.
#[must_use]
pub fn text(state: &AppState) -> Option<NodeRecordText> {
    let node = node_endpoint(state)?;
    let refusal = why_not(state);
    let reading = state.node_audit.pending.is_some();
    let Said {
        summary,
        sound,
        problems,
        segments,
    } = match &state.node_audit.read {
        Some((record, read_at)) => verification_sentence(&record.verification, *read_at),
        None => Said::only(
            if reading {
                "Reading the node's audit record...".to_owned()
            } else {
                "The node's audit record has not been read from this desktop.".to_owned()
            },
            refusal.is_none(),
        ),
    };
    // Why nothing can be read now comes first; then why the last read failed, and if an
    // earlier one succeeded, that what is drawn is that one.
    let second_line = refusal.clone().unwrap_or_else(|| {
        match (&state.node_audit.failed, &state.node_audit.read) {
            (Some(failure), Some((_, read_at))) => format!(
                "{failure}. What is shown is the node's record as it was read at T+{:.0} s.",
                read_at.0
            ),
            (Some(failure), None) => failure.clone(),
            (None, _) => String::new(),
        }
    });
    Some(NodeRecordText {
        node,
        summary,
        sound: sound && state.node_audit.failed.is_none(),
        note: second_line,
        can_read: refusal.is_none(),
        reading,
        problems,
        segments,
        page: state
            .node_audit
            .page
            .as_ref()
            .map(|(page, _)| page_text(page)),
    })
}

/// The segment lines PN-20 lists, borrowing `text`.
#[must_use]
pub fn segment_lines(text: &NodeRecordText) -> Vec<RecordSegmentLine<'_>> {
    text.segments
        .iter()
        .map(|(description, sound, readable)| RecordSegmentLine {
            description,
            sound: *sound,
            readable: *readable,
        })
        .collect()
}

/// The page's entries, borrowing `text`.
#[must_use]
pub fn page_lines(text: &NodeRecordText) -> Vec<AuditLine<'_>> {
    text.page
        .as_ref()
        .map(|page| {
            page.entries
                .iter()
                .map(|e| AuditLine {
                    action: &e.action,
                    mission_time_s: e.mission_time_s,
                    detail: &e.detail,
                    operator: e.operator,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// PN-20's view of the node's record.
#[must_use]
pub fn view<'a>(
    text: &'a NodeRecordText,
    segments: &'a [RecordSegmentLine<'a>],
    lines: &'a [AuditLine<'a>],
) -> NodeAuditRecordView<'a> {
    NodeAuditRecordView {
        node: &text.node,
        summary: &text.summary,
        sound: text.sound,
        note: &text.note,
        can_read: text.can_read,
        reading: text.reading,
        problems: &text.problems,
        segments,
        shown: text.page.as_ref().map(|page| NodeShownPage {
            segment: &page.segment,
            lines,
            note: &page.note,
            sound: page.sound,
            has_older: page.has_older,
            has_newer: page.has_newer,
        }),
    }
}
