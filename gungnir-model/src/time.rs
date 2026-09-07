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
