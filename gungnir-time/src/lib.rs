// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Time management & synchronization, per docs/gungnir-capabilities.md §5.2.
//! Formalizes what gungnir-fusion-async's out-of-sequence handling already does
//! internally, extended application-wide so ingestion, persistence, and replay all
//! agree on the same time discipline -- not just one crate's internal OOS buffer.
//!
//! Mission time is `gungnir_model::MissionTime`, seconds. Live it is Unix time;
//! in replay it is recorded time, advanced only when the replay is stepped.

use gungnir_model::MissionTime;
use std::time::{SystemTime, UNIX_EPOCH};

/// The two timestamps every observation carries, and which clock produced the
/// source time.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SourceTime {
    pub source_time: MissionTime,
    pub receipt_time: MissionTime,
    pub clock_source_id: u32,
}

impl SourceTime {
    /// Receipt minus source: transport latency, or negative if the source clock is
    /// ahead of ours.
    pub fn lateness_s(&self) -> f64 {
        self.receipt_time.seconds_since(self.source_time)
    }
}

/// What to do with an observation whose source time is earlier than data already
/// processed.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum LateDataPolicy {
    /// Drop it and report it.
    Reject,
    /// Hold it in the out-of-sequence buffer up to `max_lateness_s`, then drop.
    BufferAndReorder { max_lateness_s: f64 },
    /// Process it as if it were in order (replay and testing only).
    AcceptAsIs,
}

/// The single authority every ingest adapter, the event journal, and replay all
/// read mission time from -- live wall-clock in production, deterministic
/// step-controlled time during replay/testing.
/// Where a clock's time comes from.
///
/// The status strip (PN-01) shows this beside the time, because a replayed session
/// displaying a wall clock -- or the reverse -- makes every timestamp on screen mean
/// something other than what it says
/// (`docs/ux/information-architecture.md` §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ClockSource {
    /// The host's wall clock; mission time is Unix seconds.
    Wall,
    /// A recorded session stepped by a replay; mission time is recorded time.
    Replay,
}

pub trait TimeAuthority: Send + Sync {
    fn now(&self) -> MissionTime;
    fn late_data_policy(&self) -> LateDataPolicy;

    /// Which kind of clock this is.
    ///
    /// No default: an implementation that did not say would be displayed as a wall
    /// clock, and a replay mislabelled as live is exactly the confusion the strip
    /// exists to prevent.
    fn source(&self) -> ClockSource;
    /// Synchronization health across configured sensor clock sources -- feeds
    /// gungnir-observability, not just an internal metric.
    fn sync_health(&self) -> SyncHealth;
}

#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct SyncHealth {
    pub max_clock_skew_s: f32,
    pub sources_out_of_sync: u32,
    /// How many sources the figures above are about (GAP-008).
    ///
    /// **Zero makes the zeros above honest**: "no skew across no sources" is a statement
    /// about nothing, where "no skew" alone would be a claim that every clock agrees.
    #[serde(default)]
    pub sources_observed: u32,
}

/// Per-source clock offset, estimated from what every accepted detection already carries
/// (GAP-008, MOP-09).
///
/// A detection's `receipt_time - source_time` is transit delay plus the source's clock
/// error. Transit delay is small and positive on the links this system runs over; a
/// source whose clock is wrong shows as an offset that is large, or negative, or both --
/// and negative is the tell, because a message cannot arrive before it was sent. The
/// estimate is the **smallest** offset seen per source, which is the sample with the least
/// transit delay in it and therefore the closest to the clock error alone.
///
/// Pure and stateful by design: it is fed by the binary from ingest events, so the
/// gateway -- which is human-owned -- is not touched, and it can be driven by a test.
#[derive(Debug, Clone, Default)]
pub struct ClockSkewEstimator {
    /// Smallest `receipt - source` seen per source, seconds.
    least_offset_s: std::collections::BTreeMap<u32, f64>,
    /// Last `receipt - source` seen per source, for the panel.
    latest_offset_s: std::collections::BTreeMap<u32, f64>,
}

impl ClockSkewEstimator {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// One accepted detection from `source`.
    pub fn observe(&mut self, source: u32, source_time: MissionTime, receipt_time: MissionTime) {
        let offset = receipt_time.0 - source_time.0;
        if !offset.is_finite() {
            return;
        }
        self.least_offset_s
            .entry(source)
            .and_modify(|least| *least = least.min(offset))
            .or_insert(offset);
        self.latest_offset_s.insert(source, offset);
    }

    /// The estimated clock error of one source, seconds, if it has been heard from.
    #[must_use]
    pub fn skew_s(&self, source: u32) -> Option<f64> {
        self.least_offset_s.get(&source).copied()
    }

    /// Every source heard from, with its estimated skew.
    #[must_use]
    pub fn sources(&self) -> Vec<(u32, f64)> {
        self.least_offset_s.iter().map(|(s, o)| (*s, *o)).collect()
    }

    /// Whether a source's skew exceeds what the late-data policy tolerates.
    ///
    /// A source that is *ahead* (negative offset) is out of sync at any magnitude beyond
    /// jitter, because arrival before sending is impossible; a source that is *behind* is
    /// out of sync once its lag exceeds the policy's buffer, because its detections will be
    /// dropped as late by the policy the deployment chose.
    #[must_use]
    pub fn is_out_of_sync(&self, source: u32, policy: LateDataPolicy) -> bool {
        let Some(skew) = self.skew_s(source) else {
            return false;
        };
        match policy {
            LateDataPolicy::BufferAndReorder { max_lateness_s } => {
                skew < -AHEAD_TOLERANCE_S || skew > max_lateness_s
            }
            // Reject: anything late is dropped, so any lag at all is out of sync; ahead
            // is out of sync as above.
            LateDataPolicy::Reject => skew < -AHEAD_TOLERANCE_S || skew > 0.0,
            // Accept-as-is is replay and testing: nothing is out of sync by definition.
            LateDataPolicy::AcceptAsIs => false,
        }
    }

    /// The health summary the panel and `TimeAuthority::sync_health` report.
    #[must_use]
    pub fn health(&self, policy: LateDataPolicy) -> SyncHealth {
        let sources_observed = self.least_offset_s.len().try_into().unwrap_or(u32::MAX);
        let max_clock_skew_s = self
            .least_offset_s
            .values()
            .map(|o| o.abs())
            .fold(0.0_f64, f64::max);
        let sources_out_of_sync = self
            .least_offset_s
            .keys()
            .filter(|s| self.is_out_of_sync(**s, policy))
            .count()
            .try_into()
            .unwrap_or(u32::MAX);
        SyncHealth {
            // Narrowed for the panel; skews are seconds, not nanoseconds.
            #[allow(clippy::cast_possible_truncation)]
            max_clock_skew_s: max_clock_skew_s as f32,
            sources_out_of_sync,
            sources_observed,
        }
    }
}

/// How far ahead a source may appear before it is out of sync: jitter between two clocks
/// that are both right, not a policy value.
const AHEAD_TOLERANCE_S: f64 = 0.25;

/// Unix wall-clock time. Clock-skew tracking across sensor sources is not yet
/// implemented, so `sync_health` reports no sources out of sync.
#[derive(Debug, Clone, Copy)]
pub struct WallClockAuthority {
    policy: LateDataPolicy,
}

impl WallClockAuthority {
    pub fn new(policy: LateDataPolicy) -> Self {
        Self { policy }
    }
}

impl Default for WallClockAuthority {
    /// Buffer late data for up to two seconds -- a conservative multi-sensor default
    /// until `gungnir-config` carries a per-deployment value.
    fn default() -> Self {
        Self::new(LateDataPolicy::BufferAndReorder {
            max_lateness_s: 2.0,
        })
    }
}

impl TimeAuthority for WallClockAuthority {
    fn source(&self) -> ClockSource {
        ClockSource::Wall
    }

    fn now(&self) -> MissionTime {
        let since_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        MissionTime(since_epoch.as_secs_f64())
    }

    fn late_data_policy(&self) -> LateDataPolicy {
        self.policy
    }

    /// **No sources observed**, which is the honest form of the zeros this used to
    /// return. The wall clock authority holds no estimator: the binary owns a
    /// [`ClockSkewEstimator`] fed from ingest events and reports through it (GAP-008).
    fn sync_health(&self) -> SyncHealth {
        SyncHealth::default()
    }
}

/// Deterministic, step-controlled clock for replay (gungnir-replay) and testing
/// (gungnir-scenario round-trip fidelity) -- advances only when told to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReplayClockAuthority {
    pub current: MissionTime,
}

impl ReplayClockAuthority {
    pub fn advance(&mut self, seconds: f64) {
        self.current = MissionTime(self.current.0 + seconds);
    }
}

impl TimeAuthority for ReplayClockAuthority {
    fn source(&self) -> ClockSource {
        ClockSource::Replay
    }

    fn now(&self) -> MissionTime {
        self.current
    }

    fn late_data_policy(&self) -> LateDataPolicy {
        LateDataPolicy::AcceptAsIs
    }

    fn sync_health(&self) -> SyncHealth {
        SyncHealth {
            max_clock_skew_s: 0.0,
            sources_out_of_sync: 0,
            sources_observed: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wall_clock_is_monotone_non_decreasing_and_plausible() {
        let clock = WallClockAuthority::default();
        let a = clock.now();
        let b = clock.now();
        assert!(b >= a);
        assert!(a.0 > 1.6e9, "expected Unix seconds, got {}", a.0);
    }

    #[test]
    fn replay_clock_only_moves_when_stepped() {
        let mut clock = ReplayClockAuthority {
            current: MissionTime(5.0),
        };
        assert_eq!(clock.now(), MissionTime(5.0));
        assert_eq!(clock.now(), MissionTime(5.0));
        clock.advance(2.5);
        assert_eq!(clock.now(), MissionTime(7.5));
    }

    #[test]
    fn lateness_is_receipt_minus_source() {
        let t = SourceTime {
            source_time: MissionTime(10.0),
            receipt_time: MissionTime(10.25),
            clock_source_id: 1,
        };
        assert!((t.lateness_s() - 0.25).abs() < 1e-12);
    }
}

#[cfg(test)]
mod skew_tests {
    use super::*;

    const BUFFERED: LateDataPolicy = LateDataPolicy::BufferAndReorder {
        max_lateness_s: 2.0,
    };

    /// **Nothing observed says nothing.** The zeros this replaced claimed every clock
    /// agreed.
    #[test]
    fn an_unfed_estimator_reports_no_sources_rather_than_no_skew() {
        let h = ClockSkewEstimator::new().health(BUFFERED);
        assert_eq!(h.sources_observed, 0);
        assert_eq!(
            WallClockAuthority::default().sync_health().sources_observed,
            0
        );
    }

    /// The least offset is the estimate: transit delay only ever adds, so the smallest
    /// sample is the one closest to the clock error alone.
    #[test]
    fn the_estimate_is_the_least_offset_seen() {
        let mut e = ClockSkewEstimator::new();
        e.observe(1, MissionTime(10.0), MissionTime(10.4));
        e.observe(1, MissionTime(11.0), MissionTime(11.1));
        e.observe(1, MissionTime(12.0), MissionTime(12.9));
        assert!((e.skew_s(1).expect("heard") - 0.1).abs() < 1e-9);
        assert_eq!(e.health(BUFFERED).sources_observed, 1);
        assert_eq!(e.health(BUFFERED).sources_out_of_sync, 0);
    }

    /// A source ahead of the receiver is impossible on an honest clock, and is out of sync
    /// at any magnitude beyond jitter.
    #[test]
    fn a_source_ahead_of_the_receiver_is_out_of_sync() {
        let mut e = ClockSkewEstimator::new();
        e.observe(2, MissionTime(10.0), MissionTime(9.0));
        assert!(e.is_out_of_sync(2, BUFFERED));
        assert_eq!(e.health(BUFFERED).sources_out_of_sync, 1);
    }

    /// A source behind by more than the policy buffers is out of sync, because its
    /// detections will be dropped as late -- and the same lag under a longer buffer is
    /// not. **The threshold is the deployment's policy, not a number chosen here**
    /// (MOP-09).
    #[test]
    fn out_of_sync_is_judged_against_the_late_data_policy() {
        let mut e = ClockSkewEstimator::new();
        e.observe(3, MissionTime(10.0), MissionTime(13.0));
        assert!(e.is_out_of_sync(3, BUFFERED));
        assert!(!e.is_out_of_sync(
            3,
            LateDataPolicy::BufferAndReorder {
                max_lateness_s: 5.0
            }
        ));
        assert!(!e.is_out_of_sync(3, LateDataPolicy::AcceptAsIs));
    }
}
