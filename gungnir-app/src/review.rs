// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The after-action review on the desktop (GAP-049, DN-20).
//!
//! `gungnir_workflow::review` owns the case, the findings and the rule that a review
//! concludes before it closes and closes only over closed actions. Nothing constructed one.
//! This is the desktop's side: the review is opened against the session under replay (or
//! the live one), a finding records the replay cursor's mission time so the reviewer can
//! seek back to it, and every state change goes on the bus naming who made it.
//!
//! **Nothing here is automatic** (DN-20 §5). The system proposes no findings and scores
//! nothing; it assembles the record and holds what people concluded.
//!
//! Not built, and said where it matters: assigning an action (owner, due date) to a finding
//! has no control, so every review closes with zero actions and the "actions open" guard is
//! exercised only by the workflow's own tests; a finding's `refers_to` subjects wait on a
//! selection the panel can read; PN-12's timeline marks and PN-17's open-review count are
//! DN-20 §7 rows not yet drawn.

use crate::state::AppState;
use crate::sustainment::SustainmentState;
use crate::update::publish;
use gungnir_eventing::Event;
use gungnir_model::events::ReviewEvent;
use gungnir_model::{MissionTime, SessionId};
use gungnir_ui::panels::reports::{
    FindingKindView, FindingLine, ReviewAction, ReviewCaseView, ReviewStateView, ReviewView,
};
use gungnir_workflow::{Finding, FindingId, FindingKind, ReviewCase, ReviewState};

fn kind_from_view(kind: FindingKindView) -> FindingKind {
    match kind {
        FindingKindView::SystemBehaviour => FindingKind::SystemBehaviour,
        FindingKindView::Procedure => FindingKind::Procedure,
        FindingKindView::Practice => FindingKind::Practice,
        FindingKindView::Configuration => FindingKind::Configuration,
    }
}

fn kind_to_view(kind: FindingKind) -> FindingKindView {
    match kind {
        FindingKind::SystemBehaviour => FindingKindView::SystemBehaviour,
        FindingKind::Procedure => FindingKindView::Procedure,
        FindingKind::Practice => FindingKindView::Practice,
        FindingKind::Configuration => FindingKindView::Configuration,
    }
}

/// The kind as the bus spells it: the workflow's serialised (kebab-case) form, and a
/// test in `gungnir-workflow`'s own module keeps that spelling.
fn kind_label(kind: FindingKind) -> String {
    match kind {
        FindingKind::SystemBehaviour => "system-behaviour",
        FindingKind::Procedure => "procedure",
        FindingKind::Practice => "practice",
        FindingKind::Configuration => "configuration",
    }
    .to_string()
}

/// The finding rows as PN-13 takes them.
#[must_use]
pub fn finding_lines(case: &ReviewCase) -> Vec<FindingLine> {
    case.findings
        .iter()
        .map(|f| FindingLine {
            id: f.id.0,
            summary: f.summary.clone(),
            kind: kind_to_view(f.kind),
            at_s: f.at.map(|t| t.0),
            promoted_to: f.promoted_to_gap.clone(),
        })
        .collect()
}

/// The session a review would be about: the one under replay, else the live one.
fn reviewable_session(state: &AppState, sustainment: &SustainmentState) -> Option<SessionId> {
    sustainment
        .replay
        .open_session()
        .or_else(|| state.session())
}

/// PN-13's review section.
#[must_use]
pub fn review_view<'a>(
    state: &AppState,
    sustainment: &SustainmentState,
    lines: &'a [FindingLine],
) -> ReviewView<'a> {
    match &sustainment.review {
        Some(case) => ReviewView {
            case: Some(ReviewCaseView {
                session: case.session.0,
                state: match case.state {
                    ReviewState::Open => ReviewStateView::Open,
                    ReviewState::Concluded => ReviewStateView::Concluded,
                    ReviewState::Closed => ReviewStateView::Closed,
                },
                findings: lines,
                open_actions: case.open_actions(),
                replay_open: sustainment.replay.open_session() == Some(case.session),
            }),
            cannot_open: None,
        },
        None => ReviewView {
            case: None,
            cannot_open: reviewable_session(state, sustainment)
                .is_none()
                .then_some("no session is open to review: open one, or replay one"),
        },
    }
}

fn operator(state: &AppState) -> Option<String> {
    state.attributed_operator().map(|o| o.0.to_string())
}

/// Apply what the reviewer asked for. Every refusal is an alert, never silence.
pub fn apply(state: &mut AppState, sustainment: &mut SustainmentState, action: ReviewAction) {
    match action {
        ReviewAction::Open => open(state, sustainment),
        ReviewAction::RecordFinding { summary, kind } => {
            record_finding(state, sustainment, summary, kind);
        }
        ReviewAction::SeekTo(id) => seek_to(state, sustainment, id),
        ReviewAction::Conclude => conclude(state, sustainment),
        ReviewAction::Close => close(state, sustainment),
        ReviewAction::Promote { finding, gap } => promote(state, sustainment, finding, &gap),
    }
}

fn open(state: &mut AppState, sustainment: &mut SustainmentState) {
    let now = state.clock.now();
    let operator = operator(state);
    if sustainment
        .review
        .as_ref()
        .is_some_and(|r| r.state != ReviewState::Closed)
    {
        state
            .alerts
            .push("a review is already open; conclude and close it first".into());
        return;
    }
    let Some(session) = reviewable_session(state, sustainment) else {
        state
            .alerts
            .push("no session is open to review: open one, or replay one".into());
        return;
    };
    sustainment.review = Some(ReviewCase::open(session));
    sustainment.next_finding = 0;
    publish(
        state,
        now,
        Event::Review(ReviewEvent::Opened {
            session,
            operator,
            at: now,
        }),
    );
    crate::audit::record(
        state,
        gungnir_security::actions::REVIEW_CONDUCT,
        format!("opened a review of session {}", session.0),
    );
}

fn record_finding(
    state: &mut AppState,
    sustainment: &mut SustainmentState,
    summary: String,
    kind: FindingKindView,
) {
    let now = state.clock.now();
    let operator = operator(state);
    let Some(case) = sustainment.review.as_mut() else {
        state
            .alerts
            .push("no review is open to record a finding in".into());
        return;
    };
    if case.state != ReviewState::Open {
        state
            .alerts
            .push("the review is concluded; findings are recorded before that".into());
        return;
    }
    // The moment the reviewer is looking at, when the replay of *this* session is
    // open. A finding recorded while looking at a different session, or at the
    // live picture, has no moment and is drawn as an anecdote (DN-20 §5).
    let at = (sustainment.replay.open_session() == Some(case.session))
        .then(|| sustainment.replay.clock())
        .flatten();
    sustainment.next_finding += 1;
    let id = FindingId(sustainment.next_finding);
    let kind = kind_from_view(kind);
    case.findings.push(Finding {
        id,
        summary,
        kind,
        at,
        refers_to: Vec::new(),
        action: None,
        promoted_to_gap: None,
    });
    let session = case.session;
    sustainment.review_draft.summary.clear();
    publish(
        state,
        now,
        Event::Review(ReviewEvent::FindingRecorded {
            session,
            finding: id.0,
            kind: kind_label(kind),
            refers_to: at,
            operator,
            at: now,
        }),
    );
    crate::audit::record(
        state,
        gungnir_security::actions::REVIEW_CONDUCT,
        format!(
            "recorded finding #{} ({}) in the review of session {}",
            id.0,
            kind_label(kind),
            session.0
        ),
    );
}

fn seek_to(state: &mut AppState, sustainment: &mut SustainmentState, id: u64) {
    let Some(case) = sustainment.review.as_ref() else {
        return;
    };
    let Some(at) = case
        .findings
        .iter()
        .find(|f| f.id == FindingId(id))
        .and_then(|f| f.at)
    else {
        state
            .alerts
            .push(format!("finding #{id} records no moment to seek to"));
        return;
    };
    if sustainment.replay.open_session() != Some(case.session) {
        state.alerts.push(format!(
            "open the replay of session {} to seek to finding #{id}",
            case.session.0
        ));
        return;
    }
    sustainment.replay.seek_to(at);
}

fn conclude(state: &mut AppState, sustainment: &mut SustainmentState) {
    let now = state.clock.now();
    let operator = operator(state);
    let Some(case) = sustainment.review.as_mut() else {
        return;
    };
    if case.state != ReviewState::Open {
        return;
    }
    case.conclude();
    let (session, findings) = (case.session, case.findings.len());
    publish(
        state,
        now,
        Event::Review(ReviewEvent::Concluded {
            session,
            findings,
            operator,
            at: now,
        }),
    );
    crate::audit::record(
        state,
        gungnir_security::actions::REVIEW_CONDUCT,
        format!(
            "concluded the review of session {} with {findings} findings",
            session.0
        ),
    );
}

fn close(state: &mut AppState, sustainment: &mut SustainmentState) {
    let now = state.clock.now();
    let operator = operator(state);
    let Some(case) = sustainment.review.as_mut() else {
        return;
    };
    match case.close() {
        Ok(()) => {
            let session = case.session;
            publish(
                state,
                now,
                Event::Review(ReviewEvent::Closed {
                    session,
                    operator,
                    at: now,
                }),
            );
            crate::audit::record(
                state,
                gungnir_security::actions::REVIEW_CONDUCT,
                format!("closed the review of session {}", session.0),
            );
        }
        Err(err) => state
            .alerts
            .push(format!("the review did not close: {err}")),
    }
}

fn promote(state: &mut AppState, sustainment: &mut SustainmentState, finding: u64, gap: &str) {
    let now = state.clock.now();
    let operator = operator(state);
    let Some(case) = sustainment.review.as_mut() else {
        return;
    };
    let gap = gap.trim().to_string();
    if !gap.starts_with("GAP-") || gap.len() < 5 {
        state.alerts.push(format!(
            "a finding is promoted into a register entry, and {gap:?} is not one"
        ));
        return;
    }
    match case.promote(FindingId(finding), gap.clone()) {
        Ok(()) => {
            let session = case.session;
            sustainment.review_draft.gap.clear();
            publish(
                state,
                now,
                Event::Review(ReviewEvent::FindingPromoted {
                    session,
                    finding,
                    gap: gap.clone(),
                    operator,
                    at: now,
                }),
            );
            crate::audit::record(
                state,
                gungnir_security::actions::REVIEW_CONDUCT,
                format!("promoted finding #{finding} to {gap}"),
            );
        }
        Err(err) => state
            .alerts
            .push(format!("finding #{finding} was not promoted: {err}")),
    }
}

/// The moment the replay cursor is at, for the tests that read it back.
#[must_use]
pub fn replay_clock(sustainment: &SustainmentState) -> Option<MissionTime> {
    sustainment.replay.clock()
}
