// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The multi-rate, out-of-sequence fusion pipeline (GAP-011, Area A).
//!
//! Row: `fusion-async` / "Out-of-sequence handling, multi-rate fusion" in
//! `docs/verification-capability-table.md` §1. Method: replay a fixed multi-sensor
//! timeline through the async pipeline and compare against the offline batch.
//! Criterion: state convergence within 1e-4.
//!
//! # What this composes, and what it does not invent
//!
//! Every piece of mathematics here already existed and was signed off on 2026-09-05
//! (`ARCHITECTURE.md` §10 item 36): the Joseph-form linear Kalman filter, the
//! Jonker-Volgenant assignment solver behind [`GlobalNearestNeighbor`], the chi-square
//! gate, and the confirm/coast/delete state machine in `gungnir-track`. What was
//! missing was the composition: a loop that predicts each track to a measurement's
//! time, gates, associates, updates, initiates and ages. That is this module, and it
//! adds no new estimator.
//!
//! **Not in the pipeline yet, and named rather than implied**: the nonlinear
//! estimators are not selected here (a track is a constant-velocity Kalman filter,
//! GAP-011's remaining rows), random-finite-set filtering is not run (GAP-015), and
//! track-to-track fusion across platforms is not run (GAP-013). A deployment gets
//! single-sensor-per-detection sequential fusion with global assignment inside a scan,
//! which is what the composition honestly provides.
//!
//! # Out of sequence, and why a horizon rather than a re-filter
//!
//! Detections arrive from several sensors with different latencies, so arrival order
//! is not measurement order. The pipeline holds a detection in a reorder buffer until
//! the newest source time it has seen is [`PipelineSettings::reorder_horizon_s`] ahead
//! of it, then processes buffered detections **in source-time order**. Inside the
//! horizon, arrival order therefore cannot change the answer at all: the batch and the
//! shuffled run process an identical sequence of identical epochs, and agree exactly
//! rather than within a tolerance.
//!
//! The alternative -- accept everything immediately and re-filter backwards when a late
//! measurement lands -- needs a retrodiction step and a stored filter history per
//! track. It is the better algorithm for long latencies and it is not what is built;
//! a detection older than the processed cursor is **counted and refused**
//! ([`PipelineStats::too_late`]) rather than folded in as though it were current,
//! because folding a stale measurement into a current estimate at full weight is a
//! quiet corruption of the track rather than an approximation of one.
//!
//! # Epochs
//!
//! Detections whose source times fall within [`PipelineSettings::epoch_s`] of each
//! other are one scan and are associated together, so two sensors reporting the same
//! instant compete for the same track through one global assignment. Detections
//! further apart are separate epochs, each predicted forward to its own measurement
//! time, which is the ordinary sequential treatment of asynchronous sensors.

use crate::{BearingDetection, Detection};
use gungnir_association::{solve_assignment, ChiSquareGate, GlobalNearestNeighbor};
use gungnir_core::ConstantVelocity;
use gungnir_filters::{AzimuthElevation, BearingOnly, Filter, KalmanFilter, MeasurementModel};
use gungnir_track::{Track, TrackId, TrackManager};
use nalgebra::{DMatrix, SMatrix, SVector};
use std::collections::HashMap;

/// What one scan's association decided: the track/measurement pairs to update, the
/// tracks that got nothing, and the measurements that matched no track.
type ScanAssociation = (
    Vec<(TrackId, SVector<f64, 3>)>,
    Vec<TrackId>,
    Vec<SVector<f64, 3>>,
);

/// One track's estimator. Position-only measurement of a constant-velocity state,
/// which is the shape every sensor in `docs/test-tracks/sensor-models.md` produces.
type TrackFilter = KalmanFilter<ConstantVelocity, 6, 3>;

/// How the pipeline is tuned. Every field is a deployment choice rather than a
/// constant, because the baseline supplies them (`ConfigBaseline.tracking`).
#[derive(Debug, Clone, PartialEq)]
pub struct PipelineSettings {
    /// How long a detection waits for later-arriving earlier measurements, seconds.
    /// Larger means more tolerance of latency spread and more delay before a track
    /// moves; the deployment trades one against the other.
    pub reorder_horizon_s: f64,
    /// Source times within this of each other are one scan, seconds.
    pub epoch_s: f64,
    /// The association gate. Its threshold is the baseline's `tracking.gate_threshold`.
    pub gate: ChiSquareGate,
    /// Cumulative hits before a tentative track is confirmed.
    pub confirm_threshold: u32,
    /// Consecutive misses before a track is deleted.
    pub delete_after_misses: u32,
    /// Process-noise spectral density of the constant-velocity model, (m/s²)²/Hz.
    pub process_noise_psd: f64,
    /// Measurement-noise variances per axis, m².
    pub measurement_noise_var: [f64; 3],
    /// Initial velocity variance for a track initiated from one detection, (m/s)².
    ///
    /// A single position measurement says nothing about velocity, so this is the prior
    /// rather than an estimate. It is large on purpose: too small a value tells the
    /// filter the target is stationary and the second detection then falls outside the
    /// gate, which reads as a missed target rather than as a bad prior.
    pub initial_velocity_var: f64,
    /// How long a bearing that matched no track is kept, seconds (DN-27 §5 rule 3).
    ///
    /// A direction with no track behind it is the acoustic array's ordinary output when
    /// something is out there that the radar cannot see, and it is exactly the report an
    /// operator most needs -- so it is retained and shown as a ray rather than dropped.
    /// It is a lifetime and not a hold-forever, because a bearing is a statement about
    /// one instant and a picture full of hour-old directions is not a picture.
    pub bearing_retention_s: f64,
}

impl Default for PipelineSettings {
    fn default() -> Self {
        Self {
            reorder_horizon_s: 1.0,
            epoch_s: 0.05,
            gate: ChiSquareGate::at_99_percent(3),
            confirm_threshold: 3,
            delete_after_misses: 3,
            process_noise_psd: 4.0,
            measurement_noise_var: [400.0, 400.0, 900.0],
            initial_velocity_var: 40_000.0,
            bearing_retention_s: 60.0,
        }
    }
}

/// A filter selection this build does not implement.
///
/// Named rather than silently substituted: a deployment that promoted an IMM baseline
/// and got a constant-velocity filter would have a governance record saying one thing
/// and a picture produced by another, which is the failure DN-24 §7 exists to prevent.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "this build does not implement the filter {selection:?}; it implements {implemented:?}, \
     and a promoted baseline is not applied by substituting one filter for another"
)]
pub struct UnsupportedFilter {
    pub selection: String,
    pub implemented: &'static [&'static str],
}

/// The filter selections `PipelineSettings::from_baseline` accepts.
///
/// One, today. The names are `TrackingConfig::filter_selection`'s vocabulary and grow
/// as `gungnir-filters` gains estimators (GAP-011's remaining §1 rows).
/// The filter selections this **pipeline** can apply.
///
/// **This is not the list of filters `gungnir-filters` contains, and the difference is
/// the whole point.** As of 2026-09-06 that crate has an extended and an unscented
/// Kalman filter, an IMM, a particle filter and a square-root form, and
/// `gungnir-association` has JPDA and MHT. None of them appears here, because
/// [`TrackFilter`] -- the type this pipeline actually runs -- is a fixed linear Kalman
/// filter over a constant-velocity model, and this list is the answer to "what can the
/// pipeline apply", not "what has been written somewhere".
///
/// **Adding a name here without changing what the pipeline runs would be the exact
/// failure GAP-053 exists to prevent**: `with_algorithm_baseline` would stamp every track
/// with a baseline claiming an IMM produced it, the governance record would say one thing
/// and the picture would be another, and nothing downstream could tell. A register entry
/// once described extending this as "a one-line change". It is not one, and that sentence
/// has been corrected wherever it appeared.
///
/// Extending it means giving the pipeline a way to hold more than one filter type -- the
/// filters have different state dimensions and different update signatures -- and gating
/// the result end to end, which is its own increment.
pub const IMPLEMENTED_FILTERS: &[&str] = &["kf-cv", "linear-kf", "constant-velocity"];

impl PipelineSettings {
    /// Build settings from a promoted algorithm baseline's fields (DN-24 §7, GAP-053).
    ///
    /// Takes the primitives rather than `gungnir_config::TrackingConfig`, because this
    /// crate sits below `gungnir-config` and may not depend on it; both binaries read
    /// the baseline and pass the two fields through.
    ///
    /// # Errors
    ///
    /// [`UnsupportedFilter`] when the baseline names a filter this build does not have,
    /// and a non-positive or non-finite gate threshold, which `gungnir-config` already
    /// refuses but which this does not assume.
    pub fn from_baseline(
        gate_threshold: f64,
        filter_selection: &str,
    ) -> Result<Self, UnsupportedFilter> {
        if !IMPLEMENTED_FILTERS.contains(&filter_selection) {
            return Err(UnsupportedFilter {
                selection: filter_selection.to_owned(),
                implemented: IMPLEMENTED_FILTERS,
            });
        }
        let mut settings = Self::default();
        if gate_threshold.is_finite() && gate_threshold > 0.0 {
            settings.gate = ChiSquareGate { gate_threshold };
        }
        Ok(settings)
    }
}

/// What the pipeline has done, for the health line and the tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PipelineStats {
    /// Detections taken into the reorder buffer.
    pub accepted: u64,
    /// Detections refused because their source time was already behind the processed
    /// cursor. **Never silently folded in**; see the module documentation.
    pub too_late: u64,
    /// Epochs processed.
    pub epochs: u64,
    /// Detections that updated an existing track.
    pub associated: u64,
    /// Detections that started a new tentative track.
    ///
    /// **Only positions are counted here because only positions can be counted here.**
    /// No bearing reaches [`FusionPipeline::initiate`], by DN-27 §5 rule 1.
    pub initiated: u64,
    /// Bearings offered to [`FusionPipeline::offer_bearing`].
    pub bearings_offered: u64,
    /// Bearings that refined an existing track's estimate.
    pub bearings_updated: u64,
    /// Bearings that matched no track and were retained for
    /// [`PipelineSettings::bearing_retention_s`] (DN-27 §5 rule 3). **Retained is not
    /// dropped**: the report stays in the picture as a ray.
    pub bearings_retained: u64,
    /// Retained bearings whose lifetime ran out.
    pub bearings_expired: u64,
    /// Bearings refused before any of that; see [`BearingRefusal`].
    pub bearings_refused: u64,
    // There is deliberately no `bearings_initiated`. A counter for it would imply a
    // path that could increment it, and DN-27 §5 rule 1 is that there is none.
}

/// Why a detection did not enter the buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PushError {
    /// Older than the last processed epoch, so folding it in would corrupt a current
    /// estimate with a stale measurement.
    #[error("detection is behind the processed cursor and was refused")]
    TooLate,
    /// The source time is not a finite number, so it cannot be ordered.
    #[error("detection source time is not finite")]
    NotFinite,
}

/// Why a bearing was not used (DN-27 §5).
///
/// **None of these variants is "it started a track"**, because there is no such
/// outcome. Rule 1 is a prohibition and it is enforced by the absence of the code path,
/// not by a refusal at run time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BearingRefusal {
    /// A value in the bearing, its time or its stated error is not a finite number.
    #[error("the bearing carries a value that is not a finite number")]
    NotFinite,
    /// The azimuth variance is zero or negative, so the bearing states no error. A
    /// bearing with no error is not a measurement this pipeline will fold in: for a
    /// bearing the error *is* the information (DN-27 §4).
    #[error("the bearing states no azimuth error, and a default one would be invented")]
    NoStatedError,
    /// Its source time is further from the pipeline's processed cursor than one reorder
    /// horizon, so it is not about the instant the estimates are current for.
    ///
    /// **Bearings do not go through the reorder buffer.** They refine the estimate the
    /// pipeline currently holds rather than being retrodicted into an earlier one, which
    /// is a stated limitation of this build and not an approximation of a fix: folding a
    /// bearing into an estimate that has already moved past it would be the quiet
    /// corruption `PushError::TooLate` exists to prevent on the position path.
    #[error("the bearing's source time is outside the reorder horizon around the cursor")]
    OutsideHorizon,
}

/// What [`FusionPipeline::offer_bearing`] did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BearingOutcome {
    /// It gated into a track and refined that track's estimate (DN-27 §5 rule 1's
    /// permitted half).
    Updated(TrackId),
    /// It matched nothing and is retained until this mission time, in seconds
    /// (DN-27 §5 rule 3). Read the retained set with
    /// [`FusionPipeline::retained_bearings`].
    Retained { until_s: f64 },
    /// It was not usable at all.
    Refused(BearingRefusal),
}

/// A bearing that matched no track, kept for a stated lifetime (DN-27 §5 rule 3).
///
/// **This is not a track and must never be drawn as one.** DN-27 §7: a bearing is drawn
/// as a ray from the sensor along the azimuth, widening with the angular error, and it
/// does not terminate -- a drawn end point is a range nobody measured.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RetainedBearing {
    pub bearing: BearingDetection,
    /// Mission time, seconds, after which it is dropped.
    pub until_s: f64,
}

/// The out-of-sequence, multi-rate fusion pipeline.
///
/// Synchronous and owned by one task. [`crate::ingest`] drives it from the channel;
/// tests drive it directly, which is what lets the same pipeline be compared against
/// itself run offline in batch.
pub struct FusionPipeline {
    settings: PipelineSettings,
    manager: TrackManager<GlobalNearestNeighbor>,
    filters: HashMap<TrackId, TrackFilter>,
    /// Buffered detections, kept sorted by source time. Small by construction: it
    /// holds one reorder horizon of one sector's detections.
    buffer: Vec<Detection>,
    /// Source time of the last processed epoch, `None` before the first.
    cursor_s: Option<f64>,
    /// The newest source time seen, which is what the horizon is measured back from.
    newest_seen_s: f64,
    /// Bearings that matched no track, kept for their stated lifetime (DN-27 §5 rule 3).
    retained: Vec<RetainedBearing>,
    stats: PipelineStats,
}

impl std::fmt::Debug for FusionPipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FusionPipeline")
            .field("settings", &self.settings)
            .field("buffered", &self.buffer.len())
            .field("retained_bearings", &self.retained.len())
            .field("cursor_s", &self.cursor_s)
            .field("stats", &self.stats)
            .finish_non_exhaustive()
    }
}

impl FusionPipeline {
    #[must_use]
    pub fn new(settings: PipelineSettings) -> Self {
        let manager = TrackManager::new(
            GlobalNearestNeighbor,
            settings.confirm_threshold,
            settings.delete_after_misses,
        );
        Self {
            settings,
            manager,
            filters: HashMap::new(),
            buffer: Vec::new(),
            cursor_s: None,
            newest_seen_s: f64::NEG_INFINITY,
            retained: Vec::new(),
            stats: PipelineStats::default(),
        }
    }

    #[must_use]
    pub fn settings(&self) -> &PipelineSettings {
        &self.settings
    }

    #[must_use]
    pub fn stats(&self) -> PipelineStats {
        self.stats
    }

    /// How many detections are waiting out the reorder horizon.
    #[must_use]
    pub fn buffered(&self) -> usize {
        self.buffer.len()
    }

    /// Every track a consumer should act on: everything not deleted.
    #[must_use]
    pub fn snapshot(&self) -> Vec<Track> {
        self.manager.live_tracks().cloned().collect()
    }

    /// Take a detection into the reorder buffer.
    ///
    /// # Errors
    ///
    /// [`PushError::TooLate`] for a detection behind the processed cursor, and
    /// [`PushError::NotFinite`] for one whose source time cannot be ordered. Both are
    /// counted in [`PipelineStats`].
    pub fn push(&mut self, detection: Detection) -> Result<(), PushError> {
        if !detection.timestamp_s.is_finite() {
            self.stats.too_late = self.stats.too_late.saturating_add(1);
            return Err(PushError::NotFinite);
        }
        if let Some(cursor) = self.cursor_s {
            if detection.timestamp_s < cursor {
                self.stats.too_late = self.stats.too_late.saturating_add(1);
                return Err(PushError::TooLate);
            }
        }
        self.newest_seen_s = self.newest_seen_s.max(detection.timestamp_s);
        let at = self
            .buffer
            .partition_point(|d| d.timestamp_s <= detection.timestamp_s);
        self.buffer.insert(at, detection);
        self.stats.accepted = self.stats.accepted.saturating_add(1);
        Ok(())
    }

    /// Offer a bearing to the pipeline: **it may refine a track and it may not start
    /// one** (docs/design/DN-27-bearing-only-detections.md §5 rules 1 and 3).
    ///
    /// # Why this is a separate entry point from [`FusionPipeline::push`]
    ///
    /// Not for tidiness. A single bearing does not determine a position, so there is no
    /// state for a new track to start in, and a filter fed a sequence of bearings from
    /// one fixed sensor converges to a confident answer at the wrong range -- **a
    /// failure that is silent and looks exactly like success**. DN-27 §5 makes that a
    /// prohibition rather than a tuning parameter, and this build makes it a fact about
    /// the code: [`FusionPipeline::initiate`] takes an `SVector<f64, 3>` position, this
    /// method never calls it, and [`crate::BearingDetection`] cannot become one.
    ///
    /// Two bearings from separated sensors *may* initiate, under §5 rule 2, and that
    /// crossing is `gungnir_coord::cross_bearings`' to compute and a caller's to offer
    /// here as an ordinary [`Detection`] with the covariance the geometry gave it. This
    /// pipeline does not pair bearings itself: which crossing is real is the
    /// JPDA-shaped problem `gungnir-association` owns, and DN-27 §9 leaves it open
    /// rather than specifying a rule that would not work.
    ///
    /// # What an update does and does not do
    ///
    /// The gated track's estimate is refined through the extended-filter update at
    /// `gungnir_filters::BearingOnly` (or `AzimuthElevation` when the bearing carried
    /// one), which is the range-azimuth-elevation model with the range row removed.
    /// **It does not count as a hit for the lifecycle**: a track kept alive by bearings
    /// alone would be a track nothing has localised since it was last seen, which is
    /// within a hair of what rule 1 forbids, so confirmation and deletion still count
    /// scans of positions.
    ///
    /// The bearing is applied at the pipeline's current cursor rather than retrodicted
    /// to its own source time; one further from the cursor than a reorder horizon is
    /// refused as [`BearingRefusal::OutsideHorizon`] rather than folded in.
    pub fn offer_bearing(&mut self, bearing: &BearingDetection) -> BearingOutcome {
        self.stats.bearings_offered = self.stats.bearings_offered.saturating_add(1);
        let refuse = |stats: &mut PipelineStats, why: BearingRefusal| {
            stats.bearings_refused = stats.bearings_refused.saturating_add(1);
            BearingOutcome::Refused(why)
        };
        if !(bearing.timestamp_s.is_finite()
            && bearing.azimuth_rad.is_finite()
            && bearing.sensor_enu.iter().all(|v| v.is_finite())
            && bearing.elevation_rad.is_none_or(f64::is_finite)
            && bearing.elevation_variance_rad2.is_none_or(f64::is_finite))
        {
            return refuse(&mut self.stats, BearingRefusal::NotFinite);
        }
        if !(bearing.azimuth_variance_rad2.is_finite() && bearing.azimuth_variance_rad2 > 0.0) {
            return refuse(&mut self.stats, BearingRefusal::NoStatedError);
        }
        if let Some(cursor) = self.cursor_s {
            if (bearing.timestamp_s - cursor).abs() > self.settings.reorder_horizon_s {
                return refuse(&mut self.stats, BearingRefusal::OutsideHorizon);
            }
        }

        self.expire_bearings(bearing.timestamp_s);

        if let Some((id, state, covariance)) = self.best_bearing_match(bearing) {
            // The estimates the filters hold are the truth about kinematics, exactly as
            // in an epoch; the filter is rebuilt around the refined pair so the next
            // position update starts from it.
            self.filters.insert(id, self.new_filter(state, &covariance));
            self.manager.update_estimate(id, state, covariance);
            self.stats.bearings_updated = self.stats.bearings_updated.saturating_add(1);
            return BearingOutcome::Updated(id);
        }

        // Rule 3: a bearing that updates nothing is retained and shown, not dropped.
        let until_s = bearing.timestamp_s + self.settings.bearing_retention_s;
        self.retained.push(RetainedBearing {
            bearing: *bearing,
            until_s,
        });
        self.stats.bearings_retained = self.stats.bearings_retained.saturating_add(1);
        BearingOutcome::Retained { until_s }
    }

    /// The bearings that matched no track and are still inside their lifetime
    /// (DN-27 §5 rule 3). Drawn as rays, never as symbols (§7).
    #[must_use]
    pub fn retained_bearings(&self) -> &[RetainedBearing] {
        &self.retained
    }

    /// Drop retained bearings whose lifetime has run out at `now_s`.
    ///
    /// Called by [`FusionPipeline::offer_bearing`] on every offer; public so a host
    /// whose acoustic feed has gone quiet still ages what it is drawing.
    pub fn expire_bearings(&mut self, now_s: f64) {
        let before = self.retained.len();
        self.retained.retain(|r| r.until_s > now_s);
        let expired = before - self.retained.len();
        self.stats.bearings_expired = self
            .stats
            .bearings_expired
            .saturating_add(expired.try_into().unwrap_or(u64::MAX));
    }

    /// The live track a bearing gates into most tightly, and the estimate the update
    /// leaves behind. `None` when it gates into none, which is rule 3's case.
    ///
    /// Nearest gated neighbour, not a global assignment: one bearing is being placed
    /// against many tracks rather than a scan against a track set, so there is no
    /// assignment problem to solve. Which of several plausible tracks a bearing really
    /// belongs to is the ghost problem DN-27 §9 leaves to `gungnir-association`.
    fn best_bearing_match(
        &self,
        bearing: &BearingDetection,
    ) -> Option<(TrackId, SVector<f64, 6>, SMatrix<f64, 6, 6>)> {
        let mut best: Option<(TrackId, SVector<f64, 6>, SMatrix<f64, 6, 6>, f64)> = None;
        for track in self.manager.live_tracks() {
            let Some(filter) = self.filters.get(&track.id) else {
                continue;
            };
            let x = *filter.state();
            let p = *filter.covariance();
            // An elevation with no stated error is not folded in: the update would need
            // a variance and there is none to use. The azimuth alone still refines the
            // track, which is the information such a report actually carries.
            let updated = if let (Some(elevation), Some(variance)) =
                (bearing.elevation_rad, bearing.elevation_variance_rad2)
            {
                let mut r = SMatrix::<f64, 2, 2>::zeros();
                r[(0, 0)] = bearing.azimuth_variance_rad2;
                r[(1, 1)] = variance;
                extended_update(
                    &x,
                    &p,
                    &AzimuthElevation::at(bearing.sensor_enu),
                    &SVector::<f64, 2>::new(bearing.azimuth_rad, elevation),
                    &r,
                    ChiSquareGate::at_99_percent(2).gate_threshold,
                )
            } else {
                let r = SMatrix::<f64, 1, 1>::new(bearing.azimuth_variance_rad2);
                extended_update(
                    &x,
                    &p,
                    &BearingOnly::at(bearing.sensor_enu),
                    &SVector::<f64, 1>::new(bearing.azimuth_rad),
                    &r,
                    ChiSquareGate::at_99_percent(1).gate_threshold,
                )
            };
            let Some((state, covariance, distance)) = updated else {
                continue;
            };
            if best.is_none_or(|(_, _, _, d)| distance < d) {
                best = Some((track.id, state, covariance, distance));
            }
        }
        best.map(|(id, state, covariance, _)| (id, state, covariance))
    }

    /// Process every epoch whose reorder horizon has elapsed. Returns the number of
    /// epochs processed, so a caller knows whether the snapshot changed.
    pub fn run_ready(&mut self) -> usize {
        let mut processed = 0;
        while let Some(first) = self.buffer.first() {
            if first.timestamp_s + self.settings.reorder_horizon_s > self.newest_seen_s {
                break;
            }
            self.process_next_epoch();
            processed += 1;
        }
        processed
    }

    /// Process everything buffered regardless of the horizon: the end of a stream, or
    /// an offline batch.
    pub fn flush(&mut self) -> usize {
        let mut processed = 0;
        while !self.buffer.is_empty() {
            self.process_next_epoch();
            processed += 1;
        }
        processed
    }

    /// One scan: predict to its time, gate, associate, update, initiate, age.
    fn process_next_epoch(&mut self) {
        let Some(first) = self.buffer.first() else {
            return;
        };
        let epoch_end = first.timestamp_s + self.settings.epoch_s;
        let take = self
            .buffer
            .partition_point(|d| d.timestamp_s <= epoch_end)
            .max(1);
        let scan: Vec<Detection> = self.buffer.drain(..take).collect();
        // Predict to the last measurement in the scan: every detection in it is one
        // instant by the epoch rule, and the track must be current afterwards.
        let epoch_s = scan
            .iter()
            .map(|d| d.timestamp_s)
            .fold(f64::NEG_INFINITY, f64::max);

        let dt = self.cursor_s.map_or(0.0, |cursor| epoch_s - cursor);
        if dt > 0.0 {
            for track in self.manager.tracks() {
                if let Some(filter) = self.filters.get_mut(&track.id) {
                    filter.predict(dt);
                }
            }
        }
        self.cursor_s = Some(epoch_s);
        self.stats.epochs = self.stats.epochs.saturating_add(1);

        let live: Vec<TrackId> = self.manager.live_tracks().map(|t| t.id).collect();
        let (hits, misses, unassigned) = self.associate(&live, &scan);

        for (track, detection) in &hits {
            if let Some(filter) = self.filters.get_mut(track) {
                filter.update(detection);
            }
        }
        self.stats.associated = self
            .stats
            .associated
            .saturating_add(hits.len().try_into().unwrap_or(u64::MAX));

        let hit_ids: Vec<TrackId> = hits.iter().map(|(id, _)| *id).collect();
        let outcome = self.manager.step(&hit_ids, &misses);
        for id in &outcome.deleted {
            self.filters.remove(id);
        }

        // The estimates the filters hold are the truth about kinematics; write them
        // into the records the lifecycle owns.
        for (id, filter) in &self.filters {
            self.manager
                .update_estimate(*id, *filter.state(), *filter.covariance());
        }

        for measurement in unassigned {
            self.initiate(&measurement);
        }
    }

    /// Gate and assign one scan's detections to the live tracks.
    ///
    /// Returns the track/measurement pairs to update, the tracks that got nothing, and
    /// the measurements that matched no track.
    fn associate(&mut self, live: &[TrackId], scan: &[Detection]) -> ScanAssociation {
        if live.is_empty() {
            return (
                Vec::new(),
                Vec::new(),
                scan.iter().map(|d| d.measurement).collect(),
            );
        }
        if scan.is_empty() {
            return (Vec::new(), live.to_vec(), Vec::new());
        }

        // Squared Mahalanobis distance per pair, and whether the gate admits it. A
        // rejected pair still needs a finite cost, because the solver optimizes over
        // the whole matrix; it gets one above every admissible distance and is thrown
        // away afterwards, so it can never be chosen over a real association.
        let reject_cost = self.settings.gate.gate_threshold * 1_000.0 + 1.0;
        let mut cost = DMatrix::from_element(live.len(), scan.len(), reject_cost);
        let mut admitted = vec![vec![false; scan.len()]; live.len()];
        for (row, id) in live.iter().enumerate() {
            let Some(filter) = self.filters.get(id) else {
                continue;
            };
            let s = filter.innovation_covariance();
            for (col, detection) in scan.iter().enumerate() {
                let y = filter.innovation(&detection.measurement);
                let Ok(d2) = ChiSquareGate::squared_distance(&y, &s) else {
                    continue;
                };
                if d2 <= self.settings.gate.gate_threshold {
                    cost[(row, col)] = d2;
                    admitted[row][col] = true;
                }
            }
        }

        let assignment = match solve_assignment(&cost) {
            Ok(a) => a.row_to_col,
            // A cost matrix with no defined optimum means the estimate that produced
            // it is already broken. Every track misses this scan and every detection
            // starts a track, which is the state the deletion rule then acts on.
            Err(err) => {
                tracing::warn!(%err, "association failed for this scan; no track was updated");
                return (
                    Vec::new(),
                    live.to_vec(),
                    scan.iter().map(|d| d.measurement).collect(),
                );
            }
        };

        let mut hits = Vec::new();
        let mut misses = Vec::new();
        let mut taken = vec![false; scan.len()];
        for (row, id) in live.iter().enumerate() {
            match assignment.get(row).copied().flatten() {
                Some(col) if admitted[row][col] => {
                    taken[col] = true;
                    hits.push((*id, scan[col].measurement));
                }
                _ => misses.push(*id),
            }
        }
        let unassigned = scan
            .iter()
            .enumerate()
            .filter(|(col, _)| !taken[*col])
            .map(|(_, d)| d.measurement)
            .collect();
        (hits, misses, unassigned)
    }

    /// Start a tentative track from one detection: position measured, velocity unknown.
    fn initiate(&mut self, measurement: &SVector<f64, 3>) {
        let mut state = SVector::<f64, 6>::zeros();
        state.fixed_rows_mut::<3>(0).copy_from(measurement);
        let mut covariance = SMatrix::<f64, 6, 6>::zeros();
        for axis in 0..3 {
            covariance[(axis, axis)] = self.settings.measurement_noise_var[axis];
            covariance[(3 + axis, 3 + axis)] = self.settings.initial_velocity_var;
        }
        let id = self.manager.initiate(state, covariance);
        self.filters.insert(id, self.new_filter(state, &covariance));
        self.stats.initiated = self.stats.initiated.saturating_add(1);
    }

    fn new_filter(&self, state: SVector<f64, 6>, covariance: &SMatrix<f64, 6, 6>) -> TrackFilter {
        let mut h = SMatrix::<f64, 3, 6>::zeros();
        for axis in 0..3 {
            h[(axis, axis)] = 1.0;
        }
        let mut r = SMatrix::<f64, 3, 3>::zeros();
        for axis in 0..3 {
            r[(axis, axis)] = self.settings.measurement_noise_var[axis];
        }
        KalmanFilter::new(
            state,
            *covariance,
            ConstantVelocity {
                sigma_a_sq: self.settings.process_noise_psd,
            },
            h,
            r,
        )
    }
}

/// One extended-filter update of a constant-velocity state by an angular measurement,
/// gated (docs/design/DN-27-bearing-only-detections.md §5 rule 1).
///
/// Returns the refined state, the refined covariance and the squared Mahalanobis
/// distance, or `None` when the measurement falls outside `gate_threshold` or the
/// innovation covariance cannot be inverted.
///
/// **Written here rather than driven through `gungnir_filters::ExtendedKalmanFilter`**
/// because the pipeline's per-track estimator is a linear position filter and this is a
/// second, differently shaped measurement of the same state; constructing an EKF around
/// the track to make one update and throwing it away would hide the fact that the
/// measurement is folded into the same `(x, P)` the position filter holds. The
/// recursion is the EKF's own, taken at the model's Jacobian, with the same Joseph-form
/// covariance update `gungnir-filters` uses and for the same reason.
///
/// The residual comes from the model, which is what wraps an angle onto `(-pi, pi]`
/// (`gungnir_filters::MeasurementModel::residual`).
fn extended_update<Model, const M: usize>(
    x: &SVector<f64, 6>,
    p: &SMatrix<f64, 6, 6>,
    model: &Model,
    z: &SVector<f64, M>,
    r: &SMatrix<f64, M, M>,
    gate_threshold: f64,
) -> Option<(SVector<f64, 6>, SMatrix<f64, 6, 6>, f64)>
where
    Model: MeasurementModel<6, M>,
{
    let h = model.jacobian(x);
    let pht = p * h.transpose();
    let s = h * pht + r;
    let s_inv = s.try_inverse()?;
    let y = model.residual(z, &model.predict_measurement(x));
    let distance = (y.transpose() * s_inv * y)[(0, 0)];
    if !distance.is_finite() || distance > gate_threshold {
        return None;
    }
    let k = pht * s_inv;
    let state = x + k * y;
    let i_kh = SMatrix::<f64, 6, 6>::identity() - k * h;
    let covariance = i_kh * p * i_kh.transpose() + k * r * k.transpose();
    let covariance = (covariance + covariance.transpose()) * 0.5;
    if !state.iter().all(|v| v.is_finite()) || !covariance.iter().all(|v| v.is_finite()) {
        return None;
    }
    Some((state, covariance, distance))
}

/// Run a whole timeline offline, in source-time order, and return the final tracks.
///
/// The batch half of the `fusion-async` row's method. It drives the same
/// [`FusionPipeline`] as the async task, which is the point: the row compares the
/// async path against the offline one, so any difference is the ordering and the
/// buffering rather than two implementations of the mathematics.
#[must_use]
pub fn run_batch(settings: PipelineSettings, detections: &[Detection]) -> Vec<Track> {
    let mut ordered: Vec<Detection> = detections.to_vec();
    ordered.sort_by(|a, b| a.timestamp_s.total_cmp(&b.timestamp_s));
    let mut pipeline = FusionPipeline::new(settings);
    for detection in ordered {
        // Sorted input is never behind the cursor, so this cannot refuse.
        let _ = pipeline.push(detection);
    }
    pipeline.flush();
    pipeline.snapshot()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_track::TrackStatus;

    fn detection(sensor: u32, t: f64, position: [f64; 3]) -> Detection {
        Detection {
            sensor_id: sensor,
            timestamp_s: t,
            measurement: SVector::<f64, 3>::new(position[0], position[1], position[2]),
        }
    }

    /// A target moving at constant velocity, sampled every second, is tracked: one
    /// track, confirmed, with the velocity recovered from the positions.
    #[test]
    fn a_constant_velocity_target_is_tracked_and_its_velocity_recovered() {
        let settings = PipelineSettings::default();
        let detections: Vec<Detection> = (0..12)
            .map(|k| {
                let t = f64::from(k);
                detection(1, t, [100.0 * t, 50.0 * t, 1_000.0])
            })
            .collect();
        let tracks = run_batch(settings, &detections);
        assert_eq!(tracks.len(), 1, "one target, one track: {tracks:?}");
        let track = &tracks[0];
        assert_eq!(track.status, TrackStatus::Confirmed);
        assert!(
            (track.state[3] - 100.0).abs() < 5.0,
            "east velocity {} should be near 100",
            track.state[3]
        );
        assert!(
            (track.state[4] - 50.0).abs() < 5.0,
            "north velocity {} should be near 50",
            track.state[4]
        );
    }

    /// A track that stops being detected coasts and is deleted, and its filter goes
    /// with it rather than accumulating.
    #[test]
    fn a_lost_target_is_deleted_and_its_filter_released() {
        let mut pipeline = FusionPipeline::new(PipelineSettings::default());
        for k in 0..6 {
            let t = f64::from(k);
            pipeline
                .push(detection(1, t, [10.0 * t, 0.0, 500.0]))
                .expect("accepted");
        }
        pipeline.flush();
        assert_eq!(pipeline.snapshot().len(), 1);
        // Six scans with nothing in them: something else is detected far away, so the
        // epochs happen and the first track misses every one.
        for k in 6..14 {
            let t = f64::from(k);
            pipeline
                .push(detection(1, t, [500_000.0, 500_000.0, 500.0]))
                .expect("accepted");
        }
        pipeline.flush();
        let ids: Vec<TrackId> = pipeline.snapshot().iter().map(|t| t.id).collect();
        assert!(!ids.contains(&TrackId(0)), "the lost track was deleted");
        assert_eq!(
            pipeline.filters.len(),
            pipeline.snapshot().len(),
            "a deleted track's filter is released"
        );
    }

    /// A detection behind the processed cursor is refused and counted, never folded
    /// into a current estimate.
    #[test]
    fn a_detection_behind_the_cursor_is_refused_and_counted() {
        let mut pipeline = FusionPipeline::new(PipelineSettings::default());
        for k in 0..5 {
            let t = f64::from(k);
            pipeline
                .push(detection(1, t, [10.0 * t, 0.0, 500.0]))
                .expect("accepted");
        }
        pipeline.flush();
        assert_eq!(
            pipeline.push(detection(1, 1.0, [10.0, 0.0, 500.0])),
            Err(PushError::TooLate)
        );
        assert_eq!(pipeline.stats().too_late, 1);
        assert_eq!(
            pipeline.push(detection(1, f64::NAN, [0.0, 0.0, 0.0])),
            Err(PushError::NotFinite)
        );
    }

    /// Two detections in one scan compete for one track through the assignment, and
    /// the second starts a track of its own rather than being dropped.
    #[test]
    fn two_detections_in_one_scan_are_assigned_globally() {
        let settings = PipelineSettings::default();
        let mut detections = Vec::new();
        for k in 0..8 {
            let t = f64::from(k);
            detections.push(detection(1, t, [100.0 * t, 0.0, 500.0]));
            detections.push(detection(2, t + 0.01, [100.0 * t, 5_000.0, 500.0]));
        }
        let tracks = run_batch(settings, &detections);
        assert_eq!(tracks.len(), 2, "two targets, two tracks: {tracks:?}");
        assert!(tracks.iter().all(|t| t.status == TrackStatus::Confirmed));
    }
}
