// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The outbound control path: commands to a sensor, and what came back.
//!
//! Design: docs/design/DN-11-sensor-control-and-tasking.md. Capability CAP-1.3;
//! mission threads MT-07 recovery and MT-03 camera cueing, both of which stop at the
//! operator's screen today.
//!
//! **The rule that matters most here: local state does not change until the sensor
//! acknowledges.** A mode the operator asked for and the sensor never took is
//! displayed as requested-not-confirmed, not as the current mode. A registry that
//! reports the mode it asked for is a health flag that lies.
//!
//! No adapter exists yet: GAP-001 brings them. Until then [`issue`] returns
//! [`SensorManagementError::NotControllable`] and the panel says the sensor is not
//! controllable from here. It does not appear to succeed, which is what lets this
//! design land before the adapters without pretending.

use crate::{SensorManagementError, SensorMode};
use gungnir_model::{MissionTime, RequirementId, SensorId};

/// Identifier of one issued task.
///
/// Owned by `gungnir-model` because the event schema carries it; re-exported here
/// because this is the crate that issues them.
pub use gungnir_model::SensorTaskId;

/// Owned by `gungnir-model` since GAP-004's route carries it (the same move
/// `SensorTaskId` made); re-exported here so the issuing crate's path still resolves.
pub use gungnir_model::SensorCommand;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum TaskState {
    /// Recorded here; not yet handed to the adapter.
    Issued,
    /// The adapter accepted it for delivery.
    Sent,
    /// The sensor acknowledged. **Only now does local state change.**
    Acknowledged { at: MissionTime },
    /// The sensor refused, or the adapter could not deliver. Never retried
    /// silently: retry is an operator action.
    Failed { reason: String },
    /// No acknowledgement inside the window.
    Unacknowledged,
}

impl TaskState {
    /// True while the task might still be acknowledged.
    pub fn is_open(&self) -> bool {
        matches!(self, TaskState::Issued | TaskState::Sent)
    }

    /// True when this task's intent has taken effect on the sensor.
    pub fn is_confirmed(&self) -> bool {
        matches!(self, TaskState::Acknowledged { .. })
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SensorTask {
    pub id: SensorTaskId,
    pub sensor: SensorId,
    pub command: SensorCommand,
    /// The requirement this serves, when it came from one.
    pub requirement: Option<RequirementId>,
    pub issued: MissionTime,
    pub state: TaskState,
}

impl SensorTask {
    /// True when the acknowledgement window has passed with nothing back.
    pub fn has_timed_out(&self, now: MissionTime, window_s: f64) -> bool {
        self.state.is_open() && now.seconds_since(self.issued) > window_s
    }

    /// The mode this task asked for, if it asked for one.
    pub fn requested_mode(&self) -> Option<SensorMode> {
        match &self.command {
            SensorCommand::SetMode { mode } => Some(*mode),
            _ => None,
        }
    }
}

/// Outbound counterpart of the ingest adapter. One implementation per sensor
/// interface agreement; none exists until GAP-001 brings the adapters.
pub trait SensorControlAdapter: Send + Sync {
    fn issue(&self, task: &SensorTask) -> Result<(), SensorManagementError>;
}

/// What the registry reports for a sensor's mode: what it is, and what was asked.
///
/// The two are separate fields rather than one, because collapsing them is exactly
/// the lie this design exists to prevent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ModeStatus {
    /// The mode the sensor is confirmed to be in.
    pub confirmed: SensorMode,
    /// A mode that has been requested and not yet acknowledged.
    pub requested: Option<SensorMode>,
}

impl ModeStatus {
    pub fn settled(mode: SensorMode) -> Self {
        Self {
            confirmed: mode,
            requested: None,
        }
    }

    pub fn is_pending(&self) -> bool {
        self.requested.is_some()
    }
}

/// Marks every open task past its window as unacknowledged.
///
/// Returns the tasks that timed out so the caller can raise an alert for each. It
/// never retries: retry is an operator action.
pub fn time_out_stale(
    tasks: &mut [SensorTask],
    now: MissionTime,
    window_s: f64,
) -> Vec<SensorTaskId> {
    let mut timed_out = Vec::new();
    for task in tasks.iter_mut().filter(|t| t.has_timed_out(now, window_s)) {
        task.state = TaskState::Unacknowledged;
        timed_out.push(task.id);
    }
    timed_out
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::Geodetic;

    fn task(id: u64, command: SensorCommand) -> SensorTask {
        SensorTask {
            id: SensorTaskId(id),
            sensor: SensorId(1),
            command,
            requirement: None,
            issued: MissionTime(100.0),
            state: TaskState::Sent,
        }
    }

    #[test]
    fn a_requested_mode_is_not_the_confirmed_mode() {
        // The whole point: a mode nobody confirmed must not read as current.
        let status = ModeStatus {
            confirmed: SensorMode::Standby,
            requested: Some(SensorMode::Search),
        };
        assert!(status.is_pending());
        assert_eq!(status.confirmed, SensorMode::Standby);
        assert_ne!(status.confirmed, SensorMode::Search);

        let settled = ModeStatus::settled(SensorMode::Search);
        assert!(!settled.is_pending());
        assert_eq!(settled.confirmed, SensorMode::Search);
    }

    #[test]
    fn an_ignored_task_times_out_inside_the_window_and_is_never_retried() {
        let mut tasks = vec![task(
            1,
            SensorCommand::SetMode {
                mode: SensorMode::Search,
            },
        )];
        assert!(time_out_stale(&mut tasks, MissionTime(105.0), 30.0).is_empty());

        let timed_out = time_out_stale(&mut tasks, MissionTime(200.0), 30.0);
        assert_eq!(timed_out, vec![SensorTaskId(1)]);
        assert_eq!(tasks[0].state, TaskState::Unacknowledged);
        assert!(!tasks[0].state.is_open(), "and it is not retried");
    }

    #[test]
    fn an_acknowledged_task_is_confirmed_and_a_failed_one_is_not() {
        let mut acknowledged = task(
            1,
            SensorCommand::Calibrate {
                procedure: "a".into(),
            },
        );
        acknowledged.state = TaskState::Acknowledged {
            at: MissionTime(110.0),
        };
        assert!(acknowledged.state.is_confirmed());
        assert!(!acknowledged.state.is_open());

        let mut failed = task(
            2,
            SensorCommand::Calibrate {
                procedure: "a".into(),
            },
        );
        failed.state = TaskState::Failed {
            reason: "sensor refused".into(),
        };
        assert!(!failed.state.is_confirmed());
        assert!(!failed.state.is_open());
        match &failed.state {
            TaskState::Failed { reason } => assert!(!reason.is_empty(), "the reason travels"),
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[test]
    fn an_already_closed_task_is_not_timed_out_again() {
        let mut tasks = vec![task(
            1,
            SensorCommand::SetMode {
                mode: SensorMode::Search,
            },
        )];
        tasks[0].state = TaskState::Acknowledged {
            at: MissionTime(110.0),
        };
        assert!(time_out_stale(&mut tasks, MissionTime(1_000.0), 30.0).is_empty());
    }

    #[test]
    fn a_task_reports_the_mode_it_asked_for_only_when_it_asked_for_one() {
        let mode = task(
            1,
            SensorCommand::SetMode {
                mode: SensorMode::Track,
            },
        );
        assert_eq!(mode.requested_mode(), Some(SensorMode::Track));

        let cue = task(
            2,
            SensorCommand::Cue {
                target: Geodetic {
                    lat_rad: 0.0,
                    lon_rad: 0.0,
                    alt_m: 0.0,
                },
                dwell_s: Some(5.0),
            },
        );
        assert_eq!(cue.requested_mode(), None);
    }
}
