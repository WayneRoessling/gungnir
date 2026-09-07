// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Engagement state and effect assessment.
//!
//! Design: docs/design/DN-06-engagement-and-effect.md. Capability CAP-4.6; measure
//! MOE-01; mission thread MT-01 step 8.
//!
//! **The edge this module refuses.** Engagement state is keyed by the decision, and
//! decisions live in `gungnir-command`, a productization crate. This is a service
//! facade, and a facade may not depend on productization
//! (agentic-coding-standards.md §1.1). So it keys on
//! `gungnir_model::DecisionId`, which the canonical model owns. That is the better
//! design rather than a workaround: the facade should not know how approvals are
//! stored.
//!
//! **`Indeterminate` is the point of this module.** Without it, an engagement whose
//! outcome nobody observed becomes either a false success or a false failure, and
//! MOE-01 computed from those numbers is worse than no number at all. "We do not
//! know" is a first-class result.

use gungnir_model::{DecisionId, MissionTime, PlanId, ResourceId, TrackId};

/// What an outcome rests on. Always present, so no conclusion is an assertion.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EffectEvidence {
    pub source: EffectSource,
    pub observed_at: MissionTime,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EffectSource {
    /// Inferred from the track's own lifecycle: deletion, coasting, a change of
    /// behaviour.
    ///
    /// **Weak evidence, and labelled as such.** A track deleted inside the window
    /// may have been destroyed, may have flown behind terrain, or may have been
    /// dropped by the tracker. Effect measures report this source separately from
    /// effector reports, so a deployment with no effector reporting cannot mistake
    /// track deletions for confirmed effect.
    TrackLifecycle,
    /// Reported by the effector system through the handoff channel.
    EffectorReport,
    /// A person judged it.
    OperatorAssessment,
}

impl EffectSource {
    /// True for evidence that came from outside the tracker.
    pub fn is_corroborated(self) -> bool {
        matches!(
            self,
            EffectSource::EffectorReport | EffectSource::OperatorAssessment
        )
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum EngagementState {
    /// Decided and handed off; nothing observed yet.
    Committed,
    /// The effector reported it acted.
    Executing,
    /// The engaged track ended in a way consistent with success.
    Effective { evidence: EffectEvidence },
    /// The track persisted past the window in which an effect was expected.
    Ineffective { evidence: EffectEvidence },
    /// Abandoned before an effect could be judged.
    Aborted { reason: String },
    /// The window closed and the evidence supports neither conclusion.
    Indeterminate { reason: String },
}

impl EngagementState {
    /// True while the engagement can still change.
    pub fn is_open(&self) -> bool {
        matches!(
            self,
            EngagementState::Committed | EngagementState::Executing
        )
    }

    /// The evidence behind a closed outcome, if it has any.
    pub fn evidence(&self) -> Option<&EffectEvidence> {
        match self {
            EngagementState::Effective { evidence } | EngagementState::Ineffective { evidence } => {
                Some(evidence)
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EngagementTransition {
    pub to: EngagementState,
    pub at: MissionTime,
}

/// One engagement, opened by a decision and closed by evidence or by time.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Engagement {
    pub decision: DecisionId,
    pub plan: PlanId,
    pub track: TrackId,
    pub resource: ResourceId,
    pub started: MissionTime,
    /// Mission time by which an effect is expected, from the layer's window.
    pub expect_effect_by: MissionTime,
    pub state: EngagementState,
    pub history: Vec<EngagementTransition>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EngagementError {
    #[error("engagement for decision {0:?} is already closed")]
    AlreadyClosed(DecisionId),
}

impl Engagement {
    /// Opens an engagement. Called once per accepted decision.
    pub fn open(
        decision: DecisionId,
        plan: PlanId,
        track: TrackId,
        resource: ResourceId,
        started: MissionTime,
        effect_window_s: f64,
    ) -> Self {
        Self {
            decision,
            plan,
            track,
            resource,
            started,
            expect_effect_by: MissionTime(started.0 + effect_window_s),
            state: EngagementState::Committed,
            history: Vec::new(),
        }
    }

    fn transition(&mut self, to: EngagementState, at: MissionTime) -> Result<(), EngagementError> {
        if !self.state.is_open() {
            return Err(EngagementError::AlreadyClosed(self.decision));
        }
        self.history
            .push(EngagementTransition { to: to.clone(), at });
        self.state = to;
        Ok(())
    }

    /// The effector reported it acted.
    pub fn executing(&mut self, at: MissionTime) -> Result<(), EngagementError> {
        self.transition(EngagementState::Executing, at)
    }

    /// Close with an outcome, which always carries its evidence.
    pub fn close_effective(&mut self, evidence: EffectEvidence) -> Result<(), EngagementError> {
        let at = evidence.observed_at;
        self.transition(EngagementState::Effective { evidence }, at)
    }

    pub fn close_ineffective(&mut self, evidence: EffectEvidence) -> Result<(), EngagementError> {
        let at = evidence.observed_at;
        self.transition(EngagementState::Ineffective { evidence }, at)
    }

    pub fn abort(
        &mut self,
        reason: impl Into<String>,
        at: MissionTime,
    ) -> Result<(), EngagementError> {
        self.transition(
            EngagementState::Aborted {
                reason: reason.into(),
            },
            at,
        )
    }

    /// Close as indeterminate: the window passed and nothing settled it.
    pub fn close_indeterminate(
        &mut self,
        reason: impl Into<String>,
        at: MissionTime,
    ) -> Result<(), EngagementError> {
        self.transition(
            EngagementState::Indeterminate {
                reason: reason.into(),
            },
            at,
        )
    }

    /// True when the effect window has passed with the engagement still open.
    pub fn window_has_closed(&self, now: MissionTime) -> bool {
        self.state.is_open() && now >= self.expect_effect_by
    }
}

/// Closes every engagement whose window passed without an outcome.
///
/// They close **indeterminate**, never as success or failure. Returns the decisions
/// that were closed so the caller can journal them.
pub fn close_stale(engagements: &mut [Engagement], now: MissionTime) -> Vec<DecisionId> {
    let mut closed = Vec::new();
    for e in engagements.iter_mut().filter(|e| e.window_has_closed(now)) {
        if e.close_indeterminate("the effect window closed with no observation", now)
            .is_ok()
        {
            closed.push(e.decision);
        }
    }
    closed
}

/// Outcome counts for a reporting period, with the two evidence sources kept apart.
///
/// A deployment with no effector reporting must not mistake track deletions for
/// confirmed effect, so the report never adds these two columns together.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EffectTally {
    pub effective_corroborated: u32,
    pub effective_track_inferred: u32,
    pub ineffective_corroborated: u32,
    pub ineffective_track_inferred: u32,
    pub indeterminate: u32,
    pub aborted: u32,
    pub open: u32,
}

impl EffectTally {
    pub fn of(engagements: &[Engagement]) -> Self {
        let mut tally = Self::default();
        for e in engagements {
            match &e.state {
                EngagementState::Committed | EngagementState::Executing => tally.open += 1,
                EngagementState::Aborted { .. } => tally.aborted += 1,
                EngagementState::Indeterminate { .. } => tally.indeterminate += 1,
                EngagementState::Effective { evidence } => {
                    if evidence.source.is_corroborated() {
                        tally.effective_corroborated += 1;
                    } else {
                        tally.effective_track_inferred += 1;
                    }
                }
                EngagementState::Ineffective { evidence } => {
                    if evidence.source.is_corroborated() {
                        tally.ineffective_corroborated += 1;
                    } else {
                        tally.ineffective_track_inferred += 1;
                    }
                }
            }
        }
        tally
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engagement() -> Engagement {
        Engagement::open(
            DecisionId(1),
            PlanId(1),
            TrackId(10),
            ResourceId(5),
            MissionTime(100.0),
            60.0,
        )
    }

    fn evidence(source: EffectSource, at: f64) -> EffectEvidence {
        EffectEvidence {
            source,
            observed_at: MissionTime(at),
            detail: "observed".into(),
        }
    }

    #[test]
    fn an_engagement_opens_committed_with_its_window_set() {
        let e = engagement();
        assert_eq!(e.state, EngagementState::Committed);
        assert!(e.state.is_open());
        assert_eq!(e.expect_effect_by, MissionTime(160.0));
        assert!(e.history.is_empty());
    }

    #[test]
    fn no_outcome_closes_without_an_evidence_record() {
        let mut e = engagement();
        e.close_effective(evidence(EffectSource::EffectorReport, 120.0))
            .expect("closes");
        let recorded = e.state.evidence().expect("an outcome carries its evidence");
        assert_eq!(recorded.source, EffectSource::EffectorReport);
        assert!(!recorded.detail.is_empty());
    }

    #[test]
    fn an_unobserved_outcome_closes_indeterminate_and_never_as_success_or_failure() {
        let mut engagements = vec![engagement()];
        let closed = close_stale(&mut engagements, MissionTime(200.0));
        assert_eq!(closed, vec![DecisionId(1)]);
        match &engagements[0].state {
            EngagementState::Indeterminate { reason } => assert!(!reason.is_empty()),
            other => panic!("an unobserved outcome must be indeterminate, got {other:?}"),
        }
    }

    #[test]
    fn an_engagement_inside_its_window_is_not_closed() {
        let mut engagements = vec![engagement()];
        let closed = close_stale(&mut engagements, MissionTime(150.0));
        assert!(closed.is_empty());
        assert!(engagements[0].state.is_open());
    }

    #[test]
    fn a_closed_engagement_cannot_be_reopened_or_re_closed() {
        let mut e = engagement();
        e.close_ineffective(evidence(EffectSource::TrackLifecycle, 170.0))
            .expect("closes");
        assert_eq!(
            e.executing(MissionTime(180.0)),
            Err(EngagementError::AlreadyClosed(DecisionId(1)))
        );
        assert_eq!(
            e.close_effective(evidence(EffectSource::EffectorReport, 190.0)),
            Err(EngagementError::AlreadyClosed(DecisionId(1)))
        );
    }

    #[test]
    fn every_transition_is_recorded_with_its_time() {
        let mut e = engagement();
        e.executing(MissionTime(110.0)).expect("executing");
        e.close_effective(evidence(EffectSource::EffectorReport, 130.0))
            .expect("closes");
        assert_eq!(e.history.len(), 2);
        assert_eq!(e.history[0].at, MissionTime(110.0));
        assert_eq!(e.history[1].at, MissionTime(130.0));
    }

    #[test]
    fn track_inferred_and_effector_reported_outcomes_are_counted_separately() {
        // MOE-01 must never add these two columns together.
        let mut inferred = engagement();
        inferred
            .close_effective(evidence(EffectSource::TrackLifecycle, 120.0))
            .expect("closes");

        let mut reported = Engagement::open(
            DecisionId(2),
            PlanId(1),
            TrackId(11),
            ResourceId(5),
            MissionTime(100.0),
            60.0,
        );
        reported
            .close_effective(evidence(EffectSource::EffectorReport, 120.0))
            .expect("closes");

        let tally = EffectTally::of(&[inferred, reported]);
        assert_eq!(tally.effective_corroborated, 1);
        assert_eq!(tally.effective_track_inferred, 1);
        assert_eq!(tally.indeterminate, 0);
    }

    #[test]
    fn a_tally_accounts_for_every_engagement_exactly_once() {
        let mut open = engagement();
        open.executing(MissionTime(110.0)).expect("executing");

        let mut aborted = Engagement::open(
            DecisionId(3),
            PlanId(1),
            TrackId(12),
            ResourceId(5),
            MissionTime(100.0),
            60.0,
        );
        aborted
            .abort("plan superseded", MissionTime(115.0))
            .expect("aborts");

        let mut unknown = Engagement::open(
            DecisionId(4),
            PlanId(1),
            TrackId(13),
            ResourceId(5),
            MissionTime(100.0),
            60.0,
        );
        unknown
            .close_indeterminate("nothing observed", MissionTime(170.0))
            .expect("closes");

        let tally = EffectTally::of(&[open, aborted, unknown]);
        let total = tally.open
            + tally.aborted
            + tally.indeterminate
            + tally.effective_corroborated
            + tally.effective_track_inferred
            + tally.ineffective_corroborated
            + tally.ineffective_track_inferred;
        assert_eq!(total, 3);
    }

    #[test]
    fn an_aborted_engagement_carries_its_reason() {
        let mut e = engagement();
        e.abort("plan superseded", MissionTime(115.0))
            .expect("aborts");
        match &e.state {
            EngagementState::Aborted { reason } => assert_eq!(reason, "plan superseded"),
            other => panic!("expected an abort, got {other:?}"),
        }
        assert!(!e.state.is_open());
    }
}
