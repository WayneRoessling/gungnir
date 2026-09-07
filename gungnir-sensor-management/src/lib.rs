// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Sensor management & adaptive collection, per docs/gungnir-capabilities.md
//! §5.2. The dashboard shows sensor health; this crate is the management
//! capability behind that display: a registry initialized from `gungnir-config`,
//! a mode state machine with explicit transition rules, and coverage regions that
//! feed `gungnir-geo` map layers and `gungnir-assessment` exposure calculations.

pub mod tasking;

pub use tasking::{
    time_out_stale, ModeStatus, SensorCommand, SensorControlAdapter, SensorTask, SensorTaskId,
    TaskState,
};

use gungnir_config::SensorConfig;
use gungnir_coord::Geodetic;
use gungnir_model::{
    absence_is_planned, MaintenanceState, MaintenanceWindow, MissionTime, SensorId,
};

/// Re-exported: `gungnir-model` owns it so `gungnir-model::events` can carry a mode
/// change, and this crate cannot be depended on by the model. Every existing
/// `gungnir_sensor_management::SensorMode` path still resolves.
pub use gungnir_model::SensorMode;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SensorRecord {
    pub id: SensorId,
    pub modality: String,
    pub position: Geodetic,
    pub max_range_m: f64,
    pub calibration_version: String,
    /// The mode the sensor is **confirmed** to be in.
    ///
    /// Changed by an acknowledgement or by an operator recording what they know, never
    /// by asking a sensor to change. DN-11 §5 rule 1: a registry that reports the mode
    /// it asked for is a health flag that lies.
    pub mode: SensorMode,
    /// The endpoint a command for this sensor goes to, when one is configured.
    ///
    /// `None` means the sensor is not controllable from here, which is every sensor
    /// until GAP-001 brings the adapters.
    pub control_endpoint: Option<String>,
    /// What the sensor itself has reported through its service messages (GAP-064).
    /// `None` until it has said anything.
    pub observed: Option<ObservedService>,
    /// Planned downtime for this sensor (DN-21 §3, GAP-054).
    ///
    /// Held on the record rather than in a separate table because every question anyone
    /// asks about a window -- is this absence expected, did it overrun, what is due this
    /// watch -- is a question about one sensor.
    pub maintenance: Vec<MaintenanceWindow>,
}

impl SensorRecord {
    /// A registry entry from the applied config baseline; starts in `Standby`.
    pub fn from_config(config: &SensorConfig, calibration_version: impl Into<String>) -> Self {
        Self {
            id: SensorId(config.id),
            modality: config.modality.clone(),
            position: Geodetic {
                lat_rad: config.position[0],
                lon_rad: config.position[1],
                alt_m: config.position[2],
            },
            max_range_m: config.max_range_m,
            calibration_version: calibration_version.into(),
            mode: SensorMode::Standby,
            control_endpoint: config.control_endpoint.clone(),
            observed: None,
            maintenance: config
                .maintenance
                .iter()
                .map(|w| w.to_window(SensorId(config.id)))
                .collect(),
        }
    }

    /// True when this sensor is inside a planned window right now.
    ///
    /// **Distinguishes expected absence from failure, which is the whole point** (DN-21
    /// §5). It says nothing about whether the sensor is actually down: a sensor that is
    /// still radiating during its window is not a fault either, and coverage reports what
    /// it can see rather than what the schedule expected.
    #[must_use]
    pub fn is_in_maintenance(&self, now: MissionTime) -> bool {
        absence_is_planned(&self.maintenance, self.id, now)
    }

    /// True when this sensor is off the air.
    #[must_use]
    pub fn is_down(&self) -> bool {
        self.mode == SensorMode::Offline
    }

    /// Whether this sensor's silence right now is a fault somebody must act on.
    ///
    /// **Three states collapse into this one question and must not be confused**: a sensor
    /// radiating normally, a sensor down inside its window, and a sensor down outside one.
    /// Only the last is a failure. A window that has closed with the sensor still down is
    /// the fourth case and is louder than any of them -- see
    /// [`InMemorySensorRegistry::advance_maintenance`].
    #[must_use]
    pub fn absence_is_a_fault(&self, now: MissionTime) -> bool {
        self.is_down() && !self.is_in_maintenance(now)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SensorManagementError {
    #[error("unknown sensor id {0:?}")]
    UnknownSensor(SensorId),
    #[error("requested mode transition not permitted from {from:?} to {to:?}")]
    InvalidModeTransition { from: SensorMode, to: SensorMode },
    /// No control adapter is configured for this sensor, so nothing can be issued
    /// to it from here (docs/design/DN-11-sensor-control-and-tasking.md, rule 4).
    ///
    /// This is the state of every sensor until GAP-001 brings the adapters. It is
    /// an explicit error rather than a silent success, so the panel can say the
    /// sensor is not controllable rather than appearing to have tasked it.
    #[error("sensor {0:?} has no control adapter configured")]
    NotControllable(SensorId),
    #[error("no task with id {0:?}")]
    UnknownTask(crate::tasking::SensorTaskId),
    /// The task is already acknowledged, failed, or timed out.
    ///
    /// An error rather than a no-op: a second acknowledgement for a task that already
    /// failed would rewrite what happened, and the record is append-only.
    #[error("task {0:?} is already closed")]
    TaskClosed(crate::tasking::SensorTaskId),
    /// The sensor refused the command, or the adapter could not deliver it.
    ///
    /// Carries the adapter's own words verbatim. Paraphrasing loses the diagnosis, and
    /// the diagnosis is the only thing that makes a refusal actionable -- DN-11 §5 rule
    /// 3 leaves the retry to an operator, who needs to know what went wrong to decide.
    #[error("sensor {sensor:?} refused the command: {reason}")]
    Refused { sensor: SensorId, reason: String },
}

/// Registry + tasking surface: what `gungnir-config` initializes at startup and
/// what an operator (via `gungnir-ui`) or `gungnir-decision` (automated retasking)
/// interacts with at runtime.
pub trait SensorRegistry: Send + Sync {
    fn sensors(&self) -> &[SensorRecord];
    fn set_mode(&mut self, id: SensorId, mode: SensorMode) -> Result<(), SensorManagementError>;
    /// Coverage planning: which regions are currently observed at what confidence,
    /// given each sensor's mode and geometry.
    fn coverage(&self) -> Vec<CoverageRegion>;
}

/// A service message from the radar itself, in the registry's words (GAP-064). The host
/// converts the ingest adapter's observation to this; the registry never sees ASTERIX.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ServiceObservation {
    /// The antenna crossed north, with the rotation period when sent.
    NorthMarker {
        rotation_period_s: Option<f64>,
    },
    SectorCrossing,
    /// The system status the radar reports about itself.
    Status {
        released_for_operational_use: bool,
        overloaded: bool,
        time_source_invalid: bool,
    },
}

/// What a sensor has said about itself (GAP-064). **Confirmed state**, by DN-11 §5
/// rule 1: it comes from the sensor, not from anything we asked of it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ObservedService {
    pub last_report: MissionTime,
    pub reports: u64,
    pub rotation_period_s: Option<f64>,
    /// `Some(false)` means the radar says its data may not be used operationally, and
    /// the registry counts it as covering nothing until it says otherwise.
    pub released_for_operational_use: Option<bool>,
    pub overloaded: Option<bool>,
    pub time_source_invalid: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CoverageRegion {
    pub sensor: SensorId,
    pub center: Geodetic,
    pub radius_m: f64,
    pub confidence: f32,
}

/// The adapter a registry hands tasks to, if one has been attached.
///
/// A newtype only so the registry can keep its derived `Debug` without printing a trait
/// object: whether an adapter is attached is the interesting fact, and it is the one
/// this prints.
#[derive(Default, Clone)]
struct AttachedAdapter(Option<std::sync::Arc<dyn tasking::SensorControlAdapter>>);

impl std::fmt::Debug for AttachedAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_some() {
            "attached"
        } else {
            "none (GAP-001)"
        })
    }
}

#[derive(Debug, Default, Clone)]
pub struct InMemorySensorRegistry {
    sensors: Vec<SensorRecord>,
    /// Every task issued this session, open and closed. Append-only: a task that
    /// failed is as much a part of the record as one that was acknowledged.
    tasks: Vec<tasking::SensorTask>,
    next_task: u64,
    /// A mode asked for and not yet acknowledged, per sensor. Separate from
    /// `SensorRecord::mode` because collapsing them is the lie DN-11 exists to prevent.
    requested: std::collections::BTreeMap<SensorId, SensorMode>,
    /// What carries a task off this machine.
    ///
    /// **Nothing attaches one today**: GAP-001 brings the adapters, and until it does
    /// every task stops at `Issued`. The field is here rather than waiting for GAP-001
    /// because it is what makes `TaskState::Sent` mean something -- and because DN-11 §8
    /// verifies this crate against a stub that can acknowledge, refuse, or ignore, which
    /// needs somewhere to plug in.
    adapter: AttachedAdapter,
}

impl InMemorySensorRegistry {
    pub fn from_config(sensors: &[SensorConfig], calibration_version: &str) -> Self {
        Self {
            sensors: sensors
                .iter()
                .map(|s| SensorRecord::from_config(s, calibration_version))
                .collect(),
            tasks: Vec::new(),
            next_task: 0,
            requested: std::collections::BTreeMap::new(),
            adapter: AttachedAdapter::default(),
        }
    }

    /// Attach the thing that carries commands to sensors.
    ///
    /// One registry, one adapter: which sensor a task belongs to is already in the task,
    /// and an adapter per sensor would put the routing decision in two places. GAP-001
    /// brings the implementations; today the only caller is the test stub DN-11 §8 asks
    /// for.
    pub fn attach_adapter(&mut self, adapter: std::sync::Arc<dyn tasking::SensorControlAdapter>) {
        self.adapter = AttachedAdapter(Some(adapter));
    }

    /// Whether commands issued here can leave this machine.
    #[must_use]
    pub fn has_adapter(&self) -> bool {
        self.adapter.0.is_some()
    }

    fn record_mut(&mut self, id: SensorId) -> Result<&mut SensorRecord, SensorManagementError> {
        self.sensors
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or(SensorManagementError::UnknownSensor(id))
    }

    fn task_mut(
        &mut self,
        id: tasking::SensorTaskId,
    ) -> Result<&mut tasking::SensorTask, SensorManagementError> {
        self.tasks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or(SensorManagementError::UnknownTask(id))
    }

    pub fn get(&self, id: SensorId) -> Option<&SensorRecord> {
        self.sensors.iter().find(|s| s.id == id)
    }

    /// Advance every maintenance window to the state it should be in at `now`, and report
    /// what changed.
    ///
    /// Returns transitions rather than publishing them, so the caller decides what a
    /// change means: the desktop journals it and alerts on an overrun, a test asserts on
    /// it. **Only transitions are returned**, never a re-assertion of the state a window
    /// is already in, because a window that is still open is not news every frame and an
    /// alert repeated every tick is an alert nobody reads.
    pub fn advance_maintenance(&mut self, now: MissionTime) -> Vec<(SensorId, MaintenanceState)> {
        let mut changed = Vec::new();
        for sensor in &mut self.sensors {
            let down = sensor.mode == SensorMode::Offline;
            for window in &mut sensor.maintenance {
                if let Some(next) = window.state_at(now, down) {
                    window.state = next;
                    changed.push((sensor.id, next));
                }
            }
        }
        changed
    }

    /// Windows that are open or still to come at `now`, for the handover summary.
    ///
    /// Includes overruns, which are the ones the incoming watch most needs: a window that
    /// closed with the sensor still down is unfinished work being handed over.
    #[must_use]
    pub fn maintenance_outstanding(&self, now: MissionTime) -> Vec<MaintenanceWindow> {
        self.sensors
            .iter()
            .flat_map(|s| s.maintenance.iter())
            .filter(|w| w.state == MaintenanceState::Overrun || w.is_open_at(now) || w.from > now)
            .cloned()
            .collect()
    }
}

impl SensorRegistry for InMemorySensorRegistry {
    fn sensors(&self) -> &[SensorRecord] {
        &self.sensors
    }

    fn set_mode(&mut self, id: SensorId, mode: SensorMode) -> Result<(), SensorManagementError> {
        let record = self
            .sensors
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or(SensorManagementError::UnknownSensor(id))?;
        if !record.mode.can_transition_to(mode) {
            return Err(SensorManagementError::InvalidModeTransition {
                from: record.mode,
                to: mode,
            });
        }
        record.mode = mode;
        Ok(())
    }

    fn coverage(&self) -> Vec<CoverageRegion> {
        self.sensors
            .iter()
            .filter_map(|s| {
                // GAP-064: a radar that says its data is not released for operational
                // use covers nothing, whatever mode it is in.
                if s.observed
                    .as_ref()
                    .is_some_and(|o| o.released_for_operational_use == Some(false))
                {
                    return None;
                }
                let confidence = match s.mode {
                    SensorMode::Track => 1.0,
                    SensorMode::Search => 0.7,
                    SensorMode::Standby | SensorMode::Calibrating | SensorMode::Offline => {
                        return None
                    }
                };
                Some(CoverageRegion {
                    sensor: s.id,
                    center: s.position,
                    radius_m: s.max_range_m,
                    confidence,
                })
            })
            .collect()
    }
}

/// The outbound control path (DN-11 §5, GAP-004).
///
/// Separate from [`SensorRegistry`] because it is a different authority: reading what
/// sensors are doing is one thing, commanding them is another. Splitting the traits lets
/// a caller that only reads say so in its bounds.
///
/// # What every method here is careful about
///
/// A command is a request until a sensor says otherwise. Nothing in this trait changes a
/// confirmed mode except [`SensorControl::acknowledge`], and nothing records a task that
/// was never sent.
pub trait SensorControl {
    /// Issue a command to a sensor.
    ///
    /// Returns [`SensorManagementError::NotControllable`] when the sensor has no control
    /// endpoint, **and records nothing**: a task in the record for a command that no
    /// adapter could carry would read, later, as a command that was sent and ignored.
    ///
    /// `requirement` names the collection requirement this serves, when it came from one
    /// (GAP-005). A required argument rather than a convenience overload: `SensorTask`
    /// has always had the field and nothing ever set it, so a requirement could not be
    /// linked to the tasks serving it however hard `gungnir-workflow` tried. Making
    /// every caller say `None` deliberately is what stops that recurring.
    fn issue(
        &mut self,
        sensor: SensorId,
        command: tasking::SensorCommand,
        requirement: Option<gungnir_model::RequirementId>,
        now: MissionTime,
    ) -> Result<tasking::SensorTaskId, SensorManagementError>;

    /// The sensor acknowledged. **The only path by which a command changes the confirmed
    /// mode** (DN-11 §5).
    fn acknowledge(
        &mut self,
        task: tasking::SensorTaskId,
        now: MissionTime,
    ) -> Result<(), SensorManagementError>;

    /// The sensor refused, or the adapter could not deliver. Never retried silently:
    /// retry is an operator action.
    fn fail(
        &mut self,
        task: tasking::SensorTaskId,
        reason: String,
    ) -> Result<(), SensorManagementError>;

    /// Mark every open task past its window unacknowledged, returning them so the caller
    /// can raise an alert for each.
    fn sweep(&mut self, now: MissionTime, window_s: f64) -> Vec<tasking::SensorTaskId>;

    /// Every task issued this session, open and closed.
    fn tasks(&self) -> &[tasking::SensorTask];

    /// The tasks issued against one collection requirement, in issue order (GAP-005).
    fn tasks_for(&self, requirement: gungnir_model::RequirementId) -> Vec<&tasking::SensorTask> {
        self.tasks()
            .iter()
            .filter(|t| t.requirement == Some(requirement))
            .collect()
    }

    /// What a sensor is confirmed to be doing, and what has been asked of it.
    fn mode_status(&self, sensor: SensorId) -> Option<tasking::ModeStatus>;
}

impl InMemorySensorRegistry {
    /// Record what an operator knows a sensor is doing, without commanding it.
    ///
    /// Deliberately distinct from [`SensorControl::issue`]: "make this sensor search" and
    /// "this sensor is searching, I was told on the radio" are different acts, and while
    /// no adapter exists the second is the only one available. PN-10 labels them apart
    /// rather than giving one control two meanings.
    pub fn record_observed_mode(
        &mut self,
        id: SensorId,
        mode: SensorMode,
    ) -> Result<(), SensorManagementError> {
        self.set_mode(id, mode)
    }

    /// The radar spoke for itself (GAP-064). Records what it said, and **confirms
    /// `Search` for a sensor at `Standby` or `Offline`**: an antenna crossing north is a
    /// radar that is up and scanning, which is the confirmation DN-11 §5 rule 1 asks for.
    /// A sensor already searching or tracking keeps its mode; nothing here can tell the
    /// two apart. Returns the previous mode when the mode changed, so the host can put
    /// the change on the record.
    ///
    /// # Errors
    ///
    /// `SensorManagementError::UnknownSensor` for a sensor the baseline does not name.
    pub fn observe_service(
        &mut self,
        id: SensorId,
        observation: ServiceObservation,
        now: MissionTime,
    ) -> Result<Option<SensorMode>, SensorManagementError> {
        let record = self.record_mut(id)?;
        let observed = record.observed.get_or_insert(ObservedService {
            last_report: now,
            reports: 0,
            rotation_period_s: None,
            released_for_operational_use: None,
            overloaded: None,
            time_source_invalid: None,
        });
        observed.last_report = now;
        observed.reports += 1;
        match observation {
            ServiceObservation::NorthMarker { rotation_period_s } => {
                if rotation_period_s.is_some() {
                    observed.rotation_period_s = rotation_period_s;
                }
            }
            ServiceObservation::SectorCrossing => {}
            ServiceObservation::Status {
                released_for_operational_use,
                overloaded,
                time_source_invalid,
            } => {
                observed.released_for_operational_use = Some(released_for_operational_use);
                observed.overloaded = Some(overloaded);
                observed.time_source_invalid = Some(time_source_invalid);
            }
        }
        let scanning = matches!(
            observation,
            ServiceObservation::NorthMarker { .. } | ServiceObservation::SectorCrossing
        );
        if scanning && matches!(record.mode, SensorMode::Standby | SensorMode::Offline) {
            let from = record.mode;
            record.mode = SensorMode::Search;
            return Ok(Some(from));
        }
        Ok(None)
    }
}

impl SensorControl for InMemorySensorRegistry {
    fn issue(
        &mut self,
        sensor: SensorId,
        command: tasking::SensorCommand,
        requirement: Option<gungnir_model::RequirementId>,
        now: MissionTime,
    ) -> Result<tasking::SensorTaskId, SensorManagementError> {
        let record = self.record_mut(sensor)?;
        if record.control_endpoint.is_none() {
            return Err(SensorManagementError::NotControllable(sensor));
        }
        // An illegal transition is refused before anything is recorded, here rather than
        // at the sensor, because this side can say why and the wire cannot.
        let requested = match &command {
            tasking::SensorCommand::SetMode { mode } => {
                if !record.mode.can_transition_to(*mode) {
                    return Err(SensorManagementError::InvalidModeTransition {
                        from: record.mode,
                        to: *mode,
                    });
                }
                Some(*mode)
            }
            _ => None,
        };
        self.next_task += 1;
        let id = tasking::SensorTaskId(self.next_task);
        let mut task = tasking::SensorTask {
            id,
            sensor,
            command,
            requirement,
            issued: now,
            // With no adapter attached a task reaches `Issued` and stops there, which is
            // every task today (GAP-001). `Sent` means an adapter accepted it for
            // delivery, and nothing may report that until something can.
            state: tasking::TaskState::Issued,
        };
        if let Some(adapter) = self.adapter.0.clone() {
            task.state = match adapter.issue(&task) {
                Ok(()) => tasking::TaskState::Sent,
                // The adapter refused at the door. The task is still recorded -- it was
                // genuinely attempted, and an operator deciding whether to try again
                // needs to see that it was and why it failed.
                Err(err) => tasking::TaskState::Failed {
                    reason: err.to_string(),
                },
            };
        }
        let open = task.state.is_open();
        self.tasks.push(task);
        // Only an open task has a request outstanding. One the adapter refused asked for
        // nothing that is still pending.
        if let (Some(mode), true) = (requested, open) {
            self.requested.insert(sensor, mode);
        }
        Ok(id)
    }

    fn acknowledge(
        &mut self,
        task: tasking::SensorTaskId,
        now: MissionTime,
    ) -> Result<(), SensorManagementError> {
        let entry = self.task_mut(task)?;
        if !entry.state.is_open() {
            return Err(SensorManagementError::TaskClosed(task));
        }
        entry.state = tasking::TaskState::Acknowledged { at: now };
        let sensor = entry.sensor;
        let mode = entry.requested_mode();
        if let Some(mode) = mode {
            self.requested.remove(&sensor);
            // Set directly rather than through `set_mode`: the transition was checked
            // when the command was issued, and the sensor has now confirmed it. Checking
            // again against a mode that may have moved since would refuse a fact.
            self.record_mut(sensor)?.mode = mode;
        }
        Ok(())
    }

    fn fail(
        &mut self,
        task: tasking::SensorTaskId,
        reason: String,
    ) -> Result<(), SensorManagementError> {
        let entry = self.task_mut(task)?;
        if !entry.state.is_open() {
            return Err(SensorManagementError::TaskClosed(task));
        }
        entry.state = tasking::TaskState::Failed { reason };
        let sensor = entry.sensor;
        // The confirmed mode is untouched: a refused command leaves the sensor doing
        // whatever it was doing.
        self.requested.remove(&sensor);
        Ok(())
    }

    fn sweep(&mut self, now: MissionTime, window_s: f64) -> Vec<tasking::SensorTaskId> {
        let timed_out = tasking::time_out_stale(&mut self.tasks, now, window_s);
        let sensors: Vec<SensorId> = self
            .tasks
            .iter()
            .filter(|t| timed_out.contains(&t.id))
            .map(|t| t.sensor)
            .collect();
        // A request nobody answered stops being pending. It does not become confirmed:
        // the caller alerts, and the operator decides whether to ask again.
        for sensor in sensors {
            self.requested.remove(&sensor);
        }
        timed_out
    }

    fn tasks(&self) -> &[tasking::SensorTask] {
        &self.tasks
    }

    fn mode_status(&self, sensor: SensorId) -> Option<tasking::ModeStatus> {
        let record = self.sensors.iter().find(|s| s.id == sensor)?;
        Some(tasking::ModeStatus {
            confirmed: record.mode,
            requested: self.requested.get(&sensor).copied(),
        })
    }
}

#[cfg(test)]
mod service_observation_tests {
    use super::*;
    use gungnir_config::SensorConfig;

    fn registry() -> InMemorySensorRegistry {
        InMemorySensorRegistry::from_config(
            &[SensorConfig {
                id: 1,
                modality: "radar".into(),
                position: [0.9, 0.2, 10.0],
                max_range_m: 20_000.0,
                control_endpoint: None,
                maintenance: Vec::new(),
            }],
            "v1",
        )
    }

    #[test]
    fn a_north_marker_confirms_a_standby_radar_is_searching_and_records_its_period() {
        let mut r = registry();
        assert!(r.coverage().is_empty(), "standby covers nothing");
        let from = r
            .observe_service(
                SensorId(1),
                ServiceObservation::NorthMarker {
                    rotation_period_s: Some(4.0),
                },
                MissionTime(10.0),
            )
            .expect("known sensor");
        assert_eq!(from, Some(SensorMode::Standby));
        let record = r.get(SensorId(1)).expect("record");
        assert_eq!(record.mode, SensorMode::Search);
        let observed = record.observed.as_ref().expect("observed");
        assert_eq!(observed.rotation_period_s, Some(4.0));
        assert_eq!(observed.reports, 1);
        assert_eq!(r.coverage().len(), 1, "a scanning radar covers");
        // A second marker changes nothing about the mode.
        let again = r
            .observe_service(
                SensorId(1),
                ServiceObservation::SectorCrossing,
                MissionTime(11.0),
            )
            .expect("known");
        assert_eq!(again, None);
    }

    #[test]
    fn a_radar_not_released_for_operational_use_covers_nothing_until_it_says_otherwise() {
        let mut r = registry();
        r.observe_service(
            SensorId(1),
            ServiceObservation::NorthMarker {
                rotation_period_s: None,
            },
            MissionTime(1.0),
        )
        .expect("known");
        r.observe_service(
            SensorId(1),
            ServiceObservation::Status {
                released_for_operational_use: false,
                overloaded: false,
                time_source_invalid: false,
            },
            MissionTime(2.0),
        )
        .expect("known");
        assert!(r.coverage().is_empty(), "NOGO: nothing is covered");
        r.observe_service(
            SensorId(1),
            ServiceObservation::Status {
                released_for_operational_use: true,
                overloaded: true,
                time_source_invalid: false,
            },
            MissionTime(3.0),
        )
        .expect("known");
        assert_eq!(r.coverage().len(), 1);
        assert_eq!(
            r.get(SensorId(1))
                .and_then(|s| s.observed.as_ref())
                .and_then(|o| o.overloaded),
            Some(true)
        );
    }

    #[test]
    fn an_unknown_sensor_is_refused() {
        let mut r = registry();
        assert!(r
            .observe_service(
                SensorId(9),
                ServiceObservation::SectorCrossing,
                MissionTime(0.0)
            )
            .is_err());
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    use crate::tasking::{SensorCommand, TaskState};
    use gungnir_model::MissionTime;

    /// The stub DN-11 §8's method calls for: an adapter that can acknowledge, refuse, or
    /// ignore.
    ///
    /// It cannot acknowledge from inside `issue`, and deliberately does not try: an
    /// acknowledgement is something that arrives later, and a stub that confirmed
    /// synchronously would verify a code path no real sensor will ever take. The
    /// "acknowledge" case is the test calling [`SensorControl::acknowledge`] afterwards,
    /// which is what an adapter's receive side will do.
    #[derive(Debug)]
    struct StubAdapter {
        /// What the adapter does when handed a task.
        refuses: Option<String>,
        /// Every task it was handed, so a test can assert nothing was sent twice.
        seen: std::sync::Mutex<Vec<tasking::SensorTaskId>>,
    }

    impl StubAdapter {
        fn accepting() -> std::sync::Arc<Self> {
            std::sync::Arc::new(Self {
                refuses: None,
                seen: std::sync::Mutex::new(Vec::new()),
            })
        }

        fn refusing(reason: &str) -> std::sync::Arc<Self> {
            std::sync::Arc::new(Self {
                refuses: Some(reason.to_owned()),
                seen: std::sync::Mutex::new(Vec::new()),
            })
        }

        fn deliveries(&self) -> Vec<tasking::SensorTaskId> {
            self.seen.lock().map(|s| s.clone()).unwrap_or_default()
        }
    }

    impl tasking::SensorControlAdapter for StubAdapter {
        fn issue(&self, task: &tasking::SensorTask) -> Result<(), SensorManagementError> {
            if let Ok(mut seen) = self.seen.lock() {
                seen.push(task.id);
            }
            match &self.refuses {
                Some(reason) => Err(SensorManagementError::Refused {
                    sensor: task.sensor,
                    reason: reason.clone(),
                }),
                None => Ok(()),
            }
        }
    }

    /// The stub accepts and the sensor later confirms. This is the only path on which a
    /// command moves the confirmed mode, and it takes two steps to do it.
    #[test]
    fn a_stub_that_accepts_marks_the_task_sent_and_confirms_only_on_acknowledgement() {
        let mut registry = controllable();
        let adapter = StubAdapter::accepting();
        registry.attach_adapter(adapter.clone());

        let task = registry
            .issue(
                SensorId(1),
                SensorCommand::SetMode {
                    mode: SensorMode::Search,
                },
                None,
                MissionTime(10.0),
            )
            .expect("commandable");

        assert_eq!(
            adapter.deliveries(),
            vec![task],
            "the adapter was not handed it"
        );
        assert!(
            matches!(registry.tasks()[0].state, TaskState::Sent),
            "an adapter accepted the task and it is not marked sent: {:?}",
            registry.tasks()[0].state
        );
        // Accepted for delivery is not acknowledged. The sensor has said nothing.
        let status = registry.mode_status(SensorId(1)).expect("known");
        assert_eq!(status.confirmed, SensorMode::Standby);
        assert_eq!(status.requested, Some(SensorMode::Search));

        registry.acknowledge(task, MissionTime(11.0)).expect("open");
        assert_eq!(
            registry.mode_status(SensorId(1)).expect("known").confirmed,
            SensorMode::Search
        );
    }

    /// The stub refuses. The reason is kept verbatim, the confirmed mode does not move,
    /// nothing is left pending, and nothing is retried.
    #[test]
    fn a_stub_that_refuses_records_the_reason_and_changes_nothing() {
        let mut registry = controllable();
        let adapter = StubAdapter::refusing("transmitter inhibited");
        registry.attach_adapter(adapter.clone());

        registry
            .issue(
                SensorId(1),
                SensorCommand::SetMode {
                    mode: SensorMode::Search,
                },
                None,
                MissionTime(10.0),
            )
            .expect("the registry accepted it; the adapter is what refused");

        match &registry.tasks()[0].state {
            TaskState::Failed { reason } => assert!(
                reason.contains("transmitter inhibited"),
                "the adapter's own words did not survive: {reason}"
            ),
            other => panic!("a refused command was recorded as {other:?}"),
        }
        let status = registry.mode_status(SensorId(1)).expect("known");
        assert_eq!(status.confirmed, SensorMode::Standby);
        assert_eq!(
            status.requested, None,
            "a command the adapter refused is still shown as outstanding"
        );

        // No automatic retry: one delivery attempt, one task.
        assert_eq!(adapter.deliveries().len(), 1);
        assert_eq!(registry.tasks().len(), 1);
        let timed_out = registry.sweep(MissionTime(1_000.0), 10.0);
        assert!(
            timed_out.is_empty(),
            "a closed task was swept as though it were still waiting"
        );
        assert_eq!(adapter.deliveries().len(), 1, "the command was re-sent");
    }

    /// The stub accepts and then ignores it. The window closes, the task becomes
    /// unacknowledged, and nothing is sent again -- retry is an operator action.
    #[test]
    fn a_stub_that_ignores_lets_the_task_time_out_and_never_resends() {
        let mut registry = controllable();
        let adapter = StubAdapter::accepting();
        registry.attach_adapter(adapter.clone());

        let task = registry
            .issue(
                SensorId(1),
                SensorCommand::SetMode {
                    mode: SensorMode::Search,
                },
                None,
                MissionTime(10.0),
            )
            .expect("commandable");

        assert!(
            registry.sweep(MissionTime(15.0), 10.0).is_empty(),
            "the task timed out inside its window"
        );
        assert_eq!(registry.sweep(MissionTime(21.0), 10.0), vec![task]);

        assert!(matches!(
            registry.tasks()[0].state,
            TaskState::Unacknowledged
        ));
        assert_eq!(
            registry.mode_status(SensorId(1)).expect("known").confirmed,
            SensorMode::Standby,
            "a command nobody answered moved the confirmed mode"
        );
        assert_eq!(
            registry.mode_status(SensorId(1)).expect("known").requested,
            None
        );
        assert_eq!(
            adapter.deliveries().len(),
            1,
            "the timeout re-sent the command; retry is an operator action"
        );
    }

    /// Attaching nothing is the state of the system, and it has to be visible rather
    /// than inferred from a task that never leaves `Issued`.
    #[test]
    fn a_registry_with_no_adapter_says_so_and_stops_at_issued() {
        let mut registry = controllable();
        assert!(!registry.has_adapter());
        registry
            .issue(
                SensorId(1),
                SensorCommand::SetMode {
                    mode: SensorMode::Search,
                },
                None,
                MissionTime(10.0),
            )
            .expect("commandable");
        assert!(
            matches!(registry.tasks()[0].state, TaskState::Issued),
            "a task claimed to have been sent with nothing to send it: {:?}",
            registry.tasks()[0].state
        );
    }

    fn controllable() -> InMemorySensorRegistry {
        InMemorySensorRegistry::from_config(
            &[SensorConfig {
                id: 1,
                modality: "radar".into(),
                position: [0.0, 0.0, 0.0],
                max_range_m: 50_000.0,
                control_endpoint: Some("radar-control".into()),
                maintenance: Vec::new(),
            }],
            "v1",
        )
    }

    /// DN-11 §5 rule 1, and the single most important assertion in this crate: a mode
    /// the operator asked for and the sensor never took must not be reported as the
    /// mode the sensor is in.
    #[test]
    fn a_commanded_mode_is_not_confirmed_until_it_is_acknowledged() {
        let mut registry = controllable();
        let task = registry
            .issue(
                SensorId(1),
                SensorCommand::SetMode {
                    mode: SensorMode::Search,
                },
                None,
                MissionTime(10.0),
            )
            .expect("a sensor with a control endpoint is commandable");

        let status = registry.mode_status(SensorId(1)).expect("a known sensor");
        assert_eq!(
            status.confirmed,
            SensorMode::Standby,
            "the confirmed mode changed before the sensor acknowledged"
        );
        assert_eq!(status.requested, Some(SensorMode::Search));
        assert!(status.is_pending());
        // Coverage follows the confirmed mode, so an unacknowledged request puts no
        // ring on the map.
        assert!(
            registry.coverage().is_empty(),
            "a requested-but-unconfirmed mode contributed coverage"
        );

        registry.acknowledge(task, MissionTime(12.0)).expect("open");
        let status = registry.mode_status(SensorId(1)).expect("a known sensor");
        assert_eq!(status.confirmed, SensorMode::Search);
        assert_eq!(status.requested, None);
        assert_eq!(registry.coverage().len(), 1);
    }

    /// DN-11 §5 rule 4: a sensor with no control endpoint is not commandable, and
    /// issuing records nothing. A task in the record for a command that was never sent
    /// would be the appearance of success this rule exists to prevent.
    #[test]
    fn a_sensor_with_no_control_endpoint_records_nothing() {
        let mut registry = InMemorySensorRegistry::from_config(
            &[SensorConfig {
                id: 1,
                modality: "radar".into(),
                position: [0.0, 0.0, 0.0],
                max_range_m: 50_000.0,
                control_endpoint: None,
                maintenance: Vec::new(),
            }],
            "v1",
        );
        let err = registry
            .issue(
                SensorId(1),
                SensorCommand::SetMode {
                    mode: SensorMode::Search,
                },
                None,
                MissionTime(0.0),
            )
            .expect_err("no endpoint means not controllable");
        assert!(matches!(err, SensorManagementError::NotControllable(_)));
        assert!(registry.tasks().is_empty(), "a task was recorded anyway");
        assert_eq!(
            registry.mode_status(SensorId(1)).map(|s| s.requested),
            Some(None),
            "a refused command left a pending request"
        );
    }

    /// DN-11 §5 rules 2 and 3: an ignored task becomes unacknowledged inside the
    /// window, the request is dropped rather than left pending for ever, and nothing
    /// is retried.
    #[test]
    fn an_ignored_task_times_out_and_drops_the_request() {
        let mut registry = controllable();
        let task = registry
            .issue(
                SensorId(1),
                SensorCommand::SetMode {
                    mode: SensorMode::Search,
                },
                None,
                MissionTime(0.0),
            )
            .expect("commandable");
        assert!(registry.sweep(MissionTime(5.0), 10.0).is_empty());

        let timed_out = registry.sweep(MissionTime(20.0), 10.0);
        assert_eq!(timed_out, vec![task]);
        assert_eq!(registry.tasks()[0].state, TaskState::Unacknowledged);
        let status = registry.mode_status(SensorId(1)).expect("a known sensor");
        assert_eq!(status.confirmed, SensorMode::Standby);
        assert_eq!(status.requested, None, "a timed-out request stayed pending");
        assert_eq!(registry.tasks().len(), 1, "the task was reissued");
    }

    /// A refused command leaves the sensor doing what it was doing, keeps the reason,
    /// and cannot be reopened: the record is append-only.
    #[test]
    fn a_refused_command_keeps_its_reason_and_changes_nothing() {
        let mut registry = controllable();
        let task = registry
            .issue(
                SensorId(1),
                SensorCommand::SetMode {
                    mode: SensorMode::Track,
                },
                None,
                MissionTime(0.0),
            )
            .expect("commandable");
        registry
            .fail(task, "transmitter fault".into())
            .expect("open");

        assert_eq!(
            registry.tasks()[0].state,
            TaskState::Failed {
                reason: "transmitter fault".into()
            }
        );
        let status = registry.mode_status(SensorId(1)).expect("a known sensor");
        assert_eq!(status.confirmed, SensorMode::Standby);
        assert_eq!(status.requested, None);
        assert!(matches!(
            registry.acknowledge(task, MissionTime(1.0)),
            Err(SensorManagementError::TaskClosed(_))
        ));
    }

    /// An illegal transition is refused before it reaches the sensor, so the panel can
    /// say why. Coming back from Offline must pass through Standby.
    #[test]
    fn an_illegal_transition_is_refused_before_it_is_sent() {
        let mut registry = controllable();
        registry
            .record_observed_mode(SensorId(1), SensorMode::Offline)
            .expect("standby to offline");
        let err = registry
            .issue(
                SensorId(1),
                SensorCommand::SetMode {
                    mode: SensorMode::Track,
                },
                None,
                MissionTime(0.0),
            )
            .expect_err("offline cannot go straight to track");
        assert!(matches!(
            err,
            SensorManagementError::InvalidModeTransition { .. }
        ));
        assert!(registry.tasks().is_empty());
    }

    fn registry() -> InMemorySensorRegistry {
        InMemorySensorRegistry::from_config(
            &[
                SensorConfig {
                    id: 1,
                    modality: "radar".into(),
                    position: [0.0, 0.0, 10.0],
                    max_range_m: 20_000.0,
                    control_endpoint: None,
                    maintenance: Vec::new(),
                },
                SensorConfig {
                    id: 2,
                    modality: "eo-ir".into(),
                    position: [0.1, 0.1, 10.0],
                    max_range_m: 5_000.0,
                    control_endpoint: None,
                    maintenance: Vec::new(),
                },
            ],
            "cal-2026-09",
        )
    }

    /// A registry whose sensor 1 is down for maintenance from 10 s to 20 s.
    fn registry_with_window() -> InMemorySensorRegistry {
        InMemorySensorRegistry::from_config(
            &[SensorConfig {
                id: 1,
                modality: "radar".into(),
                position: [0.0, 0.0, 10.0],
                max_range_m: 20_000.0,
                control_endpoint: None,
                maintenance: vec![gungnir_config::MaintenanceWindowConfig {
                    from_s: 10.0,
                    to_s: 20.0,
                    reason: "antenna swap".into(),
                }],
            }],
            "cal-2026-09",
        )
    }

    /// **The distinction the feature exists for.** The same silent sensor is an expected
    /// absence inside its window and a fault outside one, and nothing else about it
    /// changes.
    #[test]
    fn a_silent_sensor_is_a_fault_outside_its_window_and_not_inside_it() {
        let mut r = registry_with_window();
        r.set_mode(SensorId(1), SensorMode::Offline)
            .expect("go offline");
        let s = r.get(SensorId(1)).expect("sensor 1");

        assert!(s.is_in_maintenance(MissionTime(15.0)));
        assert!(!s.absence_is_a_fault(MissionTime(15.0)));

        assert!(!s.is_in_maintenance(MissionTime(25.0)));
        assert!(
            s.absence_is_a_fault(MissionTime(25.0)),
            "a sensor down outside any window was not reported as a fault"
        );
    }

    /// A sensor that is radiating during its own window is not a fault either, and the
    /// registry does not pretend it is down because the schedule said it would be.
    #[test]
    fn a_sensor_still_radiating_in_its_window_is_not_a_fault() {
        let r = registry_with_window();
        let s = r.get(SensorId(1)).expect("sensor 1");
        assert!(!s.is_down());
        assert!(!s.absence_is_a_fault(MissionTime(15.0)));
    }

    /// Transitions are reported once. An alert repeated every tick is an alert nobody
    /// reads, and a window that is still open is not news.
    #[test]
    fn maintenance_transitions_are_reported_once_each() {
        let mut r = registry_with_window();
        assert!(r.advance_maintenance(MissionTime(5.0)).is_empty());

        let opened = r.advance_maintenance(MissionTime(15.0));
        assert_eq!(opened, vec![(SensorId(1), MaintenanceState::Active)]);
        assert!(
            r.advance_maintenance(MissionTime(16.0)).is_empty(),
            "an open window was reported again"
        );

        let closed = r.advance_maintenance(MissionTime(25.0));
        assert_eq!(closed, vec![(SensorId(1), MaintenanceState::Completed)]);
    }

    /// **The case that needs attention.** The window closed and the sensor never came
    /// back, which is neither an expected absence nor an ordinary failure.
    #[test]
    fn a_sensor_that_does_not_return_overruns_its_window() {
        let mut r = registry_with_window();
        r.set_mode(SensorId(1), SensorMode::Offline)
            .expect("go offline");
        r.advance_maintenance(MissionTime(15.0));

        let after = r.advance_maintenance(MissionTime(25.0));
        assert_eq!(after, vec![(SensorId(1), MaintenanceState::Overrun)]);
        // Terminal: coming back late does not turn a missed window into a completed one.
        r.set_mode(SensorId(1), SensorMode::Standby)
            .expect("back to standby");
        assert!(r.advance_maintenance(MissionTime(30.0)).is_empty());
        assert_eq!(
            r.get(SensorId(1)).expect("sensor 1").maintenance[0].state,
            MaintenanceState::Overrun
        );
    }

    /// What the incoming watch is handed: what is open, what is coming, and what was
    /// missed. A completed window is none of those.
    #[test]
    fn outstanding_maintenance_carries_the_open_the_future_and_the_overrun() {
        let mut r = registry_with_window();
        assert_eq!(r.maintenance_outstanding(MissionTime(5.0)).len(), 1);
        assert_eq!(r.maintenance_outstanding(MissionTime(15.0)).len(), 1);

        r.advance_maintenance(MissionTime(25.0));
        assert!(
            r.maintenance_outstanding(MissionTime(25.0)).is_empty(),
            "a completed window was handed over as outstanding work"
        );

        // But an overrun stays outstanding, because it is unfinished.
        let mut r = registry_with_window();
        r.set_mode(SensorId(1), SensorMode::Offline)
            .expect("go offline");
        r.advance_maintenance(MissionTime(25.0));
        assert_eq!(r.maintenance_outstanding(MissionTime(30.0)).len(), 1);
    }

    #[test]
    fn offline_sensor_must_return_through_standby() {
        let mut r = registry();
        r.set_mode(SensorId(1), SensorMode::Offline)
            .expect("go offline");
        assert!(matches!(
            r.set_mode(SensorId(1), SensorMode::Track),
            Err(SensorManagementError::InvalidModeTransition { .. })
        ));
        r.set_mode(SensorId(1), SensorMode::Standby)
            .expect("back to standby");
        r.set_mode(SensorId(1), SensorMode::Track)
            .expect("then track");
    }

    #[test]
    fn coverage_only_counts_searching_or_tracking_sensors() {
        let mut r = registry();
        assert!(r.coverage().is_empty());
        r.set_mode(SensorId(1), SensorMode::Search).expect("search");
        r.set_mode(SensorId(2), SensorMode::Track).expect("track");
        let cov = r.coverage();
        assert_eq!(cov.len(), 2);
        assert_eq!(cov[0].confidence, 0.7);
        assert_eq!(cov[1].confidence, 1.0);
        assert_eq!(cov[0].radius_m, 20_000.0);
    }

    #[test]
    fn unknown_sensor_is_an_error() {
        let mut r = registry();
        assert!(matches!(
            r.set_mode(SensorId(9), SensorMode::Search),
            Err(SensorManagementError::UnknownSensor(_))
        ));
    }
}
