// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Combines gungnir-filters/-association/-track/-rfs/-track-fusion/-fusion-async into
//! one app-facing service, per ARCHITECTURE.md §2. Each internal dependency edge
//! here is exactly the one already fixed by agentic-coding-standards.md §1.1.
//!
//! `gungnir-app` and `gungnir-node` depend on the [`TrackingService`] trait only,
//! never on the eight crates behind it, so adding/removing an internal tracking
//! capability never touches UI or node code. The public types are the canonical
//! `gungnir-model` views (ARCHITECTURE.md §7.2): the core's kinematic `Track` is
//! projected into `TrackView` by [`project_track`], and the canonical
//! `DetectionView` is reduced to the core's `Detection` by [`to_core_detection`].
//!
//! Deployment (ARCHITECTURE.md §8): [`LiveTrackingService`] is the embedded backend;
//! `gungnir-remote` provides the same trait over `gungnir-api` for the connected
//! profiles.

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use gungnir_model::Provenance;
use gungnir_track::Track;

pub mod registration;

pub use gungnir_fusion_async::Detection;
/// How the pipeline behind this service is tuned (GAP-053). Re-exported rather than
/// making every host depend on `gungnir-fusion-async`: the hosts speak to the pipeline
/// through this facade, which is the whole point of `ARCHITECTURE.md` §2.
pub use gungnir_fusion_async::{
    FilterSelection, ImmBaselineFields, PipelineSettings, UnsupportedFilter,
};
pub use gungnir_model::{DetectionView, MissionTime, SensorId, TrackId, TrackStatus, TrackView};
pub use registration::RegistrationLedger;

/// The one trait `gungnir-app::AppState`, `gungnir-node`, and `gungnir-viewport3d`
/// are allowed to depend on for "where are the targets right now."
/// Why a detection did not reach the pipeline.
///
/// One variant today, and it is an enum rather than a unit so the next reason -- a full
/// queue, a rejected epoch -- is added without changing every caller (GAP-066).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SubmitError {
    /// The pipeline task is gone, so nothing can be submitted to it.
    ///
    /// Not recoverable by retrying: `is_healthy` is already false and stays false.
    #[error("the tracking pipeline is not running; the detection was not accepted")]
    PipelineGone,
    /// The measurement is a bearing or a native range/azimuth/elevation report, and
    /// this service cannot place it (`docs/design/DN-27-bearing-only-detections.md` §4).
    ///
    /// **Named rather than dropped, and never converted.** Both angular variants need
    /// the reporting sensor's position in the local frame before they mean anything, and
    /// `DetectionView` carries a `SensorId` and not a position; nothing in this
    /// workspace resolves one for this service today. `gungnir_fusion_async` has the
    /// bearing path -- `BearingDetection` and `FusionPipeline::offer_bearing`, built to
    /// DN-27 §5 -- and this service is the wiring that is missing between them, which is
    /// an open row and not a silent conversion. Inventing a range to make one fit is
    /// exactly what DN-27 §2 exists to forbid.
    #[error(
        "the detection is a bearing or a polar report, and this service has no sensor position to place it from; it was refused rather than converted"
    )]
    NotAPosition,
    /// The measurement needs the reporting sensor's position and the resolver does not
    /// have that sensor.
    ///
    /// **Distinct from [`SubmitError::NotAPosition`] on purpose.** That one says the
    /// service has no resolver at all; this one says it has one and the sensor is not in
    /// it, which is a configuration fault with a name -- a feed reporting under a sensor
    /// identifier the baseline never declared. Collapsing the two would send an operator
    /// looking for missing wiring when the answer is a missing line in a baseline.
    #[error(
        "sensor {0} reported an angular measurement and no position is declared for it; the detection was refused rather than placed from a guessed position"
    )]
    UnknownSensorPosition(u32),
}

pub trait TrackingService: Send + Sync {
    /// Feed a validated detection in from any sensor. Non-blocking: internally hands
    /// off to the fusion-async ingestion pipeline via channel, never blocks the
    /// render/UI thread.
    /// # Errors
    ///
    /// [`SubmitError::PipelineGone`] when the detection could not be handed off, and
    /// [`SubmitError::NotAPosition`] when the measurement is a bearing or a polar report
    /// this service has no sensor position to place (DN-27 §4).
    ///
    /// **This used to return `()`** (GAP-066): a caller could not tell a detection that
    /// reached the pipeline from one that was dropped because the pipeline had died, and
    /// the gateway that feeds this counts what it accepted. Counting an accepted
    /// detection that went nowhere is how an ingest rate looks healthy while nothing is
    /// being tracked.
    fn submit_detection(&mut self, detection: DetectionView) -> Result<(), SubmitError>;

    /// Pull any completed pipeline output into the snapshot [`tracks`](Self::tracks)
    /// returns. Called once per tick by the host; non-blocking. `now` stamps the
    /// projected views.
    fn poll(&mut self, now: MissionTime);

    /// Non-blocking snapshot of current tracks (confirmed + coasting). Cheap to call
    /// every frame from `update()` per the UI standards' immediate-mode rule.
    fn tracks(&self) -> &[TrackView];

    /// True if the underlying pipeline is running and reporting (no stalled OOS
    /// buffer, no fusion divergence beyond budget). False while the pipeline is
    /// unimplemented, so the health panel never claims a working tracker.
    fn is_healthy(&self) -> bool;
}

/// Where each sensor measures from, in the local ENU frame.
///
/// **The piece GAP-001's closing action calls "the sensor-position resolver DN-27 needs
/// before a bearing can reach the tracker at all".** `DetectionView` carries a `SensorId`
/// and not a position, deliberately: a detection says who saw something, and where that
/// sensor is belongs to the deployment rather than to the report. Something has to join
/// the two, and until this existed nothing did, so every angular measurement was refused.
///
/// Held as data rather than read from configuration for the reason the pipeline settings
/// and the staleness policy are: this crate may not depend on `gungnir-config`. Both
/// binaries build it from the baseline's sensor list and hand it in.
///
/// **A sensor absent from this map is a refusal, never a default.** There is no origin
/// fallback: placing a bearing from [0, 0, 0] because the sensor is unknown would draw a
/// ray from the wrong place with no indication that anything was assumed, which is DN-27
/// §2's prohibition wearing a different hat.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SensorPositions {
    by_id: std::collections::HashMap<u32, [f64; 3]>,
}

impl SensorPositions {
    /// Build from the deployment's sensors.
    ///
    /// A non-finite coordinate is dropped rather than stored: it could only place a
    /// detection at a non-finite position, and the refusal that follows names the sensor.
    #[must_use]
    pub fn from_sensors(sensors: impl IntoIterator<Item = (u32, [f64; 3])>) -> Self {
        Self {
            by_id: sensors
                .into_iter()
                .filter(|(_, p)| p.iter().all(|v| v.is_finite()))
                .collect(),
        }
    }

    /// Where this sensor measures from, if the deployment declared it.
    #[must_use]
    pub fn get(&self, sensor: u32) -> Option<[f64; 3]> {
        self.by_id.get(&sensor).copied()
    }

    /// How many sensors have a declared position.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Whether any sensor has a declared position.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

/// Place a range-azimuth-elevation report from the sensor that made it.
///
/// **This invents nothing.** A range, an azimuth and an elevation from a known point are
/// a position; the conversion is arithmetic and DN-27 §2's prohibition does not reach it,
/// because nothing is assumed. That prohibition is about a *bearing*, which has no range
/// to convert.
///
/// Azimuth is `atan2(east, north)`: a compass bearing, zero at north, increasing to the
/// east -- the convention `gungnir_model::Measurement` and
/// `gungnir_filters::RangeAzimuthElevation` both state (DN-27 §4).
#[must_use]
pub fn place_polar(
    sensor_enu: [f64; 3],
    range_m: f64,
    azimuth_rad: f64,
    elevation_rad: f64,
) -> [f64; 3] {
    let horizontal = range_m * elevation_rad.cos();
    [
        sensor_enu[0] + horizontal * azimuth_rad.sin(),
        sensor_enu[1] + horizontal * azimuth_rad.cos(),
        sensor_enu[2] + range_m * elevation_rad.sin(),
    ]
}

/// Reduce the canonical observation to the kinematic form the core consumes.
///
/// `None` when the measurement is **not a position**, which since
/// `docs/design/DN-27-bearing-only-detections.md` §4 it need not be. There is
/// deliberately no fallback: DN-27 §2 forbids turning a bearing into a position by
/// assuming a range, in any of its three tempting forms, and a conversion here that
/// invented one would produce a valid `Detection` that entered the tracker without
/// complaint, initiated a track and was drawn as a symbol at a place nothing is.
///
/// The two angular variants need the reporting sensor's position in the local frame to
/// be used at all, and `DetectionView` carries a `SensorId` and not a position, so this
/// function cannot supply one. See [`SubmitError::NotAPosition`] for what the live
/// service does with them, and what is not yet wired.
pub fn to_core_detection(d: &DetectionView) -> Option<Detection> {
    d.measurement.position_enu().map(|measurement| Detection {
        sensor_id: d.sensor.0,
        timestamp_s: d.source_time.0,
        measurement,
    })
}

/// Whether an estimate last reported at `last_reported` is stale at `now`.
///
/// The one staleness rule in the system (GAP-012). PN-03 colours by
/// `Quality::is_stale` on the stated principle that the rule is this crate's and a second
/// one in a panel would drift from it -- and until this function existed, this crate had no
/// rule at all, so every track was drawn as fresh however long the pipeline had been silent
/// about it. `platform_class` is the key into the per-class table; it is `None` for every
/// track today because nothing on a track carries a kinematic class yet (GAP-018), so the
/// policy's `default_s` applies. That is the policy's own rule for an unclassed track, not a
/// shortcut around it.
#[must_use]
pub fn is_stale(
    last_reported: MissionTime,
    now: MissionTime,
    platform_class: Option<&str>,
    settings: &gungnir_model::StalenessSettings,
) -> bool {
    let limit_s = platform_class.map_or(settings.default_s, |c| settings.for_class(c));
    // A limit of zero or less means the deployment configured no staleness at all, and a
    // track must not be drawn stale by an unconfigured rule.
    limit_s > 0.0 && now.seconds_since(last_reported) > limit_s
}

/// Project a core track into the canonical view, stamped with the time its state and
/// covariance are actually current for. Classification is `Unknown` until
/// `gungnir-identification` sets it; quality confidence is not yet produced by the core
/// and is left at the default.
///
/// **`estimate_time` is not the caller's own clock.** It used to be: this function took
/// `now` and stamped every projection with it, so `TrackView::mission_time` read as the
/// time the estimate is *of* while actually carrying the time the service was last
/// *asked* -- a track the pipeline had gone quiet on kept reading as freshly updated for
/// as long as anyone kept polling. `estimate_time` is `gungnir_fusion_async::TimedTrack`'s
/// own per-track time, so it changes only when the pipeline actually moves the track.
pub fn project_track(
    track: &Track,
    estimate_time: MissionTime,
    provenance: &Provenance,
) -> TrackView {
    TrackView {
        id: track.id,
        status: track.status,
        state: track.state,
        covariance: track.covariance,
        classification: gungnir_model::Classification::Unknown,
        provenance: provenance.clone(),
        quality: gungnir_model::Quality::default(),
        mission_time: estimate_time,
        releasability: gungnir_model::Releasability::default(),
    }
}

/// What `Provenance::algorithm_version` says while nothing governs this service.
///
/// **This used to be the crate version**, which answers a different question than the one
/// the field asks: `Provenance` documents it as the version of the
/// algorithm/configuration that produced the track, resolved through `gungnir-modelops`,
/// and no `gungnir-modelops` baseline is promoted into anything (GAP-053). A semantic
/// version sitting in that field reads as a governed configuration, and the day the
/// pipeline lands (GAP-011) every track it produced would have carried one.
///
/// The build is still named, because it is the only true thing there is to say about what
/// produced a track today. The rest of the string says what is missing.
pub const UNGOVERNED_ALGORITHM_VERSION: &str = concat!(
    "ungoverned: no promoted gungnir-modelops baseline (GAP-053); build ",
    env!("CARGO_PKG_VERSION")
);

/// Default implementation wiring IMM filtering -> gating/JPDA association ->
/// track-manager lifecycle -> optional PHD/CPHD for dense regions -> track-fusion
/// across sensor platforms, all driven by `gungnir_fusion_async::ingest` on the
/// host's tokio runtime.
pub struct LiveTrackingService {
    tracks: Vec<TrackView>,
    /// `None` once [`LiveTrackingService::finish`] has ended the stream.
    detection_tx: Option<Sender<gungnir_fusion_async::Submission>>,
    /// Where each sensor measures from, so an angular report can be placed at all.
    sensor_positions: SensorPositions,
    track_rx: Receiver<Vec<gungnir_fusion_async::TimedTrack>>,
    pipeline_alive: bool,
    provenance: Provenance,
    /// The staleness policy in force (GAP-012). `Default` is a zero limit, which
    /// [`is_stale`] treats as "no rule configured" rather than "everything is stale".
    staleness: gungnir_model::StalenessSettings,
    /// When this service last *heard about* each track, which is what staleness is
    /// measured from. **Not the same clock as `TrackView::mission_time`**: that is the
    /// pipeline's own estimate time carried on `TimedTrack`, and this is how long since
    /// a poll last reported the track at all, which only this service can answer.
    last_reported: std::collections::HashMap<gungnir_model::TrackId, MissionTime>,
}

impl LiveTrackingService {
    /// Spawn the ingest task on `runtime` and wire the two boundary channels. Never
    /// panics; if the pipeline task later stops, `is_healthy` turns false.
    pub fn new(runtime: &tokio::runtime::Handle) -> Self {
        Self::with_pipeline_settings(runtime, gungnir_fusion_async::PipelineSettings::default())
    }

    /// As [`LiveTrackingService::new`], under a deployment's pipeline settings.
    ///
    /// Passed in as data rather than read from configuration, for the same reason the
    /// staleness policy is: this crate may not depend on `gungnir-config`. Both binaries
    /// build the settings from the promoted algorithm baseline and hand them here, which
    /// is what makes a promoted configuration reach the picture (GAP-053, DN-24 §7).
    #[must_use]
    pub fn with_pipeline_settings(
        runtime: &tokio::runtime::Handle,
        settings: gungnir_fusion_async::PipelineSettings,
    ) -> Self {
        let (detection_tx, detection_rx) =
            crossbeam_channel::unbounded::<gungnir_fusion_async::Submission>();
        let (track_tx, track_rx) =
            crossbeam_channel::unbounded::<Vec<gungnir_fusion_async::TimedTrack>>();
        runtime.spawn(gungnir_fusion_async::ingest_with(
            detection_rx,
            track_tx,
            settings,
        ));
        Self {
            tracks: Vec::new(),
            detection_tx: Some(detection_tx),
            track_rx,
            pipeline_alive: true,
            provenance: Provenance {
                source_sensor_ids: Vec::new(),
                calibration_baseline_version: None,
                algorithm_version: UNGOVERNED_ALGORITHM_VERSION.to_owned(),
                peer: None,
                conversion_loss: None,
                authentication: gungnir_model::SourceAuthentication::default(),
            },
            staleness: gungnir_model::StalenessSettings::default(),
            sensor_positions: SensorPositions::default(),
            last_reported: std::collections::HashMap::new(),
        }
    }

    /// Stamp the algorithm baseline this service is **actually applying** into every
    /// track's provenance (DN-24 §7, GAP-053).
    ///
    /// **Only a caller that also handed the matching settings to
    /// [`LiveTrackingService::with_pipeline_settings`] may call this.** DN-24 §7 states
    /// the rule the other way round and it is the same rule: the service may stamp an
    /// identifier only once it applies the configuration that identifier names. A track
    /// stamped with a baseline the pipeline is not running would read as governed and be
    /// the exact fiction `UNGOVERNED_ALGORITHM_VERSION` exists to prevent.
    #[must_use]
    pub fn with_algorithm_baseline(
        mut self,
        baseline: &gungnir_model::AlgorithmBaselineId,
    ) -> Self {
        self.provenance.algorithm_version = baseline.to_string();
        self
    }

    /// The staleness policy this service judges tracks by (GAP-012).
    ///
    /// Passed in as data rather than read from configuration, because this crate may not
    /// depend on `gungnir-config`; both binaries hand it `config.policy.staleness`.
    #[must_use]
    pub fn with_staleness(mut self, staleness: gungnir_model::StalenessSettings) -> Self {
        self.staleness = staleness;
        self
    }

    /// Supply the deployment's sensor positions, which is what lets an angular
    /// measurement be placed (GAP-001, DN-27 §4).
    ///
    /// Without this the service has no way to turn "sensor 4 saw something at bearing
    /// 037" into anything, and says so through [`SubmitError::NotAPosition`] rather than
    /// placing it from an assumed origin.
    #[must_use]
    pub fn with_sensor_positions(mut self, positions: SensorPositions) -> Self {
        self.sensor_positions = positions;
        self
    }

    /// End the detection stream, so the pipeline flushes its reorder buffer and emits a
    /// final snapshot.
    ///
    /// **A session that never ends its stream loses its last reorder horizon**: the
    /// buffer holds those detections waiting for a later one that never comes, and a
    /// replay would finish short of the recording it replayed. Ending the stream is a
    /// deliberate act rather than something a `Drop` does, because the desktop keeps
    /// this service for the life of a session and dropping it is not the same event as
    /// the sensors stopping.
    ///
    /// Submitting afterwards returns [`SubmitError::PipelineGone`], which is what it is.
    /// Turn a canonical detection into what the pipeline accepts, or say why it cannot.
    ///
    /// Three measurement kinds and three answers, and the differences are the point.
    ///
    /// * A **position** goes straight in, as it always did.
    /// * A **range, azimuth and elevation** is placed from the reporting sensor. That is
    ///   arithmetic, not assumption: a range from a known point *is* a position, and
    ///   DN-27 §2's prohibition does not reach it.
    /// * A **bearing** is handed over as a bearing, and stays one. It reaches the tracker
    ///   through `Submission::Bearing`, which the pipeline routes to `offer_bearing`, so
    ///   DN-27 §5's rules apply: it may refine a track, it may not start one, and if it
    ///   matches nothing it is retained and shown rather than dropped.
    ///
    /// Both angular kinds need the sensor's position and refuse without it, naming which
    /// of the two reasons applies.
    fn to_submission(
        &self,
        detection: &DetectionView,
    ) -> Result<gungnir_fusion_async::Submission, SubmitError> {
        use gungnir_model::Measurement;
        match &detection.measurement {
            Measurement::Position { .. } => to_core_detection(detection)
                .map(gungnir_fusion_async::Submission::Position)
                .ok_or(SubmitError::NotAPosition),
            Measurement::RangeAzimuthElevation {
                range_m,
                azimuth_rad,
                elevation_rad,
                variance,
            } => {
                let sensor_enu = self.sensor_enu(detection.sensor.0)?;
                let enu = place_polar(sensor_enu, *range_m, *azimuth_rad, *elevation_rad);
                if !enu.iter().all(|v| v.is_finite()) {
                    return Err(SubmitError::NotAPosition);
                }
                // **The stated variance does not survive this conversion**, and that is
                // a limitation of `Detection` rather than a choice made here: it carries
                // a position and no error, so the pipeline gates every detection with the
                // measurement noise in its settings. Converting the range-azimuth-
                // elevation variance properly needs the Jacobian of this transform and
                // somewhere to put the result, which is the same missing field DN-27 §6
                // describes for bearings. Recorded in GAP-001 rather than hidden; the
                // bearing path does not have the problem, because `BearingDetection`
                // carries its angular variance and the pipeline uses it.
                let _ = variance;
                Ok(gungnir_fusion_async::Submission::Position(Detection {
                    sensor_id: detection.sensor.0,
                    timestamp_s: detection.source_time.0,
                    measurement: nalgebra::Vector3::new(enu[0], enu[1], enu[2]),
                }))
            }
            Measurement::Bearing {
                azimuth_rad,
                elevation_rad,
                azimuth_variance_rad2,
                elevation_variance_rad2,
            } => {
                let sensor_enu = self.sensor_enu(detection.sensor.0)?;
                Ok(gungnir_fusion_async::Submission::Bearing(
                    gungnir_fusion_async::BearingDetection {
                        sensor_id: detection.sensor.0,
                        timestamp_s: detection.source_time.0,
                        sensor_enu,
                        azimuth_rad: *azimuth_rad,
                        elevation_rad: *elevation_rad,
                        azimuth_variance_rad2: *azimuth_variance_rad2,
                        elevation_variance_rad2: *elevation_variance_rad2,
                    },
                ))
            }
        }
    }

    /// Where this sensor measures from, or the refusal that says why not.
    fn sensor_enu(&self, sensor: u32) -> Result<[f64; 3], SubmitError> {
        if self.sensor_positions.is_empty() {
            // No resolver was supplied at all, which is the state every build was in
            // before GAP-001's wiring: the service cannot place an angular report and
            // says so, rather than placing it from the origin.
            return Err(SubmitError::NotAPosition);
        }
        self.sensor_positions
            .get(sensor)
            .ok_or(SubmitError::UnknownSensorPosition(sensor))
    }

    pub fn finish(&mut self) {
        self.detection_tx = None;
    }

    /// Project a pipeline snapshot into views, deciding staleness for each track.
    ///
    /// `now` decides staleness alone; each view's `mission_time` comes from its own
    /// `TimedTrack::estimate_time_s` instead, which is what fixed the poll-time defect
    /// (see [`project_track`]).
    ///
    /// Public so the rule can be exercised without a pipeline behind it: the channel this
    /// service polls is fed by a task that produces nothing until the pipeline reports.
    pub fn apply_snapshot(
        &mut self,
        snapshot: &[gungnir_fusion_async::TimedTrack],
        now: MissionTime,
    ) {
        for timed in snapshot {
            self.last_reported.insert(timed.track.id, now);
        }
        // A track absent from this snapshot keeps its last-reported time and ages.
        self.tracks = snapshot
            .iter()
            .map(|timed| {
                let mut view = project_track(
                    &timed.track,
                    MissionTime(timed.estimate_time_s),
                    &self.provenance,
                );
                let last = self
                    .last_reported
                    .get(&timed.track.id)
                    .copied()
                    .unwrap_or(now);
                view.quality.is_stale = is_stale(last, now, None, &self.staleness);
                view
            })
            .collect();
    }
}

impl TrackingService for LiveTrackingService {
    fn submit_detection(&mut self, detection: DetectionView) -> Result<(), SubmitError> {
        let core = self.to_submission(&detection)?;
        if self
            .detection_tx
            .as_ref()
            .is_some_and(|tx| tx.send(core).is_ok())
        {
            return Ok(());
        }
        if self.pipeline_alive {
            self.pipeline_alive = false;
            tracing::error!(
                "fusion-async ingest task is gone; detections can no longer be submitted"
            );
        }
        Err(SubmitError::PipelineGone)
    }

    fn poll(&mut self, now: MissionTime) {
        let mut latest: Option<Vec<gungnir_fusion_async::TimedTrack>> = None;
        loop {
            match self.track_rx.try_recv() {
                Ok(snapshot) => latest = Some(snapshot),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if self.pipeline_alive {
                        self.pipeline_alive = false;
                        tracing::error!(
                            "fusion-async ingest task stopped; track snapshot is frozen"
                        );
                    }
                    break;
                }
            }
        }
        if let Some(snapshot) = latest {
            self.apply_snapshot(&snapshot, now);
        } else {
            // No new snapshot: only staleness ages against the last report. A track's
            // `mission_time` stays exactly what it was -- the pipeline has not moved it,
            // so bumping it to `now` here is the poll-time defect `project_track` used to
            // have, reintroduced on this branch alone.
            for view in &mut self.tracks {
                if let Some(last) = self.last_reported.get(&view.id).copied() {
                    view.quality.is_stale = is_stale(last, now, None, &self.staleness);
                }
            }
        }
    }

    fn tracks(&self) -> &[TrackView] {
        &self.tracks
    }

    fn is_healthy(&self) -> bool {
        self.pipeline_alive && gungnir_fusion_async::PIPELINE_IMPLEMENTED
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use gungnir_model::SensorId;

    fn detection() -> DetectionView {
        DetectionView {
            sensor: SensorId(4),
            source_time: MissionTime(1.0),
            receipt_time: MissionTime(1.1),
            measurement: gungnir_model::Measurement::Position {
                enu: nalgebra::Vector3::new(1.0, 2.0, 3.0),
                variance_m2: [400.0, 400.0, 900.0],
            },
            provenance: Provenance::default(),
        }
    }

    /// **A version number in `algorithm_version` reads as a governed configuration.**
    /// This field used to carry the crate version, which answers a different question than
    /// the one `Provenance` documents it as answering, and nothing would have caught it:
    /// no track is produced today, so no stamp is visible until the pipeline lands and
    /// every track it produces is already labelled.
    #[test]
    fn the_algorithm_version_does_not_read_as_a_governed_configuration() {
        let v = UNGOVERNED_ALGORITHM_VERSION;
        assert!(
            v.contains("ungoverned"),
            "the field does not say that nothing governs it: {v}"
        );
        assert!(
            v.contains("gungnir-modelops"),
            "the field does not name what is missing: {v}"
        );
        // Not a bare version string: that is exactly what it used to be.
        assert!(
            v.split('.')
                .next()
                .is_some_and(|p| p.parse::<u32>().is_err()),
            "the field still reads as a plain version: {v}"
        );
    }

    fn track(id: u64) -> Track {
        Track {
            id: gungnir_model::TrackId(id),
            status: gungnir_model::TrackStatus::Confirmed,
            state: nalgebra::SVector::zeros(),
            covariance: nalgebra::SMatrix::identity(),
            misses_since_update: 0,
            hits: 3,
        }
    }

    /// A track paired with the estimate time a real pipeline snapshot would carry it
    /// with, distinct from whatever `MissionTime` a test then polls at.
    fn timed(id: u64, estimate_time_s: f64) -> gungnir_fusion_async::TimedTrack {
        gungnir_fusion_async::TimedTrack {
            track: track(id),
            estimate_time_s,
        }
    }

    fn policy(default_s: f64) -> gungnir_model::StalenessSettings {
        gungnir_model::StalenessSettings {
            default_s,
            by_class_s: std::collections::BTreeMap::new(),
        }
    }

    /// **The rule that did not exist.** A track the pipeline stops reporting goes stale by
    /// the policy's limit; before GAP-012 it was drawn fresh forever.
    #[test]
    fn a_track_the_pipeline_stops_reporting_goes_stale_by_policy() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("runtime");
        let mut svc = LiveTrackingService::new(runtime.handle()).with_staleness(policy(5.0));

        svc.apply_snapshot(&[timed(1, 10.0)], MissionTime(10.0));
        assert!(!svc.tracks()[0].quality.is_stale, "fresh on report");

        // Polled with nothing new for longer than the limit: stale, and still shown.
        svc.poll(MissionTime(16.0));
        assert_eq!(
            svc.tracks().len(),
            1,
            "a stale track was dropped, not marked"
        );
        assert!(svc.tracks()[0].quality.is_stale);

        // Reported again: fresh again.
        svc.apply_snapshot(&[timed(1, 17.0)], MissionTime(17.0));
        assert!(!svc.tracks()[0].quality.is_stale);
        drop(svc);
        runtime.shutdown_timeout(std::time::Duration::from_secs(1));
    }

    /// **The poll-time defect, pinned.** `TrackView::mission_time` is the estimate's own
    /// time, not the caller's polling clock: a track reported with an estimate time far
    /// behind the poll must keep reading as of that estimate, and a poll that hands back
    /// no new snapshot at all must not move it either.
    #[test]
    fn mission_time_is_the_estimate_time_not_the_poll_time() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("runtime");
        let mut svc = LiveTrackingService::new(runtime.handle());

        // Reported at t = 10 with an estimate current for t = 9.03 -- the pipeline's own
        // instant, well behind the poll's.
        svc.apply_snapshot(&[timed(1, 9.03)], MissionTime(10.0));
        assert_eq!(
            svc.tracks()[0].mission_time,
            MissionTime(9.03),
            "mission_time must be the estimate's time, not the poll's"
        );

        // Polled again with nothing new from the pipeline: mission_time is unmoved,
        // because the estimate itself has not changed. Only staleness may react.
        svc.poll(MissionTime(20.0));
        assert_eq!(
            svc.tracks()[0].mission_time,
            MissionTime(9.03),
            "a poll with no new snapshot must not silently advance mission_time"
        );
        drop(svc);
        runtime.shutdown_timeout(std::time::Duration::from_secs(1));
    }

    /// No configured limit means no rule, not an instant-stale rule.
    #[test]
    fn an_unconfigured_limit_never_marks_a_track_stale() {
        assert!(!is_stale(
            MissionTime(0.0),
            MissionTime(1e6),
            None,
            &policy(0.0)
        ));
    }

    /// Per class when a class is known; the default when it is not. Nothing on a track
    /// carries a kinematic class yet (GAP-018), so the second is every track today.
    #[test]
    fn a_class_limit_applies_when_the_class_is_known() {
        let mut p = policy(60.0);
        p.by_class_s.insert("cruise-missile".into(), 2.0);
        assert!(is_stale(
            MissionTime(0.0),
            MissionTime(3.0),
            Some("cruise-missile"),
            &p
        ));
        assert!(!is_stale(MissionTime(0.0), MissionTime(3.0), None, &p));
    }

    #[test]
    fn core_detection_keeps_sensor_source_time_and_measurement() {
        let d = to_core_detection(&detection()).expect("a position converts");
        assert_eq!(d.sensor_id, 4);
        assert_eq!(d.timestamp_s, 1.0);
        assert_eq!(d.measurement, nalgebra::Vector3::new(1.0, 2.0, 3.0));
    }

    /// A bearing is **not** converted into a position, and the service says so instead
    /// of guessing a range (docs/design/DN-27-bearing-only-detections.md §2 and §4).
    #[test]
    fn a_bearing_is_refused_rather_than_given_a_range() {
        let bearing = DetectionView {
            measurement: gungnir_model::Measurement::Bearing {
                azimuth_rad: 0.6,
                elevation_rad: None,
                azimuth_variance_rad2: 1e-4,
                elevation_variance_rad2: None,
            },
            ..detection()
        };
        assert!(to_core_detection(&bearing).is_none());
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let mut svc = LiveTrackingService::new(runtime.handle());
        assert_eq!(
            svc.submit_detection(bearing),
            Err(SubmitError::NotAPosition)
        );
        runtime.shutdown_timeout(std::time::Duration::from_secs(1));
    }

    /// Health is the pipeline's liveness and nothing else: true while the task is
    /// running, false the moment it is not.
    ///
    /// **This test used to assert the opposite** and was right to, because the
    /// pipeline was a stub (GAP-011). It now pins the other half of the same rule:
    /// health follows the pipeline rather than a constant, so a build with a running
    /// tracker says so and a build whose tracker has died says that instead.
    /// DN-24 §7 (GAP-053), both halves. A service applying a promoted baseline stamps
    /// its identifier on every track it projects; one that is not stays ungoverned, and
    /// the string says so in words rather than by being empty.
    #[test]
    fn a_track_carries_the_baseline_identifier_only_where_it_is_applied() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("runtime");
        let track = timed(1, 1.0);

        let mut ungoverned = LiveTrackingService::new(runtime.handle());
        ungoverned.apply_snapshot(std::slice::from_ref(&track), MissionTime(1.0));
        assert_eq!(
            ungoverned.tracks()[0].provenance.algorithm_version,
            UNGOVERNED_ALGORITHM_VERSION,
            "a service applying no promoted baseline must not claim one"
        );

        let id = gungnir_model::AlgorithmBaselineId {
            profile: gungnir_model::MissionProfile("air-defence".into()),
            name: "kf baseline".into(),
        };
        let settings = PipelineSettings::from_baseline(
            11.34,
            "kf-cv",
            &imm_fields(),
            [625.0, 3600.0, 22500.0],
        )
        .expect("implemented");
        let mut governed = LiveTrackingService::with_pipeline_settings(runtime.handle(), settings)
            .with_algorithm_baseline(&id);
        governed.apply_snapshot(std::slice::from_ref(&track), MissionTime(1.0));
        assert_eq!(
            governed.tracks()[0].provenance.algorithm_version,
            "air-defence/kf baseline"
        );

        // A filter this build does not have is refused rather than substituted.
        let err =
            PipelineSettings::from_baseline(11.34, "ekf", &imm_fields(), [625.0, 3600.0, 22500.0])
                .expect_err("not built");
        assert_eq!(err.selection, "ekf");
        assert!(err.to_string().contains("does not implement"), "{err}");

        runtime.shutdown_timeout(std::time::Duration::from_secs(1));
    }

    /// A well-formed placeholder: valid by `Imm::new`'s own rules, distinct from any
    /// deployment's real numbers, and enough to prove `from_baseline` reads and carries
    /// them rather than testing anything about a particular turn rate or transition.
    fn imm_fields() -> gungnir_fusion_async::ImmBaselineFields {
        gungnir_fusion_async::ImmBaselineFields {
            turn_rate_rad_s: 0.05,
            mode_transition: [[0.97, 0.03], [0.03, 0.97]],
            initial_mode_probabilities: [0.9, 0.1],
        }
    }

    /// **DN-28.** The default baseline names `imm-cv-ct`, and this is what closed the
    /// gap between that name and `IMPLEMENTED_FILTERS`: it is accepted, the pipeline
    /// selects it, and a track it produces carries the promoted baseline's identifier
    /// exactly as the linear filter already did above.
    #[test]
    fn imm_cv_ct_is_implemented_and_selects_the_imm() {
        let settings = PipelineSettings::from_baseline(
            11.34,
            "imm-cv-ct",
            &imm_fields(),
            [625.0, 3600.0, 22500.0],
        )
        .expect("DN-28: imm-cv-ct is implemented");
        assert_eq!(
            settings.filter_selection,
            gungnir_fusion_async::FilterSelection::ImmCvCt
        );
        assert_eq!(settings.imm_turn_rate_rad_s, 0.05);
        assert_eq!(settings.imm_mode_transition, [[0.97, 0.03], [0.03, 0.97]]);
        assert_eq!(settings.imm_initial_mode_probabilities, [0.9, 0.1]);
    }

    /// **DN-30.** A baseline's own measurement-noise variance reaches the pipeline's
    /// settings, unconditionally on filter selection -- unlike the `imm-cv-ct` fields.
    #[test]
    fn measurement_noise_var_is_read_from_the_baseline() {
        let settings = PipelineSettings::from_baseline(
            11.34,
            "kf-cv",
            &imm_fields(),
            [625.0, 3600.0, 22500.0],
        )
        .expect("implemented");
        assert_eq!(settings.measurement_noise_var, [625.0, 3600.0, 22500.0]);
    }

    #[test]
    fn health_follows_the_pipeline_and_turns_false_when_it_stops() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("runtime");
        let mut svc = LiveTrackingService::new(runtime.handle());
        svc.submit_detection(detection())
            .expect("the pipeline took it");
        svc.poll(MissionTime(2.0));
        assert!(
            svc.is_healthy(),
            "the pipeline is running and health must say so"
        );
        // One detection does not leave the reorder buffer: nothing later has arrived
        // for the horizon to be measured against, so the snapshot is honestly empty
        // rather than a track invented from a single position.
        assert!(svc.tracks().is_empty());

        // The pipeline stops with the runtime; the next hand-off fails and health
        // follows it down.
        runtime.shutdown_timeout(std::time::Duration::from_secs(1));
        assert_eq!(
            svc.submit_detection(detection()),
            Err(SubmitError::PipelineGone)
        );
        assert!(
            !svc.is_healthy(),
            "the pipeline is gone; health must not claim otherwise"
        );
    }
}
