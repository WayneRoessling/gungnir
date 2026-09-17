// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! DN-31 §9 row 1, the queue's half (GAP-130; D-56): "No two identifiers equal across
//! machines and restarts".
//!
//! The counters these identifiers replaced started at 1 in every workflow, so a node's
//! queue and two desktops' queues all handed out item 1 and decision 1, and a desktop that
//! restarted handed them out again. Separate workflows in one process stand in here for
//! the separate machines and the restart: nothing is shared between them but the process,
//! which is exactly what the old counters shared too.
//!
//! **Ordered by creation** is checked within this process, where `uuid` orders every v7
//! identifier it mints. Between machines a v7 identifier orders by its millisecond
//! timestamp and each machine's clock, so two machines' identifiers minted in the same
//! millisecond are distinct and not ordered, which is what DN-31 §5.1 claims and no more.

use gungnir_command::{
    ApprovalWorkflow, InMemoryApprovalWorkflow, OperatorDecision, PendingApprovalId, Submission,
};
use gungnir_model::{DecisionId, DecisionSettings, EffectorLayer, MissionTime, PlanId, PlanView};
use gungnir_policy::PolicyVerdict;
use std::collections::HashSet;

/// A workflow whose point-layer items expire after 30 s, so an expiry mints a decision too.
fn workflow() -> InMemoryApprovalWorkflow {
    let mut settings = DecisionSettings::default();
    settings.expiry_s.insert(EffectorLayer::Point, 30.0);
    InMemoryApprovalWorkflow::with_settings(settings)
}

fn submission(plan: u128, at: f64) -> Submission {
    Submission {
        plan: PlanView {
            id: PlanId(plan),
            ..PlanView::default()
        },
        verdict: PolicyVerdict::RequiresHumanApproval,
        submitted: MissionTime(at),
        layer: EffectorLayer::Point,
        priority: 0.0,
        role: "Operator".to_owned(),
    }
}

fn is_v7(value: u128) -> bool {
    uuid::Uuid::from_u128(value).get_version() == Some(uuid::Version::SortRand)
}

/// DN-31 §9 row 1: a node's workflow, two desktops' and one desktop's after a restart
/// mint queue items and decisions in turn -- decisions by a person and by an expiry --
/// and **no two are equal**, every one is a UUID v7, and in minting order each is greater
/// than the last.
#[test]
fn separate_workflows_standing_in_for_machines_and_a_restart_never_mint_the_same_identifier() {
    let mut machines = [workflow(), workflow(), workflow()]; // a node, desktop A, desktop B
    let mut items: Vec<PendingApprovalId> = Vec::new();
    let mut decisions: Vec<DecisionId> = Vec::new();

    for round in 0..40_u32 {
        let at = f64::from(round);
        if round == 20 {
            // Desktop A restarts: a new workflow, as a new process would build, with
            // nothing carried over from the one before.
            machines[1] = workflow();
        }
        for machine in &mut machines {
            // The same plan number on every machine, as three counters would have had.
            let item = machine
                .submit_for_approval(submission(u128::from(round) + 1, at))
                .expect("queued");
            items.push(item);
            if round % 3 == 0 {
                let record = machine
                    .decide(
                        item,
                        OperatorDecision::Accepted,
                        Some("7".into()),
                        Some("Operator".into()),
                        MissionTime(at),
                    )
                    .expect("decided");
                decisions.push(record.id);
            }
            // Whatever has waited past its 30 s window expires, and each expiry mints a
            // decision of its own.
            let _ = machine.sweep(MissionTime(at), &["Operator", "Supervisor"]);
            for record in machine.records() {
                if !decisions.contains(&record.id) {
                    decisions.push(record.id);
                }
            }
        }
    }

    assert!(
        decisions.iter().any(|d| {
            machines
                .iter()
                .flat_map(ApprovalWorkflow::records)
                .any(|r| r.id == *d && r.is_expiry())
        }),
        "no expiry minted a decision, so that path went unchecked"
    );
    for (what, values) in [
        ("queue item", items.iter().map(|i| i.0).collect::<Vec<_>>()),
        (
            "decision",
            decisions.iter().map(|d| d.0).collect::<Vec<_>>(),
        ),
    ] {
        let distinct: HashSet<u128> = values.iter().copied().collect();
        assert_eq!(
            distinct.len(),
            values.len(),
            "two {what} identifiers were equal"
        );
        assert!(
            values.iter().all(|v| is_v7(*v)),
            "a {what} identifier is not a UUID v7"
        );
        assert!(
            values.windows(2).all(|w| w[0] < w[1]),
            "{what} identifiers do not sort in the order they were minted"
        );
    }
}
