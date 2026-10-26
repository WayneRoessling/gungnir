// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A live weapons-control-status change, applied to the queue (GAP-203; DN-09 §10;
//! D-140).
//!
//! The host has already put the layer at its new status in the settings `cx.config`
//! carries, and has journaled nothing and audited nothing: this is where both happen,
//! beside the queue's answer to the change, so the record, the audit entry and the
//! withdrawals carry one mission time and cannot land without each other.
//!
//! **What a tightening does to the queue.** Every queued item is re-judged by the
//! control-status engine alone, and one the new status refuses leaves the queue as
//! [`gungnir_command::OperatorDecision::Withdrawn`]: not a rejection, not an expiry, and
//! said on the record as the status change that ended it. A loosening resurrects nothing;
//! the next plan the allocator proposes is judged under the new status, which is the
//! owner's choice of 2026-10-26.
//!
//! **Why the engine alone and not the chain.** A node re-judges its queue with nobody at
//! a console, and the authority engine would refuse every item for that reason rather
//! than for the status; the readiness and fires engines answer questions the change did
//! not ask. The engine is `gungnir-policy`'s own, so what withdraws an item is exactly
//! what would have refused it had it been proposed under the new status.

use crate::{ApprovalContext, ApprovalDesk, ApprovalHost};
use gungnir_command::ApprovalWorkflow;
use gungnir_eventing::Event;
use gungnir_model::events::PolicyEvent;
use gungnir_model::{
    ControlStatusChange, EffectorLayer, PendingApprovalId, PlanId, PlanView, WeaponsControlStatus,
};
use gungnir_policy::{DenialReason, PolicyVerdict};
use gungnir_security::actions::SET_CONTROL_STATUS;

/// A queue item a status change ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Withdrawn {
    pub item: PendingApprovalId,
    pub plan: PlanId,
    /// The layer whose new status refused the item.
    pub layer: EffectorLayer,
    pub status: WeaponsControlStatus,
}

impl ApprovalDesk {
    /// Record `change`, which the host has already applied to the settings `cx.config`
    /// carries, and withdraw every queued item the new status refuses.
    ///
    /// Publishes `PolicyEvent::ControlStatusSet` and one audit entry under
    /// `weapons.control_status`, then `CommandEvent::Withdrawn` per item withdrawn, each
    /// with an alert, and returns what left the queue so the host can answer a caller.
    ///
    /// The host's `audit` attributes the entry to the operator it has verified, which is
    /// the person the change names; the detail names the second person where D-140's
    /// rule was in force.
    pub fn set_control_status(
        &mut self,
        cx: &ApprovalContext<'_>,
        host: &mut dyn ApprovalHost,
        change: ControlStatusChange,
    ) -> Vec<Withdrawn> {
        debug_assert_eq!(
            cx.config.policy.control_status.for_layer(change.layer),
            change.status,
            "a status change was recorded before the host applied it"
        );
        let detail = audit_detail(&change);
        host.publish(cx.now, Event::Policy(PolicyEvent::ControlStatusSet(change)));
        host.audit(SET_CONTROL_STATUS, detail);

        // Snapshot the queue first: withdrawing mutates it, and the workflow's borrow
        // cannot outlive the loop that withdraws.
        let pending: Vec<(PendingApprovalId, PlanView)> = self
            .approvals
            .pending()
            .into_iter()
            .map(|(id, plan)| (id, plan.clone()))
            .collect();
        let mut withdrawn = Vec::new();
        for (id, plan) in pending {
            let PolicyVerdict::Denied {
                reason_code: DenialReason::ControlStatus { layer, status },
            } = crate::chain::control_status_verdict(cx, &plan)
            else {
                continue;
            };
            match self.approvals.withdraw(id, layer, status, cx.now) {
                Ok(record) => {
                    host.publish(cx.now, Event::Command(record.to_event()));
                    host.alert(format!(
                        "Plan {} withdrawn from the queue: the {layer:?} layer is at {status:?}",
                        plan.id.short()
                    ));
                    self.queue_outcomes.withdrawn += 1;
                    withdrawn.push(Withdrawn {
                        item: id,
                        plan: plan.id,
                        layer,
                        status,
                    });
                }
                // The item was in the snapshot a moment ago; nothing else withdraws
                // between the snapshot and here. Said loudly rather than ignored.
                Err(err) => {
                    tracing::error!(%err, item = ?id, "a queued item could not be withdrawn");
                }
            }
        }
        withdrawn
    }
}

/// The audit entry's detail: what changed, from what, under which baseline, by whom, and
/// why. The operator is the entry's own attribution; the detail repeats the role and
/// names the second person, because an entry is read on its own.
fn audit_detail(change: &ControlStatusChange) -> String {
    let confirmed = change.confirmed_by.as_ref().map_or_else(String::new, |c| {
        format!(", confirmed by operator {} ({})", c.operator, c.role)
    });
    let origin = change
        .origin
        .as_deref()
        .map_or_else(String::new, |o| format!(" at {o}"));
    format!(
        "weapons control status of the {:?} layer set to {:?} (was {:?}) under baseline \
         revision {} by operator {} ({}){confirmed}{origin}: {}",
        change.layer,
        change.status,
        change.previous,
        change.revision,
        change.operator,
        change.role,
        change.reason
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::{ConfirmingOperator, MissionTime};

    #[test]
    fn the_audit_detail_names_what_a_reader_needs_on_its_own() {
        let change = ControlStatusChange {
            layer: EffectorLayer::Area,
            status: WeaponsControlStatus::Hold,
            previous: WeaponsControlStatus::Tight,
            reason: "deconfliction with friendly air".into(),
            operator: "7".into(),
            role: "Supervisor".into(),
            confirmed_by: Some(ConfirmingOperator {
                operator: "9".into(),
                role: "Commander".into(),
            }),
            revision: 4,
            origin: Some("desktop-1".into()),
            at: MissionTime(10.0),
            request: None,
        };
        let detail = audit_detail(&change);
        for needle in [
            "Area layer set to Hold (was Tight)",
            "revision 4",
            "operator 7 (Supervisor)",
            "confirmed by operator 9 (Commander)",
            "at desktop-1",
            "deconfliction with friendly air",
        ] {
            assert!(detail.contains(needle), "{detail} lacks {needle:?}");
        }
    }
}
