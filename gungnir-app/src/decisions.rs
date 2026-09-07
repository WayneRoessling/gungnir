// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The approval gate between a proposed plan and anything acting on it (GAP-038).
//!
//! `ARCHITECTURE.md` §7.3 listed `gungnir-policy` and `gungnir-command` as "not yet
//! called from the tick". This module calls them: every plan the intercept service
//! proposes is evaluated by a policy chain, and a plan that clears it is queued for a
//! human decision rather than becoming actionable. Nothing here executes anything --
//! that boundary is `gungnir-policy`'s whole purpose, and contract C-01 depends on it.
//!
//! # What the chain is, and what it is not
//!
//! The chain this build constructs is the readiness-and-geofence engine, the control
//! status engine, and the authority engine. It is complete in the sense that every
//! engine `gungnir-policy` implements is in it, and incomplete in a way the operator
//! has to know about: the geofence engine consults a `GeoService` that this build has
//! nothing to populate from, because `ConfigBaseline` carries no geofences. So it
//! checks resource readiness for real and the no-go check passes vacuously.
//! [`PolicyChainReport`] carries the engine names and that caveat to PN-07, because a
//! verdict from a chain missing an engine is not the same verdict.
//!
//! # The queue is empty, and why that is the interesting part
//!
//! No plan ever reaches the queue in this build. The tracking pipeline is not
//! implemented, so `tracks()` is empty; with no tracks the allocator returns an empty
//! plan; an empty plan is denied by the first engine that sees it. Every step of that
//! is correct, and the result -- a permanently calm approval queue -- is the most
//! misleading screen in the product. [`queue_empty_reason`] derives which of the three
//! [`EmptyBecause`] situations is in force from the health flags and the denial
//! history, so PN-06 says *why* it is empty rather than only that it is.

use crate::state::AppState;
use gungnir_command::{
    governing_layer, ApprovalWorkflow, CommandError, OperatorDecision, PendingApprovalId,
    QueueOutcome, Submission,
};
use gungnir_decision::CourseOfAction;
use gungnir_eventing::Event;
use gungnir_model::events::{CommandEvent, InterceptEvent};
use gungnir_model::{Classification, PlanView, TrackId};
use gungnir_policy::{
    is_pre_delegated, AuthorityPolicy, ControlStatusPolicy, DenialReason, FiresContext,
    FiresDeconflictionPolicy, GeofencePolicy, PolicyChain, PolicyEngine, PolicyVerdict,
};
use gungnir_security::authz::role_permits;
use gungnir_security::{actions, Role};
use gungnir_ui::panels::approval_queue::{
    ApprovalQueueView, EmptyBecause, PendingId, QueueOrder, QueueRow, TimeRemaining, Verdict,
};
use gungnir_ui::panels::unavailable::Unavailable;

/// The priority tie-break in the queue ordering needs a threat score, and nothing
/// computes one (GAP-028). The queue is therefore ordered by time remaining alone, and
/// PN-06 says so rather than implying a severity order it does not have.
const PRIORITY: Unavailable<'static> = Unavailable {
    owner: "gungnir-assessment",
    gap: "GAP-028",
};

/// The escalation ladder: the roles that may take a plan decision, lowest authority
/// first.
///
/// Derived from `gungnir-security` rather than configured here, so it cannot drift from
/// the authorization it is supposed to follow: a role that cannot decide must never be
/// offered an item, and a role that can must not be skipped.
#[must_use]
pub fn escalation_ladder() -> Vec<String> {
    let mut roles: Vec<Role> = Role::ALL
        .iter()
        .copied()
        .filter(|r| role_permits(*r, DECISION_ACTION))
        .collect();
    roles.sort_by_key(|r| r.rank());
    roles.iter().map(|r| format!("{r:?}")).collect()
}

/// The authorization action a plan decision falls under.
pub const DECISION_ACTION: &str = actions::DECIDE_PLAN;

/// What the policy chain was, and which of its checks could not have failed.
///
/// The second half is the part that is easy to under-report, and I did under-report it
/// first time: the no-go check is vacuous for **two independent reasons**, and fixing
/// either one alone would leave it vacuous. Both have to be named, because an operator
/// told only about the missing fences would reasonably conclude that configuring some
/// fences makes the check real.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyChainReport {
    /// Engine names in the order they were consulted.
    pub engines: Vec<&'static str>,
    /// `ConfigBaseline` has no geofence section, so the `GeoService` holds no fences and
    /// `is_within_no_go` is a search of an empty list.
    pub no_geofences_configured: bool,
    /// The planner sets `intercept_point: None` on every solution (GAP-031), so the
    /// no-go test is never reached even with fences configured.
    pub no_intercept_geometry: bool,
    /// Three of the fires checks have no data source -- no-fire areas (GAP-088), airspace
    /// measures, interceptor points (GAP-031) -- and a check without data fails (DN-05
    /// §5), so a fires task is denied until the sources exist.
    pub fires_sources_missing: bool,
}

impl PolicyChainReport {
    /// The checks that ran and could not have failed, as PN-07 shows them.
    #[must_use]
    pub fn caveats(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.fires_sources_missing {
            out.push(
                "the fires no-fire-area, airspace-measure and interceptor-trajectory checks, \
                 which have no data source (GAP-088, GAP-031) and therefore fail: a fires \
                 task is denied until the sources exist, and PN-05 lists each check",
            );
        }
        if self.no_intercept_geometry {
            out.push(
                "the no-go geofence check, because the planner computes no intercept \
                 point to test (GAP-031)",
            );
        }
        if self.no_geofences_configured {
            out.push(
                "the no-go geofence check, because the configuration baseline has no \
                 geofence section to load fences from",
            );
        }
        out
    }
}

/// The last denial the chain returned, kept so an empty queue can say why.
#[derive(Debug, Clone, Default)]
pub struct DenialHistory {
    pub count: usize,
    pub last_reason: Option<String>,
}

/// What the queue sweep has done this session.
///
/// Only escalations are counted here. Expiries are **not**, and that is the point of
/// the DN-10 §3 conformance: an expiry is `OperatorDecision::Expired` in the record, so
/// [`expired_count`] reads the history rather than a parallel counter that could drift
/// from it. An escalation has no record by design -- an escalated item has not ended --
/// so a counter is the only place it can live.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QueueOutcomeCounts {
    /// Offers made to a higher role. Not decisions, and not failures.
    pub escalated: usize,
}

/// Windows that closed with nobody deciding, from the append-only history.
///
/// **Not rejections**: nobody chose. Answerable from the record since the DN-10 §3
/// conformance; before it, this could only have been derived from a rejection happening
/// to name no operator, which every rejection does while there is no operator session.
#[must_use]
pub fn expired_count(state: &AppState) -> usize {
    state
        .approvals
        .records()
        .iter()
        .filter(|r| r.is_expiry())
        .count()
}

/// What became of a plan handed to [`submit`].
///
/// A policy verdict alone could not say this: **a superseded plan was never evaluated**,
/// and reporting it as `Denied` would put a refusal nobody made into the record while
/// reporting it as `RequiresHumanApproval` would leave an item waiting that will never be
/// applied. Both are worse than a third case.
#[derive(Debug, Clone, PartialEq)]
pub enum Submitted {
    /// The policy chain ran, and this is what it returned.
    Evaluated(PolicyVerdict),
    /// **Not evaluated.** The baseline in force is outside its validity window, so this
    /// plan is superseded rather than applied (DN-08 §5). `PlanSuperseded` is published,
    /// so the journal records what happened rather than a gap where a plan should be.
    Superseded,
}

/// Evaluate a plan and queue it if it clears policy.
///
/// Returns what became of it so the caller can record the denial. A denied plan is never
/// queued: that is `ApprovalWorkflow::submit_for_approval`'s invariant and this
/// function does not work around it.
pub fn submit(state: &mut AppState, plan: PlanView) -> Submitted {
    // **Before policy, not after.** A plan produced under a baseline that is outside its
    // window is superseded whatever policy would have said about it (DN-08 §5), and
    // running the chain first would put a verdict in the record for a plan that was never
    // going to be applied.
    let validity = crate::status::baseline_validity(state);
    if validity.supersedes_plans() {
        let now = state.clock.now();
        let plan_id = plan.id.0;
        crate::update::publish(
            state,
            now,
            Event::Intercept(InterceptEvent::PlanSuperseded(plan.clone())),
        );
        crate::engagements::observe_superseded(state, plan.id, now);
        // Said once per plan, not once per frame: `submit` runs only when the plan
        // changes, which is why this is here rather than in the tick.
        state.alerts.push(format!(
            "Plan {plan_id} superseded: the baseline in force is outside its validity \
             window, so nothing produced now will be applied"
        ));
        tracing::warn!(
            plan = plan_id,
            ?validity,
            "plan superseded: baseline not in force"
        );
        return Submitted::Superseded;
    }
    let verdict = evaluate(state, &plan);
    // GAP-036: PN-05 lists every fires check with its result, passes included.
    state.fires_checks = fires_checks(state, &plan);
    // GAP-028: the verdict is on the record with the engines that produced it, whatever
    // it was; a denial that reached only the counter would be a decision nobody could
    // review.
    crate::update::publish(
        state,
        state.clock.now(),
        Event::Intercept(InterceptEvent::PlanEvaluated {
            plan: plan.id,
            verdict: verdict.summary(),
            engines: DESKTOP_ENGINES.iter().map(|e| (*e).to_string()).collect(),
        }),
    );
    if let PolicyVerdict::Denied { reason_code } = verdict {
        state.denials.count += 1;
        state.denials.last_reason = Some(format!("{reason_code:?}"));
        return Submitted::Evaluated(verdict);
    }
    // The layer whose window closes first governs the deadline; a plan tasking nothing
    // this deployment knows about has no layer, and policy has already denied it.
    let now = state.clock.now();
    let Some(layer) = governing_layer(&plan, &state.resources, &state.config.policy.decisions)
    else {
        tracing::error!(
            plan = plan.id.0,
            "a plan that tasks no known resource cleared policy; not queuing it"
        );
        return Submitted::Evaluated(verdict);
    };
    let submission = Submission {
        plan,
        verdict,
        submitted: now,
        layer,
        // No assessment runs, so every item scores the same and the ordering falls back
        // to time remaining. Reported by `QueueOrder`, not hidden behind a zero.
        priority: 0.0,
        role: format!("{:?}", state.role()),
    };
    match state.approvals.submit_for_approval(submission) {
        Ok(_) => {}
        // Unreachable given the branch above, but a silent `let _ =` here would be the
        // place a real refusal went missing.
        Err(err) => tracing::error!(%err, "plan was refused by the approval workflow"),
    }
    Submitted::Evaluated(verdict)
}

/// Apply expiry and escalation, publishing what happened.
///
/// Called every tick. Nothing ends silently: an expiry leaves a `DecisionRecord` that
/// is not actionable and names no operator, and both outcomes reach the bus, so the
/// journal records a decision nobody took as exactly that rather than as a rejection.
pub fn sweep(state: &mut AppState) {
    let now = state.clock.now();
    let ladder = escalation_ladder();
    let refs: Vec<&str> = ladder.iter().map(String::as_str).collect();
    let outcomes = state.approvals.sweep(now, &refs);
    for (plan, id, outcome) in outcomes {
        let event = match &outcome {
            QueueOutcome::Expired { at } => {
                tracing::warn!(
                    plan = plan.0,
                    id = id.0,
                    "approval expired with nobody deciding"
                );
                state.alerts.push(format!(
                    "Plan {} expired with nobody deciding; this is not a rejection",
                    plan.0
                ));
                CommandEvent::Expired { plan, at: *at }
            }
            QueueOutcome::Escalated { to_role, at } => {
                state.queue_outcomes.escalated += 1;
                tracing::info!(plan = plan.0, to_role, "approval escalated");
                CommandEvent::Escalated {
                    plan,
                    to_role: to_role.clone(),
                    at: *at,
                }
            }
        };
        if let Err(err) = state
            .events
            .publish(now, gungnir_eventing::Event::Command(event))
        {
            tracing::error!(%err, "queue outcome publish failed");
        }
    }
}

/// Run the policy chain over a plan.
///
/// The chain is constructed per call because two of the three engines borrow the
/// baseline and a closure over the current track snapshot; it is three small structs
/// and no allocation beyond the boxes, which is within the tick's budget.
/// The engines the desktop's chain runs, in order, as PN-07 and the record name them.
pub const DESKTOP_ENGINES: [&str; 4] = [
    "readiness and geofence",
    "control status",
    "authority",
    "fires deconfliction",
];

/// What the picture can supply to the fires checks (DN-05 §5), and nothing it cannot.
///
/// Friendly positions are the tracks carried as friendly, placed through the local frame;
/// without an origin they cannot be placed and the check is told so. No-fire areas
/// (GAP-088), airspace measures (no source) and interceptor points (GAP-031) are `None`,
/// which DN-05 §5 rule 2 turns into a failed check with the reason -- never a pass.
fn friendly_positions(state: &AppState) -> Option<Vec<gungnir_model::Geodetic>> {
    let frame = crate::sustainment::local_frame(state)?;
    Some(
        state
            .tracking
            .tracks()
            .iter()
            .filter(|t| t.classification == Classification::Friendly)
            .map(|t| frame.to_geodetic([t.state[0], t.state[1], t.state[2]]))
            .collect(),
    )
}

/// The fires checks for a plan, every one with its result (DN-05 §7 for PN-05). Empty for
/// a plan that is not a fires task.
#[must_use]
pub fn fires_checks(state: &AppState, plan: &PlanView) -> Vec<gungnir_model::DeconflictionCheck> {
    let Some(fires) = plan.fires() else {
        return Vec::new();
    };
    let friendly = friendly_positions(state);
    let policy = FiresDeconflictionPolicy {
        settings: &state.config.policy.fires,
        context: FiresContext {
            friendly_positions: friendly.as_deref(),
            no_fire_areas: None,
            airspace_measures: None,
            interceptor_points: None,
        },
    };
    policy.deconflict(fires).checks
}

fn evaluate(state: &AppState, plan: &PlanView) -> PolicyVerdict {
    with_chain(state, |chain| chain.evaluate(plan, &state.resources))
}

/// Build the desktop's policy chain and hand it to `f`.
///
/// Continuation-passing rather than a function returning the chain, because three of the
/// four engines borrow values that have to be built first -- the geofence service, the
/// role name, the friendly positions and the classifier over the current snapshot -- and
/// a chain returned by value would outlive them.
///
/// **One construction site, deliberately.** [`evaluate`] and [`alternatives`] must judge
/// a plan by the same four engines, or an alternative could be offered under a chain the
/// plan in force was never held to; building the chain twice is exactly how that drifts.
fn with_chain<T>(state: &AppState, f: impl FnOnce(&PolicyChain<'_>) -> T) -> T {
    let classification = classifier(state);
    // GAP-088: the fences the baseline declares, not an empty service.
    let geo = crate::geofences::service_from_config(&state.config);
    let role_name = format!("{:?}", state.role());
    let friendly = friendly_positions(state);
    let chain = PolicyChain::new(vec![
        Box::new(GeofencePolicy { geo: &geo }),
        Box::new(ControlStatusPolicy {
            settings: &state.config.policy.control_status,
            track_classification: &classification,
        }),
        Box::new(AuthorityPolicy {
            settings: &state.config.policy.authority,
            asking_role: &role_name,
            action: DECISION_ACTION,
            track_classification: &classification,
        }),
        // GAP-036: fourth, with what the picture can supply (see `friendly_positions`).
        Box::new(FiresDeconflictionPolicy {
            settings: &state.config.policy.fires,
            context: FiresContext {
                friendly_positions: friendly.as_deref(),
                no_fire_areas: None,
                airspace_measures: None,
                interceptor_points: None,
            },
        }),
    ]);
    f(&chain)
}

/// How many alternatives the desktop asks for alongside a recommendation (GAP-032).
///
/// Three, and not "as many as there are tasked resources": each alternative is another
/// solve, the answer is regenerated only when the plan changes rather than every frame,
/// and a list longer than a person compares under time pressure is a list nobody reads
/// (`docs/plans/` plan 06 on the saturation thread). The cap is a constant rather than a
/// baseline setting because nothing in `ConfigBaseline` covers decision support yet, and
/// inventing a section for it would be a configuration surface with no design behind it.
pub const MAX_ALTERNATIVES: usize = 3;

/// The allocator, for a snapshot that is not the live one (GAP-032).
///
/// A **fresh** `DpInterceptService` per call, deliberately. The live one carries the last
/// good plan, the plan-identifier counter, the health flag and the withheld list, and an
/// alternative or a what-if that advanced any of those would be a rehearsal that changed
/// the picture it was rehearsing against -- which is precisely what the verification row's
/// "live state unchanged after `what_if`" criterion forbids. Constructing one is a handful
/// of fields and a unit-struct allocator, so this is cheap as well as safe.
///
/// It plans the way the tick plans: `InterceptService::plan`, which is the uniform reward
/// matrix. `gungnir-assessment`'s rewards are not wired into the tick either
/// (`ARCHITECTURE.md` §7.3), and an alternative solved against a reward matrix the live
/// plan was never solved against would not be an alternative to it.
struct SnapshotPlanner {
    horizon: usize,
    frame: Option<gungnir_model::LocalFrame>,
}

impl gungnir_decision::PlanSource for SnapshotPlanner {
    fn plan(
        &self,
        now: gungnir_model::MissionTime,
        tracks: &[gungnir_model::TrackView],
        resources: &[gungnir_model::ResourceView],
    ) -> Result<PlanView, gungnir_decision::PlanUnavailable> {
        use gungnir_intercept_service::{DpInterceptService, InterceptService, PlanOutcome};
        let mut planner = DpInterceptService::new(self.horizon).with_local_frame(self.frame);
        match planner.plan(now, tracks, resources) {
            PlanOutcome::Fresh(plan) => Ok(plan),
            // A planner constructed a line ago cannot hold a stale plan, but both
            // non-fresh outcomes carry a reason and both mean the same thing here: this
            // snapshot has no answer, which is not the same as needing no action.
            PlanOutcome::Stale { reason, .. } | PlanOutcome::NoPlan { reason } => {
                Err(gungnir_decision::PlanUnavailable::Allocator { reason })
            }
        }
    }
}

/// The planner the alternatives and the what-if solve with.
fn snapshot_planner(state: &AppState) -> SnapshotPlanner {
    SnapshotPlanner {
        horizon: state.config.allocation_horizon,
        // GAP-031: the same frame the live planner solves geometry in, so an alternative
        // places its intercept points exactly where the recommendation would.
        frame: crate::sustainment::local_frame(state),
    }
}

/// The recommendation and its alternatives, each carrying the verdict the desktop's own
/// four-engine chain returned for it (GAP-032).
///
/// Called from the tick when a plan is proposed, not once per frame: each alternative is
/// another solve, and the answer only changes when the plan does.
///
/// The first element is the recommendation; the rest are alternatives, the actionable ones
/// ahead of the ones policy has already refused. **A refused alternative is shown rather
/// than filtered out**, with its denial -- an operator who cannot see that the obvious
/// second option is barred will ask for it on the radio.
#[must_use]
pub fn alternatives(state: &AppState, max_alternatives: usize) -> Vec<CourseOfAction> {
    use gungnir_decision::DecisionSupport;
    with_support(state, |support| support.recommend(max_alternatives))
}

/// Build the decision-support implementor over the live picture and hand it to `f`.
///
/// Continuation-passing for the same reason as [`with_chain`], which it wraps: every field
/// of `PlanAlternatives` is a shared borrow of something built here -- the policy chain,
/// the fresh planner, the ranking and the assessor -- so a value returned from this
/// function would outlive them. It is also what keeps [`alternatives`] and [`what_if`]
/// looking at the same picture through the same chain, which is the property that makes an
/// alternative comparable with the recommendation.
fn with_support<T>(
    state: &AppState,
    f: impl FnOnce(&mut gungnir_decision::PlanAlternatives<'_>) -> T,
) -> T {
    let planner = snapshot_planner(state);
    let ranking = crate::sustainment::asset_exposure(state);
    let assessor = crate::sustainment::asset_assessor(state);
    with_chain(state, |chain| {
        let mut support = gungnir_decision::PlanAlternatives {
            now: state.clock.now(),
            tracks: state.tracking.tracks(),
            resources: &state.resources,
            scores: ranking.scores(),
            source: &planner,
            policy: chain,
            // `None` where this deployment declares no origin or no assets, which
            // `asset_exposure` reports as `NotScored` rather than as a zero ranking.
            assessor: assessor
                .as_ref()
                .map(|a| a as &dyn gungnir_assessment::ThreatAssessor),
        };
        f(&mut support)
    })
}

/// The plan for a hypothetical track snapshot, committed to nothing (GAP-032).
///
/// Nothing published, nothing queued, nothing recorded, and no live value touched: the
/// planner is a fresh one and everything else is a shared borrow. The returned course of
/// action says so in its own rationale, because a course of action that reached a screen
/// without the word "hypothetical" on it would be read as a proposal.
#[must_use]
pub fn what_if(
    state: &AppState,
    hypothetical_tracks: &[gungnir_model::TrackView],
) -> CourseOfAction {
    use gungnir_decision::DecisionSupport;
    with_support(state, |support| support.what_if(hypothetical_tracks))
}

/// The plan if the selected track were lost, for PN-05 (GAP-032).
///
/// The what-if needs a hypothetical snapshot, and this build has no panel that composes
/// one -- there is no editor for a track list, and inventing a hypothesis nobody asked
/// for would be worse than showing none. What it does have is a *selection*: the operator
/// has already pointed at a track, and "what happens to the plan if we lose this one" is
/// the question that selection makes askable without a new input surface. It is a
/// rehearsal against the picture minus that track and it changes nothing.
///
/// `None` when nothing is selected, so the panel draws no what-if rather than a
/// hypothesis the operator did not pose.
#[must_use]
pub fn what_if_selected_track_lost(state: &AppState) -> Option<CourseOfAction> {
    let selected = state.selected_track()?;
    let remaining: Vec<gungnir_model::TrackView> = state
        .tracking
        .tracks()
        .iter()
        .filter(|t| t.id != selected)
        .cloned()
        .collect();
    Some(what_if(state, &remaining))
}

/// Refresh the decision support PN-05 draws (GAP-032).
///
/// `submitted` is what became of the plan proposed this frame, or `None` on a frame where
/// the plan did not change. The two halves are refreshed on different triggers because
/// they go stale for different reasons, and each one costs an allocator solve:
///
/// - the **alternatives** are alternatives to a particular plan, so they are regenerated
///   when a plan is proposed and cleared when one is superseded -- a superseded plan was
///   never evaluated, and options nobody could act on are worse than none;
/// - the **rehearsal** answers a question the operator posed by selecting a track, so it
///   is recomputed when the selection moves and when the plan under it has been replaced.
pub fn refresh_support(state: &mut AppState, submitted: Option<&Submitted>) {
    match submitted {
        Some(Submitted::Evaluated(_)) => {
            let courses = alternatives(state, MAX_ALTERNATIVES);
            state.alternatives = courses;
            // The held rehearsal was computed against the plan just replaced, so it is
            // stale whatever the selection is.
            state.what_if_for = None;
        }
        Some(Submitted::Superseded) => {
            state.alternatives = Vec::new();
            state.what_if_for = None;
        }
        None => {}
    }
    if state.what_if_for != state.selected_track() {
        let course = what_if_selected_track_lost(state);
        state.what_if_for = state.selected_track();
        state.what_if = course;
    }
}

/// A verdict in the words a panel can show, spelled the way the record spells it.
///
/// Built from [`PolicyVerdict::summary`] rather than from a second match over the verdict,
/// so what an operator reads on PN-05 and what a reviewer reads in the journal cannot
/// diverge -- GAP-028 put that mapping in one place on purpose.
#[must_use]
pub fn verdict_sentence(verdict: PolicyVerdict) -> String {
    match verdict.summary() {
        gungnir_model::events::VerdictSummary::Approved => {
            "approved outright by every engine".to_owned()
        }
        gungnir_model::events::VerdictSummary::RequiresHumanApproval => {
            "cleared policy; needs a person to decide".to_owned()
        }
        gungnir_model::events::VerdictSummary::Denied { reason } => {
            format!("DENIED by policy: {reason}")
        }
    }
}

/// What the chain consulted, for PN-07.
///
/// The engine is named "readiness and geofence" rather than "geofence" because that is
/// what it is: `GeofencePolicy::evaluate` denies an empty plan, an unknown resource and
/// an unready resource before it looks at geometry at all. Those three checks are real
/// and run against the configured resources. Only the geometry half is vacuous, and
/// calling the whole engine vacuous would understate the chain as badly as calling it
/// sound would overstate it.
#[must_use]
pub fn chain_report_for(config: &gungnir_config::ConfigBaseline) -> PolicyChainReport {
    PolicyChainReport {
        engines: DESKTOP_ENGINES.to_vec(),
        // GAP-088: the real count, not a hard-coded caveat.
        no_geofences_configured: config.geofences.is_empty(),
        no_intercept_geometry: true,
        fires_sources_missing: true,
    }
}

/// The report for a baseline that declares nothing, which every test fixture is.
#[must_use]
pub fn chain_report() -> PolicyChainReport {
    chain_report_for(&gungnir_config::ConfigBaseline::default())
}

/// Classification of a track, for the two engines that judge by class.
///
/// A track the snapshot no longer holds is `Unknown` rather than absent, and that is
/// the strict reading: `ControlStatusPolicy` permits `Unknown` only at Free, and
/// `AuthorityPolicy` needs a rule naming the unknown class. Defaulting to `Friendly`
/// or `Hostile` would each be a guess that changes a verdict.
fn classifier(state: &AppState) -> impl Fn(TrackId) -> Classification + Send + Sync + '_ {
    let tracks = state.tracking.tracks();
    move |id| {
        tracks
            .iter()
            .find(|t| t.id == id)
            .map_or(Classification::Unknown, |t| t.classification)
    }
}

/// Why the queue is empty, derived rather than assumed.
///
/// The order of the checks is the order of the causes: if no plan is being produced,
/// that is the reason, whatever policy has been doing; only when plans are flowing and
/// policy is refusing them all is the denial history the reason.
#[must_use]
pub fn queue_empty_reason(state: &AppState) -> EmptyBecause<'_> {
    if !state.health.tracking_healthy {
        return EmptyBecause::NoPlanProduced {
            because: "the tracking pipeline is not running, so there are no tracks \
                      to plan against",
        };
    }
    if !state.health.intercept_healthy {
        return EmptyBecause::NoPlanProduced {
            because: "the intercept service reports unhealthy; the plan on screen may \
                      be stale",
        };
    }
    if state.last_plan.is_empty() {
        return EmptyBecause::NoPlanProduced {
            because: "the allocator has produced no assignment from the current tracks \
                      and ready resources",
        };
    }
    match (&state.denials.last_reason, state.denials.count) {
        (Some(reason), count) if count > 0 => EmptyBecause::AllDenied { reason, count },
        _ => EmptyBecause::NothingPending,
    }
}

/// The queue as PN-06 draws it.
///
/// `rows` borrows nothing from the workflow because `ApprovalWorkflow::pending` returns
/// owned pairs; the caller owns the vector for the frame.
#[must_use]
pub fn queue_rows(state: &AppState) -> Vec<QueueRow<'_>> {
    let role_name = format!("{:?}", state.role());
    let classification = classifier(state);
    let may_decide = role_permits(state.role(), DECISION_ACTION);
    let now = state.clock.now();
    state
        .approvals
        .queue()
        .iter()
        .map(|item| QueueRow {
            id: PendingId(item.id.0),
            plan_id: item.plan.id.0,
            assignments: item.plan.assignments().len(),
            verdict: Verdict::RequiresHumanApproval,
            time_remaining: match item.time_remaining_s(now) {
                Some(s) => TimeRemaining::Seconds(s),
                // Not a blank and not this build's limitation: the deployment
                // configured no expiry for this layer, and DN-10 makes that mean the
                // item is preserved until somebody decides it.
                None => TimeRemaining::NoExpiryConfigured,
            },
            pre_delegated: is_pre_delegated(
                &state.config.policy.authority,
                DECISION_ACTION,
                &role_name,
                &item.plan,
                &state.resources,
                &classification,
            ),
            // Both the authorization and the offer have to hold: a role with
            // `DECIDE_PLAN` that this item was never offered to may not take it, and
            // escalation adds roles without removing the original (DN-10 §5).
            may_decide: may_decide && item.may_be_decided_by(&role_name),
            escalated_from: item.escalated_from.as_deref(),
        })
        .collect()
}

/// Build PN-06's view for this frame.
///
/// `handoffs` is every handoff the desktop has issued (`handoffs::rows`), not the subset
/// still owed: PN-06's *stays visible until delivered* rule is the panel's, so a filter
/// here could only ever weaken it (GAP-040).
#[must_use]
pub fn queue_view<'a>(
    state: &'a AppState,
    rows: &'a [QueueRow<'a>],
    handoffs: &'a [gungnir_ui::panels::handoff::HandoffRow<'a>],
    role_name: &'a str,
) -> ApprovalQueueView<'a> {
    ApprovalQueueView {
        rows,
        order: QueueOrder::TimeOnly { priority: PRIORITY },
        empty_because: queue_empty_reason(state),
        selected: state.selected_approval(),
        may_decide: role_permits(state.role(), DECISION_ACTION),
        role: role_name,
        handoffs,
        now: state.clock.now(),
    }
}

/// Record a decision and publish it.
///
/// `operator_id` is whoever is signed in, and `None` when nobody is (GAP-057, DN-23).
/// It stayed `None` unconditionally until the session authority existed, because naming
/// a role as though it were a person would put a false attribution into an append-only
/// record. That reasoning is unchanged -- what changed is that an operator who has
/// actually been verified can now be named. An expired session and an unreachable
/// account store both still yield `None`, and PN-07 says which.
pub fn decide(
    state: &mut AppState,
    id: PendingId,
    decision: OperatorDecision,
) -> Result<(), CommandError> {
    let now = state.clock.now();
    let operator = state.attributed_operator().map(|o| o.0.to_string());
    let record = state
        .approvals
        .decide(PendingApprovalId(id.0), decision, operator, now)?;
    tracing::info!(
        plan = record.plan.id.0,
        decision = ?record.decision,
        actionable = record.is_actionable(),
        "decision recorded"
    );
    let event = gungnir_eventing::Event::Command(record.to_event());
    if let Err(err) = state.events.publish(now, event) {
        tracing::error!(%err, "decision event publish failed");
    }
    // GAP-059: the decision is on the audit trail with the verified operator, or none.
    crate::audit::record(
        state,
        DECISION_ACTION,
        format!("plan {} {:?}", record.plan.id.0, record.decision),
    );
    // GAP-043: an actionable decision is the moment an engagement opens (DN-06 §5).
    let opened = crate::engagements::open_for(state, &record);
    tracing::debug!(decision = record.id.0, opened, "engagements opened");
    state.clear_selected_approval();
    Ok(())
}

/// Whether this role may override rather than only accept.
#[must_use]
pub fn may_override(role: Role) -> bool {
    role_permits(role, actions::OVERRIDE_PLAN)
}

/// The denial reason as PN-06 shows it. Kept next to the chain so a new
/// [`DenialReason`] cannot reach the screen as a debug-formatted enum by default.
#[must_use]
pub fn denial_sentence(reason: DenialReason) -> String {
    match reason {
        DenialReason::EmptyPlan => "the plan tasks nothing".to_owned(),
        DenialReason::NoGoGeofence => "an intercept point lies in a no-go area".to_owned(),
        DenialReason::InsufficientAuthority => "the asking role holds no authority".to_owned(),
        DenialReason::ResourceNotReady => "a tasked resource is not ready".to_owned(),
        DenialReason::UnknownResource => "the plan tasks a resource nobody knows".to_owned(),
        DenialReason::ControlStatus { layer, status } => {
            format!("{layer:?} is at {status:?}")
        }
        DenialReason::Authority { layer } => {
            format!("no authority rule covers {layer:?} for this role and class")
        }
        DenialReason::FiresDeconfliction => "a fires deconfliction check failed".to_owned(),
    }
}

/// The deconfliction checks in the words PN-05 uses (DN-05 §3).
#[must_use]
pub fn check_name(kind: gungnir_model::DeconflictionKind) -> &'static str {
    use gungnir_model::DeconflictionKind;
    match kind {
        DeconflictionKind::LocationAccuracy => "location accuracy",
        DeconflictionKind::FriendlyPosition => "friendly positions",
        DeconflictionKind::NoFireArea => "no-fire areas",
        DeconflictionKind::AirspaceMeasure => "airspace measures",
        DeconflictionKind::InterceptorTrajectory => "interceptor trajectories",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every denial reason has a sentence: a new variant must not reach an operator as
    /// a debug-formatted enum, because "denied" without a reason sends them to a radio
    /// to ask why (DN-09).
    #[test]
    fn every_denial_reason_reads_as_a_sentence() {
        use gungnir_model::{EffectorLayer, WeaponsControlStatus};
        for reason in [
            DenialReason::EmptyPlan,
            DenialReason::NoGoGeofence,
            DenialReason::InsufficientAuthority,
            DenialReason::ResourceNotReady,
            DenialReason::UnknownResource,
            DenialReason::ControlStatus {
                layer: EffectorLayer::Point,
                status: WeaponsControlStatus::Hold,
            },
            DenialReason::Authority {
                layer: EffectorLayer::Area,
            },
            DenialReason::FiresDeconfliction,
        ] {
            let s = denial_sentence(reason);
            assert!(!s.is_empty());
            assert!(
                !s.contains('{') && !s.starts_with("Denial"),
                "{reason:?} reached the screen as a debug format: {s}"
            );
        }
    }

    /// The chain says what it checked and admits what it could not.
    ///
    /// Both reasons the no-go check is vacuous have to be reported. Naming only the
    /// missing fences would tell an operator that configuring fences makes the check
    /// real, and it would not: the planner computes no intercept point to test.
    #[test]
    fn the_chain_names_its_engines_and_every_vacuous_check() {
        let report = chain_report();
        assert_eq!(report.engines.len(), 4, "{:?}", report.engines);
        assert!(report.no_geofences_configured);
        assert!(report.no_intercept_geometry);
        assert!(report.fires_sources_missing);

        let caveats = report.caveats();
        assert_eq!(
            caveats.len(),
            3,
            "every independent reason must be named, or fixing one would look like \
             fixing the check"
        );
        assert!(caveats.iter().any(|c| c.contains("GAP-031")));
        assert!(caveats.iter().any(|c| c.contains("geofence section")));
        assert!(caveats.iter().any(|c| c.contains("fires")));
    }

    /// Fixing either reason alone leaves the check vacuous, which is the whole point of
    /// reporting them separately.
    #[test]
    fn one_caveat_remains_when_only_one_cause_is_fixed() {
        let fences_configured = PolicyChainReport {
            engines: chain_report().engines,
            no_geofences_configured: false,
            no_intercept_geometry: true,
            fires_sources_missing: false,
        };
        assert_eq!(fences_configured.caveats().len(), 1);

        let sound = PolicyChainReport {
            engines: chain_report().engines,
            no_geofences_configured: false,
            no_intercept_geometry: false,
            fires_sources_missing: false,
        };
        assert!(
            sound.caveats().is_empty(),
            "with both causes fixed the check is real and must claim nothing"
        );
    }

    /// Accepting and overriding are different authorities. An operator holds the first
    /// and not the second, and a supervisor holds both.
    #[test]
    fn overriding_is_a_higher_authority_than_accepting() {
        assert!(role_permits(Role::Operator, DECISION_ACTION));
        assert!(!may_override(Role::Operator));
        assert!(role_permits(Role::Supervisor, DECISION_ACTION));
        assert!(may_override(Role::Supervisor));
        assert!(!role_permits(Role::Analyst, DECISION_ACTION));
    }
}
