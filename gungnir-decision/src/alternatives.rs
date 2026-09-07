// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Alternatives and what-if over a proposed plan (GAP-032).
//!
//! [`crate::DecisionSupport`] has been a trait with **no implementor anywhere in the
//! workspace** since the crate landed, which is the least discoverable stub in the
//! repository: a `todo!()` panics where it is reached and a `NotImplemented` error is
//! reported to somebody, but a trait nobody implements simply never runs, and no health
//! flag, panel or test says so. [`PlanAlternatives`] is the implementor, and
//! `gungnir-app`'s `decisions` module constructs one on every plan the tick proposes so
//! the answer reaches a person rather than a type signature.
//!
//! # Why the allocator is passed in rather than depended on
//!
//! `ARCHITECTURE.md` §7.1 draws `gungnir-decision ──► model, assessment, policy,
//! analytics`. There is no edge to `gungnir-allocation` or to
//! `gungnir-intercept-service`, and this file does not add one: the caller supplies the
//! allocator as a [`PlanSource`]. That is the same shape, for the same reason, as
//! [`crate::SensorCandidate`] -- the crate that owns the constraint enumerates it and
//! this crate searches over what it is given. It also has a second benefit that matters
//! here: the allocator the caller passes in can be a *fresh* one, which is how the
//! desktop keeps a rehearsal off the planner whose answer it is rehearsing against.
//!
//! # Why `what_if` cannot disturb the live picture
//!
//! `docs/verification-capability-table.md` requires that live state is unchanged after
//! `what_if`. Every field of [`PlanAlternatives`] is a shared borrow and
//! [`crate::DecisionSupport::what_if`] takes `&self`, so a `what_if` that wrote to the
//! live tracks, the live resource pool or the live scores would not compile. That is a
//! stronger guarantee than a test, and the test in this module is still written, because
//! the guarantee holds only as long as the fields stay shared borrows and a future
//! `RefCell` would pass the compiler while breaking the criterion.
//!
//! # What an alternative *is*
//!
//! The question the panel has to answer is "why this one, and not that one." An
//! alternative is therefore the plan the same allocator returns **with one of the
//! resources the primary tasked taken out of the pool** -- the "what if we lost that
//! effector" plan -- tried in descending order of the risk of the track that resource
//! was sent against, so the most consequential substitution is offered first. Nothing
//! here reweights the reward matrix or relaxes a constraint: an alternative produced by
//! quietly changing what the allocator was optimising would be a different question's
//! answer presented as this question's.

use gungnir_assessment::{RiskScore, ThreatAssessor};
use gungnir_model::{MissionTime, PlanView, ResourceId, ResourceView, TrackView};
use gungnir_policy::{PolicyEngine, PolicyVerdict};

use crate::{rationale_for, CourseOfAction, DecisionSupport};

/// Why no plan could be produced for a snapshot.
///
/// An explicit error rather than an empty plan, because **an empty plan is a real
/// answer** -- "this snapshot needs no action" -- and "the allocator could not answer"
/// is not the same claim. That is the distinction `gungnir_intercept_service::PlanOutcome`
/// draws one layer up, and collapsing it here would undo it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlanUnavailable {
    /// The allocator declined or failed, carrying the reason it gave.
    #[error("the allocator produced no plan: {reason}")]
    Allocator { reason: String },
}

/// The allocator, supplied by the caller.
///
/// Takes `&self` rather than `&mut self` deliberately: [`DecisionSupport::what_if`] is a
/// `&self` method because it must not change anything, and a plan source that needed
/// `&mut` to answer would be a plan source that had something to change.
pub trait PlanSource: Send + Sync {
    /// The plan this allocator returns for the snapshot it is handed.
    ///
    /// `resources` is the pool to solve over, which for an alternative is the live pool
    /// with one resource removed; the caller judges readiness and reserves as it
    /// normally would.
    ///
    /// # Errors
    ///
    /// [`PlanUnavailable`] when the allocator could not answer for this snapshot at all.
    /// A snapshot that needs no action is `Ok` with an empty plan, not an error.
    fn plan(
        &self,
        now: MissionTime,
        tracks: &[TrackView],
        resources: &[ResourceView],
    ) -> Result<PlanView, PlanUnavailable>;
}

/// Course-of-action generation over one live snapshot (GAP-032).
///
/// Construct one per use. It borrows the live picture and holds nothing across calls, so
/// there is no cached answer that could be shown against a snapshot it was not computed
/// for -- the failure mode GAP-066 exists to name one layer up.
///
/// # The plan identifiers on the courses of action are not proposal identifiers
///
/// A [`CourseOfAction`] carries whatever `id` its [`PlanSource`] minted. Only the plan an
/// operator chooses is submitted, and submission is what puts a plan and its identifier
/// into the record. An alternative that is never chosen is never numbered in the journal,
/// which is why nothing here renumbers one to look as though it were.
pub struct PlanAlternatives<'a> {
    /// The mission time the snapshot is being planned for.
    pub now: MissionTime,
    /// The live tracks. Shared, so `what_if` cannot substitute its hypothetical set.
    pub tracks: &'a [TrackView],
    /// The live resource pool, and the pool every verdict is judged against: an
    /// alternative that withholds a resource is still a plan tasking real effectors, so
    /// policy must see their real readiness rather than the reduced pool the search used.
    pub resources: &'a [ResourceView],
    /// The risk scores behind the live snapshot's reward matrix, for [`rationale_for`].
    pub scores: &'a [RiskScore],
    /// The allocator. See the module documentation for why it is passed in.
    pub source: &'a dyn PlanSource,
    /// The policy chain every course of action is evaluated by. **Not optional**: the
    /// verification row requires every alternative to carry a real verdict, and a
    /// default or an assumed pass would put an un-checked option in front of an operator.
    pub policy: &'a dyn PolicyEngine,
    /// Scores a hypothetical snapshot for [`DecisionSupport::what_if`].
    ///
    /// `None` where the deployment cannot score at all -- no local frame origin, or no
    /// defended assets -- which is a state the desktop is genuinely in. The what-if is
    /// still produced, and its rationale says the risk figures are absent rather than
    /// letting a row of zeroes read as "no track matters."
    pub assessor: Option<&'a dyn ThreatAssessor>,
}

impl PlanAlternatives<'_> {
    /// One course of action: a plan, the verdict the chain actually returned for it, and
    /// the rationale under a heading saying which question this plan answers.
    fn course(&self, plan: PlanView, heading: &str, scores: &[RiskScore]) -> CourseOfAction {
        CourseOfAction {
            // The verdict is the chain's, run here and now over this plan and the live
            // resource pool. Nothing in this file constructs a `PolicyVerdict` itself.
            policy_verdict: self.policy.evaluate(&plan, self.resources),
            rationale: format!("{heading}\n{}", rationale_for(&plan, scores)),
            plan,
        }
    }

    /// The course of action for a snapshot the allocator could not answer for.
    ///
    /// The empty plan is still run through the chain, so the verdict is real rather than
    /// invented, and the heading names the allocator's own reason. A caller that showed
    /// this as an ordinary empty plan would be reporting "nothing to do" for a snapshot
    /// nobody could plan.
    fn declined(&self, err: &PlanUnavailable, heading: &str) -> CourseOfAction {
        let plan = PlanView::default();
        CourseOfAction {
            policy_verdict: self.policy.evaluate(&plan, self.resources),
            rationale: format!("{heading}\nNo course of action could be generated: {err}."),
            plan,
        }
    }

    /// The resources worth taking out of the pool, most consequential first.
    ///
    /// Ordered by the risk of the track each was tasked against, so the first alternative
    /// offered is the substitution for the effector on the worst threat. A resource the
    /// primary tasked twice appears once: withholding it is one hypothesis, not two.
    fn withholding_order(&self, primary: &PlanView) -> Vec<ResourceId> {
        let mut ranked: Vec<(f32, ResourceId)> = primary
            .assignments()
            .into_iter()
            .map(|(resource, track)| {
                let risk = self
                    .scores
                    .iter()
                    .find(|s| s.track_id == track)
                    .map_or(0.0, |s| s.score);
                (risk, resource)
            })
            .collect();
        ranked.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mut order: Vec<ResourceId> = Vec::new();
        for (_, resource) in ranked {
            if !order.contains(&resource) {
                order.push(resource);
            }
        }
        order
    }
}

/// True when a course of action can never be acted on, whatever a person decides.
fn is_denied(course: &CourseOfAction) -> bool {
    matches!(course.policy_verdict, PolicyVerdict::Denied { .. })
}

impl DecisionSupport for PlanAlternatives<'_> {
    /// The primary recommendation first, then up to `max_alternatives` others.
    ///
    /// The primary keeps position zero whatever its verdict, because it is the plan the
    /// allocator returned for the picture in front of the operator and reordering it
    /// behind an alternative would misreport which one the system recommends. The
    /// alternatives are ranked with the ones that could still be acted on ahead of the
    /// ones policy has already refused, and within each group by policy value descending:
    /// an option nobody may take is not a better option than one somebody may.
    ///
    /// An alternative whose assignments match the primary's, or an earlier alternative's,
    /// is dropped -- the same plan under a different heading is not a second option.
    /// An alternative the allocator could not compute is dropped too: the primary is the
    /// answer, and a hypothesis with no plan behind it is not an alternative to it.
    fn recommend(&mut self, max_alternatives: usize) -> Vec<CourseOfAction> {
        let primary = match self.source.plan(self.now, self.tracks, self.resources) {
            Ok(plan) => plan,
            Err(err) => {
                return vec![self.declined(&err, "Recommended: nothing, for the live picture.")]
            }
        };
        let heading = format!(
            "Recommended, for the {} live track(s) and the {} resource(s) in the pool.",
            self.tracks.len(),
            self.resources.len()
        );
        let mut courses = vec![self.course(primary.clone(), &heading, self.scores)];

        let mut alternatives: Vec<CourseOfAction> = Vec::new();
        for withheld in self.withholding_order(&primary) {
            if alternatives.len() >= max_alternatives {
                break;
            }
            let pool: Vec<ResourceView> = self
                .resources
                .iter()
                .filter(|r| r.id != withheld)
                .cloned()
                .collect();
            let Ok(plan) = self.source.plan(self.now, self.tracks, &pool) else {
                continue;
            };
            if plan.assignments() == primary.assignments()
                || alternatives
                    .iter()
                    .any(|c| c.plan.assignments() == plan.assignments())
            {
                continue;
            }
            let heading = format!(
                "Alternative, if resource {} were unavailable (policy value {:.2} against \
                 the recommendation's {:.2}).",
                withheld.0, plan.policy_value, primary.policy_value
            );
            alternatives.push(self.course(plan, &heading, self.scores));
        }
        alternatives.sort_by(|a, b| {
            is_denied(a)
                .cmp(&is_denied(b))
                .then_with(|| b.plan.policy_value.total_cmp(&a.plan.policy_value))
        });
        courses.append(&mut alternatives);
        courses
    }

    /// The plan for a hypothetical track snapshot, committed to nothing.
    ///
    /// Nothing here writes: the fields are shared borrows and the allocator is a fresh
    /// one supplied by the caller, so the live tracks, the live resource pool, the live
    /// scores and the caller's own planner are all as they were. The rationale says so
    /// too, because a course of action that reached a screen without the word
    /// "hypothetical" on it would be read as a proposal.
    fn what_if(&self, hypothetical_tracks: &[TrackView]) -> CourseOfAction {
        let scores = self.assessor.map(|a| a.assess(hypothetical_tracks));
        let caveat = if scores.is_none() {
            " This deployment can score no tracks, so the risk and time-to-impact figures \
             below are absent rather than computed."
        } else {
            ""
        };
        let heading = format!(
            "What if: {} hypothetical track(s) in place of the {} live ones. A rehearsal \
             -- nothing here is proposed, queued or recorded, and the live picture is \
             unchanged.{caveat}",
            hypothetical_tracks.len(),
            self.tracks.len()
        );
        match self
            .source
            .plan(self.now, hypothetical_tracks, self.resources)
        {
            Ok(plan) => self.course(plan, &heading, scores.as_deref().unwrap_or(&[])),
            Err(err) => self.declined(&err, &heading),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::{
        AuthorityRule, AuthoritySettings, Classification, ControlStatusSettings, EffectorLayer,
        Geodetic, InterceptSolutionView, PlanId, PlanKind, Provenance, Quality, TrackId,
        TrackStatus, WeaponsControlStatus,
    };
    use gungnir_policy::{AuthorityPolicy, ControlStatusPolicy, DenialReason, PolicyChain};
    use std::sync::atomic::{AtomicUsize, Ordering};

    const DECIDE: &str = "decide_plan";

    /// A greedy stand-in for the DP allocator: highest-risk track first, one adequate
    /// resource each, in pool order.
    ///
    /// A test double and nothing more -- the real allocator is `gungnir-allocation`, which
    /// this crate may not depend on, and the property under test is what
    /// [`PlanAlternatives`] does with an allocator's answers rather than what the answers
    /// are. It counts its calls so a test can prove `what_if` planned and still changed
    /// nothing.
    struct GreedySource {
        scores: Vec<RiskScore>,
        calls: AtomicUsize,
        /// When set, the allocator refuses, as the real one does when the solve fails.
        refuse: Option<String>,
    }

    impl GreedySource {
        fn new(scores: Vec<RiskScore>) -> Self {
            Self {
                scores,
                calls: AtomicUsize::new(0),
                refuse: None,
            }
        }
    }

    impl PlanSource for GreedySource {
        fn plan(
            &self,
            now: MissionTime,
            tracks: &[TrackView],
            resources: &[ResourceView],
        ) -> Result<PlanView, PlanUnavailable> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some(reason) = &self.refuse {
                return Err(PlanUnavailable::Allocator {
                    reason: reason.clone(),
                });
            }
            let mut ranked: Vec<&TrackView> = tracks.iter().collect();
            ranked.sort_by(|a, b| {
                let risk = |t: &TrackView| {
                    self.scores
                        .iter()
                        .find(|s| s.track_id == t.id)
                        .map_or(0.0, |s| s.score)
                };
                risk(b).total_cmp(&risk(a))
            });
            let ready: Vec<&ResourceView> = resources.iter().filter(|r| r.is_adequate()).collect();
            let mut value = 0.0;
            let solutions: Vec<InterceptSolutionView> = ranked
                .iter()
                .zip(ready.iter())
                .map(|(track, resource)| {
                    value += f64::from(
                        self.scores
                            .iter()
                            .find(|s| s.track_id == track.id)
                            .map_or(0.0, |s| s.score),
                    );
                    InterceptSolutionView {
                        resource: resource.id,
                        track: track.id,
                        intercept_point: None,
                        time_to_intercept_s: None,
                    }
                })
                .collect();
            Ok(PlanView {
                id: PlanId(1),
                mission_time: now,
                kind: PlanKind::Intercept { solutions },
                policy_value: value,
                releasability: gungnir_model::Releasability::default(),
            })
        }
    }

    /// A track whose only interesting field is its identifier.
    ///
    /// The kinematics are left at zero and named through `Default` rather than through
    /// `nalgebra`: this crate has no `nalgebra` dependency and does not gain one for a
    /// test fixture. Nothing under test reads the state -- the risk ordering comes from
    /// the scores, exactly as it does from `gungnir-assessment` in the desktop.
    ///
    /// `default_trait_access` is allowed rather than obeyed for the same reason: naming
    /// the matrix type would name `nalgebra`.
    #[allow(clippy::default_trait_access)]
    fn track(id: u64) -> TrackView {
        TrackView {
            id: TrackId(id),
            status: TrackStatus::Confirmed,
            state: Default::default(),
            covariance: Default::default(),
            classification: Classification::Hostile,
            provenance: Provenance::default(),
            quality: Quality::default(),
            mission_time: MissionTime(0.0),
            releasability: gungnir_model::Releasability::default(),
        }
    }

    fn resource(id: u32) -> ResourceView {
        ResourceView {
            id: ResourceId(id),
            position: Geodetic {
                lat_rad: 0.0,
                lon_rad: 0.0,
                alt_m: 0.0,
            },
            capacity: 1,
            ready: true,
            layer: EffectorLayer::Point,
            cost: gungnir_model::RelativeCost::default(),
            magazine: None,
            intercept_speed_mps: Some(300.0),
        }
    }

    fn score(id: u64, value: f32) -> RiskScore {
        RiskScore {
            track_id: TrackId(id),
            score: value,
            time_to_impact_s: Some(60.0),
            exposure: None,
        }
    }

    /// The status table under which the point layer may engage.
    fn control(status: WeaponsControlStatus) -> ControlStatusSettings {
        let mut settings = ControlStatusSettings::default();
        settings.by_layer.insert(EffectorLayer::Point, status);
        settings
    }

    fn authority() -> AuthoritySettings {
        AuthoritySettings {
            rules: vec![AuthorityRule {
                action: DECIDE.to_owned(),
                role: "Operator".to_owned(),
                layer: Some(EffectorLayer::Point),
                class: None,
                pre_delegated: false,
            }],
        }
    }

    /// The real two-engine chain, built the way `gungnir-app` builds its four-engine one.
    ///
    /// The geofence engine is left out because it takes a `gungnir_geo::GeoService` and
    /// this crate has no edge to `gungnir-geo`; the desktop's own test covers the full
    /// chain. What matters here is that the verdict comes from `gungnir-policy` code
    /// rather than from this file.
    fn chain<'a>(
        control: &'a ControlStatusSettings,
        authority: &'a AuthoritySettings,
        classify: &'a (dyn Fn(TrackId) -> Classification + Send + Sync),
    ) -> PolicyChain<'a> {
        PolicyChain::new(vec![
            Box::new(ControlStatusPolicy {
                settings: control,
                track_classification: classify,
            }),
            Box::new(AuthorityPolicy {
                settings: authority,
                asking_role: "Operator",
                action: DECIDE,
                track_classification: classify,
            }),
        ])
    }

    fn hostile(_: TrackId) -> Classification {
        Classification::Hostile
    }

    /// **Every course of action carries a verdict the chain returned**, and the proof
    /// that it is the chain's rather than a constant is that changing the chain changes
    /// every one of them: at `Free` the point layer's plans need a person, at `Hold` the
    /// same plans against the same tracks are denied.
    #[test]
    fn every_alternative_carries_a_verdict_from_the_chain() {
        let tracks = vec![track(1), track(2), track(3)];
        let resources = vec![resource(1), resource(2), resource(3)];
        let scores = vec![score(1, 0.9), score(2, 0.6), score(3, 0.3)];
        let source = GreedySource::new(scores.clone());
        let authority = authority();

        for (status, expected) in [
            (
                WeaponsControlStatus::Free,
                PolicyVerdict::RequiresHumanApproval,
            ),
            (
                WeaponsControlStatus::Hold,
                PolicyVerdict::Denied {
                    reason_code: DenialReason::ControlStatus {
                        layer: EffectorLayer::Point,
                        status: WeaponsControlStatus::Hold,
                    },
                },
            ),
        ] {
            let control = control(status);
            let chain = chain(&control, &authority, &hostile);
            let mut support = PlanAlternatives {
                now: MissionTime(12.0),
                tracks: &tracks,
                resources: &resources,
                scores: &scores,
                source: &source,
                policy: &chain,
                assessor: None,
            };
            let courses = support.recommend(2);
            assert!(
                courses.len() > 1,
                "no alternatives were generated at {status:?}"
            );
            for course in &courses {
                assert_eq!(
                    course.policy_verdict, expected,
                    "a course of action carries a verdict the chain did not return, at \
                     {status:?}: {}",
                    course.rationale
                );
            }
        }
    }

    /// The primary comes first and the alternatives really are different plans.
    #[test]
    fn the_recommendation_leads_and_the_alternatives_differ_from_it() {
        let tracks = vec![track(1), track(2)];
        let resources = vec![resource(1), resource(2), resource(3)];
        let scores = vec![score(1, 0.9), score(2, 0.6)];
        let source = GreedySource::new(scores.clone());
        let control = control(WeaponsControlStatus::Free);
        let authority = authority();
        let chain = chain(&control, &authority, &hostile);
        let mut support = PlanAlternatives {
            now: MissionTime(0.0),
            tracks: &tracks,
            resources: &resources,
            scores: &scores,
            source: &source,
            policy: &chain,
            assessor: None,
        };
        let courses = support.recommend(4);
        assert!(courses[0].rationale.starts_with("Recommended"));
        let primary = courses[0].plan.assignments();
        for alternative in &courses[1..] {
            assert!(
                alternative.rationale.starts_with("Alternative"),
                "{}",
                alternative.rationale
            );
            assert_ne!(
                alternative.plan.assignments(),
                primary,
                "an alternative repeats the recommendation"
            );
        }
        let mut seen: Vec<Vec<(ResourceId, TrackId)>> =
            courses.iter().map(|c| c.plan.assignments()).collect();
        let before = seen.len();
        seen.sort();
        seen.dedup();
        assert_eq!(
            before,
            seen.len(),
            "two courses of action are the same plan"
        );
    }

    /// The caller's cap is honoured, so a large resource pool cannot stall the tick.
    #[test]
    fn the_search_is_bounded_by_the_callers_cap() {
        let tracks = vec![track(1), track(2), track(3)];
        let resources: Vec<ResourceView> = (1..=6).map(resource).collect();
        let scores = vec![score(1, 0.9), score(2, 0.6), score(3, 0.3)];
        let source = GreedySource::new(scores.clone());
        let control = control(WeaponsControlStatus::Free);
        let authority = authority();
        let chain = chain(&control, &authority, &hostile);
        let mut support = PlanAlternatives {
            now: MissionTime(0.0),
            tracks: &tracks,
            resources: &resources,
            scores: &scores,
            source: &source,
            policy: &chain,
            assessor: None,
        };
        assert!(
            support.recommend(1).len() <= 2,
            "one primary, one alternative"
        );
        assert!(
            support.recommend(0).len() == 1,
            "no alternatives were asked for"
        );
    }

    /// **The live state is unchanged after `what_if`**, which is the second half of the
    /// verification row.
    ///
    /// Snapshotted before and compared after: the live tracks, the live resource pool,
    /// the live scores, and -- the part a field-by-field comparison would miss -- the
    /// recommendation itself. If a what-if had advanced the allocator's own state, the
    /// same `recommend` call would come back different afterwards.
    #[test]
    fn what_if_leaves_the_live_state_exactly_as_it_was() {
        let tracks = vec![track(1), track(2)];
        let resources = vec![resource(1), resource(2)];
        let scores = vec![score(1, 0.9), score(2, 0.6)];
        let source = GreedySource::new(scores.clone());
        let control = control(WeaponsControlStatus::Free);
        let authority = authority();
        let chain = chain(&control, &authority, &hostile);
        let mut support = PlanAlternatives {
            now: MissionTime(7.0),
            tracks: &tracks,
            resources: &resources,
            scores: &scores,
            source: &source,
            policy: &chain,
            assessor: None,
        };

        let tracks_before = tracks.clone();
        let resources_before = resources.clone();
        let scores_before = scores.clone();
        let recommendation_before = support.recommend(3);

        // The hypothesis: the second track was never there.
        let hypothetical = vec![tracks[0].clone()];
        let what_if = support.what_if(&hypothetical);
        assert_eq!(
            what_if.plan.assignments().len(),
            1,
            "the hypothetical snapshot has one track, so one assignment: {}",
            what_if.rationale
        );
        assert!(
            what_if.rationale.contains("What if"),
            "a what-if that does not say so reads as a proposal: {}",
            what_if.rationale
        );
        assert!(
            source.calls.load(Ordering::SeqCst) > 0,
            "nothing was planned"
        );

        assert_eq!(tracks, tracks_before, "what_if changed the live tracks");
        assert_eq!(
            resources, resources_before,
            "what_if changed the live resource pool"
        );
        assert_eq!(scores, scores_before, "what_if changed the live scores");
        assert_eq!(
            support.recommend(3),
            recommendation_before,
            "the recommendation changed after a what-if, so something live moved"
        );
    }

    /// An allocator that cannot answer is reported as one, with a real verdict over the
    /// empty plan rather than a silently empty recommendation.
    #[test]
    fn an_allocator_that_declines_is_said_rather_than_shown_as_nothing_to_do() {
        let tracks = vec![track(1)];
        let resources = vec![resource(1)];
        let scores = vec![score(1, 0.9)];
        let mut source = GreedySource::new(scores.clone());
        source.refuse = Some("the allocator reported itself unimplemented".to_owned());
        let control = control(WeaponsControlStatus::Free);
        let authority = authority();
        let chain = chain(&control, &authority, &hostile);
        let mut support = PlanAlternatives {
            now: MissionTime(0.0),
            tracks: &tracks,
            resources: &resources,
            scores: &scores,
            source: &source,
            policy: &chain,
            assessor: None,
        };
        let courses = support.recommend(3);
        assert_eq!(courses.len(), 1);
        assert!(
            courses[0]
                .rationale
                .contains("the allocator reported itself unimplemented"),
            "{}",
            courses[0].rationale
        );
        assert_eq!(
            courses[0].policy_verdict,
            PolicyVerdict::Denied {
                reason_code: DenialReason::EmptyPlan
            },
            "the verdict over the empty plan is still the chain's"
        );
        let what_if = support.what_if(&tracks);
        assert!(
            what_if.rationale.contains("What if"),
            "{}",
            what_if.rationale
        );
    }

    /// A deployment that cannot score says so in the what-if rather than showing a
    /// column of zeroes that reads as "no track matters".
    #[test]
    fn a_what_if_without_an_assessor_says_the_risk_figures_are_absent() {
        let tracks = vec![track(1)];
        let resources = vec![resource(1)];
        let scores = vec![score(1, 0.9)];
        let source = GreedySource::new(scores.clone());
        let control = control(WeaponsControlStatus::Free);
        let authority = authority();
        let chain = chain(&control, &authority, &hostile);
        let support = PlanAlternatives {
            now: MissionTime(0.0),
            tracks: &tracks,
            resources: &resources,
            scores: &scores,
            source: &source,
            policy: &chain,
            assessor: None,
        };
        assert!(
            support
                .what_if(&tracks)
                .rationale
                .contains("absent rather than computed"),
            "an unscored what-if does not say so"
        );
    }
}
