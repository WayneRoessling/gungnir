// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Queue ordering, expiry, and escalation for pending decisions.
//!
//! Design: docs/design/DN-10-queue-expiry-and-escalation.md, **signed by the owner
//! on 2026-09-05**. Capabilities CAP-3.6 and CAP-3.7; mission thread MT-01 under
//! saturation.
//!
//! **Human-owned** (docs/agentic-workflow.md): this file changes what happens to a
//! decision nobody takes.
//!
//! Wired into [`crate::InMemoryApprovalWorkflow`] under GAP-034 and GAP-035, **signed by
//! the owner on 2026-09-05**. Until then every function here was correct, fully tested
//! and called by nothing, and connecting them found two places where the code did not
//! match §5 of the design note: [`PendingApproval::is_due_for_escalation`] bounded
//! escalation at once *ever* rather than once *per rank step*, and a single
//! `escalated_from` could not express "both roles see it". Each carries its own note
//! below. A module that is fully tested and entirely uncalled is not verified; it is
//! only self-consistent.
//!
//! The rule that matters most, and the one this module exists to make
//! unbreakable: **no configuration causes an expiry to accept.** There is no
//! setting for it and no code path to it. An expiry that accepts is an action
//! without a human decision, which is contract C-01, and C-01 is not dispensable.
//! [`tests::no_settings_can_make_an_expiry_accept`] searches the settings space for
//! any such path.
//!
//! The second rule is the deliberate asymmetry with authority: **silence about
//! expiry preserves.** A layer with no configured expiry never expires, because
//! silently discarding a decision nobody took would lose information. The queue
//! shows which items have no expiry and why.

use crate::{DecisionRecord, PendingApprovalId};
use gungnir_model::{DecisionSettings, EffectorLayer, MissionTime, PlanView, ResourceView};
use gungnir_policy::PolicyVerdict;

/// The queue's own view of a pending item, which is what orders it.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingApproval {
    pub id: PendingApprovalId,
    pub plan: PlanView,
    pub verdict: PolicyVerdict,
    pub submitted: MissionTime,
    /// The layer this plan engages at, which selects its expiry and escalation.
    pub layer: EffectorLayer,
    /// From the policy settings; `None` means no expiry is configured, which is
    /// shown rather than guessed.
    pub expires_at: Option<MissionTime>,
    pub escalate_at: Option<MissionTime>,
    /// The role this item was escalated from at the most recent step, when it has
    /// been escalated.
    pub escalated_from: Option<String>,
    /// Every role this item is currently offered to, in escalation order.
    ///
    /// A list rather than a single role because DN-10 §5 is explicit that escalation
    /// **does not remove the original**: "an operator who is about to decide should not
    /// have the item vanish. Both roles see it; whoever decides first ends it." A single
    /// current-role field cannot say that, and the difference is an operator losing an
    /// item mid-decision.
    pub offered_to: Vec<String>,
    /// Highest risk score among the plan's tracks, for ordering.
    pub priority: f32,
}

impl PendingApproval {
    /// Seconds remaining before expiry; `None` when nothing will expire it.
    pub fn time_remaining_s(&self, now: MissionTime) -> Option<f64> {
        self.expires_at.map(|at| at.seconds_since(now))
    }

    pub fn has_expired(&self, now: MissionTime) -> bool {
        self.expires_at.is_some_and(|at| now >= at)
    }

    /// Whether this item is due to be offered one rank higher.
    ///
    /// The bound DN-10 §5 sets is "at most once **per rank step**", and it comes from
    /// the escalation clock rather than from `escalated_from`: each step resets
    /// `escalate_at`, and reaching the top of the ladder clears it, so an item cannot
    /// loop and cannot escalate twice within one step. Keying on `escalated_from` being
    /// unset -- which this did before GAP-034 wired the module up -- bounded it at once
    /// *ever*, which is a different and stricter rule than the design states.
    pub fn is_due_for_escalation(&self, now: MissionTime) -> bool {
        self.escalate_at.is_some_and(|at| now >= at)
    }

    /// Whether this role may take this decision.
    ///
    /// True for the role it was submitted to and for every role it has since been
    /// escalated to, which is what "both roles see it" means in code.
    pub fn may_be_decided_by(&self, role: &str) -> bool {
        self.offered_to.iter().any(|r| r == role)
    }

    /// The role the item currently sits with, which is the last one it was offered to.
    pub fn current_role(&self) -> Option<&str> {
        self.offered_to.last().map(String::as_str)
    }

    /// Why this item has no expiry, for the panel.
    pub fn no_expiry_reason(&self) -> Option<&'static str> {
        self.expires_at
            .is_none()
            .then_some("no expiry configured for this layer")
    }
}

/// What ended a pending approval, beyond the two an operator chooses.
///
/// `Expired` and `Escalated` are separate outcomes rather than shades of
/// `Rejected`: an expiry and a rejection mean different things to an after-action
/// review, and conflating them would corrupt MOE-01.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum QueueOutcome {
    /// The window closed with nobody deciding. Not a rejection: nobody chose.
    Expired { at: MissionTime },
    /// Offered to a role with the authority or the attention to take it.
    Escalated { to_role: String, at: MissionTime },
}

/// The layer whose deadline governs this plan.
///
/// A plan may task several layers, and `DecisionSettings` sets expiry per layer, so one
/// of them has to decide the window. It is the layer that closes **first**: a plan is
/// only decidable while every engagement it proposes is still open, so a plan touching
/// a 30 s Point layer and a 300 s Area layer has 30 s, not 300. Taking the longest would
/// let the Point window close while the item still sat in the queue looking live.
///
/// A layer with no configured expiry never closes (DN-08's silence-preserves default),
/// so it loses to any layer that does have one. When no tasked layer has an expiry, the
/// first tasked layer is returned and the item simply never expires.
///
/// `None` means the plan tasks nothing this deployment knows about, which is a plan
/// policy will have denied before it could reach a queue.
pub fn governing_layer(
    plan: &PlanView,
    resources: &[ResourceView],
    settings: &DecisionSettings,
) -> Option<EffectorLayer> {
    let mut best: Option<(EffectorLayer, Option<f64>)> = None;
    for (resource_id, _) in plan.assignments() {
        let Some(resource) = resources.iter().find(|r| r.id == resource_id) else {
            continue;
        };
        let expiry = settings.expiry_for(resource.layer);
        best = Some(match best {
            None => (resource.layer, expiry),
            Some((_, None)) if expiry.is_some() => (resource.layer, expiry),
            Some((_, Some(best_s))) if expiry.is_some_and(|e| e < best_s) => {
                (resource.layer, expiry)
            }
            Some(unchanged) => unchanged,
        });
    }
    best.map(|(layer, _)| layer)
}

/// Deadlines for one item, from the policy settings.
///
/// Returns `(expires_at, escalate_at)`; either is `None` when the layer has no
/// setting for it.
pub fn deadlines(
    settings: &DecisionSettings,
    layer: EffectorLayer,
    submitted: MissionTime,
) -> (Option<MissionTime>, Option<MissionTime>) {
    let expires_at = settings
        .expiry_for(layer)
        .map(|s| MissionTime(submitted.0 + s));
    let escalate_at = settings
        .escalate_after_for(layer)
        .map(|s| MissionTime(submitted.0 + s));
    (expires_at, escalate_at)
}

/// Orders the queue: time remaining ascending, then priority descending.
///
/// Time pressure outranks severity, because a high-priority item with two minutes
/// left can wait behind a lower-priority one with ten seconds left, and the reverse
/// loses both. Items with no expiry sort after every item that has one, and the
/// panel says why.
pub fn order_queue(queue: &mut [PendingApproval], now: MissionTime) {
    queue.sort_by(|a, b| {
        let ta = a.time_remaining_s(now);
        let tb = b.time_remaining_s(now);
        match (ta, tb) {
            (Some(x), Some(y)) => x
                .partial_cmp(&y)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    b.priority
                        .partial_cmp(&a.priority)
                        .unwrap_or(std::cmp::Ordering::Equal)
                }),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => b
                .priority
                .partial_cmp(&a.priority)
                .unwrap_or(std::cmp::Ordering::Equal),
        }
    });
}

/// The items whose window has closed, and those due to be offered upward.
///
/// Returns `(expired, to_escalate)` without changing anything: the caller records
/// the outcomes, so nothing here can end an item silently.
pub fn due(
    queue: &[PendingApproval],
    now: MissionTime,
) -> (Vec<PendingApprovalId>, Vec<PendingApprovalId>) {
    let expired = queue
        .iter()
        .filter(|p| p.has_expired(now))
        .map(|p| p.id)
        .collect();
    let escalate = queue
        .iter()
        .filter(|p| !p.has_expired(now) && p.is_due_for_escalation(now))
        .map(|p| p.id)
        .collect();
    (expired, escalate)
}

/// The record an expiry leaves behind.
///
/// `operator_id` is `None`, and that is correct rather than a gap: nobody decided,
/// and recording a false operator would be worse than a null. Since the DN-10 §3
/// conformance the *decision* also says so -- `OperatorDecision::Expired` -- so the
/// distinction no longer rests on a reader knowing that a missing operator means a
/// timeout.
pub fn expiry_record(
    item: &PendingApproval,
    id: gungnir_model::DecisionId,
    at: MissionTime,
) -> DecisionRecord {
    DecisionRecord {
        id,
        plan: item.plan.clone(),
        verdict: item.verdict,
        decision: crate::OperatorDecision::Expired { at },
        operator_id: None,
        mission_time: at,
    }
}

/// The next role an item escalates to, or `None` when it is already at the top.
///
/// Escalation is bounded: an item escalates at most once per rank step and stops at
/// the highest role with authority for the action. It cannot loop.
pub fn next_role<'a>(ladder: &[&'a str], current: &str) -> Option<&'a str> {
    let at = ladder.iter().position(|r| *r == current)?;
    ladder.get(at + 1).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::PlanId;

    fn item(
        id: u64,
        layer: EffectorLayer,
        expiry_s: Option<f64>,
        priority: f32,
    ) -> PendingApproval {
        let submitted = MissionTime(0.0);
        PendingApproval {
            id: PendingApprovalId(id),
            plan: PlanView {
                id: PlanId(id),
                mission_time: submitted,
                ..PlanView::default()
            },
            verdict: PolicyVerdict::RequiresHumanApproval,
            submitted,
            layer,
            expires_at: expiry_s.map(MissionTime),
            escalate_at: None,
            escalated_from: None,
            offered_to: vec!["operator".to_owned()],
            priority,
        }
    }

    #[test]
    fn deadlines_come_from_the_layer_and_absent_settings_mean_no_deadline() {
        let mut s = DecisionSettings::default();
        let (expiry, escalate) = deadlines(&s, EffectorLayer::Point, MissionTime(100.0));
        assert!(expiry.is_none(), "silence about expiry preserves");
        assert!(escalate.is_none());

        s.expiry_s.insert(EffectorLayer::Point, 30.0);
        s.escalate_after_s.insert(EffectorLayer::Point, 20.0);
        let (expiry, escalate) = deadlines(&s, EffectorLayer::Point, MissionTime(100.0));
        assert_eq!(expiry, Some(MissionTime(130.0)));
        assert_eq!(escalate, Some(MissionTime(120.0)));
    }

    #[test]
    fn the_queue_orders_by_time_remaining_then_priority() {
        let mut queue = vec![
            item(1, EffectorLayer::Area, Some(120.0), 0.9),
            item(2, EffectorLayer::Point, Some(10.0), 0.1),
            item(3, EffectorLayer::Point, Some(10.0), 0.8),
        ];
        order_queue(&mut queue, MissionTime(0.0));
        assert_eq!(
            queue.iter().map(|p| p.id.0).collect::<Vec<_>>(),
            vec![3, 2, 1],
            "ten seconds left beats two minutes, and priority breaks the tie"
        );
    }

    #[test]
    fn items_with_no_expiry_sort_last_and_say_why() {
        let mut queue = vec![
            item(1, EffectorLayer::Area, None, 0.99),
            item(2, EffectorLayer::Point, Some(300.0), 0.01),
        ];
        order_queue(&mut queue, MissionTime(0.0));
        assert_eq!(queue[0].id.0, 2, "an expiring item comes first");
        assert!(queue[1].no_expiry_reason().is_some());
        assert!(queue[0].no_expiry_reason().is_none());
    }

    #[test]
    fn an_expired_item_is_reported_but_not_removed_by_this_module() {
        let queue = vec![item(1, EffectorLayer::Point, Some(10.0), 0.5)];
        let (expired, escalate) = due(&queue, MissionTime(11.0));
        assert_eq!(expired, vec![PendingApprovalId(1)]);
        assert!(escalate.is_empty());
        // Nothing was mutated: the caller records the outcome, so no item can end
        // silently here.
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn an_item_with_no_expiry_never_expires() {
        let queue = vec![item(1, EffectorLayer::Point, None, 0.5)];
        let (expired, _) = due(&queue, MissionTime(1_000_000.0));
        assert!(expired.is_empty());
    }

    #[test]
    fn an_expiry_record_names_no_operator() {
        let it = item(1, EffectorLayer::Point, Some(10.0), 0.5);
        let record = expiry_record(&it, gungnir_model::DecisionId(1), MissionTime(10.0));
        assert!(
            record.operator_id.is_none(),
            "nobody decided; a false operator would be worse than a null"
        );
        assert!(
            !record.is_actionable(),
            "an expiry never becomes actionable"
        );
    }

    #[test]
    fn no_settings_can_make_an_expiry_accept() {
        // Contract C-01. Search the settings space for any path that produces an
        // actionable record from an expiry. This test exists because this is the
        // module where such a path would most plausibly be added later.
        let layers = [
            EffectorLayer::Area,
            EffectorLayer::Point,
            EffectorLayer::SelfDefence,
            EffectorLayer::NonKinetic,
        ];
        for layer in layers {
            for expiry in [0.0_f64, 1.0, 30.0, 3_600.0] {
                for escalate in [None, Some(0.5_f64), Some(29.0)] {
                    let mut settings = DecisionSettings::default();
                    settings.expiry_s.insert(layer, expiry);
                    if let Some(e) = escalate {
                        settings.escalate_after_s.insert(layer, e);
                    }
                    let (expires_at, escalate_at) = deadlines(&settings, layer, MissionTime(0.0));
                    let mut it = item(1, layer, None, 0.5);
                    it.expires_at = expires_at;
                    it.escalate_at = escalate_at;
                    let record =
                        expiry_record(&it, gungnir_model::DecisionId(1), MissionTime(expiry));
                    assert!(
                        !record.is_actionable(),
                        "an expiry became actionable at {layer:?} with expiry {expiry}"
                    );
                    assert!(record.operator_id.is_none());
                }
            }
        }
    }

    #[test]
    fn escalation_is_offered_once_and_stops_at_the_top() {
        let ladder = ["operator", "supervisor", "commander"];
        assert_eq!(next_role(&ladder, "operator"), Some("supervisor"));
        assert_eq!(next_role(&ladder, "supervisor"), Some("commander"));
        assert_eq!(next_role(&ladder, "commander"), None, "it cannot loop");
        assert_eq!(next_role(&ladder, "analyst"), None, "not on the ladder");
    }

    /// DN-10 §5 bounds escalation at once **per rank step**, not once ever. The clock
    /// is the bound: escalating resets it, and reaching the top clears it.
    ///
    /// Before GAP-034 wired this module up, `is_due_for_escalation` also required
    /// `escalated_from` to be unset, which bounded it at once in the item's whole life.
    /// Nothing depended on that, because nothing called the function.
    #[test]
    fn escalation_is_bounded_per_rank_step_by_the_clock() {
        let mut it = item(1, EffectorLayer::Area, Some(300.0), 0.5);
        it.escalate_at = Some(MissionTime(10.0));
        assert!(it.is_due_for_escalation(MissionTime(11.0)));

        // One step taken: the clock is reset, so it is not due again at the same instant.
        it.escalated_from = Some("operator".into());
        it.offered_to.push("supervisor".into());
        it.escalate_at = Some(MissionTime(20.0));
        assert!(
            !it.is_due_for_escalation(MissionTime(11.0)),
            "an item escalated at t=11 must not escalate again at t=11"
        );
        assert!(
            it.is_due_for_escalation(MissionTime(21.0)),
            "the next rank step is due when its own clock runs out"
        );

        // At the top of the ladder the clock is cleared and it stops asking.
        it.escalate_at = None;
        assert!(!it.is_due_for_escalation(MissionTime(1_000.0)));
    }

    /// Escalation adds a role without removing the original, so an operator who is
    /// about to decide does not have the item vanish (DN-10 §5).
    #[test]
    fn escalation_adds_a_role_without_removing_the_original() {
        let mut it = item(1, EffectorLayer::Area, Some(300.0), 0.5);
        assert!(it.may_be_decided_by("operator"));
        assert!(!it.may_be_decided_by("supervisor"));

        it.offered_to.push("supervisor".into());
        assert!(
            it.may_be_decided_by("operator"),
            "the original role lost sight of an item it was deciding"
        );
        assert!(it.may_be_decided_by("supervisor"));
        assert_eq!(it.current_role(), Some("supervisor"));
    }

    /// The governing layer is the one that closes first, because a plan is only
    /// decidable while every engagement it proposes is still open.
    #[test]
    fn the_governing_layer_is_the_one_that_closes_first() {
        use gungnir_model::{Geodetic, InterceptSolutionView, RelativeCost, ResourceId};

        fn resource(id: u32, layer: EffectorLayer) -> ResourceView {
            ResourceView {
                id: ResourceId(id),
                position: Geodetic {
                    lat_rad: 0.0,
                    lon_rad: 0.0,
                    alt_m: 0.0,
                },
                capacity: 1,
                ready: true,
                layer,
                cost: RelativeCost::default(),
                magazine: None,
                intercept_speed_mps: None,
            }
        }
        let resources = [
            resource(1, EffectorLayer::Area),
            resource(2, EffectorLayer::Point),
        ];
        let two_layer = PlanView {
            id: PlanId(1),
            kind: gungnir_model::PlanKind::Intercept {
                solutions: vec![
                    InterceptSolutionView {
                        resource: ResourceId(1),
                        track: gungnir_model::TrackId(1),
                        intercept_point: None,
                        time_to_intercept_s: None,
                    },
                    InterceptSolutionView {
                        resource: ResourceId(2),
                        track: gungnir_model::TrackId(2),
                        intercept_point: None,
                        time_to_intercept_s: None,
                    },
                ],
            },
            ..PlanView::default()
        };

        let mut settings = DecisionSettings::default();
        settings.expiry_s.insert(EffectorLayer::Area, 300.0);
        settings.expiry_s.insert(EffectorLayer::Point, 30.0);
        assert_eq!(
            governing_layer(&two_layer, &resources, &settings),
            Some(EffectorLayer::Point),
            "the 300 s window would have let the 30 s one close unnoticed"
        );

        // A layer with no expiry never closes, so it loses to one that does.
        let mut only_area = DecisionSettings::default();
        only_area.expiry_s.insert(EffectorLayer::Area, 300.0);
        assert_eq!(
            governing_layer(&two_layer, &resources, &only_area),
            Some(EffectorLayer::Area)
        );

        // Nothing configured anywhere: any tasked layer will do, and the item never
        // expires either way.
        assert!(governing_layer(&two_layer, &resources, &DecisionSettings::default()).is_some());
        assert!(governing_layer(&PlanView::default(), &resources, &settings).is_none());
    }

    #[test]
    fn an_expired_item_is_not_also_escalated() {
        let mut it = item(1, EffectorLayer::Area, Some(10.0), 0.5);
        it.escalate_at = Some(MissionTime(5.0));
        let (expired, escalate) = due(&[it], MissionTime(20.0));
        assert_eq!(expired.len(), 1);
        assert!(
            escalate.is_empty(),
            "an item that is gone is not offered up"
        );
    }

    #[test]
    fn time_remaining_is_reported_for_the_countdown() {
        let it = item(1, EffectorLayer::Point, Some(30.0), 0.5);
        let remaining = it
            .time_remaining_s(MissionTime(10.0))
            .expect("has an expiry");
        assert!((remaining - 20.0).abs() < f64::EPSILON);
        assert!(item(2, EffectorLayer::Point, None, 0.5)
            .time_remaining_s(MissionTime(10.0))
            .is_none());
    }
}
