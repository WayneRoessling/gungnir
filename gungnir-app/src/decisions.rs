// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The desktop's calls into the approval desk, and what PN-05, PN-06 and PN-07 draw from
//! it (GAP-038; GAP-131, D-57, `docs/design/DN-31-node-approval-queue.md` §3).
//!
//! **The decision path itself is `gungnir-approval`'s.** The policy chain, submission, the
//! sweep, deciding with engagement opening and the handoff builder moved there whole so a
//! node runs the same rules rather than a second copy of them (D-55, D-57); what is left
//! here is this binary's half of each call -- the context `desk.rs` builds, and the panel
//! state a decision touches -- and the presentation the library has no business holding.
//!
//! # What stays, and why
//!
//! A row, a sentence and a reason-the-queue-is-empty are all answers to "what does this
//! operator see", and they name `gungnir-ui` types, which a productization crate may not
//! reach. The alternatives and the what-if (GAP-032) stay for a different reason: they
//! judge plans nobody submitted, through the same chain the recommendation was held to
//! ([`gungnir_approval::with_chain`]), and commit to nothing.
//!
//! # The queue is empty, and why that is the interesting part
//!
//! No plan ever reaches the queue in this build. The tracking pipeline produces tracks,
//! but the allocator returns an empty plan, and an empty plan is denied by the first
//! engine that sees it. Every step of that is correct, and the result -- a permanently calm
//! approval queue -- is the most misleading screen in the product. [`queue_empty_reason`]
//! derives which of the three [`EmptyBecause`] situations is in force from the health flags
//! and the denial history, so PN-06 says *why* it is empty rather than only that it is.

use crate::state::AppState;
use gungnir_command::{CommandError, OperatorDecision, PendingApprovalId};
use gungnir_decision::CourseOfAction;
use gungnir_model::PlanView;
use gungnir_policy::{is_pre_delegated, DenialReason, PolicyVerdict};
use gungnir_security::authz::role_permits;
use gungnir_ui::panels::approval_queue::{
    ApprovalQueueView, EmptyBecause, PendingId, QueueOrder, QueueRow, TimeRemaining, Verdict,
};
use gungnir_ui::panels::unavailable::Unavailable;

/// The library's surface, re-exported so a caller of this module reaches one decision path
/// rather than choosing between two names for it (`agentic-coding-standards.md` §1.2).
pub use gungnir_approval::{
    chain_report, chain_report_for, escalation_ladder, may_override, PolicyChainReport, Submitted,
    CHAIN_ENGINES, DECISION_ACTION,
};

/// The priority tie-break in the queue ordering needs a threat score, and nothing
/// computes one (GAP-028). The queue is therefore ordered by time remaining alone, and
/// PN-06 says so rather than implying a severity order it does not have.
const PRIORITY: Unavailable<'static> = Unavailable {
    owner: "gungnir-assessment",
    gap: "GAP-028",
};

/// Windows that closed with nobody deciding, from the append-only history.
///
/// **Not rejections**: nobody chose.
#[must_use]
pub fn expired_count(state: &AppState) -> usize {
    state.desk.expired_count()
}

/// Evaluate a plan and queue it if it clears policy.
pub fn submit(state: &mut AppState, plan: PlanView) -> Submitted {
    crate::desk::with_policy(state, |desk, cx, policy, host| {
        desk.submit(cx, policy, host, plan)
    })
}

/// Apply expiry and escalation, publishing what happened. Called every tick.
pub fn sweep(state: &mut AppState) {
    crate::desk::with_desk(state, |desk, cx, host| desk.sweep(cx, host));
}

/// Record a decision and publish it, and close PN-07 on the item it decided.
///
/// **Authorized here, not only drawn** (GAP-127). Until 2026-09-17 this named
/// `plan.decide` on the audit record and checked nothing: PN-06 hid the controls a role
/// may not use, so the rule held for a person at the screen and for nothing else that
/// calls this. It now asks the two questions the node's route asks, against
/// [`AppState::role`] -- the signed-in account's role whenever there is one:
///
/// 1. whether the role holds the permission the decision needs -- `plan.override` for an
///    override, which Operator does not hold, and `plan.decide` otherwise;
/// 2. whether the item was offered to that role, or escalated to it (DN-10 §5).
///
/// A refusal records nothing and publishes nothing.
///
/// # Errors
///
/// `CommandError::NotPermitted` or `CommandError::NotOffered` when the role may not take
/// this decision; `CommandError::NotFound` when the item has left the queue -- decided by
/// somebody else, or expired -- which PN-07 reports rather than retrying.
pub fn decide(
    state: &mut AppState,
    id: PendingId,
    decision: OperatorDecision,
) -> Result<(), CommandError> {
    use gungnir_command::ApprovalWorkflow;
    use gungnir_security::actions::{DECIDE_PLAN, OVERRIDE_PLAN};
    let role = state.role();
    let action = match decision {
        OperatorDecision::Overridden => OVERRIDE_PLAN,
        _ => DECIDE_PLAN,
    };
    if !role_permits(role, action) {
        return Err(CommandError::NotPermitted {
            role: format!("{role:?}"),
            action,
        });
    }
    let role_name = format!("{role:?}");
    // An item the queue does not hold goes on to the desk, which is the one place that
    // can say it was decided or expired; there is no offer to check on an item that is
    // not there.
    if let Some(item) = state
        .desk
        .approvals
        .queue()
        .iter()
        .find(|item| item.id == PendingApprovalId(id.0))
    {
        if !item.may_be_decided_by(&role_name) {
            return Err(CommandError::NotOffered {
                item: item.id,
                role: role_name,
                offered_to: item.offered_to.clone(),
            });
        }
    }
    crate::desk::with_desk(state, |desk, cx, host| {
        desk.decide(cx, host, PendingApprovalId(id.0), decision)
    })?;
    // The panel state is this binary's, not the desk's: the dialog is open on an item that
    // has just left the queue.
    state.clear_selected_approval();
    Ok(())
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
/// Continuation-passing for the same reason as [`gungnir_approval::with_chain`], which it
/// wraps: every field of `PlanAlternatives` is a shared borrow of something built here --
/// the policy chain, the fresh planner, the ranking and the assessor -- so a value returned
/// from this function would outlive them. Going through the library's chain builder rather
/// than assembling four engines here is what keeps an alternative comparable with the
/// recommendation: it is judged by the chain the plan in force was judged by.
fn with_support<T>(
    state: &AppState,
    f: impl FnOnce(&mut gungnir_decision::PlanAlternatives<'_>) -> T,
) -> T {
    let planner = snapshot_planner(state);
    let ranking = crate::sustainment::asset_exposure(state);
    let assessor = crate::sustainment::asset_assessor(state);
    crate::desk::with_context(state, |cx, policy| {
        gungnir_approval::with_chain(cx, policy, |chain| {
            let mut support = gungnir_decision::PlanAlternatives {
                now: cx.now,
                tracks: cx.tracks,
                resources: cx.resources,
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

/// What became of the plan on this tick, as the decision support reads it (GAP-133).
///
/// Separate from [`Submitted`] because the support is refreshed on **both** paths and
/// only one of them submits anything: while a desktop is linked the node holds the queue
/// and this desktop submits nothing (DN-31 §6.5), and a trigger keyed on a submission
/// would have left PN-05's options blank on every linked desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanMoved {
    /// A fresh plan was proposed, wherever it was queued.
    Proposed,
    /// A fresh plan was proposed and this desktop's own queue refused it as superseded
    /// (DN-08 §5). Only the cut-off path can say this; the node applies no baseline
    /// while it runs, so nothing it proposes is superseded.
    Superseded,
    /// The plan did not change.
    Unchanged,
}

impl PlanMoved {
    /// What the tick saw: whether the plan changed, and what this desktop's own queue
    /// made of it where it was the one that took it.
    #[must_use]
    pub fn of(plan_changed: bool, submitted: Option<&Submitted>) -> Self {
        match (plan_changed, submitted) {
            (_, Some(Submitted::Superseded)) => PlanMoved::Superseded,
            (true, _) => PlanMoved::Proposed,
            (false, _) => PlanMoved::Unchanged,
        }
    }
}

/// Refresh the decision support PN-05 draws (GAP-032).
///
/// `moved` is what became of the plan this frame. The two halves are refreshed on
/// different triggers because they go stale for different reasons, and each one costs an
/// allocator solve:
///
/// - the **alternatives** are alternatives to a particular plan, so they are regenerated
///   when a plan is proposed and cleared when one is superseded -- a superseded plan was
///   never evaluated, and options nobody could act on are worse than none;
/// - the **rehearsal** answers a question the operator posed by selecting a track, so it
///   is recomputed when the selection moves and when the plan under it has been replaced.
pub fn refresh_support(state: &mut AppState, moved: PlanMoved) {
    match moved {
        PlanMoved::Proposed => {
            let courses = alternatives(state, MAX_ALTERNATIVES);
            state.alternatives = courses;
            // The held rehearsal was computed against the plan just replaced, so it is
            // stale whatever the selection is.
            state.what_if_for = None;
        }
        PlanMoved::Superseded => {
            state.alternatives = Vec::new();
            state.what_if_for = None;
        }
        PlanMoved::Unchanged => {}
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
    match (&state.desk.denials.last_reason, state.desk.denials.count) {
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
    use gungnir_command::ApprovalWorkflow;
    let role_name = format!("{:?}", state.role());
    let tracks = state.tracking.tracks();
    let classification = |id| {
        tracks
            .iter()
            .find(|t| t.id == id)
            .map_or(gungnir_model::Classification::Unknown, |t| t.classification)
    };
    let may_decide = role_permits(state.role(), DECISION_ACTION);
    let now = state.clock.now();
    // The matrix the queue was offered under, with D-15's delegations as they stand now
    // (GAP-134): a delegation that has lapsed is not drawn as though it still stood.
    let authority = gungnir_policy::authority_in_force(
        &state.config.policy.authority,
        crate::failover::delegations(state),
    );
    state
        .desk
        .approvals
        .queue()
        .iter()
        .map(|item| QueueRow {
            id: PendingId(item.id.0),
            plan_id: item.plan.id,
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
                &authority,
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
            // Named so a row this role may not decide says who may (DN-31 §8). The
            // queue's own list, in escalation order, not a second derivation of it.
            offered_to: &item.offered_to,
            escalated_from: item.escalated_from.as_deref(),
        })
        .collect()
}

/// Build PN-06's view for this frame.
///
/// `handoffs` is every handoff the desktop has issued (`handoffs::rows`), not the subset
/// still owed: PN-06's *stays visible until delivered* rule is the panel's, so a filter
/// here could only ever weaken it (GAP-040).
///
/// `rows` and `decided` are the caller's because they come from two different places
/// depending on who holds the queue (GAP-133, DN-31 §6.6); everything else about the view
/// is the same question either way.
#[must_use]
pub fn queue_view<'a>(
    state: &'a AppState,
    rows: &'a [QueueRow<'a>],
    handoffs: &'a [gungnir_ui::panels::handoff::HandoffRow<'a>],
    role_name: &'a str,
    decided: &'a [gungnir_ui::panels::approval_queue::DecidedRow<'a>],
) -> ApprovalQueueView<'a> {
    let node = crate::projection::node_holds_the_queue(state);
    ApprovalQueueView {
        rows,
        order: QueueOrder::TimeOnly { priority: PRIORITY },
        empty_because: if node {
            node_queue_empty_reason(state)
        } else {
            queue_empty_reason(state)
        },
        selected: state.selected_approval(),
        // Whether this console may decide at all. While the node holds the queue the
        // route needs a token, so nobody signed in means nothing is actionable however
        // the desktop's selected role is set (DN-31 §6.6, DN-23 §5 rule 5).
        may_decide: if node {
            state
                .signed_in()
                .is_some_and(|s| role_permits(s.role, DECISION_ACTION))
        } else {
            role_permits(state.role(), DECISION_ACTION)
        },
        role: role_name,
        handoffs,
        now: state.clock.now(),
        authority: crate::projection::authority(state),
        decided,
        cannot_decide: (node && state.signed_in().is_none()).then_some(
            "Nobody is signed in. The node authorizes every decision against the \
             caller's role, so no decision can be taken from this console until \
             somebody signs in -- this queue is read-only until then.",
        ),
    }
}

/// Why the node's queue is showing nothing (GAP-133, DN-31 §6.6).
///
/// The desktop's own reasons do not apply: its planner, its allocator and its denial
/// history say nothing about what a node is waiting for. The two answers this desktop can
/// honestly give are "the node has not told me" and "the node is waiting on nothing", and
/// they are drawn differently.
#[must_use]
fn node_queue_empty_reason(state: &AppState) -> EmptyBecause<'_> {
    match (&state.backend, state.projection.queue.waiting.is_some()) {
        (gungnir_config::BackendConfig::Remote { endpoint }, false) => {
            EmptyBecause::NotReceived { from: endpoint }
        }
        _ => EmptyBecause::NothingPending,
    }
}

/// The denial reason as PN-06 shows it. Kept next to the panels rather than beside the
/// chain so a new [`DenialReason`] cannot reach the screen as a debug-formatted enum by
/// default.
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
}
