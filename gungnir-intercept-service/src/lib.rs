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
//!
//! # Determinism
//!
//! The same tracks, resources and rewards give the same assignment on any planner, in
//! any run (GAP-119). Rows of the reward matrix are the adequate resources in the order
//! they were passed and columns are the tracks in the order they were passed, and a tie
//! between equally good assignments is decided by `gungnir_allocation::bellman`'s
//! documented rule on those positions. The one thing two planners given the same input
//! do not share is the plan's identifier: a `PlanId` is a UUID v7 minted per plan
//! (D-56), so that no two machines ever name two recommendations alike.
//!
//! # The solve budget
//!
//! Every planning call spends at most its budget solving ([`budget`], D-81; DN-04 §10).
//! A solve that is not finished when the budget is spent is kept, not thrown away, and
//! the next call carries on with it; meanwhile the planner answers
//! [`PlanOutcome::Stale`] with the last plan it did compute, when it computed it, and how
//! far the current solve has got, and `is_healthy()` is false. The first call whose solve
//! finishes answers fresh and healthy again. A picture that has not changed since the
//! last finished solve is not solved again: the answer to the same problem is the same
//! answer, so it is fresh without spending anything.
//!
//! # A picture the exact solve cannot finish in time
//!
//! A raid toward the exact solver's size limits takes seconds of solving, and at 4 ms a
//! call far longer; past the limits the solver refuses it. Once the planner has been
//! behind the picture for longer than its stand-in wait -- MOP-07's 500 ms by default,
//! measured in mission time -- or at once for a picture the exact solver will not take,
//! it answers the current picture with `gungnir_allocation::stand_in`: the best assignment
//! for this step alone, with a floor on what it reaches over the horizon and a ceiling on
//! the optimum ([`PlanOutcome::Interim`]; GAP-156, D-93; DN-04 §11). The plan carries
//! `gungnir_model::PlanBasis::OneStep` wherever it goes, the planner stays unhealthy, and
//! the exact solve carries on underneath. When it finishes, a different assignment is a
//! new plan; the same assignment keeps the plan in force (GAP-097: one pairing, one
//! plan), and a stand-in that agrees with the plan in force keeps it too.

pub mod budget;
pub mod engagement;
pub mod geometry;

pub use budget::{
    MonotonicClock, SolveClock, SteppedClock, DEFAULT_SOLVE_BUDGET, DEFAULT_STAND_IN_AFTER,
};

pub use engagement::{
    close_stale, EffectEvidence, EffectSource, EffectTally, Engagement, EngagementError,
    EngagementState, EngagementTransition,
};

use gungnir_allocation::{ExactSolve, Progress, StandIn, MAX_RESOURCES, MAX_TRACKS};
use nalgebra::DMatrix;
use std::sync::Arc;
use std::time::Duration;

pub use gungnir_allocation::{AllocationPolicy, BellmanDpAllocator, ResourceAllocator};
pub use gungnir_model::{
    DecisionId, InterceptSolutionView, MissionTime, PlanBasis, PlanId, PlanStandingView, PlanView,
    ResourceId, ResourceView, TrackId, TrackView,
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
    /// Computed for the snapshot that was passed in: the planner's optimum.
    Fresh(PlanView),
    /// Computed for the snapshot that was passed in, **but not the optimum** (GAP-156,
    /// D-93): the exact solve could not answer this picture in time, so this is the best
    /// assignment for this step alone, standing in until it does. The plan carries
    /// [`PlanBasis::OneStep`]; `bound` says how much of the optimum's value it is known
    /// to reach; `reason` says why the exact answer is not here.
    Interim {
        plan: PlanView,
        bound: InterimBound,
        reason: String,
        /// How far the exact solve has got, while one is under way.
        progress: Option<SolveProgress>,
    },
    /// The solve failed, or has not finished inside its budget (GAP-119). This is the
    /// last plan that succeeded, when it did, and why this call's did not.
    Stale {
        plan: PlanView,
        computed_at: MissionTime,
        reason: String,
        /// How far the solve for the current picture has got, while one is under way.
        progress: Option<SolveProgress>,
    },
    /// No plan has ever been computed successfully, so there is nothing to show.
    ///
    /// Distinct from a fresh plan that proposes nothing: **one says the sector needs no
    /// action and the other says nobody can tell.**
    NoPlan {
        reason: String,
        /// How far the solve for the current picture has got, while one is under way.
        progress: Option<SolveProgress>,
    },
}

/// How good an interim answer is known to be (GAP-156): `gungnir_allocation::StandIn`'s
/// bounds, which a plan's own `policy_value` cannot carry both of.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InterimBound {
    /// What the plan reaches over the horizon when each later step is also a one-step
    /// answer: achievable, so a floor.
    pub value_at_least: f64,
    /// A ceiling on the optimum over the horizon.
    pub optimum_at_most: f64,
}

impl InterimBound {
    /// The share of the best plan's value this answer is known to reach, 0 to 1: the
    /// floor over the ceiling, and 1 where nothing is worth doing.
    #[must_use]
    pub fn share_of_optimum(&self) -> f64 {
        gungnir_allocation::one_step::share_of_optimum(self.value_at_least, self.optimum_at_most)
    }

    /// The bound in words, as PN-05 and PN-07 print it: "worth at least 87% of the best
    /// plan's value". Whole percent, rounded down, so the claim is never more than the
    /// numbers support.
    #[must_use]
    pub fn sentence(&self) -> String {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let percent = (self.share_of_optimum() * 100.0).floor() as u32;
        format!("worth at least {percent}% of the best plan's value")
    }
}

/// How far a solve that has not finished has got (GAP-119).
///
/// **Beside the reason, not inside it,** since GAP-157: the reason says why the plan is
/// not current and does not change while the picture does not, so a node can publish it
/// when it changes; this changes every call, and is for the console that owns the planner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SolveProgress {
    /// Whole percent of the value function filled, rounded down, so a solve never reads
    /// as finished before it is.
    pub percent: u64,
    /// Planning calls that have worked on it.
    pub calls: u32,
}

impl SolveProgress {
    /// "it is 37% done after 12 planning call(s)".
    #[must_use]
    pub fn sentence(&self) -> String {
        format!(
            "it is {}% done after {} planning call(s)",
            self.percent, self.calls
        )
    }
}

impl PlanOutcome {
    /// The plan to draw, when there is one.
    #[must_use]
    pub fn plan(&self) -> Option<&PlanView> {
        match self {
            PlanOutcome::Fresh(plan)
            | PlanOutcome::Interim { plan, .. }
            | PlanOutcome::Stale { plan, .. } => Some(plan),
            PlanOutcome::NoPlan { .. } => None,
        }
    }

    /// True when this is the planner's optimum for the question that was asked, rather
    /// than an older answer to an older one or a stand-in for it.
    #[must_use]
    pub fn is_fresh(&self) -> bool {
        matches!(self, PlanOutcome::Fresh(_))
    }

    /// True when the plan answers the picture that was passed in: the optimum, or an
    /// interim answer labelled as not the optimum (GAP-156). What decides whether a plan
    /// is proposed: an interim answer is a recommendation for the picture in front of the
    /// operator, and is put in front of them with its label; a stale one is not.
    #[must_use]
    pub fn answers_the_picture(&self) -> bool {
        matches!(self, PlanOutcome::Fresh(_) | PlanOutcome::Interim { .. })
    }

    /// Why this is not the optimum for the picture, with how far the solve has got where
    /// one is under way; `None` for a fresh answer.
    #[must_use]
    pub fn reason_in_full(&self) -> Option<String> {
        let (reason, progress) = match self {
            PlanOutcome::Fresh(_) => return None,
            PlanOutcome::Interim {
                reason, progress, ..
            }
            | PlanOutcome::Stale {
                reason, progress, ..
            }
            | PlanOutcome::NoPlan { reason, progress } => (reason, progress),
        };
        Some(match progress {
            Some(p) => format!("{reason}; {}", p.sentence()),
            None => reason.clone(),
        })
    }

    /// This answer's standing as a node puts it on the wire (GAP-157, D-94): everything a
    /// linked desktop needs to draw what an embedded one draws, except the plan -- which
    /// travels on its own -- and the progress, which changes every call.
    #[must_use]
    pub fn standing(&self) -> PlanStandingView {
        match self {
            PlanOutcome::Fresh(_) => PlanStandingView::Current,
            PlanOutcome::Interim { bound, reason, .. } => PlanStandingView::Interim {
                value_at_least: bound.value_at_least,
                optimum_at_most: bound.optimum_at_most,
                reason: reason.clone(),
            },
            PlanOutcome::Stale {
                computed_at,
                reason,
                ..
            } => PlanStandingView::Stale {
                computed_at: *computed_at,
                reason: reason.clone(),
            },
            PlanOutcome::NoPlan { reason, .. } => PlanStandingView::NoPlan {
                reason: reason.clone(),
            },
        }
    }
}

pub trait InterceptService: Send + Sync {
    /// Non-blocking: recompute the assignment for the latest track snapshot and
    /// resource pool. Never holds the caller past its budget (per UI standards §5 --
    /// long-running work must stay off the render thread): the embedded planner spends
    /// at most its solve budget per call, carries an unfinished solve over to the next,
    /// and returns the last good plan, stale, until it finishes (GAP-119, MOP-06).
    fn plan(
        &mut self,
        now: MissionTime,
        tracks: &[TrackView],
        resources: &[ResourceView],
    ) -> PlanOutcome;

    /// False while the plan on screen is not an answer for the current picture -- a solve
    /// failed, or has not finished inside its budget -- and true again from the first
    /// call that answers fresh.
    /// False while the planner is answering with an interim plan too (GAP-156): the plan
    /// answers the picture, but it is not the answer the planner exists to give.
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

/// The assignment problem a solve answers: which tracks, which adequate resources, in
/// which order, and what each pairing is worth (GAP-119).
///
/// **What makes two pictures the same question.** The allocator sees only the reward
/// matrix, and its rows and columns stand for these resources and tracks in this order;
/// a track's position is not part of it (the geometry is worked out afterwards, from the
/// picture at hand). So a solve finished for one call answers any later call with an
/// equal problem exactly, and a solve begun for one call answers nothing once the
/// problem has changed.
#[derive(Debug, Clone, PartialEq)]
struct Problem {
    tracks: Vec<TrackId>,
    resources: Vec<ResourceId>,
    rewards: DMatrix<f64>,
}

/// Why the exact solve will not answer a problem at all.
enum Unsolvable {
    /// Past `gungnir_allocation::MAX_TRACKS` or `MAX_RESOURCES`: the exact solver refuses
    /// it by design, so a stand-in answers it at once rather than after a wait for an
    /// answer that will never come (GAP-156).
    TooLarge { tracks: usize, resources: usize },
    /// Anything else the solver refuses: a reward that is not finite.
    Refused(gungnir_allocation::AllocationError),
}

/// A solve that has not finished, carried from one planning call to the next.
struct InFlight {
    problem: Problem,
    solve: ExactSolve,
    /// Planning calls that have worked on it.
    slices: u32,
}

/// Bellman/DP-backed planner. Keeps the last good plan and returns it when a solve
/// fails or has not finished inside its budget, flagging `is_healthy() == false`.
pub struct DpInterceptService {
    horizon: usize,
    /// How long one planning call may spend solving (GAP-119, D-81).
    solve_budget: Duration,
    /// What the budget is measured on. Shared so a test can hold the handle it steps.
    clock: Arc<dyn SolveClock>,
    /// The solve under way, if one is (GAP-119).
    in_flight: Option<InFlight>,
    /// The problem the last finished solve answered, and its answer, so an unchanged
    /// picture is answered without solving it again (GAP-119).
    solved: Option<(Problem, gungnir_allocation::AllocationPolicy)>,
    last_plan: PlanView,
    solver_ok: bool,
    /// When `last_plan` was computed, and why it is being kept if a solve has failed
    /// since (GAP-066). `None` before any solve has succeeded.
    last_solved: Option<MissionTime>,
    /// Why the plan in force is not the planner's optimum for the current picture. Stable
    /// while the picture is: how far a solve has got is `progress` (GAP-157).
    last_failure: Option<String>,
    /// How far the solve for the current picture has got, while one is under way.
    progress: Option<SolveProgress>,
    /// How long, in mission time, the planner may be behind the picture before a
    /// one-step answer stands in (GAP-156, D-93).
    stand_in_after: Duration,
    /// When the planner fell behind the picture: the first call since its last fresh
    /// answer that it could not answer. Measured from here, not from when the current
    /// solve began, so a raid whose picture changes every few ticks -- dropping each solve
    /// for the next -- still reaches its stand-in.
    behind_since: Option<MissionTime>,
    /// The last stand-in computed, and the problem it answered, so an unchanged picture
    /// is not re-solved every call.
    stood_in: Option<(Problem, StandIn)>,
    /// Set when this call's answer is an interim one: its bound and its reason.
    interim: Option<(InterimBound, String)>,
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
            horizon: horizon.max(1),
            solve_budget: DEFAULT_SOLVE_BUDGET,
            clock: Arc::new(MonotonicClock::new()),
            in_flight: None,
            solved: None,
            last_plan: PlanView::default(),
            solver_ok: true,
            last_solved: None,
            last_failure: None,
            progress: None,
            stand_in_after: DEFAULT_STAND_IN_AFTER,
            behind_since: None,
            stood_in: None,
            interim: None,
            local_frame: None,
            withheld: Vec::new(),
        }
    }

    /// How long, in mission time, the planner may be behind the picture before a
    /// one-step answer stands in (GAP-156, D-93). A deployment sets it through
    /// `gungnir-config`'s `plan_stand_in_after_ms`, whose validation keeps it finite and
    /// under a minute; this takes what it is given.
    #[must_use]
    pub fn with_stand_in_after(mut self, wait: Duration) -> Self {
        self.stand_in_after = wait;
        self
    }

    #[must_use]
    pub fn stand_in_after(&self) -> Duration {
        self.stand_in_after
    }

    /// Resources the last planning call would not propose, with the reason each.
    #[must_use]
    pub fn withheld(&self) -> &[WithheldResource] {
        &self.withheld
    }

    pub fn horizon(&self) -> usize {
        self.horizon
    }

    /// How long one planning call may spend solving (GAP-119, D-81). A deployment sets it
    /// through `gungnir-config`'s `plan_solve_budget_ms`, whose validation keeps it
    /// positive and finite; this takes what it is given.
    #[must_use]
    pub fn with_solve_budget(mut self, budget: Duration) -> Self {
        self.solve_budget = budget;
        self
    }

    #[must_use]
    pub fn solve_budget(&self) -> Duration {
        self.solve_budget
    }

    /// Measure the budget on `clock` instead of the machine's monotonic clock. For a
    /// test that makes a solve overrun on purpose ([`SteppedClock`]), or a replay that
    /// must decide it the same way every time.
    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn SolveClock>) -> Self {
        self.clock = clock;
        self
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
        if let Some((bound, reason)) = &self.interim {
            return PlanOutcome::Interim {
                plan,
                bound: *bound,
                reason: reason.clone(),
                progress: self.progress,
            };
        }
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
                progress: self.progress,
            },
            None => PlanOutcome::NoPlan {
                reason,
                progress: self.progress,
            },
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
        // Set again below if this call's answer is an interim one or a solve is under way.
        self.interim = None;
        self.progress = None;
        let ready: Vec<&ResourceView> = resources.iter().filter(|r| r.is_adequate()).collect();
        if tracks.is_empty() || ready.is_empty() {
            if !self.last_plan.is_empty() {
                self.last_plan = Self::fresh_plan(now, Vec::new(), 0.0, PlanBasis::Exact);
            }
            // **Nothing to solve is a fresh answer, not an absent one** (GAP-066): with no
            // tracks or no ready resource the empty plan is correct for this snapshot, and
            // the mark is set so a later failure can report how old the last real answer
            // is rather than reporting that there has never been one.
            //
            // **And the planner is healthy again** (GAP-119). This branch set the mark and
            // cleared the failure and left `solver_ok` alone, so a planner that had failed
            // once went on reporting itself unhealthy -- and `outcome` went on calling this
            // correct empty answer stale, with "the solve failed" as the reason -- until a
            // real solve happened to succeed.
            self.solver_ok = true;
            self.last_solved = Some(now);
            self.last_failure = None;
            self.in_flight = None;
            self.solved = None;
            self.behind_since = None;
            self.stood_in = None;
            return self.last_plan.clone();
        }
        let problem = Problem {
            tracks: tracks.iter().map(|t| t.id).collect(),
            resources: ready.iter().map(|r| r.id).collect(),
            rewards: rewards.clone(),
        };
        let answer = match self.solve(problem.clone()) {
            Ok(Some(policy)) => policy,
            // Not finished inside the budget: `solve` has said why. The last good plan
            // stands until the planner has been behind for its stand-in wait (GAP-156).
            Ok(None) => {
                let since = *self.behind_since.get_or_insert(now);
                if now.0 - since.0 >= self.stand_in_after.as_secs_f64() {
                    let why = format!(
                        "the exact solve for the current picture ({} track(s), {} ready \
                         resource(s)) has not finished {} after the planner fell behind \
                         it; it carries on, and its answer replaces this one when it does",
                        problem.tracks.len(),
                        problem.resources.len(),
                        budget::describe(self.stand_in_after),
                    );
                    return self.stand_in(now, tracks, &ready, problem, why);
                }
                return self.last_plan.clone();
            }
            // No exact answer will ever come, so nothing is gained by waiting for one.
            Err(Unsolvable::TooLarge {
                tracks: t,
                resources: r,
            }) => {
                self.in_flight = None;
                self.behind_since.get_or_insert(now);
                let why = format!(
                    "the exact solver takes at most {MAX_TRACKS} tracks and {MAX_RESOURCES} \
                     ready resources, and this picture has {t} track(s) and {r} ready \
                     resource(s); no exact answer will come for it"
                );
                return self.stand_in(now, tracks, &ready, problem, why);
            }
            Err(Unsolvable::Refused(err)) => {
                self.solver_ok = false;
                self.last_failure = Some(err.to_string());
                tracing::error!(%err, "allocation solve failed; keeping last good plan");
                return self.last_plan.clone();
            }
        };
        self.solver_ok = true;
        self.last_solved = Some(now);
        self.last_failure = None;
        self.behind_since = None;
        self.stood_in = None;
        let solutions = Self::solutions_with_geometry(
            self.local_frame.as_ref(),
            &answer.assignment,
            tracks,
            // The adequate list, in the order the matrix rows were built from.
            &ready,
        );
        // **GAP-097.** A solve that confirms the same resource/track pairs already in
        // `self.last_plan` is not a new recommendation, and must not become one:
        // `update::tick`'s "publish only when the plan changes" gate (GAP-066) compares
        // the whole `PlanView`, so minting a fresh id and `mission_time` here every tick
        // made that gate never hold once a solve succeeded, flooding the approval queue
        // with the same pairing at the tick rate. Only a genuinely different assignment
        // gets a new id and timestamp; an unchanged one keeps the plan -- geometry
        // included -- exactly as it was.
        //
        //
        // **The same rule across an interim answer** (GAP-156, D-93). An exact solve that
        // confirms the pairing of an interim plan keeps that plan, label and all: the
        // label records how the plan was reached, which does not change, and a second
        // plan for the same pairing is a second queue item for one recommendation -- the
        // flooding this rule exists to stop. An earlier draft minted one, and a rehearsal
        // whose planner fell behind queued the same pairing three times.
        if Self::assignment_changed(&self.last_plan, &solutions) {
            self.last_plan = Self::fresh_plan(now, solutions, answer.value, PlanBasis::Exact);
        }
        self.last_plan.clone()
    }

    /// Answer the current picture with a one-step answer, labelled as not the optimum,
    /// while the exact solve cannot (GAP-156, D-93; DN-04 §11).
    ///
    /// The planner stays unhealthy: the answer is for the picture, but it is not the one
    /// the planner exists to give, and the health flag is what puts that on the status
    /// strip and on the record (MOE-06). The stand-in is computed once per problem; an
    /// unchanged picture is answered from the one already computed.
    ///
    /// **Outside the solve budget, and cheap enough to be.** The one-step answer is an
    /// assignment problem, polynomial in the picture; at the exact solver's limits it
    /// takes a small fraction of the budget
    /// (`docs/record/2026-09-26/interim-plans-and-a-linked-plan-s-age.md` has the release
    /// probe). Budgeting it too would mean a stand-in that could itself fail to
    /// arrive, which is the problem it exists to end.
    fn stand_in(
        &mut self,
        now: MissionTime,
        tracks: &[TrackView],
        ready: &[&ResourceView],
        problem: Problem,
        why: String,
    ) -> PlanView {
        // The first stand-in since the planner last answered fresh: what the log says once.
        let first = self.stood_in.is_none();
        let computed = match self.stood_in.take() {
            Some((solved, answer)) if solved == problem => Ok(answer),
            _ => gungnir_allocation::stand_in(&problem.rewards, self.horizon),
        };
        let answer = match computed {
            Ok(answer) => answer,
            // Refused for the reason the exact solve would refuse it -- a reward that is
            // not finite -- so it is that failure, and the last good plan stands.
            Err(err) => {
                self.solver_ok = false;
                self.last_failure = Some(err.to_string());
                tracing::error!(%err, "the one-step answer failed too; keeping last good plan");
                return self.last_plan.clone();
            }
        };
        if first {
            tracing::warn!(
                tracks = problem.tracks.len(),
                resources = problem.resources.len(),
                share = answer.share_of_optimum(),
                "the exact solve cannot answer this picture in time; a one-step answer \
                 stands in, labelled as not the optimum"
            );
        }
        let solutions = Self::solutions_with_geometry(
            self.local_frame.as_ref(),
            &answer.first_step.assignment,
            tracks,
            ready,
        );
        // **A stand-in that recommends what the plan in force already recommends is not a
        // new recommendation** (GAP-097's rule): the plan in force stands, under the
        // interim standing this call reports. Its basis says how it was reached, which was
        // exactly; the standing says the planner cannot confirm it for this picture yet.
        if Self::assignment_changed(&self.last_plan, &solutions) {
            self.last_plan =
                Self::fresh_plan(now, solutions, answer.value_at_least, PlanBasis::OneStep);
        }
        self.solver_ok = false;
        self.last_solved = Some(now);
        self.last_failure = Some(why.clone());
        self.interim = Some((
            InterimBound {
                value_at_least: answer.value_at_least,
                optimum_at_most: answer.optimum_at_most,
            },
            why,
        ));
        self.stood_in = Some((problem, answer));
        self.last_plan.clone()
    }

    /// Answer `problem` inside this call's budget, if it can be (GAP-119).
    ///
    /// `Ok(Some)` is the optimum: from the last finished solve when the problem has not
    /// changed since, or from a solve that finished in this call. `Ok(None)` is a solve
    /// still under way, kept for the next call, with the reason recorded for
    /// [`Self::outcome`]. `Err` is a problem the solver refuses outright.
    fn solve(
        &mut self,
        problem: Problem,
    ) -> Result<Option<gungnir_allocation::AllocationPolicy>, Unsolvable> {
        if problem.tracks.len() > MAX_TRACKS || problem.resources.len() > MAX_RESOURCES {
            return Err(Unsolvable::TooLarge {
                tracks: problem.tracks.len(),
                resources: problem.resources.len(),
            });
        }
        if let Some((solved, policy)) = &self.solved {
            if *solved == problem {
                self.in_flight = None;
                return Ok(Some(policy.clone()));
            }
        }
        // A solve begun for a different problem answers nothing now: it is dropped and
        // this one begun in its place. Carrying it on would spend the budget on a
        // question nobody is asking.
        let mut flight = match self.in_flight.take() {
            Some(flight) if flight.problem == problem => flight,
            _ => InFlight {
                solve: ExactSolve::new(&problem.rewards, self.horizon)
                    .map_err(Unsolvable::Refused)?,
                problem,
                slices: 0,
            },
        };
        // The budget, measured from here: everything before this is bookkeeping over the
        // inputs, and the solve is the part that grows with the picture.
        let clock = Arc::clone(&self.clock);
        let budget = self.solve_budget;
        let started = clock.now();
        let progress = flight
            .solve
            .advance(&mut || clock.now().saturating_sub(started) <= budget);
        flight.slices += 1;
        match progress {
            Progress::Done(policy) => {
                if flight.slices > 1 {
                    tracing::info!(
                        slices = flight.slices,
                        tracks = flight.problem.tracks.len(),
                        resources = flight.problem.resources.len(),
                        "intercept solve finished over several planning calls"
                    );
                }
                self.solved = Some((flight.problem, policy.clone()));
                Ok(Some(policy))
            }
            Progress::Pending {
                states_done,
                states_total,
            } => {
                // Logged on the way into it only: a picture too big for one budget is
                // under way for several calls, and a line per call would bury the one
                // that says when it started.
                if self.solver_ok {
                    tracing::warn!(
                        budget_ms = budget.as_secs_f64() * 1e3,
                        tracks = flight.problem.tracks.len(),
                        resources = flight.problem.resources.len(),
                        "intercept solve did not finish inside its budget; keeping the last \
                         good plan and carrying the solve on"
                    );
                }
                self.solver_ok = false;
                // Whole percent, rounded down, so a solve never reads as finished before
                // it is. The sentence says nothing about the next call: this planner
                // carries the solve on, but one built for a single question (an
                // alternative, a what-if) is dropped with it.
                self.progress = Some(SolveProgress {
                    percent: states_done.saturating_mul(100) / states_total.max(1),
                    calls: flight.slices,
                });
                self.last_failure = Some(format!(
                    "the solve for the current picture ({} track(s), {} ready resource(s)) \
                     did not finish inside its {} budget",
                    flight.problem.tracks.len(),
                    flight.problem.resources.len(),
                    budget::describe(budget),
                ));
                self.in_flight = Some(flight);
                Ok(None)
            }
        }
    }

    /// A new plan, with a new identifier.
    ///
    /// **A UUID v7, not the next number** (D-56, GAP-130). The counter this replaced
    /// started at 1 in every planner, so the planner a desktop builds when it falls back
    /// numbered its plans as its node's planner did. The tick announces a plan only when
    /// its id is new (`gungnir-app`'s `last_live_plan_id`), so on switching back the
    /// node's plan was taken for the embedded plan of the same number and never proposed.
    /// The alternatives `gungnir-decision` solves on a planner built per call were all
    /// plan 1 as well.
    fn fresh_plan(
        now: MissionTime,
        solutions: Vec<InterceptSolutionView>,
        value: f64,
        basis: PlanBasis,
    ) -> PlanView {
        let id = PlanId(uuid::Uuid::now_v7().as_u128());
        PlanView {
            id,
            mission_time: now,
            kind: gungnir_model::PlanKind::Intercept { solutions },
            policy_value: value,
            releasability: gungnir_model::Releasability::default(),
            basis,
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

    /// A planner whose clock stands still, so no solve can overrun its budget: for every
    /// test that is not about the budget. On the monotonic clock such a test asserts that
    /// this machine, in this build, finished a solve inside 4 ms, which is a fact about the
    /// machine's load and not about the planner; under miri, where a solve runs about a
    /// thousand times slower, every one of them failed (GAP-164).
    fn unhurried(horizon: usize) -> DpInterceptService {
        DpInterceptService::new(horizon).with_clock(Arc::new(SteppedClock::new(Duration::ZERO)))
    }

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
        let mut svc = unhurried(10);
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
        let mut svc = unhurried(10);
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
            PlanOutcome::NoPlan { reason, progress } => {
                assert!(reason.contains("finite"), "{reason}");
                assert_eq!(*progress, None, "nothing is under way after a refusal");
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
        let mut svc = unhurried(10);
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
        let mut svc = unhurried(10);
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
        let mut svc = unhurried(10);
        let outcome = svc.plan(MissionTime(1.0), &[track(1)], &[resource(1, false)]);
        assert!(outcome.plan().expect("a plan").is_empty());
        assert!(svc.is_healthy(), "nothing was attempted, so nothing failed");
    }

    fn ready(ids: &[u32]) -> Vec<ResourceView> {
        ids.iter().map(|id| resource(*id, true)).collect()
    }

    /// What two plans recommend, which is everything but the identifier. A `PlanId` is a
    /// UUID v7 minted per plan (D-56) precisely so that two planners never name two plans
    /// alike, so it is the one field two fresh services must NOT agree on.
    fn recommendation(
        plan: &PlanView,
    ) -> (
        MissionTime,
        gungnir_model::PlanKind,
        u64,
        gungnir_model::Releasability,
    ) {
        (
            plan.mission_time,
            plan.kind.clone(),
            plan.policy_value.to_bits(),
            plan.releasability.clone(),
        )
    }

    fn pairs(plan: &PlanView) -> Vec<(u32, u64)> {
        plan.solutions()
            .iter()
            .map(|s| (s.resource.0, s.track.0))
            .collect()
    }

    /// **GAP-119, determinism with tied rewards.** Two fresh planners, the same three
    /// tracks and three ready resources, and the uniform matrix the trait method uses, so
    /// every assignment ties and only the documented tie rule decides: resource `k` in the
    /// order given takes track `k` in the order given. Identifiers deliberately differ
    /// from positions, so a rule that leaked identity into the tie would show.
    #[test]
    fn two_fresh_planners_agree_when_every_reward_ties() {
        let tracks = [track(70), track(71), track(72)];
        let resources = ready(&[40, 41, 42]);
        let mut a = unhurried(10);
        let mut b = unhurried(10);
        let pa = a.plan(MissionTime(1.0), &tracks, &resources);
        let pb = b.plan(MissionTime(1.0), &tracks, &resources);
        let (pa, pb) = match (pa, pb) {
            (PlanOutcome::Fresh(pa), PlanOutcome::Fresh(pb)) => (pa, pb),
            other => panic!("expected two fresh plans, got {other:?}"),
        };
        assert_eq!(recommendation(&pa), recommendation(&pb));
        assert_ne!(
            pa.id, pb.id,
            "two planners minted the same identifier (D-56)"
        );
        let mut got = pairs(&pa);
        got.sort_unstable();
        assert_eq!(got, vec![(40, 70), (41, 71), (42, 72)]);
    }

    /// **GAP-119, determinism with distinct rewards.** The optimum is unique and is not
    /// the diagonal, so this is the answer the numbers force rather than the tie rule.
    #[test]
    fn two_fresh_planners_agree_when_the_rewards_are_distinct() {
        let tracks = [track(70), track(71), track(72)];
        let resources = ready(&[40, 41, 42]);
        // Row r, column c: resource 40+r against track 70+c. The unique best matching is
        // 40->72, 41->70, 42->71, worth 9 + 8 + 7 = 24.
        let rewards = DMatrix::from_row_slice(3, 3, &[1.0, 2.0, 9.0, 8.0, 1.0, 3.0, 2.0, 7.0, 1.0]);
        let mut a = unhurried(1);
        let mut b = unhurried(1);
        let pa = a.plan_with_rewards(MissionTime(1.0), &tracks, &resources, &rewards);
        let pb = b.plan_with_rewards(MissionTime(1.0), &tracks, &resources, &rewards);
        assert!(a.is_healthy() && b.is_healthy());
        assert_eq!(recommendation(&pa), recommendation(&pb));
        let mut got = pairs(&pa);
        got.sort_unstable();
        assert_eq!(got, vec![(40, 72), (41, 70), (42, 71)]);
        assert!(
            (pa.policy_value - 24.0).abs() < 1e-12,
            "{}",
            pa.policy_value
        );
    }

    /// **GAP-119's degradation clause, against a real plan.** A solve at t = 1 inside its
    /// budget, then a solve at t = 2 that does not finish inside it: the answer is `Stale`
    /// carrying the t = 1 plan -- the same plan, identifier and all -- stamped t = 1, with
    /// the budget named in the reason, and the planner reports itself unhealthy. The
    /// earlier test of this path compared against an empty plan, which any bug that
    /// dropped the plan would also have produced.
    ///
    /// The t = 2 picture has gained a fourth track, because an unchanged picture is not
    /// solved again (its answer is already known, and is fresh); a new track is what makes
    /// t = 2 a solve at all. The clock is stepped, not slept on: zero while the first solve
    /// runs, so it takes no time; then ten milliseconds a reading against a four-millisecond
    /// budget, so the first question the second solve asks finds the budget spent.
    #[test]
    fn an_over_budget_solve_returns_the_last_good_plan_stale() {
        let clock = Arc::new(SteppedClock::new(Duration::ZERO));
        let mut svc = DpInterceptService::new(10)
            .with_solve_budget(Duration::from_millis(4))
            .with_clock(clock.clone());
        let tracks = [track(70), track(71), track(72)];
        let resources = ready(&[40, 41, 42]);

        let first = match svc.plan(MissionTime(1.0), &tracks, &resources) {
            PlanOutcome::Fresh(plan) => plan,
            other => panic!("the in-budget solve was not fresh: {other:?}"),
        };
        assert!(!first.is_empty(), "the t = 1 plan must be a real plan");
        assert!(svc.is_healthy());

        let grown = [track(70), track(71), track(72), track(73)];
        clock.set_step(Duration::from_millis(10));
        match svc.plan(MissionTime(2.0), &grown, &resources) {
            PlanOutcome::Stale {
                plan,
                computed_at,
                reason,
                progress,
            } => {
                assert_eq!(plan, first, "the stale plan is not the last good plan");
                assert_eq!(computed_at, MissionTime(1.0));
                assert!(reason.contains("4 ms budget"), "{reason}");
                assert!(reason.contains("4 track(s)"), "{reason}");
                assert_eq!(
                    progress,
                    Some(SolveProgress {
                        percent: 0,
                        calls: 1
                    })
                );
            }
            other => panic!("an over-budget solve returned {other:?}"),
        }
        assert!(!svc.is_healthy(), "a stale planner reported itself healthy");
        assert_eq!(svc.last_plan(), &first);

        // **Recovery.** The next call whose solve finishes inside its budget is fresh and
        // healthy again, and answers the picture it was given: the fourth track is in it.
        clock.set_step(Duration::ZERO);
        let recovered = match svc.plan(MissionTime(3.0), &grown, &resources) {
            PlanOutcome::Fresh(plan) => plan,
            other => panic!("the in-budget solve after an overrun returned {other:?}"),
        };
        assert!(
            svc.is_healthy(),
            "health did not recover on an in-budget solve"
        );
        assert_eq!(
            recovered.solutions().len(),
            3,
            "three resources, three pairs"
        );
        // Every reward ties, so the fourth track falls outside the tie rule's diagonal and
        // the pairing is the t = 1 pairing: the same plan, not a new one (GAP-097).
        assert_eq!(recovered, first);
    }

    /// **An unchanged picture is not solved again**, so a budget that could not afford a
    /// solve is never asked to: the answer is already known, and is fresh.
    #[test]
    fn an_unchanged_picture_is_answered_fresh_without_solving() {
        let clock = Arc::new(SteppedClock::new(Duration::ZERO));
        let mut svc = DpInterceptService::new(10).with_clock(clock.clone());
        let tracks = [track(70), track(71), track(72)];
        let resources = ready(&[40, 41, 42]);
        let first = svc
            .plan(MissionTime(1.0), &tracks, &resources)
            .plan()
            .cloned()
            .expect("a plan");
        clock.set_step(Duration::from_secs(1));
        match svc.plan(MissionTime(2.0), &tracks, &resources) {
            PlanOutcome::Fresh(plan) => assert_eq!(plan, first),
            other => panic!("an unchanged picture was not answered fresh: {other:?}"),
        }
        assert!(svc.is_healthy());
    }

    /// **A solve longer than one budget carries on, and finishes.** A clock that advances a
    /// millisecond a reading against a four-millisecond budget lets each call do a few
    /// units of work. Until the solve finishes, every answer is the last good plan, stale,
    /// with the progress rising; then the answer is fresh and is exactly the plan an
    /// unhurried planner computes for the same picture.
    ///
    /// The calls are a second of mission time apart, so the stand-in (GAP-156) is held
    /// off with a wait longer than the loop can run: this test is about the exact solve
    /// carrying on, and `an_interim_answer_stands_in_once_the_planner_has_waited` is
    /// about what happens when it takes too long.
    #[test]
    fn a_solve_longer_than_one_budget_carries_on_across_calls() {
        let clock = Arc::new(SteppedClock::new(Duration::ZERO));
        let mut svc = DpInterceptService::new(10)
            .with_solve_budget(Duration::from_millis(4))
            .with_stand_in_after(Duration::from_secs(100_000))
            .with_clock(clock.clone());
        let resources = ready(&[40, 41, 42]);
        let first = svc
            .plan(MissionTime(1.0), &[track(70)], &resources)
            .plan()
            .cloned()
            .expect("a plan");

        let grown = [track(70), track(71), track(72), track(73)];
        clock.set_step(Duration::from_millis(1));
        let mut calls = 0_u32;
        let mut last_percent = 0_u64;
        let finished = loop {
            calls += 1;
            assert!(calls < 10_000, "the solve never finished");
            match svc.plan(MissionTime(1.0 + f64::from(calls)), &grown, &resources) {
                PlanOutcome::Fresh(plan) => break plan,
                PlanOutcome::Stale {
                    plan,
                    computed_at,
                    reason,
                    progress,
                } => {
                    assert_eq!(plan, first);
                    assert_eq!(computed_at, MissionTime(1.0));
                    assert!(!svc.is_healthy());
                    let progress = progress.expect("the outcome says how far the solve has got");
                    assert!(progress.percent >= last_percent, "{reason}: {progress:?}");
                    last_percent = progress.percent;
                    assert_eq!(progress.calls, calls);
                    // The reason is the same sentence on every call, which is what lets a
                    // node publish it only when it changes (GAP-157).
                    assert!(!reason.contains('%'), "{reason}");
                }
                PlanOutcome::Interim { .. } => panic!("the stand-in was meant to be held off"),
                PlanOutcome::NoPlan { reason, .. } => {
                    panic!("the last good plan vanished: {reason}")
                }
            }
        };
        assert!(
            calls > 1,
            "the solve fitted one budget, so this tested nothing"
        );
        assert!(svc.is_healthy());

        let mut unhurried = unhurried(10);
        let expected = unhurried
            .plan(MissionTime(1.0), &grown, &resources)
            .plan()
            .cloned()
            .expect("a plan");
        assert_eq!(pairs(&finished), pairs(&expected));
        assert_eq!(
            finished.policy_value.to_bits(),
            expected.policy_value.to_bits()
        );
    }

    /// A picture that changes while a solve is under way drops that solve: its answer
    /// would be to a question nobody is asking any more.
    #[test]
    fn a_solve_for_a_picture_that_has_changed_is_dropped() {
        let clock = Arc::new(SteppedClock::new(Duration::from_millis(1)));
        let mut svc = DpInterceptService::new(10)
            .with_solve_budget(Duration::from_millis(4))
            .with_clock(clock);
        let resources = ready(&[40, 41, 42]);
        let four = [track(70), track(71), track(72), track(73)];
        let five = [track(70), track(71), track(72), track(73), track(74)];
        // A tenth of a second apart, inside the default stand-in wait (GAP-156).
        for n in 1..=3_u32 {
            let _ = svc.plan(MissionTime(0.1 * f64::from(n)), &four, &resources);
        }
        match svc.plan(MissionTime(0.4), &five, &resources) {
            PlanOutcome::NoPlan { reason, progress } => {
                assert!(reason.contains("5 track(s)"), "{reason}");
                assert_eq!(progress.map(|p| p.calls), Some(1), "{progress:?}");
            }
            other => panic!("expected the new picture's solve to have begun: {other:?}"),
        }
    }

    /// An overrun with nothing to fall back on is `NoPlan`, never an empty plan: the
    /// budget does not change GAP-066's distinction.
    #[test]
    fn an_over_budget_first_solve_has_no_plan_to_offer() {
        let mut svc = DpInterceptService::new(10)
            .with_clock(Arc::new(SteppedClock::new(Duration::from_millis(10))));
        match svc.plan(MissionTime(1.0), &[track(1)], &[resource(1, true)]) {
            PlanOutcome::NoPlan { reason, .. } => assert!(reason.contains("budget"), "{reason}"),
            other => panic!("expected no plan, got {other:?}"),
        }
        assert!(!svc.is_healthy());
    }

    /// The shipped planner's budget is MOP-06's. Nothing here times a real solve: a test
    /// that did would pass or fail with the machine's load, which is what the stepped
    /// clock exists to avoid.
    #[test]
    fn the_default_budget_is_mop_06() {
        let svc = DpInterceptService::new(10);
        assert_eq!(svc.solve_budget(), Duration::from_millis(4));
        assert_eq!(svc.solve_budget(), DEFAULT_SOLVE_BUDGET);
    }

    /// A planner that failed and then had nothing to solve is healthy again, and its
    /// answer is fresh: an empty picture is an answer, not a stale one.
    #[test]
    fn an_empty_picture_after_a_failure_is_fresh_and_healthy() {
        let mut svc = DpInterceptService::new(10)
            .with_clock(Arc::new(SteppedClock::new(Duration::from_millis(10))));
        let _ = svc.plan(MissionTime(1.0), &[track(1)], &[resource(1, true)]);
        assert!(!svc.is_healthy());
        let outcome = svc.plan(MissionTime(2.0), &[], &[resource(1, true)]);
        assert!(outcome.is_fresh(), "{outcome:?}");
        assert!(svc.is_healthy());
    }

    /// **GAP-097's own closing action**: an unmoving track and a ready resource,
    /// solved over many ticks, must keep the same plan -- same id, same
    /// `mission_time` -- throughout. Before the fix, every successful solve minted a
    /// fresh id regardless of whether the pairing changed, which is what flooded the
    /// approval queue with duplicates of the same recommendation at the tick rate.
    #[test]
    fn an_unchanged_assignment_keeps_the_same_plan_over_many_ticks() {
        let mut svc = unhurried(10);
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

    /// The planner's default wait before a stand-in is MOP-07's 500 ms (D-93).
    #[test]
    fn the_default_stand_in_wait_is_mop_07() {
        let svc = DpInterceptService::new(10);
        assert_eq!(svc.stand_in_after(), Duration::from_millis(500));
        assert_eq!(svc.stand_in_after(), DEFAULT_STAND_IN_AFTER);
    }

    /// **GAP-156, D-93.** A solve that cannot finish -- the clock steps ten milliseconds a
    /// reading against a four-millisecond budget, so it never advances -- is answered
    /// stale, with the last good plan, for as long as the planner's wait; from then the
    /// current picture gets a one-step answer, labelled as not the optimum, with its
    /// bound; and when the exact solve can run and reaches the same assignment, that
    /// plan stands (GAP-097), now fresh.
    ///
    /// The first picture has two tracks, so its pairing (the later resources, by the tie
    /// rule) differs from the four-track picture's (the diagonal): the stand-in is a
    /// different recommendation, and so a new plan.
    #[test]
    fn an_interim_answer_stands_in_once_the_planner_has_waited() {
        let clock = Arc::new(SteppedClock::new(Duration::ZERO));
        let mut svc = DpInterceptService::new(10)
            .with_solve_budget(Duration::from_millis(4))
            .with_stand_in_after(Duration::from_millis(500))
            .with_clock(clock.clone());
        let resources = ready(&[40, 41, 42]);
        let first = match svc.plan(MissionTime(1.0), &[track(71), track(72)], &resources) {
            PlanOutcome::Fresh(plan) => plan,
            other => panic!("the in-budget solve was not fresh: {other:?}"),
        };
        assert_eq!(first.basis, PlanBasis::Exact);

        let grown = [track(70), track(71), track(72), track(73)];
        clock.set_step(Duration::from_millis(10));
        // Behind from t = 2.0; inside the wait, the last good plan, stale -- and the
        // standing a node would publish does not change from call to call.
        let mut published = None;
        for t in [2.0, 2.2, 2.4] {
            let outcome = svc.plan(MissionTime(t), &grown, &resources);
            match &outcome {
                PlanOutcome::Stale {
                    plan, computed_at, ..
                } => {
                    assert_eq!(plan, &first);
                    assert_eq!(*computed_at, MissionTime(1.0));
                }
                other => panic!("t = {t}: inside the wait the answer was {other:?}"),
            }
            let standing = outcome.standing();
            assert!(
                published.as_ref().is_none_or(|p| p == &standing),
                "{standing:?}"
            );
            published = Some(standing);
        }

        // Half a second behind: the stand-in.
        let interim = match svc.plan(MissionTime(2.5), &grown, &resources) {
            PlanOutcome::Interim {
                plan,
                bound,
                reason,
                progress,
            } => {
                assert_eq!(plan.basis, PlanBasis::OneStep);
                assert_eq!(plan.mission_time, MissionTime(2.5));
                assert_ne!(
                    plan.id, first.id,
                    "a different pairing is a new recommendation, not the old one relabelled"
                );
                // A uniform matrix: one step at a time services all four tracks in two
                // steps, which is the optimum, so the bound is the whole of it.
                assert!((bound.value_at_least - 4.0).abs() < 1e-12, "{bound:?}");
                assert!((bound.share_of_optimum() - 1.0).abs() < 1e-12, "{bound:?}");
                assert_eq!(
                    bound.sentence(),
                    "worth at least 100% of the best plan's value"
                );
                assert!(reason.contains("has not finished 500 ms after"), "{reason}");
                assert!(reason.contains("4 track(s)"), "{reason}");
                assert!(progress.is_some(), "the exact solve is still under way");
                plan
            }
            other => panic!("half a second behind, the answer was {other:?}"),
        };
        assert!(
            !svc.is_healthy(),
            "an interim answer is not the planner's own, and the flag says so"
        );
        assert_ne!(pairs(&interim), pairs(&first));

        // The same picture again: the same interim plan, not a new one per call.
        match svc.plan(MissionTime(2.6), &grown, &resources) {
            PlanOutcome::Interim { plan, .. } => assert_eq!(plan, interim),
            other => panic!("{other:?}"),
        }

        // The exact solve can run and reaches the same assignment: the interim plan
        // stands, identifier, basis and all, now answered fresh. A second plan for the
        // same pairing would be a second queue item for one recommendation (GAP-097).
        clock.set_step(Duration::ZERO);
        match svc.plan(MissionTime(3.0), &grown, &resources) {
            PlanOutcome::Fresh(plan) => assert_eq!(plan, interim),
            other => panic!("the exact solve finished and the answer was {other:?}"),
        }
        assert!(svc.is_healthy());

        // A different picture whose optimum pairs differently: a new plan, reached
        // exactly.
        match svc.plan(MissionTime(4.0), &[track(72)], &resources) {
            PlanOutcome::Fresh(plan) => {
                assert_ne!(plan.id, interim.id);
                assert_eq!(plan.basis, PlanBasis::Exact);
            }
            other => panic!("{other:?}"),
        }
    }

    /// **A stand-in that recommends what the plan in force recommends is not a new
    /// plan** (GAP-097's rule, D-93): the plan in force stands, under an interim standing,
    /// and nothing new reaches the queue. Found by `gungnir-app/tests/rehearsal.rs` on a
    /// slow runner, where an earlier draft minted a plan for the stand-in and another for
    /// the exact answer, and queued one pairing three times.
    #[test]
    fn a_stand_in_that_agrees_with_the_plan_in_force_keeps_it() {
        let clock = Arc::new(SteppedClock::new(Duration::ZERO));
        let mut svc = DpInterceptService::new(10)
            .with_solve_budget(Duration::from_millis(4))
            .with_clock(clock.clone());
        let resources = ready(&[40, 41, 42]);
        let first = svc
            .plan(
                MissionTime(1.0),
                &[track(70), track(71), track(72)],
                &resources,
            )
            .plan()
            .cloned()
            .expect("a plan");
        let grown = [track(70), track(71), track(72), track(73)];
        clock.set_step(Duration::from_millis(10));
        let _ = svc.plan(MissionTime(2.0), &grown, &resources);
        match svc.plan(MissionTime(2.5), &grown, &resources) {
            PlanOutcome::Interim { plan, bound, .. } => {
                assert_eq!(plan, first, "the plan in force was not kept");
                assert_eq!(plan.basis, PlanBasis::Exact);
                assert!((bound.share_of_optimum() - 1.0).abs() < 1e-12);
            }
            other => panic!("{other:?}"),
        }
        assert!(!svc.is_healthy());
        clock.set_step(Duration::ZERO);
        match svc.plan(MissionTime(3.0), &grown, &resources) {
            PlanOutcome::Fresh(plan) => assert_eq!(plan, first),
            other => panic!("{other:?}"),
        }
    }

    /// **The wait is measured from falling behind, not from the current solve**: a raid
    /// whose picture grows every call drops each solve for the next, and still reaches its
    /// stand-in.
    #[test]
    fn a_picture_that_keeps_changing_still_reaches_its_stand_in() {
        let clock = Arc::new(SteppedClock::new(Duration::ZERO));
        let mut svc = DpInterceptService::new(10)
            .with_solve_budget(Duration::from_millis(4))
            .with_clock(clock.clone());
        let resources = ready(&[40, 41, 42]);
        let _ = svc.plan(MissionTime(1.0), &[track(70)], &resources);
        clock.set_step(Duration::from_millis(10));
        let mut picture = vec![track(70)];
        let mut last = None;
        for (n, t) in [2.0, 2.2, 2.4, 2.6].into_iter().enumerate() {
            picture.push(track(71 + n as u64));
            last = Some(svc.plan(MissionTime(t), &picture, &resources));
        }
        match last {
            Some(PlanOutcome::Interim { plan, .. }) => {
                assert_eq!(plan.basis, PlanBasis::OneStep);
                assert_eq!(plan.solutions().len(), 3);
            }
            other => panic!("0.6 s behind a changing picture: {other:?}"),
        }
    }

    /// **A picture the exact solver will not take is answered at once**: waiting for an
    /// answer that will never come helps nobody (GAP-156).
    #[test]
    fn a_picture_past_the_exact_limits_is_answered_at_once() {
        let mut svc = unhurried(10);
        let many: Vec<TrackView> = (0..20).map(|i| track(100 + i)).collect();
        match svc.plan(MissionTime(1.0), &many, &ready(&[40, 41])) {
            PlanOutcome::Interim {
                plan,
                reason,
                progress,
                ..
            } => {
                assert_eq!(plan.basis, PlanBasis::OneStep);
                assert_eq!(pairs(&plan), vec![(40, 100), (41, 101)]);
                assert!(reason.contains("at most 16 tracks"), "{reason}");
                assert!(reason.contains("20 track(s)"), "{reason}");
                assert_eq!(progress, None, "no exact solve is under way, nor will be");
            }
            other => panic!("twenty tracks: {other:?}"),
        }
        assert!(!svc.is_healthy());
        let effectors: Vec<u32> = (40..49).collect();
        match svc.plan(MissionTime(2.0), &[track(1)], &ready(&effectors)) {
            PlanOutcome::Interim { reason, .. } => {
                assert!(reason.contains("9 ready resource(s)"), "{reason}");
            }
            other => panic!("nine effectors: {other:?}"),
        }
        // And a picture back inside the limits is the optimum again.
        assert!(svc
            .plan(MissionTime(3.0), &[track(1)], &ready(&[40]))
            .is_fresh());
        assert!(svc.is_healthy());
    }
}
