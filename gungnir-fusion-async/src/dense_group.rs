// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The dense-group mode: random-finite-set filtering alongside the per-track
//! estimator, for the raid the one-to-one association cannot resolve (GAP-015).
//!
//! # What this is for
//!
//! [`crate::pipeline::FusionPipeline`] associates one detection to one track through a
//! global assignment. That is the right algorithm while the scene is resolvable, and it
//! stops being one when a raid puts more returns in a scan than the association can hold
//! apart: the assignment still produces an answer, and the answer is that identities swap
//! between neighbouring targets and unresolved returns delete tracks. GAP-015's Impact
//! names exactly that -- "MT-01 raids over the association limit collapse identities and
//! under-count".
//!
//! A random-finite-set filter answers a different question and answers it well in that
//! regime: not *which* target is where, but *how many* targets are there and where the
//! mass is. [`gungnir_rfs::PhdFilter`] and [`gungnir_rfs::CphdFilter`] are that filter,
//! already built and gated on their own rows. This module runs one of them beside the
//! per-track filters and reports what it says.
//!
//! # Track identity: what this deliberately does not claim
//!
//! **A PHD or CPHD intensity carries no identity at all.** The intensity says there is
//! about one target here; it does not say it is the same target that was there last
//! scan. `gungnir_rfs::PhdFilter::extract_tracks` is explicit that the identifiers it
//! mints "mean nothing across scans", and handing those to the rest of the system as
//! though they were track identities would make every target on an operator's screen
//! churn once per scan, and would put identifiers into the journal that nothing can
//! follow.
//!
//! So the dense-group mode **does not produce tracks, and cannot**. Its output is
//! [`DenseGroupEstimate`], which carries a count and a set of [`GroupComponent`]s, and
//! **there is no [`gungnir_track::TrackId`] anywhere in it** -- not a synthetic one, not
//! a per-scan one, not a field a consumer could mistake for one. That is the same
//! enforcement `crate::BearingDetection` uses against DN-27 §5 rule 1: a prohibition
//! expressed as a missing field rather than as a rule someone has to remember, because a
//! check can be forgotten and an absent field cannot be read.
//!
//! [`crate::pipeline::FusionPipeline::snapshot`] and
//! [`crate::pipeline::FusionPipeline::timed_snapshot`] are untouched by this mode. The
//! group estimate rides alongside them in [`crate::PipelineSnapshot::dense_group`], and a
//! consumer that draws tracks keeps drawing exactly the tracks it drew before.
//!
//! **Identity in a dense raid is a labelled filter's job**, which is the GLMB/LMB row of
//! GAP-015 and is not built: `gungnir_rfs::GlmbFilter::labelled_tracks` returns a refusal
//! naming itself. When it exists it slots in here as a third [`DenseGroupFilter`]
//! variant whose output *does* carry labels, and that variant -- not this one -- is what
//! may be presented as tracks.
//!
//! # Nothing draws one yet
//!
//! `gungnir-tracking-service` does not read [`crate::PipelineSnapshot::dense_group`], so
//! no picture shows a group estimate today. That is the same state
//! `crate::pipeline::FusionPipeline::retained_bearings` was left in by GAP-096 and is
//! recorded the same way rather than implied to be finished.

use gungnir_association::jpda::MAX_DETECTIONS;
use gungnir_core::ConstantVelocity;
use gungnir_rfs::{CphdFilter, GaussianComponent, PhdFilter, PhdSettings, RfsError};
use nalgebra::{SMatrix, SVector};

/// The clutter intensity the dense-group mode assumes, expected false detections per
/// cubic metre of the ENU measurement space.
///
/// # Why `PhdSettings::default()`'s figure does not carry over
///
/// That default is `1e-6`, which is the value `testdata/oracles/tools/gen_phd_fixtures.py`
/// uses. It suits that fixture: its scene spans a few hundred metres and its measurement
/// noise is 25 m² per axis, so a detection's likelihood at its own target is about
/// `4.5e-5` per cubic metre and the clutter term sits a comfortable factor below it.
///
/// It does not suit this pipeline. `PipelineSettings::default`'s measurement noise is
/// `[400, 400, 900]` m², sixteen to thirty-six times the fixture's per axis, and a
/// Gaussian's peak falls as the square root of the determinant of its covariance: the
/// same detection's likelihood here is around `1.9e-6` per cubic metre. Against a clutter
/// intensity of `1e-6` that is not a comfortable factor, it is the same order of
/// magnitude -- so the filter would attribute most of every detection's weight to clutter
/// and report a fraction of the targets that are there. **Carrying that constant across
/// would have produced exactly the under-count GAP-015 is about, from inside the filter
/// built to fix it**, and it was found by a dense scan of twelve targets counting as 2.7.
///
/// # What replaces it, and where the number comes from
///
/// `gungnir-scenario`'s sensor model is the workspace's own statement of what clutter
/// looks like: `SensorModel::false_alarms_per_scan` false alarms per scan, "uniform in
/// the sphere of the sensor's range band" (`gungnir-scenario/src/sensor.rs`). A uniform
/// intensity over a ball is the count divided by its volume, so
///
/// ```text
///     lambda = false_alarms_per_scan / ((4/3) pi range_m^3)
/// ```
///
/// which is the expression below, evaluated for `SensorModel::radar_coastal` -- 2.0 false
/// alarms per scan over 40 km, "the false-alarm rate that makes Scenario 2's clutter
/// real" and **the highest of the four rows in that library**. The densest sensor is the
/// conservative choice for a default: it makes the filter least willing to call a
/// detection a target. It works out at about `7.5e-15` per cubic metre, nine orders of
/// magnitude below the fixture's figure, which is the scale of the correction rather
/// than a tuning nudge.
///
/// A deployment with its own sensor evaluates the same expression for its own
/// false-alarm rate and range and sets `PhdSettings::clutter_density` from it. The
/// formula is written out here so that is arithmetic rather than a guess.
pub const DEFAULT_CLUTTER_DENSITY: f64 =
    2.0 / (4.0 / 3.0 * std::f64::consts::PI * 40_000.0 * 40_000.0 * 40_000.0);

/// The mixture cap the dense-group mode assumes: one component per target the scene it
/// is specified against can hold, plus one birth per detection that scan.
///
/// `docs/performance-budgets.md` sizes Scenario 4 at 200 tracks; 400 is two hundred of
/// each. See [`DenseGroupSettings::phd`] for the measured counts and costs behind it, and
/// for why `PhdSettings::default()`'s 100 is not enough.
pub const DEFAULT_MAX_COMPONENTS: usize = 400;

/// The truncation bound on the CPHD's cardinality distribution: the largest target count
/// it can represent, and so the largest it can ever report.
///
/// Sized from the same Scenario 4 figure as [`DEFAULT_MAX_COMPONENTS`], because a bound
/// below the raid size would under-count by construction rather than by estimation. Inert
/// under [`DenseGroupFilter::Phd`], which propagates no cardinality distribution.
pub const DEFAULT_MAX_CARDINALITY: usize = 200;

/// Which random-finite-set filter the dense-group mode runs.
///
/// Both are built and both are gated on their own rows; they differ in what they
/// propagate about the target count, and the default is a governance choice as much as a
/// numerical one -- see each variant.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DenseGroupFilter {
    /// The Gaussian-mixture PHD (`gungnir_rfs::PhdFilter`). Propagates the intensity
    /// alone, so the count it reports is that distribution's *mean* and nothing more.
    ///
    /// **The default, because it is the half of GAP-015 the owner has signed**
    /// (2026-09-06). A deployment that takes the default gets math that has been through
    /// human review; one that selects [`Self::Cphd`] gets math that is written and gated
    /// but not yet signed. That is the only reason the cheaper and less informative
    /// filter is the default, and when the CPHD derivation is signed this default is a
    /// one-line change with a stated reason.
    #[default]
    Phd,
    /// The Gaussian-mixture CPHD (`gungnir_rfs::CphdFilter`). Propagates the whole
    /// distribution over the target count as well as the intensity.
    ///
    /// **Better suited to what this mode is for, and not the default.** A dense raid is
    /// exactly the regime of frequent missed detections -- returns merge, targets mask
    /// each other -- and `gungnir-rfs/tests/cphd_diff.rs` measures materially lower
    /// cardinality-estimate variance than PHD in that regime rather than asserting it. It
    /// also fills [`DenseGroupEstimate::most_probable_count`] and
    /// [`DenseGroupEstimate::count_distribution`], which the PHD cannot: a commander told
    /// "about 2.4 targets" is owed the difference between "almost always 2, sometimes 3"
    /// and "often 0, occasionally 5". It is not the default only because the derivation
    /// is pending the owner's review (GAP-015, landed 2026-09-08).
    Cphd,
}

impl DenseGroupFilter {
    /// The name this filter answers to in a configuration or a report.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            DenseGroupFilter::Phd => "gm-phd",
            DenseGroupFilter::Cphd => "gm-cphd",
        }
    }
}

/// How the dense-group mode is tuned, and what makes it engage.
///
/// Every threshold here is either a constant this workspace already states somewhere
/// else or a scene parameter with the same status as `gungnir_association::JpdaSettings`'
/// clutter density: a deployment that sets it from a guess gets an answer from a guess.
/// Nothing in it is a number invented for this module.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DenseGroupSettings {
    /// Which filter runs. See [`DenseGroupFilter`] for why the default is the PHD.
    pub filter: DenseGroupFilter,
    /// The scene the filter believes it is in: survival, detection, clutter, and how the
    /// mixture is kept to a workable size.
    ///
    /// `gungnir_rfs::PhdSettings::default()` is "the conventional values from the
    /// Gaussian-mixture PHD literature, which are also the ones the oracle fixture uses",
    /// and this default is that, **with two figures replaced**: `clutter_density`, for
    /// the reason [`DEFAULT_CLUTTER_DENSITY`] gives, and `max_components`, for the reason
    /// below.
    ///
    /// # The mixture cap, and what it costs
    ///
    /// `PhdSettings::default()` truncates to 100 components. That is not enough to
    /// represent the scene this mode exists for, and the shortfall shows up as exactly
    /// the failure GAP-015 records. Measured under the default [`DenseGroupFilter::Phd`]
    /// selection, on this development machine, release profile, over 20 epochs of a raid
    /// of closely-spaced targets, against the same pipeline with the mode switched off:
    ///
    /// | targets | cap | reported count | added per epoch |
    /// |---|---|---|---|
    /// | 20 | 100 | 21.0 | 4.4 ms |
    /// | 20 | 400 | 21.1 | 6.3 ms |
    /// | 60 | 100 | 57.0 | 10.2 ms |
    /// | 60 | 600 | 63.2 | 34.2 ms |
    /// | 200 | 100 | **85.5** | 36.6 ms |
    /// | 200 | 300 | 185.1 | 120.6 ms |
    /// | 200 | 400 | 198.4 | 196.3 ms |
    /// | 200 | 600 | 208.1 | 249.4 ms |
    ///
    /// A 200-target raid counted as 85 is a 57% under-count, produced by the truncation
    /// discarding real mass rather than by anything the filter got wrong.
    ///
    /// **The cap is therefore derived from the scene the system is specified against, not
    /// from the literature default.** `docs/performance-budgets.md` sizes Scenario 4 --
    /// the dense-swarm scenario -- at 200 tracks, and
    /// `docs/mission/air-defense-and-counter-uas.md`
    /// puts raid sizes at "tens to over a hundred". The mixture needs one
    /// component per target it must hold apart, plus one birth component per detection in
    /// the scan that produced them, so 400 is two hundred of each. It counts a 200-target
    /// raid as 198.4.
    ///
    /// **This machine is 8 P-core and 12 E-core and debug-profile timings on it vary
    /// about twofold by core class** (`docs/performance-budgets.md`); the figures above
    /// are release profile and are still an order of magnitude rather than a baseline.
    /// What they establish is the shape -- hundreds of milliseconds per epoch at the
    /// specified raid size -- which is what
    /// [`crate::PipelineSettings::dense_group`] being `None` by default follows from.
    ///
    /// # The rest of it is a scene description, not a tuning knob
    ///
    /// `probability_of_detection` in particular decides what the reported count *means*.
    /// The PHD's steady-state count under measurement-driven birth is roughly
    /// `N / (1 - (1 - p_D) p_S)` for `N` detections a scan, so a filter told `p_D = 0.95`
    /// while its sensor actually resolves four returns in five reports about 15% fewer
    /// targets than are there -- which is the under-count GAP-015 exists about, arrived at
    /// from the other direction. `gungnir_scenario::SensorModel`'s own rows run from 0.6
    /// to 0.92, so the literature default of 0.95 is optimistic for every sensor in that
    /// library. It is left at the literature value here rather than replaced, because
    /// unlike the clutter density it is not wrong by scale for this pipeline -- it is a
    /// property of a deployment's sensor, and inventing a different constant would only
    /// move which sensor the default happens to suit.
    pub phd: PhdSettings,
    /// The mode engages for an epoch whose scan holds more than this many detections.
    ///
    /// **Defaults to `gungnir_association::jpda::MAX_DETECTIONS`.** That constant is this
    /// workspace's own statement of where a scan stops being one exact association
    /// problem -- "the most detections exact enumeration will attempt", with `jpda.rs`'s
    /// module documentation adding that "a dense scene that outgrows exact JPDA needs a
    /// different algorithm, and saying so is the useful report". This mode is that
    /// different algorithm, so it engages at that stated line rather than at a number
    /// chosen here.
    ///
    /// The pipeline's own associator is Jonker-Volgenant and does not refuse at this
    /// count -- it keeps producing one-to-one assignments past it, which is precisely the
    /// failure GAP-015 records. The constant is cited as the workspace's threshold for
    /// "too dense to resolve", not as a limit the pipeline currently enforces.
    ///
    /// # `MAX_TRACKS` was considered as a second trigger and deliberately is not one
    ///
    /// `jpda.rs` states a track limit beside the detection one, and an early version of
    /// this mode engaged on either. That was wrong. `MAX_TRACKS` is a bound on
    /// *combinatorial cost* -- how many tracks exact enumeration of joint association
    /// events will attempt -- and not a statement about whether a scene is resolvable.
    /// Nine well-separated aircraft are an ordinary air picture that the pipeline's global
    /// assignment handles exactly as it should, and engaging a random-finite-set filter
    /// for them would spend the work and, worse, publish a group estimate for a scene
    /// that has perfectly good tracks. A raid is characterised by how many returns arrive
    /// in one scan, which is the detection count and not the track count -- and because an
    /// epoch is `PipelineSettings::epoch_s` wide, a scan is what a radar reports at one
    /// instant, so a dense scene is a large scan.
    pub engage_above_detections: usize,
    /// The expected number of *new* targets each detection in an engaged epoch is taken
    /// to announce, which is the weight of the birth component placed at it.
    ///
    /// # This is the one modelling parameter here, and it is not derived
    ///
    /// A PHD needs a birth intensity, and this pipeline has no birth model: where targets
    /// may appear is a property of a deployment's scene -- a runway, a border, the edge of
    /// a sensor's coverage -- and not of the filter. Births are therefore placed at the
    /// epoch's detections, which is the standard measurement-driven birth model, and this
    /// is its weight.
    ///
    /// **Births are placed at every detection in the scan, not only the ones association
    /// left over.** Deriving the birth set from the assignment would make this mode
    /// depend on the association whose failure is the reason the mode exists.
    ///
    /// The default of 0.1 is a placeholder in the same sense
    /// `crate::PipelineSettings::measurement_noise_var`'s default was before DN-30: it is
    /// small because most detections in a tracked scene belong to targets the intensity
    /// already holds, and a deployment's own figure belongs in its baseline. **It is not
    /// the 0.4 the PHD oracle fixture uses**, which is a birth-*region* weight at known
    /// truth positions and a different model. Nothing promoted supplies this yet.
    ///
    /// # It matters much less than being undetermined suggests
    ///
    /// The PHD update normalises per detection by `clutter + sum of the candidate
    /// weights`, so the total weight one detection contributes is
    /// `(total - clutter) / total` however that weight is divided between the birth
    /// component at it and the surviving components near it. With
    /// [`DEFAULT_CLUTTER_DENSITY`] at 7.5e-15 against a likelihood term around 1.9e-7,
    /// that ratio is one to within about 4e-8: **each detection contributes about one
    /// target's worth of mass whatever this is set to**, and the birth weight only really
    /// decides the very first scan, before there are survivors to compete with, and how
    /// the filter behaves in heavy clutter.
    ///
    /// That is why an undetermined parameter is tolerable here and would not be in the
    /// clutter density beside it. It is still a deployment's to set, and it is named as
    /// undetermined rather than presented as a chosen value.
    pub birth_weight: f64,
    /// The truncation bound on the CPHD's cardinality distribution. Ignored under
    /// [`DenseGroupFilter::Phd`], which propagates no such distribution.
    ///
    /// `gungnir_rfs::CphdFilter::new` documents this as "a real budget, not a formality"
    /// because predict and update each cost `O(max_cardinality)` per detection. It is
    /// also a **ceiling on the answer**: a distribution truncated at `n` cannot report
    /// more than `n` targets, so a bound below the raid size would under-count by
    /// construction -- the same trap the mixture cap above was found in.
    ///
    /// It is therefore sized from the same place: `docs/performance-budgets.md`'s
    /// Scenario 4 figure of 200 tracks. See [`DEFAULT_MAX_CARDINALITY`].
    pub max_cardinality: usize,
}

impl Default for DenseGroupSettings {
    fn default() -> Self {
        Self {
            filter: DenseGroupFilter::default(),
            phd: PhdSettings {
                clutter_density: DEFAULT_CLUTTER_DENSITY,
                max_components: DEFAULT_MAX_COMPONENTS,
                ..PhdSettings::default()
            },
            engage_above_detections: MAX_DETECTIONS,
            birth_weight: 0.1,
            max_cardinality: DEFAULT_MAX_CARDINALITY,
        }
    }
}

/// One component of the group intensity: mass at a place, with no identity attached.
///
/// **Deliberately not a `gungnir_track::Track`.** It has no identifier, no status and no
/// hit count, because the filter that produced it has none of those to give. See this
/// module's documentation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroupComponent {
    /// The expected number of targets this component accounts for.
    ///
    /// **Not a probability, and it may exceed one.** A PHD weight is an expected count,
    /// and a component of weight 2.4 is about two targets that have not been resolved from
    /// each other -- which in a raid is the number that matters.
    pub expected_targets: f64,
    /// Position and velocity in the local ENU frame, the same six-element state the
    /// per-track filters carry.
    pub state: SVector<f64, 6>,
    /// Its covariance.
    pub covariance: SMatrix<f64, 6, 6>,
}

/// What the dense-group mode reports for one epoch: a count and a shape, never tracks.
///
/// Read it from [`crate::PipelineSnapshot::dense_group`]. `None` there means the mode did
/// not engage for that epoch, which is the ordinary case.
#[derive(Debug, Clone, PartialEq)]
pub struct DenseGroupEstimate {
    /// Mission time, seconds: the epoch this estimate is of. The same instant
    /// `crate::TimedTrack::estimate_time_s` carries, and for the same reason -- a caller
    /// must never stamp it with its own polling time.
    pub epoch_s: f64,
    /// The expected number of targets in the scene: the integral of the intensity.
    ///
    /// **A mean, not a count.** 2.4 means the filter's best estimate is between two and
    /// three; reporting it as either throws away what it knows.
    pub expected_targets: f64,
    /// The most probable target count, under [`DenseGroupFilter::Cphd`] only.
    ///
    /// `None` under the PHD, which propagates the mean and no distribution to take a mode
    /// of -- **not zero, and not the mean rounded**, either of which would be an invented
    /// answer to a question the PHD cannot answer.
    pub most_probable_count: Option<usize>,
    /// The posterior distribution over target count: `[n]` is the probability there are
    /// exactly `n` targets. **Empty under the PHD**, for the reason above.
    pub count_distribution: Vec<f64>,
    /// The intensity's components. Unlabelled, and in no meaningful order across scans.
    pub components: Vec<GroupComponent>,
    /// Which filter produced this, so a consumer reporting it can say which.
    pub filter: DenseGroupFilter,
    /// How many detections the epoch held: what crossed
    /// [`DenseGroupSettings::engage_above_detections`] and engaged the mode, or did not
    /// while the mode was still running out its release window.
    pub scan_size: usize,
}

/// The running filter, and the settings it was built from. Owned by the pipeline, on the
/// pipeline's own task: this type introduces no channel, no lock and no shared state.
#[derive(Debug)]
pub(crate) struct DenseGroupState {
    running: Running,
    settings: DenseGroupSettings,
    /// The epoch [`Self::step`] last ran at, so the mode owns its own `dt` rather than
    /// borrowing the pipeline's. The two differ: the mode engages and is released on its
    /// own terms, so the interval since *it* last ran is not the interval since the
    /// pipeline's previous epoch, and predicting by the wrong one would move the
    /// intensity by an interval nothing observed.
    last_epoch_s: Option<f64>,
}

#[derive(Debug)]
enum Running {
    Phd(Box<PhdFilter>),
    Cphd(Box<CphdFilter>),
}

impl DenseGroupState {
    /// Build the selected filter around the pipeline's own measurement model.
    ///
    /// # Errors
    ///
    /// [`RfsError::MalformedScene`] when the settings do not describe a scene, which the
    /// filters check for themselves.
    pub(crate) fn new(
        settings: DenseGroupSettings,
        h: SMatrix<f64, 3, 6>,
        r: SMatrix<f64, 3, 3>,
    ) -> Result<Self, RfsError> {
        let running = match settings.filter {
            DenseGroupFilter::Phd => Running::Phd(Box::new(PhdFilter::new(settings.phd, h, r)?)),
            DenseGroupFilter::Cphd => Running::Cphd(Box::new(CphdFilter::new(
                settings.phd,
                h,
                r,
                settings.max_cardinality,
            )?)),
        };
        Ok(Self {
            running,
            settings,
            last_epoch_s: None,
        })
    }

    /// One scan at `epoch_s`: predict forward from the epoch this filter last ran at,
    /// add a birth at every detection, update on all of them.
    ///
    /// Births at the detections rather than at a configured region, for the reason
    /// [`DenseGroupSettings::birth_weight`] gives. The birth covariance is the one
    /// `crate::pipeline::FusionPipeline::initiate` already gives a track started from a
    /// single detection -- the measurement's own noise on position, the settings' prior on
    /// velocity -- so no second statement of the same thing can drift from the first.
    ///
    /// # Errors
    ///
    /// Whatever the filter refuses: a non-finite detection or birth, or an innovation
    /// covariance that cannot be inverted. The caller drops the estimate rather than
    /// reporting a stale one.
    pub(crate) fn step(
        &mut self,
        motion: ConstantVelocity,
        epoch_s: f64,
        births: &[GaussianComponent],
        detections: &[SVector<f64, 3>],
    ) -> Result<(), RfsError> {
        // Zero on the first step: a filter built this epoch has nothing to carry forward,
        // and a negative interval cannot arise because the pipeline refuses a detection
        // behind its cursor before an epoch is ever formed (`PushError::TooLate`).
        let dt = self
            .last_epoch_s
            .map_or(0.0, |last| (epoch_s - last).max(0.0));
        self.last_epoch_s = Some(epoch_s);
        match &mut self.running {
            Running::Phd(phd) => {
                phd.predict(&motion, dt, births)?;
                phd.update(detections)
            }
            Running::Cphd(cphd) => {
                cphd.predict(&motion, dt, births)?;
                cphd.update(detections)
            }
        }
    }

    /// The expected number of targets the filter currently holds: the integral of the
    /// intensity under the PHD, the cardinality distribution's mean under the CPHD.
    ///
    /// **Not what decides whether the mode stays engaged.** An earlier version released
    /// the filter once this fell below the extraction threshold, which never happened --
    /// one lone target still holds the intensity near one, so the mode ran for ever after
    /// a raid ended. Release is on consecutive non-dense epochs instead; see
    /// `crate::pipeline::FusionPipeline::run_dense_group`.
    pub(crate) fn expected_targets(&self) -> f64 {
        match &self.running {
            Running::Phd(phd) => phd.cardinality(),
            Running::Cphd(cphd) => cphd.cardinality_mean(),
        }
    }

    /// Read the filter out into the report a consumer sees.
    pub(crate) fn estimate(&self, epoch_s: f64, scan_size: usize) -> DenseGroupEstimate {
        let (intensity, most_probable_count, count_distribution) = match &self.running {
            Running::Phd(phd) => (&phd.intensity_components, None, Vec::new()),
            Running::Cphd(cphd) => (
                &cphd.phd.intensity_components,
                Some(cphd.cardinality_map()),
                cphd.cardinality_distribution().to_vec(),
            ),
        };
        DenseGroupEstimate {
            epoch_s,
            expected_targets: self.expected_targets(),
            most_probable_count,
            count_distribution,
            components: intensity
                .iter()
                .map(|c| GroupComponent {
                    expected_targets: c.weight,
                    state: c.mean,
                    covariance: c.cov,
                })
                .collect(),
            filter: self.settings.filter,
            scan_size,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> (SMatrix<f64, 3, 6>, SMatrix<f64, 3, 3>) {
        let mut h = SMatrix::<f64, 3, 6>::zeros();
        for axis in 0..3 {
            h[(axis, axis)] = 1.0;
        }
        (h, SMatrix::<f64, 3, 3>::identity() * 400.0)
    }

    fn birth_at(position: [f64; 3], weight: f64) -> GaussianComponent {
        let mut mean = SVector::<f64, 6>::zeros();
        for axis in 0..3 {
            mean[axis] = position[axis];
        }
        let mut cov = SMatrix::<f64, 6, 6>::zeros();
        for axis in 0..3 {
            cov[(axis, axis)] = 400.0;
            cov[(3 + axis, 3 + axis)] = 40_000.0;
        }
        GaussianComponent { weight, mean, cov }
    }

    /// The default threshold cites the workspace's own association limit rather than
    /// inventing one, and the default filter is the signed one.
    #[test]
    fn the_engagement_threshold_is_the_workspaces_own_association_limit() {
        let settings = DenseGroupSettings::default();
        assert_eq!(settings.engage_above_detections, MAX_DETECTIONS);
        assert_eq!(settings.filter, DenseGroupFilter::Phd);
    }

    /// The clutter intensity is the one derived for this pipeline's measurement space,
    /// not the oracle fixture's, and the derivation is the sensor library's own.
    ///
    /// Asserted rather than left to the doc comment because carrying the fixture's `1e-6`
    /// across is a silent under-count -- twelve targets counted as 2.7 -- and a silent
    /// under-count is the exact failure GAP-015 records.
    #[test]
    fn the_clutter_intensity_is_derived_for_this_pipelines_measurement_space() {
        let settings = DenseGroupSettings::default();
        // radar_coastal: 2.0 false alarms a scan, uniform in the ball of a 40 km range.
        let volume = 4.0 / 3.0 * std::f64::consts::PI * 40_000.0_f64.powi(3);
        let expected = 2.0 / volume;
        assert!((settings.phd.clutter_density - expected).abs() < f64::EPSILON * expected);
        assert!(
            settings.phd.clutter_density < PhdSettings::default().clutter_density / 1e8,
            "the fixture's small-scene figure must not have carried across: {}",
            settings.phd.clutter_density
        );
    }

    /// The mixture is big enough to hold the scene the system is specified against. A
    /// cap of 100 counts a 200-target raid as 85, which is the under-count GAP-015 is
    /// about, reproduced inside the filter built to fix it.
    #[test]
    fn the_mixture_can_hold_the_specified_raid() {
        let settings = DenseGroupSettings::default();
        assert_eq!(settings.phd.max_components, DEFAULT_MAX_COMPONENTS);
        assert!(
            settings.phd.max_components >= 2 * 200,
            "one component per Scenario 4 track plus one birth per detection"
        );
    }

    /// A PHD reports a mean and refuses to invent a mode; a CPHD reports both. The
    /// point is the `None`: a consumer must be able to tell that the PHD was not asked
    /// a question it cannot answer.
    #[test]
    fn the_phd_reports_no_most_probable_count_and_the_cphd_does() {
        let (h, r) = model();
        let detections = vec![SVector::<f64, 3>::new(0.0, 0.0, 100.0)];
        let births = vec![birth_at([0.0, 0.0, 100.0], 0.4)];
        let motion = ConstantVelocity { sigma_a_sq: 4.0 };

        let mut phd = DenseGroupState::new(DenseGroupSettings::default(), h, r).expect("built");
        phd.step(motion, 1.0, &births, &detections).expect("ran");
        let phd_estimate = phd.estimate(1.0, detections.len());
        assert_eq!(phd_estimate.most_probable_count, None);
        assert!(phd_estimate.count_distribution.is_empty());
        assert_eq!(phd_estimate.filter, DenseGroupFilter::Phd);

        let cphd_settings = DenseGroupSettings {
            filter: DenseGroupFilter::Cphd,
            ..DenseGroupSettings::default()
        };
        let mut cphd = DenseGroupState::new(cphd_settings, h, r).expect("built");
        cphd.step(motion, 1.0, &births, &detections).expect("ran");
        let cphd_estimate = cphd.estimate(1.0, detections.len());
        assert!(cphd_estimate.most_probable_count.is_some());
        assert_eq!(
            cphd_estimate.count_distribution.len(),
            DEFAULT_MAX_CARDINALITY + 1,
            "the distribution must span every count the specified raid could reach"
        );
        assert_eq!(cphd_estimate.filter, DenseGroupFilter::Cphd);
    }

    /// Twelve targets in one scan are counted as about twelve, which is the whole claim
    /// of the mode: a one-to-one associator in this scene produces a track count driven
    /// by whatever the assignment happened to pair up.
    #[test]
    fn a_dense_scan_is_counted_rather_than_associated() {
        let (h, r) = model();
        let motion = ConstantVelocity { sigma_a_sq: 4.0 };
        let truth: Vec<[f64; 3]> = (0..12)
            .map(|i| [f64::from(i) * 400.0, 0.0, 100.0])
            .collect();
        let detections: Vec<SVector<f64, 3>> = truth
            .iter()
            .map(|p| SVector::<f64, 3>::new(p[0], p[1], p[2]))
            .collect();

        let mut state = DenseGroupState::new(DenseGroupSettings::default(), h, r).expect("built");
        for scan in 0..10 {
            let births: Vec<GaussianComponent> = truth.iter().map(|p| birth_at(*p, 0.1)).collect();
            state
                .step(motion, f64::from(scan), &births, &detections)
                .expect("ran");
        }
        let estimate = state.estimate(10.0, detections.len());
        assert!(
            (estimate.expected_targets - 12.0).abs() < 2.0,
            "twelve separated targets should be counted as about twelve, got {}",
            estimate.expected_targets
        );
        assert!(
            !estimate.components.is_empty(),
            "the intensity must carry the mass it counted"
        );
    }

    /// The estimate carries no identifier of any kind. This is the mode's central
    /// honesty claim and it is asserted structurally: `GroupComponent` has three fields
    /// and none of them is an id, so nothing downstream can read continuity out of it.
    #[test]
    fn a_group_component_carries_no_identifier() {
        let (h, r) = model();
        let motion = ConstantVelocity { sigma_a_sq: 4.0 };
        let mut state = DenseGroupState::new(DenseGroupSettings::default(), h, r).expect("built");
        state
            .step(
                motion,
                1.0,
                &[birth_at([0.0, 0.0, 100.0], 0.9)],
                &[SVector::<f64, 3>::new(0.0, 0.0, 100.0)],
            )
            .expect("ran");
        let estimate = state.estimate(1.0, 1);
        let component = estimate.components.first().copied().expect("one component");
        // Destructured exhaustively on purpose: adding an identifier to this type would
        // stop this test compiling, which is the point of writing it this way.
        let GroupComponent {
            expected_targets,
            state: _,
            covariance: _,
        } = component;
        assert!(expected_targets > 0.0);
    }

    /// Settings that do not describe a scene are refused rather than silently repaired.
    #[test]
    fn a_malformed_scene_is_refused() {
        let (h, r) = model();
        let settings = DenseGroupSettings {
            phd: PhdSettings {
                max_components: 0,
                ..PhdSettings::default()
            },
            ..DenseGroupSettings::default()
        };
        assert!(DenseGroupState::new(settings, h, r).is_err());
    }
}
