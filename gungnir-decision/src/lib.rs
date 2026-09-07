// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Decision support beyond DP allocation, per docs/gungnir-capabilities.md §5.4.
//! `gungnir_intercept_service::DpInterceptService` gives one optimized assignment;
//! real operational decisions usually need alternatives, explainability, and
//! "what if" before committing to a plan. Every course of action carries the policy
//! verdict it was evaluated under, so the UI never shows an un-checked alternative.

pub mod alternatives;
pub mod sensor_plan;

pub use alternatives::{PlanAlternatives, PlanSource, PlanUnavailable};
pub use sensor_plan::{
    SensorCandidate, SensorModeChange, SensorPlan, SensorPlanCost, SensorPlanner,
};

use gungnir_assessment::RiskScore;
use gungnir_model::{PlanView, TrackView};
use gungnir_policy::PolicyVerdict;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CourseOfAction {
    pub plan: PlanView,
    pub policy_verdict: PolicyVerdict,
    pub rationale: String,
}

/// Alternatives, explainability and what-if over one snapshot.
///
/// Implemented by [`alternatives::PlanAlternatives`] (GAP-032). Until that landed this
/// trait had no implementor anywhere in the workspace, which is a stub that says nothing
/// at runtime: nothing panics, nothing errors, and no health flag is false -- the
/// capability simply never runs. A second implementor is expected when a node answers
/// these questions remotely; nothing here assumes the desktop is the only one.
pub trait DecisionSupport: Send + Sync {
    /// Generates the primary recommendation plus N ranked alternatives, each
    /// already policy-checked, so the UI can show "why this one, not that one."
    fn recommend(&mut self, max_alternatives: usize) -> Vec<CourseOfAction>;

    /// Re-runs allocation against a hypothetical track snapshot without committing
    /// to it -- the "what if we lost sensor 3" question. Must not mutate live state.
    fn what_if(&self, hypothetical_tracks: &[TrackView]) -> CourseOfAction;
}

/// Human-readable explanation of a plan in terms of the risk scores that produced
/// its reward matrix: one line per assignment, highest risk first.
pub fn rationale_for(plan: &PlanView, scores: &[RiskScore]) -> String {
    if plan.is_empty() {
        return "No assignment: no tracks, no ready resources, or the allocator declined."
            .to_string();
    }
    let mut lines: Vec<(f32, String)> = plan
        .solutions()
        .iter()
        .map(|s| {
            let score = scores.iter().find(|r| r.track_id == s.track);
            let risk = score.map_or(0.0, |r| r.score);
            let tti = score.and_then(|r| r.time_to_impact_s).map_or_else(
                || "not approaching".to_string(),
                |t| format!("{t:.0} s to impact"),
            );
            (
                risk,
                format!(
                    "resource {} -> track {} (risk {risk:.2}, {tti})",
                    s.resource.0, s.track.0
                ),
            )
        })
        .collect();
    lines.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let body: Vec<String> = lines.into_iter().map(|(_, l)| l).collect();
    format!(
        "Plan #{} (policy value {:.2}):\n{}",
        plan.id.0,
        plan.policy_value,
        body.join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::{InterceptSolutionView, MissionTime, PlanId, ResourceId, TrackId};

    #[test]
    fn rationale_lists_assignments_highest_risk_first() {
        let plan = PlanView {
            id: PlanId(4),
            mission_time: MissionTime(0.0),
            kind: gungnir_model::PlanKind::Intercept {
                solutions: vec![
                    InterceptSolutionView {
                        resource: ResourceId(1),
                        track: TrackId(10),
                        intercept_point: None,
                        time_to_intercept_s: None,
                    },
                    InterceptSolutionView {
                        resource: ResourceId(2),
                        track: TrackId(20),
                        intercept_point: None,
                        time_to_intercept_s: None,
                    },
                ],
            },
            policy_value: 3.5,
            releasability: gungnir_model::Releasability::default(),
        };
        let scores = vec![
            RiskScore {
                track_id: TrackId(10),
                score: 0.2,
                time_to_impact_s: None,
                exposure: None,
            },
            RiskScore {
                track_id: TrackId(20),
                score: 0.9,
                time_to_impact_s: Some(30.0),
                exposure: None,
            },
        ];
        let text = rationale_for(&plan, &scores);
        let first = text.lines().nth(1).expect("first assignment line");
        assert!(first.contains("track 20"), "{text}");
        assert!(first.contains("30 s to impact"), "{text}");
    }

    #[test]
    fn empty_plan_explains_itself() {
        assert!(rationale_for(&PlanView::default(), &[]).starts_with("No assignment"));
    }
}
