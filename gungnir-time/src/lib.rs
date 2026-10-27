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
/// taken.
///
/// **Owned by `gungnir-core` and re-exported here** (GAP-114, D-98), so the fusion
/// pipeline -- which sits below this crate and is where a late detection is actually
/// dropped, reordered or accepted -- names the same type this crate's clock-skew estimate
/// judges against. Before GAP-114 it was defined here and nothing applied it: the
/// pipeline ran a reorder horizon of its own that no baseline set.
pub use gungnir_model::LateDataPolicy;

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
    /// The host's clock corrected by a named sensor's own time stamps (GAP-205):
    /// [`DisciplinedClockAuthority`]. Shown as its own kind, because a strip that said
    /// "wall" over a clock following a radar would be saying something the host clock
    /// is not.
    Disciplined,
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

    /// Forget every estimate (GAP-205): called when a disciplined clock locks or
    /// re-locks, because every offset measured before was measured against a different
    /// clock, and the least offset would otherwise keep a source out of sync for ever.
    pub fn reset(&mut self) {
        self.least_offset_s.clear();
        self.latest_offset_s.clear();
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
    /// [`LateDataPolicy::default`]: the one-second buffer the pipeline runs when a
    /// baseline names no policy. Both binaries build this authority from the baseline's
    /// `time.late_data` instead (GAP-114), the same value they hand the pipeline, so the
    /// skew the health panel calls out of sync is the lag at which the pipeline starts
    /// dropping. Until GAP-114 this default was two seconds while the pipeline dropped at
    /// one, and a source lagging between the two lost detections with no alert.
    fn default() -> Self {
        Self::new(LateDataPolicy::default())
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

/// The window over which a disciplined clock takes its least offset, seconds of host
/// time (GAP-205). Long enough to hold several reports from a radar rotating at a few
/// seconds a scan, short enough that a host clock drifting a few parts per million is
/// followed rather than pinned to an old minimum.
pub const DISCIPLINE_WINDOW_S: f64 = 60.0;

/// How long a disciplined clock goes without hearing its sensor before it says it is in
/// holdover, seconds of host time (GAP-205). It keeps applying the last offset; the
/// state changes so the record and PN-09 say the offset is ageing.
pub const HOLDOVER_AFTER_S: f64 = 30.0;

/// The largest offset a discipline will apply, seconds (GAP-205). A host clock wrong by
/// more than an hour is set by hand: a mission clock stepped by hours would move every
/// deadline, every token expiry and every retention window with it, and that is not a
/// correction a sensor's time stamps should be able to make on their own.
pub const MAX_DISCIPLINE_OFFSET_S: f64 = 3600.0;

/// The host's own clock, for a disciplined clock to correct. A function so a test can
/// give it a clock it steps.
pub type HostClock = std::sync::Arc<dyn Fn() -> f64 + Send + Sync>;

fn host_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

/// What a disciplined clock holds between calls.
#[derive(Debug)]
struct Discipline {
    sensor: u32,
    /// `(host receipt, receipt - source)` per sample, oldest first, within the window.
    samples: std::collections::VecDeque<(f64, f64)>,
    /// The offset applied: host time minus mission time. `None` until the first lock.
    offset: Option<f64>,
    count: u32,
    last_sample_host: Option<f64>,
    /// The latest window's estimate when it was beyond [`MAX_DISCIPLINE_OFFSET_S`].
    refused: Option<f64>,
    /// The kind [`DisciplineHandle::poll`] last reported.
    reported: Option<&'static str>,
    /// The last mission time handed out, so the clock never goes backwards except at a
    /// step the record names.
    last_returned: Option<f64>,
    /// A lock or re-lock happened since the last `now`: the next `now` may step.
    step_pending: bool,
}

impl Discipline {
    fn state(&self, host_now: f64) -> gungnir_model::ClockDisciplineState {
        use gungnir_model::ClockDisciplineState as S;
        if let Some(offset_s) = self.refused {
            return S::Refused { offset_s };
        }
        match (self.offset, self.last_sample_host) {
            (Some(offset_s), Some(last)) => {
                let last_sample = MissionTime(last - offset_s);
                if host_now - last > HOLDOVER_AFTER_S {
                    S::Holdover {
                        offset_s,
                        last_sample,
                    }
                } else {
                    S::Locked {
                        offset_s,
                        samples: self.count,
                        last_sample,
                    }
                }
            }
            _ => S::Waiting,
        }
    }
}

fn lock(inner: &std::sync::Mutex<Discipline>) -> std::sync::MutexGuard<'_, Discipline> {
    // A discipline is plain numbers; a panic mid-update leaves them consistent enough to
    // keep a clock running, and a clock that stopped because of a poisoned lock would be
    // worse than one a sample behind.
    inner
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The host's clock corrected by a named sensor's own time stamps (GAP-205; plan 12
/// tranche A). The sensor is any one the baseline configures, by the owner's choice of
/// 2026-10-26.
///
/// **The estimate.** A report from the sensor carries the sensor's source time and the
/// receipt time this clock stamped. `receipt - source` is transit delay plus how far the
/// host clock is from the sensor's, and the least of it over [`DISCIPLINE_WINDOW_S`] is
/// the sample with the least transit delay, closest to the clock error alone, the same
/// reasoning [`ClockSkewEstimator`] applies. Mission time is the host clock minus that
/// offset, so it agrees with the sensor to within the sensor's least transit delay.
///
/// **No feedback.** Receipt times are stamped by this clock, already corrected, so the
/// caller passes the offset that was applied when the reports were stamped
/// ([`DisciplineHandle::applied_offset`], read beside `now` at the tick that stamped
/// them) and the estimate is formed in host time.
///
/// **Before the sensor is heard** mission time is the host clock and the state says
/// `Waiting` (the owner's choice: run, and say so). The first lock may step the clock
/// either way; after it, mission time never goes backwards, and a re-lock after holdover
/// may step again. Each of those is a state change the binary journals as
/// `HealthEvent::ClockDiscipline`, so every step is on the record.
pub struct DisciplinedClockAuthority {
    policy: LateDataPolicy,
    host: HostClock,
    inner: std::sync::Arc<std::sync::Mutex<Discipline>>,
}

impl std::fmt::Debug for DisciplinedClockAuthority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DisciplinedClockAuthority")
            .field("policy", &self.policy)
            .field("discipline", &*lock(&self.inner))
            .finish_non_exhaustive()
    }
}

/// The side of a [`DisciplinedClockAuthority`] its binary feeds and reads.
#[derive(Clone)]
pub struct DisciplineHandle {
    host: HostClock,
    inner: std::sync::Arc<std::sync::Mutex<Discipline>>,
}

impl std::fmt::Debug for DisciplineHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DisciplineHandle")
            .field("discipline", &*lock(&self.inner))
            .finish_non_exhaustive()
    }
}

impl DisciplinedClockAuthority {
    /// A clock disciplined from `sensor`, judging late data by `policy`, on the host's
    /// own clock.
    #[must_use]
    pub fn new(policy: LateDataPolicy, sensor: u32) -> (Self, DisciplineHandle) {
        Self::with_host(policy, sensor, std::sync::Arc::new(host_seconds))
    }

    /// The same on a host clock the caller supplies, so a test can step it.
    #[must_use]
    pub fn with_host(
        policy: LateDataPolicy,
        sensor: u32,
        host: HostClock,
    ) -> (Self, DisciplineHandle) {
        let inner = std::sync::Arc::new(std::sync::Mutex::new(Discipline {
            sensor,
            samples: std::collections::VecDeque::new(),
            offset: None,
            count: 0,
            last_sample_host: None,
            refused: None,
            reported: None,
            last_returned: None,
            step_pending: false,
        }));
        (
            Self {
                policy,
                host: host.clone(),
                inner: inner.clone(),
            },
            DisciplineHandle { host, inner },
        )
    }
}

impl TimeAuthority for DisciplinedClockAuthority {
    fn source(&self) -> ClockSource {
        ClockSource::Disciplined
    }

    fn now(&self) -> MissionTime {
        let host = (self.host)();
        let mut d = lock(&self.inner);
        let mut t = host - d.offset.unwrap_or(0.0);
        if let Some(last) = d.last_returned {
            if !d.step_pending && t < last {
                t = last;
            }
        }
        d.step_pending = false;
        d.last_returned = Some(t);
        MissionTime(t)
    }

    fn late_data_policy(&self) -> LateDataPolicy {
        self.policy
    }

    /// No sources observed, as the wall clock says: the binary owns the
    /// [`ClockSkewEstimator`] and the discipline's state is read off its handle.
    fn sync_health(&self) -> SyncHealth {
        SyncHealth::default()
    }
}

impl DisciplineHandle {
    /// The sensor this clock is disciplined from.
    #[must_use]
    pub fn sensor(&self) -> u32 {
        lock(&self.inner).sensor
    }

    /// The offset the clock subtracts now. Read beside the `now` that stamps a tick's
    /// receipts, and passed to [`Self::observe`] for each report of that tick.
    #[must_use]
    pub fn applied_offset(&self) -> f64 {
        lock(&self.inner).offset.unwrap_or(0.0)
    }

    /// One report from `sensor`, stamped at receipt under `applied` (see the type's
    /// documentation). Reports from any other sensor, and non-finite stamps, are
    /// ignored.
    pub fn observe(
        &self,
        sensor: u32,
        source_time: MissionTime,
        receipt_time: MissionTime,
        applied: f64,
    ) {
        let mut d = lock(&self.inner);
        if sensor != d.sensor {
            return;
        }
        let host_receipt = receipt_time.0 + applied;
        let sample = host_receipt - source_time.0;
        if !(host_receipt.is_finite() && sample.is_finite()) {
            return;
        }
        d.count = d.count.saturating_add(1);
        d.last_sample_host = Some(host_receipt);
        d.samples.push_back((host_receipt, sample));
        while d
            .samples
            .front()
            .is_some_and(|(at, _)| host_receipt - at > DISCIPLINE_WINDOW_S)
        {
            d.samples.pop_front();
        }
        let estimate = d
            .samples
            .iter()
            .map(|(_, s)| *s)
            .fold(f64::INFINITY, f64::min);
        if estimate.abs() > MAX_DISCIPLINE_OFFSET_S {
            d.refused = Some(estimate);
            return;
        }
        d.refused = None;
        // A first lock steps; so does a re-lock after holdover, because the window it
        // rests on is new. Each is the state change the binary journals.
        let was = d.state(host_receipt).kind();
        let relock = d.offset.is_none() || was == "holdover";
        d.offset = Some(estimate);
        if relock {
            d.step_pending = true;
        }
    }

    /// The discipline's state now.
    #[must_use]
    pub fn state(&self) -> gungnir_model::ClockDisciplineState {
        let host = (self.host)();
        lock(&self.inner).state(host)
    }

    /// The state, when its kind changed since the last call: the moment the binary
    /// journals `HealthEvent::ClockDiscipline` and says it. The first call reports the
    /// state the discipline starts in, so the record says from when mission time was the
    /// host's.
    #[must_use]
    pub fn poll(&self) -> Option<gungnir_model::ClockDisciplineState> {
        let host = (self.host)();
        let mut d = lock(&self.inner);
        let state = d.state(host);
        if d.reported == Some(state.kind()) {
            return None;
        }
        d.reported = Some(state.kind());
        Some(state)
    }
}

/// The clock a binary runs on: disciplined from `discipline` when the baseline names a
/// sensor (GAP-205), the host's wall clock otherwise, both judging late data by `policy`.
#[must_use]
pub fn clock_for(
    policy: LateDataPolicy,
    discipline: Option<u32>,
) -> (Box<dyn TimeAuthority>, Option<DisciplineHandle>) {
    match discipline {
        Some(sensor) => {
            let (clock, handle) = DisciplinedClockAuthority::new(policy, sensor);
            (Box::new(clock), Some(handle))
        }
        None => (Box::new(WallClockAuthority::new(policy)), None),
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

#[cfg(test)]
mod discipline_tests {
    use super::*;
    use gungnir_model::ClockDisciplineState as S;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    const POLICY: LateDataPolicy = LateDataPolicy::BufferAndReorder {
        max_lateness_s: 1.0,
    };

    /// A host clock the test steps, in milliseconds so it fits an atomic.
    fn stepped(start_s: f64) -> (Arc<AtomicU64>, HostClock) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let ms = Arc::new(AtomicU64::new((start_s * 1000.0) as u64));
        let read = ms.clone();
        #[allow(clippy::cast_precision_loss)]
        let host: HostClock = Arc::new(move || read.load(Ordering::SeqCst) as f64 / 1000.0);
        (ms, host)
    }

    fn set(ms: &AtomicU64, s: f64) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        ms.store((s * 1000.0) as u64, Ordering::SeqCst);
    }

    /// One tick: the clock stamps receipts with `now` and the binary reads the applied
    /// offset beside it, then feeds each report. `true_s` is the sensor's (true) time at
    /// emission; transit adds `transit_s`.
    fn report(clock: &DisciplinedClockAuthority, h: &DisciplineHandle, true_s: f64) {
        let receipt = clock.now();
        let applied = h.applied_offset();
        h.observe(7, MissionTime(true_s), receipt, applied);
    }

    /// The host 40 s **behind** a GNSS radar: waiting until heard, then mission time is the
    /// radar's, and the first lock steps forward and is reported once.
    #[test]
    fn a_host_behind_its_sensor_locks_to_the_sensor_time() {
        let (ms, host) = stepped(1_000.0);
        let (clock, h) = DisciplinedClockAuthority::with_host(POLICY, 7, host);
        assert_eq!(
            h.poll(),
            Some(S::Waiting),
            "the record starts with the host clock"
        );
        assert_eq!(h.poll(), None, "reported once");
        assert!(
            (clock.now().0 - 1_000.0).abs() < 1e-9,
            "host clock while waiting"
        );
        // The radar says 1040.0 when the host says 1000.0; 20 ms transit.
        report(&clock, &h, 1_039.98);
        let Some(S::Locked {
            offset_s, samples, ..
        }) = h.poll()
        else {
            panic!("the first report locks: {:?}", h.state());
        };
        assert!((offset_s + 39.98).abs() < 1e-6, "{offset_s}");
        assert_eq!(samples, 1);
        assert!(
            (clock.now().0 - 1_039.98).abs() < 1e-6,
            "mission time is the radar's"
        );
        // Ten seconds later on the host, it still is, and a slower sample does not move it.
        set(&ms, 1_010.0);
        report(&clock, &h, 1_049.95); // 50 ms transit
        assert!((clock.now().0 - 1_049.98).abs() < 1e-6);
        assert_eq!(h.poll(), None, "still locked: no new state");
    }

    /// The host **ahead** of the sensor locks the other way, and no feedback creeps in
    /// over many ticks: the applied offset is taken out before the estimate is formed.
    #[test]
    fn a_host_ahead_locks_without_feedback() {
        let (ms, host) = stepped(5_000.0);
        let (clock, h) = DisciplinedClockAuthority::with_host(POLICY, 7, host);
        for k in 0..50 {
            let host_s = 5_000.0 + f64::from(k);
            set(&ms, host_s);
            // The sensor reads 12 s behind the host; 10 ms transit.
            report(&clock, &h, host_s - 12.0 - 0.01);
        }
        let S::Locked {
            offset_s, samples, ..
        } = h.state()
        else {
            panic!("{:?}", h.state());
        };
        assert!(
            (offset_s - 12.01).abs() < 1e-6,
            "no drift from feedback: {offset_s}"
        );
        assert_eq!(samples, 50);
    }

    /// Other sensors, and non-finite stamps, never move the clock.
    #[test]
    fn only_the_named_sensor_disciplines() {
        let (_ms, host) = stepped(100.0);
        let (clock, h) = DisciplinedClockAuthority::with_host(POLICY, 7, host);
        h.observe(8, MissionTime(0.0), clock.now(), 0.0);
        h.observe(7, MissionTime(f64::NAN), clock.now(), 0.0);
        assert_eq!(h.state(), S::Waiting);
        assert!((clock.now().0 - 100.0).abs() < 1e-9);
    }

    /// Silence past the holdover limit says so and keeps the offset; the next report
    /// re-locks, and both changes are reported.
    #[test]
    fn silence_goes_into_holdover_and_a_report_relocks() {
        let (ms, host) = stepped(2_000.0);
        let (clock, h) = DisciplinedClockAuthority::with_host(POLICY, 7, host);
        let _ = h.poll();
        report(&clock, &h, 2_005.0);
        assert!(matches!(h.poll(), Some(S::Locked { .. })));
        set(&ms, 2_000.0 + HOLDOVER_AFTER_S + 1.0);
        let Some(S::Holdover { offset_s, .. }) = h.poll() else {
            panic!("{:?}", h.state());
        };
        assert!(
            (offset_s + 5.0).abs() < 1e-6,
            "the last offset is still applied"
        );
        assert!((clock.now().0 - (2_000.0 + HOLDOVER_AFTER_S + 1.0 + 5.0)).abs() < 1e-6);
        report(&clock, &h, 2_000.0 + HOLDOVER_AFTER_S + 1.0 + 5.0);
        assert!(matches!(h.poll(), Some(S::Locked { .. })), "re-locked");
    }

    /// An offset beyond the bound is refused and not applied: the host clock is set by
    /// hand, rather than mission time stepped by hours.
    #[test]
    fn an_offset_beyond_the_bound_is_refused_and_not_applied() {
        let (_ms, host) = stepped(10_000.0);
        let (clock, h) = DisciplinedClockAuthority::with_host(POLICY, 7, host);
        report(&clock, &h, 10_000.0 + MAX_DISCIPLINE_OFFSET_S + 60.0);
        assert!(matches!(h.state(), S::Refused { .. }));
        assert!((clock.now().0 - 10_000.0).abs() < 1e-9, "nothing applied");
    }

    /// Once locked, mission time never goes backwards, even when a later window's least
    /// offset is larger (the host gained on the sensor).
    #[test]
    fn mission_time_never_goes_backwards_after_the_lock() {
        let (ms, host) = stepped(3_000.0);
        let (clock, h) = DisciplinedClockAuthority::with_host(POLICY, 7, host);
        report(&clock, &h, 3_000.0);
        let before = clock.now();
        set(&ms, 3_000.0 + DISCIPLINE_WINDOW_S + 5.0);
        // The host is now 0.5 s ahead of where the sensor says, so the offset grows.
        report(&clock, &h, 3_000.0 + DISCIPLINE_WINDOW_S + 4.5);
        let after = clock.now();
        assert!(after >= before);
        let again = clock.now();
        assert!(again >= after);
    }

    /// `clock_for` gives the wall clock and no handle when nothing is named.
    #[test]
    fn clock_for_names_its_kind() {
        let (wall, none) = clock_for(POLICY, None);
        assert_eq!(wall.source(), ClockSource::Wall);
        assert!(none.is_none());
        let (disc, some) = clock_for(POLICY, Some(3));
        assert_eq!(disc.source(), ClockSource::Disciplined);
        assert_eq!(some.map(|h| h.sensor()), Some(3));
    }
}
