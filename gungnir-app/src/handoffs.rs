// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The desktop's calls into the handoff half of the approval desk (GAP-040, DN-07 §5), and
//! the rows PN-06 and PN-20 draw (GAP-131, D-57,
//! `docs/design/DN-31-node-approval-queue.md` §3).
//!
//! **The one handoff builder is `gungnir_approval::handoffs`'.** It is the thing DN-31 §3
//! names outright: `Handoff::from_decision` is constructed in exactly one place in the
//! workspace, that place is behind `DecisionRecord::is_actionable`, and
//! `tests/no_execution_without_decision.rs` fails if a second appears anywhere. Moving it
//! rather than copying it for the node is the whole reason the crate exists.

use crate::state::AppState;
use gungnir_command::DecisionRecord;

/// One issued handoff and where its delivery stands, as the desk holds it.
pub use gungnir_approval::HandoffRecord;

/// Apply what the effector reported through the node (GAP-040, DN-06).
pub fn apply_report(
    state: &mut AppState,
    decision: gungnir_model::DecisionId,
    endpoint: &str,
    report: &gungnir_model::handoff::EffectorReport,
    at: gungnir_model::MissionTime,
) {
    crate::desk::with_desk(state, |desk, _cx, host| {
        desk.apply_report(host, decision, endpoint, report, at);
    });
}

/// Issue the handoff for an actionable decision. Called from the engagement path, which
/// has already refused a record that is not actionable.
pub fn issue_for(state: &mut AppState, record: &DecisionRecord) {
    crate::desk::with_desk(state, |desk, cx, host| desk.issue_for(cx, host, record));
}

/// Publish this desktop's whole current handoff set to its node again (GAP-145).
///
/// The same call `issue_for` makes through the host, without a new handoff to make it:
/// the set is a replacement, so sending it again costs a partner nothing and repairs a
/// node that came back without it.
///
/// **A desktop holding none publishes nothing.** An empty set is a claim -- "there are
/// none here" -- and a console that has issued nothing has no claim to make; it also
/// keeps quiet consoles out of the node's register, which is one producer slot each that
/// they would otherwise take (`PRODUCERS_PER_ITEM`).
pub fn republish_to_node(state: &mut AppState) {
    if state.desk.handoffs.is_empty() {
        return;
    }
    crate::desk::with_desk(state, |desk, _cx, host| {
        gungnir_approval::ApprovalHost::republish_handoffs(host, &desk.handoffs);
    });
}

/// The handoff rows PN-06 and PN-20 draw (GAP-040).
///
/// One builder for both panels, borrowed from the record for the frame. Unfiltered: PN-06
/// applies `handoff::stays_visible` itself so the *stays visible until delivered* rule
/// lives in one place, and PN-20 wants every row anyway.
#[must_use]
pub fn rows(state: &AppState) -> Vec<gungnir_ui::panels::handoff::HandoffRow<'_>> {
    state
        .desk
        .handoffs
        .iter()
        .map(|record| gungnir_ui::panels::handoff::HandoffRow {
            decision: record.handoff.decision,
            plan: record.handoff.plan,
            endpoint: record.endpoint.as_deref(),
            operator: &record.handoff.decided_by.operator,
            role: &record.handoff.decided_by.role,
            issued: record.handoff.issued,
            delivery: &record.delivery,
            reports: &record.reports,
        })
        .collect()
}
