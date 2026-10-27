// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The node's SAPIENT effectors (GAP-207): the same link the desktop holds, bound from the
//! same baseline rows, so a handoff the node issues reaches the same effector by the same
//! task and its outcome comes back by the same `TaskAck`s.
//!
//! What is shared lives in the libraries: the task in
//! `gungnir_sensor_management::sapient_task::engage_payload`, the link and its
//! acknowledgement rules in `gungnir_ingest::adapters::sapient_effector`. What is here is
//! what only a binary can say: where a refusal to connect is reported (a node has no
//! console, so it is logged), and the desk's three answers, which `gungnir-approval` names
//! and `gungnir-ingest` may not.

use gungnir_approval::{DeliveryAnswer, HandoffInFlight};
use gungnir_config::ConfigBaseline;
use gungnir_ingest::adapters::sapient_effector::{
    SapientEffectorLink, SapientEffectors, SapientPost, TaskDelivery, CONNECT_TIMEOUT,
};

/// Every SAPIENT effector the baseline names, connected now if its middleware answers.
///
/// One that does not answer is logged and kept: the first handoff to it connects again.
#[must_use]
pub fn bind(config: &ConfigBaseline) -> SapientEffectors {
    let mut effectors = SapientEffectors::default();
    for row in &config.sapient_effectors {
        let Some(addr) = config
            .endpoints
            .iter()
            .find(|e| e.name == row.endpoint)
            .and_then(|e| e.address.parse().ok())
        else {
            tracing::error!(endpoint = %row.endpoint, "SAPIENT effector not bound: its endpoint has no ip:port");
            continue;
        };
        let link = SapientEffectorLink::new(
            row.endpoint.clone(),
            addr,
            row.destination_id.clone(),
            row.node_type.node_type(),
        );
        match link.connect(CONNECT_TIMEOUT) {
            Ok(()) => tracing::info!(endpoint = %row.endpoint, %addr, "SAPIENT effector connected"),
            Err(err) => {
                tracing::warn!(endpoint = %row.endpoint, %err, "SAPIENT effector not reachable yet; a handoff to it connects again");
            }
        }
        effectors.insert(row.endpoint.clone(), link);
    }
    effectors
}

/// What a handoff to `endpoint` is sent as: a SAPIENT task when an effector is behind it,
/// remembered against the decision, and the handoff's own JSON otherwise.
///
/// # Errors
///
/// Why the handoff cannot be put as a SAPIENT task.
pub fn payload(
    config: &ConfigBaseline,
    effectors: Option<&SapientEffectors>,
    endpoint: &str,
    handoff: &gungnir_model::handoff::Handoff,
) -> Result<serde_json::Value, String> {
    match gungnir_sensor_management::sapient_task::engage_payload(config, endpoint, handoff) {
        None => gungnir_approval::handoffs::endpoint_payload(handoff),
        Some(task) => {
            let task = task?;
            if let Some(effectors) = effectors {
                effectors.remember(endpoint, &task.task_id, handoff.decision);
            }
            Ok(task.message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::handoff::{DecisionAttribution, Handoff};
    use gungnir_model::{
        DecisionId, Geodetic, InterceptSolutionView, MissionTime, PlanId, PlanKind, ResourceId,
        TrackId,
    };

    fn handoff() -> Handoff {
        Handoff::from_decision(
            DecisionId(42),
            PlanId(1),
            PlanKind::Intercept {
                solutions: vec![InterceptSolutionView {
                    resource: ResourceId(1),
                    track: TrackId(3),
                    intercept_point: Some(Geodetic {
                        lat_rad: 0.9,
                        lon_rad: 0.2,
                        alt_m: 100.0,
                    }),
                    time_to_intercept_s: None,
                }],
            },
            DecisionAttribution {
                operator: "7".into(),
                role: "Supervisor".into(),
                at: MissionTime(10.0),
                authority_rule: None,
            },
            Vec::new(),
            gungnir_model::Releasability::default(),
            MissionTime(10.0),
        )
    }

    /// GAP-207: the node sends a SAPIENT effector the same task the desktop does, and an
    /// HTTP endpoint the handoff as before.
    #[test]
    fn a_sapient_endpoint_is_sent_a_task_and_any_other_the_handoff() {
        let config: ConfigBaseline = serde_json::from_str(
            r#"{"version": 1,
                "sapient_node_id": "node-a",
                "endpoints": [
                    {"name": "jammer", "kind": "sapient", "address": "127.0.0.1:9"},
                    {"name": "battery", "kind": "http", "address": "http://127.0.0.1:9/"}],
                "sapient_effectors": [{"endpoint": "jammer", "destination_id": "effector-1",
                    "node_type": "jammer"}],
                "resources": [{"id": 1, "position": [0.9, 0.2, 0.0], "capacity": 1,
                    "layer": "point", "handoff_endpoint": "jammer"}]}"#,
        )
        .expect("parses");
        gungnir_config::validate(&config).expect("valid");
        let effectors = SapientEffectors::default();
        let task = payload(&config, Some(&effectors), "jammer", &handoff()).expect("a task");
        assert_eq!(task["destinationId"], "effector-1");
        assert_eq!(task["task"]["command"]["request"], "engage");
        let http = payload(&config, Some(&effectors), "battery", &handoff()).expect("the handoff");
        assert_eq!(http["decision"], "00000000-0000-0000-0000-00000000002a");
    }
}

/// One handoff task in flight to an effector, in the desk's terms: accepted once a
/// `TaskAck` came back (SAPIENT has no status code, so `0` stands where an HTTP status
/// would), unreachable to be sent again, refused when it can never be sent.
#[derive(Debug)]
pub struct Sent(pub SapientPost);

impl HandoffInFlight for Sent {
    fn poll(&self) -> Option<DeliveryAnswer> {
        self.0.poll().map(|delivery| match delivery {
            TaskDelivery::Acknowledged => DeliveryAnswer::Accepted { status: 0 },
            TaskDelivery::Unreachable(reason) => DeliveryAnswer::Unreachable { reason },
            TaskDelivery::Unsendable(body) => DeliveryAnswer::Refused { status: 0, body },
        })
    }
}
