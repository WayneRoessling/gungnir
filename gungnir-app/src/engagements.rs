// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The desktop's calls into the engagement half of the approval desk (GAP-043, DN-06),
//! and the tally PN-17 draws (GAP-131, D-57,
//! `docs/design/DN-31-node-approval-queue.md` §3).
//!
//! **Opening, observing and closing are `gungnir_approval::engagements`'.** They moved with
//! the decision that opens them: an engagement is opened by an actionable `DecisionRecord`
//! and by nothing else, and a second copy of that rule on a node would be a second answer
//! to what counts as acting (D-55). What is left here is the context this binary builds
//! and the counts a panel reads.

use crate::state::AppState;
use gungnir_command::DecisionRecord;
use gungnir_intercept_service::engagement::{EffectTally, EngagementState};
use gungnir_model::events::engagement_outcome as outcome;
use gungnir_model::{MissionTime, PlanId};
use gungnir_ui::panels::commander_summary::OutcomeCounts;

/// The outcome as the bus spells it (DN-06 §6: a string, because the model may not
/// depend on the facade's enum).
#[must_use]
pub fn outcome_label(state: &EngagementState) -> Option<&'static str> {
    Some(match state {
        EngagementState::Committed | EngagementState::Executing => return None,
        EngagementState::Effective { evidence } if evidence.source.is_corroborated() => {
            outcome::EFFECTIVE_CORROBORATED
        }
        EngagementState::Effective { .. } => outcome::EFFECTIVE_TRACK_INFERRED,
        EngagementState::Ineffective { evidence } if evidence.source.is_corroborated() => {
            outcome::INEFFECTIVE_CORROBORATED
        }
        EngagementState::Ineffective { .. } => outcome::INEFFECTIVE_TRACK_INFERRED,
        EngagementState::Aborted { .. } => outcome::ABORTED,
        EngagementState::Indeterminate { .. } => outcome::INDETERMINATE,
    })
}

/// Open the engagements an actionable decision implies: one per solution, and the handoff
/// with them. Returns how many opened.
pub fn open_for(state: &mut AppState, record: &DecisionRecord) -> usize {
    crate::desk::with_desk(state, |desk, cx, host| desk.open_for(cx, host, record))
}

/// The plan was superseded (DN-08 §5): its open engagements are abandoned before an
/// effect could be judged.
pub fn observe_superseded(state: &mut AppState, plan: PlanId, now: MissionTime) {
    crate::desk::with_desk(state, |desk, _cx, host| {
        desk.observe_superseded(host, plan, now);
    });
}

/// Every frame, after the picture has been pulled: the engaged track's lifecycle against
/// the window that opened with the decision (DN-06 §5).
pub fn sweep(state: &mut AppState) {
    crate::desk::with_desk(state, |desk, cx, host| desk.sweep_engagements(cx, host));
}

/// The facade's tally, as PN-17 takes it.
#[must_use]
pub fn outcome_counts(state: &AppState) -> OutcomeCounts {
    let t = EffectTally::of(&state.desk.engagements);
    OutcomeCounts {
        open: t.open,
        effective_corroborated: t.effective_corroborated,
        effective_track_inferred: t.effective_track_inferred,
        ineffective_corroborated: t.ineffective_corroborated,
        ineffective_track_inferred: t.ineffective_track_inferred,
        indeterminate: t.indeterminate,
        aborted: t.aborted,
    }
}
