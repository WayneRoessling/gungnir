// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Sensor tasks through the node, and what the node sends back (GAP-004, DN-11;
//! GAP-040, GAP-042).
//!
//! On a linked desktop the registry's control adapter is this link: a command an
//! operator issues here is recorded `Issued` locally, handed to the link, and posted to
//! `POST /v2/sensors/{sensor_id}/task`. The node issues it through its own registry and
//! answers with its own task id, which is mapped to ours; the sensor's acknowledgement,
//! refusal or silence then arrives on the event stream naming the node's id and is
//! applied to our task. **The local mode never changes until the sensor's
//! acknowledgement comes back this way** (DN-11 §5), which is the rule the whole path
//! exists to keep. With no link the adapter refuses at the door with the reason, and
//! the task is recorded as failed rather than left to time out.
//!
//! Two facts about the outside world arrive by the same door and are applied here for the
//! same reason -- the node records them and holds neither record itself: an effector's
//! report on a handoff (GAP-040) and a warned party's acknowledgement (GAP-042).

use gungnir_eventing::Event;
use gungnir_model::events::{HandoffEvent, SensorTaskEvent, WarningEvent};
use gungnir_model::SensorTaskId;
use gungnir_remote::link::{NodeLink, OutboundTask};
use gungnir_sensor_management::tasking::{SensorControlAdapter, SensorTask};
use gungnir_sensor_management::{SensorControl, SensorManagementError};
use std::sync::{Arc, Mutex};

use crate::state::AppState;

/// The registry's outbound adapter over the node link.
pub struct LinkControlAdapter {
    link: Arc<Mutex<Option<NodeLink>>>,
}

impl SensorControlAdapter for LinkControlAdapter {
    fn issue(&self, task: &SensorTask) -> Result<(), SensorManagementError> {
        let link = self
            .link
            .lock()
            .ok()
            .and_then(|l| l.clone())
            .ok_or_else(|| SensorManagementError::Refused {
                sensor: task.sensor,
                reason: "no node link: this desktop delivers sensor tasks through its node, \
                         and none is connected"
                    .to_string(),
            })?;
        link.queue_task(OutboundTask {
            local: task.id,
            sensor: task.sensor,
            command: task.command.clone(),
            requirement: task.requirement,
        });
        Ok(())
    }
}

/// Attach the link as the registry's adapter after a sign-in established it.
pub fn attach(state: &mut AppState, link: NodeLink) {
    if let Ok(mut slot) = state.link_control.lock() {
        *slot = Some(link);
    }
    state.sensors.attach_adapter(Arc::new(LinkControlAdapter {
        link: Arc::clone(&state.link_control),
    }));
}

/// The link is gone: the adapter stays attached and refuses with the reason.
pub fn detach(state: &mut AppState) {
    if let Ok(mut slot) = state.link_control.lock() {
        *slot = None;
    }
    state.node_task_map.clear();
}

/// Every frame: the node's answers to delivered tasks, and the events it streamed for
/// this host.
pub fn sweep(state: &mut AppState) {
    let Some(link) = state.link.clone() else {
        return;
    };
    let now = state.clock.now();
    for outcome in link.take_task_outcomes() {
        match outcome.outcome {
            Ok(node_task) => {
                state.node_task_map.insert(node_task, outcome.local);
            }
            Err(reason) => {
                let reason = format!("the node refused the task: {reason}");
                fail_local(state, outcome.local, &reason, now);
            }
        }
    }
    for envelope in link.take_inbox() {
        match envelope.event {
            Event::SensorTask(event) => apply_task_event(state, event),
            Event::Handoff(HandoffEvent::Reported {
                decision,
                endpoint,
                report,
                at,
            }) => crate::handoffs::apply_report(state, decision, &endpoint, &report, at),
            // GAP-042: the warned party answered through the node, which holds no warning
            // ledger. This desktop raised the warning, so this desktop discharges it.
            Event::Warning(WarningEvent::Acknowledged {
                asset,
                track,
                party,
                at,
            }) => crate::warnings::apply_acknowledgement(state, asset, track, &party, at),
            _ => {}
        }
    }
}

fn apply_task_event(state: &mut AppState, event: SensorTaskEvent) {
    let now = state.clock.now();
    match event {
        SensorTaskEvent::Acknowledged { task, sensor, at } => {
            let Some(local) = state.node_task_map.get(&task).copied() else {
                return;
            };
            match state.sensors.acknowledge(local, at) {
                Ok(()) => {
                    crate::sustainment::publish_task(
                        state,
                        now,
                        SensorTaskEvent::Acknowledged {
                            task: local,
                            sensor,
                            at,
                        },
                    );
                    state.alerts.push(format!(
                        "sensor {} acknowledged task {} through the node",
                        sensor.0, local.0
                    ));
                }
                Err(err) => state.alerts.push(format!(
                    "the node reported sensor {} acknowledged task {}, which this desktop \
                     could not apply: {err}",
                    sensor.0, local.0
                )),
            }
        }
        SensorTaskEvent::Failed {
            task,
            sensor: _,
            reason,
            at: _,
        } => {
            let Some(local) = state.node_task_map.get(&task).copied() else {
                return;
            };
            fail_local(state, local, &reason, now);
        }
        SensorTaskEvent::Unacknowledged { task, .. } => {
            let Some(local) = state.node_task_map.get(&task).copied() else {
                return;
            };
            fail_local(
                state,
                local,
                "the node's acknowledgement window closed with nothing back",
                now,
            );
        }
        SensorTaskEvent::Issued { .. } => {}
    }
}

fn fail_local(
    state: &mut AppState,
    local: SensorTaskId,
    reason: &str,
    now: gungnir_model::MissionTime,
) {
    let sensor = state
        .sensors
        .tasks()
        .iter()
        .find(|t| t.id == local)
        .map(|t| t.sensor);
    match state.sensors.fail(local, reason.to_owned()) {
        Ok(()) => {
            if let Some(sensor) = sensor {
                crate::sustainment::publish_task(
                    state,
                    now,
                    SensorTaskEvent::Failed {
                        task: local,
                        sensor,
                        reason: reason.to_owned(),
                        at: now,
                    },
                );
            }
            state
                .alerts
                .push(format!("sensor task {} failed: {reason}", local.0));
        }
        Err(err) => state.alerts.push(format!(
            "task {} failed at the node ({reason}) and this desktop could not record it: {err}",
            local.0
        )),
    }
}
