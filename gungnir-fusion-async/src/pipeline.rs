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
//! adds no new estimator of its own -- **the one exception is `imm-cv-ct`'s selection
//! logic** (DN-28, [`TrackFilter::ImmCvCt`]), which composes the already-signed IMM
//! (item 94) rather than estimating anything new either.
//!
//! **Not in the pipeline yet, and named rather than implied**: the nonlinear
//! estimators are not selected here beyond the CV/CT IMM (EKF, UKF and the particle and
//! square-root forms are still unreachable from a baseline, DN-28 §6), and track-to-track
//! fusion across platforms is not run (GAP-013). A deployment gets
//! single-sensor-per-detection sequential fusion with global assignment inside a scan,
//! which is what the composition honestly provides.
//!
//! # The dense-group mode
//!
//! Random-finite-set filtering **can** be run since GAP-015, but only for the scene the
//! association above cannot resolve, **only as a count and a shape, never as tracks**,
//! and **only where a deployment asks for it**: [`PipelineSettings::dense_group`] is
//! `None` by default, for a measured cost that field records. When it is enabled and an
//! epoch's scan is denser than the association limit this workspace already states --
//! more than `gungnir_association::jpda::MAX_DETECTIONS` detections -- a `gungnir-rfs`
//! PHD (or CPHD, if a deployment selects it) runs beside the per-track filters and
//! reports [`DenseGroupEstimate`]. It carries no track identifier of any kind, because
//! the filter behind it carries no identity; the labelled filters that would are
//! GAP-015's remaining row and are not built. See [`crate::dense_group`] for the whole of
//! that reasoning.
//!
//! **The mode adds output and changes none.** The per-track path above -- prediction,
//! gating, assignment, the lifecycle, [`FusionPipeline::snapshot`] and
//! [`FusionPipeline::timed_snapshot`] -- runs identically whether the mode is engaged,
//! idle or configured away, which `tests/dense_group.rs` asserts by comparing the two
//! runs field for field over a raid that engages it.
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

use crate::dense_group::{DenseGroupEstimate, DenseGroupSettings, DenseGroupState};
use crate::{BearingDetection, Detection};
use gungnir_association::{solve_assignment, ChiSquareGate, GlobalNearestNeighbor};
use gungnir_core::{ConstantVelocity, CoordinatedTurn};
use gungnir_filters::{
    AzimuthElevation, BearingOnly, Filter, Imm, KalmanFilter, MeasurementModel, ModeFilter,
};
use gungnir_rfs::GaussianComponent;
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

/// One track's estimator: the position-only measurement shape every sensor in
/// `docs/test-tracks/sensor-models.md` produces, either estimated (DN-28).
///
/// An enum rather than a trait object because there are exactly two selections this
/// pipeline runs (DN-28 §4, §6 -- the EKF/UKF/particle/square-root/JPDA/MHT selections
/// are each their own future increment) and both are known at compile time; nothing
/// outside this module needs to be generic over filter type.
///
/// **The `ImmCvCt` variant carries its own `h`/`r`** because [`Imm`] does not: each
/// mode's internal [`KalmanFilter`] already has a copy, and [`ModeFilter`] deliberately
/// does not expose it (`imm.rs` module documentation). Gating and construction both need
/// the same `h`/`r` [`FusionPipeline::new_filter`] built the modes from, so this enum is
/// where that copy lives rather than re-deriving it or reaching into a mode.
enum TrackFilter {
    ConstantVelocity(KalmanFilter<ConstantVelocity, 6, 3>),
    ImmCvCt {
        imm: Imm<6, 3>,
        h: SMatrix<f64, 3, 6>,
        r: SMatrix<f64, 3, 3>,
    },
}

impl TrackFilter {
    // `Filter::predict`/`Filter::state` disambiguated below: `ModeFilter` is in scope
    // for `new_filter`'s mode construction and also names `predict`/`state`, so
    // `KalmanFilter` -- which implements both traits -- is ambiguous on plain `.` calls.
    fn predict(&mut self, dt: f64) {
        match self {
            TrackFilter::ConstantVelocity(kf) => Filter::predict(kf, dt),
            TrackFilter::ImmCvCt { imm, .. } => imm.predict(dt),
        }
    }

    fn update(&mut self, z: &SVector<f64, 3>) {
        match self {
            TrackFilter::ConstantVelocity(kf) => Filter::update(kf, z),
            TrackFilter::ImmCvCt { imm, .. } => imm.update(z),
        }
    }

    fn state(&self) -> &SVector<f64, 6> {
        match self {
            TrackFilter::ConstantVelocity(kf) => Filter::state(kf),
            TrackFilter::ImmCvCt { imm, .. } => imm.state(),
        }
    }

    fn covariance(&self) -> &SMatrix<f64, 6, 6> {
        match self {
            TrackFilter::ConstantVelocity(kf) => kf.covariance(),
            TrackFilter::ImmCvCt { imm, .. } => imm.covariance(),
        }
    }

    /// Gating's innovation pair. For `ImmCvCt` this reads the *combined* estimate,
    /// spread term included -- DN-28 §3's open question for a reviewer, not a settled
    /// tuning choice; see [`Imm::innovation_covariance`]'s documentation.
    fn innovation(&self, z: &SVector<f64, 3>) -> SVector<f64, 3> {
        match self {
            TrackFilter::ConstantVelocity(kf) => kf.innovation(z),
            TrackFilter::ImmCvCt { imm, h, .. } => imm.innovation(z, h),
        }
    }

    fn innovation_covariance(&self) -> SMatrix<f64, 3, 3> {
        match self {
            TrackFilter::ConstantVelocity(kf) => kf.innovation_covariance(),
            TrackFilter::ImmCvCt { imm, h, r } => imm.innovation_covariance(h, r),
        }
    }
}

/// Which filter [`FusionPipeline`] runs for every track (DN-28 §4). One selection per
/// pipeline instance, not per track: DN-24 §7 already ties a baseline to a mission
/// profile, and a whole session builds one [`PipelineSettings`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FilterSelection {
    /// A fixed constant-velocity Kalman filter. What every track ran before DN-28.
    #[default]
    ConstantVelocity,
    /// The IMM over constant-velocity and coordinated-turn modes (DN-28). The default
    /// algorithm baseline names this (`"imm-cv-ct"`, DN-24), and it is what fixed the
    /// track-fragmentation defect a fixed constant-velocity filter cannot follow a real
    /// manoeuvre through (DN-28's motivation).
    ImmCvCt,
}

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
    /// Measurement-noise variances per axis, m² (east, north, height). The baseline's
    /// own figure, matched to its actual sensor, unless nothing promoted supplies one
    /// (DN-30 §5) -- before DN-30 this was fixed at `Self::default()`'s placeholder for
    /// every deployment, which DN-28 §7 found understated scenario 1's real sensor noise
    /// by up to 25x and named as the dominant driver of the fragmentation GAP-011
    /// recorded, not the motion model DN-28 was scoped to fix.
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
    /// operator most needs -- so it is retained for a caller to draw rather than
    /// dropped. **Nothing draws one yet** (GAP-096): DN-27 §7 is unbuilt, and
    /// [`FusionPipeline::retained_bearings`] has no caller outside this crate's tests.
    /// It is a lifetime and not a hold-forever, because a bearing is a statement about
    /// one instant and a picture full of hour-old directions is not a picture.
    pub bearing_retention_s: f64,
    /// Which filter every track runs (DN-28). `ConstantVelocity` unless a promoted
    /// baseline names `"imm-cv-ct"` (`PipelineSettings::from_baseline`).
    pub filter_selection: FilterSelection,
    /// The coordinated-turn mode's fixed turn rate, radians/second. Ignored unless
    /// `filter_selection` is [`FilterSelection::ImmCvCt`] -- a turn rate for a filter
    /// that is not running would be a number with nothing to mean.
    pub imm_turn_rate_rad_s: f64,
    /// Row-major mode-transition matrix over `[constant-velocity, coordinated-turn]`,
    /// `transition[i][j] = P(mode j now | mode i before)` -- [`Imm::new`]'s convention.
    /// Each row must sum to one; ignored unless `filter_selection` is `ImmCvCt`.
    pub imm_mode_transition: [[f64; 2]; 2],
    /// Initial mode probabilities over the same `[constant-velocity, coordinated-turn]`
    /// order, summing to one. Ignored unless `filter_selection` is `ImmCvCt`.
    pub imm_initial_mode_probabilities: [f64; 2],
    /// The dense-group mode (GAP-015): a random-finite-set filter run beside the
    /// per-track ones for a scan too dense to associate, reporting a count and a shape
    /// and **never tracks**. `None` switches it off entirely.
    ///
    /// # `None` by default, and the reason is a measurement rather than caution
    ///
    /// An engaged PHD epoch was measured on this development machine, release profile, at
    /// **about 6 ms added per epoch for a 20-target raid and about 200 ms for a
    /// 200-target one** (`DenseGroupSettings::phd` records the whole table). The
    /// per-frame budget in `docs/performance-budgets.md` is p99 under 4 ms, and the
    /// pipeline runs on the `tokio` executor thread that `crate::ingest_with` owns.
    /// Work of that size on that thread is precisely the case
    /// `docs/agentic-coding-standards.md` §2.2 names -- "a PHD update over a large
    /// birth/clutter set" -- as needing `tokio::task::spawn_blocking`, and this build
    /// runs it inline.
    ///
    /// So a deployment turns it on knowing the cost, rather than every deployment paying
    /// it unasked. **Switching the default on waits on that §2.2 plumbing**, which
    /// restructures how `ingest_with` drives an epoch and is a change to the concurrency
    /// shape of this crate for every deployment including the ones not using the mode --
    /// the owner's call, recorded in GAP-015 rather than taken here.
    ///
    /// While it is `None` the mode costs nothing at all: `run_dense_group` returns on the
    /// first line, before any comparison.
    ///
    /// **Not settable from a promoted baseline.** [`PipelineSettings::from_baseline`]
    /// leaves it at the default: `gungnir-config`'s `TrackingConfig` has no vocabulary for
    /// it, and inventing one here would put a field in the pipeline that no governance
    /// record could account for. That plumbing is recorded as remaining in GAP-015 rather
    /// than half-built.
    pub dense_group: Option<DenseGroupSettings>,
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
            filter_selection: FilterSelection::default(),
            // A generic medium-rate turn (Bar-Shalom's own IMM examples use a similar
            // figure); a deployment naming "imm-cv-ct" supplies its own via
            // `from_baseline`, and this value is inert under `ConstantVelocity`.
            imm_turn_rate_rad_s: 0.05,
            // 3% chance per epoch of switching mode, in both directions: quick enough
            // to follow a manoeuvre without treating every measurement's noise as the
            // start of a turn.
            imm_mode_transition: [[0.97, 0.03], [0.03, 0.97]],
            // Mostly constant-velocity to start, matching most targets most of the time.
            imm_initial_mode_probabilities: [0.9, 0.1],
            dense_group: None,
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
/// Two, as of DN-28. The names are `TrackingConfig::filter_selection`'s vocabulary and
/// grow as `gungnir-filters` gains estimators the pipeline can actually run (GAP-011's
/// remaining §1 rows).
///
/// **This is not the list of filters `gungnir-filters` contains, and the difference is
/// the whole point.** That crate also has an extended and an unscented Kalman filter, a
/// particle filter and a square-root form, and `gungnir-association` has JPDA and MHT.
/// None of them appears here: [`TrackFilter`] -- the type this pipeline actually runs --
/// is a linear Kalman filter or the CV/CT IMM, and this list is the answer to "what can
/// the pipeline apply", not "what has been written somewhere".
///
/// **Adding a name here without changing what the pipeline runs would be the exact
/// failure GAP-053 exists to prevent**: `with_algorithm_baseline` would stamp every track
/// with a baseline claiming an estimator produced it that did not, the governance record
/// would say one thing and the picture would be another, and nothing downstream could
/// tell. A register entry once described extending this as "a one-line change". It was
/// not one for `"imm-cv-ct"` either, in the ordinary sense -- see DN-28 for what the
/// increment actually was -- but that pair was smaller than the general case: CV and CT
/// are both six-dimensional `MotionModel`s with a fixed turn rate, so `Imm<6, 3>` over
/// them is dimensionally identical to the linear filter it joined (DN-28 §2). EKF/UKF
/// (a different, nonlinear measurement), the particle filter (a sample cloud, not a
/// Gaussian) and JPDA/MHT (a different axis, association rather than estimation) are
/// each still their own future increment (DN-28 §6).
pub const IMPLEMENTED_FILTERS: &[&str] = &["kf-cv", "linear-kf", "constant-velocity", "imm-cv-ct"];

/// The `"imm-cv-ct"` selection's own settings, from the baseline (DN-28 §5).
///
/// **Not optional and not defaulted here.** Every call to [`PipelineSettings::from_baseline`]
/// supplies one, whichever filter the baseline names: a caller does not know in advance
/// which selection it will turn out to be, and a `None` accepted for the common
/// `"kf-cv"` case would make `"imm-cv-ct"` runnable on invented numbers the one time it
/// mattered. `gungnir-config` is where a deployment's own file is validated against
/// [`Imm::new`]'s refusals before a candidate is promoted (DN-28 §5); this type only
/// carries what validation already passed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImmBaselineFields {
    /// The coordinated-turn mode's fixed turn rate, radians/second.
    pub turn_rate_rad_s: f64,
    /// Row-major over `[constant-velocity, coordinated-turn]`; see
    /// [`PipelineSettings::imm_mode_transition`].
    pub mode_transition: [[f64; 2]; 2],
    /// Over the same order; see [`PipelineSettings::imm_initial_mode_probabilities`].
    pub initial_mode_probabilities: [f64; 2],
}

impl PipelineSettings {
    /// Build settings from a promoted algorithm baseline's fields (DN-24 §7, GAP-053;
    /// `imm` fields added by DN-28 §5; `measurement_noise_var` added by DN-30 §5).
    ///
    /// Takes the primitives rather than `gungnir_config::TrackingConfig`, because this
    /// crate sits below `gungnir-config` and may not depend on it; both binaries read
    /// the baseline and pass the fields through. `imm` is read only when
    /// `filter_selection` turns out to be `"imm-cv-ct"`; a caller not naming that
    /// selection may still have to supply a value, since it does not know in advance
    /// which candidate a baseline promoted. `measurement_noise_var` is read
    /// unconditionally: every filter selection builds its `R` from it, unlike the `imm`
    /// fields.
    ///
    /// # Errors
    ///
    /// [`UnsupportedFilter`] when the baseline names a filter this build does not have,
    /// and a non-positive or non-finite gate threshold or measurement-noise axis, both
    /// of which `gungnir-config` already refuses but which this does not assume.
    pub fn from_baseline(
        gate_threshold: f64,
        filter_selection: &str,
        imm: &ImmBaselineFields,
        measurement_noise_var: [f64; 3],
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
        if measurement_noise_var
            .iter()
            .all(|v| v.is_finite() && *v > 0.0)
        {
            settings.measurement_noise_var = measurement_noise_var;
        }
        if filter_selection == "imm-cv-ct" {
            settings.filter_selection = FilterSelection::ImmCvCt;
            settings.imm_turn_rate_rad_s = imm.turn_rate_rad_s;
            settings.imm_mode_transition = imm.mode_transition;
            settings.imm_initial_mode_probabilities = imm.initial_mode_probabilities;
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
    /// dropped**: the report is kept for a caller to draw as a ray. No caller draws one
    /// yet (GAP-096), so this counter is what the report amounts to today.
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

/// A track's kinematic record together with the source-time instant its state and
/// covariance were last actually produced at.
///
/// `Track` deliberately carries no time of its own (`gungnir-track`'s module
/// documentation: "kinematic state only") -- the pipeline is what knows when it last
/// touched a track, and this is how that reaches a poller. Before this type existed, a
/// caller polling the pipeline had only its own polling time to put on a projected
/// [`gungnir_track::Track`], which is a different instant: this is a GAP-011 finding,
/// recorded in the register, that `gungnir-tracking-service::project_track` stamped
/// `now` -- the time the *service* was asked, not the time the estimate is *of* -- so a
/// track that had gone quiet read as freshly updated for as long as anyone kept polling.
#[derive(Debug, Clone)]
pub struct TimedTrack {
    pub track: Track,
    /// Mission time, seconds, that `track.state` and `track.covariance` are current
    /// for: the epoch a position update or initiation last moved them to, or the
    /// cursor a bearing refinement was applied at (bearings are applied at the
    /// pipeline's cursor rather than retrodicted to their own source time; see
    /// [`FusionPipeline::offer_bearing`]).
    pub estimate_time_s: f64,
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
    /// When each live track's filter was last actually moved: the epoch of a position
    /// update or initiation, or the cursor a bearing refinement was applied at. What
    /// [`FusionPipeline::timed_snapshot`] reports instead of a poller's own time.
    estimate_time_s: HashMap<TrackId, f64>,
    /// Buffered detections, kept sorted by source time. Small by construction: it
    /// holds one reorder horizon of one sector's detections.
    buffer: Vec<Detection>,
    /// Source time of the last processed epoch, `None` before the first.
    cursor_s: Option<f64>,
    /// The newest source time seen, which is what the horizon is measured back from.
    newest_seen_s: f64,
    /// Bearings that matched no track, kept for their stated lifetime (DN-27 §5 rule 3).
    retained: Vec<RetainedBearing>,
    /// The dense-group filter, present only while the mode is engaged (GAP-015).
    /// Owned here, on this pipeline's own task: no lock, no channel, no sharing.
    dense: Option<DenseGroupState>,
    /// What the dense-group mode reported for the **most recent epoch**, and nothing
    /// older. Cleared at the start of every epoch and refilled only if the mode ran and
    /// succeeded, so a caller can never be handed a group claim from an epoch that has
    /// already passed.
    dense_estimate: Option<DenseGroupEstimate>,
    /// Consecutive epochs whose scan was below the engagement threshold while the
    /// dense-group filter was running. The filter is released when this reaches
    /// `settings.delete_after_misses`; see [`FusionPipeline::run_dense_group`].
    dense_quiet_epochs: u32,
    /// Epochs the dense-group filter refused (`gungnir_rfs::RfsError`). Kept here rather
    /// than on [`PipelineStats`], whose fields are a wire contract
    /// (`gungnir_model::PipelineStatsView`, GAP-096) that this change has no business
    /// widening; reported through [`crate::PipelineSnapshot::dense_group_refusals`].
    dense_refusals: u64,
    stats: PipelineStats,
}

impl std::fmt::Debug for FusionPipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FusionPipeline")
            .field("settings", &self.settings)
            .field("buffered", &self.buffer.len())
            .field("retained_bearings", &self.retained.len())
            .field("cursor_s", &self.cursor_s)
            .field("dense_group_engaged", &self.dense.is_some())
            .field("dense_group_refusals", &self.dense_refusals)
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
            estimate_time_s: HashMap::new(),
            buffer: Vec::new(),
            cursor_s: None,
            newest_seen_s: f64::NEG_INFINITY,
            retained: Vec::new(),
            dense: None,
            dense_estimate: None,
            dense_quiet_epochs: 0,
            dense_refusals: 0,
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

    /// [`FusionPipeline::snapshot`], each track paired with the time its state and
    /// covariance are actually current for -- what [`crate::ingest`] sends, so
    /// `gungnir-tracking-service` never has to substitute its own polling time.
    ///
    /// A live track always has an entry: one is written when a track is initiated and
    /// refreshed every epoch that predicts it, whether or not it was hit that scan (an
    /// epoch predicts every live filter before associating). The cursor fallback below
    /// is defensive rather than reachable.
    #[must_use]
    pub fn timed_snapshot(&self) -> Vec<TimedTrack> {
        self.manager
            .live_tracks()
            .map(|track| TimedTrack {
                estimate_time_s: self
                    .estimate_time_s
                    .get(&track.id)
                    .copied()
                    .unwrap_or(self.cursor_s.unwrap_or(f64::NAN)),
                track: track.clone(),
            })
            .collect()
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
            // position update starts from it. For `ImmCvCt` this is a stated
            // simplification DN-28 does not resolve: every mode restarts from the same
            // refined state at the baseline's initial mode probabilities, rather than
            // carrying forward what the mode probabilities had learned. A bearing
            // refines the *position* estimate a gate already admitted it into; it is not
            // itself evidence about which mode the target is in.
            if let Some(filter) = self.new_filter(state, &covariance) {
                self.filters.insert(id, filter);
                self.manager.update_estimate(id, state, covariance);
                // Applied at the cursor, not the bearing's own source time -- see this
                // method's documentation on why the update itself is not retrodicted.
                self.estimate_time_s
                    .insert(id, self.cursor_s.unwrap_or(bearing.timestamp_s));
                self.stats.bearings_updated = self.stats.bearings_updated.saturating_add(1);
                return BearingOutcome::Updated(id);
            }
            // `new_filter` refused the baseline's own imm-cv-ct fields, which
            // `gungnir-config` validates before a candidate is promoted (DN-28 §5); the
            // existing filter is left in place rather than dropped, and the bearing
            // falls through to the retained path below as though it had matched nothing.
            tracing::error!(
                ?id,
                "a bearing gated into a track but its filter could not be rebuilt; \
                 the track keeps its previous estimate and the bearing is treated as \
                 unmatched"
            );
        }

        // Rule 3: a bearing that updates nothing is retained for a caller to draw,
        // not dropped -- and no caller draws one yet (GAP-096).
        let until_s = bearing.timestamp_s + self.settings.bearing_retention_s;
        self.retained.push(RetainedBearing {
            bearing: *bearing,
            until_s,
        });
        self.stats.bearings_retained = self.stats.bearings_retained.saturating_add(1);
        BearingOutcome::Retained { until_s }
    }

    /// The bearings that matched no track and are still inside their lifetime
    /// (DN-27 §5 rule 3). **To be drawn as rays, never as symbols** (§7) -- by a caller
    /// that does not exist yet: nothing outside this crate's own tests calls this, which
    /// is the whole of GAP-096.
    #[must_use]
    pub fn retained_bearings(&self) -> &[RetainedBearing] {
        &self.retained
    }

    /// What the dense-group mode reported for the most recent epoch, if it ran (GAP-015).
    ///
    /// **`None` is the ordinary answer** and means the mode did not engage for that
    /// epoch: the scan was resolvable, or the mode is switched off, or the filter refused
    /// the scan. It is never a stale estimate from an earlier epoch -- see
    /// [`DenseGroupEstimate::epoch_s`], which is the instant this is of.
    ///
    /// **What it is not**: tracks. See [`crate::dense_group`] for why a PHD or CPHD
    /// intensity has no identity to give and why this type refuses to imply one.
    #[must_use]
    pub fn dense_group_estimate(&self) -> Option<&DenseGroupEstimate> {
        self.dense_estimate.as_ref()
    }

    /// Whether the dense-group filter is currently running.
    ///
    /// True from the epoch the mode engaged until the epoch it was released, including
    /// epochs in between whose own scan was not dense -- see
    /// [`FusionPipeline::run_dense_group`] for why the filter is not torn down the first
    /// time a raid's scan count dips.
    #[must_use]
    pub fn dense_group_engaged(&self) -> bool {
        self.dense.is_some()
    }

    /// Epochs whose dense-group step was refused by the filter and produced no estimate.
    ///
    /// **Counted rather than only logged.** A refusal means the mode reported nothing for
    /// an epoch it was engaged for, and a caller reading a run of `None`s has to be able
    /// to tell "the scene was resolvable" from "the filter would not run".
    ///
    /// Two shapes of refusal, and they end differently. A refused *scan* releases the
    /// filter and the mode engages again on the next dense epoch. A refused *settings*
    /// object -- reachable only for a hand-built [`PipelineSettings`] the filters would
    /// not accept -- switches the mode off for the rest of the session, because the
    /// refusal is deterministic and retrying it every epoch would only fill the log.
    /// [`FusionPipeline::settings`] then reports `dense_group: None`, so what the pipeline
    /// says about itself stays true.
    #[must_use]
    pub fn dense_group_refusals(&self) -> u64 {
        self.dense_refusals
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
            self.estimate_time_s.remove(id);
        }

        // The estimates the filters hold are the truth about kinematics; write them
        // into the records the lifecycle owns. Every filter was predicted to `epoch_s`
        // above whether or not it was hit this scan, so every one is stamped with it,
        // not only the ones in `hits`.
        for (id, filter) in &self.filters {
            self.manager
                .update_estimate(*id, *filter.state(), *filter.covariance());
            self.estimate_time_s.insert(*id, epoch_s);
        }

        for measurement in unassigned {
            self.initiate(&measurement, epoch_s);
        }

        // Last, and reading `scan` rather than anything association produced: the
        // dense-group mode exists because the association above stops being meaningful
        // at this density, so deriving its input from that association's output would
        // make it inherit the failure it is there to cover (GAP-015).
        self.run_dense_group(&scan, epoch_s);
    }

    /// The dense-group mode for one epoch (GAP-015): engage, run, report, release.
    ///
    /// # When it engages
    ///
    /// When the epoch's scan holds more detections than the exact association in this
    /// workspace will attempt: more than [`DenseGroupSettings::engage_above_detections`],
    /// which defaults to `gungnir_association::jpda::MAX_DETECTIONS`. That constant is
    /// that crate's own statement of where a scan stops being one exact association
    /// problem, and GAP-015's Impact names raids "over the association limit" as the case
    /// this mode is for. `MAX_TRACKS` sits beside it there and is deliberately **not** a
    /// second trigger -- see [`DenseGroupSettings::engage_above_detections`] for why a
    /// bound on combinatorial cost is not a statement about density.
    ///
    /// # Why it is not released the moment the scan thins
    ///
    /// Once engaged the filter keeps running until the scan has been below the threshold
    /// for [`PipelineSettings::delete_after_misses`] consecutive epochs. A raid's scan
    /// count fluctuates across any threshold, and tearing the filter down on the first dip
    /// would throw away the intensity and restart the count from nothing, so the reported
    /// count would swing for a reason that is an artefact of the trigger rather than a
    /// fact about the sky.
    ///
    /// **The epoch count is the pipeline's own existing figure, not a new one.**
    /// `delete_after_misses` is already this pipeline's answer to "how many consecutive
    /// scans of contrary evidence before a belief is dropped", for a track; a group is
    /// dropped on the same evidence.
    ///
    /// # A refusal produces no estimate rather than a stale one
    ///
    /// If the filter refuses the scan the estimate is dropped, the filter is released,
    /// and [`FusionPipeline::dense_group_refusals`] counts it. Carrying the previous
    /// epoch's estimate forward would be a group claim about an instant nothing observed.
    fn run_dense_group(&mut self, scan: &[Detection], epoch_s: f64) {
        // First, so that a pipeline with the mode switched off does no work at all here
        // and not even a write: `dense_estimate` can only be `Some` if this ran.
        let Some(settings) = self.settings.dense_group else {
            return;
        };
        // The estimate is of this epoch or of nothing. Cleared before anything can refill
        // it, so a refusal or a release below leaves no claim from an earlier epoch.
        self.dense_estimate = None;

        let dense = scan.len() > settings.engage_above_detections;
        if self.dense.is_none() {
            if !dense {
                return;
            }
            let (h, r) = self.measurement_model();
            match DenseGroupState::new(settings, h, r) {
                Ok(state) => {
                    tracing::info!(
                        detections = scan.len(),
                        filter = settings.filter.as_str(),
                        "the scan is past the association limit; the dense-group filter \
                         is engaged and reports a count, not tracks"
                    );
                    self.dense = Some(state);
                }
                Err(err) => {
                    // Reachable only for hand-built settings; `DenseGroupSettings::default`
                    // describes a scene. Counted rather than only logged, for the reason
                    // `dense_group_refusals` gives.
                    tracing::error!(%err, "the dense-group filter refused its own settings");
                    self.dense_refusals = self.dense_refusals.saturating_add(1);
                    self.settings.dense_group = None;
                    return;
                }
            }
        }

        let births = self.dense_births(scan, settings.birth_weight);
        let measurements: Vec<SVector<f64, 3>> = scan.iter().map(|d| d.measurement).collect();
        let motion = ConstantVelocity {
            sigma_a_sq: self.settings.process_noise_psd,
        };
        let Some(state) = self.dense.as_mut() else {
            return;
        };
        if let Err(err) = state.step(motion, epoch_s, &births, &measurements) {
            tracing::error!(%err, "the dense-group filter refused this scan; no group is reported");
            self.dense_refusals = self.dense_refusals.saturating_add(1);
            self.dense = None;
            self.dense_quiet_epochs = 0;
            return;
        }
        self.dense_estimate = Some(state.estimate(epoch_s, scan.len()));

        if dense {
            self.dense_quiet_epochs = 0;
            return;
        }
        self.dense_quiet_epochs = self.dense_quiet_epochs.saturating_add(1);
        if self.dense_quiet_epochs >= self.settings.delete_after_misses {
            tracing::info!(
                quiet_epochs = self.dense_quiet_epochs,
                "the scan has not been dense for the deletion window; the dense-group \
                 filter is released"
            );
            self.dense = None;
            self.dense_quiet_epochs = 0;
            self.dense_estimate = None;
        }
    }

    /// One birth component per detection in the epoch, at
    /// [`DenseGroupSettings::birth_weight`].
    ///
    /// The covariance is [`FusionPipeline::single_detection_covariance`] -- the same one
    /// [`FusionPipeline::initiate`] gives a track started from a single detection, rather
    /// than a second statement of the same prior that could drift from the first.
    fn dense_births(&self, scan: &[Detection], weight: f64) -> Vec<GaussianComponent> {
        let cov = self.single_detection_covariance();
        scan.iter()
            .map(|detection| {
                let mut mean = SVector::<f64, 6>::zeros();
                mean.fixed_rows_mut::<3>(0)
                    .copy_from(&detection.measurement);
                GaussianComponent { weight, mean, cov }
            })
            .collect()
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

    /// Start a tentative track from one detection: position measured, velocity unknown,
    /// current as of `epoch_s`.
    fn initiate(&mut self, measurement: &SVector<f64, 3>, epoch_s: f64) {
        let mut state = SVector::<f64, 6>::zeros();
        state.fixed_rows_mut::<3>(0).copy_from(measurement);
        let covariance = self.single_detection_covariance();
        let id = self.manager.initiate(state, covariance);
        if let Some(filter) = self.new_filter(state, &covariance) {
            self.filters.insert(id, filter);
            self.estimate_time_s.insert(id, epoch_s);
        } else {
            // See `new_filter`'s documentation: reachable only for a hand-built
            // `PipelineSettings` carrying imm-cv-ct fields `gungnir-config` would have
            // refused. The track exists with no filter behind it, which every other read
            // path here already treats as nothing to gate, predict, or report a time for.
            tracing::error!(?id, "track initiated with no filter behind it");
        }
        self.stats.initiated = self.stats.initiated.saturating_add(1);
    }

    /// The position-only measurement model every sensor in
    /// `docs/test-tracks/sensor-models.md` produces: `H` selecting position out of the
    /// six-element state, and `R` from the deployment's own noise figures (DN-30 §5).
    ///
    /// One definition rather than one per caller. [`FusionPipeline::new_filter`] builds
    /// the per-track estimators around it and [`FusionPipeline::run_dense_group`] builds
    /// the random-finite-set filter around it, and a second copy of either would let the
    /// dense-group mode drift onto a different sensor model than the tracks beside it.
    fn measurement_model(&self) -> (SMatrix<f64, 3, 6>, SMatrix<f64, 3, 3>) {
        let mut h = SMatrix::<f64, 3, 6>::zeros();
        let mut r = SMatrix::<f64, 3, 3>::zeros();
        for axis in 0..3 {
            h[(axis, axis)] = 1.0;
            r[(axis, axis)] = self.settings.measurement_noise_var[axis];
        }
        (h, r)
    }

    /// The covariance of a state known from exactly one position measurement: the
    /// measurement's own noise on position, [`PipelineSettings::initial_velocity_var`] on
    /// velocity, nothing off-diagonal.
    ///
    /// Shared by [`FusionPipeline::initiate`] and the dense-group mode's birth components
    /// so the two cannot state the same prior differently.
    fn single_detection_covariance(&self) -> SMatrix<f64, 6, 6> {
        let mut covariance = SMatrix::<f64, 6, 6>::zeros();
        for axis in 0..3 {
            covariance[(axis, axis)] = self.settings.measurement_noise_var[axis];
            covariance[(3 + axis, 3 + axis)] = self.settings.initial_velocity_var;
        }
        covariance
    }

    /// Build this pipeline's filter for a freshly initiated or bearing-refined track, per
    /// `self.settings.filter_selection` (DN-28 §4).
    ///
    /// `None` only when `imm-cv-ct` is selected and [`Imm::new`] refuses the baseline's
    /// own transition matrix or initial mode probabilities. `gungnir-config` validates
    /// both against the same rules before a candidate is promoted (DN-28 §5), so reaching
    /// `None` means a caller built [`PipelineSettings`] directly with values no baseline
    /// could have produced -- logged and degraded by both call sites rather than a panic,
    /// matching how [`FusionPipeline::associate`] already treats a broken cost matrix as
    /// a scan nothing could be gated against rather than a reason to stop.
    fn new_filter(
        &self,
        state: SVector<f64, 6>,
        covariance: &SMatrix<f64, 6, 6>,
    ) -> Option<TrackFilter> {
        let (h, r) = self.measurement_model();
        match self.settings.filter_selection {
            FilterSelection::ConstantVelocity => {
                Some(TrackFilter::ConstantVelocity(KalmanFilter::new(
                    state,
                    *covariance,
                    ConstantVelocity {
                        sigma_a_sq: self.settings.process_noise_psd,
                    },
                    h,
                    r,
                )))
            }
            FilterSelection::ImmCvCt => {
                // Both modes start from the same estimate: a single detection says
                // nothing about which mode the target is in, so there is no basis for
                // giving them different priors.
                let cv: Box<dyn ModeFilter<6, 3> + Send> = Box::new(KalmanFilter::new(
                    state,
                    *covariance,
                    ConstantVelocity {
                        sigma_a_sq: self.settings.process_noise_psd,
                    },
                    h,
                    r,
                ));
                let ct: Box<dyn ModeFilter<6, 3> + Send> = Box::new(KalmanFilter::new(
                    state,
                    *covariance,
                    CoordinatedTurn {
                        omega: self.settings.imm_turn_rate_rad_s,
                        sigma_a_sq: self.settings.process_noise_psd,
                    },
                    h,
                    r,
                ));
                let transition: Vec<Vec<f64>> = self
                    .settings
                    .imm_mode_transition
                    .iter()
                    .map(|row| row.to_vec())
                    .collect();
                match Imm::new(
                    vec![cv, ct],
                    &self.settings.imm_initial_mode_probabilities,
                    &transition,
                ) {
                    Ok(imm) => Some(TrackFilter::ImmCvCt { imm, h, r }),
                    Err(err) => {
                        tracing::error!(
                            %err,
                            "imm-cv-ct baseline fields were refused by Imm::new; a \
                             validated config should never reach this"
                        );
                        None
                    }
                }
            }
        }
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
