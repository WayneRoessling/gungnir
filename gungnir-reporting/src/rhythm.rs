// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Battle rhythm: scheduled products and the handover summary.
//!
//! Design: docs/design/DN-21-battle-rhythm.md. Capability CAP-5.8; measure MOE-13.
//! A watch runs on a rhythm and none of it is supported today, so handover depends
//! on memory.
//!
//! **The scheduler runs on mission time**, so a replayed session produces the same
//! products at the same moments. A wall-clock scheduler would make a replay produce
//! a different set, which breaks the determinism the journal rests on. That is the
//! one property here worth a dedicated test, because a wall-clock implementation
//! would pass every other check.
//!
//! **The handover summary is assembled from the record and completed by a person.**
//! Every field except the notes comes from state the system already holds. The
//! summary is not complete until the incoming watch acknowledges it with a name and
//! a time, and that acknowledgement is what MOE-13 counts.

use gungnir_model::{AssetId, DecisionId, MissionTime, SensorId};

// The schedule and maintenance types were defined here in the first draft and now live in
// `gungnir-model::rhythm` (see its module docs for why): the registry, the configuration,
// the binary and this crate all need them, and any placement inside one consumer would
// have forced an edge for another -- which DN-21 §4 forbids. What stays here is
// `HandoverSummary`, which is assembled from the record and is genuinely this crate's.
// Re-exported rather than redefined, per the standing rule about types `gungnir-model`
// owns.
pub use gungnir_model::{
    absence_is_planned, MaintenanceState, MaintenanceWindow, ProductKind, Schedule,
    ScheduledProduct,
};

/// What the incoming watch needs, assembled from the record.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HandoverSummary {
    pub period: (MissionTime, MissionTime),
    pub outgoing: Option<String>,
    pub open_alerts: usize,
    pub open_engagements: Vec<DecisionId>,
    pub pending_approvals: usize,
    pub expired_approvals: usize,
    pub sensors_degraded: Vec<SensorId>,
    pub maintenance_due: Vec<MaintenanceWindow>,
    pub warnings_owed: Vec<AssetId>,
    pub baseline_version: u32,
    /// Free text the outgoing watch adds.
    ///
    /// The one part the system does not assemble, and the part that matters most.
    pub notes: Option<String>,
    /// Who took over, and when. Until this is set the handover is incomplete.
    pub acknowledged_by: Option<(String, MissionTime)>,
}

impl HandoverSummary {
    /// True when the incoming watch has acknowledged with a name and a time.
    ///
    /// MOE-13 counts these, so an unacknowledged handover is visible rather than
    /// assumed.
    pub fn is_complete(&self) -> bool {
        self.acknowledged_by.is_some()
    }

    /// Records the incoming watch taking over.
    pub fn acknowledge(
        &mut self,
        by: impl Into<String>,
        at: MissionTime,
    ) -> Result<(), RhythmError> {
        let by = by.into();
        if by.trim().is_empty() {
            return Err(RhythmError::NameRequired);
        }
        if self.is_complete() {
            return Err(RhythmError::AlreadyAcknowledged);
        }
        self.acknowledged_by = Some((by, at));
        Ok(())
    }

    /// True when anything in the period still needs someone's attention.
    pub fn has_outstanding_work(&self) -> bool {
        self.open_alerts > 0
            || !self.open_engagements.is_empty()
            || self.pending_approvals > 0
            || !self.warnings_owed.is_empty()
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RhythmError {
    #[error("a handover acknowledgement must name who took over")]
    NameRequired,
    #[error("this handover was already acknowledged")]
    AlreadyAcknowledged,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schedule(period_s: f64, offset_s: f64) -> Schedule {
        Schedule { period_s, offset_s }
    }

    fn window(from: f64, to: f64, state: MaintenanceState) -> MaintenanceWindow {
        MaintenanceWindow {
            sensor: SensorId(1),
            from: MissionTime(from),
            to: MissionTime(to),
            reason: "planned calibration".into(),
            state,
        }
    }

    fn summary() -> HandoverSummary {
        HandoverSummary {
            period: (MissionTime(0.0), MissionTime(3_600.0)),
            outgoing: Some("watch-a".into()),
            open_alerts: 0,
            open_engagements: Vec::new(),
            pending_approvals: 0,
            expired_approvals: 0,
            sensors_degraded: Vec::new(),
            maintenance_due: Vec::new(),
            warnings_owed: Vec::new(),
            baseline_version: 1,
            notes: None,
            acknowledged_by: None,
        }
    }

    #[test]
    fn a_replayed_period_produces_the_same_products_at_the_same_mission_times() {
        // The property that proves the scheduler runs on mission time. A wall-clock
        // implementation would pass every other test here.
        let s = schedule(600.0, 0.0);
        let first = s.due_between(MissionTime(0.0), MissionTime(1_900.0));
        let replayed = s.due_between(MissionTime(0.0), MissionTime(1_900.0));
        assert_eq!(first, replayed);
        assert_eq!(
            first,
            vec![
                MissionTime(600.0),
                MissionTime(1_200.0),
                MissionTime(1_800.0)
            ]
        );
    }

    #[test]
    fn the_offset_shifts_the_cycle() {
        let s = schedule(600.0, 100.0);
        assert_eq!(s.next_due(MissionTime(0.0)), Some(MissionTime(100.0)));
        assert_eq!(s.next_due(MissionTime(100.0)), Some(MissionTime(700.0)));
        assert_eq!(s.next_due(MissionTime(650.0)), Some(MissionTime(700.0)));
    }

    #[test]
    fn an_unusable_schedule_yields_nothing_rather_than_stalling_the_tick() {
        for bad in [0.0, -60.0, f64::NAN, f64::INFINITY] {
            let s = schedule(bad, 0.0);
            assert!(s.next_due(MissionTime(0.0)).is_none());
            assert!(s
                .due_between(MissionTime(0.0), MissionTime(10_000.0))
                .is_empty());
        }
    }

    #[test]
    fn a_sensor_down_inside_its_window_is_expected_and_outside_it_is_not() {
        let windows = [window(100.0, 200.0, MaintenanceState::Active)];
        assert!(absence_is_planned(
            &windows,
            SensorId(1),
            MissionTime(150.0)
        ));
        assert!(!absence_is_planned(
            &windows,
            SensorId(1),
            MissionTime(250.0)
        ));
        assert!(
            !absence_is_planned(&windows, SensorId(2), MissionTime(150.0)),
            "another sensor's window explains nothing"
        );
    }

    #[test]
    fn a_sensor_that_does_not_return_by_the_window_end_has_overrun() {
        let w = window(100.0, 200.0, MaintenanceState::Active);
        assert!(!w.has_overrun(MissionTime(150.0), true), "still inside");
        assert!(
            !w.has_overrun(MissionTime(250.0), false),
            "it came back on time"
        );
        assert!(
            w.has_overrun(MissionTime(250.0), true),
            "past the end and still down"
        );

        let already = window(100.0, 200.0, MaintenanceState::Overrun);
        assert!(
            !already.has_overrun(MissionTime(250.0), true),
            "it is reported once"
        );
    }

    #[test]
    fn a_handover_is_incomplete_until_someone_acknowledges_it() {
        let mut s = summary();
        assert!(!s.is_complete());
        s.acknowledge("watch-b", MissionTime(3_600.0))
            .expect("acknowledges");
        assert!(s.is_complete());
        assert_eq!(
            s.acknowledged_by.as_ref().map(|(who, _)| who.as_str()),
            Some("watch-b")
        );
    }

    #[test]
    fn an_acknowledgement_must_name_who_took_over() {
        let mut s = summary();
        assert_eq!(
            s.acknowledge("  ", MissionTime(3_600.0)),
            Err(RhythmError::NameRequired)
        );
        assert!(!s.is_complete());
    }

    #[test]
    fn a_handover_is_acknowledged_once() {
        let mut s = summary();
        s.acknowledge("watch-b", MissionTime(3_600.0))
            .expect("first");
        assert_eq!(
            s.acknowledge("watch-c", MissionTime(3_700.0)),
            Err(RhythmError::AlreadyAcknowledged)
        );
    }

    #[test]
    fn outstanding_work_is_surfaced_for_the_incoming_watch() {
        let mut s = summary();
        assert!(!s.has_outstanding_work());
        s.warnings_owed.push(AssetId(1));
        assert!(s.has_outstanding_work());

        let mut engaged = summary();
        engaged.open_engagements.push(DecisionId(4));
        assert!(engaged.has_outstanding_work());
    }

    #[test]
    fn the_notes_field_is_the_one_part_the_system_does_not_assemble() {
        let mut s = summary();
        assert!(s.notes.is_none(), "nothing writes this but a person");
        s.notes = Some("the port cell is short-handed tonight".into());
        assert!(s.notes.is_some());
    }
}
