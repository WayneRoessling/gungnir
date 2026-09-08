// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Wraps gungnir-allocation (Bellman/DP resource-to-track assignment) and
//! gungnir-coord behind one trait `gungnir-app` and `gungnir-node` depend on.
//! Internal dependency edges match agentic-coding-standards.md §1.1. The public
//! plan type is the canonical `gungnir_model::PlanView` (ARCHITECTURE.md §7.2), which
//! `gungnir-policy` and `gungnir-command` consume before anything can act on it.
//!
//! Reward values are the concern of `gungnir-assessment`; this crate accepts a
//! reward matrix through [`DpInterceptService::plan_with_rewards`] and falls back
//! to a uniform matrix in the trait method, which yields an assignment that favours
//! no track over another.

pub mod engagement;
pub mod geometry;

pub use engagement::{
    close_stale, EffectEvidence, EffectSource, EffectTally, Engagement, EngagementError,
    EngagementState, EngagementTransition,
};

use gungnir_allocation::AllocationError;
use nalgebra::DMatrix;

pub use gungnir_allocation::{AllocationPolicy, BellmanDpAllocator, ResourceAllocator};
pub use gungnir_model::{
    DecisionId, InterceptSolutionView, MissionTime, PlanId, PlanView, ResourceId, ResourceView,
    TrackId, TrackView,
};

/// A resource the planner would not propose, and why (DN-04 §5 rule 4, GAP-030).
///
/// **Shown, not silently skipped.** A resource missing from a recommendation with no reason
/// reads as a resource the planner forgot; one shown as withheld at its reserve is a
/// decision for a person, which is what rule 4 says it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithheldResource {
    pub resource: gungnir_model::ResourceId,
    pub reason: WithheldReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WithheldReason {
    /// Not ready, which the planner always respected.
    NotReady,
    /// At or below its reserve: rounds remain, and eating them is somebody's decision.
    AtReserve,
}

impl WithheldReason {
    #[must_use]
    pub fn sentence(self) -> &'static str {
        match self {
            WithheldReason::NotReady => "not ready",
            WithheldReason::AtReserve => "at its reserve; releasing it is a decision",
        }
    }
}

/// What came back from a planning call, and how much to believe it.
///
/// **`plan` used to return a bare `PlanView`** (GAP-066). `DpInterceptService`'s documented
/// behaviour is to keep the last good plan and return it when a solve fails -- so a stale
/// plan came back looking exactly like a fresh one, and the only clue was `is_healthy()`
/// somewhere else on the screen. A planner acting on a recommendation has to be able to
/// tell "this is for the picture in front of you" from "this is what we last managed to
/// compute, at 13:04".
#[derive(Debug, Clone, PartialEq)]
pub enum PlanOutcome {
    /// Computed for the snapshot that was passed in.
    Fresh(PlanView),
    /// The solve failed. This is the last plan that succeeded, and when it did.
    Stale {
        plan: PlanView,
        computed_at: MissionTime,
        reason: String,
    },
    /// No plan has ever been computed successfully, so there is nothing to show.
    ///
    /// Distinct from a fresh plan that proposes nothing: **one says the sector needs no
    /// action and the other says nobody can tell.**
    NoPlan { reason: String },
}

impl PlanOutcome {
    /// The plan to draw, when there is one.
    #[must_use]
    pub fn plan(&self) -> Option<&PlanView> {
        match self {
            PlanOutcome::Fresh(plan) | PlanOutcome::Stale { plan, .. } => Some(plan),
            PlanOutcome::NoPlan { .. } => None,
        }
    }

    /// True when this is an answer to the question that was asked, rather than an older
    /// answer to an older one.
    #[must_use]
    pub fn is_fresh(&self) -> bool {
        matches!(self, PlanOutcome::Fresh(_))
    }
}

pub trait InterceptService: Send + Sync {
    /// Non-blocking: recompute the assignment for the latest track snapshot and
    /// resource pool. Never blocks the caller (per UI standards §5 -- long-running
    /// work must stay off the render thread; if the DP solve is ever too slow for a
    /// frame budget, this implementation is expected to move it to a background
    /// thread and return the last-good plan here rather than block).
    fn plan(
        &mut self,
        now: MissionTime,
        tracks: &[TrackView],
        resources: &[ResourceView],
    ) -> PlanOutcome;

    /// False once a solve has failed (including "not implemented"), so the health
    /// panel shows the plan on screen may be stale.
    fn is_healthy(&self) -> bool;

    /// Resources the last planning call declined to propose, with the reason each
    /// (DN-04 §5 rule 4, GAP-030).
    ///
    /// Defaults to none. That is the truthful answer for a planner that did not make the
    /// decision here -- a remote service relays the node's plan and does not know what the
    /// node held back -- and the embedded planner overrides it.
    fn withheld(&self) -> Vec<WithheldResource> {
        Vec::new()
    }
}

/// Bellman/DP-backed planner. Keeps the last good plan and returns it when a solve
/// fails, flagging `is_healthy() == false`.
pub struct DpInterceptService {
    allocator: BellmanDpAllocator,
    horizon: usize,
    last_plan: PlanView,
    next_plan_id: u64,
    solver_ok: bool,
    warned: bool,
    /// When `last_plan` was computed, and why it is being kept if a solve has failed
    /// since (GAP-066). `None` before any solve has succeeded.
    last_solved: Option<MissionTime>,
    last_failure: Option<String>,
    /// The local ENU frame, so a resource's geodetic position can meet a track's ENU
    /// state and the intercept point can be reported geodetic (GAP-031). `None` when the
    /// deployment has declared no origin: the pairing is still produced and the point is
    /// not, which `geometry_unavailable` says.
    local_frame: Option<gungnir_model::LocalFrame>,
    /// Resources the last planning call declined to propose (GAP-030).
    withheld: Vec<WithheldResource>,
}

impl DpInterceptService {
    pub fn new(horizon: usize) -> Self {
        Self {
            allocator: BellmanDpAllocator,
            horizon: horizon.max(1),
            last_plan: PlanView::default(),
            next_plan_id: 1,
            solver_ok: true,
            warned: false,
            last_solved: None,
            last_failure: None,
            local_frame: None,
            withheld: Vec::new(),
        }
    }

    /// Resources the last planning call would not propose, with the reason each.
    #[must_use]
    pub fn withheld(&self) -> &[WithheldResource] {
        &self.withheld
    }

    pub fn horizon(&self) -> usize {
        self.horizon
    }

    pub fn last_plan(&self) -> &PlanView {
        &self.last_plan
    }

    /// Label a plan with how much of an answer it is (GAP-066).
    ///
    /// The distinction the caller needs: a plan computed for the snapshot they passed in,
    /// the last one that succeeded because this solve did not, or nothing at all because
    /// none ever has. **The third is not an empty plan** -- an empty plan says the sector
    /// needs no action, and this says nobody can tell.
    fn outcome(&self, plan: PlanView) -> PlanOutcome {
        if self.solver_ok {
            return PlanOutcome::Fresh(plan);
        }
        let reason = self
            .last_failure
            .clone()
            .unwrap_or_else(|| "the solve failed".to_owned());
        match self.last_solved {
            Some(computed_at) => PlanOutcome::Stale {
                plan,
                computed_at,
                reason,
            },
            None => PlanOutcome::NoPlan { reason },
        }
    }

    /// The local frame the geometry is solved in (GAP-031). Without one the planner
    /// still pairs resources with tracks and reports no point.
    #[must_use]
    pub fn with_local_frame(mut self, frame: Option<gungnir_model::LocalFrame>) -> Self {
        self.local_frame = frame;
        self
    }

    /// Why the last plan's solutions carry no geometry, when they do not.
    #[must_use]
    pub fn geometry_unavailable(&self) -> Option<&'static str> {
        self.local_frame
            .is_none()
            .then_some("no local frame origin is declared, so no intercept point can be placed")
    }

    /// The assignment as solutions, each with the earliest constant-velocity intercept
    /// where one exists (GAP-031, DN-04 §9). A pairing whose resource has no closing
    /// speed, or whose track outruns it, is still a pairing: the point and the time are
    /// `None`, never invented.
    /// `assignment` holds `(row, column)` **indices into the reward matrix**, and mapping
    /// them back to identifiers is this function's job because it is the only place that
    /// knows the ordering the matrix was built with: one row per **adequate** resource in
    /// `resources` order -- which is why `adequate` is passed rather than the full list --
    /// and one column per track.
    ///
    /// **An index outside those slices is dropped, never turned into an identifier.**
    /// That is a broken allocator, and minting a `TrackId` from a column number would put
    /// a plan in front of an operator naming a track that may not exist. That is exactly
    /// what the previous version did: it received the indices already wrapped as
    /// identifiers and looked the track up by identity, so wherever the two did not
    /// coincide the pairing named the wrong things and the geometry silently became
    /// `None` as though no intercept existed.
    fn solutions_with_geometry(
        frame: Option<&gungnir_model::LocalFrame>,
        assignment: &[(usize, usize)],
        tracks: &[TrackView],
        adequate: &[&ResourceView],
    ) -> Vec<InterceptSolutionView> {
        assignment
            .iter()
            .filter_map(|(row, column)| {
                let r = *adequate.get(*row)?;
                let t = tracks.get(*column)?;
                let geometry = frame.and_then(|frame| {
                    geometry::earliest_intercept(
                        t.position_enu(),
                        [t.state[3], t.state[4], t.state[5]],
                        frame.to_enu(r.position),
                        r.intercept_speed_mps,
                    )
                    .ok()
                });
                Some(InterceptSolutionView {
                    resource: r.id,
                    track: t.id,
                    intercept_point: match (geometry, frame) {
                        (Some(g), Some(f)) => Some(f.to_geodetic(g.point_enu)),
                        _ => None,
                    },
                    time_to_intercept_s: geometry.map(|g| g.time_s),
                })
            })
            .collect()
    }

    /// Plan against an explicit reward matrix (resources x tracks), as supplied by
    /// `gungnir-assessment`. Only **adequate** resources are tasked -- ready, and above
    /// their reserve where they carry a magazine (DN-04 §5, GAP-030) -- and the matrix
    /// must have one row per *adequate* resource in `resources` order and one column per
    /// track. A resource that is ready but at its reserve used to be proposed like any
    /// other; it is now withheld and named.
    pub fn plan_with_rewards(
        &mut self,
        now: MissionTime,
        tracks: &[TrackView],
        resources: &[ResourceView],
        rewards: &DMatrix<f64>,
    ) -> PlanView {
        self.withheld = resources
            .iter()
            .filter(|r| !r.is_adequate())
            .map(|r| WithheldResource {
                resource: r.id,
                reason: if r.ready {
                    WithheldReason::AtReserve
                } else {
                    WithheldReason::NotReady
                },
            })
            .collect();
        let ready: Vec<&ResourceView> = resources.iter().filter(|r| r.is_adequate()).collect();
        if tracks.is_empty() || ready.is_empty() {
            if !self.last_plan.is_empty() {
                self.last_plan = self.fresh_plan(now, Vec::new(), 0.0);
            }
            // **Nothing to solve is a fresh answer, not an absent one** (GAP-066): with no
            // tracks or no ready resource the empty plan is correct for this snapshot, and
            // the mark is set so a later failure can report how old the last real answer
            // is rather than reporting that there has never been one.
            self.last_solved = Some(now);
            self.last_failure = None;
            return self.last_plan.clone();
        }
        match self.allocator.solve(rewards, self.horizon) {
            Ok(policy) => {
                self.solver_ok = true;
                self.last_solved = Some(now);
                self.last_failure = None;
                let solutions = Self::solutions_with_geometry(
                    self.local_frame.as_ref(),
                    &policy.assignment,
                    tracks,
                    // The adequate list, in the order the matrix rows were built from.
                    &ready,
                );
                // **GAP-097.** A solve that confirms the same resource/track pairs
                // already in `self.last_plan` is not a new recommendation, and must
                // not become one: `update::tick`'s "publish only when the plan
                // changes" gate (GAP-066) compares the whole `PlanView`, so minting a
                // fresh id and `mission_time` here every tick made that gate never
                // hold once a solve succeeded, flooding the approval queue with the
                // same pairing at the tick rate. Only a genuinely different
                // assignment gets a new id and timestamp; an unchanged one keeps the
                // plan -- geometry included -- exactly as it was.
                if Self::assignment_changed(&self.last_plan, &solutions) {
                    self.last_plan = self.fresh_plan(now, solutions, policy.value);
                }
            }
            Err(AllocationError::NotImplemented) => {
                self.solver_ok = false;
                self.last_failure = Some("the allocator reported itself unimplemented".to_owned());
                if !self.warned {
                    self.warned = true;
                    tracing::warn!(
                        "Bellman/DP allocator not implemented; intercept plan is empty/stale"
                    );
                }
            }
            Err(err) => {
                self.solver_ok = false;
                self.last_failure = Some(err.to_string());
                tracing::error!(%err, "allocation solve failed; keeping last good plan");
            }
        }
        self.last_plan.clone()
    }

    fn fresh_plan(
        &mut self,
        now: MissionTime,
        solutions: Vec<InterceptSolutionView>,
        value: f64,
    ) -> PlanView {
        let id = PlanId(self.next_plan_id);
        self.next_plan_id += 1;
        PlanView {
            id,
            mission_time: now,
            kind: gungnir_model::PlanKind::Intercept { solutions },
            policy_value: value,
            releasability: gungnir_model::Releasability::default(),
        }
    }

    /// Whether `solutions` names a different set of resource/track pairings than
    /// `plan` already holds (GAP-097).
    ///
    /// Compared as a **set**, not the ordered `Vec` `solutions_with_geometry`
    /// returns, since two solves of the identical assignment are not guaranteed to
    /// enumerate the pairs in the same order. Compared by **the pairing alone**, not
    /// the geometry riding along with it: a moving track's intercept point and
    /// time-to-intercept legitimately change every tick even when the resource stays
    /// tasked to the same track, and comparing the full `InterceptSolutionView`
    /// would defeat the fix by minting a new plan for that reason alone.
    fn assignment_changed(plan: &PlanView, solutions: &[InterceptSolutionView]) -> bool {
        let pairs = |solutions: &[InterceptSolutionView]| -> std::collections::HashSet<(ResourceId, TrackId)> {
            solutions.iter().map(|s| (s.resource, s.track)).collect()
        };
        let existing = match &plan.kind {
            gungnir_model::PlanKind::Intercept { solutions } => pairs(solutions),
            // This service never constructs a `Fires` plan, so `last_plan` is never
            // one; treated as "no prior pairing" rather than assumed unreachable,
            // since a plan a caller substituted in some other way is still data,
            // not a broken invariant to panic over.
            gungnir_model::PlanKind::Fires(_) => std::collections::HashSet::new(),
        };
        existing != pairs(solutions)
    }
}

impl InterceptService for DpInterceptService {
    fn plan(
        &mut self,
        now: MissionTime,
        tracks: &[TrackView],
        resources: &[ResourceView],
    ) -> PlanOutcome {
        let ready = resources.iter().filter(|r| r.is_adequate()).count();
        let rewards = DMatrix::from_element(ready, tracks.len(), 1.0);
        let plan = self.plan_with_rewards(now, tracks, resources, &rewards);
        self.outcome(plan)
    }

    fn is_healthy(&self) -> bool {
        self.solver_ok
    }

    fn withheld(&self) -> Vec<WithheldResource> {
        self.withheld.clone()
    }
}

#[cfg(test)]
mod geometry_wiring_tests {
    use super::*;
    use gungnir_model::{Classification, Geodetic, LocalFrame, Provenance, Quality, TrackStatus};

    fn track(id: u64, e: f64, ve: f64) -> TrackView {
        TrackView {
            id: TrackId(id),
            status: TrackStatus::Confirmed,
            state: nalgebra::SVector::<f64, 6>::new(e, 0.0, 0.0, ve, 0.0, 0.0),
            covariance: nalgebra::SMatrix::<f64, 6, 6>::identity(),
            classification: Classification::Unknown,
            provenance: Provenance::default(),
            quality: Quality::default(),
            mission_time: MissionTime(0.0),
            releasability: gungnir_model::Releasability::default(),
        }
    }

    fn resource(id: u32, speed: Option<f64>, origin: Geodetic) -> ResourceView {
        ResourceView {
            id: ResourceId(id),
            position: origin,
            capacity: 1,
            ready: true,
            layer: gungnir_model::EffectorLayer::Point,
            cost: gungnir_model::RelativeCost::default(),
            magazine: None,
            intercept_speed_mps: speed,
        }
    }

    /// The test the previous version of this code could not fail.
    ///
    /// Every fixture in this workspace numbered its tracks and resources from zero or
    /// one, so identifiers and matrix indices coincided and a function that confused the
    /// two produced the right answer. Here they deliberately do not: the tracks are 70 and
    /// 71 and the resources 40 and 41, so a pairing resolved by identity against a matrix
    /// index would find nothing, drop the geometry, and name a track that does not exist.
    #[test]
    fn a_pairing_is_resolved_by_position_and_not_by_identifier() {
        let origin = Geodetic {
            lat_rad: 0.96,
            lon_rad: 0.21,
            alt_m: 0.0,
        };
        let frame = LocalFrame::new(origin);
        let tracks = vec![track(70, 1000.0, -50.0), track(71, 2000.0, -50.0)];
        let resources = [
            resource(40, Some(150.0), origin),
            resource(41, Some(150.0), origin),
        ];
        let adequate: Vec<&ResourceView> = resources.iter().collect();

        // Row 1, column 0: the second resource against the first track.
        let solutions = DpInterceptService::solutions_with_geometry(
            Some(&frame),
            &[(1, 0)],
            &tracks,
            &adequate,
        );
        assert_eq!(solutions.len(), 1);
        assert_eq!(
            solutions[0].resource,
            ResourceId(41),
            "the pairing must name the resource in that ROW, not the one whose identifier \
             equals the row number"
        );
        assert_eq!(solutions[0].track, TrackId(70));
        assert!(
            solutions[0].intercept_point.is_some(),
            "the geometry vanished, which is what an identity lookup against an index does"
        );

        // An index past the end is a broken allocator: dropped, never invented.
        let out_of_range = DpInterceptService::solutions_with_geometry(
            Some(&frame),
            &[(9, 0)],
            &tracks,
            &adequate,
        );
        assert!(
            out_of_range.is_empty(),
            "an out-of-range row produced a solution naming something"
        );
    }

    #[test]
    fn an_assignment_gains_a_point_and_a_time_only_where_geometry_exists() {
        let origin = Geodetic {
            lat_rad: 0.96,
            lon_rad: 0.21,
            alt_m: 0.0,
        };
        let frame = LocalFrame::new(origin);
        let tracks = vec![track(1, 1000.0, -50.0), track(2, 1000.0, -50.0)];
        let resources = [resource(1, Some(150.0), origin), resource(2, None, origin)];
        let adequate: Vec<&ResourceView> = resources.iter().collect();
        let solutions = DpInterceptService::solutions_with_geometry(
            Some(&frame),
            &[(0, 0), (1, 1)],
            &tracks,
            &adequate,
        );
        let with = &solutions[0];
        assert!((with.time_to_intercept_s.expect("solved") - 5.0).abs() < 1e-9);
        let point = with.intercept_point.expect("placed");
        let enu = frame.to_enu(point);
        assert!((enu[0] - 750.0).abs() < 1e-3, "{enu:?}");
        let without = &solutions[1];
        assert_eq!(
            without.time_to_intercept_s, None,
            "no closing speed, no time"
        );
        assert_eq!(without.intercept_point, None);
        // No frame: pairings survive, geometry does not.
        let unplaced =
            DpInterceptService::solutions_with_geometry(None, &[(0, 0)], &tracks, &adequate);
        assert_eq!(unplaced.len(), 1);
        assert_eq!(unplaced[0].intercept_point, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::{Classification, Geodetic, Provenance, Quality, TrackStatus};

    fn track(id: u64) -> TrackView {
        TrackView {
            id: TrackId(id),
            status: TrackStatus::Confirmed,
            state: nalgebra::SVector::<f64, 6>::zeros(),
            covariance: nalgebra::SMatrix::<f64, 6, 6>::identity(),
            classification: Classification::Unknown,
            provenance: Provenance::default(),
            quality: Quality::default(),
            mission_time: MissionTime(0.0),
            releasability: gungnir_model::Releasability::default(),
        }
    }

    fn resource(id: u32, ready: bool) -> ResourceView {
        ResourceView {
            id: ResourceId(id),
            position: Geodetic {
                lat_rad: 0.0,
                lon_rad: 0.0,
                alt_m: 0.0,
            },
            capacity: 1,
            ready,
            layer: gungnir_model::EffectorLayer::Point,
            cost: gungnir_model::RelativeCost::default(),
            magazine: None,
            intercept_speed_mps: None,
        }
    }

    #[test]
    fn no_tracks_yields_a_fresh_empty_plan_and_stays_healthy() {
        let mut svc = DpInterceptService::new(10);
        let outcome = svc.plan(MissionTime(1.0), &[], &[resource(1, true)]);
        // **Fresh and empty**: the sector needs no action, which is an answer.
        assert!(outcome.is_fresh(), "{outcome:?}");
        assert!(outcome.plan().expect("a plan").is_empty());
        assert!(svc.is_healthy());
    }

    /// **The distinction GAP-066 added.** Before this, a failed solve returned the last
    /// good plan as a bare `PlanView` and the caller could not tell it from a fresh one.
    /// Here no solve has ever succeeded, so there is no plan at all -- which is not the
    /// same as a plan proposing nothing.
    ///
    /// The failure used to be the allocator being unimplemented; it solves as of
    /// 2026-09-06 (GAP-029), so the failure is now a reward matrix with no defined
    /// optimum, which is a real thing an assessment can hand this service.
    #[test]
    fn a_failed_solve_reports_no_plan_rather_than_an_empty_one() {
        let mut svc = DpInterceptService::new(10);
        let mut rewards = DMatrix::from_element(1, 1, 1.0);
        rewards[(0, 0)] = f64::NAN;
        let plan = svc.plan_with_rewards(
            MissionTime(1.0),
            &[track(1)],
            &[resource(1, true)],
            &rewards,
        );
        let outcome = svc.outcome(plan);
        match &outcome {
            PlanOutcome::NoPlan { reason } => {
                assert!(reason.contains("finite"), "{reason}");
            }
            other => panic!("a solve that never succeeded produced {other:?}"),
        }
        assert!(outcome.plan().is_none(), "there is nothing honest to draw");
        assert!(!svc.is_healthy());
    }

    /// Once a solve has succeeded, a later failure returns that plan **labelled stale and
    /// stamped with when it was computed**, which is what a planner needs to judge it.
    #[test]
    fn a_failure_after_a_success_returns_the_last_plan_marked_stale() {
        let mut svc = DpInterceptService::new(10);
        // A solve with no tracks succeeds trivially and sets the mark.
        assert!(svc
            .plan(MissionTime(1.0), &[], &[resource(1, true)])
            .is_fresh());

        let mut rewards = DMatrix::from_element(1, 1, 1.0);
        rewards[(0, 0)] = f64::NAN;
        let plan = svc.plan_with_rewards(
            MissionTime(2.0),
            &[track(1)],
            &[resource(1, true)],
            &rewards,
        );
        let outcome = svc.outcome(plan);
        match outcome {
            PlanOutcome::Stale {
                computed_at,
                reason,
                ..
            } => {
                assert_eq!(computed_at, MissionTime(1.0));
                assert!(reason.contains("finite"), "{reason}");
            }
            other => panic!("a stale plan was returned as {other:?}"),
        }
    }

    /// **DN-04 §5 rule 4, the one that must not be softened.** A resource at its reserve
    /// is not proposed, and it is named as withheld rather than silently left out.
    #[test]
    fn a_resource_at_its_reserve_is_withheld_and_named() {
        let mut svc = DpInterceptService::new(10);
        let mut at_reserve = resource(2, true);
        at_reserve.magazine = Some(gungnir_model::Magazine {
            rounds_available: 4,
            reserve: 4,
        });
        let outcome = svc.plan(
            MissionTime(1.0),
            &[track(1)],
            &[resource(1, true), at_reserve],
        );
        // Nothing solves today (GAP-011), so the plan is `NoPlan`; the withholding is
        // decided before the solve and is real regardless.
        let _ = outcome;
        let withheld = svc.withheld();
        assert_eq!(withheld.len(), 1, "{withheld:?}");
        assert_eq!(withheld[0].resource, gungnir_model::ResourceId(2));
        assert_eq!(withheld[0].reason, WithheldReason::AtReserve);
    }

    #[test]
    fn unready_resources_are_never_tasked() {
        let mut svc = DpInterceptService::new(10);
        let outcome = svc.plan(MissionTime(1.0), &[track(1)], &[resource(1, false)]);
        assert!(outcome.plan().expect("a plan").is_empty());
        assert!(svc.is_healthy(), "nothing was attempted, so nothing failed");
    }

    /// **GAP-097's own closing action**: an unmoving track and a ready resource,
    /// solved over many ticks, must keep the same plan -- same id, same
    /// `mission_time` -- throughout. Before the fix, every successful solve minted a
    /// fresh id regardless of whether the pairing changed, which is what flooded the
    /// approval queue with duplicates of the same recommendation at the tick rate.
    #[test]
    fn an_unchanged_assignment_keeps_the_same_plan_over_many_ticks() {
        let mut svc = DpInterceptService::new(10);
        let tracks = [track(1)];
        let resources = [resource(1, true)];

        let outcome = svc.plan(MissionTime(1.0), &tracks, &resources);
        assert!(outcome.is_fresh(), "{outcome:?}");
        let first = outcome.plan().expect("a plan").clone();
        assert!(
            !first.is_empty(),
            "one ready resource and one track should pair into a real plan"
        );

        for tick in 2..50_u32 {
            let now = MissionTime(f64::from(tick));
            let outcome = svc.plan(now, &tracks, &resources);
            assert!(outcome.is_fresh(), "tick {tick}: {outcome:?}");
            let plan = outcome.plan().expect("a plan");
            assert_eq!(
                *plan, first,
                "tick {tick}: an unchanged resource/track pairing must not mint a \
                 new plan"
            );
        }
    }
}
