// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Mission-time primitive shared across the model, gungnir-time, gungnir-eventing,
//! and gungnir-store, so every crate agrees on one time representation.
//!
//! Mission time is seconds as an `f64`. In the live profiles it is Unix time from
//! `gungnir_time::WallClockAuthority`; during replay it is whatever the journal
//! recorded, advanced under `gungnir_time::ReplayClockAuthority`. Source-time versus
//! receipt-time semantics are carried on `DetectionView`, not here.

#[derive(
    Debug, Clone, Copy, Default, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize,
)]
pub struct MissionTime(pub f64);

impl MissionTime {
    /// Seconds elapsed from `earlier` to `self` (negative if `self` is earlier).
    pub fn seconds_since(self, earlier: MissionTime) -> f64 {
        self.0 - earlier.0
    }
}

/// A detection's two time stamps, carried on a quarantine the gateway made under one of
/// its clock rules (GAP-205): a report whose source time is too far ahead of its receipt,
/// or whose receipt is in the future.
///
/// **Only on a clock-rule quarantine, and only after authentication.** The gateway
/// authenticates a source before it validates, so a report that reaches a clock rule came
/// from the source it names; its stamps are what lets a clock disciplined from that
/// source hear it while the host clock is still behind, which is exactly when the rule
/// refuses it. On every other quarantine the stamps are absent.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ClockStamps {
    pub source_time: MissionTime,
    pub receipt_time: MissionTime,
}

/// What a clock disciplined from a named sensor is doing (GAP-205; plan 12 tranche A).
///
/// A free-running host clock against a GNSS-disciplined sensor drifts past the late-data
/// buffer within hours, and the pipeline then drops that sensor's reports as late. A
/// disciplined clock follows the sensor's own time stamps instead, so mission time is the
/// sensor's time to within its least transit delay.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum ClockDisciplineState {
    /// The sensor has not been heard from: mission time is the host clock, and says so.
    Waiting,
    /// Following the sensor: `offset_s` is how far the host clock is ahead of it (negative
    /// when behind), estimated as the least `receipt - source` over a sliding window, the
    /// sample with the least transit delay in it.
    Locked {
        offset_s: f64,
        samples: u32,
        last_sample: MissionTime,
    },
    /// Locked once, and nothing heard from the sensor for longer than the holdover limit:
    /// the last offset is still applied, and the age of the sample it rests on is said.
    Holdover {
        offset_s: f64,
        last_sample: MissionTime,
    },
    /// The sensor's time is further from the host clock than a discipline will step, so
    /// it is not applied: a host clock wrong by more than that is set by hand, because a
    /// mission clock stepped by hours would move every deadline and every expiry with it.
    Refused { offset_s: f64 },
}

impl ClockDisciplineState {
    /// The state without its figures, for "did this change" questions.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            ClockDisciplineState::Waiting => "waiting",
            ClockDisciplineState::Locked { .. } => "locked",
            ClockDisciplineState::Holdover { .. } => "holdover",
            ClockDisciplineState::Refused { .. } => "refused",
        }
    }
}
