// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The node's coverage answer and the gap acceptances it holds, as a linked desktop sees
//! them, and the acceptances this desktop sends it (GAP-194, D-131, D-134).
//!
//! **The node holds acceptances for its linked desktops**, as it holds the queue (D-55),
//! and judges each against the coverage answer it serves (D-134). So a linked desktop reads
//! that answer, `GET /v3/coverage` with its acceptance marks, and posts an acceptance to
//! `POST /v3/coverage/acceptances` rather than recording one of its own.
//!
//! **When the answer is read.** Once per connection, right after the subscription, as the
//! queue's starting picture is; again whenever the stream carries a planning event (an
//! acceptance recorded or re-opened on the node) or this desktop's own post has been
//! answered; and otherwise every [`COVERAGE_REFRESH`], because the node's coverage moves
//! with its sensors and no event says so. A failed read keeps the last answer and asks
//! again on the next forward tick.
//!
//! **An acceptance is posted until the node answers**, as a decision is: unreachable, a
//! `503` or a `504`, or a lapsed token, keeps it queued. A retry of one the node had
//! recorded is answered `409 already-accepted`, which tells this desktop the same thing.

use gungnir_model::{AcceptedGap, GapAcceptance, GapAcceptanceId};

/// The wire types a desktop reads and sends here, re-exported because `gungnir-remote` is
/// the only crate the desktop speaks to a node through.
pub use gungnir_api::v3::{
    ComputedCoverage, CoverageResponse, GapAcceptanceRefused, OfflineAcceptance,
};

/// How long a coverage answer is taken as current before it is read again, when nothing
/// on the stream has said it moved.
pub const COVERAGE_REFRESH: std::time::Duration = std::time::Duration::from_secs(5);

/// The node's coverage answer as this desktop last read it.
#[derive(Debug, Default, Clone)]
pub struct NodeCoverage {
    answer: Option<CoverageResponse>,
    stale: bool,
    read_at: Option<std::time::Instant>,
}

impl NodeCoverage {
    /// Replace the answer with one just read.
    pub fn take_answer(&mut self, answer: CoverageResponse) {
        self.answer = Some(answer);
        self.stale = false;
        self.read_at = Some(std::time::Instant::now());
    }

    /// The stream or an answered post said the answer moved.
    pub fn mark_stale(&mut self) {
        self.stale = true;
    }

    /// Whether it is owed a read: never read, said to have moved, or older than
    /// [`COVERAGE_REFRESH`].
    #[must_use]
    pub fn is_due(&self) -> bool {
        self.stale
            || self
                .read_at
                .is_none_or(|at| at.elapsed() >= COVERAGE_REFRESH)
    }

    /// The answer, if one has been read.
    #[must_use]
    pub fn answer(&self) -> Option<&CoverageResponse> {
        self.answer.as_ref()
    }

    /// Forget the answer: the link went down, and an answer from a node this desktop can
    /// no longer hear is not one to draw acceptances from.
    pub fn forget(&mut self) {
        *self = NodeCoverage::default();
    }
}

/// An acceptance on its way to the node.
#[derive(Debug, Clone, PartialEq)]
pub struct OutboundAcceptance {
    pub gap: AcceptedGap,
    pub reason: String,
    /// Set when this desktop accepted the gap while cut off and forwards it now: who
    /// accepted it, when and where (D-131).
    pub accepted_offline: Option<OfflineAcceptance>,
    /// The desktop's own acceptance this forwards, when it forwards one, so the answer is
    /// matched back to it.
    pub local: Option<GapAcceptanceId>,
    /// Posts that went unanswered, for the panel.
    pub attempts: u32,
}

/// What the node said about one acceptance.
#[derive(Debug, Clone, PartialEq)]
pub enum AcceptanceAnswer {
    /// `201`: the node holds it.
    Recorded(GapAcceptance),
    /// `409`: the node holds none for it, and why.
    Refused(GapAcceptanceRefused),
    /// `400`, `403`, or a body this desktop could not read: nothing was recorded.
    Rejected { status: u16, reason: String },
}

impl AcceptanceAnswer {
    /// The answer in words, for an alert.
    #[must_use]
    pub fn sentence(&self) -> String {
        match self {
            AcceptanceAnswer::Recorded(a) => {
                format!(
                    "the node holds gap acceptance {} of {}",
                    a.id,
                    a.gap.describe()
                )
            }
            AcceptanceAnswer::Refused(refused) => refused.sentence(),
            AcceptanceAnswer::Rejected { status, reason } => {
                format!("the node would not take it ({status}): {reason}")
            }
        }
    }
}

/// One acceptance, answered.
#[derive(Debug, Clone, PartialEq)]
pub struct AcceptanceOutcome {
    pub gap: AcceptedGap,
    pub local: Option<GapAcceptanceId>,
    pub answer: AcceptanceAnswer,
}

/// Whether a post's status means the node has not answered yet, so the acceptance goes
/// again: the decision route's rule (`crate::queue::retry_under_same_key`).
#[must_use]
pub fn retry(status: u16) -> bool {
    crate::queue::retry_under_same_key(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_is_due_until_read_and_again_once_said_to_move() {
        let mut coverage = NodeCoverage::default();
        assert!(coverage.is_due() && coverage.answer().is_none());
        coverage.take_answer(CoverageResponse::NotComputed {
            reason: "no approach".into(),
        });
        assert!(!coverage.is_due());
        coverage.mark_stale();
        assert!(coverage.is_due());
        coverage.forget();
        assert!(coverage.answer().is_none());
    }
}
