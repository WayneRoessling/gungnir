//! Battle-rhythm data: scheduled products and planned sensor downtime, per
//! docs/design/DN-21-battle-rhythm.md §3.
//!
//! # Why these are in `gungnir-model` and not where DN-21 §3 put them
//!
//! The note puts `MaintenanceWindow` in `gungnir-sensor-management`, and the first draft
//! of the code put it in `gungnir-reporting`. Four crates need it: the registry, to say a
//! sensor's absence is expected; `gungnir-reporting`, for the handover summary; the
//! configuration, to load one; and the binary, to decide what a silent sensor means.
//! Either of the first two placements forces an edge for the other, and **DN-21 §4 says
//! the design adds no edges** -- which is only true if the type lives where every layer
//! already looks. That is here.
//!
//! # The distinction the whole thing exists for
//!
//! **Expected absence is not the same as failure, and neither is the same as "not a
//! problem".** A sensor down inside a planned window should not raise a failure alert; the
//! coverage gap it leaves is real either way and is still drawn. A sensor that does not
//! come back when its window closes is the case that actually needs attention, and it has
//! its own state so it cannot be read as either of the other two.

use crate::{MissionTime, SensorId};

/// A product the deployment produces on a cycle rather than on demand.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ScheduledProduct {
    pub name: String,
    pub kind: ProductKind,
    pub schedule: Schedule,
    /// Endpoint to deliver to. Absent means the product is produced and held for a
    /// person to read, which is a real configuration and not a failure.
    pub deliver_to: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProductKind {
    /// What happened on this watch, for the people taking over.
    HandoverSummary,
    SituationReport,
    MeasuresSummary,
}

/// A period and an offset, in mission-time seconds.
///
/// Deliberately not a cron expression: a rhythm nobody can read in the panel is a
/// rhythm nobody checks.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Schedule {
    pub period_s: f64,
    pub offset_s: f64,
}

impl Schedule {
    /// The next mission time at or after `now` at which this product is due.
    ///
    /// `None` when the schedule is not usable, which validation rejects at load;
    /// this returns rather than panicking so a bad baseline cannot stop the tick.
    pub fn next_due(&self, now: MissionTime) -> Option<MissionTime> {
        let usable = self.period_s.is_finite() && self.period_s > 0.0 && self.offset_s.is_finite();
        if !usable {
            return None;
        }
        let elapsed = now.0 - self.offset_s;
        if elapsed < 0.0 {
            return Some(MissionTime(self.offset_s));
        }
        let periods = (elapsed / self.period_s).floor();
        Some(MissionTime(self.offset_s + (periods + 1.0) * self.period_s))
    }

    /// Every due time in `[from, to)`, which is what the tick asks for.
    pub fn due_between(&self, from: MissionTime, to: MissionTime) -> Vec<MissionTime> {
        let mut out = Vec::new();
        let Some(mut at) = self.next_due(from) else {
            return out;
        };
        // `next_due` is strictly after `from`, so this terminates for any positive
        // period.
        while at.0 < to.0 {
            out.push(at);
            match self.next_due(at) {
                Some(next) if next.0 > at.0 => at = next,
                _ => break,
            }
        }
        out
    }
}

/// A planned window in which a sensor is expected to be down.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MaintenanceWindow {
    pub sensor: SensorId,
    pub from: MissionTime,
    pub to: MissionTime,
    pub reason: String,
    pub state: MaintenanceState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MaintenanceState {
    Planned,
    /// The window is open and the sensor is expected to be down.
    Active,
    Completed,
    /// The window passed without the sensor coming back. **This is the case that needs
    /// attention**, and the reason planned absence is tracked at all.
    Overrun,
}

impl MaintenanceWindow {
    #[must_use]
    pub fn is_open_at(&self, now: MissionTime) -> bool {
        now >= self.from && now <= self.to
    }

    /// True when the window has passed with the sensor still down.
    #[must_use]
    pub fn has_overrun(&self, now: MissionTime, sensor_is_down: bool) -> bool {
        now > self.to && sensor_is_down && self.state != MaintenanceState::Overrun
    }

    /// The state this window should be in at `now`, given whether the sensor is down.
    ///
    /// Returns `None` when nothing should change, so a caller can tell a transition from
    /// a re-assertion and only publish the former. `Overrun` is terminal: a sensor that
    /// came back late does not retroactively become `Completed`, because the record of a
    /// window that was missed is the thing somebody has to answer for.
    #[must_use]
    pub fn state_at(&self, now: MissionTime, sensor_is_down: bool) -> Option<MaintenanceState> {
        let next = match self.state {
            MaintenanceState::Overrun => return None,
            _ if self.is_open_at(now) => MaintenanceState::Active,
            _ if now > self.to && sensor_is_down => MaintenanceState::Overrun,
            _ if now > self.to => MaintenanceState::Completed,
            _ => MaintenanceState::Planned,
        };
        (next != self.state).then_some(next)
    }
}

/// True when a sensor being offline is expected rather than a failure.
///
/// **This changes the alert, never the picture.** A coverage gap is real whether it was
/// planned or not, and a planner comparing laydowns needs to see it; confusing "expected"
/// with "not a problem" is how a scheduled outage becomes an unnoticed hole.
#[must_use]
pub fn absence_is_planned(
    windows: &[MaintenanceWindow],
    sensor: SensorId,
    now: MissionTime,
) -> bool {
    windows
        .iter()
        .any(|w| w.sensor == sensor && w.is_open_at(now))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(from: f64, to: f64, state: MaintenanceState) -> MaintenanceWindow {
        MaintenanceWindow {
            sensor: SensorId(1),
            from: MissionTime(from),
            to: MissionTime(to),
            reason: "antenna swap".into(),
            state,
        }
    }

    #[test]
    fn a_window_is_open_on_its_boundaries() {
        let w = window(10.0, 20.0, MaintenanceState::Planned);
        assert!(w.is_open_at(MissionTime(10.0)));
        assert!(w.is_open_at(MissionTime(20.0)));
        assert!(!w.is_open_at(MissionTime(9.9)));
        assert!(!w.is_open_at(MissionTime(20.1)));
    }

    #[test]
    fn a_planned_window_becomes_active_then_completed() {
        let w = window(10.0, 20.0, MaintenanceState::Planned);
        assert_eq!(w.state_at(MissionTime(5.0), false), None);
        assert_eq!(
            w.state_at(MissionTime(15.0), true),
            Some(MaintenanceState::Active)
        );
        let active = window(10.0, 20.0, MaintenanceState::Active);
        assert_eq!(
            active.state_at(MissionTime(25.0), false),
            Some(MaintenanceState::Completed)
        );
    }

    /// **The case that needs attention.** A sensor still down after its window closed is
    /// not "in maintenance" any more, and it is not an ordinary failure either.
    #[test]
    fn a_sensor_that_does_not_come_back_overruns() {
        let active = window(10.0, 20.0, MaintenanceState::Active);
        assert!(active.has_overrun(MissionTime(25.0), true));
        assert!(!active.has_overrun(MissionTime(25.0), false));
        assert_eq!(
            active.state_at(MissionTime(25.0), true),
            Some(MaintenanceState::Overrun)
        );
    }

    /// Overrun is terminal. A sensor that came back late does not turn the record of a
    /// missed window into a completed one.
    #[test]
    fn an_overrun_is_not_erased_by_the_sensor_returning() {
        let overrun = window(10.0, 20.0, MaintenanceState::Overrun);
        assert_eq!(overrun.state_at(MissionTime(30.0), false), None);
        assert!(!overrun.has_overrun(MissionTime(30.0), true));
    }

    /// The point of the whole type: an expected absence is distinguishable from a failure.
    #[test]
    fn planned_absence_is_only_planned_inside_a_window_for_that_sensor() {
        let windows = vec![window(10.0, 20.0, MaintenanceState::Active)];
        assert!(absence_is_planned(&windows, SensorId(1), MissionTime(15.0)));
        // Outside the window it is a failure again.
        assert!(!absence_is_planned(
            &windows,
            SensorId(1),
            MissionTime(25.0)
        ));
        // And it says nothing about any other sensor.
        assert!(!absence_is_planned(
            &windows,
            SensorId(2),
            MissionTime(15.0)
        ));
    }
}
