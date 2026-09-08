// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! fusion-async: out-of-sequence handling / multi-rate fusion + concurrency
//! correctness -- verification-capability-table.md `fusion-async` rows. The *only*
//! crate that uses the tokio runtime (agentic-coding-standards.md §2.2); the runtime
//! itself is owned by the host binary and handed in as a `Handle`. Channels, not
//! Arc<Mutex<..>>, are the default for cross-task state per the same section.

pub mod pipeline;

pub use pipeline::{
    run_batch, BearingOutcome, BearingRefusal, FilterSelection, FusionPipeline, ImmBaselineFields,
    PipelineSettings, PipelineStats, PushError, RetainedBearing, TimedTrack, UnsupportedFilter,
    IMPLEMENTED_FILTERS,
};

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use std::time::Duration;

/// A raw detection as the tracking core consumes it. The canonical, provenance-bearing
/// form is `gungnir_model::DetectionView`; `gungnir-tracking-service` converts.
///
/// **This type is a position and only a position, and that is the enforcement of
/// DN-27 §5 rule 1.** A bearing arrives through [`BearingDetection`] and
/// [`FusionPipeline::offer_bearing`] instead, and there is no path from that type to
/// [`FusionPipeline::initiate`] -- not a flag to set, not a branch to take wrongly, but
/// no function that accepts one and creates a track. See
/// `docs/design/DN-27-bearing-only-detections.md` §5.
#[derive(Debug, Clone, PartialEq)]
pub struct Detection {
    pub sensor_id: u32,
    /// Source (sensor) time, seconds of mission time.
    pub timestamp_s: f64,
    /// Measurement in the local ENU frame, meters.
    pub measurement: nalgebra::Vector3<f64>,
}

/// A direction with no range: what an acoustic array, a passive radio-frequency
/// direction finder and a person with a compass report
/// (docs/design/DN-27-bearing-only-detections.md §4; GAP-001, GAP-004).
///
/// **Separate from [`Detection`] on purpose.** DN-27 §5 rule 1 forbids a bearing from
/// initiating a track: a single fixed sensor cannot localise from bearings at all,
/// the problem is unobservable, and a filter given a sequence of them converges to a
/// confident answer at the wrong range -- a failure that is silent and looks exactly
/// like success. The prohibition is a type boundary rather than a check because a check
/// can be forgotten and a missing function cannot be called.
///
/// `sensor_enu` is required and is not on `Detection`, because a bearing means nothing
/// without knowing where it was taken from. The caller resolves it from the deployment's
/// sensor list; `gungnir_model::DetectionView` carries a `SensorId` and not a position.
///
/// Azimuth is `atan2(east, north)`: a compass bearing, zero at north and increasing to
/// the east, the convention `gungnir_model::Measurement` and
/// `gungnir_filters::RangeAzimuthElevation` both state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BearingDetection {
    pub sensor_id: u32,
    /// Source (sensor) time, seconds of mission time.
    pub timestamp_s: f64,
    /// Where the bearing was measured from, local ENU metres.
    pub sensor_enu: [f64; 3],
    pub azimuth_rad: f64,
    /// `None` when the sensor reported none. **A missing elevation is not a zero one**:
    /// zero means the horizon (DN-27 §4).
    pub elevation_rad: Option<f64>,
    /// Angular variance of the azimuth, radians squared. Required: for a bearing the
    /// error *is* the information, and the cross-range error it implies grows with
    /// range (DN-27 §6).
    pub azimuth_variance_rad2: f64,
    /// Present exactly when `elevation_rad` is.
    pub elevation_variance_rad2: Option<f64>,
}

/// What a producer hands the ingest task: a position, or a direction with no range.
///
/// **Two variants rather than one, for the reason `BearingDetection` is a separate type
/// at all** (DN-27 §5 rule 1). A bearing may refine an existing track and may not start
/// one, and the pipeline enforces that through two different entry points --
/// [`FusionPipeline::push`] and [`FusionPipeline::offer_bearing`]. If the channel carried
/// one type, the loop below would have to decide which entry point to use by inspecting a
/// field, and a wrong branch there would put a bearing into the reorder buffer where it
/// would initiate a track at a range nobody measured.
///
/// The sensor's position is resolved by the *producer*, not here: `DetectionView` carries
/// a `SensorId` and the deployment's sensor list is the thing that knows where that sensor
/// is, which is `gungnir-tracking-service`'s business and not this task's.
#[derive(Debug, Clone, PartialEq)]
pub enum Submission {
    /// A measurement that determines a position. Enters the reorder buffer and may
    /// initiate a track.
    Position(Detection),
    /// A direction with no range. Offered to the tracker under DN-27 §5's three rules,
    /// and never able to initiate.
    Bearing(BearingDetection),
}

impl From<Detection> for Submission {
    fn from(d: Detection) -> Self {
        Submission::Position(d)
    }
}

impl From<BearingDetection> for Submission {
    fn from(b: BearingDetection) -> Self {
        Submission::Bearing(b)
    }
}

/// How often the ingest loop re-polls its inbound channel while idle.
const IDLE_POLL: Duration = Duration::from_millis(10);

/// Everything one pass through [`ingest_with`]'s loop produced, bundled into one
/// channel message rather than sent as three (GAP-096).
///
/// **Written and gated, not signed by the owner**: this crate is human-owned
/// (`docs/agentic-workflow.md`), and this struct and the channel type change below are
/// the mechanical part of wiring [`FusionPipeline::retained_bearings`] and
/// [`FusionPipeline::stats`] out to a caller -- the four `pipeline.rs` doc comments this
/// gap corrected were signed 2026-09-08, this was not.
///
/// Bundled on purpose rather than sent over a second channel: `tracks`, the retained
/// bearings and the stats are all read from the same pipeline at the same instant inside
/// this loop, with no `.await` between them. A poller draining two independent channels
/// could see a track snapshot from one epoch next to a bearing snapshot from another,
/// which is the same kind of skew [`TimedTrack`] exists to keep out of a single track.
#[derive(Debug, Clone, Default)]
pub struct PipelineSnapshot {
    pub tracks: Vec<TimedTrack>,
    /// [`FusionPipeline::retained_bearings`] at the same instant as `tracks`
    /// (DN-27 §5 rule 3).
    pub retained_bearings: Vec<RetainedBearing>,
    /// [`FusionPipeline::stats`] at the same instant as `tracks`.
    pub stats: PipelineStats,
}

/// Read every part of `pipeline`'s current output into one [`PipelineSnapshot`].
fn snapshot_output(pipeline: &FusionPipeline) -> PipelineSnapshot {
    PipelineSnapshot {
        tracks: pipeline.timed_snapshot(),
        retained_bearings: pipeline.retained_bearings().to_vec(),
        stats: pipeline.stats(),
    }
}

/// Whether this build has an out-of-sequence, multi-rate pipeline behind
/// [`ingest`]. `gungnir-tracking-service` reports `is_healthy() == false` while this
/// is false, so no dashboard can claim a working tracker before one exists.
///
/// **True since 2026-09-06** (GAP-011): [`pipeline::FusionPipeline`] composes the
/// signed linear Kalman filter, the Jonker-Volgenant associator, the chi-square gate
/// and the `gungnir-track` lifecycle over a reorder buffer, and
/// `tests/oos_convergence.rs` gates the `fusion-async` row -- the async path against
/// the offline batch over the same multi-sensor timeline.
///
/// **What it does not claim.** The pipeline runs a constant-velocity Kalman filter per
/// track, or, where a baseline names `imm-cv-ct` (DN-28), the CV/CT IMM over it -- one
/// selection per pipeline instance (`PipelineSettings::filter_selection`), not per
/// track. The remaining nonlinear estimators, random-finite-set filtering and
/// track-to-track fusion are separate rows and separate gaps (GAP-011's remainder,
/// GAP-015, GAP-013, DN-28 §6); this constant says a pipeline exists and produces
/// tracks, not that every estimator in the capability table is in it.
pub const PIPELINE_IMPLEMENTED: bool = true;

/// The ingest task: buffers out-of-order/late detections from multiple sensors,
/// reconciles them onto a common fused timeline, and emits a [`PipelineSnapshot`] on
/// `out` after every epoch it processes.
///
/// The default settings are used; [`ingest_with`] takes a deployment's.
///
/// Cancellation-safety note (required on every async fn per §2.2): the pipeline lives
/// on the stack of this task and is mutated only between awaits. A detection is taken
/// from the channel and pushed into the reorder buffer with no await in between, so a
/// dropped future loses at most one in-flight detection and never leaves the buffer
/// half-updated.
///
/// The inbound channel is a `crossbeam` channel because it is the boundary with the
/// synchronous render/UI thread (§2.2); it is polled with `try_recv` plus a yield
/// rather than a blocking `recv`, which would stall the executor thread.
pub async fn ingest(rx: Receiver<Submission>, out: Sender<PipelineSnapshot>) {
    ingest_with(rx, out, PipelineSettings::default()).await;
}

/// [`ingest`] under a deployment's settings.
///
/// **The stream's end is a flush, not a truncation.** When the inbound channel closes,
/// whatever is still inside the reorder horizon is processed and a final snapshot is
/// emitted before the task stops. Dropping it would lose the last horizon of every
/// session, and a replay would then end short of the recording it replayed.
pub async fn ingest_with(
    rx: Receiver<Submission>,
    out: Sender<PipelineSnapshot>,
    settings: PipelineSettings,
) {
    let mut pipeline = FusionPipeline::new(settings);
    tracing::info!(
        ?pipeline,
        "fusion-async pipeline started: out-of-sequence buffering with global association"
    );
    loop {
        match rx.try_recv() {
            Ok(Submission::Position(det)) => {
                if let Err(err) = pipeline.push(det) {
                    tracing::warn!(%err, "detection refused by the reorder buffer");
                }
                if pipeline.run_ready() > 0 && out.send(snapshot_output(&pipeline)).is_err() {
                    tracing::warn!("track consumer is gone; stopping the pipeline");
                    return;
                }
            }
            Ok(Submission::Bearing(bearing)) => {
                // DN-27 §5: a bearing may update a track, may not initiate one, and is
                // retained and shown when it updates nothing. All three are the
                // pipeline's rules; this loop only reports which happened, because an
                // operator asking why a direction did not become a track needs the
                // answer to have been recorded somewhere.
                match pipeline.offer_bearing(&bearing) {
                    BearingOutcome::Updated(track) => {
                        tracing::debug!(
                            sensor = bearing.sensor_id,
                            ?track,
                            "a bearing refined a track"
                        );
                    }
                    BearingOutcome::Retained { until_s } => {
                        tracing::debug!(
                            sensor = bearing.sensor_id,
                            until_s,
                            "a bearing matched nothing and is retained"
                        );
                    }
                    BearingOutcome::Refused(why) => {
                        tracing::warn!(sensor = bearing.sensor_id, ?why, "a bearing was refused");
                    }
                }
                // A retained bearing has a stated lifetime and something has to end it.
                // Doing it here rather than on a timer keeps it on the same clock the
                // bearings themselves carry.
                pipeline.expire_bearings(bearing.timestamp_s);
                if out.send(snapshot_output(&pipeline)).is_err() {
                    tracing::warn!("track consumer is gone; stopping the pipeline");
                    return;
                }
            }
            Err(TryRecvError::Empty) => tokio::time::sleep(IDLE_POLL).await,
            Err(TryRecvError::Disconnected) => break,
        }
    }
    if pipeline.flush() > 0 {
        let _ = out.send(snapshot_output(&pipeline));
    }
    tracing::info!(
        stats = ?pipeline.stats(),
        "fusion-async ingest task stopped: inbound channel closed and the buffer flushed"
    );
}
